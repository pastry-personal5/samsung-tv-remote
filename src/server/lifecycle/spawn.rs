//! macOS spawn passes only a lock and listener, with no credentials in argv,
//! environment, output or inherited GUI descriptors.
use super::{endpoint::EndpointError, files::OwnershipLock};
use std::net::TcpListener;
use std::path::Path;

#[cfg(target_os = "macos")]
pub fn detached(
    executable: &Path,
    base: &Path,
    ownership: &OwnershipLock,
    listener: &TcpListener,
) -> Result<StartupReport, EndpointError> {
    use std::ffi::CString;
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    };
    let cstring = |bytes: &[u8]| CString::new(bytes).map_err(|_| EndpointError::Unavailable);
    let arguments = [
        cstring(executable.as_os_str().as_bytes())?,
        cstring(b"--server")?,
        cstring(b"--fake-profile")?,
        cstring(base.as_os_str().as_bytes())?,
        cstring(b"--inherited-owner")?,
    ];
    let mut argv: Vec<_> = arguments
        .iter()
        .map(|v| v.as_ptr() as *mut libc::c_char)
        .collect();
    argv.push(std::ptr::null_mut());
    let environment = [cstring(b"PATH=/usr/bin:/bin")?];
    let mut envp: Vec<_> = environment
        .iter()
        .map(|v| v.as_ptr() as *mut libc::c_char)
        .collect();
    envp.push(std::ptr::null_mut());
    let null = cstring(b"/dev/null")?;
    let duplicate = |fd| {
        // SAFETY: live source descriptor; allocate disjoint sources above all
        // child destinations so file-action ordering cannot overwrite a source.
        let fd = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10) };
        if fd < 0 {
            return Err(EndpointError::Unavailable);
        }
        // SAFETY: fcntl returned one owned duplicate.
        Ok(unsafe { std::fs::File::from_raw_fd(fd) })
    };
    let mut pipe = [0; 2];
    // SAFETY: pipe initializes two owned descriptors; set close-on-exec before
    // any spawn. Only the explicit child destination 5 survives exec.
    if unsafe { libc::pipe(pipe.as_mut_ptr()) } < 0 {
        return Err(EndpointError::Unavailable);
    }
    let read = unsafe { std::fs::File::from_raw_fd(pipe[0]) };
    let write = unsafe { std::fs::File::from_raw_fd(pipe[1]) };
    for fd in pipe {
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(EndpointError::Unavailable);
        }
    }
    let report = duplicate(write.as_raw_fd())?;
    let lock = duplicate(ownership.raw_fd())?;
    let socket = duplicate(listener.as_raw_fd())?;
    // SAFETY: spawn objects are initialized before use, pointers remain valid
    // until posix_spawn returns, and all initialized objects are destroyed.
    unsafe {
        let mut actions = std::mem::MaybeUninit::uninit();
        if libc::posix_spawn_file_actions_init(actions.as_mut_ptr()) != 0 {
            return Err(EndpointError::Unavailable);
        }
        let mut actions = actions.assume_init();
        let mut attributes = std::mem::MaybeUninit::uninit();
        if libc::posix_spawnattr_init(attributes.as_mut_ptr()) != 0 {
            libc::posix_spawn_file_actions_destroy(&mut actions);
            return Err(EndpointError::Unavailable);
        }
        let mut attributes = attributes.assume_init();
        let result = (|| {
            for (fd, flags) in [
                (0, libc::O_RDONLY),
                (1, libc::O_WRONLY),
                (2, libc::O_WRONLY),
            ] {
                if libc::posix_spawn_file_actions_addopen(&mut actions, fd, null.as_ptr(), flags, 0)
                    != 0
                {
                    return Err(EndpointError::Unavailable);
                }
            }
            if libc::posix_spawn_file_actions_adddup2(&mut actions, lock.as_raw_fd(), 3) != 0
                || libc::posix_spawn_file_actions_adddup2(&mut actions, socket.as_raw_fd(), 4) != 0
                || libc::posix_spawn_file_actions_adddup2(&mut actions, report.as_raw_fd(), 5) != 0
            {
                return Err(EndpointError::Unavailable);
            }
            for fd in [lock.as_raw_fd(), socket.as_raw_fd(), report.as_raw_fd()] {
                if libc::posix_spawn_file_actions_addclose(&mut actions, fd) != 0 {
                    return Err(EndpointError::Unavailable);
                }
            }
            // POSIX_SPAWN_SETSID is defined as 0x0400 in the macOS SDK's
            // sys/spawn.h; libc exposes CLOEXEC_DEFAULT but not SETSID yet.
            let flags = libc::POSIX_SPAWN_CLOEXEC_DEFAULT
                | 0x0400
                | libc::POSIX_SPAWN_SETSIGMASK
                | libc::POSIX_SPAWN_SETSIGDEF;
            let mut mask = std::mem::zeroed();
            libc::sigemptyset(&mut mask);
            let mut defaults = std::mem::zeroed();
            libc::sigemptyset(&mut defaults);
            for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
                libc::sigaddset(&mut defaults, signal);
            }
            if libc::posix_spawnattr_setflags(&mut attributes, flags as libc::c_short) != 0
                || libc::posix_spawnattr_setsigmask(&mut attributes, &mask) != 0
                || libc::posix_spawnattr_setsigdefault(&mut attributes, &defaults) != 0
            {
                return Err(EndpointError::Unavailable);
            }
            let mut pid = 0;
            if libc::posix_spawn(
                &mut pid,
                arguments[0].as_ptr(),
                &actions,
                &attributes,
                argv.as_ptr(),
                envp.as_ptr(),
            ) != 0
            {
                return Err(EndpointError::Unavailable);
            }
            // The parent may remain a GUI for hours; reap without owning the
            // child lifetime. Parent exit leaves the detached server running.
            std::thread::spawn(move || {
                let mut status = 0;
                while libc::waitpid(pid, &mut status, 0) < 0
                    && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
                {
                }
            });
            Ok(StartupReport(read))
        })();
        libc::posix_spawnattr_destroy(&mut attributes);
        libc::posix_spawn_file_actions_destroy(&mut actions);
        result
    }
}
#[cfg(not(target_os = "macos"))]
pub fn detached(
    _executable: &Path,
    _base: &Path,
    _ownership: &OwnershipLock,
    _listener: &TcpListener,
) -> Result<StartupReport, EndpointError> {
    Err(EndpointError::Unavailable)
}

