//! Real temporary non-secret files and fake credentials, never Keychain or TV I/O.
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

use samsung_tv_remote::application::certificate_trust::{
    CertificatePin, CertificateTrustStore, TrustRecord,
};
use samsung_tv_remote::application::control_service::*;
use samsung_tv_remote::application::credential_store::{
    CredentialStore, PairingToken, SecretError,
};
use samsung_tv_remote::application::device_repository::{
    DeviceRepository, RepositoryError, SavedDevice,
};
use samsung_tv_remote::application::recovery::{ForgetMarker, RecoveryJournal};
use samsung_tv_remote::application::runtime::{Runtime, RuntimeLimits};
use samsung_tv_remote::application::tv_address::TvHost;
use samsung_tv_remote::application::tv_control_coordinator::AppServices;
use samsung_tv_remote::application::tv_setup_service::{TvSetupPort, TvSetupService};
use samsung_tv_remote::application::wake::WakeConfiguration;
use samsung_tv_remote::infrastructure::preferences::{
    LocalCertificateTrustStore, LocalDeviceRepository, LocalRecoveryJournal,
};
use samsung_tv_remote::DeviceId;

#[derive(Default)]
struct Secrets {
    ids: HashSet<DeviceId>,
    deletes: usize,
    denied: bool,
}
#[derive(Clone, Default)]
struct FakeSecrets(Arc<Mutex<Secrets>>);
impl CredentialStore for FakeSecrets {
    fn load(&self, _: DeviceId) -> Result<Option<PairingToken>, SecretError> {
        panic!("restore and Forget must not load credentials");
    }
    fn save(&self, id: DeviceId, _: &PairingToken) -> Result<(), SecretError> {
        self.0.lock().unwrap().ids.insert(id);
        Ok(())
    }
    fn delete(&self, id: DeviceId) -> Result<(), SecretError> {
        let mut state = self.0.lock().unwrap();
        state.deletes += 1;
        if state.denied {
            return Err(SecretError::Unavailable);
        }
        state.ids.remove(&id);
        Ok(())
    }
}

