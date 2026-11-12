//! Run as its own test binary so the allocator measurement has one workload.
//! Measurements describe application heap allocations, not macOS RSS or TV latency.
mod common;
use samsung_tv_remote::application::certificate_trust::CertificatePin;
use samsung_tv_remote::application::control_service::*;
use samsung_tv_remote::application::credential_store::PairingToken;
use samsung_tv_remote::application::device_repository::SavedDevice;
use samsung_tv_remote::application::runtime::{Runtime, RuntimeLimits};
use samsung_tv_remote::application::tv_address::TvHost;
use samsung_tv_remote::application::tv_control_coordinator::AppServices;
use samsung_tv_remote::application::tv_discovery::{DiscoveryError, TvDiscovery};
use samsung_tv_remote::application::tv_session::*;
use samsung_tv_remote::DeviceId;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Counted;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
// SAFETY: every allocation/deallocation delegates the identical pointer/layout
// to System. The counters neither allocate nor alter allocator ownership.
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
struct Network;
fn host() -> TvHost {
    TvHost::parse(
        &[
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61),
        ]
        .join("."),
    )
    .unwrap()
}
impl TvDiscovery for Network {
    fn discover(&self) -> SessionFuture<'_, Result<Vec<TvHost>, DiscoveryError>> {
        Box::pin(async { Ok((0..32).map(|_| host()).collect()) })
    }
}
impl TvGateway for Network {
    fn probe(&self, _: TvHost) -> SessionFuture<'static, Result<ProbeObservation, TvSessionError>> {
        Box::pin(async {
            Ok(ProbeObservation {
                pin: CertificatePin::from_der(b"fixture"),
                name: Some("n".repeat(256)),
                model: Some("m".repeat(256)),
            })
        })
    }
    fn connect(
        &self,
        _: TvHost,
        _: CertificatePin,
        _: Option<PairingToken>,
    ) -> SessionFuture<'static, Result<Box<dyn TvConnection>, TvSessionError>> {
        panic!("measurement must never connect")
    }
}
async fn terminal(service: &ControlService, request: Request) -> Receipt {
    let mut subscription = service.subscribe().await.unwrap();
    let mut receipt = service.submit(request).await.unwrap();
    while receipt.outcome.is_none() {
        subscription.events.recv().await.unwrap();
        let Lookup::Known(found) = service
            .operation(subscription.snapshot.epoch, receipt.id)
            .await
            .unwrap()
        else {
            panic!("lost receipt")
        };
        receipt = found;
    }
    receipt
}
#[tokio::test]
async fn production_saturation_memory_and_stage_timings() {
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let profile = Arc::new(common::MemorySetup::default());
    *profile.devices.lock().unwrap() = (0..1024)
        .map(|id| SavedDevice {
            id: DeviceId::new(id),
            label: "l".repeat(256),
            host: host(),
            wake: Default::default(),
        })
        .collect();
    *profile.selected.lock().unwrap() = Some(DeviceId::new(0));
    let started = Instant::now();
    let runtime = Runtime::start(
        AppServices {
            setup: Some(profile),
            discovery: Some(Arc::new(Network)),
            gateway: Some(Arc::new(Network)),
            wake: None,
        },
        RuntimeLimits::default(),
    );
    let client = runtime.client(ClientId(1));
    let mut subscription = client.subscribe().await.unwrap();
    while subscription.snapshot.startup == StartupState::Restoring {
        subscription.snapshot = subscription.events.recv().await.unwrap().snapshot;
    }
    let snapshot = subscription.snapshot;
    drop(subscription.events);
    let restore_us = started.elapsed().as_micros();
    let stage = Instant::now();
    for i in 0..32 {
        let service = runtime.client(ClientId(10 + i / 2));
        assert!(matches!(
            terminal(
                &service,
                Request {
                    epoch: snapshot.epoch,
                    key: format!("discover-{i}"),
                    command: Command::Discover
                }
            )
            .await
            .outcome,
            Some(Outcome::Discovered(_))
        ));
    }
    let discovery_us = stage.elapsed().as_micros();
    let stage = Instant::now();
    for i in 0..128 {
        let service = runtime.client(ClientId(100 + i / 8));
        assert!(matches!(
            terminal(
                &service,
                Request {
                    epoch: snapshot.epoch,
                    key: format!("probe-{i}"),
                    command: Command::Probe(host())
                }
            )
            .await
            .outcome,
            Some(Outcome::Probed(_))
        ));
    }
    let probe_us = stage.elapsed().as_micros();
    let started = Instant::now();
    // Fill the remaining records with the largest allowed immutable Pair input
    // and retained policy failures, without consuming a TV/effect slot.
    for index in 160..32746 {
        let service = runtime.client(ClientId(1000 + index / 8192));
        service
            .submit(Request {
                epoch: snapshot.epoch,
                key: format!("{index:0128}"),
                command: Command::Pair {
                    observation: OperationId(0),
                    host: host(),
                    pin: CertificatePin::from_der(b"fixture"),
                    label: "p".repeat(256),
                    replace: None,
                    generation: 0,
                    recovery: None,
                },
            })
            .await
            .unwrap();
    }
    let registry_us = started.elapsed().as_micros();
    let mut subscribers = Vec::new();
    for index in 0..64 {
        subscribers.push(
            runtime
                .client(ClientId(2000 + index / 4))
                .subscribe()
                .await
                .unwrap(),
        );
    }
    // 21 durable configuration changes each publish pending, terminal, and
    // reconciled snapshots. Keep every full-profile version in all 64 queues.
    // One final policy result fills the 64th slot without overflowing it.
    for index in 0..21 {
        let service = runtime.client(ClientId(1003));
        let mut receipt = service
            .submit(Request {
                epoch: snapshot.epoch,
                key: format!("config-{index}"),
                command: Command::SetWake {
                    target: DeviceId::new(0),
                    generation: 0,
                    revision: index,
                    configuration: samsung_tv_remote::application::wake::WakeConfiguration {
                        wired: Some(format!("02:11:22:33:44:{index:02x}").parse().unwrap()),
                        wifi: None,
                        active: None,
                    },
                },
            })
            .await
            .unwrap();
        while receipt.outcome.is_none() {
            tokio::task::yield_now().await;
            let Lookup::Known(updated) =
                service.operation(snapshot.epoch, receipt.id).await.unwrap()
            else {
                panic!("lost config")
            };
            receipt = updated;
        }
        assert!(matches!(receipt.outcome, Some(Outcome::Saved(_))));
    }
    let service = runtime.client(ClientId(1003));
    let request = Request {
        epoch: snapshot.epoch,
        key: "final-policy-result".into(),
        command: Command::Pair {
            observation: OperationId(0),
            host: host(),
            pin: CertificatePin::from_der(b"fixture"),
            label: "p".repeat(256),
            replace: None,
            generation: 0,
            recovery: None,
        },
    };
    let receipt = service.submit(request.clone()).await.unwrap();
    assert!(matches!(
        receipt.outcome,
        Some(Outcome::Rejected(Rejection::InvalidObservation))
    ));
    let id = receipt.id;
    assert_eq!(service.submit(request).await.unwrap().id, id);
    assert_eq!(
        service
            .submit(Request {
                epoch: snapshot.epoch,
                key: "full".into(),
                command: Command::Discover
            })
            .await
            .unwrap_err(),
        ServiceError::RegistryFull
    );
    for subscriber in &subscribers {
        assert_eq!(subscriber.events.len(), 64);
    }
    let live = LIVE.load(Ordering::Relaxed).saturating_sub(baseline);
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
    // A deliberately generous regression ceiling, including runtime, fakes,
    // full profile, all retained data, indices, handles and subscriber queues.
    assert!(peak < 128 * 1024 * 1024, "unexpected heap growth: {peak}");
    let query = Instant::now();
    service.snapshot().await.unwrap();
    let query_us = query.elapsed().as_micros();
    let drain = Instant::now();
    runtime.drain().await.settled().await.unwrap();
    let drain_us = drain.elapsed().as_micros();
    eprintln!("M7 heap live={live} peak={peak} bytes; restore={restore_us}us discovery32={discovery_us}us probe128={probe_us}us registry32586={registry_us}us query={query_us}us drain={drain_us}us");
    assert!(drain.elapsed() < Duration::from_secs(5));
}
