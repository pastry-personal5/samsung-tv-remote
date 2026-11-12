//! Headless, bounded application owner. Storage workers retain their slot until
//! they actually return, including after a caller-visible timeout.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{FutureExt, StreamExt};
use tokio::sync::{mpsc, oneshot};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::Instant;

use super::control_service::*;
use super::device_repository::{RepositoryError, SavedDevice};
use super::operations::Operations;
use super::remote_dispatcher::{Admission, RequestId};
use super::tv_control_coordinator::{
    AppServices, TvControlCoordinator, WakeFailure, WakePlan, WakeStage,
};
use super::tv_session::TvSessionEvent;
use super::tv_setup_service::TvSetupPort;
use super::wake_transport::WakeSendError;
use super::ControlState;
#[path = "runtime_setup.rs"]
mod setup;
use crate::domain::DeviceId;
use setup::{LiveObservation, PairContext, PreparedPair};
#[path = "runtime_connection.rs"]
mod connection;
use connection::{ConnectionContext, ConnectionMode, PreparedConnection, WakeState};

/// Explicit production defaults; small overrides permit deterministic pressure
/// tests without thousands of tasks or a real network.
#[derive(Debug, Clone)]
pub struct RuntimeLimits {
    pub ingress: usize,
    pub registry: usize,
    pub effects: usize,
    pub subscribers: usize,
    pub subscribers_per_client: usize,
    pub subscriber_backlog: usize,
    pub saved_devices: usize,
    pub observations: usize,
    pub observations_per_client: usize,
    pub retention: Duration,
    pub storage_deadline: Duration,
    pub drain_deadline: Duration,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            ingress: 128,
            registry: 32_768,
            effects: 8,
            subscribers: 64,
            subscribers_per_client: 4,
            subscriber_backlog: 64,
            saved_devices: 1024,
            observations: 128,
            observations_per_client: 8,
            retention: Duration::from_secs(1800),
            storage_deadline: Duration::from_secs(10),
            drain_deadline: Duration::from_secs(5),
        }
    }
}

/// Composition owns this handle separately from service handles. Dropping a
/// service or a reply receiver never shuts down the owner or cancels admission.
pub struct Runtime {
    sender: mpsc::Sender<Message>,
    drain: Option<oneshot::Sender<()>>,
    drained: oneshot::Receiver<DrainStatus>,
    task: JoinHandle<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrainStatus {
    pub unresolved_storage: bool,
    pub pending_effects: usize,
}

/// The drain boundary is finite, but a blocking store call cannot be aborted.
/// Retain this handle and wait for settlement before reusing the same profile.
/// Dropping it detaches supervision; it never releases the owner's fences.
pub struct DrainHandle {
    pub status: DrainStatus,
    task: JoinHandle<()>,
}

impl DrainHandle {
    pub fn is_settled(&self) -> bool {
        self.task.is_finished()
    }

    pub async fn settled(self) -> Result<(), tokio::task::JoinError> {
        self.task.await
    }
}

impl Runtime {
    pub fn start(services: AppServices, limits: RuntimeLimits) -> Self {
        assert!(limits.ingress > 0 && limits.registry > 0 && limits.effects > 0);
        assert!(limits.subscriber_backlog > 0);
        let (sender, receiver) = mpsc::channel(limits.ingress);
        let (drain, drained) = oneshot::channel();
        let (report, reported) = oneshot::channel();
        let task = tokio::spawn(Owner::new(services, limits).run(receiver, drained, report));
        Self {
            sender,
            drain: Some(drain),
            drained: reported,
            task,
        }
    }

    pub fn client(&self, client: ClientId) -> ControlService {
        ControlService {
            client,
            sender: self.sender.clone(),
        }
    }

