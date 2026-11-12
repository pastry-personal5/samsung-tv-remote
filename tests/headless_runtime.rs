use std::sync::{Arc, Mutex};
use std::time::Duration;

use samsung_tv_remote::application::certificate_trust::CertificatePin;
use samsung_tv_remote::application::control_service::*;
use samsung_tv_remote::application::credential_store::{PairingToken, SecretError};
use samsung_tv_remote::application::device_repository::{RepositoryError, SavedDevice};
use samsung_tv_remote::application::runtime::{Runtime, RuntimeLimits};
use samsung_tv_remote::application::tv_address::TvHost;
use samsung_tv_remote::application::tv_control_coordinator::{AppServices, WakeStage};
use samsung_tv_remote::application::tv_discovery::{DiscoveryError, TvDiscovery};
use samsung_tv_remote::application::tv_session::SessionFuture;
use samsung_tv_remote::application::tv_setup_service::{
    ForgetResult, ReconnectError, ReconnectMaterial, SetupError, TvSetupPort,
};
use samsung_tv_remote::application::wake::WakeConfiguration;
use samsung_tv_remote::DeviceId;
use tokio::sync::Semaphore;

#[derive(Default)]
struct Profile {
    devices: Vec<SavedDevice>,
    selected: Option<DeviceId>,
    writes: usize,
    corrupt: bool,
    fail_after_write: bool,
    corrupt_after_write: bool,
}

#[derive(Default)]
struct FakeSetup {
    profile: Mutex<Profile>,
    gate: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    restore_gate: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    reconnect_gate: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl TvSetupPort for FakeSetup {
    fn pending_pairing(
        &self,
    ) -> Result<Option<samsung_tv_remote::application::recovery::PairingMarker>, RepositoryError>
    {
        Ok(None)
    }

    fn pending_recovery(
        &self,
    ) -> Result<Option<samsung_tv_remote::application::recovery::ForgetMarker>, RepositoryError>
    {
        Ok(None)
    }

    fn saved_devices(&self) -> Result<Vec<SavedDevice>, RepositoryError> {
        if let Some(gate) = self.restore_gate.lock().unwrap().take() {
            gate.recv().unwrap();
        }
        let profile = self.profile.lock().unwrap();
        if profile.corrupt {
            Err(RepositoryError::Corrupt)
        } else {
            Ok(profile.devices.clone())
        }
    }
    fn selected_device(&self) -> Result<Option<SavedDevice>, RepositoryError> {
        let profile = self.profile.lock().unwrap();
        Ok(profile
            .devices
            .iter()
            .find(|device| Some(device.id) == profile.selected)
            .cloned())
    }
    fn select_saved(&self, id: DeviceId) -> Result<SavedDevice, RepositoryError> {
        if let Some(gate) = self.gate.lock().unwrap().take() {
            gate.recv().unwrap();
        }
        let mut profile = self.profile.lock().unwrap();
        let device = profile
            .devices
            .iter()
            .find(|device| device.id == id)
            .cloned()
            .ok_or(RepositoryError::Corrupt)?;
        profile.selected = Some(id);
        profile.writes += 1;
        profile.corrupt = profile.corrupt_after_write;
        if profile.fail_after_write {
            return Err(RepositoryError::Unavailable);
        }
        Ok(device)
    }
    fn save_wake_configuration(
        &self,
        id: DeviceId,
        wake: WakeConfiguration,
    ) -> Result<SavedDevice, RepositoryError> {
        let mut profile = self.profile.lock().unwrap();
        let device = profile
            .devices
            .iter_mut()
            .find(|device| device.id == id)
            .ok_or(RepositoryError::Corrupt)?;
        device.wake = wake;
        let result = device.clone();
        profile.writes += 1;
        profile.corrupt = profile.corrupt_after_write;
        if profile.fail_after_write {
            return Err(RepositoryError::Unavailable);
        }
        Ok(result)
    }
    fn commit_pairing(
        &self,
        _: &str,
        _: TvHost,
        _: CertificatePin,
        _: PairingToken,
    ) -> Result<SavedDevice, SetupError> {
        panic!("unexpected pairing")
    }
    fn repair(
        &self,
        _: DeviceId,
        _: &str,
        _: TvHost,
        _: CertificatePin,
        _: PairingToken,
    ) -> Result<SavedDevice, SetupError> {
        panic!("unexpected re-pair")
    }
    fn reconnect_material(&self, _: DeviceId) -> Result<ReconnectMaterial, ReconnectError> {
        if let Some(gate) = self.reconnect_gate.lock().unwrap().take() {
            gate.recv().unwrap();
        }
        let device = self
            .profile
            .lock()
            .unwrap()
            .devices
            .first()
            .cloned()
            .ok_or(ReconnectError::MissingDevice)?;
        Ok(ReconnectMaterial {
            device,
            pin: CertificatePin::from_der(b"synthetic test certificate"),
            token: PairingToken::from_stored("synthetic-test-token".into()).unwrap(),
        })
    }
    fn save_rotated_token(&self, _: DeviceId, _: &PairingToken) -> Result<(), SecretError> {
        panic!("unexpected token write")
    }
    fn forget(&self, _: DeviceId) -> ForgetResult {
        panic!("startup must never delete records")
    }
}

struct FakeDiscovery {
    gate: Arc<Semaphore>,
    calls: Arc<std::sync::atomic::AtomicUsize>,
}
impl TvDiscovery for FakeDiscovery {
    fn discover(&self) -> SessionFuture<'_, Result<Vec<TvHost>, DiscoveryError>> {
        Box::pin(async {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.gate.acquire().await.unwrap().forget();
            Ok(vec![TvHost::parse("tv.local").unwrap()])
        })
    }
}

fn device(id: u64) -> SavedDevice {
    SavedDevice {
        id: DeviceId::new(id),
        label: "TV".into(),
        host: TvHost::parse("tv.local").unwrap(),
        wake: WakeConfiguration::default(),
    }
}
fn services(setup: Arc<FakeSetup>) -> AppServices {
    AppServices {
        setup: Some(setup),
        discovery: None,
        gateway: None,
        wake: None,
    }
}
fn request(snapshot: &Snapshot, key: &str, command: Command) -> Request {
    Request {
        epoch: snapshot.epoch,
        key: key.into(),
        command,
    }
}
async fn ready(service: &ControlService) -> Snapshot {
    let mut subscription = service.subscribe().await.unwrap();
    if subscription.snapshot.startup != StartupState::Restoring {
        return subscription.snapshot;
    }
    loop {
        let event = subscription.events.recv().await.unwrap();
        if event.snapshot.startup != StartupState::Restoring {
            return event.snapshot;
        }
    }
}
async fn terminal(service: &ControlService, snapshot: &Snapshot, key: &str) -> Receipt {
    let mut subscription = service.subscribe().await.unwrap();
    loop {
        if let Lookup::Known(receipt) = service.lookup(snapshot.epoch, key.into()).await.unwrap() {
            if receipt.outcome.is_some() {
                return receipt;
            }
        }
        subscription.events.recv().await.unwrap();
    }
}

