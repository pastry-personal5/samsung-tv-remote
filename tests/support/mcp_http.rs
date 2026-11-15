//! MCP exercises the exact listener, fake effect ports and lifecycle used by /v1.
use super::*;
use samsung_tv_remote::application::device_repository::DeviceRepository;
use samsung_tv_remote::server::mcp::protocol::VERSION;

impl Harness {
    async fn rpc(&self, client: u8, method: &str, mut params: Value) -> (u16, Value) {
        params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":VERSION,"io.modelcontextprotocol/clientCapabilities":{}});
        let stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, self.port))
            .await
            .unwrap();
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let mut request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("host", format!("127.0.0.1:{}", self.port))
            .header(
                "authorization",
                format!("Bearer {}", format!("{client:02x}").repeat(32)),
            )
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", VERSION)
            .header("mcp-method", method);
        if let Some(name) = params.get("name").and_then(Value::as_str) {
            request = request.header("mcp-name", name);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let request = request
            .body(Full::new(Bytes::from(
                serde_json::to_vec(
                    &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
                )
                .unwrap(),
            )))
            .unwrap();
        let response = tokio::time::timeout(Duration::from_secs(3), sender.send_request(request))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.headers()["cache-control"], "no-store");
        let status = response.status().as_u16();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["id"], id, "{body}");
        (status, body)
    }
    pub(super) async fn tool(&self, client: u8, name: &str, arguments: Value) -> Value {
        let (status, body) = self
            .rpc(
                client,
                "tools/call",
                json!({"name":name,"arguments":arguments}),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["result"]["resultType"], "complete");
        let result = &body["result"];
        let data = result["structuredContent"].clone();
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(data, text);
        assert_eq!(result["isError"], data.get("error").is_some());
        let definition = samsung_tv_remote::server::mcp::catalog::tools()
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap();
        let validator = jsonschema::draft202012::options()
            .offline()
            .should_validate_formats(true)
            .build(&definition["outputSchema"])
            .unwrap();
        let errors: Vec<_> = validator
            .iter_errors(&data)
            .map(|error| error.to_string())
            .collect();
        assert!(
            errors.is_empty(),
            "{name} output schema: {errors:?}; {data}"
        );
        data
    }
    pub(super) async fn mcp_done(&self, client: u8, epoch: &Value, key: &str) -> Value {
        let value = self
            .tool(
                client,
                "get_operation",
                json!({"server_epoch":epoch,"operation_key":key,"wait_seconds":1}),
            )
            .await;
        assert_eq!(value["state"], "terminal", "{value}");
        value
    }
    async fn mcp_mutate(&self, name: &str, extra: Value) -> Value {
        let state = self.state().await;
        let key = uuid::Uuid::new_v4().to_string();
        let mut arguments = json!({"server_epoch":state["server_epoch"],"operation_key":key});
        arguments
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let first = self.tool(2, name, arguments).await;
        assert!(first.get("operation_id").is_some(), "{name}: {first}");
        assert_eq!(first["operation_key"], key);
        let done = self.mcp_done(2, &state["server_epoch"], &key).await;
        assert_eq!(done["operation_id"], first["operation_id"]);
        done
    }
}

#[tokio::test]
async fn native_verification_uses_authenticated_mcp_headers_without_tv_effects() {
    let h = Harness::new().await;
    let endpoint = samsung_tv_remote::control_client::http::Endpoint::parse(&format!(
        "http://127.0.0.1:{}",
        h.port
    ))
    .unwrap();
    let request = json!({"jsonrpc":"2.0","id":"native-verify","method":"tools/list","params":{"_meta":{
        "io.modelcontextprotocol/protocolVersion":VERSION,"io.modelcontextprotocol/clientCapabilities":{}}}});
    let (status, value) = samsung_tv_remote::control_client::http::request(
        endpoint,
        &"02".repeat(32),
        "POST",
        "/mcp",
        Some(&request),
        None,
    )
    .await
    .unwrap();
    assert_eq!(status, 200);
    assert_eq!(value["id"], "native-verify");
    assert_eq!(
        value["result"]["tools"],
        *samsung_tv_remote::server::mcp::catalog::tools()
    );
    assert_eq!(h.effects.probes.load(Ordering::SeqCst), 0);
    assert_eq!(h.effects.writes.load(Ordering::SeqCst), 0);
    h.close().await;
}

