//! TV approval prepares secrets in memory. Only the owner authorizes commit
//! after rechecking the live client's observation and selection generation.
use super::*;
use crate::application::certificate_trust::CertificatePin;
use crate::application::control_service::Observation;
use crate::application::tv_address::TvHost;
use crate::application::tv_control_coordinator::PairFlowError;
use crate::application::tv_session::{TvConnection, TvSessionError};
use crate::application::tv_setup_service::{ApprovedPairing, SetupError};

pub(super) struct LiveObservation {
    pub client: ClientId,
    pub value: Observation,
    pub expires: Instant,
}
#[derive(Clone)]
pub(super) struct PairContext {
    pub client: ClientId,
    pub observation: OperationId,
    pub host: TvHost,
    pub pin: CertificatePin,
    pub label: String,
    pub replace: Option<DeviceId>,
    pub generation: u64,
    pub recovery: Option<DeviceId>,
}
pub(super) struct PreparedPair {
    approved: ApprovedPairing,
    connection: Box<dyn TvConnection>,
}
impl Owner {
    fn pair_fresh(&self, context: &PairContext) -> bool {
        self.coordinator.app_state.selection_generation() == context.generation
            && self
                .observations
                .get(&context.observation)
                .is_some_and(|observed| {
                    observed.client == context.client
                        && observed.expires > Instant::now()
                        && observed.value.host == context.host
                        && observed.value.observation.pin == context.pin
                })
            && match (self.snapshot.pairing_recovery, context.recovery) {
                (None, None) => self.snapshot.startup == StartupState::Ready,
                (Some(marker), Some(transaction)) => {
                    marker.repair
                        && marker.transaction == transaction
                        && Some(marker.target) == context.replace
                }
                _ => false,
            }
    }
    pub(super) fn prepare_pair(&mut self, client: ClientId, id: OperationId, context: PairContext) {
        debug_assert_eq!(client, context.client);
        if !self.pair_fresh(&context) {
            self.finish(id, Outcome::Rejected(Rejection::InvalidObservation));
            return;
        }
        if context.label.is_empty()
            || context.label.len() > 80
            || context.label.chars().any(char::is_control)
        {
            self.finish(id, Outcome::Rejected(Rejection::Oversize));
            return;
        }
        if context
            .replace
            .is_some_and(|target| !self.snapshot.devices.iter().any(|d| d.id == target))
        {
            self.finish(id, Outcome::Rejected(Rejection::MissingDevice));
            return;
        }
        if context.replace.is_none()
            && (self.snapshot.devices.len() >= self.limits.saved_devices
                || self.snapshot.devices.iter().any(|d| d.host == context.host))
        {
            self.finish(id, Outcome::Rejected(Rejection::InvalidConfirmation));
            return;
        }
        if !self.pairs.is_empty() || self.workers.len() >= self.limits.effects {
            self.finish(id, Outcome::Rejected(Rejection::Busy));
            return;
        }
        let Some(gateway) = self.coordinator.gateway.clone() else {
            self.finish(id, Outcome::Rejected(Rejection::Blocked));
            return;
        };
        let device = match context.replace {
            Some(target) => {
                let mut saved = self
                    .snapshot
                    .devices
                    .iter()
                    .find(|d| d.id == target)
                    .expect("validated target")
                    .clone();
                saved.label = context.label.clone();
                saved.host = context.host.clone();
                saved
            }
            None => SavedDevice {
                id: DeviceId::generate(),
                label: context.label.clone(),
                host: context.host.clone(),
                wake: Default::default(),
            },
        };
        self.pairs.insert(id, context.clone());
        let recovery = self.snapshot.pairing_recovery;
        self.spawn(id, async move {
            let prepare = async {
                let mut connection = gateway
                    .connect(context.host.clone(), context.pin, None)
                    .await
                    .map_err(PairFlowError::Session)?;
                let token = connection
                    .await_authorized(false)
                    .await
                    .map_err(PairFlowError::Session)?
                    .ok_or(PairFlowError::Session(TvSessionError::PairingTokenMissing))?;
                Ok(PreparedPair {
                    approved: ApprovedPairing {
                        device,
                        token,
                        pin: context.pin,
                        repair: context.replace.is_some(),
                        recovery,
                    },
                    connection,
                })
            };
            let result = tokio::time::timeout(Duration::from_secs(80), prepare)
                .await
                .unwrap_or(Err(PairFlowError::Session(TvSessionError::Timeout)));
            Completion::PreparedPair(id, context, result)
        });
        self.publish(Some(id));
    }
    pub(super) fn recover_pair(
        &mut self,
        id: OperationId,
        target: DeviceId,
        transaction: DeviceId,
        confirm: DeviceId,
    ) {
        let marker = self
            .snapshot
            .pairing_recovery
            .filter(|m| m.target == target && m.transaction == transaction && !m.repair);
        if confirm != target {
            self.finish(id, Outcome::Rejected(Rejection::InvalidConfirmation));
            return;
        }
        let (Some(marker), Some(setup)) = (marker, self.coordinator.service.clone()) else {
            self.finish(id, Outcome::Rejected(Rejection::Blocked));
            return;
        };
        if !self.workers.is_empty() {
            self.finish(id, Outcome::Rejected(Rejection::Busy));
            return;
        }
        self.storage = Some((id, Instant::now() + self.limits.storage_deadline));
        self.spawn(id, async move {
            let (result, profile) = tokio::task::spawn_blocking(move || {
                let result = setup.recover_first_pair(marker);
                (result, read_profile(Some(setup)))
            })
            .await
            .unwrap_or((
                Err(RepositoryError::Unavailable),
                Err(RepositoryError::Unavailable),
            ));
            Completion::PairRecovered(id, result, profile)
        });
        self.publish(Some(id));
    }
    /// Apply actual durable facts even after timeout; never replace the original
    /// terminal result. A journal or unreadable store keeps normal work blocked.
    pub(super) fn reconcile_setup(&mut self, profile: Restored, draining: bool) {
        match profile {
            Ok(profile) if profile_fits(&profile.devices, self.limits.saved_devices) => {
                let selected = profile.selected.as_ref().map(|d| d.id);
                if self.coordinator.app_state.selected_device() != selected {
                    match profile.selected {
                        Some(device) => self
                            .coordinator
                            .app_state
                            .restart_selection(device.display()),
                        None => {
                            self.coordinator.app_state.clear_selection();
                        }
                    }
                }
                let revisions = Arc::make_mut(&mut self.snapshot.configuration_revisions);
                revisions.retain(|(id, _)| profile.devices.iter().any(|d| d.id == *id));
                for device in &profile.devices {
                    if !revisions.iter().any(|(id, _)| *id == device.id) {
                        revisions.push((device.id, 0));
                    }
                }
                self.coordinator.saved_devices = profile.devices;
                self.snapshot.recovery = profile.recovery;
                self.snapshot.pairing_recovery = profile.pairing;
                if !draining {
                    self.snapshot.startup =
                        if profile.recovery.is_some() || profile.pairing.is_some() {
                            StartupState::RecoveryRequired
                        } else {
                            StartupState::Ready
                        };
                }
            }
            other if !draining => {
                self.snapshot.startup =
                    StartupState::Blocked(other.err().unwrap_or(RepositoryError::Corrupt))
            }
            _ => {}
        }
    }
    pub(super) fn complete_setup(
        &mut self,
        completion: Completion,
        draining: bool,
    ) -> Option<Completion> {
        match completion {
            Completion::PreparedPair(id, context, result) => {
                match result {
                    Err(error) => {
                        self.pairs.remove(&id);
                        self.finish(id, Outcome::PairFailed(error));
                    }
                    Ok(prepared)
                        if draining || !self.pair_fresh(&context) || self.storage.is_some() =>
                    {
                        self.pairs.remove(&id);
                        self.finish(id, Outcome::PairFailed(PairFlowError::Cancelled));
                        drop(prepared);
                    }
                    Ok(prepared) => {
                        let Some(setup) = self.coordinator.service.clone() else {
                            self.pairs.remove(&id);
                            self.finish(id, Outcome::Rejected(Rejection::Blocked));
                            return None;
                        };
                        self.storage = Some((id, Instant::now() + self.limits.storage_deadline));
                        self.cancel_wakes();
                        self.coordinator.stop_active();
                        self.collect_remote_results();
                        self.observations.remove(&context.observation);
                        self.spawn(id, async move {
                            let PreparedPair {
                                approved,
                                connection,
                            } = prepared;
                            let (result, profile) = tokio::task::spawn_blocking(move || {
                                let result = setup.commit_approved_pairing(approved);
                                (result, read_profile(Some(setup)))
                            })
                            .await
                            .unwrap_or((
                                Err(SetupError::PartialCleanup),
                                Err(RepositoryError::Unavailable),
                            ));
                            Completion::PairCommitted(
                                id,
                                context,
                                Some(connection),
                                result,
                                profile,
                            )
                        });
                    }
                }
                self.publish(None);
                None
            }
            Completion::PairCommitted(id, context, connection, result, profile) => {
                self.storage = None;
                self.pairs.remove(&id);
                let late = self.timed_out.remove(&id) || draining;
                self.reconcile_setup(profile, draining);
                if let Ok(device) = &result {
                    if context.replace.is_some() {
                        if self.coordinator.app_state.selected_device() == Some(device.id) {
                            self.coordinator
                                .app_state
                                .restart_selection(device.display());
                        }
                        if let Some((_, revision)) =
                            Arc::make_mut(&mut self.snapshot.configuration_revisions)
                                .iter_mut()
                                .find(|(id, _)| *id == device.id)
                        {
                            *revision += 1;
                        }
                    }
                    if !late
                        && self.snapshot.startup == StartupState::Ready
                        && self.coordinator.app_state.selected_device() == Some(device.id)
                    {
                        let generation = self.coordinator.app_state.selection_generation();
                        self.coordinator
                            .app_state
                            .set_pairing_state(generation, super::super::PairingState::Ready);
                        self.coordinator
                            .app_state
                            .set_connection_state(generation, super::super::ConnectionState::Ready);
                        if let Some(connection) = connection {
                            self.adopt_session(generation, connection);
                        }
                    }
                }
                self.finish(
                    id,
                    match result {
                        Ok(device) => Outcome::Paired(device),
                        Err(error) => Outcome::PairFailed(PairFlowError::Setup(error)),
                    },
                );
                self.publish(None);
                None
            }
            Completion::PairRecovered(id, result, profile) => {
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
                None
            }
            other => Some(other),
        }
    }
}
