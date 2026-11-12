//! Real loopback HTTP, isolated files and fake TV/credential/Wake boundaries.
#[path = "support/storage_faults.rs"]
mod storage_faults;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{body::Incoming, Request};
use hyper_util::rt::TokioIo;
use samsung_tv_remote::application::certificate_trust::CertificatePin;
use samsung_tv_remote::application::control_service::*;
use samsung_tv_remote::application::credential_store::*;
use samsung_tv_remote::application::remote_dispatcher::RequestId;
use samsung_tv_remote::application::runtime::{Runtime, RuntimeLimits};
use samsung_tv_remote::application::tv_address::TvHost;
use samsung_tv_remote::application::tv_control_coordinator::AppServices;
use samsung_tv_remote::application::tv_discovery::*;
use samsung_tv_remote::application::tv_session::*;
use samsung_tv_remote::application::tv_setup_service::TvSetupService;
use samsung_tv_remote::application::wake_transport::*;
use samsung_tv_remote::infrastructure::preferences::*;
use samsung_tv_remote::server::access::*;
use samsung_tv_remote::server::control_http::{response::ErrorCode, transport::*, wire};
use samsung_tv_remote::{DeviceId, RemoteAction};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use storage_faults::{Faults, Repository, Trust};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};

#[derive(Default)]
struct Effects {
    discovery: AtomicUsize,
    probes: AtomicUsize,
    pairs: AtomicUsize,
    connections: AtomicUsize,
    writes: AtomicUsize,
    packets: AtomicUsize,
    probe_error: Mutex<Option<TvSessionError>>,
    discovery_error: Mutex<Option<DiscoveryError>>,
    wake_error: Mutex<Option<WakeSendError>>,
    remote_mode: AtomicUsize,
    remote_attempts: AtomicUsize,
    remote_release: tokio::sync::Notify,
    probe_panic: std::sync::atomic::AtomicBool,
    probe_wait: std::sync::atomic::AtomicBool,
    probe_release: tokio::sync::Notify,
    connect_error: Mutex<Option<TvSessionError>>,
    approval_error: Mutex<Option<TvSessionError>>,
    rotate: std::sync::atomic::AtomicBool,
    approval_wait: std::sync::atomic::AtomicBool,
    approval_release: tokio::sync::Notify,
    connect_wait: std::sync::atomic::AtomicBool,
    connect_release: tokio::sync::Notify,
    connect_returned: AtomicUsize,
    wake_wait: std::sync::atomic::AtomicBool,
    wake_release: tokio::sync::Notify,
    wake_returned: AtomicUsize,
}
#[derive(Clone)]
struct Secrets {
    values: Arc<Mutex<HashMap<DeviceId, String>>>,
    faults: Arc<Faults>,
}
impl CredentialStore for Secrets {
    fn load(&self, id: DeviceId) -> Result<Option<PairingToken>, SecretError> {
        self.faults.secret_load.check()?;
        if self.faults.missing_secret.load(Ordering::SeqCst) {
            return Ok(None);
        }
        Ok(self
            .values
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .and_then(PairingToken::from_stored))
    }
    fn save(&self, id: DeviceId, token: &PairingToken) -> Result<(), SecretError> {
        self.faults.secret_save.check()?;
        self.values
            .lock()
            .unwrap()
            .insert(id, token.as_str().into());
        Ok(())
    }
    fn delete(&self, id: DeviceId) -> Result<(), SecretError> {
        self.faults.secret_delete.check()?;
        self.values.lock().unwrap().remove(&id);
        Ok(())
    }
}
struct Ports(Arc<Effects>);
struct Connection(Arc<Effects>);
struct Control {
    sender: Option<mpsc::Sender<TvSessionEvent>>,
    effects: Arc<Effects>,
}
impl TvSessionControl for Control {
    fn try_click(&self, id: RequestId, _action: RemoteAction) -> bool {
        self.effects.remote_attempts.fetch_add(1, Ordering::SeqCst);
        let mode = self.effects.remote_mode.load(Ordering::SeqCst);
        if mode == 5 {
            let Some(sender) = self.sender.clone() else {
                return false;
            };
            let effects = self.effects.clone();
            tokio::spawn(async move {
                effects.remote_release.notified().await;
                if sender.send(TvSessionEvent::Written(id)).await.is_ok() {
                    effects.writes.fetch_add(1, Ordering::SeqCst);
                }
            });
            return true;
        }
        let event = match mode {
            1 => TvSessionEvent::NotSent(id),
            2 => TvSessionEvent::Uncertain(id),
            3 => TvSessionEvent::Disconnected,
            4 => TvSessionEvent::TokenRejected,
            _ => TvSessionEvent::Written(id),
        };
        if self
            .sender
            .as_ref()
            .is_some_and(|s| s.try_send(event).is_ok())
        {
            if mode == 0 {
                self.effects.writes.fetch_add(1, Ordering::SeqCst);
            }
            true
        } else {
            false
        }
    }
    fn abort(&mut self) {
        self.sender.take();
    }
}
impl TvConnection for Connection {
    fn await_authorized(
        &mut self,
        reconnecting: bool,
    ) -> SessionFuture<'_, Result<Option<PairingToken>, TvSessionError>> {
        if !reconnecting {
            self.0.pairs.fetch_add(1, Ordering::SeqCst);
        }
        let error = *self.0.approval_error.lock().unwrap();
        let rotate = self.0.rotate.load(Ordering::SeqCst);
        let effects = self.0.clone();
        Box::pin(async move {
            if effects.approval_wait.load(Ordering::SeqCst) {
                effects.approval_release.notified().await;
            }
            if let Some(error) = error {
                return Err(error);
            }
            Ok((!reconnecting || rotate)
                .then(|| PairingToken::from_stored("synthetic-token".into()).unwrap()))
        })
    }
    fn start(self: Box<Self>) -> ActiveTvSession {
        let (sender, receiver) = mpsc::channel(32);
        ActiveTvSession {
            control: Box::new(Control {
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
impl TvGateway for Ports {
    fn probe(
        &self,
        _host: TvHost,
    ) -> SessionFuture<'static, Result<ProbeObservation, TvSessionError>> {
        self.0.probes.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = *self.0.probe_error.lock().unwrap() {
            return Box::pin(async move { Err(error) });
        }
        let effects = self.0.clone();
        Box::pin(async move {
            assert!(
                !effects.probe_panic.load(Ordering::SeqCst),
                "injected gateway panic"
            );
            if effects.probe_wait.load(Ordering::SeqCst) {
                effects.probe_release.notified().await;
            }
            Ok(ProbeObservation {
                pin: CertificatePin::from_der(b"synthetic"),
                name: Some("Fake TV".into()),
                model: None,
            })
        })
    }
    fn connect(
        &self,
        _host: TvHost,
        _pin: CertificatePin,
        _token: Option<PairingToken>,
    ) -> SessionFuture<'static, Result<Box<dyn TvConnection>, TvSessionError>> {
        self.0.connections.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = *self.0.connect_error.lock().unwrap() {
            return Box::pin(async move { Err(error) });
        }
        let effects = self.0.clone();
        Box::pin(async move {
            if effects.connect_wait.load(Ordering::SeqCst) {
                effects.connect_release.notified().await;
            }
            effects.connect_returned.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(Connection(effects)) as Box<dyn TvConnection>)
        })
    }
}
impl TvDiscovery for Ports {
    fn discover(&self) -> SessionFuture<'_, Result<Vec<TvHost>, DiscoveryError>> {
        self.0.discovery.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = *self.0.discovery_error.lock().unwrap() {
            return Box::pin(async move { Err(error) });
        }
        Box::pin(async { Ok(vec![TvHost::parse("tv.local").unwrap()]) })
    }
}
impl WakeTransport for Ports {
    fn send_once(
        &self,
        _host: TvHost,
        _mac: samsung_tv_remote::domain::MacAddress,
        permit: WakePermit,
    ) -> WakeFuture {
        let effects = self.0.clone();
        Box::pin(async move {
            if effects.wake_wait.load(Ordering::SeqCst) {
                effects.wake_release.notified().await;
            }
            if let Some(error) = *effects.wake_error.lock().unwrap() {
                return Err(error);
            }
            let result = permit.send_if_current(|| {
                effects.packets.fetch_add(1, Ordering::SeqCst);
                Ok(())
            });
            effects.wake_returned.fetch_add(1, Ordering::SeqCst);
            result
        })
    }
}
#[derive(Default)]
struct Stops(AtomicUsize);
impl StopPort for Stops {
    fn stop(&self, client: AuthorizedClient, _epoch: Epoch, _key: String) -> StopFuture<'_> {
        Box::pin(async move {
            client.require_management().map_err(|_| ErrorCode {
                status: 403,
                code: "forbidden",
            })?;
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"stopped":true}))
        })
    }
}
enum Owner {
    Runtime(Runtime),
    Lifecycle(samsung_tv_remote::server::lifecycle::stop::Lifecycle),
    Detached,
}
struct Harness {
    port: u16,
    gate: Arc<AccessGate>,
    effects: Arc<Effects>,
    stops: Arc<Stops>,
    faults: Arc<Faults>,
    owner: Owner,
    shutdown: watch::Sender<bool>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
    _profile: tempfile::TempDir,
}
impl Harness {
    async fn new() -> Self {
        Self::with_limits(RuntimeLimits::default()).await
    }
    async fn with_limits(limits: RuntimeLimits) -> Self {
        Self::configuration(limits, false).await
    }
    async fn configuration(limits: RuntimeLimits, durable: bool) -> Self {
        Self::seeded(limits, durable, |_| {}).await
    }
    async fn seeded(
        limits: RuntimeLimits,
        durable: bool,
        seed: impl FnOnce(&std::path::Path),
    ) -> Self {
        let profile = tempfile::tempdir().unwrap();
        seed(profile.path());
        let effects = Arc::new(Effects::default());
        let ports = Arc::new(Ports(effects.clone()));
        let faults = Arc::new(Faults::default());
        let setup = Arc::new(
            TvSetupService::new(
                Repository {
                    inner: LocalDeviceRepository::new(profile.path()),
                    faults: faults.clone(),
                },
                Secrets {
                    values: Arc::new(Mutex::new(HashMap::new())),
                    faults: faults.clone(),
                },
                Trust {
                    inner: LocalCertificateTrustStore::new(profile.path()),
                    faults: faults.clone(),
                },
            )
            .with_journal(Arc::new(LocalRecoveryJournal::new(profile.path()))),
        );
        let setup = Arc::new(storage_faults::Setup {
            inner: setup,
            faults: faults.clone(),
        });
        let runtime = Runtime::start(
            AppServices::new(Some(setup), ports.clone(), ports.clone(), ports),
            limits,
        );
        let mut ready = runtime.client(ClientId(1)).subscribe().await.unwrap();
        while ready.snapshot.startup == StartupState::Restoring {
            ready.snapshot = ready.events.recv().await.unwrap().snapshot;
        }
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let gate = Arc::new(AccessGate::new(port));
        gate.rotate(ClientId(1), Capability::Manage, &[1; 32])
            .unwrap();
        gate.rotate(ClientId(2), Capability::Control, &[2; 32])
            .unwrap();
        let stops = Arc::new(Stops::default());
        let services = [
            (ClientId(1), runtime.client(ClientId(1))),
            (ClientId(2), runtime.client(ClientId(2))),
        ];
        let (shutdown, default_receiver) = watch::channel(false);
        let (owner, port, receiver): (Owner, Arc<dyn StopPort>, _) = if durable {
            use samsung_tv_remote::server::lifecycle::{
                files::RuntimeDirectory,
                stop::{FileStopStore, Lifecycle},
            };
            let directory = Arc::new(
                RuntimeDirectory::open(&profile.path().canonicalize().unwrap().join("runtime"))
                    .unwrap(),
            );
            let lifecycle = Lifecycle::start(
                runtime,
                Arc::new(FileStopStore(directory.clone())),
                directory.lock().unwrap(),
            )
            .await
            .unwrap();
            let client = lifecycle.client.clone();
            let receiver = lifecycle.http_shutdown.clone();
            (Owner::Lifecycle(lifecycle), client, receiver)
        } else {
            (Owner::Runtime(runtime), stops.clone(), default_receiver)
        };
        let adapter = Arc::new(HttpAdapter::new(
            gate.clone(),
            services,
            port,
            "test-build".into(),
        ));
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(adapter.serve(listener, receiver));
        Self {
            port,
            gate,
            effects,
            stops,
            faults,
            owner,
            shutdown,
            task,
            _profile: profile,
        }
    }
    async fn response(
        &self,
        client: u8,
        method: &str,
        path: &str,
        body: Value,
    ) -> hyper::Response<Incoming> {
        let stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, self.port))
            .await
            .unwrap();
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let body = if method == "GET" {
            Vec::new()
        } else {
            serde_json::to_vec(&body).unwrap()
        };
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", format!("127.0.0.1:{}", self.port))
            .header(
                "authorization",
                format!("Bearer {}", format!("{client:02x}").repeat(32)),
            )
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(body)))
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), sender.send_request(request))
            .await
            .unwrap()
            .unwrap()
    }
    async fn json(&self, client: u8, method: &str, path: &str, body: Value) -> (u16, Value) {
        let response = self.response(client, method, path, body).await;
        let status = response.status().as_u16();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }
    async fn state(&self) -> Value {
        let (status, state) = self.json(1, "GET", "/v1/state", Value::Null).await;
        assert_eq!(status, 200);
        state
    }
    async fn terminal(&self, client: u8, epoch: &str, key: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let (_, value) = self
                    .json(
                        client,
                        "GET",
                        &format!("/v1/operations/by-key/{key}?epoch={epoch}"),
                        Value::Null,
                    )
                    .await;
                if value["state"] == "terminal" {
                    return value;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }
    async fn mutate(&self, client: u8, method: &str, path: &str, extra: Value) -> (String, Value) {
        let (status, state) = self.json(client, "GET", "/v1/state", Value::Null).await;
        assert_eq!(status, 200);
        let key = uuid::Uuid::new_v4().to_string();
        let mut body = json!({"server_epoch":state["server_epoch"],"operation_key":key});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let (status, receipt) = self.json(client, method, path, body).await;
        assert!(
            [200, 202, 400, 403, 404, 409, 412, 423, 429, 503].contains(&status),
            "{receipt}"
        );
        assert!(receipt.get("operation_id").is_some(), "{receipt}");
        (
            key.clone(),
            self.terminal(client, state["server_epoch"].as_str().unwrap(), &key)
                .await,
        )
    }
    async fn pair(&self) -> String {
        let (_, probe) = self
            .mutate(1, "POST", "/v1/probes", json!({"host":"tv.local"}))
            .await;
        let observation = &probe["result"];
        let(_,paired)=self.mutate(1,"POST","/v1/pairings",json!({"host":"tv.local","attempt_id":observation["attempt_id"],"observation_id":observation["observation_id"],"fingerprint":observation["fingerprint"],"confirm_fingerprint":observation["fingerprint"],"label":"Synthetic TV","selection_generation":"0"})).await;
        assert_eq!(paired["result"]["kind"], "tv_approved_and_saved");
        paired["result"]["device_id"].as_str().unwrap().into()
    }
    async fn close(self) -> tempfile::TempDir {
        match self.owner {
            Owner::Detached => {
                self.shutdown.send(true).unwrap();
                self.task.await.unwrap().unwrap();
            }
            Owner::Runtime(runtime) => {
                self.shutdown.send(true).unwrap();
                self.task.await.unwrap().unwrap();
                runtime.drain().await.settled().await.unwrap();
            }
            Owner::Lifecycle(lifecycle) => {
                if !*lifecycle.http_shutdown.borrow() {
                    lifecycle.signal_shutdown().unwrap();
                }
                self.task.await.unwrap().unwrap();
                lifecycle.settled().await.unwrap();
            }
        }
        self._profile
    }
}

