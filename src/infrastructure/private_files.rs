//! Owner-only runtime metadata. Resolve components through directory handles so
//! path replacement and symlinks cannot redirect reads or atomic writes.
use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path};

pub const DEVELOPMENT_NAMESPACE: &str = "m8-fake-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileError {
    UnsafePath,
    Unavailable,
    Locked,
    TooLarge,
}

fn name(value: &OsStr) -> Result<CString, FileError> {
    CString::new(value.as_bytes()).map_err(|_| FileError::UnsafePath)
}
fn file_name(value: &str) -> Result<CString, FileError> {
    if value.is_empty() || value.contains('/') || value == "." || value == ".." {
        return Err(FileError::UnsafePath);
    }
    name(OsStr::new(value))
}
fn opened(fd: libc::c_int) -> Result<File, FileError> {
    if fd < 0 {
        return Err(FileError::Unavailable);
    }
    // SAFETY: successful open/openat returns one newly owned file descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn validate(file: &File, directory: bool) -> Result<(), FileError> {
    validate_metadata(
        &file.metadata().map_err(|_| FileError::Unavailable)?,
        directory,
    )
}
fn validate_metadata(metadata: &std::fs::Metadata, directory: bool) -> Result<(), FileError> {
    // SAFETY: geteuid has no preconditions.
    if metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o7777 != if directory { 0o700 } else { 0o600 }
        || (directory && !metadata.is_dir())
        || (!directory && (!metadata.is_file() || metadata.nlink() != 1))
    {
        return Err(FileError::UnsafePath);
    }
    Ok(())
}

pub struct RuntimeDirectory {
    directory: File,
}
pub struct OwnershipLock {
    _file: File,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReplaceStage {
    Created,
    Written,
    FileSynced,
    Renamed,
    Committed,
}
impl OwnershipLock {
    pub(crate) fn raw_fd(&self) -> std::os::fd::RawFd {
        self._file.as_raw_fd()
    }
}

impl RuntimeDirectory {
    pub fn child(&self, value: &str) -> Result<Self, FileError> {
        self.child_mode(value, false)
    }
    pub fn new_child(&self, value: &str) -> Result<Self, FileError> {
        self.child_mode(value, true)
    }
    fn child_mode(&self, value: &str, exclusive: bool) -> Result<Self, FileError> {
        let value = file_name(value)?;
        // SAFETY: resolve one name relative to the held directory descriptor.
        let result = unsafe { libc::mkdirat(self.directory.as_raw_fd(), value.as_ptr(), 0o700) };
        if result < 0
            && (exclusive || std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST))
        {
            return Err(FileError::Unavailable);
        }
        // SAFETY: directory/name are valid; no symlink is followed.
        let file = opened(unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                value.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        validate(&file, true)?;
        Ok(Self { directory: file })
    }
    /// The parent chain must exist. Only the final directory may be created;
    /// no component is followed through a symlink, including on first launch.
    pub fn open(path: &Path) -> Result<Self, FileError> {
        if !path.is_absolute() {
            return Err(FileError::UnsafePath);
        }
        let components: Vec<_> = path.components().collect();
        if components.len() < 2 {
            return Err(FileError::UnsafePath);
        }
        let root = CString::new("/").expect("root");
        // SAFETY: path pointer is terminated; flags require no mode argument.
        let mut parent = opened(unsafe {
            libc::open(
                root.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        })?;
        for (index, component) in components.iter().enumerate().skip(1) {
            let Component::Normal(component) = component else {
                return Err(FileError::UnsafePath);
            };
            let component = name(component)?;
            let last = index == components.len() - 1;
            if last {
                // SAFETY: parent descriptor is a live directory and name valid.
                let result =
                    unsafe { libc::mkdirat(parent.as_raw_fd(), component.as_ptr(), 0o700) };
                if result < 0
                    && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST)
                {
                    return Err(FileError::Unavailable);
                }
            }
            // SAFETY: openat resolves only this single component; no links.
            let next = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    component.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if next < 0 {
                return Err(FileError::UnsafePath);
            }
            parent = opened(next)?;
        }
        validate(&parent, true)?;
        Ok(Self { directory: parent })
    }

