//! Real private temporary preference/journal files; fake secrets and transport.
//! No Keychain prompts, LAN access, Iced loop, or physical TV are involved.
use samsung_tv_remote::application::certificate_trust::*;
use samsung_tv_remote::application::control_service::*;
use samsung_tv_remote::application::credential_store::*;
use samsung_tv_remote::application::device_repository::*;
use samsung_tv_remote::application::recovery::*;
use samsung_tv_remote::application::runtime::{Runtime, RuntimeLimits};
use samsung_tv_remote::application::tv_address::TvHost;
use samsung_tv_remote::application::tv_control_coordinator::AppServices;
use samsung_tv_remote::application::tv_session::*;
use samsung_tv_remote::application::tv_setup_service::*;
use samsung_tv_remote::application::wake::WakeConfiguration;
use samsung_tv_remote::infrastructure::preferences::*;
use samsung_tv_remote::DeviceId;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Semaphore;

#[derive(Clone, Default)]
struct Secrets(Arc<Mutex<HashMap<DeviceId, String>>>);
impl CredentialStore for Secrets {
    fn load(&self, id: DeviceId) -> Result<Option<PairingToken>, SecretError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .and_then(PairingToken::from_stored))
    }
    fn save(&self, id: DeviceId, token: &PairingToken) -> Result<(), SecretError> {
        self.0.lock().unwrap().insert(id, token.as_str().into());
        Ok(())
    }
    fn delete(&self, id: DeviceId) -> Result<(), SecretError> {
        self.0.lock().unwrap().remove(&id);
        Ok(())
    }
}
struct Gateway {
    gate: Arc<Semaphore>,
    entered: Arc<Semaphore>,
    calls: Arc<AtomicUsize>,
    rotate: bool,
    failure: Option<TvSessionError>,
}
struct Connection {
    gate: Arc<Semaphore>,
    entered: Arc<Semaphore>,
    rotate: bool,
}
struct Control;
impl TvSessionControl for Control {
    fn try_click(
        &self,
        _: samsung_tv_remote::application::remote_dispatcher::RequestId,
        _: samsung_tv_remote::RemoteAction,
    ) -> bool {
        true
    }
    fn abort(&mut self) {}
}
impl TvConnection for Connection {
    fn await_authorized(
        &mut self,
        reconnect: bool,
    ) -> SessionFuture<'_, Result<Option<PairingToken>, TvSessionError>> {
        Box::pin(async move {
            self.entered.add_permits(1);
            self.gate.acquire().await.unwrap().forget();
            Ok((!reconnect || self.rotate)
                .then(|| PairingToken::from_stored("synthetic-new-token".into()).unwrap()))
        })
    }
    fn start(self: Box<Self>) -> ActiveTvSession {
        ActiveTvSession {
            control: Box::new(Control),
            events: Box::pin(futures_util::stream::pending()),
        }
    }
}
impl TvGateway for Gateway {
    fn probe(&self, _: TvHost) -> SessionFuture<'static, Result<ProbeObservation, TvSessionError>> {
        Box::pin(async {
            Ok(ProbeObservation {
                pin: pin(),
                name: Some("Fixture TV".into()),
                model: Some("Fake".into()),
            })
        })
    }
    fn connect(
        &self,
        _: TvHost,
        _: CertificatePin,
        _: Option<PairingToken>,
    ) -> SessionFuture<'static, Result<Box<dyn TvConnection>, TvSessionError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let (gate, entered, rotate, failure) = (
            self.gate.clone(),
            self.entered.clone(),
            self.rotate,
            self.failure,
        );
        Box::pin(async move {
            if let Some(error) = failure {
                return Err(error);
            }
            Ok(Box::new(Connection {
                gate,
                entered,
                rotate,
            }) as Box<dyn TvConnection>)
        })
    }
}
fn pin() -> CertificatePin {
    CertificatePin::from_der(b"synthetic certificate")
}
fn host() -> TvHost {
    TvHost::parse("fixture.local").unwrap()
}
fn saved(id: DeviceId) -> SavedDevice {
    SavedDevice {
        id,
        label: "Prior TV".into(),
        host: host(),
        wake: WakeConfiguration::default(),
    }
}
fn initialize(path: &Path, secrets: &Secrets) {
    let repo = LocalDeviceRepository::new(path);
    for id in [DeviceId::new(1), DeviceId::new(2)] {
        repo.save(&saved(id)).unwrap();
        LocalCertificateTrustStore::new(path)
            .save(
                id,
                &TrustRecord {
                    host: host(),
                    pin: pin(),
                },
            )
            .unwrap();
        secrets
            .save(
                id,
                &PairingToken::from_stored("synthetic-old-token".into()).unwrap(),
            )
            .unwrap();
    }
    repo.select(Some(DeviceId::new(1))).unwrap();
}
fn setup(
    path: &Path,
    secrets: &Secrets,
    journal: Arc<dyn RecoveryJournal>,
) -> Arc<dyn TvSetupPort> {
    Arc::new(
        TvSetupService::new(
            LocalDeviceRepository::new(path),
            secrets.clone(),
            LocalCertificateTrustStore::new(path),
        )
        .with_journal(journal),
    )
}
struct Harness {
    runtime: Runtime,
    service: ControlService,
    gate: Arc<Semaphore>,
    entered: Arc<Semaphore>,
    calls: Arc<AtomicUsize>,
}
impl Harness {
    fn new(path: &Path, secrets: &Secrets, rotate: bool) -> Self {
        Self::with_setup(
            setup(path, secrets, Arc::new(LocalRecoveryJournal::new(path))),
            rotate,
        )
    }
    fn with_setup(setup: Arc<dyn TvSetupPort>, rotate: bool) -> Self {
        let gate = Arc::new(Semaphore::new(0));
        let entered = Arc::new(Semaphore::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = Runtime::start(
            AppServices {
                setup: Some(setup),
                gateway: Some(Arc::new(Gateway {
                    gate: gate.clone(),
                    entered: entered.clone(),
                    calls: calls.clone(),
                    rotate,
                    failure: None,
                })),
                discovery: None,
                wake: None,
            },
            RuntimeLimits::default(),
        );
        let service = runtime.client(ClientId(1));
        Self {
            runtime,
            service,
            gate,
            entered,
            calls,
        }
    }
}
async fn restored(service: &ControlService) -> Snapshot {
    let mut sub = service.subscribe().await.unwrap();
    while sub.snapshot.startup == StartupState::Restoring {
        sub.snapshot = sub.events.recv().await.unwrap().snapshot;
    }
    sub.snapshot
}
async fn submit(service: &ControlService, key: &str, command: Command) -> Receipt {
    let snapshot = service.snapshot().await.unwrap();
    service
        .submit(Request {
            epoch: snapshot.epoch,
            key: key.into(),
            command,
        })
        .await
        .unwrap()
}
async fn terminal(service: &ControlService, id: OperationId) -> Receipt {
    let mut sub = service.subscribe().await.unwrap();
    loop {
        let Lookup::Known(receipt) = service.operation(sub.snapshot.epoch, id).await.unwrap()
        else {
            panic!("missing result")
        };
        if receipt.outcome.is_some() {
            return receipt;
        }
        sub.events.recv().await.unwrap();
    }
}
async fn probe(service: &ControlService, key: &str, host: TvHost) -> Observation {
    let receipt = submit(service, key, Command::Probe(host)).await;
    let Some(Outcome::Probed(observed)) = terminal(service, receipt.id).await.outcome else {
        panic!("probe failed")
    };
    *observed
}
fn pair(snapshot: &Snapshot, observation: &Observation, replace: Option<DeviceId>) -> Command {
    Command::Pair {
        observation: observation.id,
        host: observation.host.clone(),
        pin: observation.observation.pin,
        label: "Approved TV".into(),
        replace,
        generation: snapshot.control.selection_generation(),
        recovery: snapshot.pairing_recovery.map(|m| m.transaction),
    }
}

#[tokio::test]
async fn first_pair_and_repair_commit_before_generation_and_adopt_only_the_committed_session() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = Secrets::default();
    let h = Harness::new(dir.path(), &secrets, false);
    let snapshot = restored(&h.service).await;
    let observation = probe(&h.service, "probe", host()).await;
    let pairing_started = std::time::Instant::now();
    let receipt = submit(&h.service, "pair", pair(&snapshot, &observation, None)).await;
    h.entered.acquire().await.unwrap().forget();
    assert!(LocalDeviceRepository::new(dir.path())
        .list()
        .unwrap()
        .is_empty());
    assert_eq!(
        h.service
            .snapshot()
            .await
            .unwrap()
            .control
            .selection_generation(),
        0
    );
    h.gate.add_permits(1);
    let Some(Outcome::Paired(device)) = terminal(&h.service, receipt.id).await.outcome else {
        panic!("not paired")
    };
    eprintln!(
        "M7 fake-approved first Pairing persistence+owner={}us",
        pairing_started.elapsed().as_micros()
    );
    let paired = h.service.snapshot().await.unwrap();
    assert_eq!(paired.control.selected_device(), Some(device.id));
    assert_eq!(paired.control.selection_generation(), 1);
    assert_eq!(
        paired.control.connection_state(),
        samsung_tv_remote::application::ConnectionState::Ready
    );
    assert!(LocalRecoveryJournal::new(dir.path())
        .pairing()
        .unwrap()
        .is_none());
    let observation = probe(&h.service, "probe-repair", host()).await;
    let receipt = submit(
        &h.service,
        "repair",
        pair(&paired, &observation, Some(device.id)),
    )
    .await;
    h.entered.acquire().await.unwrap().forget();
    h.gate.add_permits(1);
    assert!(matches!(
        terminal(&h.service, receipt.id).await.outcome,
        Some(Outcome::Paired(_))
    ));
    let repaired = h.service.snapshot().await.unwrap();
    assert_eq!(repaired.control.selection_generation(), 2);
    assert_eq!(
        repaired.configuration_revisions.as_slice(),
        &[(device.id, 1)]
    );
    assert!(!dir.path().join("pairing-backup.json").exists());
    h.runtime.drain().await.settled().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn observations_are_client_bound_exact_expiring_and_bounded_independently_of_result_retention(
) {
    let dir = tempfile::tempdir().unwrap();
    let secrets = Secrets::default();
    let h = Harness::new(dir.path(), &secrets, false);
    let snapshot = restored(&h.service).await;
    let observation = probe(&h.service, "probe", host()).await;
    let other = h.runtime.client(ClientId(2));
    let receipt = submit(&other, "foreign", pair(&snapshot, &observation, None)).await;
    assert!(matches!(
        receipt.outcome,
        Some(Outcome::Rejected(Rejection::InvalidObservation))
    ));
    let mut wrong_pin = pair(&snapshot, &observation, None);
    if let Command::Pair { pin, .. } = &mut wrong_pin {
        *pin = CertificatePin::from_der(b"different");
    }
    assert!(matches!(
        submit(&h.service, "wrong-pin", wrong_pin).await.outcome,
        Some(Outcome::Rejected(Rejection::InvalidObservation))
    ));
    for n in 1..8 {
        probe(&h.service, &format!("probe-{n}"), host()).await;
    }
    let full = submit(&h.service, "full", Command::Probe(host())).await;
    assert!(matches!(
        full.outcome,
        Some(Outcome::Rejected(Rejection::Busy))
    ));
    tokio::time::advance(Duration::from_secs(300)).await;
    assert!(matches!(
        submit(&h.service, "expired", pair(&snapshot, &observation, None))
            .await
            .outcome,
        Some(Outcome::Rejected(Rejection::InvalidObservation))
    ));
    assert!(matches!(
        h.service
            .operation(snapshot.epoch, observation.id)
            .await
            .unwrap(),
        Lookup::Known(_)
    ));
    probe(&h.service, "after-expiry", host()).await;
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
    h.runtime.drain().await.settled().await.unwrap();
}

#[tokio::test]
async fn late_approval_and_token_rotation_cannot_mutate_a_new_selection() {
    for rotating in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let secrets = Secrets::default();
        initialize(dir.path(), &secrets);
        let h = Harness::new(dir.path(), &secrets, rotating);
        let snapshot = restored(&h.service).await;
        let command = if rotating {
            Command::Connect {
                target: DeviceId::new(1),
                generation: 0,
            }
        } else {
            let observation = probe(&h.service, "probe", host()).await;
            pair(&snapshot, &observation, Some(DeviceId::new(1)))
        };
        let receipt = submit(&h.service, "prepare", command).await;
        h.entered.acquire().await.unwrap().forget();
        let selected = submit(
            &h.service,
            "select",
            Command::Select {
                target: DeviceId::new(2),
                generation: 0,
            },
        )
        .await;
        assert!(matches!(
            terminal(&h.service, selected.id).await.outcome,
            Some(Outcome::Saved(_))
        ));
        h.gate.add_permits(1);
        assert!(matches!(
            terminal(&h.service, receipt.id).await.outcome,
            Some(Outcome::PairFailed(_) | Outcome::ConnectFailed(_))
        ));
        assert_eq!(
            secrets.0.lock().unwrap()[&DeviceId::new(1)],
            "synthetic-old-token"
        );
        assert!(LocalRecoveryJournal::new(dir.path())
            .pairing()
            .unwrap()
            .is_none());
        assert_eq!(
            h.service
                .snapshot()
                .await
                .unwrap()
                .control
                .selected_device(),
            Some(DeviceId::new(2))
        );
        h.runtime.drain().await.settled().await.unwrap();
    }
}

