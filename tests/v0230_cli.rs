use std::{fs, process::Command};

use tempfile::tempdir;

const BASE: &str = "manifests/fixtures/vertical-sound-off/manifest.yaml";

fn visibility_manifest() -> String {
    fs::read_to_string(BASE).unwrap().replace(
        "    visual_asset: frame-hook.ppm\n",
        r#"    media_kind: sprite-animation
    sprite_animation:
      background: frame-hook.ppm
      timing_fps: 24
      sprites:
        - id: late-arrival
          visible_start_frame: 8
          visible_end_frame: 40
          keyframes:
            - { frame: 0, asset: frame-hook.ppm, x: 0.30, y: 0.60, width: 0.25 }
            - { frame: 65, asset: frame-landing.ppm, x: 0.60, y: 0.45, width: 0.20 }
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
fn validates_inclusive_sprite_visibility_window() {
    let output = validate(&visibility_manifest());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn rejects_partial_sprite_visibility_window() {
    let source = visibility_manifest().replace("          visible_end_frame: 40\n", "");
    let output = validate(&source);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("must declare both visibility frames or neither")
    );
}

#[test]
fn choreography_cli_resolves_performer_visibility_beats() {
    let source = r##"
schema: reel.choreography.v0.1
fps: 24
duration_frames: 72
stage:
  marks:
    entrance: { x: 0.2, y: 0.5 }
    action: { x: 0.7, y: 0.5 }
beats:
  - { id: start, frame: 0 }
  - { id: enter, frame: 12 }
  - { id: contact, frame: 36 }
  - { id: exit, frame: 60 }
performers:
  observer:
    start: entrance
  arrival:
    start: entrance
    visible_between: [enter, exit]
    phrases:
      - { action: approach, to: action, between: [enter, contact] }
"##;
    let directory = tempdir().unwrap();
    let path = directory.path().join("choreography.yaml");
    fs::write(&path, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("choreography-compile")
        .arg(path)
        .arg("--output")
        .arg("json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arrival = plan["performers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|performer| performer["id"] == "arrival")
        .unwrap();
    assert_eq!(arrival["visible_start_frame"], 12);
    assert_eq!(arrival["visible_end_frame"], 60);
}

#[test]
fn choreography_asset_binding_can_reduce_review_sampling() {
    let directory = tempdir().unwrap();
    let source = "manifests/fixtures/choreography/simple-handoff.yaml";
    let default_assets = "manifests/fixtures/choreography/assets.yaml";
    let lean_assets = directory.path().join("assets.yaml");
    let lean_source = fs::read_to_string(default_assets).unwrap().replace(
        "background: background.ppm\n",
        "background: background.ppm\nperformer_path_subdivisions: 1\nprop_path_subdivisions: 1\n",
    );
    fs::write(&lean_assets, lean_source).unwrap();

    let default_output = directory.path().join("default.yaml");
    let lean_output = directory.path().join("lean.yaml");
    for (assets, output) in [
        (std::path::Path::new(default_assets), &default_output),
        (lean_assets.as_path(), &lean_output),
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_reel"))
            .arg("choreography-sprite-manifest")
            .arg(source)
            .arg(assets)
            .arg("--output-path")
            .arg(output)
            .arg("--output")
            .arg("json")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    let keyframe_count = |path: &std::path::Path| {
        let manifest: serde_yaml::Value =
            serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        manifest["shots"][0]["sprite_animation"]["sprites"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|sprite| sprite["keyframes"].as_sequence().unwrap().len())
            .sum::<usize>()
    };
    assert!(keyframe_count(&lean_output) < keyframe_count(&default_output));
}
