//! One bounded HTTP/1 connection parser. Handlers only query/submit the façade.
use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use bytes::Bytes;
use futures_util::stream;
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::http::{Method, Request, Response, Version};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::{watch, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinSet;
use tokio::time::{timeout, Instant};

use super::io::WriteDeadline;
use super::{
    request::{self, MutationRoute, RequestError},
    response::{self, ErrorCode},
    wire,
};
use crate::application::control_service::*;
use crate::server::access::{AccessError, AccessGate, AuthorizedClient};
use crate::server::mcp::enrollment::{Enrollment, EnrollmentError};
use crate::server::mcp::{self, protocol::Reply as McpReply};

pub const MAX_CONNECTIONS: usize = 128;
pub const MAX_HEADERS: usize = 16 * 1024;
pub const MAX_ORDINARY: usize = 64;
pub const MAX_PER_CLIENT: usize = 16;
// Classification is a subset of ordinary capacity, not a second request pool.
pub const MAX_CLASSIFY: usize = 4;
pub const MAX_CLASSIFY_PER_CLIENT: usize = 2;
pub const MAX_MUTATIONS_PER_CLIENT: usize = MAX_PER_CLIENT - 8;
type Body = UnsyncBoxBody<Bytes, io::Error>;
type HttpResponse = Response<Body>;
type RequestReservation = Arc<(OwnedSemaphorePermit, OwnedSemaphorePermit)>;
pub type StopFuture<'a> = Pin<Box<dyn Future<Output = Result<Value, ErrorCode>> + Send + 'a>>;

/// Lifecycle authority is checked again by its implementation, which owns the
/// durable marker/fence. Never pass storage or TV adapters to an HTTP handler.
pub trait StopPort: Send + Sync {
    fn serving_state(&self) -> &'static str {
        "serving"
    }
    fn stop(&self, client: AuthorizedClient, epoch: Epoch, key: String) -> StopFuture<'_>;
}

pub struct HttpAdapter {
    access: Arc<AccessGate>,
    services: HashMap<ClientId, ControlService>,
    stop: Arc<dyn StopPort>,
    build: String,
    ordinary: Arc<Semaphore>,
    queries: Arc<Semaphore>,
    management: Arc<Semaphore>,
    classification: Arc<Semaphore>,
    clients: HashMap<ClientId, [Arc<Semaphore>; 4]>,
    mcp: mcp::Adapter,
    enrollment: Option<Arc<Enrollment>>,
}
impl HttpAdapter {
    pub fn new(
        access: Arc<AccessGate>,
        services: [(ClientId, ControlService); 2],
        stop: Arc<dyn StopPort>,
        build: String,
    ) -> Self {
        let clients = services
            .iter()
            .map(|(id, _)| {
                (
                    *id,
                    [
                        Arc::new(Semaphore::new(MAX_MUTATIONS_PER_CLIENT)),
                        Arc::new(Semaphore::new(4)),
                        Arc::new(Semaphore::new(2)),
                        Arc::new(Semaphore::new(MAX_CLASSIFY_PER_CLIENT)),
                    ],
                )
            })
            .collect();
        let mcp = mcp::Adapter::new(services.iter().map(|(id, _)| *id));
        Self {
            access,
            services: services.into_iter().collect(),
            stop,
            build,
            ordinary: Arc::new(Semaphore::new(MAX_ORDINARY - 12 - MAX_CLASSIFY)),
            queries: Arc::new(Semaphore::new(8)),
            management: Arc::new(Semaphore::new(4)),
            classification: Arc::new(Semaphore::new(MAX_CLASSIFY)),
            clients,
            mcp,
            enrollment: None,
        }
    }
    pub fn with_enrollment(mut self, enrollment: Arc<Enrollment>) -> Self {
        self.enrollment = Some(enrollment);
        self
    }

