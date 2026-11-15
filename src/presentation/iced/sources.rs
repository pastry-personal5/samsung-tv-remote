//! Sources view and editor intents. Every save is one explicit click built
//! from the current snapshot and revision; nothing is resubmitted silently.
use super::*;
use crate::application::sources::{SourceChoice, SourceConfiguration, SourceLabel, SourceSetting};
use crate::domain::RemoteAction;
use crate::presentation::iced::ui_message::SourceEdit;

/// Target-bound editor state. A selection change discards it, never retargets it.
#[derive(Debug, Default)]
pub(super) struct SourceDrafts {
    target: Option<DeviceId>,
    labels: [String; 4],
    pending_show: Option<RemoteAction>,
}

fn index(action: RemoteAction) -> Option<usize> {
    RemoteAction::SOURCE_PORTS.iter().position(|a| *a == action)
}

impl App {
    pub(super) fn selected_sources(&self) -> Option<&SourceConfiguration> {
        let snapshot = self.snapshot.as_ref()?;
        let target = snapshot.control.selected_device()?;
        snapshot
            .devices
            .iter()
            .find(|d| d.id == target)
            .map(|d| &d.sources)
    }
    fn stored_labels(&self) -> [String; 4] {
        self.selected_sources()
            .map(|s| {
                (*s.ports).clone().map(|p| match p {
                    SourceSetting::Shown {
                        label: Some(label), ..
                    } => label.as_str().to_owned(),
                    _ => String::new(),
                })
            })
            .unwrap_or_default()
    }
    fn current_drafts(&mut self) -> &mut SourceDrafts {
        let target = self.control().selected_device();
        if self.source_drafts.target != target {
            let labels = self.stored_labels();
            self.source_drafts = SourceDrafts {
                target,
                labels,
                pending_show: None,
            };
        }
        &mut self.source_drafts
    }
    /// Label inputs and pending Show for the editor, for the selected TV only.
    pub(super) fn source_editor_state(&self) -> ([String; 4], Option<RemoteAction>) {
        if self.source_drafts.target == self.control().selected_device() {
            (
                self.source_drafts.labels.clone(),
                self.source_drafts.pending_show,
            )
        } else {
            (self.stored_labels(), None)
        }
    }
    pub(super) fn source_save_pending(&self) -> bool {
        self.intents
            .values()
            .any(|intent| matches!(intent.command, Command::SetSources { .. }))
    }

    pub(super) fn source_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SendSource(action) => match self.selected() {
                Some((target, generation)) => self.update(Message::AttemptRemoteAction(
                    SendRemoteAction::new(target, generation, action),
                )),
                None => self.publish(
                    MessageSeverity::Warning,
                    MessageSource::MainWindow,
                    "No TV selected. Open Settings to choose a TV.",
                ),
            },
            Message::EditSource(action, SourceEdit::Show) => {
                self.current_drafts().pending_show = Some(action);
                Task::none()
            }
            Message::CancelShowSource => {
                self.current_drafts().pending_show = None;
                Task::none()
            }
            Message::ConfirmShowSource => {
                let Some(action) = self.current_drafts().pending_show.take() else {
                    return Task::none();
                };
                self.save_label(action)
            }
            Message::EditSource(action, SourceEdit::Hide) => {
                self.submit_source(action, SourceChoice::Hidden)
            }
            Message::EditSource(action, SourceEdit::Reset) => {
                self.submit_source(action, SourceChoice::Unconfigured)
            }
            Message::SourceLabelChanged(action, value) => {
                if let Some(i) = index(action) {
                    self.current_drafts().labels[i] = value;
                }
                Task::none()
            }
            Message::SaveSourceLabel(action) => self.save_label(action),
            Message::OpenSourceSettings => {
                self.settings_page = SettingsPage::Sources;
                self.update(Message::OpenSettings)
            }
            _ => Task::none(),
        }
    }

    fn save_label(&mut self, action: RemoteAction) -> Task<Message> {
        let Some(i) = index(action) else {
            return Task::none();
        };
        let draft = self.current_drafts().labels[i].clone();
        // An empty input means "use the port name", never an empty label.
        let label = if draft.trim().is_empty() {
            None
        } else {
            match SourceLabel::parse(&draft) {
                Some(label) => Some(label),
                None => {
                    return self.publish(
                        MessageSeverity::Warning,
                        MessageSource::SettingsWindow,
                        "Labels are up to 24 characters (64 bytes) with no control or text-direction characters. Nothing was saved.",
                    )
                }
            }
        };
        self.submit_source(action, SourceChoice::Shown(label))
    }

    fn submit_source(&mut self, action: RemoteAction, choice: SourceChoice) -> Task<Message> {
        let (Some((target, generation)), Some(i)) = (self.selected(), index(action)) else {
            return Task::none();
        };
        if self.source_save_pending() {
            return self.publish(
                MessageSeverity::Warning,
                MessageSource::SettingsWindow,
                "A source save is still pending. Wait for its result before another change.",
            );
        }
        let Some(mut choices) = self.selected_sources().map(SourceConfiguration::choices) else {
            return Task::none();
        };
        choices[i] = choice;
        let revision = self.config_revision(target);
        self.command(Command::SetSources {
            target,
            generation,
            revision,
            choices,
        })
    }
}

/// Requests are never reported as an active input.
pub(super) fn source_outcome(
    command: &Command,
    receipt: &crate::control_client::projection::NativeReceipt,
) -> Option<String> {
    use crate::control_client::projection::NativeOutcome;
    let Command::Remote(request) = command else {
        return None;
    };
    request.action().source_capability()?;
    let name = crate::application::sources::port_name(request.action());
    match receipt.outcome {
        Some(NativeOutcome::Written) => Some(format!(
            "{name}: source request sent. The TV does not report its active input."
        )),
        Some(NativeOutcome::Uncertain) => Some(format!(
            "{name}: source request may have been sent. It will not be repeated."
        )),
        _ => None,
    }
}
