//! Injectable native-client boundary. Presentation sees typed client facts and
//! management results, not server launch errors or runtime/storage handles.
use super::{
    background::{Config, Remote, SubmitError, Update},
    projection::NativeReceipt,
    reconnect::Blocked,
};
use crate::application::control_service::Request;
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::mpsc;

pub type ClientFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagementIntent {
    Stop {
        epoch: crate::application::control_service::Epoch,
    },
    Resume,
    Restart,
    Recheck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopAcknowledgment {
    Durable,
    MarkerObserved,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopSettlement {
    Observed,
    Pending,
    CheckFailed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopReport {
    pub acknowledgment: StopAcknowledgment,
    pub settlement: StopSettlement,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagementReply {
    ReattachRequested,
    CredentialsRechecked,
    Stop(StopReport),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagementError {
    Busy,
    Blocked(Blocked),
    Unavailable,
    StaleEpoch,
    StopOutcomeUnknown,
    StopPersistenceFailed,
}

pub trait NativeClient: std::fmt::Debug + Send + Sync {
    fn connected(&self) -> bool;
    fn take_updates(&self) -> Option<mpsc::Receiver<Update>>;
    fn submit(
        self: Arc<Self>,
        tag: u64,
        request: Request,
    ) -> ClientFuture<Result<NativeReceipt, SubmitError>>;
    fn manage(
        self: Arc<Self>,
        intent: ManagementIntent,
    ) -> ClientFuture<Result<ManagementReply, ManagementError>>;
    fn disconnect(&self);
}

pub async fn start(config: Config) -> Result<Arc<dyn NativeClient>, ManagementError> {
    Remote::start(config)
        .await
        .map(|remote| remote as Arc<dyn NativeClient>)
        .map_err(management_error)
}
pub async fn production(
    executable: std::path::PathBuf,
) -> Result<Arc<dyn NativeClient>, ManagementError> {
    Remote::production(executable)
        .await
        .map(|remote| remote as Arc<dyn NativeClient>)
        .map_err(management_error)
}
fn management_error(error: crate::server::launch::LaunchError) -> ManagementError {
    match error {
        crate::server::launch::LaunchError::Busy => return ManagementError::Busy,
        crate::server::launch::LaunchError::StaleEpoch => return ManagementError::StaleEpoch,
        crate::server::launch::LaunchError::UncertainStop => {
            return ManagementError::StopOutcomeUnknown
        }
        crate::server::launch::LaunchError::StopPersistenceFailed => {
            return ManagementError::StopPersistenceFailed
        }
        _ => (),
    }
    super::background::blocked(error).map_or(ManagementError::Unavailable, ManagementError::Blocked)
}

impl NativeClient for Remote {
    fn connected(&self) -> bool {
        Remote::connected(self)
    }
    fn take_updates(&self) -> Option<mpsc::Receiver<Update>> {
        Remote::take_updates(self)
    }
    fn submit(
        self: Arc<Self>,
        tag: u64,
        request: Request,
    ) -> ClientFuture<Result<NativeReceipt, SubmitError>> {
        Box::pin(async move { Remote::submit(&self, tag, request).await })
    }
    fn manage(
        self: Arc<Self>,
        intent: ManagementIntent,
    ) -> ClientFuture<Result<ManagementReply, ManagementError>> {
        Box::pin(async move {
            match intent {
                ManagementIntent::Stop { epoch } => {
                    Remote::stop(&self, epoch).await.map(ManagementReply::Stop)
                }
                ManagementIntent::Resume => Remote::resume(&self)
                    .await
                    .map(|()| ManagementReply::ReattachRequested),
                ManagementIntent::Restart => Remote::restart(&self)
                    .await
                    .map(|()| ManagementReply::ReattachRequested),
                ManagementIntent::Recheck => Remote::recheck(&self)
                    .await
                    .map(|()| ManagementReply::CredentialsRechecked),
            }
            .map_err(management_error)
        })
    }
    fn disconnect(&self) {
        Remote::disconnect(self)
    }
}
