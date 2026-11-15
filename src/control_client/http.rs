//! Bounded native HTTP client. Ordinary calls are never retried here.
#[cfg(feature = "p3-spike")]
#[path = "diagnostic_stream.rs"]
pub mod diagnostic_stream;
use crate::server::bootstrap::BootstrapError;
use crate::server::control_http::response::MAX_OUTPUT_BYTES;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{body::Incoming, Request};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::{timeout, timeout_at, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientError {
    Transport,
    InvalidResponse,
    TooLarge,
    Resnapshot,
    Unauthorized,
    Bootstrap(BootstrapError),
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Endpoint {
    port: u16,
}
impl Endpoint {
    pub fn parse(value: &str) -> Result<Self, ClientError> {
        let port = value
            .strip_prefix("http://127.0.0.1:")
            .and_then(|v| v.parse::<u16>().ok())
            .filter(|v| *v >= 1024)
            .ok_or(ClientError::InvalidResponse)?;
        if value != format!("http://127.0.0.1:{port}") {
            return Err(ClientError::InvalidResponse);
        }
        Ok(Self { port })
    }
    pub fn port(self) -> u16 {
        self.port
    }
    pub fn url(self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}
struct Driver(JoinHandle<()>);
impl Drop for Driver {
    fn drop(&mut self) {
        self.0.abort();
    }
}
#[derive(Clone)]
pub struct HttpClient {
    pub(crate) endpoint: Endpoint,
    pub(crate) bearer: String,
}
impl HttpClient {
    pub async fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, Value), ClientError> {
        request(self.endpoint, &self.bearer, method, path, body, None).await
    }
    pub async fn events(&self) -> Result<Events, ClientError> {
        let (response, driver) = timeout(
            Duration::from_secs(5),
            open(self.endpoint, &self.bearer, "GET", "/v1/events", None, None),
        )
        .await
        .map_err(|_| ClientError::Transport)??;
        Events::from_response(response, driver)
    }
}
pub async fn request(
    endpoint: Endpoint,
    bearer: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
    challenge: Option<&str>,
) -> Result<(u16, Value), ClientError> {
    timeout(Duration::from_secs(5), async {
        let (response, _driver) = open(endpoint, bearer, method, path, body, challenge).await?;
        let status = response.status().as_u16();
        let mut body = response.into_body();
        let mut bytes = Vec::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|_| ClientError::Transport)?;
            let data = frame
                .into_data()
                .map_err(|_| ClientError::InvalidResponse)?;
            if data.len() > MAX_OUTPUT_BYTES.saturating_sub(bytes.len()) {
                return Err(ClientError::TooLarge);
            }
            bytes.extend_from_slice(&data);
        }
        let value = serde_json::from_slice(&bytes).map_err(|_| ClientError::InvalidResponse)?;
        Ok((status, value))
    })
    .await
    .map_err(|_| ClientError::Transport)?
}
async fn open(
    endpoint: Endpoint,
    bearer: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
    challenge: Option<&str>,
) -> Result<(hyper::Response<Incoming>, Driver), ClientError> {
    if !path.starts_with('/') || path.starts_with("//") || path.len() > 2048 {
        return Err(ClientError::InvalidResponse);
    }
    let bytes = body
        .map(serde_json::to_vec)
        .transpose()
        .map_err(|_| ClientError::InvalidResponse)?
        .unwrap_or_default();
    if bytes.len() > crate::server::control_http::request::MAX_BODY_BYTES {
        return Err(ClientError::TooLarge);
    }
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", format!("127.0.0.1:{}", endpoint.port))
        .header("authorization", format!("Bearer {bearer}"));
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    if path == "/mcp" && method == "POST" {
        let body = body.ok_or(ClientError::InvalidResponse)?;
        let rpc_method = body["method"]
            .as_str()
            .ok_or(ClientError::InvalidResponse)?;
        let version = body["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"]
            .as_str()
            .ok_or(ClientError::InvalidResponse)?;
        builder = builder
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", version)
            .header("mcp-method", rpc_method);
        if rpc_method == "tools/call" {
            builder = builder.header(
                "mcp-name",
                body["params"]["name"]
                    .as_str()
                    .ok_or(ClientError::InvalidResponse)?,
            );
        }
    }
    if let Some(challenge) = challenge {
        builder = builder.header("x-bootstrap-challenge", challenge);
    }
    let request = builder
        .body(Full::new(Bytes::from(bytes)))
        .map_err(|_| ClientError::InvalidResponse)?;
    // Connect only after the request is known to be well formed.
    let socket = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, endpoint.port))
        .await
        .map_err(|_| ClientError::Transport)?;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(socket))
        .await
        .map_err(|_| ClientError::Transport)?;
    let driver = Driver(tokio::spawn(async move {
        let _ = connection.await;
    }));
    let response = sender
        .send_request(request)
        .await
        .map_err(|_| ClientError::Transport)?;
    Ok((response, driver))
}

