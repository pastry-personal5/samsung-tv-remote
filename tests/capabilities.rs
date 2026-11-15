use samsung_tv_remote::application::{
    capabilities::*, control_service::*, control_state::ControlState,
    device_repository::SavedDevice, tv_address::TvHost, tv_control_coordinator::WakeStage,
    wake::WakeConfiguration,
};
use samsung_tv_remote::domain::{capability::*, DeviceId};
use samsung_tv_remote::{control_client::projection, server::control_http::response};
use serde_json::{json, Value};
use std::sync::Arc;

fn snapshot() -> Snapshot {
    Snapshot {
        admission_closed: false,
        epoch: Epoch::from_u128(1),
        revision: 1,
        startup: StartupState::Ready,
        control: ControlState::none(),
        devices: Arc::new(vec![SavedDevice {
            id: DeviceId::new(1),
            label: "Synthetic TV".into(),
            host: TvHost::parse("synthetic.invalid").unwrap(),
            wake: WakeConfiguration::default(),
        }]),
        capabilities: Arc::new(vec![CapabilitySummary::unknown(
            DeviceId::new(1),
            CapabilityReason::Disconnected,
        )]),
        configuration_revisions: Arc::new(vec![(DeviceId::new(1), 0)]),
        duplicate_records: false,
        storage_pending: false,
        recovery: None,
        pairing_recovery: None,
        wake: WakeStage::Idle,
        wake_operation: None,
        pairing_pending: false,
    }
}
#[test]
fn compatible_absent_capabilities_are_unknown_but_malformed_or_invented_evidence_fails_closed() {
    let s = snapshot();
    let original = response::snapshot(&s);
    let decoded = projection::snapshot(&original).unwrap();
    assert_eq!(decoded.capabilities, s.capabilities);
    let mut old = original.clone();
    old["devices"][0]
        .as_object_mut()
        .unwrap()
        .remove("capabilities");
    assert!(projection::snapshot(&old).unwrap().capabilities[0]
        .facts
        .iter()
        .all(|f| f.support == Support::Unknown));
    for bad in [Value::Null, json!({}), json!([])] {
        let mut v = original.clone();
        v["devices"][0]["capabilities"] = bad;
        assert!(projection::snapshot(&v).is_err());
    }
    for (path, bad) in [
        ("support", json!("supported")),
        ("source", json!("tv_response")),
        ("observed_at_unix_ms", json!("10")),
        ("observed_at_unix_ms", json!(10)),
        ("freshness", json!("current")),
        ("key", json!("app_launch")),
    ] {
        let mut v = original.clone();
        v["devices"][0]["capabilities"]["facts"][0][path] = bad;
        assert!(projection::snapshot(&v).is_err(), "accepted {path}");
    }
    let mut v = original.clone();
    v["devices"][0]["capabilities"]["ime_active"] = json!(true);
    assert!(projection::snapshot(&v).is_err());
    let mut v = original.clone();
    v["devices"][0]["capabilities"]["device_id"] = json!("00000000-0000-0000-0000-000000000002");
    assert!(projection::snapshot(&v).is_err());
    let mut v = original;
    v["devices"][0]["capabilities"]["facts"][1] =
        v["devices"][0]["capabilities"]["facts"][0].clone();
    assert!(projection::snapshot(&v).is_err());
}

