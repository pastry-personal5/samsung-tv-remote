//! Existing-window development composition. No local runtime or TV port exists.
use super::*;
use crate::control_client::{
    background::{Status, SubmitError, Update},
    projection::NativeReceipt,
    reconnect::Blocked,
};
impl App {
    pub(super) fn remote_command(&mut self, command: Command) -> Task<Message> {
        let (Some(remote), Some(snapshot)) = (&self.remote, &self.snapshot) else {
            return self.publish(
                MessageSeverity::Warning,
                MessageSource::MainWindow,
                "Control server is still connecting; the action was not submitted.",
            );
        };
        if !remote.connected() || snapshot.admission_closed {
            return self.publish(
                MessageSeverity::Warning,
                MessageSource::MainWindow,
                "Control server is unavailable; the action was not submitted.",
            );
        }
        if self.pending.len() + self.submitting.len() >= 64 {
            return self.publish(
                MessageSeverity::Warning,
                MessageSource::MainWindow,
                "Wait for pending operations before submitting another action.",
            );
        }
        self.sequence += 1;
        let sequence = self.sequence;
        let request = Request {
            epoch: snapshot.epoch,
            key: uuid::Uuid::new_v4().to_string(),
            command: command.clone(),
        };
        self.submitting.insert(sequence, command);
        let remote = remote.clone();
        Task::perform(
            async move { remote.submit(sequence, request).await },
            move |result| Message::RemoteAdmitted(sequence, result),
        )
    }
    pub(super) fn remote_update(&mut self, update: Update) -> Task<Message> {
        match update {
            Update::Snapshot { snapshot, initial } => {
                let replaced = initial
                    && self
                        .snapshot
                        .as_ref()
                        .is_some_and(|old| old.epoch != snapshot.epoch);
                if initial {
                    self.launch_connected = true;
                    self.selection_connect_requested = false;
                    self.wake_save_requested = false;
                }
                if replaced {
                    self.pending.clear();
                    self.submitting.clear();
                    self.remote_tags.clear();
                }
                let apply = self.apply_snapshot(*snapshot);
                if replaced {
                    Task::batch([apply,self.publish(MessageSeverity::Warning,MessageSource::MainWindow,"Server restarted. Previous unavailable outcomes remain unknown; no TV action was repeated.")])
                } else {
                    apply
                }
            }
            Update::Status(status) => {
                let available = matches!(status, Status::Connected)
                    && self.snapshot.as_ref().is_some_and(|s| !s.admission_closed);
                self.view_model.set_service_available(available);
                let text = status_text(&status);
                self.remote_status = Some(status);
                self.settings_status = text.clone();
                self.publish(
                    if available {
                        MessageSeverity::Information
                    } else {
                        MessageSeverity::Warning
                    },
                    MessageSource::MainWindow,
                    text,
                )
            }
            Update::Operation(receipt) => self.remote_complete(receipt),
            Update::Recovered { tag, receipt } => self.remote_admitted(tag, Ok(receipt)),
            Update::Unknown { tag } => {
                self.submitting.remove(&tag);
                if let Some(id) = self.remote_tags.remove(&tag) {
                    self.pending.remove(&id);
                }
                self.publish(MessageSeverity::Warning,MessageSource::MainWindow,"The original outcome is unavailable or belongs to an earlier server lifetime. It will not be repeated.")
            }
        }
    }
    pub(super) fn remote_admitted(
        &mut self,
        sequence: u64,
        result: Result<NativeReceipt, SubmitError>,
    ) -> Task<Message> {
        match result {
            Ok(receipt)=>{
                let Some(command)=self.submitting.remove(&sequence) else{return Task::none();};
                if self.snapshot.as_ref().is_none_or(|snapshot|snapshot.epoch!=receipt.epoch){return self.publish(MessageSeverity::Warning,MessageSource::MainWindow,"The server lifetime changed before the receipt arrived; no action will be repeated.");}
                self.remote_tags.insert(sequence,receipt.id);self.pending.insert(receipt.id,command);
                if receipt.terminal{self.remote_complete(receipt)}else{self.lookup(receipt.id)}
            }
            Err(SubmitError::Ambiguous)=>self.publish(MessageSeverity::Warning,MessageSource::MainWindow,"The request reply was lost. Recovering its original result by lookup; it will not be submitted again."),
            Err(_)=>{self.submitting.remove(&sequence);self.publish(MessageSeverity::Warning,MessageSource::MainWindow,"The request was not admitted. Review the current server and selection before another intent.")}
        }
    }
    pub(super) fn remote_complete(&mut self, receipt: NativeReceipt) -> Task<Message> {
        if !receipt.terminal
            || self
                .snapshot
                .as_ref()
                .is_none_or(|snapshot| snapshot.epoch != receipt.epoch)
        {
            return Task::none();
        }
        let Some(command) = self.pending.remove(&receipt.id) else {
            return Task::none();
        };
        self.remote_tags.retain(|_, id| *id != receipt.id);
        let(mut severity,text)=(MessageSeverity::Information,match receipt.kind(){
            "discovered"=>match receipt.candidates(){Ok(candidates)=>{self.candidates=candidates.iter().map(|c|c.host.clone()).collect();self.candidate_handles=candidates;"Discovery finished. Check a TV before pairing."},Err(_)=>"The discovery result could not be projected; check the server connection."},
            "probed"=>match receipt.observation(){Ok(observed)=>{if observed.host.as_str()==self.tv_address{self.observed=Some(observed);}"Review the identity and confirm Pairing."},Err(_)=>"The observation could not be projected. Check the server connection."},
            "tv_approved_and_saved"=>{self.observed=None;"TV approved and saved in the isolated profile."},
            "saved"=>{if matches!(command,Command::SetWake{..}){self.wake_save_requested=false;if self.snapshot.as_ref().and_then(|s|s.devices.iter().find(|d|Some(d.id)==s.control.selected_device())).is_some_and(|d|self.valid_wake_draft()==Some(d.wake)){self.wake_dirty=false;}}self.selection_connect_requested=false;"Saved settings committed. Connect explicitly if the TV session is not ready."},
            "remote_ready"=>"The TV remote channel is ready.",
            "written"=>"Remote action written to the simulated TV channel. Physical TV state is unobserved.",
            "uncertain"=>"The action may have been sent. It will not be repeated automatically.",
            "not_sent"=>"The remote action was not sent. Review the current connection before another intent.",
            "wake"=>"Wake finished; packet and readiness facts are available in the latest server state.",
            "forgotten"=>"Device cleanup completed.",
            "partial_setup"=>"Cleanup is incomplete. Use the explicit recovery action after reviewing Settings.",
            _=>match receipt.result["error"]["code"].as_str(){
                Some("stale_selection"|"stale_config")=>"Selection or settings changed. Review the latest state before another intent.",
                Some("observation_expired")=>{self.observed=None;"The observation expired or changed. Check the TV again before Pairing."},
                Some("storage_uncertain"|"deadline"|"uncertain")=>"The outcome is uncertain. Wait for settlement and inspect the original result; it will not be repeated.",
                _=>"The server rejected or failed the operation. Review the latest state and Settings before another intent.",
            },
        });
        if matches!(
            receipt.kind(),
            "failed" | "partial_setup" | "uncertain" | "not_sent"
        ) {
            severity = MessageSeverity::Warning;
        }
        self.settings_status = text.into();
        self.publish(severity, MessageSource::MainWindow, text)
    }
    pub(super) fn server_status_bar<'a>(
        &'a self,
        content: Element<'a, Message>,
    ) -> Element<'a, Message> {
        use ::iced::widget::{button, column, container, row, text, Space};
        use ::iced::{Alignment, Length};
        let status = self.remote_status.as_ref();
        let mut line = row![
            text(
                status
                    .map(status_text)
                    .unwrap_or_else(|| "Connecting to isolated server…".into())
            )
            .size(13),
            Space::new().width(Length::Fill)
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if matches!(status, Some(Status::Blocked(Blocked::Stopped))) {
            line = line.push(
                button("Resume server")
                    .on_press_maybe((!self.remote_management).then_some(Message::ResumeServer)),
            );
        }
        if matches!(status, Some(Status::Blocked(Blocked::IncompatibleBuild))) {
            line = line.push(
                button("Restart server")
                    .on_press_maybe((!self.remote_management).then_some(Message::RestartServer)),
            );
        }
        column![
            container(line).padding([6, 12]).width(Length::Fill),
            content
        ]
        .height(Length::Fill)
        .into()
    }
}
fn status_text(status: &Status) -> String {
    match status {
        Status::Connecting => "Connecting to isolated server…".into(),
        Status::Connected => {
            "Connected to isolated server. TV readiness is shown separately.".into()
        }
        Status::Waiting { due } => {
            let delay = due.saturating_duration_since(tokio::time::Instant::now());
            let seconds = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .saturating_add(delay.as_secs());
            format!(
                "Server disconnected. Next retry at {:02}:{:02}:{:02} UTC.",
                seconds / 3600 % 24,
                seconds / 60 % 60,
                seconds % 60
            )
        }
        Status::Blocked(Blocked::Stopped) => {
            "Server intentionally stopped. Resume explicitly to connect again.".into()
        }
        Status::Blocked(Blocked::IncompatibleBuild) => {
            "Server build is incompatible. Restart explicitly to use this build.".into()
        }
        Status::Blocked(Blocked::Revoked | Blocked::BootstrapDenied) => {
            "Server access denied. Resolve credentials before connecting again.".into()
        }
        Status::Blocked(Blocked::InteractionRequired) => {
            "Credential access needs attention. Automatic attempts are suspended.".into()
        }
        Status::Blocked(Blocked::UnsafeMetadata) => {
            "Server metadata or wire state is unsafe. Resolve the profile before connecting.".into()
        }
        Status::Exited => "Disconnected from control server.".into(),
    }
}
