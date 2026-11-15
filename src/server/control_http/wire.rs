//! Canonical, lossless identifiers shared by HTTP and native clients.
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::application::control_service::{CandidateId, Epoch, OperationId};
use crate::application::sources::{SourceChoice, SourceConfiguration, SourceLabel, SourceSetting};
use crate::domain::DeviceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    InvalidId,
    StaleEpoch,
}

fn uuid(value: &str) -> Result<uuid::Uuid, WireError> {
    let parsed = uuid::Uuid::parse_str(value).map_err(|_| WireError::InvalidId)?;
    if parsed.hyphenated().to_string() != value {
        return Err(WireError::InvalidId);
    }
    Ok(parsed)
}

pub fn epoch(value: Epoch) -> String {
    uuid::Uuid::from_u128(value.as_u128())
        .hyphenated()
        .to_string()
}

pub fn parse_epoch(value: &str) -> Result<Epoch, WireError> {
    Ok(Epoch::from_u128(uuid(value)?.as_u128()))
}

pub fn device(value: DeviceId) -> String {
    // Device record keys remain unchanged on disk.
    let key = value.to_string();
    let bits = key[4..].parse::<u128>().expect("device record identity");
    uuid::Uuid::from_u128(bits).hyphenated().to_string()
}

pub fn parse_device(value: &str) -> Result<DeviceId, WireError> {
    DeviceId::parse_record_key(&format!("dev_{}", uuid(value)?.as_u128()))
        .ok_or(WireError::InvalidId)
}

fn scoped(kind: u8, index: u8, operation: OperationId) -> String {
    let mut bytes = [0_u8; 16];
    bytes[..4].copy_from_slice(b"STR\x01");
    bytes[4] = kind;
    bytes[5] = index;
    bytes[8..].copy_from_slice(&operation.0.to_be_bytes());
    uuid::Uuid::from_bytes(bytes).hyphenated().to_string()
}

pub fn operation(value: OperationId) -> String {
    scoped(1, 0, value)
}

pub fn candidate(value: CandidateId) -> String {
    scoped(2, value.1, value.0)
}

fn parse_scoped(
    value: &str,
    supplied_epoch: Epoch,
    current_epoch: Epoch,
    kind: u8,
) -> Result<(OperationId, u8), WireError> {
    if supplied_epoch != current_epoch {
        return Err(WireError::StaleEpoch);
    }
    let bytes = *uuid(value)?.as_bytes();
    let id = u64::from_be_bytes(bytes[8..].try_into().expect("eight bytes"));
    if &bytes[..4] != b"STR\x01"
        || bytes[4] != kind
        || bytes[6..8] != [0, 0]
        || id == 0
        || (kind == 1 && bytes[5] != 0)
        || (kind == 2 && bytes[5] >= 32)
    {
        return Err(WireError::InvalidId);
    }
    Ok((OperationId(id), bytes[5]))
}

pub fn parse_operation(
    value: &str,
    supplied: Epoch,
    current: Epoch,
) -> Result<OperationId, WireError> {
    parse_scoped(value, supplied, current, 1).map(|(id, _)| id)
}

pub fn parse_candidate(
    value: &str,
    supplied: Epoch,
    current: Epoch,
) -> Result<CandidateId, WireError> {
    parse_scoped(value, supplied, current, 2).map(|(id, index)| CandidateId(id, index))
}

/// JSON strings prevent rounding in JavaScript and MCP clients. Only canonical
/// unsigned decimal spelling is accepted; numbers and leading zeroes fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counter(pub u64);

impl Serialize for Counter {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Counter {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        let number: u64 = value.parse().map_err(serde::de::Error::custom)?;
        if number.to_string() != value {
            return Err(serde::de::Error::custom("noncanonical counter"));
        }
        Ok(Self(number))
    }
}

