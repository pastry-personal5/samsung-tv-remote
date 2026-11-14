//! Conservative client/scope preflight. Never executes configuration helpers,
//! contacts a server, changes client settings or infers client attachment.
use super::client_config::{self, NAME};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::AsyncReadExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    UnsupportedClient,
    ClientUnavailable,
    Restricted,
    ScopeConflict,
    Configuration(client_config::Error),
}
/// Paths are selected by native-client composition, never by a tool argument.
#[derive(Debug, Clone)]
pub struct Client {
    pub executable: PathBuf,
    pub configuration: PathBuf,
    pub settings: PathBuf,
    pub managed: PathBuf,
}

impl Client {
    pub fn installed() -> Result<Self, Error> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or(Error::ClientUnavailable)?;
        let executable = [
            home.join(".local/bin/claude"),
            PathBuf::from("/opt/homebrew/bin/claude"),
            PathBuf::from("/usr/local/bin/claude"),
        ]
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| home.join(".local/bin/claude"));
        let (configuration, settings) = match std::env::var_os("CLAUDE_CONFIG_DIR") {
            Some(path) => {
                let path = PathBuf::from(path);
                if !path.is_absolute() {
                    return Err(Error::ClientUnavailable);
                }
                (path.join(".claude.json"), path)
            }
            None => (home.join(".claude.json"), home.join(".claude")),
        };
        Ok(Self {
            executable,
            configuration,
            settings,
            managed: PathBuf::from("/Library/Application Support/ClaudeCode"),
        })
    }
    pub async fn check(&self) -> Result<(), Error> {
        if exists(Path::new(
            "/Library/Managed Preferences/com.anthropic.claudecode.plist",
        ))? {
            return Err(Error::Restricted);
        }
        self.check_version().await?;
        let paths = self.clone();
        tokio::task::spawn_blocking(move || paths.check_files())
            .await
            .map_err(|_| Error::ClientUnavailable)?
    }

    async fn check_version(&self) -> Result<(), Error> {
        if !self.executable.is_absolute() {
            return Err(Error::ClientUnavailable);
        }
        let mut child = tokio::process::Command::new(&self.executable)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| Error::ClientUnavailable)?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or(Error::ClientUnavailable)?
            .take(129);
        let result = tokio::time::timeout(Duration::from_secs(3), async {
            let mut bytes = Vec::new();
            stdout
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| Error::ClientUnavailable)?;
            if bytes.len() > 128 {
                return Err(Error::UnsupportedClient);
            }
            let status = child.wait().await.map_err(|_| Error::ClientUnavailable)?;
            if !status.success() || !supported_version(&bytes) {
                return Err(Error::UnsupportedClient);
            }
            Ok(())
        })
        .await
        .map_err(|_| Error::ClientUnavailable)?;
        result
    }

    pub fn check_files(&self) -> Result<(), Error> {
        // Presence, including unreadable or symlinked policy, fails closed.
        // Effective organization policy is still verified by the real client.
        for path in [
            self.managed.join("managed-mcp.json"),
            self.managed.join("managed-settings.json"),
            self.managed.join("managed-settings.d"),
        ] {
            if exists(&path)? {
                return Err(Error::Restricted);
            }
        }
        let document = read(&self.configuration)?;
        settings(&self.settings.join("settings.json"))?;
        settings(&self.settings.join("settings.local.json"))?;
        let projects = match document.get("projects") {
            None => return Ok(()),
            Some(value) => value.as_object().ok_or(Error::ScopeConflict)?,
        };
        if projects.len() > 256 {
            return Err(Error::ScopeConflict);
        }
        // Projects usually share ancestors; inspect each directory once.
        let mut inspected = std::collections::HashSet::new();
        for (path, project) in projects {
            if !project.is_object() || project.get("mcpServers").is_some_and(|v| !v.is_object()) {
                return Err(Error::ScopeConflict);
            }
            if project
                .get("mcpServers")
                .and_then(|v| v.get(NAME))
                .is_some()
            {
                return Err(Error::ScopeConflict);
            }
            let path = Path::new(path);
            if !path.is_absolute() {
                return Err(Error::ScopeConflict);
            }
            // An ancestor's project entry can shadow the user entry as well.
            for ancestor in path.ancestors().take(64) {
                if !inspected.insert(ancestor) {
                    continue;
                }
                let project = read(&ancestor.join(".mcp.json"))?;
                if project
                    .get("mcpServers")
                    .and_then(|v| v.get(NAME))
                    .is_some()
                {
                    return Err(Error::ScopeConflict);
                }
                settings(&ancestor.join(".claude/settings.json"))?;
                settings(&ancestor.join(".claude/settings.local.json"))?;
            }
        }
        Ok(())
    }
}