#[tokio::test]
async fn reconnect_rotation_is_journaled_and_preserves_selected_generation() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = Secrets::default();
    initialize(dir.path(), &secrets);
    let h = Harness::new(dir.path(), &secrets, true);
    restored(&h.service).await;
    let receipt = submit(
        &h.service,
        "connect",
        Command::Connect {
            target: DeviceId::new(1),
            generation: 0,
        },
    )
    .await;
    h.entered.acquire().await.unwrap().forget();
    h.gate.add_permits(1);
    assert!(matches!(
        terminal(&h.service, receipt.id).await.outcome,
        Some(Outcome::Connected)
    ));
    assert_eq!(
        h.service
            .snapshot()
            .await
            .unwrap()
            .control
            .selection_generation(),
        0
    );
    assert_eq!(
        secrets.0.lock().unwrap()[&DeviceId::new(1)],
        "synthetic-new-token"
    );
    assert!(LocalRecoveryJournal::new(dir.path())
        .pairing()
        .unwrap()
        .is_none());
    h.runtime.drain().await.settled().await.unwrap();
}

/// Panic before/after a marker boundary models immediate process loss. There
/// is deliberately no compensation or unwinding cleanup inside the commit.
struct CrashJournal {
    inner: LocalRecoveryJournal,
    stop: usize,
    after: bool,
    calls: AtomicUsize,
}
impl CrashJournal {
    fn boundary(
        &self,
        action: impl FnOnce() -> Result<(), RepositoryError>,
    ) -> Result<(), RepositoryError> {
        let stop = self.calls.fetch_add(1, Ordering::SeqCst) + 1 == self.stop;
        assert!(!(stop && !self.after), "synthetic crash before persistence");
        let result = action();
        assert!(!stop, "synthetic crash after persistence");
        result
    }
}
impl RecoveryJournal for CrashJournal {
    fn load(&self) -> Result<Option<ForgetMarker>, RepositoryError> {
        self.inner.load()
    }
    fn write(&self, marker: ForgetMarker) -> Result<(), RepositoryError> {
        self.inner.write(marker)
    }
    fn clear(&self, transaction: DeviceId) -> Result<(), RepositoryError> {
        self.inner.clear(transaction)
    }
    fn pairing(&self) -> Result<Option<PairingMarker>, RepositoryError> {
        self.inner.pairing()
    }
    fn begin_pairing(
        &self,
        marker: PairingMarker,
        replaces: Option<DeviceId>,
    ) -> Result<(), RepositoryError> {
        self.boundary(|| self.inner.begin_pairing(marker, replaces))
    }
    fn advance_pairing(&self, marker: PairingMarker) -> Result<(), RepositoryError> {
        self.boundary(|| self.inner.advance_pairing(marker))
    }
    fn clear_pairing(&self, transaction: DeviceId) -> Result<(), RepositoryError> {
        self.boundary(|| self.inner.clear_pairing(transaction))
    }
    fn save_backup(&self, backup: &PairingBackup) -> Result<(), RepositoryError> {
        self.inner.save_backup(backup)
    }
    fn load_backup(&self, transaction: DeviceId) -> Result<PairingBackup, RepositoryError> {
        self.inner.load_backup(transaction)
    }
}
fn approved(target: DeviceId, repair: bool) -> ApprovedPairing {
    ApprovedPairing {
        device: SavedDevice {
            label: "New approval".into(),
            ..saved(target)
        },
        pin: CertificatePin::from_der(b"new synthetic certificate"),
        token: PairingToken::from_stored("synthetic-new-token".into()).unwrap(),
        repair,
        recovery: None,
    }
}
#[tokio::test]
async fn every_pairing_and_repair_commit_boundary_blocks_on_restart_and_recovers_explicitly() {
    for repair in [false, true] {
        for stop in 1..=6 {
            for after in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let secrets = Secrets::default();
                initialize(dir.path(), &secrets);
                let target = if repair {
                    DeviceId::new(1)
                } else {
                    DeviceId::new(3)
                };
                let journal = Arc::new(CrashJournal {
                    inner: LocalRecoveryJournal::new(dir.path()),
                    stop,
                    after,
                    calls: AtomicUsize::new(0),
                });
                let crashed = setup(dir.path(), &secrets, journal);
                assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || crashed.commit_approved_pairing(approved(target, repair))
                ))
                .is_err());
                drop(crashed);
                let journal = Arc::new(LocalRecoveryJournal::new(dir.path()));
                let marker = journal.pairing().unwrap();
                let h = Harness::new(dir.path(), &secrets, false);
                let snapshot = restored(&h.service).await;
                assert_eq!(h.calls.load(Ordering::SeqCst), 0);
                if let Some(marker) = marker {
                    assert_eq!(snapshot.startup, StartupState::RecoveryRequired);
                    // Neither a repeat ordinary Pair nor Connect bypasses evidence.
                    assert!(matches!(
                        submit(
                            &h.service,
                            "blocked",
                            Command::Connect {
                                target,
                                generation: 0
                            }
                        )
                        .await
                        .outcome,
                        Some(Outcome::Rejected(Rejection::Blocked))
                    ));
                    if repair {
                        let backup = journal.load_backup(marker.backup.unwrap()).unwrap();
                        assert_eq!(backup.device.label, "Prior TV");
                        assert_eq!(backup.trust.unwrap().pin, pin());
                        let observation = probe(&h.service, "recovery-check", host()).await;
                        let receipt = submit(
                            &h.service,
                            "fresh-repair",
                            pair(&snapshot, &observation, Some(target)),
                        )
                        .await;
                        h.entered.acquire().await.unwrap().forget();
                        h.gate.add_permits(1);
                        assert!(
                            matches!(
                                terminal(&h.service, receipt.id).await.outcome,
                                Some(Outcome::Paired(_))
                            ),
                            "repair boundary {stop}/{after}"
                        );
                    } else {
                        let receipt = submit(
                            &h.service,
                            "cleanup",
                            Command::RecoverPairing {
                                target,
                                transaction: marker.transaction,
                                confirm_device: target,
                            },
                        )
                        .await;
                        assert!(
                            matches!(terminal(&h.service, receipt.id).await.outcome, Some(Outcome::Forgotten(r)) if r.complete())
                        );
                        assert!(!secrets.0.lock().unwrap().contains_key(&target));
                        assert_eq!(
                            LocalDeviceRepository::new(dir.path()).selected().unwrap(),
                            Some(DeviceId::new(1))
                        );
                    }
                    assert_eq!(
                        h.service.snapshot().await.unwrap().startup,
                        StartupState::Ready
                    );
                    assert!(journal.pairing().unwrap().is_none());
                } else {
                    assert_eq!(snapshot.startup, StartupState::Ready);
                    assert!((stop == 1 && !after) || (stop == 6 && after));
                }
                // An unrelated record and credential survive every boundary.
                assert!(LocalDeviceRepository::new(dir.path())
                    .list()
                    .unwrap()
                    .iter()
                    .any(|d| d.id == DeviceId::new(2)));
                assert!(secrets.0.lock().unwrap().contains_key(&DeviceId::new(2)));
                h.runtime.drain().await.settled().await.unwrap();
            }
        }
    }
}

