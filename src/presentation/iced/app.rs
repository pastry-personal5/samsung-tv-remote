use super::ui_message::{Message, SettingsPage, Shortcut};
use super::view_model::{MessageSeverity, MessageSource, PrimaryView, ViewModel};
use super::{in_process::InProcess, view};
use crate::application::control_service::*;
use crate::application::remote_dispatcher::TerminalOutcome;
use crate::application::tv_address::TvHost;
use crate::application::tv_control_coordinator::{
    AppServices, ConnectFlowError, PairFlowError, WakeStage,
};
use crate::application::tv_session::TvSessionError;
use crate::application::wake::{WakeConfiguration, WakeInterface};
use crate::application::{ControlState, SendRemoteAction};
use crate::domain::DeviceId;
use ::iced::keyboard::{self, key, Key, Modifiers};
use ::iced::{event, window, Element, Event, Size, Subscription, Task, Theme};
use futures_util::SinkExt;
use std::collections::HashMap;
use std::sync::Arc;

pub struct App {
    main_window: Option<window::Id>,
    settings_window: Option<window::Id>,
    settings_page: SettingsPage,
    view_model: ViewModel,
    adapter: Option<Arc<InProcess>>,
    snapshot: Option<Snapshot>,
    candidates: Vec<TvHost>,
    candidate_handles: Vec<Candidate>,
    observed: Option<Observation>,
    tv_address: String,
    settings_status: String,
    wake_wired: String,
    wake_wifi: String,
    wake_active: Option<WakeInterface>,
    wake_dirty: bool,
    wake_save_requested: bool,
    wake_tick: bool,
    launch_connected: bool,
    selection_connect_requested: bool,
    sequence: u64,
    submitting: HashMap<u64, Command>,
    pending: HashMap<OperationId, Command>,
}
impl App {
    fn new() -> (Self, Task<Message>) {
        (
            Self {
                main_window: None,
                settings_window: None,
                settings_page: SettingsPage::Discovery,
                view_model: ViewModel::new(&ControlState::none()),
                adapter: None,
                snapshot: None,
                candidates: Vec::new(),
                candidate_handles: Vec::new(),
                observed: None,
                tv_address: String::new(),
                settings_status: "Enter a local TV address to begin secure setup.".into(),
                wake_wired: String::new(),
                wake_wifi: String::new(),
                wake_active: None,
                wake_dirty: false,
                wake_save_requested: false,
                wake_tick: false,
                launch_connected: false,
                selection_connect_requested: false,
                sequence: 0,
                submitting: HashMap::new(),
                pending: HashMap::new(),
            },
            Task::done(Message::OpenMainWindow),
        )
    }
    fn boot(services: AppServices) -> (Self, Task<Message>) {
        let (app, open) = Self::new();
        (
            app,
            Task::batch([
                open,
                Task::perform(
                    async move { Arc::new(InProcess::start(services)) },
                    Message::RuntimeStarted,
                ),
            ]),
        )
    }
    fn command(&mut self, command: Command) -> Task<Message> {
        let (Some(adapter), Some(snapshot)) = (&self.adapter, &self.snapshot) else {
            return self.publish(
                MessageSeverity::Warning,
                MessageSource::MainWindow,
                "TV service is still starting. Try again shortly.",
            );
        };
        if self.pending.len() + self.submitting.len() >= 64 {
            return self.publish(
                MessageSeverity::Warning,
                MessageSource::MainWindow,
                "TV service is busy. Wait for pending work to finish.",
            );
        }
        self.sequence += 1;
        let sequence = self.sequence;
        let request = Request {
            epoch: snapshot.epoch,
            key: format!("gui-{sequence}"),
            command: command.clone(),
        };
        self.submitting.insert(sequence, command);
        let service = adapter.service.clone();
        Task::perform(
            async move { service.submit(request).await },
            move |result| Message::Admitted(sequence, result),
        )
    }
    fn selected(&self) -> Option<(DeviceId, u64)> {
        let snapshot = self.snapshot.as_ref()?;
        Some((
            snapshot.control.selected_device()?,
            snapshot.control.selection_generation(),
        ))
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::RuntimeStarted(adapter) => {
                self.adapter = Some(adapter.clone());
                // A single observer serves every pending GUI request. Overflow
                // attaches atomically again; retained outcomes are queried below.
                Task::run(
                    ::iced::stream::channel(16, async move |mut output| loop {
                        let Ok(mut subscription) = adapter.service.subscribe().await else {
                            let _ = output.send(Message::ServiceUnavailable).await;
                            break;
                        };
                        if output
                            .send(Message::ServiceResnapshot(subscription.snapshot))
                            .await
                            .is_err()
                        {
                            break;
                        }
                        while let Some(event) = subscription.events.recv().await {
                            let epoch = event.snapshot.epoch;
                            if output
                                .send(Message::ServiceSnapshot(event.snapshot))
                                .await
                                .is_err()
                            {
                                return;
                            }
                            if let Some(id) = event.operation {
                                if let Ok(Lookup::Known(receipt)) =
                                    adapter.service.operation(epoch, id).await
                                {
                                    if receipt.outcome.is_some()
                                        && output
                                            .send(Message::ServiceResult(receipt))
                                            .await
                                            .is_err()
                                    {
                                        return;
                                    }
                                }
                            }
                        }
                    }),
                    |message| message,
                )
            }
            Message::ServiceResnapshot(snapshot) => {
                let mut tasks = vec![self.apply_snapshot(snapshot)];
                for id in self.pending.keys().copied() {
                    tasks.push(self.lookup(id));
                }
                Task::batch(tasks)
            }
            Message::ServiceSnapshot(snapshot) => self.apply_snapshot(snapshot),
            Message::Admitted(sequence, result) => {
                let Some(command) = self.submitting.remove(&sequence) else {
                    return Task::none();
                };
                match result {
                    Ok(receipt) => {
                        self.pending.insert(receipt.id, command);
                        if receipt.outcome.is_some() {
                            self.complete(receipt)
                        } else {
                            self.lookup(receipt.id)
                        }
                    }
                    Err(_) => self.publish(
                        MessageSeverity::Warning,
                        MessageSource::MainWindow,
                        "TV request was not admitted. Check the current selection and try again.",
                    ),
                }
            }
            Message::ServiceResult(receipt) => self.complete(receipt),
            Message::ServiceUnavailable => self.publish(
                MessageSeverity::Warning,
                MessageSource::MainWindow,
                "TV service is unavailable. Restart the application.",
            ),
            Message::OpenMainWindow if self.main_window.is_none() => {
                let (id, open) = window::open(main_window_settings());
                self.main_window = Some(id);
                open.map(Message::MainWindowOpened)
            }
            Message::OpenMainWindow => Task::none(),
            Message::OpenSettings if self.settings_window.is_none() => {
                let (id, open) = window::open(settings_window_settings());
                self.settings_window = Some(id);
                open.map(Message::SettingsWindowOpened)
            }
            Message::OpenSettings
            | Message::MainWindowOpened(_)
            | Message::SettingsWindowOpened(_) => Task::none(),
            Message::WindowClosed(id) if self.main_window == Some(id) => {
                if let Some(adapter) = self.adapter.clone() {
                    Task::perform(async move { adapter.drain().await }, |_| Message::Drained)
                } else {
                    ::iced::exit()
                }
            }
            Message::Drained => ::iced::exit(),
            Message::WindowClosed(id) if self.settings_window == Some(id) => {
                self.settings_window = None;
                Task::none()
            }
            Message::WindowClosed(_) => Task::none(),
            Message::Navigate(route) => {
                self.view_model.select_view(route);
                Task::none()
            }
            Message::SelectSettingsPage(page) => {
                self.settings_page = page;
                Task::none()
            }
            Message::ResizeMessages(height) => {
                self.view_model.resize_message_pane(height);
                Task::none()
            }
            Message::FeedScrolled { at_bottom } => {
                self.view_model.messages_mut().set_at_bottom(at_bottom);
                Task::none()
            }
            Message::Shortcut { window, shortcut } => match shortcut {
                Shortcut::Navigate(route) if self.main_window == Some(window) => {
                    self.update(Message::Navigate(route))
                }
                Shortcut::OpenSettings => self.update(Message::OpenSettings),
                Shortcut::Remote(action)
                    if self.main_window == Some(window)
                        && self.view_model.primary_view() == PrimaryView::Remote =>
                {
                    if let Some((target, generation)) = self.selected() {
                        self.update(Message::AttemptRemoteAction(SendRemoteAction::new(
                            target, generation, action,
                        )))
                    } else {
                        self.publish(
                            MessageSeverity::Warning,
                            MessageSource::MainWindow,
                            "No TV selected. Open Settings to choose a TV.",
                        )
                    }
                }
                _ => Task::none(),
            },
            Message::AttemptRemoteAction(request) | Message::PowerToggle(request) => {
                if request.action() == crate::RemoteAction::PowerToggle
                    && self.snapshot.as_ref().is_some_and(|s| {
                        s.control.connection_state() != crate::application::ConnectionState::Ready
                    })
                {
                    self.view_model.select_view(PrimaryView::Power);
                }
                self.command(Command::Remote(request))
            }
            Message::Wake | Message::CancelWake | Message::ConnectSelected => {
                let Some((target, generation)) = self.selected() else {
                    return Task::none();
                };
                let command = match message {
                    Message::Wake => Command::Wake { target, generation },
                    Message::CancelWake => {
                        let Some(operation) = self.snapshot.as_ref().and_then(|s| s.wake_operation)
                        else {
                            return Task::none();
                        };
                        Command::CancelWake {
                            operation,
                            target,
                            generation,
                        }
                    }
                    _ => Command::Connect { target, generation },
                };
                self.command(command)
            }
            Message::WakeTick => {
                self.wake_tick = !self.wake_tick;
                Task::none()
            }
            Message::WakeWiredChanged(value) => {
                self.wake_wired = value;
                self.wake_dirty = true;
                self.wake_save_requested = true;
                self.auto_save_wake_configuration()
            }
            Message::WakeWifiChanged(value) => {
                self.wake_wifi = value;
                self.wake_dirty = true;
                self.wake_save_requested = true;
                self.auto_save_wake_configuration()
            }
            Message::WakeInterfaceSelected(value) => {
                self.wake_active = Some(value);
                self.wake_dirty = true;
                self.wake_save_requested = true;
                self.auto_save_wake_configuration()
            }
            Message::WakeInterfaceCleared => {
                self.wake_active = None;
                self.wake_dirty = true;
                self.wake_save_requested = true;
                self.auto_save_wake_configuration()
            }
            Message::TvAddressChanged(value) => {
                self.tv_address = value;
                self.observed = None;
                Task::none()
            }
            Message::DiscoverTv => {
                self.settings_status = "Searching for Samsung TV advertisements…".into();
                self.command(Command::Discover)
            }
            Message::UseCandidate(host) => {
                self.observed = None;
                self.tv_address = host.as_str().into();
                if let Some(id) = self
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.devices.iter().find(|d| d.host == host).map(|d| d.id))
                {
                    self.update(Message::SelectSaved(id))
                } else {
                    Task::none()
                }
            }
            Message::ProbeCandidate(host) => {
                self.tv_address = host.as_str().into();
                self.observed = None;
                self.update(Message::ProbeTv)
            }
            Message::CheckSelectedTv => {
                if let Some(host) = self.snapshot.as_ref().and_then(|s| {
                    s.devices
                        .iter()
                        .find(|d| Some(d.id) == s.control.selected_device())
                        .map(|d| d.host.clone())
                }) {
                    self.update(Message::ProbeCandidate(host))
                } else {
                    Task::none()
                }
            }
            Message::ProbeTv => {
                let Ok(host) = TvHost::parse(&self.tv_address) else {
                    self.settings_status =
                        "Enter a local TV host without a scheme, port, or path.".into();
                    return Task::none();
                };
                self.settings_status = "Checking the TV’s secure identity…".into();
                let command = self
                    .candidate_handles
                    .iter()
                    .find(|candidate| candidate.host == host)
                    .map(|candidate| Command::ProbeCandidate {
                        candidate: candidate.id,
                        host: host.clone(),
                    })
                    .unwrap_or(Command::Probe(host));
                self.command(command)
            }
            Message::ConfirmAndPair | Message::ConfirmAndRepair => {
                let Some(observed) = self
                    .observed
                    .clone()
                    .filter(|o| o.host.as_str() == self.tv_address)
                else {
                    return Task::none();
                };
                let Some(snapshot) = self.snapshot.as_ref() else {
                    return Task::none();
                };
                let replace = if matches!(message, Message::ConfirmAndRepair) {
                    let Some(device) = snapshot.devices.iter().find(|d| {
                        Some(d.id) == snapshot.control.selected_device() && d.host == observed.host
                    }) else {
                        return Task::none();
                    };
                    Some(device.id)
                } else {
                    snapshot
                        .devices
                        .iter()
                        .find(|d| d.host == observed.host)
                        .map(|d| d.id)
                };
                let recovery = snapshot
                    .pairing_recovery
                    .filter(|m| Some(m.target) == replace)
                    .map(|m| m.transaction);
                let command = Command::Pair {
                    observation: observed.id,
                    host: observed.host,
                    pin: observed.observation.pin,
                    label: observed
                        .observation
                        .name
                        .filter(|name| {
                            !name.is_empty()
                                && name.len() <= 80
                                && !name.chars().any(char::is_control)
                        })
                        .unwrap_or_else(|| "Samsung TV".into()),
                    replace,
                    generation: snapshot.control.selection_generation(),
                    recovery,
                };
                self.settings_status =
                    "Approve this Mac on the physical TV to finish Pairing.".into();
                self.command(command)
            }
            Message::ForgetSelected => {
                let Some(snapshot) = &self.snapshot else {
                    return Task::none();
                };
                if let Some(marker) = snapshot.recovery {
                    return self.command(Command::RecoverForget {
                        target: marker.target,
                        transaction: marker.transaction,
                        confirm_device: marker.target,
                    });
                }
                if let Some(marker) = snapshot.pairing_recovery.filter(|m| !m.repair) {
                    return self.command(Command::RecoverPairing {
                        target: marker.target,
                        transaction: marker.transaction,
                        confirm_device: marker.target,
                    });
                }
                let Some((target, _)) = self.selected() else {
                    return Task::none();
                };
                let revision = self.config_revision(target);
                self.command(Command::Forget {
                    target,
                    revision,
                    confirm_device: target,
                })
            }
            Message::SelectSaved(target) => {
                let Some(snapshot) = &self.snapshot else {
                    return Task::none();
                };
                self.command(Command::Select {
                    target,
                    generation: snapshot.control.selection_generation(),
                })
            }
        }
    }
    fn lookup(&self, id: OperationId) -> Task<Message> {
        let (Some(adapter), Some(snapshot)) = (&self.adapter, &self.snapshot) else {
            return Task::none();
        };
        let (service, epoch) = (adapter.service.clone(), snapshot.epoch);
        Task::perform(
            async move { service.operation(epoch, id).await },
            |result| match result {
                Ok(Lookup::Known(receipt)) => Message::ServiceResult(receipt),
                _ => Message::ServiceUnavailable,
            },
        )
    }
    fn apply_snapshot(&mut self, snapshot: Snapshot) -> Task<Message> {
        if self
            .snapshot
            .as_ref()
            .is_some_and(|old| old.epoch == snapshot.epoch && old.revision > snapshot.revision)
        {
            return Task::none();
        }
        let changed = self
            .snapshot
            .as_ref()
            .and_then(|s| s.control.selected_device())
            != snapshot.control.selected_device();
        let mut tasks = Vec::new();
        if changed {
            self.observed = None;
            if let Some(device) = snapshot
                .devices
                .iter()
                .find(|d| Some(d.id) == snapshot.control.selected_device())
            {
                self.tv_address = device.host.as_str().into();
                // Keep a valid draft entered before the first selection.
                if self
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| s.control.selected_device().is_some())
                    || !self.wake_dirty
                {
                    self.load_wake_configuration(&device.wake);
                    self.wake_dirty = false;
                    self.wake_save_requested = false;
                }
            }
        }
        if !self.wake_dirty {
            if let Some(device) = snapshot
                .devices
                .iter()
                .find(|d| Some(d.id) == snapshot.control.selected_device())
            {
                self.load_wake_configuration(&device.wake);
            }
        }
        self.view_model.update_control_state(&snapshot.control);
        if let StartupState::Blocked(_) | StartupState::RecoveryRequired = snapshot.startup {
            self.settings_status = if snapshot.pairing_recovery.is_some_and(|m| m.repair) {
                "Interrupted re-pair needs attention. Check this TV and approve a fresh re-pair."
                    .into()
            } else {
                "Saved TV setup needs recovery. Use Forget TV to retry incomplete cleanup.".into()
            };
        }
        let launch = !self.launch_connected && snapshot.startup == StartupState::Ready;
        self.snapshot = Some(snapshot);
        if launch {
            self.launch_connected = true;
            if let Some((target, generation)) = self.selected() {
                tasks.push(self.command(Command::Connect { target, generation }));
            }
        }
        if self.wake_save_requested {
            tasks.push(self.auto_save_wake_configuration());
        }
        Task::batch(tasks)
    }
    fn complete(&mut self, receipt: Receipt) -> Task<Message> {
        let Some(outcome) = receipt.outcome else {
            return Task::none();
        };
        let Some(command) = self.pending.remove(&receipt.id) else {
            return Task::none();
        };
        let mut follow = Task::none();
        let (severity, text): (MessageSeverity, String) = match outcome {
            Outcome::Discovered(hosts) => { self.candidates = hosts.iter().map(|c| c.host.clone()).collect(); self.candidate_handles = hosts; (MessageSeverity::Information, "TV discovery finished. Check a TV before pairing.".into()) }
            Outcome::Probed(observation) => {
                if observation.host.as_str() == self.tv_address { self.observed = Some(observation); }
                (MessageSeverity::Information, "Check the TV identity, then confirm Pairing.".into())
            }
            Outcome::Paired(_) => { self.observed = None; (MessageSeverity::Information, "TV paired and saved securely.".into()) }
            Outcome::Connected => (MessageSeverity::Information, "TV remote connection is ready.".into()),
            Outcome::PairFailed(error) => (MessageSeverity::Warning, pair_error_message(error).into()),
            Outcome::ConnectFailed(error) => (MessageSeverity::Warning, connect_error_message(error).into()),
            Outcome::ProbeFailed(error) => (MessageSeverity::Warning, probe_error_message(error).into()),
            Outcome::DiscoveryFailed(crate::application::tv_discovery::DiscoveryError::Permission) => (MessageSeverity::Warning, "Local-network permission is required. Allow access, retry discovery, or enter the TV address manually.".into()),
            Outcome::DiscoveryFailed(_) => (MessageSeverity::Warning, "TV discovery failed. Retry or enter the TV address manually.".into()),
            Outcome::Saved(device) => {
                if matches!(command, Command::SetWake { .. }) {
                    self.wake_dirty = self.valid_wake_draft() != Some(device.wake);
                    if self.wake_dirty { return self.auto_save_wake_configuration(); }
                    if self.selection_connect_requested { self.selection_connect_requested = false; return self.update(Message::ConnectSelected); }
                    return Task::none();
                }
                self.selection_connect_requested = true;
                follow = self.auto_save_wake_configuration();
                if !self.wake_dirty { self.selection_connect_requested = false; follow = self.update(Message::ConnectSelected); }
                (MessageSeverity::Information, "Saved TV selected.".into())
            }
            Outcome::Forgotten(result) => (if result.complete() { MessageSeverity::Information } else { MessageSeverity::Warning },
                if result.complete() { "TV forgotten." } else { "TV cleanup is incomplete. Use Forget TV to explicitly retry the remaining stores." }.into()),
            Outcome::Remote(TerminalOutcome::Written) => (MessageSeverity::Debug, "Remote command written. TV state is not observed.".into()),
            Outcome::Remote(TerminalOutcome::Uncertain) => (MessageSeverity::Warning, "Remote command may have been sent. It will not be replayed.".into()),
            Outcome::Remote(_) => (MessageSeverity::Warning, "Remote command was not sent. Check the current TV connection.".into()),
            Outcome::Wake(stage) => (MessageSeverity::Information, wake_message(stage).into()),
            Outcome::Rejected(Rejection::Remote(crate::application::remote_dispatcher::DispatchRejection::Policy(reason))) => (MessageSeverity::Warning, super::view_model::rejection_message(reason).into()),
            Outcome::Rejected(Rejection::InvalidObservation) => (MessageSeverity::Warning, "TV check expired or changed. Check the TV again before pairing.".into()),
            Outcome::Rejected(Rejection::StaleSelection | Rejection::StaleConfiguration) => (MessageSeverity::Warning, "TV selection or settings changed. Review the current TV and try again.".into()),
            Outcome::Rejected(Rejection::WakeNotConfigured) => (MessageSeverity::Warning, "Configure an active Wake MAC address in Settings.".into()),
            Outcome::Rejected(_) => (MessageSeverity::Warning, "TV request is blocked or busy. Review Settings and try again.".into()),
            Outcome::Deadline | Outcome::StorageUncertain => (MessageSeverity::Warning, "TV operation timed out. Storage may still be settling; wait before retrying.".into()),
            Outcome::StorageFailed(_) | Outcome::SupervisorFailed => (MessageSeverity::Warning, "TV operation needs attention. Check saved settings and credential access.".into()),
            Outcome::Drained => (MessageSeverity::Information, "TV operation stopped during shutdown.".into()),
        };
        self.settings_status = text.clone();
        Task::batch([
            follow,
            self.publish(severity, MessageSource::MainWindow, text),
        ])
    }
    fn config_revision(&self, target: DeviceId) -> u64 {
        self.snapshot
            .as_ref()
            .and_then(|s| {
                s.configuration_revisions
                    .iter()
                    .find(|(id, _)| *id == target)
                    .map(|(_, revision)| *revision)
            })
            .unwrap_or(0)
    }
    fn auto_save_wake_configuration(&mut self) -> Task<Message> {
        if !self.wake_dirty
            || !self.wake_save_requested
            || self.snapshot.as_ref().is_some_and(|s| s.storage_pending)
            || self
                .submitting
                .values()
                .chain(self.pending.values())
                .any(|c| matches!(c, Command::SetWake { .. }))
        {
            return Task::none();
        }
        let (Some(configuration), Some((target, generation))) =
            (self.valid_wake_draft(), self.selected())
        else {
            return Task::none();
        };
        self.wake_save_requested = false;
        self.command(Command::SetWake {
            target,
            generation,
            revision: self.config_revision(target),
            configuration,
        })
    }
    fn load_wake_configuration(&mut self, wake: &WakeConfiguration) {
        self.wake_wired = wake
            .wired
            .map(|value| value.to_string())
            .unwrap_or_default();
        self.wake_wifi = wake.wifi.map(|value| value.to_string()).unwrap_or_default();
        self.wake_active = wake.active;
    }

    fn valid_wake_draft(&self) -> Option<WakeConfiguration> {
        let parse = |value: &str| {
            if value.is_empty() {
                Ok(None)
            } else {
                value.parse().map(Some)
            }
        };
        let (Ok(wired), Ok(wifi)) = (parse(&self.wake_wired), parse(&self.wake_wifi)) else {
            return None;
        };
        let wake = WakeConfiguration {
            wired,
            wifi,
            active: self.wake_active,
        };
        if wake.active.is_some() && wake.active_mac().is_none() {
            return None;
        }
        Some(wake)
    }

    fn control(&self) -> &ControlState {
        static EMPTY: ControlState = ControlState::none();
        self.snapshot.as_ref().map(|s| &s.control).unwrap_or(&EMPTY)
    }
    fn wake_stage(&self) -> WakeStage {
        self.snapshot
            .as_ref()
            .map(|s| s.wake)
            .unwrap_or(WakeStage::ConfigurationRequired)
    }
    fn power_recovery(&self) -> view::PowerRecovery {
        let selected = self.snapshot.as_ref().and_then(|s| {
            s.devices
                .iter()
                .find(|d| Some(d.id) == s.control.selected_device())
        });
        view::PowerRecovery {
            available: selected.is_some()
                && self
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| !s.pairing_pending && !s.storage_pending),
            checked: selected
                .is_some_and(|d| self.observed.as_ref().is_some_and(|o| o.host == d.host)),
        }
    }
    fn view(&self, window: window::Id) -> Element<'_, Message> {
        let devices = self
            .snapshot
            .as_ref()
            .map(|s| s.devices.as_slice())
            .unwrap_or(&[]);
        if self.settings_window == Some(window) {
            view::settings_window(view::SettingsView {
                settings_page: self.settings_page,
                address: &self.tv_address,
                candidates: &self.candidates,
                saved_devices: devices,
                fingerprint: self.observed.as_ref().map(|o| o.observation.pin.to_hex()),
                observed_name: self
                    .observed
                    .as_ref()
                    .and_then(|o| o.observation.name.clone()),
                observed_model: self
                    .observed
                    .as_ref()
                    .and_then(|o| o.observation.model.clone()),
                observed_host: self.observed.as_ref().map(|o| &o.host),
                status: &self.settings_status,
                selected_id: self.control().selected_device(),
                selected_label: self.control().selected_device_display().map(|d| d.label()),
                pairing_pending: self.snapshot.as_ref().is_some_and(|s| s.pairing_pending),
                forget_pending: self.snapshot.as_ref().is_some_and(|s| s.storage_pending),
                cleanup_required: self.snapshot.as_ref().is_some_and(|s| {
                    s.recovery.is_some() || s.pairing_recovery.is_some_and(|m| !m.repair)
                }),
                wake_wired: &self.wake_wired,
                wake_wifi: &self.wake_wifi,
                wake_active: self.wake_active,
            })
        } else {
            let configured = devices.iter().any(|d| {
                Some(d.id) == self.control().selected_device() && d.wake.active_mac().is_some()
            });
            view::main_window(
                &self.view_model,
                self.wake_stage(),
                configured,
                self.wake_tick,
                self.power_recovery(),
            )
        }
    }

    fn title(&self, window: window::Id) -> String {
        if self.settings_window == Some(window) {
            "Samsung TV Remote Settings".to_owned()
        } else {
            "Samsung TV Remote".to_owned()
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            window::close_events().map(Message::WindowClosed),
            event::listen_with(shortcut_event),
        ];
        if matches!(
            self.wake_stage(),
            WakeStage::CheckingConnection | WakeStage::PacketSending | WakeStage::Reconnecting
        ) {
            subscriptions.push(
                ::iced::time::every(std::time::Duration::from_millis(500))
                    .map(|_| Message::WakeTick),
            );
        }
        Subscription::batch(subscriptions)
    }

    fn publish(
        &mut self,
        severity: super::view_model::MessageSeverity,
        source: super::view_model::MessageSource,
        text: impl Into<String>,
    ) -> Task<Message> {
        if self
            .view_model
            .messages_mut()
            .append(severity, source, text)
        {
            ::iced::widget::operation::snap_to_end(view::MESSAGE_FEED_ID)
        } else {
            Task::none()
        }
    }
}
fn shortcut_event(event: Event, status: event::Status, window: window::Id) -> Option<Message> {
    if status != event::Status::Ignored {
        return None;
    }

    let Event::Keyboard(keyboard::Event::KeyPressed {
        key,
        modified_key,
        physical_key,
        modifiers,
        text,
        repeat: false,
        ..
    }) = event
    else {
        return None;
    };

    let shifted_plus = modifiers == Modifiers::SHIFT
        && (matches!(modified_key.as_ref(), Key::Character("+"))
            || matches!(
                physical_key,
                keyboard::key::Physical::Code(keyboard::key::Code::Equal)
            )
            || text.as_deref() == Some("+"));
    let shortcut = if shifted_plus {
        Some(Shortcut::Remote(crate::RemoteAction::VolumeUp))
    } else {
        shortcut_for(&key, modifiers)
    };
    shortcut.map(|shortcut| Message::Shortcut { window, shortcut })
}