fn supported_version(bytes: &[u8]) -> bool {
    // A client upgrade must earn its own isolated writer/helper evidence.
    bytes == b"2.1.325 (Claude Code)\n"
}
fn exists(path: &Path) -> Result<bool, Error> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(Error::Restricted),
    }
}
fn read(path: &Path) -> Result<Value, Error> {
    client_config::inspect_document(path).map_err(Error::Configuration)
}
fn settings(path: &Path) -> Result<(), Error> {
    let value = read(path)?;
    if [
        "allowedMcpServers",
        "deniedMcpServers",
        "managedMcpServers",
        "allowManagedMcpServersOnly",
        "strictPluginOnlyCustomization",
        "disableMcp",
    ]
    .iter()
    .any(|key| value.get(*key).is_some())
    {
        return Err(Error::Restricted);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn client(root: &Path) -> Client {
        Client {
            executable: root.join("claude"),
            configuration: root.join(".claude.json"),
            settings: root.join("settings"),
            managed: root.join("managed"),
        }
    }
    fn write(path: &Path, value: Value) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    #[test]
    fn scope_conflicts_and_managed_policy_prevent_configuration_changes() {
        let root = tempfile::tempdir().unwrap();
        let client = client(root.path());
        let project = root.path().join("project");
        write(
            &client.configuration,
            json!({"projects":{project.to_str().unwrap():{"mcpServers":{}}}}),
        );
        assert_eq!(client.check_files(), Ok(()));
        let config = std::fs::read(&client.configuration).unwrap();
        write(
            &project.join(".mcp.json"),
            json!({"mcpServers":{NAME:{"type":"http","url":"https://example.invalid"}}}),
        );
        assert_eq!(client.check_files(), Err(Error::ScopeConflict));
        std::fs::remove_file(project.join(".mcp.json")).unwrap();
        write(
            &client.settings.join("settings.json"),
            json!({"deniedMcpServers":[]}),
        );
        assert_eq!(client.check_files(), Err(Error::Restricted));
        std::fs::remove_file(client.settings.join("settings.json")).unwrap();
        write(
            &client.managed.join("managed-mcp.json"),
            json!({"mcpServers":{}}),
        );
        assert_eq!(client.check_files(), Err(Error::Restricted));
        assert_eq!(std::fs::read(client.configuration).unwrap(), config);
    }
    #[test]
    fn local_scope_and_ancestor_project_conflicts_are_detected() {
        let root = tempfile::tempdir().unwrap();
        let client = client(root.path());
        let project = root.path().join("parent/child");
        write(
            &client.configuration,
            json!({"projects":{project.to_str().unwrap():{"mcpServers":{NAME:{}}}}}),
        );
        assert_eq!(client.check_files(), Err(Error::ScopeConflict));
        write(
            &client.configuration,
            json!({"projects":{project.to_str().unwrap():{}}}),
        );
        write(
            &root.path().join("parent/.mcp.json"),
            json!({"mcpServers":{NAME:{}}}),
        );
        assert_eq!(client.check_files(), Err(Error::ScopeConflict));
    }
    #[test]
    fn unsupported_versions_and_unsafe_files_fail_closed() {
        assert!(supported_version(b"2.1.325 (Claude Code)\n"));
        assert!(!supported_version(b"2.1.326 (Claude Code)\n"));
        let root = tempfile::tempdir().unwrap();
        let client = client(root.path());
        std::fs::write(&client.configuration, b"{\"projects\":{},\"projects\":{}}").unwrap();
        assert_eq!(
            client.check_files(),
            Err(Error::Configuration(client_config::Error::Malformed))
        );
    }

    #[tokio::test]
    async fn version_probe_bounds_output_and_terminates_a_stalled_child() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let client = client(root.path());
        for (script, expected) in [
            ("#!/bin/sh\nprintf '2.1.325 (Claude Code)\\n'\n", Ok(())),
            (
                "#!/bin/sh\nprintf 'unsupported\\n'\n",
                Err(Error::UnsupportedClient),
            ),
            (
                "#!/bin/sh\nexec /usr/bin/yes x\n",
                Err(Error::UnsupportedClient),
            ),
            (
                "#!/bin/sh\nexec /bin/sleep 30\n",
                Err(Error::ClientUnavailable),
            ),
        ] {
            std::fs::write(&client.executable, script).unwrap();
            std::fs::set_permissions(&client.executable, std::fs::Permissions::from_mode(0o700))
                .unwrap();
            assert_eq!(client.check_version().await, expected);
        }
        assert!(!client.configuration.exists());
    }
}
