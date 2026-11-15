use super::private_files::{
    FileError, Namespace, RuntimeDirectory, CONTROL_NAMESPACE, DEVELOPMENT_NAMESPACE,
};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::application::certificate_trust::{
    CertificatePin, CertificateTrustStore, TrustError, TrustRecord,
};
use crate::application::device_repository::{DeviceRepository, RepositoryError, SavedDevice};
use crate::application::recovery::{ForgetMarker, RecoveryJournal};
use crate::application::sources::{SourceConfiguration, SourceLabel, SourceSetting};
use crate::application::tv_address::TvHost;
use crate::application::wake::{WakeConfiguration, WakeInterface};
use crate::domain::DeviceId;

// The full 1,024-record profile can exceed 2 MiB with valid maximally escaped
// labels and hosts. Keep disk and HTTP bounds consistent without truncation.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
// v3 adds per-TV `sources`. Older builds reject v3 as corrupt instead of
// silently dropping it; the first v3 write keeps the prior file as a backup.
const DEVICE_VERSION: u32 = 3;
const PRE_V3_BACKUP: &str = "devices.pre-v3.json";
const TRUST_VERSION: u32 = 1;

pub struct LocalDeviceRepository {
    file: JsonFile,
}

pub struct LocalCertificateTrustStore {
    file: JsonFile,
}

pub struct LocalRecoveryJournal {
    file: JsonFile,
}

#[path = "pairing_journal.rs"]
mod pairing_journal;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiskRecovery {
    version: u32,
    kind: String,
    transaction: String,
    target: String,
    selection_cleared: bool,
    credential_removed: bool,
    trust_removed: bool,
    record_removed: bool,
}

impl LocalRecoveryJournal {
    /// Isolated profile storage uses a checked directory handle for every I/O.
    pub fn in_private_directory(directory: Arc<RuntimeDirectory>) -> Self {
        Self::in_namespaced_directory(directory, Namespace::Development)
    }

    pub fn in_namespaced_directory(directory: Arc<RuntimeDirectory>, namespace: Namespace) -> Self {
        Self {
            file: JsonFile::private(directory, "recovery.json", namespace),
        }
    }

    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            file: JsonFile::new(app_data_dir.join("recovery.json")),
        }
    }

    fn read_marker(&self) -> Result<Option<ForgetMarker>, RepositoryError> {
        let value = self
            .file
            .read::<serde_json::Value>()
            .map_err(recovery_io_error)?;
        if value
            .as_ref()
            .is_some_and(|value| value["kind"] == "pairing")
        {
            self.read_pairing()?;
            return Ok(None);
        }
        value
            .map(serde_json::from_value::<DiskRecovery>)
            .transpose()
            .map_err(|_| RepositoryError::Corrupt)?
            .map(|disk| {
                if disk.version != 1 || disk.kind != "forget" {
                    return Err(RepositoryError::Corrupt);
                }
                let marker = ForgetMarker {
                    transaction: DeviceId::parse_record_key(&disk.transaction)
                        .ok_or(RepositoryError::Corrupt)?,
                    target: DeviceId::parse_record_key(&disk.target)
                        .ok_or(RepositoryError::Corrupt)?,
                    selection_cleared: disk.selection_cleared,
                    credential_removed: disk.credential_removed,
                    trust_removed: disk.trust_removed,
                    record_removed: disk.record_removed,
                };
                if !marker.valid() {
                    return Err(RepositoryError::Corrupt);
                }
                Ok(marker)
            })
            .transpose()
    }
}

fn recovery_io_error(error: std::io::Error) -> RepositoryError {
    if error.kind() == std::io::ErrorKind::InvalidData {
        RepositoryError::Corrupt
    } else {
        RepositoryError::Unavailable
    }
}

impl RecoveryJournal for LocalRecoveryJournal {
    fn pairing(
        &self,
    ) -> Result<Option<crate::application::recovery::PairingMarker>, RepositoryError> {
        self.load_pairing()
    }
    fn begin_pairing(
        &self,
        marker: crate::application::recovery::PairingMarker,
        replaces: Option<DeviceId>,
    ) -> Result<(), RepositoryError> {
        self.start_pairing(marker, replaces)
    }
    fn advance_pairing(
        &self,
        marker: crate::application::recovery::PairingMarker,
    ) -> Result<(), RepositoryError> {
        self.update_pairing(marker)
    }
    fn clear_pairing(&self, transaction: DeviceId) -> Result<(), RepositoryError> {
        self.remove_pairing(transaction)
    }
    fn save_backup(
        &self,
        backup: &crate::application::recovery::PairingBackup,
    ) -> Result<(), RepositoryError> {
        self.write_backup(backup)
    }
    fn load_backup(
        &self,
        transaction: DeviceId,
    ) -> Result<crate::application::recovery::PairingBackup, RepositoryError> {
        self.read_backup(transaction)
    }
    fn load(&self) -> Result<Option<ForgetMarker>, RepositoryError> {
        let _guard = self
            .file
            .lock
            .lock()
            .map_err(|_| RepositoryError::Unavailable)?;
        self.read_marker()
    }

