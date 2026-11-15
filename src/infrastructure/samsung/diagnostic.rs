//! Research-shaped candidate codecs and actor, compiled only for the private spike.
//! No raw frame, app identity, IME content or serde error reaches diagnostics output.
use super::*;
use crate::application::diagnostic::*;
use crate::infrastructure::unique_json::unique_json;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::Ordering;
use tokio::time::Instant;

fn shape(value: Option<&Value>) -> PayloadShape {
    match value {
        None => PayloadShape::Absent,
        Some(Value::Null) => PayloadShape::Null,
        Some(Value::Object(_)) => PayloadShape::Object,
        Some(Value::Array(_)) => PayloadShape::Array,
        Some(Value::String(_)) => PayloadShape::String,
        Some(Value::Bool(_)) => PayloadShape::Boolean,
        Some(Value::Number(_)) => PayloadShape::Number,
    }
}
fn parse(frame: &[u8]) -> CandidateEvent {
    if frame.len() > MAX_EVENT_BYTES {
        return CandidateEvent::Limit;
    }
    let Ok(value) = unique_json(frame) else {
        return CandidateEvent::Malformed;
    };
    let Some(event) = value.get("event").and_then(Value::as_str) else {
        return CandidateEvent::Malformed;
    };
    match event {
        "ed.installedApp.get" => {
            let Some(apps) = value["data"]["data"].as_array() else {
                return CandidateEvent::Malformed;
            };
            if apps.len() > MAX_APPS {
                return CandidateEvent::Limit;
            }
            let mut parsed = Vec::with_capacity(apps.len());
            let mut ids = HashSet::new();
            let mut names = HashSet::new();
            for app in apps {
                let (Some(id), Some(name)) = (app["appId"].as_str(), app["name"].as_str()) else {
                    return CandidateEvent::Malformed;
                };
                if id.len() > 128 || name.len() > 128 {
                    return CandidateEvent::Limit;
                }
                let Some(app) = DiagnosticApp::new(id, name) else {
                    return CandidateEvent::Malformed;
                };
                if !ids.insert(app.id.clone()) || !names.insert(app.name.clone()) {
                    return CandidateEvent::Malformed;
                }
                parsed.push(app);
            }
            CandidateEvent::Catalog(parsed)
        }
        "ed.apps.launch" => CandidateEvent::Launch(shape(value.get("data"))),
        "ms.remote.imeStart" => CandidateEvent::ImeStart(shape(value.get("data"))),
        "ms.remote.imeEnd" => CandidateEvent::ImeEnd(shape(value.get("data"))),
        _ => CandidateEvent::Unknown,
    }
}
fn encode(experiment: &WireExperiment) -> Option<String> {
    Some(match experiment {
        WireExperiment::Catalog => json!({"method":"ms.channel.emit","params":{"event":"ed.installedApp.get","to":"host"}}).to_string(),
        WireExperiment::Launch(app) => json!({"method":"ms.channel.emit","params":{"event":"ed.apps.launch","to":"host","data":{"appId":app.id,"action_type":"NATIVE_LAUNCH"}}}).to_string(),
        WireExperiment::Source(key) => {
            let key = match key { SourceKey::Hdmi1 => "KEY_HDMI1", SourceKey::Hdmi2 => "KEY_HDMI2", SourceKey::Hdmi3 => "KEY_HDMI3", SourceKey::Tuner => "KEY_TV", SourceKey::Sources => "KEY_SOURCE" };
            json!({"method":"ms.remote.control","params":{"Cmd":"Click","DataOfCmd":key,"Option":"false","TypeOfRemote":"SendRemoteKey"}}).to_string()
        }
        WireExperiment::Ime => return None,
    })
}

async fn write_experiment<S>(writer: &mut S, request: &SessionRequest) -> WriteOutcome
where
    S: Sink<Message, Error = WebSocketError> + Unpin,
{
    if !request.alive.load(Ordering::SeqCst) {
        return WriteOutcome::NotSent;
    }
    let Some(frame) = encode(&request.experiment) else {
        return WriteOutcome::ObservationOnly;
    };
    match timeout(WRITE_TIMEOUT, writer.send(Message::Text(frame.into()))).await {
        Ok(Ok(())) => WriteOutcome::Written,
        _ => WriteOutcome::Uncertain,
    }
}