pub struct Event {
    pub kind: String,
    pub state: Value,
}
pub struct Events {
    body: Incoming,
    _driver: Driver,
    buffer: Vec<u8>,
    scan: usize,
    epoch: Option<String>,
    revision: u64,
    read_deadline: Instant,
}
impl Events {
    fn from_response(
        response: hyper::Response<Incoming>,
        driver: Driver,
    ) -> Result<Self, ClientError> {
        if matches!(response.status().as_u16(), 429 | 503) {
            // Capacity and drain races are transient; they must not leave the
            // GUI blocked on supposedly unsafe metadata until it is reopened.
            return Err(ClientError::Transport);
        }
        if response.status() == 401 {
            return Err(ClientError::Unauthorized);
        }
        if response.status() != 200
            || response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                != Some("text/event-stream")
        {
            return Err(ClientError::InvalidResponse);
        }
        Ok(Events {
            body: response.into_body(),
            _driver: driver,
            buffer: Vec::new(),
            scan: 0,
            epoch: None,
            revision: 0,
            read_deadline: Instant::now() + Duration::from_secs(35),
        })
    }

    /// EOF, overflow and revision gaps require a new atomic subscription. This
    /// reader holds no mutation and cannot replay one during reconnect.
    pub async fn next(&mut self) -> Result<Event, ClientError> {
        loop {
            if let Some(end) = self.buffer[self.scan..]
                .windows(2)
                .position(|v| v == b"\n\n")
                .map(|n| n + self.scan)
            {
                let bytes: Vec<_> = self.buffer.drain(..end + 2).collect();
                self.scan = 0;
                let text = std::str::from_utf8(&bytes).map_err(|_| ClientError::InvalidResponse)?;
                if text.starts_with(':') {
                    continue;
                }
                let mut name = None;
                let mut data = None;
                let mut id = None;
                for line in text.lines().filter(|line| !line.is_empty()) {
                    if let Some(value) = line.strip_prefix("event: ") {
                        if name.replace(value).is_some() {
                            return Err(ClientError::InvalidResponse);
                        }
                    } else if let Some(value) = line.strip_prefix("data: ") {
                        if data.replace(value).is_some() {
                            return Err(ClientError::InvalidResponse);
                        }
                    } else if let Some(value) = line.strip_prefix("id: ") {
                        if id.replace(value).is_some() {
                            return Err(ClientError::InvalidResponse);
                        }
                    } else {
                        return Err(ClientError::InvalidResponse);
                    }
                }
                let name = name.ok_or(ClientError::InvalidResponse)?;
                if name == "resync_required" {
                    return Err(ClientError::Resnapshot);
                }
                let state: Value = serde_json::from_str(data.ok_or(ClientError::InvalidResponse)?)
                    .map_err(|_| ClientError::InvalidResponse)?;
                let epoch = state["server_epoch"]
                    .as_str()
                    .ok_or(ClientError::InvalidResponse)?;
                crate::server::control_http::wire::parse_epoch(epoch)
                    .map_err(|_| ClientError::InvalidResponse)?;
                let revision: crate::server::control_http::wire::Counter =
                    serde_json::from_value(state["revision"].clone())
                        .map_err(|_| ClientError::InvalidResponse)?;
                if id != Some(revision.0.to_string().as_str()) {
                    return Err(ClientError::InvalidResponse);
                }
                match &self.epoch {
                    None if name == "snapshot" => {
                        self.epoch = Some(epoch.into());
                    }
                    Some(current)
                        if current == epoch
                            && matches!(name, "state_changed" | "operation_changed")
                            && self.revision.checked_add(1) == Some(revision.0) => {}
                    _ => return Err(ClientError::Resnapshot),
                }
                self.revision = revision.0;
                return Ok(Event {
                    kind: name.into(),
                    state,
                });
            }
            self.scan = self.buffer.len().saturating_sub(1);
            // The connection loop may cancel `next` to service other work.
            // Keep the idle deadline on the stream so cancellations cannot
            // keep a silent connection alive forever. Heartbeats count as
            // activity even though they do not produce an application event.
            let frame = timeout_at(self.read_deadline, self.body.frame())
                .await
                .map_err(|_| ClientError::Transport)?
                .ok_or(ClientError::Resnapshot)?
                .map_err(|_| ClientError::Transport)?;
            let data = frame
                .into_data()
                .map_err(|_| ClientError::InvalidResponse)?;
            if data.len() > (MAX_OUTPUT_BYTES + 128).saturating_sub(self.buffer.len()) {
                return Err(ClientError::TooLarge);
            }
            if !data.is_empty() {
                self.read_deadline = Instant::now() + Duration::from_secs(35);
            }
            self.buffer.extend_from_slice(&data);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

    // Exercise Hyper's real Incoming body without a socket or wall-clock waits.
    async fn event_response(status: u16) -> (Result<Events, ClientError>, DuplexStream) {
        let (client, mut server) = tokio::io::duplex(4096);
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(client))
            .await
            .unwrap();
        let driver = Driver(tokio::spawn(async move {
            let _ = connection.await;
        }));
        let response = sender.send_request(
            Request::builder()
                .uri("/v1/events")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        );
        let serve = async {
            let mut byte = [0];
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                server.read_exact(&mut byte).await.unwrap();
                headers.push(byte[0]);
            }
            server
                .write_all(format!("HTTP/1.1 {status} Test\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").as_bytes())
                .await
                .unwrap();
        };
        let (response, ()) = tokio::join!(response, serve);
        (Events::from_response(response.unwrap(), driver), server)
    }

