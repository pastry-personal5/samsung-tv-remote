//! Pairing uses the same recovery file/lane as Forget. Backups contain only
//! existing preference and trust data and are private to Application Support.
use super::*;
use crate::application::recovery::{PairingBackup, PairingCleanup, PairingMarker, PairingStage};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiskPairing {
    version: u32,
    kind: String,
    transaction: String,
    target: String,
    prior_selection: Option<String>,
    repair: bool,
    backup: Option<String>,
    stage: u8,
    cleanup: Option<[bool; 4]>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiskBackup {
    version: u32,
    transaction: String,
    device: DiskDevice,
    trust: Option<DiskTrustRecord>,
}
const BACKUP_VERSION: u32 = 2;
fn id(value: &str) -> Result<DeviceId, RepositoryError> {
    DeviceId::parse_record_key(value).ok_or(RepositoryError::Corrupt)
}
impl LocalRecoveryJournal {
    pub(super) fn read_pairing(&self) -> Result<Option<PairingMarker>, RepositoryError> {
        let Some(value) = self
            .file
            .read::<serde_json::Value>()
            .map_err(recovery_io_error)?
        else {
            return Ok(None);
        };
        if value["kind"] == "forget" {
            return Ok(None);
        }
        let disk: DiskPairing =
            serde_json::from_value(value).map_err(|_| RepositoryError::Corrupt)?;
        let stage = match disk.stage {
            0 => PairingStage::Prepared,
            1 => PairingStage::TokenSaved,
            2 => PairingStage::TrustSaved,
            3 => PairingStage::DeviceSaved,
            4 => PairingStage::SelectionSaved,
            _ => return Err(RepositoryError::Corrupt),
        };
        let marker = PairingMarker {
            transaction: id(&disk.transaction)?,
            target: id(&disk.target)?,
            prior_selection: disk.prior_selection.as_deref().map(id).transpose()?,
            repair: disk.repair,
            backup: disk.backup.as_deref().map(id).transpose()?,
            stage,
            cleanup: disk.cleanup.map(
                |[credential_removed, trust_removed, record_removed, selection_restored]| {
                    PairingCleanup {
                        credential_removed,
                        trust_removed,
                        record_removed,
                        selection_restored,
                    }
                },
            ),
        };
        if disk.version != 1 || disk.kind != "pairing" || !marker.valid() {
            return Err(RepositoryError::Corrupt);
        }
        Ok(Some(marker))
    }
    fn write_pairing(&self, marker: PairingMarker) -> Result<(), RepositoryError> {
        self.file
            .write(&DiskPairing {
                version: 1,
                kind: "pairing".into(),
                transaction: marker.transaction.to_string(),
                target: marker.target.to_string(),
                prior_selection: marker.prior_selection.map(|id| id.to_string()),
                repair: marker.repair,
                backup: marker.backup.map(|id| id.to_string()),
                stage: marker.stage as u8,
                cleanup: marker.cleanup.map(|c| {
                    [
                        c.credential_removed,
                        c.trust_removed,
                        c.record_removed,
                        c.selection_restored,
                    ]
                }),
            })
            .map_err(recovery_io_error)
    }
    pub(super) fn load_pairing(&self) -> Result<Option<PairingMarker>, RepositoryError> {
        let _guard = self
            .file
            .lock
            .lock()
            .map_err(|_| RepositoryError::Unavailable)?;
        // Validate the other kind as well; an unknown/corrupt marker fails closed.
        self.read_marker()?;
        let marker = self.read_pairing()?;
        if let Some(marker) = marker.filter(|m| m.repair) {
            let backup = self.read_backup(marker.backup.ok_or(RepositoryError::Corrupt)?)?;
            if backup.device.id != marker.target {
                return Err(RepositoryError::Corrupt);
            }
        }
        Ok(marker)
    }
    pub(super) fn start_pairing(
        &self,
        marker: PairingMarker,
        replaces: Option<DeviceId>,
    ) -> Result<(), RepositoryError> {
        let _guard = self
            .file
            .lock
            .lock()
            .map_err(|_| RepositoryError::Unavailable)?;
        if !marker.valid()
            || marker.stage != PairingStage::Prepared
            || marker.cleanup.is_some()
            || self.read_marker()?.is_some()
        {
            return Err(RepositoryError::Corrupt);
        }
        match (self.read_pairing()?, replaces) {
            (None, None) => {}
            (Some(old), Some(transaction))
                if old.repair
                    && marker.repair
                    && old.transaction == transaction
                    && old.target == marker.target
                    && old.backup == marker.backup
                    && old.prior_selection == marker.prior_selection => {}
            _ => return Err(RepositoryError::Corrupt),
        }
        self.write_pairing(marker)
    }
    pub(super) fn update_pairing(&self, marker: PairingMarker) -> Result<(), RepositoryError> {
        let _guard = self
            .file
            .lock
            .lock()
            .map_err(|_| RepositoryError::Unavailable)?;
        let old = self.read_pairing()?.ok_or(RepositoryError::Corrupt)?;
        if !marker.follows(old) {
            return Err(RepositoryError::Corrupt);
        }
        self.write_pairing(marker)
    }
    pub(super) fn remove_pairing(&self, transaction: DeviceId) -> Result<(), RepositoryError> {
        let _guard = self
            .file
            .lock
            .lock()
            .map_err(|_| RepositoryError::Unavailable)?;
        let marker = self.read_pairing()?.ok_or(RepositoryError::Corrupt)?;
        if marker.transaction != transaction || !marker.complete() {
            return Err(RepositoryError::Corrupt);
        }
        self.file.remove().map_err(recovery_io_error)?;
        if let Some(backup) = marker.backup {
            self.backup_file(backup)?
                .remove()
                .map_err(recovery_io_error)?;
        }
        Ok(())
    }
    fn backup_file(&self, _transaction: DeviceId) -> Result<JsonFile, RepositoryError> {
        self.file
            .sibling("pairing-backup.json")
            .map_err(recovery_io_error)
    }
    pub(super) fn write_backup(&self, backup: &PairingBackup) -> Result<(), RepositoryError> {
        let device = &backup.device;
        self.backup_file(backup.transaction)?
            .write(&DiskBackup {
                // v2 carries `sources`. The nested record ignores unknown
                // fields, so only this version stops an older reader dropping them.
                version: BACKUP_VERSION,
                transaction: backup.transaction.to_string(),
                device: disk_device(device)?,
                trust: backup.trust.as_ref().map(|t| DiskTrustRecord {
                    host: t.host.as_str().into(),
                    certificate_sha256: t.pin.to_hex(),
                }),
            })
            .map_err(recovery_io_error)
    }
    pub(super) fn read_backup(
        &self,
        transaction: DeviceId,
    ) -> Result<PairingBackup, RepositoryError> {
        let backup: DiskBackup = self
            .backup_file(transaction)?
            .read()
            .map_err(recovery_io_error)?
            .ok_or(RepositoryError::Corrupt)?;
        if !(1..=BACKUP_VERSION).contains(&backup.version)
            || id(&backup.transaction)? != transaction
        {
            return Err(RepositoryError::Corrupt);
        }
        Ok(PairingBackup {
            transaction,
            device: convert_device(backup.device)?,
            trust: backup
                .trust
                .as_ref()
                .map(convert_trust)
                .transpose()
                .map_err(|_| RepositoryError::Corrupt)?,
        })
    }
}
