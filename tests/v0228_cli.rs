use std::{fs, process::Command};

use tempfile::tempdir;

const BASE: &str = "manifests/fixtures/vertical-sound-off/manifest.yaml";

fn sprite_manifest() -> String {
    fs::read_to_string(BASE).unwrap().replace(
        "    visual_asset: frame-hook.ppm\n",
        r#"    media_kind: sprite-animation
    sprite_animation:
      background: frame-hook.ppm
      timing_fps: 24
      intentional_holds:
        - { start_frame: 0, end_frame: 8, reason: readable anticipation }
      sprites:
        - id: performer
          movement: stepped
          movement_steps: 3
          keyframes:
            - { frame: 0, asset: frame-hook.ppm, x: 0.30, y: 0.60, width: 0.25 }
            - { frame: 65, asset: frame-landing.ppm, x: 0.60, y: 0.45, width: 0.20 }
        - id: token
          parent: performer
          position_space: parent-width
          movement: stepped
          movement_steps: 3
          keyframes:
            - { frame: 0, asset: frame-landing.ppm, x: 0.35, y: 0.10, width: 0.02 }
            - { frame: 65, asset: frame-landing.ppm, x: 0.25, y: 0.05, width: 0.02 }
"#,
    )
}

fn validate(source: &str) -> std::process::Output {
    let directory = tempdir().unwrap();
    let path = directory.path().join("manifest.yaml");
    fs::write(&path, source).unwrap();
    Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("validate")
        .arg(path)
        .arg("--output")
        .arg("json")
        .output()
        .unwrap()
}

#[test]
fn validates_parent_relative_tracks_and_reasoned_holds() {
    let output = validate(&sprite_manifest());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["sprite_animation_events"], 1);
    assert_eq!(report["delivery_ready"], true);
}

#[test]
fn rejects_unknown_sprite_parent() {
    let output = validate(&sprite_manifest().replace("parent: performer", "parent: missing"));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid parent missing"));
}

#[test]
fn rejects_overlapping_intentional_holds() {
    let source = sprite_manifest().replace(
        "        - { start_frame: 0, end_frame: 8, reason: readable anticipation }",
        "        - { start_frame: 0, end_frame: 8, reason: readable anticipation }\n        - { start_frame: 7, end_frame: 12, reason: overlapping concealment }",
    );
    let output = validate(&source);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ordered and non-overlapping"));
}

#[test]
fn rejects_parent_relative_track_with_a_different_cadence() {
    let output = validate(&sprite_manifest().replace(
        "          movement_steps: 3\n          keyframes:\n            - { frame: 0, asset: frame-landing.ppm",
        "          movement_steps: 4\n          keyframes:\n            - { frame: 0, asset: frame-landing.ppm",
    ));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("share movement cadence"));
}

#[test]
fn rejects_intentional_holds_covering_most_of_a_shot() {
    let output = validate(&sprite_manifest().replace(
        "start_frame: 0, end_frame: 8",
        "start_frame: 0, end_frame: 40",
    ));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot exceed half"));
}
