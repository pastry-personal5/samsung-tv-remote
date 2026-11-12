//! Narrow user-scope Claude configuration edits. The caller must verify the
//! installed client's sibling mkdir-lock interoperability before using this.
//! A swap preserves a racing writer's document so conflicts can be recovered.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        },
    },
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

pub const NAME: &str = "samsung-tv-remote";
const LIMIT: u64 = 8 * 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Busy,
    Unsafe,
    Malformed,
    Conflict,
    Unavailable,
    Recovery(PathBuf),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    #[serde(rename = "type")]
    transport: String,
    url: String,
    #[serde(rename = "headersHelper")]
    helper: String,
}
impl Entry {
    pub fn new(executable: &Path, fake_profile: Option<&Path>, url: &str) -> Result<Self, Error> {
        if !executable.is_absolute() {
            return Err(Error::Unsafe);
        }
        crate::control_client::http::Endpoint::parse(
            url.strip_suffix("/mcp").ok_or(Error::Unsafe)?,
        )
        .map_err(|_| Error::Unsafe)?;
        let mut helper = format!("{} --mcp-headers", quote(executable)?);
        if let Some(profile) = fake_profile {
            if !profile.is_absolute() {
                return Err(Error::Unsafe);
            }
            helper.push_str(&format!(" --fake-profile {}", quote(profile)?));
        }
        Ok(Self {
            transport: "http".into(),
            url: url.into(),
            helper,
        })
    }
    pub fn value(&self) -> Value {
        json!(self)
    }
    pub fn url(&self) -> &str {
        &self.url
    }
}
fn quote(path: &Path) -> Result<String, Error> {
    let text = path
        .to_str()
        .filter(|v| !v.contains('\0'))
        .ok_or(Error::Unsafe)?;
    Ok(format!("'{}'", text.replace('\'', "'\\''")))
}

struct Snapshot {
    bytes: Vec<u8>,
    identity: (u64, u64, u32),
}
fn read(path: &Path) -> Result<Option<Snapshot>, Error> {
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Error::Unsafe),
    };
    let metadata = file.metadata().map_err(|_| Error::Unavailable)?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != super::super::bootstrap::socket::uid()
        || metadata.mode() & 0o022 != 0
        || metadata.len() > LIMIT
    {
        return Err(Error::Unsafe);
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Unavailable)?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::Unsafe);
    }
    Ok(Some(Snapshot {
        bytes,
        identity: (metadata.dev(), metadata.ino(), metadata.mode() & 0o777),
    }))
}
fn same(left: Option<&Snapshot>, right: Option<&Snapshot>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(a), Some(b)) => a.identity == b.identity && a.bytes == b.bytes,
        _ => false,
    }
}
fn document(snapshot: Option<&Snapshot>) -> Result<Value, Error> {
    let value = match snapshot {
        Some(snapshot) => {
            super::protocol::unique_json(&snapshot.bytes).map_err(|_| Error::Malformed)?
        }
        None => json!({}),
    };
    if !value.is_object() || value.get("mcpServers").is_some_and(|v| !v.is_object()) {
        return Err(Error::Malformed);
    }
    Ok(value)
}
pub fn inspect(path: &Path) -> Result<Option<Value>, Error> {
    let snapshot = read(path)?;
    Ok(document(snapshot.as_ref())?["mcpServers"]
        .get(NAME)
        .cloned())
}

/// Serialize whole app workflows, including the gap between durable disable
/// and entry removal. An advisory inode lock survives no process crash and is
/// deliberately never unlinked, so another process cannot lock a new inode.
pub(super) fn workflow_lock(path: &Path) -> Result<File, Error> {
    if !path.is_absolute() {
        return Err(Error::Unsafe);
    }
    let parent = path.parent().ok_or(Error::Unsafe)?;
    let directory = std::fs::symlink_metadata(parent).map_err(|_| Error::Unsafe)?;
    if !directory.is_dir()
        || directory.uid() != super::super::bootstrap::socket::uid()
        || directory.mode() & 0o022 != 0
    {
        return Err(Error::Unsafe);
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(Error::Unsafe)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path.with_file_name(format!("{name}.samsung-tv-remote.lock")))
        .map_err(|_| Error::Unsafe)?;
    let metadata = file.metadata().map_err(|_| Error::Unavailable)?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != directory.uid()
        || metadata.mode() & 0o077 != 0
    {
        return Err(Error::Unsafe);
    }
    // SAFETY: file owns a valid descriptor; LOCK_NB never blocks the UI runtime.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock {
                Error::Busy
            } else {
                Error::Unavailable
            },
        );
    }
    Ok(file)
}