#[test]
fn journal_refuses_wrong_transaction_stage_regression_and_conflicting_forget() {
    let dir = tempfile::tempdir().unwrap();
    let journal = LocalRecoveryJournal::new(dir.path());
    let mut marker = PairingMarker {
        transaction: DeviceId::new(10),
        target: DeviceId::new(1),
        prior_selection: None,
        repair: false,
        backup: None,
        stage: PairingStage::Prepared,
        cleanup: None,
    };
    journal.begin_pairing(marker, None).unwrap();
    assert_eq!(
        journal.write(ForgetMarker::prepared(DeviceId::new(2))),
        Err(RepositoryError::Corrupt)
    );
    assert_eq!(
        journal.clear_pairing(marker.transaction),
        Err(RepositoryError::Corrupt)
    );
    let prepared = marker;
    marker.stage = PairingStage::TokenSaved;
    journal.advance_pairing(marker).unwrap();
    assert_eq!(
        journal.advance_pairing(prepared),
        Err(RepositoryError::Corrupt)
    );
    marker.transaction = DeviceId::new(11);
    assert_eq!(
        journal.advance_pairing(marker),
        Err(RepositoryError::Corrupt)
    );
    let bytes = std::fs::read_to_string(dir.path().join("recovery.json")).unwrap();
    assert!(!bytes.contains("fixture.local") && !bytes.contains("synthetic"));
}

