use samsung_tv_remote::server::lifecycle::files::{FileError, RuntimeDirectory};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};

#[test]
fn concurrent_first_lock_creation_and_atomic_reads_keep_one_owner() {
    let base = tempfile::tempdir().unwrap();
    let base = base.path().canonicalize().unwrap();
    for round in 0..16 {
        let path = base.join(format!("runtime-{round}"));
        let barrier = std::sync::Barrier::new(8);
        let owners = std::thread::scope(|scope| {
            let tasks: Vec<_> = (0..8)
                .map(|_| {
                    let path = &path;
                    let barrier = &barrier;
                    scope.spawn(move || {
                        let directory = RuntimeDirectory::open(path).unwrap();
                        barrier.wait();
                        let lock = match directory.lock() {
                            Ok(lock) => Some(lock),
                            Err(FileError::Locked) => None,
                            Err(error) => panic!("concurrent creation: {error:?}"),
                        };
                        // Keep the winning descriptor held until every
                        // contender has completed its nonblocking attempt.
                        barrier.wait();
                        lock.is_some()
                    })
                })
                .collect();
            tasks
                .into_iter()
                .map(|t| t.join().unwrap())
                .filter(|owned| *owned)
                .count()
        });
        assert_eq!(owners, 1);
    }
    let directory = RuntimeDirectory::open(&base.join("atomic")).unwrap();
    directory.replace("state", b"old").unwrap();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for _ in 0..128 {
                directory.replace("state", b"new").unwrap();
            }
        });
        for _ in 0..512 {
            let bytes = directory.read("state", 16).unwrap().unwrap();
            assert!(bytes == b"old" || bytes == b"new");
        }
    });
}

