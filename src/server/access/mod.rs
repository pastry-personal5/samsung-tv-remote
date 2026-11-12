//! Shared HTTP/MCP access policy. Credentials never become registry identities.
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use hyper::http::{HeaderMap, Uri};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::sync::watch;
use tokio::time::Instant;

use crate::application::control_service::{AdmissionAuthorization, ClientId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Control,
    Manage,
    Readiness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessError {
    Forbidden,
    Unauthenticated,
    Busy,
}

struct Credential {
    digest: [u8; 32],
    capability: Capability,
    authorization: AdmissionAuthorization,
    revoked: watch::Sender<bool>,
}

/// Cloning carries a non-secret admission lease and stream cancellation only.
#[derive(Clone)]
pub struct AuthorizedClient {
    pub identity: ClientId,
    pub capability: Capability,
    pub admission: AdmissionAuthorization,
    pub revoked: watch::Receiver<bool>,
    pub readiness_proof: Option<String>,
}
impl AuthorizedClient {
    pub fn require_management(&self) -> Result<(), AccessError> {
        if self.capability == Capability::Manage && self.admission.is_active() {
            Ok(())
        } else {
            Err(AccessError::Forbidden)
        }
    }
}

struct State {
    credentials: HashMap<ClientId, Credential>,
    known: HashSet<ClientId>,
    window: Instant,
    attempts: usize,
    probes: HashMap<String, ReadinessProbe>,
}
struct ReadinessProbe {
    digest: [u8; 32],
    identity: ClientId,
    proof: String,
    expires: Instant,
    consumed: Arc<AtomicBool>,
    authorization: AdmissionAuthorization,
    revoked: watch::Receiver<bool>,
}

pub struct AccessGate {
    authority: String,
    origin: String,
    state: Mutex<State>,
}
impl AccessGate {
    pub fn is_current(&self, identity: ClientId, secret: &[u8; 32]) -> bool {
        let digest: [u8; 32] = Sha256::digest(secret).into();
        self.state
            .lock()
            .expect("access state")
            .credentials
            .get(&identity)
            .is_some_and(|entry| {
                bool::from(entry.digest.ct_eq(&digest)) && entry.authorization.is_active()
            })
    }
    /// Non-secret distinction for an invalidated bootstrap exchange: a rotated
    /// identity may start a fresh exchange; a revoked identity stays blocked.
    pub(crate) fn identity_active(&self, identity: ClientId) -> bool {
        self.state
            .lock()
            .expect("access state")
            .credentials
            .get(&identity)
            .is_some_and(|entry| entry.authorization.is_active())
    }
    pub fn new(port: u16) -> Self {
        let authority = format!("127.0.0.1:{port}");
        Self {
            origin: format!("http://{authority}"),
            authority,
            state: Mutex::new(State {
                credentials: HashMap::new(),
                known: HashSet::new(),
                window: Instant::now(),
                attempts: 0,
                probes: HashMap::new(),
            }),
        }
    }

    /// A single-use, read-only credential whose proof key stays on the
    /// protected bootstrap channel. It cannot authorize a TV route.
    pub fn register_readiness(
        &self,
        identity: ClientId,
        challenge: String,
        secret: &[u8; 32],
        proof: String,
    ) -> Result<Arc<AtomicBool>, AccessError> {
        let mut state = self.state.lock().expect("access state");
        state.probes.retain(|_, p| p.expires > Instant::now());
        if state.probes.len() >= 32 || state.probes.contains_key(&challenge) {
            return Err(AccessError::Busy);
        }
        let credential = state
            .credentials
            .get(&identity)
            .ok_or(AccessError::Unauthenticated)?;
        let consumed = Arc::new(AtomicBool::new(false));
        let probe = ReadinessProbe {
            digest: Sha256::digest(secret).into(),
            identity,
            proof,
            expires: Instant::now() + Duration::from_secs(30),
            consumed: consumed.clone(),
            authorization: credential.authorization.clone(),
            revoked: credential.revoked.subscribe(),
        };
        state.probes.insert(challenge, probe);
        Ok(consumed)
    }
    pub fn forget_readiness(&self, challenge: &str) {
        self.state
            .lock()
            .expect("access state")
            .probes
            .remove(challenge);
    }

    /// Stable identities survive rotation. Revoke old leases before publishing
    /// the replacement. The old receiver closes even with buffered SSE output.
    pub fn rotate(
        &self,
        identity: ClientId,
        capability: Capability,
        secret: &[u8; 32],
    ) -> Result<(), AccessError> {
        let mut state = self.state.lock().expect("access state");
        Self::install(&mut state, identity, capability, secret)
    }

    /// Initial enrollment after a blocked store is distinct from rotation.
    /// A revoked identity leaves a tombstone and can never enter this path.
    pub(crate) fn enroll_initial(
        &self,
        identity: ClientId,
        capability: Capability,
        secret: &[u8; 32],
    ) -> Result<(), AccessError> {
        let mut state = self.state.lock().expect("access state");
        if state.known.contains(&identity) {
            return Err(AccessError::Forbidden);
        }
        Self::install(&mut state, identity, capability, secret)
    }
    fn install(
        state: &mut State,
        identity: ClientId,
        capability: Capability,
        secret: &[u8; 32],
    ) -> Result<(), AccessError> {
        let digest: [u8; 32] = Sha256::digest(secret).into();
        if (!state.known.contains(&identity) && state.known.len() >= 2)
            || state
                .credentials
                .iter()
                .any(|(id, entry)| *id != identity && bool::from(entry.digest.ct_eq(&digest)))
        {
            return Err(AccessError::Forbidden);
        }
        if let Some(old) = state.credentials.remove(&identity) {
            old.authorization.revoke();
            let _ = old.revoked.send(true);
        }
        let (revoked, _) = watch::channel(false);
        state.known.insert(identity);
        state.credentials.insert(
            identity,
            Credential {
                digest,
                capability,
                authorization: AdmissionAuthorization::default(),
                revoked,
            },
        );
        Ok(())
    }

    pub fn revoke(&self, identity: ClientId) {
        let mut state = self.state.lock().expect("access state");
        if state.known.len() < 2 {
            state.known.insert(identity);
        }
        if let Some(old) = state.credentials.remove(&identity) {
            old.authorization.revoke();
            let _ = old.revoked.send(true);
        }
    }

    pub fn authorize(
        &self,
        peer: SocketAddr,
        uri: &Uri,
        headers: &HeaderMap,
    ) -> Result<AuthorizedClient, AccessError> {
        if !matches!(peer.ip(), std::net::IpAddr::V4(ip) if ip == std::net::Ipv4Addr::LOCALHOST)
            || uri.scheme().is_some()
            || uri.authority().is_some()
            || uri.path().len() > 1024
            || uri.to_string().len() > 2048
            || headers.contains_key("forwarded")
            || headers
                .keys()
                .any(|k| k.as_str().starts_with("x-forwarded-"))
        {
            return Err(AccessError::Forbidden);
        }
        let single = |name: &str| -> Result<Option<&str>, AccessError> {
            let mut values = headers.get_all(name).iter();
            let value = values.next();
            if values.next().is_some() {
                return Err(AccessError::Forbidden);
            }
            value
                .map(|v| v.to_str().map_err(|_| AccessError::Forbidden))
                .transpose()
        };
        if single("host")? != Some(self.authority.as_str()) {
            return Err(AccessError::Forbidden);
        }
        if single("origin")?.is_some_and(|v| v != self.origin) {
            return Err(AccessError::Forbidden);
        }
        let digest = single("authorization")?
            .and_then(|value| value.strip_prefix("Bearer "))
            .and_then(credential_digest);
        let mut state = self.state.lock().expect("access state");
        state
            .probes
            .retain(|_, probe| probe.expires > Instant::now());
        if let Some(challenge) = single("x-bootstrap-challenge")? {
            if uri.path() != "/v1/ready" || uri.query().is_some() {
                return Err(AccessError::Forbidden);
            }
            if let Some(probe) = state.probes.get(challenge) {
                if digest.is_some_and(|digest| bool::from(probe.digest.ct_eq(&digest)))
                    && probe.authorization.is_active()
                {
                    let probe = state.probes.remove(challenge).expect("registered probe");
                    probe.consumed.store(true, Ordering::Release);
                    return Ok(AuthorizedClient {
                        identity: probe.identity,
                        capability: Capability::Readiness,
                        admission: probe.authorization,
                        revoked: probe.revoked,
                        readiness_proof: Some(probe.proof),
                    });
                }
            }
            return Err(AccessError::Unauthenticated);
        }
        // The bounded identity set is provisioned by bootstrap, never a bearer.
        let mut found = None;
        for (id, entry) in &state.credentials {
            if bool::from(entry.digest.ct_eq(&digest.unwrap_or([0; 32]))) && digest.is_some() {
                found = Some(AuthorizedClient {
                    identity: *id,
                    capability: entry.capability,
                    admission: entry.authorization.clone(),
                    revoked: entry.revoked.subscribe(),
                    readiness_proof: None,
                });
            }
        }
        if let Some(client) = found {
            return Ok(client);
        }
        // A bounded number of comparisons (two provisioned identities) lets
        // known clients reach their reserved query/management slots even when
        // failed authentication traffic exhausts its response budget.
        if state.window.elapsed() >= Duration::from_secs(1) {
            state.window = Instant::now();
            state.attempts = 0;
        }
        if state.attempts >= 64 {
            return Err(AccessError::Busy);
        }
        state.attempts += 1;
        Err(AccessError::Unauthenticated)
    }
}

fn credential_digest(bearer: &str) -> Option<[u8; 32]> {
    if bearer.len() != 64 {
        return None;
    }
    let mut secret = [0_u8; 32];
    for (i, byte) in secret.iter_mut().enumerate() {
        let pair = bearer.get(i * 2..i * 2 + 2)?;
        if !pair
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
        {
            return None;
        }
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(Sha256::digest(secret).into())
}