#[tokio::test]
async fn every_first_pair_compensation_boundary_resumes_only_incomplete_steps() {
    for stop in 1..=5 {
        for after in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let secrets = Secrets::default();
            initialize(dir.path(), &secrets);
            let target = DeviceId::new(3);
            let journal = LocalRecoveryJournal::new(dir.path());
            let marker = PairingMarker {
                transaction: DeviceId::new(10),
                target,
                prior_selection: Some(DeviceId::new(1)),
                repair: false,
                backup: None,
                stage: PairingStage::Prepared,
                cleanup: None,
            };
            journal.begin_pairing(marker, None).unwrap();
            LocalDeviceRepository::new(dir.path())
                .save(&saved(target))
                .unwrap();
            LocalDeviceRepository::new(dir.path())
                .select(Some(target))
                .unwrap();
            secrets
                .save(
                    target,
                    &PairingToken::from_stored("synthetic-partial-token".into()).unwrap(),
                )
                .unwrap();
            let crashed = setup(
                dir.path(),
                &secrets,
                Arc::new(CrashJournal {
                    inner: LocalRecoveryJournal::new(dir.path()),
                    stop,
                    after,
                    calls: AtomicUsize::new(0),
                }),
            );
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || crashed.recover_first_pair(marker)
            ))
            .is_err());
            drop(crashed);
            let h = Harness::new(dir.path(), &secrets, false);
            let snapshot = restored(&h.service).await;
            if let Some(marker) = snapshot.pairing_recovery {
                let receipt = submit(
                    &h.service,
                    "recover",
                    Command::RecoverPairing {
                        target,
                        transaction: marker.transaction,
                        confirm_device: target,
                    },
                )
                .await;
                assert!(
                    matches!(terminal(&h.service, receipt.id).await.outcome, Some(Outcome::Forgotten(r)) if r.complete())
                );
            }
            assert_eq!(
                LocalDeviceRepository::new(dir.path()).selected().unwrap(),
                Some(DeviceId::new(1))
            );
            assert_eq!(
                LocalDeviceRepository::new(dir.path()).list().unwrap().len(),
                2
            );
            assert!(!secrets.0.lock().unwrap().contains_key(&target));
            assert!(journal.pairing().unwrap().is_none());
            h.runtime.drain().await.settled().await.unwrap();
        }
    }
}

