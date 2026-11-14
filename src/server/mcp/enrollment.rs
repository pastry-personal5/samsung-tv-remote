//! Durable opt-in is separate from API Keychain availability. A fresh grant
//! derives a new bearer without replacing or exposing the underlying API item.
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use crate::infrastructure::private_files::{Namespace, OwnershipLock, RuntimeDirectory};
use crate::server::{
    access::{AccessGate, AuthorizedClient},
    bootstrap::{ApiCredentials, BootstrapError, Role},
    control_http::wire::Counter,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrollmentError {
    Denied,
    StaleGeneration,
    KeyConflict,
    UnsafeMetadata,
    Persistence,
    Credential(BootstrapError),
    RevocationPending,
    Exhausted,
}
impl EnrollmentError {
    /// One stable code for the live route, offline CLI and remote decoding.
    pub fn code(self) -> &'static str {
        match self {
            Self::Denied => "forbidden",
            Self::StaleGeneration => "stale_enrollment",
            Self::KeyConflict => "operation_key_conflict",
            Self::UnsafeMetadata => "unsafe_enrollment",
            Self::Persistence => "enrollment_persistence_failed",
            Self::Credential(_) => "mcp_credentials_blocked",
            Self::RevocationPending => "mcp_revocation_pending",
            Self::Exhausted => "enrollment_exhausted",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    version: u8,
    profile: String,
    pub generation: Counter,
    pub enabled: bool,
    nonce: Option<String>,
    operation_key: Option<String>,
}
impl Record {
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"enabled":self.enabled,"generation":self.generation,"operation_key":self.operation_key})
    }
    fn disabled(namespace: Namespace) -> Self {
        Self {
            version: 1,
            profile: namespace.name().into(),
            generation: Counter(0),
            enabled: false,
            nonce: None,
            operation_key: None,
        }
    }
    fn validate(&self, namespace: Namespace) -> Result<(), EnrollmentError> {
        if self.version != 1
            || self.profile != namespace.name()
            || self.enabled != self.nonce.is_some()
            || (self.generation.0 == 0 && (self.enabled || self.operation_key.is_some()))
            || (self.generation.0 > 0 && self.operation_key.is_none())
            || self.nonce.as_deref().is_some_and(|v| !canonical_uuid(v))
            || self
                .operation_key
                .as_deref()
                .is_some_and(|v| !canonical_uuid(v))
        {
            return Err(EnrollmentError::UnsafeMetadata);
        }
        Ok(())
    }
}
fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

pub trait EnrollmentStore: Send + Sync {
    fn read(&self) -> Result<Record, EnrollmentError>;
    fn write(&self, record: &Record) -> Result<(), EnrollmentError>;
}
pub struct FileEnrollment {
    directory: Arc<RuntimeDirectory>,
    namespace: Namespace,
}
impl FileEnrollment {
    pub fn new(directory: Arc<RuntimeDirectory>, namespace: Namespace) -> Self {
        Self {
            directory,
            namespace,
        }
    }
}
impl EnrollmentStore for FileEnrollment {
    fn read(&self) -> Result<Record, EnrollmentError> {
        let expected = format!("{}:mcp-enrollment:v1", self.namespace.name());
        let marker = self
            .directory
            .read("mcp-enrollment.present", 128)
            .map_err(|_| EnrollmentError::UnsafeMetadata)?;
        if marker
            .as_ref()
            .is_some_and(|value| value != expected.as_bytes())
        {
            return Err(EnrollmentError::UnsafeMetadata);
        }
        let bytes = self
            .directory
            .read("mcp-enrollment.json", 2048)
            .map_err(|_| EnrollmentError::UnsafeMetadata)?;
        let record = match bytes {
            Some(bytes) => serde_json::from_slice::<Record>(&bytes)
                .map_err(|_| EnrollmentError::UnsafeMetadata)?,
            None if marker.is_none() => Record::disabled(self.namespace),
            None => return Err(EnrollmentError::UnsafeMetadata),
        };
        record.validate(self.namespace)?;
        Ok(record)
    }
    fn write(&self, record: &Record) -> Result<(), EnrollmentError> {
        record.validate(self.namespace)?;
        self.directory
            .replace(
                "mcp-enrollment.json",
                &serde_json::to_vec(record).map_err(|_| EnrollmentError::Persistence)?,
            )
            .map_err(|_| EnrollmentError::Persistence)?;
        // Missing state after an established enrollment must fail closed. An
        // interrupted first write with a readable record can be adopted safely.
        self.directory
            .replace(
                "mcp-enrollment.present",
                format!("{}:mcp-enrollment:v1", self.namespace.name()).as_bytes(),
            )
            .map_err(|_| EnrollmentError::Persistence)
    }
}

