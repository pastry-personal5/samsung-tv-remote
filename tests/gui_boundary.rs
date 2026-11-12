//! Keep the GUI/API split mandatory in the normal Cargo suite.
#[test]
fn presentation_and_default_composition_reject_runtime_and_adapter_dependencies() {
    let output = std::process::Command::new("python3")
        .arg("scripts/check_gui_boundary.py")
        .arg("--self-test")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("Python 3 is required for the GUI dependency check");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
