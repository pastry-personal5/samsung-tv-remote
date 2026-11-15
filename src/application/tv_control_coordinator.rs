//! Internal remote/session policy used exclusively by the headless owner.
use super::control_state::{ControlState, PlaybackState};
use super::device_repository::SavedDevice;
use super::remote_dispatcher::{
    Admission, NotSentReason, RemoteDispatcher, RequestId, TerminalOutcome, TerminalResult,
};
use super::remote_request::SendRemoteAction;
use super::tv_address::TvHost;
use super::tv_discovery::TvDiscovery;
use super::tv_session::{
    TvConnection, TvGateway, TvSessionControl, TvSessionError, TvSessionEvent,
};
use super::tv_setup_service::{ReconnectError, SetupError, TvSetupPort};
use super::wake_transport::{WakePermit, WakeSendError, WakeTransport};
use crate::domain::{DeviceId, RemoteAction};
use futures_util::stream::BoxStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
#[derive(Clone)]
pub struct AppServices {
    pub setup: Option<Arc<dyn TvSetupPort>>,
    pub discovery: Option<Arc<dyn TvDiscovery>>,
    pub gateway: Option<Arc<dyn TvGateway>>,
    pub wake: Option<Arc<dyn WakeTransport>>,
}

impl AppServices {
    pub fn new(
        setup: Option<Arc<dyn TvSetupPort>>,
        discovery: Arc<dyn TvDiscovery>,
        gateway: Arc<dyn TvGateway>,
        wake: Arc<dyn WakeTransport>,
    ) -> Self {
        Self {
            setup,
            discovery: Some(discovery),
            gateway: Some(gateway),
            wake: Some(wake),
        }
    }

    #[cfg(test)]
    pub fn without_adapters() -> Self {
        Self {
            setup: None,
            discovery: None,
            gateway: None,
            wake: None,
        }
    }
}

