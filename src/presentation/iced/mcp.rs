//! Explicit connection controls. Saved configuration, enrollment, endpoint
//! verification and actual client attachment are distinct facts.
use super::*;
use crate::control_client::mcp::{Configuration, Error, Intent, State};

impl App {
    pub(super) fn mcp_panel(&self) -> Element<'_, Message> {
        use ::iced::widget::{button, column, row, text};
        let available = self.remote.is_some()
            && !self.client_exited
            && !self.remote_management
            && !self.mcp_pending;
        let mut panel = column![text("Claude Code").size(17),
            text("Connect once to let Claude Code use this remote from any project. TV Pairing and Forget still require your confirmation.").size(13),
            button("Check connection settings").on_press_maybe(available.then_some(Message::CheckMcp)),
        ].spacing(8);
        if let Some(state) = &self.mcp_state {
            panel = panel.push(text(match state.configuration {
                Configuration::Absent => "Configuration: no app entry saved.",
                Configuration::Saved => "Configuration: app entry saved.",
                Configuration::RepairRequired => "Configuration: the helper changed or the app moved. Review repair before replacing the entry.",
                Configuration::Conflict => "Configuration: conflicting or relocated entry. Resolve it before Connect; Disconnect preserves changed entries.",
            }).size(13));
            panel = panel.push(
                text(if state.enabled {
                    "MCP access: enabled for all Claude sessions."
                } else {
                    "MCP access: disabled."
                })
                .size(13),
            );
            panel = panel.push(text(if state.endpoint_verified {
                "Endpoint: authenticated MCP catalog verified by this app. Actual Claude Code attachment: not verified."
            } else { "Endpoint: not verified. Actual Claude Code attachment: not verified." }).size(13));
            let mut actions = row![].spacing(8);
            if matches!(
                state.configuration,
                Configuration::Absent | Configuration::Saved
            ) {
                actions = actions.push(
                    button("Connect Claude Code")
                        .on_press_maybe(available.then_some(Message::ConnectMcp)),
                );
            }
            if state.configuration == Configuration::RepairRequired {
                actions = actions.push(
                    button("Review connection repair")
                        .on_press_maybe(available.then_some(Message::RequestRepairMcp)),
                );
            }
            if state.enabled
                || matches!(
                    state.configuration,
                    Configuration::Saved | Configuration::RepairRequired
                )
            {
                actions = actions.push(
                    button("Disconnect Claude Code")
                        .on_press_maybe(available.then_some(Message::RequestDisconnectMcp)),
                );
            }
            if state.enabled && state.configuration == Configuration::Saved {
                actions = actions.push(
                    button("Verify MCP endpoint")
                        .on_press_maybe(available.then_some(Message::VerifyMcp)),
                );
            }
            panel = panel.push(actions);
        }
        if self.mcp_pending {
            panel = panel.push(text("Checking or updating Claude Code access…").size(13));
        }
        if let Some(error) = &self.mcp_error {
            panel = panel.push(text(error_text(error)).size(13));
            if self.mcp_intent.is_some() {
                panel = panel.push(
                    button("Retry the same connection request")
                        .on_press_maybe(available.then_some(Message::RetryMcp)),
                );
            }
        }
        if self.mcp_disconnect_confirmation {
            panel = panel.push(column![text("Disconnect all Claude Code sessions?").size(15),
                text("This revokes MCP access for every session. Accepted TV operations may still finish. GUI control stays available.").size(13),
                row![button("Cancel").on_press(Message::CancelDisconnectMcp),
                    button("Disconnect all sessions").on_press_maybe(available.then_some(Message::ConfirmDisconnectMcp))].spacing(8),
            ].spacing(6));
        }
        if self.mcp_repair_confirmation {
            panel = panel.push(column![text("Replace the changed app entry?").size(15),
                text("Use this app's current bundled authentication helper for the saved endpoint. Other Claude settings are preserved. A concurrent entry change will block replacement.").size(13),
                row![button("Cancel").on_press(Message::CancelRepairMcp),
                    button("Repair Claude Code connection").on_press_maybe(available.then_some(Message::ConfirmRepairMcp))].spacing(8),
            ].spacing(6));
        }
        panel.into()
    }

    pub(super) fn mcp_action(&mut self, message: Message) -> Task<Message> {
        if self.client_exited || self.mcp_pending || self.remote_management {
            return Task::none();
        }
        if matches!(message, Message::CancelDisconnectMcp) {
            self.mcp_disconnect_confirmation = false;
            return Task::none();
        }
        if matches!(message, Message::CancelRepairMcp) {
            self.mcp_repair_confirmation = false;
            return Task::none();
        }
        if matches!(message, Message::RequestRepairMcp) {
            self.mcp_repair_confirmation = self
                .mcp_state
                .as_ref()
                .is_some_and(|state| state.configuration == Configuration::RepairRequired);
            return Task::none();
        }
        if matches!(message, Message::RequestDisconnectMcp) {
            self.mcp_disconnect_confirmation = self.mcp_state.is_some();
            return Task::none();
        }
        let intent = match message {
            Message::CheckMcp => Intent::Check,
            Message::VerifyMcp => Intent::Verify,
            Message::ConnectMcp => {
                let Some(state) = self.mcp_state.clone() else {
                    return Task::none();
                };
                if !matches!(
                    state.configuration,
                    Configuration::Absent | Configuration::Saved
                ) {
                    return Task::none();
                }
                Intent::connect(state)
            }
            Message::ConfirmRepairMcp if self.mcp_repair_confirmation => {
                self.mcp_repair_confirmation = false;
                let Some(state) = self.mcp_state.clone() else {
                    return Task::none();
                };
                if state.configuration != Configuration::RepairRequired {
                    return Task::none();
                }
                Intent::connect(state)
            }
            Message::ConfirmDisconnectMcp if self.mcp_disconnect_confirmation => {
                self.mcp_disconnect_confirmation = false;
                let Some(state) = self.mcp_state.clone() else {
                    return Task::none();
                };
                Intent::disconnect(state)
            }
            Message::RetryMcp => {
                let Some(intent) = self.mcp_intent.clone() else {
                    return Task::none();
                };
                intent
            }
            _ => return Task::none(),
        };
        let Some(client) = self.remote.clone() else {
            return Task::none();
        };
        self.mcp_pending = true;
        self.mcp_disconnect_confirmation = false;
        self.mcp_repair_confirmation = false;
        self.mcp_error = None;
        self.mcp_intent = Some(intent.clone());
        Task::perform(
            async move { client.mcp(intent).await },
            Message::McpFinished,
        )
    }

    pub(super) fn mcp_finished(&mut self, result: Result<State, Error>) -> Task<Message> {
        if !self.mcp_pending {
            return Task::none();
        }
        self.mcp_pending = false;
        match result {
            Ok(state) => {
                self.mcp_state = Some(state);
                self.mcp_intent = None;
                self.mcp_error = None;
            }
            Err(error) => {
                self.mcp_state = None;
                self.mcp_error = Some(error);
            }
        }
        Task::none()
    }
}

