use std::process::Command;

#[test]
fn cli_emits_private_human_review_queue() {
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args([
            "music-evidence-compare",
            "manifests/fixtures/music-interchange-intake/comparison.yaml",
            "--output",
            "json",
        ])
        .output()
        .expect("comparison command runs");
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "reel.music-evidence-comparison.v0.1");
    assert_eq!(report["queue"].as_array().unwrap().len(), 2);
    assert_eq!(report["shareable"], false);
    assert_eq!(report["verified"], true);
}
