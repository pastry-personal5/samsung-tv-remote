//! Parse/dispatch process modes before constructing Iced or production adapters.
use super::{
    bootstrap::Role,
    launch::{self, LaunchError},
};
use serde_json::json;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Server,
    Start,
    Status,
    Stop,
    Resume,
    Restart,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Gui,
    Help,
    Probe {
        base: PathBuf,
    },
    Development {
        action: Action,
        base: PathBuf,
        inherited: bool,
    },
}
pub fn parse(arguments: &[OsString]) -> Result<Mode, LaunchError> {
    if arguments.is_empty() {
        return Ok(Mode::Gui);
    }
    if arguments.len() == 1 && arguments[0] == "--help" {
        return Ok(Mode::Help);
    }
    if arguments[0] == "--gui-probe" {
        if arguments.len() != 3 || arguments[1] != "--fake-profile" {
            return Err(LaunchError::Denied);
        }
        let base = PathBuf::from(&arguments[2]);
        if !base.is_absolute() {
            return Err(LaunchError::UnsafeMetadata);
        }
        return Ok(Mode::Probe { base });
    }
    let action = match arguments[0].to_str() {
        Some("--server") => Action::Server,
        Some("--server-start") => Action::Start,
        Some("--server-status") => Action::Status,
        Some("--server-stop") => Action::Stop,
        Some("--server-resume") => Action::Resume,
        Some("--server-restart") => Action::Restart,
        _ => return Err(LaunchError::Denied),
    };
    if !matches!(arguments.len(), 3 | 4) || arguments[1] != "--fake-profile" {
        return Err(LaunchError::Denied);
    }
    let base = PathBuf::from(&arguments[2]);
    if !base.is_absolute() {
        return Err(LaunchError::UnsafeMetadata);
    }
    let inherited = arguments.len() == 4;
    if inherited && (action != Action::Server || arguments[3] != "--inherited-owner") {
        return Err(LaunchError::Denied);
    }
    Ok(Mode::Development {
        action,
        base,
        inherited,
    })
}
/// None selects the unchanged GUI composition; development commands return
/// sanitized status only. API and pairing secrets are never printed.
pub fn dispatch(arguments: &[OsString]) -> Option<i32> {
    let mode = match parse(arguments) {
        Ok(Mode::Gui) => return None,
        Ok(mode) => mode,
        Err(error) => {
            eprintln!("{}", json!({"error":error.code()}));
            return Some(1);
        }
    };
    if let Mode::Probe { base } = mode {
        let Ok(executable) = std::env::current_exe() else {
            return Some(1);
        };
        return Some(
            if crate::presentation::iced::app::run_probe(
                crate::control_client::background::Config { executable, base },
            )
            .is_ok()
            {
                0
            } else {
                1
            },
        );
    }
    if mode == Mode::Help {
        println!("Samsung TV Remote\nDefault: open GUI\nDevelopment: --server[|-start|-status|-stop|-resume|-restart] --fake-profile <absolute-existing-parent>\nGUI probe: --gui-probe --fake-profile <absolute-existing-parent>");
        return Some(0);
    }
    let Mode::Development {
        action,
        base,
        inherited,
    } = mode
    else {
        unreachable!()
    };
    let inherited = if inherited {
        match inherited_handles() {
            Ok(handles) => Some(handles),
            Err(error) => {
                eprintln!("{}", json!({"error":error.code()}));
                return Some(1);
            }
        }
    } else {
        None
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return Some(1),
    };
    let result = runtime.block_on(async {
        if action == Action::Server {
            return super::process::run(&base, inherited).await.map(|()| None);
        }
        let executable = std::env::current_exe().map_err(|_| LaunchError::Unavailable)?;
        let client = match action {
            Action::Start => launch::start(&executable, &base, Role::Gui).await?,
            Action::Status => {
                let build = super::lifecycle::endpoint::build_identity(&executable)
                    .map_err(LaunchError::Endpoint)?;
                launch::attach(&base, Role::Gui, Some(&build)).await?
            }
            Action::Resume => launch::resume(&executable, &base, Role::Gui).await?,
            Action::Restart => launch::restart(&executable, &base, Role::Gui).await?,
            Action::Stop => {
                let client = launch::attach(&base, Role::Gui, None).await?;
                return launch::stop(&base, &client).await.map(Some);
            }
            Action::Server => unreachable!(),
        };
        let (status, ready) = client
            .call("GET", "/v1/ready", None)
            .await
            .map_err(LaunchError::Client)?;
        if status != 200 {
            return Err(LaunchError::Unavailable);
        }
        Ok(Some(ready))
    });
    match result {
        Ok(Some(value)) => {
            println!("{value}");
            Some(0)
        }
        Ok(None) => Some(0),
        Err(error) => {
            eprintln!("{}", json!({"error":error.code()}));
            Some(1)
        }
    }
}
fn inherited_handles() -> Result<super::process::Inherited, LaunchError> {
    use std::os::fd::FromRawFd;
    // SAFETY: inspect inherited descriptor kinds before taking ownership, and
    // before starting Tokio (whose internal descriptors could otherwise be 3/4).
    unsafe {
        let mut report_metadata = std::mem::zeroed();
        if libc::fstat(5, &mut report_metadata) != 0
            || report_metadata.st_mode & libc::S_IFMT != libc::S_IFIFO
        {
            return Err(LaunchError::UnsafeMetadata);
        }
        let mut report = std::fs::File::from_raw_fd(5);
        let validated = (|| {
            let mut metadata = std::mem::zeroed();
            if libc::fstat(3, &mut metadata) != 0
                || metadata.st_mode & libc::S_IFMT != libc::S_IFREG
            {
                return Err(LaunchError::InvalidLockDescriptor);
            }
            let mut socket_type = 0;
            let mut length = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
            let socket_result = libc::getsockopt(
                4,
                libc::SOL_SOCKET,
                libc::SO_TYPE,
                &mut socket_type as *mut _ as *mut libc::c_void,
                &mut length,
            );
            if socket_result != 0 || socket_type != libc::SOCK_STREAM {
                return Err(LaunchError::InvalidListenerDescriptor);
            }
            if libc::fcntl(3, libc::F_SETFD, libc::FD_CLOEXEC) < 0
                || libc::fcntl(4, libc::F_SETFD, libc::FD_CLOEXEC) < 0
                || libc::fcntl(5, libc::F_SETFD, libc::FD_CLOEXEC) < 0
            {
                return Err(LaunchError::InvalidDescriptorFlags);
            }
            Ok((
                std::fs::File::from_raw_fd(3),
                std::net::TcpListener::from_raw_fd(4),
            ))
        })();
        match validated {
            Ok((owner, listener)) => Ok(super::process::Inherited {
                owner,
                listener,
                report: Some(report),
            }),
            Err(error) => {
                use std::io::Write;
                let _ = report.write_all(format!("{}\n", error.code()).as_bytes());
                Err(error)
            }
        }
    }
}
