//! Transport-neutral facts exposed to native clients. Unknown protocol codes
//! are decoding errors, never guessed rejection or terminal success.
use super::http::ClientError;
use serde::Deserialize;

macro_rules! codes {
    ($($name:ident => $wire:literal),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
        pub enum FailureCode { $(#[serde(rename = $wire)] $name,)* }
        impl FailureCode {
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$name => $wire,)* }
            }
            pub fn parse(value: &str) -> Result<Self, ClientError> {
                match value { $($wire => Ok(Self::$name),)* _ => Err(ClientError::InvalidResponse) }
            }
        }
    }
}
codes! {
    Busy => "busy",
    ServerStopping => "server_stopping",
    StaleEpoch => "stale_epoch",
    InvalidRequest => "invalid_request",
    OperationKeyConflict => "operation_key_conflict",
    Unauthenticated => "unauthenticated",
    Forbidden => "forbidden",
    StorageUnavailable => "storage_unavailable",
    StorageCorrupt => "storage_corrupt",
    StorageBlocked => "storage_blocked",
    InvalidPairingMaterial => "invalid_pairing_material",
    TrustUnavailable => "trust_unavailable",
    TrustCorrupt => "trust_corrupt",
    InvalidTarget => "invalid_target",
    NonLocalTarget => "non_local_target",
    TargetUnresolved => "target_unresolved",
    Offline => "offline",
    Timeout => "timeout",
    TlsFailed => "tls_failed",
    CertificateChanged => "certificate_changed",
    RemoteChannelFailed => "remote_channel_failed",
    TvDenied => "tv_denied",
    PairingRequired => "pairing_required",
    PairingTokenMissing => "pairing_token_missing",
    TvProtocolFailed => "tv_protocol_failed",
    Uncertain => "uncertain",
    UnsupportedAction => "unsupported_action",
    NoSelectedTv => "no_selected_tv",
    StaleSelection => "stale_selection",
    RemoteNotReady => "remote_not_ready",
    StaleConfig => "stale_config",
    NotFound => "not_found",
    InvalidConfiguration => "invalid_configuration",
    InputTooLarge => "input_too_large",
    InvalidConfirmation => "invalid_confirmation",
    WakeNotConfigured => "wake_not_configured",
    ObservationExpired => "observation_expired",
    InvalidLabel => "invalid_label",
    PartialSetup => "partial_setup",
    Cancelled => "cancelled",
    NetworkPermissionDenied => "network_permission_denied",
    DiscoveryUnavailable => "discovery_unavailable",
    StorageUncertain => "storage_uncertain",
    Deadline => "deadline",
    SupervisorFailed => "supervisor_failed",
    ResponseTooLarge => "response_too_large",
    InvalidId => "invalid_id",
    AmbiguousFraming => "ambiguous_framing",
    HttpVersionUnsupported => "http_version_unsupported",
    InvalidBody => "invalid_body",
    TrailersUnsupported => "trailers_unsupported",
    UnexpectedBody => "unexpected_body",
    BodyTimeout => "body_timeout",
    OutcomeUnavailable => "outcome_unavailable",
    UnknownAfterRestart => "unknown_after_restart",
    ContentEncodingUnsupported => "content_encoding_unsupported",
    ContentTypeUnsupported => "content_type_unsupported",
    RequestTimeout => "request_timeout",
    ServerStopped => "server_stopped",
    UnsafeLifecycleMetadata => "unsafe_lifecycle_metadata",
    StopPersistenceFailed => "stop_persistence_failed",
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotSent {
    Cancelled,
    TransportUnavailable,
    NoSelectedTv,
    StaleSelection,
    PairingRequired,
    RemoteNotReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanupFacts {
    pub credential_removed: bool,
    pub trust_removed: bool,
    pub record_removed: bool,
}
impl CleanupFacts {
    pub const fn complete(self) -> bool {
        self.credential_removed && self.trust_removed && self.record_removed
    }
}

// Pure display enums; these re-exports do not expose a coordinator handle.
pub use crate::application::tv_control_coordinator::{WakeFailure, WakeStage};

pub use crate::application::tv_session::ProbeObservation;
