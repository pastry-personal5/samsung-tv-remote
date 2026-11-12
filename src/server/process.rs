//! Same-executable headless composition. Only server mode constructs adapters.
use super::launch::LaunchError;
use crate::application::tv_control_coordinator::AppServices;
use crate::application::{
    control_service::ClientId,
    runtime::{Runtime, RuntimeLimits},
};
use crate::infrastructure::{
    macos::{
        api_credentials::{bootstrap_error, ServerApiCredentials},
        signed_peer::SignedPeer,
    },
    private_files::{Namespace, OwnershipLock, RuntimeDirectory},
};
use crate::server::{
    access::AccessGate,
    bootstrap::{
        socket::Location, ApiCredentials, Bootstrap, Context, DevelopmentPeer, MemoryCredentials,
        PeerVerifier,
    },
    control_http::{transport::HttpAdapter, wire},
    development::DevelopmentProfile,
    lifecycle::{
        endpoint::{build_identity, Manifest},
        stop::{settle_before_release, Lifecycle, NamespacedStopStore, StopStore},
    },
    production::ProductionProfile,
    profile::Profile,
};
use std::fs::File;
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;

pub struct Inherited {
    pub owner: File,
    pub listener: TcpListener,
    pub report: Option<File>,
}
pub async fn run(base: &Path, inherited: Option<Inherited>) -> Result<(), LaunchError> {
    run_profile(
        Profile::development(base).map_err(LaunchError::File)?,
        inherited,
    )
    .await
}
pub async fn run_production(inherited: Option<Inherited>) -> Result<(), LaunchError> {
    run_profile(Profile::Control, inherited).await
}
async fn run_profile(
    profile: Profile,
    mut inherited: Option<Inherited>,
) -> Result<(), LaunchError> {
    let report = Arc::new(std::sync::Mutex::new(
        inherited.as_mut().and_then(|value| value.report.take()),
    ));
    let result = run_inner(&profile, inherited, report.clone()).await;
    if let Err(error) = result {
        report_startup(&report, error.code());
    }
    result
}
fn report_startup(report: &Arc<std::sync::Mutex<Option<File>>>, text: &str) {
    use std::io::Write;
    if let Some(mut file) = report.lock().expect("startup report").take() {
        let _ = file.write_all(format!("{text}\n").as_bytes());
    }
}
async fn run_inner(
    selected: &Profile,
    inherited: Option<Inherited>,
    report: Arc<std::sync::Mutex<Option<File>>>,
) -> Result<(), LaunchError> {
    let namespace = selected.namespace();
    // Both directions use the same signed requirement. Fail before opening a
    // production profile, even when a caller supplies inherited descriptors.
    let verifier: Arc<dyn PeerVerifier> = match namespace {
        Namespace::Development => Arc::new(DevelopmentPeer {
            uid: super::bootstrap::socket::uid(),
        }),
        Namespace::Control => Arc::new(
            SignedPeer::for_current_executable().map_err(|_| LaunchError::SigningRequired)?,
        ),
    };
    let base = selected.base().map_err(LaunchError::File)?;
    let build = build_identity(&std::env::current_exe().map_err(|_| LaunchError::Unavailable)?)
        .map_err(LaunchError::Endpoint)?;
    let (profile, ownership, listener) = match inherited {
        Some(inherited) => {
            let (profile, ownership) = OpenProfile::open(selected, Some(inherited.owner))?;
            let manifest = Manifest::read_for(profile.directory(), namespace)
                .map_err(LaunchError::File)?
                .ok_or(LaunchError::UnsafeMetadata)?;
            if inherited
                .listener
                .local_addr()
                .map_err(|_| LaunchError::UnsafeMetadata)?
                != std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, manifest.port))
            {
                return Err(LaunchError::InheritedEndpoint);
            }
            if manifest.build != build {
                return Err(LaunchError::InheritedBuild);
            }
            // SO_ACCEPTCONN is not a queryable option on all supported macOS
            // kernels. Reassert listen only after validating profile and port.
            use std::os::fd::AsRawFd;
            // SAFETY: live, validated IPv4 stream descriptor owned by this child.
            if unsafe { libc::listen(inherited.listener.as_raw_fd(), 128) } < 0 {
                return Err(LaunchError::InvalidListenerDescriptor);
            }

            inherited
                .listener
                .set_nonblocking(true)
                .map_err(|_| LaunchError::Unavailable)?;
            (profile, ownership, inherited.listener)
        }
        None => {
            let (profile, ownership) = OpenProfile::open(selected, None)?;
            if NamespacedStopStore::new(profile.directory().clone(), namespace)
                .read()
                .map_err(LaunchError::File)?
                .is_some()
            {
                return Err(LaunchError::Stopped);
            }
            let (_, listener) =
                Manifest::bind_for(profile.directory(), &ownership, &build, namespace)
                    .map_err(LaunchError::Endpoint)?;
            (profile, ownership, listener)
        }
    };
    let store = Arc::new(NamespacedStopStore::new(
        profile.directory().clone(),
        namespace,
    ));
    if store.read().map_err(LaunchError::File)?.is_some() {
        return Err(LaunchError::Stopped);
    }
    let location = Location::for_namespace(&base, namespace).map_err(LaunchError::Bootstrap)?;
    let socket = location.bind().await.map_err(LaunchError::Bootstrap)?;
    let listener =
        tokio::net::TcpListener::from_std(listener).map_err(|_| LaunchError::Unavailable)?;
    let port = listener
        .local_addr()
        .map_err(|_| LaunchError::Unavailable)?
        .port();
    let access = Arc::new(AccessGate::new(port));
    let credentials: Arc<dyn ApiCredentials> = match namespace {
        Namespace::Development => {
            Arc::new(MemoryCredentials::random().map_err(LaunchError::Bootstrap)?)
        }
        Namespace::Control => Arc::new(
            ServerApiCredentials::new(profile.directory().clone())
                .map_err(|error| LaunchError::Bootstrap(bootstrap_error(error)))?,
        ),
    };
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| LaunchError::Unavailable)?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|_| LaunchError::Unavailable)?;
    let runtime = Runtime::start(profile.services()?, RuntimeLimits::default());
    let services = [
        (ClientId(1), runtime.client(ClientId(1))),
        (ClientId(2), runtime.client(ClientId(2))),
    ];
    let epoch = match services[0].1.snapshot().await {
        Ok(snapshot) => snapshot.epoch,
        Err(_) => {
            settle_before_release(runtime).await;
            return Err(LaunchError::Unavailable);
        }
    };
    let bootstrap = match Bootstrap::new(
        access.clone(),
        credentials,
        verifier,
        Context {
            profile: location.profile,
            endpoint: format!("http://127.0.0.1:{port}"),
            build: build.clone(),
            api: "v1".into(),
            epoch: wire::epoch(epoch),
        },
    ) {
        Ok(bootstrap) => Arc::new(bootstrap),
        Err(error) => {
            settle_before_release(runtime).await;
            return Err(LaunchError::Bootstrap(error));
        }
    };
    let lifecycle = Lifecycle::start_in_namespace(runtime, store, ownership, namespace)
        .await
        .map_err(|_| LaunchError::Unavailable)?;
    let http = Arc::new(HttpAdapter::new(
        access,
        services,
        lifecycle.client.clone(),
        build,
    ));
    let mut shutdown = lifecycle.http_shutdown.clone();
    let mut http_task = tokio::spawn(http.serve(listener, shutdown.clone()));
    let mut bootstrap_task = tokio::spawn(bootstrap.serve(socket, shutdown.clone()));
    report_startup(&report, "ready");
    let mut http_done = false;
    let mut bootstrap_done = false;
    tokio::select! {biased;
        _=shutdown.changed()=>(),
        _=term.recv()=>(),
        _=interrupt.recv()=>(),
        _=&mut http_task=>{http_done=true;},
        _=&mut bootstrap_task=>{bootstrap_done=true;},
    }
    // Release the watch borrow before awaiting the actor, which may send on
    // that same channel while handling a concurrent shutdown.
    let shutdown_requested = *lifecycle.http_shutdown.borrow();
    if !shutdown_requested
        && lifecycle.request_shutdown().await.is_err()
        && !*lifecycle.http_shutdown.borrow()
    {
        return Err(LaunchError::Unavailable);
    }
    if !http_done {
        let _ = http_task.await;
    }
    if !bootstrap_done {
        let _ = bootstrap_task.await;
    }
    lifecycle
        .settled()
        .await
        .map_err(|_| LaunchError::Unavailable)?;
    if http_done || bootstrap_done {
        Err(LaunchError::Unavailable)
    } else {
        Ok(())
    }
}

