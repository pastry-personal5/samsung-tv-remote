//! Typed native connection workflow. No raw configuration or credentials cross
//! into presentation. Checking status never grants access or starts a server.
use crate::server::{
    mcp::{
        client_config::{self, Entry},
        connection::{self, Connection},
        management::{self, Action},
        preflight,
    },
    profile::Profile,
};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Configuration {
    Absent,
    Saved,
    RepairRequired,
    Conflict,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub configuration: Configuration,
    pub enabled: bool,
    pub endpoint_verified: bool,
    pub(crate) generation: u64,
    pub(crate) entry: Option<Entry>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    Check,
    Verify,
    Connect { state: State, key: String },
    Disconnect { state: State, key: String },
}
impl Intent {
    pub fn connect(state: State) -> Self {
        Self::Connect {
            state,
            key: uuid::Uuid::new_v4().to_string(),
        }
    }
    pub fn disconnect(state: State) -> Self {
        Self::Disconnect {
            state,
            key: uuid::Uuid::new_v4().to_string(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    Busy,
    UnsupportedClient,
    ClientUnavailable,
    Restricted,
    ScopeConflict,
    Management,
    ConfigurationConflict,
    Workflow(Progress),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowConfiguration {
    Unchanged,
    Unknown,
    Saved,
    Removed,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub configuration: WorkflowConfiguration,
    pub enabled: Option<bool>,
    pub recovery_file: Option<PathBuf>,
}
fn preflight_error(error: preflight::Error) -> Error {
    match error {
        preflight::Error::UnsupportedClient => Error::UnsupportedClient,
        preflight::Error::ClientUnavailable => Error::ClientUnavailable,
        preflight::Error::Restricted => Error::Restricted,
        preflight::Error::ScopeConflict => Error::ScopeConflict,
        preflight::Error::Configuration(_) => Error::ConfigurationConflict,
    }
}
fn workflow_error(failure: connection::Failure) -> Error {
    Error::Workflow(Progress {
        recovery_file: match failure.cause {
            connection::Cause::Configuration(client_config::Error::Recovery(path)) => Some(path),
            _ => None,
        },
        configuration: match failure.progress.configuration {
            connection::Configuration::Unchanged => WorkflowConfiguration::Unchanged,
            connection::Configuration::Unknown => WorkflowConfiguration::Unknown,
            connection::Configuration::Saved => WorkflowConfiguration::Saved,
            connection::Configuration::Removed => WorkflowConfiguration::Removed,
        },
        enabled: failure
            .progress
            .enrollment
            .as_ref()
            .map(|state| state.enabled),
    })
}

pub(crate) async fn run(
    profile: Profile,
    executable: PathBuf,
    intent: Intent,
) -> Result<State, Error> {
    // The development probe cannot write the owner's default client entry.
    if matches!(profile, Profile::Development(_)) && std::env::var_os("CLAUDE_CONFIG_DIR").is_none()
    {
        return Err(Error::ClientUnavailable);
    }
    let client = preflight::Client::installed().map_err(preflight_error)?;
    let connection = Connection {
        profile,
        executable,
        configuration: client.configuration.clone(),
    };
    match intent {
        Intent::Check => inspect(&connection).await,
        Intent::Verify => {
            let before = inspect(&connection).await?;
            if !before.enabled || before.configuration != Configuration::Saved {
                return Err(Error::Unavailable);
            }
            let url = before
                .entry
                .as_ref()
                .ok_or(Error::Unavailable)?
                .url()
                .to_owned();
            let headers = crate::server::mcp::helper::headers(
                connection.profile.clone(),
                connection.executable.clone(),
                url.clone(),
            )
            .await
            .map_err(|_| Error::Unavailable)?;
            let bearer = headers["Authorization"]
                .as_str()
                .and_then(|v| v.strip_prefix("Bearer "))
                .ok_or(Error::Unavailable)?;
            let endpoint =
                super::http::Endpoint::parse(url.strip_suffix("/mcp").ok_or(Error::Unavailable)?)
                    .map_err(|_| Error::Unavailable)?;
            let id = uuid::Uuid::new_v4().to_string();
            let request = serde_json::json!({"jsonrpc":"2.0","id":id,"method":"tools/list",
                "params":{"_meta":{"io.modelcontextprotocol/protocolVersion":crate::server::mcp::protocol::VERSION,
                "io.modelcontextprotocol/clientCapabilities":{}}}});
            let (status, value) =
                super::http::request(endpoint, bearer, "POST", "/mcp", Some(&request), None)
                    .await
                    .map_err(|_| Error::Unavailable)?;
            if status != 200
                || value["id"] != id
                || value["result"]["tools"] != *crate::server::mcp::catalog::tools()
            {
                return Err(Error::Unavailable);
            }
            let mut after = inspect(&connection).await?;
            if !after.enabled
                || after.generation != before.generation
                || after.entry != before.entry
            {
                return Err(Error::Unavailable);
            }
            after.endpoint_verified = true;
            Ok(after)
        }
        Intent::Connect { state, key } => {
            client.check().await.map_err(preflight_error)?;
            if state.configuration == Configuration::Conflict {
                return Err(Error::ConfigurationConflict);
            }
            connection
                .connect(state.generation, &key, state.entry)
                .await
                .map_err(workflow_error)?;
            inspect(&connection).await
        }
        Intent::Disconnect { state, key } => {
            // Revocation remains available if the installed client changed.
            // A conflicting entry cannot authorize deleting another entry.
            let entry = match state.entry {
                Some(entry) => entry,
                None => {
                    let value = management::run(
                        connection.profile.clone(),
                        connection.executable.clone(),
                        Action::Status,
                    )
                    .await
                    .map_err(|_| Error::Management)?;
                    let url = value["url"].as_str().ok_or(Error::Management)?;
                    connection
                        .entry(url)
                        .map_err(|_| Error::ConfigurationConflict)?
                }
            };
            connection
                .disconnect(state.generation, &key, entry)
                .await
                .map_err(workflow_error)?;
            inspect(&connection).await
        }
    }
}

async fn inspect(connection: &Connection) -> Result<State, Error> {
    let status = management::run(
        connection.profile.clone(),
        connection.executable.clone(),
        Action::Status,
    )
    .await
    .map_err(|_| Error::Management)?;
    let enrollment: connection::Enrollment =
        serde_json::from_value(status).map_err(|_| Error::Management)?;
    let path = connection.configuration.clone();
    let inspected = tokio::task::spawn_blocking(move || client_config::inspect(&path))
        .await
        .map_err(|_| Error::Unavailable)?;
    // An unreadable/unsafe client file must not hide the enrollment controls.
    // Disconnect can still revoke first and visibly fail safe entry removal.
    let configuration_failed = inspected.is_err();
    let value = inspected.ok().flatten();
    let expected = enrollment
        .url
        .as_deref()
        .map(|url| connection.entry(url))
        .transpose()
        .map_err(|_| Error::ConfigurationConflict)?;
    let previous = value
        .as_ref()
        .filter(|value| {
            value["type"] == "http"
                && enrollment
                    .url
                    .as_ref()
                    .is_some_and(|url| value["url"] == *url)
        })
        .and_then(|value| serde_json::from_value::<Entry>(value.clone()).ok());
    let configuration = match value.as_ref() {
        _ if configuration_failed => Configuration::Conflict,
        None => Configuration::Absent,
        Some(value)
            if expected
                .as_ref()
                .is_some_and(|entry| entry.value() == *value) =>
        {
            Configuration::Saved
        }
        Some(_) if previous.is_some() => Configuration::RepairRequired,
        Some(_) => Configuration::Conflict,
    };
    Ok(State {
        configuration,
        enabled: enrollment.enabled,
        endpoint_verified: false,
        generation: enrollment.generation.0,
        entry: if configuration == Configuration::Saved {
            expected
        } else {
            previous
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn connection(root: &std::path::Path) -> Connection {
        Connection {
            profile: Profile::development(&root.canonicalize().unwrap()).unwrap(),
            executable: std::env::current_exe().unwrap(),
            configuration: root.join(".claude.json"),
        }
    }
    #[tokio::test]
    async fn malformed_client_file_does_not_hide_revocation_and_failed_removal_stays_disabled() {
        let root = tempfile::tempdir().unwrap();
        let connection = connection(root.path());
        let connected = connection
            .connect(0, &uuid::Uuid::new_v4().to_string(), None)
            .await
            .unwrap();
        std::fs::write(&connection.configuration, b"{").unwrap();
        let state = inspect(&connection).await.unwrap();
        assert_eq!(state.configuration, Configuration::Conflict);
        assert!(state.enabled);
        let failure = connection
            .disconnect(
                state.generation,
                &uuid::Uuid::new_v4().to_string(),
                connected.entry.unwrap(),
            )
            .await
            .unwrap_err();
        let progress = workflow_error(failure);
        assert!(matches!(
            progress,
            Error::Workflow(Progress {
                enabled: Some(false),
                ..
            })
        ));
        assert!(!inspect(&connection).await.unwrap().enabled);
        assert_eq!(std::fs::read(&connection.configuration).unwrap(), b"{");
    }
    #[tokio::test]
    async fn relocation_is_repairable_only_at_the_saved_trusted_endpoint() {
        let root = tempfile::tempdir().unwrap();
        let connection = connection(root.path());
        let connected = connection
            .connect(0, &uuid::Uuid::new_v4().to_string(), None)
            .await
            .unwrap();
        let current = connected.entry.unwrap();
        let old = Entry::new(
            std::path::Path::new("/synthetic/old remote"),
            Some(root.path()),
            current.url(),
        )
        .unwrap();
        client_config::edit(&connection.configuration, Some(&current), Some(&old)).unwrap();
        let state = inspect(&connection).await.unwrap();
        assert_eq!(state.configuration, Configuration::RepairRequired);
        connection
            .connect(
                state.generation,
                &uuid::Uuid::new_v4().to_string(),
                state.entry,
            )
            .await
            .unwrap();
        let repaired = inspect(&connection).await.unwrap();
        assert_eq!(repaired.configuration, Configuration::Saved);
        assert_eq!(repaired.generation, 1);
        assert!(!repaired.endpoint_verified);
        let current = repaired.entry.unwrap();
        let other_url = if current.url() == "http://127.0.0.1:54321/mcp" {
            "http://127.0.0.1:54322/mcp"
        } else {
            "http://127.0.0.1:54321/mcp"
        };
        let foreign =
            Entry::new(std::path::Path::new("/synthetic/other"), None, other_url).unwrap();
        client_config::edit(&connection.configuration, Some(&current), Some(&foreign)).unwrap();
        assert_eq!(
            inspect(&connection).await.unwrap().configuration,
            Configuration::Conflict
        );
    }
}
