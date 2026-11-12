//! One native background attachment loop. It retries only connection and
//! observation; mutation payloads are never retained as replayable work.
use super::{
    http::{ClientError, Events, HttpClient},
    projection::{self, NativeReceipt},
    reconnect::{Blocked, Reconnect, State, ATTEMPT_TIMEOUT, HEALTHY_RESET},
};
use crate::{
    application::control_service::{Epoch, OperationId, Request, Snapshot},
    server::{
        bootstrap::{BootstrapError, Role},
        launch::{self, LaunchError},
        lifecycle::endpoint::build_identity,
    },
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{mpsc, watch, Semaphore};
use tokio::time::{timeout, Instant};

#[derive(Debug, Clone)]
pub enum Status {
    Connecting,
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
    Operation(NativeReceipt),
    Recovered {
        tag: u64,
        receipt: NativeReceipt,
    },
    Unknown {
        tag: u64,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitError {
    Unavailable,
    Busy,
    Invalid,
    NotAdmitted,
    Ambiguous,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    InFlight,
    Uncertain,
    Pending,
}
struct Intent {
    epoch: Epoch,
    key: String,
    phase: Phase,
}
type Intents = Arc<Mutex<HashMap<u64, Intent>>>;

// Dropping an HTTP future cannot prove that the owner did not admit its request.
// Retain the original key for lookup even when the submitting UI task is cancelled.
struct SubmissionGuard {
    intents: Intents,
    tag: u64,
}
impl Drop for SubmissionGuard {
    fn drop(&mut self) {
        if let Some(intent) = self.intents.lock().expect("intents").get_mut(&self.tag) {
            if intent.phase == Phase::InFlight {
                intent.phase = Phase::Uncertain;
            }
        }
    }
}
#[derive(Clone)]
pub struct Config {
    pub executable: PathBuf,
    pub base: PathBuf,
}
pub struct Remote {
    config: Config,
    client: Arc<Mutex<Option<HttpClient>>>,
    intents: Intents,
    closed: watch::Sender<bool>,
    signals: mpsc::Sender<Blocked>,
    updates: Mutex<Option<mpsc::Receiver<Update>>>,
    management: Semaphore,
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
        let executable = config.executable.clone();
        let build = tokio::task::spawn_blocking(move || build_identity(&executable))
            .await
            .map_err(|_| LaunchError::Unavailable)?
            .map_err(LaunchError::Endpoint)?;
        let (closed, close_rx) = watch::channel(false);
        let (signals, signal_rx) = mpsc::channel(2);
        let (output, updates) = mpsc::channel(16);
        let client = Arc::new(Mutex::new(None));
        let intents = Arc::new(Mutex::new(HashMap::new()));
        tokio::spawn(run(
            config.clone(),
            build,
            client.clone(),
            intents.clone(),
            output,
            close_rx,
            signal_rx,
        ));
        Ok(Arc::new(Self {
            config,
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
        let _slot = self
            .management
            .try_acquire()
            .map_err(|_| LaunchError::Busy)?;
        let _ = launch::resume(&self.config.executable, &self.config.base, Role::Gui).await?;
        let _ = self.signals.send(Blocked::Stopped).await;
        Ok(())
    }
    pub async fn restart(&self) -> Result<(), LaunchError> {
        let _slot = self
            .management
            .try_acquire()
            .map_err(|_| LaunchError::Busy)?;
        let _ = launch::restart(&self.config.executable, &self.config.base, Role::Gui).await?;
        let _ = self.signals.send(Blocked::IncompatibleBuild).await;
        Ok(())
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
        {
            let mut intents = self.intents.lock().expect("intents");
            if intents.len() >= 64 || intents.contains_key(&tag) {
                return Err(SubmitError::Busy);
            }
            intents.insert(
                tag,
                Intent {
                    epoch: request.epoch,
                    key: request.key.clone(),
                    phase: Phase::InFlight,
                },
            );
        }
        let _submission = SubmissionGuard {
            intents: self.intents.clone(),
            tag,
        };
        match client.call(method, &path, Some(&body)).await {
            Ok((_, value)) if value.get("operation_id").is_some() => {
                match NativeReceipt::parse(&value, request.epoch) {
                    Ok(receipt) => {
                        let mut intents = self.intents.lock().expect("intents");
                        if receipt.terminal {
                            intents.remove(&tag);
                        } else if let Some(intent) = intents.get_mut(&tag) {
                            intent.phase = Phase::Pending;
                        }
                        Ok(receipt)
                    }
                    Err(_) => {
                        self.uncertain(tag);
                        Err(SubmitError::Ambiguous)
                    }
                }
            }
            Ok((status, value))
                if matches!(status, 400 | 401 | 403 | 404 | 409 | 413 | 415 | 429)
                    && value.get("operation_id").is_none() =>
            {
                self.intents.lock().expect("intents").remove(&tag);
                Err(SubmitError::NotAdmitted)
            }
            _ => {
                self.uncertain(tag);
                Err(SubmitError::Ambiguous)
            }
        }
    }
    fn uncertain(&self, tag: u64) {
        if let Some(intent) = self.intents.lock().expect("intents").get_mut(&tag) {
            intent.phase = Phase::Uncertain;
        }
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
fn blocked(error: LaunchError) -> Option<Blocked> {
    match error {
        LaunchError::Stopped => Some(Blocked::Stopped),
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
    config: &Config,
    build: &str,
) -> Result<(HttpClient, Events, Snapshot), LaunchError> {
    let client =
        launch::start_with_build(&config.executable, &config.base, Role::Gui, build).await?;
    let mut events = client.events().await.map_err(LaunchError::Client)?;
    let initial = events.next().await.map_err(LaunchError::Client)?;
    let snapshot = projection::snapshot(&initial.state).map_err(LaunchError::Client)?;
    Ok((client, events, snapshot))
}
async fn recover(
    client: &HttpClient,
    intents: &Intents,
    output: &mpsc::Sender<Update>,
    closed: &mut watch::Receiver<bool>,
) -> bool {
    let pending: Vec<_> = intents
        .lock()
        .expect("intents")
        .iter()
        .filter(|(_, i)| i.phase != Phase::InFlight)
        .map(|(tag, i)| (*tag, i.epoch, i.key.clone(), i.phase))
        .collect();
    for (tag, epoch, key, phase) in pending {
        let path = format!(
            "/v1/operations/by-key/{key}?epoch={}",
            crate::server::control_http::wire::epoch(epoch)
        );
        let result = tokio::select! {biased;_=closed.changed()=>return false,result=client.call("GET",&path,None)=>result};
        match result {
            Ok((_, value)) if value.get("operation_id").is_some() => {
                if let Ok(receipt) = NativeReceipt::parse(&value, epoch) {
                    if receipt.terminal {
                        intents.lock().expect("intents").remove(&tag);
                    } else if let Some(intent) = intents.lock().expect("intents").get_mut(&tag) {
                        intent.phase = Phase::Pending;
                    }
                    let update = if phase == Phase::Uncertain {
                        Update::Recovered { tag, receipt }
                    } else {
                        Update::Operation(receipt)
                    };
                    if !publish(output, update, closed).await {
                        return false;
                    }
                }
            }
            Ok((410, _)) => {
                intents.lock().expect("intents").remove(&tag);
                if !publish(output, Update::Unknown { tag }, closed).await {
                    return false;
                }
            }
            _ => (),
        }
    }
    true
}
async fn run(
    config: Config,
    build: String,
    current: Arc<Mutex<Option<HttpClient>>>,
    intents: Intents,
    output: mpsc::Sender<Update>,
    mut closed: watch::Receiver<bool>,
    mut signals: mpsc::Receiver<Blocked>,
) {
    let mut policy = Reconnect::new(Instant::now());
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
                tokio::select! {biased;_=closed.changed()=>break,reason=signals.recv()=>{if let Some(reason)=reason{policy.resolved(reason,Instant::now());}continue;},_=tokio::time::sleep_until(due)=>()}
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
                tokio::select! {biased;_=closed.changed()=>break,reason=signals.recv()=>{if let Some(reason)=reason{policy.resolved(reason,Instant::now());}continue;}}
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
        let connected = tokio::select! {biased;_=closed.changed()=>break,result=timeout(ATTEMPT_TIMEOUT,attach(&config,&build))=>result};
        let (client, mut events, snapshot) = match connected {
            Ok(Ok(connected)) => connected,
            Ok(Err(error)) => {
                if let Some(reason) = blocked(error) {
                    policy.block(reason);
                } else {
                    policy.failed(Instant::now());
                }
                continue;
            }
            Err(_) => {
                policy.failed(Instant::now());
                continue;
            }
        };
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
        let mut healthy = false;
        let healthy_at = Instant::now() + HEALTHY_RESET;
        // Lookups may each wait for an HTTP timeout. Keep consuming SSE and
        // observing disconnects while recovering the bounded set of intents.
        let mut recovery_closed = closed.clone();
        let recovery = async {
            let mut lookups = tokio::time::interval(Duration::from_secs(1));
            lookups.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                lookups.tick().await;
                if !recover(&client, &intents, &output, &mut recovery_closed).await {
                    break;
                }
            }
        };
        tokio::pin!(recovery);
        loop {
            tokio::select! {biased;
                _=closed.changed()=>break 'connection,
                _=tokio::time::sleep_until(healthy_at),if !healthy=>{healthy=true;policy.tick(Instant::now());},
                _=&mut recovery=>break 'connection,
                event=events.next()=>match event {
                    Ok(event)=>{
                        let snapshot=match projection::snapshot(&event.state){Ok(value)=>value,Err(_)=>{policy.block(Blocked::UnsafeMetadata);break;}};
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
            config: Config {
                executable: PathBuf::new(),
                base: PathBuf::new(),
            },
            client: Arc::new(Mutex::new(Some(HttpClient {
                endpoint,
                bearer: "fake".into(),
            }))),
            intents: Arc::new(Mutex::new(HashMap::new())),
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
        let intents = remote.intents.lock().unwrap();
        let intent = intents.get(&7).unwrap();
        assert_eq!(intent.epoch, epoch);
        assert_eq!(intent.key, key);
        assert!(intent.phase == Phase::Uncertain);
    }
}