    fn write(&self, marker: ForgetMarker) -> Result<(), RepositoryError> {
        let _guard = self
            .file
            .lock
            .lock()
            .map_err(|_| RepositoryError::Unavailable)?;
        if !marker.valid() || self.read_pairing()?.is_some() {
            return Err(RepositoryError::Corrupt);
        }
        if let Some(existing) = self.read_marker()? {
            if existing.transaction != marker.transaction
                || existing.target != marker.target
                || (existing.selection_cleared && !marker.selection_cleared)
                || (existing.credential_removed && !marker.credential_removed)
                || (existing.trust_removed && !marker.trust_removed)
                || (existing.record_removed && !marker.record_removed)
            {
                return Err(RepositoryError::Corrupt);
            }
        } else if marker.selection_cleared {
            return Err(RepositoryError::Corrupt);
        }
        self.file
            .write(&DiskRecovery {
                version: 1,
                kind: "forget".into(),
                transaction: marker.transaction.to_string(),
                target: marker.target.to_string(),
                selection_cleared: marker.selection_cleared,
                credential_removed: marker.credential_removed,
                trust_removed: marker.trust_removed,
                record_removed: marker.record_removed,
            })
            .map_err(recovery_io_error)
    }

    fn clear(&self, transaction: DeviceId) -> Result<(), RepositoryError> {
        let _guard = self
            .file
            .lock
            .lock()
            .map_err(|_| RepositoryError::Unavailable)?;
        let marker = self.read_marker()?.ok_or(RepositoryError::Corrupt)?;
        if marker.transaction != transaction
            || !marker.selection_cleared
            || !marker.credential_removed
            || !marker.trust_removed
            || !marker.record_removed
        {
            return Err(RepositoryError::Corrupt);
        }
        self.file.remove().map_err(recovery_io_error)
    }
}

struct JsonFile {
    path: PathBuf,
    private: Option<Arc<RuntimeDirectory>>,
    namespace: Namespace,
    lock: Mutex<()>,
}

/// Namespaced documents deliberately cannot decode as legacy records,
/// even after a directory rename or an individual file copy. The explicit
/// envelope also prevents private adapters from importing untagged data.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NamespacedDocument<T> {
    namespace: String,
    data: T,
}

#[derive(Serialize, Deserialize)]
struct DiskDevices {
    version: u32,
    selected: Option<String>,
    devices: Vec<DiskDevice>,
}

#[derive(Clone, Serialize, Deserialize)]
struct DiskDevice {
    id: String,
    label: String,
    host: String,
    #[serde(default)]
    wake: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sources: Option<DiskSources>,
}

/// Typed, unlike `wake`: derived structs reject duplicate and unknown keys.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiskSources {
    hdmi1: DiskSource,
    hdmi2: DiskSource,
    hdmi3: DiskSource,
    tuner: DiskSource,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiskSource {
    state: String,
    label: Option<String>,
    confirmed_at_unix_ms: Option<u64>,
}

fn disk_sources(value: &SourceConfiguration) -> DiskSources {
    let [hdmi1, hdmi2, hdmi3, tuner] = (*value.ports).clone().map(|setting| match setting {
        SourceSetting::Unconfigured => DiskSource {
            state: "unconfigured".into(),
            label: None,
            confirmed_at_unix_ms: None,
        },
        SourceSetting::Hidden => DiskSource {
            state: "hidden".into(),
            label: None,
            confirmed_at_unix_ms: None,
        },
        SourceSetting::Shown {
            label,
            confirmed_at_unix_ms,
        } => DiskSource {
            state: "shown".into(),
            label: label.map(|l| l.as_str().to_owned()),
            confirmed_at_unix_ms: Some(confirmed_at_unix_ms),
        },
    });
    DiskSources {
        hdmi1,
        hdmi2,
        hdmi3,
        tuner,
    }
}

/// Present malformed data fails closed; nothing is invented at startup.
fn convert_sources(value: DiskSources) -> Result<SourceConfiguration, RepositoryError> {
    let mut ports: [SourceSetting; 4] = Default::default();
    for (port, item) in ports
        .iter_mut()
        .zip([value.hdmi1, value.hdmi2, value.hdmi3, value.tuner])
    {
        *port = match (item.state.as_str(), item.label, item.confirmed_at_unix_ms) {
            ("unconfigured", None, None) => SourceSetting::Unconfigured,
            ("hidden", None, None) => SourceSetting::Hidden,
            ("shown", label, Some(at)) if at > 0 => SourceSetting::Shown {
                label: match label {
                    Some(label) => Some(
                        SourceLabel::parse(&label)
                            .filter(|parsed| parsed.as_str() == label)
                            .ok_or(RepositoryError::Corrupt)?,
                    ),
                    None => None,
                },
                confirmed_at_unix_ms: at,
            },
            _ => return Err(RepositoryError::Corrupt),
        };
    }
    Ok(SourceConfiguration {
        ports: Box::new(ports),
    })
}

