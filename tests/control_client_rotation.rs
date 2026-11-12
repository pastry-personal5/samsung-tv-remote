//! Native GUI actor, protected bootstrap and actual HTTP/SSE share one fake TV
//! owner. Credential rotation/revocation must not become mutation replay.
use samsung_tv_remote::{
    application::{
        control_service::*,
        runtime::{Runtime, RuntimeLimits},
        SendRemoteAction,
    },
    control_client::{
        background::{Config, Remote, Status, Update},
        bootstrap::authenticate,
        projection::NativeReceipt,
        reconnect::Blocked,
    },
    server::{
        access::AccessGate,
        bootstrap::{
            self, ApiCredentials, Bootstrap, BootstrapError, Context, DevelopmentPeer, Role,
        },
        control_http::{transport::HttpAdapter, wire},
        development::DevelopmentProfile,
        lifecycle::{
            endpoint::{build_identity, Manifest},
            stop::{FileStopStore, Lifecycle},
        },
    },
    RemoteAction,
};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
struct Credentials {
    gui: Mutex<[u8; 32]>,
    reads: AtomicUsize,
}
impl ApiCredentials for Credentials {
    fn load(&self, role: Role) -> Result<[u8; 32], BootstrapError> {
        Ok(if role == Role::Gui {
            self.reads.fetch_add(1, Ordering::SeqCst);
            *self.gui.lock().unwrap()
        } else {
            [2; 32]
        })
    }
}
async fn initial(updates: &mut tokio::sync::mpsc::Receiver<Update>) -> Snapshot {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            match updates.recv().await.unwrap() {
                Update::Snapshot {
                    snapshot,
                    initial: true,
                } => return *snapshot,
                Update::Status(Status::Blocked(reason)) => {
                    panic!("unexpected attach block: {reason:?}")
                }
                _ => (),
            }
        }
    })
    .await
    .unwrap()
}
async fn terminal(remote: &Remote, tag: u64, request: Request) -> NativeReceipt {
    let mut receipt = remote.submit(tag, request).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !receipt.terminal {
            tokio::time::sleep(Duration::from_millis(5)).await;
            receipt = remote.operation(receipt.epoch, receipt.id).await.unwrap();
        }
        receipt
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn gui_actor_rotates_to_a_fresh_snapshot_then_blocks_on_revocation_without_replaying_any_tv_effect(
) {
    let base = tempfile::tempdir().unwrap();
    let base_path = base.path().canonicalize().unwrap();
    let (profile, owner) = DevelopmentProfile::open(&base_path).unwrap();
    let executable = Path::new(env!("CARGO_BIN_EXE_samsung-tv-remote"));
    let build = build_identity(executable).unwrap();
    let (manifest, listener) = Manifest::bind(&profile.directory, &owner, &build).unwrap();
    let port = manifest.port;
    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
    let runtime = Runtime::start(profile.services(), RuntimeLimits::default());
    let mut ready = runtime.client(ClientId(1)).subscribe().await.unwrap();
    while ready.snapshot.startup == StartupState::Restoring {
        ready.snapshot = ready.events.recv().await.unwrap().snapshot;
    }
    let epoch = ready.snapshot.epoch;
    drop(ready);
    let services = [
        (ClientId(1), runtime.client(ClientId(1))),
        (ClientId(2), runtime.client(ClientId(2))),
    ];
    let gate = Arc::new(AccessGate::new(port));
    let credentials = Arc::new(Credentials {
        gui: Mutex::new([1; 32]),
        reads: AtomicUsize::new(0),
    });
    let location = bootstrap::socket::Location::for_profile(&base_path).unwrap();
    let socket = location.bind().await.unwrap();
    let peer = Arc::new(DevelopmentPeer {
        uid: bootstrap::socket::uid(),
    });
    let bootstrap = Arc::new(
        Bootstrap::new(
            gate.clone(),
            credentials.clone(),
            peer.clone(),
            Context {
                profile: location.profile.clone(),
                endpoint: manifest.url.clone(),
                build: build.clone(),
                api: "v1".into(),
                epoch: wire::epoch(epoch),
            },
        )
        .unwrap(),
    );
    let lifecycle = Lifecycle::start(
        runtime,
        Arc::new(FileStopStore(profile.directory.clone())),
        owner,
    )
    .await
    .unwrap();
    let http = Arc::new(HttpAdapter::new(
        gate.clone(),
        services,
        lifecycle.client.clone(),
        build.clone(),
    ));
    let http_task = tokio::spawn(http.serve(listener, lifecycle.http_shutdown.clone()));
    let bootstrap_task = tokio::spawn(
        bootstrap
            .clone()
            .serve(socket, lifecycle.http_shutdown.clone()),
    );
    let old_client = authenticate(
        &location.path,
        &location.profile,
        &manifest.url,
        Role::Gui,
        Some(&build),
        peer,
    )
    .await
    .unwrap();
    let remote = Remote::start(Config {
        executable: executable.into(),
        base: base_path,
    })
    .await
    .unwrap();
    let mut updates = remote.take_updates().unwrap();
    let first = initial(&mut updates).await;
    assert_eq!(first.epoch, epoch);
    assert!(first.devices.is_empty());
    let request = |command| Request {
        epoch,
        key: uuid::Uuid::new_v4().to_string(),
        command,
    };
    let probe = terminal(
        &remote,
        1,
        request(Command::Probe(
            samsung_tv_remote::application::tv_address::TvHost::parse("synthetic-tv.invalid")
                .unwrap(),
        )),
    )
    .await;
    let observed = probe.observation().unwrap();
    let pair = terminal(
        &remote,
        2,
        request(Command::Pair {
            observation: observed.id,
            host: observed.host,
            pin: observed.observation.pin,
            label: "Rotation fixture".into(),
            replace: None,
            generation: 0,
            recovery: None,
        }),
    )
    .await;
    assert_eq!(pair.kind(), "tv_approved_and_saved");
    let paired = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Update::Snapshot { snapshot, .. } = updates.recv().await.unwrap() {
                if snapshot.control.selected_device().is_some() {
                    break snapshot;
                }
            }
        }
    })
    .await
    .unwrap();
    let device = paired.control.selected_device().unwrap();
    let generation = paired.control.selection_generation();
    let action = terminal(
        &remote,
        3,
        request(Command::Remote(SendRemoteAction::new(
            device,
            generation,
            RemoteAction::VolumeUp,
        ))),
    )
    .await;
    assert_eq!(action.kind(), "written");
    *credentials.gui.lock().unwrap() = [9; 32];
    bootstrap.refresh(Role::Gui).unwrap();
    let refreshed = initial(&mut updates).await;
    assert_eq!(refreshed.epoch, epoch);
    assert_eq!(refreshed.control.selected_device(), Some(device));
    assert_eq!(refreshed.control.selection_generation(), generation);
    assert_eq!(
        old_client.call("GET", "/v1/ready", None).await.unwrap().0,
        401
    );
    assert_eq!(
        remote.operation(epoch, action.id).await.unwrap().result,
        action.result
    );
    assert_eq!(profile.effects.probes.load(Ordering::SeqCst), 1);
    assert_eq!(profile.effects.approvals.load(Ordering::SeqCst), 1);
    assert_eq!(profile.effects.connections.load(Ordering::SeqCst), 1);
    assert_eq!(profile.effects.writes.load(Ordering::SeqCst), 1);
    assert_eq!(profile.effects.packets.load(Ordering::SeqCst), 0);
    gate.revoke(ClientId(1));
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if matches!(
                updates.recv().await.unwrap(),
                Update::Status(Status::Blocked(Blocked::BootstrapDenied))
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert!(!remote.connected());
    let reads = credentials.reads.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        credentials.reads.load(Ordering::SeqCst),
        reads,
        "revocation must not keep authenticating automatically"
    );
    assert_eq!(profile.effects.writes.load(Ordering::SeqCst), 1);
    remote.disconnect();
    lifecycle.signal_shutdown().unwrap();
    http_task.await.unwrap().unwrap();
    bootstrap_task.await.unwrap().unwrap();
    lifecycle.settled().await.unwrap();
    assert!(profile.directory.lock().is_ok());
    std::fs::remove_file(&location.path).unwrap();
    std::fs::remove_dir(location.path.parent().unwrap()).unwrap();
}
