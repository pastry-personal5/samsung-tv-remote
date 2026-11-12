//! Server-owned API credentials. Bootstrap reads existing secrets; genuine
//! first use may create one item. Durable non-secret presence prevents silent
//! replacement after deletion, including across server restarts.
use super::server_keychain::{CredentialError, ServerKeychain};
use crate::{
    infrastructure::private_files::RuntimeDirectory,
    server::bootstrap::{self, ApiCredentials, BootstrapError, Role},
};
use std::sync::{Arc, Mutex};

pub struct ServerApiCredentials {
    keychain: ServerKeychain,
    directory: Arc<RuntimeDirectory>,
    serial: Mutex<()>,
}
impl ServerApiCredentials {
    pub fn new(directory: Arc<RuntimeDirectory>) -> Result<Self, CredentialError> {
        Ok(Self {
            keychain: ServerKeychain::api()?,
            directory,
            serial: Mutex::new(()),
        })
    }
}
impl ApiCredentials for ServerApiCredentials {
    fn load(&self, role: Role) -> Result<[u8; 32], BootstrapError> {
        let _guard = self
            .serial
            .lock()
            .map_err(|_| BootstrapError::CredentialUnavailable)?;
        load_role(&self.keychain, &self.directory, role)
    }
}
trait Vault {
    fn read(&self, account: &str) -> Result<Option<Vec<u8>>, CredentialError>;
    fn create(&self, account: &str, bytes: &[u8]) -> Result<(), CredentialError>;
}
impl Vault for ServerKeychain {
    fn read(&self, account: &str) -> Result<Option<Vec<u8>>, CredentialError> {
        self.load(account)
    }
    fn create(&self, account: &str, bytes: &[u8]) -> Result<(), CredentialError> {
        ServerKeychain::create(self, account, bytes)
    }
}
pub fn bootstrap_error(error: CredentialError) -> BootstrapError {
    match error {
        CredentialError::Locked | CredentialError::InteractionRequired => {
            BootstrapError::InteractionRequired
        }
        CredentialError::Denied => BootstrapError::Denied,
        CredentialError::Unavailable => BootstrapError::CredentialUnavailable,
    }
}
fn load_role(
    vault: &impl Vault,
    directory: &RuntimeDirectory,
    role: Role,
) -> Result<[u8; 32], BootstrapError> {
    let account = match role {
        Role::Gui => "gui",
        Role::Mcp => "mcp",
    };
    let name = format!("api-{account}.present");
    let expected = format!("control-v1:api:{account}:v1");
    let marker = directory
        .read(&name, 128)
        .map_err(|_| BootstrapError::UnsafeMetadata)?;
    if marker
        .as_ref()
        .is_some_and(|bytes| bytes != expected.as_bytes())
    {
        return Err(BootstrapError::UnsafeMetadata);
    }
    let bytes = match vault.read(account).map_err(bootstrap_error)? {
        Some(bytes) => bytes,
        None if marker.is_some() => return Err(BootstrapError::CredentialUnavailable),
        None => {
            let secret = bootstrap::random()?;
            // Add never overwrites a raced-in item. A failed Add cannot become
            // success by guessing whether its error meant absence or denial.
            vault.create(account, &secret).map_err(bootstrap_error)?;
            vault
                .read(account)
                .map_err(bootstrap_error)?
                .ok_or(BootstrapError::CredentialUnavailable)?
        }
    };
    let secret: [u8; 32] = bytes
        .try_into()
        .map_err(|_| BootstrapError::CredentialUnavailable)?;
    if marker.is_none() {
        directory
            .replace(&name, expected.as_bytes())
            .map_err(|_| BootstrapError::UnsafeMetadata)?;
    }
    Ok(secret)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    struct Fake {
        values: Mutex<HashMap<String, Vec<u8>>>,
        blocked: Mutex<Option<CredentialError>>,
        writes: Mutex<usize>,
    }
    impl Vault for Fake {
        fn read(&self, account: &str) -> Result<Option<Vec<u8>>, CredentialError> {
            if let Some(error) = *self.blocked.lock().unwrap() {
                return Err(error);
            }
            Ok(self.values.lock().unwrap().get(account).cloned())
        }
        fn create(&self, account: &str, bytes: &[u8]) -> Result<(), CredentialError> {
            *self.writes.lock().unwrap() += 1;
            let mut values = self.values.lock().unwrap();
            if values.contains_key(account) {
                return Err(CredentialError::Denied);
            }
            values.insert(account.into(), bytes.into());
            Ok(())
        }
    }
    fn fake() -> Fake {
        Fake {
            values: Mutex::new(HashMap::new()),
            blocked: Mutex::new(None),
            writes: Mutex::new(0),
        }
    }
    #[test]
    fn api_first_use_restart_and_deleted_item_never_regenerate_or_overwrite() {
        let base = tempfile::tempdir().unwrap();
        let directory =
            RuntimeDirectory::open(&base.path().canonicalize().unwrap().join("control-v1"))
                .unwrap();
        let vault = fake();
        let gui = load_role(&vault, &directory, Role::Gui).unwrap();
        let mcp = load_role(&vault, &directory, Role::Mcp).unwrap();
        assert_ne!(gui, mcp);
        assert_eq!(*vault.writes.lock().unwrap(), 2);
        let reopened =
            RuntimeDirectory::open(&base.path().canonicalize().unwrap().join("control-v1"))
                .unwrap();
        assert_eq!(load_role(&vault, &reopened, Role::Gui).unwrap(), gui);
        assert_eq!(*vault.writes.lock().unwrap(), 2);
        vault.values.lock().unwrap().remove("gui");
        assert_eq!(
            load_role(&vault, &reopened, Role::Gui),
            Err(BootstrapError::CredentialUnavailable)
        );
        assert_eq!(*vault.writes.lock().unwrap(), 2);
        vault
            .values
            .lock()
            .unwrap()
            .insert("gui".into(), vec![3; 31]);
        assert_eq!(
            load_role(&vault, &reopened, Role::Gui),
            Err(BootstrapError::CredentialUnavailable)
        );
        assert_eq!(*vault.writes.lock().unwrap(), 2);
    }
    #[test]
    fn blocked_or_corrupt_stores_do_not_trigger_creation() {
        let base = tempfile::tempdir().unwrap();
        let directory =
            RuntimeDirectory::open(&base.path().canonicalize().unwrap().join("control-v1"))
                .unwrap();
        let vault = fake();
        for error in [
            CredentialError::Locked,
            CredentialError::Denied,
            CredentialError::InteractionRequired,
            CredentialError::Unavailable,
        ] {
            *vault.blocked.lock().unwrap() = Some(error);
            assert_eq!(
                load_role(&vault, &directory, Role::Gui),
                Err(bootstrap_error(error))
            );
            assert_eq!(*vault.writes.lock().unwrap(), 0);
            assert!(directory.read("api-gui.present", 128).unwrap().is_none());
        }
        *vault.blocked.lock().unwrap() = None;
        // Existing unmarked items are adopted, never overwritten.
        vault
            .values
            .lock()
            .unwrap()
            .insert("gui".into(), vec![7; 32]);
        assert_eq!(load_role(&vault, &directory, Role::Gui).unwrap(), [7; 32]);
        assert_eq!(*vault.writes.lock().unwrap(), 0);
        directory.replace("api-gui.present", b"m8-fake-v1").unwrap();
        assert_eq!(
            load_role(&vault, &directory, Role::Gui),
            Err(BootstrapError::UnsafeMetadata)
        );
        assert_eq!(*vault.writes.lock().unwrap(), 0);
    }
    #[test]
    fn raced_first_use_add_never_overwrites_or_claims_success() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Raced(AtomicUsize);
        impl Vault for Raced {
            fn read(&self, _: &str) -> Result<Option<Vec<u8>>, CredentialError> {
                Ok(if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                    None
                } else {
                    Some(vec![9; 32])
                })
            }
            fn create(&self, _: &str, _: &[u8]) -> Result<(), CredentialError> {
                Err(CredentialError::Denied)
            }
        }
        let base = tempfile::tempdir().unwrap();
        let directory =
            RuntimeDirectory::open(&base.path().canonicalize().unwrap().join("control-v1"))
                .unwrap();
        let vault = Raced(AtomicUsize::new(0));
        assert_eq!(
            load_role(&vault, &directory, Role::Gui),
            Err(BootstrapError::Denied)
        );
        assert!(directory.read("api-gui.present", 128).unwrap().is_none());
        assert_eq!(load_role(&vault, &directory, Role::Gui).unwrap(), [9; 32]);
    }
}
