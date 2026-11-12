//! Exercise installer failure paths without desktop, signing or credential access.
#[test]
fn macos_installation_preserves_live_and_rollback_bundles() {
    let output = std::process::Command::new("python3")
        .arg("-B")
        .arg("tests/test_macos_bundle_install.py")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("Python 3 is required for the installer regression tests");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
