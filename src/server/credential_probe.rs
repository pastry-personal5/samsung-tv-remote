//! Explicit native acceptance tool in server mode. Only fixed-prefix,
//! canonical v4 test accounts are reachable; GUI/MCP and TV accounts are not.
//! Synthetic markers are never API secrets and no loaded value is printed.
use crate::infrastructure::macos::{
    server_keychain::{CredentialError, ServerKeychain},
    signed_peer::SignedPeer,
};
use serde_json::{json, Value};

const FIRST: &[u8] = b"m9-native-probe-first";
const UPDATED: &[u8] = b"m9-native-probe-updated";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Create,
    Read,
    Update,
    Delete,
}
impl Action {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "create" => Some(Self::Create),
            "read" => Some(Self::Read),
            "update" => Some(Self::Update),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Read => "read",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    Store(CredentialError),
    Existing,
    Missing,
    Changed,
}
impl Failure {
    fn code(self) -> &'static str {
        match self {
            Self::Store(CredentialError::Locked) => "credential_locked",
            Self::Store(CredentialError::Denied) => "credential_denied",
            Self::Store(CredentialError::InteractionRequired) => "interaction_required",
            Self::Store(CredentialError::Unavailable) => "credential_unavailable",
            Self::Existing => "probe_account_exists",
            Self::Missing => "probe_account_missing",
            Self::Changed => "probe_account_changed",
        }
    }
}
impl From<CredentialError> for Failure {
    fn from(error: CredentialError) -> Self {
        Self::Store(error)
    }
}
trait Store {
    fn load(&self, account: &str) -> Result<Option<Vec<u8>>, CredentialError>;
    fn create(&self, account: &str, value: &[u8]) -> Result<(), CredentialError>;
    fn save(&self, account: &str, value: &[u8]) -> Result<(), CredentialError>;
    fn delete(&self, account: &str) -> Result<(), CredentialError>;
}
impl Store for ServerKeychain {
    fn load(&self, account: &str) -> Result<Option<Vec<u8>>, CredentialError> {
        ServerKeychain::load(self, account)
    }
    fn create(&self, account: &str, value: &[u8]) -> Result<(), CredentialError> {
        ServerKeychain::create(self, account, value)
    }
    fn save(&self, account: &str, value: &[u8]) -> Result<(), CredentialError> {
        ServerKeychain::save(self, account, value)
    }
    fn delete(&self, account: &str) -> Result<(), CredentialError> {
        ServerKeychain::delete(self, account)
    }
}
pub fn account(id: &str) -> Option<String> {
    let uuid = uuid::Uuid::parse_str(id).ok()?;
    (uuid.to_string() == id && uuid.get_version_num() == 4).then(|| format!("m9-probe-{uuid}"))
}
fn execute(store: &impl Store, account: &str, action: Action) -> Result<(), Failure> {
    let prior = store.load(account)?;
    match action {
        Action::Create => {
            if prior.is_some() {
                return Err(Failure::Existing);
            }
            store.create(account, FIRST)?;
            // A raced duplicate Add must fail without changing the first item.
            if store.create(account, UPDATED).is_ok() {
                return Err(Failure::Changed);
            }
            if store.load(account)?.as_deref() != Some(FIRST) {
                return Err(Failure::Changed);
            }
        }
        Action::Read => match prior.as_deref() {
            Some(value) if value == FIRST || value == UPDATED => (),
            None => return Err(Failure::Missing),
            _ => return Err(Failure::Changed),
        },
        Action::Update => {
            if prior.as_deref() != Some(FIRST) {
                return Err(if prior.is_none() {
                    Failure::Missing
                } else {
                    Failure::Changed
                });
            }
            store.save(account, UPDATED)?;
            if store.load(account)?.as_deref() != Some(UPDATED) {
                return Err(Failure::Changed);
            }
        }
        Action::Delete => {
            // Never remove an unrelated item even if a test UUID collides or
            // the caller accidentally reuses an account. Each store settles
            // independently so partial fixture cleanup remains possible.
            match prior.as_deref() {
                Some(value) if value == FIRST || value == UPDATED => (),
                None => return Ok(()),
                _ => return Err(Failure::Changed),
            }
            store.delete(account)?;
            if store.load(account)?.is_some() {
                return Err(Failure::Changed);
            }
        }
    }
    Ok(())
}
/// Caller dispatches this before Iced. Signed identity is checked before any
/// Keychain construction; this mode never opens a profile or starts a server.
pub fn run(action: Action, id: &str) -> Result<(Value, bool), &'static str> {
    let account = account(id).ok_or("invalid_probe_id")?;
    SignedPeer::for_current_executable().map_err(|_| "signed_identity_required")?;
    let check = |store: Result<ServerKeychain, CredentialError>| {
        store
            .map_err(Failure::Store)
            .and_then(|store| execute(&store, &account, action))
    };
    let pairing = check(ServerKeychain::pairing());
    let api = check(ServerKeychain::api());
    let code = |result: Result<(), Failure>| result.err().map(Failure::code).unwrap_or("passed");
    Ok((
        json!({"action":action.name(),"pairing":code(pairing),"api":code(api),"keychain_ui":"disabled"}),
        pairing.is_ok() && api.is_ok(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    #[derive(Default)]
    struct Fake {
        value: Mutex<Option<Vec<u8>>>,
        blocked: Mutex<Option<CredentialError>>,
        writes: Mutex<Vec<&'static str>>,
    }
    impl Store for Fake {
        fn load(&self, _: &str) -> Result<Option<Vec<u8>>, CredentialError> {
            if let Some(error) = *self.blocked.lock().unwrap() {
                return Err(error);
            }
            Ok(self.value.lock().unwrap().clone())
        }
        fn create(&self, _: &str, value: &[u8]) -> Result<(), CredentialError> {
            let mut current = self.value.lock().unwrap();
            if current.is_some() {
                return Err(CredentialError::Denied);
            }
            *current = Some(value.into());
            self.writes.lock().unwrap().push("create");
            Ok(())
        }
        fn save(&self, _: &str, value: &[u8]) -> Result<(), CredentialError> {
            *self.value.lock().unwrap() = Some(value.into());
            self.writes.lock().unwrap().push("update");
            Ok(())
        }
        fn delete(&self, _: &str) -> Result<(), CredentialError> {
            *self.value.lock().unwrap() = None;
            self.writes.lock().unwrap().push("delete");
            Ok(())
        }
    }
    #[test]
    fn only_canonical_disposable_accounts_and_known_values_can_be_changed() {
        for id in [
            "gui",
            "mcp",
            "dev_1",
            "../gui",
            "00000000-0000-0000-0000-000000000000",
            "550E8400-E29B-41D4-A716-446655440000",
        ] {
            assert!(account(id).is_none());
        }
        assert_eq!(
            account("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            "m9-probe-550e8400-e29b-41d4-a716-446655440000"
        );
        let fake = Fake::default();
        *fake.value.lock().unwrap() = Some(b"unrelated".to_vec());
        for action in [Action::Create, Action::Update, Action::Delete] {
            assert!(execute(&fake, "fixture", action).is_err());
        }
        assert!(fake.writes.lock().unwrap().is_empty());
    }
    #[test]
    fn synthetic_lifecycle_and_repeat_cleanup_do_not_recreate_or_overwrite() {
        let fake = Fake::default();
        for action in [
            Action::Create,
            Action::Read,
            Action::Update,
            Action::Read,
            Action::Delete,
            Action::Delete,
        ] {
            execute(&fake, "fixture", action).unwrap();
        }
        assert_eq!(
            *fake.writes.lock().unwrap(),
            vec!["create", "update", "delete"]
        );
        assert_eq!(
            execute(&fake, "fixture", Action::Read),
            Err(Failure::Missing)
        );
    }
    #[test]
    fn locked_denied_and_interaction_required_never_create_or_repair_items() {
        let fake = Fake::default();
        for error in [
            CredentialError::Locked,
            CredentialError::Denied,
            CredentialError::InteractionRequired,
            CredentialError::Unavailable,
        ] {
            *fake.blocked.lock().unwrap() = Some(error);
            for action in [Action::Create, Action::Read, Action::Update, Action::Delete] {
                assert_eq!(
                    execute(&fake, "fixture", action),
                    Err(Failure::Store(error))
                );
            }
        }
        assert!(fake.writes.lock().unwrap().is_empty());
    }
}
