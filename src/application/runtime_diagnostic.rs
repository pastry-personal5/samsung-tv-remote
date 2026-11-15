//! Private diagnostic lease policy. The runtime remains the sole TV owner.
use super::*;
use crate::application::diagnostic::*;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct Lease {
    id: String,
    client: ClientId,
    authorization: AdmissionAuthorization,
    context: (DeviceId, u64, u64),
    alive: Arc<AtomicBool>,
    deadline: Instant,
    commands: mpsc::Sender<SessionRequest>,
    task: JoinHandle<()>,
    status: DiagnosticStatus,
    catalog: Vec<DiagnosticApp>,
    aliases: Option<(usize, usize)>,
    alias_revision: u64,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        self.task.abort();
    }
}
impl Lease {
    fn pending(&mut self) -> Option<&mut ExperimentRecord> {
        self.status.records.last_mut().filter(|r| !r.complete)
    }
    fn current(&self, client: ClientId, id: &str) -> bool {
        self.client == client && self.id == id
    }
}
impl Owner {
    pub(super) fn diagnostic_message(&mut self, client: ClientId, message: DiagnosticMessage) {
        if self.check_diagnostic() {
            self.publish(None);
        }
        match message {
            DiagnosticMessage::Open {
                epoch,
                target,
                generation,
                authorization,
                reply,
            } => {
                let result = authorization
                    .at_admission(|| {
                        self.open_diagnostic(
                            client,
                            epoch,
                            target,
                            generation,
                            authorization.clone(),
                        )
                    })
                    .unwrap_or(Err(ServiceError::Unauthorized));
                let _ = reply.send(result);
            }
            DiagnosticMessage::Intent {
                lease,
                key,
                experiment,
                authorization,
                reply,
            } => {
                let result = authorization
                    .at_admission(|| self.diagnostic_intent(client, &lease, key, experiment))
                    .unwrap_or(Err(ServiceError::Unauthorized));
                let _ = reply.send(result);
            }
            DiagnosticMessage::Choose { lease, a, b, reply } => {
                let result = self
                    .diagnostic
                    .as_mut()
                    .filter(|d| d.current(client, &lease) && !d.status.closed)
                    .ok_or(ServiceError::Closed)
                    .and_then(|d| {
                        if d.pending().is_some() {
                            return Err(ServiceError::Busy);
                        }
                        if a == b || a >= d.catalog.len() || b >= d.catalog.len() {
                            return Err(ServiceError::InvalidPayload);
                        }
                        if d.aliases != Some((a, b)) {
                            d.alias_revision =
                                d.alias_revision.checked_add(1).ok_or(ServiceError::Busy)?;
                        }
                        d.aliases = Some((a, b));
                        d.status.aliases_set = true;
                        Ok(d.status.clone())
                    });
                let _ = reply.send(result);
            }
            DiagnosticMessage::Status { lease, reply } => {
                let result = self
                    .diagnostic
                    .as_ref()
                    .filter(|d| d.current(client, &lease))
                    .map(|d| d.status.clone())
                    .ok_or(ServiceError::Closed);
                let _ = reply.send(result);
            }
        }
    }
    fn open_diagnostic(
        &mut self,
        client: ClientId,
        epoch: Epoch,
        target: DeviceId,
        generation: u64,
        authorization: AdmissionAuthorization,
    ) -> Result<LeaseStream, ServiceError> {
        if epoch != self.snapshot.epoch {
            return Err(ServiceError::StaleEpoch);
        }
        if self.snapshot.admission_closed || self.snapshot.startup != StartupState::Ready {
            return Err(ServiceError::Closed);
        }
        if self.diagnostic_busy()
            || self.storage.is_some()
            || !self.workers.is_empty()
            || !self.remotes.is_empty()
            || !self.pairs.is_empty()
            || !self.connections.is_empty()
            || !self.wakes.is_empty()
        {
            return Err(ServiceError::Busy);
        }
        let context = self
            .coordinator
            .evidence_session()
            .filter(|(tv, g, _)| *tv == target && *g == generation)
            .ok_or(ServiceError::InvalidPayload)?;
        let DiagnosticPort {
            commands,
            mut events,
        } = self
            .coordinator
            .take_diagnostic()
            .ok_or(ServiceError::InvalidPayload)?;
        let id = uuid::Uuid::new_v4().to_string();
        let alive = Arc::new(AtomicBool::new(true));
        let live = alive.clone();
        let sender = self.session_sender.clone();
        let task = tokio::spawn(async move {
            while let Some(observation) = events.recv().await {
                if sender
                    .try_send((
                        context.1,
                        context.2,
                        OwnerSessionEvent::Diagnostic(observation),
                    ))
                    .is_err()
                {
                    live.store(false, Ordering::SeqCst);
                    return;
                }
            }
            live.store(false, Ordering::SeqCst);
        });
        self.diagnostic = Some(Lease {
            id: id.clone(),
            client,
            authorization,
            context,
            alive: alive.clone(),
            deadline: Instant::now() + Duration::from_secs(1800),
            commands,
            task,
            status: DiagnosticStatus {
                lease: id.clone(),
                closed: false,
                catalog_quarantined: false,
                launch_quarantined: false,
                records: Vec::new(),
                apps: Vec::new(),
                aliases_set: false,
            },
            catalog: Vec::new(),
            aliases: None,
            alias_revision: 0,
        });
        Ok(LeaseStream { id, alive })
    }
    fn diagnostic_intent(
        &mut self,
        client: ClientId,
        lease: &str,
        key: String,
        experiment: Experiment,
    ) -> Result<DiagnosticStatus, ServiceError> {
        let d = self
            .diagnostic
            .as_mut()
            .filter(|d| d.current(client, lease))
            .ok_or(ServiceError::Closed)?;
        // Recover an admitted intent before live-state/capacity checks, without replay.
        if let Some(record) = d.status.records.iter().find(|r| r.intent == key) {
            let alias_revision = matches!(experiment, Experiment::LaunchA | Experiment::LaunchB)
                .then(|| d.alias_revision.to_string());
            return if record.experiment == experiment && record.alias_revision == alias_revision {
                Ok(d.status.clone())
            } else {
                Err(ServiceError::KeyConflict)
            };
        }
        if d.status.closed || !d.alive.load(Ordering::SeqCst) {
            return Err(ServiceError::Closed);
        }
        if uuid::Uuid::parse_str(&key)
            .ok()
            .is_none_or(|u| u.to_string() != key)
        {
            return Err(ServiceError::InvalidKey);
        }
        if d.pending().is_some() || d.status.records.len() == MAX_INTENTS {
            return Err(ServiceError::Busy);
        }
        let wire = match experiment {
            Experiment::Catalog if !d.status.catalog_quarantined => {
                d.alias_revision.checked_add(1).ok_or(ServiceError::Busy)?;
                WireExperiment::Catalog
            }
            Experiment::LaunchA | Experiment::LaunchB if !d.status.launch_quarantined => {
                let (a, b) = d.aliases.ok_or(ServiceError::InvalidPayload)?;
                WireExperiment::Launch(
                    d.catalog
                        .get(if experiment == Experiment::LaunchA {
                            a
                        } else {
                            b
                        })
                        .ok_or(ServiceError::InvalidPayload)?
                        .clone(),
                )
            }
            Experiment::Ime => WireExperiment::Ime,
            e => WireExperiment::Source(e.source().ok_or(ServiceError::InvalidPayload)?),
        };
        let sequence = d.status.records.len() as u64 + 1;
        let catalog = matches!(wire, WireExperiment::Catalog);
        // The only transport handoff is finite and guarded by the stream's live flag.
        d.commands
            .try_send(SessionRequest {
                sequence,
                experiment: wire,
                alive: d.alive.clone(),
            })
            .map_err(|_| ServiceError::Busy)?;
        // A rejected handoff must not discard the current session's choices.
        if catalog {
            d.alias_revision += 1; // Overflow was rejected before the handoff.
            d.catalog.clear();
            d.status.apps.clear();
            d.aliases = None;
            d.status.aliases_set = false;
        }
        let mut record = ExperimentRecord::new(sequence, key, experiment);
        record.alias_revision = matches!(experiment, Experiment::LaunchA | Experiment::LaunchB)
            .then(|| d.alias_revision.to_string());
        d.status.records.push(record);
        Ok(d.status.clone())
    }
    pub(super) fn diagnostic_busy(&self) -> bool {
        self.diagnostic.as_ref().is_some_and(|d| !d.status.closed)
    }
    pub(super) fn check_diagnostic(&mut self) -> bool {
        let close = self.diagnostic.as_ref().is_some_and(|d| {
            !d.status.closed
                && (!d.alive.load(Ordering::SeqCst)
                    || !d.authorization.is_active()
                    || Instant::now() >= d.deadline
                    || self.snapshot.admission_closed
                    || self.snapshot.startup != StartupState::Ready
                    || self.coordinator.evidence_session() != Some(d.context))
        });
        if !close {
            return false;
        }
        let d = self.diagnostic.as_mut().expect("lease");
        d.alive.store(false, Ordering::SeqCst);
        d.task.abort();
        d.status.closed = true;
        d.status.apps.clear();
        d.status.aliases_set = false;
        d.catalog.clear();
        d.aliases = None;
        if let Some(record) = d.pending() {
            // Missing completion cannot prove an admitted socket write was absent.
            if record.write.is_none() {
                record.write = Some(WriteOutcome::Uncertain);
            }
            record.finish();
            record.inconclusive = true;
        }
        if self.coordinator.evidence_session() == Some(d.context) {
            self.coordinator.stop_active();
        }
        true
    }
    pub(super) fn diagnostic_observation(
        &mut self,
        generation: u64,
        session: u64,
        observation: SessionObservation,
    ) {
        if self.check_diagnostic() {
            self.publish(None);
        }
        let Some(d) = self
            .diagnostic
            .as_mut()
            .filter(|d| !d.status.closed && d.context.1 == generation && d.context.2 == session)
        else {
            return;
        };
        let Some(record) = d.pending() else { return };
        let expected = record.sequence.parse::<u64>().expect("finite sequence");
        match observation {
            SessionObservation::Write(seq, outcome) if seq == expected => {
                record.write = Some(outcome);
            }
            SessionObservation::End(seq) if seq == expected => {
                record.finish();
                record.inconclusive = match record.experiment {
                    Experiment::Catalog => {
                        record.catalog_events != 1 || record.catalog_count.is_none()
                    }
                    Experiment::LaunchA | Experiment::LaunchB => record.launch_events != 1,
                    Experiment::Ime => record.ime_starts == 0 || record.ime_ends == 0,
                    _ => true, // Physical source state always needs the operator.
                } || record.limit_hit
                    || record.malformed_events > 0;
                let catalog_timeout =
                    record.experiment == Experiment::Catalog && record.catalog_events == 0;
                let catalog_invalid =
                    record.experiment == Experiment::Catalog && record.inconclusive;
                let launch_timeout =
                    matches!(record.experiment, Experiment::LaunchA | Experiment::LaunchB)
                        && record.launch_events == 0;
                d.status.catalog_quarantined |= catalog_timeout;
                d.status.launch_quarantined |= launch_timeout;
                if catalog_invalid {
                    d.catalog.clear();
                    d.status.apps.clear();
                    d.aliases = None;
                    d.status.aliases_set = false;
                }
            }
            SessionObservation::Frame {
                sequence,
                bytes,
                event,
            } if sequence == expected => {
                if record.first_frame_at_unix_ms.is_none() {
                    record.first_frame_at_unix_ms = wall_time();
                }
                record.frames = record.frames.saturating_add(1);
                record.max_frame_bytes = record.max_frame_bytes.max(bytes);
                match event {
                    CandidateEvent::Catalog(apps) if record.experiment == Experiment::Catalog => {
                        record.catalog_events += 1;
                        record.catalog_count = Some(apps.len());
                        record.catalog_unicode_names =
                            Some(apps.iter().map(|a| a.unicode_name).collect());
                        record.association = Association::AmbiguousSingleFlight;
                        if record.catalog_events == 1 {
                            d.status.apps = apps
                                .iter()
                                .enumerate()
                                .map(|(choice, app)| AppChoice {
                                    choice,
                                    name: app.name.clone(),
                                })
                                .collect();
                            d.catalog = apps;
                        } else {
                            d.catalog.clear();
                            d.status.apps.clear();
                        }
                    }
                    CandidateEvent::Launch(shape)
                        if matches!(
                            record.experiment,
                            Experiment::LaunchA | Experiment::LaunchB
                        ) =>
                    {
                        record.launch_events += 1;
                        record.launch_shape = Some(shape);
                        record.association = Association::AmbiguousSingleFlight;
                    }
                    CandidateEvent::ImeStart(shape) if record.experiment == Experiment::Ime => {
                        record.ime_starts += 1;
                        record.ime_start_shape = Some(shape);
                        record.association = Association::PassiveEvent;
                    }
                    CandidateEvent::ImeEnd(shape) if record.experiment == Experiment::Ime => {
                        record.ime_ends += 1;
                        record.ime_end_shape = Some(shape);
                        record.association = Association::PassiveEvent;
                    }
                    CandidateEvent::Malformed => {
                        record.malformed_events += 1;
                    }
                    CandidateEvent::Limit => {
                        record.limit_hit = true;
                        d.alive.store(false, Ordering::SeqCst);
                    }
                    _ => {
                        record.unknown_events += 1;
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::certificate_trust::CertificatePin;
    use crate::application::credential_store::PairingToken;
    use crate::application::tv_session::*;
    use crate::application::wake::WakeConfiguration;
    use std::sync::atomic::AtomicUsize;
    struct FakeControl {
        port: Option<DiagnosticPort>,
        aborts: Arc<AtomicUsize>,
    }
    impl TvSessionControl for FakeControl {
        fn take_diagnostic(&mut self) -> Option<DiagnosticPort> {
            self.port.take()
        }
        fn try_click(&self, _: RequestId, _: crate::domain::RemoteAction) -> bool {
            panic!("ordinary write escaped lease policy")
        }
        fn abort(&mut self) {
            self.aborts.fetch_add(1, Ordering::SeqCst);
        }
    }
    struct FakeConnection(FakeControl);
    impl TvConnection for FakeConnection {
        fn await_authorized(
            &mut self,
            _: bool,
        ) -> SessionFuture<'_, Result<Option<PairingToken>, TvSessionError>> {
            Box::pin(async { Ok(None) })
        }
        fn start(self: Box<Self>) -> ActiveTvSession {
            ActiveTvSession {
                control: Box::new(self.0),
                events: Box::pin(futures_util::stream::pending()),
            }
        }
    }
    fn owner() -> (
        Owner,
        mpsc::Receiver<SessionRequest>,
        mpsc::Sender<SessionObservation>,
        Arc<AtomicUsize>,
    ) {
        let mut owner = Owner::new(AppServices::without_adapters(), RuntimeLimits::default());
        let tv = SavedDevice {
            id: DeviceId::new(1),
            label: "Synthetic".into(),
            host: super::super::super::tv_address::TvHost::parse("synthetic.invalid").unwrap(),
            wake: WakeConfiguration::default(),
        };
        owner.coordinator.saved_devices = vec![tv.clone()];
        owner.coordinator.app_state.select_device(tv.display());
        owner
            .coordinator
            .app_state
            .set_pairing_state(1, crate::application::PairingState::Ready);
        owner
            .coordinator
            .app_state
            .set_connection_state(1, crate::application::ConnectionState::Ready);
        let (commands, requests) = mpsc::channel(1);
        let (events, receiver) = mpsc::channel(16);
        let aborts = Arc::new(AtomicUsize::new(0));
        let _ = owner.coordinator.install_session(
            1,
            Box::new(FakeConnection(FakeControl {
                port: Some(DiagnosticPort {
                    commands,
                    events: receiver,
                }),
                aborts: aborts.clone(),
            })),
        );
        owner.snapshot.startup = StartupState::Ready;
        owner.restoring = false;
        let _ = CertificatePin::from_der(b"synthetic trusted session");
        (owner, requests, events, aborts)
    }
    fn open(owner: &mut Owner) -> LeaseStream {
        owner
            .open_diagnostic(
                ClientId(1),
                owner.snapshot.epoch,
                DeviceId::new(1),
                1,
                AdmissionAuthorization::default(),
            )
            .unwrap()
    }
    fn key() -> String {
        uuid::Uuid::new_v4().to_string()
    }
    fn event(owner: &mut Owner, event: SessionObservation) {
        owner.diagnostic_observation(1, 1, event);
    }
    fn catalog(owner: &mut Owner, requests: &mut mpsc::Receiver<SessionRequest>, lease: &str) {
        owner
            .diagnostic_intent(ClientId(1), lease, key(), Experiment::Catalog)
            .unwrap();
        assert!(matches!(
            requests.try_recv().unwrap().experiment,
            WireExperiment::Catalog
        ));
        event(owner, SessionObservation::Write(1, WriteOutcome::Written));
        event(
            owner,
            SessionObservation::Frame {
                sequence: 1,
                bytes: 200,
                event: CandidateEvent::Catalog(vec![
                    DiagnosticApp::new("first", "First").unwrap(),
                    DiagnosticApp::new("second", "Second").unwrap(),
                ]),
            },
        );
        event(owner, SessionObservation::End(1));
        let d = owner.diagnostic.as_mut().unwrap();
        d.aliases = Some((0, 1));
        d.status.aliases_set = true;
    }
    #[tokio::test]
    async fn competing_controllers_duplicate_intents_and_ordinary_mutations_cannot_duplicate_effects(
    ) {
        let (mut owner, mut requests, _events, _aborts) = owner();
        let lease = open(&mut owner);
        assert!(matches!(
            owner.open_diagnostic(
                ClientId(2),
                owner.snapshot.epoch,
                DeviceId::new(1),
                1,
                AdmissionAuthorization::default()
            ),
            Err(ServiceError::Busy)
        ));
        let intent = key();
        owner
            .diagnostic_intent(ClientId(1), &lease.id, intent.clone(), Experiment::Hdmi1)
            .unwrap();
        owner
            .diagnostic_intent(ClientId(1), &lease.id, intent.clone(), Experiment::Hdmi1)
            .unwrap();
        assert!(matches!(
            requests.try_recv().unwrap().experiment,
            WireExperiment::Source(SourceKey::Hdmi1)
        ));
        assert!(requests.try_recv().is_err());
        assert!(matches!(
            owner.diagnostic_intent(ClientId(1), &lease.id, intent.clone(), Experiment::Hdmi2),
            Err(ServiceError::KeyConflict)
        ));
        assert!(matches!(
            owner.diagnostic_intent(ClientId(2), &lease.id, key(), Experiment::Hdmi1),
            Err(ServiceError::Closed)
        ));
        let receipt = owner
            .submit(
                ClientId(2),
                Request {
                    epoch: owner.snapshot.epoch,
                    key: key(),
                    command: Command::Connect {
                        target: DeviceId::new(1),
                        generation: 1,
                        only_if_idle: false,
                    },
                },
            )
            .unwrap();
        assert!(matches!(
            receipt.outcome.as_ref(),
            Some(Outcome::Rejected(Rejection::Busy))
        ));
        assert!(owner.coordinator.evidence_session().is_some());
    }
    #[tokio::test]
    async fn controller_loss_revokes_unsent_permit_and_retains_uncertainty_without_replay() {
        let (mut owner, mut requests, _events, aborts) = owner();
        let lease = open(&mut owner);
        let intent = key();
        owner
            .diagnostic_intent(ClientId(1), &lease.id, intent.clone(), Experiment::Hdmi1)
            .unwrap();
        let pending = requests.try_recv().unwrap();
        drop(lease);
        assert!(!pending.alive.load(Ordering::SeqCst));
        assert!(owner.check_diagnostic());
        assert_eq!(aborts.load(Ordering::SeqCst), 1);
        let id = owner.diagnostic.as_ref().unwrap().id.clone();
        let status = owner
            .diagnostic_intent(ClientId(1), &id, intent, Experiment::Hdmi1)
            .unwrap();
        assert!(status.closed);
        assert_eq!(status.records[0].write, Some(WriteOutcome::Uncertain));
        assert!(requests.try_recv().is_err());
    }
    #[tokio::test]
    async fn catalog_aliases_are_session_scoped_and_timeout_quarantines_late_replies() {
        let (mut owner, mut requests, _events, _aborts) = owner();
        let lease = open(&mut owner);
        assert!(matches!(
            owner.diagnostic_intent(ClientId(1), &lease.id, key(), Experiment::LaunchA),
            Err(ServiceError::InvalidPayload)
        ));
        catalog(&mut owner, &mut requests, &lease.id);
        owner
            .diagnostic_intent(ClientId(1), &lease.id, key(), Experiment::LaunchA)
            .unwrap();
        let WireExperiment::Launch(app) = requests.try_recv().unwrap().experiment else {
            panic!("wrong effect")
        };
        assert_eq!(app.id, "first");
        event(
            &mut owner,
            SessionObservation::Write(2, WriteOutcome::Written),
        );
        event(&mut owner, SessionObservation::End(2));
        assert!(owner.diagnostic.as_ref().unwrap().status.launch_quarantined);
        event(
            &mut owner,
            SessionObservation::Frame {
                sequence: 2,
                bytes: 20,
                event: CandidateEvent::Launch(PayloadShape::Object),
            },
        );
        assert!(matches!(
            owner.diagnostic_intent(ClientId(1), &lease.id, key(), Experiment::LaunchB),
            Err(ServiceError::InvalidPayload)
        ));
        assert!(requests.try_recv().is_err());
        owner
            .coordinator
            .app_state
            .restart_selection(owner.coordinator.saved_devices[0].display());
        assert!(owner.check_diagnostic());
        assert!(owner.diagnostic.as_ref().unwrap().catalog.is_empty());
        assert!(!owner.diagnostic.as_ref().unwrap().status.aliases_set);
    }
    #[tokio::test]
    async fn changing_aliases_conflicts_with_an_old_launch_intent_and_never_retargets_it() {
        let (mut owner, mut requests, _events, _) = owner();
        let lease = open(&mut owner);
        catalog(&mut owner, &mut requests, &lease.id);
        let intent = key();
        owner
            .diagnostic_intent(ClientId(1), &lease.id, intent.clone(), Experiment::LaunchA)
            .unwrap();
        let WireExperiment::Launch(admitted) = requests.try_recv().unwrap().experiment else {
            panic!("wrong experiment")
        };
        assert_eq!(admitted.id, "first");
        event(
            &mut owner,
            SessionObservation::Write(2, WriteOutcome::Written),
        );
        event(
            &mut owner,
            SessionObservation::Frame {
                sequence: 2,
                bytes: 32,
                event: CandidateEvent::Launch(PayloadShape::Object),
            },
        );
        event(&mut owner, SessionObservation::End(2));
        let d = owner.diagnostic.as_mut().unwrap();
        d.aliases = Some((1, 0));
        d.alias_revision += 1;
        assert!(matches!(
            owner.diagnostic_intent(ClientId(1), &lease.id, intent, Experiment::LaunchA),
            Err(ServiceError::KeyConflict)
        ));
        assert!(requests.try_recv().is_err());
        owner
            .diagnostic_intent(ClientId(1), &lease.id, key(), Experiment::LaunchA)
            .unwrap();
        let WireExperiment::Launch(fresh) = requests.try_recv().unwrap().experiment else {
            panic!("wrong experiment")
        };
        assert_eq!(fresh.id, "second");
        assert_eq!(admitted.id, "first");
    }
    #[tokio::test(start_paused = true)]
    async fn request_pressure_deadlines_and_revocation_close_the_lease_without_new_effects() {
        let (mut owner, mut requests, _events, aborts) = owner();
        let lease = open(&mut owner);
        owner
            .diagnostic_intent(ClientId(1), &lease.id, key(), Experiment::Catalog)
            .unwrap();
        assert!(matches!(
            owner.diagnostic_intent(ClientId(1), &lease.id, key(), Experiment::Hdmi1),
            Err(ServiceError::Busy)
        ));
        assert!(requests.try_recv().is_ok());
        event(&mut owner, SessionObservation::End(1));
        assert!(
            owner
                .diagnostic
                .as_ref()
                .unwrap()
                .status
                .catalog_quarantined
        );
        assert!(matches!(
            owner.diagnostic_intent(ClientId(1), &lease.id, key(), Experiment::Catalog),
            Err(ServiceError::InvalidPayload)
        ));
        tokio::time::advance(Duration::from_secs(1800)).await;
        assert!(owner.check_diagnostic());
        assert_eq!(aborts.load(Ordering::SeqCst), 1);
        assert!(requests.try_recv().is_err());
        let (mut owner, _, _events, _) = self::owner();
        let _lease = open(&mut owner);
        owner.diagnostic.as_ref().unwrap().authorization.revoke();
        assert!(owner.check_diagnostic());
    }
    #[tokio::test]
    async fn event_queue_pressure_revokes_the_out_of_band_flag() {
        let (mut owner, _requests, events, _aborts) = owner();
        let lease = open(&mut owner);
        for _ in 0..32 {
            owner
                .session_sender
                .try_send((
                    99,
                    99,
                    OwnerSessionEvent::Normal(TvSessionEvent::Disconnected),
                ))
                .unwrap();
        }
        events.send(SessionObservation::End(1)).await.unwrap();
        events.closed().await;
        assert!(!lease.alive.load(Ordering::SeqCst));
        assert!(owner.check_diagnostic());
    }
}
