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

fn headers_output(fixture: &Fixture, url: &str) -> Output {
    Command::new(BINARY)
        .args([
            std::ffi::OsStr::new("--mcp-headers"),
            std::ffi::OsStr::new("--fake-profile"),
            fixture.base().as_os_str(),
        ])
        .env("CLAUDE_CODE_MCP_SERVER_URL", url)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn mcp_management(fixture: &Fixture, arguments: &[&str]) -> Value {
    let output = Command::new(BINARY)
        .arg("--mcp-management")
        .args(arguments)
        .arg("--fake-profile")
        .arg(fixture.base())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.stdout.windows(6).any(|window| window == b"Bearer"));
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test]
async fn mcp_offline_preparation_and_revocation_preserve_stop_without_constructing_a_runtime() {
    use samsung_tv_remote::server::lifecycle::stop::{FileStopStore, StopMarker, StopStore};
    let fixture = Fixture::new();
    let status = mcp_management(&fixture, &["status"]);
    assert_eq!(status["enabled"], false);
    assert_eq!(status["server_running"], false);
    assert!(status["url"].is_null());
    let directory = Arc::new(RuntimeDirectory::open(&fixture.base().join("m8-fake-v1")).unwrap());
    let owner = directory.lock().unwrap();
    FileStopStore(directory.clone())
        .write(&StopMarker {
            version: 1,
            profile: "m8-fake-v1".into(),
            server_epoch: uuid::Uuid::new_v4().to_string(),
            operation_key: uuid::Uuid::new_v4().to_string(),
            identity: "gui".into(),
        })
        .unwrap();
    let marker = directory.read("stopped.json", 4096).unwrap().unwrap();
    drop(owner);
    let prepared = mcp_management(&fixture, &["prepare"]);
    assert_eq!(prepared["server_running"], false);
    let url = prepared["url"].as_str().unwrap();
    let enabled = mcp_management(
        &fixture,
        &["enable", "0", &uuid::Uuid::new_v4().to_string()],
    );
    assert_eq!(enabled["enabled"], true);
    assert_eq!(enabled["generation"], "1");
    let stopped = headers_output(&fixture, url);
    assert!(!stopped.status.success());
    assert!(stopped.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&stopped.stderr).unwrap()["error"],
        "server_stopped"
    );
    let disabled = mcp_management(
        &fixture,
        &["disable", "1", &uuid::Uuid::new_v4().to_string()],
    );
    assert_eq!(disabled["enabled"], false);
    assert_eq!(disabled["generation"], "2");
    assert_eq!(
        directory.read("stopped.json", 4096).unwrap().unwrap(),
        marker
    );
    assert!(directory.lock().is_ok());
    // Only explicit Resume constructs the fake runtime; it cannot regrant MCP.
    fixture.run("--server-resume");
    let denied = headers_output(&fixture, url);
    assert!(!denied.status.success());
    assert!(denied.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&denied.stderr).unwrap()["error"],
        "mcp_not_enrolled"
    );
}

