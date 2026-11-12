#[cfg(test)]
#[path = "client_tests.rs"]
mod client_tests;
#[path = "lifecycle.rs"]
mod lifecycle;
#[path = "remote_probe.rs"]
mod remote_probe;
#[path = "workflows.rs"]
mod workflows;
use super::ui_message::{Message, SettingsPage, Shortcut};
use super::view;
use super::view_model::{MessageSeverity, MessageSource, PrimaryView, ViewModel};
use crate::application::control_service::{
    Candidate, Command, Epoch, Observation, OperationId, Request, Snapshot, StartupState,
    TerminalState,
};
use crate::application::tv_address::TvHost;
use crate::application::wake::{WakeConfiguration, WakeInterface};
use crate::application::{ControlState, SendRemoteAction};
use crate::control_client::model::WakeStage;
use crate::domain::DeviceId;
use ::iced::keyboard::{self, key, Key, Modifiers};
use ::iced::{event, window, Element, Event, Size, Subscription, Task, Theme};
use futures_util::SinkExt;
use std::collections::HashMap;
use std::sync::Arc;

/// Captured presentation context only. The client tracker owns delivery,
/// lookup and uncertainty; these rows cannot resubmit their commands.
struct LocalIntent {
    command: Command,
    operation: Option<OperationId>,
    context: Option<workflows::IntentContext>,
}
pub struct App {
    main_window: Option<window::Id>,
    settings_window: Option<window::Id>,
    settings_page: SettingsPage,
    view_model: ViewModel,
    remote: Option<Arc<dyn crate::control_client::port::NativeClient>>,
    development_probe: bool,
    remote_status: Option<crate::control_client::background::Status>,
    remote_management: bool,
    pending_management: Option<crate::control_client::port::ManagementIntent>,
    management_error: Option<crate::control_client::port::ManagementError>,
    stop_result: Option<
        Result<
            crate::control_client::port::StopReport,
            crate::control_client::port::ManagementError,
        >,
    >,
    stop_confirmation: Option<Epoch>,
    awaiting_snapshot: bool,
    client_exited: bool,
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
    sequence: u64,
    intents: HashMap<u64, LocalIntent>,
    workflows: workflows::Workflows,
}
impl App {
    fn new() -> (Self, Task<Message>) {
        (
            Self {
                main_window: None,
                settings_window: None,
                settings_page: SettingsPage::Discovery,
                view_model: ViewModel::new(&ControlState::none()),
                remote: None,
                development_probe: false,
                remote_status: None,
                remote_management: false,
                pending_management: None,
                management_error: None,
                stop_result: None,
                stop_confirmation: None,
                awaiting_snapshot: false,
                client_exited: false,
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
                sequence: 0,
                intents: HashMap::new(),
                workflows: workflows::Workflows::default(),
            },
            Task::done(Message::OpenMainWindow),
        )
    }
    fn boot(executable: std::path::PathBuf) -> (Self, Task<Message>) {
        let (mut app, open) = Self::new();
        app.view_model.set_service_available(false);
        (
            app,
            Task::batch([
                open,
                Task::perform(
                    crate::control_client::port::production(executable),
                    Message::RemoteStarted,
                ),
            ]),
        )
    }
    fn boot_probe(config: crate::control_client::background::Config) -> (Self, Task<Message>) {
        let (mut app, open) = Self::new();
        app.development_probe = true;
        app.launch_connected = false;
        app.tv_address = "synthetic-tv.invalid".into();
        app.settings_status = "Development probe: connecting to the isolated fake server.".into();
        app.view_model.set_service_available(false);
        (
            app,
            Task::batch([
                open,
                if cfg!(debug_assertions)
                    && std::env::var_os("SAMSUNG_TV_REMOTE_PROBE_CAPTURE").is_some()
                {
                    Task::perform(
                        tokio::time::sleep(std::time::Duration::from_secs(8)),
                        |_| Message::CaptureProbe,
                    )
                } else {
                    Task::none()
                },
                Task::perform(
                    crate::control_client::port::start(config),
                    Message::RemoteStarted,
                ),
            ]),
        )
    }
    fn command(&mut self, command: Command) -> Task<Message> {
        self.remote_command(command)
    }
    fn selected(&self) -> Option<(DeviceId, u64)> {
        let snapshot = self.snapshot.as_ref()?;
        Some((
            snapshot.control.selected_device()?,
            snapshot.control.selection_generation(),
        ))
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        if self.client_exited {
            if let Message::RemoteStarted(Ok(client)) = message {
                client.disconnect();
            }
            return Task::none();
        }
        self.prepare_client_intent(&message);
        match message {
            Message::CaptureProbe if self.development_probe && cfg!(debug_assertions) => {
                self.main_window.map_or_else(Task::none, |id| {
                    window::screenshot(id).map(Message::ProbeCaptured)
                })
            }
            Message::ProbeCaptured(screenshot)
                if self.development_probe && cfg!(debug_assertions) =>
            {
                // Renderer-only evidence avoids capturing other applications or
                // requiring desktop Screen Recording access. This debug hook
                // is opt-in and exists only in the isolated fake GUI mode.
                if let Some(path) = std::env::var_os("SAMSUNG_TV_REMOTE_PROBE_CAPTURE") {
                    use std::io::Write;
                    let _ = (|| -> std::io::Result<()> {
                        let mut file = std::fs::File::create_new(path)?;
                        write!(
                            file,
                            "P6\n{} {}\n255\n",
                            screenshot.size.width, screenshot.size.height
                        )?;
                        let rgb: Vec<_> = screenshot
                            .rgba
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .flat_map(|p| p[..3].iter().copied())
                            .collect();
                        file.write_all(&rgb)
                    })();
                }
                Task::none()
            }
            Message::CaptureProbe | Message::ProbeCaptured(_) => Task::none(),
            Message::ResumeServer
            | Message::RestartServer
            | Message::RecheckServer
            | Message::RequestStopServer
            | Message::ConfirmStopServer
            | Message::CancelStopServer => self.client_lifecycle(message),
            Message::RemoteManagementFinished(result) => self.finish_client_management(result),
            Message::RemoteStarted(Ok(remote)) => {
                let Some(mut updates) = remote.take_updates() else {
                    return Task::none();
                };
                self.remote = Some(remote);
                Task::run(
                    ::iced::stream::channel(16, async move |mut output| {
                        while let Some(update) = updates.recv().await {
                            if output.send(Message::RemoteUpdate(update)).await.is_err() {
                                return;
                            }
                        }
                    }),
                    |message| message,
                )
            }
            Message::RemoteStarted(Err(error)) => {
                self.management_error = Some(error);
                self.remote_status = Some(crate::control_client::background::Status::Blocked(
                    match error {
                        crate::control_client::port::ManagementError::Blocked(reason) => reason,
                        _ => crate::control_client::reconnect::Blocked::RecheckFailed,
                    },
                ));
                self.refresh_client_controls();
                self.publish(
                    MessageSeverity::Warning,
                    MessageSource::MainWindow,
                    lifecycle::management_error_text(error),
                )
            }
            Message::RemoteUpdate(update) => self.remote_update(update),
            Message::RemoteAdmitted(sequence, result) => self.remote_admitted(sequence, result),
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
                self.client_exited = true;
                self.stop_confirmation = None;
                if let Some(remote) = &self.remote {
                    remote.disconnect();
                }
                ::iced::exit()
            }
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
                    _ => Command::Connect {
                        target,
                        generation,
                        only_if_idle: false,
                    },
                };
                self.command(command)
            }
            Message::WakeTick => {
                self.wake_tick = !self.wake_tick;
                Task::none()
            }
            Message::WakeWiredChanged(value) => {
                self.begin_wake_edit();
                self.wake_wired = value;
                self.wake_dirty = true;
                self.wake_save_requested = true;
                self.auto_save_wake_configuration()
            }
            Message::WakeWifiChanged(value) => {
                self.begin_wake_edit();
                self.wake_wifi = value;
                self.wake_dirty = true;
                self.wake_save_requested = true;
                self.auto_save_wake_configuration()
            }
            Message::WakeInterfaceSelected(value) => {
                self.begin_wake_edit();
                self.wake_active = Some(value);
                self.wake_dirty = true;
                self.wake_save_requested = true;
                self.auto_save_wake_configuration()
            }
            Message::WakeInterfaceCleared => {
                self.begin_wake_edit();
                self.wake_active = None;
                self.wake_dirty = true;
                self.wake_save_requested = true;
                self.auto_save_wake_configuration()
            }
            Message::ReloadWakeSettings => self.reload_client_wake(),
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
                if !self.client_confirmation_current() {
                    self.observed = None;
                    return self.publish(
                        MessageSeverity::Warning,
                        MessageSource::MainWindow,
                        "The TV check or selection changed. Check the TV again before Pairing.",
                    );
                }
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
                self.consume_client_confirmation();
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
                {
                    if let Some(device) = self
                        .snapshot
                        .as_ref()
                        .and_then(|state| state.devices.iter().find(|device| device.id == target))
                    {
                        self.tv_address = device.host.as_str().into();
                    }
                }
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
    fn apply_snapshot(&mut self, snapshot: Snapshot) -> Task<Message> {
        self.apply_client_snapshot(snapshot)
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
        self.save_client_wake()
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
            available: self.view_model.control_state().service_available
                && selected.is_some()
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
            let content = view::settings_window(view::SettingsView {
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
                wake_conflict: self.workflows.conflict,
                service_available: self.view_model.control_state().service_available,
            });
            self.server_status_bar(content)
        } else {
            let configured = devices.iter().any(|d| {
                Some(d.id) == self.control().selected_device() && d.wake.active_mac().is_some()
            });
            let content = view::main_window(
                &self.view_model,
                self.wake_stage(),
                configured,
                self.wake_tick,
                self.power_recovery(),
            );
            self.server_status_bar(content)
        }
    }

    fn title(&self, window: window::Id) -> String {
        if self.settings_window == Some(window) {
            if self.development_probe {
                "Samsung TV Remote Settings — Development probe".to_owned()
            } else {
                "Samsung TV Remote Settings".to_owned()
            }
        } else {
            if self.development_probe {
                "Samsung TV Remote — Development probe".to_owned()
            } else {
                "Samsung TV Remote".to_owned()
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            window::close_events().map(Message::WindowClosed),
            event::listen_with(shortcut_event),
        ];
        if self.view_model.control_state().service_available
            && matches!(
                self.wake_stage(),
                WakeStage::CheckingConnection | WakeStage::PacketSending | WakeStage::Reconnecting
            )
        {
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

pub fn run_probe(config: crate::control_client::background::Config) -> ::iced::Result {
    ::iced::daemon(
        move || App::boot_probe(config.clone()),
        App::update,
        App::view,
    )
    .title(App::title)
    .theme(dark_theme)
    .subscription(App::subscription)
    .run()
}

pub fn run(executable: std::path::PathBuf) -> ::iced::Result {
    ::iced::daemon(
        move || App::boot(executable.clone()),
        App::update,
        App::view,
    )
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
        WakeStage::Ready=>"Wake packet sent; TV remote connection is ready.",
        WakeStage::Connected=>"TV remote connection is ready; no Wake packet was sent.",
        WakeStage::Cancelled=>"Wake cancelled before sending a packet.",
        WakeStage::CancelledDuringSend=>"Wake cancelled during packet send. The packet may have been sent and cannot be recalled.",
        WakeStage::CancelledAfterSend=>"Wake cancelled. A packet already sent cannot be recalled.",
        WakeStage::FailedAfterSend(_)=>"Wake packet sent, but the TV remote connection is not ready. Check the TV and retry connecting.",
        WakeStage::Failed(_)=>"Wake failed. Check network access and Wake configuration.",
        _=>"Wake status updated.",
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
    fn client_probe_has_no_local_owner_and_reattach_never_repeats_launch() {
        let config = crate::control_client::background::Config {
            executable: "/synthetic/executable".into(),
            base: "/synthetic/profile".into(),
        };
        let (mut app, _) = App::boot_probe(config);
        assert!(app.development_probe);
        assert!(!app.launch_connected);
        let _ = app.update(Message::RemoteUpdate(
            crate::control_client::background::Update::Snapshot {
                snapshot: Box::new(projection(Some(DeviceId::new(1)), 0, 1)),
                initial: true,
            },
        ));
        assert!(app.launch_connected);
        assert!(app.intents.is_empty());
        assert!(!app.wake_save_requested);
        let _ = app.update(Message::RemoteUpdate(
            crate::control_client::background::Update::Status(
                crate::control_client::background::Status::Waiting {
                    due: tokio::time::Instant::now() + std::time::Duration::from_secs(1),
                },
            ),
        ));
        assert!(!app.view_model.control_state().remote_actions_enabled());
        assert_eq!(app.control().selected_device(), Some(DeviceId::new(1)));
        let _ = app.remote_update(crate::control_client::background::Update::Snapshot {
            snapshot: Box::new(projection(Some(DeviceId::new(1)), 0, 1)),
            initial: true,
        });
        assert!(app.launch_connected && app.intents.is_empty());
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
        assert!(app.intents.is_empty());
    }
    pub(super) fn projection(target: Option<DeviceId>, generation: u64, revision: u64) -> Snapshot {
        use crate::application::device_repository::SavedDevice;
        let mut control =
            ControlState::restored(target.map(|id| crate::DeviceDisplay::new(id, "Fixture TV")));
        if let Some(target) = target {
            for _ in 0..generation {
                control.restart_selection(crate::DeviceDisplay::new(target, "Fixture TV"));
            }
        }
        Snapshot {
            admission_closed: false,
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
    #[test]
    fn retained_history_preserves_local_drafts_confirmation_and_navigation() {
        use crate::control_client::{
            background::Update,
            projection::{NativeOutcome, NativeReceipt},
        };
        let (mut app, _) = App::new();
        app.launch_connected = true;
        let snapshot = projection(Some(DeviceId::new(1)), 4, 8);
        let epoch = snapshot.epoch;
        let _ = app.apply_snapshot(snapshot);
        app.wake_wired = "unfinished local draft".into();
        app.wake_dirty = true;
        app.tv_address = "draft.invalid".into();
        app.view_model.select_view(PrimaryView::Remote);
        let receipt = NativeReceipt {
            id: OperationId(9),
            epoch,
            target: Some(DeviceId::new(1)),
            generation: Some(0),
            terminal: true,
            outcome: Some(NativeOutcome::Saved(DeviceId::new(1))),
            expires_at: std::num::NonZeroU64::new(1),
            terminal_state: Some(TerminalState {
                selected: Some(DeviceId::new(1)),
                generation: 1,
                configuration_revision: Some(2),
            }),
        };
        let _ = app.remote_update(Update::History(receipt.clone()));
        assert_eq!(app.wake_wired, "unfinished local draft");
        assert!(app.wake_dirty);
        assert_eq!(app.tv_address, "draft.invalid");
        assert_eq!(app.view_model.primary_view(), PrimaryView::Remote);
        assert!(app.observed.is_none());
        assert!(app.intents.is_empty());
        assert_eq!(app.view_model.messages().entries().len(), 1);
        app.intents.insert(
            7,
            LocalIntent {
                command: Command::Select {
                    target: DeviceId::new(1),
                    generation: 0,
                },
                operation: Some(receipt.id),
                context: None,
            },
        );
        let _ = app.remote_update(Update::KnownBeforeRestart { tag: 7, receipt });
        assert!(app.intents.is_empty());
        assert_eq!(app.wake_wired, "unfinished local draft");
        assert!(app.wake_dirty);
        assert_eq!(app.view_model.primary_view(), PrimaryView::Remote);
        assert_eq!(app.view_model.messages().entries().len(), 2);
    }
    #[test]
    fn client_terminal_before_late_admission_settles_once_without_an_iced_lookup() {
        use crate::control_client::projection::{NativeOutcome, NativeReceipt};
        let (mut app, _) = App::new();
        app.launch_connected = true;
        let snapshot = projection(None, 0, 1);
        let epoch = snapshot.epoch;
        let _ = app.apply_snapshot(snapshot);
        app.intents.insert(
            3,
            LocalIntent {
                command: Command::Discover,
                operation: None,
                context: None,
            },
        );
        let terminal = NativeReceipt {
            id: OperationId(10),
            epoch,
            target: None,
            generation: None,
            terminal: true,
            outcome: Some(NativeOutcome::Discovered(vec![])),
            expires_at: std::num::NonZeroU64::new(1),
            terminal_state: Some(TerminalState {
                selected: None,
                generation: 0,
                configuration_revision: None,
            }),
        };
        let _ = app.remote_admitted(3, Ok(terminal.clone()));
        let pending = NativeReceipt {
            terminal: false,
            outcome: None,
            expires_at: None,
            terminal_state: None,
            ..terminal.clone()
        };
        let _ = app.remote_admitted(3, Ok(pending));
        let _ = app.remote_admitted(3, Ok(terminal));
        assert!(app.intents.is_empty());
        assert_eq!(app.view_model.messages().entries().len(), 1);
        app.intents.insert(
            4,
            LocalIntent {
                command: Command::Discover,
                operation: None,
                context: None,
            },
        );
        let code = crate::control_client::model::FailureCode::StaleEpoch;
        let _ = app.remote_admitted(
            4,
            Err(crate::control_client::background::SubmitError::NotAdmitted(
                code,
            )),
        );
        let _ =
            app.remote_update(crate::control_client::background::Update::Rejected { tag: 4, code });
        assert!(app.intents.is_empty());
        assert_eq!(app.view_model.messages().entries().len(), 2);
    }
}
