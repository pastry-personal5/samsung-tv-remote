use samsung_tv_remote::application::certificate_trust::{CertificatePin, TrustError};
use samsung_tv_remote::application::control_service::*;
use samsung_tv_remote::application::credential_store::SecretError;
use samsung_tv_remote::application::device_repository::{RepositoryError, SavedDevice};
use samsung_tv_remote::application::remote_dispatcher::{
    DispatchRejection, NotSentReason, TerminalOutcome,
};
use samsung_tv_remote::application::remote_request::RemoteActionRejection;
use samsung_tv_remote::application::tv_address::{TargetError, TvHost};
use samsung_tv_remote::application::tv_control_coordinator::{
    ConnectFlowError, PairFlowError, WakeFailure, WakeStage,
};
use samsung_tv_remote::application::tv_discovery::DiscoveryError;
use samsung_tv_remote::application::tv_session::{ProbeObservation, TvSessionError};
use samsung_tv_remote::application::tv_setup_service::{ForgetResult, ReconnectError, SetupError};
use samsung_tv_remote::application::wake::WakeConfiguration;
use samsung_tv_remote::application::wake_transport::WakeSendError;
use samsung_tv_remote::domain::DeviceId;
use samsung_tv_remote::server::control_http::{response, wire};

fn saved() -> SavedDevice {
    SavedDevice {
        id: DeviceId::new(1),
        label: "Synthetic TV".into(),
        host: TvHost::parse("tv.local").unwrap(),
        wake: WakeConfiguration::default(),
    }
}
fn receipt(outcome: Option<Outcome>) -> Receipt {
    Receipt {
        id: OperationId(1),
        metadata: OperationMetadata {
            epoch: Epoch::from_u128(7),
            target: Some(DeviceId::new(1)),
            generation: Some(u64::MAX),
        },
        expires_at: outcome
            .as_ref()
            .map(|_| std::num::NonZeroU64::new(1234).unwrap()),
        outcome,
        terminal_state: None,
    }
}

#[test]
fn pending_terminal_and_retained_errors_preserve_metadata_without_debug_output() {
    let (status, pending) = response::receipt(&receipt(None));
    assert_eq!(status, 202);
    assert!(pending.get("expires_at_unix_ms").is_none());
    assert_eq!(pending["selection_generation"], u64::MAX.to_string());
    let (status, failed) =
        response::receipt(&receipt(Some(Outcome::Rejected(Rejection::StaleSelection))));
    assert_eq!(status, 409);
    assert_eq!(failed["error"]["code"], "stale_selection");
    assert_eq!(failed["expires_at_unix_ms"], "1234");
    assert_eq!(failed["operation_id"], wire::operation(OperationId(1)));
    assert_eq!(failed["device_id"], wire::device(DeviceId::new(1)));
    assert_eq!(failed["server_epoch"], wire::epoch(Epoch::from_u128(7)));
    assert!(!failed.to_string().contains("StaleSelection"));
}

#[test]
fn every_top_level_outcome_has_a_safe_terminal_representation() {
    let expires_at = std::num::NonZeroU64::new(1000).unwrap();
    let cases = [
        (
            Outcome::Discovered(vec![Candidate {
                id: CandidateId(OperationId(1), 0),
                host: TvHost::parse("tv.local").unwrap(),
                expires_at,
            }]),
            200,
            "discovered",
        ),
        (
            Outcome::Probed(Box::new(Observation {
                id: OperationId(1),
                host: TvHost::parse("tv.local").unwrap(),
                observation: ProbeObservation {
                    pin: CertificatePin::from_der(b"synthetic-certificate"),
                    name: None,
                    model: None,
                },
                expires_at,
            })),
            200,
            "probed",
        ),
        (Outcome::Paired(saved()), 200, "tv_approved_and_saved"),
        (Outcome::Saved(saved()), 200, "saved"),
        (Outcome::Connected, 200, "remote_ready"),
        (Outcome::Remote(TerminalOutcome::Written), 200, "written"),
        (
            Outcome::Remote(TerminalOutcome::Uncertain),
            200,
            "uncertain",
        ),
        (Outcome::Wake(WakeStage::Ready), 200, "wake"),
        (Outcome::PairFailed(PairFlowError::Cancelled), 409, "failed"),
        (
            Outcome::ConnectFailed(ConnectFlowError::Cancelled),
            409,
            "failed",
        ),
        (Outcome::Rejected(Rejection::Busy), 429, "failed"),
        (
            Outcome::DiscoveryFailed(DiscoveryError::Permission),
            403,
            "failed",
        ),
        (
            Outcome::DiscoveryFailed(DiscoveryError::Unavailable),
            503,
            "failed",
        ),
        (Outcome::ProbeFailed(TvSessionError::Offline), 503, "failed"),
        (
            Outcome::StorageFailed(RepositoryError::Unavailable),
            503,
            "failed",
        ),
        (
            Outcome::StorageFailed(RepositoryError::Corrupt),
            423,
            "failed",
        ),
        (Outcome::StorageUncertain, 503, "failed"),
        (
            Outcome::Forgotten(ForgetResult {
                credential_removed: true,
                trust_removed: true,
                record_removed: true,
            }),
            200,
            "forgotten",
        ),
        (
            Outcome::Forgotten(ForgetResult {
                credential_removed: false,
                trust_removed: true,
                record_removed: true,
            }),
            423,
            "partial_setup",
        ),
        (Outcome::Deadline, 503, "failed"),
        (Outcome::SupervisorFailed, 503, "failed"),
        (Outcome::Drained, 503, "failed"),
    ];
    for (value, status, kind) in cases {
        let (actual, body) = response::outcome(&value);
        assert_eq!(actual, status);
        assert_eq!(body["kind"], kind);
        assert!(!body.to_string().contains("synthetic-certificate"));
    }
}

