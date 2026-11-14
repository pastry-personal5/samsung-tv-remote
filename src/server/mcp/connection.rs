//! Ordered configuration/enrollment workflow. Client compatibility and scope
//! checks belong to the caller; this does not prove endpoint or client attachment.
use super::{
    client_config::{self, Entry},
    management::{self, Action},
};
use crate::server::{control_http::wire::Counter, profile::Profile};
use serde::Deserialize;
use std::{future::Future, path::PathBuf, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Enrollment {
    pub generation: Counter,
    pub enabled: bool,
    pub operation_key: Option<String>,
    pub url: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Configuration {
    #[default]
    Unchanged,
    /// A failed publication/removal can require inspection or retained-file
    /// recovery; do not claim the original file remains intact.
    Unknown,
    Saved,
    Removed,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Progress {
    pub configuration: Configuration,
    /// Latest acknowledged state, never inferred from a configuration file.
    /// `None` after an ambiguous management failure means recheck is required.
    pub enrollment: Option<Enrollment>,
    pub entry: Option<Entry>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cause {
    Configuration(client_config::Error),
    Management(management::Error),
    InvalidResponse,
    StaleGeneration,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub cause: Cause,
    pub progress: Box<Progress>,
}

/// Construct only after checking the installed client's supported writer,
/// helper capability, managed restrictions and local/project name conflicts.
/// Production callers use the signed executable's fixed production profile.
pub struct Connection {
    pub profile: Profile,
    pub executable: PathBuf,
    pub configuration: PathBuf,
}
trait Backend {
    fn manage(&self, action: Action) -> impl Future<Output = Result<Enrollment, Cause>> + Send;
    fn entry(&self, url: &str) -> Result<Entry, Cause>;
    fn edit(
        &self,
        expected: Option<Entry>,
        desired: Option<Entry>,
    ) -> impl Future<Output = Result<(), Cause>> + Send;
}
struct Workflow<'a> {
    connection: &'a Connection,
    lock: Arc<std::fs::File>,
}
impl Backend for Workflow<'_> {
    async fn manage(&self, action: Action) -> Result<Enrollment, Cause> {
        let value = management::run(
            self.connection.profile.clone(),
            self.connection.executable.clone(),
            action,
        )
        .await
        .map_err(Cause::Management)?;
        serde_json::from_value(value).map_err(|_| Cause::InvalidResponse)
    }
    fn entry(&self, url: &str) -> Result<Entry, Cause> {
        self.connection.entry(url).map_err(Cause::Configuration)
    }
    async fn edit(&self, expected: Option<Entry>, desired: Option<Entry>) -> Result<(), Cause> {
        let path = self.connection.configuration.clone();
        let guard = self.lock.clone();
        tokio::task::spawn_blocking(move || {
            // Cancellation may drop the workflow before this blocking edit
            // finishes. Retain serialization until publication/removal settles.
            let _guard = guard;
            client_config::edit(&path, expected.as_ref(), desired.as_ref())
        })
        .await
        .map_err(|_| Cause::Configuration(client_config::Error::Unavailable))?
        .map_err(Cause::Configuration)
    }
}
impl Connection {
    /// This app's exact client entry for an enrolled endpoint URL.
    pub fn entry(&self, url: &str) -> Result<Entry, client_config::Error> {
        let fake = match &self.profile {
            Profile::Development(path) => Some(path.as_path()),
            Profile::Control => None,
        };
        Entry::new(&self.executable, fake, url)
    }
    /// Only an explicit Connect may invoke this. A saved entry is reported
    /// separately from enrollment; endpoint/client verification is subsequent.
    pub async fn connect(
        &self,
        generation: u64,
        key: &str,
        expected: Option<Entry>,
    ) -> Result<Progress, Failure> {
        let lock = client_config::workflow_lock(&self.configuration).map_err(|error| Failure {
            cause: Cause::Configuration(error),
            progress: Box::default(),
        })?;
        connect(
            &Workflow {
                connection: self,
                lock: Arc::new(lock),
            },
            generation,
            key,
            expected,
        )
        .await
    }
    /// A failed revocation never removes the entry. A failed removal retains
    /// acknowledged disabled state for a safe, visible repair action.
    pub async fn disconnect(
        &self,
        generation: u64,
        key: &str,
        expected: Entry,
    ) -> Result<Progress, Failure> {
        let lock = client_config::workflow_lock(&self.configuration).map_err(|error| Failure {
            cause: Cause::Configuration(error),
            progress: Box::default(),
        })?;
        disconnect(
            &Workflow {
                connection: self,
                lock: Arc::new(lock),
            },
            generation,
            key,
            expected,
        )
        .await
    }
}
async fn connect(
    backend: &impl Backend,
    generation: u64,
    key: &str,
    expected: Option<Entry>,
) -> Result<Progress, Failure> {
    let mut progress = Progress::default();
    let result = async {
        let prepared = backend.manage(Action::Prepare).await?;
        // A retry may observe its own already-durable grant after a lost reply.
        if prepared.generation.0 != generation
            && !(prepared.enabled && prepared.operation_key.as_deref() == Some(key))
        {
            progress.enrollment = Some(prepared);
            return Err(Cause::StaleGeneration);
        }
        let url = prepared.url.as_deref().ok_or(Cause::InvalidResponse)?;
        let entry = backend.entry(url)?;
        progress.enrollment = Some(prepared);
        progress.configuration = Configuration::Unknown;
        backend.edit(expected, Some(entry.clone())).await?;
        progress.configuration = Configuration::Saved;
        progress.entry = Some(entry);
        // Even an identical Connect goes through the protected generation
        // comparison. A concurrent Disconnect must not be silently regranted.
        progress.enrollment = None;
        let granted = backend
            .manage(Action::Change {
                generation,
                key: key.into(),
                enabled: true,
            })
            .await?;
        if !granted.enabled || granted.url.as_deref() != progress.entry.as_ref().map(Entry::url) {
            return Err(Cause::InvalidResponse);
        }
        progress.enrollment = Some(granted);
        Ok(())
    }
    .await;
    result.map(|()| progress.clone()).map_err(|cause| Failure {
        cause,
        progress: Box::new(progress),
    })
}
async fn disconnect(
    backend: &impl Backend,
    generation: u64,
    key: &str,
    expected: Entry,
) -> Result<Progress, Failure> {
    let mut progress = Progress::default();
    let result = async {
        let disabled = backend
            .manage(Action::Change {
                generation,
                key: key.into(),
                enabled: false,
            })
            .await?;
        if disabled.enabled || disabled.operation_key.as_deref() != Some(key) {
            return Err(Cause::InvalidResponse);
        }
        progress.enrollment = Some(disabled);
        progress.configuration = Configuration::Unknown;
        backend.edit(Some(expected), None).await?;
        progress.configuration = Configuration::Removed;
        Ok(())
    }
    .await;
    result.map(|()| progress.clone()).map_err(|cause| Failure {
        cause,
        progress: Box::new(progress),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, sync::Mutex};
    const KEY: &str = "synthetic-operation";
    struct Fake {
        states: Mutex<VecDeque<Result<Enrollment, Cause>>>,
        edits: Result<(), Cause>,
        events: Mutex<Vec<&'static str>>,
    }
    impl Backend for Fake {
        async fn manage(&self, action: Action) -> Result<Enrollment, Cause> {
            self.events.lock().unwrap().push(match action {
                Action::Prepare => "prepare",
                Action::Change { enabled: true, .. } => "grant",
                Action::Change { enabled: false, .. } => "revoke",
                _ => "status",
            });
            self.states.lock().unwrap().pop_front().unwrap()
        }
        fn entry(&self, url: &str) -> Result<Entry, Cause> {
            Entry::new(std::path::Path::new("/synthetic/remote"), None, url)
                .map_err(Cause::Configuration)
        }
        async fn edit(&self, _: Option<Entry>, desired: Option<Entry>) -> Result<(), Cause> {
            self.events
                .lock()
                .unwrap()
                .push(if desired.is_some() { "save" } else { "remove" });
            self.edits.clone()
        }
    }
    fn state(generation: u64, enabled: bool) -> Enrollment {
        Enrollment {
            generation: Counter(generation),
            enabled,
            operation_key: Some(KEY.into()),
            url: Some("http://127.0.0.1:54321/mcp".into()),
        }
    }
    fn fake(states: Vec<Result<Enrollment, Cause>>, edits: Result<(), Cause>) -> Fake {
        Fake {
            states: Mutex::new(states.into()),
            edits,
            events: Mutex::default(),
        }
    }
    #[tokio::test]
    async fn connect_saves_before_grant_and_identical_connect_keeps_generation() {
        let backend = fake(vec![Ok(state(1, true)), Ok(state(1, true))], Ok(()));
        let entry = backend
            .entry(state(1, true).url.as_deref().unwrap())
            .unwrap();
        let result = connect(&backend, 1, KEY, Some(entry)).await.unwrap();
        assert_eq!(
            *backend.events.lock().unwrap(),
            ["prepare", "save", "grant"]
        );
        assert_eq!(result.enrollment.unwrap().generation.0, 1);
        assert_eq!(result.configuration, Configuration::Saved);
    }
    #[tokio::test]
    async fn failed_configuration_write_never_grants() {
        let backend = fake(
            vec![Ok(state(0, false))],
            Err(Cause::Configuration(client_config::Error::Conflict)),
        );
        let failure = connect(&backend, 0, KEY, None).await.unwrap_err();
        assert_eq!(*backend.events.lock().unwrap(), ["prepare", "save"]);
        assert!(!failure.progress.enrollment.unwrap().enabled);
        assert_eq!(failure.progress.configuration, Configuration::Unknown);
    }
    #[tokio::test]
    async fn lost_grant_reply_leaves_saved_entry_with_unknown_enrollment() {
        let backend = fake(
            vec![
                Ok(state(0, false)),
                Err(Cause::Management(management::Error::Timeout)),
            ],
            Ok(()),
        );
        let failure = connect(&backend, 0, KEY, None).await.unwrap_err();
        assert_eq!(failure.progress.configuration, Configuration::Saved);
        assert!(failure.progress.enrollment.is_none());
        assert!(failure.progress.entry.is_some());
    }
    #[tokio::test]
    async fn stale_connect_does_not_write_or_regrant_but_original_key_can_recover() {
        let backend = fake(vec![Ok(state(2, false))], Ok(()));
        assert_eq!(
            connect(&backend, 0, KEY, None).await.unwrap_err().cause,
            Cause::StaleGeneration
        );
        assert_eq!(*backend.events.lock().unwrap(), ["prepare"]);
        let backend = fake(vec![Ok(state(1, true)), Ok(state(1, true))], Ok(()));
        assert!(connect(&backend, 0, KEY, None).await.is_ok());
    }
    #[tokio::test]
    async fn concurrent_disconnect_after_save_cannot_be_silently_regranted() {
        let backend = fake(
            vec![
                Ok(state(1, true)),
                Err(Cause::Management(management::Error::Enrollment(
                    super::super::enrollment::EnrollmentError::StaleGeneration,
                ))),
            ],
            Ok(()),
        );
        let failure = connect(&backend, 1, KEY, None).await.unwrap_err();
        assert_eq!(
            *backend.events.lock().unwrap(),
            ["prepare", "save", "grant"]
        );
        assert_eq!(failure.progress.configuration, Configuration::Saved);
        assert!(failure.progress.enrollment.is_none());
    }
    #[tokio::test]
    async fn disconnect_retains_revocation_when_removal_conflicts() {
        let backend = fake(
            vec![Ok(state(2, false))],
            Err(Cause::Configuration(client_config::Error::Conflict)),
        );
        let entry = backend
            .entry(state(1, true).url.as_deref().unwrap())
            .unwrap();
        let failure = disconnect(&backend, 1, KEY, entry).await.unwrap_err();
        assert_eq!(*backend.events.lock().unwrap(), ["revoke", "remove"]);
        assert!(!failure.progress.enrollment.unwrap().enabled);
    }
    #[tokio::test]
    async fn failed_revocation_never_removes_configuration() {
        let backend = fake(
            vec![Err(Cause::Management(management::Error::Timeout))],
            Ok(()),
        );
        let entry = backend
            .entry(state(1, true).url.as_deref().unwrap())
            .unwrap();
        let failure = disconnect(&backend, 1, KEY, entry).await.unwrap_err();
        assert_eq!(*backend.events.lock().unwrap(), ["revoke"]);
        assert!(failure.progress.enrollment.is_none());
        assert_eq!(failure.progress.configuration, Configuration::Unchanged);
    }
}