fn shortcut_for(key: &Key, modifiers: Modifiers) -> Option<Shortcut> {
    if modifiers == Modifiers::NONE {
        return match key.as_ref() {
            Key::Named(key::Named::ArrowUp) => Some(Shortcut::Remote(crate::RemoteAction::Up)),
            Key::Named(key::Named::ArrowDown) => Some(Shortcut::Remote(crate::RemoteAction::Down)),
            Key::Named(key::Named::ArrowLeft) => Some(Shortcut::Remote(crate::RemoteAction::Left)),
            Key::Named(key::Named::ArrowRight) => {
                Some(Shortcut::Remote(crate::RemoteAction::Right))
            }
            Key::Named(key::Named::Enter) => Some(Shortcut::Remote(crate::RemoteAction::Enter)),
            Key::Named(key::Named::Escape) => Some(Shortcut::Remote(crate::RemoteAction::Back)),
            Key::Named(key::Named::Home) => Some(Shortcut::Remote(crate::RemoteAction::Home)),
            Key::Character("m") => Some(Shortcut::Remote(crate::RemoteAction::Mute)),
            Key::Character("+") => Some(Shortcut::Remote(crate::RemoteAction::VolumeUp)),
            Key::Character("-") => Some(Shortcut::Remote(crate::RemoteAction::VolumeDown)),
            _ => None,
        };
    }
    if !modifiers.command() || modifiers.alt() {
        return None;
    }

    if modifiers.shift() {
        return None;
    }

    match key.as_ref() {
        Key::Character("1") => Some(Shortcut::Navigate(super::view_model::PrimaryView::Power)),
        Key::Character("2") => Some(Shortcut::Navigate(super::view_model::PrimaryView::Remote)),
        Key::Character("3") => Some(Shortcut::Navigate(super::view_model::PrimaryView::Sources)),
        Key::Character("4") => Some(Shortcut::Navigate(super::view_model::PrimaryView::Apps)),
        Key::Character("5") => Some(Shortcut::Navigate(
            super::view_model::PrimaryView::TextInput,
        )),
        Key::Character(",") => Some(Shortcut::OpenSettings),
        _ => None,
    }
}

