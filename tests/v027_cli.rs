use std::{fs, process::Command};

use tempfile::tempdir;

fn write_package(root: &std::path::Path, with_approved_gate: bool) -> std::path::PathBuf {
    fs::write(root.join("manifest.yaml"), "work: fixture\n").unwrap();
    fs::write(root.join("score.json"), "{\"score\":\"fixture\"}\n").unwrap();
    fs::write(root.join("review.json"), "{\"review\":\"approved\"}\n").unwrap();
    let manifest_hash = reel::production::sha256_path(root.join("manifest.yaml")).unwrap();
    let score_hash = reel::production::sha256_path(root.join("score.json")).unwrap();
    let review_hash = reel::production::sha256_path(root.join("review.json")).unwrap();
    let gates = if with_approved_gate {
        "review_gates:\n  - { id: editorial, owner: editor, status: approved, evidence_component: editorial-review }\n"
    } else {
        "review_gates: []\n"
    };
    let source = format!(
        "schema: reel.production-package.v0.1\nwork: fixture\nrevision: r1\npublication_scope: release-candidate\ncomponents:\n  - {{ id: manifest, kind: production-manifest, path: manifest.yaml, sha256: {manifest_hash} }}\n  - {{ id: score, kind: score-plan, path: score.json, sha256: {score_hash} }}\n  - {{ id: editorial-review, kind: review-evidence, path: review.json, sha256: {review_hash} }}\n{gates}"
    );
    let package = root.join("package.yaml");
    fs::write(&package, source).unwrap();
    package
}

#[test]
fn writes_path_free_release_ready_receipt_and_detects_component_tampering() {
    let root = tempdir().unwrap();
    let package = write_package(root.path(), true);
    let receipt = root.path().join("receipt.json");
    let emitted = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("production-package-receipt")
        .arg(&package)
        .arg("--output-path")
        .arg(&receipt)
        .arg("--output")
        .arg("json")
        .output()
        .unwrap();
    assert!(
        emitted.status.success(),
        "{}",
        String::from_utf8_lossy(&emitted.stderr)
    );
    let bytes = fs::read(&receipt).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["release_ready"], true);
    assert_eq!(value["components"].as_array().unwrap().len(), 3);
    assert!(!String::from_utf8_lossy(&bytes).contains(root.path().to_string_lossy().as_ref()));
    assert!(!value.to_string().contains("path"));

    let checked = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("production-package-check")
        .arg(&receipt)
        .arg(&package)
        .arg("--output")
        .arg("json")
        .output()
        .unwrap();
    assert!(checked.status.success());
    let report: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(report["passed"], true);
    assert_eq!(report["release_ready"], true);

    fs::write(root.path().join("score.json"), "tampered\n").unwrap();
    let rejected = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("production-package-check")
        .arg(&receipt)
        .arg(&package)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("hash mismatch"));
}

#[test]
fn integrity_does_not_infer_release_approval() {
    let root = tempdir().unwrap();
    let package = write_package(root.path(), false);
    let receipt = root.path().join("receipt.json");
    let emitted = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("production-package-receipt")
        .arg(&package)
        .arg("--output-path")
        .arg(&receipt)
        .output()
        .unwrap();
    assert!(emitted.status.success());
    let value: serde_json::Value = serde_json::from_slice(&fs::read(receipt).unwrap()).unwrap();
    assert_eq!(value["required_components_verified"], true);
    assert_eq!(value["review_gates_approved"], false);
    assert_eq!(value["release_ready"], false);
}