    /// Close admission immediately and report after at most the drain deadline.
    /// Unresolved workers remain supervised until their actual completion.
    pub async fn drain(mut self) -> DrainHandle {
        if let Some(drain) = self.drain.take() {
            let _ = drain.send(());
        }
        let status = self.drained.await.unwrap_or(DrainStatus {
            unresolved_storage: true,
            pending_effects: 0,
        });
        DrainHandle {
            status,
            task: self.task,
        }
    }
}

struct RestoredProfile {
    devices: Vec<SavedDevice>,
    selected: Option<SavedDevice>,
    recovery: Option<super::recovery::ForgetMarker>,
    pairing: Option<super::recovery::PairingMarker>,
}
type Restored = Result<RestoredProfile, RepositoryError>;

enum Completion {
    Failed(OperationId),
    Restore(Restored),
    Storage(
        OperationId,
        Result<SavedDevice, RepositoryError>,
        bool,
        Restored,
    ),
    Forget(
        OperationId,
        Result<super::tv_setup_service::ForgetResult, RepositoryError>,
        Restored,
    ),
    Discovery(
        OperationId,
        ClientId,
        Result<Vec<super::tv_address::TvHost>, super::tv_discovery::DiscoveryError>,
    ),
    PreparedPair(
        OperationId,
        PairContext,
        Result<PreparedPair, super::tv_control_coordinator::PairFlowError>,
    ),
    PairCommitted(
        OperationId,
        PairContext,
        Option<Box<dyn super::tv_session::TvConnection>>,
        Result<SavedDevice, super::tv_setup_service::SetupError>,
        Restored,
    ),
    PairRecovered(
        OperationId,
        Result<super::tv_setup_service::ForgetResult, RepositoryError>,
        Restored,
    ),
    Probe(
        OperationId,
        ClientId,
        super::tv_address::TvHost,
        Result<super::tv_session::ProbeObservation, super::tv_session::TvSessionError>,
    ),
    Material(
        OperationId,
        ConnectionContext,
        Result<super::tv_setup_service::ReconnectMaterial, super::tv_setup_service::ReconnectError>,
    ),
    ConnectionPrepared(
        OperationId,
        ConnectionContext,
        Result<PreparedConnection, super::tv_control_coordinator::ConnectFlowError>,
    ),
    RotationCommitted(
        OperationId,
        ConnectionContext,
        Box<dyn super::tv_session::TvConnection>,
        Result<(), super::tv_setup_service::SetupError>,
        Restored,
    ),
    WakeSent(OperationId, Result<(), WakeSendError>),
}

struct Owner {
    coordinator: TvControlCoordinator,
    snapshot: Snapshot,
    operations: Operations,
    limits: RuntimeLimits,
    workers: JoinSet<(Option<OperationId>, Completion)>,
    subscribers: Vec<(ClientId, mpsc::Sender<Event>)>,
    storage: Option<(OperationId, Instant)>,
    remotes: HashMap<RequestId, (OperationId, Instant)>,
    timed_out: HashSet<OperationId>,
    session_events: mpsc::Receiver<(u64, u64, TvSessionEvent)>,
    session_sender: mpsc::Sender<(u64, u64, TvSessionEvent)>,
    session_task: Option<JoinHandle<()>>,
    restoring: bool,
    restore_deadline: Instant,
    observations: HashMap<OperationId, LiveObservation>,
    candidates: HashMap<CandidateId, (ClientId, super::tv_address::TvHost, Instant)>,
    pairs: HashMap<OperationId, PairContext>,
    connections: HashMap<OperationId, ConnectionContext>,
    wakes: HashMap<OperationId, WakeState>,
}

impl Owner {
    fn new(services: AppServices, limits: RuntimeLimits) -> Self {
        let epoch = Epoch(uuid::Uuid::new_v4().as_u128());
        let mut coordinator = TvControlCoordinator::new(services);
        coordinator.dispatcher = super::remote_dispatcher::RemoteDispatcher::new(32, 33);
        let snapshot = Snapshot {
            epoch,
            revision: 0,
            startup: StartupState::Restoring,
            control: ControlState::none(),
            devices: Arc::new(Vec::new()),
            configuration_revisions: Arc::new(Vec::new()),
            duplicate_records: false,
            storage_pending: false,
            recovery: None,
            pairing_recovery: None,
            wake: super::tv_control_coordinator::WakeStage::ConfigurationRequired,
            pairing_pending: false,
            wake_operation: None,
        };
        let (session_sender, session_events) = mpsc::channel(32);
        Self {
            coordinator,
            snapshot,
            operations: Operations::new(epoch, limits.registry, limits.retention),
            limits: limits.clone(),
            workers: JoinSet::new(),
            subscribers: Vec::new(),
            storage: None,
            remotes: HashMap::new(),
            timed_out: HashSet::new(),
            session_events,
            session_sender,
            session_task: None,
            restoring: true,
            restore_deadline: Instant::now() + limits.storage_deadline,
            observations: HashMap::new(),
            candidates: HashMap::new(),
            pairs: HashMap::new(),
            connections: HashMap::new(),
            wakes: HashMap::new(),
        }
    }

