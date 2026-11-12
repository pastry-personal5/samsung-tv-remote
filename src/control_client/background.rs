//! One native background attachment loop. It retries only connection and
//! observation; mutation payloads are never retained as replayable work.
use super::tracker::Settlement;
pub use super::tracker::UnknownReason;
use super::{
    http::{ClientError, Events, HttpClient},
    projection::{self, NativeReceipt},
    reconnect::{Blocked, Reconnect, State, ATTEMPT_TIMEOUT, HEALTHY_RESET},
    tracker::{ReserveError, Ticket, Tracker},
};
use crate::{
    application::control_service::{Epoch, OperationId, Request, Snapshot},
    server::{
        bootstrap::{BootstrapError, Role},
        launch::{LaunchError, Launcher},
        lifecycle::endpoint::build_identity,
    },
};
use futures_util::{stream, StreamExt};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, watch, Notify, Semaphore};
use tokio::time::{timeout, Instant};

#[derive(Debug, Clone)]
pub enum Status {
    Connecting,
    Starting,
    Waiting { due: Instant },
    Connected,
    Blocked(Blocked),
    Exited,
}
#[derive(Debug, Clone)]
pub enum Update {
    Status(Status),
    Snapshot {
        snapshot: Box<Snapshot>,
        initial: bool,
    },
    History(NativeReceipt),
    Rejected {
        tag: u64,
        code: super::model::FailureCode,
    },
    KnownBeforeRestart {
        tag: u64,
        receipt: NativeReceipt,
    },
    HistoryUnavailable {
        epoch: Epoch,
        id: OperationId,
        reason: UnknownReason,
    },
    Recovered {
        tag: u64,
        receipt: NativeReceipt,
    },
    Unknown {
        tag: u64,
        reason: UnknownReason,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitError {
    Unavailable,
    Busy,
    Invalid,
    NotAdmitted(super::model::FailureCode),
    Ambiguous,
    Superseded,
}
#[derive(Default)]
struct Tracking {
    tracker: Mutex<Tracker>,
    changed: Notify,
}
impl Tracking {
    fn lock(&self) -> std::sync::LockResult<std::sync::MutexGuard<'_, Tracker>> {
        self.tracker.lock()
    }
}
type Intents = Arc<Tracking>;

// Dropping an HTTP future cannot prove that the owner did not admit its request.
// Retain the original key for lookup even when the submitting UI task is cancelled.
struct SubmissionGuard {
    intents: Intents,
    ticket: Ticket,
}
impl Drop for SubmissionGuard {
    fn drop(&mut self) {
        self.intents.lock().expect("intents").ambiguous(self.ticket);
        self.intents.changed.notify_one();
    }
}
#[derive(Clone)]
pub struct Config {
    pub executable: PathBuf,
    pub base: PathBuf,
}
pub struct Remote {
    executable: PathBuf,
    launcher: Launcher,
    client: Arc<Mutex<Option<HttpClient>>>,
    intents: Intents,
    closed: watch::Sender<bool>,
    signals: mpsc::Sender<Control>,
    updates: Mutex<Option<mpsc::Receiver<Update>>>,
    management: Semaphore,
}
enum Control {
    Resolved(Blocked),
    Block(Blocked),
    PauseStop(oneshot::Sender<()>),
    Recheck(oneshot::Sender<Result<(), LaunchError>>),
}
struct Attachment {
    executable: PathBuf,
    launcher: Launcher,
    build: String,
}
impl std::fmt::Debug for Remote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Remote")
    }
}
impl Drop for Remote {
    fn drop(&mut self) {
        let _ = self.closed.send(true);
    }
}
impl Remote {
    pub async fn start(config: Config) -> Result<Arc<Self>, LaunchError> {
        let launcher = Launcher::development(&config.base)?;
        Self::start_launcher(config.executable, launcher).await
    }
    pub async fn production(executable: PathBuf) -> Result<Arc<Self>, LaunchError> {
        Self::start_launcher(executable, Launcher::production()?).await
    }
    async fn start_launcher(
        executable: PathBuf,
        launcher: Launcher,
    ) -> Result<Arc<Self>, LaunchError> {
        let build_executable = executable.clone();
        let build = tokio::task::spawn_blocking(move || build_identity(&build_executable))
            .await
            .map_err(|_| LaunchError::Unavailable)?
            .map_err(LaunchError::Endpoint)?;
        let (closed, close_rx) = watch::channel(false);
        let (signals, signal_rx) = mpsc::channel(2);
        let (output, updates) = mpsc::channel(16);
        let client = Arc::new(Mutex::new(None));
        let intents = Arc::new(Tracking::default());
        tokio::spawn(run(
            Attachment {
                executable: executable.clone(),
                launcher: launcher.clone(),
                build,
            },
            client.clone(),
            intents.clone(),
            output,
            close_rx,
            signal_rx,
        ));
        Ok(Arc::new(Self {
            executable,
            launcher,
            client,
            intents,
            closed,
            signals,
            updates: Mutex::new(Some(updates)),
            management: Semaphore::new(1),
        }))
    }
    pub fn take_updates(&self) -> Option<mpsc::Receiver<Update>> {
        self.updates.lock().expect("updates").take()
    }
    pub fn connected(&self) -> bool {
        self.client.lock().expect("client").is_some()
    }
    pub fn disconnect(&self) {
        let _ = self.closed.send(true);
    }
    pub async fn resume(&self) -> Result<(), LaunchError> {
        if *self.closed.borrow() {
            return Err(LaunchError::Unavailable);
        }
        let _slot = self
            .management
            .try_acquire()
            .map_err(|_| LaunchError::Busy)?;
        let _ = self.launcher.resume(&self.executable, Role::Gui).await?;
        let _ = self.signals.send(Control::Resolved(Blocked::Stopped)).await;
        Ok(())
    }
    pub async fn restart(&self) -> Result<(), LaunchError> {
        if *self.closed.borrow() {
            return Err(LaunchError::Unavailable);
        }
        let _slot = self
            .management
            .try_acquire()
            .map_err(|_| LaunchError::Busy)?;
        let _ = self.launcher.restart(&self.executable, Role::Gui).await?;
        let _ = self
            .signals
            .send(Control::Resolved(Blocked::IncompatibleBuild))
            .await;
        Ok(())
    }
    pub async fn recheck(&self) -> Result<(), LaunchError> {
        if *self.closed.borrow() {
            return Err(LaunchError::Unavailable);
        }
        let _slot = self
            .management
            .try_acquire()
            .map_err(|_| LaunchError::Busy)?;
        let (reply, received) = oneshot::channel();
        self.signals
            .send(Control::Recheck(reply))
            .await
            .map_err(|_| LaunchError::Unavailable)?;
        received.await.map_err(|_| LaunchError::Unavailable)?
    }
    pub async fn stop(&self, epoch: Epoch) -> Result<super::port::StopReport, LaunchError> {
        use super::port::{StopAcknowledgment, StopReport, StopSettlement};
        let _slot = self
            .management
            .try_acquire()
            .map_err(|_| LaunchError::Busy)?;
        let client = self
            .client
            .lock()
            .expect("client")
            .clone()
            .ok_or(LaunchError::Unavailable)?;
        let (paused, received) = oneshot::channel();
        self.signals
            .send(Control::PauseStop(paused))
            .await
            .map_err(|_| LaunchError::Unavailable)?;
        received.await.map_err(|_| LaunchError::Unavailable)?;
        if *self.closed.borrow() {
            return Err(LaunchError::Unavailable);
        }
        let value = self.launcher.stop_expected(&client, Some(epoch)).await?;
        let acknowledgment = if value["durable_stop"] == true {
            StopAcknowledgment::Durable
        } else if value["marker_observed"] == true && value["durable_acknowledgment"] == false {
            StopAcknowledgment::MarkerObserved
        } else {
            return Err(LaunchError::UnsafeMetadata);
        };
        let launcher = self.launcher.clone();
        let observed =
            tokio::task::spawn_blocking(move || launcher.stop_settlement_observed(&value)).await;
        let settlement = match observed {
            Ok(Ok(true)) => StopSettlement::Observed,
            Ok(Ok(false)) => StopSettlement::Pending,
            _ => StopSettlement::CheckFailed,
        };
        let _ = self.signals.send(Control::Block(Blocked::Stopped)).await;
        Ok(StopReport {
            acknowledgment,
            settlement,
        })
    }
    pub async fn submit(&self, tag: u64, request: Request) -> Result<NativeReceipt, SubmitError> {
        let (method, path, body) =
            projection::encode(&request).map_err(|_| SubmitError::Invalid)?;
        let client = self
            .client
            .lock()
            .expect("client")
            .clone()
            .ok_or(SubmitError::Unavailable)?;
        let ticket = self
            .intents
            .lock()
            .expect("intents")
            .reserve(request.epoch, tag, request.key.clone())
            .map_err(|error| match error {
                ReserveError::Busy => SubmitError::Busy,
                ReserveError::StaleEpoch => {
                    SubmitError::NotAdmitted(super::model::FailureCode::StaleEpoch)
                }
            })?;
        let _submission = SubmissionGuard {
            intents: self.intents.clone(),
            ticket,
        };
        match client.call(method, &path, Some(&body)).await {
            Ok((_, value)) if value.get("operation_id").is_some() => {
                match NativeReceipt::parse(&value, request.epoch) {
                    Ok(receipt) => {
                        let accepted = self
                            .intents
                            .lock()
                            .expect("intents")
                            .receipt(ticket, &receipt);
                        if accepted {
                            Ok(receipt)
                        } else if self.intents.lock().expect("intents").contains(ticket) {
                            self.uncertain(ticket);
                            Err(SubmitError::Ambiguous)
                        } else {
                            Err(SubmitError::Superseded)
                        }
                    }
                    Err(_) => {
                        self.uncertain(ticket);
                        Err(SubmitError::Ambiguous)
                    }
                }
            }
            Ok((status, value))
                if matches!(status, 400 | 401 | 403 | 404 | 409 | 413 | 415 | 429)
                    && value.get("operation_id").is_none() =>
            {
                match value["error"]["code"]
                    .as_str()
                    .and_then(|code| super::model::FailureCode::parse(code).ok())
                {
                    Some(code) => {
                        if self
                            .intents
                            .lock()
                            .expect("intents")
                            .not_admitted(ticket, code)
                        {
                            Err(SubmitError::NotAdmitted(code))
                        } else {
                            Err(SubmitError::Superseded)
                        }
                    }
                    None => {
                        self.uncertain(ticket);
                        Err(SubmitError::Ambiguous)
                    }
                }
            }
            _ => {
                self.uncertain(ticket);
                Err(SubmitError::Ambiguous)
            }
        }
    }
    fn uncertain(&self, ticket: Ticket) {
        self.intents.lock().expect("intents").ambiguous(ticket);
        self.intents.changed.notify_one();
    }
    pub async fn operation(
        &self,
        epoch: Epoch,
        id: OperationId,
    ) -> Result<NativeReceipt, ClientError> {
        let client = self
            .client
            .lock()
            .expect("client")
            .clone()
            .ok_or(ClientError::Transport)?;
        let (status, value) = client
            .call(
                "GET",
                &format!(
                    "/v1/operations/{}?epoch={}",
                    crate::server::control_http::wire::operation(id),
                    crate::server::control_http::wire::epoch(epoch)
                ),
                None,
            )
            .await?;
        if status == 410 {
            return Err(ClientError::Resnapshot);
        }
        NativeReceipt::parse(&value, epoch)
    }
}
async fn publish(
    output: &mpsc::Sender<Update>,
    update: Update,
    closed: &mut watch::Receiver<bool>,
) -> bool {
    tokio::select! {biased;_=closed.changed()=>false,result=output.send(update)=>result.is_ok()}
}
pub(super) fn blocked(error: LaunchError) -> Option<Blocked> {
    match error {
        LaunchError::Stopped => Some(Blocked::Stopped),
        LaunchError::SigningRequired => Some(Blocked::SigningRequired),
        LaunchError::Bootstrap(BootstrapError::IncompatibleBuild) => {
            Some(Blocked::IncompatibleBuild)
        }
        LaunchError::Bootstrap(BootstrapError::Denied) => Some(Blocked::BootstrapDenied),
        LaunchError::Bootstrap(BootstrapError::InteractionRequired) => {
            Some(Blocked::InteractionRequired)
        }
        LaunchError::Bootstrap(BootstrapError::CredentialUnavailable) => {
            Some(Blocked::BootstrapDenied)
        }
        LaunchError::File(_)
        | LaunchError::UnsafeMetadata
        | LaunchError::Bootstrap(
            BootstrapError::UnsafeMetadata
            | BootstrapError::UntrustedListener
            | BootstrapError::InvalidFrame,
        ) => Some(Blocked::UnsafeMetadata),
        LaunchError::Denied => Some(Blocked::BootstrapDenied),
        LaunchError::Client(ClientError::InvalidResponse | ClientError::TooLarge) => {
            Some(Blocked::UnsafeMetadata)
        }
        _ => None,
    }
}
async fn attach(
    executable: &std::path::Path,
    launcher: &Launcher,
    build: &str,
    check_only: bool,
    output: &mpsc::Sender<Update>,
    closed: &mut watch::Receiver<bool>,
) -> Result<(HttpClient, Events, Snapshot, Vec<OperationId>), LaunchError> {
    let client = if check_only {
        launcher.attach(Role::Gui, Some(build)).await?
    } else {
        launcher
            .start_with_build_observed(executable, Role::Gui, build, || async {
                if publish(output, Update::Status(Status::Starting), closed).await {
                    Ok(())
                } else {
                    Err(LaunchError::Unavailable)
                }
            })
            .await?
    };
    let mut events = client.events().await.map_err(LaunchError::Client)?;
    let initial = events.next().await.map_err(LaunchError::Client)?;
    let (snapshot, references) =
        projection::initial(&initial.state).map_err(LaunchError::Client)?;
    Ok((client, events, snapshot, references))
}
impl From<Settlement> for Update {
    fn from(value: Settlement) -> Self {
        match value {
            Settlement::Local { tag, receipt } => Self::Recovered { tag, receipt },
            Settlement::History(receipt) => Self::History(receipt),
            Settlement::NotAdmitted { tag, code } => Self::Rejected { tag, code },
            Settlement::KnownBeforeRestart { tag, receipt } => {
                Self::KnownBeforeRestart { tag, receipt }
            }
            Settlement::Unknown { tag, reason } => Self::Unknown { tag, reason },
            Settlement::HistoryUnavailable { epoch, id, reason } => {
                Self::HistoryUnavailable { epoch, id, reason }
            }
        }
    }
}
async fn deliver(
    intents: &Intents,
    output: &mpsc::Sender<Update>,
    closed: &mut watch::Receiver<bool>,
) -> bool {
    loop {
        let ready = intents.lock().expect("intents").deliveries();
        if ready.is_empty() {
            return true;
        }
        for delivery in ready {
            if !publish(output, delivery.update.into(), closed).await {
                return false;
            }
            intents
                .lock()
                .expect("intents")
                .ack(delivery.ticket, delivery.version);
        }
    }
}
async fn recover(
    client: &HttpClient,
    intents: &Intents,
    output: &mpsc::Sender<Update>,
    closed: &mut watch::Receiver<bool>,
) -> bool {
    // A direct HTTP reply is not retired until the observer channel accepts
    // its facts. Cancelling an Iced callback or this pass cannot lose them.
    if !deliver(intents, output, closed).await {
        return false;
    }
    let pending = intents.lock().expect("intents").lookups();
    let mut requests = stream::iter(pending)
        .map(|lookup| async move {
            let active = intents.lock().expect("intents").contains(lookup.ticket);
            let result = if active {
                Some(client.call("GET", &lookup.path(), None).await)
            } else {
                None
            };
            (lookup, result)
        })
        .buffer_unordered(4);
    loop {
        let next =
            tokio::select! {biased;_=closed.changed()=>return false,next=requests.next()=>next};
        let (lookup, result) = match next {
            Some((lookup, Some(result))) => (lookup, result),
            Some((_, None)) => continue,
            None => break,
        };
        match result {
            Ok((_, value)) if value.get("operation_id").is_some() => {
                if let Ok(receipt) = NativeReceipt::parse(&value, lookup.epoch) {
                    intents
                        .lock()
                        .expect("intents")
                        .receipt(lookup.ticket, &receipt);
                }
            }
            Ok((status @ (403 | 404), value))
                if matches!(
                    (status, value["error"]["code"].as_str()),
                    (403, Some("forbidden")) | (404, Some("not_found"))
                ) =>
            {
                intents
                    .lock()
                    .expect("intents")
                    .unowned_reference(lookup.ticket);
            }
            Ok((410, value)) => {
                let reason = match value["error"]["code"].as_str() {
                    Some("outcome_unavailable") => Some(UnknownReason::OutcomeUnavailable),
                    Some("unknown_after_restart") => Some(UnknownReason::UnknownAfterRestart),
                    _ => None,
                };
                if let Some(reason) = reason {
                    intents
                        .lock()
                        .expect("intents")
                        .unavailable(lookup.ticket, reason);
                }
            }
            _ => (),
        }
        if !deliver(intents, output, closed).await {
            return false;
        }
    }
    true
}

