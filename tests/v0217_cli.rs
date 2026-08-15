use std::{fs, process::Command};

use serde_json::Value;
use tempfile::tempdir;

fn reel() -> Command {
    Command::new(env!("CARGO_BIN_EXE_reel"))
}

#[test]
fn cli_accepts_scene_matching_approved_voice_profile() {
    let dir = tempdir().unwrap();
    let retained = dir.path().join("voice-consistency.json");
    let output = reel()
        .args([
            "voice-consistency-check",
            "manifests/fixtures/voice-performance/manifest.yaml",
            "manifests/fixtures/voice-consistency/profile.yaml",
            "manifests/fixtures/voice-consistency/scene-pass.yaml",
            "--report",
        ])
        .arg(&retained)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "reel.voice-consistency-report.v0.1");
    assert_eq!(report["passed"], true);
    assert_eq!(report["speakers"][0]["measured_wpm"], 120.0);
    assert!(retained.exists());
    let stored = fs::read_to_string(retained).unwrap();
    assert!(!stored.contains("manifests/"));
}

#[test]
fn cli_rejects_fast_scene_and_short_pauses() {
    let output = reel()
        .args([
            "voice-consistency-check",
            "manifests/fixtures/voice-performance/manifest.yaml",
            "manifests/fixtures/voice-consistency/profile.yaml",
            "manifests/fixtures/voice-consistency/scene-too-fast.yaml",
            "--output",
            "json",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["passed"], false);
    let codes: Vec<_> = report["violations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|violation| violation["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"pace-too-fast"));
    assert!(codes.contains(&"pause-too-short"));
    assert!(codes.contains(&"speaker-aggregate-pace"));
}

#[test]
fn cli_rejects_stale_measurement_binding() {
    let dir = tempdir().unwrap();
    let stale = dir.path().join("stale.yaml");
    fs::write(
        &stale,
        fs::read_to_string("manifests/fixtures/voice-consistency/scene-pass.yaml")
            .unwrap()
            .replace(
                "d4b39a0204641cca7cde6d6984576ccda1230432d39b04ec8b90bb0a5888a452",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
    )
    .unwrap();
    let output = reel()
        .arg("voice-consistency-check")
        .arg("manifests/fixtures/voice-performance/manifest.yaml")
        .arg("manifests/fixtures/voice-consistency/profile.yaml")
        .arg(&stale)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("measurements are stale"));
}
