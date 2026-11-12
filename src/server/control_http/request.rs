//! Strict route DTOs, decoded once before calling the typed service.
use serde::Deserialize;

use super::wire::{self, Counter};
use crate::application::certificate_trust::CertificatePin;
use crate::application::control_service::{Command, Epoch, Request};
use crate::application::tv_address::TvHost;
use crate::application::wake::{WakeConfiguration, WakeInterface};
use crate::application::SendRemoteAction;
use crate::domain::RemoteAction;

pub const MAX_BODY_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestError {
    InvalidRequest,
    StaleEpoch,
    InvalidId,
    InvalidTarget,
    InvalidConfirmation,
    TooLarge,
}

impl From<wire::WireError> for RequestError {
    fn from(value: wire::WireError) -> Self {
        match value {
            wire::WireError::InvalidId => Self::InvalidId,
            wire::WireError::StaleEpoch => Self::StaleEpoch,
        }
    }
}

// Expand the envelope into each DTO so serde rejects duplicate and unknown
// fields, including envelope fields. Flattened maps can discard duplicates.
macro_rules! dto {
    ($name:ident { $($(#[$attribute:meta])* $field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct $name {
            server_epoch: String,
            operation_key: String,
            $($(#[$attribute])* $field: $ty,)*
        }
        impl $name {
            fn request(self, current: Epoch) -> Result<(Epoch, String, Self), RequestError> {
                let epoch = wire::parse_epoch(&self.server_epoch)?;
                if epoch != current { return Err(RequestError::StaleEpoch); }
                let key = uuid::Uuid::parse_str(&self.operation_key)
                    .map_err(|_| RequestError::InvalidId)?;
                if key.hyphenated().to_string() != self.operation_key {
                    return Err(RequestError::InvalidId);
                }
                Ok((epoch, self.operation_key.clone(), self))
            }
        }
    }
}

dto!(Empty {});
dto!(Probe { host: String, candidate_id: Option<String> });
dto!(Pair {
    attempt_id: String,
    observation_id: String,
    host: String,
    fingerprint: String,
    confirm_fingerprint: String,
    label: String,
    selection_generation: Counter,
    device_id: Option<String>,
    recovery_transaction_id: Option<String>,
});
dto!(Selection {
    device_id: String,
    expected_generation: Counter
});
dto!(Selected {
    device_id: String,
    selection_generation: Counter
});
dto!(Connect {
    device_id: String,
    selection_generation: Counter,
    #[serde(default)]
    only_if_idle: bool,
});
dto!(Remote {
    device_id: String,
    selection_generation: Counter,
    action: RemoteAction
});
dto!(Wake {
    device_id: String,
    selection_generation: Counter,
    expected_config_revision: Counter,
    wired_mac: Option<String>,
    wifi_mac: Option<String>,
    active_interface: Option<Interface>,
});
dto!(Forget {
    device_id: String,
    expected_config_revision: Counter,
    confirm_device_id: String
});
dto!(Recovery {
    device_id: String,
    transaction_id: String,
    confirm_device_id: String
});
dto!(McpEnrollment {
    expected_generation: Counter,
    enabled: bool,
});

#[derive(Deserialize)]
enum Interface {
    Wired,
    WiFi,
}

fn decode<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, RequestError> {
    if body.len() > MAX_BODY_BYTES {
        return Err(RequestError::TooLarge);
    }
    serde_json::from_slice(body).map_err(|_| RequestError::InvalidRequest)
}

fn host(value: &str) -> Result<TvHost, RequestError> {
    TvHost::parse(value).map_err(|_| RequestError::InvalidTarget)
}

fn matching_device(path: &str, body: &str) -> Result<crate::domain::DeviceId, RequestError> {
    let id = wire::parse_device(body)?;
    if wire::parse_device(path)? != id {
        return Err(RequestError::InvalidTarget);
    }
    Ok(id)
}

/// Routing selects this enum without interpreting request JSON or creating a
/// second application policy path. Lifecycle Stop uses a separate interface.
#[derive(Debug, Clone, Copy)]
pub enum MutationRoute<'a> {
    Discover,
    Probe,
    Pair,
    Select,
    Connect,
    SetWake(&'a str),
    Remote,
    Wake,
    CancelWake(&'a str),
    Forget(&'a str),
    RecoverPairing(&'a str),
    RecoverForget(&'a str),
}

pub fn parse_mutation(
    route: MutationRoute<'_>,
    body: &[u8],
    current: Epoch,
) -> Result<Request, RequestError> {
    let (epoch, key, command) = match route {
        MutationRoute::Discover => {
            let (e, k, _) = decode::<Empty>(body)?.request(current)?;
            (e, k, Command::Discover)
        }
        MutationRoute::Probe => {
            let (e, k, v) = decode::<Probe>(body)?.request(current)?;
            let host = host(&v.host)?;
            let command = match v.candidate_id {
                Some(id) => Command::ProbeCandidate {
                    candidate: wire::parse_candidate(&id, e, current)?,
                    host,
                },
                None => Command::Probe(host),
            };
            (e, k, command)
        }
        MutationRoute::Pair => {
            let (e, k, v) = decode::<Pair>(body)?.request(current)?;
            let observation = wire::parse_operation(&v.observation_id, e, current)?;
            if wire::parse_operation(&v.attempt_id, e, current)? != observation
                || v.fingerprint != v.confirm_fingerprint
            {
                return Err(RequestError::InvalidConfirmation);
            }
            let pin = CertificatePin::from_hex(&v.fingerprint)
                .ok_or(RequestError::InvalidConfirmation)?;
            if pin.to_hex() != v.fingerprint {
                return Err(RequestError::InvalidConfirmation);
            }
            if v.label.len() > 256 {
                return Err(RequestError::TooLarge);
            }
            (
                e,
                k,
                Command::Pair {
                    observation,
                    host: host(&v.host)?,
                    pin,
                    label: v.label,
                    replace: v.device_id.as_deref().map(wire::parse_device).transpose()?,
                    generation: v.selection_generation.0,
                    recovery: v
                        .recovery_transaction_id
                        .as_deref()
                        .map(wire::parse_device)
                        .transpose()?,
                },
            )
        }
        MutationRoute::Select => {
            let (e, k, v) = decode::<Selection>(body)?.request(current)?;
            (
                e,
                k,
                Command::Select {
                    target: wire::parse_device(&v.device_id)?,
                    generation: v.expected_generation.0,
                },
            )
        }
        MutationRoute::Connect => {
            let (e, k, v) = decode::<Connect>(body)?.request(current)?;
            (
                e,
                k,
                Command::Connect {
                    target: wire::parse_device(&v.device_id)?,
                    generation: v.selection_generation.0,
                    only_if_idle: v.only_if_idle,
                },
            )
        }
        MutationRoute::Wake | MutationRoute::CancelWake(_) => {
            let (e, k, v) = decode::<Selected>(body)?.request(current)?;
            let target = wire::parse_device(&v.device_id)?;
            let generation = v.selection_generation.0;
            let command = match route {
                MutationRoute::Wake => Command::Wake { target, generation },
                MutationRoute::CancelWake(id) => Command::CancelWake {
                    operation: wire::parse_operation(id, e, current)?,
                    target,
                    generation,
                },
                _ => unreachable!(),
            };
            (e, k, command)
        }
        MutationRoute::SetWake(path) => {
            let (e, k, v) = decode::<Wake>(body)?.request(current)?;
            let mac = |value: Option<String>| {
                value
                    .map(|v| v.parse().map_err(|_| RequestError::InvalidTarget))
                    .transpose()
            };
            let configuration = WakeConfiguration {
                wired: mac(v.wired_mac)?,
                wifi: mac(v.wifi_mac)?,
                active: v.active_interface.map(|v| match v {
                    Interface::Wired => WakeInterface::Wired,
                    Interface::WiFi => WakeInterface::WiFi,
                }),
            };
            (
                e,
                k,
                Command::SetWake {
                    target: matching_device(path, &v.device_id)?,
                    generation: v.selection_generation.0,
                    revision: v.expected_config_revision.0,
                    configuration,
                },
            )
        }
        MutationRoute::Remote => {
            let (e, k, v) = decode::<Remote>(body)?.request(current)?;
            (
                e,
                k,
                Command::Remote(SendRemoteAction::new(
                    wire::parse_device(&v.device_id)?,
                    v.selection_generation.0,
                    v.action,
                )),
            )
        }
        MutationRoute::Forget(path) => {
            let (e, k, v) = decode::<Forget>(body)?.request(current)?;
            let target = matching_device(path, &v.device_id)?;
            let confirm_device = wire::parse_device(&v.confirm_device_id)?;
            if confirm_device != target {
                return Err(RequestError::InvalidConfirmation);
            }
            (
                e,
                k,
                Command::Forget {
                    target,
                    revision: v.expected_config_revision.0,
                    confirm_device,
                },
            )
        }
        MutationRoute::RecoverPairing(path) | MutationRoute::RecoverForget(path) => {
            let (e, k, v) = decode::<Recovery>(body)?.request(current)?;
            let target = matching_device(path, &v.device_id)?;
            let transaction = wire::parse_device(&v.transaction_id)?;
            let confirm_device = wire::parse_device(&v.confirm_device_id)?;
            if confirm_device != target {
                return Err(RequestError::InvalidConfirmation);
            }
            let command = match route {
                MutationRoute::RecoverPairing(_) => Command::RecoverPairing {
                    target,
                    transaction,
                    confirm_device,
                },
                _ => Command::RecoverForget {
                    target,
                    transaction,
                    confirm_device,
                },
            };
            (e, k, command)
        }
    };
    Ok(Request {
        epoch,
        key,
        command,
    })
}

pub fn parse_stop(body: &[u8], current: Epoch) -> Result<(Epoch, String), RequestError> {
    let (epoch, key, _) = decode::<Empty>(body)?.request(current)?;
    Ok((epoch, key))
}

pub fn parse_mcp_enrollment(
    body: &[u8],
    current: Epoch,
) -> Result<(String, u64, bool), RequestError> {
    let (_, key, value) = decode::<McpEnrollment>(body)?.request(current)?;
    Ok((key, value.expected_generation.0, value.enabled))
}
