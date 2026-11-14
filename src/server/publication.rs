//! Cooperate with bundle replacement before opening profiles or dispatching modes.
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::OnceLock;

// Process lifetime, including GUI dispatch returning None. Close-on-exec keeps
// a spawned helper from retaining its parent's guard; each child takes its own.
static EXECUTABLE: OnceLock<Result<Option<File>, ()>> = OnceLock::new();

pub(super) fn enter() -> Result<(), ()> {
    EXECUTABLE
        .get_or_init(|| {
            let executable = std::env::current_exe().map_err(|_| ())?;
            if !bundled(&executable) {
                return Ok(None);
            }
            shared(&executable).map(Some).map_err(|_| ())
        })
        .as_ref()
        .map(|_| ())
        .map_err(|_| ())
}

fn bundled(executable: &Path) -> bool {
    executable
        .file_name()
        .is_some_and(|v| v == "samsung-tv-remote")
        && executable
            .parent()
            .is_some_and(|v| v.ends_with("Contents/MacOS"))
        && executable
            .ancestors()
            .nth(3)
            .is_some_and(|v| v.extension().is_some_and(|e| e == "app"))
}

fn shared(executable: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(executable)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("non-regular executable"));
    }
    // SAFETY: the descriptor remains owned by file. Nonblocking admission
    // fails before any service/profile side effect while publication is active.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_bundle_entrypoints_take_a_publication_guard() {
        assert!(bundled(Path::new(
            "/tmp/Test.app/Contents/MacOS/samsung-tv-remote"
        )));
        assert!(!bundled(Path::new("/tmp/target/debug/samsung-tv-remote")));
        assert!(!bundled(Path::new(
            "/tmp/Test/Contents/MacOS/samsung-tv-remote"
        )));
    }

    #[test]
    fn publication_excludes_launch_and_lifetime_guards_exclude_replacement() {
        let executable = tempfile::NamedTempFile::new().unwrap();
        let first = shared(executable.path()).unwrap();
        let second = shared(executable.path()).unwrap();
        let exclusive = File::open(executable.path()).unwrap();
        let lock = || unsafe { libc::flock(exclusive.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        assert_ne!(lock(), 0);
        drop(first);
        assert_ne!(lock(), 0);
        drop(second);
        assert_eq!(lock(), 0);
        assert!(shared(executable.path()).is_err());
        drop(exclusive);
        assert!(shared(executable.path()).is_ok());
    }
}
