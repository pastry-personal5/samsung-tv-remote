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
use crate::application::tv_address::TvHost;
use crate::application::wake::{WakeConfiguration, WakeInterface};
use crate::domain::DeviceId;

// The full 1,024-record profile can exceed 2 MiB with valid maximally escaped
// labels and hosts. Keep disk and HTTP bounds consistent without truncation.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const DEVICE_VERSION: u32 = 2;
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

#[derive(Serialize, Deserialize)]
struct DiskDevice {
    id: String,
    label: String,
    host: String,
    #[serde(default)]
    wake: Option<serde_json::Value>,
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
        let mut disk = self.load()?;
        disk.version = DEVICE_VERSION;
        let record = DiskDevice {
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
        };
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
        let mut disk = self.load()?;
        disk.version = DEVICE_VERSION;
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
        let mut disk = self.load()?;
        disk.version = DEVICE_VERSION;
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
        if disk.version != 1 && disk.version != DEVICE_VERSION {
            return Err(RepositoryError::Corrupt);
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
        assert_eq!(disk["version"], 2);
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
}
