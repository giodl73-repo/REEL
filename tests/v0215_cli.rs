use std::{fs, process::Command};

use serde_json::Value;
use tempfile::tempdir;

fn reel() -> Command {
    Command::new(env!("CARGO_BIN_EXE_reel"))
}

#[test]
fn cli_builds_and_rechecks_voice_performance_packet() {
    let dir = tempdir().unwrap();
    let packet = dir.path().join("packet");
    let output = reel()
        .args([
            "voice-performance-plan",
            "manifests/fixtures/voice-performance/manifest.yaml",
            "manifests/fixtures/voice-performance/performance.yaml",
            "--engine",
            "chatterbox",
            "--engine-version",
            "0.1.7",
            "--seed",
            "1947",
            "--output-dir",
        ])
        .arg(&packet)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["verified"], true);
    assert_eq!(report["span_count"], 6);
    let checked = reel()
        .arg("voice-performance-plan-check")
        .arg(&packet)
        .args([
            "manifests/fixtures/voice-performance/manifest.yaml",
            "manifests/fixtures/voice-performance/performance.yaml",
            "--output",
            "json",
        ])
        .output()
        .unwrap();
    assert!(checked.status.success());
    let report: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(report["verified"], true);
    let plan: Value = serde_json::from_slice(&fs::read(packet.join("plan.json")).unwrap()).unwrap();
    assert_eq!(plan["spans"][1]["action"], "explosive-interruption");
    assert_eq!(
        plan["spans"][1]["execution"]["native_parameters"]["exaggeration"],
        0.9
    );
    assert_eq!(
        plan["spans"][1]["execution"]["clamps"][0],
        "exaggeration 0.920 clamped to 0.900"
    );
    assert!(
        plan["spans"][1]["execution"]["advisory_only"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "energy")
    );
    assert!(
        plan["spans"][1]["execution"]["advisory_only"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "cultural_register")
    );
}

#[test]
fn cli_rejects_stale_manifest_binding_without_writing_packet() {
    let dir = tempdir().unwrap();
    let bad = dir.path().join("bad.yaml");
    let text = fs::read_to_string("manifests/fixtures/voice-performance/performance.yaml")
        .unwrap()
        .replace(
            "d4b39a0204641cca7cde6d6984576ccda1230432d39b04ec8b90bb0a5888a452",
            "deadbeef",
        );
    fs::write(&bad, text).unwrap();
    let packet = dir.path().join("packet");
    let output = reel()
        .arg("voice-performance-plan")
        .arg("manifests/fixtures/voice-performance/manifest.yaml")
        .arg(&bad)
        .args([
            "--engine",
            "generic",
            "--engine-version",
            "fixture",
            "--output-dir",
        ])
        .arg(&packet)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!packet.exists());
}
