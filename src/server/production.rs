//! Fresh production storage and server-only adapter composition. This module
//! never opens the legacy root or adopts a development namespace.
use crate::{
    application::{tv_control_coordinator::AppServices, tv_setup_service::TvSetupService},
    infrastructure::{
        macos::server_keychain::{CredentialError, ServerPairingCredentials},
        preferences::{LocalCertificateTrustStore, LocalDeviceRepository, LocalRecoveryJournal},
        private_files::{FileError, Namespace, OwnershipLock, RuntimeDirectory, CONTROL_NAMESPACE},
        samsung::session::SamsungGateway,
        ssdp_discovery::SsdpDiscovery,
        wake_udp::UdpWakeTransport,
    },
};
use std::{path::PathBuf, sync::Arc};

pub struct ProductionProfile {
    pub directory: Arc<RuntimeDirectory>,
    data: Arc<RuntimeDirectory>,
}
impl ProductionProfile {
    /// Path selection alone performs no I/O and is safe in client composition.
    pub fn base() -> Result<PathBuf, FileError> {
        // Detached spawn intentionally inherits no HOME. Resolve the effective
        // user's home from the OS rather than a redirectable environment value.
        let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut buffer = vec![0u8; 16 * 1024];
        let mut result = std::ptr::null_mut();
        // SAFETY: the output record and buffer remain live until pw_dir is
        // copied. getpwuid_r provides only pointers into these owned buffers.
        let home = unsafe {
            if libc::getpwuid_r(
                libc::geteuid(),
                record.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            ) != 0
                || result.is_null()
            {
                return Err(FileError::Unavailable);
            }
            let record = record.assume_init();
            if record.pw_dir.is_null() {
                return Err(FileError::Unavailable);
            }
            use std::os::unix::ffi::OsStrExt;
            PathBuf::from(std::ffi::OsStr::from_bytes(
                std::ffi::CStr::from_ptr(record.pw_dir).to_bytes(),
            ))
        };
        if !home.is_absolute() {
            return Err(FileError::UnsafePath);
        }
        Ok(home.join("Library/Application Support/Samsung TV Remote Control"))
    }
    pub fn directory() -> Result<Arc<RuntimeDirectory>, FileError> {
        let base = RuntimeDirectory::open(&Self::base()?)?;
        Ok(Arc::new(base.child(CONTROL_NAMESPACE)?))
    }
    pub fn open() -> Result<(Self, OwnershipLock), FileError> {
        let directory = Self::directory()?;
        let ownership = directory.lock()?;
        Self::initialize(directory, ownership)
    }
    pub fn inherited(file: std::fs::File) -> Result<(Self, OwnershipLock), FileError> {
        let directory = Self::directory()?;
        let ownership = directory.adopt_lock(file)?;
        Self::initialize(directory, ownership)
    }
    fn initialize(
        directory: Arc<RuntimeDirectory>,
        ownership: OwnershipLock,
    ) -> Result<(Self, OwnershipLock), FileError> {
        let data = match directory.read("namespace", 64)? {
            None => {
                // Exclusive child creation prevents adoption of untagged data,
                // including an interrupted initialization or copied legacy root.
                let data = directory.new_child("data")?;
                directory.replace("namespace", CONTROL_NAMESPACE.as_bytes())?;
                data
            }
            Some(tag) if tag == CONTROL_NAMESPACE.as_bytes() => directory.child("data")?,
            Some(_) => return Err(FileError::UnsafePath),
        };
        Ok((
            Self {
                directory,
                data: Arc::new(data),
            },
            ownership,
        ))
    }
    /// Construct only in the server process, after ownership and profile checks.
    pub fn services(&self) -> Result<AppServices, CredentialError> {
        let setup = TvSetupService::new(
            LocalDeviceRepository::in_namespaced_directory(self.data.clone(), Namespace::Control),
            ServerPairingCredentials::new()?,
            LocalCertificateTrustStore::in_namespaced_directory(
                self.data.clone(),
                Namespace::Control,
            ),
        )
        .with_journal(Arc::new(LocalRecoveryJournal::in_namespaced_directory(
            self.data.clone(),
            Namespace::Control,
        )));
        Ok(AppServices::new(
            Some(Arc::new(setup)),
            Arc::new(SsdpDiscovery),
            Arc::new(SamsungGateway),
            Arc::new(UdpWakeTransport),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{device_repository::DeviceRepository, recovery::RecoveryJournal};
    #[test]
    fn fresh_control_profile_is_empty_and_rejects_fake_or_untagged_data() {
        let base = tempfile::tempdir().unwrap();
        let directory = Arc::new(
            RuntimeDirectory::open(&base.path().canonicalize().unwrap().join(CONTROL_NAMESPACE))
                .unwrap(),
        );
        let owner = directory.lock().unwrap();
        let (profile, owner) = ProductionProfile::initialize(directory.clone(), owner).unwrap();
        assert!(LocalDeviceRepository::in_namespaced_directory(
            profile.data.clone(),
            Namespace::Control
        )
        .list()
        .unwrap()
        .is_empty());
        assert!(LocalRecoveryJournal::in_namespaced_directory(
            profile.data.clone(),
            Namespace::Control
        )
        .load()
        .unwrap()
        .is_none());
        drop(owner);
        // A copied development tag cannot become a production profile.
        directory.replace("namespace", b"m8-fake-v1").unwrap();
        assert!(matches!(
            ProductionProfile::initialize(directory.clone(), directory.lock().unwrap()),
            Err(FileError::UnsafePath)
        ));
        directory.remove("namespace").unwrap();
        assert!(
            ProductionProfile::initialize(directory.clone(), directory.lock().unwrap()).is_err()
        );
    }
}