fn disk_device(device: &SavedDevice) -> Result<DiskDevice, RepositoryError> {
    Ok(DiskDevice {
        id: device.id.to_string(),
        label: device.label.clone(),
        host: device.host.as_str().to_owned(),
        wake: Some(
            serde_json::to_value(DiskWake {
                wired: device.wake.wired.map(|mac| mac.to_string()),
                wifi: device.wake.wifi.map(|mac| mac.to_string()),
                active: device.wake.active.map(|active| match active {
                    WakeInterface::Wired => "wired".to_owned(),
                    WakeInterface::WiFi => "wifi".to_owned(),
                }),
            })
            .map_err(|_| RepositoryError::Unavailable)?,
        ),
        sources: Some(disk_sources(&device.sources)),
    })
}

#[derive(Serialize, Deserialize)]
struct DiskWake {
    wired: Option<String>,
    wifi: Option<String>,
    active: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct DiskTrust {
    version: u32,
    records: HashMap<String, DiskTrustRecord>,
}

#[derive(Serialize, Deserialize)]
struct DiskTrustRecord {
    host: String,
    certificate_sha256: String,
}

impl Default for DiskDevices {
    fn default() -> Self {
        Self {
            version: DEVICE_VERSION,
            selected: None,
            devices: Vec::new(),
        }
    }
}

impl Default for DiskTrust {
    fn default() -> Self {
        Self {
            version: TRUST_VERSION,
            records: HashMap::new(),
        }
    }
}

impl LocalDeviceRepository {
    /// Isolated profile storage uses a checked directory handle for every I/O.
    pub fn in_private_directory(directory: Arc<RuntimeDirectory>) -> Self {
        Self::in_namespaced_directory(directory, Namespace::Development)
    }

    pub fn in_namespaced_directory(directory: Arc<RuntimeDirectory>, namespace: Namespace) -> Self {
        Self {
            file: JsonFile::private(directory, "devices.json", namespace),
        }
    }

    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            file: JsonFile::new(app_data_dir.join("devices.json")),
        }
    }
}

impl LocalCertificateTrustStore {
    /// Isolated profile storage uses a checked directory handle for every I/O.
    pub fn in_private_directory(directory: Arc<RuntimeDirectory>) -> Self {
        Self::in_namespaced_directory(directory, Namespace::Development)
    }

    pub fn in_namespaced_directory(directory: Arc<RuntimeDirectory>, namespace: Namespace) -> Self {
        Self {
            file: JsonFile::private(directory, "trust.json", namespace),
        }
    }

    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            file: JsonFile::new(app_data_dir.join("trust.json")),
        }
    }
}

pub fn default_app_data_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library/Application Support/Samsung TV Remote"))
}

impl DeviceRepository for LocalDeviceRepository {
    fn list(&self) -> Result<Vec<SavedDevice>, RepositoryError> {
        let _guard = self
            .file
            .lock
            .lock()
            .map_err(|_| RepositoryError::Unavailable)?;
        let disk = self.load()?;
        disk.devices.into_iter().map(convert_device).collect()
    }

    fn save(&self, device: &SavedDevice) -> Result<(), RepositoryError> {
        let _guard = self
            .file
            .lock
            .lock()
            .map_err(|_| RepositoryError::Unavailable)?;
        let mut disk = self.load_for_write()?;
        let record = disk_device(device)?;
        if let Some(existing) = disk.devices.iter_mut().find(|item| item.id == record.id) {
            *existing = record;
        } else {
            disk.devices.push(record);
        }
        self.file
            .write(&disk)
            .map_err(|_| RepositoryError::Unavailable)
    }

    fn delete(&self, id: DeviceId) -> Result<(), RepositoryError> {
        let _guard = self
            .file
            .lock
            .lock()
            .map_err(|_| RepositoryError::Unavailable)?;
        let mut disk = self.load_for_write()?;
        disk.devices.retain(|item| item.id != id.to_string());
        if disk.selected.as_deref() == Some(id.to_string().as_str()) {
            disk.selected = None;
        }
        self.file
            .write(&disk)
            .map_err(|_| RepositoryError::Unavailable)
    }

    fn selected(&self) -> Result<Option<DeviceId>, RepositoryError> {
        let _guard = self
            .file
            .lock
            .lock()
            .map_err(|_| RepositoryError::Unavailable)?;
        let disk = self.load()?;
        disk.selected
            .map(|key| DeviceId::parse_record_key(&key).ok_or(RepositoryError::Corrupt))
            .transpose()
    }

    fn select(&self, id: Option<DeviceId>) -> Result<(), RepositoryError> {
        let _guard = self
            .file
            .lock
            .lock()
            .map_err(|_| RepositoryError::Unavailable)?;
        let mut disk = self.load_for_write()?;
        if let Some(id) = id {
            if !disk.devices.iter().any(|item| item.id == id.to_string()) {
                return Err(RepositoryError::Corrupt);
            }
        }
        disk.selected = id.map(|id| id.to_string());
        self.file
            .write(&disk)
            .map_err(|_| RepositoryError::Unavailable)
    }
}

