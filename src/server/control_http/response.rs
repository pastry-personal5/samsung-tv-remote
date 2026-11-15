//! Explicit safe projections. No Debug strings, tokens or raw certificates.
use super::wire::{self, Counter};
use crate::application::certificate_trust::TrustError;
use crate::application::control_service::*;
use crate::application::control_state::{ConnectionState, PairingState, PlaybackState};
use crate::application::credential_store::SecretError;
use crate::application::device_repository::{RepositoryError, SavedDevice};
use crate::application::remote_dispatcher::{DispatchRejection, NotSentReason, TerminalOutcome};
use crate::application::remote_request::RemoteActionRejection;
use crate::application::tv_address::TargetError;
use crate::application::tv_control_coordinator::{
    ConnectFlowError, PairFlowError, WakeFailure, WakeStage,
};
use crate::application::tv_discovery::DiscoveryError;
use crate::application::tv_session::TvSessionError;
use crate::application::tv_setup_service::{ReconnectError, SetupError};
use crate::application::wake::WakeInterface;
use crate::application::wake_transport::WakeSendError;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorCode {
    pub status: u16,
    pub code: &'static str,
}
const fn error(status: u16, code: &'static str) -> ErrorCode {
    ErrorCode { status, code }
}
impl ErrorCode {
    pub fn body(self) -> Value {
        json!({"error":{"code":self.code}})
    }
}

pub fn service_error(value: ServiceError) -> ErrorCode {
    match value {
        ServiceError::Busy
        | ServiceError::RegistryFull
        | ServiceError::SubscriberLimit
        | ServiceError::ClientLimit => error(429, "busy"),
        ServiceError::Closed => error(503, "server_stopping"),
        ServiceError::StaleEpoch => error(409, "stale_epoch"),
        ServiceError::InvalidKey | ServiceError::InvalidPayload => error(400, "invalid_request"),
        ServiceError::KeyConflict => error(409, "operation_key_conflict"),
        ServiceError::Unauthorized => error(401, "unauthenticated"),
    }
}
fn repository(value: RepositoryError) -> ErrorCode {
    match value {
        RepositoryError::Unavailable => error(503, "storage_unavailable"),
        RepositoryError::Corrupt => error(423, "storage_corrupt"),
    }
}
fn secret(value: SecretError) -> ErrorCode {
    match value {
        SecretError::Unavailable => error(423, "storage_blocked"),
        SecretError::InvalidToken => error(423, "invalid_pairing_material"),
    }
}
fn trust(value: TrustError) -> ErrorCode {
    match value {
        TrustError::Unavailable => error(503, "trust_unavailable"),
        TrustError::Corrupt => error(423, "trust_corrupt"),
    }
}
fn session(value: TvSessionError) -> ErrorCode {
    match value {
        TvSessionError::Target(TargetError::InvalidHost) => error(400, "invalid_target"),
        TvSessionError::Target(TargetError::NonLocalAddress) => error(400, "non_local_target"),
        TvSessionError::Target(TargetError::NoResolvedAddress) => error(503, "target_unresolved"),
        TvSessionError::Offline => error(503, "offline"),
        TvSessionError::Timeout => error(503, "timeout"),
        TvSessionError::Tls => error(503, "tls_failed"),
        TvSessionError::CertificateChanged => error(412, "certificate_changed"),
        TvSessionError::RemoteChannel => error(503, "remote_channel_failed"),
        TvSessionError::PairingDenied => error(412, "tv_denied"),
        TvSessionError::TokenRejected => error(412, "pairing_required"),
        TvSessionError::PairingTokenMissing => error(412, "pairing_token_missing"),
        TvSessionError::Protocol => error(503, "tv_protocol_failed"),
        TvSessionError::UncertainWrite => error(503, "uncertain"),
        TvSessionError::UnsupportedAction => error(400, "unsupported_action"),
    }
}
fn policy(value: RemoteActionRejection) -> ErrorCode {
    match value {
        RemoteActionRejection::NoSelectedTv => error(409, "no_selected_tv"),
        RemoteActionRejection::WrongTarget | RemoteActionRejection::StaleSelectionGeneration => {
            error(409, "stale_selection")
        }
        RemoteActionRejection::PairingRequired => error(412, "pairing_required"),
        RemoteActionRejection::NotConnected => error(412, "remote_not_ready"),
    }
}
fn rejected(value: Rejection) -> ErrorCode {
    match value {
        Rejection::Busy | Rejection::Remote(DispatchRejection::Busy) => error(429, "busy"),
        Rejection::Blocked => error(423, "storage_blocked"),
        Rejection::StaleSelection => error(409, "stale_selection"),
        Rejection::StaleConfiguration => error(409, "stale_config"),
        Rejection::MissingDevice => error(404, "not_found"),
        Rejection::InvalidConfiguration => error(400, "invalid_configuration"),
        Rejection::Remote(DispatchRejection::Policy(reason)) => policy(reason),
        Rejection::Oversize => error(400, "input_too_large"),
        Rejection::InvalidConfirmation => error(400, "invalid_confirmation"),
        Rejection::WakeNotConfigured => error(412, "wake_not_configured"),
        Rejection::MissingOperation => error(404, "not_found"),
        Rejection::InvalidObservation => error(409, "observation_expired"),
    }
}
fn setup(value: SetupError) -> ErrorCode {
    match value {
        SetupError::InvalidLabel => error(400, "invalid_label"),
        SetupError::Credential(e) => secret(e),
        SetupError::Trust(e) => trust(e),
        SetupError::Preferences(e) => repository(e),
        SetupError::PartialCleanup => error(423, "partial_setup"),
    }
}
fn reconnect(value: ReconnectError) -> ErrorCode {
    match value {
        ReconnectError::Preferences(e) => repository(e),
        ReconnectError::Trust(e) => trust(e),
        ReconnectError::Credential(e) => secret(e),
        ReconnectError::MissingDevice => error(404, "not_found"),
        ReconnectError::MissingTrust | ReconnectError::PairingRequired => {
            error(412, "pairing_required")
        }
        ReconnectError::HostChanged => error(412, "certificate_changed"),
    }
}
fn wake_failure(value: WakeFailure) -> &'static str {
    match value {
        WakeFailure::Timeout => "timeout",
        WakeFailure::PairingRequired => "pairing_required",
        WakeFailure::Connection => "connection_failed",
        WakeFailure::Local(value) => match value {
            WakeSendError::Cancelled => "cancelled",
            WakeSendError::InvalidTarget => "invalid_target",
            WakeSendError::NoIpv4Route => "no_ipv4_route",
            WakeSendError::NoBroadcast => "no_broadcast",
            WakeSendError::Permission => "permission_denied",
            WakeSendError::SendFailed => "send_failed",
        },
    }
}

