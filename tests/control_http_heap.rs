//! Isolated allocator workload: full disk profile, HTTP JSON and eight live SSE
//! clients. Counts Rust heap on both sides; excludes kernel buffers/macOS RSS.
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{body::Incoming, Request, Response};
use hyper_util::rt::TokioIo;
use samsung_tv_remote::application::control_service::ClientId;
use samsung_tv_remote::application::runtime::{Runtime, RuntimeLimits};
use samsung_tv_remote::server::{
    access::{AccessGate, AuthorizedClient, Capability},
    control_http::{
        response::ErrorCode,
        transport::{HttpAdapter, StopFuture, StopPort},
    },
    development::DevelopmentProfile,
};
use serde_json::{json, Value};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
struct Counted;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
// SAFETY: System receives each original pointer/layout. Counters allocate no
// memory. Default realloc/alloc_zeroed delegate through these counted methods.
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(pointer, layout) };
    }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;
struct Stop;
impl StopPort for Stop {
    fn stop(
        &self,
        _: AuthorizedClient,
        _: samsung_tv_remote::application::control_service::Epoch,
        _: String,
    ) -> StopFuture<'_> {
        Box::pin(async {
            Err(ErrorCode {
                status: 403,
                code: "forbidden",
            })
        })
    }
}
async fn get(port: u16, identity: u8, path: &str) -> Response<Incoming> {
    let stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let request = Request::builder()
        .uri(path)
        .header("host", format!("127.0.0.1:{port}"))
        .header(
            "authorization",
            format!("Bearer {}", format!("{identity:02x}").repeat(32)),
        )
        .body(Full::new(Bytes::new()))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), sender.send_request(request))
        .await
        .unwrap()
        .unwrap()
}
#[tokio::test]
async fn full_profile_http_and_eight_subscribers_have_a_measured_heap_bound() {
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let base = tempfile::tempdir().unwrap();
    let base_path = base.path().canonicalize().unwrap();
    let (profile, _ownership) = DevelopmentProfile::open(&base_path).unwrap();
    let host = [
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61),
    ]
    .join(".");
    let devices: Vec<_> = (0..1024)
        .map(|n| {
            json!({"id":format!("dev_{n}"),"label":"\u{0001}".repeat(256),"host":host,
        "wake":{"wired":"02:00:00:00:00:01","wifi":"02:00:00:00:00:02","active":"wired"}})
        })
        .collect();
    let disk = serde_json::to_vec(
        &json!({"namespace":"m8-fake-v1","data":{"version":2,"selected":null,"devices":devices}}),
    )
    .unwrap();
    let disk_bytes = disk.len();
    profile
        .directory
        .child("data")
        .unwrap()
        .replace("devices.json", &disk)
        .unwrap();
    drop(disk);
    let runtime = Runtime::start(profile.services(), RuntimeLimits::default());
    let mut subscription = runtime.client(ClientId(1)).subscribe().await.unwrap();
    while subscription.snapshot.startup
        == samsung_tv_remote::application::control_service::StartupState::Restoring
    {
        subscription.snapshot = subscription.events.recv().await.unwrap().snapshot;
    }
    assert_eq!(subscription.snapshot.devices.len(), 1024);
    drop(subscription);
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let gate = Arc::new(AccessGate::new(port));
    gate.rotate(ClientId(1), Capability::Manage, &[1; 32])
        .unwrap();
    gate.rotate(ClientId(2), Capability::Control, &[2; 32])
        .unwrap();
    let adapter = Arc::new(HttpAdapter::new(
        gate,
        [
            (ClientId(1), runtime.client(ClientId(1))),
            (ClientId(2), runtime.client(ClientId(2))),
        ],
        Arc::new(Stop),
        "heap-fixture".into(),
    ));
    let (shutdown, receiver) = watch::channel(false);
    let task = tokio::spawn(adapter.serve(listener, receiver));
    let start = Instant::now();
    let response = get(port, 1, "/v1/state").await;
    assert_eq!(response.status(), 200);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json_bytes = bytes.len();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["devices"]
            .as_array()
            .unwrap()
            .len(),
        1024
    );
    drop(bytes);
    // Hold every initial SSE frame and every reachable query response before
    // consuming bodies, so slow output allocations overlap instead of measuring
    // only one completed client at a time.
    let mut streams = Vec::new();
    let mut queries = Vec::new();
    for identity in [1, 2] {
        for _ in 0..4 {
            let response = get(port, identity, "/v1/events").await;
            assert_eq!(response.status(), 200);
            streams.push(response.into_body());
        }
        for _ in 0..4 {
            let response = get(port, identity, "/v1/state").await;
            assert_eq!(response.status(), 200);
            queries.push(response.into_body());
        }
    }
    assert_eq!(get(port, 1, "/v1/events").await.status(), 429);
    let mut sse_bytes = 0;
    for body in &mut streams {
        let mut bytes = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !bytes.ends_with(b"\n\n") {
                let data = body.frame().await.unwrap().unwrap().into_data().unwrap();
                bytes.extend_from_slice(&data);
                assert!(bytes.len() < 4 * 1024 * 1024 + 128);
            }
        })
        .await
        .unwrap();
        sse_bytes = bytes.len();
        assert!(bytes.starts_with(b"event: snapshot\n"));
    }
    for body in queries {
        let bytes = body.collect().await.unwrap().to_bytes();
        assert_eq!(bytes.len(), json_bytes);
    }
    assert_eq!(get(port, 1, "/v1/ready").await.status(), 200);
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
    assert!(peak < 128 * 1024 * 1024, "unexpected heap growth: {peak}");
    eprintln!("M8 HTTP heap: peak={peak} bytes; disk={disk_bytes}; JSON={json_bytes}; SSE={sse_bytes}; eight SSE + eight queries={} ms",start.elapsed().as_millis());
    shutdown.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    runtime.drain().await.settled().await.unwrap();
    assert_eq!(profile.effects.credential_reads.load(Ordering::SeqCst), 0);
    assert_eq!(profile.effects.probes.load(Ordering::SeqCst), 0);
    assert_eq!(profile.effects.connections.load(Ordering::SeqCst), 0);
    drop(streams);
}
