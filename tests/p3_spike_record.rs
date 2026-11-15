//! The hardware observation recorder is validated offline in the Cargo gate.
#[test]
fn spike_records_are_bounded_and_cannot_invent_protocol_evidence() {
    let output = std::process::Command::new("python3")
        .args(["-B", "tests/test_p3_spike_record.py"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("Python 3 is required for spike recorder tests");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