fn initialize(path: &Path, secrets: &FakeSecrets) {
    let repo = LocalDeviceRepository::new(path);
    let trust = LocalCertificateTrustStore::new(path);
    for id in [DeviceId::new(1), DeviceId::new(2)] {
        let host = TvHost::parse("fixture.local").unwrap();
        repo.save(&SavedDevice {
            sources: Default::default(),
            id,
            host: host.clone(),
            label: "Fixture TV".into(),
            wake: WakeConfiguration::default(),
        })
        .unwrap();
        trust
            .save(
                id,
                &TrustRecord {
                    host,
                    pin: CertificatePin::from_der(b"fake certificate"),
                },
            )
            .unwrap();
        secrets.0.lock().unwrap().ids.insert(id);
    }
    repo.select(Some(DeviceId::new(1))).unwrap();
}
fn setup(
    path: &Path,
    secrets: &FakeSecrets,
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
fn start(setup: Arc<dyn TvSetupPort>) -> (Runtime, ControlService) {
    let runtime = Runtime::start(
        AppServices {
            setup: Some(setup),
            discovery: None,
            gateway: None,
            wake: None,
        },
        RuntimeLimits::default(),
    );
    let client = runtime.client(ClientId(1));
    (runtime, client)
}
async fn restored(client: &ControlService) -> Snapshot {
    let mut subscription = client.subscribe().await.unwrap();
    while subscription.snapshot.startup == StartupState::Restoring {
        subscription.snapshot = subscription.events.recv().await.unwrap().snapshot;
    }
    subscription.snapshot
}
async fn submit(
    client: &ControlService,
    snapshot: &Snapshot,
    key: &str,
    command: Command,
) -> Receipt {
    let mut events = client.subscribe().await.unwrap().events;
    let mut receipt = client
        .submit(Request {
            epoch: snapshot.epoch,
            key: key.into(),
            command,
        })
        .await
        .unwrap();
    while receipt.outcome.is_none() {
        events.recv().await.unwrap();
        let Lookup::Known(updated) = client.lookup(snapshot.epoch, key.into()).await.unwrap()
        else {
            panic!("lost result");
        };
        receipt = updated;
    }
    receipt
}
fn forget(confirm: DeviceId, revision: u64) -> Command {
    Command::Forget {
        target: DeviceId::new(1),
        revision,
        confirm_device: confirm,
    }
}
fn recover(marker: ForgetMarker) -> Command {
    Command::RecoverForget {
        target: marker.target,
        transaction: marker.transaction,
        confirm_device: marker.target,
    }
}

#[tokio::test]
async fn forget_validates_confirmation_and_revision_then_publishes_durable_removal() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = FakeSecrets::default();
    initialize(dir.path(), &secrets);
    let journal = Arc::new(LocalRecoveryJournal::new(dir.path()));
    let (runtime, client) = start(setup(dir.path(), &secrets, journal.clone()));
    let snapshot = restored(&client).await;
    for (key, command, expected) in [
        (
            "confirmation",
            forget(DeviceId::new(2), 0),
            Rejection::InvalidConfirmation,
        ),
        (
            "revision",
            forget(DeviceId::new(1), 1),
            Rejection::StaleConfiguration,
        ),
    ] {
        assert!(
            matches!(submit(&client, &snapshot, key, command).await.outcome, Some(Outcome::Rejected(reason)) if reason == expected)
        );
    }
    assert_eq!(secrets.0.lock().unwrap().deletes, 0);
    assert!(journal.load().unwrap().is_none());
    let result = submit(&client, &snapshot, "forget", forget(DeviceId::new(1), 0)).await;
    assert!(matches!(result.outcome, Some(Outcome::Forgotten(result)) if result.complete()));
    let after = client.snapshot().await.unwrap();
    assert_eq!(after.control.selected_device(), None);
    assert_eq!(after.control.selection_generation(), 1);
    assert_eq!(after.devices.len(), 1);
    assert!(!after.duplicate_records);
    assert_eq!(after.devices[0].id, DeviceId::new(2));
    assert_eq!(*after.configuration_revisions, vec![(DeviceId::new(2), 0)]);
    assert!(journal.load().unwrap().is_none());
    assert_eq!(
        secrets.0.lock().unwrap().ids,
        HashSet::from([DeviceId::new(2)])
    );
    // A known duplicate returns the retained result after the record is gone.
    assert_eq!(
        submit(&client, &snapshot, "forget", forget(DeviceId::new(1), 0))
            .await
            .id,
        result.id
    );
    runtime.drain().await.settled().await.unwrap();
}

#[tokio::test]
async fn partial_forget_blocks_across_restart_and_retries_only_on_explicit_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = FakeSecrets::default();
    initialize(dir.path(), &secrets);
    secrets.0.lock().unwrap().denied = true;
    let journal = Arc::new(LocalRecoveryJournal::new(dir.path()));
    let (runtime, client) = start(setup(dir.path(), &secrets, journal.clone()));
    let snapshot = restored(&client).await;
    assert!(
        matches!(submit(&client, &snapshot, "forget", forget(DeviceId::new(1), 0)).await.outcome, Some(Outcome::Forgotten(result)) if !result.credential_removed && result.trust_removed && result.record_removed)
    );
    let blocked = client.snapshot().await.unwrap();
    assert_eq!(blocked.startup, StartupState::RecoveryRequired);
    assert_eq!(blocked.control.selected_device(), None);
    let marker = blocked.recovery.unwrap();
    runtime.drain().await.settled().await.unwrap();
    let (replacement, client) = start(setup(dir.path(), &secrets, journal.clone()));
    let snapshot = restored(&client).await;
    assert_eq!(snapshot.startup, StartupState::RecoveryRequired);
    assert_eq!(secrets.0.lock().unwrap().deletes, 1);
    assert!(matches!(
        submit(
            &client,
            &snapshot,
            "bypass",
            Command::Select {
                target: DeviceId::new(2),
                generation: 0
            }
        )
        .await
        .outcome,
        Some(Outcome::Rejected(Rejection::Blocked))
    ));
    let mut wrong = marker;
    wrong.transaction = DeviceId::new(1234);
    assert!(matches!(
        submit(&client, &snapshot, "wrong-marker", recover(wrong))
            .await
            .outcome,
        Some(Outcome::Rejected(Rejection::Blocked))
    ));
    secrets.0.lock().unwrap().denied = false;
    assert!(
        matches!(submit(&client, &snapshot, "recover", recover(marker)).await.outcome, Some(Outcome::Forgotten(result)) if result.complete())
    );
    assert_eq!(secrets.0.lock().unwrap().deletes, 2);
    assert_eq!(
        client.snapshot().await.unwrap().startup,
        StartupState::Ready
    );
    assert!(journal.load().unwrap().is_none());
    replacement.drain().await.settled().await.unwrap();
}

