//! Explicit M8 development composition. No Samsung, Keychain, DNS or UDP adapter
//! is reachable from this module. Synthetic credentials persist as presence
//! markers only, in a namespace rejected by the legacy production adapters.
use crate::application::{
    certificate_trust::CertificatePin,
    credential_store::{CredentialStore, PairingToken, SecretError},
    remote_dispatcher::RequestId,
    tv_address::TvHost,
    tv_control_coordinator::AppServices,
    tv_discovery::{DiscoveryError, TvDiscovery},
    tv_session::*,
    tv_setup_service::TvSetupService,
    wake_transport::{WakeFuture, WakePermit, WakeTransport},
};
use crate::domain::{DeviceId, MacAddress, RemoteAction};
use crate::infrastructure::{
    preferences::{LocalCertificateTrustStore, LocalDeviceRepository, LocalRecoveryJournal},
    private_files::{FileError, OwnershipLock, RuntimeDirectory, DEVELOPMENT_NAMESPACE},
};
use std::path::Path;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::mpsc;

const SYNTHETIC_TOKEN: &str = "m8-synthetic-pairing";
const CREDENTIAL_MARKER: &[u8] = b"synthetic-presence-v1";
#[derive(Default)]
pub struct Effects {
    pub credential_reads: AtomicUsize,
    pub discovery: AtomicUsize,
    pub probes: AtomicUsize,
    pub approvals: AtomicUsize,
    pub connections: AtomicUsize,
    pub writes: AtomicUsize,
    pub packets: AtomicUsize,
}

pub struct DevelopmentProfile {
    pub directory: Arc<RuntimeDirectory>,
    pub effects: Arc<Effects>,
    services: AppServices,
}
impl DevelopmentProfile {
    pub fn directory(base: &Path) -> Result<Arc<RuntimeDirectory>, FileError> {
        Ok(Arc::new(RuntimeDirectory::open(
            &base.join(DEVELOPMENT_NAMESPACE),
        )?))
    }
    pub fn open(base: &Path) -> Result<(Self, OwnershipLock), FileError> {
        let directory = Self::directory(base)?;
        let ownership = directory.lock()?;
        Self::initialize(directory, ownership)
    }
    pub fn inherited(base: &Path, file: std::fs::File) -> Result<(Self, OwnershipLock), FileError> {
        let directory = Self::directory(base)?;
        let ownership = directory.adopt_lock(file)?;
        Self::initialize(directory, ownership)
    }
    fn initialize(
        directory: Arc<RuntimeDirectory>,
        ownership: OwnershipLock,
    ) -> Result<(Self, OwnershipLock), FileError> {
        let (data, credentials) = match directory.read("namespace", 64)? {
            None => {
                // Interrupted initialization fails closed if it left unknown
                // directories. Never adopt preexisting data without the tag.
                let data = directory.new_child("data")?;
                let credentials = directory.new_child("synthetic-credentials")?;
                directory.replace("namespace", DEVELOPMENT_NAMESPACE.as_bytes())?;
                (data, credentials)
            }
            Some(tag) if tag == DEVELOPMENT_NAMESPACE.as_bytes() => (
                directory.child("data")?,
                directory.child("synthetic-credentials")?,
            ),
            Some(_) => return Err(FileError::UnsafePath),
        };
        let data = Arc::new(data);
        let effects = Arc::new(Effects::default());
        let ports = Arc::new(FakePorts(effects.clone()));
        let setup = Arc::new(
            TvSetupService::new(
                LocalDeviceRepository::in_private_directory(data.clone()),
                SyntheticCredentials {
                    directory: Arc::new(credentials),
                    effects: effects.clone(),
                },
                LocalCertificateTrustStore::in_private_directory(data.clone()),
            )
            .with_journal(Arc::new(LocalRecoveryJournal::in_private_directory(data))),
        );
        let services = AppServices::new(Some(setup), ports.clone(), ports.clone(), ports);
        Ok((
            Self {
                directory,
                effects,
                services,
            },
            ownership,
        ))
    }
    pub fn services(&self) -> AppServices {
        self.services.clone()
    }
}