fn probe_error_message(error: TvSessionError) -> &'static str {
    match error {
        TvSessionError::Target(_) => "This TV address does not resolve to a local network target.",
        TvSessionError::Timeout => {
            "TV probe timed out. Check local-network permission and try again."
        }
        TvSessionError::Offline => {
            "TV was not reachable on secure port 8002. Check its address and network."
        }
        TvSessionError::Tls => "Secure connection failed. Check the TV and retry.",
        _ => "TV probe failed. Retry from Settings.",
    }
}

fn pair_error_message(error: PairFlowError) -> &'static str {
    match error {
        PairFlowError::Cancelled => "TV setup was cancelled before it could be saved.",
        PairFlowError::Session(TvSessionError::PairingDenied) => "TV pairing was denied. Approve this Mac on the physical TV and retry.",
        PairFlowError::Session(TvSessionError::CertificateChanged) => "TV certificate changed. Probe and confirm the TV again.",
        PairFlowError::Session(TvSessionError::Timeout) => "TV pairing timed out. Check the TV prompt and retry.",
        PairFlowError::Session(TvSessionError::PairingTokenMissing) => "TV accepted the connection without a pairing token. Remove this client on the TV, then retry Pairing.",
        PairFlowError::Session(TvSessionError::RemoteChannel) => "TV rejected the secure remote channel. Confirm its address and retry.",
        PairFlowError::Session(TvSessionError::Protocol) => "TV sent an invalid pairing response. Retry Pairing.",
        PairFlowError::Session(TvSessionError::Tls) => "Secure TV handshake failed. Probe the TV again.",
        PairFlowError::Session(TvSessionError::Offline) => "TV connection closed during Pairing. Check the network and retry.",
        PairFlowError::Setup(crate::application::tv_setup_service::SetupError::PartialCleanup) => "Pairing could not be saved and credential cleanup was incomplete. Check Keychain.",
        PairFlowError::Setup(_) => "Pairing succeeded but could not be saved securely. Retry after checking Keychain access.",
        PairFlowError::Session(_) => "TV pairing failed. Check the TV and retry.",
    }
}

