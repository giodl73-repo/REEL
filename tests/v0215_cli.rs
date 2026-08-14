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

#[test]
fn cli_records_and_rechecks_path_free_prosody_evidence() {
    let dir = tempdir().unwrap();
    let packet = dir.path().join("packet");
    let planned = reel()
        .args([
            "voice-performance-plan",
            "manifests/fixtures/voice-performance/manifest.yaml",
            "manifests/fixtures/voice-performance/performance.yaml",
            "--engine",
            "indextts25",
            "--engine-version",
            "2.5",
            "--output-dir",
        ])
        .arg(&packet)
        .output()
        .unwrap();
    assert!(planned.status.success());
    let plan_bytes = fs::read(packet.join("plan.json")).unwrap();
    let plan_hash = sha256(&plan_bytes);
    let audio = dir.path().join("rendered.wav");
    fs::write(&audio, b"sanitized fixture audio bytes").unwrap();
    let audio_hash = reel::production::sha256_path(&audio).unwrap();
    let measurements = dir.path().join("measurements.yaml");
    let spans = [
        ("neutral-setup", 200, 200, 200),
        ("explosive-interruption", 200, 260, 170),
        ("fear-warning", 200, 250, 180),
        ("suspense-build", 180, 190, 210),
        ("decisive-action", 200, 188, 168),
        ("comic-button", 190, 180, 160),
    ];
    let mut yaml = format!(
        "schema: reel.voice-prosody-measurements.v0.1\nplan_sha256: {plan_hash}\nrendered_audio_sha256: {audio_hash}\nanalyzer: sanitized-pyin\nanalyzer_version: fixture-1\nspans:\n"
    );
    for (index, (id, first, middle, final_f0)) in spans.into_iter().enumerate() {
        yaml.push_str(&format!(
            "  - span_id: {id}\n    start_seconds: {index}\n    end_seconds: {}\n    median_f0_hz: {middle}\n    first_f0_hz: {first}\n    middle_f0_hz: {middle}\n    final_f0_hz: {final_f0}\n    voiced_frame_coverage: 0.8\n    duration_seconds: 1\n",
            index + 1
        ));
    }
    fs::write(&measurements, yaml).unwrap();
    let evidence = dir.path().join("evidence");
    let output = reel()
        .arg("voice-prosody-evidence")
        .arg(&packet)
        .arg(&measurements)
        .arg(&audio)
        .arg("--output-dir")
        .arg(&evidence)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["passed"], true);
    let stored: Value =
        serde_json::from_slice(&fs::read(evidence.join("evidence.json")).unwrap()).unwrap();
    assert_eq!(stored["spans"][4]["detected_contour"], "falling");
    assert_eq!(stored["spans"][4]["terminal_boundary_match"], true);
    let serialized = serde_json::to_string(&stored).unwrap();
    assert!(!serialized.contains(dir.path().to_string_lossy().as_ref()));

    let checked = reel()
        .arg("voice-prosody-evidence-check")
        .arg(&evidence)
        .arg(&packet)
        .arg(&measurements)
        .arg(&audio)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(checked.status.success());
    let report: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(report["verified"], true);

    fs::write(&audio, b"changed sanitized fixture audio bytes").unwrap();
    let changed = reel()
        .arg("voice-prosody-evidence-check")
        .arg(&evidence)
        .arg(&packet)
        .arg(&measurements)
        .arg(&audio)
        .output()
        .unwrap();
    assert!(!changed.status.success());
}

fn sha256(bytes: &[u8]) -> String {
    let dir = tempdir().unwrap();
    let path = dir.path().join("bytes");
    fs::write(&path, bytes).unwrap();
    reel::production::sha256_path(&path).unwrap()
}
