//! Transport-independent contracts for the headless owner. Client identities are
//! assigned by the composition/access layer, never parsed from a TV command.
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use super::device_repository::{RepositoryError, SavedDevice};
use super::remote_dispatcher::{DispatchRejection, TerminalOutcome};
use super::remote_request::SendRemoteAction;
use super::tv_address::TvHost;
use super::tv_discovery::DiscoveryError;
use super::tv_session::{ProbeObservation, TvSessionError};
use super::wake::WakeConfiguration;
use super::ControlState;
use crate::domain::DeviceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Epoch(pub(crate) u128);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Discover,
    Probe(TvHost),
    ProbeCandidate {
        candidate: CandidateId,
        host: TvHost,
    },
    Pair {
        observation: OperationId,
        host: TvHost,
        pin: super::certificate_trust::CertificatePin,
        label: String,
        replace: Option<DeviceId>,
        generation: u64,
        /// Exact interrupted re-pair transaction, with fresh TV approval.
        recovery: Option<DeviceId>,
    },
    RecoverPairing {
        target: DeviceId,
        transaction: DeviceId,
        confirm_device: DeviceId,
    },
    Select {
        target: DeviceId,
        generation: u64,
    },
    SetWake {
        target: DeviceId,
        generation: u64,
        revision: u64,
        configuration: WakeConfiguration,
    },
    Connect {
        target: DeviceId,
        generation: u64,
    },
    Forget {
        target: DeviceId,
        revision: u64,
        confirm_device: DeviceId,
    },
    RecoverForget {
        target: DeviceId,
        transaction: DeviceId,
        confirm_device: DeviceId,
    },
    Remote(SendRemoteAction),
    /// Sends at most one packet using the configuration active at admission.
    Wake {
        target: DeviceId,
        generation: u64,
    },
    CancelWake {
        operation: OperationId,
        target: DeviceId,
        generation: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub epoch: Epoch,
    pub key: String,
    pub command: Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CandidateId(pub OperationId, pub u8);
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: CandidateId,
    pub host: TvHost,
}