/// Stop before/after the Nth durable marker write (1..5), or removal (6).
/// Returning an error stops the flow at the same boundary without running its
/// remaining stages. Fresh owners see only the real durable files.
struct InterruptedJournal {
    inner: LocalRecoveryJournal,
    stop: usize,
    after: bool,
    calls: Mutex<usize>,
}
impl RecoveryJournal for InterruptedJournal {
    fn load(&self) -> Result<Option<ForgetMarker>, RepositoryError> {
        self.inner.load()
    }
    fn write(&self, marker: ForgetMarker) -> Result<(), RepositoryError> {
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        let stop = *calls == self.stop;
        if stop && !self.after {
            return Err(RepositoryError::Unavailable);
        }
        self.inner.write(marker)?;
        if stop {
            return Err(RepositoryError::Unavailable);
        }
        Ok(())
    }
    fn clear(&self, transaction: DeviceId) -> Result<(), RepositoryError> {
        if self.stop == 6 && !self.after {
            return Err(RepositoryError::Unavailable);
        }
        self.inner.clear(transaction)?;
        if self.stop == 6 {
            return Err(RepositoryError::Unavailable);
        }
        Ok(())
    }
}

#[tokio::test]
async fn every_forget_journal_boundary_recovers_without_implicit_credential_access() {
    for stop in 1..=6 {
        for after in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let secrets = FakeSecrets::default();
            initialize(dir.path(), &secrets);
            let interrupted = Arc::new(InterruptedJournal {
                inner: LocalRecoveryJournal::new(dir.path()),
                stop,
                after,
                calls: Mutex::new(0),
            });
            let crashed = setup(dir.path(), &secrets, interrupted);
            assert_eq!(
                crashed.forget_durable(DeviceId::new(1)),
                Err(RepositoryError::Unavailable)
            );
            drop(crashed);
            let deletes = secrets.0.lock().unwrap().deletes;
            let journal = Arc::new(LocalRecoveryJournal::new(dir.path()));
            let marker = journal.load().unwrap();
            let (runtime, client) = start(setup(dir.path(), &secrets, journal.clone()));
            let snapshot = restored(&client).await;
            assert_eq!(
                secrets.0.lock().unwrap().deletes,
                deletes,
                "startup retried at {stop}/{after}"
            );
            if let Some(marker) = marker {
                assert_eq!(snapshot.startup, StartupState::RecoveryRequired);
                assert!(
                    matches!(submit(&client, &snapshot, "recover", recover(marker)).await.outcome, Some(Outcome::Forgotten(result)) if result.complete())
                );
                assert!(journal.load().unwrap().is_none());
            } else {
                assert_eq!(snapshot.startup, StartupState::Ready);
                // Before preparation: no mutation. After removal: all complete.
                assert!((stop == 1 && !after) || (stop == 6 && after));
            }
            let remaining = LocalDeviceRepository::new(dir.path()).list().unwrap();
            assert!(remaining.iter().any(|device| device.id == DeviceId::new(2)));
            if !(stop == 1 && !after) {
                assert!(!remaining.iter().any(|device| device.id == DeviceId::new(1)));
                assert!(!secrets.0.lock().unwrap().ids.contains(&DeviceId::new(1)));
            }
            runtime.drain().await.settled().await.unwrap();
        }
    }
}