/// Source block shared by requests, responses and the native client. Typed
/// structs reject missing, duplicate and unknown keys before any effect.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sources {
    hdmi1: SourceEntry,
    hdmi2: SourceEntry,
    hdmi3: SourceEntry,
    tuner: SourceEntry,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceEntry {
    state: SourceState,
    #[serde(default)]
    label: Option<String>,
    /// Present only in owner responses; a request cannot assert evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    confirmed_at_unix_ms: Option<Counter>,
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SourceState {
    Unconfigured,
    Hidden,
    Shown,
}

impl Sources {
    fn entries(self) -> [SourceEntry; 4] {
        [self.hdmi1, self.hdmi2, self.hdmi3, self.tuner]
    }
    fn from_entries([hdmi1, hdmi2, hdmi3, tuner]: [SourceEntry; 4]) -> Self {
        Self {
            hdmi1,
            hdmi2,
            hdmi3,
            tuner,
        }
    }
    pub fn from_choices(choices: &[SourceChoice; 4]) -> Self {
        Self::from_entries(choices.clone().map(|choice| match choice {
            SourceChoice::Unconfigured => SourceEntry::bare(SourceState::Unconfigured),
            SourceChoice::Hidden => SourceEntry::bare(SourceState::Hidden),
            SourceChoice::Shown(label) => SourceEntry {
                state: SourceState::Shown,
                label: label.map(|l| l.as_str().to_owned()),
                confirmed_at_unix_ms: None,
            },
        }))
    }
    pub fn from_configuration(value: &SourceConfiguration) -> Self {
        Self::from_entries((*value.ports).clone().map(|setting| match setting {
            SourceSetting::Unconfigured => SourceEntry::bare(SourceState::Unconfigured),
            SourceSetting::Hidden => SourceEntry::bare(SourceState::Hidden),
            SourceSetting::Shown {
                label,
                confirmed_at_unix_ms,
            } => SourceEntry {
                state: SourceState::Shown,
                label: label.map(|l| l.as_str().to_owned()),
                confirmed_at_unix_ms: Some(Counter(confirmed_at_unix_ms)),
            },
        }))
    }
    /// Request form: labels only on Shown, validated, and no timestamps.
    pub fn choices(self) -> Option<[SourceChoice; 4]> {
        let entries = self.entries();
        if entries.iter().any(|e| e.confirmed_at_unix_ms.is_some()) {
            return None;
        }
        let mut choices: [SourceChoice; 4] = Default::default();
        for (choice, entry) in choices.iter_mut().zip(entries) {
            *choice = match (entry.state, entry.label) {
                (SourceState::Unconfigured, None) => SourceChoice::Unconfigured,
                (SourceState::Hidden, None) => SourceChoice::Hidden,
                (SourceState::Shown, None) => SourceChoice::Shown(None),
                (SourceState::Shown, Some(label)) => {
                    SourceChoice::Shown(Some(SourceLabel::parse(&label)?))
                }
                _ => return None,
            };
        }
        Some(choices)
    }
    /// Response form: a Shown entry needs a positive time; others have none.
    pub fn configuration(self) -> Option<SourceConfiguration> {
        let mut ports: [SourceSetting; 4] = Default::default();
        for (port, entry) in ports.iter_mut().zip(self.entries()) {
            *port = match (entry.state, entry.label, entry.confirmed_at_unix_ms) {
                (SourceState::Unconfigured, None, None) => SourceSetting::Unconfigured,
                (SourceState::Hidden, None, None) => SourceSetting::Hidden,
                (SourceState::Shown, label, Some(Counter(at))) if at > 0 => {
                    let label = match label {
                        Some(label) => {
                            let parsed = SourceLabel::parse(&label)?;
                            // Stored spelling is already trimmed; anything else is malformed.
                            if parsed.as_str() != label {
                                return None;
                            }
                            Some(parsed)
                        }
                        None => None,
                    };
                    SourceSetting::Shown {
                        label,
                        confirmed_at_unix_ms: at,
                    }
                }
                _ => return None,
            };
        }
        Some(SourceConfiguration {
            ports: Box::new(ports),
        })
    }
}
impl SourceEntry {
    fn bare(state: SourceState) -> Self {
        Self {
            state,
            label: None,
            confirmed_at_unix_ms: None,
        }
    }
}
