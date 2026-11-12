use hyper::http::HeaderMap;
use samsung_tv_remote::application::control_service::ClientId;
use samsung_tv_remote::server::{access::*, bootstrap::*};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UnixStream;

struct Secrets;
impl ApiCredentials for Secrets {
    fn load(&self, role: Role) -> Result<[u8; 32], BootstrapError> {
        Ok(match role {
            Role::Gui => [1; 32],
            Role::Mcp => [2; 32],
        })
    }
}
fn context() -> Context {
    Context {
        profile: "profile-fixture".into(),
        endpoint: "http://127.0.0.1:54321".into(),
        build: "build-fixture".into(),
        api: "v1".into(),
        epoch: uuid::Uuid::new_v4().to_string(),
    }
}
fn server(
    verifier: Arc<dyn PeerVerifier>,
    store: Arc<dyn ApiCredentials>,
) -> (Arc<Bootstrap>, Arc<AccessGate>) {
    let gate = Arc::new(AccessGate::new(54321));
    let server = Arc::new(Bootstrap::new(gate.clone(), store, verifier, context()).unwrap());
    (server, gate)
}
fn peer() -> Arc<dyn PeerVerifier> {
    Arc::new(DevelopmentPeer { uid: socket::uid() })
}
fn hello(nonce: String) -> ClientFrame {
    ClientFrame::Hello(Hello {
        version: 1,
        role: Role::Gui,
        nonce,
        profile: "profile-fixture".into(),
    })
}
async fn challenge(
    server: Arc<Bootstrap>,
    nonce: String,
) -> (
    UnixStream,
    Challenge,
    tokio::task::JoinHandle<Result<(), BootstrapError>>,
) {
    let (mut client, mut socket) = UnixStream::pair().unwrap();
    let task = tokio::spawn(async move { server.exchange(&mut socket).await });
    write_frame(&mut client, &hello(nonce)).await.unwrap();
    let ServerFrame::Challenge(challenge) = read_frame(&mut client, Duration::from_secs(2))
        .await
        .unwrap()
    else {
        panic!("missing challenge")
    };
    (client, challenge, task)
}
fn probe(
    gate: &AccessGate,
    challenge: &Challenge,
    path: &str,
) -> Result<AuthorizedClient, AccessError> {
    let mut headers = HeaderMap::new();
    headers.insert("host", "127.0.0.1:54321".parse().unwrap());
    headers.insert(
        "authorization",
        format!("Bearer {}", challenge.probe_bearer)
            .parse()
            .unwrap(),
    );
    headers.insert(
        "x-bootstrap-challenge",
        challenge.challenge.parse().unwrap(),
    );
    gate.authorize(
        "127.0.0.1:1".parse().unwrap(),
        &path.parse().unwrap(),
        &headers,
    )
}