    /// The composition owns listener and runtime lifetimes independently.
    /// Shutdown closes connections but never cancels admitted owner work.
    pub async fn serve(
        self: Arc<Self>,
        listener: TcpListener,
        mut shutdown: watch::Receiver<bool>,
    ) -> io::Result<()> {
        if !matches!(listener.local_addr()?.ip(),std::net::IpAddr::V4(ip) if ip==Ipv4Addr::LOCALHOST)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "literal loopback listener required",
            ));
        }
        let connections = Arc::new(Semaphore::new(MAX_CONNECTIONS));
        let mut tasks = JoinSet::new();
        if *shutdown.borrow() {
            return Ok(());
        }
        loop {
            tokio::select! { biased;
                _=shutdown.changed()=>break,
                done=tasks.join_next(),if !tasks.is_empty()=>{ let _=done; },
                accepted=listener.accept()=>{
                    let (socket,peer)=accepted?;
                    let Ok(slot)=connections.clone().try_acquire_owned() else { drop(socket);continue; };
                    let adapter=self.clone();
                    let mut closed=shutdown.clone();
                    tasks.spawn(async move {
                        let _slot=slot;
                        let (authorized,mut auth_rx)=watch::channel(None::<AuthorizedClient>);
                        let (streaming,stream_rx)=watch::channel(false);
                        let (reserved,_reservation_lifetime)=watch::channel(None::<RequestReservation>);
                        let framing=Arc::new(AtomicBool::new(false));
                        let audit=framing.clone();
                        let service=service_fn(move |request| {
                            let adapter=adapter.clone(); let authorized=authorized.clone();let streaming=streaming.clone();let audit=audit.clone();let reserved=reserved.clone();
                            async move { Ok::<_,Infallible>(adapter.handle(peer,request,authorized,streaming,audit,reserved).await) }
                        });
                        let mut builder=http1::Builder::new();
                        builder.keep_alive(false).half_close(false).max_headers(64).max_header_size(MAX_HEADERS).max_buf_size(MAX_HEADERS)
                            .header_read_timeout(Duration::from_secs(5)).timer(TokioTimer::new()).pipeline_flush(false);
                        let connection=builder.serve_connection(TokioIo::new(WriteDeadline::new(socket,framing)),service);
                        tokio::pin!(connection);
                        let revoked=async move {
                            let client=match auth_rx.wait_for(|v|v.is_some()).await { Ok(value)=>value.as_ref().expect("authorized").clone(), Err(_)=>return };
                            let mut changes=client.revoked;
                            if !client.admission.is_active() || *changes.borrow() {return;}
                            let _=changes.changed().await;
                        };
                        tokio::pin!(revoked);
                        let ordinary_deadline=tokio::time::sleep(Duration::from_secs(30));
                        tokio::pin!(ordinary_deadline);
                        let mut ordinary_expired=false;
                        loop {
                            tokio::select! { biased;
                                _=closed.changed()=>{
                                    if !*stream_rx.borrow() {
                                        connection.as_mut().graceful_shutdown();
                                        let _=timeout(Duration::from_secs(10),&mut connection).await;
                                    }
                                    break;
                                },
                                _=&mut revoked=>break,
                                result=&mut connection=>{let _=result;break;},
                                _=&mut ordinary_deadline,if !ordinary_expired=>{
                                    if !*stream_rx.borrow(){break;}
                                    ordinary_expired=true;
                                }
                            }
                        }
                    });
                }
            }
        }
        if timeout(Duration::from_secs(11), async {
            while tasks.join_next().await.is_some() {}
        })
        .await
        .is_err()
        {
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
        }
        Ok(())
    }

    async fn handle(
        &self,
        peer: SocketAddr,
        request: Request<Incoming>,
        authorized: watch::Sender<Option<AuthorizedClient>>,
        streaming: watch::Sender<bool>,
        invalid_framing: Arc<AtomicBool>,
        reserved: watch::Sender<Option<RequestReservation>>,
    ) -> HttpResponse {
        let is_mcp = request.uri().path() == "/mcp";
        let reject = |error: ErrorCode| {
            if is_mcp {
                mcp_response(McpReply::error(
                    error.status,
                    None,
                    -32600,
                    "HTTP request rejected",
                ))
            } else {
                failure(error)
            }
        };
        let client = match self
            .access
            .authorize(peer, request.uri(), request.headers())
        {
            Ok(client) => client,
            Err(e) => return reject(access_error(e)),
        };
        let _ = authorized.send(Some(client.clone()));
        if invalid_framing.load(Ordering::Acquire) {
            return reject(code(400, "ambiguous_framing"));
        }
        let result = timeout(
            Duration::from_secs(15),
            self.dispatch(request, &client, streaming, reserved),
        )
        .await;
        if !client.admission.is_active() {
            return reject(code(401, "unauthenticated"));
        }
        match result {
            Ok(Ok(response)) => response,
            Ok(Err(e)) => reject(e),
            Err(_) => reject(code(503, "request_timeout")),
        }
    }

    fn slots(
        &self,
        client: ClientId,
        read: bool,
        management: bool,
    ) -> Result<(OwnedSemaphorePermit, OwnedSemaphorePermit), ErrorCode> {
        // Read/lifecycle reservations do not compete with ordinary mutations.
        let pool = if management {
            &self.management
        } else if read {
            &self.queries
        } else {
            &self.ordinary
        };
        let global = pool
            .clone()
            .try_acquire_owned()
            .map_err(|_| code(429, "busy"))?;
        let per = self.clients.get(&client).ok_or(code(403, "forbidden"))?[if management {
            2
        } else if read {
            1
        } else {
            0
        }]
        .clone()
        .try_acquire_owned()
        .map_err(|_| code(429, "busy"))?;
        Ok((global, per))
    }

    async fn dispatch(
        &self,
        request: Request<Incoming>,
        client: &AuthorizedClient,
        streaming: watch::Sender<bool>,
        reserved: watch::Sender<Option<RequestReservation>>,
    ) -> Result<HttpResponse, ErrorCode> {
        // Bound MCP waiting by the original request, including body parsing.
        let mcp_deadline = Instant::now() + Duration::from_secs(14);
        if request.version() != Version::HTTP_11 {
            return Err(code(400, "http_version_unsupported"));
        }
        let path = request.uri().path().to_owned();
        let query = request.uri().query().map(str::to_owned);
        let method = request.method().clone();
        let management = matches!(
            path.as_str(),
            "/v1/server/stop" | "/v1/mcp" | "/v1/mcp/enrollment"
        );
        if management {
            client.require_management().map_err(access_error)?;
        }
        let service = self
            .services
            .get(&client.identity)
            .ok_or(code(403, "forbidden"))?;
        if matches!(path.as_str(), "/v1/mcp" | "/v1/mcp/enrollment") {
            no_query(&query)?;
            let enrollment = self
                .enrollment
                .as_ref()
                .ok_or(code(503, "mcp_enrollment_unavailable"))?
                .clone();
            let slots = Arc::new(self.slots(client.identity, false, true)?);
            reserved
                .send(Some(slots))
                .map_err(|_| code(503, "server_stopping"))?;
            let result = match (method.as_str(), path.as_str()) {
                ("GET", "/v1/mcp") => {
                    empty_body(request).await?;
                    tokio::task::spawn_blocking(move || enrollment.status_summary()).await
                }
                ("POST", "/v1/mcp/enrollment") => {
                    let body = read_body(request, true).await?;
                    let current = service.snapshot().await.map_err(response::service_error)?;
                    let (key, generation, enabled) =
                        request::parse_mcp_enrollment(&body, current.epoch)
                            .map_err(request_error)?;
                    let client = client.clone();
                    // Enrollment retains a duplicate of profile ownership. A
                    // dropped HTTP future cannot release the lock under this
                    // blocking durable transition or cancel an admitted grant.
                    tokio::task::spawn_blocking(move || {
                        enrollment
                            .change_authorized(&client, generation, &key, enabled)
                            .map(|record| record.summary())
                    })
                    .await
                }
                _ => return Err(code(405, "method_not_allowed")),
            };
            let value = result
                .map_err(|_| code(503, "mcp_management_failed"))?
                .map_err(enrollment_error)?;
            return Ok(json_response(200, value));
        }
        if path == "/mcp" {
            no_query(&query)?;
            if method != Method::POST {
                let mut response = mcp_response(McpReply::error(405, None, -32600, "Use POST"));
                response
                    .headers_mut()
                    .insert("allow", "POST".parse().expect("static method"));
                return Ok(response);
            }
            if client.capability == crate::server::access::Capability::Readiness {
                return Err(code(403, "forbidden"));
            }
            if !mcp::protocol::accepts_json_and_events(request.headers()) {
                return Ok(mcp_response(McpReply::error(
                    406,
                    None,
                    -32600,
                    "Accept JSON and event-stream responses",
                )));
            }
            let global = self
                .classification
                .clone()
                .try_acquire_owned()
                .map_err(|_| code(429, "busy"))?;
            let per = self
                .clients
                .get(&client.identity)
                .ok_or(code(403, "forbidden"))?[3]
                .clone()
                .try_acquire_owned()
                .map_err(|_| code(429, "busy"))?;
            reserved
                .send(Some(Arc::new((global, per))))
                .map_err(|_| code(503, "server_stopping"))?;
            let headers = request.headers().clone();
            let body = read_body(request, true).await?;
            let rpc = match mcp::protocol::parse(&headers, &body) {
                Ok(rpc) => rpc,
                Err(reply) => return Ok(mcp_response(reply)),
            };
            // No await between releasing parsing capacity and taking semantic
            // capacity. A failed transfer rejects promptly; it never nests pools.
            reserved
                .send(None)
                .map_err(|_| code(503, "server_stopping"))?;
            let slots = match self.slots(client.identity, rpc.is_read(), false) {
                Ok(slots) => Arc::new(slots),
                Err(error) => {
                    return Ok(mcp_response(McpReply::error(
                        error.status,
                        Some(&rpc.id),
                        1001,
                        "Request capacity occupied",
                    )))
                }
            };
            reserved
                .send(Some(slots))
                .map_err(|_| code(503, "server_stopping"))?;
            let id = rpc.id.clone();
            let result = tokio::time::timeout_at(
                mcp_deadline,
                self.mcp
                    .dispatch(rpc, service, client, &self.build, mcp_deadline),
            )
            .await;
            return Ok(mcp_response(result.unwrap_or_else(|_| {
                McpReply::error(
                    503,
                    Some(&id),
                    -32603,
                    "Request timed out; recover by original operation key",
                )
            })));
        }
        // SSE uses owner subscriber limits (64 total/four per identity), not the
        // ordinary request reservation, for its lifetime.
        if method == Method::GET && path == "/v1/events" {
            no_query(&query)?;
            empty_body(request).await?;
            let subscription = service.subscribe().await.map_err(response::service_error)?;
            if !client.admission.is_active() {
                return Err(code(401, "unauthenticated"));
            }
            let _ = streaming.send(true);
            return events(subscription, client.clone());
        }
        let slots = Arc::new(self.slots(client.identity, method == Method::GET, management)?);
        // Keep the reservation in the connection's watch state through the
        // final flush/drop. Releasing it at handler return lets stalled large
        // responses bypass per-identity limits and accumulate up to the much
        // larger connection cap. SSE uses its separate subscriber budget.
        reserved
            .send(Some(slots))
            .map_err(|_| code(503, "server_stopping"))?;
        if method == Method::GET {
            empty_body(request).await?;
            let snapshot = service
                .client_snapshot()
                .await
                .map_err(response::service_error)?;
            return match path.as_str() {
                "/v1/ready" => {
                    no_query(&query)?;
                    Ok(json_response(
                        200,
                        json!({"server_epoch":wire::epoch(snapshot.snapshot.epoch),"api_version":"v1","management_version":"v1","build":self.build,"serving":self.stop.serving_state()=="serving" && !snapshot.snapshot.admission_closed,"bootstrap_proof":client.readiness_proof,"lifecycle":self.stop.serving_state(),"startup":response::startup(snapshot.snapshot.startup)}),
                    ))
                }
                "/v1/state" => {
                    no_query(&query)?;
                    let mut state = response::snapshot(&snapshot.snapshot);
                    state["recent_operations"] = json!(snapshot
                        .recent_operations
                        .into_iter()
                        .map(wire::operation)
                        .collect::<Vec<_>>());
                    Ok(json_response(200, state))
                }
                _ => {
                    if let Some(key) = path.strip_prefix("/v1/operations/by-key/") {
                        let epoch = query_epoch(query.as_deref())?;
                        canonical_key(key)?;
                        let lookup = service
                            .lookup(epoch, key.to_owned())
                            .await
                            .map_err(response::service_error)?;
                        lookup_response(lookup)
                    } else if let Some(id) = path.strip_prefix("/v1/operations/") {
                        let epoch = query_epoch(query.as_deref())?;
                        // A changed epoch must report uncertainty rather than
                        // resolve the same counter in the replacement owner.
                        if epoch != snapshot.snapshot.epoch {
                            return Err(code(410, "unknown_after_restart"));
                        }
                        let id = wire::parse_operation(id, epoch, snapshot.snapshot.epoch)
                            .map_err(|_| code(400, "invalid_id"))?;
                        lookup_response(
                            service
                                .operation(epoch, id)
                                .await
                                .map_err(response::service_error)?,
                        )
                    } else if let Some(id) = path.strip_prefix("/v1/devices/") {
                        no_query(&query)?;
                        let id = wire::parse_device(id).map_err(|_| code(400, "invalid_id"))?;
                        let device = snapshot
                            .snapshot
                            .devices
                            .iter()
                            .find(|d| d.id == id)
                            .ok_or(code(404, "not_found"))?;
                        let revision = snapshot
                            .snapshot
                            .configuration_revisions
                            .iter()
                            .find(|(d, _)| *d == id)
                            .map_or(0, |(_, r)| *r);
                        Ok(json_response(200, response::device(device, revision)))
                    } else {
                        Err(code(404, "not_found"))
                    }
                }
            };
        }
        no_query(&query)?;
        let route = mutation_route(&method, &path);
        if route.is_none() && !(management && method == Method::POST) {
            return Err(code(404, "not_found"));
        }
        let body = read_body(request, true).await?;
        let current = service.snapshot().await.map_err(response::service_error)?;
        if management {
            let (epoch, key) = request::parse_stop(&body, current.epoch).map_err(request_error)?;
            let result = self.stop.stop(client.clone(), epoch, key).await?;
            return Ok(json_response(200, result));
        }
        let request =
            match request::parse_mutation(route.expect("known route"), &body, current.epoch) {
                Ok(request) => request,
                Err(RequestError::StaleEpoch) => {
                    let mut value = request_error(RequestError::StaleEpoch).body();
                    value["current"] = current_preconditions(&current);
                    return Ok(json_response(409, value));
                }
                Err(error) => return Err(request_error(error)),
            };
        let receipt = service
            .submit_authorized(request, client.admission.clone())
            .await
            .map_err(response::service_error)?;
        let (status, mut result) = response::receipt(&receipt);
        if matches!(status, 409 | 412 | 423) {
            if let Ok(snapshot) = service.snapshot().await {
                result["current"] = current_preconditions(&snapshot);
            }
        }
        Ok(json_response(status, result))
    }
}