#[test]
fn maximal_history_is_lossless_and_fits_snapshot_and_sse_limits() {
    let mut s = snapshot();
    s.epoch = Epoch::from_u128(u128::MAX);
    let host = format!(
        "{}.{}.{}.{}",
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61)
    );
    let devices: Vec<_> = (1..=1024)
        .map(|id| SavedDevice {
            id: DeviceId::new(id),
            label: "\u{0001}".repeat(256),
            host: TvHost::parse(&host).unwrap(),
            wake: WakeConfiguration {
                wired: Some("02:00:00:00:00:01".parse().unwrap()),
                wifi: Some("02:00:00:00:00:02".parse().unwrap()),
                active: Some(samsung_tv_remote::application::wake::WakeInterface::Wired),
            },
        })
        .collect();
    let summaries = devices
        .iter()
        .map(|d| CapabilitySummary {
            device: d.id,
            reason: CapabilityReason::NotSelected,
            ime_active: false,
            facts: CapabilityKey::ALL.map(|key| CapabilityFact {
                key,
                support: Support::Supported,
                source: match key {
                    CapabilityKey::NativeIme => EvidenceSource::TvEvent,
                    _ => EvidenceSource::UserConfirmation,
                },
                observed_at_unix_ms: Some(u64::MAX),
                context: Some(EvidenceContext {
                    epoch: Epoch::from_u128(u128::MAX),
                    selection_generation: u64::MAX,
                    session: u64::MAX,
                }),
                freshness: Freshness::Stale,
            }),
        })
        .collect();
    s.configuration_revisions = Arc::new(devices.iter().map(|d| (d.id, u64::MAX)).collect());
    s.devices = Arc::new(devices);
    s.capabilities = Arc::new(summaries);
    let value = response::snapshot(&s);
    let bytes = serde_json::to_vec(&value).unwrap();
    println!(
        "maximal capability snapshot={} bytes; limit={}",
        bytes.len(),
        response::MAX_OUTPUT_BYTES
    );
    assert!(bytes.len() + 128 < response::MAX_OUTPUT_BYTES);
    let mut event = value.clone();
    event["recent_operations"] = json!((1..=64)
        .map(|id| samsung_tv_remote::server::control_http::wire::operation(OperationId(id)))
        .collect::<Vec<_>>());
    assert!(
        response::encode(&event, response::MAX_OUTPUT_BYTES)
            .unwrap()
            .len()
            + 128
            < response::MAX_OUTPUT_BYTES
    );
    let decoded = projection::snapshot(&value).unwrap();
    assert_eq!(decoded.capabilities, s.capabilities);
}

#[test]
fn current_evidence_requires_matching_epoch_selection_session_and_ready_owner() {
    use samsung_tv_remote::application::control_state::{ConnectionState, PairingState};
    let mut s = snapshot();
    s.control.select_device(s.devices[0].display());
    let generation = s.control.selection_generation();
    s.control.set_pairing_state(generation, PairingState::Ready);
    s.control
        .set_connection_state(generation, ConnectionState::Ready);
    let mut summary = CapabilitySummary::unknown(DeviceId::new(1), CapabilityReason::Connected);
    summary.facts[2] = CapabilityFact {
        key: CapabilityKey::NativeIme,
        support: Support::Supported,
        source: EvidenceSource::TvEvent,
        observed_at_unix_ms: Some(u64::MAX),
        freshness: Freshness::Current,
        context: Some(EvidenceContext {
            epoch: s.epoch,
            selection_generation: generation,
            session: u64::MAX,
        }),
    };
    summary.ime_active = true;
    s.capabilities = Arc::new(vec![summary]);
    let original = response::snapshot(&s);
    assert_eq!(
        projection::snapshot(&original).unwrap().capabilities,
        s.capabilities
    );
    for (field, bad) in [
        (
            "evidence_epoch",
            json!("00000000-0000-0000-0000-000000000002"),
        ),
        ("reason", json!("disconnected")),
    ] {
        let mut v = original.clone();
        v["devices"][0]["capabilities"][field] = bad;
        assert!(projection::snapshot(&v).is_err());
    }
    for (field, bad) in [
        ("selection_generation", json!("0")),
        ("session", json!("0")),
        ("session", json!(2)),
    ] {
        let mut v = original.clone();
        v["devices"][0]["capabilities"]["facts"][2]["context"][field] = bad;
        assert!(projection::snapshot(&v).is_err());
    }
    for (field, bad) in [
        ("connection", json!("failed")),
        ("pairing", json!("failed")),
        ("admission_closed", json!(true)),
        ("startup", json!("draining")),
        ("selected", Value::Null),
    ] {
        let mut v = original.clone();
        v[field] = bad;
        assert!(projection::snapshot(&v).is_err());
    }
}
