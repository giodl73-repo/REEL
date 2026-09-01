use std::process::Command;

#[test]
fn cli_validates_and_writes_selected_semantic_evidence() {
    let validate = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args([
            "music-semantic-import-validate",
            "manifests/fixtures/music-interchange-intake/semantic-import.yaml",
            "--output",
            "json",
        ])
        .output()
        .expect("semantic import validation runs");
    assert!(validate.status.success());
    let report: serde_json::Value = serde_json::from_slice(&validate.stdout).unwrap();
    assert_eq!(report["events"], 3);
    assert_eq!(report["shareable"], false);

    let temporary = tempfile::tempdir_in(".").unwrap();
    let output = temporary.path().join("analysis.yaml");
    let write = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("music-semantic-import-write")
        .arg("manifests/fixtures/music-interchange-intake/semantic-import.yaml")
        .arg("--output-path")
        .arg(&output)
        .args(["--output", "json"])
        .output()
        .expect("semantic import write runs");
    assert!(
        write.status.success(),
        "{}",
        String::from_utf8_lossy(&write.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&write.stdout).unwrap();
    assert_eq!(report["observations"], 3);
    assert_eq!(report["verified"], true);
    assert!(output.exists());
}