struct BlockingSecrets {
    inner: Secrets,
    gate: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    entered: Arc<Semaphore>,
}
impl CredentialStore for BlockingSecrets {
    fn load(&self, id: DeviceId) -> Result<Option<PairingToken>, SecretError> {
        self.inner.load(id)
    }
    fn save(&self, id: DeviceId, token: &PairingToken) -> Result<(), SecretError> {
        let gate = self.gate.lock().unwrap().take();
        if let Some(gate) = gate {
            self.entered.add_permits(1);
            gate.recv().unwrap();
        }
        self.inner.save(id, token)
    }
    fn delete(&self, id: DeviceId) -> Result<(), SecretError> {
        self.inner.delete(id)
    }
}
#[tokio::test(start_paused = true)]
async fn timed_out_pair_commit_stays_fenced_through_drain_and_late_success_never_installs_a_session(
) {
    let dir = tempfile::tempdir().unwrap();
    let secrets = Secrets::default();
    let (release, gate) = std::sync::mpsc::channel();
    let entered = Arc::new(Semaphore::new(0));
    let setup = Arc::new(
        TvSetupService::new(
            LocalDeviceRepository::new(dir.path()),
            BlockingSecrets {
                inner: secrets.clone(),
                gate: Mutex::new(Some(gate)),
                entered: entered.clone(),
            },
            LocalCertificateTrustStore::new(dir.path()),
        )
        .with_journal(Arc::new(LocalRecoveryJournal::new(dir.path()))),
    );
    let h = Harness::with_setup(setup, false);
    let snapshot = restored(&h.service).await;
    let observation = probe(&h.service, "probe", host()).await;
    let receipt = submit(&h.service, "pair", pair(&snapshot, &observation, None)).await;
    h.entered.acquire().await.unwrap().forget();
    h.gate.add_permits(1);
    entered.acquire().await.unwrap().forget();
    tokio::time::advance(Duration::from_secs(11)).await;
    assert!(matches!(
        terminal(&h.service, receipt.id).await.outcome,
        Some(Outcome::Deadline)
    ));
    assert!(h.service.snapshot().await.unwrap().storage_pending);
    assert!(matches!(
        submit(&h.service, "competing", Command::Discover)
            .await
            .outcome,
        Some(Outcome::Rejected(Rejection::Busy))
    ));
    let draining = tokio::spawn(h.runtime.drain());
    // Process the lifecycle request before advancing its explicit deadline;
    // blocked spawn_blocking calls suppress Tokio's automatic time advance.
    while h.service.snapshot().await.unwrap().startup != StartupState::Draining {
        tokio::task::yield_now().await;
    }
    tokio::time::advance(Duration::from_secs(6)).await;
    let drain = draining.await.unwrap();
    assert!(drain.status.unresolved_storage);
    assert!(!drain.is_settled());
    release.send(()).unwrap();
    drain.settled().await.unwrap();
    // The late actual commit is recoverable durable data, never a late session.
    assert_eq!(secrets.0.lock().unwrap().len(), 1);
    assert!(LocalRecoveryJournal::new(dir.path())
        .pairing()
        .unwrap()
        .is_none());
    let replacement = Harness::new(dir.path(), &secrets, false);
    let snapshot = restored(&replacement.service).await;
    assert_eq!(snapshot.control.selection_generation(), 0);
    assert_eq!(
        snapshot.control.connection_state(),
        samsung_tv_remote::application::ConnectionState::NotConnected
    );
    assert_eq!(replacement.calls.load(Ordering::SeqCst), 0);
    replacement.runtime.drain().await.settled().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn observation_is_rechecked_after_approval_before_any_store_commit() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = Secrets::default();
    let h = Harness::new(dir.path(), &secrets, false);
    let snapshot = restored(&h.service).await;
    let observation = probe(&h.service, "probe", host()).await;
    tokio::time::advance(Duration::from_secs(299)).await;
    let receipt = submit(&h.service, "pair", pair(&snapshot, &observation, None)).await;
    h.entered.acquire().await.unwrap().forget();
    tokio::time::advance(Duration::from_secs(2)).await;
    h.gate.add_permits(1);
    assert!(matches!(
        terminal(&h.service, receipt.id).await.outcome,
        Some(Outcome::PairFailed(
            samsung_tv_remote::application::tv_control_coordinator::PairFlowError::Cancelled
        ))
    ));
    assert!(secrets.0.lock().unwrap().is_empty());
    assert!(LocalRecoveryJournal::new(dir.path())
        .pairing()
        .unwrap()
        .is_none());
    h.runtime.drain().await.settled().await.unwrap();
}