pub struct StartupReport(std::fs::File);
impl StartupReport {
    pub async fn ready(self) -> Result<(), EndpointError> {
        tokio::task::spawn_blocking(move || {
            use std::io::Read;
            use std::os::fd::AsRawFd;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let mut bytes = Vec::new();
            let mut file = self.0;
            loop {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Err(EndpointError::Unavailable);
                }
                let mut poll = libc::pollfd {
                    fd: file.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                // SAFETY: one live pipe descriptor and a finite timeout.
                let result = unsafe {
                    libc::poll(
                        &mut poll,
                        1,
                        remaining.as_millis().min(i32::MAX as u128) as i32,
                    )
                };
                if result < 0
                    && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
                {
                    continue;
                }
                if result <= 0 {
                    return Err(EndpointError::Unavailable);
                }
                let mut buffer = [0_u8; 128];
                let count = file
                    .read(&mut buffer)
                    .map_err(|_| EndpointError::Unavailable)?;
                if count == 0 {
                    return Err(EndpointError::Unavailable);
                }
                if count > 128 - bytes.len() {
                    return Err(EndpointError::Unavailable);
                }
                bytes.extend_from_slice(&buffer[..count]);
                if bytes.last() == Some(&b'\n') {
                    break;
                }
                if bytes.len() == 128 {
                    return Err(EndpointError::Unavailable);
                }
            }
            let text = std::str::from_utf8(&bytes[..bytes.len() - 1])
                .map_err(|_| EndpointError::Unavailable)?;
            match text {
                "ready" => Ok(()),
                "invalid_lock_descriptor" => Err(EndpointError::InvalidLockDescriptor),
                "invalid_listener_descriptor" => Err(EndpointError::InvalidListenerDescriptor),
                "invalid_descriptor_flags" => Err(EndpointError::InvalidDescriptorFlags),
                "unsafe_metadata" => Err(EndpointError::UnsafeInheritance),
                "inherited_endpoint_mismatch" => Err(EndpointError::InheritedEndpoint),
                "inherited_build_mismatch" => Err(EndpointError::InheritedBuild),
                "unsafe_profile" => Err(EndpointError::RejectedProfile),
                _ => Err(EndpointError::Unavailable),
            }
        })
        .await
        .map_err(|_| EndpointError::Unavailable)?
    }
}