pub fn wake(value: WakeStage) -> Value {
    let (stage, packet, remote, failure) = match value {
        WakeStage::ConfigurationRequired => {
            ("configuration_required", "not_sent", "not_ready", None)
        }
        WakeStage::Idle => ("idle", "not_sent", "not_ready", None),
        WakeStage::CheckingConnection => ("checking_connection", "not_sent", "unknown", None),
        WakeStage::PacketSending => ("packet_sending", "uncertain", "unknown", None),
        WakeStage::Reconnecting => ("reconnecting", "sent", "unknown", None),
        WakeStage::Connected => ("connected", "not_sent", "ready", None),
        WakeStage::Ready => ("ready", "sent", "ready", None),
        WakeStage::Failed(f) => (
            "failed",
            if matches!(f, WakeFailure::Local(WakeSendError::SendFailed)) {
                "uncertain"
            } else {
                "not_sent"
            },
            "not_ready",
            Some(wake_failure(f)),
        ),
        WakeStage::FailedAfterSend(f) => (
            "failed_after_send",
            "sent",
            "not_ready",
            Some(wake_failure(f)),
        ),
        WakeStage::Cancelled => ("cancelled", "not_sent", "unknown", None),
        WakeStage::CancelledDuringSend => ("cancelled_during_send", "uncertain", "unknown", None),
        WakeStage::CancelledAfterSend => ("cancelled_after_send", "sent", "unknown", None),
    };
    json!({"stage":stage,"packet":packet,"remote":remote,"failure":failure})
}

pub fn device(value: &SavedDevice, revision: u64) -> Value {
    json!({"device_id":wire::device(value.id),"label":value.label,"host":value.host.as_str(),"config_revision":Counter(revision),
        "wake":{"wired_mac":value.wake.wired.map(|m|m.to_string()),"wifi_mac":value.wake.wifi.map(|m|m.to_string()),"active_interface":value.wake.active.map(|v| match v { WakeInterface::Wired => "Wired", WakeInterface::WiFi => "WiFi" })}})
}

pub fn capabilities(summary: &crate::application::capabilities::CapabilitySummary) -> Value {
    let facts: Vec<_> = summary.facts.iter().map(|fact| json!({
        "key":fact.key,"support":fact.support,"source":fact.source,"freshness":fact.freshness,
        "observed_at_unix_ms":fact.observed_at_unix_ms.map(Counter),
            "context":fact.context.map(|c| json!({
            "selection_generation":Counter(c.selection_generation),"session":Counter(c.session)}))
    })).collect();
    // All in-memory history belongs to one owner epoch. Encode it once per TV
    // to keep the full 1,024-record snapshot inside the existing output cap.
    let evidence_epoch = summary
        .facts
        .iter()
        .find_map(|f| f.context.map(|c| wire::epoch(c.epoch)));
    json!({"device_id":wire::device(summary.device),"evidence_epoch":evidence_epoch,"reason":summary.reason,"ime_active":summary.ime_active,"facts":facts})
}

