//! Stateless, authenticated pages. No per-client snapshot registry is created.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::time::Instant;

use crate::application::control_service::{ClientId, Epoch};
use crate::server::control_http::wire;

pub const CURSOR_LIFETIME_MS: u64 = 120_000;

pub struct Cursors {
    secret: [u8; 32],
    started: Instant,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    version: u8,
    client: u64,
    epoch: String,
    query: String,
    source: String,
    offset: usize,
    size: usize,
    expires: u64,
}
pub struct Scope<'a> {
    pub client: ClientId,
    pub epoch: Epoch,
    pub query: &'a str,
    pub source: &'a str,
    pub size: usize,
}
impl Default for Cursors {
    fn default() -> Self {
        Self {
            secret: Sha256::digest(uuid::Uuid::new_v4().as_bytes()).into(),
            started: Instant::now(),
        }
    }
}
impl Cursors {
    fn elapsed(&self) -> u64 {
        self.started.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
    pub fn decode(&self, cursor: Option<&str>, scope: &Scope<'_>) -> Result<usize, &'static str> {
        let Some(cursor) = cursor else {
            return Ok(0);
        };
        if cursor.len() > 1024 {
            return Err("invalid_cursor");
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(cursor)
            .map_err(|_| "invalid_cursor")?;
        if bytes.len() < 32 {
            return Err("invalid_cursor");
        }
        let (payload, signature) = bytes.split_at(bytes.len() - 32);
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).expect("fixed HMAC key");
        mac.update(payload);
        mac.verify_slice(signature).map_err(|_| "invalid_cursor")?;
        let page: Page = serde_json::from_slice(payload).map_err(|_| "invalid_cursor")?;
        if page.version != 1
            || page.client != scope.client.0
            || page.epoch != wire::epoch(scope.epoch)
            || page.query != scope.query
            || page.source != scope.source
            || page.size != scope.size
            || page.expires <= self.elapsed()
        {
            return Err("stale_cursor");
        }
        Ok(page.offset)
    }
    pub fn encode(&self, offset: usize, scope: &Scope<'_>) -> String {
        let page = Page {
            version: 1,
            client: scope.client.0,
            epoch: wire::epoch(scope.epoch),
            query: scope.query.into(),
            source: scope.source.into(),
            offset,
            size: scope.size,
            expires: self.elapsed().saturating_add(CURSOR_LIFETIME_MS),
        };
        let mut payload = serde_json::to_vec(&page).expect("page serialization");
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).expect("fixed HMAC key");
        mac.update(&payload);
        payload.extend_from_slice(&mac.finalize().into_bytes());
        URL_SAFE_NO_PAD.encode(payload)
    }
}

pub fn digest(bytes: &[u8]) -> String {
    crate::server::bootstrap::hex(&Sha256::digest(bytes).into())
}