#[tokio::test]
async fn removal_disconnect_then_stop_preserves_disable_across_reopen_and_config_conflict() {
    use samsung_tv_remote::server::mcp::{client_config, connection::Connection};
    use samsung_tv_remote::server::profile::Profile;
    for already_stopped in [false, true] {
        for conflict in [false, true] {
            let fixture = Fixture::new();
            let connection = Connection {
                profile: Profile::development(&fixture.base()).unwrap(),
                executable: PathBuf::from(BINARY),
                configuration: fixture.base().join(".claude.json"),
            };
            let entry = connection
                .connect(0, &uuid::Uuid::new_v4().to_string(), None)
                .await
                .unwrap()
                .entry
                .unwrap();
            fixture.run("--server-start");
            let client = fixture.client().await;
            let observation = mutation(
                &client,
                "/v1/probes",
                json!({"host":"synthetic-tv.invalid"}),
            )
            .await;
            let o = &observation["result"];
            let paired = mutation(
                &client,
                "/v1/pairings",
                json!({"host":"synthetic-tv.invalid",
                "attempt_id":o["attempt_id"],"observation_id":o["observation_id"],
                "fingerprint":o["fingerprint"],"confirm_fingerprint":o["fingerprint"],
                "label":"Retained removal fixture","selection_generation":"0"}),
            )
            .await;
            assert_eq!(paired["result"]["kind"], "tv_approved_and_saved");
            let before = state(&client).await;
            assert_eq!(before["devices"].as_array().unwrap().len(), 1);
            if already_stopped {
                fixture.run("--server-stop");
                fixture.owner_settled();
            }
            let changed = json!({"mcpServers":{"samsung-tv-remote":{"type":"http","url":"https://example.invalid"}}});
            if conflict {
                std::fs::write(
                    &connection.configuration,
                    serde_json::to_vec(&changed).unwrap(),
                )
                .unwrap();
            }
            let key = uuid::Uuid::new_v4().to_string();
            let result = connection.disconnect(1, &key, entry.clone()).await;
            if conflict {
                assert!(!result.unwrap_err().progress.enrollment.unwrap().enabled);
                let document: Value =
                    serde_json::from_slice(&std::fs::read(&connection.configuration).unwrap())
                        .unwrap();
                assert_eq!(document, changed);
            } else {
                assert!(!result.unwrap().enrollment.unwrap().enabled);
                // Lost response recovery remains safe even after entry removal.
                connection.disconnect(1, &key, entry.clone()).await.unwrap();
                assert!(client_config::inspect(&connection.configuration)
                    .unwrap()
                    .is_none());
            }
            if already_stopped {
                let stopped = command(Path::new(BINARY), &fixture.base(), "--server-stop");
                assert!(!stopped.status.success());
                assert!(String::from_utf8_lossy(&stopped.stderr).contains("server_stopped"));
            } else {
                fixture.run("--server-stop");
            }
            fixture.owner_settled();
            let directory = RuntimeDirectory::open(&fixture.base().join("m8-fake-v1")).unwrap();
            let marker = directory.read("stopped.json", 4096).unwrap().unwrap();
            let denied = headers_output(&fixture, entry.url());
            assert!(!denied.status.success());
            assert!(denied.stdout.is_empty());
            let reopened = command(Path::new(BINARY), &fixture.base(), "--server-start");
            assert!(!reopened.status.success());
            assert_eq!(
                directory.read("stopped.json", 4096).unwrap().unwrap(),
                marker
            );
            assert_eq!(mcp_management(&fixture, &["status"])["enabled"], false);
            // Explicit Resume restores GUI service, never MCP enrollment.
            fixture.run("--server-resume");
            let after = state(&fixture.client().await).await;
            assert_ne!(before["server_epoch"], after["server_epoch"]);
            assert_eq!(before["devices"], after["devices"]);
            let denied = headers_output(&fixture, entry.url());
            assert!(!denied.status.success());
            assert!(denied.stdout.is_empty());
        }
    }
}

#[tokio::test]
async fn crash_after_durable_disable_keeps_leftover_entry_inert_until_explicit_repair() {
    use samsung_tv_remote::server::mcp::{
        client_config,
        connection::{Cause, Connection},
    };
    use samsung_tv_remote::server::profile::Profile;

    let fixture = Fixture::new();
    let connection = Connection {
        profile: Profile::development(&fixture.base()).unwrap(),
        executable: PathBuf::from(BINARY),
        configuration: fixture.base().join(".claude.json"),
    };
    let connect_key = uuid::Uuid::new_v4().to_string();
    let entry = connection
        .connect(0, &connect_key, None)
        .await
        .unwrap()
        .entry
        .unwrap();
    let base = fixture.base();
    let mut child = DirectChild(
        Command::new(BINARY)
            .arg("--server")
            .arg("--fake-profile")
            .arg(&base)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let directory = RuntimeDirectory::open(&base.join("m8-fake-v1")).unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "fixture owner exited during startup"
            );
            if matches!(directory.lock(), Err(FileError::Locked)) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let client = launch::Launcher::development(&base)
        .unwrap()
        .start(Path::new(BINARY), Role::Gui)
        .await
        .unwrap();
    let before = state(&client).await;
    let old_headers = headers_output(&fixture, entry.url());
    assert!(old_headers.status.success());

    // Disconnect commits revocation before editing client configuration.
    // Kill the isolated owner at that durable boundary, retaining the entry.
    let disconnect_key = uuid::Uuid::new_v4().to_string();
    let disabled = mcp_management(&fixture, &["disable", "1", &disconnect_key]);
    assert_eq!(disabled["generation"], "2");
    assert_eq!(disabled["enabled"], false);
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    fixture.owner_settled();
    assert!(client_config::inspect(&connection.configuration).unwrap() == Some(entry.value()));
    let denied = headers_output(&fixture, entry.url());
    assert!(!denied.status.success());
    assert!(denied.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&denied.stderr).unwrap()["error"],
        "mcp_not_enrolled"
    );
    assert_eq!(
        mcp_management(&fixture, &["status"])["server_running"],
        false
    );

    // A delayed retry of the original Connect cannot regrant after restart.
    let stale = connection.connect(0, &connect_key, None).await.unwrap_err();
    assert_eq!(stale.cause, Cause::StaleGeneration);
    assert_eq!(
        stale.progress.configuration,
        samsung_tv_remote::server::mcp::connection::Configuration::Unchanged
    );
    fixture.run("--server-start");
    let after = state(&fixture.client().await).await;
    assert_ne!(after["server_epoch"], before["server_epoch"]);
    assert_eq!(mcp_management(&fixture, &["status"])["enabled"], false);
    assert!(!headers_output(&fixture, entry.url()).status.success());

    // Repeating the lost Disconnect completes only entry removal. Recovery
    // then requires a fresh explicit Connect, producing a different grant.
    connection
        .disconnect(1, &disconnect_key, entry)
        .await
        .unwrap();
    assert!(client_config::inspect(&connection.configuration)
        .unwrap()
        .is_none());
    let repaired = connection
        .connect(2, &uuid::Uuid::new_v4().to_string(), None)
        .await
        .unwrap();
    let new_headers = headers_output(&fixture, repaired.entry.unwrap().url());
    assert!(new_headers.status.success());
    // Never print credential-bearing output, including in assertion failures.
    assert!(old_headers.stdout != new_headers.stdout);
    assert_eq!(mcp_management(&fixture, &["status"])["generation"], "3");
}

