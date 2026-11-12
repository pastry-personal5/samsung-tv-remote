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
        if self.intents.len() >= 64 {
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
        let context = self.capture_client_context(&command);
        self.intents.insert(
            sequence,
            LocalIntent {
                command,
                operation: None,
                context,
            },
        );
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
                    self.intents.clear();
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
                    history_severity(&receipt),
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
                if !self.end_client_context(tag, true) {
                    return Task::none();
                }
                self.publish(
                    history_severity(&receipt),
                    MessageSource::MainWindow,
                    history_message(&receipt),
                )
            }
            Update::Unknown { tag, .. } => {
                if !self.end_client_context(tag, true) {
                    return Task::none();
                }
                self.publish(MessageSeverity::Warning,MessageSource::MainWindow,"The original outcome is unavailable or belongs to an earlier server lifetime. It will not be repeated.")
            }
        }
    }
    pub(super) fn remote_admitted(
        &mut self,
        tag: u64,
        result: Result<NativeReceipt, SubmitError>,
    ) -> Task<Message> {
        let Some(intent) = self.intents.get(&tag) else {
            return Task::none();
        };
        match result {
            Ok(receipt)=>{
                if intent.operation.is_some_and(|id|id!=receipt.id)
                    || self.intents.iter().any(|(other,intent)| *other!=tag && intent.operation==Some(receipt.id)) {
                    return Task::none();
                }
                if self.snapshot.as_ref().is_none_or(|snapshot|snapshot.epoch!=receipt.epoch) {
                    return self.publish(MessageSeverity::Warning,MessageSource::MainWindow,"The server lifetime changed before the receipt arrived; no action will be repeated.");
                }
                if receipt.terminal {
                    let intent=self.intents.remove(&tag).expect("known local context");
                    let follow=self.client_terminal(&intent.command,intent.context,&receipt);
                    let (severity,text)=workflows::outcome_message(&receipt);
                    if !self.workflows.conflict {self.settings_status=text.clone();}
                    Task::batch([follow,self.publish(severity,MessageSource::MainWindow,text)])
                } else {
                    self.intents.get_mut(&tag).expect("known local context").operation=Some(receipt.id);
                    Task::none()
                }
            }
            // Known admission is monotonic even if a cancelled/late direct
            // callback disagrees. Only the tracker can settle it by lookup.
            Err(_) if intent.operation.is_some()=>Task::none(),
            Err(SubmitError::Superseded)=>Task::none(),
            Err(SubmitError::Ambiguous)=>self.publish(MessageSeverity::Warning,MessageSource::MainWindow,"The request reply was lost. Recovering its original result by lookup; it will not be submitted again."),
            Err(error)=>{
                self.end_client_context(tag,false);
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
fn history_severity(receipt: &NativeReceipt) -> MessageSeverity {
    match workflows::outcome_message(receipt).0 {
        MessageSeverity::Warning => MessageSeverity::Warning,
        _ => MessageSeverity::Information,
    }
}
fn history_message(receipt: &NativeReceipt) -> String {
    match &receipt.outcome {
        Some(NativeOutcome::Discovered(_)) => {
            "An earlier discovery finished. Check a TV explicitly before Pairing.".into()
        }
        Some(NativeOutcome::Probed(_)) => {
            "An earlier TV check finished. Check again to confirm Pairing in this window.".into()
        }
        Some(NativeOutcome::RemoteReady) => {
            "An earlier connection became ready. Current readiness is shown separately.".into()
        }
        Some(_) => {
            let (_, fact) = workflows::outcome_message(receipt);
            format!("Earlier operation: {fact}")
        }
        None => "An earlier operation is still pending.".into(),
    }
}