#[tokio::test]
async fn probe_fast_path_preserves_pending_receipt_and_retained_observation_without_replay() {
    let h = Harness::new().await;
    let state = h.state().await;
    let key = uuid::Uuid::new_v4().to_string();
    let arguments = json!({"server_epoch":state["server_epoch"],"operation_key":key,"host":"synthetic-tv.invalid"});
    h.effects.probe_wait.store(true, Ordering::SeqCst);
    // A slow effect cannot make the initial receipt wait for its completion.
    let pending = h.tool(2, "probe_tv", arguments.clone()).await;
    assert_eq!(pending["state"], "pending");
    assert_eq!(pending["operation_key"], key);
    h.effects.probe_release.notify_one();
    let done = h.mcp_done(2, &state["server_epoch"], &key).await;
    let duplicate = h.tool(2, "probe_tv", arguments).await;
    assert_eq!(duplicate, done);
    assert_eq!(duplicate["operation_id"], pending["operation_id"]);
    assert_eq!(h.effects.probes.load(Ordering::SeqCst), 1);
    assert!(duplicate["result"]["fingerprint"].is_string());
    assert!(duplicate["result"]["observation_id"].is_string());
    h.close().await;
}

#[tokio::test]
async fn first_call_setup_selection_configuration_control_and_forget_use_shared_service() {
    let h = Harness::new().await;
    let initial = h.tool(2, "list_tvs", json!({})).await;
    assert_eq!(initial["tvs"], json!([]));
    assert_eq!(initial["startup"], "ready");
    let (_, catalog) = h.rpc(2, "tools/list", json!({})).await;
    assert_eq!(catalog["result"]["tools"].as_array().unwrap().len(), 15);
    assert_eq!(catalog["result"]["cacheScope"], "private");
    let (_, discovered) = h.rpc(2, "server/discover", json!({})).await;
    assert_eq!(discovered["result"]["supportedVersions"], json!([VERSION]));
    assert!(discovered["result"]["capabilities"]
        .get("subscriptions")
        .is_none());
    let discovery = h.mcp_mutate("discover_tvs", json!({})).await;
    let candidate = &discovery["result"]["candidates"][0];
    let probe = h
        .mcp_mutate(
            "probe_tv",
            json!({"host":candidate["host"],"candidate_id":candidate["candidate_id"]}),
        )
        .await;
    let observed = &probe["result"];
    let paired = h.mcp_mutate("pair_tv", json!({"host":observed["host"],"attempt_id":observed["attempt_id"],"observation_id":observed["observation_id"],"fingerprint":observed["fingerprint"],"confirm_fingerprint":observed["fingerprint"],"label":"Synthetic MCP TV","selection_generation":initial["selection_generation"]})).await;
    assert_eq!(paired["result"]["kind"], "tv_approved_and_saved");
    let device = &paired["result"]["device_id"];
    let state = h.state().await;
    assert_eq!(&state["selected"], device);
    let selected = h
        .mcp_mutate(
            "select_tv",
            json!({"device_id":device,"expected_generation":state["selection_generation"]}),
        )
        .await;
    assert_eq!(selected["terminal_state"]["selection_generation"], "2");
    let generation = &selected["terminal_state"]["selection_generation"];
    let connected = h
        .mcp_mutate(
            "connect_tv",
            json!({"device_id":device,"selection_generation":generation}),
        )
        .await;
    assert_eq!(connected["result"]["kind"], "remote_ready");
    let saved = h.mcp_mutate("set_wake", json!({"device_id":device,"selection_generation":generation,"expected_config_revision":"0","wired_mac":"02:00:00:00:00:01","active_interface":"Wired"})).await;
    assert_eq!(saved["result"]["kind"], "saved");
    let inspected = h.tool(2, "inspect_tv", json!({"device_id":device})).await;
    assert_eq!(inspected["tv"]["config_revision"], "1");
    assert_eq!(inspected["connection"], "ready");
    let written = h
        .mcp_mutate(
            "remote_action",
            json!({"device_id":device,"selection_generation":generation,"action":"VolumeUp"}),
        )
        .await;
    assert_eq!(written["result"]["kind"], "written");
    assert_eq!(h.effects.writes.load(Ordering::SeqCst), 1);
    let wake = h
        .mcp_mutate(
            "wake_tv",
            json!({"device_id":device,"selection_generation":generation}),
        )
        .await;
    // The shared policy rejects explicit Wake while already remote-ready.
    assert_eq!(wake["error"]["code"], "storage_blocked");
    let cancelled = h.mcp_mutate("cancel_wake", json!({"device_id":device,"selection_generation":generation,"operation_id":written["operation_id"]})).await;
    assert_eq!(cancelled["error"]["code"], "not_found");
    let forgotten = h
        .mcp_mutate(
            "forget_tv",
            json!({"device_id":device,"expected_config_revision":"1","confirm_device_id":device}),
        )
        .await;
    assert_eq!(forgotten["result"]["kind"], "forgotten");
    assert_eq!(h.state().await["devices"], json!([]));
    let (_, after) = h.rpc(2, "tools/list", json!({})).await;
    assert_eq!(after["result"]["tools"], catalog["result"]["tools"]);
    h.close().await;
}

