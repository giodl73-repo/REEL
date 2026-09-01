use std::process::Command;

#[test]
fn cli_validates_existing_tool_interchange_fixture() {
    let result = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args([
            "music-interchange-validate",
            "manifests/fixtures/music-interchange-intake/intake.yaml",
            "--output",
            "json",
        ])
        .output()
        .expect("interchange command runs");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report = String::from_utf8_lossy(&result.stdout);
    assert!(report.contains("\"formats\": ["));
    assert!(report.contains("\"csv\""));
    assert!(report.contains("\"jams\""));
    assert!(report.contains("\"shareable\": false"));
    assert!(report.contains("\"verified\": true"));
}
