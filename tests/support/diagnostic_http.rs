//! Feature-only private route proof using the existing fake listener and owner.
use super::*;
use samsung_tv_remote::application::diagnostic::*;

#[tokio::test]
async fn diagnostic_lease_is_manage_only_exclusive_guarded_and_revoked_on_disconnect() {
    let h = Harness::new().await;
    let id = h.pair().await;
    let state = h.state().await;
    let guards = json!({"server_epoch":state["server_epoch"],"device_id":id,"selection_generation":state["selection_generation"]});
    assert_eq!(h.json(2, "POST", "/_p3/lease", guards.clone()).await.0, 403);
    let response = h.response(1, "POST", "/_p3/lease", guards.clone()).await;
    assert_eq!(response.status(), 200);
    let mut stream = Events {
        body: response.into_body(),
        buffer: Vec::new(),
    };
    let (name, initial) = stream.next().await;
    assert_eq!(name, "diagnostic_lease");
    let lease = initial["lease"].as_str().unwrap();
    assert_eq!(h.json(1, "POST", "/_p3/lease", guards).await.0, 429);
    let key = uuid::Uuid::new_v4().to_string();
    let mut intent =
        json!({"lease":lease,"intent":key,"experiment":"hdmi1","confirm_experiment":"hdmi2"});
    assert_eq!(
        h.json(1, "POST", "/_p3/intent", intent.clone()).await.0,
        400
    );
    assert!(h
        .effects
        .diagnostic_requests
        .lock()
        .unwrap()
        .last_mut()
        .unwrap()
        .try_recv()
        .is_err());
    intent["experiment"] = json!("catalog");
    intent["confirm_experiment"] = json!("catalog");
    assert_eq!(
        h.json(1, "POST", "/_p3/intent", intent.clone()).await.0,
        200
    );
    assert_eq!(h.json(1, "POST", "/_p3/intent", intent).await.0, 200);
    let request = h
        .effects
        .diagnostic_requests
        .lock()
        .unwrap()
        .last_mut()
        .unwrap()
        .try_recv()
        .unwrap();
    assert!(matches!(request.experiment, WireExperiment::Catalog));
    assert!(h
        .effects
        .diagnostic_requests
        .lock()
        .unwrap()
        .last_mut()
        .unwrap()
        .try_recv()
        .is_err());
    let sender = h
        .effects
        .diagnostic_events
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    sender
        .send(SessionObservation::Write(
            request.sequence,
            WriteOutcome::Written,
        ))
        .await
        .unwrap();
    sender
        .send(SessionObservation::Frame {
            sequence: request.sequence,
            bytes: 128,
            event: CandidateEvent::Catalog(vec![
                DiagnosticApp::new("first.id", "First").unwrap(),
                DiagnosticApp::new("second.id", "Second").unwrap(),
            ]),
        })
        .await
        .unwrap();
    sender
        .send(SessionObservation::End(request.sequence))
        .await
        .unwrap();
    let path = format!("/_p3/status?lease={lease}");
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let (_, status) = h.json(1, "GET", &path, Value::Null).await;
            if status["records"][0]["complete"] == true {
                assert_eq!(status["apps"].as_array().unwrap().len(), 2);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        h.json(
            1,
            "POST",
            "/_p3/aliases",
            json!({"lease":lease,"a":0,"b":1})
        )
        .await
        .0,
        200
    );
    let inspection = h.tool(2, "inspect_tv", json!({"device_id":id})).await;
    assert!(inspection["tv"]["capabilities"]["facts"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["support"] == "unknown"));
    assert_eq!(h.effects.writes.load(Ordering::SeqCst), 0);
    drop(stream);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let (_, status) = h.json(1, "GET", &path, Value::Null).await;
            if status["closed"] == true {
                assert_eq!(status["aliases_set"], false);
                assert_eq!(status["apps"], json!([]));
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(h.state().await["connection"], "not_connected");
    h.close().await;
}
