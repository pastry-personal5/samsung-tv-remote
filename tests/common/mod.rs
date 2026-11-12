//! Reusable M8 fake profile boundary: no filesystem, Keychain, TV, or Iced.
use samsung_tv_remote::application::certificate_trust::CertificatePin;
use samsung_tv_remote::application::credential_store::{PairingToken, SecretError};
use samsung_tv_remote::application::device_repository::{RepositoryError, SavedDevice};
use samsung_tv_remote::application::recovery::{ForgetMarker, PairingMarker};
use samsung_tv_remote::application::tv_address::TvHost;
use samsung_tv_remote::application::tv_setup_service::*;
use samsung_tv_remote::application::wake::WakeConfiguration;
use samsung_tv_remote::DeviceId;
use std::sync::Mutex;

#[derive(Default)]
pub struct MemorySetup {
    pub devices: Mutex<Vec<SavedDevice>>,
    pub selected: Mutex<Option<DeviceId>>,
}
impl TvSetupPort for MemorySetup {
    fn saved_devices(&self) -> Result<Vec<SavedDevice>, RepositoryError> {
        Ok(self.devices.lock().unwrap().clone())
    }
    fn selected_device(&self) -> Result<Option<SavedDevice>, RepositoryError> {
        let selected = *self.selected.lock().unwrap();
        Ok(self
            .devices
            .lock()
            .unwrap()
            .iter()
            .find(|d| Some(d.id) == selected)
            .cloned())
    }
    fn pending_recovery(&self) -> Result<Option<ForgetMarker>, RepositoryError> {
        Ok(None)
    }
    fn pending_pairing(&self) -> Result<Option<PairingMarker>, RepositoryError> {
        Ok(None)
    }
    fn select_saved(&self, id: DeviceId) -> Result<SavedDevice, RepositoryError> {
        let saved = self
            .devices
            .lock()
            .unwrap()
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .ok_or(RepositoryError::Corrupt)?;
        *self.selected.lock().unwrap() = Some(id);
        Ok(saved)
    }
    fn save_wake_configuration(
        &self,
        id: DeviceId,
        wake: WakeConfiguration,
    ) -> Result<SavedDevice, RepositoryError> {
        let mut devices = self.devices.lock().unwrap();
        let device = devices
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or(RepositoryError::Corrupt)?;
        device.wake = wake;
        Ok(device.clone())
    }
    fn commit_pairing(
        &self,
        _: &str,
        _: TvHost,
        _: CertificatePin,
        _: PairingToken,
    ) -> Result<SavedDevice, SetupError> {
        panic!("legacy pairing must not run")
    }
    fn repair(
        &self,
        _: DeviceId,
        _: &str,
        _: TvHost,
        _: CertificatePin,
        _: PairingToken,
    ) -> Result<SavedDevice, SetupError> {
        panic!("legacy repair must not run")
    }
    fn reconnect_material(&self, id: DeviceId) -> Result<ReconnectMaterial, ReconnectError> {
        let device = self
            .devices
            .lock()
            .unwrap()
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .ok_or(ReconnectError::MissingDevice)?;
        Ok(ReconnectMaterial {
            device,
            pin: CertificatePin::from_der(b"fake certificate"),
            token: PairingToken::from_stored("synthetic-token".into()).unwrap(),
        })
    }
    fn save_rotated_token(&self, _: DeviceId, _: &PairingToken) -> Result<(), SecretError> {
        panic!("legacy rotation must not run")
    }
    fn forget(&self, _: DeviceId) -> ForgetResult {
        panic!("legacy forget must not run")
    }
}
