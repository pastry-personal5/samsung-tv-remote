//! macOS staging tests use disposable fixtures, never a signing identity or
//! credential item. Compiler/signing fakes cover interrupted-build publication;
//! an ad-hoc fixture covers the real copy and verification path.
#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

const BUILDERS: [&str; 2] = [
    "build_m6_signed_launch_agent_probe.sh",
    "build_m6_signed_keychain_probe.sh",
];

fn fixture() -> TempDir {
    let root = tempfile::tempdir().unwrap();
    for directory in ["scripts", "target", "docs/phase-2", "bin"] {
        fs::create_dir_all(root.path().join(directory)).unwrap();
    }
    for script in BUILDERS.into_iter().chain(["stage_m6_human_handoff.sh"]) {
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts")
                .join(script),
            root.path().join("scripts").join(script),
        )
        .unwrap();
    }
    fs::write(
        root.path().join("target/local-signing-identity.txt"),
        "synthetic-fixture-only",
    )
    .unwrap();
    fs::write(
        root.path()
            .join("docs/phase-2/milestone-06-human-runbook.md"),
        "stage='/private/tmp/m6probe-bundles.REPLACE_ME'\n",
    )
    .unwrap();
    for reference in [
        "milestone-06-acceptance.md",
        "milestone-06-platform-evidence.md",
        "milestone-06-review.md",
    ] {
        fs::write(
            root.path().join("docs/phase-2").join(reference),
            "Disposable context fixture.\n",
        )
        .unwrap();
    }
    root
}