    async fn run(
        mut self,
        mut ingress: mpsc::Receiver<Message>,
        mut drain: oneshot::Receiver<()>,
        report: oneshot::Sender<DrainStatus>,
    ) {
        let setup = self.coordinator.service.clone();
        self.workers.spawn(async move {
            let result = tokio::task::spawn_blocking(move || read_profile(setup))
                .await
                .unwrap_or(Err(RepositoryError::Unavailable));
            (None, Completion::Restore(result))
        });
        let mut clock = tokio::time::interval(Duration::from_millis(100));
        clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut draining = false;
        let mut ingress_open = true;
        let mut report = Some(report);
        let mut drain_at = None;
        let mut transitions = 0;
        loop {
            // A hostile session stream cannot monopolize the owner. Lifecycle
            // and completion checks still run between every external request.
            transitions += 1;
            if transitions == 16 {
                if let Ok(message) = ingress.try_recv() {
                    self.message(message);
                }
                transitions = 0;
            }
            tokio::select! {
                biased;
                _ = &mut drain, if !draining => {
                    draining = true;
                    drain_at = Some(Instant::now() + self.limits.drain_deadline);
                    self.begin_drain();
                }
                completion = self.workers.join_next(), if !self.workers.is_empty() => {
                    if let Some(Ok((_, completion))) = completion {
                        self.complete(completion, draining);
                    }
                }
                _ = tokio::time::sleep_until(drain_at.unwrap_or_else(Instant::now)),
                    if draining && report.is_some() => {
                    self.report_drain(&mut report);
                }
                _ = clock.tick() => {
                    self.operations.expire();
                    self.expire_wake();
                    self.candidates.retain(|_, (_, _, expires)| *expires > Instant::now());
                    if self.restoring && Instant::now() >= self.restore_deadline && self.snapshot.startup == StartupState::Restoring {
                        self.snapshot.startup = StartupState::Blocked(RepositoryError::Unavailable);
                        self.publish(None);
                    }
                    self.observations.retain(|_, record| record.expires > Instant::now());
                    if let Some((id, deadline)) = self.storage {
                        if Instant::now() >= deadline && self.timed_out.insert(id) {
                            if let Some(context) = self.connections.get(&id).copied() {
                                self.clear_connecting(context, super::ConnectionState::Failed);
                            }
                            if self.wakes.contains_key(&id) { self.cancel_wakes(); }
                            else { self.finish(id, Outcome::Deadline); }
                        }
                    }
                    if self.remotes.values().any(|(_, deadline)| Instant::now() >= *deadline) {
                        self.coordinator.stop_active();
                        let generation = self.coordinator.app_state.selection_generation();
                        self.coordinator.app_state.set_connection_state(generation, super::ConnectionState::Failed);
                        self.coordinator.app_state.reset_playback_state(generation);
                        self.advance_remote();
                    }
                }
                event = self.session_events.recv() => {
                    if let Some((generation, session, event)) = event {
                        self.coordinator.process_session_event(generation, session, event);
                        self.advance_remote();
                        self.publish(None);
                    }
                }
                message = ingress.recv(), if ingress_open => {
                    match message {
                        Some(message) => self.message(message),
                        None => {
                            // A closed channel is permanently ready. Disable it
                            // so detached callers cannot create a hot owner loop.
                            ingress_open = false;
                            if !draining {
                                draining = true;
                                drain_at = Some(Instant::now() + self.limits.drain_deadline);
                                self.begin_drain();
                            }
                        }
                    }
                }
            }
            if draining && self.workers.is_empty() {
                self.report_drain(&mut report);
                break;
            }
        }
    }

    fn begin_drain(&mut self) {
        self.snapshot.startup = StartupState::Draining;
        self.cancel_wakes();
        self.coordinator.stop_active();
        if let Some(task) = self.session_task.take() {
            task.abort();
        }
        self.collect_remote_results();
        if let Some((id, _)) = self.storage {
            self.finish(id, Outcome::StorageUncertain);
        }
        self.operations.drain();
        self.publish(None);
    }

    fn report_drain(&self, report: &mut Option<oneshot::Sender<DrainStatus>>) {
        if let Some(report) = report.take() {
            let _ = report.send(DrainStatus {
                // Restore may be blocked in a synchronous store read as well.
                unresolved_storage: self.storage.is_some() || self.restoring,
                pending_effects: self.workers.len(),
            });
        }
    }

    fn message(&mut self, message: Message) {
        self.operations.expire();
        match message {
            Message::Snapshot(reply) => {
                let _ = reply.send(self.snapshot.clone());
            }
            Message::Lookup(client, epoch, key, reply) => {
                let result = if epoch != self.snapshot.epoch {
                    Lookup::UnknownAfterRestart
                } else {
                    self.operations
                        .lookup(client, &key)
                        .map(Lookup::Known)
                        .unwrap_or(Lookup::OutcomeUnavailable)
                };
                let _ = reply.send(result);
            }
            Message::Operation(client, epoch, id, reply) => {
                let result = if epoch != self.snapshot.epoch {
                    Lookup::UnknownAfterRestart
                } else {
                    self.operations
                        .operation(client, id)
                        .map(Lookup::Known)
                        .unwrap_or(Lookup::OutcomeUnavailable)
                };
                let _ = reply.send(result);
            }
            Message::Subscribe(client, reply) => {
                self.subscribers.retain(|(_, sender)| !sender.is_closed());
                if self.subscribers.len() >= self.limits.subscribers
                    || self
                        .subscribers
                        .iter()
                        .filter(|(id, _)| *id == client)
                        .count()
                        >= self.limits.subscribers_per_client
                {
                    let _ = reply.send(Err(ServiceError::SubscriberLimit));
                } else {
                    let (sender, events) = mpsc::channel(self.limits.subscriber_backlog);
                    let subscription = Subscription {
                        snapshot: self.snapshot.clone(),
                        events,
                    };
                    if reply.send(Ok(subscription)).is_ok() {
                        self.subscribers.push((client, sender));
                    }
                }
            }
            Message::Submit(client, request, reply) => {
                let result = self.submit(client, request);
                let _ = reply.send(result);
            }
        }
    }

