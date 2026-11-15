//! Dedicated lease stream; never starts an owner or submits a TV experiment.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub struct DiagnosticStream {
    pub id: String,
    alive: Arc<AtomicBool>,
    task: JoinHandle<()>,
}
impl DiagnosticStream {
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
}
impl Drop for DiagnosticStream {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl HttpClient {
    pub async fn diagnostic_lease(&self, guards: &Value) -> Result<DiagnosticStream, ClientError> {
        let (response, driver) = timeout(
            Duration::from_secs(5),
            open(
                self.endpoint,
                &self.bearer,
                "POST",
                "/_p3/lease",
                Some(guards),
                None,
            ),
        )
        .await
        .map_err(|_| ClientError::Transport)??;
        if response.status() != 200
            || response
                .headers()
                .get("content-type")
                .and_then(|h| h.to_str().ok())
                != Some("text/event-stream")
        {
            return Err(ClientError::InvalidResponse);
        }
        let mut body = response.into_body();
        let mut bytes = Vec::new();
        let id = timeout(Duration::from_secs(5), async {
            loop {
                let frame = body
                    .frame()
                    .await
                    .ok_or(ClientError::Transport)?
                    .map_err(|_| ClientError::Transport)?;
                let data = frame.data_ref().ok_or(ClientError::InvalidResponse)?;
                if data.len() > 512usize.saturating_sub(bytes.len()) {
                    return Err(ClientError::TooLarge);
                }
                bytes.extend_from_slice(data);
                if let Some(end) = bytes.windows(2).position(|w| w == b"\n\n") {
                    let text = std::str::from_utf8(&bytes[..end])
                        .map_err(|_| ClientError::InvalidResponse)?;
                    let data = text
                        .lines()
                        .find_map(|line| line.strip_prefix("data: "))
                        .ok_or(ClientError::InvalidResponse)?;
                    let value: Value =
                        serde_json::from_str(data).map_err(|_| ClientError::InvalidResponse)?;
                    let id = value["lease"]
                        .as_str()
                        .ok_or(ClientError::InvalidResponse)?;
                    if uuid::Uuid::parse_str(id)
                        .ok()
                        .is_none_or(|u| u.to_string() != id)
                    {
                        return Err(ClientError::InvalidResponse);
                    }
                    return Ok(id.to_owned());
                }
            }
        })
        .await
        .map_err(|_| ClientError::Transport)??;
        let alive = Arc::new(AtomicBool::new(true));
        let live = alive.clone();
        let task = tokio::spawn(async move {
            let _driver = driver;
            while let Ok(Some(Ok(frame))) = timeout(Duration::from_secs(5), body.frame()).await {
                if frame.data_ref().is_none_or(|d| d.len() > 512) {
                    break;
                }
            }
            live.store(false, Ordering::SeqCst);
        });
        Ok(DiagnosticStream { id, alive, task })
    }
}