pub(super) fn start(
    socket: WebSocketStream<TlsStream<TcpStream>>,
    commands: mpsc::Receiver<SessionCommand>,
    events: mpsc::Sender<SessionEvent>,
) -> (tokio::task::JoinHandle<()>, DiagnosticPort) {
    let (sender, requests) = mpsc::channel(1);
    let (observed, receiver) = mpsc::channel(16);
    (
        tokio::spawn(run(socket, commands, events, requests, observed)),
        DiagnosticPort {
            commands: sender,
            events: receiver,
        },
    )
}

async fn run(
    socket: WebSocketStream<TlsStream<TcpStream>>,
    mut commands: mpsc::Receiver<SessionCommand>,
    events: mpsc::Sender<SessionEvent>,
    mut requests: mpsc::Receiver<SessionRequest>,
    observed: mpsc::Sender<SessionObservation>,
) {
    let (mut writer, mut reader) = socket.split();
    let mut heartbeat = tokio::time::interval_at(
        Instant::now() + Duration::from_secs(20),
        Duration::from_secs(20),
    );
    let mut awaiting_pong = false;
    let mut pending: Option<(SessionRequest, Instant)> = None;
    let mut diag_open = true;
    loop {
        let deadline = pending
            .as_ref()
            .map(|(_, t)| *t)
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(1800));
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline), if pending.is_some() => {
                let (request, _) = pending.take().expect("pending experiment");
                if observed.try_send(SessionObservation::End(request.sequence)).is_err() { break; }
            }
            request = requests.recv(), if diag_open => {
                let Some(request) = request else { diag_open = false; continue; };
                if pending.is_some() { break; } // Owner and actor both enforce single flight.
                // The controller's wait is derived from the same experiment policy.
                let seconds = if matches!(request.experiment, WireExperiment::Ime) { Experiment::Ime } else { Experiment::Catalog }.window_seconds();
                let outcome = write_experiment(&mut writer, &request).await;
                if observed.try_send(SessionObservation::Write(request.sequence, outcome)).is_err() { break; }
                if outcome == WriteOutcome::Uncertain { break; }
                if outcome == WriteOutcome::NotSent {
                    let _ = observed.try_send(SessionObservation::End(request.sequence));
                } else { pending = Some((request, Instant::now() + Duration::from_secs(seconds))); }
            }
            command = commands.recv() => match command {
                Some(SessionCommand::Click { id, action }) => {
                    // No normal click may interleave with the exclusive lease.
                    if pending.is_some() {
                        if events.send(SessionEvent::NotSent(id)).await.is_err() { break; }
                        continue;
                    }
                    let outcome = write_click(&mut writer, id, action, WRITE_TIMEOUT).await;
                    let failed = matches!(outcome, SessionEvent::Uncertain(_));
                    if events.send(outcome).await.is_err() || failed { break; }
                }
                Some(SessionCommand::Close) | None => break,
            },
            incoming = reader.next() => match incoming {
                Some(Ok(Message::Pong(_))) => awaiting_pong = false,
                Some(Ok(Message::Text(text))) => {
                    if matches!(parse_channel_event(text.as_bytes()), Ok(ChannelEvent::Denied)) {
                        let _ = events.send(SessionEvent::TokenRejected).await; break;
                    }
                    if let Some((request, _)) = &pending {
                        let event = parse(text.as_bytes()); let limit = matches!(event, CandidateEvent::Limit);
                        if observed.try_send(SessionObservation::Frame { sequence: request.sequence, bytes: text.len(), event }).is_err() || limit { break; }
                    }
                }
                Some(Err(WebSocketError::Capacity(_))) => {
                    if let Some((request, _)) = &pending {
                        let _ = observed.try_send(SessionObservation::Frame { sequence: request.sequence, bytes: 0, event: CandidateEvent::Limit });
                    }
                    break;
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                _ => {}
            },
            _ = heartbeat.tick() => {
                if awaiting_pong || !matches!(timeout(WRITE_TIMEOUT, writer.send(Message::Ping(Vec::new().into()))).await, Ok(Ok(()))) { break; }
                awaiting_pong = true;
            }
        }
    }
    let _ = events.send(SessionEvent::Disconnected).await;
    let _ = timeout(WRITE_TIMEOUT, writer.close()).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Writer {
        frames: usize,
        block_flush: bool,
    }
    impl Sink<Message> for Writer {
        type Error = WebSocketError;
        fn poll_ready(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }
        fn start_send(mut self: std::pin::Pin<&mut Self>, _: Message) -> Result<(), Self::Error> {
            self.frames += 1;
            Ok(())
        }
        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            if self.block_flush {
                std::task::Poll::Pending
            } else {
                std::task::Poll::Ready(Ok(()))
            }
        }
        fn poll_close(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }
    }
    #[tokio::test(start_paused = true)]
    async fn cancelled_unsent_request_writes_zero_and_write_timeout_never_retries() {
        let alive = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let request = SessionRequest {
            sequence: 1,
            experiment: WireExperiment::Source(SourceKey::Hdmi1),
            alive: alive.clone(),
        };
        let mut writer = Writer {
            frames: 0,
            block_flush: true,
        };
        assert_eq!(
            write_experiment(&mut writer, &request).await,
            WriteOutcome::NotSent
        );
        assert_eq!(writer.frames, 0);
        alive.store(true, Ordering::SeqCst);
        assert_eq!(
            write_experiment(&mut writer, &request).await,
            WriteOutcome::Uncertain
        );
        assert_eq!(writer.frames, 1);
        let request = SessionRequest {
            sequence: 2,
            experiment: WireExperiment::Ime,
            alive,
        };
        assert_eq!(
            write_experiment(&mut writer, &request).await,
            WriteOutcome::ObservationOnly
        );
        assert_eq!(writer.frames, 1);
    }
    #[test]
    fn candidate_parser_is_bounded_strict_and_never_exports_sensitive_payloads() {
        let valid = br#"{"event":"ed.installedApp.get","data":{"data":[{"appId":"safe.id","name":"App"}]}}"#;
        let CandidateEvent::Catalog(apps) = parse(valid) else {
            panic!("candidate catalog rejected")
        };
        assert_eq!(apps.len(), 1);
        assert!(!format!("{apps:?}").contains("safe.id"));
        for input in [
            br#"{"event":"ed.installedApp.get","data":{"data":[{"appId":"a","appId":"b","name":"App"}]}}"#.as_slice(),
            br#"{"event":"ed.installedApp.get","data":{"data":[{"appId":"unsafe\n","name":"App"}]}}"#,
            br#"{"event":"ed.installedApp.get","data":{"data":[{"appId":"a","name":"App"},{"appId":"a","name":"Other"}]}}"#,
            br#"{"event":"ed.installedApp.get","data":{"data":[{"appId":"a","name":"App"},{"appId":"b","name":"\u202eApp"}]}}"#,
        ] { assert!(matches!(parse(input), CandidateEvent::Malformed)); }
        assert!(matches!(
            parse(&vec![b'x'; MAX_EVENT_BYTES + 1]),
            CandidateEvent::Limit
        ));
        for event in [
            "ms.remote.imeStart",
            "ms.remote.imeEnd",
            "ed.apps.launch",
            "private-unknown",
        ] {
            let frame = json!({"event":event,"data":{"field":"SENSITIVE_MARKER","text":"BASE64_SECRET_MARKER"}}).to_string();
            let projected = format!("{:?}", parse(frame.as_bytes()));
            assert!(!projected.contains("MARKER"));
        }
    }
    #[test]
    fn only_finite_encoders_exist_and_launch_has_no_content_parameters() {
        assert!(encode(&WireExperiment::Ime).is_none());
        let app = DiagnosticApp::new("current.app", "App").unwrap();
        let value: Value =
            serde_json::from_str(&encode(&WireExperiment::Launch(app)).unwrap()).unwrap();
        assert_eq!(
            value["params"]["data"],
            json!({"appId":"current.app","action_type":"NATIVE_LAUNCH"})
        );
        for key in [
            SourceKey::Hdmi1,
            SourceKey::Hdmi2,
            SourceKey::Hdmi3,
            SourceKey::Tuner,
            SourceKey::Sources,
        ] {
            let text = encode(&WireExperiment::Source(key)).unwrap();
            assert!(
                !text.contains("POWER") && !text.contains("ENTER") && !text.contains("SendInput")
            );
        }
    }
}