#[tokio::test]
async fn complete_fake_setup_remote_deduplication_two_clients_and_forget_over_http() {
    let h = Harness::new().await;
    let initial = h.state().await;
    assert_eq!(initial["devices"], json!([]));
    assert_eq!(initial["selected"], Value::Null);
    assert_eq!(h.effects.connections.load(Ordering::SeqCst), 0);
    assert_eq!(h.effects.discovery.load(Ordering::SeqCst), 0);
    let (_, discovery) = h.mutate(1, "POST", "/v1/discoveries", json!({})).await;
    assert_eq!(
        discovery["result"]["candidates"].as_array().unwrap().len(),
        1
    );
    let device = h.pair().await;
    let state = h.state().await;
    let generation = state["selection_generation"].clone();
    assert_eq!(state["selected"], device);
    let (_, bob) = h.json(2, "GET", "/v1/state", Value::Null).await;
    assert_eq!(bob["selected"], device);
    assert_eq!(bob["recent_operations"], json!([]));
    let (_, read) = h
        .json(2, "GET", &format!("/v1/devices/{device}"), Value::Null)
        .await;
    assert_eq!(read["device_id"], device);
    let epoch = state["server_epoch"].as_str().unwrap();
    let key = uuid::Uuid::new_v4().to_string();
    let mut command = json!({"server_epoch":epoch,"operation_key":key,"device_id":device,"selection_generation":generation,"action":"VolumeUp"});
    let (status, first) = h
        .json(1, "POST", "/v1/remote-actions", command.clone())
        .await;
    assert!([200, 202].contains(&status));
    let done = h.terminal(1, epoch, &key).await;
    assert_eq!(done["result"]["kind"], "written");
    let (_, duplicate) = h
        .json(1, "POST", "/v1/remote-actions", command.clone())
        .await;
    assert_eq!(duplicate, done);
    assert_eq!(first["operation_id"], done["operation_id"]);
    assert_eq!(h.effects.writes.load(Ordering::SeqCst), 1);
    command["action"] = json!("VolumeDown");
    let (status, conflict) = h.json(1, "POST", "/v1/remote-actions", command).await;
    assert_eq!(status, 409);
    assert_eq!(conflict["error"]["code"], "operation_key_conflict");
    let (status, private) = h
        .json(
            2,
            "GET",
            &format!(
                "/v1/operations/{}?epoch={epoch}",
                done["operation_id"].as_str().unwrap()
            ),
            Value::Null,
        )
        .await;
    assert_eq!(status, 410);
    assert_eq!(private["error"]["code"], "outcome_unavailable");
    let (_, selected) = h
        .mutate(
            2,
            "POST",
            "/v1/selection",
            json!({"device_id":device,"expected_generation":generation}),
        )
        .await;
    assert_eq!(selected["result"]["kind"], "saved");
    assert_eq!(selected["terminal_state"]["selection_generation"], "2");
    let (_, stale) = h
        .mutate(
            1,
            "POST",
            "/v1/remote-actions",
            json!({"device_id":device,"selection_generation":generation,"action":"VolumeUp"}),
        )
        .await;
    assert_eq!(stale["error"]["code"], "stale_selection");
    assert_eq!(h.effects.writes.load(Ordering::SeqCst), 1);
    let state = h.state().await;
    let generation = state["selection_generation"].clone();
    let (_, connected) = h
        .mutate(
            1,
            "POST",
            "/v1/connections",
            json!({"device_id":device,"selection_generation":generation}),
        )
        .await;
    assert_eq!(connected["result"]["kind"], "remote_ready");
    let(configuration_key,configured)=h.mutate(1,"PUT",&format!("/v1/devices/{device}/wake"),json!({"device_id":device,"selection_generation":generation,"expected_config_revision":"0","wired_mac":"02:00:00:00:00:01","active_interface":"Wired"})).await;
    assert_eq!(configured["result"]["kind"], "saved");
    assert_eq!(configured["terminal_state"]["config_revision"], "1");
    assert_eq!(
        configured["terminal_state"]["selection_generation"],
        generation
    );
    h.mutate(
        1,
        "POST",
        "/v1/selection",
        json!({"device_id":device,"expected_generation":generation}),
    )
    .await;
    let generation = h.state().await["selection_generation"].clone();
    assert_eq!(h.terminal(1, epoch, &configuration_key).await, configured);
    let (_, woken) = h
        .mutate(
            1,
            "POST",
            "/v1/wakes",
            json!({"device_id":device,"selection_generation":generation}),
        )
        .await;
    assert_eq!(woken["result"]["facts"]["packet"], "sent", "{woken}");
    assert_eq!(h.effects.packets.load(Ordering::SeqCst), 1);
    let (_, cancelled) = h
        .mutate(
            1,
            "POST",
            &format!(
                "/v1/wakes/{}/cancel",
                woken["operation_id"].as_str().unwrap()
            ),
            json!({"device_id":device,"selection_generation":generation}),
        )
        .await;
    assert_eq!(cancelled["error"]["code"], "not_found");
    let (_, forgotten) = h
        .mutate(
            1,
            "POST",
            &format!("/v1/devices/{device}/forget"),
            json!({"device_id":device,"expected_config_revision":"1","confirm_device_id":device}),
        )
        .await;
    assert_eq!(forgotten["result"]["kind"], "forgotten");
    assert_eq!(h.state().await["devices"], json!([]));
    h.close().await;
}