#[test]
fn nested_setup_session_policy_and_service_failures_have_explicit_codes() {
    let session = [
        (
            TvSessionError::Target(TargetError::InvalidHost),
            "invalid_target",
        ),
        (
            TvSessionError::Target(TargetError::NonLocalAddress),
            "non_local_target",
        ),
        (
            TvSessionError::Target(TargetError::NoResolvedAddress),
            "target_unresolved",
        ),
        (TvSessionError::Offline, "offline"),
        (TvSessionError::Timeout, "timeout"),
        (TvSessionError::Tls, "tls_failed"),
        (TvSessionError::CertificateChanged, "certificate_changed"),
        (TvSessionError::RemoteChannel, "remote_channel_failed"),
        (TvSessionError::PairingDenied, "tv_denied"),
        (TvSessionError::TokenRejected, "pairing_required"),
        (TvSessionError::PairingTokenMissing, "pairing_token_missing"),
        (TvSessionError::Protocol, "tv_protocol_failed"),
        (TvSessionError::UncertainWrite, "uncertain"),
        (TvSessionError::UnsupportedAction, "unsupported_action"),
    ];
    for (value, code) in session {
        for outcome in [
            Outcome::PairFailed(PairFlowError::Session(value)),
            Outcome::ConnectFailed(ConnectFlowError::Session(value)),
            Outcome::ProbeFailed(value),
        ] {
            assert_eq!(response::outcome(&outcome).1["error"]["code"], code);
        }
    }
    for (value, code) in [
        (SetupError::InvalidLabel, "invalid_label"),
        (
            SetupError::Credential(SecretError::Unavailable),
            "storage_blocked",
        ),
        (
            SetupError::Credential(SecretError::InvalidToken),
            "invalid_pairing_material",
        ),
        (
            SetupError::Trust(TrustError::Unavailable),
            "trust_unavailable",
        ),
        (SetupError::Trust(TrustError::Corrupt), "trust_corrupt"),
        (
            SetupError::Preferences(RepositoryError::Unavailable),
            "storage_unavailable",
        ),
        (
            SetupError::Preferences(RepositoryError::Corrupt),
            "storage_corrupt",
        ),
        (SetupError::PartialCleanup, "partial_setup"),
    ] {
        assert_eq!(
            response::outcome(&Outcome::PairFailed(PairFlowError::Setup(value))).1["error"]["code"],
            code
        );
    }
    for value in [
        ReconnectError::Preferences(RepositoryError::Unavailable),
        ReconnectError::Preferences(RepositoryError::Corrupt),
        ReconnectError::Trust(TrustError::Unavailable),
        ReconnectError::Trust(TrustError::Corrupt),
        ReconnectError::Credential(SecretError::Unavailable),
        ReconnectError::Credential(SecretError::InvalidToken),
        ReconnectError::MissingDevice,
        ReconnectError::MissingTrust,
        ReconnectError::HostChanged,
        ReconnectError::PairingRequired,
    ] {
        assert!(
            response::outcome(&Outcome::ConnectFailed(ConnectFlowError::Reconnect(value))).1
                ["error"]["code"]
                .is_string()
        );
    }
    assert_eq!(
        response::outcome(&Outcome::ConnectFailed(ConnectFlowError::CredentialSave)).0,
        423
    );
    for reason in [
        RemoteActionRejection::NoSelectedTv,
        RemoteActionRejection::WrongTarget,
        RemoteActionRejection::StaleSelectionGeneration,
        RemoteActionRejection::PairingRequired,
        RemoteActionRejection::NotConnected,
    ] {
        assert!(response::outcome(&Outcome::Rejected(Rejection::Remote(
            DispatchRejection::Policy(reason)
        )))
        .1["error"]["code"]
            .is_string());
        assert_eq!(
            response::outcome(&Outcome::Remote(TerminalOutcome::NotSent(
                NotSentReason::Policy(reason)
            )))
            .1["kind"],
            "not_sent"
        );
    }
    for reason in [
        NotSentReason::Cancelled,
        NotSentReason::TransportUnavailable,
    ] {
        assert_eq!(
            response::outcome(&Outcome::Remote(TerminalOutcome::NotSent(reason))).1["kind"],
            "not_sent"
        );
    }
    for reason in [
        Rejection::Busy,
        Rejection::Blocked,
        Rejection::StaleSelection,
        Rejection::StaleConfiguration,
        Rejection::MissingDevice,
        Rejection::InvalidConfiguration,
        Rejection::Remote(DispatchRejection::Busy),
        Rejection::Oversize,
        Rejection::InvalidConfirmation,
        Rejection::WakeNotConfigured,
        Rejection::MissingOperation,
        Rejection::InvalidObservation,
    ] {
        assert!(response::outcome(&Outcome::Rejected(reason)).1["error"]["code"].is_string());
    }
    for reason in [
        ServiceError::Busy,
        ServiceError::Closed,
        ServiceError::StaleEpoch,
        ServiceError::InvalidKey,
        ServiceError::KeyConflict,
        ServiceError::RegistryFull,
        ServiceError::SubscriberLimit,
        ServiceError::ClientLimit,
        ServiceError::InvalidPayload,
        ServiceError::Unauthorized,
    ] {
        assert!(response::service_error(reason).body()["error"]["code"].is_string());
    }
}

