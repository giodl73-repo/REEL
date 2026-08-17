use std::{fs, process::Command};

use tempfile::tempdir;

const FIXTURE: &str = "manifests/fixtures/craft-plan/three-period-memoir.yaml";

#[test]
fn validates_and_reports_structural_coverage_without_quality_claim() {
    let binary = env!("CARGO_BIN_EXE_reel");
    let validated = Command::new(binary)
        .args(["craft-validate", FIXTURE, "--output", "json"])
        .output()
        .unwrap();
    assert!(
        validated.status.success(),
        "{}",
        String::from_utf8_lossy(&validated.stderr)
    );
    let validation: serde_json::Value = serde_json::from_slice(&validated.stdout).unwrap();
    assert_eq!(validation["departments_present"], 12);
    assert_eq!(validation["passed"], true);

    let covered = Command::new(binary)
        .args(["craft-coverage", FIXTURE, "--output", "json"])
        .output()
        .unwrap();
    assert!(covered.status.success());
    let coverage: serde_json::Value = serde_json::from_slice(&covered.stdout).unwrap();
    assert_eq!(coverage["structurally_complete"], true);
    assert_eq!(coverage["artistic_quality_assessed"], false);
    assert!(
        coverage["missing_departments"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn exports_only_the_selected_departments_routed_information() {
    let directory = tempdir().unwrap();
    let output = directory.path().join("costume.json");
    let exported = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["department-packet", FIXTURE, "costume", "--output-path"])
        .arg(&output)
        .arg("--output")
        .arg("json")
        .output()
        .unwrap();
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    let bytes = fs::read(&output).unwrap();
    let packet: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(packet["schema"], "reel.department-packet.v0.1");
    assert_eq!(packet["department"], "costume");
    assert!(packet["evidence"].get("ev-photo-early").is_some());
    assert!(packet["evidence"].get("ev-audio-late").is_none());
    assert!(packet["editorial"].as_array().unwrap().is_empty());
    assert!(packet["vfx"].as_array().unwrap().is_empty());
    assert!(!String::from_utf8_lossy(&bytes).contains("score-department"));
}

#[test]
fn rejects_unknown_department_without_writing_packet() {
    let directory = tempdir().unwrap();
    let output = directory.path().join("unknown.json");
    let rejected = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["department-packet", FIXTURE, "wardrobe", "--output-path"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(!output.exists());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("unknown department"));
}

#[test]
fn external_packet_requires_approval_and_receipt_detects_tampering() {
    let directory = tempdir().unwrap();
    let packet = directory.path().join("costume-external.json");
    let refused = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args([
            "department-packet",
            FIXTURE,
            "costume",
            "--distribution",
            "external",
            "--output-path",
        ])
        .arg(&packet)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(!packet.exists());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--approval-reference"));

    let exported = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args([
            "department-packet",
            FIXTURE,
            "costume",
            "--distribution",
            "external",
            "--approval-reference",
            "fixture-review-001",
            "--output-path",
        ])
        .arg(&packet)
        .output()
        .unwrap();
    assert!(exported.status.success());

    let receipt = directory.path().join("costume-external.receipt.json");
    let emitted = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("department-packet-receipt")
        .arg(&packet)
        .arg("--output-path")
        .arg(&receipt)
        .output()
        .unwrap();
    assert!(emitted.status.success());

    let checked = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("department-packet-check")
        .arg(&receipt)
        .arg(&packet)
        .arg("--output")
        .arg("json")
        .output()
        .unwrap();
    assert!(checked.status.success());
    let report: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(report["passed"], true);

    fs::write(&packet, b"{}\n").unwrap();
    let tampered = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("department-packet-check")
        .arg(&receipt)
        .arg(&packet)
        .output()
        .unwrap();
    assert!(!tampered.status.success());
}
