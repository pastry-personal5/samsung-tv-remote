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
    McpHeaders {
        base: Option<PathBuf>,
    },
    McpManagement {
        base: Option<PathBuf>,
        action: super::mcp::management::Action,
    },
    ApiAccess {
        request: bool,
    },
    CredentialProbe {
        action: super::credential_probe::Action,
        id: String,
    },
    Probe {
        base: PathBuf,
    },
    Production {
        action: Action,
        inherited: bool,
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
    if arguments[0] == "--mcp-headers" {
        let base = match arguments {
            [_] => None,
            [_, flag, base] if flag == "--fake-profile" && PathBuf::from(base).is_absolute() => {
                Some(PathBuf::from(base))
            }
            _ => return Err(LaunchError::Denied),
        };
        return Ok(Mode::McpHeaders { base });
    }
    if arguments[0] == "--mcp-management" {
        use super::mcp::management::Action as McpAction;
        let (values, base) =
            if arguments.len() >= 3 && arguments[arguments.len() - 2] == "--fake-profile" {
                let base = PathBuf::from(&arguments[arguments.len() - 1]);
                if !base.is_absolute() {
                    return Err(LaunchError::Denied);
                }
                (&arguments[1..arguments.len() - 2], Some(base))
            } else {
                (&arguments[1..], None)
            };
        let action = match values {
            [value] if value == "status" => McpAction::Status,
            [value] if value == "prepare" => McpAction::Prepare,
            [value, generation, key] if value == "enable" || value == "disable" => {
                let generation = generation.to_str().ok_or(LaunchError::Denied)?;
                let parsed: u64 = generation.parse().map_err(|_| LaunchError::Denied)?;
                if parsed.to_string() != generation {
                    return Err(LaunchError::Denied);
                }
                let key = key.to_str().ok_or(LaunchError::Denied)?;
                let id = uuid::Uuid::parse_str(key).map_err(|_| LaunchError::Denied)?;
                if id.hyphenated().to_string() != key {
                    return Err(LaunchError::Denied);
                }
                McpAction::Change {
                    generation: parsed,
                    key: key.into(),
                    enabled: value == "enable",
                }
            }
            _ => return Err(LaunchError::Denied),
        };
        return Ok(Mode::McpManagement { base, action });
    }
    if arguments[0] == "--server-api-access" {
        return match arguments.get(1).and_then(|argument| argument.to_str()) {
            Some("--inspect") if arguments.len() == 2 => Ok(Mode::ApiAccess { request: false }),
            Some("--request-access") if arguments.len() == 2 => {
                Ok(Mode::ApiAccess { request: true })
            }
            _ => Err(LaunchError::Denied),
        };
    }
    if arguments[0] == "--server-credential-probe" {
        if arguments.len() != 3 {
            return Err(LaunchError::Denied);
        }
        let action = arguments[1]
            .to_str()
            .and_then(super::credential_probe::Action::parse)
            .ok_or(LaunchError::Denied)?;
        let id = arguments[2]
            .to_str()
            .filter(|id| super::credential_probe::account(id).is_some())
            .ok_or(LaunchError::Denied)?
            .to_owned();
        return Ok(Mode::CredentialProbe { action, id });
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
    if arguments.len() == 1
        || (arguments.len() == 2 && arguments[1] == "--inherited-owner" && action == Action::Server)
    {
        return Ok(Mode::Production {
            action,
            inherited: arguments.len() == 2,
        });
    }
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
/// One foreground MCP call. Only a success value reaches stdout; failures
/// print a stable error code to stderr.
fn foreground<F, T, E>(
    base: Option<PathBuf>,
    code: fn(E) -> &'static str,
    run: impl FnOnce(super::profile::Profile, PathBuf) -> F,
) -> i32
where
    F: std::future::Future<Output = Result<T, E>>,
    T: std::fmt::Display,
{
    let Ok(executable) = std::env::current_exe() else {
        return 1;
    };
    let profile = base.map_or(
        super::profile::Profile::Control,
        super::profile::Profile::Development,
    );
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return 1,
    };
    let result = runtime.block_on(run(profile, executable));
    // A timed-out read-only preflight may still own a blocking worker.
    // Do not let Runtime::drop extend Claude's helper execution deadline.
    runtime.shutdown_background();
    match result {
        Ok(value) => {
            println!("{value}");
            0
        }
        Err(error) => {
            eprintln!("{}", json!({"error":code(error)}));
            1
        }
    }
}
/// None selects GUI composition; management commands return
/// sanitized status only. The dedicated MCP header mode intentionally emits
/// one client Authorization object; TV pairing secrets are never printed.
pub fn dispatch(arguments: &[OsString]) -> Option<i32> {
    if super::publication::enter().is_err() {
        eprintln!("{}", json!({"error":"bundle_replacement_in_progress"}));
        return Some(1);
    }
    let mode = match parse(arguments) {
        Ok(Mode::Gui) => return None,
        Ok(mode) => mode,
        Err(error) => {
            eprintln!("{}", json!({"error":error.code()}));
            return Some(1);
        }
    };
    if let Mode::McpHeaders { base } = mode {
        let destination = std::env::var("CLAUDE_CODE_MCP_SERVER_URL").unwrap_or_default();
        return Some(foreground(
            base,
            super::mcp::helper::HelperError::code,
            |profile, executable| super::mcp::helper::headers(profile, executable, destination),
        ));
    }
    if let Mode::McpManagement { base, action } = mode {
        return Some(foreground(
            base,
            super::mcp::management::Error::code,
            |profile, executable| super::mcp::management::run(profile, executable, action),
        ));
    }
    if let Mode::ApiAccess { request } = mode {
        // A single foreground maintenance call, before Tokio, Iced, profile
        // access or server composition. Never reached by automatic recheck.
        return Some(
            match std::env::current_exe().and_then(|executable| {
                crate::infrastructure::macos::api_access::run(!request, &executable)
            }) {
                Ok(report) => {
                    let passed =
                        !request || (report["gui"] == "available" && report["mcp"] == "available");
                    println!("{report}");
                    if passed {
                        0
                    } else {
                        1
                    }
                }
                Err(_) => {
                    eprintln!("{}", json!({"error":"api_access_unavailable"}));
                    1
                }
            },
        );
    }
    if let Mode::CredentialProbe { action, id } = mode {
        return Some(match super::credential_probe::run(action, &id) {
            Ok((report, passed)) => {
                println!("{report}");
                if passed {
                    0
                } else {
                    1
                }
            }
            Err(error) => {
                eprintln!("{}", json!({"error":error}));
                1
            }
        });
    }
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
        println!("Samsung TV Remote\nDefault: open GUI\nProduction server: --server[|-start|-status|-stop|-resume|-restart] (signed executable required)\nDevelopment: --server[|-start|-status|-stop|-resume|-restart] --fake-profile <absolute-existing-parent>\nGUI probe: --gui-probe --fake-profile <absolute-existing-parent>\nForeground API access: --server-api-access --inspect|--request-access (Apple-signed executable required)\nSigned credential acceptance: --server-credential-probe create|read|update|delete <disposable-v4-uuid>");
        return Some(0);
    }
    let (action, selected, inherited) = match mode {
        Mode::Development {
            action,
            base,
            inherited,
        } => (
            action,
            super::profile::Profile::Development(base),
            inherited,
        ),
        Mode::Production { action, inherited } => {
            (action, super::profile::Profile::Control, inherited)
        }
        _ => unreachable!(),
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
            return match selected {
                super::profile::Profile::Development(base) => {
                    super::process::run(&base, inherited).await
                }
                super::profile::Profile::Control => super::process::run_production(inherited).await,
            }
            .map(|()| None);
        }
        let launcher = launch::Launcher::new(selected)?;
        let executable = std::env::current_exe().map_err(|_| LaunchError::Unavailable)?;
        let client = match action {
            Action::Start => launcher.start(&executable, Role::Gui).await?,
            Action::Status => {
                let build = super::lifecycle::endpoint::build_identity(&executable)
                    .map_err(LaunchError::Endpoint)?;
                launcher.attach(Role::Gui, Some(&build)).await?
            }
            Action::Resume => launcher.resume(&executable, Role::Gui).await?,
            Action::Restart => launcher.restart(&executable, Role::Gui).await?,
            Action::Stop => {
                let client = launcher.attach(Role::Gui, None).await?;
                return launcher.stop(&client).await.map(Some);
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