fn connect_error_message(error: ConnectFlowError) -> &'static str {
    match error {
        ConnectFlowError::Session(TvSessionError::CertificateChanged) => {
            "TV certificate changed. Reconfirm its identity and re-pair."
        }
        ConnectFlowError::Session(TvSessionError::TokenRejected) => {
            "TV rejected the saved token. Re-pair in Settings."
        }
        ConnectFlowError::Session(TvSessionError::Offline) => {
            "Saved TV is offline or unreachable. Check the network and retry."
        }
        ConnectFlowError::Session(TvSessionError::Timeout) => {
            "TV connection timed out. Check local-network permission and retry."
        }
        ConnectFlowError::Reconnect(
            crate::application::tv_setup_service::ReconnectError::HostChanged,
        ) => "Saved TV address changed. Confirm the TV and re-pair.",
        ConnectFlowError::Reconnect(
            crate::application::tv_setup_service::ReconnectError::PairingRequired,
        ) => "Saved TV token is missing. Re-pair in Settings.",
        ConnectFlowError::CredentialSave => {
            "TV returned a new token, but Keychain could not save it. Re-pair in Settings."
        }
        ConnectFlowError::Session(TvSessionError::RemoteChannel) => {
            "TV rejected the saved remote channel. Re-pair in Settings."
        }
        ConnectFlowError::Session(TvSessionError::Protocol) => {
            "TV sent an invalid reconnect response. Re-pair in Settings."
        }
        ConnectFlowError::Session(TvSessionError::Tls) => {
            "Secure TV handshake failed. Probe the TV and retry."
        }
        _ => "Saved TV connection failed. Retry or re-pair in Settings.",
    }
}