#[test]
fn wake_packet_facts_survive_cancellation_and_do_not_claim_physical_power() {
    for (stage, packet, remote) in [
        (WakeStage::ConfigurationRequired, "not_sent", "not_ready"),
        (WakeStage::Idle, "not_sent", "not_ready"),
        (WakeStage::CheckingConnection, "not_sent", "unknown"),
        (WakeStage::PacketSending, "uncertain", "unknown"),
        (WakeStage::Reconnecting, "sent", "unknown"),
        (WakeStage::Connected, "not_sent", "ready"),
        (WakeStage::Ready, "sent", "ready"),
        (WakeStage::Cancelled, "not_sent", "unknown"),
        (WakeStage::CancelledDuringSend, "uncertain", "unknown"),
        (WakeStage::CancelledAfterSend, "sent", "unknown"),
    ] {
        let facts = response::wake(stage);
        assert_eq!(facts["packet"], packet);
        assert_eq!(facts["remote"], remote);
        assert!(facts.get("power").is_none());
    }
    for failure in [
        WakeFailure::Timeout,
        WakeFailure::PairingRequired,
        WakeFailure::Connection,
        WakeFailure::Local(WakeSendError::Cancelled),
        WakeFailure::Local(WakeSendError::InvalidTarget),
        WakeFailure::Local(WakeSendError::NoIpv4Route),
        WakeFailure::Local(WakeSendError::NoBroadcast),
        WakeFailure::Local(WakeSendError::Permission),
        WakeFailure::Local(WakeSendError::SendFailed),
    ] {
        assert_eq!(
            response::wake(WakeStage::FailedAfterSend(failure))["packet"],
            "sent"
        );
        assert!(response::wake(WakeStage::Failed(failure))["failure"].is_string());
    }
}