fn current_preconditions(snapshot: &Snapshot) -> Value {
    json!({"server_epoch":wire::epoch(snapshot.epoch),"selected":snapshot.control.selected_device().map(wire::device),"selection_generation":wire::Counter(snapshot.control.selection_generation())})
}
fn code(status: u16, code: &'static str) -> ErrorCode {
    ErrorCode { status, code }
}
fn access_error(error: AccessError) -> ErrorCode {
    match error {
        AccessError::Forbidden => code(403, "forbidden"),
        AccessError::Unauthenticated => code(401, "unauthenticated"),
        AccessError::Busy => code(429, "busy"),
    }
}
fn enrollment_error(error: EnrollmentError) -> ErrorCode {
    let status = match error {
        EnrollmentError::Denied => 403,
        EnrollmentError::StaleGeneration | EnrollmentError::KeyConflict => 409,
        EnrollmentError::UnsafeMetadata | EnrollmentError::Credential(_) => 423,
        EnrollmentError::Persistence
        | EnrollmentError::RevocationPending
        | EnrollmentError::Exhausted => 503,
    };
    code(status, error.code())
}
pub(crate) fn request_error(error: RequestError) -> ErrorCode {
    match error {
        RequestError::InvalidRequest => code(400, "invalid_request"),
        RequestError::StaleEpoch => code(409, "stale_epoch"),
        RequestError::InvalidId => code(400, "invalid_id"),
        RequestError::InvalidTarget => code(400, "invalid_target"),
        RequestError::InvalidConfirmation => code(400, "invalid_confirmation"),
        RequestError::TooLarge => code(413, "input_too_large"),
    }
}
fn no_query(query: &Option<String>) -> Result<(), ErrorCode> {
    if query.is_some() {
        Err(code(400, "invalid_request"))
    } else {
        Ok(())
    }
}
fn query_epoch(query: Option<&str>) -> Result<Epoch, ErrorCode> {
    let value = query
        .and_then(|q| q.strip_prefix("epoch="))
        .filter(|v| !v.contains('&'))
        .ok_or(code(400, "invalid_request"))?;
    wire::parse_epoch(value).map_err(|_| code(400, "invalid_id"))
}
fn canonical_key(key: &str) -> Result<(), ErrorCode> {
    let id = uuid::Uuid::parse_str(key).map_err(|_| code(400, "invalid_id"))?;
    if id.hyphenated().to_string() != key {
        return Err(code(400, "invalid_id"));
    }
    Ok(())
}
fn lookup_response(lookup: Lookup) -> Result<HttpResponse, ErrorCode> {
    match lookup {
        Lookup::Known(receipt) => {
            let (status, value) = response::receipt(&receipt);
            Ok(json_response(status, value))
        }
        Lookup::UnknownAfterRestart => Err(code(410, "unknown_after_restart")),
        Lookup::OutcomeUnavailable => Err(code(410, "outcome_unavailable")),
    }
}