// Keep concrete production ports confined to server composition; callers see
// no store/session handles, and fake profile selection cannot choose Control.
enum OpenProfile {
    Development(DevelopmentProfile),
    Control(ProductionProfile),
}
impl OpenProfile {
    fn open(
        profile: &Profile,
        inherited: Option<File>,
    ) -> Result<(Self, OwnershipLock), LaunchError> {
        match profile {
            Profile::Development(base) => {
                let (profile, owner) = match inherited {
                    Some(file) => DevelopmentProfile::inherited(base, file),
                    None => DevelopmentProfile::open(base),
                }
                .map_err(LaunchError::File)?;
                Ok((Self::Development(profile), owner))
            }
            Profile::Control => {
                let (profile, owner) = match inherited {
                    Some(file) => ProductionProfile::inherited(file),
                    None => ProductionProfile::open(),
                }
                .map_err(LaunchError::File)?;
                Ok((Self::Control(profile), owner))
            }
        }
    }
    fn directory(&self) -> &Arc<RuntimeDirectory> {
        match self {
            Self::Development(profile) => &profile.directory,
            Self::Control(profile) => &profile.directory,
        }
    }
    fn services(&self) -> Result<AppServices, LaunchError> {
        match self {
            Self::Development(profile) => Ok(profile.services()),
            Self::Control(profile) => profile
                .services()
                .map_err(|error| LaunchError::Bootstrap(bootstrap_error(error))),
        }
    }
}
