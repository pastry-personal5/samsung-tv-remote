//! Feature-gated experiment values; no production capability evidence is created here.
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::mpsc;

pub const MAX_APPS: usize = 64;
pub const MAX_INTENTS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKey {
    Hdmi1,
    Hdmi2,
    Hdmi3,
    Tuner,
    Sources,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Experiment {
    Catalog,
    LaunchA,
    LaunchB,
    Ime,
    Hdmi1,
    Hdmi2,
    Hdmi3,
    Tuner,
    Sources,
}
impl Experiment {
    pub fn source(self) -> Option<SourceKey> {
        Some(match self {
            Self::Hdmi1 => SourceKey::Hdmi1,
            Self::Hdmi2 => SourceKey::Hdmi2,
            Self::Hdmi3 => SourceKey::Hdmi3,
            Self::Tuner => SourceKey::Tuner,
            Self::Sources => SourceKey::Sources,
            _ => return None,
        })
    }
    pub fn window_seconds(self) -> u64 {
        if self == Self::Ime {
            30
        } else {
            10
        }
    }
}

/// Neither identity nor display names are printable through diagnostic Debug.
#[derive(Clone)]
pub struct DiagnosticApp {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) unicode_name: bool,
}
impl std::fmt::Debug for DiagnosticApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DiagnosticApp([redacted])")
    }
}
impl DiagnosticApp {
    pub fn new(id: &str, name: &str) -> Option<Self> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || name.len() > 128
        {
            return None;
        }
        let name: String = name.chars().filter(|c| !c.is_control() && !matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')).collect();
        if name.is_empty() {
            return None;
        }
        Some(Self {
            id: id.into(),
            unicode_name: !name.is_ascii(),
            name,
        })
    }
}