pub(super) struct ActiveRuntime {
    pub generation: u64,
    pub session_id: u64,
    pub control: Box<dyn TvSessionControl>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairFlowError {
    Session(TvSessionError),
    Setup(SetupError),
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectFlowError {
    Session(TvSessionError),
    Reconnect(ReconnectError),
    CredentialSave,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeStage {
    ConfigurationRequired,
    Idle,
    CheckingConnection,
    PacketSending,
    Reconnecting,
    /// Disconnected Power found a ready connection before sending any packet.
    Connected,
    Ready,
    Failed(WakeFailure),
    FailedAfterSend(WakeFailure),
    Cancelled,
    CancelledDuringSend,
    CancelledAfterSend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeFailure {
    Local(WakeSendError),
    Timeout,
    PairingRequired,
    Connection,
}

pub(super) struct WakePlan {
    pub generation: u64,
    pub device: DeviceId,
    host: TvHost,
    mac: crate::domain::MacAddress,
    transport: Arc<dyn WakeTransport>,
    permit: WakePermit,
}

impl WakePlan {
    pub(super) async fn run(self) -> Result<(), WakeSendError> {
        if !self.permit.is_current() {
            return Err(WakeSendError::Cancelled);
        }
        self.transport
            .send_once(self.host, self.mac, self.permit)
            .await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionImpact {
    Ignored,
    RemoteResult,
    Disconnected,
    TokenRejected,
}

pub(super) struct TvControlCoordinator {
    pub app_state: ControlState,
    pub service: Option<Arc<dyn TvSetupPort>>,
    pub discovery: Option<Arc<dyn TvDiscovery>>,
    pub gateway: Option<Arc<dyn TvGateway>>,
    pub wake_transport: Option<Arc<dyn WakeTransport>>,
    pub wake_stage: WakeStage,
    wake_attempt: u64,
    wake_epoch: Arc<AtomicU64>,
    wake_send_guard: Arc<Mutex<()>>,
    pub saved_devices: Vec<SavedDevice>,
    pub dispatcher: RemoteDispatcher,
    active_runtime: Option<ActiveRuntime>,
    next_session_id: u64,
}
impl TvControlCoordinator {
    pub(super) fn new(services: AppServices) -> Self {
        Self {
            app_state: ControlState::none(),
            service: services.setup,
            discovery: services.discovery,
            gateway: services.gateway,
            wake_transport: services.wake,
            wake_stage: WakeStage::ConfigurationRequired,
            wake_attempt: 0,
            wake_epoch: Arc::new(AtomicU64::new(0)),
            wake_send_guard: Arc::new(Mutex::new(())),
            saved_devices: Vec::new(),
            dispatcher: RemoteDispatcher::new(32, 33),
            active_runtime: None,
            next_session_id: 0,
        }
    }
    pub(super) fn cancel_wake(&mut self) {
        let _guard = self
            .wake_send_guard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.wake_attempt = self.wake_attempt.saturating_add(1);
        self.wake_epoch.store(self.wake_attempt, Ordering::SeqCst);
        self.wake_stage = match self.wake_stage {
            WakeStage::CheckingConnection => WakeStage::Cancelled,
            WakeStage::PacketSending => WakeStage::CancelledDuringSend,
            WakeStage::Reconnecting => WakeStage::CancelledAfterSend,
            stage => stage,
        };
    }
    pub(super) fn selected_wake_configured(&self) -> bool {
        self.app_state
            .selected_device()
            .and_then(|id| self.saved_devices.iter().find(|device| device.id == id))
            .is_some_and(|device| device.wake.active_mac().is_some())
    }

    pub(super) fn begin_wake(&mut self) -> Option<WakePlan> {
        if matches!(
            self.wake_stage,
            WakeStage::CheckingConnection | WakeStage::PacketSending | WakeStage::Reconnecting
        ) || self.app_state.connection_state() == super::ConnectionState::Ready
        {
            return None;
        }
        let device = self.app_state.selected_device()?;
        let saved = self.saved_devices.iter().find(|item| item.id == device)?;
        let mac = saved.wake.active_mac()?;
        let host = saved.host.clone();
        let transport = self.wake_transport.clone()?;
        if self.service.is_none() || self.gateway.is_none() {
            return None;
        }
        self.cancel_wake();
        self.app_state
            .reset_playback_state(self.app_state.selection_generation());
        self.wake_stage = WakeStage::PacketSending;
        Some(WakePlan {
            generation: self.app_state.selection_generation(),
            device,
            host,
            mac,
            transport,
            permit: WakePermit {
                epoch: self.wake_epoch.clone(),
                attempt: self.wake_attempt,
                send_guard: self.wake_send_guard.clone(),
            },
        })
    }

    pub(super) fn stop_active(&mut self) {
        if let Some(mut active) = self.active_runtime.take() {
            let _ =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| active.control.abort()));
        }
        self.dispatcher.abort_in_flight();
        self.dispatcher.cancel_waiting();
        let generation = self.app_state.selection_generation();
        self.app_state.reset_playback_state(generation);
        self.app_state
            .set_connection_state(generation, super::ConnectionState::NotConnected);
    }

    pub(super) fn install_session(
        &mut self,
        generation: u64,
        connection: Box<dyn TvConnection>,
    ) -> (u64, BoxStream<'static, super::tv_session::TvSessionEvent>) {
        let active = connection.start();
        self.next_session_id = self.next_session_id.saturating_add(1);
        let session_id = self.next_session_id;
        // The caller schedules only the event stream. The session control
        // remains owned by the application coordinator.
        let super::tv_session::ActiveTvSession { control, events } = active;
        self.active_runtime = Some(ActiveRuntime {
            generation,
            session_id,
            control,
        });
        (session_id, events)
    }

    #[cfg(feature = "p3-spike")]
    pub(super) fn take_diagnostic(&mut self) -> Option<super::diagnostic::DiagnosticPort> {
        self.active_runtime.as_mut()?.control.take_diagnostic()
    }

    pub(super) fn active_session_matches(&self, generation: u64, session_id: u64) -> bool {
        self.active_runtime.as_ref().is_some_and(|active| {
            active.generation == generation && active.session_id == session_id
        })
    }

    pub(super) fn evidence_session(&self) -> Option<(crate::domain::DeviceId, u64, u64)> {
        let active = self.active_runtime.as_ref()?;
        let device = self.app_state.selected_device()?;
        (active.generation == self.app_state.selection_generation()
            && self.app_state.connection_state() == super::ConnectionState::Ready
            && self.app_state.pairing_state() == super::PairingState::Ready)
            .then_some((device, active.generation, active.session_id))
    }

    pub(super) fn try_click(&self, id: RequestId, action: crate::domain::RemoteAction) -> bool {
        self.active_runtime
            .as_ref()
            .filter(|active| active.generation == self.app_state.selection_generation())
            .is_some_and(|active| active.control.try_click(id, action))
    }

    pub(super) fn admit_remote(&mut self, request: SendRemoteAction) -> Admission {
        self.dispatcher.admit(&self.app_state, request)
    }

    pub(super) fn pump_dispatch(&mut self) {
        let Some(mut item) = self.dispatcher.start_next(&self.app_state) else {
            return;
        };
        if item.request.action() == RemoteAction::PlayPause {
            let action = match self.app_state.playback_state() {
                PlaybackState::Unknown | PlaybackState::AssumedPaused => RemoteAction::Play,
                PlaybackState::AssumedPlaying => RemoteAction::Pause,
            };
            assert!(self.dispatcher.resolve_in_flight_action(item.id, action));
            item.request = SendRemoteAction::new(
                item.request.target(),
                item.request.selection_generation(),
                action,
            );
        }
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.try_click(item.id, item.request.action())
        })) {
            Ok(true) => {}
            Ok(false) => {
                self.dispatcher.finish(
                    item.id,
                    TerminalOutcome::NotSent(NotSentReason::TransportUnavailable),
                );
            }
            Err(_) => {
                // A callback can panic after accepting the write. Preserve
                // uncertainty instead of claiming it was never handed off.
                self.stop_active();
                self.app_state.set_connection_state(
                    self.app_state.selection_generation(),
                    super::ConnectionState::Failed,
                );
            }
        }
    }

    pub(super) fn terminal_results_snapshot(&self) -> Vec<TerminalResult> {
        self.dispatcher.terminal_results().iter().cloned().collect()
    }

    pub(super) fn acknowledge_terminal_through(&mut self, id: RequestId) {
        self.dispatcher.acknowledge_through(id);
    }

    pub(super) fn process_session_event(
        &mut self,
        generation: u64,
        session_id: u64,
        event: TvSessionEvent,
    ) -> SessionImpact {
        if !self.active_session_matches(generation, session_id) {
            return SessionImpact::Ignored;
        }
        match event {
            TvSessionEvent::ImeStarted | TvSessionEvent::ImeEnded => SessionImpact::Ignored,
            TvSessionEvent::Written(id)
            | TvSessionEvent::Uncertain(id)
            | TvSessionEvent::NotSent(id) => {
                let outcome = match event {
                    TvSessionEvent::Written(_) => TerminalOutcome::Written,
                    TvSessionEvent::Uncertain(_) => TerminalOutcome::Uncertain,
                    _ => TerminalOutcome::NotSent(NotSentReason::TransportUnavailable),
                };
                let action = self.dispatcher.in_flight_action(id);
                if self.dispatcher.finish(id, outcome) {
                    if matches!(event, TvSessionEvent::Written(_)) {
                        if let Some(action) = action {
                            self.app_state.record_playback_write(generation, action);
                        }
                    }
                    if matches!(event, TvSessionEvent::Uncertain(_)) {
                        self.app_state.reset_playback_state(generation);
                    }
                    SessionImpact::RemoteResult
                } else {
                    SessionImpact::Ignored
                }
            }
            TvSessionEvent::Disconnected | TvSessionEvent::TokenRejected => {
                self.active_runtime = None;
                self.app_state.reset_playback_state(generation);
                self.dispatcher.abort_in_flight();
                self.dispatcher.cancel_waiting();
                self.app_state.set_connection_state(
                    generation,
                    super::control_state::ConnectionState::Failed,
                );
                self.wake_stage = if self.selected_wake_configured() {
                    WakeStage::Idle
                } else {
                    WakeStage::ConfigurationRequired
                };
                if matches!(event, TvSessionEvent::TokenRejected) {
                    self.app_state
                        .set_pairing_state(generation, super::control_state::PairingState::Failed);
                    SessionImpact::TokenRejected
                } else {
                    SessionImpact::Disconnected
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::control_state::{ConnectionState, PairingState, PlaybackState};
    use crate::domain::{DeviceDisplay, RemoteAction};

    #[derive(Clone)]
    struct FakeControl {
        writes: Arc<Mutex<Vec<(RequestId, RemoteAction)>>>,
    }

    impl TvSessionControl for FakeControl {
        fn try_click(&self, id: RequestId, action: RemoteAction) -> bool {
            self.writes.lock().unwrap().push((id, action));
            true
        }

        fn abort(&mut self) {}
    }

    fn saved_device(id: DeviceId) -> SavedDevice {
        SavedDevice {
            sources: Default::default(),
            id,
            label: "TV".to_owned(),
            host: TvHost::parse("tv.local").unwrap(),
            wake: super::super::wake::WakeConfiguration::default(),
        }
    }

    fn connected_coordinator() -> (TvControlCoordinator, SavedDevice) {
        let mut coordinator = TvControlCoordinator::new(AppServices::without_adapters());
        let device = saved_device(DeviceId::new(7));
        coordinator
            .app_state
            .select_device(DeviceDisplay::new(device.id, device.label.clone()));
        let generation = coordinator.app_state.selection_generation();
        coordinator
            .app_state
            .set_pairing_state(generation, PairingState::Ready);
        coordinator
            .app_state
            .set_connection_state(generation, ConnectionState::Ready);
        (coordinator, device)
    }

    #[test]
    fn stale_session_result_cannot_finish_the_active_request() {
        let (mut coordinator, device) = connected_coordinator();
        let generation = coordinator.app_state.selection_generation();
        let writes = Arc::new(Mutex::new(Vec::new()));
        coordinator.active_runtime = Some(ActiveRuntime {
            generation,
            session_id: 2,
            control: Box::new(FakeControl {
                writes: Arc::clone(&writes),
            }),
        });
        let request = SendRemoteAction::new(device.id, generation, RemoteAction::Up);
        let Admission::Queued(id) = coordinator.admit_remote(request) else {
            panic!("request was not queued");
        };
        coordinator.pump_dispatch();
        assert_eq!(*writes.lock().unwrap(), vec![(id, RemoteAction::Up)]);
        assert_eq!(
            coordinator.process_session_event(generation, 1, TvSessionEvent::Written(id)),
            SessionImpact::Ignored
        );
        assert!(coordinator.terminal_results_snapshot().is_empty());
        assert_eq!(
            coordinator.process_session_event(generation, 2, TvSessionEvent::Written(id)),
            SessionImpact::RemoteResult
        );
        let results = coordinator.terminal_results_snapshot();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].outcome, TerminalOutcome::Written);
        coordinator.acknowledge_terminal_through(results[0].id);
        assert!(coordinator.terminal_results_snapshot().is_empty());
        assert_eq!(
            coordinator.process_session_event(generation, 2, TvSessionEvent::Written(id)),
            SessionImpact::Ignored
        );
    }

    #[test]
    fn unknown_playback_sends_play_then_pause_after_a_confirmed_play_write() {
        let (mut coordinator, device) = connected_coordinator();
        let generation = coordinator.app_state.selection_generation();
        let writes = Arc::new(Mutex::new(Vec::new()));
        coordinator.active_runtime = Some(ActiveRuntime {
            generation,
            session_id: 4,
            control: Box::new(FakeControl {
                writes: Arc::clone(&writes),
            }),
        });
        assert_eq!(
            coordinator.app_state.playback_state(),
            PlaybackState::Unknown
        );

        for _ in 0..2 {
            assert!(matches!(
                coordinator.admit_remote(SendRemoteAction::new(
                    device.id,
                    generation,
                    RemoteAction::PlayPause,
                )),
                Admission::Queued(_)
            ));
        }

        coordinator.pump_dispatch();
        let first = writes.lock().unwrap()[0];
        assert_eq!(first.1, RemoteAction::Play);
        assert_eq!(
            coordinator.process_session_event(generation, 4, TvSessionEvent::Written(first.0)),
            SessionImpact::RemoteResult
        );
        assert_eq!(
            coordinator.app_state.playback_state(),
            PlaybackState::AssumedPlaying
        );

        coordinator.pump_dispatch();
        let second = writes.lock().unwrap()[1];
        assert_eq!(second.1, RemoteAction::Pause);
        assert_eq!(
            coordinator.process_session_event(generation, 4, TvSessionEvent::Written(second.0)),
            SessionImpact::RemoteResult
        );
        assert_eq!(
            coordinator.app_state.playback_state(),
            PlaybackState::AssumedPaused
        );
    }

    #[test]
    fn uncertain_playback_write_keeps_state_unknown_and_retries_play() {
        let (mut coordinator, device) = connected_coordinator();
        let generation = coordinator.app_state.selection_generation();
        let writes = Arc::new(Mutex::new(Vec::new()));
        coordinator.active_runtime = Some(ActiveRuntime {
            generation,
            session_id: 5,
            control: Box::new(FakeControl {
                writes: Arc::clone(&writes),
            }),
        });

        for _ in 0..2 {
            assert!(matches!(
                coordinator.admit_remote(SendRemoteAction::new(
                    device.id,
                    generation,
                    RemoteAction::PlayPause,
                )),
                Admission::Queued(_)
            ));
        }

        coordinator.pump_dispatch();
        let first = writes.lock().unwrap()[0];
        assert_eq!(first.1, RemoteAction::Play);
        assert_eq!(
            coordinator.process_session_event(generation, 5, TvSessionEvent::Uncertain(first.0)),
            SessionImpact::RemoteResult
        );
        assert_eq!(
            coordinator.app_state.playback_state(),
            PlaybackState::Unknown
        );

        coordinator.pump_dispatch();
        assert_eq!(writes.lock().unwrap()[1].1, RemoteAction::Play);
    }

    #[test]
    fn token_rejection_marks_active_write_uncertain_and_cancels_waiting_work() {
        let (mut coordinator, device) = connected_coordinator();
        let generation = coordinator.app_state.selection_generation();
        coordinator.active_runtime = Some(ActiveRuntime {
            generation,
            session_id: 3,
            control: Box::new(FakeControl {
                writes: Arc::new(Mutex::new(Vec::new())),
            }),
        });
        for action in [RemoteAction::Up, RemoteAction::Down] {
            assert!(matches!(
                coordinator.admit_remote(SendRemoteAction::new(device.id, generation, action)),
                Admission::Queued(_)
            ));
        }
        coordinator.pump_dispatch();
        assert_eq!(
            coordinator.process_session_event(generation, 2, TvSessionEvent::TokenRejected),
            SessionImpact::Ignored
        );
        assert_eq!(
            coordinator.process_session_event(generation, 3, TvSessionEvent::TokenRejected),
            SessionImpact::TokenRejected
        );
        let results = coordinator.terminal_results_snapshot();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].outcome, TerminalOutcome::Uncertain);
        assert_eq!(
            results[1].outcome,
            TerminalOutcome::NotSent(NotSentReason::Cancelled)
        );
        assert_eq!(
            coordinator.app_state.connection_state(),
            ConnectionState::Failed
        );
        assert_eq!(coordinator.app_state.pairing_state(), PairingState::Failed);
    }
}
