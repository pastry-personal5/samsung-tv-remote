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