    fn open_file(&self, value: &str, create: bool) -> Result<Option<File>, FileError> {
        let value = file_name(value)?;
        let flags = libc::O_RDWR
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | libc::O_NONBLOCK
            | if create { libc::O_CREAT } else { 0 };
        for _ in 0..8 {
            // SAFETY: directory and name are valid; mode is passed with O_CREAT.
            let fd =
                unsafe { libc::openat(self.directory.as_raw_fd(), value.as_ptr(), flags, 0o600) };
            if fd < 0 {
                let error = std::io::Error::last_os_error().raw_os_error();
                if error == Some(libc::ENOENT) {
                    if create {
                        // A concurrent first creator can invalidate Darwin's
                        // negative name lookup. Re-resolve the same checked
                        // directory/name; never relax O_NOFOLLOW or permissions.
                        continue;
                    }
                    return Ok(None);
                }
                return Err(FileError::UnsafePath);
            }
            let file = opened(fd)?;
            let metadata = file.metadata().map_err(|_| FileError::Unavailable)?;
            if metadata.is_file() && metadata.nlink() == 0 {
                // Atomic replacement may unlink the old inode between openat
                // and fstat. Retry the current name, still validating its owner,
                // mode and link count before returning any content.
                continue;
            }
            validate_metadata(&metadata, false)?;
            return Ok(Some(file));
        }
        Err(FileError::Unavailable)
    }