impl LocalDeviceRepository {
    fn load(&self) -> Result<DiskDevices, RepositoryError> {
        let disk: DiskDevices = self
            .file
            .read()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::InvalidData => RepositoryError::Corrupt,
                _ => RepositoryError::Unavailable,
            })?
            .unwrap_or_default();
        if !(1..=DEVICE_VERSION).contains(&disk.version) {
            return Err(RepositoryError::Corrupt);
        }
        Ok(disk)
    }

    /// Every write path (save, select, delete) migrates through here. A
    /// fresh profile loads as v3 and needs no backup.
    fn load_for_write(&self) -> Result<DiskDevices, RepositoryError> {
        let mut disk = self.load()?;
        if disk.version < DEVICE_VERSION {
            // Validate before preserving: a corrupt v1/v2 is never migrated.
            for item in &disk.devices {
                convert_device(item.clone())?;
            }
            let backup = self
                .file
                .sibling(PRE_V3_BACKUP)
                .map_err(|_| RepositoryError::Unavailable)?;
            let expected = serde_json::to_value(&disk).map_err(|_| RepositoryError::Unavailable)?;
            match backup.read::<serde_json::Value>() {
                // A retry after a failed live write reuses its own backup.
                Ok(Some(existing)) if existing == expected => {}
                // Never overwrite a different historical backup.
                Ok(Some(_)) => return Err(RepositoryError::Corrupt),
                Ok(None) => backup
                    .write(&disk)
                    .map_err(|_| RepositoryError::Unavailable)?,
                Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                    return Err(RepositoryError::Corrupt)
                }
                Err(_) => return Err(RepositoryError::Unavailable),
            }
            disk.version = DEVICE_VERSION;
        }
        Ok(disk)
    }
}

impl CertificateTrustStore for LocalCertificateTrustStore {
    fn load(&self, device: DeviceId) -> Result<Option<TrustRecord>, TrustError> {
        let _guard = self.file.lock.lock().map_err(|_| TrustError::Unavailable)?;
        let disk = self.load_disk()?;
        disk.records
            .get(&device.to_string())
            .map(convert_trust)
            .transpose()
    }

    fn save(&self, device: DeviceId, record: &TrustRecord) -> Result<(), TrustError> {
        let _guard = self.file.lock.lock().map_err(|_| TrustError::Unavailable)?;
        let mut disk = self.load_disk()?;
        disk.records.insert(
            device.to_string(),
            DiskTrustRecord {
                host: record.host.as_str().to_owned(),
                certificate_sha256: record.pin.to_hex(),
            },
        );
        self.file.write(&disk).map_err(|_| TrustError::Unavailable)
    }

    fn delete(&self, device: DeviceId) -> Result<(), TrustError> {
        let _guard = self.file.lock.lock().map_err(|_| TrustError::Unavailable)?;
        let mut disk = self.load_disk()?;
        disk.records.remove(&device.to_string());
        self.file.write(&disk).map_err(|_| TrustError::Unavailable)
    }
}

impl LocalCertificateTrustStore {
    fn load_disk(&self) -> Result<DiskTrust, TrustError> {
        let disk: DiskTrust = self
            .file
            .read()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::InvalidData => TrustError::Corrupt,
                _ => TrustError::Unavailable,
            })?
            .unwrap_or_default();
        if disk.version != TRUST_VERSION {
            return Err(TrustError::Corrupt);
        }
        Ok(disk)
    }
}

fn convert_device(item: DiskDevice) -> Result<SavedDevice, RepositoryError> {
    let wake = item
        .wake
        .and_then(|value| serde_json::from_value::<DiskWake>(value).ok())
        .and_then(|value| {
            let wired = value.wired.map(|mac| mac.parse()).transpose().ok()?;
            let wifi = value.wifi.map(|mac| mac.parse()).transpose().ok()?;
            let active = match value.active.as_deref() {
                None => None,
                Some("wired") => Some(WakeInterface::Wired),
                Some("wifi") => Some(WakeInterface::WiFi),
                _ => return None,
            };
            let wake = WakeConfiguration {
                wired,
                wifi,
                active,
            };
            if wake.active.is_some() && wake.active_mac().is_none() {
                return None;
            }
            Some(wake)
        })
        .unwrap_or_default();
    Ok(SavedDevice {
        sources: item
            .sources
            .map(convert_sources)
            .transpose()?
            .unwrap_or_default(),
        id: DeviceId::parse_record_key(&item.id).ok_or(RepositoryError::Corrupt)?,
        label: item.label,
        host: TvHost::parse(&item.host).map_err(|_| RepositoryError::Corrupt)?,
        wake,
    })
}