#[tokio::test]
async fn restore_preserves_duplicate_hosts_and_generation_zero_without_tv_io() {
    let setup = Arc::new(FakeSetup::default());
    {
        let mut profile = setup.profile.lock().unwrap();
        profile.devices = vec![device(1), device(2)];
        profile.selected = Some(DeviceId::new(2));
    }
    let runtime = Runtime::start(services(setup.clone()), RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    assert_eq!(snapshot.devices.len(), 2);
    assert!(snapshot.duplicate_records);
    assert_eq!(snapshot.control.selection_generation(), 0);
    assert_eq!(snapshot.control.selected_device(), Some(DeviceId::new(2)));
    assert_eq!(setup.profile.lock().unwrap().writes, 0);
    runtime.drain().await;
}

#[tokio::test]
async fn corruption_blocks_admission_visibly_and_retains_rejection() {
    let setup = Arc::new(FakeSetup::default());
    setup.profile.lock().unwrap().corrupt = true;
    let runtime = Runtime::start(services(setup), RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    assert_eq!(
        snapshot.startup,
        StartupState::Blocked(RepositoryError::Corrupt)
    );
    let receipt = service
        .submit(request(&snapshot, "blocked", Command::Discover))
        .await
        .unwrap();
    assert!(matches!(
        receipt.outcome,
        Some(Outcome::Rejected(Rejection::Blocked))
    ));
    assert!(matches!(
        service
            .lookup(snapshot.epoch, "blocked".into())
            .await
            .unwrap(),
        Lookup::Known(_)
    ));
    runtime.drain().await;
}

#[tokio::test]
async fn duplicates_precede_capacity_and_changed_payload_conflicts() {
    let runtime = Runtime::start(
        services(Arc::new(FakeSetup::default())),
        RuntimeLimits {
            registry: 1,
            ..RuntimeLimits::default()
        },
    );
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    let original = request(&snapshot, "once", Command::Discover);
    let receipt = service.submit(original.clone()).await.unwrap();
    assert!(receipt.outcome.is_some());
    assert_eq!(service.submit(original).await.unwrap().id, receipt.id);
    assert_eq!(
        service
            .submit(request(
                &snapshot,
                "once",
                Command::Probe(TvHost::parse("tv.local").unwrap())
            ))
            .await
            .unwrap_err(),
        ServiceError::KeyConflict
    );
    assert_eq!(
        service
            .submit(request(&snapshot, "new", Command::Discover))
            .await
            .unwrap_err(),
        ServiceError::RegistryFull
    );
    runtime.drain().await;
}

#[tokio::test]
async fn two_clients_cas_selection_and_same_id_reselection_changes_generation() {
    let setup = Arc::new(FakeSetup::default());
    setup.profile.lock().unwrap().devices = vec![device(1), device(2)];
    let runtime = Runtime::start(services(setup.clone()), RuntimeLimits::default());
    let one = runtime.client(ClientId(1));
    let two = runtime.client(ClientId(2));
    let snapshot = ready(&one).await;
    one.submit(request(
        &snapshot,
        "select",
        Command::Select {
            target: DeviceId::new(1),
            generation: 0,
        },
    ))
    .await
    .unwrap();
    assert!(matches!(
        terminal(&one, &snapshot, "select").await.outcome,
        Some(Outcome::Saved(_))
    ));
    let failed = two
        .submit(request(
            &snapshot,
            "select",
            Command::Select {
                target: DeviceId::new(2),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        failed.outcome,
        Some(Outcome::Rejected(Rejection::StaleSelection))
    ));
    one.submit(request(
        &snapshot,
        "reselect",
        Command::Select {
            target: DeviceId::new(1),
            generation: 1,
        },
    ))
    .await
    .unwrap();
    terminal(&one, &snapshot, "reselect").await;
    assert_eq!(
        one.snapshot().await.unwrap().control.selection_generation(),
        2
    );
    assert_eq!(setup.profile.lock().unwrap().writes, 2);
    runtime.drain().await;
}

#[tokio::test(start_paused = true)]
async fn timed_out_blocking_worker_keeps_fence_and_queries_responsive() {
    let setup = Arc::new(FakeSetup::default());
    setup.profile.lock().unwrap().devices = vec![device(1)];
    let (release, wait) = std::sync::mpsc::channel();
    *setup.gate.lock().unwrap() = Some(wait);
    let runtime = Runtime::start(services(setup.clone()), RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    service
        .submit(request(
            &snapshot,
            "slow",
            Command::Select {
                target: DeviceId::new(1),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(11)).await;
    tokio::task::yield_now().await;
    assert!(matches!(
        terminal(&service, &snapshot, "slow").await.outcome,
        Some(Outcome::Deadline)
    ));
    let during = service.snapshot().await.unwrap();
    assert!(during.storage_pending);
    assert_eq!(during.control.selected_device(), None);
    let blocked = service
        .submit(request(
            &snapshot,
            "replacement",
            Command::Select {
                target: DeviceId::new(1),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        blocked.outcome,
        Some(Outcome::Rejected(Rejection::Busy))
    ));
    release.send(()).unwrap();
    let mut subscription = service.subscribe().await.unwrap();
    while subscription.snapshot.storage_pending {
        subscription.snapshot = subscription.events.recv().await.unwrap().snapshot;
    }
    assert_eq!(setup.profile.lock().unwrap().writes, 1);
    assert!(matches!(
        service.lookup(snapshot.epoch, "slow".into()).await.unwrap(),
        Lookup::Known(Receipt {
            outcome: Some(Outcome::Deadline),
            ..
        })
    ));
    runtime.drain().await;
}

#[tokio::test(start_paused = true)]
async fn terminal_retention_is_monotonic_and_restart_is_explicit() {
    let setup = Arc::new(FakeSetup::default());
    let runtime = Runtime::start(
        services(setup.clone()),
        RuntimeLimits {
            retention: Duration::from_secs(30),
            ..RuntimeLimits::default()
        },
    );
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    service
        .submit(request(&snapshot, "key", Command::Discover))
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(29)).await;
    assert!(matches!(
        service.lookup(snapshot.epoch, "key".into()).await.unwrap(),
        Lookup::Known(_)
    ));
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(matches!(
        service.lookup(snapshot.epoch, "key".into()).await.unwrap(),
        Lookup::OutcomeUnavailable
    ));
    runtime.drain().await;
    let replacement = Runtime::start(services(setup), RuntimeLimits::default());
    let new_service = replacement.client(ClientId(1));
    ready(&new_service).await;
    assert!(matches!(
        new_service
            .lookup(snapshot.epoch, "key".into())
            .await
            .unwrap(),
        Lookup::UnknownAfterRestart
    ));
    assert_eq!(
        new_service
            .submit(request(&snapshot, "key", Command::Discover))
            .await
            .unwrap_err(),
        ServiceError::StaleEpoch
    );
    replacement.drain().await;
}

#[tokio::test]
async fn subscription_is_atomic_overflow_closes_and_client_results_are_private() {
    let runtime = Runtime::start(
        services(Arc::new(FakeSetup::default())),
        RuntimeLimits {
            subscriber_backlog: 1,
            ..RuntimeLimits::default()
        },
    );
    let one = runtime.client(ClientId(1));
    let two = runtime.client(ClientId(2));
    let snapshot = ready(&one).await;
    let mut subscription = two.subscribe().await.unwrap();
    let revision = subscription.snapshot.revision;
    one.submit(request(&snapshot, "one", Command::Discover))
        .await
        .unwrap();
    assert_eq!(
        subscription.events.recv().await.unwrap().snapshot.revision,
        revision + 1
    );
    one.submit(request(&snapshot, "two", Command::Discover))
        .await
        .unwrap();
    one.submit(request(&snapshot, "three", Command::Discover))
        .await
        .unwrap();
    subscription.events.recv().await.unwrap();
    assert!(subscription.events.recv().await.is_none());
    assert!(matches!(
        two.lookup(snapshot.epoch, "one".into()).await.unwrap(),
        Lookup::OutcomeUnavailable
    ));
    assert!(matches!(
        one.lookup(snapshot.epoch, "one".into()).await.unwrap(),
        Lookup::Known(_)
    ));
    runtime.drain().await;
}

#[tokio::test]
async fn discovery_progress_survives_client_detach_and_duplicate_has_one_effect() {
    let gate = Arc::new(Semaphore::new(0));
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut adapters = services(Arc::new(FakeSetup::default()));
    adapters.discovery = Some(Arc::new(FakeDiscovery {
        gate: gate.clone(),
        calls: calls.clone(),
    }));
    let runtime = Runtime::start(adapters, RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    let original = request(&snapshot, "discovery", Command::Discover);
    let receipt = service.submit(original.clone()).await.unwrap();
    drop(service);
    let replacement = runtime.client(ClientId(1));
    assert_eq!(replacement.submit(original).await.unwrap().id, receipt.id);
    gate.add_permits(1);
    assert!(matches!(
        terminal(&replacement, &snapshot, "discovery").await.outcome,
        Some(Outcome::Discovered(_))
    ));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    runtime.drain().await;
}

use samsung_tv_remote::application::remote_dispatcher::RequestId;
use samsung_tv_remote::application::tv_session::{
    ActiveTvSession, ProbeObservation, TvConnection, TvGateway, TvSessionControl, TvSessionError,
    TvSessionEvent,
};
use samsung_tv_remote::{RemoteAction, SendRemoteAction};

struct FakeGateway {
    writes: Arc<Mutex<Vec<RemoteAction>>>,
    uncertain: Arc<std::sync::atomic::AtomicBool>,
    hold_writes: bool,
}
struct FakeConnection {
    writes: Arc<Mutex<Vec<RemoteAction>>>,
    uncertain: Arc<std::sync::atomic::AtomicBool>,
    hold_writes: bool,
}
struct FakeControl {
    writes: Arc<Mutex<Vec<RemoteAction>>>,
    events: tokio::sync::mpsc::Sender<TvSessionEvent>,
    uncertain: Arc<std::sync::atomic::AtomicBool>,
    hold_writes: bool,
}
impl TvSessionControl for FakeControl {
    fn try_click(&self, id: RequestId, action: RemoteAction) -> bool {
        self.writes.lock().unwrap().push(action);
        if self.hold_writes {
            return true;
        }
        let event = if self.uncertain.load(std::sync::atomic::Ordering::SeqCst) {
            TvSessionEvent::Uncertain(id)
        } else {
            TvSessionEvent::Written(id)
        };
        self.events.try_send(event).is_ok()
    }
    fn abort(&mut self) {}
}
impl TvConnection for FakeConnection {
    fn await_authorized(
        &mut self,
        _: bool,
    ) -> SessionFuture<'_, Result<Option<PairingToken>, TvSessionError>> {
        Box::pin(async { Ok(None) })
    }
    fn start(self: Box<Self>) -> ActiveTvSession {
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        ActiveTvSession {
            control: Box::new(FakeControl {
                writes: self.writes,
                events: sender,
                uncertain: self.uncertain,
                hold_writes: self.hold_writes,
            }),
            events: Box::pin(futures_util::stream::unfold(
                receiver,
                |mut receiver| async { receiver.recv().await.map(|event| (event, receiver)) },
            )),
        }
    }
}
impl TvGateway for FakeGateway {
    fn probe(&self, _: TvHost) -> SessionFuture<'static, Result<ProbeObservation, TvSessionError>> {
        Box::pin(async { Err(TvSessionError::Offline) })
    }
    fn connect(
        &self,
        _: TvHost,
        _: CertificatePin,
        _: Option<PairingToken>,
    ) -> SessionFuture<'static, Result<Box<dyn TvConnection>, TvSessionError>> {
        let connection = FakeConnection {
            writes: self.writes.clone(),
            uncertain: self.uncertain.clone(),
            hold_writes: self.hold_writes,
        };
        Box::pin(async move { Ok(Box::new(connection) as Box<dyn TvConnection>) })
    }
}

#[tokio::test]
async fn headless_session_pumps_remote_and_inference_without_gui_polling() {
    let setup = Arc::new(FakeSetup::default());
    {
        let mut profile = setup.profile.lock().unwrap();
        profile.devices = vec![device(1)];
        profile.selected = Some(DeviceId::new(1));
    }
    let writes = Arc::new(Mutex::new(Vec::new()));
    let uncertain = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut adapters = services(setup);
    adapters.gateway = Some(Arc::new(FakeGateway {
        writes: writes.clone(),
        uncertain: uncertain.clone(),
        hold_writes: false,
    }));
    let runtime = Runtime::start(adapters, RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    service
        .submit(request(
            &snapshot,
            "connect",
            Command::Connect {
                target: DeviceId::new(1),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        terminal(&service, &snapshot, "connect").await.outcome,
        Some(Outcome::Connected)
    ));
    let action = Command::Remote(SendRemoteAction::new(
        DeviceId::new(1),
        0,
        RemoteAction::PlayPause,
    ));
    service
        .submit(request(&snapshot, "play", action.clone()))
        .await
        .unwrap();
    terminal(&service, &snapshot, "play").await;
    uncertain.store(true, std::sync::atomic::Ordering::SeqCst);
    service
        .submit(request(&snapshot, "pause", action.clone()))
        .await
        .unwrap();
    terminal(&service, &snapshot, "pause").await;
    assert_eq!(
        service.snapshot().await.unwrap().control.playback_state(),
        samsung_tv_remote::application::PlaybackState::Unknown
    );
    uncertain.store(false, std::sync::atomic::Ordering::SeqCst);
    service
        .submit(request(&snapshot, "play-again", action.clone()))
        .await
        .unwrap();
    terminal(&service, &snapshot, "play-again").await;
    service
        .submit(request(&snapshot, "play", action))
        .await
        .unwrap();
    assert_eq!(
        *writes.lock().unwrap(),
        vec![RemoteAction::Play, RemoteAction::Pause, RemoteAction::Play]
    );
    runtime.drain().await;
}

struct PanickingDiscovery;
impl TvDiscovery for PanickingDiscovery {
    fn discover(&self) -> SessionFuture<'_, Result<Vec<TvHost>, DiscoveryError>> {
        Box::pin(async { panic!("synthetic effect panic") })
    }
}
#[tokio::test]
async fn effect_panic_settles_original_reservation_and_owner_survives() {
    let mut adapters = services(Arc::new(FakeSetup::default()));
    adapters.discovery = Some(Arc::new(PanickingDiscovery));
    let runtime = Runtime::start(adapters, RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    service
        .submit(request(&snapshot, "panic", Command::Discover))
        .await
        .unwrap();
    assert!(matches!(
        terminal(&service, &snapshot, "panic").await.outcome,
        Some(Outcome::SupervisorFailed)
    ));
    assert_eq!(
        service.snapshot().await.unwrap().startup,
        StartupState::Ready
    );
    runtime.drain().await;
}

#[tokio::test]
async fn subscribers_and_effects_have_finite_caps_and_drain_closes_clients() {
    let gate = Arc::new(Semaphore::new(0));
    let mut adapters = services(Arc::new(FakeSetup::default()));
    adapters.discovery = Some(Arc::new(FakeDiscovery {
        gate: gate.clone(),
        calls: Arc::default(),
    }));
    let runtime = Runtime::start(
        adapters,
        RuntimeLimits {
            effects: 1,
            subscribers_per_client: 1,
            ..RuntimeLimits::default()
        },
    );
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    let observer = service.subscribe().await.unwrap();
    assert!(matches!(
        service.subscribe().await,
        Err(ServiceError::SubscriberLimit)
    ));
    service
        .submit(request(&snapshot, "active", Command::Discover))
        .await
        .unwrap();
    let rejected = service
        .submit(request(&snapshot, "overload", Command::Discover))
        .await
        .unwrap();
    assert!(matches!(
        rejected.outcome,
        Some(Outcome::Rejected(Rejection::Busy))
    ));
    drop(observer);
    gate.add_permits(1);
    runtime.drain().await;
    assert_eq!(service.snapshot().await.unwrap_err(), ServiceError::Closed);
}

#[tokio::test]
async fn wake_configuration_uses_durable_revision_and_retains_stale_rejection() {
    let setup = Arc::new(FakeSetup::default());
    {
        let mut profile = setup.profile.lock().unwrap();
        profile.devices = vec![device(1)];
        profile.selected = Some(DeviceId::new(1));
    }
    let runtime = Runtime::start(services(setup.clone()), RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    let command = Command::SetWake {
        target: DeviceId::new(1),
        generation: 0,
        revision: 0,
        configuration: WakeConfiguration::default(),
    };
    let original = request(&snapshot, "configuration", command.clone());
    let receipt = service.submit(original.clone()).await.unwrap();
    assert!(matches!(
        terminal(&service, &snapshot, "configuration").await.outcome,
        Some(Outcome::Saved(_))
    ));
    assert_eq!(
        service.snapshot().await.unwrap().configuration_revisions[0].1,
        1
    );
    assert_eq!(service.submit(original).await.unwrap().id, receipt.id);
    let stale = service
        .submit(request(&snapshot, "stale", command))
        .await
        .unwrap();
    assert!(matches!(
        stale.outcome,
        Some(Outcome::Rejected(Rejection::StaleConfiguration))
    ));
    assert_eq!(setup.profile.lock().unwrap().writes, 1);
    runtime.drain().await;
}

#[tokio::test(start_paused = true)]
async fn discovery_deadline_releases_effect_capacity() {
    let gate = Arc::new(Semaphore::new(0));
    let mut adapters = services(Arc::new(FakeSetup::default()));
    adapters.discovery = Some(Arc::new(FakeDiscovery {
        gate,
        calls: Arc::default(),
    }));
    let runtime = Runtime::start(
        adapters,
        RuntimeLimits {
            effects: 1,
            ..RuntimeLimits::default()
        },
    );
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    service
        .submit(request(&snapshot, "timeout", Command::Discover))
        .await
        .unwrap();
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(10)).await;
    assert!(matches!(
        terminal(&service, &snapshot, "timeout").await.outcome,
        Some(Outcome::DiscoveryFailed(DiscoveryError::Unavailable))
    ));
    let receipt = service
        .submit(request(&snapshot, "next", Command::Discover))
        .await
        .unwrap();
    assert!(receipt.outcome.is_none());
    runtime.drain().await;
}

#[tokio::test(start_paused = true)]
async fn drain_reports_blocking_storage_and_keeps_supervision_until_settlement() {
    let setup = Arc::new(FakeSetup::default());
    setup.profile.lock().unwrap().devices = vec![device(1)];
    let (release, wait) = std::sync::mpsc::channel();
    *setup.gate.lock().unwrap() = Some(wait);
    let runtime = Runtime::start(services(setup.clone()), RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    service
        .submit(request(
            &snapshot,
            "pending-store",
            Command::Select {
                target: DeviceId::new(1),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    let mut observer = service.subscribe().await.unwrap();
    let drain = tokio::spawn(runtime.drain());
    // Observe the actual admission-close boundary before advancing fake time.
    loop {
        let event = observer.events.recv().await.unwrap();
        if event.snapshot.startup == StartupState::Draining {
            assert!(event.snapshot.storage_pending);
            break;
        }
    }
    tokio::time::advance(Duration::from_secs(5)).await;
    let drained = drain.await.unwrap();
    assert!(drained.status.unresolved_storage);
    assert_eq!(drained.status.pending_effects, 1);
    assert!(!drained.is_settled());
    assert_eq!(
        service.snapshot().await.unwrap().startup,
        StartupState::Draining
    );
    assert!(matches!(
        service
            .lookup(snapshot.epoch, "pending-store".into())
            .await
            .unwrap(),
        Lookup::Known(Receipt {
            outcome: Some(Outcome::StorageUncertain),
            ..
        })
    ));
    assert_eq!(setup.profile.lock().unwrap().writes, 0);
    release.send(()).unwrap();
    drained.settled().await.unwrap();
    assert_eq!(setup.profile.lock().unwrap().writes, 1);
    assert_eq!(service.snapshot().await.unwrap_err(), ServiceError::Closed);
    // The final event publishes the actual late durable result, with its fence
    // released only after the blocking worker returned.
    let mut final_snapshot = snapshot;
    while let Some(event) = observer.events.recv().await {
        final_snapshot = event.snapshot;
    }
    assert!(!final_snapshot.storage_pending);
    assert_eq!(
        final_snapshot.control.selected_device(),
        Some(DeviceId::new(1))
    );
}

#[tokio::test(start_paused = true)]
async fn drain_deadline_preserves_running_effect_and_closes_admission() {
    let gate = Arc::new(Semaphore::new(0));
    let mut adapters = services(Arc::new(FakeSetup::default()));
    adapters.discovery = Some(Arc::new(FakeDiscovery {
        gate: gate.clone(),
        calls: Arc::default(),
    }));
    let runtime = Runtime::start(adapters, RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    service
        .submit(request(&snapshot, "discovery", Command::Discover))
        .await
        .unwrap();
    let mut observer = service.subscribe().await.unwrap();
    let drain = tokio::spawn(runtime.drain());
    while observer.events.recv().await.unwrap().snapshot.startup != StartupState::Draining {}
    tokio::time::advance(Duration::from_secs(5)).await;
    let drained = drain.await.unwrap();
    assert!(!drained.status.unresolved_storage);
    assert_eq!(drained.status.pending_effects, 1);
    assert_eq!(
        service
            .submit(request(&snapshot, "late", Command::Discover))
            .await
            .unwrap_err(),
        ServiceError::Closed
    );
    gate.add_permits(1);
    drained.settled().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn drain_reports_unresolved_restore_without_claiming_a_ready_profile() {
    let setup = Arc::new(FakeSetup::default());
    let (release, wait) = std::sync::mpsc::channel();
    *setup.restore_gate.lock().unwrap() = Some(wait);
    let runtime = Runtime::start(services(setup.clone()), RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let mut observer = service.subscribe().await.unwrap();
    assert_eq!(observer.snapshot.startup, StartupState::Restoring);
    let drain = tokio::spawn(runtime.drain());
    while observer.events.recv().await.unwrap().snapshot.startup != StartupState::Draining {}
    tokio::time::advance(Duration::from_secs(5)).await;
    let drained = drain.await.unwrap();
    assert!(drained.status.unresolved_storage);
    assert_eq!(drained.status.pending_effects, 1);
    assert_eq!(
        service.snapshot().await.unwrap().startup,
        StartupState::Draining
    );
    release.send(()).unwrap();
    drained.settled().await.unwrap();
    while let Some(event) = observer.events.recv().await {
        assert_eq!(event.snapshot.startup, StartupState::Draining);
    }
    assert_eq!(setup.profile.lock().unwrap().writes, 0);
}

#[tokio::test]
async fn idle_drain_settles_without_waiting_for_deadline() {
    let runtime = Runtime::start(
        services(Arc::new(FakeSetup::default())),
        RuntimeLimits::default(),
    );
    let service = runtime.client(ClientId(1));
    ready(&service).await;
    let drained = runtime.drain().await;
    assert!(!drained.status.unresolved_storage);
    assert_eq!(drained.status.pending_effects, 0);
    drained.settled().await.unwrap();
    assert_eq!(service.snapshot().await.unwrap_err(), ServiceError::Closed);
}

#[tokio::test(start_paused = true)]
async fn drain_retains_uncertain_writer_and_not_sent_waiters_without_replay() {
    use samsung_tv_remote::application::remote_dispatcher::{NotSentReason, TerminalOutcome};
    let setup = Arc::new(FakeSetup::default());
    {
        let mut profile = setup.profile.lock().unwrap();
        profile.devices = vec![device(1)];
        profile.selected = Some(DeviceId::new(1));
    }
    let writes = Arc::new(Mutex::new(Vec::new()));
    let gate = Arc::new(Semaphore::new(0));
    let mut adapters = services(setup);
    adapters.gateway = Some(Arc::new(FakeGateway {
        writes: writes.clone(),
        uncertain: Arc::default(),
        hold_writes: true,
    }));
    adapters.discovery = Some(Arc::new(FakeDiscovery {
        gate: gate.clone(),
        calls: Arc::default(),
    }));
    let runtime = Runtime::start(adapters, RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    service
        .submit(request(
            &snapshot,
            "connect",
            Command::Connect {
                target: DeviceId::new(1),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    terminal(&service, &snapshot, "connect").await;
    // Keep the draining owner observable while a separate effect settles.
    service
        .submit(request(&snapshot, "discovery", Command::Discover))
        .await
        .unwrap();
    for key in ["writing", "waiting"] {
        service
            .submit(request(
                &snapshot,
                key,
                Command::Remote(SendRemoteAction::new(
                    DeviceId::new(1),
                    0,
                    RemoteAction::VolumeUp,
                )),
            ))
            .await
            .unwrap();
    }
    let mut observer = service.subscribe().await.unwrap();
    let drain = tokio::spawn(runtime.drain());
    while observer.events.recv().await.unwrap().snapshot.startup != StartupState::Draining {}
    assert!(matches!(
        service
            .lookup(snapshot.epoch, "writing".into())
            .await
            .unwrap(),
        Lookup::Known(Receipt {
            outcome: Some(Outcome::Remote(TerminalOutcome::Uncertain)),
            ..
        })
    ));
    assert!(matches!(
        service
            .lookup(snapshot.epoch, "waiting".into())
            .await
            .unwrap(),
        Lookup::Known(Receipt {
            outcome: Some(Outcome::Remote(TerminalOutcome::NotSent(
                NotSentReason::Cancelled
            ))),
            ..
        })
    ));
    assert!(matches!(
        service
            .lookup(snapshot.epoch, "discovery".into())
            .await
            .unwrap(),
        Lookup::Known(Receipt {
            outcome: Some(Outcome::Drained),
            ..
        })
    ));
    assert_eq!(*writes.lock().unwrap(), vec![RemoteAction::VolumeUp]);
    tokio::time::advance(Duration::from_secs(5)).await;
    let drained = drain.await.unwrap();
    gate.add_permits(1);
    drained.settled().await.unwrap();
    assert_eq!(*writes.lock().unwrap(), vec![RemoteAction::VolumeUp]);
}

#[tokio::test(start_paused = true)]
async fn operation_id_lookup_is_client_scoped_retained_and_epoch_bound() {
    let runtime = Runtime::start(
        services(Arc::new(FakeSetup::default())),
        RuntimeLimits {
            retention: Duration::from_secs(30),
            ..RuntimeLimits::default()
        },
    );
    let one = runtime.client(ClientId(1));
    let two = runtime.client(ClientId(2));
    let snapshot = ready(&one).await;
    let receipt = one
        .submit(request(&snapshot, "retained", Command::Discover))
        .await
        .unwrap();
    assert!(
        matches!(one.operation(snapshot.epoch, receipt.id).await.unwrap(), Lookup::Known(found) if found.id == receipt.id)
    );
    assert!(matches!(
        two.operation(snapshot.epoch, receipt.id).await.unwrap(),
        Lookup::OutcomeUnavailable
    ));
    tokio::time::advance(Duration::from_secs(30)).await;
    assert!(matches!(
        one.operation(snapshot.epoch, receipt.id).await.unwrap(),
        Lookup::OutcomeUnavailable
    ));
    runtime.drain().await.settled().await.unwrap();
    let replacement = Runtime::start(
        services(Arc::new(FakeSetup::default())),
        RuntimeLimits::default(),
    );
    let one = replacement.client(ClientId(1));
    ready(&one).await;
    assert!(matches!(
        one.operation(snapshot.epoch, receipt.id).await.unwrap(),
        Lookup::UnknownAfterRestart
    ));
    replacement.drain().await.settled().await.unwrap();
}

struct OfflineGateway;
impl TvGateway for OfflineGateway {
    fn probe(&self, _: TvHost) -> SessionFuture<'static, Result<ProbeObservation, TvSessionError>> {
        Box::pin(async { Err(TvSessionError::Offline) })
    }
    fn connect(
        &self,
        _: TvHost,
        _: CertificatePin,
        _: Option<PairingToken>,
    ) -> SessionFuture<'static, Result<Box<dyn TvConnection>, TvSessionError>> {
        Box::pin(async { Err(TvSessionError::Offline) })
    }
}

#[derive(Default)]
struct CountingWake(std::sync::atomic::AtomicUsize);
impl samsung_tv_remote::application::wake_transport::WakeTransport for CountingWake {
    fn send_once(
        &self,
        _: TvHost,
        _: samsung_tv_remote::domain::MacAddress,
        permit: samsung_tv_remote::application::wake_transport::WakePermit,
    ) -> samsung_tv_remote::application::wake_transport::WakeFuture {
        let sent = permit.send_if_current(|| {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });
        Box::pin(async move { sent })
    }
}

async fn power_fixture(
    gateway: Arc<dyn TvGateway>,
    wake: Arc<CountingWake>,
) -> (Runtime, ControlService, Snapshot) {
    let setup = Arc::new(FakeSetup::default());
    {
        let mut profile = setup.profile.lock().unwrap();
        let mut tv = device(1);
        tv.wake = WakeConfiguration {
            wired: Some("02:11:22:33:44:55".parse().unwrap()),
            wifi: None,
            active: Some(samsung_tv_remote::application::wake::WakeInterface::Wired),
        };
        profile.devices = vec![tv];
        profile.selected = Some(DeviceId::new(1));
    }
    let mut adapters = services(setup);
    adapters.gateway = Some(gateway);
    adapters.wake = Some(wake);
    let runtime = Runtime::start(adapters, RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    (runtime, service, snapshot)
}

fn power() -> Command {
    Command::Remote(SendRemoteAction::new(
        DeviceId::new(1),
        0,
        RemoteAction::PowerToggle,
    ))
}

#[tokio::test]
async fn disconnected_power_reachable_is_ready_without_key_or_packet() {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let wake = Arc::new(CountingWake::default());
    let (runtime, service, snapshot) = power_fixture(
        Arc::new(FakeGateway {
            writes: writes.clone(),
            uncertain: Arc::new(Default::default()),
            hold_writes: false,
        }),
        wake.clone(),
    )
    .await;
    service
        .submit(request(&snapshot, "power", power()))
        .await
        .unwrap();
    assert!(matches!(
        terminal(&service, &snapshot, "power").await.outcome,
        Some(Outcome::Connected)
    ));
    assert!(writes.lock().unwrap().is_empty());
    assert_eq!(wake.0.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(service.snapshot().await.unwrap().wake, WakeStage::Connected);
    runtime.drain().await;
}

#[tokio::test(start_paused = true)]
async fn disconnected_power_offline_sends_exactly_one_packet_and_cancel_preserves_facts() {
    let wake = Arc::new(CountingWake::default());
    let (runtime, service, snapshot) = power_fixture(Arc::new(OfflineGateway), wake.clone()).await;
    service
        .submit(request(&snapshot, "power", power()))
        .await
        .unwrap();
    // Let the packet send, then cancel during the reconnect window.
    while wake.0.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    service
        .submit(request(
            &snapshot,
            "cancel",
            Command::CancelWake {
                operation: service.snapshot().await.unwrap().wake_operation.unwrap(),
                target: DeviceId::new(1),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    let Some(Outcome::Wake(stage)) = terminal(&service, &snapshot, "power").await.outcome else {
        panic!("power did not settle with a wake outcome");
    };
    assert_eq!(
        stage,
        samsung_tv_remote::application::tv_control_coordinator::WakeStage::CancelledAfterSend
    );
    assert_eq!(wake.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    runtime.drain().await;
}

struct GatedWake {
    gate: Arc<Semaphore>,
    entered: Arc<Semaphore>,
    sent: Arc<Mutex<Vec<samsung_tv_remote::domain::MacAddress>>>,
}
impl samsung_tv_remote::application::wake_transport::WakeTransport for GatedWake {
    fn send_once(
        &self,
        _: TvHost,
        mac: samsung_tv_remote::domain::MacAddress,
        permit: samsung_tv_remote::application::wake_transport::WakePermit,
    ) -> samsung_tv_remote::application::wake_transport::WakeFuture {
        let (gate, entered, sent) = (self.gate.clone(), self.entered.clone(), self.sent.clone());
        Box::pin(async move {
            entered.add_permits(1);
            gate.acquire().await.unwrap().forget();
            permit.send_if_current(|| {
                sent.lock().unwrap().push(mac);
                Ok(())
            })
        })
    }
}
#[tokio::test(start_paused = true)]
async fn wake_captures_configuration_and_edits_do_not_cancel_or_replace_its_packet() {
    let setup = Arc::new(FakeSetup::default());
    let old: samsung_tv_remote::domain::MacAddress = "02:11:22:33:44:55".parse().unwrap();
    let new = "02:22:33:44:55:66".parse().unwrap();
    let configuration = |mac| WakeConfiguration {
        wired: Some(mac),
        wifi: None,
        active: Some(samsung_tv_remote::application::wake::WakeInterface::Wired),
    };
    let mut tv = device(1);
    tv.wake = configuration(old);
    setup.profile.lock().unwrap().devices = vec![tv];
    setup.profile.lock().unwrap().selected = Some(DeviceId::new(1));
    let gate = Arc::new(Semaphore::new(0));
    let entered = Arc::new(Semaphore::new(0));
    let sent = Arc::new(Mutex::new(Vec::new()));
    let mut adapters = services(setup);
    adapters.gateway = Some(Arc::new(OfflineGateway));
    adapters.wake = Some(Arc::new(GatedWake {
        gate: gate.clone(),
        entered: entered.clone(),
        sent: sent.clone(),
    }));
    let runtime = Runtime::start(adapters, RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    service
        .submit(request(
            &snapshot,
            "wake",
            Command::Wake {
                target: DeviceId::new(1),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    entered.acquire().await.unwrap().forget();
    let stale_cancel = service
        .submit(request(
            &snapshot,
            "stale-cancel",
            Command::CancelWake {
                operation: OperationId(u64::MAX),
                target: DeviceId::new(1),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        stale_cancel.outcome,
        Some(Outcome::Rejected(Rejection::MissingOperation))
    ));
    service
        .submit(request(
            &snapshot,
            "edit",
            Command::SetWake {
                target: DeviceId::new(1),
                generation: 0,
                revision: 0,
                configuration: configuration(new),
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        terminal(&service, &snapshot, "edit").await.outcome,
        Some(Outcome::Saved(_))
    ));
    assert_eq!(
        service.snapshot().await.unwrap().wake,
        samsung_tv_remote::application::tv_control_coordinator::WakeStage::PacketSending
    );
    gate.add_permits(1);
    let mut events = service.subscribe().await.unwrap();
    while sent.lock().unwrap().is_empty() {
        events.events.recv().await.unwrap();
    }
    assert_eq!(*sent.lock().unwrap(), vec![old]);
    service
        .submit(request(
            &snapshot,
            "cancel",
            Command::CancelWake {
                operation: service.snapshot().await.unwrap().wake_operation.unwrap(),
                target: DeviceId::new(1),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        terminal(&service, &snapshot, "wake").await.outcome,
        Some(Outcome::Wake(
            samsung_tv_remote::application::tv_control_coordinator::WakeStage::CancelledAfterSend
        ))
    ));
    runtime.drain().await;
}

#[tokio::test(start_paused = true)]
async fn wake_cancel_before_final_send_and_send_deadline_revoke_the_packet_permit() {
    for deadline in [false, true] {
        let setup = Arc::new(FakeSetup::default());
        let mut tv = device(1);
        tv.wake = WakeConfiguration {
            wired: Some("02:11:22:33:44:55".parse().unwrap()),
            wifi: None,
            active: Some(samsung_tv_remote::application::wake::WakeInterface::Wired),
        };
        setup.profile.lock().unwrap().devices = vec![tv];
        setup.profile.lock().unwrap().selected = Some(DeviceId::new(1));
        let gate = Arc::new(Semaphore::new(0));
        let entered = Arc::new(Semaphore::new(0));
        let sent = Arc::new(Mutex::new(Vec::new()));
        let mut adapters = services(setup);
        adapters.gateway = Some(Arc::new(OfflineGateway));
        adapters.wake = Some(Arc::new(GatedWake {
            gate: gate.clone(),
            entered: entered.clone(),
            sent: sent.clone(),
        }));
        let runtime = Runtime::start(adapters, RuntimeLimits::default());
        let service = runtime.client(ClientId(1));
        let snapshot = ready(&service).await;
        service
            .submit(request(
                &snapshot,
                "wake",
                Command::Wake {
                    target: DeviceId::new(1),
                    generation: 0,
                },
            ))
            .await
            .unwrap();
        entered.acquire().await.unwrap().forget();
        if deadline {
            tokio::time::advance(Duration::from_secs(11)).await;
        } else {
            service
                .submit(request(
                    &snapshot,
                    "cancel",
                    Command::CancelWake {
                        operation: service.snapshot().await.unwrap().wake_operation.unwrap(),
                        target: DeviceId::new(1),
                        generation: 0,
                    },
                ))
                .await
                .unwrap();
        }
        assert!(matches!(terminal(&service, &snapshot, "wake").await.outcome, Some(Outcome::Wake(samsung_tv_remote::application::tv_control_coordinator::WakeStage::CancelledDuringSend))));
        gate.add_permits(1);
        runtime.drain().await.settled().await.unwrap();
        assert!(sent.lock().unwrap().is_empty());
    }
}

#[tokio::test(start_paused = true)]
async fn client_candidates_expire_without_extending_retained_discovery_results() {
    let setup = Arc::new(FakeSetup::default());
    let mut adapters = services(setup);
    let gate = Arc::new(Semaphore::new(1));
    adapters.discovery = Some(Arc::new(FakeDiscovery {
        gate,
        calls: Arc::new(Default::default()),
    }));
    let runtime = Runtime::start(adapters, RuntimeLimits::default());
    let one = runtime.client(ClientId(1));
    let two = runtime.client(ClientId(2));
    let snapshot = ready(&one).await;
    one.submit(request(&snapshot, "discover", Command::Discover))
        .await
        .unwrap();
    let Some(Outcome::Discovered(candidates)) = terminal(&one, &snapshot, "discover").await.outcome
    else {
        panic!("discovery failed")
    };
    let candidate = &candidates[0];
    let command = Command::ProbeCandidate {
        candidate: candidate.id,
        host: candidate.host.clone(),
    };
    assert!(matches!(
        two.submit(request(&snapshot, "foreign", command.clone()))
            .await
            .unwrap()
            .outcome,
        Some(Outcome::Rejected(Rejection::InvalidObservation))
    ));
    tokio::time::advance(Duration::from_secs(120)).await;
    assert!(matches!(
        one.submit(request(&snapshot, "expired", command))
            .await
            .unwrap()
            .outcome,
        Some(Outcome::Rejected(Rejection::InvalidObservation))
    ));
    assert!(matches!(
        one.lookup(snapshot.epoch, "discover".into()).await.unwrap(),
        Lookup::Known(_)
    ));
    runtime.drain().await.settled().await.unwrap();
}

#[tokio::test]
async fn completions_and_queries_progress_while_client_ingress_is_saturated() {
    let setup = Arc::new(FakeSetup::default());
    let mut adapters = services(setup);
    let gate = Arc::new(Semaphore::new(0));
    adapters.discovery = Some(Arc::new(FakeDiscovery {
        gate: gate.clone(),
        calls: Arc::new(Default::default()),
    }));
    let runtime = Runtime::start(adapters, RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    service
        .submit(request(&snapshot, "discover", Command::Discover))
        .await
        .unwrap();
    let mut load = tokio::task::JoinSet::new();
    for _ in 0..128 {
        let client = service.clone();
        load.spawn(async move {
            for _ in 0..16 {
                match client.snapshot().await {
                    Ok(_) | Err(ServiceError::Busy) => {}
                    error => panic!("unexpected pressure response: {error:?}"),
                };
                tokio::task::yield_now().await;
            }
        });
    }
    gate.add_permits(1);
    let receipt = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(Lookup::Known(receipt)) =
                service.lookup(snapshot.epoch, "discover".into()).await
            {
                if receipt.outcome.is_some() {
                    break receipt;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(receipt.outcome, Some(Outcome::Discovered(_))));
    while let Some(result) = load.join_next().await {
        result.unwrap();
    }
    runtime.drain().await.settled().await.unwrap();
}

#[tokio::test]
async fn ambiguous_selection_and_configuration_failures_reconcile_actual_durable_facts() {
    let setup = Arc::new(FakeSetup::default());
    {
        let mut profile = setup.profile.lock().unwrap();
        profile.devices = vec![device(1), device(2)];
        profile.selected = Some(DeviceId::new(1));
        profile.fail_after_write = true;
    }
    let runtime = Runtime::start(services(setup.clone()), RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    service
        .submit(request(
            &snapshot,
            "select",
            Command::Select {
                target: DeviceId::new(2),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        terminal(&service, &snapshot, "select").await.outcome,
        Some(Outcome::StorageFailed(RepositoryError::Unavailable))
    ));
    let selected = service.snapshot().await.unwrap();
    assert_eq!(selected.control.selected_device(), Some(DeviceId::new(2)));
    assert_eq!(selected.control.selection_generation(), 1);
    assert!(!selected.storage_pending);
    let config = WakeConfiguration {
        wired: Some("02:11:22:33:44:55".parse().unwrap()),
        wifi: None,
        active: Some(samsung_tv_remote::application::wake::WakeInterface::Wired),
    };
    service
        .submit(request(
            &selected,
            "config",
            Command::SetWake {
                target: DeviceId::new(2),
                generation: 1,
                revision: 0,
                configuration: config,
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        terminal(&service, &selected, "config").await.outcome,
        Some(Outcome::StorageFailed(RepositoryError::Unavailable))
    ));
    let after = service.snapshot().await.unwrap();
    assert_eq!(
        after
            .devices
            .iter()
            .find(|d| d.id == DeviceId::new(2))
            .unwrap()
            .wake,
        config
    );
    assert!(after
        .configuration_revisions
        .contains(&(DeviceId::new(2), 1)));
    assert!(!after.storage_pending);
    runtime.drain().await.settled().await.unwrap();
}

#[tokio::test]
async fn per_client_retention_quota_deduplicates_and_leaves_space_for_another_identity() {
    let runtime = Runtime::start(
        services(Arc::new(FakeSetup::default())),
        RuntimeLimits::default(),
    );
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    let first = service
        .submit(request(&snapshot, "0", Command::Discover))
        .await
        .unwrap();
    for n in 1..8192 {
        service
            .submit(request(&snapshot, &n.to_string(), Command::Discover))
            .await
            .unwrap();
    }
    assert_eq!(
        service
            .submit(request(&snapshot, "over-quota", Command::Discover))
            .await
            .unwrap_err(),
        ServiceError::ClientLimit
    );
    // A new handle for the same stable logical identity models credential refresh.
    let refreshed = runtime.client(ClientId(1));
    assert_eq!(
        refreshed
            .submit(request(&snapshot, "0", Command::Discover))
            .await
            .unwrap()
            .id,
        first.id
    );
    assert!(runtime
        .client(ClientId(2))
        .submit(request(&snapshot, "new-client", Command::Discover))
        .await
        .is_ok());
    runtime.drain().await.settled().await.unwrap();
}

struct GatedGateway {
    gate: Arc<Semaphore>,
    entered: Arc<Semaphore>,
}
impl TvGateway for GatedGateway {
    fn probe(&self, _: TvHost) -> SessionFuture<'static, Result<ProbeObservation, TvSessionError>> {
        Box::pin(async { Err(TvSessionError::Offline) })
    }
    fn connect(
        &self,
        _: TvHost,
        _: CertificatePin,
        _: Option<PairingToken>,
    ) -> SessionFuture<'static, Result<Box<dyn TvConnection>, TvSessionError>> {
        let (gate, entered) = (self.gate.clone(), self.entered.clone());
        Box::pin(async move {
            entered.add_permits(1);
            gate.acquire().await.unwrap().forget();
            Ok(Box::new(FakeConnection {
                writes: Arc::new(Mutex::new(Vec::new())),
                uncertain: Arc::new(Default::default()),
                hold_writes: false,
            }) as Box<dyn TvConnection>)
        })
    }
}
#[tokio::test]
async fn prepared_connection_cannot_install_inside_an_uncommitted_selection_fence() {
    let setup = Arc::new(FakeSetup::default());
    {
        let mut profile = setup.profile.lock().unwrap();
        profile.devices = vec![device(1)];
        profile.selected = Some(DeviceId::new(1));
    }
    let gate = Arc::new(Semaphore::new(0));
    let entered = Arc::new(Semaphore::new(0));
    let mut adapters = services(setup.clone());
    adapters.gateway = Some(Arc::new(GatedGateway {
        gate: gate.clone(),
        entered: entered.clone(),
    }));
    let runtime = Runtime::start(adapters, RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    service
        .submit(request(
            &snapshot,
            "connect",
            Command::Connect {
                target: DeviceId::new(1),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    entered.acquire().await.unwrap().forget();
    let (release, storage_gate) = std::sync::mpsc::channel();
    *setup.gate.lock().unwrap() = Some(storage_gate);
    service
        .submit(request(
            &snapshot,
            "reselect",
            Command::Select {
                target: DeviceId::new(1),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    gate.add_permits(1);
    assert!(matches!(
        terminal(&service, &snapshot, "connect").await.outcome,
        Some(Outcome::ConnectFailed(_))
    ));
    assert_ne!(
        service.snapshot().await.unwrap().control.connection_state(),
        samsung_tv_remote::application::ConnectionState::Ready
    );
    release.send(()).unwrap();
    assert!(matches!(
        terminal(&service, &snapshot, "reselect").await.outcome,
        Some(Outcome::Saved(_))
    ));
    assert_eq!(
        service
            .snapshot()
            .await
            .unwrap()
            .control
            .selection_generation(),
        1
    );
    runtime.drain().await.settled().await.unwrap();
}

#[tokio::test]
async fn cancelled_connection_during_configuration_commit_clears_connecting_projection() {
    let setup = Arc::new(FakeSetup::default());
    setup.profile.lock().unwrap().devices = vec![device(1)];
    setup.profile.lock().unwrap().selected = Some(DeviceId::new(1));
    let gate = Arc::new(Semaphore::new(0));
    let entered = Arc::new(Semaphore::new(0));
    let mut adapters = services(setup.clone());
    adapters.gateway = Some(Arc::new(GatedGateway {
        gate: gate.clone(),
        entered: entered.clone(),
    }));
    let runtime = Runtime::start(adapters, RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    service
        .submit(request(
            &snapshot,
            "connect",
            Command::Connect {
                target: DeviceId::new(1),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    entered.acquire().await.unwrap().forget();
    let (release, wait) = std::sync::mpsc::channel();
    *setup.restore_gate.lock().unwrap() = Some(wait);
    service
        .submit(request(
            &snapshot,
            "config",
            Command::SetWake {
                target: DeviceId::new(1),
                generation: 0,
                revision: 0,
                configuration: WakeConfiguration::default(),
            },
        ))
        .await
        .unwrap();
    gate.add_permits(1);
    let result = terminal(&service, &snapshot, "connect").await;
    let state = service.snapshot().await.unwrap().control.connection_state();
    release.send(()).unwrap();
    terminal(&service, &snapshot, "config").await;
    runtime.drain().await.settled().await.unwrap();
    assert!(matches!(result.outcome, Some(Outcome::ConnectFailed(_))));
    assert_eq!(
        state,
        samsung_tv_remote::application::ConnectionState::NotConnected
    );
}

#[tokio::test(start_paused = true)]
async fn connection_storage_deadline_clears_connecting_before_worker_returns() {
    let setup = Arc::new(FakeSetup::default());
    setup.profile.lock().unwrap().devices = vec![device(1)];
    setup.profile.lock().unwrap().selected = Some(DeviceId::new(1));
    let runtime = Runtime::start(services(setup.clone()), RuntimeLimits::default());
    let service = runtime.client(ClientId(1));
    let snapshot = ready(&service).await;
    let (release, wait) = std::sync::mpsc::channel();
    *setup.reconnect_gate.lock().unwrap() = Some(wait);
    service
        .submit(request(
            &snapshot,
            "connect",
            Command::Connect {
                target: DeviceId::new(1),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    tokio::time::advance(Duration::from_secs(10)).await;
    let result = terminal(&service, &snapshot, "connect").await;
    let timed_out = service.snapshot().await.unwrap();
    release.send(()).unwrap();
    while service.snapshot().await.unwrap().storage_pending {
        tokio::task::yield_now().await;
    }
    let late = service.snapshot().await.unwrap();
    runtime.drain().await.settled().await.unwrap();
    assert!(matches!(result.outcome, Some(Outcome::Deadline)));
    assert!(timed_out.storage_pending);
    assert_eq!(
        timed_out.control.connection_state(),
        samsung_tv_remote::application::ConnectionState::Failed
    );
    assert_eq!(
        late.control.connection_state(),
        samsung_tv_remote::application::ConnectionState::Failed
    );
}

#[tokio::test]
async fn successful_preference_writes_still_validate_the_authoritative_profile() {
    for selection in [false, true] {
        let setup = Arc::new(FakeSetup::default());
        setup.profile.lock().unwrap().devices = vec![device(1)];
        setup.profile.lock().unwrap().selected = Some(DeviceId::new(1));
        let runtime = Runtime::start(services(setup.clone()), RuntimeLimits::default());
        let service = runtime.client(ClientId(1));
        let snapshot = ready(&service).await;
        setup.profile.lock().unwrap().corrupt_after_write = true;
        let command = if selection {
            Command::Select {
                target: DeviceId::new(1),
                generation: 0,
            }
        } else {
            Command::SetWake {
                target: DeviceId::new(1),
                generation: 0,
                revision: 0,
                configuration: WakeConfiguration::default(),
            }
        };
        service
            .submit(request(&snapshot, "write", command))
            .await
            .unwrap();
        terminal(&service, &snapshot, "write").await;
        let state = service.snapshot().await.unwrap();
        runtime.drain().await.settled().await.unwrap();
        assert_eq!(
            state.startup,
            StartupState::Blocked(RepositoryError::Corrupt)
        );
        assert!(!state.storage_pending);
    }
}

#[tokio::test]
async fn cancelling_wake_during_reconnect_clears_connecting_projection() {
    let gate = Arc::new(Semaphore::new(0));
    let entered = Arc::new(Semaphore::new(0));
    let (runtime, service, snapshot) = power_fixture(
        Arc::new(GatedGateway {
            gate: gate.clone(),
            entered: entered.clone(),
        }),
        Arc::new(CountingWake::default()),
    )
    .await;
    let wake = service
        .submit(request(
            &snapshot,
            "wake",
            Command::Wake {
                target: DeviceId::new(1),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    entered.acquire().await.unwrap().forget();
    let cancelled = service
        .submit(request(
            &snapshot,
            "cancel",
            Command::CancelWake {
                operation: wake.id,
                target: DeviceId::new(1),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    let state = service.snapshot().await.unwrap();
    gate.add_permits(1);
    runtime.drain().await.settled().await.unwrap();
    assert!(matches!(
        cancelled.outcome,
        Some(Outcome::Wake(
            samsung_tv_remote::application::tv_control_coordinator::WakeStage::CancelledAfterSend
        ))
    ));
    assert_eq!(state.wake_operation, None);
    assert_eq!(
        state.control.connection_state(),
        samsung_tv_remote::application::ConnectionState::NotConnected
    );
}

struct PanickingGateway;
impl TvGateway for PanickingGateway {
    fn probe(&self, _: TvHost) -> SessionFuture<'static, Result<ProbeObservation, TvSessionError>> {
        Box::pin(async { panic!("unexpected probe") })
    }
    fn connect(
        &self,
        _: TvHost,
        _: CertificatePin,
        _: Option<PairingToken>,
    ) -> SessionFuture<'static, Result<Box<dyn TvConnection>, TvSessionError>> {
        Box::pin(async { panic!("synthetic connection panic") })
    }
}

#[tokio::test]
async fn connection_supervisor_failure_clears_connecting_projection() {
    let (runtime, service, snapshot) = power_fixture(
        Arc::new(PanickingGateway),
        Arc::new(CountingWake::default()),
    )
    .await;
    service
        .submit(request(
            &snapshot,
            "connect",
            Command::Connect {
                target: DeviceId::new(1),
                generation: 0,
            },
        ))
        .await
        .unwrap();
    let result = terminal(&service, &snapshot, "connect").await;
    let state = service.snapshot().await.unwrap();
    runtime.drain().await.settled().await.unwrap();
    assert!(matches!(result.outcome, Some(Outcome::SupervisorFailed)));
    assert_eq!(
        state.control.connection_state(),
        samsung_tv_remote::application::ConnectionState::Failed
    );
    assert!(!state.storage_pending);
}

#[tokio::test(start_paused = true)]
async fn private_snapshot_references_and_terminal_display_expiry_are_stable() {
    let runtime = Runtime::start(
        services(Arc::new(FakeSetup::default())),
        RuntimeLimits {
            retention: Duration::from_secs(30),
            ..RuntimeLimits::default()
        },
    );
    let alice = runtime.client(ClientId(1));
    let bob = runtime.client(ClientId(2));
    let snapshot = ready(&alice).await;
    let request = request(&snapshot, "private", Command::Discover);
    let receipt = alice.submit(request.clone()).await.unwrap();
    assert!(receipt.outcome.is_some());
    assert!(receipt.expires_at.is_some());
    let state = alice.client_snapshot().await.unwrap();
    assert_eq!(state.recent_operations, vec![receipt.id]);
    let subscription = alice.subscribe().await.unwrap();
    assert_eq!(subscription.recent_operations, vec![receipt.id]);
    assert_eq!(subscription.snapshot.revision, state.snapshot.revision);
    assert!(bob
        .client_snapshot()
        .await
        .unwrap()
        .recent_operations
        .is_empty());
    assert!(bob.subscribe().await.unwrap().recent_operations.is_empty());
    assert!(matches!(
        bob.operation(snapshot.epoch, receipt.id).await.unwrap(),
        Lookup::OutcomeUnavailable
    ));
    tokio::time::advance(Duration::from_secs(29)).await;
    assert_eq!(
        alice.submit(request).await.unwrap().expires_at,
        receipt.expires_at
    );
    match alice.operation(snapshot.epoch, receipt.id).await.unwrap() {
        Lookup::Known(lookup) => assert_eq!(lookup.expires_at, receipt.expires_at),
        _ => panic!("retained result"),
    }
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(alice
        .client_snapshot()
        .await
        .unwrap()
        .recent_operations
        .is_empty());
    runtime.drain().await.settled().await.unwrap();
}

#[tokio::test]
async fn revocation_between_middleware_and_owner_prevents_admission() {
    use samsung_tv_remote::application::control_service::AdmissionAuthorization;
    let setup = Arc::new(FakeSetup::default());
    let runtime = Runtime::start(services(setup.clone()), RuntimeLimits::default());
    let client = runtime.client(ClientId(1));
    let snapshot = ready(&client).await;
    let authorization = AdmissionAuthorization::default();
    // Represent middleware success, followed by rotation/revocation before the
    // queued message can reach the owner. No credential is carried to it.
    assert!(authorization.is_active());
    authorization.revoke();
    assert!(matches!(
        client
            .submit_authorized(
                request(&snapshot, "revoked", Command::Discover),
                authorization
            )
            .await,
        Err(ServiceError::Unauthorized)
    ));
    assert!(matches!(
        client
            .lookup(snapshot.epoch, "revoked".into())
            .await
            .unwrap(),
        Lookup::OutcomeUnavailable
    ));
    assert!(client
        .client_snapshot()
        .await
        .unwrap()
        .recent_operations
        .is_empty());
    assert_eq!(setup.profile.lock().unwrap().writes, 0);
    runtime.drain().await.settled().await.unwrap();
}
