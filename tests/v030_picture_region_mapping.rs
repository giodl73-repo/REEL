use std::{fs, path::Path, process::Command};

use reel::production::{
    self, FocalPoint, ProtectedRegion, SpriteCameraCurve, SpriteCameraKeyframe,
    StillCameraGeometry, StillCameraTrack, VisualFit,
};
use serde_json::Value;
use tempfile::tempdir;

const FIXTURE: &str = "manifests/fixtures/smooth-motion/manifest.yaml";
const ASSETS: &str = "manifests/fixtures/smooth-motion";
const CAPTIONS: &str = "manifests/fixtures/smooth-motion/captions.srt";

fn mapped_manifest() -> production::ProductionManifest {
    let mut manifest = production::load(FIXTURE).unwrap().manifest;
    let shot = &mut manifest.shots[0];
    let source = Path::new(ASSETS).join(shot.visual_asset.as_ref().unwrap());
    let (source_width, source_height) = image::image_dimensions(source).unwrap();
    shot.motion.clear();
    shot.visual_fit = VisualFit::Contain;
    shot.camera_track = Some(StillCameraTrack {
        timing_fps: 24,
        keyframes: vec![
            SpriteCameraKeyframe {
                frame: 0,
                center_x: 0.5,
                center_y: 0.5,
                zoom: 1.0,
                curve_to_next: SpriteCameraCurve::EaseInOut,
            },
            SpriteCameraKeyframe {
                frame: 479,
                center_x: 0.5,
                center_y: 0.5,
                zoom: 1.2,
                curve_to_next: SpriteCameraCurve::Linear,
            },
        ],
        geometry: Some(StillCameraGeometry {
            source_width,
            source_height,
            canvas_width: 1280,
            canvas_height: 520,
        }),
    });
    shot.focal_point = Some(FocalPoint { x: 0.5, y: 0.5 });
    shot.protected_regions = vec![ProtectedRegion {
        id: "center-label".to_string(),
        x: 0.45,
        y: 0.45,
        width: 0.1,
        height: 0.1,
    }];
    manifest
}

fn render(
    manifest: &production::ProductionManifest,
    output_name: &str,
    extra: &[&str],
) -> std::process::Output {
    let temp = tempdir().unwrap();
    let manifest_path = temp.path().join("manifest.yaml");
    fs::write(&manifest_path, serde_yaml::to_string(manifest).unwrap()).unwrap();
    Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["animatic-render"])
        .arg(&manifest_path)
        .args([
            "--asset-root",
            ASSETS,
            "--silent",
            "--captions",
            CAPTIONS,
            "--caption-picture-layout",
            "reserve-caption-band",
            "--output",
        ])
        .arg(temp.path().join(output_name))
        .args(["--dry-run", "--format", "json"])
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn reserve_caption_band_maps_source_safety_into_the_picture_canvas() {
    let manifest = mapped_manifest();
    let geometry = manifest.shots[0]
        .camera_track
        .as_ref()
        .unwrap()
        .geometry
        .as_ref()
        .unwrap();
    let output = render(&manifest, "mapped.mp4", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let mapping = &report[0]["motion"]["safety"][0]["source_to_canvas"];
    assert_eq!(mapping["source_width"], geometry.source_width);
    assert_eq!(mapping["source_height"], geometry.source_height);
    assert_eq!(mapping["canvas_width"], 1280);
    assert_eq!(mapping["canvas_height"], 520);
    assert_eq!(mapping["fitted_y"], 0);
    assert_eq!(mapping["fitted_height"], 520);
    assert_eq!(report[0]["motion"]["safety"][0]["focal_point_safe"], true);
    assert_eq!(
        report[0]["motion"]["safety"][0]["protected_regions"][0]["safe"],
        true
    );
    let fitted_width = mapping["fitted_width"].as_u64().unwrap();
    let fitted_x = mapping["fitted_x"].as_u64().unwrap();
    assert!(
        report[0]["command_arguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|argument| argument.as_str().is_some_and(|argument| {
                argument.contains(&format!(
                    "format=rgba,scale={fitted_width}:520,pad=1280:520:{fitted_x}:0:color=black"
                ))
            }))
    );

    let legacy = render(
        &manifest,
        "mapped-legacy.mp4",
        &["--motion-quality", "legacy"],
    );
    assert!(
        legacy.status.success(),
        "{}",
        String::from_utf8_lossy(&legacy.stderr)
    );
    let legacy: Value = serde_json::from_slice(&legacy.stdout).unwrap();
    assert!(
        legacy[0]["command_arguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|argument| argument.as_str().is_some_and(|argument| {
                argument.contains(&format!(
                    "format=rgba,scale={fitted_width}:520,pad=1280:520:{fitted_x}:0:color=black,zoompan="
                ))
            }))
    );
}

#[test]
fn mapped_camera_rejects_stale_source_and_canvas_geometry() {
    let mut stale_source = mapped_manifest();
    stale_source.shots[0]
        .camera_track
        .as_mut()
        .unwrap()
        .geometry
        .as_mut()
        .unwrap()
        .source_width += 1;
    let output = render(&stale_source, "stale-source.mp4", &[]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("does not match actual asset dimensions")
    );

    let mut stale_canvas = mapped_manifest();
    stale_canvas.shots[0]
        .camera_track
        .as_mut()
        .unwrap()
        .geometry
        .as_mut()
        .unwrap()
        .canvas_height = 518;
    let output = render(&stale_canvas, "stale-canvas.mp4", &[]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains(
        "camera_track geometry canvas 1280x518 does not match render picture canvas 1280x520"
    ));
}

#[test]
fn mapped_camera_still_rejects_an_unsafe_source_region() {
    let mut manifest = mapped_manifest();
    manifest.shots[0].focal_point = Some(FocalPoint { x: 0.0, y: 0.5 });
    manifest.shots[0].camera_track.as_mut().unwrap().keyframes[1].zoom = 2.0;
    let output = render(&manifest, "unsafe.mp4", &[]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("motion transform would crop a declared focal point")
    );
}

#[test]
fn reserve_caption_band_rejects_source_safety_without_a_camera_mapping() {
    let mut manifest = production::load(FIXTURE).unwrap().manifest;
    manifest.shots[0].motion = "hold".to_string();
    manifest.shots[0].visual_fit = VisualFit::Contain;
    let output = render(&manifest, "unmapped-hold.mp4", &[]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("requires visual_fit contain and camera_track geometry")
    );
}
