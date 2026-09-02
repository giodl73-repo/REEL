use std::{fs, path::Path, process::Command};

use serde_yaml::Value;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn sha256(path: &Path) -> String {
    Sha256::digest(fs::read(path).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn ffmpeg(args: &[&str]) {
    let output = Command::new("ffmpeg").args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let background = dir.path().join("background.ppm");
    fs::write(
        &background,
        b"P6\n2 2\n255\n\x20\x30\x40\x20\x30\x40\x20\x30\x40\x20\x30\x40",
    )
    .unwrap();
    let color = dir.path().join("color.nut");
    let matte = dir.path().join("matte.nut");
    let occlusion = dir.path().join("occlusion.nut");
    ffmpeg(&[
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=c=0xc08040:s=64x64:r=24:d=2",
        "-an",
        "-c:v",
        "ffv1",
        color.to_str().unwrap(),
    ]);
    ffmpeg(&[
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=c=white:s=64x64:r=24:d=2",
        "-vf",
        "format=gray",
        "-an",
        "-c:v",
        "ffv1",
        matte.to_str().unwrap(),
    ]);
    ffmpeg(&[
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=c=white:s=64x64:r=24:d=2",
        "-vf",
        "drawbox=x=32:y=0:w=32:h=64:color=black:t=fill,format=gray",
        "-an",
        "-c:v",
        "ffv1",
        occlusion.to_str().unwrap(),
    ]);

    let mut manifest: Value = serde_yaml::from_str(
        &fs::read_to_string("manifests/fixtures/vertical-sound-off/manifest.yaml").unwrap(),
    )
    .unwrap();
    manifest["scenes"].as_sequence_mut().unwrap()[0]["duration_seconds"] = Value::from(2.0);
    manifest["shots"].as_sequence_mut().unwrap().truncate(1);
    let shot = &mut manifest["shots"].as_sequence_mut().unwrap()[0];
    shot["duration_seconds"] = Value::from(2.0);
    shot["visual_asset"] = Value::from("background.ppm");
    shot["motion"] = Value::from("hold");
    shot["effect_passes"] = serde_yaml::from_str(&format!(
        r#"
- id: synthetic-dust
  color: {{ path: color.nut, sha256: {} }}
  matte: {{ path: matte.nut, sha256: {} }}
  occlusion_matte: {{ path: occlusion.nut, sha256: {} }}
  alpha_mode: separate-matte
  composite_operator: over
  color_space: srgb
  alpha_mode_detail: straight
  timing_fps: 24
  duration_frames: 48
  placement: {{ space: normalized, x: 0.0, y: 0.0, width: 1.0, height: 1.0 }}
  visible_start_frame: 0
  visible_end_frame: 47
  z_index: 10
"#,
        sha256(&color),
        sha256(&matte),
        sha256(&occlusion)
    ))
    .unwrap();
    manifest["narration_cues"]
        .as_sequence_mut()
        .unwrap()
        .truncate(1);
    manifest["narration_cues"].as_sequence_mut().unwrap()[0]["duration_seconds"] = Value::from(2.0);
    manifest["platforms"].as_sequence_mut().unwrap()[0]["target_duration_seconds"] =
        Value::from(2.0);
    manifest["exports"].as_sequence_mut().unwrap()[0]["duration_seconds"] = Value::from(2.0);
    let path = dir.path().join("manifest.yaml");
    fs::write(&path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    (dir, path)
}

#[test]
#[cfg_attr(
    windows,
    ignore = "requires FFmpeg/ffprobe through WSL on Windows; exercised by Linux CI"
)]
fn effect_pass_is_hash_pinned_occluded_and_visible_in_dry_run_graph() {
    let (dir, manifest) = fixture();
    let output = dir.path().join("effect.mp4");
    let result = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-render")
        .arg(&manifest)
        .arg("--asset-root")
        .arg(dir.path())
        .arg("--silent")
        .arg("--no-captions")
        .arg("--output")
        .arg(&output)
        .arg("--width")
        .arg("320")
        .arg("--height")
        .arg("180")
        .arg("--fps")
        .arg("24")
        .arg("--dry-run")
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report = fs::read_to_string(output.with_extension("artifacts.json")).unwrap();
    assert!(report.contains("alphamerge"));
    assert!(report.contains("blend=all_mode=multiply"));
    assert!(report.contains("overlay=x=0:y=0"));
    assert!(report.contains("\"effect_passes\": 1"));
    assert!(report.contains("effect-occlusion-matte"));

    let real = dir.path().join("effect-real.mp4");
    let result = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-render")
        .arg(&manifest)
        .arg("--asset-root")
        .arg(dir.path())
        .arg("--silent")
        .arg("--no-captions")
        .arg("--disclosure")
        .arg("")
        .arg("--output")
        .arg(&real)
        .arg("--width")
        .arg("320")
        .arg("--height")
        .arg("180")
        .arg("--fps")
        .arg("24")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let check = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("effect-pass-check")
        .arg(real.with_extension("artifacts.json"))
        .arg("--output")
        .arg("json")
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(String::from_utf8_lossy(&check.stdout).contains("\"passed\": true"));

    let repeated = dir.path().join("effect-repeat.mp4");
    let repeat = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-render")
        .arg(&manifest)
        .arg("--asset-root")
        .arg(dir.path())
        .arg("--silent")
        .arg("--no-captions")
        .arg("--disclosure")
        .arg("")
        .arg("--output")
        .arg(&repeated)
        .arg("--width")
        .arg("320")
        .arg("--height")
        .arg("180")
        .arg("--fps")
        .arg("24")
        .output()
        .unwrap();
    assert!(
        repeat.status.success(),
        "{}",
        String::from_utf8_lossy(&repeat.stderr)
    );
    assert_eq!(sha256(&real), sha256(&repeated));

    let alternate_matte = dir.path().join("alternate-matte.nut");
    ffmpeg(&[
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=c=gray:s=64x64:r=24:d=2",
        "-vf",
        "format=gray",
        "-an",
        "-c:v",
        "ffv1",
        alternate_matte.to_str().unwrap(),
    ]);
    let original_manifest = fs::read_to_string(&manifest).unwrap();
    let original_matte_hash = sha256(&dir.path().join("matte.nut"));
    let alternate = original_manifest
        .replace("path: matte.nut", "path: alternate-matte.nut")
        .replace(&original_matte_hash, &sha256(&alternate_matte));
    fs::write(&manifest, alternate).unwrap();
    let changed = dir.path().join("effect-changed-matte.mp4");
    let changed_render = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-render")
        .arg(&manifest)
        .arg("--asset-root")
        .arg(dir.path())
        .arg("--silent")
        .arg("--no-captions")
        .arg("--disclosure")
        .arg("")
        .arg("--output")
        .arg(&changed)
        .arg("--width")
        .arg("320")
        .arg("--height")
        .arg("180")
        .arg("--fps")
        .arg("24")
        .output()
        .unwrap();
    assert!(
        changed_render.status.success(),
        "{}",
        String::from_utf8_lossy(&changed_render.stderr)
    );
    assert_ne!(sha256(&real), sha256(&changed));
    let changed_report = fs::read_to_string(changed.with_extension("artifacts.json")).unwrap();
    assert!(changed_report.contains(&sha256(&alternate_matte)));
}

#[test]
fn effect_pass_fails_closed_for_hash_duration_and_asset_root_errors() {
    let (dir, manifest) = fixture();
    let original = fs::read_to_string(&manifest).unwrap();
    for (name, changed, expected) in [
        (
            "hash",
            original.replacen(
                "sha256: ",
                "sha256: 0000000000000000000000000000000000000000000000000000000000000000 # ",
                1,
            ),
            "hash mismatch",
        ),
        (
            "duration",
            original.replace("duration_frames: 48", "duration_frames: 47"),
            "invalid timing or visibility",
        ),
    ] {
        fs::write(&manifest, changed).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_reel"))
            .arg("animatic-render")
            .arg(&manifest)
            .arg("--asset-root")
            .arg(dir.path())
            .arg("--silent")
            .arg("--no-captions")
            .arg("--output")
            .arg(dir.path().join(format!("{name}.mp4")))
            .arg("--dry-run")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let outside = dir.path().parent().unwrap().join("outside-effect.nut");
    fs::copy(dir.path().join("color.nut"), &outside).unwrap();
    fs::write(
        &manifest,
        original.replace("path: color.nut", "path: ../outside-effect.nut"),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-render")
        .arg(&manifest)
        .arg("--asset-root")
        .arg(dir.path())
        .arg("--silent")
        .arg("--no-captions")
        .arg("--output")
        .arg(dir.path().join("escape.mp4"))
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("escapes asset root"));
    fs::remove_file(outside).unwrap();
}