    fn submit(&mut self, client: ClientId, request: Request) -> Result<Receipt, ServiceError> {
        if request.epoch != self.snapshot.epoch {
            return Err(ServiceError::StaleEpoch);
        }
        if request.key.is_empty() || request.key.len() > 128 {
            return Err(ServiceError::InvalidKey);
        }
        if matches!(&request.command, Command::Pair { label, .. } if label.len() > 256) {
            return Err(ServiceError::InvalidPayload);
        }
        if self.snapshot.startup == StartupState::Draining {
            return self
                .operations
                .duplicate(client, &request.key, &request.command)?
                .ok_or(ServiceError::Closed);
        }
        let key = request.key.clone();
        let (receipt, fresh) =
            self.operations
                .admit(client, request.key, request.command.clone())?;
        if !fresh {
            return Ok(receipt);
        }
        let recovery = self.snapshot.startup == StartupState::RecoveryRequired
            && matches!(
                request.command,
                Command::RecoverForget { .. }
                    | Command::RecoverPairing { .. }
                    | Command::Probe(_)
                    | Command::ProbeCandidate { .. }
                    | Command::Pair {
                        recovery: Some(_),
                        ..
                    }
            );
        if self.snapshot.startup != StartupState::Ready && !recovery {
            self.finish(receipt.id, Outcome::Rejected(Rejection::Blocked));
        } else {
            self.execute(client, receipt.id, request.command);
        }
        // A policy failure is admitted and retained, including in the first reply.
        Ok(self
            .operations
            .lookup(client, &key)
            .expect("reserved operation"))
    }

