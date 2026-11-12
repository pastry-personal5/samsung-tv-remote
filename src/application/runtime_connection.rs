//! Each blocking storage stage remains joined until it returns. Network waits
//! contain no hidden storage jobs, so cancelling them cannot release a store fence.
use super::*;
use crate::application::credential_store::PairingToken;
use crate::application::tv_control_coordinator::ConnectFlowError;
use crate::application::tv_session::{TvConnection, TvSessionError};
use crate::application::tv_setup_service::ReconnectMaterial;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ConnectionMode {
    Connect,
    Power,
    Wake,
}
#[derive(Clone, Copy)]
pub(super) struct ConnectionContext {
    pub target: DeviceId,
    pub generation: u64,
    pub mode: ConnectionMode,
    pub deadline: Instant,
}
pub(super) struct PreparedConnection {
    material: ReconnectMaterial,
    connection: Box<dyn TvConnection>,
    rotated: Option<PairingToken>,
}
pub(super) struct WakeState {
    plan: Option<WakePlan>,
    stage: WakeStage,
    deadline: Instant,
}
impl Owner {
    // A discarded worker must not leave the current projection Connecting, or
    // overwrite a newer generation/session which has already become Ready.
    pub(super) fn clear_connecting(
        &mut self,
        context: ConnectionContext,
        state: super::super::ConnectionState,
    ) {
        if self.coordinator.app_state.selected_device() == Some(context.target)
            && self.coordinator.app_state.selection_generation() == context.generation
            && self.coordinator.app_state.connection_state()
                == super::super::ConnectionState::Connecting
        {
            self.coordinator
                .app_state
                .set_connection_state(context.generation, state);
            self.coordinator
                .app_state
                .reset_playback_state(context.generation);
        }
    }

    pub(super) fn expire_wake(&mut self) {
        if self
            .wakes
            .values()
            .any(|w| w.stage == WakeStage::PacketSending && Instant::now() >= w.deadline)
        {
            self.cancel_wakes();
        }
    }