fn mutation_route<'a>(method: &Method, path: &'a str) -> Option<MutationRoute<'a>> {
    if *method == Method::POST {
        let route = match path {
            "/v1/discoveries" => Some(MutationRoute::Discover),
            "/v1/probes" => Some(MutationRoute::Probe),
            "/v1/pairings" => Some(MutationRoute::Pair),
            "/v1/selection" => Some(MutationRoute::Select),
            "/v1/connections" => Some(MutationRoute::Connect),
            "/v1/remote-actions" => Some(MutationRoute::Remote),
            "/v1/wakes" => Some(MutationRoute::Wake),
            _ => None,
        };
        if route.is_some() {
            return route;
        }
        if let Some(id) = path
            .strip_prefix("/v1/wakes/")
            .and_then(|v| v.strip_suffix("/cancel"))
        {
            return Some(MutationRoute::CancelWake(id));
        }
        if let Some(suffix) = path.strip_prefix("/v1/devices/") {
            if let Some(id) = suffix.strip_suffix("/forget") {
                return Some(MutationRoute::Forget(id));
            }
            if let Some(id) = suffix.strip_suffix("/recover-forget") {
                return Some(MutationRoute::RecoverForget(id));
            }
            if let Some(id) = suffix.strip_suffix("/recover-pairing") {
                return Some(MutationRoute::RecoverPairing(id));
            }
        }
    } else if *method == Method::PUT {
        if let Some(id) = path
            .strip_prefix("/v1/devices/")
            .and_then(|v| v.strip_suffix("/wake"))
        {
            return Some(MutationRoute::SetWake(id));
        }
    }
    None
}