    fn execute(&mut self, client: ClientId, id: OperationId, command: Command) {
        let selection = match &command {
            Command::Pair { generation, .. } | Command::Select { generation, .. } => {
                Some((None, *generation))
            }
            Command::SetWake {
                target, generation, ..
            }
            | Command::Connect { target, generation }
            | Command::Wake { target, generation }
            | Command::CancelWake {
                target, generation, ..
            } => Some((Some(*target), *generation)),
            Command::Remote(request) => {
                Some((Some(request.target()), request.selection_generation()))
            }
            _ => None,
        };
        if selection.is_some_and(|(target, generation)| {
            generation != self.coordinator.app_state.selection_generation()
                || target.is_some_and(|target| {
                    Some(target) != self.coordinator.app_state.selected_device()
                })
        }) {
            self.finish(id, Outcome::Rejected(Rejection::StaleSelection));
            return;
        }
        if let Command::CancelWake { operation, .. } = command {
            if !self.wakes.contains_key(&operation) {
                self.finish(id, Outcome::Rejected(Rejection::MissingOperation));
                return;
            }
            self.cancel_wakes();
            self.finish(id, Outcome::Wake(self.coordinator.wake_stage));
            return;
        }
        if self.storage.is_some() {
            self.finish(id, Outcome::Rejected(Rejection::Busy));
            return;
        }
        let command = match command {
            Command::ProbeCandidate { candidate, host } => {
                if !self
                    .candidates
                    .get(&candidate)
                    .is_some_and(|(owner, target, expires)| {
                        *owner == client && *target == host && *expires > Instant::now()
                    })
                {
                    self.finish(id, Outcome::Rejected(Rejection::InvalidObservation));
                    return;
                }
                Command::Probe(host)
            }
            command => command,
        };
        match command {
            Command::ProbeCandidate { .. } => unreachable!(),
            Command::Pair {
                observation,
                host,
                pin,
                label,
                replace,
                generation,
                recovery,
            } => self.prepare_pair(
                client,
                id,
                PairContext {
                    client,
                    observation,
                    host,
                    pin,
                    label,
                    replace,
                    generation,
                    recovery,
                },
            ),
            Command::RecoverPairing {
                target,
                transaction,
                confirm_device,
            } => self.recover_pair(id, target, transaction, confirm_device),
            Command::CancelWake { .. } => unreachable!(),
            Command::Wake { .. } => self.admit_wake(id, false),
            Command::Forget {
                target,
                revision,
                confirm_device,
            } => {
                let error = if confirm_device != target {
                    Some(Rejection::InvalidConfirmation)
                } else if !self
                    .snapshot
                    .devices
                    .iter()
                    .any(|device| device.id == target)
                {
                    Some(Rejection::MissingDevice)
                } else if !self
                    .snapshot
                    .configuration_revisions
                    .iter()
                    .any(|(device, current)| *device == target && *current == revision)
                {
                    Some(Rejection::StaleConfiguration)
                } else {
                    None
                };
                if let Some(error) = error {
                    self.finish(id, Outcome::Rejected(error));
                } else {
                    self.forget(id, target, None);
                }
            }
            Command::RecoverForget {
                target,
                transaction,
                confirm_device,
            } => {
                if confirm_device != target {
                    self.finish(id, Outcome::Rejected(Rejection::InvalidConfirmation));
                } else if let Some(marker) = self
                    .snapshot
                    .recovery
                    .filter(|marker| marker.target == target && marker.transaction == transaction)
                {
                    self.forget(id, target, Some(marker));
                } else {
                    self.finish(id, Outcome::Rejected(Rejection::Blocked));
                }
            }
            Command::Select { target, .. } => self.storage_write(id, target, None),
            Command::SetWake {
                target,
                revision,
                configuration,
                ..
            } => {
                if !self
                    .snapshot
                    .configuration_revisions
                    .iter()
                    .any(|(device, current)| *device == target && *current == revision)
                {
                    self.finish(id, Outcome::Rejected(Rejection::StaleConfiguration));
                } else if configuration.active.is_some() && configuration.active_mac().is_none() {
                    self.finish(id, Outcome::Rejected(Rejection::InvalidConfiguration));
                } else {
                    self.storage_write(id, target, Some(configuration));
                }
            }
            Command::Remote(request) => {
                if request.action() == crate::domain::RemoteAction::PowerToggle
                    && self.coordinator.app_state.connection_state()
                        != super::ConnectionState::Ready
                {
                    self.admit_wake(id, true);
                } else {
                    self.queue_remote(id, request);
                }
            }
            Command::Connect { target, generation } => self.start_connection(
                id,
                ConnectionContext {
                    target,
                    generation,
                    mode: ConnectionMode::Connect,
                    deadline: Instant::now() + Duration::from_secs(75),
                },
            ),
            Command::Discover => {
                if self.workers.len() >= self.limits.effects.min(2) {
                    self.finish(id, Outcome::Rejected(Rejection::Busy));
                    return;
                }
                let Some(discovery) = self.coordinator.discovery.clone() else {
                    self.finish(id, Outcome::Rejected(Rejection::Blocked));
                    return;
                };
                self.spawn(id, async move {
                    let result =
                        tokio::time::timeout(Duration::from_secs(10), discovery.discover())
                            .await
                            .unwrap_or(Err(super::tv_discovery::DiscoveryError::Unavailable));
                    Completion::Discovery(id, client, result)
                });
                self.publish(Some(id));
            }
            Command::Probe(host) => {
                if self.workers.len() >= self.limits.effects.min(2) {
                    self.finish(id, Outcome::Rejected(Rejection::Busy));
                    return;
                }
                let Some(gateway) = self.coordinator.gateway.clone() else {
                    self.finish(id, Outcome::Rejected(Rejection::Blocked));
                    return;
                };
                self.observations
                    .retain(|_, record| record.expires > Instant::now());
                if self.observations.len() + self.workers.len() >= self.limits.observations
                    || self
                        .observations
                        .values()
                        .filter(|o| o.client == client)
                        .count()
                        + self.workers.len()
                        >= self.limits.observations_per_client
                {
                    self.finish(id, Outcome::Rejected(Rejection::Busy));
                    return;
                }
                let probe_host = host.clone();
                self.spawn(id, async move {
                    let result = tokio::time::timeout(Duration::from_secs(20), gateway.probe(host))
                        .await
                        .unwrap_or(Err(super::tv_session::TvSessionError::Timeout));
                    Completion::Probe(id, client, probe_host, result)
                });
                self.publish(Some(id));
            }
        }
    }

    fn queue_remote(&mut self, id: OperationId, request: super::remote_request::SendRemoteAction) {
        match self.coordinator.admit_remote(request) {
            Admission::Queued(request) => {
                self.remotes
                    .insert(request, (id, Instant::now() + Duration::from_secs(136)));
                self.advance_remote();
            }
            Admission::Rejected(reason) => {
                self.finish(id, Outcome::Rejected(Rejection::Remote(reason)))
            }
        }
    }

    fn adopt_session(
        &mut self,
        generation: u64,
        connection: Box<dyn super::tv_session::TvConnection>,
    ) {
        if let Some(task) = self.session_task.take() {
            task.abort();
        }
        let (session, mut events) = self.coordinator.install_session(generation, connection);
        let sender = self.session_sender.clone();
        self.session_task = Some(tokio::spawn(async move {
            while let Some(event) = events.next().await {
                if sender.send((generation, session, event)).await.is_err() {
                    return;
                }
            }
            let _ = sender
                .send((generation, session, TvSessionEvent::Disconnected))
                .await;
        }));
    }

