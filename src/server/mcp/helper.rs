//! One-shot dynamic headers. No GUI, TV adapter or credential file is opened.
use super::enrollment::{EnrollmentStore, FileEnrollment};
use crate::infrastructure::private_files::RuntimeDirectory;
use crate::server::{
    bootstrap::Role,
    launch::{LaunchError, Launcher},
    lifecycle::endpoint::{build_identity, Manifest},
    profile::Profile,
};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc, time::Duration};

pub const DEADLINE: Duration = Duration::from_secs(8);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperError {
    Destination,
    Disabled,
    EnrollmentChanged,
    Metadata,
    Timeout,
    Unavailable,
    Launch(LaunchError),
}
impl HelperError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Destination => "mcp_destination_mismatch",
            Self::Disabled => "mcp_not_enrolled",
            Self::EnrollmentChanged => "mcp_enrollment_changed",
            Self::Metadata => "unsafe_mcp_metadata",
            Self::Timeout => "mcp_helper_timeout",
            Self::Unavailable => "mcp_helper_unavailable",
            Self::Launch(error) => error.code(),
        }
    }
}
struct Prepared {
    launcher: Launcher,
    directory: Arc<RuntimeDirectory>,
    generation: u64,
    build: String,
}
fn prepare(
    profile: &Profile,
    executable: &std::path::Path,
    destination: &str,
) -> Result<Prepared, HelperError> {
    // Production signing is verified before any profile I/O. Development trust
    // is selected only by the executable's explicit --fake-profile mode.
    let launcher = Launcher::new(profile.clone()).map_err(HelperError::Launch)?;
    let directory = profile.directory().map_err(|_| HelperError::Metadata)?;
    let manifest = Manifest::read_for(&directory, profile.namespace())
        .map_err(|_| HelperError::Metadata)?
        .ok_or(HelperError::Metadata)?;
    if destination != manifest.mcp_url {
        return Err(HelperError::Destination);
    }
    let enrollment = FileEnrollment::new(directory.clone(), profile.namespace())
        .read()
        .map_err(|_| HelperError::Metadata)?;
    if !enrollment.enabled {
        return Err(HelperError::Disabled);
    }
    let build = build_identity(executable).map_err(|_| HelperError::Unavailable)?;
    Ok(Prepared {
        launcher,
        directory,
        generation: enrollment.generation.0,
        build,
    })
}

/// Caller provides Claude's URL environment as comparison data, never as a
/// profile selector. Only the final successful return contains a credential.
pub async fn headers(
    profile: Profile,
    executable: PathBuf,
    destination: String,
) -> Result<Value, HelperError> {
    if destination.is_empty() || destination.len() > 1024 {
        return Err(HelperError::Destination);
    }
    tokio::time::timeout(DEADLINE, async move {
        let selected = profile.clone();
        let path = executable.clone();
        let url = destination.clone();
        let prepared = tokio::task::spawn_blocking(move || prepare(&selected, &path, &url))
            .await
            .map_err(|_| HelperError::Unavailable)??;
        let client = prepared
            .launcher
            .start_with_build(&executable, Role::Mcp, &prepared.build)
            .await
            .map_err(HelperError::Launch)?;
        if format!("{}/mcp", client.endpoint.url()) != destination {
            return Err(HelperError::Destination);
        }
        let generation = prepared.generation;
        tokio::task::spawn_blocking(move || {
            let manifest = Manifest::read_for(&prepared.directory, profile.namespace())
                .map_err(|_| HelperError::Metadata)?
                .ok_or(HelperError::Metadata)?;
            if destination != manifest.mcp_url {
                return Err(HelperError::Destination);
            }
            let enrollment = FileEnrollment::new(prepared.directory, profile.namespace())
                .read()
                .map_err(|_| HelperError::Metadata)?;
            if !enrollment.enabled || enrollment.generation.0 != generation {
                return Err(HelperError::EnrollmentChanged);
            }
            Ok(())
        })
        .await
        .map_err(|_| HelperError::Unavailable)??;
        Ok(json!({"Authorization":format!("Bearer {}", client.bearer)}))
    })
    .await
    .map_err(|_| HelperError::Timeout)?
}
