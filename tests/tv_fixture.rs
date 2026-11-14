//! Keep the synthetic TLS/WebSocket fixture checks in the default Cargo gate.
#[test]
fn synthetic_tv_fixture_validates_tls_frames_and_redaction_without_hardware() {
    let output = std::process::Command::new("python3")
        .arg("-B")
        .arg("tests/test_m9_tv_fixture.py")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("Python 3 is required for the synthetic TV fixture tests");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
