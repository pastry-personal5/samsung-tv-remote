//! The auth-header mode has a narrow, explicit CLI and never selects a caller's
//! production root or interprets the URL as a profile path.
use samsung_tv_remote::server::entry::{self, Mode};
use std::ffi::OsString;

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn helper_mode_is_explicit_and_development_can_select_only_an_absolute_fake_profile() {
    assert_eq!(
        entry::parse(&args(&["--mcp-headers"])).unwrap(),
        Mode::McpHeaders { base: None }
    );
    assert_eq!(
        entry::parse(&args(&[
            "--mcp-headers",
            "--fake-profile",
            "/private/tmp/synthetic"
        ]))
        .unwrap(),
        Mode::McpHeaders {
            base: Some("/private/tmp/synthetic".into())
        }
    );
    for values in [
        vec!["--mcp-headers", "--profile", "/private/tmp/synthetic"],
        vec!["--mcp-headers", "--fake-profile", "relative"],
        vec!["--mcp-headers", "--url", "http://127.0.0.1:12345/mcp"],
        vec!["--mcp-headers", "--resume"],
        vec![
            "--mcp-headers",
            "--fake-profile",
            "/private/tmp/synthetic",
            "extra",
        ],
    ] {
        assert!(entry::parse(&args(&values)).is_err());
    }
}

#[test]
fn management_mode_has_strict_actions_keys_and_generations() {
    use samsung_tv_remote::server::mcp::management::Action;
    assert_eq!(
        entry::parse(&args(&["--mcp-management", "status"])).unwrap(),
        Mode::McpManagement {
            base: None,
            action: Action::Status
        }
    );
    assert_eq!(
        entry::parse(&args(&[
            "--mcp-management",
            "prepare",
            "--fake-profile",
            "/private/tmp/synthetic"
        ]))
        .unwrap(),
        Mode::McpManagement {
            base: Some("/private/tmp/synthetic".into()),
            action: Action::Prepare
        }
    );
    let key = uuid::Uuid::new_v4().to_string();
    assert!(matches!(
        entry::parse(&args(&["--mcp-management", "enable", "0", &key])),
        Ok(Mode::McpManagement {
            action: Action::Change { enabled: true, .. },
            ..
        })
    ));
    for values in [
        vec!["--mcp-management", "enable", "00", &key],
        vec!["--mcp-management", "disable", "0", "not-a-key"],
        vec!["--mcp-management", "resume"],
        vec![
            "--mcp-management",
            "prepare",
            "--profile",
            "/private/tmp/synthetic",
        ],
    ] {
        assert!(entry::parse(&args(&values)).is_err());
    }
}