#[tokio::test]
async fn lost_receipts_cross_route_keys_private_lookup_and_uncertainty_do_not_replay() {
    let h = Harness::new().await;
    let device = h.pair().await;
    let state = h.state().await;
    let key = uuid::Uuid::new_v4().to_string();
    let arguments = json!({"server_epoch":state["server_epoch"],"operation_key":key,"device_id":device,"selection_generation":state["selection_generation"],"action":"VolumeUp"});
    // Discard the initial reply, then recover with only the original key.
    h.tool(2, "remote_action", arguments.clone()).await;
    let done = h.mcp_done(2, &state["server_epoch"], &key).await;
    assert_eq!(done["result"]["kind"], "written");
    let (status, duplicate) = h
        .json(2, "POST", "/v1/remote-actions", arguments.clone())
        .await;
    assert_eq!(status, 200);
    assert_eq!(duplicate["operation_id"], done["operation_id"]);
    let duplicate = h.tool(2, "remote_action", arguments.clone()).await;
    assert_eq!(duplicate["operation_id"], done["operation_id"]);
    assert_eq!(h.effects.writes.load(Ordering::SeqCst), 1);
    let mut changed = arguments;
    changed["action"] = json!("VolumeDown");
    let conflict = h.tool(2, "remote_action", changed).await;
    assert_eq!(conflict["error"]["code"], "operation_key_conflict");
    let private = h
        .tool(
            1,
            "get_operation",
            json!({"server_epoch":state["server_epoch"],"operation_id":done["operation_id"]}),
        )
        .await;
    assert_eq!(private["state"], "unknown");
    assert!(private.get("result").is_none());
    let mut old_epoch =
        json!({"server_epoch":uuid::Uuid::new_v4().to_string(),"operation_key":key});
    let unknown = h.tool(2, "get_operation", old_epoch.clone()).await;
    assert_eq!(unknown["error"]["code"], "unknown_after_restart");
    old_epoch["server_epoch"] = state["server_epoch"].clone();
    old_epoch["operation_id"] = done["operation_id"].clone();
    let invalid = h.tool(2, "get_operation", old_epoch).await;
    assert_eq!(invalid["error"]["code"], "invalid_request");
    h.effects.remote_mode.store(2, Ordering::SeqCst);
    let uncertain = h.mcp_mutate("remote_action", json!({"device_id":device,"selection_generation":state["selection_generation"],"action":"VolumeUp"})).await;
    assert_eq!(uncertain["result"]["kind"], "uncertain");
    assert_eq!(h.effects.remote_attempts.load(Ordering::SeqCst), 2);
    h.close().await;
}