struct SyntheticCredentials {
    directory: Arc<RuntimeDirectory>,
    effects: Arc<Effects>,
}
impl CredentialStore for SyntheticCredentials {
    fn load(&self, id: DeviceId) -> Result<Option<PairingToken>, SecretError> {
        self.effects.credential_reads.fetch_add(1, Ordering::SeqCst);
        match self
            .directory
            .read(&format!("{id}.synthetic"), 64)
            .map_err(|_| SecretError::Unavailable)?
        {
            None => Ok(None),
            Some(marker) if marker == CREDENTIAL_MARKER => {
                Ok(PairingToken::from_stored(SYNTHETIC_TOKEN.into()))
            }
            Some(_) => Err(SecretError::InvalidToken),
        }
    }
    fn save(&self, id: DeviceId, token: &PairingToken) -> Result<(), SecretError> {
        if token.as_str() != SYNTHETIC_TOKEN {
            return Err(SecretError::InvalidToken);
        }
        self.directory
            .replace(&format!("{id}.synthetic"), CREDENTIAL_MARKER)
            .map_err(|_| SecretError::Unavailable)
    }
    fn delete(&self, id: DeviceId) -> Result<(), SecretError> {
        self.directory
            .remove(&format!("{id}.synthetic"))
            .map_err(|_| SecretError::Unavailable)
    }
}
struct FakePorts(Arc<Effects>);
struct FakeConnection(Arc<Effects>);
struct FakeControl {
    #[cfg(feature = "p3-spike")]
    diagnostic: Option<crate::application::diagnostic::DiagnosticPort>,
    #[cfg(feature = "p3-spike")]
    diagnostic_task: tokio::task::JoinHandle<()>,
    sender: Option<mpsc::Sender<TvSessionEvent>>,
    effects: Arc<Effects>,
}
impl TvSessionControl for FakeControl {
    #[cfg(feature = "p3-spike")]
    fn take_diagnostic(&mut self) -> Option<crate::application::diagnostic::DiagnosticPort> {
        self.diagnostic.take()
    }
    fn try_click(&self, id: RequestId, _action: RemoteAction) -> bool {
        if self
            .sender
            .as_ref()
            .is_some_and(|s| s.try_send(TvSessionEvent::Written(id)).is_ok())
        {
            self.effects.writes.fetch_add(1, Ordering::SeqCst);
            true
        } else {
            false
        }
    }
    fn abort(&mut self) {
        #[cfg(feature = "p3-spike")]
        self.diagnostic_task.abort();
        self.sender.take();
    }
}
impl TvConnection for FakeConnection {
    fn await_authorized(
        &mut self,
        reconnecting: bool,
    ) -> SessionFuture<'_, Result<Option<PairingToken>, TvSessionError>> {
        if !reconnecting {
            self.0.approvals.fetch_add(1, Ordering::SeqCst);
        }
        Box::pin(async move {
            Ok((!reconnecting).then(|| {
                PairingToken::from_stored(SYNTHETIC_TOKEN.into()).expect("synthetic token")
            }))
        })
    }
    fn start(self: Box<Self>) -> ActiveTvSession {
        let (sender, receiver) = mpsc::channel(32);
        #[cfg(feature = "p3-spike")]
        let (diagnostic, diagnostic_task) = fake_diagnostic(self.0.clone());
        ActiveTvSession {
            control: Box::new(FakeControl {
                #[cfg(feature = "p3-spike")]
                diagnostic: Some(diagnostic),
                #[cfg(feature = "p3-spike")]
                diagnostic_task,
                sender: Some(sender),
                effects: self.0,
            }),
            events: Box::pin(futures_util::stream::unfold(
                receiver,
                |mut receiver| async move { receiver.recv().await.map(|event| (event, receiver)) },
            )),
        }
    }
}
impl TvGateway for FakePorts {
    fn probe(
        &self,
        _host: TvHost,
    ) -> SessionFuture<'static, Result<ProbeObservation, TvSessionError>> {
        self.0.probes.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok(ProbeObservation {
                pin: CertificatePin::from_der(b"m8 synthetic certificate"),
                name: Some("Synthetic TV".into()),
                model: Some("M8 fixture".into()),
            })
        })
    }
    fn connect(
        &self,
        _host: TvHost,
        pin: CertificatePin,
        token: Option<PairingToken>,
    ) -> SessionFuture<'static, Result<Box<dyn TvConnection>, TvSessionError>> {
        self.0.connections.fetch_add(1, Ordering::SeqCst);
        let effects = self.0.clone();
        Box::pin(async move {
            if pin != CertificatePin::from_der(b"m8 synthetic certificate") {
                return Err(TvSessionError::CertificateChanged);
            }
            if token.is_some_and(|t| t.as_str() != SYNTHETIC_TOKEN) {
                return Err(TvSessionError::TokenRejected);
            }
            Ok(Box::new(FakeConnection(effects)) as Box<dyn TvConnection>)
        })
    }
}
impl TvDiscovery for FakePorts {
    fn discover(&self) -> SessionFuture<'_, Result<Vec<TvHost>, DiscoveryError>> {
        self.0.discovery.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok(vec![
                TvHost::parse("synthetic-tv.invalid").expect("synthetic host")
            ])
        })
    }
}
impl WakeTransport for FakePorts {
    fn send_once(&self, _host: TvHost, _mac: MacAddress, permit: WakePermit) -> WakeFuture {
        let effects = self.0.clone();
        Box::pin(async move {
            permit.send_if_current(|| {
                effects.packets.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        })
    }
}

#[cfg(feature = "p3-spike")]
fn fake_diagnostic(
    effects: Arc<Effects>,
) -> (
    crate::application::diagnostic::DiagnosticPort,
    tokio::task::JoinHandle<()>,
) {
    use crate::application::diagnostic::*;
    let (commands, mut requests) = mpsc::channel::<SessionRequest>(1);
    let (observed, events) = mpsc::channel(16);
    let task = tokio::spawn(async move {
        while let Some(request) = requests.recv().await {
            let outcome = if !request.alive.load(Ordering::SeqCst) {
                WriteOutcome::NotSent
            } else if matches!(request.experiment, WireExperiment::Ime) {
                WriteOutcome::ObservationOnly
            } else {
                effects.writes.fetch_add(1, Ordering::SeqCst);
                WriteOutcome::Written
            };
            if observed
                .send(SessionObservation::Write(request.sequence, outcome))
                .await
                .is_err()
            {
                return;
            }
            if outcome == WriteOutcome::Written {
                let event = match request.experiment {
                    WireExperiment::Catalog => Some(CandidateEvent::Catalog(vec![
                        DiagnosticApp::new("synthetic.a", "Synthetic A").expect("fake app"),
                        DiagnosticApp::new("synthetic.b", "Synthetic B").expect("fake app"),
                    ])),
                    WireExperiment::Launch(_) => Some(CandidateEvent::Launch(PayloadShape::Object)),
                    _ => None,
                };
                if let Some(event) = event {
                    let _ = observed
                        .send(SessionObservation::Frame {
                            sequence: request.sequence,
                            bytes: 128,
                            event,
                        })
                        .await;
                }
            }
            if observed
                .send(SessionObservation::End(request.sequence))
                .await
                .is_err()
            {
                return;
            }
        }
    });
    (DiagnosticPort { commands, events }, task)
}
