//! Canonical, lossless identifiers shared by HTTP and native clients.
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::application::control_service::{CandidateId, Epoch, OperationId};
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