struct Lock {
    path: PathBuf,
    directory: File,
    stop: mpsc::Sender<()>,
    heartbeat: Option<thread::JoinHandle<()>>,
}
impl Lock {
    fn acquire(path: &Path) -> Result<Self, Error> {
        use std::os::unix::fs::DirBuilderExt;
        let name = path
            .file_name()
            .ok_or(Error::Unsafe)?
            .to_str()
            .ok_or(Error::Unsafe)?;
        let lock = path.with_file_name(format!("{name}.lock"));
        let mut builder = std::fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(&lock).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Error::Busy
            } else {
                Error::Unavailable
            }
        })?;
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&lock)
            .map_err(|_| Error::Unsafe)?;
        let heartbeat_file = directory.try_clone().map_err(|_| Error::Unavailable)?;
        let (stop, receiver) = mpsc::channel();
        let heartbeat = thread::spawn(move || {
            while receiver.recv_timeout(Duration::from_secs(1))
                == Err(mpsc::RecvTimeoutError::Timeout)
            {
                let _ = heartbeat_file.set_modified(std::time::SystemTime::now());
            }
        });
        Ok(Self {
            path: lock,
            directory,
            stop,
            heartbeat: Some(heartbeat),
        })
    }
    fn valid(&self) -> bool {
        let Ok(ours) = self.directory.metadata() else {
            return false;
        };
        std::fs::symlink_metadata(&self.path).is_ok_and(|current| {
            current.is_dir() && current.dev() == ours.dev() && current.ino() == ours.ino()
        })
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.heartbeat.take() {
            let _ = thread.join();
        }
        if self.valid() {
            let _ = std::fs::remove_dir(&self.path);
        }
    }
}