#[tokio::test]
async fn re_pair_after_certificate_change_requires_fresh_matching_confirmation_and_keeps_identity()
{
    let h = Harness::new().await;
    let device = h.pair().await;
    let state = h.state().await;
    let old = h.mcp_mutate("probe_tv", json!({"host":"tv.local"})).await;
    h.effects.changed_certificate.store(true, Ordering::SeqCst);
    let fresh = h.mcp_mutate("probe_tv", json!({"host":"tv.local"})).await;
    let observed = &fresh["result"];
    assert_ne!(old["result"]["fingerprint"], observed["fingerprint"]);
    let mut arguments = json!({"device_id":device,"host":"tv.local",
        "attempt_id":observed["attempt_id"],"observation_id":observed["observation_id"],
        "fingerprint":observed["fingerprint"],"confirm_fingerprint":old["result"]["fingerprint"],
        "label":"Repaired synthetic TV","selection_generation":state["selection_generation"]});
    let before = h.effects.pairs.load(Ordering::SeqCst);
    let mut rejected_arguments = arguments.clone();
    rejected_arguments["server_epoch"] = state["server_epoch"].clone();
    rejected_arguments["operation_key"] = json!(uuid::Uuid::new_v4().to_string());
    let rejected = h.tool(2, "pair_tv", rejected_arguments).await;
    assert_eq!(rejected["error"]["code"], "invalid_confirmation");
    assert_eq!(h.effects.pairs.load(Ordering::SeqCst), before);
    arguments["confirm_fingerprint"] = observed["fingerprint"].clone();
    let repaired = h.mcp_mutate("pair_tv", arguments).await;
    assert_eq!(repaired["result"]["kind"], "tv_approved_and_saved");
    assert_eq!(repaired["result"]["device_id"], device);
    assert_eq!(h.effects.pairs.load(Ordering::SeqCst), before + 1);
    assert_eq!(h.state().await["devices"].as_array().unwrap().len(), 1);
    h.close().await;
}

#[tokio::test]
async fn guarded_cleanup_targets_remain_inspectable_without_a_saved_row() {
    use samsung_tv_remote::application::recovery::*;
    for pairing in [false, true] {
        let target = DeviceId::generate();
        let transaction = DeviceId::generate();
        let h = Harness::seeded(RuntimeLimits::default(), false, |path| {
            let journal = LocalRecoveryJournal::new(path);
            if pairing {
                journal
                    .begin_pairing(
                        PairingMarker {
                            transaction,
                            target,
                            prior_selection: None,
                            repair: false,
                            backup: None,
                            stage: PairingStage::Prepared,
                            cleanup: None,
                        },
                        None,
                    )
                    .unwrap();
            } else {
                let mut marker = ForgetMarker::prepared(target);
                marker.transaction = transaction;
                journal.write(marker).unwrap();
            }
        })
        .await;
        let list = h.tool(2, "list_tvs", json!({})).await;
        assert_eq!(list["startup"], "recovery_required");
        assert_eq!(list["tvs"], json!([]));
        assert_eq!(
            list["recovery_targets"][0]["device_id"],
            wire::device(target)
        );
        let detail = h
            .tool(2, "inspect_tv", json!({"device_id":wire::device(target)}))
            .await;
        assert!(detail.get("error").is_none(), "{detail}");
        assert!(detail["tv"].is_null());
        let marker = &detail[if pairing {
            "pairing_recovery"
        } else {
            "forget_recovery"
        }];
        assert_eq!(marker["transaction_id"], wire::device(transaction));
        let name = if pairing {
            "recover_pairing"
        } else {
            "recover_forget"
        };
        let invalid = h.tool(2, name, json!({"server_epoch":list["server_epoch"],"operation_key":uuid::Uuid::new_v4().to_string(),"device_id":wire::device(target),"transaction_id":marker["transaction_id"],"confirm_device_id":wire::device(DeviceId::generate())})).await;
        assert_eq!(invalid["error"]["code"], "invalid_confirmation");
        assert!(invalid.get("operation_id").is_none());
        let done = h.mcp_mutate(name, json!({"device_id":wire::device(target),"transaction_id":marker["transaction_id"],"confirm_device_id":wire::device(target)})).await;
        assert_eq!(done["result"]["kind"], "forgotten");
        assert_eq!(h.effects.probes.load(Ordering::SeqCst), 0);
        assert_eq!(h.effects.connections.load(Ordering::SeqCst), 0);
        h.close().await;
    }
}

