use std::{fs, process::Command};

use tempfile::tempdir;

const BASE: &str = "manifests/fixtures/vertical-sound-off/manifest.yaml";

fn emission_manifest() -> String {
    fs::read_to_string(BASE).unwrap().replace(
        "    visual_asset: frame-hook.ppm\n",
        r#"    media_kind: sprite-animation
    sprite_animation:
      background: frame-hook.ppm
      timing_fps: 24
      sprites:
        - id: performer
          movement: stepped
          movement_steps: 3
          keyframes:
            - { frame: 0, asset: frame-hook.ppm, x: 0.30, y: 0.60, width: 0.25 }
            - { frame: 65, asset: frame-landing.ppm, x: 0.60, y: 0.45, width: 0.20 }
      emissions:
        - id: contact-dust
          asset: frame-landing.ppm
          parent: performer
          frame: 12
          duration_frames: 8
          offset_x: -0.20
          offset_y: 0.35
          width: 0.08
          end_width: 0.12
          drift_x: -0.03
          drift_y: 0.01
          rotation_degrees: -5
          end_rotation_degrees: -20
          fade_out_frames: 5
          z_index: 20
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
fn validates_spawn_and_detach_emission_contract() {
    let output = validate(&emission_manifest());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["sprite_animation_events"], 1);
}

#[test]
fn rejects_emission_with_unknown_parent() {
    let output = validate(&emission_manifest().replace("parent: performer", "parent: missing"));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown parent missing"));
}

#[test]
fn rejects_emission_fade_longer_than_lifetime() {
    let output = validate(&emission_manifest().replace("fade_out_frames: 5", "fade_out_frames: 9"));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("fade exceeds its duration"));
}