    async fn silent_events() -> (Events, DuplexStream) {
        let (events, server) = event_response(200).await;
        (events.unwrap(), server)
    }

    #[tokio::test]
    async fn subscription_capacity_and_drain_failures_remain_retryable() {
        for status in [429, 503] {
            let (result, _server) = event_response(status).await;
            assert!(matches!(result, Err(ClientError::Transport)));
        }
        let (result, _server) = event_response(401).await;
        assert!(matches!(result, Err(ClientError::Unauthorized)));
        let (result, _server) = event_response(400).await;
        assert!(matches!(result, Err(ClientError::InvalidResponse)));
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_next_does_not_extend_a_silent_stream_deadline() {
        let (mut events, _server) = silent_events().await;
        let start = Instant::now();
        loop {
            tokio::select! {
                biased;
                result = events.next() => {
                    assert!(matches!(result, Err(ClientError::Transport)));
                    break;
                }
                _ = tokio::time::sleep(Duration::from_secs(1)) => (),
            }
        }
        assert_eq!(start.elapsed(), Duration::from_secs(35));
    }

    #[tokio::test(start_paused = true)]
    async fn heartbeats_refresh_the_idle_deadline_without_producing_events() {
        let (mut events, mut server) = silent_events().await;
        let start = Instant::now();
        let heartbeats = async {
            for _ in 0..2 {
                tokio::time::sleep(Duration::from_secs(20)).await;
                server.write_all(b"d\r\n: heartbeat\n\n\r\n").await.unwrap();
            }
            // Keep the transport open until the reader detects silence.
            std::future::pending::<()>().await;
        };
        tokio::select! {
            result = events.next() => assert!(matches!(result, Err(ClientError::Transport))),
            _ = heartbeats => unreachable!(),
        }
        assert_eq!(start.elapsed(), Duration::from_secs(75));
    }
}