#[tokio::test]
async fn concurrent_header_helpers_cold_start_one_owner_and_reuse_one_mcp_bearer() {
    let fixture = Fixture::new();
    let prepared = mcp_management(&fixture, &["prepare"]);
    let url = prepared["url"].as_str().unwrap();
    mcp_management(
        &fixture,
        &["enable", "0", &uuid::Uuid::new_v4().to_string()],
    );
    let outputs = std::thread::scope(|scope| {
        let jobs: Vec<_> = (0..3)
            .map(|_| scope.spawn(|| headers_output(&fixture, url)))
            .collect();
        jobs.into_iter()
            .map(|job| job.join().unwrap())
            .collect::<Vec<_>>()
    });
    let mut first: Option<Value> = None;
    for output in outputs {
        // Helper stderr contains only fixed error codes. Never include its
        // credential-bearing stdout in a failure diagnostic.
        assert!(
            output.status.success(),
            "helper failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let headers: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(headers.as_object().unwrap().len(), 1);
        assert!(headers["Authorization"]
            .as_str()
            .unwrap()
            .starts_with("Bearer "));
        if let Some(first) = &first {
            assert!(headers == *first);
        } else {
            first = Some(headers);
        }
    }
    let client = fixture.client().await;
    assert_eq!(state(&client).await["devices"], json!([]));
    let directory = RuntimeDirectory::open(&fixture.base().join("m8-fake-v1")).unwrap();
    assert!(directory.lock().is_err());
    assert!(Manifest::read(&directory).unwrap().unwrap().mcp_url == url);
}

#[tokio::test]
async fn header_helper_withholds_credentials_on_wrong_destination_disable_and_sticky_stop() {
    let fixture = Fixture::new();
    fixture.run("--server-start");
    let gui = fixture.client().await;
    let state = state(&gui).await;
    let directory = RuntimeDirectory::open(&fixture.base().join("m8-fake-v1")).unwrap();
    let url = Manifest::read(&directory).unwrap().unwrap().mcp_url;
    for destination in [
        url.clone(),
        "https://example.invalid/mcp".into(),
        format!("{url}?redirect=1"),
        String::new(),
    ] {
        let denied = headers_output(&fixture, &destination);
        assert!(!denied.status.success());
        assert!(denied.stdout.is_empty());
    }
    let enable = json!({"server_epoch":state["server_epoch"],"operation_key":uuid::Uuid::new_v4().to_string(),"expected_generation":"0","enabled":true});
    assert_eq!(
        gui.call("POST", "/v1/mcp/enrollment", Some(&enable))
            .await
            .unwrap()
            .0,
        200
    );
    let first = headers_output(&fixture, &url);
    assert!(first.status.success());
    assert!(first.stderr.is_empty());
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first.as_object().unwrap().len(), 1);
    let bearer = first["Authorization"].as_str().unwrap();
    assert!(bearer.starts_with("Bearer "));
    assert_eq!(bearer.len(), 71);
    let again = headers_output(&fixture, &url);
    assert!(again.status.success());
    let again: Value = serde_json::from_slice(&again.stdout).unwrap();
    // Avoid printing the header, even on a test failure.
    assert!(again["Authorization"] == first["Authorization"]);
    let denied = headers_output(&fixture, "https://example.invalid/mcp");
    assert!(!denied.status.success());
    assert!(denied.stdout.is_empty());
    fixture.run("--server-stop");
    fixture.owner_settled();
    let stopped = headers_output(&fixture, &url);
    assert!(!stopped.status.success());
    assert!(stopped.stdout.is_empty());
    let error: Value = serde_json::from_slice(&stopped.stderr).unwrap();
    assert_eq!(error["error"], "server_stopped");
    assert!(directory.read("stopped.json", 4096).unwrap().is_some());
}

