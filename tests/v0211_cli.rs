use std::{fs, io::Write, path::Path, process::Command};

use serde_json::Value;
use tempfile::tempdir;

const MANIFEST: &str = "manifests/fixtures/vertical-sound-off/manifest.yaml";
const ASSETS: &str = "manifests/fixtures/vertical-sound-off";
const CAPTIONS: &str = "manifests/fixtures/vertical-sound-off/captions.srt";

#[test]
#[ignore = "requires external FFmpeg/ffprobe"]
fn audio_gate_measures_privately_rejects_clipping_and_reports_stem_margin() {
    let dir = tempdir().unwrap();
    let master = dir.path().join("private-master.wav");
    let clipped = dir.path().join("private-clipped.wav");
    let narration = dir.path().join("private-narration.wav");
    let effects = dir.path().join("private-effects.wav");
    write_sine_wav(&master, 6, 0.16);
    write_sine_wav(&clipped, 6, 2.0);
    write_sine_wav(&narration, 6, 0.16);
    write_sine_wav(&effects, 6, 0.04);

    let retained = dir.path().join("audio-check.json");
    let passing = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("audio-check")
        .arg(&master)
        .args(["--profile", "private-review", "--manifest", MANIFEST])
        .arg("--narration-stem")
        .arg(&narration)
        .arg("--effects-music-stem")
        .arg(&effects)
        .arg("--report")
        .arg(&retained)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(
        passing.status.success(),
        "{}",
        String::from_utf8_lossy(&passing.stderr)
    );
    let report: Value = serde_json::from_slice(&passing.stdout).unwrap();
    assert_eq!(report["schema"], "reel.audio-check.v0.1");
    assert_eq!(report["audio"]["duration_ms"], 6000);
    assert_eq!(report["audio"]["sample_rate_hz"], 48000);
    assert!(
        report["stem_margin"]["narration_margin_db"]
            .as_f64()
            .unwrap()
            > 11.0
    );
    assert!(report["passed"].as_bool().unwrap());
    let public_json = String::from_utf8(passing.stdout).unwrap();
    assert!(!public_json.contains("private-master"));
    assert!(!public_json.contains(&dir.path().display().to_string()));
    assert!(retained.is_file());

    let overwrite = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("audio-check")
        .arg(&master)
        .arg("--report")
        .arg(&retained)
        .output()
        .unwrap();
    assert!(!overwrite.status.success());
    assert!(String::from_utf8_lossy(&overwrite.stderr).contains("refusing to overwrite"));

    let failing = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("audio-check")
        .arg(&clipped)
        .args(["--profile", "private-review", "--output", "json"])
        .output()
        .unwrap();
    assert!(!failing.status.success());
    let failed: Value = serde_json::from_slice(&failing.stdout).unwrap();
    let codes = failed["violations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["code"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"true-peak"));
    assert!(codes.contains(&"near-clipping"));
    assert!(failed["audio"]["peak_samples_at_maximum"].as_u64().unwrap() > 0);

    let mismatched = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("audio-check")
        .arg(&master)
        .args([
            "--manifest",
            "manifests/fixtures/smooth-motion/manifest.yaml",
            "--output",
            "json",
        ])
        .output()
        .unwrap();
    assert!(!mismatched.status.success());
    let mismatch: Value = serde_json::from_slice(&mismatched.stdout).unwrap();
    assert!(
        mismatch["violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["code"] == "duration-mismatch")
    );
}

#[test]
#[ignore = "requires external FFmpeg/ffprobe and renders a six-second fixture"]
fn render_binds_passing_audio_report_and_rejects_tampering() {
    let dir = tempdir().unwrap();
    let audio = dir.path().join("master.wav");
    write_sine_wav(&audio, 6, 0.16);
    let retained = dir.path().join("audio-check.json");
    let checked = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("audio-check")
        .arg(&audio)
        .args(["--profile", "private-review", "--manifest", MANIFEST])
        .arg("--report")
        .arg(&retained)
        .output()
        .unwrap();
    assert!(checked.status.success());

    let video = dir.path().join("bound.mp4");
    let render = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["animatic-render", MANIFEST, "--asset-root", ASSETS])
        .arg("--audio")
        .arg(&audio)
        .arg("--audio-check-report")
        .arg(&retained)
        .args(["--captions", CAPTIONS, "--width", "720", "--height", "1280"])
        .arg("--output")
        .arg(&video)
        .output()
        .unwrap();
    assert!(
        render.status.success(),
        "{}",
        String::from_utf8_lossy(&render.stderr)
    );
    let artifacts = video.with_extension("artifacts.json");
    let artifact: Value = serde_json::from_slice(&fs::read(&artifacts).unwrap()).unwrap();
    assert_eq!(artifact["audio_quality"]["profile"], "private-review");
    assert_eq!(
        artifact["audio_quality"]["audio_sha256"],
        artifact["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|input| input["kind"] == "audio")
            .unwrap()["sha256"]
    );
    let verified = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-check")
        .arg(&artifacts)
        .output()
        .unwrap();
    assert!(verified.status.success());

    let changed = dir.path().join("changed.wav");
    write_sine_wav(&changed, 6, 0.15);
    let rejected_video = dir.path().join("rejected.mp4");
    let rejected = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["animatic-render", MANIFEST, "--asset-root", ASSETS])
        .arg("--audio")
        .arg(&changed)
        .arg("--audio-check-report")
        .arg(&retained)
        .args(["--captions", CAPTIONS, "--width", "720", "--height", "1280"])
        .arg("--output")
        .arg(&rejected_video)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("hash does not match"));
    assert!(!rejected_video.exists());
    assert!(!rejected_video.with_extension("artifacts.json").exists());
}

fn write_sine_wav(path: &Path, seconds: u32, amplitude: f64) {
    let sample_rate = 48_000_u32;
    let samples = sample_rate * seconds;
    let data_bytes = samples * 2;
    let mut file = fs::File::create(path).unwrap();
    file.write_all(b"RIFF").unwrap();
    file.write_all(&(36 + data_bytes).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt ").unwrap();
    file.write_all(&16_u32.to_le_bytes()).unwrap();
    file.write_all(&1_u16.to_le_bytes()).unwrap();
    file.write_all(&1_u16.to_le_bytes()).unwrap();
    file.write_all(&sample_rate.to_le_bytes()).unwrap();
    file.write_all(&(sample_rate * 2).to_le_bytes()).unwrap();
    file.write_all(&2_u16.to_le_bytes()).unwrap();
    file.write_all(&16_u16.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&data_bytes.to_le_bytes()).unwrap();
    for index in 0..samples {
        let phase = 2.0 * std::f64::consts::PI * 220.0 * f64::from(index) / f64::from(sample_rate);
        let sample = (phase.sin() * amplitude * f64::from(i16::MAX))
            .round()
            .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16;
        file.write_all(&sample.to_le_bytes()).unwrap();
    }
}
