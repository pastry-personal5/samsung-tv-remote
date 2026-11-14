//! Explicit server lifecycle UI. Stop confirmation is bound to the observed
//! epoch; server acknowledgment, marker observation and settlement stay distinct.
use super::*;
use crate::control_client::{
    background::Status,
    port::{
        ManagementError, ManagementIntent, ManagementReply, StopAcknowledgment, StopSettlement,
    },
    reconnect::Blocked,
};
impl App {
    pub(super) fn server_settings_panel(&self) -> Element<'_, Message> {
        use ::iced::widget::{button, column, container, row, text};
        use ::iced::{Alignment, Length};
        let status = self.remote_status.as_ref();
        let enabled = !self.remote_management && !self.mcp_pending && !self.client_exited;
        let mut line = row![text(
            status
                .map(super::remote_probe::status_text)
                .unwrap_or_else(|| "Starting or attaching to control server…".into())
        )
        .size(13)
        .width(Length::Fill)]
        .spacing(8)
        .align_y(Alignment::Center);
        if matches!(status, Some(Status::Connected)) {
            line = line.push(
                button("Stop server").on_press_maybe(enabled.then_some(Message::RequestStopServer)),
            );
        }
        if matches!(status, Some(Status::Blocked(Blocked::Stopped))) {
            line = line.push(
                button("Resume server").on_press_maybe(enabled.then_some(Message::ResumeServer)),
            );
        }
        if matches!(status, Some(Status::Blocked(Blocked::IncompatibleBuild))) {
            line = line.push(
                button("Restart server").on_press_maybe(enabled.then_some(Message::RestartServer)),
            );
        }
        if !matches!(
            status,
            Some(Status::Blocked(
                Blocked::Stopped | Blocked::IncompatibleBuild | Blocked::SigningRequired
            ))
        ) && self.remote.is_some()
            && (matches!(status, Some(Status::Blocked(_)))
                || self.management_error.is_some()
                || matches!(self.stop_result, Some(Err(_))))
        {
            let credentials = matches!(
                status,
                Some(Status::Blocked(
                    Blocked::BootstrapDenied | Blocked::Revoked | Blocked::InteractionRequired
                ))
            );
            line = line.push(
                button(if credentials {
                    "Recheck credentials"
                } else {
                    "Check server state"
                })
                .on_press_maybe(enabled.then_some(Message::RecheckServer)),
            );
        }
        let mut panel = column![line].spacing(12);
        if let Some(notice) = &self.server_notice {
            panel = panel.push(text(notice).size(13));
        }
        if self.remote_management {
            panel = panel
                .push(text("Server management is pending. TV commands are disabled.").size(13));
        }
        if let Some(result) = self.stop_result {
            panel = panel.push(
                text(match result {
                    Ok(report) => stop_text(report),
                    Err(error) => management_error_text(error).into(),
                })
                .size(13),
            );
        }
        if let Some(error) = self.management_error {
            panel = panel.push(text(management_error_text(error)).size(13));
        }
        if self.stop_confirmation.is_some() {
            panel=panel.push(column![
                text("Stop the control server for every client?").size(15),
                text("Every connected remote client will disconnect. Operations already accepted may still finish. This window stays open.").size(13),
                row![button("Cancel").on_press(Message::CancelStopServer),button("Stop for all clients").on_press_maybe(enabled.then_some(Message::ConfirmStopServer))].spacing(8),
            ].spacing(6));
        }
        container(panel.push(self.mcp_panel()))
            .width(Length::Fill)
            .into()
    }
    pub(super) fn refresh_client_controls(&mut self) {
        let available = !self.client_exited
            && !self.remote_management
            && !self.awaiting_snapshot
            && self
                .remote
                .as_ref()
                .is_some_and(|client| client.connected())
            && matches!(self.remote_status, Some(Status::Connected))
            && self
                .snapshot
                .as_ref()
                .is_some_and(|state| !state.admission_closed);
        self.view_model.set_service_available(available);
    }
    pub(super) fn client_lifecycle(&mut self, message: Message) -> Task<Message> {
        if self.client_exited {
            return Task::none();
        }
        if matches!(message, Message::CancelStopServer) {
            self.stop_confirmation = None;
            return Task::none();
        }
        if self.remote_management || self.mcp_pending {
            return Task::none();
        }
        let intent = match message {
            Message::RequestStopServer => {
                if self
                    .remote
                    .as_ref()
                    .is_some_and(|client| client.connected())
                    && matches!(self.remote_status, Some(Status::Connected))
                {
                    self.stop_confirmation = self.snapshot.as_ref().map(|state| state.epoch);
                }
                return Task::none();
            }
            Message::ConfirmStopServer => {
                let Some(epoch) = self.stop_confirmation.take() else {
                    return Task::none();
                };
                if !matches!(self.remote_status, Some(Status::Connected))
                    || self
                        .snapshot
                        .as_ref()
                        .is_none_or(|state| state.epoch != epoch)
                {
                    self.server_notice = Some("Server changed before Stop was confirmed. Review the current server before stopping it.".into());
                    return Task::none();
                }
                ManagementIntent::Stop { epoch }
            }
            Message::ResumeServer
                if matches!(self.remote_status, Some(Status::Blocked(Blocked::Stopped))) =>
            {
                ManagementIntent::Resume
            }
            Message::RestartServer
                if matches!(
                    self.remote_status,
                    Some(Status::Blocked(Blocked::IncompatibleBuild))
                ) =>
            {
                ManagementIntent::Restart
            }
            Message::RecheckServer
                if !matches!(
                    self.remote_status,
                    Some(Status::Blocked(
                        Blocked::Stopped | Blocked::IncompatibleBuild | Blocked::SigningRequired
                    ))
                ) =>
            {
                ManagementIntent::Recheck
            }
            _ => return Task::none(),
        };
        let Some(client) = self.remote.clone() else {
            return Task::none();
        };
        self.interrupt_client_flow();
        self.launch_connected = true;
        self.stop_confirmation = None;
        self.remote_management = true;
        self.pending_management = Some(intent);
        self.management_error = None;
        self.server_notice = None;
        if matches!(intent, ManagementIntent::Stop { .. }) {
            self.stop_result = None;
        } else {
            self.awaiting_snapshot = true;
        }
        self.refresh_client_controls();
        Task::perform(
            async move { client.manage(intent).await },
            Message::RemoteManagementFinished,
        )
    }
    pub(super) fn finish_client_management(
        &mut self,
        result: Result<ManagementReply, ManagementError>,
    ) -> Task<Message> {
        if self.client_exited {
            return Task::none();
        }
        let Some(intent) = self.pending_management.take() else {
            return Task::none();
        };
        self.remote_management = false;
        let result = match (intent, result) {
            (ManagementIntent::Stop { .. }, Ok(ManagementReply::Stop(report))) => {
                self.stop_result = Some(Ok(report));
                Ok(ManagementReply::Stop(report))
            }
            (ManagementIntent::Stop { .. }, Err(error)) => {
                self.stop_result = Some(Err(error));
                Err(error)
            }
            (
                ManagementIntent::Resume | ManagementIntent::Restart,
                Ok(ManagementReply::ReattachRequested),
            ) => {
                self.stop_result = None;
                Ok(ManagementReply::ReattachRequested)
            }
            (ManagementIntent::Recheck, Ok(ManagementReply::CredentialsRechecked)) => {
                Ok(ManagementReply::CredentialsRechecked)
            }
            (_, Err(error)) => {
                self.management_error = Some(error);
                Err(error)
            }
            _ => {
                let error = ManagementError::Blocked(Blocked::UnsafeMetadata);
                if matches!(intent, ManagementIntent::Stop { .. }) {
                    self.stop_result = Some(Err(error));
                } else {
                    self.management_error = Some(error);
                }
                Err(error)
            }
        };
        self.refresh_client_controls();
        let text = match result {
            Ok(ManagementReply::Stop(_)) | Err(_) => {
                // Stop results and errors already have their own rows in the panel.
                self.server_notice = None;
                return Task::none();
            }
            Ok(ManagementReply::ReattachRequested) => "Server attachment requested. Waiting for authenticated current state; no TV action was repeated.".into(),
            Ok(ManagementReply::CredentialsRechecked) => "Server access rechecked through the protected channel. No TV action was issued.".into(),
        };
        self.server_notice = Some(text);
        Task::none()
    }
}
pub(super) fn stop_text(report: crate::control_client::port::StopReport) -> String {
    let ack = match report.acknowledgment {
        StopAcknowledgment::Durable => "Stop was durably acknowledged for every client.",
        StopAcknowledgment::MarkerObserved => {
            "A Stop marker was observed, but the durability acknowledgment was not received."
        }
    };
    let settlement = match report.settlement {
        StopSettlement::Observed => "Server settlement was observed.",
        StopSettlement::Pending => {
            "Settlement is not yet observed; accepted operations may still be finishing."
        }
        StopSettlement::CheckFailed => {
            "Settlement could not be checked; its completion is unknown."
        }
    };
    format!("{ack} {settlement} The GUI remains open; Resume is explicit.")
}
pub(super) fn management_error_text(error: ManagementError) -> &'static str {
    match error {
        ManagementError::Busy=>"Server management is busy or the previous owner is still settling. No process was killed; check server state before another action.",
        ManagementError::Unavailable=>"Server management could not complete. Check server state explicitly; no automatic Stop, Resume or restart will be sent.",
        ManagementError::StaleEpoch=>"Server changed before Stop admission. Nothing was stopped by this request; review current server state.",
        ManagementError::StopOutcomeUnknown=>"Stop acknowledgment was lost and its outcome is unknown. Check server state explicitly; Stop will not be repeated.",
        ManagementError::StopPersistenceFailed=>"Stop persistence failed. Durability and settlement are unconfirmed; no automatic Resume or Stop retry will be made.",
        ManagementError::Blocked(Blocked::Stopped)=>"A Stop marker is present. Resume explicitly after the previous owner settles.",
        ManagementError::Blocked(Blocked::IncompatibleBuild)=>"The authenticated server build differs. Restart explicitly to use this build.",
        ManagementError::Blocked(Blocked::UnsafeMetadata)=>"Server identity, endpoint or profile metadata is unsafe. Resolve it before checking again.",
        ManagementError::Blocked(Blocked::BootstrapDenied|Blocked::Revoked)=>"Server access is denied. Resolve credentials and recheck explicitly; revoked access is not regranted.",
        ManagementError::Blocked(Blocked::InteractionRequired)=>"Credential access needs attention. Resolve it outside the app, then recheck explicitly.",
        ManagementError::Blocked(Blocked::StopUnconfirmed)=>"Stop durability and settlement remain unconfirmed. Check server state explicitly.",
        ManagementError::Blocked(Blocked::SigningRequired)=>"A signed application identity is required. Build and open the locally signed app bundle.",
        ManagementError::Blocked(Blocked::RecheckFailed)=>"The explicit server check failed. Automatic attempts are paused; resolve the cause before another check.",
    }
}
