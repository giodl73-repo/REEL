use std::{fs, process::Command};

use tempfile::tempdir;

const FIXTURE: &str = "manifests/fixtures/choreography/simple-handoff.yaml";

#[test]
fn validates_and_compiles_choreography_sidecar() {
    let binary = env!("CARGO_BIN_EXE_reel");
    let validated = Command::new(binary)
        .args(["choreography-validate", FIXTURE, "--output", "json"])
        .output()
        .unwrap();
    assert!(
        validated.status.success(),
        "{}",
        String::from_utf8_lossy(&validated.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&validated.stdout).unwrap();
    assert_eq!(report["passed"], true);
    assert_eq!(report["approach_phrases"], 2);
    assert_eq!(report["handoff_phrases"], 1);
    assert_eq!(report["react_phrases"], 2);
    assert_eq!(report["camera_phrases"], 4);
    assert_eq!(report["production_bound"], true);

    let directory = tempdir().unwrap();
    let output = directory.path().join("resolved.json");
    let compiled = Command::new(binary)
        .args(["choreography-compile", FIXTURE, "--output-path"])
        .arg(&output)
        .arg("--output")
        .arg("json")
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let plan: serde_json::Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert_eq!(plan["schema"], "reel.choreography-plan.v0.1");
    assert_eq!(plan["performers"].as_array().unwrap().len(), 3);
    assert_eq!(plan["props"][0]["handoffs"].as_array().unwrap().len(), 1);
    assert_eq!(plan["camera"].as_array().unwrap().len(), 4);
    assert_eq!(
        plan["production_binding"]["work"],
        "shared-production-fixture"
    );

    let sprite_output = directory.path().join("sprite-production.yaml");
    let sprite = Command::new(binary)
        .args([
            "choreography-sprite-manifest",
            FIXTURE,
            "manifests/fixtures/choreography/assets.yaml",
            "--output-path",
        ])
        .arg(&sprite_output)
        .arg("--output")
        .arg("json")
        .output()
        .unwrap();
    assert!(
        sprite.status.success(),
        "{}",
        String::from_utf8_lossy(&sprite.stderr)
    );
    let sprite_report: serde_json::Value = serde_json::from_slice(&sprite.stdout).unwrap();
    assert_eq!(sprite_report["passed"], true);
    assert_eq!(sprite_report["performers"], 3);
    assert_eq!(sprite_report["camera_phrases"], 4);
    let sprite_manifest = fs::read_to_string(sprite_output).unwrap();
    assert!(sprite_manifest.contains("choreography_execution"));
    assert!(sprite_manifest.contains("sprite_animation"));
}

#[test]
fn rejects_a_handoff_from_the_wrong_owner() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("invalid.yaml");
    let source = fs::read_to_string(FIXTURE)
        .unwrap()
        .replace("owner: initiator", "owner: receiver");
    fs::write(&path, source).unwrap();
    let rejected = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("choreography-validate")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("cannot hand it off"));
}

#[test]
fn rejects_a_stale_production_manifest_hash() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("stale.yaml");
    let source = fs::read_to_string(FIXTURE)
        .unwrap()
        .replace(
            "manifest: ../shared-production/manifest.yaml",
            &format!(
                "manifest: '{}'",
                fs::canonicalize("manifests/fixtures/shared-production/manifest.yaml")
                    .unwrap()
                    .display()
            ),
        )
        .replace(
            "99957ce88740be4112f54d0c8bfdc166a09c8b292a3409cd8f7b6ea6a3d74823",
            "0000000000000000000000000000000000000000000000000000000000000000",
        );
    fs::write(&path, source).unwrap();
    let rejected = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("choreography-validate")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("hash mismatch"));
}
