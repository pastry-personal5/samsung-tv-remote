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
            sources: Default::default(),
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
    // M2 worst case: every TV shows every port with a 64-byte label; only
    // the selected entry projects its labels.
    let label = samsung_tv_remote::application::sources::SourceLabel::parse(&"😀".repeat(16));
    let sources = samsung_tv_remote::application::sources::SourceConfiguration {
        ports: Box::new(std::array::from_fn(|_| {
            samsung_tv_remote::application::sources::SourceSetting::Shown {
                label: label.clone(),
                confirmed_at_unix_ms: u64::MAX,
            }
        })),
    };
    let devices: Vec<_> = (1..=1024)
        .map(|id| SavedDevice {
            sources: sources.clone(),
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
                // Durable source facts carry no session context.
                context: (!matches!(
                    key,
                    CapabilityKey::Hdmi1
                        | CapabilityKey::Hdmi2
                        | CapabilityKey::Hdmi3
                        | CapabilityKey::Tuner
                ))
                .then_some(EvidenceContext {
                    epoch: Epoch::from_u128(u128::MAX),
                    selection_generation: u64::MAX,
                    session: u64::MAX,
                }),
                freshness: Freshness::Stale,
            }),
        })
        .collect();
    s.configuration_revisions = Arc::new(devices.iter().map(|d| (d.id, u64::MAX)).collect());
    s.control.select_device(devices[0].display());
    s.devices = Arc::new(devices);
    s.capabilities = Arc::new(summaries);
    let value = response::snapshot(&s);
    assert!(value["devices"][0].get("sources").is_some());
    assert!(value["devices"][1].get("sources").is_none());
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

#[test]
fn durable_source_facts_decode_disconnected_and_sources_blocks_are_strict() {
    use samsung_tv_remote::application::sources::*;
    let mut s = snapshot();
    s.control.select_device(s.devices[0].display());
    let mut device = s.devices[0].clone();
    device.sources = SourceConfiguration::default().apply(
        [
            SourceChoice::Shown(SourceLabel::parse("PS5")),
            SourceChoice::Hidden,
            SourceChoice::Unconfigured,
            SourceChoice::Shown(None),
        ],
        10,
    );
    let mut summary = CapabilitySummary::unknown(DeviceId::new(1), CapabilityReason::Disconnected);
    overlay_sources(&mut summary, &device.sources, 20);
    s.devices = Arc::new(vec![device.clone()]);
    s.capabilities = Arc::new(vec![summary]);
    let value = response::snapshot(&s);
    let decoded = projection::snapshot(&value).unwrap();
    assert_eq!(decoded.devices[0].sources, device.sources);
    assert_eq!(decoded.capabilities, s.capabilities);
    // Durable facts stay current even while admission is closed.
    let mut closed = value.clone();
    closed["admission_closed"] = json!(true);
    assert!(projection::snapshot(&closed).is_ok());
    // An absent block is the old-response fallback; a present bad one fails.
    let mut old = value.clone();
    old["devices"][0].as_object_mut().unwrap().remove("sources");
    assert_eq!(
        projection::snapshot(&old).unwrap().devices[0].sources,
        SourceConfiguration::default()
    );
    for bad in [
        json!(null),
        json!({}),
        json!({"state":"shown","label":"PS5","confirmed_at_unix_ms":null}),
        json!({"state":"shown","label":" PS5","confirmed_at_unix_ms":"10"}),
        json!({"state":"hidden","label":null,"confirmed_at_unix_ms":"10"}),
    ] {
        let mut v = value.clone();
        if bad.is_object() && !bad.as_object().unwrap().is_empty() {
            v["devices"][0]["sources"]["hdmi1"] = bad;
        } else {
            v["devices"][0]["sources"] = bad;
        }
        assert!(projection::snapshot(&v).is_err(), "{v}");
    }
    // A durable Sources-menu fact, or one with a session-free app key, fails closed.
    let mut v = value;
    let facts = v["devices"][0]["capabilities"]["facts"]
        .as_array_mut()
        .unwrap();
    facts[7] = facts[3].clone();
    facts[7]["key"] = json!("sources");
    assert!(projection::snapshot(&v).is_err());
}
