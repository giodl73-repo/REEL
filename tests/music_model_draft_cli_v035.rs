use std::process::Command;

#[test]
fn cli_checks_complete_model_draft_dispositions() {
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args([
            "music-model-draft-check",
            "manifests/fixtures/music-model-corrected/draft.yaml",
            "--output",
            "json",
        ])
        .output()
        .expect("model draft command runs");
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["observations"], 7);
    assert_eq!(report["mapped_targets"], 11);
    assert_eq!(report["human_corrected_targets"], 1);
    assert_eq!(report["shareable"], false);
}