    fn spawn(
        &mut self,
        id: OperationId,
        work: impl std::future::Future<Output = Completion> + Send + 'static,
    ) {
        self.workers.spawn(async move {
            let completion = std::panic::AssertUnwindSafe(work).catch_unwind().await;
            // Convert panic to a tagged completion so the original reservation settles.
            (Some(id), completion.unwrap_or(Completion::Failed(id)))
        });
    }

    fn storage_write(
        &mut self,
        id: OperationId,
        target: DeviceId,
        configuration: Option<super::wake::WakeConfiguration>,
    ) {
        if self.workers.len() >= self.limits.effects {
            self.finish(id, Outcome::Rejected(Rejection::Busy));
            return;
        }
        if !self
            .coordinator
            .saved_devices
            .iter()
            .any(|device| device.id == target)
        {
            self.finish(id, Outcome::Rejected(Rejection::MissingDevice));
            return;
        }
        let Some(setup) = self.coordinator.service.clone() else {
            self.finish(id, Outcome::Rejected(Rejection::Blocked));
            return;
        };
        self.storage = Some((id, Instant::now() + self.limits.storage_deadline));
        // Selection fences all session writes before its durable commit starts.
        if configuration.is_none() {
            self.cancel_wakes();
            self.coordinator.stop_active();
            let generation = self.coordinator.app_state.selection_generation();
            self.coordinator
                .app_state
                .set_connection_state(generation, super::ConnectionState::NotConnected);
            self.coordinator.app_state.reset_playback_state(generation);
            self.collect_remote_results();
        }
        self.spawn(id, async move {
            let (result, profile) = tokio::task::spawn_blocking(move || {
                let result = match configuration {
                    Some(configuration) => setup.save_wake_configuration(target, configuration),
                    None => setup.select_saved(target),
                };
                (result, read_profile(Some(setup)))
            })
            .await
            .unwrap_or((
                Err(RepositoryError::Unavailable),
                Err(RepositoryError::Unavailable),
            ));
            Completion::Storage(id, result, configuration.is_none(), profile)
        });
        self.publish(Some(id));
    }

    fn forget(
        &mut self,
        id: OperationId,
        target: DeviceId,
        recovery: Option<super::recovery::ForgetMarker>,
    ) {
        if self.workers.len() >= self.limits.effects {
            self.finish(id, Outcome::Rejected(Rejection::Busy));
            return;
        }
        let Some(setup) = self.coordinator.service.clone() else {
            self.finish(id, Outcome::Rejected(Rejection::Blocked));
            return;
        };
        self.storage = Some((id, Instant::now() + self.limits.storage_deadline));
        if self.coordinator.app_state.selected_device() == Some(target) {
            self.cancel_wakes();
            self.coordinator.stop_active();
            let generation = self.coordinator.app_state.selection_generation();
            self.coordinator
                .app_state
                .set_connection_state(generation, super::ConnectionState::NotConnected);
            self.coordinator.app_state.reset_playback_state(generation);
            self.collect_remote_results();
        }
        self.spawn(id, async move {
            let result = tokio::task::spawn_blocking(move || {
                let result = match recovery {
                    Some(marker) => setup.recover_forget(marker),
                    None => setup.forget_durable(target),
                };
                // Re-read durable facts even after a failed marker update; never
                // infer selection or cleanup from an attempted write.
                let profile = read_profile(Some(setup));
                (result, profile)
            })
            .await
            .unwrap_or((
                Err(RepositoryError::Unavailable),
                Err(RepositoryError::Unavailable),
            ));
            Completion::Forget(id, result.0, result.1)
        });
        self.publish(Some(id));
    }