#[tokio::test]
async fn bounded_pages_are_complete_private_and_ignore_connection_only_revisions() {
    let h = Harness::seeded(RuntimeLimits::default(), false, |path| {
        let repository = LocalDeviceRepository::new(path);
        for n in 1..=10u64 {
            repository
                .save(
                    &samsung_tv_remote::application::device_repository::SavedDevice {
                        sources: Default::default(),
                        id: DeviceId::new(n),
                        label: format!("Synthetic {n}"),
                        host: TvHost::parse(&format!("synthetic-{n}.invalid")).unwrap(),
                        wake: Default::default(),
                    },
                )
                .unwrap();
        }
        repository.select(Some(DeviceId::new(10))).unwrap();
    })
    .await;
    let first = h.tool(2, "list_tvs", json!({"page_size":3})).await;
    assert_eq!(first["selected"], wire::device(DeviceId::new(10)));
    assert_eq!(first["tvs"].as_array().unwrap().len(), 3);
    let private = h
        .tool(
            1,
            "list_tvs",
            json!({"page_size":3,"cursor":first["next_cursor"]}),
        )
        .await;
    assert_eq!(private["error"]["code"], "stale_cursor");
    let mut altered = first["next_cursor"].as_str().unwrap().as_bytes().to_vec();
    altered[5] = if altered[5] == b'A' { b'B' } else { b'A' };
    let invalid = h
        .tool(
            2,
            "list_tvs",
            json!({"page_size":3,"cursor":String::from_utf8(altered).unwrap()}),
        )
        .await;
    assert_eq!(invalid["error"]["code"], "invalid_cursor");
    let mut ids: Vec<_> = first["tvs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["device_id"].clone())
        .collect();
    let mut page = first.clone();
    // A same-ID select changes global state/generation, not the saved list.
    h.mcp_mutate(
        "select_tv",
        json!({"device_id":first["selected"],"expected_generation":first["selection_generation"]}),
    )
    .await;
    while !page["next_cursor"].is_null() {
        page = h
            .tool(
                2,
                "list_tvs",
                json!({"page_size":3,"cursor":page["next_cursor"]}),
            )
            .await;
        assert!(page.get("error").is_none(), "{page}");
        ids.extend(
            page["tvs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v["device_id"].clone()),
        );
    }
    assert_eq!(ids.len(), 10);
    let distinct: std::collections::HashSet<_> = ids.iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(distinct.len(), 10);
    let state = h.state().await;
    h.mcp_mutate("set_wake", json!({"device_id":state["selected"],"selection_generation":state["selection_generation"],"expected_config_revision":"0","wired_mac":"02:00:00:00:00:01","active_interface":"Wired"})).await;
    let stale = h
        .tool(
            2,
            "list_tvs",
            json!({"page_size":3,"cursor":first["next_cursor"]}),
        )
        .await;
    assert_eq!(stale["error"]["code"], "stale_cursor");
    h.close().await;
}

#[tokio::test]
async fn mutation_saturation_does_not_block_post_recovery_classification() {
    use tokio::io::AsyncWriteExt;
    let h = Harness::new().await;
    let mut slow = Vec::new();
    for _ in 0..MAX_MUTATIONS_PER_CLIENT {
        let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, h.port))
            .await
            .unwrap();
        stream.write_all(format!("POST /v1/discoveries HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: 1000\r\n\r\n{{", h.port, "02".repeat(32)).as_bytes()).await.unwrap();
        slow.push(stream);
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if h.json(2, "POST", "/v1/discoveries", json!({})).await.0 == 429 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let list = h.tool(2, "list_tvs", json!({})).await;
    assert_eq!(list["startup"], "ready");
    let blocked = h.tool(2, "get_operation", json!({"server_epoch":list["server_epoch"],"operation_key":uuid::Uuid::new_v4().to_string()})).await;
    assert_eq!(blocked["state"], "unknown");
    assert_eq!(h.effects.discovery.load(Ordering::SeqCst), 0);
    drop(slow);
    // Closing incomplete requests must return their reservations. Wait on
    // the observable admission result, then exercise MCP through the same pool.
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let status = h.json(2, "POST", "/v1/discoveries", json!({})).await.0;
            if status != 429 {
                assert_eq!(status, 400);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let done = h.mcp_mutate("discover_tvs", json!({})).await;
    assert_eq!(done["result"]["kind"], "discovered");
    assert_eq!(h.effects.discovery.load(Ordering::SeqCst), 1);
    h.close().await;
}

#[tokio::test]
async fn cancelling_a_named_wake_preserves_uncertain_packet_facts_and_one_send() {
    let h = Harness::new().await;
    let device = h.pair().await;
    let state = h.state().await;
    h.mcp_mutate("set_wake", json!({"device_id":device,"selection_generation":state["selection_generation"],"expected_config_revision":"0","wired_mac":"02:00:00:00:00:01","active_interface":"Wired"})).await;
    h.mcp_mutate(
        "select_tv",
        json!({"device_id":device,"expected_generation":state["selection_generation"]}),
    )
    .await;
    let state = h.state().await;
    h.effects.wake_after_send_wait.store(true, Ordering::SeqCst);
    let key = uuid::Uuid::new_v4().to_string();
    let arguments = json!({"server_epoch":state["server_epoch"],"operation_key":key,"device_id":device,"selection_generation":state["selection_generation"]});
    let receipt = h.tool(2, "wake_tv", arguments.clone()).await;
    wait_until(|| h.effects.packets.load(Ordering::SeqCst) == 1).await;
    let pending = h
        .tool(
            2,
            "get_operation",
            json!({"server_epoch":state["server_epoch"],"operation_key":key}),
        )
        .await;
    assert_eq!(pending["state"], "pending");
    let duplicate = h.tool(2, "wake_tv", arguments).await;
    assert_eq!(duplicate["operation_id"], receipt["operation_id"]);
    let cancelled = h.mcp_mutate("cancel_wake", json!({"device_id":device,"selection_generation":state["selection_generation"],"operation_id":receipt["operation_id"]})).await;
    assert_eq!(
        cancelled["result"]["facts"]["stage"],
        "cancelled_during_send"
    );
    assert_eq!(cancelled["result"]["facts"]["packet"], "uncertain");
    let original = h.mcp_done(2, &state["server_epoch"], &key).await;
    assert_eq!(original["result"]["facts"], cancelled["result"]["facts"]);
    h.effects.wake_after_send_release.notify_one();
    wait_until(|| h.effects.wake_returned.load(Ordering::SeqCst) == 1).await;
    assert_eq!(h.effects.packets.load(Ordering::SeqCst), 1);
    h.close().await;
}

#[tokio::test]
async fn transport_and_confirmation_failures_never_reach_effect_ports() {
    let h = Harness::new().await;
    let state = h.state().await;
    let (status, denied) = h.json(3, "POST", "/mcp", json!({})).await;
    assert_eq!(status, 401);
    assert!(denied.get("id").is_none());
    let response = h.response(2, "GET", "/mcp", Value::Null).await;
    assert_eq!(response.status(), 405);
    assert_eq!(response.headers()["allow"], "POST");
    let (_, stale) = h.rpc(2, "tools/call", json!({"name":"remote_action","arguments":{"server_epoch":uuid::Uuid::new_v4().to_string(),"operation_key":uuid::Uuid::new_v4().to_string(),"device_id":wire::device(DeviceId::new(1)),"selection_generation":"0","action":"VolumeUp"}})).await;
    assert_eq!(
        stale["result"]["structuredContent"]["error"]["code"],
        "stale_epoch"
    );
    let confirmation = h.tool(2, "pair_tv", json!({"server_epoch":state["server_epoch"],"operation_key":uuid::Uuid::new_v4().to_string(),"host":"synthetic.invalid","attempt_id":wire::operation(OperationId(1)),"observation_id":wire::operation(OperationId(1)),"fingerprint":"00".repeat(32),"confirm_fingerprint":"11".repeat(32),"label":"Synthetic","selection_generation":"0"})).await;
    assert_eq!(confirmation["error"]["code"], "invalid_confirmation");
    assert!(confirmation.get("operation_id").is_none());
    let bad_counter = h.tool(2, "select_tv", json!({"server_epoch":state["server_epoch"],"operation_key":uuid::Uuid::new_v4().to_string(),"device_id":wire::device(DeviceId::new(1)),"expected_generation":0})).await;
    assert_eq!(bad_counter["error"]["code"], "invalid_request");
    let unknown_field = h
        .tool(2, "list_tvs", json!({"execute":"PowerToggle"}))
        .await;
    assert_eq!(unknown_field["error"]["code"], "invalid_request");
    assert_eq!(h.effects.probes.load(Ordering::SeqCst), 0);
    assert_eq!(h.effects.connections.load(Ordering::SeqCst), 0);
    assert_eq!(h.effects.writes.load(Ordering::SeqCst), 0);
    assert_eq!(h.effects.packets.load(Ordering::SeqCst), 0);
    h.close().await;
}

#[tokio::test]
async fn inspection_bounds_unicode_labels_without_changing_saved_configuration() {
    let label = "한a".repeat(45);
    let h = Harness::seeded(RuntimeLimits::default(), false, |path| {
        LocalDeviceRepository::new(path)
            .save(
                &samsung_tv_remote::application::device_repository::SavedDevice {
                    sources: Default::default(),
                    id: DeviceId::new(1),
                    label: label.clone(),
                    host: TvHost::parse("synthetic.invalid").unwrap(),
                    wake: Default::default(),
                },
            )
            .unwrap();
    })
    .await;
    let inspected = h
        .tool(
            2,
            "inspect_tv",
            json!({"device_id":wire::device(DeviceId::new(1))}),
        )
        .await;
    let list = h.tool(2, "list_tvs", json!({})).await;
    assert_eq!(list["startup"], "ready", "{list}");
    assert!(inspected.get("error").is_none(), "{inspected}");
    assert_eq!(
        inspected["tv"]["label"],
        label.chars().take(80).collect::<String>()
    );
    assert_eq!(inspected["tv"]["label_truncated"], true);
    assert_eq!(inspected["tv"]["label"], list["tvs"][0]["label"]);
    let (_, native) = h
        .json(
            2,
            "GET",
            &format!("/v1/devices/{}", wire::device(DeviceId::new(1))),
            json!(null),
        )
        .await;
    assert_eq!(native["label"], label);
    h.close().await;
}

#[tokio::test]
async fn waiting_survives_an_overflowed_subscription_without_claiming_server_stop() {
    let h = Harness::with_limits(RuntimeLimits {
        subscriber_backlog: 1,
        ..Default::default()
    })
    .await;
    h.effects.probe_wait.store(true, Ordering::SeqCst);
    let state = h.state().await;
    let epoch = state["server_epoch"].clone();
    let key = uuid::Uuid::new_v4().to_string();
    let pending = h
        .tool(
            2,
            "probe_tv",
            json!({"server_epoch":epoch,"operation_key":key,"host":"synthetic-tv.invalid"}),
        )
        .await;
    assert_eq!(pending["state"], "pending", "{pending}");
    let Owner::Runtime(runtime) = &h.owner else {
        panic!("runtime harness")
    };
    let gui = runtime.client(ClientId(1));
    let current = gui.snapshot().await.unwrap().epoch;
    let burst = async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        // Queued together, these events overflow the one-slot waiter backlog.
        let submit = |key: &'static str| {
            gui.submit(samsung_tv_remote::application::control_service::Request {
                epoch: current,
                key: key.into(),
                command: Command::Discover,
            })
        };
        let _ = tokio::join!(submit("burst-1"), submit("burst-2"), submit("burst-3"));
        tokio::time::sleep(Duration::from_millis(100)).await;
        h.effects.probe_release.notify_one();
    };
    let (waited, ()) = tokio::join!(
        h.tool(
            2,
            "get_operation",
            json!({"server_epoch":epoch,"operation_key":key,"wait_seconds":2}),
        ),
        burst
    );
    assert!(waited.get("error").is_none(), "{waited}");
    assert_eq!(waited["state"], "terminal", "{waited}");
    assert_eq!(waited["result"]["kind"], "probed");
}

