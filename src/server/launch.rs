//! Detached start/attach and explicit GUI lifecycle intents for M8 fake profiles.
use super::{
    bootstrap::{
        socket::{uid, Location},
        BootstrapError, DevelopmentPeer, Role,
    },
    development::DevelopmentProfile,
    lifecycle::{
        endpoint::{build_identity, EndpointError, Manifest},
        files::{FileError, OwnershipLock, RuntimeDirectory},
        spawn,
        stop::{FileStopStore, StopStore},
    },
};
use crate::control_client::{
    bootstrap::authenticate,
    http::{ClientError, HttpClient},
};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{timeout, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchError {
    InvalidLockDescriptor,
    InvalidListenerDescriptor,
    InvalidDescriptorFlags,
    InheritedEndpoint,
    InheritedBuild,
    File(FileError),
    Endpoint(EndpointError),
    Bootstrap(BootstrapError),
    Client(ClientError),
    Stopped,
    NotRunning,
    Busy,
    Unavailable,
    UnsafeMetadata,
    Denied,
    UncertainStop,
    StopPersistenceFailed,
}
impl LaunchError {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidLockDescriptor => "invalid_lock_descriptor",
            Self::InvalidListenerDescriptor => "invalid_listener_descriptor",
            Self::InvalidDescriptorFlags => "invalid_descriptor_flags",
            Self::Endpoint(EndpointError::InvalidLockDescriptor) => "invalid_lock_descriptor",
            Self::Endpoint(EndpointError::InvalidListenerDescriptor) => {
                "invalid_listener_descriptor"
            }
            Self::Endpoint(EndpointError::InvalidDescriptorFlags) => "invalid_descriptor_flags",
            Self::InheritedEndpoint => "inherited_endpoint_mismatch",
            Self::InheritedBuild => "inherited_build_mismatch",
            Self::Stopped => "server_stopped",
            Self::NotRunning => "server_not_running",
            Self::Busy => "server_busy",
            Self::Unavailable => "server_unavailable",
            Self::UnsafeMetadata => "unsafe_metadata",
            Self::Denied => "forbidden",
            Self::UncertainStop => "stop_outcome_unknown",
            Self::StopPersistenceFailed => "stop_persistence_failed",
            Self::File(FileError::Locked) => "profile_owned",
            Self::File(_) => "unsafe_profile",
            Self::Endpoint(EndpointError::InheritedEndpoint) => "inherited_endpoint_mismatch",
            Self::Endpoint(EndpointError::InheritedBuild) => "inherited_build_mismatch",
            Self::Endpoint(EndpointError::UnsafeInheritance) => "unsafe_inheritance",
            Self::Endpoint(EndpointError::RejectedProfile) => "child_profile_rejected",
            Self::Endpoint(EndpointError::Collision) => "endpoint_collision",
            Self::Endpoint(_) => "endpoint_unavailable",
            Self::Bootstrap(BootstrapError::IncompatibleBuild) => "incompatible_build",
            Self::Bootstrap(BootstrapError::Denied) => "bootstrap_denied",
            Self::Bootstrap(BootstrapError::InteractionRequired) => "interaction_required",
            Self::Bootstrap(BootstrapError::CredentialUnavailable) => "credential_unavailable",
            Self::Bootstrap(BootstrapError::UnsafeMetadata) => "unsafe_metadata",
            Self::Bootstrap(BootstrapError::UntrustedListener) => "untrusted_listener",
            Self::Bootstrap(_) => "bootstrap_unavailable",
            Self::Client(_) => "client_unavailable",
        }
    }
}
fn directory(base: &Path) -> Result<Arc<RuntimeDirectory>, LaunchError> {
    if !base.is_absolute() {
        return Err(LaunchError::UnsafeMetadata);
    }
    DevelopmentProfile::directory(base).map_err(LaunchError::File)
}
fn stopped(directory: &Arc<RuntimeDirectory>) -> Result<bool, LaunchError> {
    Ok(FileStopStore(directory.clone())
        .read()
        .map_err(LaunchError::File)?
        .is_some())
}
pub async fn attach(
    base: &Path,
    role: Role,
    build: Option<&str>,
) -> Result<HttpClient, LaunchError> {
    let directory = directory(base)?;
    if stopped(&directory)? {
        return Err(LaunchError::Stopped);
    }
    let manifest = Manifest::read(&directory)
        .map_err(LaunchError::File)?
        .ok_or(LaunchError::NotRunning)?;
    let location = Location::for_profile(base).map_err(LaunchError::Bootstrap)?;
    authenticate(
        &location.path,
        &location.profile,
        &manifest.url,
        role,
        build,
        Arc::new(DevelopmentPeer { uid: uid() }),
    )
    .await
    .map_err(LaunchError::Bootstrap)
}
fn transient(error: LaunchError) -> bool {
    matches!(
        error,
        LaunchError::NotRunning
            | LaunchError::Bootstrap(BootstrapError::Unavailable | BootstrapError::Expired)
    )
}
async fn wait_for_attach(base: &Path, role: Role, build: &str) -> Result<HttpClient, LaunchError> {
    timeout(Duration::from_secs(10), async {
        loop {
            match attach(base, role, Some(build)).await {
                Ok(client) => return Ok(client),
                Err(error) if transient(error) => {
                    tokio::time::sleep(Duration::from_millis(25)).await
                }
                Err(error) => return Err(error),
            }
        }
    })
    .await
    .map_err(|_| LaunchError::Unavailable)?
}
pub async fn start(executable: &Path, base: &Path, role: Role) -> Result<HttpClient, LaunchError> {
    let build = build_identity(executable).map_err(LaunchError::Endpoint)?;
    start_with_build(executable, base, role, &build).await
}
pub async fn start_with_build(
    executable: &Path,
    base: &Path,
    role: Role,
    build: &str,
) -> Result<HttpClient, LaunchError> {
    match attach(base, role, Some(build)).await {
        Ok(client) => return Ok(client),
        Err(error) if transient(error) => (),
        Err(error) => return Err(error),
    }
    let directory = directory(base)?;
    let ownership = match directory.lock() {
        Ok(lock) => lock,
        Err(FileError::Locked) => return wait_for_attach(base, role, build).await,
        Err(error) => return Err(LaunchError::File(error)),
    };
    if stopped(&directory)? {
        return Err(LaunchError::Stopped);
    }
    let (_, listener) =
        Manifest::bind(&directory, &ownership, build).map_err(LaunchError::Endpoint)?;
    let report =
        spawn::detached(executable, base, &ownership, &listener).map_err(LaunchError::Endpoint)?;
    report.ready().await.map_err(LaunchError::Endpoint)?;
    drop(listener);
    drop(ownership);
    wait_for_attach(base, role, build).await
}
async fn wait_for_lock(directory: &RuntimeDirectory) -> Result<OwnershipLock, LaunchError> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match directory.lock() {
            Ok(lock) => return Ok(lock),
            Err(FileError::Locked) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(25)).await
            }
            Err(FileError::Locked) => return Err(LaunchError::Busy),
            Err(error) => return Err(LaunchError::File(error)),
        }
    }
}
pub async fn resume(executable: &Path, base: &Path, role: Role) -> Result<HttpClient, LaunchError> {
    if role != Role::Gui {
        return Err(LaunchError::Denied);
    }
    let directory = directory(base)?;
    let ownership = wait_for_lock(&directory).await?;
    FileStopStore(directory)
        .clear()
        .map_err(LaunchError::File)?;
    drop(ownership);
    start(executable, base, role).await
}
/// No effect retry: one Stop intent, then inspect its marker if its HTTP reply
/// was lost. Observing a marker blocks auto-start; only an acknowledged HTTP
/// result claims the marker write was durably acknowledged by the owner.
pub async fn stop(base: &Path, client: &HttpClient) -> Result<Value, LaunchError> {
    let (status, ready) = client
        .call("GET", "/v1/ready", None)
        .await
        .map_err(LaunchError::Client)?;
    if status != 200 || ready["management_version"] != "v1" {
        return Err(LaunchError::Denied);
    }
    let epoch = ready["server_epoch"]
        .as_str()
        .ok_or(LaunchError::UnsafeMetadata)?;
    let key = uuid::Uuid::new_v4().to_string();
    let result = client
        .call(
            "POST",
            "/v1/server/stop",
            Some(&json!({"server_epoch":epoch,"operation_key":key})),
        )
        .await;
    if let Ok((status, value)) = &result {
        if *status == 200 && value["durable_stop"] == true {
            if value["server_epoch"] != epoch || value["operation_key"] != key {
                return Err(LaunchError::UnsafeMetadata);
            }
            return Ok(value.clone());
        }
        // A definite failure is not a lost acknowledgment. In particular a
        // failed directory sync can leave an observable marker while the owner
        // remains fenced/blocked; do not turn that known failure into success.
        return Err(match *status {
            401 | 403 => LaunchError::Denied,
            429 => LaunchError::Busy,
            503 if value["error"]["code"] == "stop_persistence_failed" => {
                LaunchError::StopPersistenceFailed
            }
            200 => LaunchError::UnsafeMetadata,
            _ => LaunchError::Unavailable,
        });
    }
    let directory = directory(base)?;
    if let Some(marker) = FileStopStore(directory).read().map_err(LaunchError::File)? {
        if marker.server_epoch == epoch && marker.operation_key == key {
            return Ok(
                json!({"server_epoch":epoch,"operation_key":key,"marker_observed":true,"durable_acknowledgment":false}),
            );
        }
    }
    Err(LaunchError::UncertainStop)
}
pub async fn restart(
    executable: &Path,
    base: &Path,
    role: Role,
) -> Result<HttpClient, LaunchError> {
    if role != Role::Gui {
        return Err(LaunchError::Denied);
    }
    let client = attach(base, role, None).await?;
    let intent = stop(base, &client).await?;
    let directory = directory(base)?;
    let ownership = wait_for_lock(&directory).await?;
    let store = FileStopStore(directory);
    let marker = store
        .read()
        .map_err(LaunchError::File)?
        .ok_or(LaunchError::UncertainStop)?;
    if marker.operation_key != intent["operation_key"]
        || marker.server_epoch != intent["server_epoch"]
    {
        return Err(LaunchError::Stopped);
    }
    store.clear().map_err(LaunchError::File)?;
    drop(ownership);
    start(executable, base, role).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_client::http::Endpoint;
    use crate::server::lifecycle::stop::StopMarker;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn explicit_stop_failure_and_mismatched_ack_are_not_lost_ack_success() {
        for mode in ["failed", "mismatched_key", "mismatched_epoch", "lost"] {
            let base = tempfile::tempdir().unwrap();
            let base = base.path().canonicalize().unwrap();
            let directory = DevelopmentProfile::directory(&base).unwrap();
            let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                .await
                .unwrap();
            let port = listener.local_addr().unwrap().port();
            let epoch = uuid::Uuid::new_v4().to_string();
            let server_epoch = epoch.clone();
            let task = tokio::spawn(async move {
                for stopping in [false, true] {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut header = Vec::new();
                    while !header.ends_with(b"\r\n\r\n") {
                        header.push(socket.read_u8().await.unwrap());
                    }
                    let text = std::str::from_utf8(&header).unwrap();
                    assert!(text.to_ascii_lowercase().contains("authorization: bearer "));
                    let length = text
                        .lines()
                        .filter_map(|line| line.split_once(':'))
                        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                        .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                        .unwrap_or(0);
                    let mut body = vec![0; length];
                    socket.read_exact(&mut body).await.unwrap();
                    let (status, value) = if stopping {
                        let request: Value = serde_json::from_slice(&body).unwrap();
                        let key = request["operation_key"].as_str().unwrap().to_owned();
                        FileStopStore(directory.clone())
                            .write(&StopMarker {
                                version: 1,
                                profile: "m8-fake-v1".into(),
                                server_epoch: server_epoch.clone(),
                                operation_key: key.clone(),
                                identity: "gui".into(),
                            })
                            .unwrap();
                        if mode == "lost" {
                            drop(socket);
                            break;
                        }
                        if mode == "failed" {
                            (503, json!({"error":{"code":"stop_persistence_failed"}}))
                        } else {
                            (
                                200,
                                json!({"durable_stop":true,"server_epoch":if mode=="mismatched_epoch" {uuid::Uuid::new_v4().to_string()}else{server_epoch.clone()},"operation_key":if mode=="mismatched_key" {uuid::Uuid::new_v4().to_string()}else{key}}),
                            )
                        }
                    } else {
                        (
                            200,
                            json!({"server_epoch":server_epoch,"management_version":"v1"}),
                        )
                    };
                    let body = value.to_string();
                    socket.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
                }
            });
            let client = HttpClient {
                endpoint: Endpoint::parse(&format!("http://127.0.0.1:{port}")).unwrap(),
                bearer: crate::server::bootstrap::hex(&[1; 32]),
            };
            let result = stop(&base, &client).await;
            match mode {
                "failed" => assert!(matches!(result, Err(LaunchError::StopPersistenceFailed))),
                "mismatched_key" | "mismatched_epoch" => {
                    assert!(matches!(result, Err(LaunchError::UnsafeMetadata)))
                }
                _ => {
                    let result = result.unwrap();
                    assert_eq!(result["marker_observed"], true);
                    assert_eq!(result["durable_acknowledgment"], false);
                    assert_eq!(result["server_epoch"], epoch);
                }
            }
            task.await.unwrap();
        }
    }
}
