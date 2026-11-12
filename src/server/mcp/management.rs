//! Protected live/offline enrollment management. Profile ownership arbitrates
//! with a server; none of these actions starts a TV runtime or clears Stop.
use super::enrollment::{Enrollment, EnrollmentError, EnrollmentStore, FileEnrollment};
use crate::infrastructure::{
    macos::api_credentials::{bootstrap_error, ServerApiCredentials},
    private_files::{FileError, Namespace, OwnershipLock, RuntimeDirectory},
};
use crate::server::{
    bootstrap::{ApiCredentials, BootstrapError, MemoryCredentials, Role},
    launch::{LaunchError, Launcher},
    lifecycle::endpoint::{build_identity, Manifest},
    profile::Profile,
};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Status,
    Prepare,
    Change {
        generation: u64,
        key: String,
        enabled: bool,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Launch(LaunchError),
    Enrollment(EnrollmentError),
    Unavailable,
    Timeout,
    Remote(&'static str),
}
impl Error {
    pub fn code(self) -> &'static str {
        match self {
            Self::Launch(error) => error.code(),
            Self::Enrollment(error) => match error {
                EnrollmentError::Denied => "forbidden",
                EnrollmentError::StaleGeneration => "stale_enrollment",
                EnrollmentError::KeyConflict => "operation_key_conflict",
                EnrollmentError::UnsafeMetadata => "unsafe_enrollment",
                EnrollmentError::Persistence => "enrollment_persistence_failed",
                EnrollmentError::Credential(_) => "mcp_credentials_blocked",
                EnrollmentError::RevocationPending => "mcp_revocation_pending",
                EnrollmentError::Exhausted => "enrollment_exhausted",
            },
            Self::Unavailable => "mcp_management_unavailable",
            Self::Timeout => "mcp_management_outcome_unknown",
            Self::Remote(code) => code,
        }
    }
}
struct NoCredentials;
impl ApiCredentials for NoCredentials {
    fn load(&self, _: Role) -> Result<[u8; 32], BootstrapError> {
        Err(BootstrapError::Denied)
    }
}
struct Context {
    launcher: Launcher,
    directory: Arc<RuntimeDirectory>,
    profile: Profile,
    build: String,
}

fn context(profile: Profile, executable: PathBuf) -> Result<Context, Error> {
    let launcher = Launcher::new(profile.clone()).map_err(Error::Launch)?;
    let directory = profile
        .directory()
        .map_err(|error| Error::Launch(LaunchError::File(error)))?;
    let build =
        build_identity(&executable).map_err(|error| Error::Launch(LaunchError::Endpoint(error)))?;
    Ok(Context {
        launcher,
        directory,
        profile,
        build,
    })
}
fn remote_error(body: &Value) -> Error {
    Error::Remote(match body["error"]["code"].as_str() {
        Some("stale_epoch") => "stale_epoch",
        Some("stale_enrollment") => "stale_enrollment",
        Some("operation_key_conflict") => "operation_key_conflict",
        Some("forbidden") => "forbidden",
        Some("unsafe_enrollment") => "unsafe_enrollment",
        Some("enrollment_persistence_failed") => "enrollment_persistence_failed",
        Some("mcp_credentials_blocked") => "mcp_credentials_blocked",
        Some("mcp_revocation_pending") => "mcp_revocation_pending",
        _ => "mcp_management_unavailable",
    })
}
fn offline(context: &Context, ownership: OwnershipLock, action: &Action) -> Result<Value, Error> {
    let namespace = context.profile.namespace();
    let store = Arc::new(FileEnrollment::new(context.directory.clone(), namespace));
    // Corruption is never replaced with an empty/default enrollment.
    store.read().map_err(Error::Enrollment)?;
    if *action == Action::Prepare {
        let (_, listener) =
            Manifest::bind_for(&context.directory, &ownership, &context.build, namespace)
                .map_err(|error| Error::Launch(LaunchError::Endpoint(error)))?;
        drop(listener);
    }
    let manifest = Manifest::read_for(&context.directory, namespace)
        .map_err(|_| Error::Launch(LaunchError::UnsafeMetadata))?;
    let credentials: Arc<dyn ApiCredentials> =
        if matches!(action, Action::Change { enabled: true, .. }) {
            if manifest.is_none() {
                return Err(Error::Launch(LaunchError::UnsafeMetadata));
            }
            match namespace {
                Namespace::Development => Arc::new(
                    MemoryCredentials::random()
                        .map_err(|error| Error::Launch(LaunchError::Bootstrap(error)))?,
                ),
                Namespace::Control => Arc::new(
                    ServerApiCredentials::new(context.directory.clone()).map_err(|error| {
                        Error::Launch(LaunchError::Bootstrap(bootstrap_error(error)))
                    })?,
                ),
            }
        } else {
            Arc::new(NoCredentials)
        };
    let enrollment = Enrollment::offline(store, credentials);
    let mut result = match action {
        Action::Change {
            generation,
            key,
            enabled,
        } => enrollment
            .change_offline(&ownership, *generation, key, *enabled)
            .map_err(Error::Enrollment)?
            .summary(),
        _ => enrollment.status_summary().map_err(Error::Enrollment)?,
    };
    result["url"] = json!(manifest.map(|manifest| manifest.mcp_url));
    result["server_running"] = json!(false);
    Ok(result)
}

pub async fn run(profile: Profile, executable: PathBuf, action: Action) -> Result<Value, Error> {
    tokio::time::timeout(Duration::from_secs(12), async move {
        let context = Arc::new(tokio::task::spawn_blocking(move || context(profile, executable)).await.map_err(|_| Error::Unavailable)??);
        loop {
            match context.launcher.attach(Role::Gui, Some(&context.build)).await {
                Ok(client) => {
                    let (status, mut result) = match &action {
                        Action::Change { generation, key, enabled } => {
                            let (status, state) = client.call("GET", "/v1/ready", None).await.map_err(|error| Error::Launch(LaunchError::Client(error)))?;
                            if status != 200 { return Err(remote_error(&state)); }
                            client.call("POST", "/v1/mcp/enrollment", Some(&json!({"server_epoch":state["server_epoch"],"operation_key":key,"expected_generation":generation.to_string(),"enabled":enabled}))).await
                        }
                        _ => client.call("GET", "/v1/mcp", None).await,
                    }.map_err(|error| Error::Launch(LaunchError::Client(error)))?;
                    if status != 200 { return Err(remote_error(&result)); }
                    let manifest = Manifest::read_for(&context.directory, context.profile.namespace()).map_err(|_| Error::Launch(LaunchError::UnsafeMetadata))?.ok_or(Error::Unavailable)?;
                    if manifest.url != client.endpoint.url() { return Err(Error::Launch(LaunchError::UnsafeMetadata)); }
                    result["url"] = json!(manifest.mcp_url); result["server_running"] = json!(true); return Ok(result);
                }
                Err(LaunchError::NotRunning | LaunchError::Stopped | LaunchError::Bootstrap(BootstrapError::Unavailable | BootstrapError::Expired)) => {
                    let selected = context.clone(); let intent = action.clone();
                    let result = tokio::task::spawn_blocking(move || match selected.directory.lock() {
                        Ok(owner) => offline(&selected, owner, &intent).map(Some),
                        Err(FileError::Locked) => Ok(None),
                        Err(error) => Err(Error::Launch(LaunchError::File(error))),
                    }).await.map_err(|_| Error::Unavailable)??;
                    if let Some(result) = result { return Ok(result); }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                Err(error) => return Err(Error::Launch(error)),
            }
        }
    }).await.map_err(|_| Error::Timeout)?
}
