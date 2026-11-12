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
    Ok(Snapshot {
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
    Ok(match text(v, "stage")? {
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
    })
}

#[derive(Debug, Clone)]
pub struct NativeReceipt {
    pub id: OperationId,
    pub epoch: Epoch,
    pub terminal: bool,
    pub result: Value,
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
        if terminal {
            std::num::NonZeroU64::new(count(v, "expires_at_unix_ms")?).ok_or_else(invalid)?;
            if !matches!(
                text(&v["result"], "kind")?,
                "discovered"
                    | "probed"
                    | "tv_approved_and_saved"
                    | "saved"
                    | "remote_ready"
                    | "written"
                    | "uncertain"
                    | "not_sent"
                    | "wake"
                    | "forgotten"
                    | "partial_setup"
                    | "failed"
            ) {
                return Err(invalid());
            }
        }
        Ok(Self {
            id: operation(v, "operation_id", epoch)?,
            epoch,
            terminal,
            result: v["result"].clone(),
        })
    }
    pub fn kind(&self) -> &str {
        self.result["kind"].as_str().unwrap_or("pending")
    }
    pub fn observation(&self) -> Result<Observation, ClientError> {
        let v = &self.result;
        if !self.terminal
            || self.kind() != "probed"
            || operation(v, "observation_id", self.epoch)? != self.id
            || operation(v, "attempt_id", self.epoch)? != self.id
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
        let expiry =
            std::num::NonZeroU64::new(count(v, "expires_at_unix_ms")?).ok_or_else(invalid)?;
        Ok(Observation {
            id: operation(v, "observation_id", self.epoch)?,
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
            expires_at: expiry,
        })
    }
    pub fn candidates(&self) -> Result<Vec<Candidate>, ClientError> {
        if !self.terminal || self.kind() != "discovered" {
            return Err(invalid());
        }
        self.result["candidates"]
            .as_array()
            .filter(|a| a.len() <= 32)
            .ok_or_else(invalid)?
            .iter()
            .enumerate()
            .map(|(index, v)| {
                let id = wire::parse_candidate(text(v, "candidate_id")?, self.epoch, self.epoch)
                    .map_err(|_| invalid())?;
                if id.0 != self.id || usize::from(id.1) != index {
                    return Err(invalid());
                }
                Ok(Candidate {
                    id,
                    host: TvHost::parse(text(v, "host")?).map_err(|_| invalid())?,
                    expires_at: std::num::NonZeroU64::new(count(v, "expires_at_unix_ms")?)
                        .ok_or_else(invalid)?,
                })
            })
            .collect()
    }
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
        Command::Connect { target, generation } => (
            "/v1/connections".into(),
            json!({"device_id":wire::device(*target),"selection_generation":Counter(*generation)}),
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
