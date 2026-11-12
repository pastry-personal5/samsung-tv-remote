use samsung_tv_remote::{
    application::{
        control_service::*,
        device_repository::DeviceRepository,
        remote_dispatcher::TerminalOutcome,
        runtime::{Runtime, RuntimeLimits},
        tv_address::TvHost,
        SendRemoteAction,
    },
    infrastructure::preferences::LocalDeviceRepository,
    server::development::DevelopmentProfile,
    RemoteAction,
};
use std::sync::atomic::Ordering;
use std::time::Duration;

async fn ready(service: &ControlService) -> Snapshot {
    let mut events = service.subscribe().await.unwrap();
    if events.snapshot.startup != StartupState::Restoring {
        return events.snapshot;
    }
    loop {
        let snapshot = events.events.recv().await.unwrap().snapshot;
        if snapshot.startup != StartupState::Restoring {
            return snapshot;
        }
    }
}
async fn terminal(service: &ControlService, epoch: Epoch, key: &str, command: Command) -> Receipt {
    service
        .submit(Request {
            epoch,
            key: key.into(),
            command,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut subscription = service.subscribe().await.unwrap();
        loop {
            if let Lookup::Known(receipt) = service.lookup(epoch, key.into()).await.unwrap() {
                if receipt.outcome.is_some() {
                    return receipt;
                }
            }
            subscription.events.recv().await.unwrap();
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn isolated_profile_restarts_with_synthetic_credentials_and_zero_startup_effects() {
    let base = tempfile::tempdir().unwrap();
    let parent = base.path().canonicalize().unwrap();
    std::fs::write(parent.join("devices.json"), b"legacy sentinel").unwrap();
    let (profile, ownership) = DevelopmentProfile::open(&parent).unwrap();
    let runtime = Runtime::start(profile.services(), RuntimeLimits::default());
    let client = runtime.client(ClientId(1));
    let initial = ready(&client).await;
    assert!(initial.devices.is_empty());
    assert!(initial.control.selected_device().is_none());
    assert_eq!(profile.effects.probes.load(Ordering::SeqCst), 0);
    assert_eq!(profile.effects.connections.load(Ordering::SeqCst), 0);
    assert_eq!(profile.effects.discovery.load(Ordering::SeqCst), 0);
    assert_eq!(profile.effects.credential_reads.load(Ordering::SeqCst), 0);
    let observed = terminal(
        &client,
        initial.epoch,
        "probe",
        Command::Probe(TvHost::parse("synthetic-tv.invalid").unwrap()),
    )
    .await;
    let Some(Outcome::Probed(observed)) = observed.outcome else {
        panic!("observation")
    };
    let paired = terminal(
        &client,
        initial.epoch,
        "pair",
        Command::Pair {
            observation: observed.id,
            host: observed.host,
            pin: observed.observation.pin,
            label: "Synthetic TV".into(),
            replace: None,
            generation: 0,
            recovery: None,
        },
    )
    .await;
    let Some(Outcome::Paired(saved)) = paired.outcome else {
        panic!("pairing")
    };
    runtime.drain().await.settled().await.unwrap();
    drop(client);
    drop(profile);
    drop(ownership);
    assert_eq!(
        std::fs::read(parent.join("devices.json")).unwrap(),
        b"legacy sentinel"
    );
    let data = parent.join("m8-fake-v1/data");
    assert!(LocalDeviceRepository::new(&data).list().is_err());
    let alias = parent.join("alias");
    std::os::unix::fs::symlink(&data, &alias).unwrap();
    assert!(LocalDeviceRepository::new(&alias).list().is_err());
    for file in std::fs::read_dir(parent.join("m8-fake-v1/synthetic-credentials")).unwrap() {
        let bytes = std::fs::read(file.unwrap().path()).unwrap();
        assert_eq!(bytes, b"synthetic-presence-v1");
        assert!(!String::from_utf8(bytes)
            .unwrap()
            .contains("m8-synthetic-pairing"));
    }
    // A rename or a copied file must not turn synthetic records into a
    // production-format profile. The document envelope survives both changes.
    let renamed = parent.join("renamed-profile");
    std::fs::rename(parent.join("m8-fake-v1"), &renamed).unwrap();
    assert!(LocalDeviceRepository::new(&renamed.join("data"))
        .list()
        .is_err());
    let copied = parent.join("copied-records");
    std::fs::create_dir(&copied).unwrap();
    std::fs::copy(
        renamed.join("data/devices.json"),
        copied.join("devices.json"),
    )
    .unwrap();
    assert!(LocalDeviceRepository::new(&copied).list().is_err());
    std::fs::rename(&renamed, parent.join("m8-fake-v1")).unwrap();
    let (profile, ownership) = DevelopmentProfile::open(&parent).unwrap();
    let runtime = Runtime::start(profile.services(), RuntimeLimits::default());
    let client = runtime.client(ClientId(1));
    let restored = ready(&client).await;
    assert_ne!(restored.epoch, initial.epoch);
    assert_eq!(restored.devices.as_slice(), std::slice::from_ref(&saved));
    assert_eq!(restored.control.selected_device(), Some(saved.id));
    assert_eq!(restored.control.selection_generation(), 0);
    assert_eq!(profile.effects.connections.load(Ordering::SeqCst), 0);
    assert_eq!(profile.effects.credential_reads.load(Ordering::SeqCst), 0);
    assert!(matches!(
        terminal(
            &client,
            restored.epoch,
            "connect",
            Command::Connect {
                target: saved.id,
                generation: 0
            }
        )
        .await
        .outcome,
        Some(Outcome::Connected)
    ));
    assert!(matches!(
        terminal(
            &client,
            restored.epoch,
            "remote",
            Command::Remote(SendRemoteAction::new(saved.id, 0, RemoteAction::VolumeUp))
        )
        .await
        .outcome,
        Some(Outcome::Remote(TerminalOutcome::Written))
    ));
    assert_eq!(profile.effects.writes.load(Ordering::SeqCst), 1);
    assert!(profile.effects.credential_reads.load(Ordering::SeqCst) > 0);
    runtime.drain().await.settled().await.unwrap();
    drop(ownership);
}
#[tokio::test]
async fn isolated_store_rejects_symlinked_profile_records_without_repair_or_effects() {
    let base = tempfile::tempdir().unwrap();
    let parent = base.path().canonicalize().unwrap();
    let (profile, ownership) = DevelopmentProfile::open(&parent).unwrap();
    drop(profile);
    drop(ownership);
    let outside = parent.join("outside.json");
    std::fs::write(&outside, b"private sentinel").unwrap();
    std::os::unix::fs::symlink(&outside, parent.join("m8-fake-v1/data/devices.json")).unwrap();
    let (profile, ownership) = DevelopmentProfile::open(&parent).unwrap();
    let runtime = Runtime::start(profile.services(), RuntimeLimits::default());
    let state = ready(&runtime.client(ClientId(1))).await;
    assert!(matches!(state.startup, StartupState::Blocked(_)));
    assert_eq!(profile.effects.credential_reads.load(Ordering::SeqCst), 0);
    assert_eq!(profile.effects.connections.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(outside).unwrap(), b"private sentinel");
    runtime.drain().await.settled().await.unwrap();
    drop(ownership);
}

#[tokio::test]
async fn private_adapters_reject_untagged_wrong_namespace_and_duplicate_envelopes() {
    for bytes in [
        br#"{"version":2,"selected":null,"devices":[]}"#.as_slice(),
        br#"{"namespace":"production","data":{"version":2,"selected":null,"devices":[]}}"#.as_slice(),
        br#"{"namespace":"m8-fake-v1","namespace":"m8-fake-v1","data":{"version":2,"selected":null,"devices":[]}}"#.as_slice(),
    ] {
        let base=tempfile::tempdir().unwrap();
        let parent=base.path().canonicalize().unwrap();
        let (profile,_ownership)=DevelopmentProfile::open(&parent).unwrap();
        let data=profile.directory.child("data").unwrap();
        data.replace("devices.json",bytes).unwrap();
        let runtime=Runtime::start(profile.services(),RuntimeLimits::default());
        let snapshot=ready(&runtime.client(ClientId(1))).await;
        assert!(matches!(snapshot.startup,StartupState::Blocked(_)));
        assert!(snapshot.devices.is_empty());
        assert_eq!(profile.effects.credential_reads.load(Ordering::SeqCst),0);
        assert_eq!(profile.effects.connections.load(Ordering::SeqCst),0);
        assert_eq!(data.read("devices.json",4096).unwrap().unwrap(),bytes);
        runtime.drain().await.settled().await.unwrap();
    }
}
