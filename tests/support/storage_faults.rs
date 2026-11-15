//! Fault switches at the actual setup service's storage boundaries.
use samsung_tv_remote::application::{
    certificate_trust::{CertificateTrustStore, TrustError, TrustRecord},
    credential_store::SecretError,
    device_repository::{DeviceRepository, RepositoryError, SavedDevice},
};
use samsung_tv_remote::infrastructure::preferences::{
    LocalCertificateTrustStore, LocalDeviceRepository,
};
use samsung_tv_remote::DeviceId;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Condvar, Mutex,
};

pub struct Script<E> {
    pub error: Mutex<Option<E>>,
    pub gate: Mutex<Option<Arc<Gate>>>,
    pub calls: AtomicUsize,
}
impl<E> Default for Script<E> {
    fn default() -> Self {
        Self {
            error: Mutex::new(None),
            gate: Mutex::new(None),
            calls: AtomicUsize::new(0),
        }
    }
}
impl<E: Copy> Script<E> {
    pub fn check(&self) -> Result<(), E> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let gate = self.gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.wait();
        }
        match *self.error.lock().unwrap() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
pub struct Gate {
    open: Mutex<bool>,
    condition: Condvar,
    pub entered: AtomicBool,
}
pub struct Release(Arc<Gate>);
impl Drop for Release {
    fn drop(&mut self) {
        *self.0.open.lock().unwrap() = true;
        self.0.condition.notify_all();
    }
}
impl Gate {
    pub fn closed() -> (Arc<Self>, Release) {
        let gate = Arc::new(Self {
            open: Mutex::new(false),
            condition: Condvar::new(),
            entered: AtomicBool::new(false),
        });
        (gate.clone(), Release(gate))
    }
    fn wait(&self) {
        let mut open = self.open.lock().unwrap();
        self.entered.store(true, Ordering::SeqCst);
        while !*open {
            open = self.condition.wait(open).unwrap();
        }
    }
}
#[derive(Default)]
pub struct Faults {
    pub setup_commit: Script<SetupError>,
    pub list: Script<RepositoryError>,
    pub select: Script<RepositoryError>,
    pub device_save: Script<RepositoryError>,
    pub device_delete: Script<RepositoryError>,
    pub trust_load: Script<TrustError>,
    pub trust_save: Script<TrustError>,
    pub trust_delete: Script<TrustError>,
    pub secret_load: Script<SecretError>,
    pub secret_save: Script<SecretError>,
    pub secret_delete: Script<SecretError>,
    pub hide_records: AtomicBool,
    pub missing_secret: AtomicBool,
}
pub struct Repository {
    pub inner: LocalDeviceRepository,
    pub faults: Arc<Faults>,
}
impl DeviceRepository for Repository {
    fn list(&self) -> Result<Vec<SavedDevice>, RepositoryError> {
        self.faults.list.check()?;
        if self.faults.hide_records.load(Ordering::SeqCst) {
            return Ok(Vec::new());
        }
        self.inner.list()
    }
    fn save(&self, value: &SavedDevice) -> Result<(), RepositoryError> {
        self.faults.device_save.check()?;
        self.inner.save(value)
    }
    fn delete(&self, id: DeviceId) -> Result<(), RepositoryError> {
        self.faults.device_delete.check()?;
        self.inner.delete(id)
    }
    fn selected(&self) -> Result<Option<DeviceId>, RepositoryError> {
        self.inner.selected()
    }
    fn select(&self, id: Option<DeviceId>) -> Result<(), RepositoryError> {
        self.faults.select.check()?;
        self.inner.select(id)
    }
}
pub struct Trust {
    pub inner: LocalCertificateTrustStore,
    pub faults: Arc<Faults>,
}
impl CertificateTrustStore for Trust {
    fn load(&self, id: DeviceId) -> Result<Option<TrustRecord>, TrustError> {
        self.faults.trust_load.check()?;
        self.inner.load(id)
    }
    fn save(&self, id: DeviceId, value: &TrustRecord) -> Result<(), TrustError> {
        self.faults.trust_save.check()?;
        self.inner.save(id, value)
    }
    fn delete(&self, id: DeviceId) -> Result<(), TrustError> {
        self.faults.trust_delete.check()?;
        self.inner.delete(id)
    }
}

/// A setup-boundary rejection before commit is also legal for other injected
/// setup ports. Exercise every typed failure through the real owner/HTTP path,
/// separately from the actual journal/store fault cases above.
pub struct Setup {
    pub inner: Arc<dyn samsung_tv_remote::application::tv_setup_service::TvSetupPort>,
    pub faults: Arc<Faults>,
}
macro_rules! forward_setup {
    ($($name:ident($($arg:ident:$ty:ty),*)->$result:ty;)*) => {
        $(fn $name(&self,$($arg:$ty),*)->$result {self.inner.$name($($arg),*)})*
    }
}
use samsung_tv_remote::application::{
    certificate_trust::CertificatePin,
    credential_store::PairingToken,
    recovery::{ForgetMarker, PairingMarker},
    tv_address::TvHost,
    tv_setup_service::{
        ApprovedPairing, ForgetResult, ReconnectError, ReconnectMaterial, SetupError, TvSetupPort,
    },
    wake::WakeConfiguration,
};
impl TvSetupPort for Setup {
    fn commit_approved_pairing(
        &self,
        approved: ApprovedPairing,
    ) -> Result<SavedDevice, SetupError> {
        self.faults.setup_commit.check()?;
        self.inner.commit_approved_pairing(approved)
    }
    forward_setup! {
        saved_devices()->Result<Vec<SavedDevice>,RepositoryError>;
        selected_device()->Result<Option<SavedDevice>,RepositoryError>;
        select_saved(id:DeviceId)->Result<SavedDevice,RepositoryError>;
        commit_pairing(label:&str,host:TvHost,pin:CertificatePin,token:PairingToken)->Result<SavedDevice,SetupError>;
        repair(id:DeviceId,label:&str,host:TvHost,pin:CertificatePin,token:PairingToken)->Result<SavedDevice,SetupError>;
        reconnect_material(id:DeviceId)->Result<ReconnectMaterial,ReconnectError>;
        save_rotated_token(id:DeviceId,token:&PairingToken)->Result<(),SecretError>;
        save_wake_configuration(id:DeviceId,wake:WakeConfiguration)->Result<SavedDevice,RepositoryError>;
        save_sources(id:DeviceId,sources:samsung_tv_remote::application::sources::SourceConfiguration)->Result<SavedDevice,RepositoryError>;
        forget(id:DeviceId)->ForgetResult;
        pending_recovery()->Result<Option<ForgetMarker>,RepositoryError>;
        forget_durable(id:DeviceId)->Result<ForgetResult,RepositoryError>;
        recover_forget(marker:ForgetMarker)->Result<ForgetResult,RepositoryError>;
        pending_pairing()->Result<Option<PairingMarker>,RepositoryError>;
        commit_rotated_token(material:ReconnectMaterial,token:PairingToken)->Result<(),SetupError>;
        recover_first_pair(marker:PairingMarker)->Result<ForgetResult,RepositoryError>;
    }
}