#[test]
fn maximum_profile_snapshot_and_sse_fit_the_bounded_output_budget() {
    use samsung_tv_remote::application::tv_control_coordinator::WakeStage;
    use samsung_tv_remote::ControlState;
    use std::sync::Arc;
    let host = [
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61),
    ]
    .join(".");
    let devices: Vec<_> = (0..1024)
        .map(|n| SavedDevice {
            id: DeviceId::new(n),
            label: "\u{0001}".repeat(256),
            host: TvHost::parse(&host).unwrap(),
            wake: WakeConfiguration {
                wired: Some("02:00:00:00:00:01".parse().unwrap()),
                wifi: Some("02:00:00:00:00:02".parse().unwrap()),
                active: Some(samsung_tv_remote::application::wake::WakeInterface::Wired),
            },
        })
        .collect();
    let revisions = Arc::new(devices.iter().map(|d| (d.id, u64::MAX)).collect());
    let snapshot = Snapshot {
        admission_closed: false,
        epoch: Epoch::from_u128(u128::MAX),
        revision: u64::MAX,
        startup: StartupState::Ready,
        control: ControlState::none(),
        devices: Arc::new(devices),
        configuration_revisions: revisions,
        duplicate_records: true,
        storage_pending: false,
        recovery: None,
        pairing_recovery: None,
        wake: WakeStage::ConfigurationRequired,
        wake_operation: None,
        pairing_pending: false,
    };
    let value = response::snapshot(&snapshot);
    let projected = samsung_tv_remote::control_client::projection::snapshot(&value).unwrap();
    assert_eq!(projected.devices, snapshot.devices);
    assert_eq!(
        projected.configuration_revisions,
        snapshot.configuration_revisions
    );
    assert_eq!(projected.epoch, snapshot.epoch);
    assert_eq!(projected.revision, u64::MAX);
    let encoded = response::encode(&value, response::MAX_OUTPUT_BYTES).unwrap();
    // Includes worst JSON escaping in every label and maximal host/counter size.
    assert!(encoded.len() > 2_000_000);
    assert!(encoded.len() + 128 < response::MAX_OUTPUT_BYTES);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&encoded).unwrap()["revision"],
        u64::MAX.to_string()
    );
    assert_eq!(
        response::encode(&value, 1024).unwrap_err().code,
        "response_too_large"
    );
    println!(
        "maximum-profile JSON={} bytes; SSE framing allowance=128 bytes; cap={} bytes",
        encoded.len(),
        response::MAX_OUTPUT_BYTES
    );
}

#[test]
fn native_setup_projections_reject_cross_operation_handles_and_oversized_metadata() {
    use samsung_tv_remote::control_client::projection::NativeReceipt;
    let expiry = std::num::NonZeroU64::new(1000).unwrap();
    let (_, value) = response::receipt(&receipt(Some(Outcome::Probed(Box::new(Observation {
        id: OperationId(1),
        host: TvHost::parse("synthetic-tv.invalid").unwrap(),
        observation: ProbeObservation {
            pin: CertificatePin::from_der(b"fake"),
            name: Some("Synthetic".into()),
            model: None,
        },
        expires_at: expiry,
    })))));
    let epoch = Epoch::from_u128(7);
    assert!(NativeReceipt::parse(&value, epoch)
        .unwrap()
        .observation()
        .is_ok());
    for field in ["attempt_id", "observation_id"] {
        let mut changed = value.clone();
        changed["result"][field] = serde_json::json!(wire::operation(OperationId(2)));
        assert!(NativeReceipt::parse(&changed, epoch)
            .unwrap()
            .observation()
            .is_err());
    }
    let mut invalid_expiry = value.clone();
    invalid_expiry["expires_at_unix_ms"] = serde_json::json!("0");
    assert!(NativeReceipt::parse(&invalid_expiry, epoch).is_err());
    let mut changed = value;
    changed["result"]["name"] = serde_json::json!("x".repeat(257));
    assert!(NativeReceipt::parse(&changed, epoch)
        .unwrap()
        .observation()
        .is_err());
    let (_, mut value) = response::receipt(&receipt(Some(Outcome::Discovered(vec![Candidate {
        id: CandidateId(OperationId(1), 0),
        host: TvHost::parse("synthetic-tv.invalid").unwrap(),
        expires_at: expiry,
    }]))));
    assert!(NativeReceipt::parse(&value, epoch)
        .unwrap()
        .candidates()
        .is_ok());
    value["result"]["candidates"][0]["candidate_id"] =
        serde_json::json!(wire::candidate(CandidateId(OperationId(2), 0)));
    assert!(NativeReceipt::parse(&value, epoch)
        .unwrap()
        .candidates()
        .is_err());
}
