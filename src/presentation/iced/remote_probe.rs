//! API client observation and outcomes. No local runtime or TV port exists.
use super::*;
use crate::control_client::{
    background::{Status, SubmitError, Update},
    projection::{NativeOutcome, NativeReceipt},
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
        if self.remote_management
            || self.awaiting_snapshot
            || !remote.connected()
            || snapshot.admission_closed
        {
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
        let remote = remote.clone();
        self.capture_client_context(sequence, &command);
        self.submitting.insert(sequence, command);
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
                    self.awaiting_snapshot = false;
                    self.stop_confirmation = None;
                    if self.workflows.attached {
                        self.launch_connected = true;
                        self.workflows.cancel();
                        self.wake_save_requested = false;
                    }
                    self.workflows.attached = true;
                }
                if replaced {
                    self.reset_client_workflows();
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
                if !matches!(
                    status,
                    Status::Connected | Status::Connecting | Status::Starting
                ) {
                    self.interrupt_client_flow();
                }
                let text = status_text(&status);
                let ordinary = matches!(
                    status,
                    Status::Connected | Status::Connecting | Status::Starting
                );
                self.remote_status = Some(status);
                self.refresh_client_controls();
                self.settings_status = text.clone();
                self.publish(
                    if ordinary {
                        MessageSeverity::Information
                    } else {
                        MessageSeverity::Warning
                    },
                    MessageSource::MainWindow,
                    text,
                )
            }
            Update::History(receipt) => {
                if self
                    .snapshot
                    .as_ref()
                    .is_none_or(|state| state.epoch != receipt.epoch)
                {
                    return Task::none();
                }
                self.publish(
                    MessageSeverity::Information,
                    MessageSource::MainWindow,
                    history_message(&receipt),
                )
            }
            Update::HistoryUnavailable { epoch, .. }
                if self
                    .snapshot
                    .as_ref()
                    .is_none_or(|state| state.epoch != epoch) =>
            {
                Task::none()
            }
            Update::HistoryUnavailable { .. } => self.publish(
                MessageSeverity::Warning,
                MessageSource::MainWindow,
                "An earlier operation's retained outcome is unavailable. No action was repeated.",
            ),
            Update::Recovered { tag, receipt } => self.remote_admitted(tag, Ok(receipt)),
            Update::Rejected { tag, code } => {
                self.remote_admitted(tag, Err(SubmitError::NotAdmitted(code)))
            }
            Update::KnownBeforeRestart { tag, receipt } => {
                self.end_client_context(tag, true);
                let mut active = self.submitting.remove(&tag).is_some();
                if let Some(id) = self.remote_tags.remove(&tag) {
                    active |= self.pending.remove(&id).is_some();
                }
                if !active {
                    return Task::none();
                }
                self.publish(
                    MessageSeverity::Information,
                    MessageSource::MainWindow,
                    history_message(&receipt),
                )
            }
            Update::Unknown { tag, .. } => {
                self.end_client_context(tag, true);
                let mut active = self.submitting.remove(&tag).is_some();
                if let Some(id) = self.remote_tags.remove(&tag) {
                    active |= self.pending.remove(&id).is_some();
                }
                if !active {
                    return Task::none();
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
                if !self.submitting.contains_key(&sequence) && self.remote_tags.get(&sequence) != Some(&receipt.id) {return Task::none();}
                if self.snapshot.as_ref().is_none_or(|snapshot|snapshot.epoch!=receipt.epoch){return self.publish(MessageSeverity::Warning,MessageSource::MainWindow,"The server lifetime changed before the receipt arrived; no action will be repeated.");}
                if let Some(command) = self.submitting.remove(&sequence) {
                    self.remote_tags.insert(sequence, receipt.id);
                    self.pending.insert(receipt.id, command);
                } else if self.remote_tags.get(&sequence) != Some(&receipt.id) { return Task::none(); }
                if receipt.terminal { self.remote_complete(receipt) } else { Task::none() }
            }
            Err(SubmitError::Superseded) => Task::none(),
            Err(SubmitError::Ambiguous) if !self.submitting.contains_key(&sequence) && !self.remote_tags.contains_key(&sequence) => Task::none(),
            Err(SubmitError::Ambiguous)=>self.publish(MessageSeverity::Warning,MessageSource::MainWindow,"The request reply was lost. Recovering its original result by lookup; it will not be submitted again."),
            Err(_) if !self.submitting.contains_key(&sequence) && !self.remote_tags.contains_key(&sequence) => Task::none(),
            Err(error)=>{
                self.end_client_context(sequence,false);self.submitting.remove(&sequence);
                let text=match error {
                    SubmitError::NotAdmitted(code)=>workflows::failure_message(code),
                    SubmitError::Unavailable=>"The server disconnected before submission. No TV request was made.",
                    SubmitError::Busy=>"The client cannot track another request yet. No TV request was made.",
                    SubmitError::Invalid=>"The request is invalid. No TV request was made.",
                    SubmitError::Ambiguous|SubmitError::Superseded=>unreachable!("handled above"),
                };
                self.publish(MessageSeverity::Warning,MessageSource::MainWindow,text)
            }
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
        let tag = self
            .remote_tags
            .iter()
            .find_map(|(tag, id)| (*id == receipt.id).then_some(*tag));
        let context = tag.and_then(|tag| self.take_client_context(tag));
        self.remote_tags.retain(|_, id| *id != receipt.id);
        let follow = self.client_terminal(&command, context, &receipt);
        let (severity, text) = workflows::outcome_message(&receipt);
        if !self.workflows.conflict {
            self.settings_status = text.clone();
        }
        Task::batch([
            follow,
            self.publish(severity, MessageSource::MainWindow, text),
        ])
    }
}
pub(super) fn status_text(status: &Status) -> String {
    match status {
        Status::Connecting => "Attaching to control server…".into(),
        Status::Starting => "Starting control server…".into(),
        Status::Connected => {
            "Connected to control server. TV readiness is shown separately.".into()
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
            "A Stop marker is present. Resume explicitly after the previous owner settles.".into()
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
        Status::Blocked(Blocked::StopUnconfirmed) => {
            "Stop durability and settlement are unconfirmed. Check server state explicitly.".into()
        }
        Status::Blocked(Blocked::SigningRequired) => {
            "A signed application identity is required. Open the locally signed app bundle.".into()
        }
        Status::Blocked(Blocked::RecheckFailed) => {
            "The explicit server check failed. Automatic attempts are paused.".into()
        }
        Status::Exited => "Disconnected from control server.".into(),
    }
}

// History has no originating local command. Report retained facts only: never
// set confirmation handles, clear drafts or enter Select/save/Connect chains.
fn history_message(receipt: &NativeReceipt) -> String {
    match &receipt.outcome {
        Some(NativeOutcome::Discovered(_)) => {
            "An earlier discovery finished. Check a TV explicitly before Pairing.".into()
        }
        Some(NativeOutcome::Probed(_)) => {
            "An earlier TV check finished. Check again to confirm Pairing in this window.".into()
        }
        Some(NativeOutcome::Paired(_)) => "An earlier Pairing was approved and saved.".into(),
        Some(NativeOutcome::Saved(_)) => "An earlier settings change was saved.".into(),
        Some(NativeOutcome::RemoteReady) => {
            "An earlier connection became ready. Current readiness is shown separately.".into()
        }
        Some(NativeOutcome::Written) => {
            "An earlier remote command was written. Physical TV state was not observed.".into()
        }
        Some(NativeOutcome::Uncertain) => {
            "An earlier command may have been sent. It will not be repeated.".into()
        }
        Some(NativeOutcome::NotSent(_)) => "An earlier remote command was not sent.".into(),
        Some(NativeOutcome::Wake(stage)) => format!("Earlier Wake: {}", wake_message(*stage)),
        Some(NativeOutcome::Forgotten(_)) => "An earlier device cleanup completed.".into(),
        Some(NativeOutcome::PartialSetup(_)) => {
            "An earlier cleanup was incomplete. Review Settings before explicit recovery.".into()
        }
        Some(NativeOutcome::Failed(_)) => {
            "An earlier operation failed. Review the current TV and Settings before another intent."
                .into()
        }
        None => "An earlier operation is still pending.".into(),
    }
}
