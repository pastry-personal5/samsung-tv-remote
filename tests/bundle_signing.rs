//! Bundle publication uses fake tools and never touches real signing keys.
#![cfg(target_os = "macos")]

use std::{fs, os::unix::fs::PermissionsExt, process::Command};

fn executable(path: &std::path::Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn rejected_signers_preserve_existing_bundle_and_apple_signer_publishes() {
    let root = tempfile::tempdir().unwrap();
    for directory in ["scripts", "bin", "target/release"] {
        fs::create_dir_all(root.path().join(directory)).unwrap();
    }
    fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/build_macos_bundle.sh"),
        root.path().join("scripts/build_macos_bundle.sh"),
    )
    .unwrap();
    fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/install_macos_bundle.py"
        ),
        root.path().join("scripts/install_macos_bundle.py"),
    )
    .unwrap();
    fs::write(root.path().join("target/release/samsung-tv-remote"), "new").unwrap();
    let bundle = root.path().join("target/bundle/Samsung TV Remote.app");
    fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
    let binary = bundle.join("Contents/MacOS/samsung-tv-remote");
    fs::write(&binary, "approved-old").unwrap();
    let bundle_guard_path = bundle.canonicalize().unwrap();
    executable(&root.path().join("bin/cargo"), "#!/bin/zsh\nexit 0\n");
    executable(
        &root.path().join("bin/ps"),
        r#"#!/bin/zsh
if [[ "${BUNDLE_GUARD_ACTIVE:-}" == yes ]]; then
    printf '501 424242 Thu Nov 12 22:11:43 2026 %s/Contents/MacOS/samsung-tv-remote\n' "$BUNDLE_GUARD_PATH"
fi
exit 0
"#,
    );
    executable(
        &root.path().join("bin/codesign"),
        r#"#!/bin/zsh
set -eu
case "$1" in
    --force) [[ "$SAMSUNG_TV_CODESIGN_IDENTITY" != sign-failure ]] ;;
    --verify)
        [[ "$*" == *'-R=anchor apple generic'* ]]
        [[ "$SAMSUNG_TV_CODESIGN_IDENTITY" != self-signed ]] ;;
    --display)
        if [[ "$SAMSUNG_TV_CODESIGN_IDENTITY" == no-team ]]; then
            print 'TeamIdentifier=not set'
        else
            print 'TeamIdentifier=ABCDEFGHIJ'
        fi ;;
    *) exit 1 ;;
esac
"#,
    );
    for signer in [
        "running",
        "sign-failure",
        "self-signed",
        "no-team",
        "apple",
        "apple-fresh",
    ] {
        if signer == "apple-fresh" {
            fs::remove_dir_all(root.path().join("target/bundle")).unwrap();
        }
        let accepted = signer.starts_with("apple");
        let output = Command::new("/bin/zsh")
            .arg(root.path().join("scripts/build_macos_bundle.sh"))
            .env(
                "SAMSUNG_TV_CODESIGN_IDENTITY",
                if accepted { "apple" } else { signer },
            )
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", root.path().join("bin").display()),
            )
            .env(
                "BUNDLE_GUARD_ACTIVE",
                if signer == "running" { "yes" } else { "no" },
            )
            .env("BUNDLE_GUARD_PATH", &bundle_guard_path)
            .output()
            .unwrap();
        assert_eq!(output.status.success(), accepted, "{output:?}");
        assert_eq!(
            fs::read_to_string(&binary).unwrap(),
            if accepted { "new" } else { "approved-old" }
        );
        if accepted {
            let version = Command::new("/usr/bin/plutil")
                .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
                .arg(bundle.join("Contents/Info.plist"))
                .output()
                .unwrap();
            assert!(version.status.success());
            assert_eq!(
                String::from_utf8(version.stdout).unwrap().trim(),
                env!("CARGO_PKG_VERSION")
            );
        }
        assert!(!fs::read_dir(root.path().join("target"))
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("bundle-staging.")));
    }
}
