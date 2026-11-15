//! Narrow safe native projections shared by the development GUI and harness.
use super::http::ClientError;
use crate::application::{
    control_service::*,
    control_state::{ConnectionState, ControlState, PairingState, PlaybackState},
    device_repository::{RepositoryError, SavedDevice},
    recovery::{ForgetMarker, PairingCleanup, PairingMarker, PairingStage},
    tv_address::TvHost,
    tv_control_coordinator::{WakeFailure, WakeStage},
    wake::{WakeConfiguration, WakeInterface},
    wake_transport::WakeSendError,
};
use crate::domain::DeviceId;
use crate::server::control_http::wire::{self, Counter};
use serde_json::{json, Value};
use std::sync::Arc;
fn invalid() -> ClientError {
    ClientError::InvalidResponse
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, ClientError> {
    v[key].as_str().ok_or_else(invalid)
}
fn flag(v: &Value, key: &str) -> Result<bool, ClientError> {
    v[key].as_bool().ok_or_else(invalid)
}
fn count(v: &Value, key: &str) -> Result<u64, ClientError> {
    serde_json::from_value::<Counter>(v[key].clone())
        .map(|n| n.0)
        .map_err(|_| invalid())
}
fn id(v: &Value, key: &str) -> Result<DeviceId, ClientError> {
    wire::parse_device(text(v, key)?).map_err(|_| invalid())
}
fn optional_id(v: &Value, key: &str) -> Result<Option<DeviceId>, ClientError> {
    if v[key].is_null() {
        Ok(None)
    } else {
        id(v, key).map(Some)
    }
}
fn operation(v: &Value, key: &str, epoch: Epoch) -> Result<OperationId, ClientError> {
    wire::parse_operation(text(v, key)?, epoch, epoch).map_err(|_| invalid())
}
fn optional_operation(
    v: &Value,
    key: &str,
    epoch: Epoch,
) -> Result<Option<OperationId>, ClientError> {
    if v[key].is_null() {
        Ok(None)
    } else {
        operation(v, key, epoch).map(Some)
    }
}
pub fn device(v: &Value) -> Result<SavedDevice, ClientError> {
    let wake = &v["wake"];
    let mac = |key| -> Result<_, ClientError> {
        if wake[key].is_null() {
            Ok(None)
        } else {
            Ok(Some(text(wake, key)?.parse().map_err(|_| invalid())?))
        }
    };
    let label = text(v, "label")?;
    if label.len() > 256 {
        return Err(invalid());
    }
    Ok(SavedDevice {
        id: id(v, "device_id")?,
        label: label.into(),
        host: TvHost::parse(text(v, "host")?).map_err(|_| invalid())?,
        wake: WakeConfiguration {
            wired: mac("wired_mac")?,
            wifi: mac("wifi_mac")?,
            active: match wake["active_interface"].as_str() {
                None if wake["active_interface"].is_null() => None,
                Some("Wired") => Some(WakeInterface::Wired),
                Some("WiFi") => Some(WakeInterface::WiFi),
                _ => return Err(invalid()),
            },
        },
    })
}
pub fn snapshot(v: &Value) -> Result<Snapshot, ClientError> {
    let epoch = wire::parse_epoch(text(v, "server_epoch")?).map_err(|_| invalid())?;
    let values = v["devices"]
        .as_array()
        .filter(|a| a.len() <= 1024)
        .ok_or_else(invalid)?;
    let mut devices = Vec::with_capacity(values.len());
    let mut revisions = Vec::with_capacity(values.len());
    for value in values {
        let device = device(value)?;
        if devices.iter().any(|d: &SavedDevice| d.id == device.id) {
            return Err(invalid());
        }
        revisions.push((device.id, count(value, "config_revision")?));
        devices.push(device);
    }
    let selected = optional_id(v, "selected")?
        .map(|id| {
            devices
                .iter()
                .find(|d| d.id == id)
                .map(SavedDevice::display)
                .ok_or_else(invalid)
        })
        .transpose()?;
    let control = ControlState::projected(
        selected,
        count(v, "selection_generation")?,
        match text(v, "pairing")? {
            "not_started" => PairingState::NotStarted,
            "in_progress" => PairingState::InProgress,
            "ready" => PairingState::Ready,
            "failed" => PairingState::Failed,
            _ => return Err(invalid()),
        },
        match text(v, "connection")? {
            "not_connected" => ConnectionState::NotConnected,
            "connecting" => ConnectionState::Connecting,
            "ready" => ConnectionState::Ready,
            "failed" => ConnectionState::Failed,
            _ => return Err(invalid()),
        },
        match text(v, "playback")? {
            "unknown" => PlaybackState::Unknown,
            "assumed_paused" => PlaybackState::AssumedPaused,
            "assumed_playing" => PlaybackState::AssumedPlaying,
            _ => return Err(invalid()),
        },
    );
    let recovery = if v["forget_recovery"].is_null() {
        None
    } else {
        let m = &v["forget_recovery"];
        let marker = ForgetMarker {
            transaction: id(m, "transaction_id")?,
            target: id(m, "device_id")?,
            selection_cleared: flag(m, "selection_cleared")?,
            credential_removed: flag(m, "credential_removed")?,
            trust_removed: flag(m, "trust_removed")?,
            record_removed: flag(m, "record_removed")?,
        };
        if !marker.valid() {
            return Err(invalid());
        }
        Some(marker)
    };
    let pairing_recovery = if v["pairing_recovery"].is_null() {
        None
    } else {
        let m = &v["pairing_recovery"];
        let cleanup = if m["cleanup"].is_null() {
            None
        } else {
            let c = &m["cleanup"];
            Some(PairingCleanup {
                credential_removed: flag(c, "credential_removed")?,
                trust_removed: flag(c, "trust_removed")?,
                record_removed: flag(c, "record_removed")?,
                selection_restored: flag(c, "selection_restored")?,
            })
        };
        let marker = PairingMarker {
            transaction: id(m, "transaction_id")?,
            target: id(m, "device_id")?,
            prior_selection: optional_id(m, "prior_selection")?,
            repair: flag(m, "repair")?,
            backup: optional_id(m, "backup")?,
            stage: match text(m, "stage")? {
                "prepared" => PairingStage::Prepared,
                "token_saved" => PairingStage::TokenSaved,
                "trust_saved" => PairingStage::TrustSaved,
                "device_saved" => PairingStage::DeviceSaved,
                "selection_saved" => PairingStage::SelectionSaved,
                _ => return Err(invalid()),
            },
            cleanup,
        };
        if !marker.valid() {
            return Err(invalid());
        }
        Some(marker)
    };
    let mut capabilities = Vec::with_capacity(values.len());
    let blocked = text(v, "startup")? != "ready" || flag(v, "admission_closed")?;
    for value in values {
        let summary = decode_capabilities(value, epoch, &control)?;
        if blocked
            && (summary.ime_active
                || summary
                    .facts
                    .iter()
                    .any(|f| f.freshness == crate::domain::capability::Freshness::Current))
        {
            return Err(invalid());
        }
        capabilities.push(summary);
    }
    Ok(Snapshot {
        capabilities: Arc::new(capabilities),
        epoch,
        revision: count(v, "revision")?,
        admission_closed: flag(v, "admission_closed")?,
        startup: match text(v, "startup")? {
            "restoring" => StartupState::Restoring,
            "ready" => StartupState::Ready,
            "storage_unavailable" => StartupState::Blocked(RepositoryError::Unavailable),
            "storage_corrupt" => StartupState::Blocked(RepositoryError::Corrupt),
            "recovery_required" => StartupState::RecoveryRequired,
            "draining" => StartupState::Draining,
            _ => return Err(invalid()),
        },
        control,
        devices: Arc::new(devices),
        configuration_revisions: Arc::new(revisions),
        duplicate_records: flag(v, "duplicate_records")?,
        storage_pending: flag(v, "storage_pending")?,
        recovery,
        pairing_recovery,
        wake: wake(&v["wake"])?,
        wake_operation: optional_operation(v, "wake_operation", epoch)?,
        pairing_pending: flag(v, "pairing_pending")?,
    })
}

fn decode_capabilities(
    v: &Value,
    epoch: Epoch,
    control: &ControlState,
) -> Result<crate::application::capabilities::CapabilitySummary, ClientError> {
    use crate::application::capabilities::{CapabilityFact, CapabilitySummary, EvidenceContext};
    use crate::domain::capability::*;
    let device = id(v, "device_id")?;
    // Only an absent field is the compatible-old-server fallback. Null is malformed.
    let Some(v) = v.get("capabilities") else {
        return Ok(CapabilitySummary::unknown(
            device,
            CapabilityReason::EvidenceMissing,
        ));
    };
    if id(v, "device_id")? != device {
        return Err(invalid());
    }
    let evidence_epoch = if v["evidence_epoch"].is_null() {
        None
    } else {
        Some(wire::parse_epoch(text(v, "evidence_epoch")?).map_err(|_| invalid())?)
    };
    // M1 never restores evidence across owner restarts.
    if evidence_epoch.is_some_and(|observed| observed != epoch) {
        return Err(invalid());
    }
    let reason: CapabilityReason =
        serde_json::from_value(v["reason"].clone()).map_err(|_| invalid())?;
    let values = v["facts"]
        .as_array()
        .filter(|a| a.len() == CapabilityKey::ALL.len())
        .ok_or_else(invalid)?;
    let mut facts = CapabilityKey::ALL.map(CapabilityFact::unknown);
    for (index, value) in values.iter().enumerate() {
        let key: CapabilityKey =
            serde_json::from_value(value["key"].clone()).map_err(|_| invalid())?;
        if CapabilityKey::ALL[index] != key {
            return Err(invalid());
        }
        let context = if value["context"].is_null() {
            None
        } else {
            let c = &value["context"];
            Some(EvidenceContext {
                epoch: evidence_epoch.ok_or_else(invalid)?,
                selection_generation: count(c, "selection_generation")?,
                session: count(c, "session")?,
            })
        };
        let fact = CapabilityFact {
            key,
            support: serde_json::from_value(value["support"].clone()).map_err(|_| invalid())?,
            source: serde_json::from_value(value["source"].clone()).map_err(|_| invalid())?,
            freshness: serde_json::from_value(value["freshness"].clone()).map_err(|_| invalid())?,
            observed_at_unix_ms: if value["observed_at_unix_ms"].is_null() {
                None
            } else {
                Some(count(value, "observed_at_unix_ms")?)
            },
            context,
        };
        if !fact.valid() {
            return Err(invalid());
        }
        if fact.freshness == Freshness::Current
            && (control.selected_device() != Some(device)
                || control.connection_state() != ConnectionState::Ready
                || control.pairing_state() != PairingState::Ready
                || context.is_none_or(|c| {
                    c.epoch != epoch || c.selection_generation != control.selection_generation()
                })
                || reason != CapabilityReason::Connected)
        {
            return Err(invalid());
        }
        facts[index] = fact;
    }
    let ime_active = flag(v, "ime_active")?;
    if evidence_epoch.is_some() != facts.iter().any(|f| f.context.is_some()) {
        return Err(invalid());
    }
    let summary = CapabilitySummary {
        device,
        reason,
        ime_active,
        facts,
    };
    if ime_active
        && (summary.fact(CapabilityKey::NativeIme).freshness != Freshness::Current
            || summary.fact(CapabilityKey::NativeIme).support != Support::Supported)
    {
        return Err(invalid());
    }
    let mut session = None;
    for fact in &summary.facts {
        if fact.freshness == Freshness::Current {
            if session.is_some_and(|s| fact.context != Some(s)) {
                return Err(invalid());
            }
            session = fact.context;
        }
    }
    Ok(summary)
}
pub(crate) fn initial(v: &Value) -> Result<(Snapshot, Vec<OperationId>), ClientError> {
    let snapshot = snapshot(v)?;
    let values = v["recent_operations"]
        .as_array()
        .filter(|values| values.len() <= 64)
        .ok_or_else(invalid)?;
    let mut references = Vec::with_capacity(values.len());
    for value in values {
        let id = wire::parse_operation(
            value.as_str().ok_or_else(invalid)?,
            snapshot.epoch,
            snapshot.epoch,
        )
        .map_err(|_| invalid())?;
        if references.contains(&id) {
            return Err(invalid());
        }
        references.push(id);
    }
    Ok((snapshot, references))
}
pub(crate) fn changed(
    v: &Value,
    kind: &str,
) -> Result<(Snapshot, Option<OperationId>), ClientError> {
    let snapshot = snapshot(v)?;
    let reference = match kind {
        "operation_changed" if matches!(v["phase"].as_str(), Some("pending" | "terminal")) => {
            Some(operation(v, "operation_id", snapshot.epoch)?)
        }
        "state_changed" if v["operation_id"].is_null() && v["phase"].is_null() => None,
        _ => return Err(invalid()),
    };
    Ok((snapshot, reference))
}
fn wake(v: &Value) -> Result<WakeStage, ClientError> {
    let failure = || -> Result<WakeFailure, ClientError> {
        Ok(match text(v, "failure")? {
            "timeout" => WakeFailure::Timeout,
            "pairing_required" => WakeFailure::PairingRequired,
            "connection_failed" => WakeFailure::Connection,
            "cancelled" => WakeFailure::Local(WakeSendError::Cancelled),
            "invalid_target" => WakeFailure::Local(WakeSendError::InvalidTarget),
            "no_ipv4_route" => WakeFailure::Local(WakeSendError::NoIpv4Route),
            "no_broadcast" => WakeFailure::Local(WakeSendError::NoBroadcast),
            "permission_denied" => WakeFailure::Local(WakeSendError::Permission),
            "send_failed" => WakeFailure::Local(WakeSendError::SendFailed),
            _ => return Err(invalid()),
        })
    };
    let stage = match text(v, "stage")? {
        "configuration_required" => WakeStage::ConfigurationRequired,
        "idle" => WakeStage::Idle,
        "checking_connection" => WakeStage::CheckingConnection,
        "packet_sending" => WakeStage::PacketSending,
        "reconnecting" => WakeStage::Reconnecting,
        "connected" => WakeStage::Connected,
        "ready" => WakeStage::Ready,
        "failed" => WakeStage::Failed(failure()?),
        "failed_after_send" => WakeStage::FailedAfterSend(failure()?),
        "cancelled" => WakeStage::Cancelled,
        "cancelled_during_send" => WakeStage::CancelledDuringSend,
        "cancelled_after_send" => WakeStage::CancelledAfterSend,
        _ => return Err(invalid()),
    };
    let (packet, remote) = match stage {
        WakeStage::ConfigurationRequired | WakeStage::Idle => ("not_sent", "not_ready"),
        WakeStage::CheckingConnection | WakeStage::Cancelled => ("not_sent", "unknown"),
        WakeStage::PacketSending | WakeStage::CancelledDuringSend => ("uncertain", "unknown"),
        WakeStage::Reconnecting | WakeStage::CancelledAfterSend => ("sent", "unknown"),
        WakeStage::Connected => ("not_sent", "ready"),
        WakeStage::Ready => ("sent", "ready"),
        WakeStage::Failed(WakeFailure::Local(WakeSendError::SendFailed)) => {
            ("uncertain", "not_ready")
        }
        WakeStage::Failed(_) => ("not_sent", "not_ready"),
        WakeStage::FailedAfterSend(_) => ("sent", "not_ready"),
    };
    if text(v, "packet")? != packet || text(v, "remote")? != remote {
        return Err(invalid());
    }
    if !matches!(stage, WakeStage::Failed(_) | WakeStage::FailedAfterSend(_))
        && !v["failure"].is_null()
    {
        return Err(invalid());
    }
    Ok(stage)
}

/// Fully decoded terminal facts. No JSON is exposed to presentation code.
#[derive(Debug, Clone)]
pub enum NativeOutcome {
    Discovered(Vec<Candidate>),
    Probed(Box<Observation>),
    Paired(DeviceId),
    Saved(DeviceId),
    RemoteReady,
    Written,
    Uncertain,
    NotSent(super::model::NotSent),
    Wake(WakeStage),
    Forgotten(super::model::CleanupFacts),
    PartialSetup(super::model::CleanupFacts),
    Failed(super::model::FailureCode),
}
impl NativeOutcome {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Discovered(_) => "discovered",
            Self::Probed(_) => "probed",
            Self::Paired(_) => "tv_approved_and_saved",
            Self::Saved(_) => "saved",
            Self::RemoteReady => "remote_ready",
            Self::Written => "written",
            Self::Uncertain => "uncertain",
            Self::NotSent(_) => "not_sent",
            Self::Wake(_) => "wake",
            Self::Forgotten(_) => "forgotten",
            Self::PartialSetup(_) => "partial_setup",
            Self::Failed(_) => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct NativeReceipt {
    pub id: OperationId,
    pub epoch: Epoch,
    pub target: Option<DeviceId>,
    pub generation: Option<u64>,
    pub terminal: bool,
    pub outcome: Option<NativeOutcome>,
    pub expires_at: Option<std::num::NonZeroU64>,
    pub terminal_state: Option<TerminalState>,
}
impl NativeReceipt {
    pub fn parse(v: &Value, expected: Epoch) -> Result<Self, ClientError> {
        let epoch = wire::parse_epoch(text(v, "server_epoch")?).map_err(|_| invalid())?;
        if epoch != expected {
            return Err(ClientError::Resnapshot);
        }
        let terminal = match text(v, "state")? {
            "pending" => false,
            "terminal" => true,
            _ => return Err(invalid()),
        };
        let id = operation(v, "operation_id", epoch)?;
        let target = optional_id(v, "device_id")?;
        let generation = if v["selection_generation"].is_null() {
            None
        } else {
            Some(count(v, "selection_generation")?)
        };
        let (outcome, expires_at, terminal_state) = if terminal {
            let expires =
                std::num::NonZeroU64::new(count(v, "expires_at_unix_ms")?).ok_or_else(invalid)?;
            let state = &v["terminal_state"];
            if !state.is_object() {
                return Err(invalid());
            }
            let terminal_state = TerminalState {
                selected: optional_id(state, "selected")?,
                generation: count(state, "selection_generation")?,
                configuration_revision: if state["config_revision"].is_null() {
                    None
                } else {
                    Some(count(state, "config_revision")?)
                },
            };
            (
                Some(parse_outcome(&v["result"], id, epoch)?),
                Some(expires),
                Some(terminal_state),
            )
        } else {
            if !v["result"].is_null()
                || !v["terminal_state"].is_null()
                || !v["expires_at_unix_ms"].is_null()
            {
                return Err(invalid());
            }
            (None, None, None)
        };
        Ok(Self {
            id,
            epoch,
            target,
            generation,
            terminal,
            outcome,
            expires_at,
            terminal_state,
        })
    }
    pub fn kind(&self) -> &str {
        self.outcome.as_ref().map_or("pending", NativeOutcome::kind)
    }
    pub fn failure(&self) -> Option<super::model::FailureCode> {
        match self.outcome {
            Some(NativeOutcome::Failed(code)) => Some(code),
            _ => None,
        }
    }
    pub fn observation(&self) -> Result<Observation, ClientError> {
        match &self.outcome {
            Some(NativeOutcome::Probed(observed)) => Ok(*observed.clone()),
            _ => Err(invalid()),
        }
    }
    pub fn candidates(&self) -> Result<Vec<Candidate>, ClientError> {
        match &self.outcome {
            Some(NativeOutcome::Discovered(candidates)) => Ok(candidates.clone()),
            _ => Err(invalid()),
        }
    }
}

fn parse_outcome(v: &Value, id: OperationId, epoch: Epoch) -> Result<NativeOutcome, ClientError> {
    use super::model::{CleanupFacts, FailureCode, NotSent};
    Ok(match text(v, "kind")? {
        "discovered" => {
            let candidates = v["candidates"]
                .as_array()
                .filter(|a| a.len() <= 32)
                .ok_or_else(invalid)?
                .iter()
                .enumerate()
                .map(|(index, v)| {
                    let candidate = wire::parse_candidate(text(v, "candidate_id")?, epoch, epoch)
                        .map_err(|_| invalid())?;
                    if candidate.0 != id || usize::from(candidate.1) != index {
                        return Err(invalid());
                    }
                    Ok(Candidate {
                        id: candidate,
                        host: TvHost::parse(text(v, "host")?).map_err(|_| invalid())?,
                        expires_at: std::num::NonZeroU64::new(count(v, "expires_at_unix_ms")?)
                            .ok_or_else(invalid)?,
                    })
                })
                .collect::<Result<Vec<_>, ClientError>>()?;
            NativeOutcome::Discovered(candidates)
        }
        "probed" => {
            if operation(v, "observation_id", epoch)? != id
                || operation(v, "attempt_id", epoch)? != id
            {
                return Err(invalid());
            }
            let optional_text = |key| -> Result<Option<String>, ClientError> {
                match &v[key] {
                    Value::Null => Ok(None),
                    Value::String(s) if s.len() <= 256 => Ok(Some(s.clone())),
                    _ => Err(invalid()),
                }
            };
            NativeOutcome::Probed(Box::new(Observation {
                id,
                host: TvHost::parse(text(v, "host")?).map_err(|_| invalid())?,
                observation: crate::application::tv_session::ProbeObservation {
                    pin: crate::application::certificate_trust::CertificatePin::from_hex(text(
                        v,
                        "fingerprint",
                    )?)
                    .ok_or_else(invalid)?,
                    name: optional_text("name")?,
                    model: optional_text("model")?,
                },
                expires_at: std::num::NonZeroU64::new(count(v, "expires_at_unix_ms")?)
                    .ok_or_else(invalid)?,
            }))
        }
        "tv_approved_and_saved" => NativeOutcome::Paired(id_from_result(v)?),
        "saved" => NativeOutcome::Saved(id_from_result(v)?),
        "remote_ready" => NativeOutcome::RemoteReady,
        "written" => NativeOutcome::Written,
        "uncertain" => NativeOutcome::Uncertain,
        "not_sent" => NativeOutcome::NotSent(
            serde_json::from_value::<NotSent>(v["reason"].clone()).map_err(|_| invalid())?,
        ),
        "wake" => NativeOutcome::Wake(wake(&v["facts"])?),
        kind @ ("forgotten" | "partial_setup") => {
            let facts = CleanupFacts {
                credential_removed: flag(v, "credential_removed")?,
                trust_removed: flag(v, "trust_removed")?,
                record_removed: flag(v, "record_removed")?,
            };
            if facts.complete() != (kind == "forgotten") {
                return Err(invalid());
            }
            if kind == "forgotten" {
                NativeOutcome::Forgotten(facts)
            } else {
                if FailureCode::parse(text(&v["error"], "code")?)? != FailureCode::PartialSetup {
                    return Err(invalid());
                }
                NativeOutcome::PartialSetup(facts)
            }
        }
        "failed" => NativeOutcome::Failed(FailureCode::parse(text(&v["error"], "code")?)?),
        _ => return Err(invalid()),
    })
}
fn id_from_result(v: &Value) -> Result<DeviceId, ClientError> {
    id(v, "device_id")
}

pub fn encode(request: &Request) -> Result<(&'static str, String, Value), ClientError> {
    if uuid::Uuid::parse_str(&request.key)
        .map_err(|_| invalid())?
        .to_string()
        != request.key
    {
        return Err(invalid());
    }
    let mut method = "POST";
    let (route, extra) = match &request.command {
        Command::Discover => ("/v1/discoveries".into(), json!({})),
        Command::Probe(host) => ("/v1/probes".into(), json!({"host":host.as_str()})),
        Command::ProbeCandidate { candidate, host } => (
            "/v1/probes".into(),
            json!({"host":host.as_str(),"candidate_id":wire::candidate(*candidate)}),
        ),
        Command::Pair {
            observation,
            host,
            pin,
            label,
            replace,
            generation,
            recovery,
        } => (
            "/v1/pairings".into(),
            json!({"attempt_id":wire::operation(*observation),"observation_id":wire::operation(*observation),"host":host.as_str(),"fingerprint":pin.to_hex(),"confirm_fingerprint":pin.to_hex(),"label":label,"device_id":replace.map(wire::device),"selection_generation":Counter(*generation),"recovery_transaction_id":recovery.map(wire::device)}),
        ),
        Command::Select { target, generation } => (
            "/v1/selection".into(),
            json!({"device_id":wire::device(*target),"expected_generation":Counter(*generation)}),
        ),
        Command::Connect {
            target,
            generation,
            only_if_idle,
        } => (
            "/v1/connections".into(),
            json!({"device_id":wire::device(*target),"selection_generation":Counter(*generation),"only_if_idle":only_if_idle}),
        ),
        Command::Remote(action) => (
            "/v1/remote-actions".into(),
            json!({"device_id":wire::device(action.target()),"selection_generation":Counter(action.selection_generation()),"action":action.action()}),
        ),
        Command::Wake { target, generation } => (
            "/v1/wakes".into(),
            json!({"device_id":wire::device(*target),"selection_generation":Counter(*generation)}),
        ),
        Command::CancelWake {
            operation,
            target,
            generation,
        } => (
            format!("/v1/wakes/{}/cancel", wire::operation(*operation)),
            json!({"device_id":wire::device(*target),"selection_generation":Counter(*generation)}),
        ),
        Command::SetWake {
            target,
            generation,
            revision,
            configuration,
        } => {
            method = "PUT";
            (
                format!("/v1/devices/{}/wake", wire::device(*target)),
                json!({"device_id":wire::device(*target),"selection_generation":Counter(*generation),"expected_config_revision":Counter(*revision),"wired_mac":configuration.wired.map(|m|m.to_string()),"wifi_mac":configuration.wifi.map(|m|m.to_string()),"active_interface":configuration.active.map(|v|match v{WakeInterface::Wired=>"Wired",WakeInterface::WiFi=>"WiFi"})}),
            )
        }
        Command::Forget {
            target,
            revision,
            confirm_device,
        } => (
            format!("/v1/devices/{}/forget", wire::device(*target)),
            json!({"device_id":wire::device(*target),"expected_config_revision":Counter(*revision),"confirm_device_id":wire::device(*confirm_device)}),
        ),
        Command::RecoverPairing {
            target,
            transaction,
            confirm_device,
        }
        | Command::RecoverForget {
            target,
            transaction,
            confirm_device,
        } => {
            let suffix = if matches!(request.command, Command::RecoverPairing { .. }) {
                "recover-pairing"
            } else {
                "recover-forget"
            };
            (
                format!("/v1/devices/{}/{suffix}", wire::device(*target)),
                json!({"device_id":wire::device(*target),"transaction_id":wire::device(*transaction),"confirm_device_id":wire::device(*confirm_device)}),
            )
        }
    };
    let mut body = json!({"server_epoch":wire::epoch(request.epoch),"operation_key":request.key});
    body.as_object_mut()
        .expect("envelope")
        .extend(extra.as_object().expect("command").clone());
    Ok((method, route, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> Value {
        let snapshot = Snapshot {
            epoch: Epoch::from_u128(1),
            revision: 0,
            admission_closed: false,
            startup: StartupState::Ready,
            control: ControlState::none(),
            devices: Arc::new(vec![]),
            capabilities: Arc::new(vec![]),
            configuration_revisions: Arc::new(vec![]),
            duplicate_records: false,
            storage_pending: false,
            recovery: None,
            pairing_recovery: None,
            wake: WakeStage::Idle,
            wake_operation: None,
            pairing_pending: false,
        };
        crate::server::control_http::response::snapshot(&snapshot)
    }
    #[test]
    fn initial_private_references_are_bounded_unique_and_kind_checked() {
        let mut state = state();
        assert!(initial(&state).is_err());
        state["recent_operations"] = json!([wire::operation(OperationId(7))]);
        let (snapshot, references) = initial(&state).unwrap();
        assert_eq!(snapshot.epoch, Epoch::from_u128(1));
        assert_eq!(references, vec![OperationId(7)]);
        for invalid in [
            json!([
                wire::operation(OperationId(7)),
                wire::operation(OperationId(7))
            ]),
            json!([wire::device(DeviceId::new(1))]),
            json!([wire::operation(OperationId(0))]),
            json!((1..=65)
                .map(|id| wire::operation(OperationId(id)))
                .collect::<Vec<_>>()),
        ] {
            state["recent_operations"] = invalid;
            assert!(initial(&state).is_err());
        }
    }
    #[test]
    fn operation_change_is_only_a_reference_and_requires_a_valid_phase() {
        let mut state = state();
        state["operation_id"] = json!(wire::operation(OperationId(2)));
        for phase in ["pending", "terminal"] {
            state["phase"] = json!(phase);
            assert_eq!(
                changed(&state, "operation_changed").unwrap().1,
                Some(OperationId(2))
            );
        }
        assert!(changed(&state, "state_changed").is_err());
        state["phase"] = json!("written");
        assert!(changed(&state, "operation_changed").is_err());
        state["operation_id"] = Value::Null;
        state["phase"] = Value::Null;
        assert!(changed(&state, "operation_changed").is_err());
        assert!(changed(&state, "state_changed").unwrap().1.is_none());
    }
}