#[tokio::test]
async fn mcp_enrollment_defaults_off_regrant_preserves_private_operations_and_disable_survives_restart(
) {
    use samsung_tv_remote::server::{bootstrap::BootstrapError, launch::LaunchError};
    let fixture = Fixture::new();
    fixture.run("--server-start");
    let gui = fixture.client().await;
    let state = state(&gui).await;
    let build = build_identity(Path::new(BINARY)).unwrap();
    assert!(matches!(
        launch::attach(&fixture.base(), Role::Mcp, Some(&build)).await,
        Err(LaunchError::Bootstrap(BootstrapError::Denied))
    ));
    let (status, initial) = gui.call("GET", "/v1/mcp", None).await.unwrap();
    assert_eq!(status, 200);
    assert_eq!(initial["enabled"], false);
    assert_eq!(initial["generation"], "0");
    let change = |generation: &str, enabled: bool| json!({"server_epoch":state["server_epoch"],"operation_key":uuid::Uuid::new_v4().to_string(),"expected_generation":generation,"enabled":enabled});
    let enable = change("0", true);
    let (status, first) = gui
        .call("POST", "/v1/mcp/enrollment", Some(&enable))
        .await
        .unwrap();
    assert_eq!(status, 200, "{first}");
    assert_eq!(first["enabled"], true);
    assert_eq!(first["generation"], "1");
    let mcp = launch::attach(&fixture.base(), Role::Mcp, Some(&build))
        .await
        .unwrap();
    assert_eq!(mcp.call("GET", "/v1/mcp", None).await.unwrap().0, 403);
    let probe = mutation(&mcp, "/v1/probes", json!({"host":"synthetic-tv.invalid"})).await;
    assert_eq!(probe["result"]["kind"], "probed");
    let (status, disabled) = gui
        .call("POST", "/v1/mcp/enrollment", Some(&change("1", false)))
        .await
        .unwrap();
    assert_eq!(status, 200, "{disabled}");
    assert_eq!(disabled["enabled"], false);
    assert_eq!(mcp.call("GET", "/v1/state", None).await.unwrap().0, 401);
    assert!(matches!(
        launch::attach(&fixture.base(), Role::Mcp, Some(&build)).await,
        Err(LaunchError::Bootstrap(BootstrapError::Denied))
    ));
    // Explicit regrant keeps the same principal and its retained operations,
    // while the previous bearer remains rejected.
    assert_eq!(
        gui.call("POST", "/v1/mcp/enrollment", Some(&change("2", true)))
            .await
            .unwrap()
            .0,
        200
    );
    let next = launch::attach(&fixture.base(), Role::Mcp, Some(&build))
        .await
        .unwrap();
    let (status, retained) = next
        .call(
            "GET",
            &format!(
                "/v1/operations/{}?epoch={}",
                probe["operation_id"].as_str().unwrap(),
                state["server_epoch"].as_str().unwrap()
            ),
            None,
        )
        .await
        .unwrap();
    assert_eq!(status, 200);
    assert_eq!(retained["operation_id"], probe["operation_id"]);
    assert_eq!(mcp.call("GET", "/v1/state", None).await.unwrap().0, 401);
    assert_eq!(
        gui.call("POST", "/v1/mcp/enrollment", Some(&change("3", false)))
            .await
            .unwrap()
            .0,
        200
    );
    fixture.run("--server-stop");
    fixture.owner_settled();
    fixture.run("--server-resume");
    let gui = fixture.client().await;
    let (status, persisted) = gui.call("GET", "/v1/mcp", None).await.unwrap();
    assert_eq!(status, 200);
    assert_eq!(persisted["enabled"], false);
    assert_eq!(persisted["generation"], "4");
    assert!(matches!(
        launch::attach(&fixture.base(), Role::Mcp, Some(&build)).await,
        Err(LaunchError::Bootstrap(BootstrapError::Denied))
    ));
}

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

#[test]
fn foreground_api_access_requires_an_explicit_closed_mode() {
    use samsung_tv_remote::server::entry::{parse, Mode};
    for (flag, request) in [("--inspect", false), ("--request-access", true)] {
        assert_eq!(
            parse(&["--server-api-access".into(), flag.into()]).unwrap(),
            Mode::ApiAccess { request }
        );
    }
    for arguments in [
        vec!["--server-api-access"],
        vec!["--server-api-access", "gui"],
        vec!["--server-api-access", "--request-access", "mcp"],
        vec!["--server-api-access", "--inspect", "--inherited-owner"],
        vec!["--server-api-access", "--fake-profile", "/private/tmp/test"],
    ] {
        let arguments = arguments.into_iter().map(Into::into).collect::<Vec<_>>();
        assert!(parse(&arguments).is_err());
    }
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