pub struct Enrollment {
    store: Arc<dyn EnrollmentStore>,
    credentials: Arc<dyn ApiCredentials>,
    access: Option<Arc<AccessGate>>,
    serial: Mutex<()>,
    fenced: AtomicBool,
    _ownership: Option<OwnershipLock>,
}
impl Enrollment {
    pub fn online(
        store: Arc<dyn EnrollmentStore>,
        credentials: Arc<dyn ApiCredentials>,
        access: Arc<AccessGate>,
    ) -> Self {
        Self {
            store,
            credentials,
            access: Some(access),
            serial: Mutex::new(()),
            fenced: AtomicBool::new(false),
            _ownership: None,
        }
    }
    pub fn offline(store: Arc<dyn EnrollmentStore>, credentials: Arc<dyn ApiCredentials>) -> Self {
        Self {
            store,
            credentials,
            access: None,
            serial: Mutex::new(()),
            fenced: AtomicBool::new(false),
            _ownership: None,
        }
    }
    pub fn retain_ownership(mut self, ownership: &OwnershipLock) -> Result<Self, EnrollmentError> {
        self._ownership = Some(
            ownership
                .try_clone()
                .map_err(|_| EnrollmentError::UnsafeMetadata)?,
        );
        Ok(self)
    }
    pub fn status(&self) -> Result<Record, EnrollmentError> {
        let _guard = self
            .serial
            .lock()
            .map_err(|_| EnrollmentError::UnsafeMetadata)?;
        self.store.read()
    }
    pub fn status_summary(&self) -> Result<serde_json::Value, EnrollmentError> {
        let _guard = self
            .serial
            .lock()
            .map_err(|_| EnrollmentError::UnsafeMetadata)?;
        let record = self.store.read()?;
        let mut value = record.summary();
        value["admission_active"] = serde_json::json!(self
            .access
            .as_ref()
            .map(|gate| gate.identity_active(Role::Mcp.identity())));
        value["revocation_pending"] =
            serde_json::json!(record.enabled && self.fenced.load(Ordering::Acquire));
        Ok(value)
    }
    pub fn change_authorized(
        &self,
        client: &AuthorizedClient,
        expected: u64,
        key: &str,
        enabled: bool,
    ) -> Result<Record, EnrollmentError> {
        client
            .require_management()
            .map_err(|_| EnrollmentError::Denied)?;
        if self.access.is_none() {
            return Err(EnrollmentError::Denied);
        }
        client
            .admission
            .at_admission(|| self.change(expected, key, enabled))
            .ok_or(EnrollmentError::Denied)?
    }
    /// Caller resolved the trusted profile and holds its lifetime ownership
    /// lock; this path never starts a runtime or clears an intentional Stop.
    pub fn change_offline(
        &self,
        _ownership: &OwnershipLock,
        expected: u64,
        key: &str,
        enabled: bool,
    ) -> Result<Record, EnrollmentError> {
        if self.access.is_some() {
            return Err(EnrollmentError::Denied);
        }
        self.change(expected, key, enabled)
    }
    fn bearer(&self, record: &Record) -> Result<[u8; 32], BootstrapError> {
        if !record.enabled {
            return Err(BootstrapError::Denied);
        }
        let base = self.credentials.load(Role::Mcp)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&base).expect("fixed API secret");
        mac.update(b"samsung-tv-remote/mcp-enrollment/v1\0");
        mac.update(record.profile.as_bytes());
        mac.update(&record.generation.0.to_be_bytes());
        mac.update(
            record
                .nonce
                .as_deref()
                .ok_or(BootstrapError::UnsafeMetadata)?
                .as_bytes(),
        );
        Ok(mac.finalize().into_bytes().into())
    }
    fn change(&self, expected: u64, key: &str, enabled: bool) -> Result<Record, EnrollmentError> {
        if !canonical_uuid(key) {
            return Err(EnrollmentError::UnsafeMetadata);
        }
        let _guard = self
            .serial
            .lock()
            .map_err(|_| EnrollmentError::UnsafeMetadata)?;
        let previous = self.store.read()?;
        if previous.operation_key.as_deref() == Some(key) {
            if previous.enabled != enabled || previous.generation.0.checked_sub(1) != Some(expected)
            {
                return Err(EnrollmentError::KeyConflict);
            }
            if enabled
                && self
                    .access
                    .as_ref()
                    .is_some_and(|gate| !gate.identity_active(Role::Mcp.identity()))
            {
                return Err(EnrollmentError::RevocationPending);
            }
            // Reassert a potentially interrupted durability marker before ack.
            self.store.write(&previous)?;
            return Ok(previous);
        }
        if previous.generation.0 != expected {
            return Err(EnrollmentError::StaleGeneration);
        }
        if previous.enabled && enabled {
            let active = self
                .access
                .as_ref()
                .is_none_or(|gate| gate.identity_active(Role::Mcp.identity()));
            if active {
                return Ok(previous);
            }
            // An explicit new Connect can repair a failed live revocation, but
            // must never resurrect that previous generation's bearer.
        }
        let next = Record {
            version: 1,
            profile: previous.profile,
            generation: Counter(
                previous
                    .generation
                    .0
                    .checked_add(1)
                    .ok_or(EnrollmentError::Exhausted)?,
            ),
            enabled,
            nonce: enabled.then(|| uuid::Uuid::new_v4().to_string()),
            operation_key: Some(key.into()),
        };
        let bearer = if enabled {
            Some(self.bearer(&next).map_err(EnrollmentError::Credential)?)
        } else {
            None
        };
        // Fence live grants/admissions before persistence. A failed write stays
        // fenced in this owner and never reports durable revocation.
        self.fenced.store(true, Ordering::Release);
        if let Some(access) = &self.access {
            access.revoke(Role::Mcp.identity());
        }
        self.store.write(&next)?;
        if let (Some(access), Some(bearer)) = (&self.access, bearer) {
            access
                .rotate(Role::Mcp.identity(), Role::Mcp.capability(), &bearer)
                .map_err(|_| EnrollmentError::RevocationPending)?;
        }
        self.fenced.store(!enabled, Ordering::Release);
        Ok(next)
    }
}
impl ApiCredentials for Enrollment {
    fn load(&self, role: Role) -> Result<[u8; 32], BootstrapError> {
        if role == Role::Gui {
            return self.credentials.load(role);
        }
        let _guard = self
            .serial
            .lock()
            .map_err(|_| BootstrapError::UnsafeMetadata)?;
        if self.fenced.load(Ordering::Acquire) {
            return Err(BootstrapError::Denied);
        }
        let record = self
            .store
            .read()
            .map_err(|_| BootstrapError::UnsafeMetadata)?;
        self.bearer(&record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::access::Capability;

    struct Credentials;
    impl ApiCredentials for Credentials {
        fn load(&self, role: Role) -> Result<[u8; 32], BootstrapError> {
            Ok(match role {
                Role::Gui => [1; 32],
                Role::Mcp => [2; 32],
            })
        }
    }
    struct MemoryStore {
        state: Mutex<Record>,
        fail: AtomicBool,
    }
    impl MemoryStore {
        fn new() -> Self {
            Self {
                state: Mutex::new(Record::disabled(Namespace::Development)),
                fail: AtomicBool::new(false),
            }
        }
    }
    impl EnrollmentStore for MemoryStore {
        fn read(&self) -> Result<Record, EnrollmentError> {
            Ok(self.state.lock().unwrap().clone())
        }
        fn write(&self, record: &Record) -> Result<(), EnrollmentError> {
            if self.fail.load(Ordering::Acquire) {
                return Err(EnrollmentError::Persistence);
            }
            *self.state.lock().unwrap() = record.clone();
            Ok(())
        }
    }
    fn gate() -> Arc<AccessGate> {
        let gate = Arc::new(AccessGate::new(54321));
        gate.rotate(Role::Gui.identity(), Capability::Manage, &[1; 32])
            .unwrap();
        gate
    }
    fn client(gate: &AccessGate, secret: &[u8; 32]) -> AuthorizedClient {
        let mut headers = hyper::http::HeaderMap::new();
        headers.insert("host", "127.0.0.1:54321".parse().unwrap());
        headers.insert(
            "authorization",
            format!("Bearer {}", crate::server::bootstrap::hex(secret))
                .parse()
                .unwrap(),
        );
        gate.authorize(
            "127.0.0.1:1".parse().unwrap(),
            &"/v1/mcp".parse().unwrap(),
            &headers,
        )
        .unwrap()
    }
    fn key() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    #[test]
    fn explicit_enrollment_reuses_credentials_and_regrant_never_restores_an_old_bearer() {
        let store = Arc::new(MemoryStore::new());
        let access = gate();
        let enrollment = Enrollment::online(store.clone(), Arc::new(Credentials), access.clone());
        let gui = client(&access, &[1; 32]);
        assert_eq!(enrollment.load(Role::Mcp), Err(BootstrapError::Denied));
        assert_eq!(enrollment.load(Role::Gui).unwrap(), [1; 32]);
        let first_key = key();
        let enabled = enrollment
            .change_authorized(&gui, 0, &first_key, true)
            .unwrap();
        assert_eq!(enabled.generation, Counter(1));
        let first = enrollment.load(Role::Mcp).unwrap();
        assert_ne!(first, [2; 32]);
        let mcp = client(&access, &first);
        assert_eq!(mcp.identity, Role::Mcp.identity());
        assert_eq!(
            enrollment.change_authorized(&mcp, 1, &key(), false),
            Err(EnrollmentError::Denied)
        );
        assert_eq!(
            enrollment
                .change_authorized(&gui, 0, &first_key, true)
                .unwrap(),
            enabled
        );
        assert_eq!(
            enrollment.change_authorized(&gui, 1, &key(), true).unwrap(),
            enabled
        );
        assert_eq!(enrollment.load(Role::Mcp).unwrap(), first);
        let disabled = enrollment
            .change_authorized(&gui, 1, &key(), false)
            .unwrap();
        assert_eq!(disabled.generation, Counter(2));
        assert!(!mcp.admission.is_active());
        assert_eq!(enrollment.load(Role::Mcp), Err(BootstrapError::Denied));
        assert_eq!(
            enrollment.change_authorized(&gui, 0, &first_key, true),
            Err(EnrollmentError::StaleGeneration)
        );
        let enabled = enrollment.change_authorized(&gui, 2, &key(), true).unwrap();
        let next = enrollment.load(Role::Mcp).unwrap();
        assert_ne!(first, next);
        assert_eq!(enabled.generation, Counter(3));
        assert!(access.is_current(Role::Mcp.identity(), &next));
        assert!(!access.is_current(Role::Mcp.identity(), &first));
        let restart = Enrollment::online(store, Arc::new(Credentials), gate());
        assert_eq!(restart.load(Role::Mcp).unwrap(), next);
    }

    #[test]
    fn failed_revocation_remains_fenced_even_if_bootstrap_refresh_can_read_the_old_record() {
        let store = Arc::new(MemoryStore::new());
        let access = gate();
        let enrollment = Arc::new(Enrollment::online(
            store.clone(),
            Arc::new(Credentials),
            access.clone(),
        ));
        let gui = client(&access, &[1; 32]);
        enrollment.change_authorized(&gui, 0, &key(), true).unwrap();
        let old = enrollment.load(Role::Mcp).unwrap();
        let bootstrap = crate::server::bootstrap::Bootstrap::new(
            access.clone(),
            enrollment.clone(),
            Arc::new(crate::server::bootstrap::DevelopmentPeer {
                uid: crate::server::bootstrap::socket::uid(),
            }),
            crate::server::bootstrap::Context {
                profile: "test".into(),
                endpoint: "http://127.0.0.1:54321".into(),
                build: "test".into(),
                api: "v1".into(),
                epoch: uuid::Uuid::new_v4().to_string(),
            },
        )
        .unwrap();
        store.fail.store(true, Ordering::Release);
        assert_eq!(
            enrollment.change_authorized(&gui, 1, &key(), false),
            Err(EnrollmentError::Persistence)
        );
        assert_eq!(enrollment.load(Role::Mcp), Err(BootstrapError::Denied));
        assert_eq!(bootstrap.refresh(Role::Mcp), Err(BootstrapError::Denied));
        assert!(!access.identity_active(Role::Mcp.identity()));
        assert!(gui.admission.is_active());
        store.fail.store(false, Ordering::Release);
        enrollment.change_authorized(&gui, 1, &key(), true).unwrap();
        let next = enrollment.load(Role::Mcp).unwrap();
        assert_ne!(next, old);
        assert_eq!(bootstrap.refresh(Role::Mcp), Ok(()));
        assert!(!access.is_current(Role::Mcp.identity(), &old));
    }

    #[test]
    fn retained_keys_bind_the_original_expected_generation_for_grants_and_revocations() {
        let store = Arc::new(MemoryStore::new());
        let access = gate();
        let enrollment = Enrollment::online(store, Arc::new(Credentials), access.clone());
        let gui = client(&access, &[1; 32]);
        for (expected, enabled) in [(0, true), (1, false)] {
            let operation_key = key();
            let record = enrollment
                .change_authorized(&gui, expected, &operation_key, enabled)
                .unwrap();
            for changed in [expected + 1, u64::MAX] {
                assert_eq!(
                    enrollment.change_authorized(&gui, changed, &operation_key, enabled),
                    Err(EnrollmentError::KeyConflict)
                );
            }
            assert_eq!(enrollment.status().unwrap(), record);
            assert_eq!(
                enrollment.change_authorized(&gui, expected, &operation_key, enabled),
                Ok(record)
            );
        }
    }

    #[test]
    fn stale_grants_and_changed_payload_keys_fail_without_replacing_the_grant() {
        let store = Arc::new(MemoryStore::new());
        let access = gate();
        let enrollment = Arc::new(Enrollment::online(
            store,
            Arc::new(Credentials),
            access.clone(),
        ));
        let gui = client(&access, &[1; 32]);
        let operation_key = key();
        enrollment
            .change_authorized(&gui, 0, &operation_key, true)
            .unwrap();
        let first = enrollment.load(Role::Mcp).unwrap();
        assert_eq!(
            enrollment.change_authorized(&gui, 1, &operation_key, false),
            Err(EnrollmentError::KeyConflict)
        );
        assert_eq!(
            enrollment.change_authorized(&gui, 0, &key(), true),
            Err(EnrollmentError::StaleGeneration)
        );
        assert_eq!(enrollment.load(Role::Mcp).unwrap(), first);
    }

    #[test]
    fn explicit_disconnect_while_already_disabled_fences_a_previously_prepared_grant() {
        let access = gate();
        let enrollment = Enrollment::online(
            Arc::new(MemoryStore::new()),
            Arc::new(Credentials),
            access.clone(),
        );
        let gui = client(&access, &[1; 32]);
        let pending_grant_key = key();
        let disabled = enrollment
            .change_authorized(&gui, 0, &key(), false)
            .unwrap();
        assert!(!disabled.enabled);
        assert_eq!(disabled.generation, Counter(1));
        assert_eq!(
            enrollment.change_authorized(&gui, 0, &pending_grant_key, true),
            Err(EnrollmentError::StaleGeneration)
        );
        assert_eq!(enrollment.load(Role::Mcp), Err(BootstrapError::Denied));
        enrollment.change_authorized(&gui, 1, &key(), true).unwrap();
        assert!(enrollment.load(Role::Mcp).is_ok());
    }

    #[test]
    fn stopped_profile_management_is_durable_and_missing_or_wrong_namespace_state_fails_closed() {
        let base = tempfile::tempdir().unwrap();
        let directory = Arc::new(
            RuntimeDirectory::open(&base.path().canonicalize().unwrap().join("profile")).unwrap(),
        );
        let owner = directory.lock().unwrap();
        let store = Arc::new(FileEnrollment::new(
            directory.clone(),
            Namespace::Development,
        ));
        let enrollment = Enrollment::offline(store.clone(), Arc::new(Credentials));
        assert!(!enrollment.status().unwrap().enabled);
        let enabled = enrollment.change_offline(&owner, 0, &key(), true).unwrap();
        let secret = enrollment.load(Role::Mcp).unwrap();
        let persisted = directory
            .read("mcp-enrollment.json", 2048)
            .unwrap()
            .unwrap();
        assert!(!String::from_utf8(persisted)
            .unwrap()
            .contains(&crate::server::bootstrap::hex(&secret)));
        assert_eq!(store.read().unwrap(), enabled);
        assert!(FileEnrollment::new(directory.clone(), Namespace::Control)
            .read()
            .is_err());
        enrollment.change_offline(&owner, 1, &key(), false).unwrap();
        let restart = Enrollment::offline(store.clone(), Arc::new(Credentials));
        assert_eq!(restart.load(Role::Mcp), Err(BootstrapError::Denied));
        directory.remove("mcp-enrollment.json").unwrap();
        assert_eq!(store.read(), Err(EnrollmentError::UnsafeMetadata));
        assert_eq!(restart.load(Role::Mcp), Err(BootstrapError::UnsafeMetadata));
    }

    #[test]
    fn admitted_management_retains_profile_ownership_after_composition_guard_drops() {
        let base = tempfile::tempdir().unwrap();
        let directory = Arc::new(
            RuntimeDirectory::open(&base.path().canonicalize().unwrap().join("profile")).unwrap(),
        );
        let owner = directory.lock().unwrap();
        let enrollment = Enrollment::online(
            Arc::new(FileEnrollment::new(
                directory.clone(),
                Namespace::Development,
            )),
            Arc::new(Credentials),
            gate(),
        )
        .retain_ownership(&owner)
        .unwrap();
        drop(owner);
        assert!(matches!(
            directory.lock(),
            Err(crate::infrastructure::private_files::FileError::Locked)
        ));
        drop(enrollment);
        let released = directory.lock();
        assert!(
            released.is_ok(),
            "ownership after final guard: {:?}",
            released.err()
        );
    }
}