async fn empty_body(request: Request<Incoming>) -> Result<(), ErrorCode> {
    if read_body(request, false).await?.is_empty() {
        Ok(())
    } else {
        Err(code(400, "unexpected_body"))
    }
}
async fn read_body(request: Request<Incoming>, json: bool) -> Result<Vec<u8>, ErrorCode> {
    let headers = request.headers();
    for name in [
        "content-length",
        "transfer-encoding",
        "content-type",
        "content-encoding",
    ] {
        if headers.get_all(name).iter().count() > 1 {
            return Err(code(400, "invalid_request"));
        }
    }
    if headers.contains_key("content-length") && headers.contains_key("transfer-encoding") {
        return Err(code(400, "ambiguous_framing"));
    }
    if headers.contains_key("content-encoding") {
        return Err(code(415, "content_encoding_unsupported"));
    }
    if json && headers.get("content-type").and_then(|v| v.to_str().ok()) != Some("application/json")
    {
        return Err(code(415, "content_type_unsupported"));
    }
    if headers
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
        .is_some_and(|n| n > request::MAX_BODY_BYTES)
    {
        return Err(code(413, "input_too_large"));
    }
    let mut incoming = request.into_body();
    timeout(Duration::from_secs(10), async move {
        let mut bytes = Vec::new();
        while let Some(frame) = incoming.frame().await {
            let frame = frame.map_err(|_| code(400, "invalid_body"))?;
            let data = frame
                .into_data()
                .map_err(|_| code(400, "trailers_unsupported"))?;
            if data.len() > request::MAX_BODY_BYTES.saturating_sub(bytes.len()) {
                return Err(code(413, "input_too_large"));
            }
            bytes.extend_from_slice(&data);
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| code(408, "body_timeout"))?
}

fn failure(error: ErrorCode) -> HttpResponse {
    json_response(error.status, error.body())
}
fn json_response(status: u16, value: Value) -> HttpResponse {
    json_response_limited(status, value, response::MAX_OUTPUT_BYTES)
}
fn mcp_response(reply: McpReply) -> HttpResponse {
    match response::encode(&reply.body, mcp::MAX_RESULT_BYTES) {
        Ok(bytes) => json_bytes(reply.status, bytes),
        Err(_) => json_response(
            503,
            McpReply::error(
                503,
                reply.body.get("id"),
                -32603,
                "Response too large; recover by original operation key",
            )
            .body,
        ),
    }
}
fn json_response_limited(status: u16, value: Value, cap: usize) -> HttpResponse {
    let (status, bytes) = match response::encode(&value, cap) {
        Ok(bytes) => (status, bytes),
        Err(error) => (
            error.status,
            serde_json::to_vec(&error.body()).expect("static error"),
        ),
    };
    json_bytes(status, bytes)
}
fn json_bytes(status: u16, bytes: Vec<u8>) -> HttpResponse {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .header("connection", "close")
        .body(
            Full::new(Bytes::from(bytes))
                .map_err(|never| match never {})
                .boxed_unsync(),
        )
        .expect("static response")
}
fn sse_frame(name: &str, id: u64, value: &Value) -> Result<Frame<Bytes>, io::Error> {
    let encoded = response::encode(value, response::MAX_OUTPUT_BYTES)
        .map_err(|_| io::Error::other("event output limit"))?;
    let mut bytes = format!("event: {name}\nid: {id}\ndata: ").into_bytes();
    bytes.extend_from_slice(&encoded);
    bytes.extend_from_slice(b"\n\n");
    Ok(Frame::data(Bytes::from(bytes)))
}
fn events(subscription: Subscription, client: AuthorizedClient) -> Result<HttpResponse, ErrorCode> {
    let mut initial = response::snapshot(&subscription.snapshot);
    initial["recent_operations"] = json!(subscription
        .recent_operations
        .iter()
        .copied()
        .map(wire::operation)
        .collect::<Vec<_>>());
    let first = sse_frame("snapshot", subscription.snapshot.revision, &initial)
        .map_err(|_| code(503, "response_too_large"))?;
    struct State {
        first: Option<Frame<Bytes>>,
        events: tokio::sync::mpsc::Receiver<Event>,
        client: AuthorizedClient,
        revision: u64,
        heartbeat: Instant,
        closed: bool,
    }
    let state = State {
        first: Some(first),
        events: subscription.events,
        client,
        revision: subscription.snapshot.revision,
        heartbeat: Instant::now() + Duration::from_secs(15),
        closed: false,
    };
    let stream = stream::unfold(state, |mut state| async move {
        if state.closed || !state.client.admission.is_active() {
            return None;
        }
        if let Some(first) = state.first.take() {
            return Some((Ok(first), state));
        }
        if state.events.is_closed() {
            state.closed = true;
            let frame = sse_frame(
                "resync_required",
                state.revision,
                &json!({"reason":"subscription_closed"}),
            );
            return Some((frame, state));
        }
        let frame = tokio::select! {biased;
            _=state.client.revoked.changed()=>return None,
            event=state.events.recv()=>{
                match event {
                    Some(event) if event.snapshot.revision==state.revision+1=>{
                        state.revision=event.snapshot.revision;
                        let mut value=response::snapshot(&event.snapshot);
                        value["operation_id"]=json!(event.operation.map(wire::operation));
                        value["phase"]=json!(event.phase.map(|v|match v{OperationPhase::Pending=>"pending",OperationPhase::Terminal=>"terminal"}));
                        sse_frame(if event.operation.is_some(){"operation_changed"}else{"state_changed"},state.revision,&value)
                    }
                    _=>{state.closed=true;sse_frame("resync_required",state.revision,&json!({"reason":"subscription_closed"}))}
                }
            },
            _=tokio::time::sleep_until(state.heartbeat)=>{state.heartbeat=Instant::now()+Duration::from_secs(15);Ok(Frame::data(Bytes::from_static(b": heartbeat\n\n")))}
        };
        if !state.client.admission.is_active() {
            return None;
        }
        Some((frame, state))
    });
    Ok(Response::builder()
        .status(200)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-store")
        .header("connection", "close")
        .body(StreamBody::new(stream).boxed_unsync())
        .expect("static event response"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{
        runtime::{Runtime, RuntimeLimits},
        tv_control_coordinator::AppServices,
    };
    use crate::server::access::Capability;
    #[tokio::test]
    async fn overflow_discards_buffered_events_and_requests_an_atomic_resnapshot() {
        let runtime = Runtime::start(
            AppServices::without_adapters(),
            RuntimeLimits {
                subscriber_backlog: 1,
                ..RuntimeLimits::default()
            },
        );
        let service = runtime.client(ClientId(1));
        let subscription = service.subscribe().await.unwrap();
        let epoch = subscription.snapshot.epoch;
        for key in ["overflow-1", "overflow-2", "overflow-3"] {
            service
                .submit(crate::application::control_service::Request {
                    epoch,
                    key: key.into(),
                    command: Command::Discover,
                })
                .await
                .unwrap();
        }
        assert!(subscription.events.is_closed());
        let gate = AccessGate::new(54321);
        gate.rotate(ClientId(1), Capability::Manage, &[1; 32])
            .unwrap();
        let mut headers = hyper::http::HeaderMap::new();
        headers.insert("host", "127.0.0.1:54321".parse().unwrap());
        headers.insert(
            "authorization",
            format!("Bearer {}", "01".repeat(32)).parse().unwrap(),
        );
        let client = gate
            .authorize(
                "127.0.0.1:1".parse().unwrap(),
                &"/v1/events".parse().unwrap(),
                &headers,
            )
            .unwrap();
        let mut body = events(subscription, client).unwrap().into_body();
        let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert!(first.starts_with(b"event: snapshot\n"));
        let closed = body.frame().await.unwrap().unwrap().into_data().unwrap();
        assert!(closed.starts_with(b"event: resync_required\n"));
        assert!(body.frame().await.is_none());
        runtime.drain().await.settled().await.unwrap();
    }
}