#[test]
fn runtime_ownership_and_atomic_metadata_survive_reopen() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().canonicalize().unwrap().join("runtime");
    let directory = RuntimeDirectory::open(&path).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let owner = directory.lock().unwrap();
    let contender = RuntimeDirectory::open(&path).unwrap();
    assert!(matches!(contender.lock(), Err(FileError::Locked)));
    directory
        .replace("manifest.json", br#"{"port":54321}"#)
        .unwrap();
    assert_eq!(
        fs::metadata(path.join("manifest.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        contender.read("manifest.json", 1024).unwrap().unwrap(),
        br#"{"port":54321}"#
    );
    directory
        .replace("manifest.json", br#"{"port":54322}"#)
        .unwrap();
    assert_eq!(
        contender.read("manifest.json", 1024).unwrap().unwrap(),
        br#"{"port":54322}"#
    );
    assert_eq!(contender.read("manifest.json", 3), Err(FileError::TooLarge));
    drop(owner);
    let _replacement = contender.lock().unwrap();
    contender.remove("manifest.json").unwrap();
    assert_eq!(directory.read("manifest.json", 1024).unwrap(), None);
}

#[test]
fn unsafe_paths_files_and_locks_are_rejected_without_repair() {
    let base = tempfile::tempdir().unwrap();
    let base = base.path().canonicalize().unwrap();
    let path = base.join("runtime");
    let directory = RuntimeDirectory::open(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        RuntimeDirectory::open(&path),
        Err(FileError::UnsafePath)
    ));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&path, base.join("linked")).unwrap();
    assert!(matches!(
        RuntimeDirectory::open(&base.join("linked")),
        Err(FileError::UnsafePath)
    ));
    assert!(matches!(
        RuntimeDirectory::open(&base.join("linked/child")),
        Err(FileError::UnsafePath)
    ));
    assert!(!path.join("child").exists());
    assert!(matches!(
        RuntimeDirectory::open(std::path::Path::new("relative")),
        Err(FileError::UnsafePath)
    ));
    let outside = base.join("outside");
    fs::write(&outside, "leave alone").unwrap();
    for name in ["manifest.json", "owner.lock"] {
        symlink(&outside, path.join(name)).unwrap();
        assert!(directory.read(name, 1024).is_err());
        assert!(directory.replace(name, b"replacement").is_err());
    }
    assert!(matches!(directory.lock(), Err(FileError::UnsafePath)));
    assert_eq!(fs::read_to_string(&outside).unwrap(), "leave alone");
    fs::remove_file(path.join("manifest.json")).unwrap();
    fs::write(path.join("manifest.json"), "permissive").unwrap();
    fs::set_permissions(
        path.join("manifest.json"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert_eq!(
        directory.read("manifest.json", 1024),
        Err(FileError::UnsafePath)
    );
    assert_eq!(
        directory.replace("manifest.json", b"replacement"),
        Err(FileError::UnsafePath)
    );
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
    fs::remove_file(path.join("manifest.json")).unwrap();
    fs::hard_link(&outside, path.join("manifest.json")).unwrap();
    assert_eq!(
        directory.read("manifest.json", 1024),
        Err(FileError::UnsafePath)
    );
    assert_eq!(
        directory.read("../outside", 1024),
        Err(FileError::UnsafePath)
    );
}

use samsung_tv_remote::application::control_service::*;
use samsung_tv_remote::application::runtime::{Runtime, RuntimeLimits};
use samsung_tv_remote::application::tv_control_coordinator::AppServices;
use samsung_tv_remote::server::access::*;
use samsung_tv_remote::server::control_http::transport::StopPort;
use samsung_tv_remote::server::lifecycle::stop::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Condvar, Mutex,
};

struct MemoryStop {
    marker: Mutex<Option<StopMarker>>,
    gate: Mutex<bool>,
    wake: Condvar,
    entered: tokio::sync::Notify,
    fail: AtomicBool,
    writes: AtomicUsize,
}
impl MemoryStop {
    fn new(fail: bool) -> Self {
        Self {
            marker: Mutex::new(None),
            gate: Mutex::new(false),
            wake: Condvar::new(),
            entered: tokio::sync::Notify::new(),
            fail: AtomicBool::new(fail),
            writes: AtomicUsize::new(0),
        }
    }
    fn release(&self) {
        *self.gate.lock().unwrap() = true;
        self.wake.notify_all();
    }
}
impl StopStore for MemoryStop {
    fn read(&self) -> Result<Option<StopMarker>, FileError> {
        Ok(self.marker.lock().unwrap().clone())
    }
    fn write(&self, marker: &StopMarker) -> Result<(), FileError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        let mut ready = self.gate.lock().unwrap();
        while !*ready {
            ready = self.wake.wait(ready).unwrap();
        }
        if self.fail.swap(false, Ordering::SeqCst) {
            return Err(FileError::Unavailable);
        }
        *self.marker.lock().unwrap() = Some(marker.clone());
        Ok(())
    }
    fn clear(&self) -> Result<(), FileError> {
        self.marker.lock().unwrap().take();
        Ok(())
    }
}
fn authorization() -> (Arc<AccessGate>, AuthorizedClient, AuthorizedClient) {
    let gate = Arc::new(AccessGate::new(54321));
    gate.rotate(ClientId(1), Capability::Manage, &[1; 32])
        .unwrap();
    gate.rotate(ClientId(2), Capability::Control, &[2; 32])
        .unwrap();
    let make = |n: u8| {
        let mut headers = hyper::http::HeaderMap::new();
        headers.insert("host", "127.0.0.1:54321".parse().unwrap());
        headers.insert(
            "authorization",
            format!("Bearer {}", format!("{n:02x}").repeat(32))
                .parse()
                .unwrap(),
        );
        gate.authorize(
            "127.0.0.1:1".parse().unwrap(),
            &"/v1/server/stop".parse().unwrap(),
            &headers,
        )
        .unwrap()
    };
    let (gui, mcp) = (make(1), make(2));
    (gate, gui, mcp)
}
fn runtime() -> Runtime {
    Runtime::start(
        AppServices {
            setup: None,
            discovery: None,
            gateway: None,
            wake: None,
        },
        RuntimeLimits::default(),
    )
}

#[tokio::test]
async fn stop_fences_before_persistence_retains_failed_key_and_requires_new_intent() {
    let base = tempfile::tempdir().unwrap();
    let directory = Arc::new(
        RuntimeDirectory::open(&base.path().canonicalize().unwrap().join("runtime")).unwrap(),
    );
    let owner = directory.lock().unwrap();
    let runtime = runtime();
    let service = runtime.client(ClientId(1));
    let epoch = service.snapshot().await.unwrap().epoch;
    let before = Request {
        epoch,
        key: "retained".into(),
        command: Command::Discover,
    };
    let admitted = service.submit(before.clone()).await.unwrap();
    let store = Arc::new(MemoryStop::new(true));
    let lifecycle = Lifecycle::start(runtime, store.clone(), owner)
        .await
        .unwrap();
    let (_gate, gui, mcp) = authorization();
    let key = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        lifecycle
            .client
            .stop(mcp, epoch, key.clone())
            .await
            .unwrap_err()
            .code,
        "forbidden"
    );
    assert!(!service.snapshot().await.unwrap().admission_closed);
    let stop = lifecycle.client.clone();
    let first_key = key.clone();
    let first_gui = gui.clone();
    let pending = tokio::spawn(async move { stop.stop(first_gui, epoch, first_key).await });
    store.entered.notified().await;
    assert!(service.snapshot().await.unwrap().admission_closed);
    assert!(matches!(
        service
            .submit(Request {
                epoch,
                key: "after-fence".into(),
                command: Command::Discover
            })
            .await,
        Err(ServiceError::Closed)
    ));
    assert_eq!(service.submit(before).await.unwrap().id, admitted.id);
    assert!(matches!(directory.lock(), Err(FileError::Locked)));
    assert!(store.read().unwrap().is_none());
    store.release();
    assert_eq!(
        pending.await.unwrap().unwrap_err().code,
        "stop_persistence_failed"
    );
    assert_eq!(
        lifecycle
            .client
            .stop(gui.clone(), epoch, key)
            .await
            .unwrap_err()
            .code,
        "stop_persistence_failed"
    );
    assert_eq!(store.writes.load(Ordering::SeqCst), 1);
    assert_eq!(lifecycle.client.serving_state(), "stop_blocked");
    assert!(matches!(directory.lock(), Err(FileError::Locked)));
    let key = uuid::Uuid::new_v4().to_string();
    let result = lifecycle
        .client
        .stop(gui.clone(), epoch, key.clone())
        .await
        .unwrap();
    assert_eq!(result["durable_stop"], true);
    assert_eq!(store.read().unwrap().unwrap().operation_key, key);
    assert_eq!(
        lifecycle.client.stop(gui, epoch, key).await.unwrap(),
        result
    );
    assert_eq!(store.writes.load(Ordering::SeqCst), 2);
    lifecycle.settled().await.unwrap();
    let _replacement = directory.lock().unwrap();
}