#[tokio::test]
async fn protected_readiness_unknown_routes_and_gui_management_have_no_tv_effects() {
    let h = Harness::new().await;
    for path in ["/v1/ready", "/v1/state", "/v1/events", "/unknown"] {
        let (status, _) = h.json(3, "GET", path, Value::Null).await;
        assert_eq!(status, 401);
    }
    let (status, ready) = h.json(1, "GET", "/v1/ready", Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(ready["build"], "test-build");
    let state = h.state().await;
    let body = json!({"server_epoch":state["server_epoch"],"operation_key":uuid::Uuid::new_v4().to_string()});
    let (status, _) = h.json(2, "POST", "/v1/server/stop", body.clone()).await;
    assert_eq!(status, 403);
    assert_eq!(h.stops.0.load(Ordering::SeqCst), 0);
    let (status, _) = h.json(1, "POST", "/v1/server/stop", body).await;
    assert_eq!(status, 200);
    assert_eq!(h.stops.0.load(Ordering::SeqCst), 1);
    let (status, _) = h.json(1, "GET", "/unknown", Value::Null).await;
    assert_eq!(status, 404);
    assert_eq!(h.effects.connections.load(Ordering::SeqCst), 0);
    assert_eq!(h.effects.packets.load(Ordering::SeqCst), 0);
    assert_eq!(h.effects.probes.load(Ordering::SeqCst), 0);
    h.close().await;
}

struct Events {
    body: Incoming,
    buffer: Vec<u8>,
}
impl Events {
    async fn next(&mut self) -> (String, Value) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(end) = self.buffer.windows(2).position(|v| v == b"\n\n") {
                    let text = String::from_utf8(self.buffer.drain(..end + 2).collect()).unwrap();
                    if text.starts_with(':') {
                        continue;
                    }
                    let name = text
                        .lines()
                        .find_map(|v| v.strip_prefix("event: "))
                        .unwrap()
                        .to_owned();
                    let value = text.lines().find_map(|v| v.strip_prefix("data: ")).unwrap();
                    return (name, serde_json::from_str(value).unwrap());
                }
                let frame = self
                    .body
                    .frame()
                    .await
                    .expect("event stream closed")
                    .unwrap();
                if let Ok(data) = frame.into_data() {
                    self.buffer.extend_from_slice(&data);
                }
            }
        })
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn atomic_events_are_consecutive_private_and_rotation_closes_old_streams() {
    let h = Harness::new().await;
    let first = h.response(1, "GET", "/v1/events", Value::Null).await;
    assert_eq!(first.status(), 200);
    let second = h.response(2, "GET", "/v1/events", Value::Null).await;
    assert_eq!(second.status(), 200);
    let mut alice = Events {
        body: first.into_body(),
        buffer: Vec::new(),
    };
    let mut bob = Events {
        body: second.into_body(),
        buffer: Vec::new(),
    };
    let (name, initial) = alice.next().await;
    assert_eq!(name, "snapshot");
    let (_, bob_initial) = bob.next().await;
    assert_eq!(initial["revision"], bob_initial["revision"]);
    let mut revision = initial["revision"]
        .as_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    let (_, probe) = h
        .mutate(1, "POST", "/v1/probes", json!({"host":"tv.local"}))
        .await;
    loop {
        let (_, a) = alice.next().await;
        let (_, b) = bob.next().await;
        revision += 1;
        assert_eq!(a["revision"], revision.to_string());
        assert_eq!(a, b);
        assert!(a.get("result").is_none());
        assert!(a.get("recent_operations").is_none());
        assert!(!a.to_string().contains("fingerprint"));
        if a["operation_id"] == probe["operation_id"] && a["phase"] == "terminal" {
            break;
        }
    }
    h.gate
        .rotate(ClientId(1), Capability::Manage, &[3; 32])
        .unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(Ok(_)) = alice.body.frame().await {}
    })
    .await
    .is_ok());
    let (status, _) = h.json(1, "GET", "/v1/state", Value::Null).await;
    assert_eq!(status, 401);
    let (status, state) = h.json(3, "GET", "/v1/state", Value::Null).await;
    assert_eq!(status, 200);
    assert_eq!(state["recent_operations"].as_array().unwrap().len(), 1);
    let fresh = h.response(3, "GET", "/v1/events", Value::Null).await;
    let mut fresh = Events {
        body: fresh.into_body(),
        buffer: Vec::new(),
    };
    let (name, snapshot) = fresh.next().await;
    assert_eq!(name, "snapshot");
    assert_eq!(snapshot["recent_operations"], state["recent_operations"]);
    let (_, discovery) = h.mutate(2, "POST", "/v1/discoveries", json!({})).await;
    loop {
        let (_, event) = bob.next().await;
        if event["operation_id"] == discovery["operation_id"] && event["phase"] == "terminal" {
            break;
        }
    }
    drop((alice, bob, fresh));
    h.close().await;
}

async fn raw(port: u16, request: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut socket = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    let _ = socket.write_all(request.as_bytes()).await;
    let mut output = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(3), socket.read_to_end(&mut output))
        .await
        .unwrap();
    String::from_utf8(output).unwrap()
}

#[tokio::test]
async fn raw_framing_authority_and_streamed_body_limits_prevent_admission() {
    let h = Harness::new().await;
    let state = h.state().await;
    let body=json!({"server_epoch":state["server_epoch"],"operation_key":uuid::Uuid::new_v4().to_string()}).to_string();
    let header=format!("POST /v1/discoveries HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\n",h.port,"01".repeat(32));
    for extra in [format!("Content-Length: {}\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",body.len(),body.len()),format!("Transfer-Encoding: chunked\r\nContent-Length: {}\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",body.len(),body.len()),format!("Content-Length: {}\r\nContent-Length: {}\r\n\r\n{body}",body.len(),body.len()),format!("Content-Length: {}\r\nContent-Length: 1\r\n\r\n{body}",body.len()),format!("Transfer-Encoding: chunked\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",body.len())]{
        let reply=raw(h.port,&format!("{header}{extra}")).await;assert!(reply.starts_with("HTTP/1.1 400"),"{reply}");
    }
    let large = " ".repeat(16 * 1024 + 1);
    let reply = raw(
        h.port,
        &format!(
            "{header}Transfer-Encoding: chunked\r\n\r\n{:x}\r\n{large}\r\n0\r\n\r\n",
            large.len()
        ),
    )
    .await;
    assert!(reply.starts_with("HTTP/1.1 413"), "{reply}");
    for (extra, status) in [
        ("Host: evil.invalid\r\n", 403),
        ("Origin: null\r\n", 403),
        ("Authorization: Bearer duplicate\r\n", 403),
        ("X-Forwarded-Host: evil.invalid\r\n", 403),
        ("Content-Encoding: gzip\r\n", 415),
    ] {
        let reply = raw(
            h.port,
            &format!(
                "{header}{extra}Content-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        )
        .await;
        assert!(reply.starts_with(&format!("HTTP/1.1 {status}")), "{reply}");
    }
    let reply = raw(
        h.port,
        &format!("{header}X-Large: {}\r\n\r\n", "x".repeat(16 * 1024)),
    )
    .await;
    assert!(reply.starts_with("HTTP/1.1 431"), "{reply}");
    assert_eq!(h.effects.discovery.load(Ordering::SeqCst), 0);
    assert_eq!(h.state().await["recent_operations"], json!([]));
    h.close().await;
}

#[tokio::test]
async fn stale_epoch_reports_current_and_old_epoch_lookup_never_replays() {
    let h = Harness::new().await;
    let epoch = wire::epoch(Epoch::from_u128(1));
    let (status, value) = h
        .json(
            1,
            "POST",
            "/v1/discoveries",
            json!({"server_epoch":epoch,"operation_key":uuid::Uuid::new_v4().to_string()}),
        )
        .await;
    assert_eq!(status, 409);
    assert_eq!(value["error"]["code"], "stale_epoch");
    assert!(value["current"]["server_epoch"].is_string());
    assert!(value.get("operation_id").is_none());
    let (status, value) = h
        .json(
            1,
            "GET",
            &format!(
                "/v1/operations/{}?epoch={epoch}",
                wire::operation(OperationId(1))
            ),
            Value::Null,
        )
        .await;
    assert_eq!(status, 410);
    assert_eq!(value["error"]["code"], "unknown_after_restart");
    assert_eq!(h.effects.discovery.load(Ordering::SeqCst), 0);
    h.close().await;
}

#[tokio::test]
async fn durable_stop_acknowledges_marker_before_gracefully_closing_http() {
    use samsung_tv_remote::server::lifecycle::{
        files::RuntimeDirectory,
        stop::{FileStopStore, StopStore},
    };
    let h = Harness::configuration(RuntimeLimits::default(), true).await;
    let state = h.state().await;
    let key = uuid::Uuid::new_v4().to_string();
    let (status, result) = h
        .json(
            1,
            "POST",
            "/v1/server/stop",
            json!({"server_epoch":state["server_epoch"],"operation_key":key}),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(result["durable_stop"], true);
    let directory = Arc::new(
        RuntimeDirectory::open(&h._profile.path().canonicalize().unwrap().join("runtime")).unwrap(),
    );
    let marker = FileStopStore(directory.clone()).read().unwrap().unwrap();
    assert_eq!(marker.operation_key, key);
    assert_eq!(marker.server_epoch, state["server_epoch"]);
    let _profile = h.close().await;
    let _next_owner = directory.lock().unwrap();
}

#[tokio::test]
async fn queries_and_management_keep_reserved_capacity_under_slow_mutation_load() {
    use tokio::io::AsyncWriteExt;
    let h = Harness::new().await;
    let state = h.state().await;
    let mut slow = Vec::new();
    for _ in 0..MAX_MUTATIONS_PER_CLIENT {
        let mut socket = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, h.port))
            .await
            .unwrap();
        socket.write_all(format!("POST /v1/discoveries HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: 1000\r\n\r\n{{",h.port,"01".repeat(32)).as_bytes()).await.unwrap();
        slow.push(socket);
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let (status, _) = h.json(1, "POST", "/v1/discoveries", json!({})).await;
            if status == 429 {
                break;
            }
            assert_eq!(status, 400);
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let (status, _) = h.json(1, "GET", "/v1/ready", Value::Null).await;
    assert_eq!(status, 200);
    let (status, _) = h.json(1, "GET", "/v1/state", Value::Null).await;
    assert_eq!(status, 200);
    // Bad credentials exhaust only their failure-response budget. A known
    // identity can still reach the reserved management/query pools.
    for _ in 0..65 {
        let (status, _) = h.json(3, "GET", "/v1/ready", Value::Null).await;
        assert!([401, 429].contains(&status));
    }
    let (status, _) = h.json(1, "GET", "/v1/ready", Value::Null).await;
    assert_eq!(status, 200);
    let(status,_)=h.json(1,"POST","/v1/server/stop",json!({"server_epoch":state["server_epoch"],"operation_key":uuid::Uuid::new_v4().to_string()})).await;
    assert_eq!(status, 200);
    assert_eq!(h.stops.0.load(Ordering::SeqCst), 1);
    assert_eq!(h.effects.discovery.load(Ordering::SeqCst), 0);
    drop(slow);
    h.close().await;
}

#[tokio::test]
async fn guarded_recovery_routes_compensate_exact_transactions_without_tv_io() {
    use samsung_tv_remote::application::recovery::*;
    for pairing in [false, true] {
        let target = DeviceId::generate();
        let transaction = DeviceId::generate();
        let h = Harness::seeded(RuntimeLimits::default(), false, |path| {
            let journal = LocalRecoveryJournal::new(path);
            if pairing {
                journal
                    .begin_pairing(
                        PairingMarker {
                            transaction,
                            target,
                            prior_selection: None,
                            repair: false,
                            backup: None,
                            stage: PairingStage::Prepared,
                            cleanup: None,
                        },
                        None,
                    )
                    .unwrap();
            } else {
                let mut marker = ForgetMarker::prepared(target);
                marker.transaction = transaction;
                journal.write(marker).unwrap();
            }
        })
        .await;
        let state = h.state().await;
        assert_eq!(state["startup"], "recovery_required");
        let device = wire::device(target);
        let route = if pairing {
            "recover-pairing"
        } else {
            "recover-forget"
        };
        let path = format!("/v1/devices/{device}/{route}");
        let(_,wrong)=h.mutate(1,"POST",&path,json!({"device_id":device,"transaction_id":wire::device(DeviceId::generate()),"confirm_device_id":device})).await;
        assert_eq!(wrong["error"]["code"], "storage_blocked");
        assert_eq!(h.state().await["startup"], "recovery_required");
        let(_,recovered)=h.mutate(1,"POST",&path,json!({"device_id":device,"transaction_id":wire::device(transaction),"confirm_device_id":device})).await;
        assert_eq!(recovered["result"]["kind"], "forgotten");
        assert_eq!(h.state().await["startup"], "ready");
        assert_eq!(h.effects.connections.load(Ordering::SeqCst), 0);
        assert_eq!(h.effects.probes.load(Ordering::SeqCst), 0);
        assert_eq!(h.effects.packets.load(Ordering::SeqCst), 0);
        h.close().await;
    }
}

#[tokio::test]
async fn candidate_authority_is_private_and_repair_preserves_the_device_id() {
    let h = Harness::new().await;
    let (_, discovery) = h.mutate(1, "POST", "/v1/discoveries", json!({})).await;
    let candidate = &discovery["result"]["candidates"][0]["candidate_id"];
    let (_, denied) = h
        .mutate(
            2,
            "POST",
            "/v1/probes",
            json!({"candidate_id":candidate,"host":"tv.local"}),
        )
        .await;
    assert_eq!(denied["error"]["code"], "observation_expired");
    assert_eq!(h.effects.probes.load(Ordering::SeqCst), 0);
    let (_, allowed) = h
        .mutate(
            1,
            "POST",
            "/v1/probes",
            json!({"candidate_id":candidate,"host":"tv.local"}),
        )
        .await;
    assert_eq!(allowed["result"]["kind"], "probed");
    let device = h.pair().await;
    let state = h.state().await;
    let (_, probe) = h
        .mutate(1, "POST", "/v1/probes", json!({"host":"tv.local"}))
        .await;
    let observation = &probe["result"];
    let(_,repaired)=h.mutate(1,"POST","/v1/pairings",json!({"device_id":device,"host":"tv.local","attempt_id":observation["attempt_id"],"observation_id":observation["observation_id"],"fingerprint":observation["fingerprint"],"confirm_fingerprint":observation["fingerprint"],"label":"Repaired synthetic TV","selection_generation":state["selection_generation"]})).await;
    assert_eq!(repaired["result"]["kind"], "tv_approved_and_saved");
    assert_eq!(repaired["result"]["device_id"], device);
    assert_eq!(h.effects.pairs.load(Ordering::SeqCst), 2);
    let state = h.state().await;
    assert_eq!(state["devices"].as_array().unwrap().len(), 1);
    assert_eq!(state["devices"][0]["label"], "Repaired synthetic TV");
    h.close().await;
}

#[tokio::test]
async fn losing_an_admitted_http_reply_does_not_cancel_or_repeat_the_remote_write() {
    use tokio::io::AsyncWriteExt;
    let h = Harness::new().await;
    let device = h.pair().await;
    let state = h.state().await;
    let key = uuid::Uuid::new_v4().to_string();
    let command = json!({"server_epoch":state["server_epoch"],"operation_key":key,"device_id":device,"selection_generation":state["selection_generation"],"action":"VolumeUp"});
    let body = command.to_string();
    let mut abandoned = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, h.port))
        .await
        .unwrap();
    abandoned.write_all(format!("POST /v1/remote-actions HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",h.port,"01".repeat(32),body.len()).as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while h.effects.writes.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(abandoned);
    let retained = h
        .terminal(1, state["server_epoch"].as_str().unwrap(), &key)
        .await;
    assert_eq!(retained["result"]["kind"], "written");
    let (status, duplicate) = h.json(1, "POST", "/v1/remote-actions", command).await;
    assert_eq!(status, 200);
    assert_eq!(duplicate, retained);
    assert_eq!(h.effects.writes.load(Ordering::SeqCst), 1);
    h.close().await;
}

#[tokio::test]
async fn maximum_profile_streams_are_bounded_and_subscriber_limits_preserve_queries() {
    let host = [
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61),
    ]
    .join(".");
    let devices: Vec<_> = (0..1024u64)
        .map(|n| {
            json!({
                "id":format!("dev_{n}"),"label":"\u{0001}".repeat(256),"host":host,
                "wake":{"wired":"02:00:00:00:00:01","wifi":"02:00:00:00:00:02","active":"wired"}
            })
        })
        .collect();
    let disk = serde_json::to_vec(&json!({"version":2,"selected":null,"devices":devices})).unwrap();
    let h = Harness::seeded(RuntimeLimits::default(), false, |path| {
        std::fs::write(path.join("devices.json"), &disk).unwrap();
    })
    .await;
    let start = std::time::Instant::now();
    let state = h.state().await;
    assert_eq!(state["devices"].as_array().unwrap().len(), 1024);
    let json_bytes = serde_json::to_vec(&state).unwrap().len();
    assert!(json_bytes > 2_000_000);
    let mut streams = Vec::new();
    for client in [1, 2] {
        for _ in 0..4 {
            let response = h.response(client, "GET", "/v1/events", Value::Null).await;
            assert_eq!(response.status(), 200);
            streams.push(Events {
                body: response.into_body(),
                buffer: Vec::new(),
            });
        }
        let response = h.response(client, "GET", "/v1/events", Value::Null).await;
        assert_eq!(response.status(), 429);
    }
    for stream in &mut streams {
        let (name, snapshot) = stream.next().await;
        assert_eq!(name, "snapshot");
        assert_eq!(snapshot["devices"].as_array().unwrap().len(), 1024);
        assert!(
            serde_json::to_vec(&snapshot).unwrap().len() + 128
                < samsung_tv_remote::server::control_http::response::MAX_OUTPUT_BYTES
        );
    }
    assert_eq!(h.json(1, "GET", "/v1/ready", Value::Null).await.0, 200);
    assert_eq!(h.json(2, "GET", "/v1/state", Value::Null).await.0, 200);
    assert_eq!(h.effects.probes.load(Ordering::SeqCst), 0);
    assert_eq!(h.effects.connections.load(Ordering::SeqCst), 0);
    eprintln!(
        "M8 max-profile: JSON={json_bytes} bytes, disk={} bytes, 8 SSE snapshots and queries={} ms",
        disk.len(),
        start.elapsed().as_millis()
    );
    // Drain closes all active subscribers without waiting for their readers.
    tokio::time::timeout(Duration::from_secs(2), h.close())
        .await
        .unwrap();
    drop(streams);
}