    fn complete(&mut self, completion: Completion, draining: bool) {
        let completion = match self.complete_connection(completion, draining) {
            Some(completion) => completion,
            None => return,
        };
        let completion = match self.complete_setup(completion, draining) {
            Some(completion) => completion,
            None => return,
        };
        match completion {
            Completion::PreparedPair(..)
            | Completion::PairCommitted(..)
            | Completion::PairRecovered(..) => unreachable!(),
            Completion::Failed(id) => {
                self.pairs.remove(&id);
                if let Some(context) = self.connections.remove(&id) {
                    self.clear_connecting(context, super::ConnectionState::Failed);
                }
                if self.wakes.contains_key(&id) {
                    self.cancel_wakes();
                }
                if self.storage.is_some_and(|(operation, _)| operation == id) {
                    self.storage = None;
                    self.timed_out.remove(&id);
                }
                self.finish(id, Outcome::SupervisorFailed);
                self.publish(None);
            }
            Completion::Restore(result) => {
                self.restoring = false;
                if !draining {
                    match result {
                        Ok(RestoredProfile {
                            devices,
                            selected,
                            recovery,
                            pairing,
                        }) if profile_fits(&devices, self.limits.saved_devices) => {
                            self.snapshot.duplicate_records =
                                devices.iter().enumerate().any(|(index, device)| {
                                    devices[..index]
                                        .iter()
                                        .any(|other| other.host == device.host)
                                });
                            self.snapshot.configuration_revisions =
                                Arc::new(devices.iter().map(|device| (device.id, 0)).collect());
                            self.coordinator.saved_devices = devices;
                            self.coordinator.app_state =
                                ControlState::restored(selected.map(|device| device.display()));
                            self.snapshot.recovery = recovery;
                            self.snapshot.pairing_recovery = pairing;
                            self.snapshot.startup = if recovery.is_some() || pairing.is_some() {
                                StartupState::RecoveryRequired
                            } else {
                                StartupState::Ready
                            };
                        }
                        Ok(_) => {
                            self.snapshot.startup = StartupState::Blocked(RepositoryError::Corrupt)
                        }
                        Err(error) => self.snapshot.startup = StartupState::Blocked(error),
                    }
                }
                self.publish(None);
            }
            Completion::Forget(id, result, profile) => {
                self.storage = None;
                self.timed_out.remove(&id);
                self.reconcile_setup(profile, draining);
                self.finish(
                    id,
                    result
                        .map(Outcome::Forgotten)
                        .unwrap_or_else(Outcome::StorageFailed),
                );
                self.publish(None);
            }
            Completion::Storage(id, result, selection, profile) => {
                let generation = self.coordinator.app_state.selection_generation();
                if result.is_err() {
                    // A failed fsync/rename acknowledgment can still have changed
                    // the file. Reconcile durable facts before releasing the lane.
                    if !selection {
                        if let Ok(profile) = &profile {
                            if let Some(device) = profile.devices.iter().find(|d| {
                                self.coordinator
                                    .saved_devices
                                    .iter()
                                    .any(|old| old.id == d.id && old.wake != d.wake)
                            }) {
                                if let Some((_, revision)) =
                                    Arc::make_mut(&mut self.snapshot.configuration_revisions)
                                        .iter_mut()
                                        .find(|(id, _)| *id == device.id)
                                {
                                    *revision += 1;
                                }
                            }
                        }
                    }
                }
                // Even a successful write must leave a readable, bounded
                // profile. Publish the authoritative reread, not a cached
                // return value which can conceal a corrupt recovery fence.
                self.reconcile_setup(profile, draining);
                self.storage = None;
                self.timed_out.remove(&id);
                if let Ok(device) = &result {
                    if selection
                        && self.snapshot.startup == StartupState::Ready
                        && self.coordinator.app_state.selected_device() == Some(device.id)
                        && self.coordinator.app_state.selection_generation() == generation
                    {
                        self.coordinator
                            .app_state
                            .restart_selection(device.display());
                    } else if !selection {
                        if let Some((_, revision)) =
                            Arc::make_mut(&mut self.snapshot.configuration_revisions)
                                .iter_mut()
                                .find(|(target, _)| *target == device.id)
                        {
                            *revision += 1;
                        }
                    }
                }
                self.finish(
                    id,
                    result
                        .map(Outcome::Saved)
                        .unwrap_or_else(Outcome::StorageFailed),
                );
                self.publish(None);
            }
            Completion::Material(..)
            | Completion::ConnectionPrepared(..)
            | Completion::RotationCommitted(..)
            | Completion::WakeSent(..) => unreachable!(),
            Completion::Discovery(id, client, result) => {
                self.candidates
                    .retain(|_, (_, _, expires)| *expires > Instant::now());
                let outcome = match result {
                    Ok(hosts) if hosts.len() <= 32 => {
                        if self.candidates.len() + hosts.len() > 1024
                            || self
                                .candidates
                                .values()
                                .filter(|(owner, _, _)| *owner == client)
                                .count()
                                + hosts.len()
                                > 64
                        {
                            Outcome::Rejected(Rejection::Busy)
                        } else {
                            let candidates = hosts
                                .into_iter()
                                .enumerate()
                                .map(|(index, host)| {
                                    let candidate = Candidate {
                                        id: CandidateId(id, index as u8),
                                        host,
                                    };
                                    self.candidates.insert(
                                        candidate.id,
                                        (
                                            client,
                                            candidate.host.clone(),
                                            Instant::now() + Duration::from_secs(120),
                                        ),
                                    );
                                    candidate
                                })
                                .collect();
                            Outcome::Discovered(candidates)
                        }
                    }
                    Ok(_) => Outcome::Rejected(Rejection::Oversize),
                    Err(error) => Outcome::DiscoveryFailed(error),
                };
                self.finish(id, outcome);
            }
            Completion::Probe(id, client, host, result) => {
                let outcome = match result {
                    Ok(observation)
                        if observation
                            .name
                            .as_ref()
                            .is_none_or(|name| name.len() <= 256)
                            && observation
                                .model
                                .as_ref()
                                .is_none_or(|model| model.len() <= 256) =>
                    {
                        let observed = Observation {
                            id,
                            host,
                            observation,
                        };
                        self.observations.insert(
                            id,
                            LiveObservation {
                                client,
                                value: observed.clone(),
                                expires: Instant::now() + Duration::from_secs(300),
                            },
                        );
                        Outcome::Probed(observed)
                    }
                    Ok(_) => Outcome::Rejected(Rejection::Oversize),
                    Err(error) => Outcome::ProbeFailed(error),
                };
                self.finish(id, outcome);
            }
        }
    }

