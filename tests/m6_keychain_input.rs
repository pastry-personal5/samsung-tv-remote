//! Exercise the real Swift argument boundary without reading or changing Keychain.
#![cfg(target_os = "macos")]

use std::process::Command;

use samsung_tv_remote::domain::DeviceId;

#[test]
fn probe_accepts_persisted_device_ids_and_rejects_unsafe_inputs_before_keychain() {
    let root = tempfile::tempdir().unwrap();
    let probe = root.path().join("probe");
    let compile = Command::new("swiftc")
        .args(["-module-cache-path"])
        .arg(root.path().join("module-cache"))
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/m6_keychain_probe.swift"
        ))
        .arg("-o")
        .arg(&probe)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let username = Command::new("id").arg("-un").output().unwrap();
    assert!(username.status.success());
    let username = String::from_utf8(username.stdout).unwrap();
    let saved = DeviceId::new(42).to_string();
    let maximum = format!("dev_{}", u128::MAX);
    for id in ["dev_0", &saved, &maximum] {
        let output = Command::new(&probe)
            .args(["validate-input", id])
            .env("M6_ISOLATED_KEYCHAIN_ACCOUNT", username.trim())
            .env("M6_SYNTHETIC_DEVICE_ID", id)
            .output()
            .unwrap();
        assert!(output.status.success(), "{id}: {output:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            "input: valid (Keychain not accessed)"
        );
    }
    // Every rejected case exits at the argument/isolation boundary, including
    // destructive requests. No test issues a valid read or mutation.
    let invalid_ids = [
        "",
        "00000000-0000-4000-8000-000000000001",
        "dev_",
        "dev_-1",
        "dev_+1",
        "dev_01",
        "dev_340282366920938463463374607431768211456",
        "dev_9999999999999999999999999999999999999999",
        "dev_４２",
        "DEV_42",
        "dev_42 ",
        "dev_42\n",
    ];
    let other_account = format!("{}-different-account", username.trim());
    for operation in ["read", "replace-and-delete", "validate-input"] {
        for id in invalid_ids {
            let output = Command::new(&probe)
                .args([operation, id])
                .env("M6_ISOLATED_KEYCHAIN_ACCOUNT", username.trim())
                .env("M6_SYNTHETIC_DEVICE_ID", id)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(2), "{output:?}");
            assert!(String::from_utf8_lossy(&output.stderr).contains("invalid device ID"));
            assert!(output.stdout.is_empty(), "{output:?}");
        }
        for (account, device, diagnostic) in [
            (None, Some("dev_42"), "M6_ISOLATED_KEYCHAIN_ACCOUNT"),
            (Some(""), Some("dev_42"), "M6_ISOLATED_KEYCHAIN_ACCOUNT"),
            (
                Some(other_account.as_str()),
                Some("dev_42"),
                "M6_ISOLATED_KEYCHAIN_ACCOUNT",
            ),
            (Some(username.trim()), None, "M6_SYNTHETIC_DEVICE_ID"),
            (
                Some(username.trim()),
                Some("dev_43"),
                "M6_SYNTHETIC_DEVICE_ID",
            ),
        ] {
            let mut command = Command::new(&probe);
            command
                .args([operation, "dev_42"])
                .env_remove("M6_ISOLATED_KEYCHAIN_ACCOUNT")
                .env_remove("M6_SYNTHETIC_DEVICE_ID")
                .env_remove("M6_SYNTHETIC_DEVICE_UUID");
            if let Some(account) = account {
                command.env("M6_ISOLATED_KEYCHAIN_ACCOUNT", account);
            }
            if let Some(device) = device {
                command.env("M6_SYNTHETIC_DEVICE_ID", device);
            }
            let output = command.output().unwrap();
            assert_eq!(output.status.code(), Some(3), "{output:?}");
            assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
            assert!(output.stdout.is_empty(), "{output:?}");
        }
    }
    for arguments in [
        vec![],
        vec!["read"],
        vec!["invalid-operation", "dev_42"],
        vec!["validate-input", "dev_42", "extra"],
    ] {
        let output = Command::new(&probe).args(arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
        assert!(output.stdout.is_empty(), "{output:?}");
    }
    let output = Command::new(&probe)
        .args(["read", "dev_42"])
        .env("M6_ISOLATED_KEYCHAIN_ACCOUNT", username.trim())
        .env_remove("M6_SYNTHETIC_DEVICE_ID")
        .env("M6_SYNTHETIC_DEVICE_UUID", "dev_42")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("M6_SYNTHETIC_DEVICE_UUID is obsolete; export M6_SYNTHETIC_DEVICE_ID"));
    assert!(output.stdout.is_empty(), "{output:?}");
}

#[test]
fn runbook_stops_before_read_when_preflight_fails() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let probe = root.path().join("fake-probe");
    fs::write(
        &probe,
        "#!/bin/zsh\nif [[ $1 == validate-input ]]; then exit 2; fi\nprint 'unexpected Keychain access'\n",
    )
    .unwrap();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();
    let runbook = include_str!("../docs/phase-2/milestone-06-human-runbook.md");
    let start = runbook.find("\"$probe\" validate-input").unwrap();
    let block = runbook[start..].split("```").next().unwrap();
    let output = Command::new("/bin/zsh")
        .args(["-c", block])
        .env("probe", &probe)
        .env("M6_SYNTHETIC_DEVICE_ID", "dev_42")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "exit=2");
    assert!(output.stderr.is_empty(), "{output:?}");
}
