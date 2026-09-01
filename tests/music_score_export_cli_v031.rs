use std::{fs, process::Command};

use tempfile::tempdir;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(args)
        .output()
        .expect("REEL command runs")
}

#[test]
fn cli_exports_rechecks_and_detects_score_tampering() {
    let temporary = tempdir().unwrap();
    let plan = temporary.path().join("plan.json");
    let packet = temporary.path().join("packet");
    let model = "manifests/fixtures/music-model-corrected/model.yaml";

    let planned = run(&[
        "music-score-export-plan",
        model,
        "--output-path",
        plan.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert!(
        planned.status.success(),
        "{}",
        String::from_utf8_lossy(&planned.stderr)
    );

    let rendered = run(&[
        "music-score-export-render",
        plan.to_str().unwrap(),
        model,
        "--output-dir",
        packet.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert!(
        rendered.status.success(),
        "{}",
        String::from_utf8_lossy(&rendered.stderr)
    );
    let rendered_report = String::from_utf8_lossy(&rendered.stdout);
    assert!(rendered_report.contains("\"midi_round_trip\": true"));
    assert!(rendered_report.contains("\"musicxml_round_trip\": true"));
    assert!(rendered_report.contains("\"shareable\": false"));

    let receipt = packet.join("receipt.json");
    let checked = run(&[
        "music-score-export-check",
        receipt.to_str().unwrap(),
        plan.to_str().unwrap(),
        model,
        packet.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );

    let musicxml_path = packet.join("score.musicxml");
    let musicxml = fs::read_to_string(&musicxml_path).unwrap();
    fs::write(
        &musicxml_path,
        musicxml.replacen("<step>C</step>", "<step>D</step>", 1),
    )
    .unwrap();
    let tampered = run(&[
        "music-score-export-check",
        receipt.to_str().unwrap(),
        plan.to_str().unwrap(),
        model,
        packet.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert!(!tampered.status.success());
    assert!(String::from_utf8_lossy(&tampered.stderr).contains("round-trip comparison"));
}