    fn connection_current(&self, id: OperationId, context: ConnectionContext) -> bool {
        self.coordinator.app_state.selected_device() == Some(context.target)
            && self.coordinator.app_state.selection_generation() == context.generation
            && (context.mode == ConnectionMode::Connect || self.wakes.contains_key(&id))
            && self.snapshot.startup == StartupState::Ready
            && self.storage.is_none()
    }
    pub(super) fn cancel_wakes(&mut self) {
        self.coordinator.cancel_wake();
        let pending: Vec<_> = self
            .wakes
            .drain()
            .map(|(id, wake)| {
                let stage = match wake.stage {
                    WakeStage::CheckingConnection => WakeStage::Cancelled,
                    WakeStage::PacketSending => WakeStage::CancelledDuringSend,
                    _ => WakeStage::CancelledAfterSend,
                };
                (id, stage)
            })
            .collect();
        for (id, stage) in pending {
            if let Some(context) = self.connections.get(&id).copied() {
                self.clear_connecting(context, super::super::ConnectionState::NotConnected);
            }
            self.finish(id, Outcome::Wake(stage));
        }
    }
    pub(super) fn admit_wake(&mut self, id: OperationId, power: bool) {
        if !self.wakes.is_empty()
            || self.workers.len() >= self.limits.effects
            || !self.connections.is_empty()
        {
            self.finish(id, Outcome::Rejected(Rejection::Busy));
            return;
        }
        if !self.coordinator.selected_wake_configured() {
            self.finish(id, Outcome::Rejected(Rejection::WakeNotConfigured));
            return;
        }
        let Some(plan) = self.coordinator.begin_wake() else {
            self.finish(id, Outcome::Rejected(Rejection::Blocked));
            return;
        };
        let context = ConnectionContext {
            target: plan.device,
            generation: plan.generation,
            deadline: Instant::now() + Duration::from_secs(if power { 2 } else { 30 }),
            mode: if power {
                ConnectionMode::Power
            } else {
                ConnectionMode::Wake
            },
        };
        // The plan captures host, MAC, and a revocable send permit *now*, before
        // the Power reachability check. Later Wake edits affect only new work.
        let stage = if power {
            WakeStage::CheckingConnection
        } else {
            WakeStage::PacketSending
        };
        self.coordinator.wake_stage = stage;
        self.wakes.insert(
            id,
            WakeState {
                plan: Some(plan),
                stage,
                deadline: Instant::now() + Duration::from_secs(10),
            },
        );
        if power {
            self.start_connection(id, context);
        } else {
            self.send_wake(id);
        }
    }
    fn send_wake(&mut self, id: OperationId) {
        let Some(wake) = self.wakes.get_mut(&id) else {
            return;
        };
        let Some(plan) = wake.plan.take() else { return };
        wake.stage = WakeStage::PacketSending;
        wake.deadline = Instant::now() + Duration::from_secs(10);
        self.coordinator.wake_stage = WakeStage::PacketSending;
        // A timeout must revoke the permit before a blocking DNS worker could
        // send. Keep the worker joined; the owner timer reports uncertainty.
        self.spawn(
            id,
            async move { Completion::WakeSent(id, plan.run().await) },
        );
        self.publish(Some(id));
    }
    pub(super) fn start_connection(&mut self, id: OperationId, context: ConnectionContext) {
        if self.storage.is_some()
            || self.workers.len() >= self.limits.effects
            || self.wakes.keys().any(|operation| *operation != id)
            || self
                .connections
                .values()
                .any(|c| c.generation == context.generation)
        {
            if let Some(wake) = self.wakes.remove(&id) {
                let stage = if wake.stage == WakeStage::Reconnecting {
                    WakeStage::FailedAfterSend(WakeFailure::Connection)
                } else {
                    WakeStage::Failed(WakeFailure::Connection)
                };
                self.coordinator.wake_stage = stage;
                self.finish(id, Outcome::Wake(stage));
            } else {
                self.finish(id, Outcome::Rejected(Rejection::Busy));
            }
            return;
        }
        let Some(setup) = self.coordinator.service.clone() else {
            self.finish(id, Outcome::Rejected(Rejection::Blocked));
            self.wakes.remove(&id);
            return;
        };
        self.connections.insert(id, context);
        self.storage = Some((
            id,
            std::cmp::min(
                Instant::now() + self.limits.storage_deadline,
                context.deadline,
            ),
        ));
        self.coordinator.stop_active();
        self.collect_remote_results();
        self.coordinator.app_state.set_connection_state(
            context.generation,
            super::super::ConnectionState::Connecting,
        );
        self.coordinator
            .app_state
            .reset_playback_state(context.generation);
        self.spawn(id, async move {
            let result =
                tokio::task::spawn_blocking(move || setup.reconnect_material(context.target))
                    .await
                    .unwrap_or(Err(
                        super::super::tv_setup_service::ReconnectError::Preferences(
                            RepositoryError::Unavailable,
                        ),
                    ));
            Completion::Material(id, context, result)
        });
        self.publish(Some(id));
    }
    fn connection_failed(
        &mut self,
        id: OperationId,
        context: ConnectionContext,
        error: ConnectFlowError,
    ) {
        self.connections.remove(&id);
        if context.mode == ConnectionMode::Power
            && self.connection_current(id, context)
            && matches!(
                error,
                ConnectFlowError::Session(TvSessionError::Offline | TvSessionError::Timeout)
            )
        {
            self.send_wake(id);
            return;
        }
        if self.coordinator.app_state.selection_generation() == context.generation {
            self.coordinator
                .app_state
                .set_connection_state(context.generation, super::super::ConnectionState::Failed);
        }
        if let Some(wake) = self.wakes.remove(&id) {
            let failure = match error {
                ConnectFlowError::Session(TvSessionError::Timeout) => WakeFailure::Timeout,
                ConnectFlowError::Session(
                    TvSessionError::TokenRejected
                    | TvSessionError::CertificateChanged
                    | TvSessionError::PairingDenied,
                )
                | ConnectFlowError::Reconnect(_) => WakeFailure::PairingRequired,
                _ => WakeFailure::Connection,
            };
            let stage = if wake.stage == WakeStage::Reconnecting {
                WakeStage::FailedAfterSend(failure)
            } else {
                WakeStage::Failed(failure)
            };
            self.coordinator.wake_stage = stage;
            self.finish(id, Outcome::Wake(stage));
        } else {
            self.finish(id, Outcome::ConnectFailed(error));
        }
        self.publish(None);
    }
    fn connection_ready(
        &mut self,
        id: OperationId,
        context: ConnectionContext,
        connection: Box<dyn TvConnection>,
    ) {
        self.connections.remove(&id);
        if !self.connection_current(id, context) || self.timed_out.remove(&id) {
            self.clear_connecting(context, super::super::ConnectionState::NotConnected);
            self.wakes.remove(&id);
            self.finish(id, Outcome::ConnectFailed(ConnectFlowError::Cancelled));
            self.publish(None);
            return;
        }
        self.coordinator
            .app_state
            .set_pairing_state(context.generation, super::super::PairingState::Ready);
        self.coordinator
            .app_state
            .set_connection_state(context.generation, super::super::ConnectionState::Ready);
        self.adopt_session(context.generation, connection);
        if self.wakes.remove(&id).is_some() {
            self.coordinator.wake_stage = WakeStage::Ready;
            if context.mode == ConnectionMode::Power {
                self.finish(id, Outcome::Connected);
            } else {
                self.finish(id, Outcome::Wake(WakeStage::Ready));
            }
        } else {
            self.finish(id, Outcome::Connected);
        }
    }
    pub(super) fn complete_connection(
        &mut self,
        completion: Completion,
        draining: bool,
    ) -> Option<Completion> {
        match completion {
            Completion::Material(id, context, result) => {
                self.storage = None;
                if draining || self.timed_out.remove(&id) || !self.connection_current(id, context) {
                    self.connections.remove(&id);
                    self.clear_connecting(context, super::super::ConnectionState::NotConnected);
                    if self.wakes.contains_key(&id) {
                        self.cancel_wakes();
                    } else {
                        self.finish(id, Outcome::ConnectFailed(ConnectFlowError::Cancelled));
                    }
                    self.publish(None);
                    return None;
                }
                match result {
                    Err(error) => {
                        self.connection_failed(id, context, ConnectFlowError::Reconnect(error))
                    }
                    Ok(material) => {
                        let Some(gateway) = self.coordinator.gateway.clone() else {
                            self.connection_failed(
                                id,
                                context,
                                ConnectFlowError::Session(TvSessionError::Offline),
                            );
                            return None;
                        };
                        self.spawn(id, async move {
                            let duration = match context.mode {
                                ConnectionMode::Power => 2,
                                ConnectionMode::Connect => 65,
                                ConnectionMode::Wake => 30,
                            };
                            let network = async {
                                loop {
                                    let result = async {
                                        let mut connection = gateway
                                            .connect(
                                                material.device.host.clone(),
                                                material.pin,
                                                PairingToken::from_stored(
                                                    material.token.as_str().to_owned(),
                                                ),
                                            )
                                            .await?;
                                        let rotated = connection.await_authorized(true).await?;
                                        Ok::<_, TvSessionError>((connection, rotated))
                                    }
                                    .await;
                                    match result {
                                        Ok((connection, rotated)) => {
                                            return Ok(PreparedConnection {
                                                material,
                                                connection,
                                                rotated,
                                            })
                                        }
                                        Err(
                                            TvSessionError::Offline
                                            | TvSessionError::Timeout
                                            | TvSessionError::RemoteChannel,
                                        ) if context.mode == ConnectionMode::Wake => {
                                            tokio::time::sleep(Duration::from_millis(500)).await
                                        }
                                        Err(error) => return Err(ConnectFlowError::Session(error)),
                                    }
                                }
                            };
                            let result = tokio::time::timeout(
                                std::cmp::min(
                                    Duration::from_secs(duration),
                                    context.deadline.saturating_duration_since(Instant::now()),
                                ),
                                network,
                            )
                            .await
                            .unwrap_or(Err(ConnectFlowError::Session(TvSessionError::Timeout)));
                            Completion::ConnectionPrepared(id, context, result)
                        });
                        self.publish(None);
                    }
                }
                None
            }
            Completion::ConnectionPrepared(id, context, result) => {
                if draining || !self.connection_current(id, context) {
                    self.connections.remove(&id);
                    self.clear_connecting(context, super::super::ConnectionState::NotConnected);
                    if self.wakes.contains_key(&id) {
                        self.cancel_wakes();
                    } else {
                        self.finish(id, Outcome::ConnectFailed(ConnectFlowError::Cancelled));
                    }
                    self.publish(None);
                    return None;
                }
                match result {
                    Err(error) => self.connection_failed(id, context, error),
                    Ok(prepared) => {
                        let PreparedConnection {
                            material,
                            connection,
                            rotated,
                        } = prepared;
                        match rotated {
                            None => self.connection_ready(id, context, connection),
                            Some(token) if self.storage.is_none() => {
                                let setup =
                                    self.coordinator.service.clone().expect("material port");
                                self.storage = Some((
                                    id,
                                    std::cmp::min(
                                        Instant::now() + self.limits.storage_deadline,
                                        context.deadline,
                                    ),
                                ));
                                self.spawn(id, async move {
                                    let (result, profile) = tokio::task::spawn_blocking(move || {
                                        let result = setup.commit_rotated_token(material, token);
                                        (result, read_profile(Some(setup)))
                                    }).await.unwrap_or((Err(super::super::tv_setup_service::SetupError::PartialCleanup), Err(RepositoryError::Unavailable)));
                                    Completion::RotationCommitted(id, context, connection, result, profile)
                                });
                                self.publish(None);
                            }
                            Some(_) => self.connection_failed(
                                id,
                                context,
                                ConnectFlowError::CredentialSave,
                            ),
                        }
                    }
                }
                None
            }
            Completion::RotationCommitted(id, context, connection, result, profile) => {
                self.storage = None;
                self.reconcile_setup(profile, draining);
                if draining || self.timed_out.remove(&id) {
                    self.connections.remove(&id);
                    self.wakes.remove(&id);
                    self.publish(None);
                } else if result.is_ok() {
                    self.connection_ready(id, context, connection);
                } else {
                    self.connection_failed(id, context, ConnectFlowError::CredentialSave);
                }
                None
            }
            Completion::WakeSent(id, result) => {
                let Some(wake) = self.wakes.get_mut(&id) else {
                    // Cancellation during send was already reported as uncertain;
                    // a late fact cannot turn it into a definitive not-sent result.
                    return None;
                };
                if draining {
                    return None;
                }
                match result {
                    Ok(()) => {
                        wake.stage = WakeStage::Reconnecting;
                        self.coordinator.wake_stage = WakeStage::Reconnecting;
                        let context = ConnectionContext {
                            target: self
                                .coordinator
                                .app_state
                                .selected_device()
                                .expect("wake target"),
                            generation: self.coordinator.app_state.selection_generation(),
                            mode: ConnectionMode::Wake,
                            deadline: Instant::now() + Duration::from_secs(30),
                        };
                        self.start_connection(id, context);
                    }
                    Err(error) => {
                        let stage = WakeStage::Failed(WakeFailure::Local(error));
                        self.wakes.remove(&id);
                        self.coordinator.wake_stage = stage;
                        self.finish(id, Outcome::Wake(stage));
                    }
                }
                None
            }
            other => Some(other),
        }
    }
}