/// Client-bound, expiring evidence. Retention of an outcome does not extend
/// this handle's authority; Pair validates the live owner-side record again.
#[derive(Debug, Clone)]
pub struct Observation {
    pub id: OperationId,
    pub host: TvHost,
    pub observation: ProbeObservation,
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Discovered(Vec<Candidate>),
    Probed(Observation),
    Paired(SavedDevice),
    PairFailed(super::tv_control_coordinator::PairFlowError),
    Saved(SavedDevice),
    Connected,
    ConnectFailed(super::tv_control_coordinator::ConnectFlowError),
    Remote(TerminalOutcome),
    /// Terminal wake stage; packet-sent facts survive cancellation.
    Wake(super::tv_control_coordinator::WakeStage),
    Rejected(Rejection),
    DiscoveryFailed(DiscoveryError),
    ProbeFailed(TvSessionError),
    StorageFailed(RepositoryError),
    /// Drain interrupted observation of a worker which may still mutate storage.
    /// This does not claim cancellation or rollback.
    StorageUncertain,
    Forgotten(super::tv_setup_service::ForgetResult),
    Deadline,
    SupervisorFailed,
    Drained,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    Busy,
    Blocked,
    StaleSelection,
    StaleConfiguration,
    MissingDevice,
    InvalidConfiguration,
    Remote(DispatchRejection),
    Oversize,
    InvalidConfirmation,
    WakeNotConfigured,
    MissingOperation,
    InvalidObservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationMetadata {
    pub epoch: Epoch,
    pub target: Option<DeviceId>,
    pub generation: Option<u64>,
}
impl Command {
    pub(crate) fn metadata(&self, epoch: Epoch) -> OperationMetadata {
        let (target, generation) = match self {
            Self::Select { target, generation }
            | Self::Connect { target, generation }
            | Self::SetWake {
                target, generation, ..
            }
            | Self::Wake { target, generation }
            | Self::CancelWake {
                target, generation, ..
            } => (Some(*target), Some(*generation)),
            Self::Pair {
                replace,
                generation,
                ..
            } => (*replace, Some(*generation)),
            Self::Forget { target, .. }
            | Self::RecoverForget { target, .. }
            | Self::RecoverPairing { target, .. } => (Some(*target), None),
            Self::Remote(request) => (Some(request.target()), Some(request.selection_generation())),
            _ => (None, None),
        };
        OperationMetadata {
            epoch,
            target,
            generation,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Receipt {
    pub id: OperationId,
    pub metadata: OperationMetadata,
    pub outcome: Option<Outcome>,
}

#[derive(Debug, Clone)]
pub enum Lookup {
    Known(Receipt),
    UnknownAfterRestart,
    OutcomeUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceError {
    Busy,
    Closed,
    StaleEpoch,
    InvalidKey,
    KeyConflict,
    RegistryFull,
    SubscriberLimit,
    ClientLimit,
    InvalidPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupState {
    Restoring,
    Ready,
    Blocked(RepositoryError),
    RecoveryRequired,
    Draining,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub epoch: Epoch,
    pub revision: u64,
    pub startup: StartupState,
    pub control: ControlState,
    pub devices: Arc<Vec<SavedDevice>>,
    pub configuration_revisions: Arc<Vec<(DeviceId, u64)>>,
    /// Duplicate hosts are preserved for explicit resolution, never deleted.
    pub duplicate_records: bool,
    pub storage_pending: bool,
    pub recovery: Option<super::recovery::ForgetMarker>,
    pub pairing_recovery: Option<super::recovery::PairingMarker>,
    pub wake: super::tv_control_coordinator::WakeStage,
    pub wake_operation: Option<OperationId>,
    pub pairing_pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationPhase {
    Pending,
    Terminal,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub snapshot: Snapshot,
    /// Cross-client events expose only this opaque identifier.
    pub operation: Option<OperationId>,
    pub phase: Option<OperationPhase>,
}

/// A closed receiver means resubscribe atomically. No replay is promised.
pub struct Subscription {
    pub snapshot: Snapshot,
    pub events: mpsc::Receiver<Event>,
}

pub(crate) enum Message {
    Submit(
        ClientId,
        Request,
        oneshot::Sender<Result<Receipt, ServiceError>>,
    ),
    Snapshot(oneshot::Sender<Snapshot>),
    Subscribe(
        ClientId,
        oneshot::Sender<Result<Subscription, ServiceError>>,
    ),
    Lookup(ClientId, Epoch, String, oneshot::Sender<Lookup>),
    Operation(ClientId, Epoch, OperationId, oneshot::Sender<Lookup>),
}

#[derive(Clone)]
pub struct ControlService {
    pub(crate) client: ClientId,
    pub(crate) sender: mpsc::Sender<Message>,
}

impl ControlService {
    async fn exchange<T>(
        &self,
        message: Message,
        response: oneshot::Receiver<T>,
    ) -> Result<T, ServiceError> {
        self.sender.try_send(message).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => ServiceError::Busy,
            mpsc::error::TrySendError::Closed(_) => ServiceError::Closed,
        })?;
        response.await.map_err(|_| ServiceError::Closed)
    }

    pub async fn submit(&self, request: Request) -> Result<Receipt, ServiceError> {
        let (reply, response) = oneshot::channel();
        self.exchange(Message::Submit(self.client, request, reply), response)
            .await?
    }

    pub async fn snapshot(&self) -> Result<Snapshot, ServiceError> {
        let (reply, response) = oneshot::channel();
        self.exchange(Message::Snapshot(reply), response).await
    }

    pub async fn subscribe(&self) -> Result<Subscription, ServiceError> {
        let (reply, response) = oneshot::channel();
        self.exchange(Message::Subscribe(self.client, reply), response)
            .await?
    }

    pub async fn lookup(&self, epoch: Epoch, key: String) -> Result<Lookup, ServiceError> {
        let (reply, response) = oneshot::channel();
        self.exchange(Message::Lookup(self.client, epoch, key, reply), response)
            .await
    }

    pub async fn operation(&self, epoch: Epoch, id: OperationId) -> Result<Lookup, ServiceError> {
        let (reply, response) = oneshot::channel();
        self.exchange(Message::Operation(self.client, epoch, id, reply), response)
            .await
    }
}
