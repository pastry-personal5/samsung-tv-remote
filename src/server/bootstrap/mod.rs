//! Protected development bootstrap. Signed peer verification is an injected M9
//! port; the M8 adapter explicitly verifies only the same effective user.
pub mod socket;
use crate::application::control_service::ClientId;
use crate::server::access::{AccessGate, Capability};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::{atomic::Ordering, Arc, Mutex};
use std::time::Duration;
use subtle::ConstantTimeEq;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{watch, Semaphore};
use tokio::task::JoinSet;
use tokio::time::{timeout, Instant};

pub const MAX_FRAME: usize = 4096;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Gui,
    Mcp,
}
impl Role {
    pub fn identity(self) -> ClientId {
        match self {
            Self::Gui => ClientId(1),
            Self::Mcp => ClientId(2),
        }
    }
    pub fn capability(self) -> Capability {
        match self {
            Self::Gui => Capability::Manage,
            Self::Mcp => Capability::Control,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapError {
    Denied,
    InteractionRequired,
    CredentialUnavailable,
    InvalidFrame,
    Replay,
    Busy,
    Expired,
    UntrustedListener,
    Unavailable,
    IncompatibleBuild,
    UnsafeMetadata,
}

pub trait ApiCredentials: Send + Sync {
    fn load(&self, role: Role) -> Result<[u8; 32], BootstrapError>;
}
pub struct MemoryCredentials {
    gui: [u8; 32],
    mcp: [u8; 32],
}
impl MemoryCredentials {
    pub fn random() -> Result<Self, BootstrapError> {
        Ok(Self {
            gui: random()?,
            mcp: random()?,
        })
    }
}
impl ApiCredentials for MemoryCredentials {
    fn load(&self, role: Role) -> Result<[u8; 32], BootstrapError> {
        Ok(match role {
            Role::Gui => self.gui,
            Role::Mcp => self.mcp,
        })
    }
}

pub trait PeerVerifier: Send + Sync {
    fn verify(&self, stream: &UnixStream, role: Role) -> Result<(), BootstrapError>;
}
/// Fake trust for explicit development composition only. It is not signed code
/// identity verification and is never installed in a production composition.
pub struct DevelopmentPeer {
    pub uid: u32,
}
impl PeerVerifier for DevelopmentPeer {
    fn verify(&self, stream: &UnixStream, _role: Role) -> Result<(), BootstrapError> {
        if stream
            .peer_cred()
            .map_err(|_| BootstrapError::Denied)?
            .uid()
            == self.uid
        {
            Ok(())
        } else {
            Err(BootstrapError::Denied)
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub profile: String,
    pub endpoint: String,
    pub build: String,
    pub api: String,
    pub epoch: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub version: u8,
    pub role: Role,
    pub nonce: String,
    pub profile: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub version: u8,
    pub role: Role,
    pub nonce: String,
    pub challenge: String,
    pub context: Context,
    pub probe_bearer: String,
    pub proof_key: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Redeem {
    pub challenge: String,
    pub proof: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub role: Role,
    pub context: Context,
    pub bearer: String,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
pub enum ClientFrame {
    Hello(Hello),
    Redeem(Redeem),
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
pub enum ServerFrame {
    Challenge(Challenge),
    Credentials(Credentials),
    Error(BootstrapError),
}

pub struct Bootstrap {
    access: Arc<AccessGate>,
    store: Arc<dyn ApiCredentials>,
    verifier: Arc<dyn PeerVerifier>,
    context: Context,
    nonces: Mutex<HashMap<String, Instant>>,
}
impl Bootstrap {
    pub fn new(
        access: Arc<AccessGate>,
        store: Arc<dyn ApiCredentials>,
        verifier: Arc<dyn PeerVerifier>,
        context: Context,
    ) -> Result<Self, BootstrapError> {
        for role in [Role::Gui, Role::Mcp] {
            if let Ok(secret) = store.load(role) {
                access
                    .rotate(role.identity(), role.capability(), &secret)
                    .map_err(|_| BootstrapError::Denied)?;
            }
        }
        Ok(Self {
            access,
            store,
            verifier,
            context,
            nonces: Mutex::new(HashMap::new()),
        })
    }
    /// Explicit credential resolution/rotation, not a reconnect side effect.
    pub fn refresh(&self, role: Role) -> Result<(), BootstrapError> {
        let secret = self.store.load(role)?;
        self.access
            .rotate(role.identity(), role.capability(), &secret)
            .map_err(|_| BootstrapError::Denied)
    }
    fn changed_authorization(&self, role: Role) -> BootstrapError {
        if self.access.identity_active(role.identity()) {
            BootstrapError::Expired
        } else {
            BootstrapError::Denied
        }
    }
    fn reserve_nonce(&self, nonce: &str) -> Result<(), BootstrapError> {
        if unhex(nonce).is_err() {
            return Err(BootstrapError::InvalidFrame);
        }
        let mut nonces = self.nonces.lock().expect("bootstrap nonces");
        nonces.retain(|_, until| *until > Instant::now());
        if nonces.contains_key(nonce) {
            return Err(BootstrapError::Replay);
        }
        if nonces.len() >= 128 {
            return Err(BootstrapError::Busy);
        }
        nonces.insert(nonce.into(), Instant::now() + Duration::from_secs(30));
        Ok(())
    }
    pub async fn serve(
        self: Arc<Self>,
        listener: UnixListener,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), BootstrapError> {
        let slots = Arc::new(Semaphore::new(16));
        let mut tasks = JoinSet::new();
        if *shutdown.borrow() {
            return Ok(());
        }
        loop {
            tokio::select! {biased;
                _=shutdown.changed()=>break,
                done=tasks.join_next(),if !tasks.is_empty()=>{let _=done;},
                accepted=listener.accept()=>{
                    let(stream,_)=accepted.map_err(|_|BootstrapError::Unavailable)?;
                    let Ok(slot)=slots.clone().try_acquire_owned() else {drop(stream);continue};let server=self.clone();
                    tasks.spawn(async move{let _slot=slot;let mut stream=stream;if let Err(error)=server.exchange(&mut stream).await{let _=write_frame(&mut stream,&ServerFrame::Error(error)).await;}});
                }
            }
        }
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        Ok(())
    }
    pub async fn exchange(&self, stream: &mut UnixStream) -> Result<(), BootstrapError> {
        let ClientFrame::Hello(hello) = read_frame(stream, Duration::from_secs(5)).await? else {
            return Err(BootstrapError::InvalidFrame);
        };
        self.verifier.verify(stream, hello.role)?;
        if hello.version != 1 || hello.profile != self.context.profile {
            return Err(BootstrapError::Denied);
        }
        self.reserve_nonce(&hello.nonce)?;
        // Blocked credential stores are surfaced without prompts or HTTP grants.
        let secret = self.store.load(hello.role)?;
        if !self.access.is_current(hello.role.identity(), &secret) {
            return Err(self.changed_authorization(hello.role));
        }
        let challenge = Challenge {
            version: 1,
            role: hello.role,
            nonce: hello.nonce,
            challenge: uuid::Uuid::new_v4().to_string(),
            context: self.context.clone(),
            probe_bearer: hex(&random()?),
            proof_key: hex(&random()?),
        };
        let proof = proof(&challenge)?;
        let consumed = self
            .access
            .register_readiness(
                hello.role.identity(),
                challenge.challenge.clone(),
                &unhex(&challenge.probe_bearer)?,
                proof.clone(),
            )
            .map_err(|_| BootstrapError::Busy)?;
        let issued = Instant::now();
        let id = challenge.challenge.clone();
        let result = async {
            write_frame(stream, &ServerFrame::Challenge(challenge)).await?;
            let ClientFrame::Redeem(redeem) = read_frame(stream, Duration::from_secs(10)).await?
            else {
                return Err(BootstrapError::InvalidFrame);
            };
            if issued.elapsed() >= Duration::from_secs(30) {
                return Err(BootstrapError::Expired);
            }
            if redeem.challenge != id
                || !consumed.load(Ordering::Acquire)
                || !equal(&redeem.proof, &proof)
            {
                return Err(BootstrapError::Denied);
            }
            self.verifier.verify(stream, hello.role)?;
            // Rotation/revocation after challenge issuance must not re-grant an
            // old secret. A caller must begin a new protected exchange instead.
            if !self.access.is_current(hello.role.identity(), &secret) {
                return Err(self.changed_authorization(hello.role));
            }
            write_frame(
                stream,
                &ServerFrame::Credentials(Credentials {
                    role: hello.role,
                    context: self.context.clone(),
                    bearer: hex(&secret),
                }),
            )
            .await
        }
        .await;
        self.access.forget_readiness(&id);
        result
    }
}

/// The MAC key is carried only by the protected channel. The HTTP listener sees
/// a distinct one-use probe bearer, so a stolen endpoint cannot compute proof.
pub fn proof(challenge: &Challenge) -> Result<String, BootstrapError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(&unhex(&challenge.proof_key)?)
        .map_err(|_| BootstrapError::InvalidFrame)?;
    let message = serde_json::to_vec(&(
        "samsung-tv-remote/bootstrap-ready/v1",
        challenge.version,
        challenge.role,
        &challenge.nonce,
        &challenge.challenge,
        &challenge.context.profile,
        &challenge.context.endpoint,
        &challenge.context.build,
        &challenge.context.api,
        &challenge.context.epoch,
    ))
    .map_err(|_| BootstrapError::InvalidFrame)?;
    mac.update(&message);
    Ok(hex(&mac.finalize().into_bytes().into()))
}
pub fn equal(left: &str, right: &str) -> bool {
    left.len() == right.len() && bool::from(left.as_bytes().ct_eq(right.as_bytes()))
}
pub fn random() -> Result<[u8; 32], BootstrapError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| BootstrapError::Unavailable)?;
    Ok(bytes)
}
pub fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn unhex(value: &str) -> Result<[u8; 32], BootstrapError> {
    if value.len() != 64 {
        return Err(BootstrapError::InvalidFrame);
    }
    let mut bytes = [0_u8; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        let pair = value
            .get(i * 2..i * 2 + 2)
            .ok_or(BootstrapError::InvalidFrame)?;
        if !pair
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
        {
            return Err(BootstrapError::InvalidFrame);
        }
        *byte = u8::from_str_radix(pair, 16).map_err(|_| BootstrapError::InvalidFrame)?;
    }
    Ok(bytes)
}
pub async fn read_frame<T: for<'de> Deserialize<'de>>(
    stream: &mut UnixStream,
    deadline: Duration,
) -> Result<T, BootstrapError> {
    timeout(deadline, async {
        let length = stream
            .read_u32()
            .await
            .map_err(|_| BootstrapError::InvalidFrame)? as usize;
        if length == 0 || length > MAX_FRAME {
            return Err(BootstrapError::InvalidFrame);
        }
        let mut bytes = vec![0; length];
        stream
            .read_exact(&mut bytes)
            .await
            .map_err(|_| BootstrapError::InvalidFrame)?;
        serde_json::from_slice(&bytes).map_err(|_| BootstrapError::InvalidFrame)
    })
    .await
    .map_err(|_| BootstrapError::Expired)?
}
pub async fn write_frame(
    stream: &mut UnixStream,
    value: &impl Serialize,
) -> Result<(), BootstrapError> {
    let bytes = serde_json::to_vec(value).map_err(|_| BootstrapError::InvalidFrame)?;
    if bytes.len() > MAX_FRAME {
        return Err(BootstrapError::InvalidFrame);
    }
    timeout(Duration::from_secs(5), async {
        stream.write_u32(bytes.len() as u32).await?;
        stream.write_all(&bytes).await?;
        stream.flush().await
    })
    .await
    .map_err(|_| BootstrapError::Expired)?
    .map_err(|_| BootstrapError::Unavailable)
}