    fn advance_remote(&mut self) {
        self.collect_remote_results();
        // A failed transport handoff can complete synchronously; keep pumping
        // until either one write is active or all bounded waiting work settles.
        for _ in 0..33 {
            self.coordinator.pump_dispatch();
            if self.coordinator.terminal_results_snapshot().is_empty() {
                break;
            }
            self.collect_remote_results();
        }
    }

    fn collect_remote_results(&mut self) {
        for result in self.coordinator.terminal_results_snapshot() {
            if let Some((operation, _)) = self.remotes.remove(&result.id) {
                self.finish(operation, Outcome::Remote(result.outcome));
            }
            self.coordinator.acknowledge_terminal_through(result.id);
        }
    }

    fn finish(&mut self, id: OperationId, outcome: Outcome) {
        if self.operations.finish(id, outcome) {
            self.publish(Some(id));
        }
    }

    fn publish(&mut self, operation: Option<OperationId>) {
        self.snapshot.revision += 1;
        self.snapshot.control = self.coordinator.app_state.clone();
        self.snapshot.wake = self.coordinator.wake_stage;
        self.snapshot.wake_operation = self.wakes.keys().next().copied();
        self.snapshot.pairing_pending = !self.pairs.is_empty();
        if *self.snapshot.devices != self.coordinator.saved_devices {
            self.snapshot.devices = Arc::new(self.coordinator.saved_devices.clone());
            self.snapshot.duplicate_records =
                self.snapshot
                    .devices
                    .iter()
                    .enumerate()
                    .any(|(index, device)| {
                        self.snapshot.devices[..index]
                            .iter()
                            .any(|other| other.host == device.host)
                    });
        }
        self.snapshot.storage_pending = self.storage.is_some() || self.restoring;
        let event = Event {
            snapshot: self.snapshot.clone(),
            operation,
            phase: operation.and_then(|id| self.operations.phase(id)),
        };
        // Removing a full sender guarantees eventual EOF without requiring a
        // free slot for a special overflow message. Consumers must resubscribe.
        self.subscribers
            .retain(|(_, sender)| sender.try_send(event.clone()).is_ok());
    }
}

fn read_profile(setup: Option<Arc<dyn TvSetupPort>>) -> Restored {
    let setup = setup.ok_or(RepositoryError::Unavailable)?;
    // Load crash fences before inspecting the profile or allowing any TV I/O.
    let recovery = setup.pending_recovery()?;
    let pairing = setup.pending_pairing()?;
    if recovery.is_some() && pairing.is_some() {
        return Err(RepositoryError::Corrupt);
    }
    let devices = setup.saved_devices()?;
    let selected = setup.selected_device()?;
    if selected
        .as_ref()
        .is_some_and(|selected| !devices.iter().any(|device| device == selected))
    {
        return Err(RepositoryError::Corrupt);
    }
    Ok(RestoredProfile {
        devices,
        selected,
        recovery,
        pairing,
    })
}

fn profile_fits(devices: &[SavedDevice], capacity: usize) -> bool {
    devices.len() <= capacity
        && devices.iter().all(|device| device.label.len() <= 256)
        && devices
            .iter()
            .enumerate()
            .all(|(index, device)| !devices[..index].iter().any(|other| other.id == device.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dropped_reply_does_not_remove_admission() {
        let services = AppServices::without_adapters();
        let runtime = Runtime::start(services, RuntimeLimits::default());
        let client = runtime.client(ClientId(1));
        let snapshot = client.snapshot().await.unwrap();
        let (reply, response) = oneshot::channel();
        drop(response);
        client
            .sender
            .try_send(Message::Submit(
                ClientId(1),
                Request {
                    epoch: snapshot.epoch,
                    key: "abandoned".into(),
                    command: Command::Discover,
                },
                reply,
            ))
            .unwrap();
        // This query follows admission in the same bounded channel.
        let Lookup::Known(receipt) = client
            .lookup(snapshot.epoch, "abandoned".into())
            .await
            .unwrap()
        else {
            panic!("lost admitted result");
        };
        assert!(receipt.outcome.is_some());
        runtime.drain().await;
    }
}