#[tokio::test]
async fn corrupt_or_unrecognized_journal_blocks_before_any_credential_access() {
    for bytes in ["not json", r#"{"version":999,"kind":"pair"}"#] {
        let dir = tempfile::tempdir().unwrap();
        let secrets = FakeSecrets::default();
        initialize(dir.path(), &secrets);
        std::fs::write(dir.path().join("recovery.json"), bytes).unwrap();
        let (runtime, client) = start(setup(
            dir.path(),
            &secrets,
            Arc::new(LocalRecoveryJournal::new(dir.path())),
        ));
        assert_eq!(
            restored(&client).await.startup,
            StartupState::Blocked(RepositoryError::Corrupt)
        );
        assert_eq!(secrets.0.lock().unwrap().deletes, 0);
        runtime.drain().await.settled().await.unwrap();
    }
}

#[test]
fn file_journal_rejects_transaction_replacement_regression_and_premature_removal() {
    let dir = tempfile::tempdir().unwrap();
    let journal = LocalRecoveryJournal::new(dir.path());
    let prepared = ForgetMarker::prepared(DeviceId::new(1));
    journal.write(prepared).unwrap();
    assert_eq!(
        journal.clear(prepared.transaction),
        Err(RepositoryError::Corrupt)
    );
    assert_eq!(
        journal.write(ForgetMarker::prepared(DeviceId::new(2))),
        Err(RepositoryError::Corrupt)
    );
    let mut progressed = prepared;
    progressed.selection_cleared = true;
    journal.write(progressed).unwrap();
    assert_eq!(journal.write(prepared), Err(RepositoryError::Corrupt));
    let mut invalid = prepared;
    invalid.credential_removed = true;
    assert_eq!(journal.write(invalid), Err(RepositoryError::Corrupt));
    assert_eq!(journal.load().unwrap(), Some(progressed));
    progressed.credential_removed = true;
    progressed.trust_removed = true;
    progressed.record_removed = true;
    journal.write(progressed).unwrap();
    journal.clear(progressed.transaction).unwrap();
    assert!(journal.load().unwrap().is_none());
    assert_eq!(journal.write(progressed), Err(RepositoryError::Corrupt));
}

#[tokio::test]
async fn forgetting_an_unselected_record_preserves_selection_and_generation() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = FakeSecrets::default();
    initialize(dir.path(), &secrets);
    let journal = Arc::new(LocalRecoveryJournal::new(dir.path()));
    let (runtime, client) = start(setup(dir.path(), &secrets, journal));
    let snapshot = restored(&client).await;
    assert!(
        matches!(submit(&client, &snapshot, "other", Command::Forget { target: DeviceId::new(2), revision: 0, confirm_device: DeviceId::new(2) }).await.outcome, Some(Outcome::Forgotten(result)) if result.complete())
    );
    let after = client.snapshot().await.unwrap();
    assert_eq!(after.control.selected_device(), Some(DeviceId::new(1)));
    assert_eq!(after.control.selection_generation(), 0);
    assert_eq!(
        LocalDeviceRepository::new(dir.path()).selected().unwrap(),
        Some(DeviceId::new(1))
    );
    runtime.drain().await.settled().await.unwrap();
}

#[tokio::test]
async fn adapter_without_journal_support_blocks_instead_of_assuming_no_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = FakeSecrets::default();
    initialize(dir.path(), &secrets);
    let legacy = Arc::new(TvSetupService::new(
        LocalDeviceRepository::new(dir.path()),
        secrets.clone(),
        LocalCertificateTrustStore::new(dir.path()),
    ));
    let (runtime, client) = start(legacy);
    assert_eq!(
        restored(&client).await.startup,
        StartupState::Blocked(RepositoryError::Unavailable)
    );
    assert_eq!(secrets.0.lock().unwrap().deletes, 0);
    runtime.drain().await.settled().await.unwrap();
}
