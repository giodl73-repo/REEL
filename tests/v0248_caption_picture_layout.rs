use std::{fs, process::Command};

use reel::production;
use serde_json::Value;
use tempfile::tempdir;

const MANIFEST: &str = "manifests/fixtures/smooth-motion/manifest.yaml";
const ASSETS: &str = "manifests/fixtures/smooth-motion";
const CAPTIONS: &str = "manifests/fixtures/smooth-motion/captions.srt";

fn dry_run(output_name: &str, extra: &[&str]) -> std::process::Output {
    let temp = tempdir().unwrap();
    let mut manifest = production::load(MANIFEST).unwrap().manifest;
    for shot in &mut manifest.shots {
        shot.focal_point = None;
        shot.protected_regions.clear();
    }
    let manifest_path = temp.path().join("manifest.yaml");
    fs::write(&manifest_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    let output_path = temp.path().join(output_name);
    let mut command = Command::new(env!("CARGO_BIN_EXE_reel"));
    command
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
        .arg(output_path)
        .args(["--dry-run", "--format", "json"])
        .args(extra);
    command.output().unwrap()
}

#[test]
fn reserve_caption_band_places_picture_before_caption_overlay() {
    let baseline = dry_run("overlay.mp4", &[]);
    assert!(
        baseline.status.success(),
        "{}",
        String::from_utf8_lossy(&baseline.stderr)
    );
    let baseline: Value = serde_json::from_slice(&baseline.stdout).unwrap();
    assert!(baseline[0]["captions"]["picture_layout"].is_null());
    assert!(
        baseline[0]["command_arguments"]
            .as_array()
            .unwrap()
            .iter()
            .all(|argument| argument
                .as_str()
                .is_none_or(|argument| !argument.contains("pad=1280:720:0:0:color=black")))
    );

    let reserved = dry_run(
        "reserved.mp4",
        &["--caption-picture-layout", "reserve-caption-band"],
    );
    assert!(
        reserved.status.success(),
        "{}",
        String::from_utf8_lossy(&reserved.stderr)
    );
    let reserved: Value = serde_json::from_slice(&reserved.stdout).unwrap();
    assert_eq!(
        reserved[0]["captions"]["picture_layout"]["strategy"],
        "reserve-caption-band"
    );
    assert_eq!(
        reserved[0]["captions"]["picture_layout"]["picture_region"],
        serde_json::json!({ "x": 0, "y": 0, "width": 1280, "height": 520 })
    );
    assert_eq!(reserved[0]["motion"]["working_width"], 1280);
    assert_eq!(reserved[0]["motion"]["working_height"], 520);
    assert!(
        reserved[0]["motion"]["estimated_peak_memory_mib"]
            .as_u64()
            .unwrap()
            < baseline[0]["motion"]["estimated_peak_memory_mib"]
                .as_u64()
                .unwrap()
    );
    assert!(
        reserved[0]["command_arguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|argument| argument.as_str().is_some_and(|argument| {
                argument.contains("crop=1280:520")
                    && argument.contains("pad=1280:720:0:0:color=black")
                    && argument.find("crop=1280:520") < argument.find("subtitles=filename=")
            }))
    );

    let legacy = dry_run(
        "reserved-legacy.mp4",
        &[
            "--caption-picture-layout",
            "reserve-caption-band",
            "--motion-quality",
            "legacy",
        ],
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
                argument.contains("zoompan=")
                    && argument.contains("s=1280x520")
                    && argument.contains("pad=1280:720:0:0:color=black")
            }))
    );
}

#[test]
fn reserve_caption_band_rejects_unmapped_source_safety_geometry() {
    let temp = tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["animatic-render", MANIFEST])
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
        .arg(temp.path().join("source-safety.mp4"))
        .args(["--dry-run", "--format", "json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("cannot map source-space focal_point or protected_regions")
    );
}

#[test]
fn reserve_caption_band_requires_caption_input() {
    let temp = tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["animatic-render", MANIFEST])
        .args([
            "--asset-root",
            ASSETS,
            "--silent",
            "--no-captions",
            "--caption-picture-layout",
            "reserve-caption-band",
            "--output",
        ])
        .arg(temp.path().join("captionless.mp4"))
        .args(["--dry-run", "--format", "json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("reserve-caption-band requires captions")
    );
}
