//! Non-secret crash evidence for serialized setup mutations. Never put network
//! addresses, certificates, or credentials in a recovery marker.
use crate::domain::DeviceId;

use super::device_repository::RepositoryError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForgetMarker {
    pub transaction: DeviceId,
    pub target: DeviceId,
    pub selection_cleared: bool,
    pub credential_removed: bool,
    pub trust_removed: bool,
    pub record_removed: bool,
}

impl ForgetMarker {
    pub fn valid(self) -> bool {
        self.selection_cleared
            || !(self.credential_removed || self.trust_removed || self.record_removed)
    }

    pub fn prepared(target: DeviceId) -> Self {
        Self {
            transaction: DeviceId::generate(),
            target,
            selection_cleared: false,
            credential_removed: false,
            trust_removed: false,
            record_removed: false,
        }
    }
}

/// A profile has one serialized mutation lane and at most one active marker.
/// Writes and removal must sync the file and its containing directory.
pub trait RecoveryJournal: Send + Sync {
    fn load(&self) -> Result<Option<ForgetMarker>, RepositoryError>;
    fn write(&self, marker: ForgetMarker) -> Result<(), RepositoryError>;
    fn clear(&self, transaction: DeviceId) -> Result<(), RepositoryError>;
    // An adapter that cannot write pairing markers cannot have one pending;
    // begin_pairing still fails closed, so no pairing mutation can start.
    fn pairing(&self) -> Result<Option<PairingMarker>, RepositoryError> {
        Ok(None)
    }
    fn begin_pairing(
        &self,
        _marker: PairingMarker,
        _replaces: Option<DeviceId>,
    ) -> Result<(), RepositoryError> {
        Err(RepositoryError::Unavailable)
    }
    fn advance_pairing(&self, _marker: PairingMarker) -> Result<(), RepositoryError> {
        Err(RepositoryError::Unavailable)
    }
    fn clear_pairing(&self, _transaction: DeviceId) -> Result<(), RepositoryError> {
        Err(RepositoryError::Unavailable)
    }
    fn save_backup(&self, _backup: &PairingBackup) -> Result<(), RepositoryError> {
        Err(RepositoryError::Unavailable)
    }
    fn load_backup(&self, _transaction: DeviceId) -> Result<PairingBackup, RepositoryError> {
        Err(RepositoryError::Unavailable)
    }
}

use super::certificate_trust::TrustRecord;
use super::device_repository::SavedDevice;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PairingStage {
    Prepared,
    TokenSaved,
    TrustSaved,
    DeviceSaved,
    SelectionSaved,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PairingCleanup {
    pub credential_removed: bool,
    pub trust_removed: bool,
    pub record_removed: bool,
    pub selection_restored: bool,
}
impl PairingCleanup {
    pub fn complete(self) -> bool {
        self.credential_removed
            && self.trust_removed
            && self.record_removed
            && self.selection_restored
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairingMarker {
    pub transaction: DeviceId,
    pub target: DeviceId,
    pub prior_selection: Option<DeviceId>,
    pub repair: bool,
    pub backup: Option<DeviceId>,
    pub stage: PairingStage,
    pub cleanup: Option<PairingCleanup>,
}
impl PairingMarker {
    pub fn valid(self) -> bool {
        self.repair == self.backup.is_some() && !(self.repair && self.cleanup.is_some())
    }
    pub fn complete(self) -> bool {
        match self.cleanup {
            Some(cleanup) => cleanup.complete(),
            None => self.stage == PairingStage::SelectionSaved,
        }
    }
    pub fn follows(self, old: Self) -> bool {
        self.valid()
            && self.transaction == old.transaction
            && self.target == old.target
            && self.prior_selection == old.prior_selection
            && self.repair == old.repair
            && self.backup == old.backup
            && self.stage >= old.stage
            && match (old.cleanup, self.cleanup) {
                (Some(_), None) => false,
                (Some(old), Some(new)) => {
                    (!old.credential_removed || new.credential_removed)
                        && (!old.trust_removed || new.trust_removed)
                        && (!old.record_removed || new.record_removed)
                        && (!old.selection_restored || new.selection_restored)
                }
                _ => true,
            }
    }
}

#[derive(Debug, Clone)]
pub struct PairingBackup {
    pub transaction: DeviceId,
    pub device: SavedDevice,
    pub trust: Option<TrustRecord>,
}
