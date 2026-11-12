//! Durable Stop actor. Dropping an HTTP reply cannot cancel an admitted intent.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

use super::files::{FileError, OwnershipLock, RuntimeDirectory};
use crate::application::control_service::{ClientId, Epoch};
use crate::application::runtime::Runtime;
use crate::server::access::AuthorizedClient;
use crate::server::control_http::{
    response::{self, ErrorCode},
    transport::{StopFuture, StopPort},
    wire,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopMarker {
    pub version: u8,
    pub profile: String,
    pub server_epoch: String,
    pub operation_key: String,
    pub identity: String,
}
impl StopMarker {
    pub fn validate(&self) -> Result<(), FileError> {
        let key = uuid::Uuid::parse_str(&self.operation_key).map_err(|_| FileError::UnsafePath)?;
        if self.version != 1
            || self.profile != "m8-fake-v1"
            || self.identity != "gui"
            || wire::parse_epoch(&self.server_epoch).is_err()
            || key.hyphenated().to_string() != self.operation_key
        {
            return Err(FileError::UnsafePath);
        }
        Ok(())
    }
}

pub trait StopStore: Send + Sync {
    fn read(&self) -> Result<Option<StopMarker>, FileError>;
    fn write(&self, marker: &StopMarker) -> Result<(), FileError>;
    /// Only an explicit, authorized launcher Resume may call this under lock.
    fn clear(&self) -> Result<(), FileError>;
}
pub struct FileStopStore(pub Arc<RuntimeDirectory>);
impl StopStore for FileStopStore {
    fn read(&self) -> Result<Option<StopMarker>, FileError> {
        let Some(bytes) = self.0.read("stopped.json", 4096)? else {
            return Ok(None);
        };
        let marker: StopMarker =
            serde_json::from_slice(&bytes).map_err(|_| FileError::UnsafePath)?;
        marker.validate()?;
        Ok(Some(marker))
    }
    fn write(&self, marker: &StopMarker) -> Result<(), FileError> {
        marker.validate()?;
        self.0.replace(
            "stopped.json",
            &serde_json::to_vec(marker).map_err(|_| FileError::Unavailable)?,
        )
    }
    fn clear(&self) -> Result<(), FileError> {
        self.read()?;
        self.0.remove("stopped.json")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Serving,
    Stopping,
    StopBlocked,
    Stopped,
    SettlementFailed,
}
impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Serving => "serving",
            Self::Stopping => "stopping",
            Self::StopBlocked => "stop_blocked",
            Self::Stopped => "stopped",
            Self::SettlementFailed => "settlement_failed",
        }
    }
}
type ResultCache = Arc<Mutex<HashMap<(ClientId, Epoch, String), Result<Value, ErrorCode>>>>;
enum Message {
    Stop(
        AuthorizedClient,
        Epoch,
        String,
        oneshot::Sender<Result<Value, ErrorCode>>,
    ),
    Signal,
}
pub struct StopClient {
    sender: mpsc::Sender<Message>,
    cache: ResultCache,
    state: watch::Receiver<State>,
    epoch: Epoch,
}
impl StopPort for StopClient {
    fn serving_state(&self) -> &'static str {
        self.state.borrow().as_str()
    }
    fn stop(&self, client: AuthorizedClient, epoch: Epoch, key: String) -> StopFuture<'_> {
        Box::pin(async move {
            client
                .require_management()
                .map_err(|_| error(403, "forbidden"))?;
            if epoch != self.epoch {
                return Err(error(409, "stale_epoch"));
            }
            let parsed = uuid::Uuid::parse_str(&key).map_err(|_| error(400, "invalid_request"))?;
            if parsed.hyphenated().to_string() != key {
                return Err(error(400, "invalid_request"));
            }
            if let Some(result) = self
                .cache
                .lock()
                .expect("stop cache")
                .get(&(client.identity, epoch, key.clone()))
                .cloned()
            {
                return result;
            }
            let (reply, response) = oneshot::channel();
            self.sender
                .try_send(Message::Stop(client, epoch, key, reply))
                .map_err(|_| error(429, "busy"))?;
            response.await.map_err(|_| error(503, "server_stopping"))?
        })
    }
}