#[tokio::test]
async fn incomplete_header_saturation_expires_and_shutdown_has_a_finite_deadline() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let h = Harness::new().await;
    let mut sockets = Vec::new();
    for _ in 0..MAX_CONNECTIONS {
        let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, h.port))
            .await
            .unwrap();
        stream
            .write_all(b"GET /v1/ready HTTP/1.1\r\n")
            .await
            .unwrap();
        sockets.push(stream);
    }
    // Either all slots are occupied or accept is catching up. The finite
    // header deadline releases them without requiring a client EOF.
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert_eq!(h.json(1, "GET", "/v1/ready", Value::Null).await.0, 200);
    for mut stream in sockets.drain(..) {
        let mut bytes = Vec::new();
        tokio::time::timeout(Duration::from_secs(1), stream.read_to_end(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        assert!(bytes.is_empty());
    }
    for _ in 0..MAX_CONNECTIONS {
        let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, h.port))
            .await
            .unwrap();
        stream
            .write_all(b"GET /v1/ready HTTP/1.1\r\n")
            .await
            .unwrap();
        sockets.push(stream);
    }
    assert_eq!(h.effects.connections.load(Ordering::SeqCst), 0);
    tokio::time::timeout(Duration::from_secs(12), h.close())
        .await
        .unwrap();
    drop(sockets);
}