/// Snapshot, detail and inspect share exactly one owner projection.
pub fn device_detail(value: &SavedDevice, snapshot: &Snapshot) -> Value {
    let revision = snapshot
        .configuration_revisions
        .iter()
        .find(|(id, _)| *id == value.id)
        .map_or(0, |(_, r)| *r);
    let mut result = device(value, revision);
    let fallback = crate::application::capabilities::CapabilitySummary::unknown(
        value.id,
        crate::domain::capability::CapabilityReason::EvidenceMissing,
    );
    result["capabilities"] = capabilities(
        snapshot
            .capabilities
            .iter()
            .find(|c| c.device == value.id)
            .unwrap_or(&fallback),
    );
    result
}

pub fn startup(value: StartupState) -> &'static str {
    match value {
        StartupState::Restoring => "restoring",
        StartupState::Ready => "ready",
        StartupState::Blocked(RepositoryError::Unavailable) => "storage_unavailable",
        StartupState::Blocked(RepositoryError::Corrupt) => "storage_corrupt",
        StartupState::RecoveryRequired => "recovery_required",
        StartupState::Draining => "draining",
    }
}

pub fn recovery(value: &Snapshot) -> Value {
    json!({"forget_recovery":value.recovery.map(|m|json!({"device_id":wire::device(m.target),"transaction_id":wire::device(m.transaction),"selection_cleared":m.selection_cleared,"credential_removed":m.credential_removed,"trust_removed":m.trust_removed,"record_removed":m.record_removed})),
        "pairing_recovery":value.pairing_recovery.map(|m|json!({"device_id":wire::device(m.target),"transaction_id":wire::device(m.transaction),"repair":m.repair,"prior_selection":m.prior_selection.map(wire::device),"backup":m.backup.map(wire::device),"stage":match m.stage {crate::application::recovery::PairingStage::Prepared=>"prepared",crate::application::recovery::PairingStage::TokenSaved=>"token_saved",crate::application::recovery::PairingStage::TrustSaved=>"trust_saved",crate::application::recovery::PairingStage::DeviceSaved=>"device_saved",crate::application::recovery::PairingStage::SelectionSaved=>"selection_saved"},"cleanup":m.cleanup.map(|c|json!({"credential_removed":c.credential_removed,"trust_removed":c.trust_removed,"record_removed":c.record_removed,"selection_restored":c.selection_restored}))}))})
}

pub fn snapshot(value: &Snapshot) -> Value {
    let startup = startup(value.startup);
    let devices: Vec<_> = value
        .devices
        .iter()
        .map(|d| device_detail(d, value))
        .collect();
    let recovery = recovery(value);
    json!({"server_epoch":wire::epoch(value.epoch),"revision":Counter(value.revision),"startup":startup,"admission_closed":value.admission_closed,
        "selected":value.control.selected_device().map(wire::device),"selection_generation":Counter(value.control.selection_generation()),
        "pairing": match value.control.pairing_state() { PairingState::NotStarted=>"not_started", PairingState::InProgress=>"in_progress", PairingState::Ready=>"ready", PairingState::Failed=>"failed" },
        "connection":match value.control.connection_state() {ConnectionState::NotConnected=>"not_connected",ConnectionState::Connecting=>"connecting",ConnectionState::Ready=>"ready",ConnectionState::Failed=>"failed"},
        "playback":match value.control.playback_state() {PlaybackState::Unknown=>"unknown",PlaybackState::AssumedPaused=>"assumed_paused",PlaybackState::AssumedPlaying=>"assumed_playing"},
        "devices":devices,"duplicate_records":value.duplicate_records,"storage_pending":value.storage_pending,"pairing_pending":value.pairing_pending,
        "wake":wake(value.wake),"wake_operation":value.wake_operation.map(wire::operation),
        "forget_recovery":recovery["forget_recovery"],"pairing_recovery":recovery["pairing_recovery"]})
}