fn convert_trust(item: &DiskTrustRecord) -> Result<TrustRecord, TrustError> {
    Ok(TrustRecord {
        host: TvHost::parse(&item.host).map_err(|_| TrustError::Corrupt)?,
        pin: CertificatePin::from_hex(&item.certificate_sha256).ok_or(TrustError::Corrupt)?,
    })
}

impl JsonFile {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            private: None,
            namespace: Namespace::Development,
            lock: Mutex::new(()),
        }
    }

    fn private(directory: Arc<RuntimeDirectory>, name: &str, namespace: Namespace) -> Self {
        Self {
            path: PathBuf::from(name),
            private: Some(directory),
            namespace,
            lock: Mutex::new(()),
        }
    }
    fn sibling(&self, name: &str) -> Result<Self, std::io::Error> {
        if let Some(directory) = &self.private {
            return Ok(Self::private(directory.clone(), name, self.namespace));
        }
        Ok(Self::new(
            self.path
                .parent()
                .ok_or_else(|| std::io::Error::other("missing parent"))?
                .join(name),
        ))
    }
    fn name(&self) -> Result<&str, std::io::Error> {
        self.path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| std::io::Error::other("invalid file name"))
    }
    fn legacy_allowed(&self) -> Result<(), std::io::Error> {
        let forbidden = |p: &Path| {
            p.components().any(|v| {
                v.as_os_str() == DEVELOPMENT_NAMESPACE || v.as_os_str() == CONTROL_NAMESPACE
            })
        };
        if forbidden(&self.path)
            || self
                .path
                .parent()
                .and_then(|p| p.canonicalize().ok())
                .is_some_and(|p| forbidden(&p))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "server profile requires isolated adapters",
            ));
        }
        Ok(())
    }
    fn remove(&self) -> Result<(), std::io::Error> {
        if let Some(directory) = &self.private {
            return directory.remove(self.name()?).map_err(private_error);
        }
        self.legacy_allowed()?;
        match fs::remove_file(&self.path) {
            Ok(()) => File::open(
                self.path
                    .parent()
                    .ok_or_else(|| std::io::Error::other("missing parent"))?,
            )?
            .sync_all(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn read<T: for<'de> Deserialize<'de>>(&self) -> Result<Option<T>, std::io::Error> {
        if let Some(directory) = &self.private {
            return directory
                .read(self.name()?, MAX_FILE_BYTES as usize)
                .map_err(private_error)?
                .map(|bytes| {
                    let document: NamespacedDocument<T> =
                        serde_json::from_slice(&bytes).map_err(|_| {
                            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid settings")
                        })?;
                    if document.namespace != self.namespace.name() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "wrong profile namespace",
                        ));
                    }
                    Ok(document.data)
                })
                .transpose();
        }
        self.legacy_allowed()?;
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if file.metadata()?.len() > MAX_FILE_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "oversized settings",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "oversized settings",
            ));
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid settings"))
    }

    fn write<T: Serialize>(&self, value: &T) -> Result<(), std::io::Error> {
        if let Some(directory) = &self.private {
            let bytes = serde_json::to_vec(&NamespacedDocument {
                namespace: self.namespace.name().into(),
                data: value,
            })
            .map_err(|_| std::io::Error::other("settings encoding failed"))?;
            if bytes.len() as u64 > MAX_FILE_BYTES {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "oversized settings",
                ));
            }
            return directory
                .replace(self.name()?, &bytes)
                .map_err(private_error);
        }
        self.legacy_allowed()?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| std::io::Error::other("missing data directory"))?;
        fs::create_dir_all(parent)?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        let bytes = serde_json::to_vec(value)
            .map_err(|_| std::io::Error::other("settings encoding failed"))?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "oversized settings",
            ));
        }
        let temporary = self
            .path
            .with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, &self.path)?;
            // Rename is not durable until the directory entry is synced.
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn private_error(error: FileError) -> std::io::Error {
    std::io::Error::new(
        match error {
            FileError::TooLarge | FileError::UnsafePath => std::io::ErrorKind::InvalidData,
            FileError::Unavailable | FileError::Locked => std::io::ErrorKind::Other,
        },
        "private settings unavailable",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_and_development_records_remain_distinct_after_copy_and_backup() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        for namespace in [Namespace::Development, Namespace::Control] {
            let directory = Arc::new(RuntimeDirectory::open(&root.join(namespace.name())).unwrap());
            let file = JsonFile::private(directory.clone(), "devices.json", namespace);
            file.write(&serde_json::json!({"version":2,"devices":[],"selected":null}))
                .unwrap();
            let other = if namespace == Namespace::Control {
                Namespace::Development
            } else {
                Namespace::Control
            };
            let copied = JsonFile::private(directory.clone(), "devices.json", other);
            assert!(copied.read::<serde_json::Value>().is_err());
            assert!(
                LocalDeviceRepository::in_namespaced_directory(directory.clone(), namespace)
                    .list()
                    .unwrap()
                    .is_empty()
            );
            // Pairing backup files inherit the selected envelope through sibling().
            let backup = file.sibling("pairing-backup.json").unwrap();
            backup.write(&serde_json::json!({"fixture":true})).unwrap();
            let bytes = directory
                .read("pairing-backup.json", 4096)
                .unwrap()
                .unwrap();
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["namespace"], namespace.name());
            directory
                .replace(
                    "devices.json",
                    br#"{"version":2,"devices":[],"selected":null}"#,
                )
                .unwrap();
            assert!(
                LocalDeviceRepository::in_namespaced_directory(directory, namespace)
                    .list()
                    .is_err()
            );
            assert!(LocalDeviceRepository::new(&root.join(namespace.name()))
                .list()
                .is_err());
        }
    }

    #[test]
    fn device_and_trust_records_roundtrip_without_a_token_in_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let repository = LocalDeviceRepository::new(dir.path());
        let trust = LocalCertificateTrustStore::new(dir.path());
        let device = SavedDevice {
            sources: Default::default(),
            id: DeviceId::generate(),
            label: "TV".to_owned(),
            host: TvHost::parse("tv.local").unwrap(),
            wake: WakeConfiguration::default(),
        };
        repository.save(&device).unwrap();
        repository.select(Some(device.id)).unwrap();
        trust
            .save(
                device.id,
                &TrustRecord {
                    host: device.host.clone(),
                    pin: CertificatePin::from_der(b"fake certificate"),
                },
            )
            .unwrap();
        assert_eq!(repository.list().unwrap(), vec![device.clone()]);
        assert_eq!(repository.selected().unwrap(), Some(device.id));
        assert!(trust.load(device.id).unwrap().is_some());
        let preferences = fs::read_to_string(dir.path().join("devices.json")).unwrap();
        assert!(!preferences.contains("token"));
        repository.delete(device.id).unwrap();
        trust.delete(device.id).unwrap();
        assert!(repository.list().unwrap().is_empty());
        assert_eq!(repository.selected().unwrap(), None);
        assert!(trust.load(device.id).unwrap().is_none());
    }

    #[test]
    fn wake_settings_roundtrip_and_legacy_records_remain_selected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("devices.json");
        let id = DeviceId::generate().to_string();
        fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "version": 1,
                "selected": id,
                "devices": [{"id": id, "label": "TV", "host": "tv.local"}]
            }))
            .unwrap(),
        )
        .unwrap();
        let repository = LocalDeviceRepository::new(dir.path());
        let mut device = repository.list().unwrap().remove(0);
        assert_eq!(repository.selected().unwrap(), Some(device.id));
        assert_eq!(device.wake, WakeConfiguration::default());
        device.wake = WakeConfiguration {
            wired: Some("02:11:22:33:44:55".parse().unwrap()),
            wifi: None,
            active: Some(WakeInterface::Wired),
        };
        repository.save(&device).unwrap();
        assert_eq!(repository.list().unwrap(), vec![device]);
        let disk: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(disk["version"], 3);
    }

    #[test]
    fn malformed_wake_field_does_not_lose_saved_device_or_selection() {
        let dir = tempfile::tempdir().unwrap();
        let id = DeviceId::generate().to_string();
        fs::write(
            dir.path().join("devices.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": 2,
                "selected": id,
                "devices": [{"id": id, "label": "TV", "host": "tv.local",
                    "wake": {"wired": "not-a-mac", "wifi": null, "active": "wired"}}]
            }))
            .unwrap(),
        )
        .unwrap();
        let repository = LocalDeviceRepository::new(dir.path());
        let device = repository.list().unwrap().remove(0);
        assert_eq!(device.wake, WakeConfiguration::default());
        assert_eq!(repository.selected().unwrap(), Some(device.id));
    }

    #[test]
    fn obsolete_verified_actions_are_ignored_and_removed_on_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("devices.json");
        let id = DeviceId::generate().to_string();
        let second_id = DeviceId::generate().to_string();
        fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "version": 1,
                "selected": id,
                "devices": [{
                    "id": id,
                    "label": "TV",
                    "host": "tv.local",
                    "verified_actions": ["Select"]
            }, {
                "id": second_id,
                "label": "Other TV",
                "host": "other.local",
                "verified_actions": ["Enter"]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let repository = LocalDeviceRepository::new(dir.path());
        let devices = repository.list().unwrap();
        assert_eq!(devices.len(), 2);
        for device in devices {
            repository.save(&device).unwrap();
        }
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert!(saved["devices"]
            .as_array()
            .unwrap()
            .iter()
            .all(|device| device.get("verified_actions").is_none()));
    }

    fn legacy_profile(version: u32) -> (tempfile::TempDir, String, serde_json::Value) {
        let dir = tempfile::tempdir().unwrap();
        let id = DeviceId::generate().to_string();
        let document = serde_json::json!({
            "version": version,
            "selected": id,
            "devices": [{"id": id, "label": "TV", "host": "tv.local", "wake": null}]
        });
        fs::write(
            dir.path().join("devices.json"),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
        (dir, id, document)
    }

    fn shown_sources() -> SourceConfiguration {
        use crate::application::sources::SourceChoice;
        SourceConfiguration::default().apply(
            [
                SourceChoice::Shown(SourceLabel::parse("PlayStation")),
                SourceChoice::Hidden,
                SourceChoice::Unconfigured,
                SourceChoice::Shown(None),
            ],
            1_700_000_000_000,
        )
    }

    #[test]
    fn every_first_v3_write_keeps_the_original_version_as_a_backup() {
        for version in [1, 2] {
            for write in ["save", "select", "delete"] {
                let (dir, id, original) = legacy_profile(version);
                let repository = LocalDeviceRepository::new(dir.path());
                let mut device = repository.list().unwrap().remove(0);
                match write {
                    "save" => {
                        device.sources = shown_sources();
                        repository.save(&device).unwrap();
                        assert_eq!(repository.list().unwrap(), vec![device.clone()]);
                    }
                    "select" => repository.select(None).unwrap(),
                    _ => repository.delete(device.id).unwrap(),
                }
                let backup: serde_json::Value =
                    serde_json::from_slice(&fs::read(dir.path().join(PRE_V3_BACKUP)).unwrap())
                        .unwrap();
                assert_eq!(backup["version"], version, "{write}");
                assert_eq!(backup["devices"][0]["id"], id);
                assert_eq!(backup["devices"], original["devices"]);
                let live: serde_json::Value =
                    serde_json::from_slice(&fs::read(dir.path().join("devices.json")).unwrap())
                        .unwrap();
                assert_eq!(live["version"], 3);
            }
        }
    }

    #[test]
    fn fresh_profiles_need_no_backup_and_conflicting_backups_block_migration() {
        let dir = tempfile::tempdir().unwrap();
        let repository = LocalDeviceRepository::new(dir.path());
        let device = SavedDevice {
            sources: shown_sources(),
            id: DeviceId::generate(),
            label: "TV".into(),
            host: TvHost::parse("tv.local").unwrap(),
            wake: WakeConfiguration::default(),
        };
        repository.save(&device).unwrap();
        assert!(!dir.path().join(PRE_V3_BACKUP).exists());

        let (dir, _, original) = legacy_profile(2);
        // A matching backup from an interrupted earlier attempt is reused.
        fs::write(
            dir.path().join(PRE_V3_BACKUP),
            serde_json::to_vec(&original).unwrap(),
        )
        .unwrap();
        let repository = LocalDeviceRepository::new(dir.path());
        repository.select(None).unwrap();

        let (dir, _, _) = legacy_profile(2);
        fs::write(
            dir.path().join(PRE_V3_BACKUP),
            br#"{"version":2,"selected":null,"devices":[]}"#,
        )
        .unwrap();
        let repository = LocalDeviceRepository::new(dir.path());
        let before = fs::read(dir.path().join("devices.json")).unwrap();
        assert_eq!(repository.select(None), Err(RepositoryError::Corrupt));
        assert_eq!(fs::read(dir.path().join("devices.json")).unwrap(), before);

        // An unwritable destination blocks before the live file changes.
        let (dir, _, _) = legacy_profile(1);
        fs::create_dir(dir.path().join(PRE_V3_BACKUP)).unwrap();
        let repository = LocalDeviceRepository::new(dir.path());
        let before = fs::read(dir.path().join("devices.json")).unwrap();
        assert!(repository.select(None).is_err());
        assert_eq!(fs::read(dir.path().join("devices.json")).unwrap(), before);
    }

    #[test]
    fn malformed_v3_sources_and_future_versions_fail_closed() {
        let entry = |state: &str| serde_json::json!({"state":state,"label":null,"confirmed_at_unix_ms":null});
        let shown = serde_json::json!({"state":"shown","label":"PS5","confirmed_at_unix_ms":5});
        let valid = serde_json::json!({"hdmi1":shown,"hdmi2":entry("hidden"),"hdmi3":entry("unconfigured"),"tuner":entry("unconfigured")});
        let mut cases = vec![];
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove("tuner");
        cases.push(serde_json::to_string(&missing).unwrap());
        let mut unknown = valid.clone();
        unknown["hdmi4"] = entry("hidden");
        cases.push(serde_json::to_string(&unknown).unwrap());
        // Duplicate key, spelled by hand; serde_json::Value would keep the last.
        cases.push(serde_json::to_string(&valid).unwrap().replacen(
            r#""hdmi2":"#,
            r#""hdmi2":{"state":"hidden","label":null,"confirmed_at_unix_ms":null},"hdmi2":"#,
            1,
        ));
        for bad in [
            serde_json::json!({"state":"hidden","label":"x","confirmed_at_unix_ms":null}),
            serde_json::json!({"state":"shown","label":null,"confirmed_at_unix_ms":0}),
            serde_json::json!({"state":"shown","label":null,"confirmed_at_unix_ms":null}),
            serde_json::json!({"state":"shown","label":" padded ","confirmed_at_unix_ms":5}),
            serde_json::json!({"state":"shown","label":"a\u{202E}b","confirmed_at_unix_ms":5}),
            serde_json::json!({"state":"active","label":null,"confirmed_at_unix_ms":null}),
            serde_json::json!({"state":"hidden","label":null,"confirmed_at_unix_ms":3}),
        ] {
            let mut value = valid.clone();
            value["hdmi3"] = bad;
            cases.push(serde_json::to_string(&value).unwrap());
        }
        let id = DeviceId::generate().to_string();
        for sources in cases {
            let dir = tempfile::tempdir().unwrap();
            fs::write(
                dir.path().join("devices.json"),
                format!(r#"{{"version":3,"selected":null,"devices":[{{"id":"{id}","label":"TV","host":"tv.local","wake":null,"sources":{sources}}}]}}"#),
            )
            .unwrap();
            assert_eq!(
                LocalDeviceRepository::new(dir.path()).list(),
                Err(RepositoryError::Corrupt),
                "{sources}"
            );
        }
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("devices.json"),
            format!(r#"{{"version":3,"selected":null,"devices":[{{"id":"{id}","label":"TV","host":"tv.local","wake":null,"sources":{valid}}}]}}"#),
        )
        .unwrap();
        let device = LocalDeviceRepository::new(dir.path())
            .list()
            .unwrap()
            .remove(0);
        assert_eq!(
            device.sources.ports[0],
            SourceSetting::Shown {
                label: SourceLabel::parse("PS5"),
                confirmed_at_unix_ms: 5
            }
        );
        fs::write(
            dir.path().join("devices.json"),
            br#"{"version":4,"selected":null,"devices":[]}"#,
        )
        .unwrap();
        assert_eq!(
            LocalDeviceRepository::new(dir.path()).list(),
            Err(RepositoryError::Corrupt)
        );
    }

    #[test]
    fn maximal_labelled_profile_fits_the_disk_read_limit() {
        let dir = tempfile::tempdir().unwrap();
        let repository = LocalDeviceRepository::new(dir.path());
        let host = format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );
        let label = SourceLabel::parse(&"\"".repeat(24)).unwrap();
        let sources = SourceConfiguration {
            ports: Box::new(std::array::from_fn(|_| SourceSetting::Shown {
                label: Some(label.clone()),
                confirmed_at_unix_ms: u64::MAX,
            })),
        };
        let mut disk = DiskDevices::default();
        for _ in 0..1024 {
            disk.devices.push(
                disk_device(&SavedDevice {
                    sources: sources.clone(),
                    id: DeviceId::generate(),
                    label: "\u{0001}".repeat(256),
                    host: TvHost::parse(&host).unwrap(),
                    wake: WakeConfiguration {
                        wired: Some("02:00:00:00:00:01".parse().unwrap()),
                        wifi: Some("02:00:00:00:00:02".parse().unwrap()),
                        active: Some(WakeInterface::Wired),
                    },
                })
                .unwrap(),
            );
        }
        let bytes = serde_json::to_vec(&disk).unwrap().len();
        println!("maximal v3 device file={bytes} bytes; limit={MAX_FILE_BYTES}");
        assert!((bytes as u64) < MAX_FILE_BYTES);
        repository.file.write(&disk).unwrap();
        assert_eq!(repository.list().unwrap().len(), 1024);
    }

    #[test]
    fn pairing_backups_keep_sources_as_v2_and_legacy_v1_still_reads() {
        use crate::application::recovery::PairingBackup;
        let dir = tempfile::tempdir().unwrap();
        let journal = LocalRecoveryJournal::new(dir.path());
        let transaction = DeviceId::generate();
        let device = SavedDevice {
            sources: shown_sources(),
            id: DeviceId::generate(),
            label: "TV".into(),
            host: TvHost::parse("tv.local").unwrap(),
            wake: WakeConfiguration::default(),
        };
        journal
            .write_backup(&PairingBackup {
                transaction,
                device: device.clone(),
                trust: None,
            })
            .unwrap();
        let path = dir.path().join("pairing-backup.json");
        let mut disk: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(disk["version"], 2);
        assert_eq!(journal.read_backup(transaction).unwrap().device, device);
        // A v1 backup predates sources and restores them as unconfigured.
        disk["version"] = serde_json::json!(1);
        disk["device"].as_object_mut().unwrap().remove("sources");
        fs::write(&path, serde_json::to_vec(&disk).unwrap()).unwrap();
        assert_eq!(
            journal.read_backup(transaction).unwrap().device.sources,
            SourceConfiguration::default()
        );
        disk["version"] = serde_json::json!(3);
        fs::write(&path, serde_json::to_vec(&disk).unwrap()).unwrap();
        assert_eq!(
            journal.read_backup(transaction).unwrap_err(),
            RepositoryError::Corrupt
        );
    }
}
