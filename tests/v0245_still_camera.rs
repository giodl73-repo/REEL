use std::{fs, path::Path, process::Command};

use reel::production::{
    self, SpriteCameraCurve, SpriteCameraKeyframe, StillCameraTrack, VisualFit,
};
use serde_json::Value;
use tempfile::tempdir;

const FIXTURE: &str = "manifests/fixtures/smooth-motion/manifest.yaml";
const ASSETS: &str = "manifests/fixtures/smooth-motion";
const CAPTIONS: &str = "manifests/fixtures/smooth-motion/captions.srt";

fn camera_manifest() -> production::ProductionManifest {
    let mut manifest = production::load(FIXTURE).unwrap().manifest;
    let shot = &mut manifest.shots[0];
    shot.motion.clear();
    shot.focal_point = None;
    shot.protected_regions.clear();
    shot.camera_track = Some(StillCameraTrack {
        timing_fps: 24,
        keyframes: vec![
            SpriteCameraKeyframe {
                frame: 0,
                center_x: 0.2,
                center_y: 0.5,
                zoom: 1.1,
                curve_to_next: SpriteCameraCurve::EaseInOut,
            },
            SpriteCameraKeyframe {
                frame: 240,
                center_x: 0.5,
                center_y: 0.48,
                zoom: 1.8,
                curve_to_next: SpriteCameraCurve::HoldThenBurst,
            },
            SpriteCameraKeyframe {
                frame: 479,
                center_x: 0.8,
                center_y: 0.46,
                zoom: 1.4,
                curve_to_next: SpriteCameraCurve::Linear,
            },
        ],
    });
    manifest
}

fn validation_error(manifest: &production::ProductionManifest) -> String {
    let temp = tempdir().unwrap();
    let manifest_path = temp.path().join("invalid-camera.yaml");
    fs::write(&manifest_path, serde_yaml::to_string(manifest).unwrap()).unwrap();
    let loaded = production::load(&manifest_path).unwrap();
    production::validate(&loaded).unwrap_err().to_string()
}

#[test]
fn dry_run_executes_a_bounded_still_camera_track() {
    let temp = tempdir().unwrap();
    let manifest_path = temp.path().join("camera.yaml");
    fs::write(
        &manifest_path,
        serde_yaml::to_string(&camera_manifest()).unwrap(),
    )
    .unwrap();
    let output_path = temp.path().join("camera.mp4");
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["animatic-render"])
        .arg(&manifest_path)
        .args([
            "--asset-root",
            ASSETS,
            "--silent",
            "--captions",
            CAPTIONS,
            "--output",
        ])
        .arg(&output_path)
        .args(["--dry-run", "--format", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let report = &report[0];
    assert_eq!(report["mixed_media"]["still_camera_tracks"], 1);
    assert_eq!(report["motion"]["shots"][0]["treatment"], "camera-track");
    assert!(
        report["command_arguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|argument| argument.as_str().is_some_and(|argument| {
                argument.contains("perspective=x0=")
                    && argument.contains("between(on\\,0\\,240)")
                    && argument.contains("max(0,min(W-W/")
                    && argument.contains("interpolation=cubic:sense=source:eval=frame")
            }))
    );
    assert!(!Path::new(&output_path).exists());
}

#[test]
fn validation_rejects_invalid_still_camera_contracts() {
    let mut manifest = camera_manifest();
    manifest.shots[0].camera_track.as_mut().unwrap().keyframes[1].frame = 0;
    assert!(validation_error(&manifest).contains("camera_track keyframe frames must increase"));

    let mut manifest = camera_manifest();
    manifest.shots[0].motion = "hold".to_string();
    assert!(validation_error(&manifest).contains("cannot combine camera_track with motion"));

    let mut manifest = camera_manifest();
    manifest.shots[0].camera_track.as_mut().unwrap().timing_fps = 0;
    assert!(validation_error(&manifest).contains("timing_fps must be between 1 and 60"));

    let mut manifest = camera_manifest();
    manifest.shots[0]
        .camera_track
        .as_mut()
        .unwrap()
        .keyframes
        .truncate(1);
    assert!(validation_error(&manifest).contains("must declare at least two keyframes"));

    let mut manifest = camera_manifest();
    manifest.shots[0].camera_track.as_mut().unwrap().keyframes[2].frame = 480;
    assert!(validation_error(&manifest).contains("falls outside the shot"));

    let mut manifest = camera_manifest();
    manifest.shots[0].camera_track.as_mut().unwrap().keyframes[1].center_x = -0.1;
    assert!(validation_error(&manifest).contains("keyframe geometry is invalid"));

    let mut manifest = camera_manifest();
    let track = manifest.shots[0].camera_track.as_mut().unwrap();
    let first = (
        track.keyframes[0].center_x,
        track.keyframes[0].center_y,
        track.keyframes[0].zoom,
    );
    for keyframe in &mut track.keyframes[1..] {
        keyframe.center_x = first.0;
        keyframe.center_y = first.1;
        keyframe.zoom = first.2;
    }
    assert!(validation_error(&manifest).contains("must change center or zoom"));

    let mut manifest = camera_manifest();
    manifest.shots[0].visual_fit = VisualFit::Contain;
    manifest.shots[0].focal_point = Some(production::FocalPoint { x: 0.5, y: 0.5 });
    assert!(
        validation_error(&manifest)
            .contains("cannot combine a contained camera_track with source-space focal_point")
    );

    let mut manifest = camera_manifest();
    manifest.shots[0].visual_fit = VisualFit::Contain;
    manifest.shots[0]
        .protected_regions
        .push(production::ProtectedRegion {
            id: "owner-label".to_string(),
            x: 0.1,
            y: 0.1,
            width: 0.2,
            height: 0.2,
        });
    assert!(
        validation_error(&manifest)
            .contains("cannot combine a contained camera_track with source-space focal_point")
    );
}

#[test]
fn dry_run_executes_a_contained_still_camera_track() {
    let temp = tempdir().unwrap();
    let mut manifest = camera_manifest();
    manifest.shots[0].visual_fit = VisualFit::Contain;
    let manifest_path = temp.path().join("contained-camera.yaml");
    fs::write(&manifest_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    let output_path = temp.path().join("contained-camera.mp4");
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["animatic-render"])
        .arg(&manifest_path)
        .args([
            "--asset-root",
            ASSETS,
            "--silent",
            "--captions",
            CAPTIONS,
            "--output",
        ])
        .arg(&output_path)
        .args(["--dry-run", "--format", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let report = &report[0];
    assert_eq!(report["motion"]["shots"][0]["visual_fit"], "contain");
    assert!(
        report["command_arguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|argument| argument.as_str().is_some_and(|argument| {
                argument.contains("force_original_aspect_ratio=decrease")
                    && argument.contains("pad=1280:720")
                    && argument.contains("perspective=x0=")
            }))
    );

    let legacy_output_path = temp.path().join("contained-camera-legacy.mp4");
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["animatic-render"])
        .arg(&manifest_path)
        .args([
            "--asset-root",
            ASSETS,
            "--silent",
            "--captions",
            CAPTIONS,
            "--output",
        ])
        .arg(&legacy_output_path)
        .args([
            "--motion-quality",
            "legacy",
            "--dry-run",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report[0]["command_arguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|argument| argument.as_str().is_some_and(|argument| {
                argument.contains("force_original_aspect_ratio=decrease")
                    && argument.contains("pad=1280:720")
                    && argument.contains("zoompan=z=")
            }))
    );
}
