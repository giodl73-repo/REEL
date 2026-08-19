use std::{fs, process::Command};

use tempfile::tempdir;

const LIBRARY: &str = "manifests/fixtures/sprite-library/library.yaml";
const PROFILE: &str = "manifests/fixtures/sprite-library/profile.yaml";
const CAST: &str = "manifests/fixtures/sprite-library/cast.yaml";

fn reel() -> Command {
    Command::new(env!("CARGO_BIN_EXE_reel"))
}

#[test]
fn validates_library_and_profile_contracts() {
    let library = reel()
        .args(["sprite-library-validate", LIBRARY, "--output", "json"])
        .output()
        .unwrap();
    assert!(
        library.status.success(),
        "{}",
        String::from_utf8_lossy(&library.stderr)
    );

    let profile = reel()
        .args([
            "sprite-profile-validate",
            LIBRARY,
            PROFILE,
            "--output",
            "json",
        ])
        .output()
        .unwrap();
    assert!(
        profile.status.success(),
        "{}",
        String::from_utf8_lossy(&profile.stderr)
    );
}

#[test]
fn resolves_path_free_layers_with_post_transform_decals() {
    let directory = tempdir().unwrap();
    let plan_path = directory.path().join("plan.json");
    let output = reel()
        .args([
            "sprite-cast-resolve",
            LIBRARY,
            PROFILE,
            CAST,
            "--output-path",
        ])
        .arg(&plan_path)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let plan: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(plan_path).unwrap()).unwrap();
    assert_eq!(plan["resolved_requests"], 3);
    let serialized = serde_json::to_string(&plan).unwrap();
    assert!(!serialized.contains(":\\"));
    assert!(!serialized.contains("M:"));

    let carry = &plan["items"][0];
    assert_eq!(carry["mirror_x"], true);
    let decal = carry["ordered_layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["slot"] == "readable-decal")
        .unwrap();
    assert_eq!(decal["recipe"], "fixture/decal/08/v1");
    assert_eq!(decal["transform_stage"], "post-transform");
}

#[test]
fn rejects_unknown_selector_instead_of_guessing_a_pose() {
    let directory = tempdir().unwrap();
    let cast_path = directory.path().join("cast.yaml");
    let source = fs::read_to_string(CAST)
        .unwrap()
        .replace("action: carry", "action: coast");
    fs::write(&cast_path, source).unwrap();
    let output = reel()
        .args(["sprite-cast-resolve", LIBRARY, PROFILE])
        .arg(cast_path)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("resolved 0 bindings"));
}

#[test]
fn rejects_tampered_dependency_hashes() {
    let directory = tempdir().unwrap();
    let profile_path = directory.path().join("profile.yaml");
    let source = fs::read_to_string(PROFILE)
        .unwrap()
        .replace("1e4367c1", "0e4367c1");
    fs::write(&profile_path, source).unwrap();
    let output = reel()
        .args(["sprite-profile-validate", LIBRARY])
        .arg(profile_path)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("hash does not match"));
}

#[test]
fn rejects_duplicate_stable_subject_bindings() {
    let directory = tempdir().unwrap();
    let cast_path = directory.path().join("cast.yaml");
    let source = fs::read_to_string(CAST)
        .unwrap()
        .replace("fixture:keeper", "fixture:initiator");
    fs::write(&cast_path, source).unwrap();
    let output = reel()
        .args(["sprite-cast-resolve", LIBRARY, PROFILE])
        .arg(cast_path)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("duplicate stable subject"));
}