fn error_text(error: &Error) -> String {
    use crate::control_client::mcp::Error::*;
    match error {
        UnsupportedClient => "This Claude Code version has not passed the app's compatibility checks. Connect requires the verified client version; no grant was made.".into(),
        ClientUnavailable => "Claude Code could not be found or its version check failed. Install the supported client and check again; no grant was made.".into(),
        Restricted => "Claude Code has managed or restrictive MCP settings. Resolve the policy before Connect; no grant was made.".into(),
        ScopeConflict => "A local or project entry shadows this app's user entry. Resolve the conflicting entry before Connect; no grant was made.".into(),
        ConfigurationConflict => "Claude configuration changed or could not be read safely. Check the entry before another action.".into(),
        Workflow(failure) => {
            let configuration = match failure.configuration {
                crate::control_client::mcp::WorkflowConfiguration::Saved => "Configuration saved.",
                crate::control_client::mcp::WorkflowConfiguration::Removed => "Configuration removed.",
                crate::control_client::mcp::WorkflowConfiguration::Unchanged => "Configuration unchanged by this request.",
                crate::control_client::mcp::WorkflowConfiguration::Unknown => "Configuration outcome requires inspection.",
            };
            let access = match failure.enabled {
                Some(true) => "Last acknowledged MCP access is enabled.",
                Some(false) => "Last acknowledged MCP access is disabled.",
                None => "MCP access outcome is unconfirmed; revocation may be incomplete.",
            };
            let recovery = failure.recovery_file.as_ref().map(|path| format!(" Retained configuration for recovery: {}.", path.display())).unwrap_or_default();
            format!("{configuration} {access}{recovery} Retry this same request or check settings explicitly.")
        }
        Busy => "Connection management is busy. Check again when it finishes.".into(),
        _ => "Connection state is unconfirmed. Check settings explicitly before another action.".into(),
    }
}