fn control(
    policy: &mut Reconnect,
    check: &mut Option<oneshot::Sender<Result<(), LaunchError>>>,
    signal: Control,
) -> bool {
    match signal {
        Control::Resolved(reason) => {
            if matches!(policy.state(), State::Subscribed { .. }) {
                policy.failed(Instant::now());
            }
            policy.resolved(reason, Instant::now());
            true
        }
        Control::Block(reason) => {
            policy.block(reason);
            true
        }
        Control::PauseStop(reply) => {
            policy.block(Blocked::StopUnconfirmed);
            let _ = reply.send(());
            true
        }
        Control::Recheck(reply) => {
            if check.is_some() || !policy.recheck(Instant::now()) {
                let error = match policy.state() {
                    State::Blocked(Blocked::Stopped) => LaunchError::Stopped,
                    State::Blocked(Blocked::SigningRequired) => LaunchError::SigningRequired,
                    State::Blocked(Blocked::IncompatibleBuild) => {
                        LaunchError::Bootstrap(BootstrapError::IncompatibleBuild)
                    }
                    _ => LaunchError::Busy,
                };
                let _ = reply.send(Err(error));
                false
            } else {
                *check = Some(reply);
                true
            }
        }
    }
}
async fn run(
    attachment: Attachment,
    current: Arc<Mutex<Option<HttpClient>>>,
    intents: Intents,
    output: mpsc::Sender<Update>,
    mut closed: watch::Receiver<bool>,
    mut signals: mpsc::Receiver<Control>,
) {
    let Attachment {
        executable,
        launcher,
        build,
    } = attachment;
    let mut policy = Reconnect::new(Instant::now());
    let mut check = None;
    'connection: loop {
        if *closed.borrow() {
            break;
        }
        match policy.state() {
            State::Waiting { due } => {
                if due > Instant::now()
                    && !publish(
                        &output,
                        Update::Status(Status::Waiting { due }),
                        &mut closed,
                    )
                    .await
                {
                    break;
                }
                tokio::select! {biased;_=closed.changed()=>break,reason=signals.recv()=>{if let Some(signal)=reason{control(&mut policy,&mut check,signal);}continue;},_=tokio::time::sleep_until(due)=>()}
            }
            State::Blocked(reason) => {
                if !publish(
                    &output,
                    Update::Status(Status::Blocked(reason)),
                    &mut closed,
                )
                .await
                {
                    break;
                }
                tokio::select! {biased;_=closed.changed()=>break,reason=signals.recv()=>{if let Some(signal)=reason{control(&mut policy,&mut check,signal);}continue;}}
            }
            State::Exited => break,
            _ => (),
        }
        if !policy.begin_attempt(Instant::now()) {
            continue;
        }
        if !publish(&output, Update::Status(Status::Connecting), &mut closed).await {
            break;
        }
        let mut starting_closed = closed.clone();
        let connected = tokio::select! {biased;_=closed.changed()=>break,result=timeout(ATTEMPT_TIMEOUT,attach(&executable,&launcher,&build,check.is_some(),&output,&mut starting_closed))=>result};
        let (client, mut events, snapshot, references) = match connected {
            Ok(Ok(connected)) => connected,
            Ok(Err(error)) => {
                if let Some(reply) = check.take() {
                    policy.block(blocked(error).unwrap_or(Blocked::RecheckFailed));
                    let _ = reply.send(Err(error));
                    continue;
                }
                if let Some(reason) = blocked(error) {
                    policy.block(reason);
                } else {
                    policy.failed(Instant::now());
                }
                continue;
            }
            Err(_) => {
                if let Some(reply) = check.take() {
                    policy.block(Blocked::RecheckFailed);
                    let _ = reply.send(Err(LaunchError::Unavailable));
                } else {
                    policy.failed(Instant::now());
                }
                continue;
            }
        };
        let settlements = intents
            .lock()
            .expect("intents")
            .attach(snapshot.epoch, &references);
        for settlement in settlements {
            if !publish(&output, settlement.into(), &mut closed).await {
                break 'connection;
            }
        }
        policy.subscribed(Instant::now());
        *current.lock().expect("client") = Some(client.clone());
        if !publish(
            &output,
            Update::Snapshot {
                snapshot: Box::new(snapshot),
                initial: true,
            },
            &mut closed,
        )
        .await
            || !publish(&output, Update::Status(Status::Connected), &mut closed).await
        {
            break;
        }
        if let Some(reply) = check.take() {
            let _ = reply.send(Ok(()));
        }
        let mut healthy = false;
        let healthy_at = Instant::now() + HEALTHY_RESET;
        // Lookups may each wait for an HTTP timeout. Keep consuming SSE and
        // observing disconnects while recovering the bounded set of intents.
        let mut recovery_closed = closed.clone();
        let recovery = async {
            let mut lookups = tokio::time::interval(Duration::from_secs(1));
            lookups.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {biased;_=recovery_closed.changed()=>break, _=intents.changed.notified()=>(), _=lookups.tick()=>()}
                if !recover(&client, &intents, &output, &mut recovery_closed).await {
                    break;
                }
            }
        };
        tokio::pin!(recovery);
        loop {
            tokio::select! {biased;
                _=closed.changed()=>break 'connection,
                signal=signals.recv()=>{if let Some(signal)=signal {if control(&mut policy,&mut check,signal) {break;}} else {break 'connection;}},
                _=tokio::time::sleep_until(healthy_at),if !healthy=>{healthy=true;policy.tick(Instant::now());},
                _=&mut recovery=>break 'connection,
                event=events.next()=>match event {
                    Ok(event)=>{
                        let (snapshot, reference)=match projection::changed(&event.state, &event.kind){Ok(value)=>value,Err(_)=>{policy.block(Blocked::UnsafeMetadata);break;}};
                        if let Some(id) = reference {
                            if !intents.lock().expect("intents").reference(snapshot.epoch, id) {
                                policy.failed(Instant::now());
                                break;
                            }
                            intents.changed.notify_one();
                        }
                        if !publish(&output,Update::Snapshot{snapshot:Box::new(snapshot),initial:false},&mut closed).await{break 'connection;}
                    },
                    Err(ClientError::InvalidResponse|ClientError::TooLarge)=>{policy.block(Blocked::UnsafeMetadata);break;},
                    Err(_)=>{policy.failed(Instant::now());break;},
                }
            }
        }
        *current.lock().expect("client") = None;
    }
    *current.lock().expect("client") = None;
    policy.exit();
    let _ = output.try_send(Update::Status(Status::Exited));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn cancelled_submission_retains_the_original_key_for_recovery() {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let endpoint = super::super::http::Endpoint::parse(&format!(
            "http://127.0.0.1:{}",
            listener.local_addr().unwrap().port()
        ))
        .unwrap();
        let (closed, _) = watch::channel(false);
        let (signals, _) = mpsc::channel(2);
        let remote = Remote {
            executable: PathBuf::new(),
            launcher: Launcher::development(std::path::Path::new("/private/tmp")).unwrap(),
            client: Arc::new(Mutex::new(Some(HttpClient {
                endpoint,
                bearer: "fake".into(),
            }))),
            intents: Arc::new(Tracking::default()),
            closed,
            signals,
            updates: Mutex::new(None),
            management: Semaphore::new(1),
        };
        let epoch = Epoch(1);
        let key = uuid::Uuid::new_v4().to_string();
        {
            let submission = remote.submit(
                7,
                Request {
                    epoch,
                    key: key.clone(),
                    command: crate::application::control_service::Command::Discover,
                },
            );
            tokio::pin!(submission);
            let received = async {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut byte = [0];
                socket.read_exact(&mut byte).await.unwrap();
                socket
            };
            tokio::select! {
                result = &mut submission => panic!("unexpected response: {result:?}"),
                _socket = received => (),
            }
            // Cancel after sending; a lost reply is not evidence of non-admission.
        }
        let lookups = remote.intents.lock().unwrap().lookups();
        assert_eq!(lookups.len(), 1);
        assert_eq!(lookups[0].epoch, epoch);
        assert_eq!(lookups[0].key.as_deref(), Some(key.as_str()));
    }
}