#[tokio::test]
async fn listener_proof_precedes_reusable_credentials_and_probe_is_one_use() {
    let (server, gate) = server(peer(), Arc::new(Secrets));
    let (mut client, challenge, task) = challenge(server, hex(&random().unwrap())).await;
    assert!(matches!(
        probe(&gate, &challenge, "/v1/state"),
        Err(AccessError::Forbidden)
    ));
    let authorized = probe(&gate, &challenge, "/v1/ready").unwrap();
    assert_eq!(authorized.identity, ClientId(1));
    assert_eq!(authorized.capability, Capability::Readiness);
    assert!(authorized.require_management().is_err());
    assert_eq!(
        authorized.readiness_proof.as_deref(),
        Some(proof(&challenge).unwrap().as_str())
    );
    assert!(probe(&gate, &challenge, "/v1/ready").is_err());
    write_frame(
        &mut client,
        &ClientFrame::Redeem(Redeem {
            challenge: challenge.challenge.clone(),
            proof: proof(&challenge).unwrap(),
        }),
    )
    .await
    .unwrap();
    let ServerFrame::Credentials(credentials) = read_frame(&mut client, Duration::from_secs(2))
        .await
        .unwrap()
    else {
        panic!("credentials not returned")
    };
    assert_eq!(credentials.bearer, hex(&[1; 32]));
    assert_eq!(credentials.role, Role::Gui);
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn unconsumed_challenges_replayed_nonces_and_revoked_identities_fail_closed() {
    let (server, gate) = server(peer(), Arc::new(Secrets));
    let nonce = hex(&random().unwrap());
    let (mut client, challenge, task) = challenge(server.clone(), nonce.clone()).await;
    write_frame(
        &mut client,
        &ClientFrame::Redeem(Redeem {
            challenge: challenge.challenge.clone(),
            proof: proof(&challenge).unwrap(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(task.await.unwrap(), Err(BootstrapError::Denied));
    let (mut client, mut socket) = UnixStream::pair().unwrap();
    let repeated = server.clone();
    let task = tokio::spawn(async move { repeated.exchange(&mut socket).await });
    write_frame(&mut client, &hello(nonce)).await.unwrap();
    assert_eq!(task.await.unwrap(), Err(BootstrapError::Replay));
    gate.revoke(ClientId(1));
    let (mut client, mut socket) = UnixStream::pair().unwrap();
    let task = tokio::spawn(async move { server.exchange(&mut socket).await });
    write_frame(&mut client, &hello(hex(&random().unwrap())))
        .await
        .unwrap();
    assert_eq!(task.await.unwrap(), Err(BootstrapError::Denied));
}
struct Blocked;
impl ApiCredentials for Blocked {
    fn load(&self, _: Role) -> Result<[u8; 32], BootstrapError> {
        Err(BootstrapError::InteractionRequired)
    }
}
struct RejectedIdentity;
impl PeerVerifier for RejectedIdentity {
    fn verify(&self, _: &UnixStream, _: Role) -> Result<(), BootstrapError> {
        Err(BootstrapError::Denied)
    }
}

#[tokio::test]
async fn peer_and_credential_gates_are_explicit_and_frames_are_bounded() {
    use tokio::io::AsyncWriteExt;
    for (verifier, store, expected) in [
        (
            Arc::new(DevelopmentPeer { uid: u32::MAX }) as Arc<dyn PeerVerifier>,
            Arc::new(Secrets) as Arc<dyn ApiCredentials>,
            BootstrapError::Denied,
        ),
        (
            Arc::new(RejectedIdentity),
            Arc::new(Secrets),
            BootstrapError::Denied,
        ),
        (
            peer(),
            Arc::new(Blocked),
            BootstrapError::InteractionRequired,
        ),
    ] {
        let (server, _) = server(verifier, store);
        let (mut client, mut socket) = UnixStream::pair().unwrap();
        let task = tokio::spawn(async move { server.exchange(&mut socket).await });
        write_frame(&mut client, &hello(hex(&random().unwrap())))
            .await
            .unwrap();
        assert_eq!(task.await.unwrap(), Err(expected));
    }
    let (server, _) = server(peer(), Arc::new(Secrets));
    let (mut client, mut socket) = UnixStream::pair().unwrap();
    let task = tokio::spawn(async move { server.exchange(&mut socket).await });
    client.write_u32((MAX_FRAME + 1) as u32).await.unwrap();
    assert_eq!(task.await.unwrap(), Err(BootstrapError::InvalidFrame));
}

#[tokio::test(start_paused = true)]
async fn probe_expiry_does_not_renew() {
    let (server, gate) = server(peer(), Arc::new(Secrets));
    let (mut client, challenge, task) = challenge(server, hex(&random().unwrap())).await;
    tokio::time::advance(Duration::from_secs(31)).await;
    assert!(probe(&gate, &challenge, "/v1/ready").is_err());
    let _ = write_frame(
        &mut client,
        &ClientFrame::Redeem(Redeem {
            challenge: challenge.challenge.clone(),
            proof: proof(&challenge).unwrap(),
        }),
    )
    .await;
    assert!(task.await.unwrap().is_err());
}

#[tokio::test]
async fn nonce_capacity_rejects_new_exchanges_until_expiry() {
    let (server, gate) = server(peer(), Arc::new(Secrets));
    for index in 0..128u64 {
        let mut bytes = [0; 32];
        bytes[..8].copy_from_slice(&index.to_be_bytes());
        let (mut client, challenge, task) = challenge(server.clone(), hex(&bytes)).await;
        probe(&gate, &challenge, "/v1/ready").unwrap();
        write_frame(
            &mut client,
            &ClientFrame::Redeem(Redeem {
                challenge: challenge.challenge.clone(),
                proof: proof(&challenge).unwrap(),
            }),
        )
        .await
        .unwrap();
        assert!(matches!(
            read_frame::<ServerFrame>(&mut client, Duration::from_secs(2))
                .await
                .unwrap(),
            ServerFrame::Credentials(_)
        ));
        task.await.unwrap().unwrap();
    }
    let (mut client, mut socket) = UnixStream::pair().unwrap();
    let saturated = server.clone();
    let task = tokio::spawn(async move { saturated.exchange(&mut socket).await });
    write_frame(&mut client, &hello(hex(&[255; 32])))
        .await
        .unwrap();
    assert_eq!(task.await.unwrap(), Err(BootstrapError::Busy));
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(31)).await;
    let (client, _, task) = challenge(server, hex(&[255; 32])).await;
    drop(client);
    assert!(task.await.unwrap().is_err());
}

async fn observe_listener(valid: bool, serving: bool, status: u16) -> BootstrapError {
    use samsung_tv_remote::control_client::bootstrap::authenticate;
    use samsung_tv_remote::server::development::DevelopmentProfile;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let base = tempfile::tempdir().unwrap();
    let base = base.path().canonicalize().unwrap();
    let (_profile, _owner) = DevelopmentProfile::open(&base).unwrap();
    let location = socket::Location::for_profile(&base).unwrap();
    let unix = location.bind().await.unwrap();
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let endpoint = format!("http://127.0.0.1:{port}");
    let mut context = context();
    context.endpoint = endpoint.clone();
    context.profile = location.profile.clone();
    let gate = Arc::new(AccessGate::new(port));
    let bootstrap =
        Arc::new(Bootstrap::new(gate.clone(), Arc::new(Secrets), peer(), context.clone()).unwrap());
    let exchange = tokio::spawn(async move {
        let (mut stream, _) = unix.accept().await.unwrap();
        bootstrap.exchange(&mut stream).await
    });
    let occupant = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            assert!(request.len() < 16384);
            request.push(stream.read_u8().await.unwrap());
        }
        let response_proof = if valid {
            let mut headers = HeaderMap::new();
            for line in std::str::from_utf8(&request).unwrap().lines().skip(1) {
                if let Some((name, value)) = line.split_once(':') {
                    headers.insert(
                        hyper::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                        value.trim().parse().unwrap(),
                    );
                }
            }
            gate.authorize(
                "127.0.0.1:1".parse().unwrap(),
                &"/v1/ready".parse().unwrap(),
                &headers,
            )
            .unwrap()
            .readiness_proof
            .unwrap()
        } else {
            hex(&[0; 32])
        };
        let body = serde_json::json!({"bootstrap_proof":response_proof,"server_epoch":context.epoch,"build":context.build,"api_version":"v1","serving":serving}).to_string();
        let response = format!(
            "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8(request).unwrap()
    });
    let result = authenticate(
        &location.path,
        &location.profile,
        &endpoint,
        Role::Gui,
        None,
        peer(),
    )
    .await;
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("untrusted/nonserving listener must not release credentials"),
    };
    let captured = occupant.await.unwrap();
    assert!(captured.contains("/v1/ready"));
    assert!(captured
        .to_ascii_lowercase()
        .contains("authorization: bearer "));
    assert!(!captured.contains(&hex(&[1; 32])));
    assert!(!captured.contains(&hex(&[2; 32])));
    assert!(exchange.await.unwrap().is_err());
    std::fs::remove_file(&location.path).unwrap();
    std::fs::remove_dir(location.path.parent().unwrap()).unwrap();
    error
}

#[tokio::test]
async fn occupied_http_listener_never_receives_a_reusable_credential() {
    assert_eq!(
        observe_listener(false, true, 200).await,
        BootstrapError::UntrustedListener
    );
}
#[tokio::test]
async fn proved_nonserving_listener_is_transient_but_a_false_proof_remains_untrusted() {
    assert_eq!(
        observe_listener(false, true, 401).await,
        BootstrapError::Expired
    );
    for status in [429, 503] {
        assert_eq!(
            observe_listener(false, true, status).await,
            BootstrapError::Unavailable
        );
    }
    assert_eq!(
        observe_listener(true, false, 200).await,
        BootstrapError::Unavailable
    );
    assert_eq!(
        observe_listener(false, false, 200).await,
        BootstrapError::UntrustedListener
    );
}

#[tokio::test]
async fn listener_proof_binds_every_context_field_and_never_accepts_a_changed_proof() {
    let (server, gate) = server(peer(), Arc::new(Secrets));
    let (mut client, challenge, task) = challenge(server, hex(&random().unwrap())).await;
    let original = proof(&challenge).unwrap();
    let value = serde_json::to_value(&challenge).unwrap();
    for pointer in [
        "/version",
        "/role",
        "/nonce",
        "/challenge",
        "/context/profile",
        "/context/endpoint",
        "/context/build",
        "/context/api",
        "/context/epoch",
    ] {
        let mut changed = value.clone();
        *changed.pointer_mut(pointer).unwrap() = match pointer {
            "/version" => serde_json::json!(2),
            "/role" => serde_json::json!("mcp"),
            _ => serde_json::json!("changed"),
        };
        let changed: Challenge = serde_json::from_value(changed).unwrap();
        assert_ne!(
            proof(&changed).unwrap(),
            original,
            "unbound field: {pointer}"
        );
    }
    probe(&gate, &challenge, "/v1/ready").unwrap();
    write_frame(
        &mut client,
        &ClientFrame::Redeem(Redeem {
            challenge: challenge.challenge.clone(),
            proof: hex(&[0; 32]),
        }),
    )
    .await
    .unwrap();
    assert_eq!(task.await.unwrap(), Err(BootstrapError::Denied));
}

struct MutableSecrets(std::sync::Mutex<[u8; 32]>);
impl ApiCredentials for MutableSecrets {
    fn load(&self, role: Role) -> Result<[u8; 32], BootstrapError> {
        Ok(if role == Role::Gui {
            *self.0.lock().unwrap()
        } else {
            [2; 32]
        })
    }
}
#[tokio::test]
async fn rotation_during_a_challenge_requires_a_fresh_exchange_while_revocation_stays_denied() {
    for consumed in [false, true] {
        let credentials = Arc::new(MutableSecrets(std::sync::Mutex::new([1; 32])));
        let (server, gate) = server(peer(), credentials.clone());
        let (mut client, old, task) = challenge(server.clone(), hex(&random().unwrap())).await;
        if consumed {
            probe(&gate, &old, "/v1/ready").unwrap();
        }
        *credentials.0.lock().unwrap() = [9; 32];
        server.refresh(Role::Gui).unwrap();
        if consumed {
            write_frame(
                &mut client,
                &ClientFrame::Redeem(Redeem {
                    challenge: old.challenge.clone(),
                    proof: proof(&old).unwrap(),
                }),
            )
            .await
            .unwrap();
            assert_eq!(task.await.unwrap(), Err(BootstrapError::Expired));
        } else {
            assert!(probe(&gate, &old, "/v1/ready").is_err());
            drop(client);
            assert!(task.await.unwrap().is_err());
        }
        let (mut client, fresh, task) = challenge(server.clone(), hex(&random().unwrap())).await;
        probe(&gate, &fresh, "/v1/ready").unwrap();
        write_frame(
            &mut client,
            &ClientFrame::Redeem(Redeem {
                challenge: fresh.challenge.clone(),
                proof: proof(&fresh).unwrap(),
            }),
        )
        .await
        .unwrap();
        let ServerFrame::Credentials(returned) = read_frame(&mut client, Duration::from_secs(2))
            .await
            .unwrap()
        else {
            panic!("fresh credential")
        };
        assert_eq!(returned.bearer, hex(&[9; 32]));
        task.await.unwrap().unwrap();
        gate.revoke(ClientId(1));
        let (mut client, mut socket) = UnixStream::pair().unwrap();
        let task = tokio::spawn(async move { server.exchange(&mut socket).await });
        write_frame(&mut client, &hello(hex(&random().unwrap())))
            .await
            .unwrap();
        assert_eq!(task.await.unwrap(), Err(BootstrapError::Denied));
    }
}

struct Unlockable {
    locked: std::sync::atomic::AtomicBool,
}
impl ApiCredentials for Unlockable {
    fn load(&self, role: Role) -> Result<[u8; 32], BootstrapError> {
        if self.locked.load(std::sync::atomic::Ordering::SeqCst) {
            Err(BootstrapError::InteractionRequired)
        } else {
            Secrets.load(role)
        }
    }
}
#[tokio::test]
async fn initially_locked_role_enrolls_after_unlock_without_rotating_or_regranting_revocation() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let store = Arc::new(Unlockable {
        locked: AtomicBool::new(true),
    });
    let (server, gate) = server(peer(), store.clone());
    let (mut client, mut socket) = UnixStream::pair().unwrap();
    let blocked = server.clone();
    let task = tokio::spawn(async move { blocked.exchange(&mut socket).await });
    write_frame(&mut client, &hello(hex(&random().unwrap())))
        .await
        .unwrap();
    assert_eq!(
        task.await.unwrap(),
        Err(BootstrapError::InteractionRequired)
    );
    assert!(!gate.is_current(ClientId(1), &[1; 32]));
    store.locked.store(false, Ordering::SeqCst);
    let (mut client, challenge, task) = challenge(server.clone(), hex(&random().unwrap())).await;
    probe(&gate, &challenge, "/v1/ready").unwrap();
    write_frame(
        &mut client,
        &ClientFrame::Redeem(Redeem {
            challenge: challenge.challenge.clone(),
            proof: proof(&challenge).unwrap(),
        }),
    )
    .await
    .unwrap();
    assert!(matches!(
        read_frame(&mut client, Duration::from_secs(2))
            .await
            .unwrap(),
        ServerFrame::Credentials(_)
    ));
    task.await.unwrap().unwrap();
    assert!(gate.is_current(ClientId(1), &[1; 32]));
    gate.revoke(ClientId(1));
    let (mut client, mut socket) = UnixStream::pair().unwrap();
    let task = tokio::spawn(async move { server.exchange(&mut socket).await });
    write_frame(&mut client, &hello(hex(&random().unwrap())))
        .await
        .unwrap();
    assert_eq!(task.await.unwrap(), Err(BootstrapError::Denied));
    assert!(!gate.is_current(ClientId(1), &[1; 32]));
}
#[tokio::test]
async fn revoking_a_role_while_its_initial_store_is_blocked_prevents_later_enrollment() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let store = Arc::new(Unlockable {
        locked: AtomicBool::new(true),
    });
    let (server, gate) = server(peer(), store.clone());
    gate.revoke(ClientId(1));
    store.locked.store(false, Ordering::SeqCst);
    let (mut client, mut socket) = UnixStream::pair().unwrap();
    let task = tokio::spawn(async move { server.exchange(&mut socket).await });
    write_frame(&mut client, &hello(hex(&random().unwrap())))
        .await
        .unwrap();
    assert_eq!(task.await.unwrap(), Err(BootstrapError::Denied));
    assert!(!gate.is_current(ClientId(1), &[1; 32]));
}