/// Safe terminal details and HTTP status. Policy failures retain admission
/// metadata in the enclosing receipt, independently of the HTTP error status.
pub fn outcome(value: &Outcome) -> (u16, Value) {
    let failure = match value {
        Outcome::Discovered(candidates) => {
            return (
                200,
                json!({"kind":"discovered","candidates":candidates.iter().map(|c|json!({"candidate_id":wire::candidate(c.id),"host":c.host.as_str(),"expires_at_unix_ms":Counter(c.expires_at.get())})).collect::<Vec<_>>()}),
            )
        }
        Outcome::Probed(v) => {
            return (
                200,
                json!({"kind":"probed","attempt_id":wire::operation(v.id),"observation_id":wire::operation(v.id),"host":v.host.as_str(),"fingerprint":v.observation.pin.to_hex(),"name":v.observation.name,"model":v.observation.model,"expires_at_unix_ms":Counter(v.expires_at.get())}),
            )
        }
        Outcome::Paired(v) => {
            return (
                200,
                json!({"kind":"tv_approved_and_saved","device_id":wire::device(v.id)}),
            )
        }
        Outcome::Saved(v) => return (200, json!({"kind":"saved","device_id":wire::device(v.id)})),
        Outcome::Connected => return (200, json!({"kind":"remote_ready"})),
        Outcome::Remote(v) => {
            return match v {
                TerminalOutcome::Written => (200, json!({"kind":"written"})),
                TerminalOutcome::Uncertain => (200, json!({"kind":"uncertain"})),
                TerminalOutcome::NotSent(reason) => {
                    let code = match reason {
                        NotSentReason::Cancelled => "cancelled",
                        NotSentReason::TransportUnavailable => "transport_unavailable",
                        NotSentReason::Policy(p) => policy(*p).code,
                    };
                    (200, json!({"kind":"not_sent","reason":code}))
                }
            }
        }
        Outcome::Wake(v) => return (200, json!({"kind":"wake","facts":wake(*v)})),
        Outcome::Forgotten(v) => {
            let mut body = json!({"kind":if v.complete(){"forgotten"}else{"partial_setup"},"credential_removed":v.credential_removed,"trust_removed":v.trust_removed,"record_removed":v.record_removed});
            if !v.complete() {
                body["error"] = json!({"code":"partial_setup"});
            }
            return (if v.complete() { 200 } else { 423 }, body);
        }
        Outcome::Rejected(v) => rejected(*v),
        Outcome::PairFailed(v) => match v {
            PairFlowError::Session(v) => session(*v),
            PairFlowError::Setup(v) => setup(*v),
            PairFlowError::Cancelled => error(409, "cancelled"),
        },
        Outcome::ConnectFailed(v) => match v {
            ConnectFlowError::Session(v) => session(*v),
            ConnectFlowError::Reconnect(v) => reconnect(*v),
            ConnectFlowError::CredentialSave => error(423, "storage_blocked"),
            ConnectFlowError::Cancelled => error(409, "cancelled"),
        },
        Outcome::DiscoveryFailed(v) => match v {
            DiscoveryError::Permission => error(403, "network_permission_denied"),
            DiscoveryError::Unavailable => error(503, "discovery_unavailable"),
        },
        Outcome::ProbeFailed(v) => session(*v),
        Outcome::StorageFailed(v) => repository(*v),
        Outcome::StorageUncertain => error(503, "storage_uncertain"),
        Outcome::Deadline => error(503, "deadline"),
        Outcome::SupervisorFailed => error(503, "supervisor_failed"),
        Outcome::Drained => error(503, "server_stopping"),
    };
    (
        failure.status,
        json!({"kind":"failed","error":{"code":failure.code}}),
    )
}

pub fn receipt(value: &Receipt) -> (u16, Value) {
    let (status, result) = value
        .outcome
        .as_ref()
        .map(outcome)
        .unwrap_or((202, Value::Null));
    let mut body = json!({"operation_id":wire::operation(value.id),"server_epoch":wire::epoch(value.metadata.epoch),"device_id":value.metadata.target.map(wire::device),"selection_generation":value.metadata.generation.map(Counter),"state":if value.outcome.is_some(){"terminal"}else{"pending"},"result":result});
    if let Some(expiry) = value.expires_at {
        body["expires_at_unix_ms"] = json!(Counter(expiry.get()));
    }
    if let Some(state) = &value.terminal_state {
        body["terminal_state"] = json!({"selected":state.selected.map(wire::device),"selection_generation":Counter(state.generation),"config_revision":state.configuration_revision.map(Counter)});
    }
    if let Some(error) = body["result"].get("error").cloned() {
        body["error"] = error;
    }
    (status, body)
}

pub const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;

/// Stop serialization before exceeding the cap, rather than allocating a full
/// oversized string and checking afterward. Never truncate a valid document.
pub fn encode(value: &impl serde::Serialize, cap: usize) -> Result<Vec<u8>, ErrorCode> {
    struct Bounded {
        bytes: Vec<u8>,
        cap: usize,
    }
    impl std::io::Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.cap.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other("output limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Bounded {
        bytes: Vec::with_capacity(cap.min(4096)),
        cap,
    };
    serde_json::to_writer(&mut output, value).map_err(|_| error(503, "response_too_large"))?;
    Ok(output.bytes)
}