#[tokio::test]
async fn receipt_pages_continue_by_either_operation_selector() {
    let h = Harness::new().await;
    h.effects.second_host.store(true, Ordering::SeqCst);
    let discovery = h.mcp_mutate("discover_tvs", json!({})).await;
    let epoch = &discovery["server_epoch"];
    let first = h
        .tool(
            2,
            "get_operation",
            json!({"server_epoch":epoch,"operation_id":discovery["operation_id"],"page_size":1}),
        )
        .await;
    assert_eq!(first["result"]["candidates"].as_array().unwrap().len(), 1);
    let second = h
        .tool(
            2,
            "get_operation",
            json!({"server_epoch":epoch,"operation_key":discovery["operation_key"],"page_size":1,"cursor":first["next_cursor"]}),
        )
        .await;
    assert!(second.get("error").is_none(), "{second}");
    assert_ne!(
        second["result"]["candidates"][0],
        first["result"]["candidates"][0]
    );
    assert!(second["next_cursor"].is_null());
}

#[tokio::test]
async fn capability_inspection_budget_sample_is_bounded_and_keeps_fifteen_tools() {
    let h = Harness::new().await;
    let id = h.pair().await;
    let (status, list) = h.rpc(2, "tools/list", json!({})).await;
    assert_eq!(status, 200);
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 15);
    let (status, inspect) = h
        .rpc(
            2,
            "tools/call",
            json!({"name":"inspect_tv","arguments":{"device_id":id}}),
        )
        .await;
    assert_eq!(status, 200);
    let catalog_bytes = serde_json::to_vec(&list["result"]).unwrap().len();
    let inspection_bytes = serde_json::to_vec(&inspect["result"]).unwrap().len();
    assert!(inspection_bytes < 128 * 1024);
    println!("P3-M1 actual server JSON: tools/list={catalog_bytes} bytes; inspect_tv result (both channels)={inspection_bytes} bytes; this is not an installed-client token measurement");
    h.close().await;
}