fn executable(path: PathBuf, contents: &str) {
    fs::write(&path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn fake_tools(root: &Path) {
    let compiler = r#"#!/bin/zsh
set -eu
while (( $# )); do
    if [[ "$1" == '-o' ]]; then
        shift
        print -r -- 'synthetic compiler output' > "$1"
        exit 0
    fi
    shift
done
exit 1
"#;
    for compiler_name in ["rustc", "swiftc"] {
        executable(root.join("bin").join(compiler_name), compiler);
    }
    executable(root.join("bin/plutil"), "#!/bin/zsh\nexit 0\n");
    executable(
        root.join("bin/codesign"),
        r#"#!/bin/zsh
set -eu
artifact="${argv[-1]}"
if [[ "$1" == '--verify' ]]; then
    if [[ "${M6_TEST_FAIL:-}" == 'verify' || ! -f "$artifact/Contents/_CodeSignature/CodeResources" ]]; then
        print -ru2 -- 'code has no resources but signature indicates they must be present'
        exit 1
    fi
elif [[ "${M6_TEST_FAIL:-}" == 'sign' ]]; then
    print -ru2 -- 'synthetic signing failure'
    exit 1
elif [[ -d "$artifact" ]]; then
    mkdir -p "$artifact/Contents/_CodeSignature"
    print -r -- 'synthetic resource seal' > "$artifact/Contents/_CodeSignature/CodeResources"
fi
"#,
    );
}

fn run(root: &Path, script: &str, failure: &str) -> Output {
    run_with_args(root, script, failure, &[])
}

fn run_with_args(root: &Path, script: &str, failure: &str, arguments: &[&str]) -> Output {
    Command::new("/bin/zsh")
        .arg(root.join("scripts").join(script))
        .args(arguments)
        .env(
            "PATH",
            format!(
                "{}:/usr/bin:/bin:/usr/sbin:/sbin",
                root.join("bin").display()
            ),
        )
        .env("M6_TEST_FAIL", failure)
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn failed_probe_builds_do_not_publish_or_leave_incomplete_candidates() {
    for builder in BUILDERS {
        for failure in ["sign", "verify"] {
            let root = fixture();
            fake_tools(root.path());
            let old = root.path().join("target/m6-launch-agent.existing");
            fs::create_dir(&old).unwrap();
            fs::write(old.join("preserve"), "previous build").unwrap();
            let output = run(root.path(), builder, failure);
            assert!(!output.status.success(), "{}", text(&output));
            let target = root.path().join("target");
            for entry in fs::read_dir(target).unwrap() {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_str().unwrap();
                assert!(!name.starts_with(".m6-"), "unfinished build leaked: {name}");
                if name.starts_with("m6-launch-agent.") || name.starts_with("m6-keychain.") {
                    assert_eq!(path, old, "failed build was published");
                }
            }
            assert_eq!(
                fs::read_to_string(old.join("preserve")).unwrap(),
                "previous build"
            );
        }
    }
}

#[test]
fn successful_probe_builds_publish_the_verified_path() {
    for builder in BUILDERS {
        let root = fixture();
        fake_tools(root.path());
        let output = run(root.path(), builder, "");
        assert!(output.status.success(), "{}", text(&output));
        let output = String::from_utf8(output.stdout).unwrap();
        let bundle = Path::new(output.lines().last().unwrap().split_once(": ").unwrap().1);
        assert!(bundle
            .join("Contents/_CodeSignature/CodeResources")
            .is_file());
        assert!(!bundle.to_str().unwrap().contains("/.m6-"));
    }
}

#[test]
fn incomplete_newest_bundle_fails_with_rebuild_guidance() {
    let root = fixture();
    fake_tools(root.path());
    fs::create_dir_all(
        root.path()
            .join("target/m6-launch-agent.incomplete/M6 LaunchAgent Probe.app/Contents"),
    )
    .unwrap();
    fs::create_dir_all(
        root.path()
            .join("target/m6-keychain.fixture/M6 Keychain Probe.app"),
    )
    .unwrap();
    let older = root
        .path()
        .join("target/m6-launch-agent.older/M6 LaunchAgent Probe.app");
    fs::create_dir_all(older.join("Contents/_CodeSignature")).unwrap();
    fs::write(
        older.join("Contents/_CodeSignature/CodeResources"),
        "synthetic resource seal",
    )
    .unwrap();
    fs::File::open(&older)
        .unwrap()
        .set_times(
            fs::FileTimes::new()
                .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1)),
        )
        .unwrap();
    let output = run(root.path(), "stage_m6_human_handoff.sh", "");
    assert_eq!(output.status.code(), Some(2));
    let message = text(&output);
    assert!(message.contains("resource seal"), "{message}");
    assert!(
        message.contains("zsh scripts/build_m6_signed_launch_agent_probe.sh"),
        "{message}"
    );
    assert!(
        message.contains("No older build was substituted"),
        "{message}"
    );
    assert!(!message.contains("stage='"), "incomplete handoff published");
}

fn adhoc_bundle(root: &Path, relative: &str, identifier: &str) {
    let bundle = root.join(relative);
    fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
    fs::create_dir_all(bundle.join("Contents/Resources")).unwrap();
    fs::copy("/usr/bin/true", bundle.join("Contents/MacOS/fixture")).unwrap();
    fs::write(
        bundle.join("Contents/Resources/fixture.txt"),
        "disposable test fixture",
    )
    .unwrap();
    fs::write(
        bundle.join("Contents/Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{identifier}</string>
<key>CFBundleExecutable</key><string>fixture</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>"#
        ),
    )
    .unwrap();
    let output = Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-", "--timestamp=none"])
        .arg(&bundle)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
}

fn init_repository(root: &Path) {
    // The handoff metadata command uses a disposable repository, not the user's.
    let output = Command::new("/usr/bin/git")
        .args(["init", "--quiet"])
        .arg(root)
        .output()
        .unwrap();
    assert!(output.status.success());
    let output = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "fixture",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
}

#[test]
fn real_staging_preserves_fixture_signatures_and_fills_runbook_path() {
    for mode in ["full", "keychain-only", "keychain-only-explicit"] {
        let root = fixture();
        let keychain_only = mode != "full";
        if !keychain_only {
            adhoc_bundle(
                root.path(),
                "target/m6-launch-agent.fixture/M6 LaunchAgent Probe.app",
                "dev.samsungtvremote.local.m6agentprobe",
            );
        } else if mode == "keychain-only-explicit" {
            // An unfinished optional probe must not block the required run.
            fs::create_dir_all(
                root.path()
                    .join("target/m6-launch-agent.incomplete/M6 LaunchAgent Probe.app"),
            )
            .unwrap();
        }
        let keychain = "target/m6-keychain.fixture/M6 Keychain Probe.app";
        let gui = "target/bundle/Samsung TV Remote.app";
        adhoc_bundle(
            root.path(),
            keychain,
            "dev.samsungtvremote.local.m6keychainprobe",
        );
        adhoc_bundle(root.path(), gui, "dev.samsungtvremote.local");
        init_repository(root.path());
        let keychain = root.path().join(keychain);
        let gui = root.path().join(gui);
        let arguments = match mode {
            "full" => vec![],
            "keychain-only" => vec!["--keychain-only"],
            _ => vec![
                "--keychain-only",
                keychain.to_str().unwrap(),
                gui.to_str().unwrap(),
            ],
        };
        let output = run_with_args(root.path(), "stage_m6_human_handoff.sh", "", &arguments);
        assert!(output.status.success(), "{mode}: {}", text(&output));
        let output = String::from_utf8(output.stdout).unwrap();
        let assignment = output
            .lines()
            .find(|line| line.starts_with("stage='"))
            .unwrap();
        let stage = Path::new(
            assignment
                .strip_prefix("stage='")
                .unwrap()
                .strip_suffix('\'')
                .unwrap(),
        );
        assert!(stage.starts_with("/private/tmp"));
        assert!(stage
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("m6probe-bundles."));
        let result = fs::read_to_string(stage.join("milestone-06-human-runbook.md")).unwrap();
        let metadata = fs::read_to_string(stage.join("HANDOFF.txt")).unwrap();
        let optional_agent_present = stage.join("M6 LaunchAgent Probe.app").exists();
        // Remove only the unique handoff this test created, including on assertion failure.
        let cleanup = fs::remove_dir_all(stage);
        assert_eq!(result.trim(), assignment);
        assert_eq!(optional_agent_present, !keychain_only);
        assert!(metadata.contains(&format!(
            "Keychain-only handoff: {}",
            u8::from(keychain_only)
        )));
        assert!(metadata.contains("not the binary build revision"));
        cleanup.unwrap();
    }
}

#[test]
fn staging_rejects_wrong_number_of_explicit_paths() {
    let root = fixture();
    for arguments in [
        vec!["keychain.app", "gui.app"],
        vec!["--keychain-only", "keychain.app"],
        vec!["--keychain-only", "agent.app", "keychain.app", "gui.app"],
    ] {
        let output = run_with_args(root.path(), "stage_m6_human_handoff.sh", "", &arguments);
        assert_eq!(output.status.code(), Some(2));
        assert!(text(&output).contains("usage:"));
        assert!(!text(&output).contains("stage='"));
    }
}

#[test]
fn missing_real_resource_seal_reproduces_reported_failure() {
    let root = fixture();
    let relative = "target/m6-launch-agent.fixture/M6 LaunchAgent Probe.app";
    adhoc_bundle(
        root.path(),
        relative,
        "dev.samsungtvremote.local.m6agentprobe",
    );
    fs::create_dir_all(
        root.path()
            .join("target/m6-keychain.fixture/M6 Keychain Probe.app"),
    )
    .unwrap();
    fs::remove_dir_all(root.path().join(relative).join("Contents/_CodeSignature")).unwrap();
    // An interrupted builder leaves a standalone signed compiler output in a
    // bundle that has never received its own resource envelope.
    let standalone = root.path().join("standalone");
    fs::copy("/usr/bin/true", &standalone).unwrap();
    let output = Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-", "--timestamp=none"])
        .arg(&standalone)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
    fs::copy(
        &standalone,
        root.path().join(relative).join("Contents/MacOS/fixture"),
    )
    .unwrap();
    let output = run(root.path(), "stage_m6_human_handoff.sh", "");
    assert_eq!(output.status.code(), Some(2));
    let message = text(&output);
    assert!(
        message.contains("code has no resources but signature indicates they must be present"),
        "{message}"
    );
    assert!(
        message.contains("zsh scripts/build_m6_signed_launch_agent_probe.sh"),
        "{message}"
    );
}
