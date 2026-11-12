//! Local intent/draft context. Owner receipts and atomic snapshots authorize
//! continuations; window state alone never supplies a new target or revision.
use super::*;
use crate::control_client::projection::{NativeOutcome, NativeReceipt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Selection {
    epoch: Epoch,
    target: Option<DeviceId>,
    generation: u64,
}
impl Selection {
    fn of(state: &Snapshot) -> Self {
        Self {
            epoch: state.epoch,
            target: state.control.selected_device(),
            generation: state.control.selection_generation(),
        }
    }
}
#[derive(Debug, Clone, Copy)]
struct DraftOrigin {
    selection: Selection,
    revision: Option<u64>,
}
#[derive(Debug, Clone, Copy)]
pub(super) struct IntentContext {
    selection: Selection,
    target_revision: Option<u64>,
    setup: u64,
    draft: u64,
    flow: u64,
}
enum Advance {
    Selected {
        context: IntentContext,
        target: DeviceId,
        state: TerminalState,
    },
    WakeSaved {
        context: IntentContext,
        target: DeviceId,
        configuration: WakeConfiguration,
        state: TerminalState,
    },
}
#[derive(Default)]
pub(super) struct Workflows {
    origin: Option<DraftOrigin>,
    pub conflict: bool,
    draft: u64,
    setup: u64,
    confirmation: Option<(Selection, u64)>,
    flow: u64,
    connect: Option<(Selection, u64)>,
    advance: Option<Advance>,
    pub attached: bool,
}
impl Workflows {
    pub fn cancel(&mut self) {
        self.flow = self.flow.saturating_add(1);
        self.connect = None;
        self.advance = None;
    }
}
fn revision(state: &Snapshot, target: DeviceId) -> Option<u64> {
    state
        .configuration_revisions
        .iter()
        .find(|(id, _)| *id == target)
        .map(|(_, revision)| *revision)
}
fn active_wake(state: &Snapshot) -> Option<WakeConfiguration> {
    state
        .devices
        .iter()
        .find(|device| Some(device.id) == state.control.selected_device())
        .map(|device| device.wake)
}
pub(super) fn outcome_message(receipt: &NativeReceipt) -> (MessageSeverity, String) {
    use crate::control_client::model::NotSent;
    let (severity, text) = match &receipt.outcome {
        Some(NativeOutcome::Discovered(_)) => (
            MessageSeverity::Information,
            "TV discovery finished. Check a TV before Pairing.".into(),
        ),
        Some(NativeOutcome::Probed(_)) => (
            MessageSeverity::Information,
            "TV identity check finished. Review the current identity before confirming Pairing."
                .into(),
        ),
        Some(NativeOutcome::Paired(_)) => (
            MessageSeverity::Information,
            "TV approved and saved securely.".into(),
        ),
        Some(NativeOutcome::Saved(_)) => {
            (MessageSeverity::Information, "TV settings saved.".into())
        }
        Some(NativeOutcome::RemoteReady) => (
            MessageSeverity::Information,
            "TV remote connection became ready.".into(),
        ),
        Some(NativeOutcome::Written) => (
            MessageSeverity::Debug,
            "Remote command written. TV state is not observed.".into(),
        ),
        Some(NativeOutcome::Uncertain) => (
            MessageSeverity::Warning,
            "Remote command may have been sent. It will not be replayed.".into(),
        ),
        Some(NativeOutcome::NotSent(reason)) => (
            MessageSeverity::Warning,
            match reason {
                NotSent::Cancelled => "Remote command was cancelled before sending.",
                NotSent::TransportUnavailable => {
                    "Remote command was not sent because its connection was unavailable."
                }
                NotSent::NoSelectedTv => "No TV was selected. The command was not sent.",
                NotSent::StaleSelection => "TV selection changed. The command was not sent.",
                NotSent::PairingRequired => {
                    "Pair this TV before remote control. The command was not sent."
                }
                NotSent::RemoteNotReady => {
                    "TV remote connection is not ready. The command was not sent."
                }
            }
            .into(),
        ),
        Some(NativeOutcome::Wake(stage)) => {
            (MessageSeverity::Information, wake_message(*stage).into())
        }
        Some(NativeOutcome::Forgotten(facts) | NativeOutcome::PartialSetup(facts)) => {
            let yes = |removed| if removed { "removed" } else { "not removed" };
            (
                if facts.complete() {
                    MessageSeverity::Information
                } else {
                    MessageSeverity::Warning
                },
                format!(
                    "Device cleanup: credential {}; trust {}; saved record {}.{}",
                    yes(facts.credential_removed),
                    yes(facts.trust_removed),
                    yes(facts.record_removed),
                    if facts.complete() {
                        ""
                    } else {
                        " Review Settings and explicitly retry incomplete cleanup."
                    }
                ),
            )
        }
        Some(NativeOutcome::Failed(code)) => {
            (MessageSeverity::Warning, failure_message(*code).into())
        }
        None => (
            MessageSeverity::Information,
            "Waiting for the operation's retained outcome.".into(),
        ),
    };
    (severity, text)
}
pub(super) fn failure_message(code: crate::control_client::model::FailureCode) -> &'static str {
    use crate::control_client::model::FailureCode::*;
    match code {
        Busy=>"The app is busy with another TV operation. Wait for it to finish, then choose a new command.",
        ServerStopping|ServerStopped=>"Control server is stopping or stopped. Resume explicitly after settlement.",
        StaleEpoch|UnknownAfterRestart=>"Server restarted. The earlier outcome is unavailable; no action will be repeated.",
        StaleSelection|StaleConfig=>"TV selection or settings changed. Review current settings before another intent.",
        StorageUnavailable|StorageCorrupt|StorageBlocked|TrustUnavailable|TrustCorrupt|InvalidPairingMaterial=>"Saved TV or credential access needs attention. Review Settings before explicit recovery.",
        InvalidTarget|NonLocalTarget|TargetUnresolved=>"The TV address is invalid or unavailable on the local network. Review the address and check explicitly.",
        Offline=>"The TV did not respond. Check its availability before another explicit connection attempt.",
        Timeout=>"The TV operation timed out. Its request will not be repeated automatically.",
        TlsFailed=>"The secure TV connection failed. Check the TV before another connection attempt.",
        CertificateChanged=>"TV certificate changed. Check its identity and explicitly confirm re-pairing in Settings.",
        RemoteChannelFailed=>"The TV remote channel failed. Check the current connection.",
        TvDenied=>"Pairing was denied on the TV. Check the TV again before a new approval attempt.",
        PairingRequired|PairingTokenMissing=>"This TV requires Pairing. Check its identity and approve this Mac on the TV.",
        TvProtocolFailed=>"The TV returned an invalid protocol response. No automatic retry was made.",
        Uncertain=>"The command may have been sent. It will not be repeated automatically.",
        UnsupportedAction=>"This TV does not support the requested action.",
        NoSelectedTv|NotFound=>"The selected TV or operation is no longer available. Review Settings.",
        RemoteNotReady=>"The selected TV remote connection is not ready.",
        InvalidConfiguration|InvalidLabel=>"TV settings are invalid. Review the label and Wake configuration.",
        InvalidConfirmation|ObservationExpired=>"The TV check expired or changed. Check the TV again before Pairing.",
        WakeNotConfigured=>"Configure an active Wake MAC address in Settings.",
        PartialSetup=>"TV setup or cleanup is incomplete. Review Settings before explicit recovery.",
        Cancelled=>"The TV workflow was cancelled. No follow-up will be started.",
        NetworkPermissionDenied=>"Local Network access is denied. Allow access in macOS settings, then check the TV explicitly.",
        DiscoveryUnavailable=>"TV discovery failed. Try explicit discovery again or enter a local TV address.",
        StorageUncertain|Deadline=>"The operation timed out. Storage may still be settling; wait and review the original outcome before another intent.",
        SupervisorFailed=>"The TV operation needs attention. Review saved settings and credential access.",
        OutcomeUnavailable=>"The retained outcome is unavailable. The action will not be repeated.",
        Unauthenticated|Forbidden=>"Server access is denied. Resolve credentials and recheck access explicitly.",
        UnsafeLifecycleMetadata|StopPersistenceFailed=>"Server management could not safely complete. Review its state before another management request.",
        OperationKeyConflict|InvalidRequest|InvalidId|AmbiguousFraming|HttpVersionUnsupported|InvalidBody|TrailersUnsupported|UnexpectedBody|
        ContentEncodingUnsupported|ContentTypeUnsupported|ResponseTooLarge|InputTooLarge=>"The request or response could not be safely processed. No automatic retry was made.",
        BodyTimeout|RequestTimeout=>"The request timed out. Its retained outcome must be reviewed; it will not be repeated automatically.",
    }
}
enum Compatibility {
    Waiting,
    Current,
    Conflict,
}
fn compatibility(
    state: &Snapshot,
    epoch: Epoch,
    target: DeviceId,
    terminal: TerminalState,
) -> Compatibility {
    if state.epoch != epoch
        || state.startup != StartupState::Ready
        || state.admission_closed
        || state.recovery.is_some()
        || state.pairing_recovery.is_some()
        || terminal.selected != Some(target)
    {
        return Compatibility::Conflict;
    }
    let generation = state.control.selection_generation();
    if generation < terminal.generation {
        return Compatibility::Waiting;
    }
    if generation != terminal.generation || state.control.selected_device() != Some(target) {
        return Compatibility::Conflict;
    }
    match (revision(state, target), terminal.configuration_revision) {
        (Some(current), Some(expected)) if current < expected => Compatibility::Waiting,
        (Some(current), Some(expected)) if current == expected => Compatibility::Current,
        _ => Compatibility::Conflict,
    }
}
impl App {
    pub(super) fn interrupt_client_flow(&mut self) {
        self.workflows.cancel();
        self.workflows.setup = self.workflows.setup.saturating_add(1);
        self.consume_client_confirmation();
        self.wake_save_requested = false;
    }
    pub(super) fn reset_client_workflows(&mut self) {
        self.workflows.cancel();
        self.workflows.confirmation = None;
        self.workflows.setup = self.workflows.setup.saturating_add(1);
        self.observed = None;
        self.candidate_handles.clear();
        self.wake_save_requested = false;
        if self.wake_dirty {
            self.workflows.conflict = true;
        }
    }
    pub(super) fn consume_client_confirmation(&mut self) {
        self.workflows.confirmation = None;
        self.observed = None;
    }
    pub(super) fn prepare_client_intent(&mut self, message: &Message) {
        if matches!(
            message,
            Message::SelectSaved(_)
                | Message::ConnectSelected
                | Message::Wake
                | Message::CancelWake
                | Message::AttemptRemoteAction(_)
                | Message::PowerToggle(_)
                | Message::ConfirmAndPair
                | Message::ConfirmAndRepair
                | Message::ForgetSelected
        ) {
            self.workflows.cancel();
        }
        if matches!(
            message,
            Message::DiscoverTv
                | Message::ProbeTv
                | Message::TvAddressChanged(_)
                | Message::UseCandidate(_)
                | Message::ForgetSelected
                | Message::SelectSaved(_)
        ) {
            self.workflows.setup = self.workflows.setup.saturating_add(1);
            self.workflows.confirmation = None;
            self.observed = None;
        }
    }
    pub(super) fn capture_client_context(&self, command: &Command) -> Option<IntentContext> {
        let state = self.snapshot.as_ref()?;
        let target = command.metadata(state.epoch).target;
        Some(IntentContext {
            selection: Selection::of(state),
            target_revision: target.and_then(|target| revision(state, target)),
            setup: self.workflows.setup,
            draft: self.workflows.draft,
            flow: self.workflows.flow,
        })
    }
    pub(super) fn begin_wake_edit(&mut self) {
        if !self.wake_dirty {
            self.workflows.origin = self.snapshot.as_ref().map(|state| DraftOrigin {
                selection: Selection::of(state),
                revision: state
                    .control
                    .selected_device()
                    .and_then(|id| revision(state, id)),
            });
            self.workflows.conflict = false;
        }
        self.workflows.draft = self.workflows.draft.saturating_add(1);
    }
    pub(super) fn reload_client_wake(&mut self) -> Task<Message> {
        if self
            .remote
            .as_ref()
            .is_none_or(|client| !client.connected())
            || self.snapshot.as_ref().is_none_or(|state| {
                state.storage_pending
                    || state.admission_closed
                    || state.startup != StartupState::Ready
            })
        {
            return self.publish(
                MessageSeverity::Warning,
                MessageSource::MainWindow,
                "Wait for current server settings before discarding the Wake draft.",
            );
        }
        self.workflows.cancel();
        let wake = self
            .snapshot
            .as_ref()
            .and_then(active_wake)
            .unwrap_or_default();
        self.load_wake_configuration(&wake);
        self.wake_dirty = false;
        self.wake_save_requested = false;
        self.workflows.origin = None;
        self.workflows.conflict = false;
        self.workflows.draft = self.workflows.draft.saturating_add(1);
        self.publish(
            MessageSeverity::Information,
            MessageSource::MainWindow,
            "Wake draft reloaded from the current TV settings.",
        )
    }
    pub(super) fn client_confirmation_current(&self) -> bool {
        let Some(state) = &self.snapshot else {
            return false;
        };
        let current =
            self.workflows.confirmation == Some((Selection::of(state), self.workflows.setup));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        current
            && self
                .observed
                .as_ref()
                .is_some_and(|observed| u128::from(observed.expires_at.get()) > now)
    }
    pub(super) fn stop_client_flow(
        &mut self,
        context: Option<IntentContext>,
        configuration_uncertain: bool,
    ) {
        if context.is_some_and(|context| context.flow == self.workflows.flow) {
            self.workflows.cancel();
            self.wake_save_requested = false;
        }
        if configuration_uncertain
            && self.wake_dirty
            && context.is_some_and(|context| {
                self.workflows
                    .origin
                    .is_some_and(|origin| origin.selection == context.selection)
            })
        {
            self.workflows.conflict = true;
            self.wake_save_requested = false;
        }
    }
    pub(super) fn end_client_context(&mut self, tag: u64, uncertain: bool) -> bool {
        let Some(intent) = self.intents.remove(&tag) else {
            return false;
        };
        if matches!(
            intent.command,
            Command::Select { .. } | Command::SetWake { .. }
        ) {
            self.stop_client_flow(
                intent.context,
                uncertain && matches!(intent.command, Command::SetWake { .. }),
            );
        }
        true
    }
    pub(super) fn apply_client_snapshot(&mut self, state: Snapshot) -> Task<Message> {
        if self
            .snapshot
            .as_ref()
            .is_some_and(|old| old.epoch == state.epoch && old.revision > state.revision)
        {
            return Task::none();
        }
        let changed = self
            .snapshot
            .as_ref()
            .is_some_and(|old| Selection::of(old) != Selection::of(&state));
        if changed {
            let expected_local_select=self.intents.values().map(|intent| &intent.command).any(|command|
                matches!(command,Command::Select {target,generation} if Some(*target)==state.control.selected_device()
                    && generation.checked_add(1)==Some(state.control.selection_generation())))
                || matches!(&self.workflows.advance,Some(Advance::Selected {target,state:terminal,..})
                    if Some(*target)==state.control.selected_device() && terminal.generation==state.control.selection_generation());
            if !expected_local_select {
                self.workflows.cancel();
                self.wake_save_requested = false;
            }
            self.observed = None;
            self.workflows.confirmation = None;
            self.workflows.setup = self.workflows.setup.saturating_add(1);
            self.candidate_handles.clear();
        }
        if self.wake_dirty {
            if let Some(origin) = self.workflows.origin {
                let current = Selection::of(&state);
                let config = current.target.and_then(|target| revision(&state, target));
                if origin.selection != current || origin.revision != config {
                    // An observed write may precede its receipt. Hold its draft
                    // until the matching terminal receipt proves association.
                    let own_write=origin.selection==current && self.intents.values().map(|intent| &intent.command).any(|command| {
                        matches!(command,Command::SetWake {target,generation,revision:base,configuration}
                            if Some(*target)==current.target && *generation==current.generation
                            && base.checked_add(1)==config && active_wake(&state)==Some(*configuration))
                    });
                    if !own_write {
                        self.workflows.conflict = true;
                        self.wake_save_requested = false;
                    }
                }
            } else if state.control.selected_device().is_some() {
                self.workflows.conflict = true;
                self.wake_save_requested = false;
            }
        } else if let Some(wake) = active_wake(&state) {
            self.load_wake_configuration(&wake);
        }
        self.view_model.update_control_state(&state.control);
        let launch = !self.launch_connected && state.startup == StartupState::Ready;
        let eligible = launch
            && state.control.selected_device().is_some()
            && state.control.connection_state()
                == crate::application::ConnectionState::NotConnected
            && state.control.pairing_state() != crate::application::PairingState::InProgress
            && !state.admission_closed
            && !state.storage_pending
            && !state.pairing_pending
            && state.recovery.is_none()
            && state.pairing_recovery.is_none()
            && state.wake_operation.is_none();
        self.snapshot = Some(state);
        self.refresh_client_controls();
        let mut tasks = vec![];
        if launch {
            self.launch_connected = true;
        }
        if eligible {
            let (target, generation) = self.selected().expect("eligible selection");
            tasks.push(self.command(Command::Connect {
                target,
                generation,
                only_if_idle: true,
            }));
        }
        tasks.push(self.advance_client_flow());
        if self.wake_save_requested {
            tasks.push(self.save_client_wake());
        }
        if self.workflows.conflict {
            self.settings_status="Wake draft conflicts with the current TV or settings. Reload current settings before saving.".into();
        }
        Task::batch(tasks)
    }
    pub(super) fn save_client_wake(&mut self) -> Task<Message> {
        if !self.wake_dirty
            || !self.wake_save_requested
            || self.workflows.conflict
            || self
                .snapshot
                .as_ref()
                .is_none_or(|state| state.storage_pending || state.admission_closed)
            || self
                .intents
                .values()
                .map(|intent| &intent.command)
                .any(|command| matches!(command, Command::SetWake { .. }))
        {
            return Task::none();
        }
        let (Some(configuration), Some(state), Some(origin)) = (
            self.valid_wake_draft(),
            self.snapshot.as_ref(),
            self.workflows.origin,
        ) else {
            return Task::none();
        };
        let current = Selection::of(state);
        if origin.selection != current
            || current.target.is_none()
            || origin.revision != current.target.and_then(|target| revision(state, target))
        {
            return Task::none();
        }
        let (target, generation) = (current.target.expect("selected draft"), current.generation);
        let Some(revision) = origin.revision else {
            return Task::none();
        };
        self.wake_save_requested = false;
        self.command(Command::SetWake {
            target,
            generation,
            revision,
            configuration,
        })
    }
    pub(super) fn client_terminal(
        &mut self,
        command: &Command,
        context: Option<IntentContext>,
        receipt: &NativeReceipt,
    ) -> Task<Message> {
        let Some(context) = context else {
            return Task::none();
        };
        match (&receipt.outcome, command) {
            (Some(NativeOutcome::Discovered(candidates)), Command::Discover)
                if context.setup == self.workflows.setup =>
            {
                self.candidates = candidates
                    .iter()
                    .map(|candidate| candidate.host.clone())
                    .collect();
                self.candidate_handles = candidates.clone();
            }
            (
                Some(NativeOutcome::Probed(observed)),
                Command::Probe(_) | Command::ProbeCandidate { .. },
            ) if context.setup == self.workflows.setup
                && self
                    .snapshot
                    .as_ref()
                    .is_some_and(|state| Selection::of(state) == context.selection)
                && observed.host.as_str() == self.tv_address =>
            {
                self.observed = Some(*observed.clone());
                self.workflows.confirmation = Some((context.selection, context.setup));
            }
            (Some(NativeOutcome::Paired(_)), Command::Pair { .. })
                if context.setup == self.workflows.setup =>
            {
                self.observed = None;
                self.workflows.confirmation = None;
            }
            (Some(NativeOutcome::Saved(saved)), Command::Select { target, generation })
                if context.flow == self.workflows.flow =>
            {
                if let Some(state) = receipt.terminal_state.filter(|state| {
                    state.selected == Some(*target)
                        && *saved == *target
                        && generation.checked_add(1) == Some(state.generation)
                        && state.configuration_revision == context.target_revision
                }) {
                    self.workflows.advance = Some(Advance::Selected {
                        context,
                        target: *target,
                        state,
                    });
                    return self.advance_client_flow();
                }
                self.stop_client_flow(Some(context), false);
                return self.publish(MessageSeverity::Warning,MessageSource::MainWindow,"TV selected, but its settings changed. Review the current TV and connect explicitly.");
            }
            (
                Some(NativeOutcome::Saved(saved)),
                Command::SetWake {
                    target,
                    generation,
                    revision,
                    configuration,
                },
            ) if context.flow == self.workflows.flow => {
                if let Some(state) = receipt.terminal_state.filter(|state| {
                    state.selected == Some(*target)
                        && *saved == *target
                        && state.generation == *generation
                        && revision.checked_add(1) == state.configuration_revision
                }) {
                    self.workflows.advance = Some(Advance::WakeSaved {
                        context,
                        target: *target,
                        configuration: *configuration,
                        state,
                    });
                    return self.advance_client_flow();
                }
                self.stop_client_flow(Some(context), true);
                return self.publish(MessageSeverity::Warning,MessageSource::MainWindow,"Settings were saved, but the current TV or configuration changed. Your Wake draft is retained.");
            }
            (
                Some(NativeOutcome::Failed(code)),
                Command::SetWake { .. } | Command::Select { .. },
            ) => {
                let uncertain = matches!(command, Command::SetWake { .. })
                    && !matches!(
                        code,
                        crate::control_client::model::FailureCode::Busy
                            | crate::control_client::model::FailureCode::InvalidConfiguration
                    );
                self.stop_client_flow(Some(context), uncertain)
            }
            (Some(NativeOutcome::Saved(_)), Command::SetWake { .. }) => {
                self.stop_client_flow(Some(context), self.wake_dirty)
            }
            _ => (),
        }
        Task::none()
    }
    fn advance_client_flow(&mut self) -> Task<Message> {
        let Some(advance) = self.workflows.advance.take() else {
            return Task::none();
        };
        let (context, target, terminal) = match &advance {
            Advance::Selected {
                context,
                target,
                state,
            }
            | Advance::WakeSaved {
                context,
                target,
                state,
                ..
            } => (*context, *target, *state),
        };
        if context.flow != self.workflows.flow {
            return Task::none();
        }
        let Some(state) = &self.snapshot else {
            self.workflows.advance = Some(advance);
            return Task::none();
        };
        match compatibility(state, context.selection.epoch, target, terminal) {
            Compatibility::Waiting => {
                self.workflows.advance = Some(advance);
                return Task::none();
            }
            Compatibility::Conflict => {
                self.stop_client_flow(Some(context), self.wake_dirty);
                return self.publish(MessageSeverity::Warning,MessageSource::MainWindow,"Selection or Wake settings changed. The next step stopped; review the current TV before continuing.");
            }
            Compatibility::Current => (),
        }
        let selection = Selection::of(state);
        let config = terminal.configuration_revision;
        match advance {
            Advance::Selected { .. } => {
                if self.wake_dirty {
                    let bindable = self.workflows.origin.is_some_and(|origin| {
                        origin.selection == context.selection
                            && (origin.selection.target.is_none()
                                || origin.selection.target == Some(target)
                                    && origin.revision == context.target_revision)
                    }) || self.workflows.origin.is_none()
                        && context.selection.target.is_none();
                    if !bindable {
                        self.stop_client_flow(Some(context), true);
                        return self.publish(MessageSeverity::Warning,MessageSource::MainWindow,"The unsaved Wake draft belongs to an earlier TV or configuration. Reload current settings before saving or connecting.");
                    }
                    self.workflows.origin = Some(DraftOrigin {
                        selection,
                        revision: config,
                    });
                    self.workflows.conflict = false;
                }
                self.workflows.connect = Some((selection, self.workflows.flow));
            }
            Advance::WakeSaved { configuration, .. } => {
                if active_wake(state) != Some(configuration) {
                    self.stop_client_flow(Some(context), true);
                    return self.publish(MessageSeverity::Warning,MessageSource::MainWindow,"Current Wake settings differ from the saved result. The follow-up stopped; reload current settings before saving again.");
                }
                self.workflows.origin = Some(DraftOrigin {
                    selection,
                    revision: config,
                });
                self.workflows.conflict = false;
                self.wake_dirty = context.draft != self.workflows.draft
                    || self.valid_wake_draft() != Some(configuration);
                if self.workflows.connect.is_some() {
                    self.workflows.connect = Some((selection, self.workflows.flow));
                }
            }
        }
        if self.wake_dirty {
            self.wake_save_requested = true;
            return self.save_client_wake();
        }
        self.wake_save_requested = false;
        if self.workflows.connect.take() == Some((selection, self.workflows.flow)) {
            // Automatic continuation must not replace a session made ready by
            // another client between observation and owner admission.
            return self.command(Command::Connect {
                target,
                generation: selection.generation,
                only_if_idle: true,
            });
        }
        Task::none()
    }
}