struct PendingWake {
    entered: Arc<Semaphore>,
    gate: Arc<Semaphore>,
    sends: Arc<AtomicUsize>,
}
impl samsung_tv_remote::application::wake_transport::WakeTransport for PendingWake {
    fn send_once(
        &self,
        _: TvHost,
        _: samsung_tv_remote::domain::MacAddress,
        permit: samsung_tv_remote::application::wake_transport::WakePermit,
    ) -> samsung_tv_remote::application::wake_transport::WakeFuture {
        let (entered, gate, sends) = (self.entered.clone(), self.gate.clone(), self.sends.clone());
        Box::pin(async move {
            entered.add_permits(1);
            gate.acquire().await.unwrap().forget();
            permit.send_if_current(|| {
                sends.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        })
    }
}

#[tokio::test]
async fn pairing_commit_settles_concurrent_wake_and_revokes_pending_packet() {
    use samsung_tv_remote::application::tv_control_coordinator::WakeStage;
    use samsung_tv_remote::application::wake::WakeInterface;
    let dir = tempfile::tempdir().unwrap();
    let secrets = Secrets::default();
    initialize(dir.path(), &secrets);
    let repo = LocalDeviceRepository::new(dir.path());
    let mut device = saved(DeviceId::new(1));
    device.wake = WakeConfiguration {
        wired: Some("02:11:22:33:44:55".parse().unwrap()),
        wifi: None,
        active: Some(WakeInterface::Wired),
    };
    repo.save(&device).unwrap();
    let gate = Arc::new(Semaphore::new(0));
    let entered = Arc::new(Semaphore::new(0));
    let wake_gate = Arc::new(Semaphore::new(0));
    let wake_entered = Arc::new(Semaphore::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let sends = Arc::new(AtomicUsize::new(0));
    let runtime = Runtime::start(
        AppServices {
            setup: Some(setup(
                dir.path(),
                &secrets,
                Arc::new(LocalRecoveryJournal::new(dir.path())),
            )),
            gateway: Some(Arc::new(Gateway {
                gate: gate.clone(),
                entered: entered.clone(),
                calls: calls.clone(),
                rotate: false,
                failure: None,
            })),
            discovery: None,
            wake: Some(Arc::new(PendingWake {
                entered: wake_entered.clone(),
                gate: wake_gate.clone(),
                sends: sends.clone(),
            })),
        },
        RuntimeLimits::default(),
    );
    let service = runtime.client(ClientId(1));
    let snapshot = restored(&service).await;
    let observation = probe(&service, "probe", host()).await;
    let wake = submit(
        &service,
        "wake",
        Command::Wake {
            target: DeviceId::new(1),
            generation: snapshot.control.selection_generation(),
        },
    )
    .await;
    wake_entered.acquire().await.unwrap().forget();
    let repair = submit(
        &service,
        "repair",
        pair(&snapshot, &observation, Some(DeviceId::new(1))),
    )
    .await;
    entered.acquire().await.unwrap().forget();
    gate.add_permits(1);
    assert!(matches!(
        terminal(&service, repair.id).await.outcome,
        Some(Outcome::Paired(_))
    ));
    let after = service.snapshot().await.unwrap();
    let wake_result = service.operation(snapshot.epoch, wake.id).await.unwrap();
    wake_gate.add_permits(1);
    runtime.drain().await.settled().await.unwrap();
    assert!(matches!(
        wake_result,
        Lookup::Known(Receipt {
            outcome: Some(Outcome::Wake(WakeStage::CancelledDuringSend)),
            ..
        })
    ));
    assert_eq!(after.wake_operation, None);
    assert_eq!(
        after.control.connection_state(),
        samsung_tv_remote::application::ConnectionState::Ready
    );
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
