//! Short owner-only Unix socket paths, derived from the entire isolated profile.
use super::BootstrapError;
use crate::infrastructure::private_files::{Namespace, RuntimeDirectory};
use sha2::{Digest, Sha256};
use std::os::unix::{
    ffi::OsStrExt,
    fs::{FileTypeExt, MetadataExt, PermissionsExt},
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::{UnixListener, UnixStream};

pub struct Location {
    pub profile: String,
    pub path: PathBuf,
    pub directory: Arc<RuntimeDirectory>,
}
pub fn uid() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}
impl Location {
    pub fn for_profile(base: &Path) -> Result<Self, BootstrapError> {
        Self::for_namespace(base, Namespace::Development)
    }
    pub fn for_namespace(base: &Path, namespace: Namespace) -> Result<Self, BootstrapError> {
        // Validate every component before canonicalizing for the binding hash.
        RuntimeDirectory::open(&base.join(namespace.name()))
            .map_err(|_| BootstrapError::UnsafeMetadata)?;
        let canonical = base
            .join(namespace.name())
            .canonicalize()
            .map_err(|_| BootstrapError::UnsafeMetadata)?;
        let hash: [u8; 32] = Sha256::digest(canonical.as_os_str().as_bytes()).into();
        let profile = super::hex(&hash);
        let prefix = match namespace {
            Namespace::Development => "m8",
            Namespace::Control => "control-v1",
        };
        let path = PathBuf::from(format!(
            "/private/tmp/str-{prefix}-{}-{}",
            uid(),
            &profile[..24]
        ));
        let directory =
            Arc::new(RuntimeDirectory::open(&path).map_err(|_| BootstrapError::UnsafeMetadata)?);
        Ok(Self {
            profile,
            path: path.join("bootstrap.sock"),
            directory,
        })
    }
    /// Caller holds the profile owner lock before removing any dead socket.
    pub async fn bind(&self) -> Result<UnixListener, BootstrapError> {
        self.bind_observed(|_| {}).await
    }
    async fn bind_observed(
        &self,
        before_publish: impl FnOnce(&Path),
    ) -> Result<UnixListener, BootstrapError> {
        match std::fs::symlink_metadata(&self.path) {
            Ok(_) => {
                validate(&self.path)?;
                match tokio::time::timeout(
                    std::time::Duration::from_millis(200),
                    UnixStream::connect(&self.path),
                )
                .await
                {
                    Ok(Err(error))
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                        ) =>
                    {
                        std::fs::remove_file(&self.path)
                            .map_err(|_| BootstrapError::UnsafeMetadata)?;
                    }
                    _ => return Err(BootstrapError::Busy),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(BootstrapError::UnsafeMetadata),
        }
        // Publish only a fully checked socket. Binding directly at the public
        // name exposes a brief umask-dependent mode before chmod; a concurrent
        // legitimate launcher would mistake that initialization for corruption.
        let temporary = self
            .path
            .with_file_name(format!(".{}.sock", uuid::Uuid::new_v4()));
        let listener = UnixListener::bind(&temporary).map_err(|_| BootstrapError::Unavailable)?;
        let published = (|| {
            std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| BootstrapError::UnsafeMetadata)?;
            validate(&temporary)?;
            before_publish(&temporary);
            publish(&temporary, &self.path)?;
            Ok(listener)
        })();
        if published.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        published
    }
}
// The profile lock excludes cooperating owners; exclusive publication also
// preserves an unrelated socket that appears after the initial stale check.
#[cfg(target_os = "macos")]
fn publish(temporary: &Path, target: &Path) -> Result<(), BootstrapError> {
    use std::ffi::CString;
    let source = CString::new(temporary.as_os_str().as_bytes())
        .map_err(|_| BootstrapError::UnsafeMetadata)?;
    let target =
        CString::new(target.as_os_str().as_bytes()).map_err(|_| BootstrapError::UnsafeMetadata)?;
    // SAFETY: both NUL-terminated names stay live; RENAME_EXCL never replaces a
    // concurrently published target. Both entries are in the checked directory.
    if unsafe {
        libc::renameatx_np(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            target.as_ptr(),
            libc::RENAME_EXCL,
        )
    } < 0
    {
        return Err(
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
                BootstrapError::Busy
            } else {
                BootstrapError::Unavailable
            },
        );
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn publish(_temporary: &Path, _target: &Path) -> Result<(), BootstrapError> {
    Err(BootstrapError::Unavailable)
}
pub fn validate(path: &Path) -> Result<(), BootstrapError> {
    let parent = path.parent().ok_or(BootstrapError::UnsafeMetadata)?;
    RuntimeDirectory::open(parent).map_err(|_| BootstrapError::UnsafeMetadata)?;
    let metadata = std::fs::symlink_metadata(path).map_err(|_| BootstrapError::Unavailable)?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != uid()
        || metadata.permissions().mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(BootstrapError::UnsafeMetadata);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn public_socket_appears_only_after_mode_validation_and_a_live_socket_is_never_replaced()
    {
        let base = tempfile::tempdir().unwrap();
        let base_path = base.path().canonicalize().unwrap();
        let (_profile, _owner) =
            crate::server::development::DevelopmentProfile::open(&base_path).unwrap();
        let location = Location::for_profile(&base_path).unwrap();
        let listener = location
            .bind_observed(|temporary| {
                assert!(!location.path.exists());
                validate(temporary).unwrap();
                assert_eq!(
                    std::fs::metadata(temporary).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            })
            .await
            .unwrap();
        validate(&location.path).unwrap();
        let inode = std::fs::symlink_metadata(&location.path).unwrap().ino();
        assert!(matches!(location.bind().await, Err(BootstrapError::Busy)));
        assert_eq!(
            std::fs::symlink_metadata(&location.path).unwrap().ino(),
            inode
        );
        let (stream, _) = listener.accept().await.unwrap();
        drop(stream);
        drop(listener);
        std::fs::remove_file(&location.path).unwrap();
        let occupant = std::sync::Mutex::new(None);
        let raced = location
            .bind_observed(|_| {
                let listener = UnixListener::bind(&location.path).unwrap();
                std::fs::set_permissions(&location.path, std::fs::Permissions::from_mode(0o600))
                    .unwrap();
                *occupant.lock().unwrap() = Some(listener);
            })
            .await;
        assert!(matches!(raced, Err(BootstrapError::Busy)));
        validate(&location.path).unwrap();
        drop(occupant.into_inner().unwrap());
        std::fs::remove_file(&location.path).unwrap();
        std::fs::remove_dir(location.path.parent().unwrap()).unwrap();
    }
}
