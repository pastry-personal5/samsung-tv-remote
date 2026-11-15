//! Semantic evidence only. Wire event names and request outcomes do not belong here.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKey {
    InstalledApps,
    AppLaunch,
    NativeIme,
    Hdmi1,
    Hdmi2,
    Hdmi3,
    Tuner,
    Sources,
}

impl CapabilityKey {
    pub const ALL: [Self; 8] = [
        Self::InstalledApps,
        Self::AppLaunch,
        Self::NativeIme,
        Self::Hdmi1,
        Self::Hdmi2,
        Self::Hdmi3,
        Self::Tuner,
        Self::Sources,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Unknown,
    Supported,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    None,
    TvResponse,
    TvEvent,
    UserConfirmation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Unobserved,
    Current,
    Stale,
}

/// A display explanation, never a mutation admission grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityReason {
    EvidenceMissing,
    EvidenceStale,
    NotSelected,
    Disconnected,
    RecoveryBlocked,
    FeatureUnsupported,
    Connected,
    Observed,
}