fn session_failures() -> [(TvSessionError, u16, &'static str); 14] {
    use samsung_tv_remote::application::tv_address::TargetError;
    [
        (
            TvSessionError::Target(TargetError::InvalidHost),
            400,
            "invalid_target",
        ),
        (
            TvSessionError::Target(TargetError::NonLocalAddress),
            400,
            "non_local_target",
        ),
        (
            TvSessionError::Target(TargetError::NoResolvedAddress),
            503,
            "target_unresolved",
        ),
        (TvSessionError::Offline, 503, "offline"),
        (TvSessionError::Timeout, 503, "timeout"),
        (TvSessionError::Tls, 503, "tls_failed"),
        (
            TvSessionError::CertificateChanged,
            412,
            "certificate_changed",
        ),
        (TvSessionError::RemoteChannel, 503, "remote_channel_failed"),
        (TvSessionError::PairingDenied, 412, "tv_denied"),
        (TvSessionError::TokenRejected, 412, "pairing_required"),
        (
            TvSessionError::PairingTokenMissing,
            412,
            "pairing_token_missing",
        ),
        (TvSessionError::Protocol, 503, "tv_protocol_failed"),
        (TvSessionError::UncertainWrite, 503, "uncertain"),
        (TvSessionError::UnsupportedAction, 400, "unsupported_action"),
    ]
}

#[tokio::test]
async fn every_gateway_and_discovery_failure_is_a_retained_http_result_without_other_effects() {
    let h = Harness::new().await;
    for (index, (error, status, code)) in session_failures().into_iter().enumerate() {
        *h.effects.probe_error.lock().unwrap() = Some(error);
        let (key, result) = h
            .mutate(
                1,
                "POST",
                "/v1/probes",
                json!({"host":"synthetic-tv.invalid"}),
            )
            .await;
        assert_eq!(result["state"], "terminal");
        assert_eq!(result["error"]["code"], code);
        assert_eq!(result["result"]["kind"], "failed");
        let path = format!(
            "/v1/operations/by-key/{key}?epoch={}",
            result["server_epoch"].as_str().unwrap()
        );
        let (found, retained) = h.json(1, "GET", &path, Value::Null).await;
        assert_eq!(found, status);
        assert_eq!(retained, result);
        assert_eq!(h.effects.probes.load(Ordering::SeqCst), index + 1);
        assert_eq!(h.effects.connections.load(Ordering::SeqCst), 0);
        assert_eq!(h.effects.pairs.load(Ordering::SeqCst), 0);
        assert_eq!(h.effects.writes.load(Ordering::SeqCst), 0);
        assert_eq!(h.effects.packets.load(Ordering::SeqCst), 0);
    }
    for (error, status, code) in [
        (DiscoveryError::Permission, 403, "network_permission_denied"),
        (DiscoveryError::Unavailable, 503, "discovery_unavailable"),
    ] {
        *h.effects.discovery_error.lock().unwrap() = Some(error);
        let (key, result) = h.mutate(1, "POST", "/v1/discoveries", json!({})).await;
        assert_eq!(result["error"]["code"], code);
        let path = format!(
            "/v1/operations/by-key/{key}?epoch={}",
            result["server_epoch"].as_str().unwrap()
        );
        assert_eq!(h.json(1, "GET", &path, Value::Null).await.0, status);
    }
    assert_eq!(h.effects.discovery.load(Ordering::SeqCst), 2);
    h.close().await;
}

#[tokio::test]
async fn remote_and_wake_terminal_facts_survive_http_lookup_without_retry() {
    for (mode, kind, writes) in [
        (0, "written", 1),
        (1, "not_sent", 0),
        (2, "uncertain", 0),
        (3, "uncertain", 0),
        (4, "uncertain", 0),
    ] {
        let h = Harness::new().await;
        let device = h.pair().await;
        let generation = h.state().await["selection_generation"].clone();
        h.effects.remote_mode.store(mode, Ordering::SeqCst);
        let (key, result) = h
            .mutate(
                1,
                "POST",
                "/v1/remote-actions",
                json!({"device_id":device,"selection_generation":generation,"action":"VolumeUp"}),
            )
            .await;
        assert_eq!(result["result"]["kind"], kind);
        assert_eq!(
            h.terminal(1, result["server_epoch"].as_str().unwrap(), &key)
                .await,
            result
        );
        assert_eq!(h.effects.remote_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(h.effects.writes.load(Ordering::SeqCst), writes);
        assert_eq!(h.effects.packets.load(Ordering::SeqCst), 0);
        if mode >= 3 {
            let (key,rejected)=h.mutate(1,"POST","/v1/remote-actions",json!({"device_id":device,"selection_generation":generation,"action":"VolumeUp"})).await;
            assert_eq!(
                rejected["error"]["code"],
                if mode == 3 {
                    "remote_not_ready"
                } else {
                    "pairing_required"
                }
            );
            assert_eq!(
                h.terminal(1, rejected["server_epoch"].as_str().unwrap(), &key)
                    .await,
                rejected
            );
            assert_eq!(h.effects.remote_attempts.load(Ordering::SeqCst), 1);
        }
        h.close().await;
    }
    for (error, code, packet) in [
        (WakeSendError::Cancelled, "cancelled", "not_sent"),
        (WakeSendError::InvalidTarget, "invalid_target", "not_sent"),
        (WakeSendError::NoIpv4Route, "no_ipv4_route", "not_sent"),
        (WakeSendError::NoBroadcast, "no_broadcast", "not_sent"),
        (WakeSendError::Permission, "permission_denied", "not_sent"),
        (WakeSendError::SendFailed, "send_failed", "uncertain"),
    ] {
        let h = Harness::new().await;
        let device = h.pair().await;
        let generation = h.state().await["selection_generation"].clone();
        h.mutate(1,"PUT",&format!("/v1/devices/{device}/wake"),json!({"device_id":device,"selection_generation":generation,"expected_config_revision":"0","wired_mac":"02:00:00:00:00:01","active_interface":"Wired"})).await;
        h.mutate(
            1,
            "POST",
            "/v1/selection",
            json!({"device_id":device,"expected_generation":generation}),
        )
        .await;
        let generation = h.state().await["selection_generation"].clone();
        *h.effects.wake_error.lock().unwrap() = Some(error);
        let (key, result) = h
            .mutate(
                1,
                "POST",
                "/v1/wakes",
                json!({"device_id":device,"selection_generation":generation}),
            )
            .await;
        assert_eq!(result["result"]["kind"], "wake", "{error:?}: {result}");
        assert_eq!(result["result"]["facts"]["packet"], packet);
        assert_eq!(result["result"]["facts"]["failure"], code);
        assert_eq!(
            h.terminal(1, result["server_epoch"].as_str().unwrap(), &key)
                .await,
            result
        );
        assert_eq!(h.effects.packets.load(Ordering::SeqCst), 0);
        assert_eq!(h.effects.remote_attempts.load(Ordering::SeqCst), 0);
        assert_eq!(h.effects.writes.load(Ordering::SeqCst), 0);
        h.close().await;
    }
}

async fn assert_retained_failure(h: &Harness, key: &str, result: &Value, status: u16, code: &str) {
    assert_eq!(result["state"], "terminal");
    assert_eq!(result["error"]["code"], code, "{result}");
    assert!(
        result["expires_at_unix_ms"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 0
    );
    let path = format!(
        "/v1/operations/by-key/{key}?epoch={}",
        result["server_epoch"].as_str().unwrap()
    );
    let (found, retained) = h.json(1, "GET", &path, Value::Null).await;
    assert_eq!(found, status, "{result}");
    assert_eq!(&retained, result);
    assert_eq!(h.effects.remote_attempts.load(Ordering::SeqCst), 0);
    assert_eq!(h.effects.writes.load(Ordering::SeqCst), 0);
    assert_eq!(h.effects.packets.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn pairing_storage_faults_over_http_compensate_or_expose_the_exact_recovery_marker() {
    use samsung_tv_remote::application::{
        certificate_trust::TrustError, device_repository::RepositoryError,
    };
    for case in 1..=7 {
        let h = Harness::new().await;
        match case {
            1 => *h.faults.secret_save.error.lock().unwrap() = Some(SecretError::Unavailable),
            2 => *h.faults.secret_save.error.lock().unwrap() = Some(SecretError::InvalidToken),
            3 => *h.faults.trust_save.error.lock().unwrap() = Some(TrustError::Unavailable),
            4 => *h.faults.trust_save.error.lock().unwrap() = Some(TrustError::Corrupt),
            5 => *h.faults.device_save.error.lock().unwrap() = Some(RepositoryError::Unavailable),
            6 => *h.faults.device_save.error.lock().unwrap() = Some(RepositoryError::Corrupt),
            7 => {
                *h.faults.device_save.error.lock().unwrap() = Some(RepositoryError::Unavailable);
                *h.faults.secret_delete.error.lock().unwrap() = Some(SecretError::Unavailable);
            }
            _ => (),
        }
        let (_, probe) = h
            .mutate(
                1,
                "POST",
                "/v1/probes",
                json!({"host":"synthetic-tv.invalid"}),
            )
            .await;
        let observed = &probe["result"];
        let (key,result)=h.mutate(1,"POST","/v1/pairings",json!({
            "attempt_id":observed["attempt_id"],"observation_id":observed["observation_id"],
            "host":observed["host"],"fingerprint":observed["fingerprint"],"confirm_fingerprint":observed["fingerprint"],
            "selection_generation":"0","label":if case==0 {" "} else {"Synthetic"}
        })).await;
        assert_retained_failure(&h, &key, &result, 423, "partial_setup").await;
        let state = h.state().await;
        assert_eq!(state["devices"], json!([]));
        assert!(state["selected"].is_null());
        assert_eq!(state["startup"], "recovery_required");
        assert_eq!(h.effects.probes.load(Ordering::SeqCst), 1);
        assert_eq!(h.effects.connections.load(Ordering::SeqCst), 1);
        assert_eq!(h.effects.pairs.load(Ordering::SeqCst), 1);
        let marker = &state["pairing_recovery"];
        assert!(marker["device_id"].is_string());
        assert_eq!(
            marker["stage"],
            match case {
                1 | 2 => "prepared",
                3 | 4 => "token_saved",
                _ => "trust_saved",
            }
        );
        assert!(marker["cleanup"].is_null());
        let path = format!(
            "/v1/devices/{}/recover-pairing",
            marker["device_id"].as_str().unwrap()
        );
        let recovery = json!({"device_id":marker["device_id"],"transaction_id":marker["transaction_id"],"confirm_device_id":marker["device_id"]});
        if case == 7 {
            let (_, partial) = h.mutate(1, "POST", &path, recovery.clone()).await;
            assert_eq!(partial["result"]["kind"], "partial_setup");
            assert_eq!(partial["result"]["credential_removed"], false);
            assert_eq!(h.state().await["startup"], "recovery_required");
        }
        *h.faults.secret_delete.error.lock().unwrap() = None;
        let (_, recovered) = h.mutate(1, "POST", &path, recovery).await;
        assert_eq!(recovered["result"]["kind"], "forgotten");
        assert_eq!(h.state().await["startup"], "ready");
        assert_eq!(h.effects.connections.load(Ordering::SeqCst), 1);
        h.close().await;
    }
}

#[tokio::test]
async fn failed_selection_wake_edits_and_partial_forget_retain_http_facts_without_tv_io() {
    use samsung_tv_remote::application::device_repository::RepositoryError;
    for error in [RepositoryError::Unavailable, RepositoryError::Corrupt] {
        for wake in [false, true] {
            let h = Harness::new().await;
            let device = h.pair().await;
            let generation = h.state().await["selection_generation"].clone();
            let (key, result) = if wake {
                *h.faults.device_save.error.lock().unwrap() = Some(error);
                h.mutate(1,"PUT",&format!("/v1/devices/{device}/wake"),json!({"device_id":device,"selection_generation":generation,"expected_config_revision":"0","wired_mac":"02:00:00:00:00:01","active_interface":"Wired"})).await
            } else {
                *h.faults.select.error.lock().unwrap() = Some(error);
                h.mutate(
                    1,
                    "POST",
                    "/v1/selection",
                    json!({"device_id":device,"expected_generation":generation}),
                )
                .await
            };
            let (status, code) = match error {
                RepositoryError::Unavailable => (503, "storage_unavailable"),
                RepositoryError::Corrupt => (423, "storage_corrupt"),
            };
            assert_retained_failure(&h, &key, &result, status, code).await;
            assert_eq!(h.state().await["selected"], device);
            assert_eq!(h.effects.connections.load(Ordering::SeqCst), 1);
            h.close().await;
        }
    }
    for failure in 0..3 {
        let h = Harness::new().await;
        let device = h.pair().await;
        match failure {
            0 => *h.faults.secret_delete.error.lock().unwrap() = Some(SecretError::Unavailable),
            1 => {
                *h.faults.trust_delete.error.lock().unwrap() =
                    Some(samsung_tv_remote::application::certificate_trust::TrustError::Unavailable)
            }
            _ => *h.faults.device_delete.error.lock().unwrap() = Some(RepositoryError::Unavailable),
        }
        let (key,result)=h.mutate(1,"POST",&format!("/v1/devices/{device}/forget"),json!({"device_id":device,"expected_config_revision":"0","confirm_device_id":device})).await;
        assert_retained_failure(&h, &key, &result, 423, "partial_setup").await;
        assert_eq!(result["result"]["kind"], "partial_setup");
        assert_eq!(result["result"]["credential_removed"], failure != 0);
        assert_eq!(result["result"]["trust_removed"], failure != 1);
        assert_eq!(result["result"]["record_removed"], failure != 2);
        let state = h.state().await;
        assert_eq!(state["startup"], "recovery_required");
        assert!(state["selected"].is_null());
        let marker = &state["forget_recovery"];
        assert_eq!(marker["device_id"], device);
        *h.faults.secret_delete.error.lock().unwrap() = None;
        *h.faults.trust_delete.error.lock().unwrap() = None;
        *h.faults.device_delete.error.lock().unwrap() = None;
        let (_,recovered)=h.mutate(1,"POST",&format!("/v1/devices/{device}/recover-forget"),json!({"device_id":device,"transaction_id":marker["transaction_id"],"confirm_device_id":device})).await;
        assert_eq!(recovered["result"]["kind"], "forgotten");
        assert_eq!(h.state().await["startup"], "ready");
        assert_eq!(h.effects.connections.load(Ordering::SeqCst), 1);
        h.close().await;
    }
}

async fn wait_until(mut check: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !check() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn storage_deadline_and_supervisor_failure_are_retained_without_replaying_effects() {
    let h=Harness::seeded(RuntimeLimits {storage_deadline:Duration::from_millis(500),..Default::default()},false,|path| {
        std::fs::write(path.join("devices.json"),serde_json::to_vec(&json!({"version":2,"selected":"dev_1","devices":[{"id":"dev_1","label":"Stored synthetic","host":"tv.local"}]})).unwrap()).unwrap();
    }).await;
    let device = wire::device(DeviceId::new(1));
    let (gate, release) = storage_faults::Gate::closed();
    *h.faults.select.gate.lock().unwrap() = Some(gate.clone());
    let generation = h.state().await["selection_generation"].clone();
    let (key, result) = h
        .mutate(
            1,
            "POST",
            "/v1/selection",
            json!({"device_id":device,"expected_generation":generation}),
        )
        .await;
    assert!(gate.entered.load(Ordering::SeqCst));
    assert_retained_failure(&h, &key, &result, 503, "deadline").await;
    assert_eq!(h.state().await["storage_pending"], true);
    assert_eq!(h.effects.connections.load(Ordering::SeqCst), 0);
    drop(release);
    tokio::time::timeout(Duration::from_secs(3), async {
        while h.state().await["storage_pending"] == true {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        h.terminal(1, result["server_epoch"].as_str().unwrap(), &key)
            .await,
        result
    );
    h.close().await;
    let h = Harness::new().await;
    h.effects.probe_panic.store(true, Ordering::SeqCst);
    let (key, result) = h
        .mutate(
            1,
            "POST",
            "/v1/probes",
            json!({"host":"synthetic-tv.invalid"}),
        )
        .await;
    assert_retained_failure(&h, &key, &result, 503, "supervisor_failed").await;
    assert_eq!(h.effects.probes.load(Ordering::SeqCst), 1);
    assert_eq!(h.effects.connections.load(Ordering::SeqCst), 0);
    h.close().await;
}

#[tokio::test]
async fn drain_retains_storage_uncertainty_and_pending_operation_cancellation_over_http() {
    let mut h = Harness::with_limits(RuntimeLimits {
        drain_deadline: Duration::from_millis(50),
        ..Default::default()
    })
    .await;
    let device = h.pair().await;
    let state = h.state().await;
    let epoch = state["server_epoch"].as_str().unwrap();
    h.effects.probe_wait.store(true, Ordering::SeqCst);
    let probe_key = uuid::Uuid::new_v4().to_string();
    let (status, probe) = h
        .json(
            1,
            "POST",
            "/v1/probes",
            json!({"server_epoch":epoch,"operation_key":probe_key,"host":"synthetic-tv.invalid"}),
        )
        .await;
    assert_eq!(status, 202);
    assert_eq!(probe["state"], "pending");
    let (gate, release) = storage_faults::Gate::closed();
    *h.faults.select.gate.lock().unwrap() = Some(gate.clone());
    let key = uuid::Uuid::new_v4().to_string();
    let (status,selection)=h.json(1,"POST","/v1/selection",json!({"server_epoch":epoch,"operation_key":key,"device_id":device,"expected_generation":state["selection_generation"]})).await;
    assert_eq!(status, 202);
    assert_eq!(selection["state"], "pending");
    wait_until(|| gate.entered.load(Ordering::SeqCst)).await;
    let Owner::Runtime(runtime) = std::mem::replace(&mut h.owner, Owner::Detached) else {
        panic!("runtime fixture")
    };
    let drain = runtime.drain().await;
    assert!(drain.status.unresolved_storage);
    assert!(!drain.is_settled());
    let uncertain = h.terminal(1, epoch, &key).await;
    assert_retained_failure(&h, &key, &uncertain, 503, "storage_uncertain").await;
    let cancelled = h.terminal(1, epoch, &probe_key).await;
    assert_retained_failure(&h, &probe_key, &cancelled, 503, "server_stopping").await;
    assert_eq!(h.state().await["startup"], "draining");
    assert_eq!(cancelled["terminal_state"]["selected"], device);
    assert_eq!(
        cancelled["terminal_state"]["selection_generation"],
        state["selection_generation"]
    );
    assert_eq!(h.effects.probes.load(Ordering::SeqCst), 2);
    assert_eq!(h.effects.connections.load(Ordering::SeqCst), 1);
    drop(release);
    h.effects.probe_release.notify_one();
    tokio::time::timeout(Duration::from_secs(3), drain.settled())
        .await
        .unwrap()
        .unwrap();
    h.close().await;
}

#[tokio::test]
async fn pairing_and_connect_session_failures_return_their_http_status_without_storage_or_remote_effects(
) {
    for pairing in [false, true] {
        for approval in [false, true] {
            for (error, status, code) in session_failures() {
                let h = Harness::new().await;
                let (path, extra) = if pairing {
                    let (_, probe) = h
                        .mutate(
                            1,
                            "POST",
                            "/v1/probes",
                            json!({"host":"synthetic-tv.invalid"}),
                        )
                        .await;
                    let observed = &probe["result"];
                    (
                        "/v1/pairings",
                        json!({"host":observed["host"],"attempt_id":observed["attempt_id"],"observation_id":observed["observation_id"],"fingerprint":observed["fingerprint"],"confirm_fingerprint":observed["fingerprint"],"selection_generation":"0","label":"Synthetic"}),
                    )
                } else {
                    let device = h.pair().await;
                    let generation = h.state().await["selection_generation"].clone();
                    h.mutate(
                        1,
                        "POST",
                        "/v1/selection",
                        json!({"device_id":device,"expected_generation":generation}),
                    )
                    .await;
                    (
                        "/v1/connections",
                        json!({"device_id":device,"selection_generation":h.state().await["selection_generation"]}),
                    )
                };
                let stores = h.faults.secret_save.calls.load(Ordering::SeqCst);
                let connections = h.effects.connections.load(Ordering::SeqCst);
                let approvals = h.effects.pairs.load(Ordering::SeqCst);
                if approval {
                    *h.effects.approval_error.lock().unwrap() = Some(error);
                } else {
                    *h.effects.connect_error.lock().unwrap() = Some(error);
                }
                let (key, result) = h.mutate(1, "POST", path, extra).await;
                assert_retained_failure(&h, &key, &result, status, code).await;
                assert_eq!(h.faults.secret_save.calls.load(Ordering::SeqCst), stores);
                assert_eq!(
                    h.effects.connections.load(Ordering::SeqCst),
                    connections + 1
                );
                assert_eq!(
                    h.effects.pairs.load(Ordering::SeqCst),
                    approvals + usize::from(pairing && approval)
                );
                h.close().await;
            }
        }
    }
}

#[tokio::test]
async fn reconnect_material_and_rotated_credential_failures_are_private_retained_http_results() {
    use samsung_tv_remote::application::{
        certificate_trust::{CertificateTrustStore, TrustError, TrustRecord},
        device_repository::RepositoryError,
    };
    for (case, status, code) in [
        (0, 503, "storage_unavailable"),
        (1, 423, "storage_corrupt"),
        (2, 503, "trust_unavailable"),
        (3, 423, "trust_corrupt"),
        (4, 423, "storage_blocked"),
        (5, 423, "invalid_pairing_material"),
        (6, 404, "not_found"),
        (7, 412, "pairing_required"),
        (8, 412, "certificate_changed"),
        (9, 412, "pairing_required"),
    ] {
        let h = Harness::new().await;
        let device = h.pair().await;
        let generation = h.state().await["selection_generation"].clone();
        h.mutate(
            1,
            "POST",
            "/v1/selection",
            json!({"device_id":device,"expected_generation":generation}),
        )
        .await;
        match case {
            0 => *h.faults.list.error.lock().unwrap() = Some(RepositoryError::Unavailable),
            1 => *h.faults.list.error.lock().unwrap() = Some(RepositoryError::Corrupt),
            2 => *h.faults.trust_load.error.lock().unwrap() = Some(TrustError::Unavailable),
            3 => *h.faults.trust_load.error.lock().unwrap() = Some(TrustError::Corrupt),
            4 => *h.faults.secret_load.error.lock().unwrap() = Some(SecretError::Unavailable),
            5 => *h.faults.secret_load.error.lock().unwrap() = Some(SecretError::InvalidToken),
            6 => h.faults.hide_records.store(true, Ordering::SeqCst),
            7 => LocalCertificateTrustStore::new(h._profile.path())
                .delete(wire::parse_device(&device).unwrap())
                .unwrap(),
            8 => LocalCertificateTrustStore::new(h._profile.path())
                .save(
                    wire::parse_device(&device).unwrap(),
                    &TrustRecord {
                        host: TvHost::parse("different.invalid").unwrap(),
                        pin: CertificatePin::from_der(b"synthetic"),
                    },
                )
                .unwrap(),
            _ => h.faults.missing_secret.store(true, Ordering::SeqCst),
        }
        let (key,result)=h.mutate(1,"POST","/v1/connections",json!({"device_id":device,"selection_generation":h.state().await["selection_generation"]})).await;
        assert_retained_failure(&h, &key, &result, status, code).await;
        assert_eq!(h.effects.connections.load(Ordering::SeqCst), 1);
        assert_eq!(h.effects.pairs.load(Ordering::SeqCst), 1);
        h.close().await;
    }
    let h = Harness::new().await;
    let device = h.pair().await;
    let generation = h.state().await["selection_generation"].clone();
    h.mutate(
        1,
        "POST",
        "/v1/selection",
        json!({"device_id":device,"expected_generation":generation}),
    )
    .await;
    h.effects.rotate.store(true, Ordering::SeqCst);
    *h.faults.secret_save.error.lock().unwrap() = Some(SecretError::Unavailable);
    let (key,result)=h.mutate(1,"POST","/v1/connections",json!({"device_id":device,"selection_generation":h.state().await["selection_generation"]})).await;
    assert_retained_failure(&h, &key, &result, 423, "storage_blocked").await;
    assert_eq!(h.state().await["startup"], "recovery_required");
    assert_eq!(h.effects.connections.load(Ordering::SeqCst), 2);
    assert_eq!(h.faults.secret_save.calls.load(Ordering::SeqCst), 2);
    h.close().await;
}

#[tokio::test]
async fn generation_changes_cancel_late_pairing_and_connect_without_adopting_or_saving_them() {
    for pairing in [false, true] {
        let h = Harness::new().await;
        let device = h.pair().await;
        let state = h.state().await;
        let epoch = state["server_epoch"].as_str().unwrap();
        let generation = state["selection_generation"].clone();
        let key = uuid::Uuid::new_v4().to_string();
        let path = if pairing {
            "/v1/pairings"
        } else {
            "/v1/connections"
        };
        let mut body = json!({"server_epoch":epoch,"operation_key":key,"device_id":device,"selection_generation":generation});
        if pairing {
            let (_, probe) = h
                .mutate(1, "POST", "/v1/probes", json!({"host":"tv.local"}))
                .await;
            let observed = &probe["result"];
            body.as_object_mut().unwrap().extend(json!({"host":"tv.local","attempt_id":observed["attempt_id"],"observation_id":observed["observation_id"],"fingerprint":observed["fingerprint"],"confirm_fingerprint":observed["fingerprint"],"label":"Never saved"}).as_object().unwrap().clone());
            h.effects.approval_wait.store(true, Ordering::SeqCst);
        } else {
            h.effects.connect_wait.store(true, Ordering::SeqCst);
        }
        let stores = h.faults.secret_save.calls.load(Ordering::SeqCst);
        let (status, pending) = h.json(1, "POST", path, body).await;
        assert_eq!(status, 202);
        assert_eq!(pending["state"], "pending");
        wait_until(|| {
            if pairing {
                h.effects.pairs.load(Ordering::SeqCst) == 2
            } else {
                h.effects.connections.load(Ordering::SeqCst) == 2
            }
        })
        .await;
        h.mutate(
            2,
            "POST",
            "/v1/selection",
            json!({"device_id":device,"expected_generation":generation}),
        )
        .await;
        if pairing {
            h.effects.approval_release.notify_one();
        } else {
            h.effects.connect_release.notify_one();
        }
        let result = h.terminal(1, epoch, &key).await;
        assert_retained_failure(&h, &key, &result, 409, "cancelled").await;
        assert_eq!(h.faults.secret_save.calls.load(Ordering::SeqCst), stores);
        let after = h.state().await;
        assert_eq!(after["selected"], device);
        assert_eq!(after["connection"], "not_connected");
        assert_eq!(after["devices"][0]["label"], "Synthetic TV");
        assert_eq!(h.effects.connections.load(Ordering::SeqCst), 2);
        h.close().await;
    }
}

#[tokio::test]
async fn idle_launch_connect_cannot_replace_another_clients_ready_or_busy_work() {
    for competing in ["ready", "connect", "pair", "wake"] {
        let h = Harness::new().await;
        let device = h.pair().await;
        let state = h.state().await;
        let generation = state["selection_generation"].clone();
        if competing == "wake" {
            h.mutate(1,"PUT",&format!("/v1/devices/{device}/wake"),json!({"device_id":device,"selection_generation":generation,"expected_config_revision":state["devices"][0]["config_revision"],"wired_mac":"02:00:00:00:00:01","active_interface":"Wired"})).await;
        }
        if competing != "ready" {
            h.mutate(
                1,
                "POST",
                "/v1/selection",
                json!({"device_id":device,"expected_generation":generation}),
            )
            .await;
        }
        // This is the automatic client's observed idle selection. The other
        // client starts work before its request reaches owner admission.
        let observed = h.state().await;
        let epoch = observed["server_epoch"].as_str().unwrap();
        let generation = observed["selection_generation"].clone();
        let mut pending_key = None;
        if competing != "ready" {
            let key = uuid::Uuid::new_v4().to_string();
            let mut body = json!({"server_epoch":epoch,"operation_key":key,"device_id":device,"selection_generation":generation});
            let path = match competing {
                "pair" => {
                    let (_, probe) = h
                        .mutate(1, "POST", "/v1/probes", json!({"host":"tv.local"}))
                        .await;
                    let o = &probe["result"];
                    body.as_object_mut().unwrap().extend(json!({"host":o["host"],"attempt_id":o["attempt_id"],"observation_id":o["observation_id"],"fingerprint":o["fingerprint"],"confirm_fingerprint":o["fingerprint"],"label":"Synthetic TV"}).as_object().unwrap().clone());
                    h.effects.approval_wait.store(true, Ordering::SeqCst);
                    "/v1/pairings"
                }
                "wake" => {
                    h.effects.wake_wait.store(true, Ordering::SeqCst);
                    "/v1/wakes"
                }
                _ => {
                    h.effects.connect_wait.store(true, Ordering::SeqCst);
                    "/v1/connections"
                }
            };
            assert_eq!(h.json(1, "POST", path, body).await.0, 202);
            if competing == "pair" {
                wait_until(|| h.effects.pairs.load(Ordering::SeqCst) == 2).await;
            } else if competing == "connect" {
                wait_until(|| h.effects.connections.load(Ordering::SeqCst) == 2).await;
            }
            pending_key = Some(key);
        }
        let before = h.state().await;
        let connections = h.effects.connections.load(Ordering::SeqCst);
        let packets = h.effects.packets.load(Ordering::SeqCst);
        let (key, result) = h
            .mutate(
                2,
                "POST",
                "/v1/connections",
                json!({"device_id":device,"selection_generation":generation,"only_if_idle":true}),
            )
            .await;
        assert_eq!(result["result"]["kind"], "failed", "{competing}: {result}");
        assert_eq!(
            result["result"]["error"]["code"], "busy",
            "{competing}: {result}"
        );
        assert_eq!(h.terminal(2, epoch, &key).await, result);
        let after = h.state().await;
        assert_eq!(after["connection"], before["connection"], "{competing}");
        assert_eq!(after["wake"], before["wake"], "{competing}");
        assert_eq!(
            after["pairing_pending"], before["pairing_pending"],
            "{competing}"
        );
        assert_eq!(h.effects.connections.load(Ordering::SeqCst), connections);
        assert_eq!(h.effects.packets.load(Ordering::SeqCst), packets);
        h.effects.approval_release.notify_one();
        h.effects.connect_release.notify_one();
        h.effects.wake_release.notify_one();
        if let Some(key) = pending_key {
            let result = h.terminal(1, epoch, &key).await;
            assert_ne!(result["result"]["kind"], "failed", "{competing}: {result}");
        }
        h.close().await;
    }
}

#[tokio::test]
async fn idle_launch_connect_starts_once_and_retains_its_success_after_becoming_ready() {
    let h = Harness::new().await;
    let device = h.pair().await;
    let state = h.state().await;
    h.mutate(
        1,
        "POST",
        "/v1/selection",
        json!({"device_id":device,"expected_generation":state["selection_generation"]}),
    )
    .await;
    let observed = h.state().await;
    assert_eq!(observed["connection"], "not_connected");
    let key = uuid::Uuid::new_v4().to_string();
    let body = json!({"server_epoch":observed["server_epoch"],"operation_key":key,"device_id":device,"selection_generation":observed["selection_generation"],"only_if_idle":true});
    let connections = h.effects.connections.load(Ordering::SeqCst);
    assert_eq!(
        h.json(2, "POST", "/v1/connections", body.clone()).await.0,
        202
    );
    let result = h
        .terminal(2, observed["server_epoch"].as_str().unwrap(), &key)
        .await;
    assert_eq!(result["result"]["kind"], "remote_ready");
    assert_eq!(h.state().await["connection"], "ready");
    // The original operation remains identifiable after its precondition is
    // false. Duplicate requests do not become new attempts or policy failures.
    let (status, duplicate) = h.json(2, "POST", "/v1/connections", body).await;
    assert_eq!(status, 200);
    assert_eq!(duplicate, result);
    assert_eq!(
        h.effects.connections.load(Ordering::SeqCst),
        connections + 1
    );
    h.close().await;
}

#[tokio::test]
async fn wake_cancellation_over_http_preserves_packet_facts_and_drops_late_connections() {
    for stage in 0..4 {
        let h = Harness::new().await;
        let device = h.pair().await;
        let generation = h.state().await["selection_generation"].clone();
        h.mutate(1,"PUT",&format!("/v1/devices/{device}/wake"),json!({"device_id":device,"selection_generation":generation,"expected_config_revision":"0","wired_mac":"02:00:00:00:00:01","active_interface":"Wired"})).await;
        h.mutate(
            1,
            "POST",
            "/v1/selection",
            json!({"device_id":device,"expected_generation":generation}),
        )
        .await;
        let state = h.state().await;
        let epoch = state["server_epoch"].as_str().unwrap();
        let generation = state["selection_generation"].clone();
        let key = uuid::Uuid::new_v4().to_string();
        if stage == 1 {
            h.effects.wake_wait.store(true, Ordering::SeqCst);
        }
        if stage == 0 || stage == 2 {
            h.effects.connect_wait.store(true, Ordering::SeqCst);
        }
        let mut body = json!({"server_epoch":epoch,"operation_key":key,"device_id":device,"selection_generation":generation});
        let route = if stage == 0 || stage == 3 {
            body["action"] = json!("PowerToggle");
            "/v1/remote-actions"
        } else {
            "/v1/wakes"
        };
        let (_, pending) = h.json(1, "POST", route, body).await;
        if stage == 3 {
            let result = h.terminal(1, epoch, &key).await;
            assert_eq!(result["result"]["kind"], "remote_ready");
            let after = h.state().await;
            assert_eq!(after["wake"]["stage"], "connected");
            assert_eq!(after["wake"]["packet"], "not_sent");
            assert_eq!(h.effects.packets.load(Ordering::SeqCst), 0);
        } else {
            tokio::time::timeout(Duration::from_secs(3), async {
                let desired = match stage {
                    0 => "checking_connection",
                    1 => "packet_sending",
                    _ => "reconnecting",
                };
                while h.state().await["wake"]["stage"] != desired {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            if stage == 0 || stage == 2 {
                // The stage is published before the material read finishes.
                // Wait for the held network call, so this case specifically
                // proves late connection completion rather than earlier cancel.
                wait_until(|| h.effects.connections.load(Ordering::SeqCst) == 2).await;
            }
            let (_, cancelled) = h
                .mutate(
                    1,
                    "POST",
                    &format!(
                        "/v1/wakes/{}/cancel",
                        pending["operation_id"].as_str().unwrap()
                    ),
                    json!({"device_id":device,"selection_generation":generation}),
                )
                .await;
            let expected_stage = match stage {
                0 => "cancelled",
                1 => "cancelled_during_send",
                _ => "cancelled_after_send",
            };
            let packet = match stage {
                0 => "not_sent",
                1 => "uncertain",
                _ => "sent",
            };
            assert_eq!(cancelled["result"]["facts"]["stage"], expected_stage);
            assert_eq!(cancelled["result"]["facts"]["packet"], packet);
            let original = h.terminal(1, epoch, &key).await;
            assert_eq!(original["result"]["facts"], cancelled["result"]["facts"]);
            h.effects.wake_release.notify_one();
            h.effects.connect_release.notify_one();
            wait_until(|| {
                if stage == 1 {
                    h.effects.wake_returned.load(Ordering::SeqCst) == 1
                } else {
                    h.effects.connect_returned.load(Ordering::SeqCst) == 2
                }
            })
            .await;
            assert_eq!(h.terminal(1, epoch, &key).await, original);
            assert_ne!(h.state().await["connection"], "ready");
            assert_eq!(
                h.effects.packets.load(Ordering::SeqCst),
                usize::from(stage == 2)
            );
        }
        assert_eq!(h.effects.remote_attempts.load(Ordering::SeqCst), 0);
        h.close().await;
    }
}

#[tokio::test]
async fn policy_rejections_are_retained_while_ingress_confirmation_failures_create_no_record() {
    let h = Harness::new().await;
    let missing = wire::device(DeviceId::generate());
    let (key, result) = h
        .mutate(
            1,
            "POST",
            "/v1/remote-actions",
            json!({"device_id":missing,"selection_generation":"0","action":"VolumeUp"}),
        )
        .await;
    assert_retained_failure(&h, &key, &result, 409, "stale_selection").await;
    assert_eq!(h.effects.connections.load(Ordering::SeqCst), 0);
    let device = h.pair().await;
    let generation = h.state().await["selection_generation"].clone();
    for (route, extra, status, code) in [
        (
            "/v1/selection".to_owned(),
            json!({"device_id":missing,"expected_generation":generation}),
            404,
            "not_found",
        ),
        (
            "/v1/selection".to_owned(),
            json!({"device_id":device,"expected_generation":"0"}),
            409,
            "stale_selection",
        ),
        (
            format!("/v1/devices/{device}/wake"),
            json!({"device_id":device,"selection_generation":generation,"expected_config_revision":"1"}),
            409,
            "stale_config",
        ),
        (
            format!("/v1/devices/{device}/wake"),
            json!({"device_id":device,"selection_generation":generation,"expected_config_revision":"0","active_interface":"Wired"}),
            400,
            "invalid_configuration",
        ),
        (
            "/v1/wakes".to_owned(),
            json!({"device_id":device,"selection_generation":generation}),
            412,
            "wake_not_configured",
        ),
        (
            format!("/v1/wakes/{}/cancel", wire::operation(OperationId(999))),
            json!({"device_id":device,"selection_generation":generation}),
            404,
            "not_found",
        ),
    ] {
        let method = if route.ends_with("/wake") {
            "PUT"
        } else {
            "POST"
        };
        let (key, result) = h.mutate(1, method, &route, extra).await;
        assert_retained_failure(&h, &key, &result, status, code).await;
    }
    let (_, probe) = h
        .mutate(1, "POST", "/v1/probes", json!({"host":"tv.local"}))
        .await;
    let observed = &probe["result"];
    let pair = json!({"host":"tv.local","attempt_id":observed["attempt_id"],"observation_id":observed["observation_id"],"fingerprint":observed["fingerprint"],"confirm_fingerprint":observed["fingerprint"],"label":"Duplicate host","selection_generation":generation});
    let (key, result) = h.mutate(1, "POST", "/v1/pairings", pair.clone()).await;
    assert_retained_failure(&h, &key, &result, 400, "invalid_confirmation").await;
    for label in ["".to_owned(), "x".repeat(81), "bad\nlabel".into()] {
        let mut body = pair.clone();
        body["label"] = json!(label);
        let (key, result) = h.mutate(1, "POST", "/v1/pairings", body).await;
        assert_retained_failure(&h, &key, &result, 400, "input_too_large").await;
    }
    let before = h.state().await;
    let key = uuid::Uuid::new_v4().to_string();
    let (status,invalid)=h.json(1,"POST",&format!("/v1/devices/{device}/forget"),json!({"server_epoch":before["server_epoch"],"operation_key":key,"device_id":device,"expected_config_revision":"0","confirm_device_id":missing})).await;
    assert_eq!(status, 400);
    assert_eq!(invalid["error"]["code"], "invalid_confirmation");
    assert!(invalid.get("operation_id").is_none());
    assert_eq!(
        h.state().await["recent_operations"],
        before["recent_operations"]
    );
    assert_eq!(h.effects.connections.load(Ordering::SeqCst), 1);
    assert_eq!(h.effects.pairs.load(Ordering::SeqCst), 1);
    assert_eq!(h.faults.secret_save.calls.load(Ordering::SeqCst), 1);
    assert_eq!(h.faults.secret_delete.calls.load(Ordering::SeqCst), 0);
    h.close().await;
}

#[tokio::test]
async fn owner_effect_and_remote_fifo_overload_are_retained_without_dispatching_rejected_work() {
    let h = Harness::with_limits(RuntimeLimits {
        effects: 1,
        ..Default::default()
    })
    .await;
    h.effects.probe_wait.store(true, Ordering::SeqCst);
    let state = h.state().await;
    let epoch = state["server_epoch"].as_str().unwrap();
    let key = uuid::Uuid::new_v4().to_string();
    let (status, _) = h
        .json(
            1,
            "POST",
            "/v1/probes",
            json!({"server_epoch":epoch,"operation_key":key,"host":"synthetic-tv.invalid"}),
        )
        .await;
    assert_eq!(status, 202);
    let (busy_key, busy) = h
        .mutate(
            1,
            "POST",
            "/v1/probes",
            json!({"host":"synthetic-tv.invalid"}),
        )
        .await;
    assert_retained_failure(&h, &busy_key, &busy, 429, "busy").await;
    assert_eq!(h.effects.probes.load(Ordering::SeqCst), 1);
    h.effects.probe_release.notify_one();
    assert_eq!(h.terminal(1, epoch, &key).await["result"]["kind"], "probed");
    h.close().await;
    let h = Harness::new().await;
    let device = h.pair().await;
    let state = h.state().await;
    let epoch = state["server_epoch"].as_str().unwrap();
    h.effects.remote_mode.store(5, Ordering::SeqCst);
    let mut keys = Vec::new();
    for _ in 0..33 {
        let key = uuid::Uuid::new_v4().to_string();
        let (status,receipt)=h.json(1,"POST","/v1/remote-actions",json!({"server_epoch":epoch,"operation_key":key,"device_id":device,"selection_generation":state["selection_generation"],"action":"VolumeUp"})).await;
        assert_eq!(status, 202);
        assert_eq!(receipt["state"], "pending");
        keys.push(key);
    }
    let (busy_key,busy)=h.mutate(1,"POST","/v1/remote-actions",json!({"device_id":device,"selection_generation":state["selection_generation"],"action":"VolumeUp"})).await;
    assert_eq!(busy["error"]["code"], "busy");
    let path = format!("/v1/operations/by-key/{busy_key}?epoch={epoch}");
    assert_eq!(h.json(1, "GET", &path, Value::Null).await.0, 429);
    assert_eq!(h.effects.remote_attempts.load(Ordering::SeqCst), 1);
    h.effects.remote_mode.store(0, Ordering::SeqCst);
    h.effects.remote_release.notify_one();
    for key in keys {
        assert_eq!(
            h.terminal(1, epoch, &key).await["result"]["kind"],
            "written"
        );
    }
    assert_eq!(h.effects.remote_attempts.load(Ordering::SeqCst), 33);
    assert_eq!(h.effects.writes.load(Ordering::SeqCst), 33);
    assert_eq!(h.terminal(1, epoch, &busy_key).await, busy);
    assert_eq!(h.effects.packets.load(Ordering::SeqCst), 0);
    h.close().await;
}

#[tokio::test]
async fn every_setup_boundary_error_has_a_retained_http_fixture_and_no_store_commit() {
    use samsung_tv_remote::application::{
        certificate_trust::TrustError, device_repository::RepositoryError,
        tv_setup_service::SetupError,
    };
    for (error, status, code) in [
        (SetupError::InvalidLabel, 400, "invalid_label"),
        (
            SetupError::Credential(SecretError::Unavailable),
            423,
            "storage_blocked",
        ),
        (
            SetupError::Credential(SecretError::InvalidToken),
            423,
            "invalid_pairing_material",
        ),
        (
            SetupError::Trust(TrustError::Unavailable),
            503,
            "trust_unavailable",
        ),
        (SetupError::Trust(TrustError::Corrupt), 423, "trust_corrupt"),
        (
            SetupError::Preferences(RepositoryError::Unavailable),
            503,
            "storage_unavailable",
        ),
        (
            SetupError::Preferences(RepositoryError::Corrupt),
            423,
            "storage_corrupt",
        ),
    ] {
        let h = Harness::new().await;
        *h.faults.setup_commit.error.lock().unwrap() = Some(error);
        let (_, probe) = h
            .mutate(
                1,
                "POST",
                "/v1/probes",
                json!({"host":"synthetic-tv.invalid"}),
            )
            .await;
        let observed = &probe["result"];
        let (key,result)=h.mutate(1,"POST","/v1/pairings",json!({"host":observed["host"],"attempt_id":observed["attempt_id"],"observation_id":observed["observation_id"],"fingerprint":observed["fingerprint"],"confirm_fingerprint":observed["fingerprint"],"label":"Synthetic","selection_generation":"0"})).await;
        assert_retained_failure(&h, &key, &result, status, code).await;
        assert_eq!(h.faults.setup_commit.calls.load(Ordering::SeqCst), 1);
        assert_eq!(h.faults.secret_save.calls.load(Ordering::SeqCst), 0);
        assert_eq!(h.faults.trust_save.calls.load(Ordering::SeqCst), 0);
        assert_eq!(h.faults.device_save.calls.load(Ordering::SeqCst), 0);
        assert_eq!(h.effects.probes.load(Ordering::SeqCst), 1);
        assert_eq!(h.effects.connections.load(Ordering::SeqCst), 1);
        assert_eq!(h.effects.pairs.load(Ordering::SeqCst), 1);
        assert_eq!(h.state().await["devices"], json!([]));
        h.close().await;
    }
    // PartialCleanup is exercised with a real incomplete journal and failed
    // removals in pairing_storage_faults..., rather than inventing a markerless
    // partial state at this pre-commit boundary.
}

#[tokio::test]
async fn stalled_large_responses_keep_their_query_reservations_until_connection_close() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let disk=serde_json::to_vec(&json!({"version":2,"selected":null,"devices":(0..1024).map(|n|json!({"id":format!("dev_{n}"),"label":"\u{0001}".repeat(256),"host":"synthetic-tv.invalid"})).collect::<Vec<_>>()})).unwrap();
    let h = Harness::seeded(RuntimeLimits::default(), false, |path| {
        std::fs::write(path.join("devices.json"), &disk).unwrap();
    })
    .await;
    let state = h.state().await;
    assert_eq!(state["devices"].as_array().unwrap().len(), 1024);
    let mut slow = Vec::new();
    for _ in 0..4 {
        let socket = tokio::net::TcpSocket::new_v4().unwrap();
        socket.set_recv_buffer_size(4096).unwrap();
        // A fixture drop resets the connection rather than leaving an unread
        // receive window alive until its normal ten-second write deadline.
        use std::os::fd::AsRawFd;
        let linger = libc::linger {
            l_onoff: 1,
            l_linger: 0,
        };
        // SAFETY: a live TCP descriptor, correctly sized zero-timeout option.
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    socket.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_LINGER,
                    (&linger as *const libc::linger).cast(),
                    std::mem::size_of_val(&linger) as libc::socklen_t,
                )
            },
            0
        );
        let mut stream = socket
            .connect((std::net::Ipv4Addr::LOCALHOST, h.port).into())
            .await
            .unwrap();
        stream.write_all(format!("GET /v1/state HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\n\r\n",h.port,"01".repeat(32)).as_bytes()).await.unwrap();
        let mut header = Vec::new();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !header.ends_with(b"\r\n\r\n") {
                header.push(stream.read_u8().await.unwrap());
                assert!(header.len() < 16384);
            }
        })
        .await
        .unwrap();
        assert!(header.starts_with(b"HTTP/1.1 200"));
        slow.push(stream);
    }
    let (status, busy) = h.json(1, "GET", "/v1/state", Value::Null).await;
    assert_eq!(status, 429);
    assert_eq!(busy["error"]["code"], "busy");
    assert_eq!(h.json(2, "GET", "/v1/ready", Value::Null).await.0, 200);
    let (status,_)=h.json(1,"POST","/v1/server/stop",json!({"server_epoch":state["server_epoch"],"operation_key":uuid::Uuid::new_v4().to_string()})).await;
    assert_eq!(status, 200);
    assert_eq!(h.stops.0.load(Ordering::SeqCst), 1);
    drop(slow);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let status = h.json(1, "GET", "/v1/ready", Value::Null).await.0;
            if status == 200 {
                break;
            }
            assert_eq!(status, 429);
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(h.effects.probes.load(Ordering::SeqCst), 0);
    assert_eq!(h.effects.connections.load(Ordering::SeqCst), 0);
    assert_eq!(h.effects.writes.load(Ordering::SeqCst), 0);
    h.close().await;
}
