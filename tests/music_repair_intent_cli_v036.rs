use std::process::Command;

#[test]
fn cli_checks_model_bound_repair_intent() {
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args([
            "music-repair-intent-check",
            "manifests/fixtures/music-repair-intent/intent.yaml",
            "--output",
            "json",
        ])
        .output()
        .expect("repair intent command runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["mutating_operations"], 1);
    assert_eq!(report["model_targets"], 1);
    assert_eq!(report["candidate_checks"], 6);
    assert_eq!(report["source_lineage_matches"], true);
    assert_eq!(report["shareable"], false);
}
