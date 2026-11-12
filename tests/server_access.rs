use hyper::http::{HeaderMap, HeaderValue, Uri};
use samsung_tv_remote::application::control_service::ClientId;
use samsung_tv_remote::server::access::{AccessError, AccessGate, Capability};

fn headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("host", HeaderValue::from_static("127.0.0.1:54321"));
    h.insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {}", "01".repeat(32))).unwrap(),
    );
    h
}
fn authorize(gate: &AccessGate, h: &HeaderMap) -> Result<ClientId, AccessError> {
    gate.authorize(
        "127.0.0.1:12345".parse().unwrap(),
        &Uri::from_static("/v1/state"),
        h,
    )
    .map(|c| c.identity)
}

#[tokio::test]
async fn rotation_revokes_old_authority_and_streams_preserving_identity() {
    let gate = AccessGate::new(54321);
    gate.rotate(ClientId(1), Capability::Manage, &[1; 32])
        .unwrap();
    gate.rotate(ClientId(2), Capability::Control, &[2; 32])
        .unwrap();
    assert_eq!(
        gate.rotate(ClientId(3), Capability::Control, &[3; 32]),
        Err(AccessError::Forbidden)
    );
    assert_eq!(
        gate.rotate(ClientId(2), Capability::Control, &[1; 32]),
        Err(AccessError::Forbidden)
    );
    let mut client = gate
        .authorize(
            "127.0.0.1:1".parse().unwrap(),
            &Uri::from_static("/v1/events"),
            &headers(),
        )
        .unwrap();
    assert!(client.require_management().is_ok());
    gate.rotate(ClientId(1), Capability::Manage, &[3; 32])
        .unwrap();
    client.revoked.changed().await.unwrap();
    assert!(*client.revoked.borrow());
    assert!(!client.admission.is_active());
    assert_eq!(
        authorize(&gate, &headers()),
        Err(AccessError::Unauthenticated)
    );
    let mut h = headers();
    h.insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {}", "03".repeat(32))).unwrap(),
    );
    assert_eq!(authorize(&gate, &h), Ok(ClientId(1)));
    h.insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {}", "02".repeat(32))).unwrap(),
    );
    assert_eq!(
        gate.authorize(
            "127.0.0.1:1".parse().unwrap(),
            &Uri::from_static("/v1/server/stop"),
            &h
        )
        .unwrap()
        .require_management(),
        Err(AccessError::Forbidden)
    );
    gate.revoke(ClientId(2));
    assert_eq!(authorize(&gate, &h), Err(AccessError::Unauthenticated));
}

#[test]
fn every_path_uses_literal_authority_origin_and_peer_policy() {
    let gate = AccessGate::new(54321);
    gate.rotate(ClientId(1), Capability::Manage, &[1; 32])
        .unwrap();
    for path in ["/v1/ready", "/v1/state", "/v1/events", "/missing", "/mcp"] {
        let uri: Uri = path.parse().unwrap();
        assert!(gate
            .authorize("127.0.0.1:1".parse().unwrap(), &uri, &headers())
            .is_ok());
        for peer in ["[::1]:1", "192.0.2.1:1", "127.0.0.2:1"] {
            assert!(matches!(
                gate.authorize(peer.parse().unwrap(), &uri, &headers()),
                Err(AccessError::Forbidden)
            ));
        }
    }
    for (name, value) in [
        ("host", "localhost:54321"),
        ("host", "127.0.0.1:54322"),
        ("origin", "null"),
        ("origin", "http://localhost:54321"),
        ("origin", "http://127.0.0.1:54322"),
        ("forwarded", "host=127.0.0.1:54321"),
        ("x-forwarded-host", "127.0.0.1:54321"),
    ] {
        let mut h = headers();
        h.insert(name, value.parse().unwrap());
        assert_eq!(authorize(&gate, &h), Err(AccessError::Forbidden));
    }
    for name in ["host", "origin", "authorization"] {
        let mut h = headers();
        h.append(name, HeaderValue::from_static("duplicate"));
        h.append(name, HeaderValue::from_static("duplicate"));
        assert_eq!(authorize(&gate, &h), Err(AccessError::Forbidden));
    }
    let mut h = headers();
    h.insert("origin", HeaderValue::from_static("http://127.0.0.1:54321"));
    assert_eq!(authorize(&gate, &h), Ok(ClientId(1)));
    assert!(matches!(
        gate.authorize(
            "127.0.0.1:1".parse().unwrap(),
            &"http://127.0.0.1:54321/v1/state".parse().unwrap(),
            &h
        ),
        Err(AccessError::Forbidden)
    ));
}

#[tokio::test(start_paused = true)]
async fn invalid_credentials_have_a_bounded_attempt_window() {
    let gate = AccessGate::new(54321);
    let mut h = headers();
    h.remove("authorization");
    for _ in 0..64 {
        assert_eq!(authorize(&gate, &h), Err(AccessError::Unauthenticated));
    }
    assert_eq!(authorize(&gate, &h), Err(AccessError::Busy));
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    assert_eq!(authorize(&gate, &h), Err(AccessError::Unauthenticated));
}
