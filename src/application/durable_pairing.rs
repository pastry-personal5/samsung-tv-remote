//! Journaled Pairing/re-pair commit. A non-secret marker is durable before the
//! first store mutation, so a crash leaves evidence instead of a silent partial.
use super::{ApprovedPairing, ForgetResult, SetupError, TvSetupService};
use crate::application::certificate_trust::{CertificateTrustStore, TrustRecord};
use crate::application::credential_store::CredentialStore;
use crate::application::device_repository::{DeviceRepository, RepositoryError, SavedDevice};
use crate::application::recovery::{PairingBackup, PairingMarker, PairingStage};
use crate::domain::DeviceId;

impl<R: DeviceRepository, S: CredentialStore, T: CertificateTrustStore> TvSetupService<R, S, T> {
    pub(super) fn commit_durable_pairing(
        &self,
        approved: ApprovedPairing,
    ) -> Result<SavedDevice, SetupError> {
        let journal = self
            .journal
            .as_ref()
            .ok_or(SetupError::Preferences(RepositoryError::Unavailable))?;
        let device = approved.device;
        let label = &device.label;
        if label.is_empty() || label.len() > 80 || label.chars().any(char::is_control) {
            return Err(SetupError::InvalidLabel);
        }
        let pending = journal.pairing().map_err(SetupError::Preferences)?;
        if journal.load().map_err(SetupError::Preferences)?.is_some()
            || pending != approved.recovery
            || approved
                .recovery
                .is_some_and(|m| !m.repair || m.target != device.id)
        {
            return Err(SetupError::Preferences(RepositoryError::Unavailable));
        }
        let record = TrustRecord {
            host: device.host.clone(),
            pin: approved.pin,
        };
        let prior_selection = self.devices.selected().map_err(SetupError::Preferences)?;
        let mut marker = PairingMarker {
            transaction: DeviceId::generate(),
            target: device.id,
            prior_selection,
            repair: approved.repair,
            backup: approved.repair.then(DeviceId::generate),
            stage: PairingStage::Prepared,
            cleanup: None,
        };
        if let Some(previous) = approved.recovery {
            // A fresh TV approval replaces the ambiguous token; retain the
            // original metadata backup across repeated interrupted recovery.
            marker.backup = previous.backup;
            marker.prior_selection = previous.prior_selection;
        } else if approved.repair {
            let previous = self
                .devices
                .list()
                .map_err(SetupError::Preferences)?
                .into_iter()
                .find(|saved| saved.id == device.id)
                .ok_or(SetupError::Preferences(RepositoryError::Corrupt))?;
            journal
                .save_backup(&PairingBackup {
                    transaction: marker.backup.expect("repair backup"),
                    device: previous,
                    trust: self.trust.load(device.id).map_err(SetupError::Trust)?,
                })
                .map_err(SetupError::Preferences)?;
        }
        journal
            .begin_pairing(marker, approved.recovery.map(|m| m.transaction))
            .map_err(SetupError::Preferences)?;

        let steps = (|| -> Result<(), SetupError> {
            self.secrets
                .save(device.id, &approved.token)
                .map_err(SetupError::Credential)?;
            self.advance(&mut marker, PairingStage::TokenSaved)?;
            self.trust
                .save(device.id, &record)
                .map_err(SetupError::Trust)?;
            self.advance(&mut marker, PairingStage::TrustSaved)?;
            self.devices
                .save(&device)
                .map_err(SetupError::Preferences)?;
            self.advance(&mut marker, PairingStage::DeviceSaved)?;
            if !approved.repair {
                self.devices
                    .select(Some(device.id))
                    .map_err(SetupError::Preferences)?;
            }
            self.advance(&mut marker, PairingStage::SelectionSaved)
        })();
        match steps {
            Ok(()) => {
                journal
                    .clear_pairing(marker.transaction)
                    .map_err(SetupError::Preferences)?;
                Ok(device)
            }
            // Never erase evidence after a partial write. Recovery is explicit
            // and re-pair requires fresh approval because old tokens are secret.
            Err(_) => Err(SetupError::PartialCleanup),
        }
    }

    fn advance(&self, marker: &mut PairingMarker, stage: PairingStage) -> Result<(), SetupError> {
        marker.stage = stage;
        self.journal
            .as_ref()
            .ok_or(SetupError::Preferences(RepositoryError::Unavailable))?
            .advance_pairing(*marker)
            .map_err(SetupError::Preferences)
    }

    /// Removes only the marker's reserved ID and restores a still-valid prior
    /// selection. It never connects; each completed step is journaled.
    pub(super) fn cleanup_first_pair(
        &self,
        mut marker: PairingMarker,
    ) -> Result<ForgetResult, RepositoryError> {
        let journal = self.journal.as_ref().ok_or(RepositoryError::Unavailable)?;
        if marker.repair || !marker.valid() || journal.pairing()? != Some(marker) {
            return Err(RepositoryError::Corrupt);
        }
        let mut cleanup = marker.cleanup.unwrap_or_default();
        macro_rules! step {
            ($done:ident, $action:expr) => {
                if !cleanup.$done && $action.is_ok() {
                    cleanup.$done = true;
                    marker.cleanup = Some(cleanup);
                    journal.advance_pairing(marker)?;
                }
            };
        }
        if !cleanup.selection_restored {
            let selected = self.devices.selected()?;
            let prior = match marker.prior_selection {
                Some(id) if self.devices.list()?.iter().any(|device| device.id == id) => Some(id),
                _ => None,
            };
            if selected != prior && selected == Some(marker.target) {
                self.devices.select(prior)?;
            }
            cleanup.selection_restored = true;
            marker.cleanup = Some(cleanup);
            journal.advance_pairing(marker)?;
        }
        step!(credential_removed, self.secrets.delete(marker.target));
        step!(trust_removed, self.trust.delete(marker.target));
        step!(record_removed, self.devices.delete(marker.target));
        let result = ForgetResult {
            credential_removed: cleanup.credential_removed,
            trust_removed: cleanup.trust_removed,
            record_removed: cleanup.record_removed,
        };
        if cleanup.complete() {
            journal.clear_pairing(marker.transaction)?;
        }
        Ok(result)
    }
}