/// Await settlement before opening a replacement profile owner. If the caller
/// drops this handle, the actor remains detached and retains the lock itself.
#[must_use]
pub struct Lifecycle {
    pub client: Arc<StopClient>,
    pub http_shutdown: watch::Receiver<bool>,
    task: JoinHandle<OwnershipLock>,
}
impl Lifecycle {
    pub async fn start(
        runtime: Runtime,
        store: Arc<dyn StopStore>,
        ownership: OwnershipLock,
    ) -> Result<Self, ErrorCode> {
        let persisted = store.clone();
        let initial = tokio::task::spawn_blocking(move || persisted.read()).await;
        let failure = match initial {
            Ok(Ok(None)) => None,
            Ok(Ok(Some(_))) => Some(error(423, "server_stopped")),
            _ => Some(error(423, "unsafe_lifecycle_metadata")),
        };
        if let Some(error) = failure {
            settle_before_release(runtime).await;
            drop(ownership);
            return Err(error);
        }
        let epoch = match runtime.client(ClientId(1)).snapshot().await {
            Ok(snapshot) => snapshot.epoch,
            Err(error) => {
                settle_before_release(runtime).await;
                drop(ownership);
                return Err(response::service_error(error));
            }
        };
        let (sender, mut receiver) = mpsc::channel(4);
        let (cache, state) = (
            Arc::new(Mutex::new(HashMap::new())),
            watch::channel(State::Serving),
        );
        let (shutdown, http_shutdown) = watch::channel(false);
        let client = Arc::new(StopClient {
            sender,
            cache: cache.clone(),
            state: state.1,
            epoch,
        });
        let task = tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                match message {
                    Message::Signal => break,
                    Message::Stop(client, supplied, key, reply) => {
                        if client.require_management().is_err() {
                            let _ = reply.send(Err(error(403, "forbidden")));
                            continue;
                        }
                        let cache_key = (client.identity, supplied, key.clone());
                        let existing = cache.lock().expect("stop cache").get(&cache_key).cloned();
                        if let Some(result) = existing {
                            let _ = reply.send(result);
                            continue;
                        }
                        if cache.lock().expect("stop cache").len() >= 64 {
                            let _ = reply.send(Err(error(429, "busy")));
                            continue;
                        }
                        if let Err(e) = runtime.fence_admission(supplied, client.admission).await {
                            let _ = reply.send(Err(response::service_error(e)));
                            continue;
                        }
                        let _ = state.0.send(State::Stopping);
                        let marker = StopMarker {
                            version: 1,
                            profile: "m8-fake-v1".into(),
                            server_epoch: wire::epoch(epoch),
                            operation_key: key,
                            identity: "gui".into(),
                        };
                        let persistence = store.clone();
                        let saved = marker.clone();
                        let result =
                            tokio::task::spawn_blocking(move || persistence.write(&saved)).await;
                        let result = match result {
                            Ok(Ok(())) => Ok(
                                json!({"server_epoch":marker.server_epoch,"operation_key":marker.operation_key,"durable_stop":true,"state":"stopping"}),
                            ),
                            _ => Err(error(503, "stop_persistence_failed")),
                        };
                        let durable = result.is_ok();
                        cache
                            .lock()
                            .expect("stop cache")
                            .insert(cache_key, result.clone());
                        let _ = reply.send(result);
                        if durable {
                            break;
                        }
                        let _ = state.0.send(State::StopBlocked);
                    }
                }
            }
            // Signals/client loss drain without creating an intentional Stop.
            let _ = state.0.send(State::Stopping);
            let drain = runtime.drain().await;
            let _ = shutdown.send(true);
            if drain.settled().await.is_err() {
                let _ = state.0.send(State::SettlementFailed);
                // An owner panic cannot prove its blocking workers settled.
                // Retain ownership until process exit instead of overlapping
                // an unknown persistence worker with a replacement runtime.
                std::future::pending::<()>().await;
            }
            let _ = state.0.send(State::Stopped);
            // Keep ownership in the completed join result until composition
            // has closed HTTP/bootstrap listeners and awaits settled(). Worker
            // settlement alone must not expose a still-bound old endpoint.
            ownership
        });
        Ok(Self {
            client,
            http_shutdown,
            task,
        })
    }
    pub fn signal_shutdown(&self) -> Result<(), ErrorCode> {
        self.client
            .sender
            .try_send(Message::Signal)
            .map_err(|_| error(429, "busy"))
    }
    pub async fn request_shutdown(&self) -> Result<(), ErrorCode> {
        self.client
            .sender
            .send(Message::Signal)
            .await
            .map_err(|_| error(503, "server_stopping"))
    }
    pub async fn settled(self) -> Result<(), tokio::task::JoinError> {
        self.task.await.map(drop)
    }
}
/// Startup failure must obey the same ownership rule as normal shutdown. A
/// failed owner join cannot prove that its blocking persistence workers ended.
/// The caller keeps its ownership guard while this future remains pending.
pub(crate) async fn settle_before_release(runtime: Runtime) {
    if runtime.drain().await.settled().await.is_err() {
        std::future::pending::<()>().await;
    }
}
fn error(status: u16, code: &'static str) -> ErrorCode {
    ErrorCode { status, code }
}