    pub fn lock(&self) -> Result<OwnershipLock, FileError> {
        let file = self
            .open_file("owner.lock", true)?
            .ok_or(FileError::Unavailable)?;
        // SAFETY: descriptor is live; nonblocking exclusive advisory lock.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } < 0 {
            return Err(FileError::Locked);
        }
        Ok(OwnershipLock { _file: file })
    }
    /// Adopt the launcher-passed descriptor only if it names this directory's
    /// checked lock inode. The same open file description keeps flock held
    /// across spawn, so concurrent launchers cannot create another owner.
    pub fn adopt_lock(&self, file: File) -> Result<OwnershipLock, FileError> {
        validate(&file, false)?;
        let expected = self
            .open_file("owner.lock", false)?
            .ok_or(FileError::UnsafePath)?;
        let left = file.metadata().map_err(|_| FileError::Unavailable)?;
        let right = expected.metadata().map_err(|_| FileError::Unavailable)?;
        if left.dev() != right.dev() || left.ino() != right.ino() {
            return Err(FileError::UnsafePath);
        }
        // SAFETY: the descriptor is live and validated, flags/lock are local.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0
            || unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } < 0
        {
            return Err(FileError::Locked);
        }
        Ok(OwnershipLock { _file: file })
    }

    pub fn read(&self, value: &str, cap: usize) -> Result<Option<Vec<u8>>, FileError> {
        let Some(file) = self.open_file(value, false)? else {
            return Ok(None);
        };
        let length = file.metadata().map_err(|_| FileError::Unavailable)?.len();
        if length > cap as u64 {
            return Err(FileError::TooLarge);
        }
        let mut bytes = Vec::new();
        file.take(cap as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| FileError::Unavailable)?;
        if bytes.len() > cap {
            return Err(FileError::TooLarge);
        }
        Ok(Some(bytes))
    }

    pub fn replace(&self, value: &str, bytes: &[u8]) -> Result<(), FileError> {
        self.replace_observed(value, bytes, |_| {})
    }
    // Private checkpoint hook lets subprocess tests terminate the actual write
    // path without introducing environment switches or fault modes in the app.
    fn replace_observed(
        &self,
        value: &str,
        bytes: &[u8],
        checkpoint: impl Fn(ReplaceStage),
    ) -> Result<(), FileError> {
        // Refuse unsafe existing targets instead of silently repairing them.
        self.open_file(value, false)?;
        let target = file_name(value)?;
        let temporary = file_name(&format!(".{}.tmp", uuid::Uuid::new_v4()))?;
        // SAFETY: unique, single-component name in an already checked directory.
        let mut file = opened(unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                temporary.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        })?;
        checkpoint(ReplaceStage::Created);
        let result = (|| {
            file.write_all(bytes).map_err(|_| FileError::Unavailable)?;
            checkpoint(ReplaceStage::Written);
            file.sync_all().map_err(|_| FileError::Unavailable)?;
            checkpoint(ReplaceStage::FileSynced);
            // SAFETY: both names are within the held directory handle.
            if unsafe {
                libc::renameat(
                    self.directory.as_raw_fd(),
                    temporary.as_ptr(),
                    self.directory.as_raw_fd(),
                    target.as_ptr(),
                )
            } < 0
            {
                return Err(FileError::Unavailable);
            }
            checkpoint(ReplaceStage::Renamed);
            self.directory
                .sync_all()
                .map_err(|_| FileError::Unavailable)?;
            checkpoint(ReplaceStage::Committed);
            Ok(())
        })();
        if result.is_err() {
            // SAFETY: only our randomly named temporary entry is removed.
            unsafe {
                libc::unlinkat(self.directory.as_raw_fd(), temporary.as_ptr(), 0);
            }
        }
        result
    }

    pub fn remove(&self, value: &str) -> Result<(), FileError> {
        if self.open_file(value, false)?.is_none() {
            return Ok(());
        }
        let value = file_name(value)?;
        // SAFETY: single-component name in the checked directory.
        if unsafe { libc::unlinkat(self.directory.as_raw_fd(), value.as_ptr(), 0) } < 0 {
            return Err(FileError::Unavailable);
        }
        self.directory
            .sync_all()
            .map_err(|_| FileError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::lifecycle::stop::{FileStopStore, StopStore};
    use std::sync::Arc;
    const NEXT:&[u8]=br#"{"version":1,"profile":"m8-fake-v1","server_epoch":"e1111111-1111-4111-8111-111111111111","operation_key":"a2222222-2222-4222-8222-222222222222","identity":"gui"}"#;
    const OLD:&[u8]=br#"{"version":1,"profile":"m8-fake-v1","server_epoch":"e1111111-1111-4111-8111-111111111111","operation_key":"b3333333-3333-4333-8333-333333333333","identity":"gui"}"#;

    #[test]
    fn atomic_write_crash_child() {
        let Some(path) = std::env::var_os("STR_TEST_CRASH_DIRECTORY") else {
            return;
        };
        let stage = match std::env::var("STR_TEST_CRASH_STAGE").unwrap().as_str() {
            "created" => ReplaceStage::Created,
            "written" => ReplaceStage::Written,
            "file_synced" => ReplaceStage::FileSynced,
            "renamed" => ReplaceStage::Renamed,
            "committed" => ReplaceStage::Committed,
            _ => panic!("fixture stage"),
        };
        let directory = RuntimeDirectory::open(Path::new(&path)).unwrap();
        let _owner = directory.lock().unwrap();
        directory
            .replace_observed("stopped.json", NEXT, |reached| {
                if reached == stage {
                    // Abrupt exit deliberately skips Rust destructors, temporary
                    // cleanup and normal shutdown. The kernel closes the lock.
                    unsafe { libc::_exit(91) }
                }
            })
            .unwrap();
        panic!("crash checkpoint was not reached");
    }
    #[test]
    fn crashes_at_each_atomic_write_boundary_keep_only_complete_authoritative_markers() {
        for previous in [false, true] {
            for stage in ["created", "written", "file_synced", "renamed", "committed"] {
                let base = tempfile::tempdir().unwrap();
                let path = base.path().canonicalize().unwrap().join("runtime");
                let directory = Arc::new(RuntimeDirectory::open(&path).unwrap());
                if previous {
                    directory.replace("stopped.json", OLD).unwrap();
                }
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "infrastructure::private_files::tests::atomic_write_crash_child",
                        "--nocapture",
                    ])
                    .env("STR_TEST_CRASH_DIRECTORY", &path)
                    .env("STR_TEST_CRASH_STAGE", stage)
                    .output()
                    .unwrap();
                assert_eq!(
                    output.status.code(),
                    Some(91),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let _replacement_owner = directory
                    .lock()
                    .expect("crash must close ownership descriptor");
                let committed = matches!(stage, "renamed" | "committed");
                let expected = if committed {
                    Some(NEXT)
                } else if previous {
                    Some(OLD)
                } else {
                    None
                };
                assert_eq!(
                    directory.read("stopped.json", 4096).unwrap().as_deref(),
                    expected
                );
                let marker = FileStopStore(directory.clone()).read().unwrap();
                assert_eq!(marker.is_some(), committed || previous);
                if let Some(marker) = marker {
                    assert_eq!(
                        marker.operation_key,
                        if committed {
                            "a2222222-2222-4222-8222-222222222222"
                        } else {
                            "b3333333-3333-4333-8333-333333333333"
                        }
                    );
                }
                // Leftover temporary entries never become lifecycle authority.
                for entry in std::fs::read_dir(&path).unwrap() {
                    let entry = entry.unwrap();
                    if entry.file_name().to_string_lossy().ends_with(".tmp") {
                        assert_eq!(
                            entry.metadata().unwrap().permissions().mode() & 0o777,
                            0o600
                        );
                    }
                }
            }
        }
    }
}