#[tokio::test]
async fn lost_stop_reply_still_leaves_durable_marker_and_prevents_ordinary_start() {
    let base = tempfile::tempdir().unwrap();
    let directory = Arc::new(
        RuntimeDirectory::open(&base.path().canonicalize().unwrap().join("runtime")).unwrap(),
    );
    let runtime = runtime();
    let epoch = runtime.client(ClientId(1)).snapshot().await.unwrap().epoch;
    let store = Arc::new(MemoryStop::new(false));
    let lifecycle = Lifecycle::start(runtime, store.clone(), directory.lock().unwrap())
        .await
        .unwrap();
    let (_gate, gui, _) = authorization();
    let key = uuid::Uuid::new_v4().to_string();
    let expected = key.clone();
    let stop = lifecycle.client.clone();
    let pending = tokio::spawn(async move { stop.stop(gui, epoch, key).await });
    store.entered.notified().await;
    pending.abort();
    store.release();
    lifecycle.settled().await.unwrap();
    assert_eq!(store.read().unwrap().unwrap().operation_key, expected);
    let result = Lifecycle::start(self::runtime(), store, directory.lock().unwrap()).await;
    assert!(matches!(result,Err(error) if error.code=="server_stopped"));
}

#[test]
fn durable_stop_metadata_is_strict_and_never_contains_credentials() {
    let base = tempfile::tempdir().unwrap();
    let directory = Arc::new(
        RuntimeDirectory::open(&base.path().canonicalize().unwrap().join("runtime")).unwrap(),
    );
    let store = FileStopStore(directory.clone());
    let marker = StopMarker {
        version: 1,
        profile: "m8-fake-v1".into(),
        server_epoch: uuid::Uuid::new_v4().to_string(),
        operation_key: uuid::Uuid::new_v4().to_string(),
        identity: "gui".into(),
    };
    store.write(&marker).unwrap();
    assert_eq!(store.read().unwrap(), Some(marker));
    directory
        .replace("stopped.json", br#"{"version":1,"profile":"production"}"#)
        .unwrap();
    assert_eq!(store.read(), Err(FileError::UnsafePath));
    assert_eq!(store.clear(), Err(FileError::UnsafePath));
    assert!(directory.read("stopped.json", 4096).unwrap().is_some());
}

struct BlockingDevices {
    gate: Arc<(Mutex<bool>, Condvar, tokio::sync::Notify)>,
}
impl samsung_tv_remote::application::device_repository::DeviceRepository for BlockingDevices {
    fn list(
        &self,
    ) -> Result<
        Vec<samsung_tv_remote::application::device_repository::SavedDevice>,
        samsung_tv_remote::application::device_repository::RepositoryError,
    > {
        self.gate.2.notify_one();
        let mut ready = self.gate.0.lock().unwrap();
        while !*ready {
            ready = self.gate.1.wait(ready).unwrap();
        }
        Ok(Vec::new())
    }
    fn save(
        &self,
        _device: &samsung_tv_remote::application::device_repository::SavedDevice,
    ) -> Result<(), samsung_tv_remote::application::device_repository::RepositoryError> {
        unreachable!()
    }
    fn delete(
        &self,
        _id: samsung_tv_remote::DeviceId,
    ) -> Result<(), samsung_tv_remote::application::device_repository::RepositoryError> {
        unreachable!()
    }
    fn selected(
        &self,
    ) -> Result<
        Option<samsung_tv_remote::DeviceId>,
        samsung_tv_remote::application::device_repository::RepositoryError,
    > {
        Ok(None)
    }
    fn select(
        &self,
        _id: Option<samsung_tv_remote::DeviceId>,
    ) -> Result<(), samsung_tv_remote::application::device_repository::RepositoryError> {
        unreachable!()
    }
}
struct NoSecrets;
impl samsung_tv_remote::application::credential_store::CredentialStore for NoSecrets {
    fn load(
        &self,
        _: samsung_tv_remote::DeviceId,
    ) -> Result<
        Option<samsung_tv_remote::application::credential_store::PairingToken>,
        samsung_tv_remote::application::credential_store::SecretError,
    > {
        panic!("startup opened credentials")
    }
    fn save(
        &self,
        _: samsung_tv_remote::DeviceId,
        _: &samsung_tv_remote::application::credential_store::PairingToken,
    ) -> Result<(), samsung_tv_remote::application::credential_store::SecretError> {
        unreachable!()
    }
    fn delete(
        &self,
        _: samsung_tv_remote::DeviceId,
    ) -> Result<(), samsung_tv_remote::application::credential_store::SecretError> {
        unreachable!()
    }
}

#[tokio::test]
async fn signal_drain_keeps_profile_lock_until_late_blocking_restore_settles() {
    use samsung_tv_remote::application::tv_setup_service::TvSetupService;
    use samsung_tv_remote::infrastructure::preferences::{
        LocalCertificateTrustStore, LocalRecoveryJournal,
    };
    let base = tempfile::tempdir().unwrap();
    let directory = Arc::new(
        RuntimeDirectory::open(&base.path().canonicalize().unwrap().join("runtime")).unwrap(),
    );
    let gate = Arc::new((
        Mutex::new(false),
        Condvar::new(),
        tokio::sync::Notify::new(),
    ));
    let setup = Arc::new(
        TvSetupService::new(
            BlockingDevices { gate: gate.clone() },
            NoSecrets,
            LocalCertificateTrustStore::new(base.path()),
        )
        .with_journal(Arc::new(LocalRecoveryJournal::new(base.path()))),
    );
    let runtime = Runtime::start(
        AppServices {
            setup: Some(setup),
            discovery: None,
            gateway: None,
            wake: None,
        },
        RuntimeLimits {
            drain_deadline: std::time::Duration::from_millis(1),
            ..RuntimeLimits::default()
        },
    );
    gate.2.notified().await;
    let store = Arc::new(MemoryStop::new(false));
    let lifecycle = Lifecycle::start(runtime, store.clone(), directory.lock().unwrap())
        .await
        .unwrap();
    let mut shutdown = lifecycle.http_shutdown.clone();
    lifecycle.signal_shutdown().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), shutdown.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(directory.lock(), Err(FileError::Locked)));
    assert!(store.read().unwrap().is_none());
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    lifecycle.settled().await.unwrap();
    let _replacement = directory.lock().unwrap();
}

#[tokio::test]
async fn completed_runtime_retains_ownership_until_composition_has_closed_its_listeners() {
    let base = tempfile::tempdir().unwrap();
    let directory = Arc::new(
        RuntimeDirectory::open(&base.path().canonicalize().unwrap().join("runtime")).unwrap(),
    );
    let store = Arc::new(MemoryStop::new(false));
    let lifecycle = Lifecycle::start(runtime(), store, directory.lock().unwrap())
        .await
        .unwrap();
    lifecycle.signal_shutdown().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while lifecycle.client.serving_state() != "stopped" {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    // Runtime settlement can precede the listener tasks observing shutdown.
    // Completion of the actor must keep its lock in the join result meanwhile.
    assert!(*lifecycle.http_shutdown.borrow());
    assert!(matches!(directory.lock(), Err(FileError::Locked)));
    lifecycle.settled().await.unwrap();
    assert!(directory.lock().is_ok());
}
