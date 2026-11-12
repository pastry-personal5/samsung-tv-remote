//! Poll actual Iced command tasks against an injected client. No owner,
//! launcher, socket, TV or credential store is constructed by these tests.
use super::*;
use crate::application::control_service::CandidateId;
use crate::control_client::{
    background::{SubmitError, Update},
    model::FailureCode,
    port::{ClientFuture, ManagementError, ManagementIntent, ManagementReply, NativeClient},
    projection::NativeReceipt,
};
use futures_util::StreamExt;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tokio::sync::mpsc;

#[derive(Debug, Default)]
struct FakeClient {
    online: AtomicBool,
    requests: Mutex<Vec<(u64, Request)>>,
    management: Mutex<Vec<ManagementIntent>>,
    disconnected: AtomicBool,
    replies: Mutex<std::collections::VecDeque<Result<NativeReceipt, SubmitError>>>,
    management_replies: Mutex<std::collections::VecDeque<Result<ManagementReply, ManagementError>>>,
}
impl NativeClient for FakeClient {
    fn connected(&self) -> bool {
        self.online.load(Ordering::SeqCst)
    }
    fn take_updates(&self) -> Option<mpsc::Receiver<Update>> {
        None
    }
    fn submit(
        self: Arc<Self>,
        tag: u64,
        request: Request,
    ) -> ClientFuture<Result<NativeReceipt, SubmitError>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push((tag, request));
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Err(SubmitError::NotAdmitted(FailureCode::Busy)))
        })
    }
    fn manage(
        self: Arc<Self>,
        intent: ManagementIntent,
    ) -> ClientFuture<Result<ManagementReply, ManagementError>> {
        Box::pin(async move {
            self.management.lock().unwrap().push(intent);
            if matches!(intent, ManagementIntent::Stop { .. }) {
                self.online.store(false, Ordering::SeqCst);
            }
            let reply = self
                .management_replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or({
                    Ok(match intent {
                        ManagementIntent::Stop { .. } => {
                            ManagementReply::Stop(crate::control_client::port::StopReport {
                                acknowledgment:
                                    crate::control_client::port::StopAcknowledgment::Durable,
                                settlement: crate::control_client::port::StopSettlement::Pending,
                            })
                        }
                        ManagementIntent::Resume | ManagementIntent::Restart => {
                            ManagementReply::ReattachRequested
                        }
                        ManagementIntent::Recheck => ManagementReply::CredentialsRechecked,
                    })
                });
            if matches!(reply, Ok(ManagementReply::CredentialsRechecked)) {
                self.online.store(true, Ordering::SeqCst);
            }
            reply
        })
    }
    fn disconnect(&self) {
        self.disconnected.store(true, Ordering::SeqCst);
    }
}
fn app() -> (App, Arc<FakeClient>) {
    let (mut app, _) = App::new();
    let client = Arc::new(FakeClient::default());
    client.online.store(true, Ordering::SeqCst);
    app.remote = Some(client.clone());
    app.launch_connected = true;
    let mut snapshot = super::tests::projection(Some(DeviceId::new(1)), 4, 9);
    snapshot.wake_operation = Some(OperationId(7));
    let _ = app.apply_snapshot(snapshot);
    (app, client)
}
async fn run(app: &mut App, task: Task<Message>) -> usize {
    let mut exits = 0;
    let mut tasks = vec![task];
    while let Some(task) = tasks.pop() {
        if let Some(mut stream) = iced_runtime::task::into_stream(task) {
            while let Some(action) = stream.next().await {
                match action {
                    iced_runtime::Action::Output(message) => tasks.push(app.update(message)),
                    iced_runtime::Action::Exit => exits += 1,
                    _ => (),
                }
            }
        }
    }
    exits
}
#[tokio::test]
async fn client_port_captures_remote_wake_cancel_connect_and_selection_preconditions() {
    let (mut app, client) = app();
    let epoch = app.snapshot.as_ref().unwrap().epoch;
    for message in [
        Message::AttemptRemoteAction(SendRemoteAction::new(
            DeviceId::new(1),
            4,
            crate::RemoteAction::VolumeUp,
        )),
        Message::Wake,
        Message::CancelWake,
        Message::ConnectSelected,
        Message::SelectSaved(DeviceId::new(2)),
    ] {
        let task = app.update(message);
        run(&mut app, task).await;
    }
    let requests = client.requests.lock().unwrap();
    assert_eq!(requests.len(), 5);
    for (_, request) in requests.iter() {
        assert_eq!(request.epoch, epoch);
        assert_eq!(
            uuid::Uuid::parse_str(&request.key)
                .unwrap()
                .get_version_num(),
            4
        );
    }
    let keys: std::collections::HashSet<_> =
        requests.iter().map(|(_, request)| &request.key).collect();
    assert_eq!(keys.len(), 5);
    assert!(
        matches!(requests[0].1.command,Command::Remote(ref action) if action.target()==DeviceId::new(1) && action.selection_generation()==4)
    );
    assert!(
        matches!(requests[1].1.command,Command::Wake {target,generation:4} if target==DeviceId::new(1))
    );
    assert!(
        matches!(requests[2].1.command,Command::CancelWake {operation:OperationId(7),target,generation:4} if target==DeviceId::new(1))
    );
    assert!(
        matches!(requests[3].1.command,Command::Connect {target,generation:4,only_if_idle:false} if target==DeviceId::new(1))
    );
    assert!(
        matches!(requests[4].1.command,Command::Select {target,generation:4} if target==DeviceId::new(2))
    );
}
#[tokio::test]
async fn client_port_offline_intents_do_not_submit_or_replay_when_connection_returns() {
    let (mut app, client) = app();
    client.online.store(false, Ordering::SeqCst);
    let task = app.update(Message::Wake);
    run(&mut app, task).await;
    client.online.store(true, Ordering::SeqCst);
    assert!(client.requests.lock().unwrap().is_empty());
    assert!(app.submitting.is_empty() && app.pending.is_empty());
    let task = app.update(Message::ConnectSelected);
    run(&mut app, task).await;
    assert_eq!(client.requests.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn client_port_management_is_explicit_and_duplicate_requests_are_disabled() {
    let (mut app, client) = app();
    app.remote_status = Some(crate::control_client::background::Status::Blocked(
        crate::control_client::reconnect::Blocked::Stopped,
    ));
    let resume = app.update(Message::ResumeServer);
    let duplicate = app.update(Message::ResumeServer);
    let blocked_tv = app.update(Message::Wake);
    run(&mut app, blocked_tv).await;
    assert!(client.requests.lock().unwrap().is_empty());
    run(&mut app, resume).await;
    run(&mut app, duplicate).await;
    app.remote_status = Some(crate::control_client::background::Status::Blocked(
        crate::control_client::reconnect::Blocked::IncompatibleBuild,
    ));
    let restart = app.update(Message::RestartServer);
    run(&mut app, restart).await;
    assert_eq!(
        *client.management.lock().unwrap(),
        vec![ManagementIntent::Resume, ManagementIntent::Restart]
    );
    assert!(client.requests.lock().unwrap().is_empty());
    assert!(!app.remote_management);
    let main = window::Id::unique();
    app.main_window = Some(main);
    let close = app.update(Message::WindowClosed(main));
    run(&mut app, close).await;
    assert!(client.disconnected.load(Ordering::SeqCst));
    assert_eq!(client.management.lock().unwrap().len(), 2);
    assert!(client.requests.lock().unwrap().is_empty());
}

fn state(
    target: Option<DeviceId>,
    generation: u64,
    revision: u64,
    config_revision: u64,
    wake: WakeConfiguration,
) -> Snapshot {
    let mut state = super::tests::projection(target, generation, revision);
    let id = target.unwrap_or(DeviceId::new(1));
    state.devices = Arc::new(vec![crate::application::device_repository::SavedDevice {
        id,
        label: "Fixture TV".into(),
        host: TvHost::parse("fixture.local").unwrap(),
        wake,
    }]);
    state.configuration_revisions = Arc::new(vec![(id, config_revision)]);
    state
}
fn native(
    command: &Command,
    id: u64,
    outcome: Option<crate::control_client::projection::NativeOutcome>,
    selected: Option<DeviceId>,
    generation: u64,
    revision: Option<u64>,
) -> NativeReceipt {
    let epoch = Epoch::from_u128(1);
    let metadata = command.metadata(epoch);
    let terminal = outcome.is_some();
    NativeReceipt {
        id: OperationId(id),
        epoch,
        target: metadata.target,
        generation: metadata.generation,
        terminal,
        outcome,
        expires_at: terminal.then(|| {
            crate::application::control_service::display_deadline(std::time::Duration::from_secs(
                900,
            ))
        }),
        terminal_state: terminal.then_some(TerminalState {
            selected,
            generation,
            configuration_revision: revision,
        }),
    }
}
fn queue(client: &FakeClient, receipt: NativeReceipt) {
    client.replies.lock().unwrap().push_back(Ok(receipt));
}
async fn snapshot(app: &mut App, state: Snapshot) {
    let task = app.remote_update(Update::Snapshot {
        snapshot: Box::new(state),
        initial: false,
    });
    run(app, task).await;
}
async fn recovered(app: &mut App, tag: u64, receipt: NativeReceipt) {
    let task = app.remote_update(Update::Recovered { tag, receipt });
    run(app, task).await;
}
fn wake(mac: &str) -> WakeConfiguration {
    WakeConfiguration {
        wired: Some(mac.parse().unwrap()),
        wifi: None,
        active: None,
    }
}

#[tokio::test]
async fn client_workflow_select_save_connect_uses_terminal_preconditions_in_both_event_orders() {
    use crate::control_client::projection::NativeOutcome;
    for event_first in [false, true] {
        let (mut app, client) = app();
        let target = DeviceId::new(1);
        snapshot(
            &mut app,
            state(None, 0, 10, 0, WakeConfiguration::default()),
        )
        .await;
        let edit = app.update(Message::WakeWiredChanged("02:00:00:00:00:01".into()));
        run(&mut app, edit).await;
        assert!(client.requests.lock().unwrap().is_empty());
        let select = Command::Select {
            target,
            generation: 0,
        };
        let selected = native(
            &select,
            1,
            Some(NativeOutcome::Saved(target)),
            Some(target),
            1,
            Some(0),
        );
        queue(
            &client,
            if event_first {
                native(&select, 1, None, None, 0, None)
            } else {
                selected.clone()
            },
        );
        let task = app.update(Message::SelectSaved(target));
        run(&mut app, task).await;
        let select_tag = client.requests.lock().unwrap()[0].0;
        let configuration = wake("02:00:00:00:00:01");
        let save = Command::SetWake {
            target,
            generation: 1,
            revision: 0,
            configuration,
        };
        let saved = native(
            &save,
            2,
            Some(NativeOutcome::Saved(target)),
            Some(target),
            1,
            Some(1),
        );
        queue(
            &client,
            if event_first {
                native(&save, 2, None, None, 0, None)
            } else {
                saved.clone()
            },
        );
        snapshot(
            &mut app,
            state(Some(target), 1, 11, 0, WakeConfiguration::default()),
        )
        .await;
        if event_first {
            recovered(&mut app, select_tag, selected).await;
        }
        assert_eq!(client.requests.lock().unwrap().len(), 2);
        assert!(
            matches!(client.requests.lock().unwrap()[1].1.command,Command::SetWake {target:actual,generation:1,revision:0,configuration:value} if actual==target && value==configuration)
        );
        let save_tag = client.requests.lock().unwrap()[1].0;
        snapshot(&mut app, state(Some(target), 1, 12, 1, configuration)).await;
        if event_first {
            recovered(&mut app, save_tag, saved).await;
        }
        assert_eq!(client.requests.lock().unwrap().len(), 3);
        assert!(
            matches!(client.requests.lock().unwrap()[2].1.command,Command::Connect {target:actual,generation:1,only_if_idle:true} if actual==target)
        );
        assert!(!app.wake_dirty && !app.workflows.conflict);
        snapshot(&mut app, state(Some(target), 1, 12, 1, configuration)).await;
        assert_eq!(client.requests.lock().unwrap().len(), 3);
    }
}

#[tokio::test]
async fn client_workflow_pending_save_keeps_new_edits_and_saves_only_the_new_draft_after_success() {
    use crate::control_client::projection::NativeOutcome;
    let (mut app, client) = app();
    let target = DeviceId::new(1);
    let first = wake("02:00:00:00:00:01");
    let second = wake("02:00:00:00:00:02");
    let first_command = Command::SetWake {
        target,
        generation: 4,
        revision: 0,
        configuration: first,
    };
    queue(&client, native(&first_command, 1, None, None, 0, None));
    let edit = app.update(Message::WakeWiredChanged("02:00:00:00:00:01".into()));
    run(&mut app, edit).await;
    let first_tag = client.requests.lock().unwrap()[0].0;
    let edit = app.update(Message::WakeWiredChanged("02:00:00:00:00:02".into()));
    run(&mut app, edit).await;
    assert_eq!(client.requests.lock().unwrap().len(), 1);
    snapshot(&mut app, state(Some(target), 4, 10, 1, first)).await;
    let second_command = Command::SetWake {
        target,
        generation: 4,
        revision: 1,
        configuration: second,
    };
    queue(&client, native(&second_command, 2, None, None, 0, None));
    recovered(
        &mut app,
        first_tag,
        native(
            &first_command,
            1,
            Some(NativeOutcome::Saved(target)),
            Some(target),
            4,
            Some(1),
        ),
    )
    .await;
    assert!(app.wake_dirty);
    assert_eq!(app.wake_wired, "02:00:00:00:00:02");
    assert_eq!(client.requests.lock().unwrap().len(), 2);
    assert!(
        matches!(client.requests.lock().unwrap()[1].1.command,Command::SetWake {generation:4,revision:1,configuration,..} if configuration==second)
    );
    let second_tag = client.requests.lock().unwrap()[1].0;
    snapshot(&mut app, state(Some(target), 4, 11, 2, second)).await;
    recovered(
        &mut app,
        second_tag,
        native(
            &second_command,
            2,
            Some(NativeOutcome::Saved(target)),
            Some(target),
            4,
            Some(2),
        ),
    )
    .await;
    assert!(!app.wake_dirty);
    assert_eq!(client.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn client_workflow_external_selection_or_config_change_never_retargets_a_dirty_draft() {
    use crate::control_client::projection::NativeOutcome;
    for (other_target, other_generation, other_revision) in [
        (DeviceId::new(2), 5, 0),
        (DeviceId::new(1), 5, 1),
        (DeviceId::new(1), 4, 2),
    ] {
        let (mut app, client) = app();
        let target = DeviceId::new(1);
        let configuration = wake("02:00:00:00:00:01");
        let command = Command::SetWake {
            target,
            generation: 4,
            revision: 0,
            configuration,
        };
        queue(&client, native(&command, 1, None, None, 0, None));
        let task = app.update(Message::WakeWiredChanged("02:00:00:00:00:01".into()));
        run(&mut app, task).await;
        let tag = client.requests.lock().unwrap()[0].0;
        app.view_model.select_view(PrimaryView::Remote);
        snapshot(
            &mut app,
            state(
                Some(other_target),
                other_generation,
                10,
                other_revision,
                configuration,
            ),
        )
        .await;
        recovered(
            &mut app,
            tag,
            native(
                &command,
                1,
                Some(NativeOutcome::Saved(target)),
                Some(target),
                4,
                Some(1),
            ),
        )
        .await;
        assert!(app.wake_dirty && app.workflows.conflict);
        assert_eq!(app.wake_wired, "02:00:00:00:00:01");
        assert_eq!(app.view_model.primary_view(), PrimaryView::Remote);
        assert_eq!(client.requests.lock().unwrap().len(), 1);
        let edit = app.update(Message::WakeWiredChanged("02:00:00:00:00:02".into()));
        run(&mut app, edit).await;
        assert_eq!(client.requests.lock().unwrap().len(), 1);
        let reload = app.update(Message::ReloadWakeSettings);
        run(&mut app, reload).await;
        assert!(!app.wake_dirty && !app.workflows.conflict);
        let edit = app.update(Message::WakeWiredChanged("02:00:00:00:00:03".into()));
        run(&mut app, edit).await;
        assert!(
            matches!(client.requests.lock().unwrap()[1].1.command,Command::SetWake {target:actual,generation,revision,..} if actual==other_target && generation==other_generation && revision==other_revision)
        );
    }
}

#[tokio::test]
async fn client_workflow_unknown_save_and_reattach_never_replay_or_complete_a_new_draft() {
    let (mut app, client) = app();
    let target = DeviceId::new(1);
    let configuration = wake("02:00:00:00:00:01");
    let command = Command::SetWake {
        target,
        generation: 4,
        revision: 0,
        configuration,
    };
    queue(&client, native(&command, 1, None, None, 0, None));
    let task = app.update(Message::WakeWiredChanged("02:00:00:00:00:01".into()));
    run(&mut app, task).await;
    let tag = client.requests.lock().unwrap()[0].0;
    let unknown = app.remote_update(Update::Unknown {
        tag,
        reason: crate::control_client::background::UnknownReason::OutcomeUnavailable,
    });
    run(&mut app, unknown).await;
    let reconnect = app.remote_update(Update::Snapshot {
        snapshot: Box::new(state(Some(target), 4, 10, 1, configuration)),
        initial: true,
    });
    run(&mut app, reconnect).await;
    assert!(app.wake_dirty && app.workflows.conflict);
    assert_eq!(client.requests.lock().unwrap().len(), 1);
    assert!(app.pending.is_empty() && app.submitting.is_empty());
}

#[tokio::test]
async fn client_workflow_launch_connect_is_idle_only_once_and_never_deferred_until_busy_clears() {
    use crate::application::ConnectionState;
    for blocked in [
        "idle",
        "empty",
        "ready",
        "connecting",
        "pairing",
        "wake",
        "storage",
        "closed",
    ] {
        let (mut app, client) = app();
        app.launch_connected = false;
        app.snapshot = None;
        let target = DeviceId::new(1);
        let mut first = state(
            if blocked == "empty" {
                None
            } else {
                Some(target)
            },
            4,
            1,
            0,
            WakeConfiguration::default(),
        );
        match blocked {
            "ready" => {
                first
                    .control
                    .set_connection_state(4, ConnectionState::Ready);
            }
            "connecting" => {
                first
                    .control
                    .set_connection_state(4, ConnectionState::Connecting);
            }
            "pairing" => first.pairing_pending = true,
            "wake" => first.wake_operation = Some(OperationId(8)),
            "storage" => first.storage_pending = true,
            "closed" => first.admission_closed = true,
            _ => (),
        }
        let task = app.remote_update(Update::Snapshot {
            snapshot: Box::new(first),
            initial: true,
        });
        run(&mut app, task).await;
        let expected = usize::from(blocked == "idle");
        assert_eq!(client.requests.lock().unwrap().len(), expected, "{blocked}");
        if blocked == "idle" {
            assert!(
                matches!(client.requests.lock().unwrap()[0].1.command,Command::Connect {target:actual,generation:4,only_if_idle:true} if actual==target)
            );
        }
        snapshot(
            &mut app,
            state(Some(target), 4, 2, 0, WakeConfiguration::default()),
        )
        .await;
        let task = app.remote_update(Update::Snapshot {
            snapshot: Box::new(state(Some(target), 4, 2, 0, WakeConfiguration::default())),
            initial: true,
        });
        run(&mut app, task).await;
        assert_eq!(client.requests.lock().unwrap().len(), expected, "{blocked}");
    }
}

fn observation(id: u64, host: &str) -> Observation {
    Observation {
        id: OperationId(id),
        host: TvHost::parse(host).unwrap(),
        observation: crate::control_client::model::ProbeObservation {
            pin: crate::application::certificate_trust::CertificatePin::from_der(
                b"synthetic-fixture-certificate",
            ),
            name: Some("Fixture TV".into()),
            model: None,
        },
        expires_at: crate::application::control_service::display_deadline(
            std::time::Duration::from_secs(900),
        ),
    }
}
#[tokio::test]
async fn client_workflow_probe_pair_and_repair_require_the_current_explicit_confirmation() {
    use crate::control_client::projection::NativeOutcome;
    for repair in [false, true] {
        let (mut app, client) = app();
        let host = "fixture.local";
        app.tv_address = host.into();
        let probe = Command::Probe(TvHost::parse(host).unwrap());
        queue(
            &client,
            native(
                &probe,
                1,
                Some(NativeOutcome::Probed(Box::new(observation(1, host)))),
                Some(DeviceId::new(1)),
                4,
                Some(0),
            ),
        );
        let task = app.update(Message::ProbeTv);
        run(&mut app, task).await;
        assert!(app.observed.is_some());
        let task = app.update(if repair {
            Message::ConfirmAndRepair
        } else {
            Message::ConfirmAndPair
        });
        run(&mut app, task).await;
        assert!(app.observed.is_none());
        assert_eq!(client.requests.lock().unwrap().len(), 2);
        assert!(
            matches!(&client.requests.lock().unwrap()[1].1.command,Command::Pair {observation:OperationId(1),host:actual,pin,replace:Some(target),generation:4,..}
            if actual.as_str()==host && *target==DeviceId::new(1) && *pin==observation(1,host).observation.pin)
        );
        let task = app.update(Message::ConfirmAndPair);
        run(&mut app, task).await;
        assert_eq!(client.requests.lock().unwrap().len(), 2);
    }
}
#[tokio::test]
async fn client_workflow_same_device_generation_change_and_epoch_change_invalidate_late_setup_results(
) {
    use crate::control_client::projection::NativeOutcome;
    for changed_epoch in [false, true] {
        let (mut app, client) = app();
        app.tv_address = "fixture.local".into();
        let probe = Command::Probe(TvHost::parse("fixture.local").unwrap());
        queue(&client, native(&probe, 1, None, None, 0, None));
        let task = app.update(Message::ProbeTv);
        run(&mut app, task).await;
        let tag = client.requests.lock().unwrap()[0].0;
        let mut next = state(
            Some(DeviceId::new(1)),
            if changed_epoch { 4 } else { 5 },
            10,
            0,
            WakeConfiguration::default(),
        );
        if changed_epoch {
            next.epoch = Epoch::from_u128(2);
        }
        let task = app.remote_update(Update::Snapshot {
            snapshot: Box::new(next),
            initial: changed_epoch,
        });
        run(&mut app, task).await;
        recovered(
            &mut app,
            tag,
            native(
                &probe,
                1,
                Some(NativeOutcome::Probed(Box::new(observation(
                    1,
                    "fixture.local",
                )))),
                Some(DeviceId::new(1)),
                4,
                Some(0),
            ),
        )
        .await;
        assert!(app.observed.is_none());
        let task = app.update(Message::ConfirmAndPair);
        run(&mut app, task).await;
        assert_eq!(client.requests.lock().unwrap().len(), 1);
    }
}
#[tokio::test]
async fn client_workflow_late_probe_cannot_replace_a_newer_confirmation_and_expired_checks_do_not_pair(
) {
    use crate::control_client::projection::NativeOutcome;
    let (mut app, client) = app();
    app.tv_address = "fixture.local".into();
    let probe = Command::Probe(TvHost::parse("fixture.local").unwrap());
    queue(&client, native(&probe, 1, None, None, 0, None));
    let task = app.update(Message::ProbeTv);
    run(&mut app, task).await;
    let tag = client.requests.lock().unwrap()[0].0;
    queue(
        &client,
        native(
            &probe,
            2,
            Some(NativeOutcome::Probed(Box::new(observation(
                2,
                "fixture.local",
            )))),
            Some(DeviceId::new(1)),
            4,
            Some(0),
        ),
    );
    let task = app.update(Message::ProbeTv);
    run(&mut app, task).await;
    recovered(
        &mut app,
        tag,
        native(
            &probe,
            1,
            Some(NativeOutcome::Probed(Box::new(observation(
                1,
                "fixture.local",
            )))),
            Some(DeviceId::new(1)),
            4,
            Some(0),
        ),
    )
    .await;
    assert_eq!(app.observed.as_ref().unwrap().id, OperationId(2));
    app.observed.as_mut().unwrap().expires_at = std::num::NonZeroU64::new(1).unwrap();
    let task = app.update(Message::ConfirmAndPair);
    run(&mut app, task).await;
    assert_eq!(client.requests.lock().unwrap().len(), 2);
    assert!(app.observed.is_none());
}
#[tokio::test]
async fn client_workflow_discovery_candidates_and_explicit_forget_recovery_use_captured_handles() {
    use crate::control_client::projection::NativeOutcome;
    let (mut app, client) = app();
    let discover = Command::Discover;
    let candidate = Candidate {
        id: CandidateId(OperationId(1), 0),
        host: TvHost::parse("new-fixture.local").unwrap(),
        expires_at: crate::application::control_service::display_deadline(
            std::time::Duration::from_secs(900),
        ),
    };
    queue(
        &client,
        native(
            &discover,
            1,
            Some(NativeOutcome::Discovered(vec![candidate.clone()])),
            Some(DeviceId::new(1)),
            4,
            Some(0),
        ),
    );
    let task = app.update(Message::DiscoverTv);
    run(&mut app, task).await;
    let task = app.update(Message::ProbeCandidate(candidate.host.clone()));
    run(&mut app, task).await;
    assert!(
        matches!(&client.requests.lock().unwrap()[1].1.command,Command::ProbeCandidate {candidate:actual,host} if *actual==candidate.id && *host==candidate.host)
    );
    let task = app.update(Message::ForgetSelected);
    run(&mut app, task).await;
    assert!(
        matches!(client.requests.lock().unwrap()[2].1.command,Command::Forget {target,revision:0,confirm_device} if target==DeviceId::new(1) && confirm_device==target)
    );
    let marker = crate::application::recovery::ForgetMarker::prepared(DeviceId::new(1));
    let mut next = state(None, 5, 10, 0, WakeConfiguration::default());
    next.recovery = Some(marker);
    next.startup = StartupState::RecoveryRequired;
    snapshot(&mut app, next).await;
    let task = app.update(Message::ForgetSelected);
    run(&mut app, task).await;
    assert!(
        matches!(client.requests.lock().unwrap()[3].1.command,Command::RecoverForget {target,transaction,confirm_device} if target==marker.target && transaction==marker.transaction && confirm_device==target)
    );
}

#[tokio::test]
async fn client_workflow_reselecting_same_tv_does_not_authorize_overwriting_an_unseen_configuration(
) {
    use crate::control_client::projection::NativeOutcome;
    let (mut app, client) = app();
    let target = DeviceId::new(1);
    let mut pending = state(Some(target), 4, 10, 0, WakeConfiguration::default());
    pending.storage_pending = true;
    snapshot(&mut app, pending).await;
    let task = app.update(Message::WakeWiredChanged("02:00:00:00:00:01".into()));
    run(&mut app, task).await;
    let external = wake("02:00:00:00:00:02");
    snapshot(&mut app, state(Some(target), 4, 11, 1, external)).await;
    assert!(app.workflows.conflict);
    let select = Command::Select {
        target,
        generation: 4,
    };
    queue(
        &client,
        native(
            &select,
            1,
            Some(NativeOutcome::Saved(target)),
            Some(target),
            5,
            Some(1),
        ),
    );
    let task = app.update(Message::SelectSaved(target));
    run(&mut app, task).await;
    snapshot(&mut app, state(Some(target), 5, 12, 1, external)).await;
    assert_eq!(client.requests.lock().unwrap().len(), 1);
    assert!(app.workflows.conflict && app.wake_dirty);
    assert_eq!(app.wake_wired, "02:00:00:00:00:01");
}

#[tokio::test]
async fn client_workflow_partial_pairing_cleanup_and_fresh_repair_use_exact_recovery_transactions()
{
    use crate::{
        application::recovery::{PairingCleanup, PairingMarker, PairingStage},
        control_client::{model::CleanupFacts, projection::NativeOutcome},
    };
    let (mut app, client) = app();
    let target = DeviceId::new(2);
    let transaction = DeviceId::new(99);
    let marker = PairingMarker {
        transaction,
        target,
        prior_selection: Some(DeviceId::new(1)),
        repair: false,
        backup: None,
        stage: PairingStage::TokenSaved,
        cleanup: Some(PairingCleanup::default()),
    };
    assert!(marker.valid());
    let mut recovering = state(None, 5, 10, 0, WakeConfiguration::default());
    recovering.pairing_recovery = Some(marker);
    recovering.startup = StartupState::RecoveryRequired;
    snapshot(&mut app, recovering).await;
    let command = Command::RecoverPairing {
        target,
        transaction,
        confirm_device: target,
    };
    queue(
        &client,
        native(
            &command,
            1,
            Some(NativeOutcome::PartialSetup(CleanupFacts {
                credential_removed: false,
                trust_removed: true,
                record_removed: true,
            })),
            None,
            5,
            None,
        ),
    );
    let task = app.update(Message::ForgetSelected);
    run(&mut app, task).await;
    assert_eq!(client.requests.lock().unwrap()[0].1.command, command);
    assert!(app.snapshot.as_ref().unwrap().pairing_recovery.is_some());
    let message = &app.view_model.messages().entries().back().unwrap().text;
    assert!(
        message.contains("credential not removed")
            && message.contains("trust removed")
            && message.contains("saved record removed")
    );
    let target = DeviceId::new(1);
    let mut repairing = state(Some(target), 6, 11, 0, WakeConfiguration::default());
    repairing.pairing_recovery = Some(PairingMarker {
        transaction,
        target,
        prior_selection: Some(target),
        repair: true,
        backup: Some(transaction),
        stage: PairingStage::Prepared,
        cleanup: None,
    });
    repairing.startup = StartupState::RecoveryRequired;
    snapshot(&mut app, repairing).await;
    app.tv_address = "fixture.local".into();
    let probe = Command::Probe(TvHost::parse("fixture.local").unwrap());
    queue(
        &client,
        native(
            &probe,
            2,
            Some(NativeOutcome::Probed(Box::new(observation(
                2,
                "fixture.local",
            )))),
            Some(target),
            6,
            Some(0),
        ),
    );
    let task = app.update(Message::ProbeTv);
    run(&mut app, task).await;
    let task = app.update(Message::ConfirmAndRepair);
    run(&mut app, task).await;
    assert!(
        matches!(client.requests.lock().unwrap()[2].1.command,Command::Pair {replace:Some(actual),generation:6,recovery:Some(actual_transaction),..} if actual==target && actual_transaction==transaction)
    );
}

#[tokio::test]
async fn client_lifecycle_stop_confirms_shared_impact_once_and_keeps_the_gui_open() {
    use crate::control_client::{
        background::Status,
        port::{StopAcknowledgment, StopSettlement},
    };
    let (mut app, client) = app();
    app.remote_status = Some(Status::Connected);
    app.refresh_client_controls();
    let main = window::Id::unique();
    app.main_window = Some(main);
    let request = app.update(Message::RequestStopServer);
    assert_eq!(run(&mut app, request).await, 0);
    assert_eq!(app.stop_confirmation, Some(Epoch::from_u128(1)));
    assert!(client.management.lock().unwrap().is_empty());
    let cancel = app.update(Message::CancelStopServer);
    run(&mut app, cancel).await;
    let confirm = app.update(Message::ConfirmStopServer);
    run(&mut app, confirm).await;
    assert!(client.management.lock().unwrap().is_empty());
    let request = app.update(Message::RequestStopServer);
    run(&mut app, request).await;
    let stop = app.update(Message::ConfirmStopServer);
    let duplicate = app.update(Message::ConfirmStopServer);
    assert!(app.remote_management);
    assert!(!app.view_model.control_state().service_available);
    let tv = app.update(Message::Wake);
    run(&mut app, tv).await;
    assert!(client.requests.lock().unwrap().is_empty());
    assert_eq!(run(&mut app, stop).await, 0);
    run(&mut app, duplicate).await;
    assert_eq!(
        *client.management.lock().unwrap(),
        vec![ManagementIntent::Stop {
            epoch: Epoch::from_u128(1)
        }]
    );
    assert_eq!(app.main_window, Some(main));
    assert!(!app.client_exited);
    let report = app.stop_result.unwrap().unwrap();
    assert_eq!(report.acknowledgment, StopAcknowledgment::Durable);
    assert_eq!(report.settlement, StopSettlement::Pending);
    let message = super::lifecycle::stop_text(report);
    assert!(
        message.contains("durably acknowledged")
            && message.contains("Settlement is not yet observed")
    );
}
#[tokio::test]
async fn client_lifecycle_stop_confirmation_cannot_target_a_replacement_epoch() {
    use crate::control_client::background::Status;
    let (mut app, client) = app();
    app.remote_status = Some(Status::Connected);
    let request = app.update(Message::RequestStopServer);
    run(&mut app, request).await;
    let mut next = state(
        Some(DeviceId::new(1)),
        4,
        10,
        0,
        WakeConfiguration::default(),
    );
    next.epoch = Epoch::from_u128(2);
    let task = app.remote_update(Update::Snapshot {
        snapshot: Box::new(next),
        initial: true,
    });
    run(&mut app, task).await;
    let confirm = app.update(Message::ConfirmStopServer);
    run(&mut app, confirm).await;
    assert!(client.management.lock().unwrap().is_empty());
}
#[tokio::test]
async fn client_lifecycle_lost_stop_reply_and_settlement_are_independent_and_failures_stay_visible()
{
    use crate::control_client::{
        background::Status,
        port::{StopAcknowledgment, StopReport, StopSettlement},
        reconnect::Blocked,
    };
    for settlement in [
        StopSettlement::Pending,
        StopSettlement::Observed,
        StopSettlement::CheckFailed,
    ] {
        let (mut app, client) = app();
        app.remote_status = Some(Status::Connected);
        let report = StopReport {
            acknowledgment: StopAcknowledgment::MarkerObserved,
            settlement,
        };
        client
            .management_replies
            .lock()
            .unwrap()
            .push_back(Ok(ManagementReply::Stop(report)));
        let request = app.update(Message::RequestStopServer);
        run(&mut app, request).await;
        let stop = app.update(Message::ConfirmStopServer);
        run(&mut app, stop).await;
        assert_eq!(app.stop_result, Some(Ok(report)));
        let text = super::lifecycle::stop_text(report);
        assert!(text.contains("acknowledgment was not received"));
        assert!(!text.contains("durably acknowledged"));
    }
    let (mut app, client) = app();
    app.remote_status = Some(Status::Connected);
    client
        .management_replies
        .lock()
        .unwrap()
        .push_back(Err(ManagementError::StopPersistenceFailed));
    let request = app.update(Message::RequestStopServer);
    run(&mut app, request).await;
    let stop = app.update(Message::ConfirmStopServer);
    run(&mut app, stop).await;
    assert_eq!(
        app.stop_result,
        Some(Err(ManagementError::StopPersistenceFailed))
    );
    app.remote_status = Some(Status::Blocked(Blocked::StopUnconfirmed));
    let check = app.update(Message::RecheckServer);
    run(&mut app, check).await;
    assert_eq!(
        app.stop_result,
        Some(Err(ManagementError::StopPersistenceFailed))
    );
    assert_eq!(
        *client.management.lock().unwrap(),
        vec![
            ManagementIntent::Stop {
                epoch: Epoch::from_u128(1)
            },
            ManagementIntent::Recheck
        ]
    );
    assert!(client.requests.lock().unwrap().is_empty());
}
#[tokio::test]
async fn client_lifecycle_recheck_is_single_explicit_and_waits_for_fresh_atomic_state_in_both_orders(
) {
    use crate::control_client::{background::Status, reconnect::Blocked};
    for snapshot_first in [false, true] {
        let (mut app, client) = app();
        app.remote_status = Some(Status::Blocked(Blocked::BootstrapDenied));
        app.refresh_client_controls();
        let check = app.update(Message::RecheckServer);
        let duplicate = app.update(Message::RecheckServer);
        if snapshot_first {
            let task = app.remote_update(Update::Snapshot {
                snapshot: Box::new(state(
                    Some(DeviceId::new(1)),
                    4,
                    10,
                    0,
                    WakeConfiguration::default(),
                )),
                initial: true,
            });
            run(&mut app, task).await;
            let task = app.remote_update(Update::Status(Status::Connected));
            run(&mut app, task).await;
            assert!(!app.view_model.control_state().service_available);
        }
        run(&mut app, check).await;
        run(&mut app, duplicate).await;
        if !snapshot_first {
            let task = app.remote_update(Update::Status(Status::Connected));
            run(&mut app, task).await;
            assert!(!app.view_model.control_state().service_available);
            let task = app.remote_update(Update::Snapshot {
                snapshot: Box::new(state(
                    Some(DeviceId::new(1)),
                    4,
                    10,
                    0,
                    WakeConfiguration::default(),
                )),
                initial: true,
            });
            run(&mut app, task).await;
        }
        assert!(app.view_model.control_state().service_available);
        assert_eq!(
            *client.management.lock().unwrap(),
            vec![ManagementIntent::Recheck]
        );
        assert!(client.requests.lock().unwrap().is_empty());
    }
}
#[tokio::test]
async fn client_lifecycle_window_exit_cancels_late_callbacks_without_reopening_or_sending_stop() {
    use crate::control_client::{
        background::Status,
        port::{StopAcknowledgment, StopReport, StopSettlement},
    };
    let (mut app, client) = app();
    app.remote_status = Some(Status::Connected);
    let main = window::Id::unique();
    app.main_window = Some(main);
    let request = app.update(Message::RequestStopServer);
    run(&mut app, request).await;
    let pending = app.update(Message::ConfirmStopServer);
    let close = app.update(Message::WindowClosed(main));
    assert_eq!(run(&mut app, close).await, 1);
    drop(pending);
    let late = app.update(Message::RemoteManagementFinished(Ok(
        ManagementReply::Stop(StopReport {
            acknowledgment: StopAcknowledgment::Durable,
            settlement: StopSettlement::Observed,
        }),
    )));
    run(&mut app, late).await;
    let new_client = Arc::new(FakeClient::default());
    let late = app.update(Message::RemoteStarted(Ok(new_client.clone())));
    run(&mut app, late).await;
    let reopen = app.update(Message::OpenSettings);
    run(&mut app, reopen).await;
    assert!(app.client_exited && app.settings_window.is_none());
    assert!(new_client.disconnected.load(Ordering::SeqCst));
    assert!(app.stop_result.is_none());
    assert!(client.management.lock().unwrap().is_empty());
    assert!(client.disconnected.load(Ordering::SeqCst));
}

#[tokio::test]
async fn client_lifecycle_missing_signed_identity_blocks_initialization_and_all_mutations() {
    use crate::control_client::{background::Status, reconnect::Blocked};
    let (mut app, _) = App::new();
    let task = app.update(Message::RemoteStarted(Err(ManagementError::Blocked(
        Blocked::SigningRequired,
    ))));
    run(&mut app, task).await;
    assert!(matches!(
        app.remote_status,
        Some(Status::Blocked(Blocked::SigningRequired))
    ));
    assert!(app.remote.is_none());
    assert!(!app.view_model.control_state().service_available);
    assert!(app
        .view_model
        .messages()
        .entries()
        .back()
        .unwrap()
        .text
        .contains("signed app bundle"));
    let task = app.update(Message::RecheckServer);
    run(&mut app, task).await;
    assert!(!app.remote_management && app.submitting.is_empty());
    let task = app.update(Message::DiscoverTv);
    run(&mut app, task).await;
    assert!(app.submitting.is_empty());
}
