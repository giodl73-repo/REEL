use std::process::Command;

#[test]
fn cli_validates_external_analysis_and_corrected_model_fixture() {
    let reel = env!("CARGO_BIN_EXE_reel");
    let analysis = Command::new(reel)
        .args([
            "music-analysis-validate",
            "manifests/fixtures/music-model-corrected/analysis.yaml",
            "--output",
            "json",
        ])
        .output()
        .expect("analysis command runs");
    assert!(
        analysis.status.success(),
        "{}",
        String::from_utf8_lossy(&analysis.stderr)
    );
    let analysis_report = String::from_utf8_lossy(&analysis.stdout);
    assert!(analysis_report.contains("\"verified\": true"));
    assert!(analysis_report.contains(
        "\"contract_sha256\": \"c92cd3b3773e8e9b3e128c5335fee5ac2819628507f554d222f9cf6ee92efcdd\""
    ));

    let model = Command::new(reel)
        .args([
            "music-model-validate",
            "manifests/fixtures/music-model-corrected/model.yaml",
            "--output",
            "json",
        ])
        .output()
        .expect("model command runs");
    assert!(
        model.status.success(),
        "{}",
        String::from_utf8_lossy(&model.stderr)
    );
    let model_report = String::from_utf8_lossy(&model.stdout);
    assert!(model_report.contains("\"verified\": true"));
    assert!(model_report.contains("\"human_corrected_events\": 1"));
    assert!(model_report.contains(
        "\"contract_sha256\": \"995413528d8a4d6a99458174f7bf69198c1b428c0cb025ddf1275eb5e59a516b\""
    ));
}