pub fn run(services: AppServices) -> ::iced::Result {
    ::iced::daemon(move || App::boot(services.clone()), App::update, App::view)
        .title(App::title)
        .theme(dark_theme)
        .subscription(App::subscription)
        .run()
}

fn dark_theme(_: &App, _: window::Id) -> Theme {
    Theme::Dark
}

fn main_window_settings() -> window::Settings {
    window::Settings {
        size: Size::new(1100.0, 760.0),
        min_size: Some(Size::new(1100.0, 760.0)),
        position: window::Position::Centered,
        ..window::Settings::default()
    }
}

fn settings_window_settings() -> window::Settings {
    window::Settings {
        size: Size::new(1000.0, 660.0),
        min_size: Some(Size::new(1000.0, 660.0)),
        position: window::Position::Centered,
        ..window::Settings::default()
    }
}

fn wake_message(stage: WakeStage) -> &'static str {
    match stage {
        WakeStage::Ready | WakeStage::Connected => "Wake packet sent; TV remote connection is ready.",
        WakeStage::Cancelled => "Wake cancelled before sending a packet.",
        WakeStage::CancelledDuringSend => "Wake cancelled during packet send. The packet may have been sent and cannot be recalled.",
        WakeStage::CancelledAfterSend => "Wake cancelled. A packet already sent cannot be recalled.",
        WakeStage::FailedAfterSend(_) => "Wake packet sent, but the TV remote connection is not ready. Check the TV and retry connecting.",
        WakeStage::Failed(_) => "Wake failed. Check network access and Wake configuration.",
        _ => "Wake status updated.",
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opening_settings_is_idempotent_until_the_window_closes() {
        let (mut app, _) = App::new();
        let _ = app.update(Message::OpenSettings);
        let settings_id = app.settings_window;

        let _ = app.update(Message::OpenSettings);
        assert_eq!(app.settings_window, settings_id);

        let _ = app.update(Message::WindowClosed(settings_id.expect("opened")));
        assert!(app.settings_window.is_none());
    }

    #[test]
    fn opening_and_closing_settings_preserves_main_navigation_and_message_state() {
        let (mut app, _) = App::new();
        let _ = app.update(Message::Navigate(
            super::super::view_model::PrimaryView::Apps,
        ));
        let _ = app.publish(
            super::super::view_model::MessageSeverity::Information,
            super::super::view_model::MessageSource::MainWindow,
            "A safe presentation message",
        );
        let _ = app.update(Message::OpenSettings);
        let settings_id = app.settings_window.expect("settings window");

        let _ = app.update(Message::WindowClosed(settings_id));

        assert_eq!(
            app.view_model.primary_view(),
            super::super::view_model::PrimaryView::Apps
        );
        let entries = app.view_model.messages().entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "A safe presentation message");
    }

    #[test]
    fn delayed_window_open_event_does_not_restore_closed_settings() {
        let (mut app, _) = App::new();
        let _ = app.update(Message::OpenSettings);
        let settings_id = app.settings_window.expect("settings window");

        let _ = app.update(Message::WindowClosed(settings_id));
        let _ = app.update(Message::SettingsWindowOpened(settings_id));

        assert!(app.settings_window.is_none());
    }

    #[test]
    fn keyboard_shortcuts_require_command_and_match_routes() {
        let command = Modifiers::COMMAND;
        let routes = [
            ("1", super::super::view_model::PrimaryView::Power),
            ("2", super::super::view_model::PrimaryView::Remote),
            ("3", super::super::view_model::PrimaryView::Sources),
            ("4", super::super::view_model::PrimaryView::Apps),
            ("5", super::super::view_model::PrimaryView::TextInput),
        ];
        for (key, route) in routes {
            assert_eq!(
                shortcut_for(&Key::Character(key.into()), command),
                Some(Shortcut::Navigate(route))
            );
        }
        assert_eq!(
            shortcut_for(&Key::Character(",".into()), command),
            Some(Shortcut::OpenSettings)
        );
        assert_eq!(
            shortcut_for(&Key::Named(key::Named::ArrowUp), command | Modifiers::SHIFT),
            None
        );
        assert_eq!(
            shortcut_for(&Key::Character("1".into()), Modifiers::NONE),
            None
        );
    }

    #[test]
    fn windows_use_the_approved_dark_layout_dimensions() {
        assert_eq!(
            main_window_settings().min_size,
            Some(Size::new(1100.0, 760.0))
        );
        assert_eq!(
            settings_window_settings().min_size,
            Some(Size::new(1000.0, 660.0))
        );
        let (app, _) = App::new();
        assert_eq!(dark_theme(&app, window::Id::unique()), Theme::Dark);
    }

    #[test]
    fn remote_shortcuts_map_to_semantic_actions_only_for_ignored_nonrepeat_keys() {
        assert_eq!(
            shortcut_for(&Key::Named(key::Named::ArrowUp), Modifiers::NONE),
            Some(Shortcut::Remote(crate::RemoteAction::Up))
        );
        assert_eq!(
            shortcut_for(&Key::Named(key::Named::Enter), Modifiers::NONE),
            Some(Shortcut::Remote(crate::RemoteAction::Enter))
        );
        assert_eq!(
            shortcut_for(&Key::Character("m".into()), Modifiers::NONE),
            Some(Shortcut::Remote(crate::RemoteAction::Mute))
        );
        let event = Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(key::Named::ArrowUp),
            modified_key: Key::Named(key::Named::ArrowUp),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::ArrowUp),
            location: keyboard::Location::Standard,
            modifiers: Modifiers::NONE,
            text: None,
            repeat: true,
        });
        let window = window::Id::unique();
        assert!(shortcut_event(event.clone(), event::Status::Ignored, window).is_none());
        assert!(shortcut_event(event, event::Status::Captured, window).is_none());

        let shifted_plus = Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Character("=".into()),
            modified_key: Key::Character("+".into()),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Equal),
            location: keyboard::Location::Standard,
            modifiers: Modifiers::SHIFT,
            text: Some("+".into()),
            repeat: false,
        });
        assert!(matches!(
            shortcut_event(shifted_plus, event::Status::Ignored, window),
            Some(Message::Shortcut {
                shortcut: Shortcut::Remote(crate::RemoteAction::VolumeUp),
                ..
            })
        ));

        let shifted_equal_without_modified_plus = Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Character("=".into()),
            modified_key: Key::Character("=".into()),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Equal),
            location: keyboard::Location::Standard,
            modifiers: Modifiers::SHIFT,
            text: None,
            repeat: false,
        });
        assert!(matches!(
            shortcut_event(
                shifted_equal_without_modified_plus,
                event::Status::Ignored,
                window
            ),
            Some(Message::Shortcut {
                shortcut: Shortcut::Remote(crate::RemoteAction::VolumeUp),
                ..
            })
        ));
    }

    #[test]
    fn navigation_shortcut_from_settings_does_not_change_main_route() {
        let (mut app, _) = App::new();
        let _ = app.update(Message::OpenMainWindow);
        let _ = app.update(Message::OpenSettings);
        let settings_id = app.settings_window.expect("settings window");

        let _ = app.update(Message::Shortcut {
            window: settings_id,
            shortcut: Shortcut::Navigate(super::super::view_model::PrimaryView::Sources),
        });

        assert_eq!(
            app.view_model.primary_view(),
            super::super::view_model::PrimaryView::Remote
        );
    }

    #[test]
    fn settings_sidebar_switches_between_discovery_and_wake_on_lan() {
        let (mut app, _) = App::new();
        assert_eq!(app.settings_page, SettingsPage::Discovery);

        let _ = app.update(Message::SelectSettingsPage(SettingsPage::WakeOnLan));

        assert_eq!(app.settings_page, SettingsPage::WakeOnLan);
    }

    #[test]
    fn remote_shortcut_from_settings_does_not_emit_a_tv_request() {
        let (mut app, _) = App::new();
        let _ = app.update(Message::OpenSettings);
        let settings_id = app.settings_window.expect("settings window");
        let before = app.view_model.messages().entries().len();
        let _ = app.update(Message::Shortcut {
            window: settings_id,
            shortcut: Shortcut::Remote(crate::RemoteAction::Up),
        });
        assert_eq!(app.view_model.messages().entries().len(), before);
        assert!(app.pending.is_empty() && app.submitting.is_empty());
    }
    fn projection(target: Option<DeviceId>, generation: u64, revision: u64) -> Snapshot {
        use crate::application::device_repository::SavedDevice;
        let mut control =
            ControlState::restored(target.map(|id| crate::DeviceDisplay::new(id, "Fixture TV")));
        if let Some(target) = target {
            for _ in 0..generation {
                control.restart_selection(crate::DeviceDisplay::new(target, "Fixture TV"));
            }
        }
        Snapshot {
            epoch: Epoch(1),
            revision,
            startup: StartupState::Ready,
            control,
            devices: Arc::new(
                target
                    .into_iter()
                    .map(|id| SavedDevice {
                        id,
                        label: "Fixture TV".into(),
                        host: TvHost::parse("fixture.local").unwrap(),
                        wake: WakeConfiguration::default(),
                    })
                    .collect(),
            ),
            configuration_revisions: Arc::new(target.into_iter().map(|id| (id, 0)).collect()),
            duplicate_records: false,
            storage_pending: false,
            recovery: None,
            pairing_recovery: None,
            wake: WakeStage::Idle,
            wake_operation: None,
            pairing_pending: false,
        }
    }
    #[tokio::test]
    async fn gui_launch_connect_is_explicit_once_and_does_not_select() {
        let (mut app, _) = App::new();
        let adapter = Arc::new(InProcess::start(AppServices::without_adapters()));
        app.adapter = Some(adapter.clone());
        let snapshot = projection(Some(DeviceId::new(1)), 0, 1);
        let _ = app.apply_snapshot(snapshot.clone());
        let _ = app.apply_snapshot(snapshot);
        assert_eq!(app.submitting.len(), 1);
        assert!(
            matches!(app.submitting.values().next(), Some(Command::Connect { target, generation: 0 }) if *target == DeviceId::new(1))
        );
        adapter.drain().await;
    }
    #[tokio::test]
    async fn wake_drafts_survive_resnapshot_and_coalesce_while_storage_is_fenced() {
        let (mut app, _) = App::new();
        let adapter = Arc::new(InProcess::start(AppServices::without_adapters()));
        app.adapter = Some(adapter.clone());
        app.launch_connected = true;
        let mut snapshot = projection(Some(DeviceId::new(1)), 1, 1);
        snapshot.storage_pending = true;
        let _ = app.apply_snapshot(snapshot.clone());
        let _ = app.update(Message::WakeWiredChanged("02:11:22:33:44:55".into()));
        let _ = app.update(Message::WakeInterfaceSelected(WakeInterface::Wired));
        assert!(app.submitting.is_empty());
        let _ = app.apply_snapshot(snapshot.clone());
        assert_eq!(app.wake_wired, "02:11:22:33:44:55");
        snapshot.storage_pending = false;
        snapshot.revision += 1;
        let _ = app.apply_snapshot(snapshot);
        assert_eq!(app.submitting.len(), 1);
        assert!(
            matches!(app.submitting.values().next(), Some(Command::SetWake { target, revision: 0, configuration, .. }) if *target == DeviceId::new(1) && configuration.active == Some(WakeInterface::Wired))
        );
        let _ = app.update(Message::OpenSettings);
        let settings = app.settings_window.unwrap();
        let _ = app.update(Message::WindowClosed(settings));
        assert_eq!(app.submitting.len(), 1);
        adapter.drain().await;
    }
    #[tokio::test]
    async fn terminal_before_admission_reply_is_recovered_without_duplicate_feedback() {
        let (mut app, _) = App::new();
        let adapter = Arc::new(InProcess::start(AppServices::without_adapters()));
        app.adapter = Some(adapter.clone());
        app.launch_connected = true;
        let _ = app.apply_snapshot(projection(None, 0, 1));
        let _ = app.command(Command::Discover);
        let receipt = Receipt {
            id: OperationId(9),
            metadata: Command::Discover.metadata(Epoch(1)),
            outcome: Some(Outcome::Discovered(Vec::new())),
        };
        let _ = app.complete(receipt.clone());
        assert!(app.view_model.messages().entries().is_empty());
        let _ = app.update(Message::Admitted(
            1,
            Ok(Receipt {
                id: receipt.id,
                metadata: receipt.metadata,
                outcome: None,
            }),
        ));
        let _ = app.update(Message::ServiceResult(receipt.clone()));
        let _ = app.update(Message::ServiceResult(receipt));
        assert!(app.pending.is_empty());
        assert_eq!(app.view_model.messages().entries().len(), 1);
        adapter.drain().await;
    }
    #[tokio::test]
    async fn cleanup_remains_actionable_after_partial_forget_removes_selected_record() {
        let (mut app, _) = App::new();
        let adapter = Arc::new(InProcess::start(AppServices::without_adapters()));
        app.adapter = Some(adapter.clone());
        app.launch_connected = true;
        let mut snapshot = projection(None, 0, 1);
        let marker = crate::application::recovery::ForgetMarker::prepared(DeviceId::new(1));
        snapshot.recovery = Some(marker);
        snapshot.startup = StartupState::RecoveryRequired;
        let _ = app.apply_snapshot(snapshot);
        let _ = app.update(Message::ForgetSelected);
        assert!(
            matches!(app.submitting.values().next(), Some(Command::RecoverForget { target, transaction, confirm_device }) if *target == marker.target && *transaction == marker.transaction && *confirm_device == marker.target)
        );
        adapter.drain().await;
    }
}