#[derive(Clone, Copy)]
enum Stage {
    BeforeCompare,
    BeforePublish,
}
fn rename(
    parent: &File,
    from: &std::ffi::OsStr,
    to: &std::ffi::OsStr,
    swap: bool,
) -> Result<(), Error> {
    let from = std::ffi::CString::new(from.as_bytes()).map_err(|_| Error::Unsafe)?;
    let to = std::ffi::CString::new(to.as_bytes()).map_err(|_| Error::Unsafe)?;
    #[cfg(target_os = "macos")]
    {
        // SAFETY: names are single components under one held parent descriptor;
        // EXCL never replaces an unexpected file; SWAP preserves both inodes.
        if unsafe {
            libc::renameatx_np(
                parent.as_raw_fd(),
                from.as_ptr(),
                parent.as_raw_fd(),
                to.as_ptr(),
                if swap {
                    libc::RENAME_SWAP
                } else {
                    libc::RENAME_EXCL
                },
            )
        } != 0
        {
            return Err(Error::Conflict);
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (parent, from, to, swap);
        Err(Error::Unavailable)
    }
}

/// Exactly compare this app's entry under the verified client lock. `None`
/// means absence, not permission to overwrite an arbitrary existing entry.
pub fn edit(path: &Path, expected: Option<&Entry>, desired: Option<&Entry>) -> Result<(), Error> {
    edit_observed(path, expected, desired, |_| {})
}
fn edit_observed(
    path: &Path,
    expected: Option<&Entry>,
    desired: Option<&Entry>,
    mut observe: impl FnMut(Stage),
) -> Result<(), Error> {
    if !path.is_absolute() {
        return Err(Error::Unsafe);
    }
    let parent_path = path.parent().ok_or(Error::Unsafe)?;
    let parent = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(parent_path)
        .map_err(|_| Error::Unsafe)?;
    let metadata = parent.metadata().map_err(|_| Error::Unavailable)?;
    if metadata.uid() != super::super::bootstrap::socket::uid() || metadata.mode() & 0o022 != 0 {
        return Err(Error::Unsafe);
    }
    let lock = Lock::acquire(path)?;
    let original = read(path)?;
    if original
        .as_ref()
        .is_some_and(|value| value.identity.2 & 0o200 == 0)
    {
        return Err(Error::Unsafe);
    }
    let mut value = document(original.as_ref())?;
    let present = value["mcpServers"].get(NAME);
    // An interrupted removal can be retried with the same expected entry.
    // Absence requires no mutation and cannot discard another writer's entry.
    if desired.is_none() && present.is_none() {
        return Ok(());
    }
    // A lost grant reply can leave the exact desired entry already saved.
    // Accept it without rewriting bytes; enrollment still checks generation.
    if desired.is_some() && present == desired.map(Entry::value).as_ref() {
        return Ok(());
    }
    if present != expected.map(Entry::value).as_ref() {
        return Err(Error::Conflict);
    }
    if expected == desired {
        return Ok(());
    }
    if value.get("mcpServers").is_none() {
        value["mcpServers"] = json!({});
    }
    let entries = value["mcpServers"]
        .as_object_mut()
        .ok_or(Error::Malformed)?;
    if let Some(desired) = desired {
        entries.insert(NAME.into(), desired.value());
    } else {
        entries.remove(NAME);
    }
    let bytes = serde_json::to_vec_pretty(&value).map_err(|_| Error::Malformed)?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::Unsafe);
    }
    let temporary =
        path.with_file_name(format!(".claude.json.samsung-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temporary)
        .map_err(|_| Error::Unavailable)?;
    let prepared = (|| {
        if let Some(original) = &original {
            file.set_permissions(std::fs::Permissions::from_mode(original.identity.2))
                .map_err(|_| Error::Unavailable)?;
        }
        file.write_all(&bytes).map_err(|_| Error::Unavailable)?;
        file.sync_all().map_err(|_| Error::Unavailable)?;
        observe(Stage::BeforeCompare);
        if !lock.valid() || !same(original.as_ref(), read(path)?.as_ref()) {
            return Err(Error::Conflict);
        }
        observe(Stage::BeforePublish);
        rename(
            &parent,
            temporary.file_name().ok_or(Error::Unsafe)?,
            path.file_name().ok_or(Error::Unsafe)?,
            original.is_some(),
        )
    })();
    if let Err(error) = prepared {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    if let Some(original) = &original {
        // Publication already happened. Any unreadable displaced document must
        // remain named for repair, including a racing symlink/permission edit.
        let replaced = read(&temporary).map_err(|_| Error::Recovery(temporary.clone()))?;
        if !same(Some(original), replaced.as_ref()) {
            // An editor ignored the client lock between compare and publish.
            // Restore only while our exact document remains at the target.
            let published = file
                .metadata()
                .map_err(|_| Error::Recovery(temporary.clone()))?;
            let ours = read(path)
                .map_err(|_| Error::Recovery(temporary.clone()))?
                .is_some_and(|current| {
                    current.bytes == bytes
                        && current.identity.0 == published.dev()
                        && current.identity.1 == published.ino()
                });
            if ours
                && rename(
                    &parent,
                    temporary.file_name().ok_or(Error::Unsafe)?,
                    path.file_name().ok_or(Error::Unsafe)?,
                    true,
                )
                .is_ok()
            {
                let _ = parent.sync_all();
                let _ = std::fs::remove_file(&temporary);
                return Err(Error::Conflict);
            }
            return Err(Error::Recovery(temporary));
        }
        std::fs::remove_file(&temporary).map_err(|_| Error::Recovery(temporary.clone()))?;
    }
    parent.sync_all().map_err(|_| Error::Unavailable)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry() -> Entry {
        Entry::new(
            Path::new("/Applications/Synthetic TV's Remote.app/Contents/MacOS/remote"),
            None,
            "http://127.0.0.1:54321/mcp",
        )
        .unwrap()
    }
    #[test]
    fn token_free_entry_is_quoted_and_user_edits_preserve_unrelated_settings() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".claude.json");
        let original = json!({"sentinel":{"keep":[1,2,3]},"projects":{"/synthetic/project":{"mcpServers":{"local":{"type":"http","url":"https://example.invalid"}}}},"mcpServers":{"unrelated":{"type":"http","url":"https://example.invalid"}}});
        std::fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
        let entry = entry();
        assert!(entry.helper.contains("'\\''"));
        assert!(entry.value().get("headers").is_none());
        edit(&path, None, Some(&entry)).unwrap();
        assert!(inspect(&path).unwrap() == Some(entry.value()));
        let current: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(current["sentinel"], original["sentinel"]);
        assert_eq!(current["projects"], original["projects"]);
        assert_eq!(
            current["mcpServers"]["unrelated"],
            original["mcpServers"]["unrelated"]
        );
        edit(&path, Some(&entry), None).unwrap();
        let current: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(current, original);
    }
    #[test]
    fn conflicting_entries_and_existing_locks_are_never_overwritten_or_removed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".claude.json");
        let entry = entry();
        edit(&path, None, Some(&entry)).unwrap();
        assert_eq!(edit(&path, None, None), Err(Error::Conflict));
        let lock = directory.path().join(".claude.json.lock");
        std::fs::create_dir(&lock).unwrap();
        assert_eq!(edit(&path, Some(&entry), None), Err(Error::Busy));
        assert!(lock.is_dir());
        std::fs::remove_dir(lock).unwrap();
        assert!(inspect(&path).unwrap() == Some(entry.value()));
    }
    #[test]
    fn writer_racing_after_compare_is_restored_instead_of_losing_its_settings() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".claude.json");
        std::fs::write(&path, b"{\"original\":true}").unwrap();
        let concurrent = json!({"original":true,"concurrent":"keep"});
        let result = edit_observed(&path, None, Some(&entry()), |stage| {
            if matches!(stage, Stage::BeforePublish) {
                std::fs::write(&path, serde_json::to_vec(&concurrent).unwrap()).unwrap();
            }
        });
        assert_eq!(result, Err(Error::Conflict));
        let current: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(current, concurrent);
    }
    #[test]
    fn malformed_duplicate_or_symlinked_configuration_is_not_modified() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".claude.json");
        for content in [b"{".as_slice(), b"{\"same\":1,\"same\":2}".as_slice()] {
            std::fs::write(&path, content).unwrap();
            assert_eq!(edit(&path, None, Some(&entry())), Err(Error::Malformed));
            assert_eq!(std::fs::read(&path).unwrap(), content);
        }
        std::fs::remove_file(&path).unwrap();
        let target = directory.path().join("other.json");
        std::fs::write(&target, b"{}").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert_eq!(edit(&path, None, Some(&entry())), Err(Error::Unsafe));
        assert_eq!(std::fs::read(target).unwrap(), b"{}");
    }

    #[test]
    fn identical_entry_is_a_byte_preserving_noop_and_new_files_are_private() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".claude.json");
        let entry = entry();
        edit(&path, None, Some(&entry)).unwrap();
        let before = read(&path).unwrap().unwrap();
        assert_eq!(before.identity.2, 0o600);
        edit(&path, Some(&entry), Some(&entry)).unwrap();
        edit(&path, None, Some(&entry)).unwrap();
        assert!(same(Some(&before), read(&path).unwrap().as_ref()));
    }

    #[test]
    fn relocation_requires_exact_previous_entry_and_preserves_file_permissions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".claude.json");
        let previous = entry();
        edit(&path, None, Some(&previous)).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let next = Entry::new(Path::new("/synthetic/new remote"), None, previous.url()).unwrap();
        assert_eq!(edit(&path, None, Some(&next)), Err(Error::Conflict));
        edit(&path, Some(&previous), Some(&next)).unwrap();
        assert_eq!(inspect(&path).unwrap(), Some(next.value()));
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o640);
        assert_eq!(edit(&path, Some(&previous), None), Err(Error::Conflict));
    }

    #[test]
    fn initial_creation_never_replaces_a_racing_new_document() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".claude.json");
        let concurrent = b"{\"concurrent\":true}";
        let result = edit_observed(&path, None, Some(&entry()), |stage| {
            if matches!(stage, Stage::BeforePublish) {
                std::fs::write(&path, concurrent).unwrap();
            }
        });
        assert_eq!(result, Err(Error::Conflict));
        assert_eq!(std::fs::read(&path).unwrap(), concurrent);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn replaced_lock_is_preserved_and_cannot_authorize_publication() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".claude.json");
        let lock = directory.path().join(".claude.json.lock");
        let result = edit_observed(&path, None, Some(&entry()), |stage| {
            if matches!(stage, Stage::BeforeCompare) {
                std::fs::rename(&lock, directory.path().join("old-lock")).unwrap();
                std::fs::create_dir(&lock).unwrap();
            }
        });
        assert_eq!(result, Err(Error::Conflict));
        assert!(!path.exists());
        assert!(lock.is_dir());
    }

    #[test]
    fn unreadable_displaced_document_is_retained_with_a_recovery_path() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".claude.json");
        let target = directory.path().join("other.json");
        std::fs::write(&path, b"{}").unwrap();
        std::fs::write(&target, b"{\"keep\":true}").unwrap();
        let result = edit_observed(&path, None, Some(&entry()), |stage| {
            if matches!(stage, Stage::BeforePublish) {
                std::fs::remove_file(&path).unwrap();
                std::os::unix::fs::symlink(&target, &path).unwrap();
            }
        });
        let Err(Error::Recovery(recovery)) = result else {
            panic!("displaced document must be retained");
        };
        assert!(std::fs::symlink_metadata(recovery)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read(target).unwrap(), b"{\"keep\":true}");
    }

    #[test]
    fn unsafe_modes_hardlinks_and_nonregular_files_fail_without_changes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".claude.json");
        std::fs::write(&path, b"{}").unwrap();
        for mode in [0o666, 0o444] {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            assert_eq!(edit(&path, None, Some(&entry())), Err(Error::Unsafe));
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let linked = directory.path().join("hardlink");
        std::fs::hard_link(&path, &linked).unwrap();
        assert_eq!(edit(&path, None, Some(&entry())), Err(Error::Unsafe));
        assert_eq!(std::fs::read(&linked).unwrap(), b"{}");
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert_eq!(edit(&path, None, Some(&entry())), Err(Error::Unsafe));
    }

    #[test]
    fn entries_reject_unpinned_destinations_and_relative_helpers() {
        for url in [
            "https://example.invalid/mcp",
            "http://localhost:54321/mcp",
            "http://127.0.0.1:54321/mcp?redirect=true",
        ] {
            assert_eq!(
                Entry::new(Path::new("/synthetic/remote"), None, url),
                Err(Error::Unsafe)
            );
        }
        assert_eq!(
            Entry::new(Path::new("remote"), None, "http://127.0.0.1:54321/mcp"),
            Err(Error::Unsafe)
        );
        assert_eq!(
            Entry::new(
                Path::new("/synthetic/remote"),
                Some(Path::new("relative")),
                "http://127.0.0.1:54321/mcp"
            ),
            Err(Error::Unsafe)
        );
    }

    #[test]
    fn workflow_lock_serializes_connect_and_disconnect_until_last_guard_drops() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".claude.json");
        let lock = std::sync::Arc::new(workflow_lock(&path).unwrap());
        let pending_edit = lock.clone();
        drop(lock);
        assert_eq!(workflow_lock(&path).unwrap_err(), Error::Busy);
        drop(pending_edit);
        assert!(workflow_lock(&path).is_ok());
        let metadata =
            std::fs::metadata(directory.path().join(".claude.json.samsung-tv-remote.lock"))
                .unwrap();
        assert_eq!(metadata.mode() & 0o777, 0o600);
    }

    #[test]
    fn already_removed_entry_can_be_retried_without_creating_a_document() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(".claude.json");
        edit(&path, Some(&entry()), None).unwrap();
        assert!(!path.exists());
        std::fs::write(&path, b"{\"unrelated\":true}").unwrap();
        let before = read(&path).unwrap().unwrap();
        edit(&path, Some(&entry()), None).unwrap();
        assert!(same(Some(&before), read(&path).unwrap().as_ref()));
    }
}