#[derive(Debug, Clone)]
pub enum WireExperiment {
    Catalog,
    Launch(DiagnosticApp),
    Source(SourceKey),
    Ime,
}
pub struct SessionRequest {
    pub sequence: u64,
    pub experiment: WireExperiment,
    pub alive: Arc<AtomicBool>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteOutcome {
    ObservationOnly,
    NotSent,
    Written,
    Uncertain,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayloadShape {
    Absent,
    Null,
    Object,
    Array,
    String,
    Number,
    Boolean,
}
#[derive(Debug)]
pub enum CandidateEvent {
    Catalog(Vec<DiagnosticApp>),
    Launch(PayloadShape),
    ImeStart(PayloadShape),
    ImeEnd(PayloadShape),
    Unknown,
    Malformed,
    Limit,
}
#[derive(Debug)]
pub enum SessionObservation {
    Write(u64, WriteOutcome),
    Frame {
        sequence: u64,
        bytes: usize,
        event: CandidateEvent,
    },
    End(u64),
}
pub struct DiagnosticPort {
    pub commands: mpsc::Sender<SessionRequest>,
    pub events: mpsc::Receiver<SessionObservation>,
}

/// A stream owns this guard. Connection loss revokes it without an ingress slot.
pub struct LeaseStream {
    pub id: String,
    pub alive: Arc<AtomicBool>,
}
impl Drop for LeaseStream {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Association {
    None,
    AmbiguousSingleFlight,
    PassiveEvent,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AppChoice {
    pub choice: usize,
    pub name: String,
}
impl std::fmt::Debug for AppChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppChoice([redacted])")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentRecord {
    pub sequence: String,
    pub admitted_at_unix_ms: Option<String>,
    pub first_frame_at_unix_ms: Option<String>,
    pub window_seconds: u64,
    pub intent: String,
    pub alias_revision: Option<String>,
    pub experiment: Experiment,
    pub write: Option<WriteOutcome>,
    pub complete: bool,
    pub completed_at_unix_ms: Option<String>,
    pub elapsed_ms: Option<String>,
    #[serde(skip)]
    pub(crate) admitted_mono: Option<tokio::time::Instant>,
    pub inconclusive: bool,
    pub association: Association,
    pub frames: u32,
    pub max_frame_bytes: usize,
    pub catalog_count: Option<usize>,
    pub catalog_events: u32,
    pub catalog_unicode_names: Option<Vec<bool>>,
    pub launch_shape: Option<PayloadShape>,
    pub ime_start_shape: Option<PayloadShape>,
    pub ime_end_shape: Option<PayloadShape>,
    pub launch_events: u32,
    pub ime_starts: u32,
    pub ime_ends: u32,
    pub unknown_events: u32,
    pub malformed_events: u32,
    pub limit_hit: bool,
}
impl ExperimentRecord {
    pub(crate) fn finish(&mut self) {
        self.complete = true;
        self.completed_at_unix_ms = wall_time();
        self.elapsed_ms = self
            .admitted_mono
            .map(|t| t.elapsed().as_millis().to_string());
    }
    pub(crate) fn new(sequence: u64, intent: String, experiment: Experiment) -> Self {
        Self {
            sequence: sequence.to_string(),
            admitted_at_unix_ms: wall_time(),
            first_frame_at_unix_ms: None,
            window_seconds: experiment.window_seconds(),
            intent,
            alias_revision: None,
            experiment,
            write: None,
            complete: false,
            completed_at_unix_ms: None,
            elapsed_ms: None,
            admitted_mono: Some(tokio::time::Instant::now()),
            inconclusive: false,
            association: Association::None,
            frames: 0,
            max_frame_bytes: 0,
            catalog_count: None,
            catalog_events: 0,
            catalog_unicode_names: None,
            launch_shape: None,
            ime_start_shape: None,
            ime_end_shape: None,
            launch_events: 0,
            ime_starts: 0,
            ime_ends: 0,
            unknown_events: 0,
            malformed_events: 0,
            limit_hit: false,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticStatus {
    pub lease: String,
    pub closed: bool,
    pub catalog_quarantined: bool,
    pub launch_quarantined: bool,
    pub records: Vec<ExperimentRecord>,
    /// Local choice display only; export removes this field and substitutes aliases.
    pub apps: Vec<AppChoice>,
    pub aliases_set: bool,
}

pub(crate) enum DiagnosticMessage {
    Open {
        epoch: super::control_service::Epoch,
        target: crate::domain::DeviceId,
        generation: u64,
        authorization: super::control_service::AdmissionAuthorization,
        reply:
            tokio::sync::oneshot::Sender<Result<LeaseStream, super::control_service::ServiceError>>,
    },
    Intent {
        lease: String,
        key: String,
        experiment: Experiment,
        authorization: super::control_service::AdmissionAuthorization,
        reply: tokio::sync::oneshot::Sender<
            Result<DiagnosticStatus, super::control_service::ServiceError>,
        >,
    },
    Choose {
        lease: String,
        a: usize,
        b: usize,
        reply: tokio::sync::oneshot::Sender<
            Result<DiagnosticStatus, super::control_service::ServiceError>,
        >,
    },
    Status {
        lease: String,
        reply: tokio::sync::oneshot::Sender<
            Result<DiagnosticStatus, super::control_service::ServiceError>,
        >,
    },
}

impl super::control_service::ControlService {
    pub async fn diagnostic_open(
        &self,
        epoch: super::control_service::Epoch,
        target: crate::domain::DeviceId,
        generation: u64,
        authorization: super::control_service::AdmissionAuthorization,
    ) -> Result<LeaseStream, super::control_service::ServiceError> {
        let (reply, response) = tokio::sync::oneshot::channel();
        self.exchange(
            super::control_service::Message::Diagnostic(
                self.client,
                DiagnosticMessage::Open {
                    epoch,
                    target,
                    generation,
                    authorization,
                    reply,
                },
            ),
            response,
        )
        .await?
    }
    pub async fn diagnostic_intent(
        &self,
        lease: String,
        key: String,
        experiment: Experiment,
        authorization: super::control_service::AdmissionAuthorization,
    ) -> Result<DiagnosticStatus, super::control_service::ServiceError> {
        let (reply, response) = tokio::sync::oneshot::channel();
        self.exchange(
            super::control_service::Message::Diagnostic(
                self.client,
                DiagnosticMessage::Intent {
                    lease,
                    key,
                    experiment,
                    authorization,
                    reply,
                },
            ),
            response,
        )
        .await?
    }
    pub async fn diagnostic_choose(
        &self,
        lease: String,
        a: usize,
        b: usize,
    ) -> Result<DiagnosticStatus, super::control_service::ServiceError> {
        let (reply, response) = tokio::sync::oneshot::channel();
        self.exchange(
            super::control_service::Message::Diagnostic(
                self.client,
                DiagnosticMessage::Choose { lease, a, b, reply },
            ),
            response,
        )
        .await?
    }
    pub async fn diagnostic_status(
        &self,
        lease: String,
    ) -> Result<DiagnosticStatus, super::control_service::ServiceError> {
        let (reply, response) = tokio::sync::oneshot::channel();
        self.exchange(
            super::control_service::Message::Diagnostic(
                self.client,
                DiagnosticMessage::Status { lease, reply },
            ),
            response,
        )
        .await?
    }
}

pub(crate) fn wall_time() -> Option<String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis().to_string())
}
