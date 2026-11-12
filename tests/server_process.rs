//! Actual same-executable startup, detached lifetime, bootstrap and fresh profile.
use samsung_tv_remote::{
    control_client::http::HttpClient,
    server::{
        bootstrap::{socket::Location, Role},
        launch,
        lifecycle::{
            endpoint::{build_identity, Manifest},
            files::{FileError, RuntimeDirectory},
        },
    },
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
const BINARY: &str = env!("CARGO_BIN_EXE_samsung-tv-remote");
fn command(binary: &Path, base: &Path, action: &str) -> Output {
    let child = Command::new(binary)
        .args([
            std::ffi::OsStr::new(action),
            std::ffi::OsStr::new("--fake-profile"),
            base.as_os_str(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut child = child;
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("development command timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
struct Fixture {
    base: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self {
            base: tempfile::tempdir().unwrap(),
        }
    }
    fn base(&self) -> PathBuf {
        self.base.path().canonicalize().unwrap()
    }
    fn run(&self, action: &str) -> Value {
        let output = command(Path::new(BINARY), &self.base(), action);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains("bearer"));
        assert!(!text.contains("proof_key"));
        serde_json::from_str(&text).unwrap()
    }
    async fn client(&self) -> HttpClient {
        let build = build_identity(Path::new(BINARY)).unwrap();
        launch::attach(&self.base(), Role::Gui, Some(&build))
            .await
            .unwrap()
    }
    fn owner_settled(&self) {
        let directory = RuntimeDirectory::open(&self.base().join("m8-fake-v1")).unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            match directory.lock() {
                Ok(lock) => {
                    drop(lock);
                    return;
                }
                Err(FileError::Locked) if Instant::now() < until => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => panic!("owner failed to settle"),
            }
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = command(Path::new(BINARY), &self.base(), "--server-stop");
        let directory = RuntimeDirectory::open(&self.base().join("m8-fake-v1"));
        let until = Instant::now() + Duration::from_secs(10);
        if let Ok(directory) = directory {
            while matches!(directory.lock(), Err(FileError::Locked)) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        if let Ok(location) = Location::for_profile(&self.base()) {
            let _ = std::fs::remove_file(&location.path);
            if let Some(parent) = location.path.parent() {
                let _ = std::fs::remove_dir(parent);
            }
        }
    }
}
async fn state(client: &HttpClient) -> Value {
    let mut events = client.events().await.unwrap();
    loop {
        let event = events.next().await.unwrap();
        if event.state["startup"] != "restoring" {
            return event.state;
        }
    }
}
async fn mutation(client: &HttpClient, path: &str, extra: Value) -> Value {
    let state = state(client).await;
    let key = uuid::Uuid::new_v4().to_string();
    let mut request = json!({"server_epoch":state["server_epoch"],"operation_key":key});
    request
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let (status, receipt) = client.call("POST", path, Some(&request)).await.unwrap();
    assert!([200, 202].contains(&status), "{receipt}");
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let (_, value) = client
                .call(
                    "GET",
                    &format!(
                        "/v1/operations/by-key/{key}?epoch={}",
                        state["server_epoch"].as_str().unwrap()
                    ),
                    None,
                )
                .await
                .unwrap();
            if value["state"] == "terminal" {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn detached_parent_exit_sticky_stop_explicit_resume_and_fake_pairing_survive_restart() {
    let fixture = Fixture::new();
    let base = fixture.base();
    std::fs::write(
        base.join("devices.json"),
        b"legacy fixture must remain untouched",
    )
    .unwrap();
    let ready = fixture.run("--server-start");
    assert_eq!(ready["serving"], true);
    // The launcher process above has exited. A distinct status process and
    // native HTTP/event client now attach to the surviving server.
    let status = fixture.run("--server-status");
    assert_eq!(status["server_epoch"], ready["server_epoch"]);
    let client = fixture.client().await;
    let initial = state(&client).await;
    assert_eq!(initial["devices"], json!([]));
    assert_eq!(initial["selected"], Value::Null);
    assert_eq!(
        std::fs::read(base.join("devices.json")).unwrap(),
        b"legacy fixture must remain untouched"
    );
    assert_eq!(
        std::fs::read_dir(base.join("m8-fake-v1/synthetic-credentials"))
            .unwrap()
            .count(),
        0
    );
    let observation = mutation(
        &client,
        "/v1/probes",
        json!({"host":"synthetic-tv.invalid"}),
    )
    .await;
    let o = &observation["result"];
    let paired=mutation(&client,"/v1/pairings",json!({"host":"synthetic-tv.invalid","attempt_id":o["attempt_id"],"observation_id":o["observation_id"],"fingerprint":o["fingerprint"],"confirm_fingerprint":o["fingerprint"],"label":"Synthetic persisted TV","selection_generation":"0"})).await;
    assert_eq!(paired["result"]["kind"], "tv_approved_and_saved");
    let device = paired["result"]["device_id"].clone();
    let old_epoch = ready["server_epoch"].clone();
    fixture.run("--server-stop");
    fixture.owner_settled();
    let stopped = command(Path::new(BINARY), &base, "--server-start");
    assert!(!stopped.status.success());
    assert!(String::from_utf8(stopped.stderr)
        .unwrap()
        .contains("server_stopped"));
    assert!(matches!(
        launch::resume(Path::new(BINARY), &base, Role::Mcp).await,
        Err(launch::LaunchError::Denied)
    ));
    let resumed = fixture.run("--server-resume");
    assert_ne!(resumed["server_epoch"], old_epoch);
    let client = fixture.client().await;
    let restored = state(&client).await;
    assert_eq!(restored["selected"], device);
    assert_eq!(restored["connection"], "not_connected");
    assert_eq!(restored["selection_generation"], "0");
    assert_eq!(restored["devices"].as_array().unwrap().len(), 1);
    let connected = mutation(
        &client,
        "/v1/connections",
        json!({"device_id":device,"selection_generation":"0"}),
    )
    .await;
    assert_eq!(connected["result"]["kind"], "remote_ready");
    let action = mutation(
        &client,
        "/v1/remote-actions",
        json!({"device_id":device,"selection_generation":"0","action":"VolumeUp"}),
    )
    .await;
    assert_eq!(action["result"]["kind"], "written");
    let (_, lost) = client
        .call(
            "GET",
            &format!(
                "/v1/operations/{}?epoch={}",
                observation["operation_id"].as_str().unwrap(),
                old_epoch.as_str().unwrap()
            ),
            None,
        )
        .await
        .unwrap();
    assert_eq!(lost["error"]["code"], "unknown_after_restart");
    let restarted = fixture.run("--server-restart");
    assert_ne!(restarted["server_epoch"], resumed["server_epoch"]);
    fixture.run("--server-stop");
    fixture.owner_settled();
}

#[test]
fn simultaneous_launches_attach_to_one_owner_and_keep_the_endpoint_stable() {
    let fixture = Fixture::new();
    let base = fixture.base();
    let outputs = std::thread::scope(|scope| {
        let first = scope.spawn(|| command(Path::new(BINARY), &base, "--server-start"));
        let second = scope.spawn(|| command(Path::new(BINARY), &base, "--server-start"));
        [first.join().unwrap(), second.join().unwrap()]
    });
    let values: Vec<Value> = outputs
        .iter()
        .map(|output| {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice(&output.stdout).unwrap()
        })
        .collect();
    assert_eq!(values[0]["server_epoch"], values[1]["server_epoch"]);
    let directory = Arc::new(RuntimeDirectory::open(&base.join("m8-fake-v1")).unwrap());
    let manifest = Manifest::read(&directory).unwrap().unwrap();
    assert!(matches!(directory.lock(), Err(FileError::Locked)));
    fixture.run("--server-stop");
    fixture.owner_settled();
    fixture.run("--server-resume");
    assert_eq!(
        Manifest::read(&directory).unwrap().unwrap().port,
        manifest.port
    );
    fixture.run("--server-stop");
    fixture.owner_settled();
}

struct DirectChild(std::process::Child);
impl Drop for DirectChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
async fn projected_state(
    receiver: &mut tokio::sync::watch::Receiver<
        Option<samsung_tv_remote::application::control_service::Snapshot>,
    >,
    epoch: Option<samsung_tv_remote::application::control_service::Epoch>,
) -> samsung_tv_remote::application::control_service::Snapshot {
    use samsung_tv_remote::application::control_service::StartupState;
    tokio::time::timeout(Duration::from_secs(15), async {
        receiver
            .wait_for(|snapshot| {
                snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.startup == StartupState::Ready
                        && epoch.is_none_or(|old| old != snapshot.epoch)
                })
            })
            .await
            .unwrap()
            .as_ref()
            .unwrap()
            .clone()
    })
    .await
    .unwrap()
}
async fn remote_terminal(
    remote: &samsung_tv_remote::control_client::background::Remote,
    tag: u64,
    request: samsung_tv_remote::application::control_service::Request,
) -> samsung_tv_remote::control_client::projection::NativeReceipt {
    let receipt = remote.submit(tag, request).await.unwrap();
    if receipt.terminal {
        return receipt;
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let result = remote.operation(receipt.epoch, receipt.id).await.unwrap();
            if result.terminal {
                return result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn gui_client_reattaches_after_process_outage_without_replaying_setup_connect_or_action() {
    use samsung_tv_remote::application::control_service::{Command, Request};
    use samsung_tv_remote::application::SendRemoteAction;
    use samsung_tv_remote::control_client::{
        background::{Config, Remote, Status, Update},
        reconnect::Blocked,
    };
    let fixture = Fixture::new();
    let base = fixture.base();
    let mut child = DirectChild(
        std::process::Command::new(BINARY)
            .args([
                std::ffi::OsStr::new("--server"),
                std::ffi::OsStr::new("--fake-profile"),
                base.as_os_str(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let directory = RuntimeDirectory::open(&base.join("m8-fake-v1")).unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if matches!(directory.lock(), Err(FileError::Locked)) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let remote = Remote::start(Config {
        executable: BINARY.into(),
        base: base.clone(),
    })
    .await
    .unwrap();
    let mut updates = remote.take_updates().unwrap();
    let (state_tx, mut states) = tokio::sync::watch::channel(None);
    let (status_tx, mut statuses) = tokio::sync::watch::channel(None);
    let observer = tokio::spawn(async move {
        while let Some(update) = updates.recv().await {
            match update {
                Update::Snapshot { snapshot, .. } => {
                    let _ = state_tx.send(Some(*snapshot));
                }
                Update::Status(status) => {
                    let exited = matches!(status, Status::Exited);
                    let _ = status_tx.send(Some(status));
                    if exited {
                        break;
                    }
                }
                _ => (),
            }
        }
    });
    let initial = projected_state(&mut states, None).await;
    assert!(initial.devices.is_empty());
    let request = |epoch, command| Request {
        epoch,
        key: uuid::Uuid::new_v4().to_string(),
        command,
    };
    let probe = remote_terminal(
        &remote,
        1,
        request(
            initial.epoch,
            Command::Probe(
                samsung_tv_remote::application::tv_address::TvHost::parse("synthetic-tv.invalid")
                    .unwrap(),
            ),
        ),
    )
    .await;
    let observed = probe.observation().unwrap();
    let paired = remote_terminal(
        &remote,
        2,
        request(
            initial.epoch,
            Command::Pair {
                observation: observed.id,
                host: observed.host,
                pin: observed.observation.pin,
                label: "GUI probe fixture".into(),
                replace: None,
                generation: 0,
                recovery: None,
            },
        ),
    )
    .await;
    assert_eq!(paired.kind(), "tv_approved_and_saved");
    tokio::time::timeout(
        Duration::from_secs(3),
        states.wait_for(|state| {
            state
                .as_ref()
                .is_some_and(|state| state.control.selected_device().is_some())
        }),
    )
    .await
    .unwrap()
    .unwrap();
    let paired_state = states.borrow().clone().unwrap();
    let device = paired_state.control.selected_device().unwrap();
    let generation = paired_state.control.selection_generation();
    let action = remote_terminal(
        &remote,
        3,
        request(
            initial.epoch,
            Command::Remote(SendRemoteAction::new(
                device,
                generation,
                samsung_tv_remote::RemoteAction::VolumeUp,
            )),
        ),
    )
    .await;
    assert_eq!(action.kind(), "written");
    // This exact Child handle belongs to the fixture; killing it models a
    // process crash without a sticky marker or a forged PID from disk.
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let restored = projected_state(&mut states, Some(initial.epoch)).await;
    assert_eq!(restored.control.selected_device(), Some(device));
    assert_eq!(restored.control.selection_generation(), 0);
    assert_eq!(
        restored.control.connection_state(),
        samsung_tv_remote::application::ConnectionState::NotConnected
    );
    let client = fixture.client().await;
    let (status, state) = client.call("GET", "/v1/state", None).await.unwrap();
    assert_eq!(status, 200);
    assert_eq!(state["recent_operations"], json!([]));
    assert!(matches!(
        remote.operation(initial.epoch, action.id).await,
        Err(samsung_tv_remote::control_client::http::ClientError::Resnapshot)
    ));
    fixture.run("--server-stop");
    fixture.owner_settled();
    tokio::time::timeout(
        Duration::from_secs(10),
        statuses.wait_for(|status| matches!(status, Some(Status::Blocked(Blocked::Stopped)))),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!remote.connected());
    remote.resume().await.unwrap();
    let resumed = projected_state(&mut states, Some(restored.epoch)).await;
    assert_eq!(resumed.control.selected_device(), Some(device));
    assert_eq!(
        resumed.control.connection_state(),
        samsung_tv_remote::application::ConnectionState::NotConnected
    );
    remote.disconnect();
    tokio::time::timeout(
        Duration::from_secs(3),
        statuses.wait_for(|status| matches!(status, Some(Status::Exited))),
    )
    .await
    .unwrap()
    .unwrap();
    observer.await.unwrap();
    let reopened = Remote::start(Config {
        executable: BINARY.into(),
        base: base.clone(),
    })
    .await
    .unwrap();
    let mut updates = reopened.take_updates().unwrap();
    let same_owner = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Update::Snapshot { snapshot, .. } = updates.recv().await.unwrap() {
                if snapshot.startup
                    == samsung_tv_remote::application::control_service::StartupState::Ready
                {
                    break snapshot;
                }
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(same_owner.epoch, resumed.epoch);
    assert_eq!(same_owner.control.selected_device(), Some(device));
    assert_eq!(
        same_owner.control.connection_state(),
        samsung_tv_remote::application::ConnectionState::NotConnected
    );
    reopened.disconnect();
    assert_eq!(
        fixture.run("--server-status")["server_epoch"],
        samsung_tv_remote::server::control_http::wire::epoch(resumed.epoch)
    );
    assert!(matches!(directory.lock(), Err(FileError::Locked)));
    fixture.run("--server-stop");
    fixture.owner_settled();
}

#[test]
fn executable_modes_select_the_gui_probe_before_any_local_adapter() {
    use samsung_tv_remote::server::entry::{parse, Action, Mode};
    assert_eq!(parse(&[]).unwrap(), Mode::Gui);
    assert!(matches!(
        parse(&[
            "--server-credential-probe".into(),
            "read".into(),
            "550e8400-e29b-41d4-a716-446655440000".into()
        ])
        .unwrap(),
        Mode::CredentialProbe { .. }
    ));
    for target in ["gui", "mcp", "dev_1", "../gui"] {
        assert!(parse(&[
            "--server-credential-probe".into(),
            "delete".into(),
            target.into()
        ])
        .is_err());
    }
    assert_eq!(
        parse(&["--server".into()]).unwrap(),
        Mode::Production {
            action: Action::Server,
            inherited: false
        }
    );
    assert_eq!(
        parse(&["--server".into(), "--inherited-owner".into()]).unwrap(),
        Mode::Production {
            action: Action::Server,
            inherited: true
        }
    );
    assert!(parse(&["--server-start".into(), "--inherited-owner".into()]).is_err());
    assert!(matches!(
        parse(&[
            "--gui-probe".into(),
            "--fake-profile".into(),
            "/private/tmp/synthetic".into()
        ])
        .unwrap(),
        Mode::Probe { .. }
    ));
    assert!(parse(&[
        "--server".into(),
        "--profile".into(),
        "/private/tmp/production".into()
    ])
    .is_err());
    assert!(parse(&[
        "--server".into(),
        "--fake-profile".into(),
        "relative".into()
    ])
    .is_err());
}

#[tokio::test]
async fn incompatible_build_requires_explicit_restart_and_never_replaces_the_owner() {
    let fixture = Fixture::new();
    let original = fixture.run("--server-start");
    let wrong_build = "00".repeat(32);
    let result =
        launch::start_with_build(Path::new(BINARY), &fixture.base(), Role::Gui, &wrong_build).await;
    assert!(matches!(
        result,
        Err(launch::LaunchError::Bootstrap(
            samsung_tv_remote::server::bootstrap::BootstrapError::IncompatibleBuild
        ))
    ));
    assert_eq!(
        fixture.run("--server-status")["server_epoch"],
        original["server_epoch"]
    );
    assert!(matches!(
        launch::restart(Path::new(BINARY), &fixture.base(), Role::Mcp).await,
        Err(launch::LaunchError::Denied)
    ));
    assert_eq!(
        fixture.run("--server-status")["server_epoch"],
        original["server_epoch"]
    );
    let replacement = fixture.run("--server-restart");
    assert_ne!(replacement["server_epoch"], original["server_epoch"]);
    fixture.run("--server-stop");
    fixture.owner_settled();
}

#[test]
fn occupied_endpoint_and_corrupt_manifest_fail_closed_without_reallocation() {
    let fixture = Fixture::new();
    fixture.run("--server-start");
    fixture.run("--server-stop");
    fixture.owner_settled();
    let directory = RuntimeDirectory::open(&fixture.base().join("m8-fake-v1")).unwrap();
    let original = directory.read("endpoint.json", 4096).unwrap().unwrap();
    let manifest = Manifest::read(&directory).unwrap().unwrap();
    let occupant =
        std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, manifest.port)).unwrap();
    occupant.set_nonblocking(true).unwrap();
    for action in ["--server-resume", "--server-start"] {
        let failed = command(Path::new(BINARY), &fixture.base(), action);
        assert!(!failed.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&failed.stderr).unwrap()["error"],
            "endpoint_collision"
        );
        assert_eq!(
            directory.read("endpoint.json", 4096).unwrap().unwrap(),
            original
        );
        assert_eq!(
            occupant.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert!(directory.lock().is_ok());
    }
    drop(occupant);
    fixture.run("--server-start");
    assert_eq!(
        Manifest::read(&directory).unwrap().unwrap().port,
        manifest.port
    );
    fixture.run("--server-stop");
    fixture.owner_settled();
    directory
        .replace("endpoint.json", b"{invalid metadata}")
        .unwrap();
    let failed = command(Path::new(BINARY), &fixture.base(), "--server-resume");
    assert!(!failed.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&failed.stderr).unwrap()["error"],
        "unsafe_profile"
    );
    assert_eq!(
        directory.read("endpoint.json", 4096).unwrap().unwrap(),
        b"{invalid metadata}"
    );
    assert!(directory.lock().is_ok());
    // Restore only this fixture's bytes so its normal cleanup can authenticate.
    directory.replace("endpoint.json", &original).unwrap();
}

#[tokio::test]
async fn detached_spawn_uses_a_new_session_and_inherits_only_declared_descriptors() {
    use samsung_tv_remote::server::lifecycle::spawn;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let base = fixture.base();
    let directory = RuntimeDirectory::open(&base.join("m8-fake-v1")).unwrap();
    let ownership = directory.lock().unwrap();
    let (_, listener) = Manifest::bind(&directory, &ownership, &"11".repeat(32)).unwrap();
    let unrelated = std::fs::File::open("/dev/null").unwrap();
    assert!(unrelated.as_raw_fd() > 2);
    let executable = base.join("spawn-fixture");
    let source = base.join("spawn-fixture.c");
    std::fs::write(&source,r#"
#include <unistd.h>
#include <sys/stat.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
extern char **environ;
int main(int argc,char **argv) {
    if(argc!=5) return 1;
    unsigned mask=0; int unexpected=0; struct stat status, null_status;
    for(int fd=0;fd<1024;fd++) if(fstat(fd,&status)==0) {
        if(fd<6) mask|=1u<<fd; else unexpected=1;
    }
    int stdio_null=stat("/dev/null",&null_status)==0;
    for(int fd=0;fd<3;fd++) stdio_null &= fstat(fd,&status)==0 && status.st_rdev==null_status.st_rdev;
    fstat(3,&status); int regular=S_ISREG(status.st_mode);
    fstat(4,&status); int socket=S_ISSOCK(status.st_mode);
    fstat(5,&status); int fifo=S_ISFIFO(status.st_mode);
    int env_ok=environ[0] && strcmp(environ[0],"PATH=/usr/bin:/bin")==0 && !environ[1];
    char path[4096]; snprintf(path,sizeof(path),"%s/spawn-evidence.json",argv[3]);
    FILE *output=fopen(path,"wx"); if(!output) return 2;
    fprintf(output,"{\"session_is_pid\":%d,\"fd_mask\":%u,\"unexpected_fd\":%d,\"environment_path_only\":%d,\"lock_regular\":%d,\"listener_socket\":%d,\"report_fifo\":%d,\"stdio_null\":%d}",getsid(0)==getpid(),mask,unexpected,env_ok,regular,socket,fifo,stdio_null);
    if(fclose(output)!=0) return 3;
    return write(5,"ready\n",6)==6 ? 0:4;
}
"#).unwrap();
    let compiled = Command::new("cc")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    spawn::detached(&executable, &base, &ownership, &listener)
        .unwrap()
        .ready()
        .await
        .unwrap();
    let evidence: Value =
        serde_json::from_slice(&std::fs::read(base.join("spawn-evidence.json")).unwrap()).unwrap();
    assert_eq!(evidence["session_is_pid"], 1);
    assert_eq!(evidence["fd_mask"], 63);
    assert_eq!(evidence["unexpected_fd"], 0);
    for field in [
        "environment_path_only",
        "lock_regular",
        "listener_socket",
        "report_fifo",
        "stdio_null",
    ] {
        assert_eq!(evidence[field], 1, "{field}");
    }
}

#[test]
fn stop_racing_ordinary_start_preserves_the_marker_and_never_creates_a_new_epoch() {
    let fixture = Fixture::new();
    let original = fixture.run("--server-start");
    let base = fixture.base();
    let (stop, start) = std::thread::scope(|scope| {
        let stop = scope.spawn(|| command(Path::new(BINARY), &base, "--server-stop"));
        let start = scope.spawn(|| command(Path::new(BINARY), &base, "--server-start"));
        (stop.join().unwrap(), start.join().unwrap())
    });
    assert!(
        stop.status.success(),
        "{}",
        String::from_utf8_lossy(&stop.stderr)
    );
    if start.status.success() {
        assert_eq!(
            serde_json::from_slice::<Value>(&start.stdout).unwrap()["server_epoch"],
            original["server_epoch"]
        );
    } else {
        let error = serde_json::from_slice::<Value>(&start.stderr).unwrap();
        assert!(
            [
                "server_stopped",
                "bootstrap_unavailable",
                "untrusted_listener"
            ]
            .iter()
            .any(|code| error["error"] == *code),
            "{error}"
        );
    }
    fixture.owner_settled();
    let stopped = command(Path::new(BINARY), &base, "--server-start");
    assert!(!stopped.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&stopped.stderr).unwrap()["error"],
        "server_stopped"
    );
}

#[test]
fn different_profile_bindings_never_attach_to_another_profiles_occupied_endpoint() {
    let first = Fixture::new();
    let second = Fixture::new();
    let original = first.run("--server-start");
    let first_dir = RuntimeDirectory::open(&first.base().join("m8-fake-v1")).unwrap();
    let second_dir = RuntimeDirectory::open(&second.base().join("m8-fake-v1")).unwrap();
    let bytes = first_dir.read("endpoint.json", 4096).unwrap().unwrap();
    let first_location = Location::for_profile(&first.base()).unwrap();
    let second_location = Location::for_profile(&second.base()).unwrap();
    assert_ne!(first_location.profile, second_location.profile);
    assert_ne!(first_location.path, second_location.path);
    second_dir.replace("endpoint.json", &bytes).unwrap();
    let rejected = command(Path::new(BINARY), &second.base(), "--server-start");
    assert!(!rejected.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&rejected.stderr).unwrap()["error"],
        "endpoint_collision"
    );
    assert!(second_dir.lock().is_ok());
    assert_eq!(
        first.run("--server-status")["server_epoch"],
        original["server_epoch"]
    );
    assert_eq!(
        first_dir.read("endpoint.json", 4096).unwrap().unwrap(),
        bytes
    );
    first.run("--server-stop");
    first.owner_settled();
}

#[tokio::test]
async fn explicit_resume_waits_for_profile_ownership_before_clearing_stop() {
    use samsung_tv_remote::server::{
        development::DevelopmentProfile,
        lifecycle::stop::{FileStopStore, StopMarker, StopStore},
    };
    let fixture = Fixture::new();
    let base = fixture.base();
    let (profile, owner) = DevelopmentProfile::open(&base).unwrap();
    let marker = StopMarker {
        version: 1,
        profile: "m8-fake-v1".into(),
        server_epoch: uuid::Uuid::new_v4().to_string(),
        operation_key: uuid::Uuid::new_v4().to_string(),
        identity: "gui".into(),
    };
    let store = FileStopStore(profile.directory.clone());
    store.write(&marker).unwrap();
    let executable = PathBuf::from(BINARY);
    let launch_base = base.clone();
    let resume =
        tokio::spawn(async move { launch::resume(&executable, &launch_base, Role::Gui).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!resume.is_finished());
    assert_eq!(store.read().unwrap(), Some(marker.clone()));
    // This guard models an owner still settling a blocking store. The actor's
    // real late-worker/lock rule is separately exercised in server_lifecycle.
    drop(owner);
    let client = tokio::time::timeout(Duration::from_secs(15), resume)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let (status, ready) = client.call("GET", "/v1/ready", None).await.unwrap();
    assert_eq!(status, 200);
    assert_ne!(ready["server_epoch"], marker.server_epoch);
    assert!(store.read().unwrap().is_none());
    assert!(matches!(profile.directory.lock(), Err(FileError::Locked)));
    fixture.run("--server-stop");
    fixture.owner_settled();
}

#[tokio::test]
async fn termination_signal_models_logout_without_an_intentional_stop_or_implicit_tv_reconnect() {
    let fixture = Fixture::new();
    let base = fixture.base();
    let mut child = DirectChild(
        Command::new(BINARY)
            .args([
                std::ffi::OsStr::new("--server"),
                std::ffi::OsStr::new("--fake-profile"),
                base.as_os_str(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let before = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(client) = launch::attach(&base, Role::Gui, None).await {
                break client;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    let (_, ready) = before.call("GET", "/v1/ready", None).await.unwrap();
    // The PID comes from this exact owned Child, never a manifest or endpoint.
    assert_eq!(unsafe { libc::kill(child.0.id() as i32, libc::SIGTERM) }, 0);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let directory = Arc::new(RuntimeDirectory::open(&base.join("m8-fake-v1")).unwrap());
    assert!(directory.lock().is_ok());
    use samsung_tv_remote::server::lifecycle::stop::{FileStopStore, StopStore};
    assert!(FileStopStore(directory).read().unwrap().is_none());
    let restarted = fixture.run("--server-start");
    assert_ne!(restarted["server_epoch"], ready["server_epoch"]);
    let client = fixture.client().await;
    let snapshot = state(&client).await;
    assert_eq!(snapshot["devices"], json!([]));
    assert_eq!(snapshot["connection"], "not_connected");
    assert_eq!(snapshot["recent_operations"], json!([]));
    fixture.run("--server-stop");
    fixture.owner_settled();
}

#[tokio::test]
async fn native_stop_is_epoch_fenced_sticky_for_all_clients_and_only_explicit_resume_reopens() {
    use samsung_tv_remote::{
        application::control_service::Epoch,
        control_client::{
            background::{Config, Remote, Status, Update},
            port::{StopAcknowledgment, StopSettlement},
            reconnect::Blocked,
        },
    };
    async fn attached(updates: &mut tokio::sync::mpsc::Receiver<Update>) -> Epoch {
        tokio::time::timeout(Duration::from_secs(15), async {
            let mut epoch = None;
            loop {
                match updates.recv().await.unwrap() {
                    Update::Snapshot {
                        snapshot,
                        initial: true,
                    } => epoch = Some(snapshot.epoch),
                    Update::Status(Status::Connected) => {
                        return epoch.expect("atomic state before connected")
                    }
                    _ => (),
                }
            }
        })
        .await
        .unwrap()
    }
    async fn stopped(updates: &mut tokio::sync::mpsc::Receiver<Update>) {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if matches!(
                    updates.recv().await.unwrap(),
                    Update::Status(Status::Blocked(Blocked::Stopped))
                ) {
                    return;
                }
            }
        })
        .await
        .unwrap()
    }
    let fixture = Fixture::new();
    fixture.run("--server-start");
    let config = Config {
        executable: BINARY.into(),
        base: fixture.base(),
    };
    let remote = Remote::start(config.clone()).await.unwrap();
    let mut updates = remote.take_updates().unwrap();
    let epoch = attached(&mut updates).await;
    let observer = Remote::start(config.clone()).await.unwrap();
    let mut observations = observer.take_updates().unwrap();
    assert_eq!(attached(&mut observations).await, epoch);
    let client = fixture.client().await;
    assert!(matches!(
        launch::stop_expected(
            &fixture.base(),
            &client,
            Some(Epoch::from_u128(epoch.as_u128().wrapping_add(1)))
        )
        .await,
        Err(launch::LaunchError::StaleEpoch)
    ));
    assert_eq!(client.call("GET", "/v1/ready", None).await.unwrap().0, 200);
    let report = remote.stop(epoch).await.unwrap();
    assert_eq!(report.acknowledgment, StopAcknowledgment::Durable);
    assert!(matches!(
        report.settlement,
        StopSettlement::Observed | StopSettlement::Pending
    ));
    stopped(&mut updates).await;
    stopped(&mut observations).await;
    assert!(!remote.connected() && !observer.connected());
    assert!(matches!(
        observer.recheck().await,
        Err(launch::LaunchError::Stopped)
    ));
    remote.disconnect();
    observer.disconnect();
    fixture.owner_settled();
    let reopened = Remote::start(config).await.unwrap();
    let mut fresh = reopened.take_updates().unwrap();
    stopped(&mut fresh).await;
    assert!(!reopened.connected());
    assert!(matches!(
        reopened.recheck().await,
        Err(launch::LaunchError::Stopped)
    ));
    reopened.resume().await.unwrap();
    let resumed = attached(&mut fresh).await;
    assert_ne!(resumed, epoch);
    let state = state(&fixture.client().await).await;
    assert_eq!(state["devices"], json!([]));
    assert_eq!(state["selected"], Value::Null);
    reopened.disconnect();
    assert!(matches!(
        reopened.resume().await,
        Err(launch::LaunchError::Unavailable)
    ));
    assert!(matches!(
        reopened.restart().await,
        Err(launch::LaunchError::Unavailable)
    ));
    assert!(matches!(
        reopened.recheck().await,
        Err(launch::LaunchError::Unavailable)
    ));
}

#[tokio::test]
async fn native_start_status_names_new_owner_creation_separately_from_attachment() {
    use samsung_tv_remote::control_client::background::{Config, Remote, Status, Update};
    let fixture = Fixture::new();
    let config = Config {
        executable: BINARY.into(),
        base: fixture.base(),
    };
    for starts_owner in [true, false] {
        let remote = Remote::start(config.clone()).await.unwrap();
        let mut updates = remote.take_updates().unwrap();
        let started = tokio::time::timeout(Duration::from_secs(15), async {
            let mut started = false;
            let mut snapshot = false;
            loop {
                match updates.recv().await.unwrap() {
                    Update::Status(Status::Starting) => started = true,
                    Update::Snapshot { initial: true, .. } => snapshot = true,
                    Update::Status(Status::Connected) => {
                        assert!(snapshot);
                        return started;
                    }
                    Update::Status(Status::Blocked(reason)) => {
                        panic!("unexpected blocked status {reason:?}")
                    }
                    _ => (),
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(started, starts_owner);
        remote.disconnect();
    }
}
