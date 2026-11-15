//! Native encoder/transport/projection and MCP against one listener and owner.
use super::*;
use samsung_tv_remote::application::control_service::Request as Intent;
use samsung_tv_remote::application::remote_request::SendRemoteAction;
use samsung_tv_remote::control_client::{http, projection};

async fn native(h: &Harness, principal: u8, intent: &Intent) -> Value {
    let (method, path, body) = projection::encode(intent).unwrap();
    let (_, value) = http::request(
        http::Endpoint::parse(&format!("http://127.0.0.1:{}", h.port)).unwrap(),
        &format!("{principal:02x}").repeat(32),
        method,
        &path,
        Some(&body),
        None,
    )
    .await
    .unwrap();
    projection::NativeReceipt::parse(&value, intent.epoch).unwrap();
    value
}

fn intent(state: &Value, command: Command) -> Intent {
    Intent {
        epoch: wire::parse_epoch(state["server_epoch"].as_str().unwrap()).unwrap(),
        key: uuid::Uuid::new_v4().to_string(),
        command,
    }
}

#[tokio::test]
async fn mixed_client_load_preserves_recovery_events_management_and_cancelled_capacity() {
    use tokio::io::AsyncWriteExt;

    let h = Harness::new().await;
    let state = h.state().await;
    h.effects.probe_wait.store(true, Ordering::SeqCst);
    let key = uuid::Uuid::new_v4().to_string();
    let pending = h
        .tool(
            2,
            "probe_tv",
            json!({"server_epoch":state["server_epoch"],
        "operation_key":key,"host":"synthetic.invalid"}),
        )
        .await;
    assert_eq!(pending["state"], "pending");
    let response = h.response(1, "GET", "/v1/events", Value::Null).await;
    let mut events = Events {
        body: response.into_body(),
        buffer: Vec::new(),
    };
    assert_eq!(events.next().await.0, "snapshot");

    // Both roles exhaust their mutation reservations with incomplete bodies.
    // Observe admission before checking reserved read/management capacity.
    let mut stalled = Vec::new();
    for principal in [1, 2] {
        for _ in 0..MAX_MUTATIONS_PER_CLIENT {
            let mut socket = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, h.port))
                .await
                .unwrap();
            socket.write_all(format!("POST /v1/discoveries HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: 1000\r\n\r\n{{", h.port, format!("{principal:02x}").repeat(32)).as_bytes()).await.unwrap();
            stalled.push(socket);
        }
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let status = h
                    .json(principal, "POST", "/v1/discoveries", json!({}))
                    .await
                    .0;
                if status == 429 {
                    break;
                }
                assert_eq!(status, 400);
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    let waiter = h.mcp_done(2, &state["server_epoch"], &key);
    let progress = async {
        let list = h.tool(2, "list_tvs", json!({})).await;
        assert_eq!(list["server_epoch"], state["server_epoch"]);
        assert_eq!(h.json(1, "GET", "/v1/ready", Value::Null).await.0, 200);
        h.effects.probe_release.notify_one();
        let (_, event) = events.next().await;
        assert_eq!(event["server_epoch"], state["server_epoch"]);
    };
    let (done, ()) = tokio::join!(waiter, progress);
    assert_eq!(done["operation_id"], pending["operation_id"]);
    assert_eq!(h.effects.probes.load(Ordering::SeqCst), 1);
    assert_eq!(h.effects.discovery.load(Ordering::SeqCst), 0);

    // The fake management port records admission without shutting down this
    // fixture, so cancellation recovery can be checked on the same listener.
    let (status, _) = h
        .json(
            1,
            "POST",
            "/v1/server/stop",
            json!({"server_epoch":state["server_epoch"],
                "operation_key":uuid::Uuid::new_v4().to_string()}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(h.stops.0.load(Ordering::SeqCst), 1);

    // Cancelling the bodies returns capacity for each role; invalid probes
    // cannot accidentally dispatch an effect while waiting for that release.
    drop(stalled);
    for principal in [1, 2] {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let status = h
                    .json(principal, "POST", "/v1/discoveries", json!({}))
                    .await
                    .0;
                if status != 429 {
                    assert_eq!(status, 400);
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    let key = uuid::Uuid::new_v4().to_string();
    h.tool(
        2,
        "discover_tvs",
        json!({"server_epoch":state["server_epoch"],"operation_key":key}),
    )
    .await;
    assert_eq!(
        h.mcp_done(2, &state["server_epoch"], &key).await["result"]["kind"],
        "discovered"
    );
    assert_eq!(h.effects.discovery.load(Ordering::SeqCst), 1);

    assert_eq!(h.effects.writes.load(Ordering::SeqCst), 0);
    assert_eq!(h.effects.packets.load(Ordering::SeqCst), 0);
    drop(events);
    h.close().await;
}

#[tokio::test]
async fn captured_native_and_mcp_selection_and_configuration_guards_never_retarget() {
    let h = Harness::new().await;
    let device = h.pair().await;
    let state = h.state().await;
    let snapshot = projection::snapshot(&state).unwrap();
    let target = wire::parse_device(&device).unwrap();
    let generation = state["selection_generation"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let captured = intent(
        &state,
        Command::Remote(SendRemoteAction::new(
            target,
            generation,
            RemoteAction::VolumeUp,
        )),
    );
    let select_key = uuid::Uuid::new_v4().to_string();
    h.tool(2, "select_tv", json!({"server_epoch":state["server_epoch"],"operation_key":select_key,"device_id":device,"expected_generation":state["selection_generation"]})).await;
    h.mcp_done(2, &state["server_epoch"], &select_key).await;
    native(&h, 1, &captured).await;
    let rejected = h
        .terminal(1, state["server_epoch"].as_str().unwrap(), &captured.key)
        .await;
    assert_eq!(rejected["error"]["code"], "stale_selection");
    assert_eq!(h.effects.remote_attempts.load(Ordering::SeqCst), 0);

    let current = h.state().await;
    let current_snapshot = projection::snapshot(&current).unwrap();
    assert_eq!(snapshot.epoch, current_snapshot.epoch);
    assert!(current_snapshot.revision > snapshot.revision);
    let generation = current["selection_generation"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let update = intent(
        &current,
        Command::SetWake {
            target,
            generation,
            revision: 0,
            configuration: Default::default(),
        },
    );
    native(&h, 1, &update).await;
    let done = h
        .terminal(1, current["server_epoch"].as_str().unwrap(), &update.key)
        .await;
    assert_eq!(done["result"]["kind"], "saved");
    let stale_key = uuid::Uuid::new_v4().to_string();
    h.tool(2, "set_wake", json!({"server_epoch":current["server_epoch"],"operation_key":stale_key,"device_id":device,"selection_generation":current["selection_generation"],"expected_config_revision":"0"})).await;
    let stale = h.mcp_done(2, &current["server_epoch"], &stale_key).await;
    assert_eq!(stale["error"]["code"], "stale_config");
    // Compare only quiescent, identically tagged projections.
    let native_state = h.state().await;
    let mcp_state = h.tool(2, "list_tvs", json!({})).await;
    assert_eq!(native_state["server_epoch"], mcp_state["server_epoch"]);
    assert_eq!(native_state["revision"], mcp_state["revision"]);
    assert_eq!(native_state["selected"], mcp_state["selected"]);
    assert_eq!(
        native_state["selection_generation"],
        mcp_state["selection_generation"]
    );
    assert_eq!(h.effects.packets.load(Ordering::SeqCst), 0);
    h.close().await;
}

#[tokio::test]
async fn native_reply_loss_and_mcp_recovery_share_dedup_but_not_principal_ownership() {
    let h = Harness::new().await;
    let device = h.pair().await;
    let state = h.state().await;
    let target = wire::parse_device(&device).unwrap();
    let generation = state["selection_generation"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let request = intent(
        &state,
        Command::Remote(SendRemoteAction::new(
            target,
            generation,
            RemoteAction::VolumeUp,
        )),
    );
    // Pause the actual write after admission; discard its native receipt.
    h.effects.remote_mode.store(5, Ordering::SeqCst);
    native(&h, 2, &request).await;
    let (_, _, arguments) = projection::encode(&request).unwrap();
    let duplicate = h.tool(2, "remote_action", arguments.clone()).await;
    assert_eq!(duplicate["state"], "pending");
    assert_eq!(h.effects.remote_attempts.load(Ordering::SeqCst), 1);
    let private = h
        .tool(
            1,
            "get_operation",
            json!({"server_epoch":state["server_epoch"],"operation_key":request.key}),
        )
        .await;
    assert_eq!(private["state"], "unknown");
    h.effects.remote_release.notify_one();
    let done = h.mcp_done(2, &state["server_epoch"], &request.key).await;
    assert_eq!(done["result"]["kind"], "written");
    assert_eq!(duplicate["operation_id"], done["operation_id"]);
    let retained = native(&h, 2, &request).await;
    assert_eq!(retained["operation_id"], done["operation_id"]);
    let mut changed = arguments;
    changed["action"] = json!("VolumeDown");
    assert_eq!(
        h.tool(2, "remote_action", changed).await["error"]["code"],
        "operation_key_conflict"
    );
    assert_eq!(h.effects.writes.load(Ordering::SeqCst), 1);
    // Identical key text under the GUI principal is independent explicit intent.
    h.effects.remote_mode.store(0, Ordering::SeqCst);
    native(&h, 1, &request).await;
    let own = h
        .terminal(1, state["server_epoch"].as_str().unwrap(), &request.key)
        .await;
    assert_ne!(own["operation_id"], done["operation_id"]);
    assert_eq!(h.effects.writes.load(Ordering::SeqCst), 2);
    h.close().await;
}

#[tokio::test]
async fn attaching_gui_events_during_mcp_completion_cannot_lose_the_terminal_revision() {
    let h = Harness::new().await;
    h.effects.probe_wait.store(true, Ordering::SeqCst);
    let state = h.state().await;
    let key = uuid::Uuid::new_v4().to_string();
    let pending = h.tool(2, "probe_tv", json!({"server_epoch":state["server_epoch"],"operation_key":key,"host":"synthetic.invalid"})).await;
    let attach = h.response(1, "GET", "/v1/events", Value::Null);
    let complete = async {
        h.effects.probe_release.notify_one();
        h.mcp_done(2, &state["server_epoch"], &key).await
    };
    let (response, done) = tokio::join!(attach, complete);
    let final_state = h.state().await;
    let mut stream = Events {
        body: response.into_body(),
        buffer: Vec::new(),
    };
    let (name, mut observed) = stream.next().await;
    assert_eq!(name, "snapshot");
    let terminal_revision: u64 = final_state["revision"].as_str().unwrap().parse().unwrap();
    let mut revision: u64 = observed["revision"].as_str().unwrap().parse().unwrap();
    while revision < terminal_revision {
        let (_, event) = stream.next().await;
        let next: u64 = event["revision"].as_str().unwrap().parse().unwrap();
        assert_eq!(next, revision + 1);
        assert!(event.get("result").is_none());
        revision = next;
        observed = event;
    }
    assert_eq!(observed["server_epoch"], final_state["server_epoch"]);
    assert_eq!(done["operation_id"], pending["operation_id"]);
    assert_eq!(h.effects.probes.load(Ordering::SeqCst), 1);
    drop(stream);
    h.close().await;
}

#[tokio::test]
async fn partial_native_forget_is_shared_but_only_explicit_mcp_recovery_finishes_cleanup() {
    let h = Harness::new().await;
    let device = h.pair().await;
    let state = h.state().await;
    let target = wire::parse_device(&device).unwrap();
    *h.faults.secret_delete.error.lock().unwrap() = Some(SecretError::Unavailable);
    let request = intent(
        &state,
        Command::Forget {
            target,
            revision: 0,
            confirm_device: target,
        },
    );
    native(&h, 1, &request).await;
    let done = h
        .terminal(1, state["server_epoch"].as_str().unwrap(), &request.key)
        .await;
    assert_eq!(done["result"]["kind"], "partial_setup");
    assert_eq!(done["result"]["credential_removed"], false);
    let blocked = h.state().await;
    assert_eq!(blocked["startup"], "recovery_required");
    let list = h.tool(2, "list_tvs", json!({})).await;
    assert_eq!(list["startup"], blocked["startup"]);
    assert_eq!(list["revision"], blocked["revision"]);
    let deletes = h.faults.secret_delete.calls.load(Ordering::SeqCst);
    // Recovery queries and duplicates never retry a partial storage effect.
    native(&h, 1, &request).await;
    assert_eq!(h.faults.secret_delete.calls.load(Ordering::SeqCst), deletes);
    *h.faults.secret_delete.error.lock().unwrap() = None;
    let recovery_key = uuid::Uuid::new_v4().to_string();
    h.tool(
        2,
        "recover_forget",
        json!({"server_epoch":state["server_epoch"],"operation_key":recovery_key,
        "device_id":device,"confirm_device_id":device,
        "transaction_id":blocked["forget_recovery"]["transaction_id"]}),
    )
    .await;
    let recovered = h.mcp_done(2, &state["server_epoch"], &recovery_key).await;
    assert_eq!(recovered["result"]["kind"], "forgotten");
    assert_eq!(h.state().await["startup"], "ready");
    assert_eq!(h.effects.pairs.load(Ordering::SeqCst), 1);
    assert_eq!(h.effects.writes.load(Ordering::SeqCst), 0);
    assert_eq!(h.effects.packets.load(Ordering::SeqCst), 0);
    h.close().await;
}

#[tokio::test]
async fn capability_reads_share_owner_revision_and_session_fences_without_tv_effects() {
    let h = Harness::new().await;
    let device = h.pair().await;
    let response = h.response(1, "GET", "/v1/events", Value::Null).await;
    let mut events = Events {
        body: response.into_body(),
        buffer: Vec::new(),
    };
    let (_, initial) = events.next().await;
    let unknown = &initial["devices"][0]["capabilities"];
    assert_eq!(unknown["facts"].as_array().unwrap().len(), 8);
    assert!(unknown["facts"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["support"] == "unknown"));
    let sender = h.effects.sessions.lock().unwrap().last().unwrap().clone();
    sender.send(TvSessionEvent::ImeStarted).await.unwrap();
    let (_, started) = events.next().await;
    assert_eq!(started["devices"][0]["capabilities"]["ime_active"], true);
    let native = projection::snapshot(&started).unwrap();
    assert!(native.capabilities[0].ime_active);
    let inspected = h.tool(2, "inspect_tv", json!({"device_id":device})).await;
    let (_, detail) = h
        .json(1, "GET", &format!("/v1/devices/{device}"), Value::Null)
        .await;
    assert_eq!(inspected["revision"], started["revision"]);
    assert_eq!(inspected["tv"]["capabilities"], detail["capabilities"]);
    assert_eq!(
        detail["capabilities"],
        started["devices"][0]["capabilities"]
    );
    sender.send(TvSessionEvent::ImeStarted).await.unwrap();
    sender.send(TvSessionEvent::ImeEnded).await.unwrap();
    let (_, ended) = events.next().await;
    assert_eq!(ended["devices"][0]["capabilities"]["ime_active"], false);
    assert_eq!(
        ended["revision"].as_str().unwrap().parse::<u64>().unwrap(),
        native.revision + 1
    );
    sender.send(TvSessionEvent::ImeStarted).await.unwrap();
    events.next().await;
    // Same-TV selection still fences the live field and keeps bounded history.
    h.tool(2, "select_tv", json!({"server_epoch":ended["server_epoch"],"operation_key":uuid::Uuid::new_v4().to_string(),
        "device_id":device,"expected_generation":ended["selection_generation"]})).await;
    let fenced = loop {
        let (_, event) = events.next().await;
        if event["selection_generation"] != ended["selection_generation"] {
            break event;
        }
    };
    let summary = &fenced["devices"][0]["capabilities"];
    assert_eq!(summary["ime_active"], false);
    assert_eq!(summary["facts"][2]["freshness"], "stale");
    assert_eq!(summary["facts"][2]["support"], "supported");
    projection::snapshot(&fenced).unwrap();
    // A frame still queued on the retired stream cannot revive that session.
    let _ = sender.send(TvSessionEvent::ImeStarted).await;
    let current = h.state().await;
    assert_eq!(current["devices"][0]["capabilities"]["ime_active"], false);
    assert_eq!(h.effects.probes.load(Ordering::SeqCst), 1);
    assert_eq!(h.effects.connections.load(Ordering::SeqCst), 1);
    assert_eq!(h.effects.remote_attempts.load(Ordering::SeqCst), 0);
    assert_eq!(h.effects.writes.load(Ordering::SeqCst), 0);
    assert_eq!(h.effects.packets.load(Ordering::SeqCst), 0);
    drop(events);
    h.close().await;
}

#[tokio::test]
async fn capability_disconnect_and_token_rejection_keep_history_without_reconnect_or_replay() {
    for event in [TvSessionEvent::Disconnected, TvSessionEvent::TokenRejected] {
        let h = Harness::new().await;
        let id = h.pair().await;
        let response = h.response(1, "GET", "/v1/events", Value::Null).await;
        let mut events = Events {
            body: response.into_body(),
            buffer: Vec::new(),
        };
        events.next().await;
        let sender = h.effects.sessions.lock().unwrap().last().unwrap().clone();
        sender.send(TvSessionEvent::ImeStarted).await.unwrap();
        events.next().await;
        sender.send(event).await.unwrap();
        let (_, disconnected) = events.next().await;
        let summary = &disconnected["devices"][0]["capabilities"];
        assert_eq!(summary["reason"], "disconnected");
        assert_eq!(summary["ime_active"], false);
        assert_eq!(summary["facts"][2]["freshness"], "stale");
        assert_eq!(summary["facts"][2]["support"], "supported");
        let inspected = h.tool(2, "inspect_tv", json!({"device_id":id})).await;
        assert_eq!(&inspected["tv"]["capabilities"], summary);
        projection::snapshot(&disconnected).unwrap();
        assert_eq!(h.effects.connections.load(Ordering::SeqCst), 1);
        assert_eq!(h.effects.writes.load(Ordering::SeqCst), 0);
        drop(events);
        h.close().await;
    }
}
