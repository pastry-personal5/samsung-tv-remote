//! Private, build-gated management lane. No MCP tool or generic frame route.
use super::*;
use crate::application::diagnostic::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    server_epoch: String,
    device_id: String,
    selection_generation: wire::Counter,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    lease: String,
    intent: String,
    experiment: Experiment,
    confirm_experiment: Experiment,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Choose {
    lease: String,
    a: usize,
    b: usize,
}
fn uuid(value: &str) -> Result<(), ErrorCode> {
    if uuid::Uuid::parse_str(value)
        .ok()
        .is_some_and(|u| u.to_string() == value)
    {
        Ok(())
    } else {
        Err(code(400, "invalid_id"))
    }
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ErrorCode> {
    // Struct decoding rejects duplicate and unknown request fields.
    serde_json::from_slice(bytes).map_err(|_| code(400, "invalid_request"))
}
pub(super) async fn dispatch(
    request: Request<Incoming>,
    client: &AuthorizedClient,
    service: &ControlService,
    streaming: watch::Sender<bool>,
) -> Result<HttpResponse, ErrorCode> {
    client.require_management().map_err(access_error)?;
    let path = request.uri().path().to_owned();
    let query = request.uri().query().map(str::to_owned);
    if request.method() == Method::GET && path == "/_p3/status" {
        let lease = query
            .as_deref()
            .and_then(|q| q.strip_prefix("lease="))
            .ok_or(code(400, "invalid_request"))?;
        uuid(lease)?;
        empty_body(request).await?;
        let status = service
            .diagnostic_status(lease.into())
            .await
            .map_err(response::service_error)?;
        return Ok(json_response(
            200,
            serde_json::to_value(status).map_err(|_| code(503, "diagnostic_failed"))?,
        ));
    }
    if request.method() != Method::POST {
        return Err(code(405, "method_not_allowed"));
    }
    no_query(&query)?;
    let body = read_body(request, true).await?;
    match path.as_str() {
        "/_p3/lease" => {
            let open: Open = decode(&body)?;
            let epoch =
                wire::parse_epoch(&open.server_epoch).map_err(|_| code(400, "invalid_id"))?;
            let target =
                wire::parse_device(&open.device_id).map_err(|_| code(400, "invalid_id"))?;
            let lease = service
                .diagnostic_open(
                    epoch,
                    target,
                    open.selection_generation.0,
                    client.admission.clone(),
                )
                .await
                .map_err(response::service_error)?;
            streaming
                .send(true)
                .map_err(|_| code(503, "server_stopping"))?;
            let first = format!(
                "event: diagnostic_lease\ndata: {}\n\n",
                json!({"lease":lease.id})
            );
            let state = (lease, client.clone(), Some(first));
            let frames = stream::unfold(state, |(lease, client, first)| async move {
                if !lease.alive.load(Ordering::SeqCst) || !client.admission.is_active() {
                    return None;
                }
                let bytes = if let Some(first) = first {
                    first
                } else {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    ": diagnostic\n\n".into()
                };
                Some((
                    Ok::<_, io::Error>(Frame::data(Bytes::from(bytes))),
                    (lease, client, None),
                ))
            });
            Response::builder()
                .status(200)
                .header("content-type", "text/event-stream")
                .header("cache-control", "no-store")
                .body(StreamBody::new(frames).boxed_unsync())
                .map_err(|_| code(503, "diagnostic_failed"))
        }
        "/_p3/intent" => {
            let intent: Intent = decode(&body)?;
            uuid(&intent.lease)?;
            uuid(&intent.intent)?;
            if intent.experiment != intent.confirm_experiment {
                return Err(code(400, "invalid_confirmation"));
            }
            let status = service
                .diagnostic_intent(
                    intent.lease,
                    intent.intent,
                    intent.experiment,
                    client.admission.clone(),
                )
                .await
                .map_err(response::service_error)?;
            Ok(json_response(
                200,
                serde_json::to_value(status).map_err(|_| code(503, "diagnostic_failed"))?,
            ))
        }
        "/_p3/aliases" => {
            let choose: Choose = decode(&body)?;
            uuid(&choose.lease)?;
            let status = service
                .diagnostic_choose(choose.lease, choose.a, choose.b)
                .await
                .map_err(response::service_error)?;
            Ok(json_response(
                200,
                serde_json::to_value(status).map_err(|_| code(503, "diagnostic_failed"))?,
            ))
        }
        _ => Err(code(404, "not_found")),
    }
}
