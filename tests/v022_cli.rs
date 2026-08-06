use std::{fs, process::Command};

use serde_json::Value;
use tempfile::tempdir;

const MANIFEST: &str = "manifests/fixtures/smooth-motion/manifest.yaml";
const ASSETS: &str = "manifests/fixtures/smooth-motion";
const CAPTIONS: &str = "manifests/fixtures/smooth-motion/captions.srt";

fn dry_run(extra: &[&str]) -> (tempfile::TempDir, std::process::Output, Value) {
    let dir = tempdir().unwrap();
    let video = dir.path().join("proof.mp4");
    let mut command = Command::new(env!("CARGO_BIN_EXE_reel"));
    command
        .args(["animatic-render", MANIFEST, "--asset-root", ASSETS])
        .arg("--silent")
        .args(["--captions", CAPTIONS, "--output"])
        .arg(&video)
        .args(["--dry-run", "--format", "json"])
        .args(extra);
    let output = command.output().expect("dry-run command executes");
    let report: Value = serde_json::from_slice(&output.stdout).expect("report is json");
    (dir, output, report)
}

#[test]
fn smooth_motion_is_the_default_and_records_complete_lineage() {
    let (dir, output, report) = dry_run(&[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = &report[0];
    assert_eq!(report["tool_version"], "0.2.11");
    assert!(report["render_environment"].is_null());
    assert_eq!(report["motion"]["backend"], "ffmpeg-perspective");
    assert_eq!(report["motion"]["quality"], "smooth");
    assert_eq!(report["motion"]["interpolation"], "cubic");
    assert_eq!(report["motion"]["curve"], "ease-in-out");
    assert_eq!(report["motion"]["working_width"], 1280);
    assert_eq!(report["motion"]["working_height"], 720);
    assert_eq!(report["motion"]["fps"], 24);
    assert_eq!(report["motion"]["perspective_filter_instances"], 1);
    assert_eq!(report["motion"]["maximum_estimated_peak_memory_mib"], 2048);
    assert!(report["motion"]["safety"][0]["passed"].as_bool().unwrap());
    let args = report["command_arguments"].as_array().unwrap();
    assert!(args.iter().any(|arg| {
        arg.as_str()
            .is_some_and(|text| text.contains("perspective=") && text.contains("eval=frame"))
    }));
    assert!(args.iter().any(|arg| {
        arg.as_str()
            .is_some_and(|text| text.contains("framerate=fps=24,settb=AVTB"))
    }));
    assert!(!dir.path().join("proof.mp4").exists());
    assert!(dir.path().join("proof.artifacts.json").exists());
}

#[test]
fn legacy_mode_preserves_the_v021_zoompan_path() {
    let (_dir, output, report) = dry_run(&["--motion-quality", "legacy"]);
    assert!(output.status.success());
    let report = &report[0];
    assert_eq!(report["motion"]["backend"], "ffmpeg-zoompan");
    assert_eq!(report["motion"]["quality"], "legacy");
    assert_eq!(report["motion"]["curve"], "legacy-linear");
    assert_eq!(
        report["motion"]["quality_override"],
        "legacy deterministic reproduction"
    );
    let args = report["command_arguments"].as_array().unwrap();
    assert!(args.iter().any(|arg| {
        arg.as_str()
            .is_some_and(|text| text.contains("zoompan") && !text.contains("perspective="))
    }));
}

#[test]
fn renderer_refuses_overwrite_and_infeasible_quality_without_partial_reports() {
    let dir = tempdir().unwrap();
    let video = dir.path().join("existing.mp4");
    fs::write(&video, b"preserve-me").unwrap();
    let overwrite = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["animatic-render", MANIFEST, "--asset-root", ASSETS])
        .arg("--silent")
        .args(["--captions", CAPTIONS, "--output"])
        .arg(&video)
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(!overwrite.status.success());
    assert!(String::from_utf8_lossy(&overwrite.stderr).contains("refusing to overwrite"));
    assert_eq!(fs::read(&video).unwrap(), b"preserve-me");
    assert!(!video.with_extension("artifacts.json").exists());

    let oversized = dir.path().join("oversized.mp4");
    let infeasible = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["animatic-render", MANIFEST, "--asset-root", ASSETS])
        .arg("--silent")
        .args(["--captions", CAPTIONS, "--output"])
        .arg(&oversized)
        .args(["--width", "7680", "--height", "4320", "--dry-run"])
        .output()
        .unwrap();
    assert!(!infeasible.status.success());
    assert!(String::from_utf8_lossy(&infeasible.stderr).contains("infeasible"));
    assert!(!oversized.exists());
    assert!(!oversized.with_extension("artifacts.json").exists());
}

#[test]
#[ignore = "requires external FFmpeg/ffprobe"]
fn real_sanitized_pan_makes_legacy_fail_and_smooth_pass() {
    let dir = tempdir().unwrap();
    let smooth = dir.path().join("smooth.mp4");
    let legacy = dir.path().join("legacy.mp4");
    for (quality, video) in [("smooth", &smooth), ("legacy", &legacy)] {
        let render = Command::new(env!("CARGO_BIN_EXE_reel"))
            .args(["animatic-render", MANIFEST, "--asset-root", ASSETS])
            .arg("--silent")
            .args(["--captions", CAPTIONS, "--output"])
            .arg(video)
            .args(["--motion-quality", quality, "--format", "json"])
            .output()
            .unwrap();
        assert!(
            render.status.success(),
            "{}",
            String::from_utf8_lossy(&render.stderr)
        );
    }
    let smooth_analysis = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("motion-analyze")
        .arg(&smooth)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(smooth_analysis.status.success());
    let smooth_report: Value = serde_json::from_slice(&smooth_analysis.stdout).unwrap();
    assert!(smooth_report["passed"].as_bool().unwrap());
    assert!(smooth_report["near_stationary_fraction"].as_f64().unwrap() <= 0.10);
    let smooth_artifact: Value =
        serde_json::from_slice(&fs::read(smooth.with_extension("artifacts.json")).unwrap())
            .unwrap();
    assert_eq!(smooth_artifact["fps"], 24);
    assert_eq!(smooth_artifact["width"], 1280);
    assert_eq!(smooth_artifact["height"], 720);
    assert_eq!(smooth_artifact["output_duration_ms"], 20_000);
    assert!(
        smooth_artifact["render_environment"]["passed"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(
        smooth_artifact["render_environment"]["checks"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    assert_eq!(
        smooth_artifact["render_environment"]["fingerprint_sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );

    let motion_check = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("motion-check")
        .arg(MANIFEST)
        .arg(&smooth)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(
        motion_check.status.success(),
        "{}",
        String::from_utf8_lossy(&motion_check.stderr)
    );
    let motion_report: Value = serde_json::from_slice(&motion_check.stdout).unwrap();
    assert_eq!(motion_report["shots"][0]["expectation"], "moving");
    assert!(motion_report["shots"][0]["passed"].as_bool().unwrap());
    assert!(motion_report["safety"][0]["passed"].as_bool().unwrap());

    let artifact_path = smooth.with_extension("artifacts.json");
    let animatic_check = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-check")
        .arg(&artifact_path)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(
        animatic_check.status.success(),
        "{}",
        String::from_utf8_lossy(&animatic_check.stderr)
    );
    let verification: Value = serde_json::from_slice(&animatic_check.stdout).unwrap();
    assert!(verification["passed"].as_bool().unwrap());
    assert_eq!(verification["codec"], "h264");
    assert_eq!(verification["pixel_format"], "yuv420p");
    assert_eq!(verification["render_capabilities"], 7);
    assert_eq!(
        verification["render_environment_fingerprint"]
            .as_str()
            .unwrap()
            .len(),
        64
    );

    let receipt_path = dir.path().join("shareable.receipt.json");
    let receipt_command = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-receipt")
        .arg(&artifact_path)
        .arg("--output")
        .arg(&receipt_path)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert!(
        receipt_command.status.success(),
        "{}",
        String::from_utf8_lossy(&receipt_command.stderr)
    );
    let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    assert_eq!(receipt["schema"], "reel.animatic-receipt.v0.1");
    assert_eq!(receipt["output_sha256"], smooth_artifact["output_sha256"]);
    assert!(receipt["verified"].as_bool().unwrap());
    assert_eq!(
        receipt["render_environment_fingerprint"],
        smooth_artifact["render_environment"]["fingerprint_sha256"]
    );
    let receipt_text = serde_json::to_string(&receipt).unwrap();
    assert!(!receipt_text.contains(MANIFEST));
    assert!(!receipt_text.contains(&dir.path().display().to_string()));
    assert!(!receipt_text.contains("artifact_manifest"));
    assert!(!receipt_text.contains("\"path\""));

    let receipt_check = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-receipt-check")
        .arg(&receipt_path)
        .arg(&smooth)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(
        receipt_check.status.success(),
        "{}",
        String::from_utf8_lossy(&receipt_check.stderr)
    );
    let receipt_verification: Value = serde_json::from_slice(&receipt_check.stdout).unwrap();
    assert_eq!(
        receipt_verification["schema"],
        "reel.animatic-receipt-check.v0.1"
    );
    assert!(receipt_verification["passed"].as_bool().unwrap());
    assert_eq!(
        receipt_verification["video_sha256"],
        receipt["output_sha256"]
    );
    let verification_text = serde_json::to_string(&receipt_verification).unwrap();
    assert!(!verification_text.contains(&dir.path().display().to_string()));
    assert!(!verification_text.contains("\"path\""));

    let mut receipt_with_path = receipt.clone();
    receipt_with_path["path"] = Value::String(r"C:\private\frame.png".to_string());
    let receipt_with_path_file = dir.path().join("receipt-with-path.json");
    fs::write(
        &receipt_with_path_file,
        serde_json::to_vec_pretty(&receipt_with_path).unwrap(),
    )
    .unwrap();
    let receipt_with_path_check = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-receipt-check")
        .arg(&receipt_with_path_file)
        .arg(&smooth)
        .output()
        .unwrap();
    assert!(!receipt_with_path_check.status.success());

    let mut wrong_video_hash = receipt.clone();
    wrong_video_hash["output_sha256"] = Value::String("0".repeat(64));
    let wrong_video_hash_file = dir.path().join("wrong-video-hash.receipt.json");
    fs::write(
        &wrong_video_hash_file,
        serde_json::to_vec_pretty(&wrong_video_hash).unwrap(),
    )
    .unwrap();
    let wrong_video_hash_check = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-receipt-check")
        .arg(&wrong_video_hash_file)
        .arg(&smooth)
        .output()
        .unwrap();
    assert!(!wrong_video_hash_check.status.success());
    assert!(String::from_utf8_lossy(&wrong_video_hash_check.stderr).contains("SHA-256"));

    let preserved_receipt = fs::read(&receipt_path).unwrap();
    let overwrite_receipt = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-receipt")
        .arg(&artifact_path)
        .arg("--output")
        .arg(&receipt_path)
        .output()
        .unwrap();
    assert!(!overwrite_receipt.status.success());
    assert_eq!(fs::read(&receipt_path).unwrap(), preserved_receipt);

    let mut unknown_kind_artifact = smooth_artifact.clone();
    let visual_input = unknown_kind_artifact["inputs"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|input| input["kind"] == "visual")
        .unwrap();
    visual_input["kind"] = Value::String(r"C:\private\photo.jpg".to_string());
    let unknown_kind_path = dir.path().join("unknown-kind.artifacts.json");
    fs::write(
        &unknown_kind_path,
        serde_json::to_vec_pretty(&unknown_kind_artifact).unwrap(),
    )
    .unwrap();
    let unknown_kind_receipt_path = dir.path().join("unknown-kind.receipt.json");
    let unknown_kind_receipt = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-receipt")
        .arg(&unknown_kind_path)
        .arg("--output")
        .arg(&unknown_kind_receipt_path)
        .output()
        .unwrap();
    assert!(unknown_kind_receipt.status.success());
    let unknown_kind_text = fs::read_to_string(&unknown_kind_receipt_path).unwrap();
    assert!(!unknown_kind_text.contains("private"));
    let unknown_kind_value: Value = serde_json::from_str(&unknown_kind_text).unwrap();
    assert_eq!(unknown_kind_value["input_kinds"]["other"], 1);

    let mut tampered_artifact = smooth_artifact.clone();
    tampered_artifact["output_sha256"] = Value::String("0".repeat(64));
    let tampered_path = dir.path().join("tampered.artifacts.json");
    fs::write(
        &tampered_path,
        serde_json::to_vec_pretty(&tampered_artifact).unwrap(),
    )
    .unwrap();
    let tampered_check = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-check")
        .arg(&tampered_path)
        .output()
        .unwrap();
    assert!(!tampered_check.status.success());
    assert!(String::from_utf8_lossy(&tampered_check.stderr).contains("SHA-256"));

    let mut missing_environment = smooth_artifact.clone();
    missing_environment
        .as_object_mut()
        .unwrap()
        .remove("render_environment");
    let missing_environment_path = dir.path().join("missing-environment.artifacts.json");
    fs::write(
        &missing_environment_path,
        serde_json::to_vec_pretty(&missing_environment).unwrap(),
    )
    .unwrap();
    let missing_environment_check = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("animatic-check")
        .arg(&missing_environment_path)
        .output()
        .unwrap();
    assert!(!missing_environment_check.status.success());
    assert!(
        String::from_utf8_lossy(&missing_environment_check.stderr)
            .contains("no render environment lineage")
    );

    let legacy_analysis = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("motion-analyze")
        .arg(&legacy)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(!legacy_analysis.status.success());
    let legacy_report: Value = serde_json::from_slice(&legacy_analysis.stdout).unwrap();
    assert!(!legacy_report["passed"].as_bool().unwrap());
    assert!(legacy_report["near_stationary_fraction"].as_f64().unwrap() > 0.10);

    let manifest = fs::read_to_string(MANIFEST).unwrap();
    let captions = fs::read_to_string(CAPTIONS).unwrap();
    let captions_25 = dir.path().join("captions-25.srt");
    fs::write(
        &captions_25,
        captions.replace("00:00:20,000", "00:00:25,000"),
    )
    .unwrap();
    for motion in ["push", "pull"] {
        let variant_manifest = dir.path().join(format!("{motion}.yaml"));
        fs::write(
            &variant_manifest,
            manifest
                .replace("20.0", "25.0")
                .replace("motion: pan-right", &format!("motion: {motion}")),
        )
        .unwrap();
        let video = dir.path().join(format!("{motion}.mp4"));
        let render = Command::new(env!("CARGO_BIN_EXE_reel"))
            .arg("animatic-render")
            .arg(&variant_manifest)
            .args(["--asset-root", ASSETS, "--silent", "--captions"])
            .arg(&captions_25)
            .arg("--output")
            .arg(&video)
            .args(["--motion-quality", "smooth", "--format", "json"])
            .output()
            .unwrap();
        assert!(
            render.status.success(),
            "{}",
            String::from_utf8_lossy(&render.stderr)
        );
        let analysis = Command::new(env!("CARGO_BIN_EXE_reel"))
            .arg("motion-analyze")
            .arg(&video)
            .args(["--output", "json"])
            .output()
            .unwrap();
        assert!(analysis.status.success());
        let report: Value = serde_json::from_slice(&analysis.stdout).unwrap();
        assert!(report["near_stationary_fraction"].as_f64().unwrap() <= 0.10);
    }

    let captions_5 = dir.path().join("captions-5.srt");
    fs::write(
        &captions_5,
        captions.replace("00:00:20,000", "00:00:05,000"),
    )
    .unwrap();
    for motion in ["hold", "hold-dark"] {
        let variant_manifest = dir.path().join(format!("{motion}.yaml"));
        fs::write(
            &variant_manifest,
            manifest
                .replace("20.0", "5.0")
                .replace("motion: pan-right", &format!("motion: {motion}")),
        )
        .unwrap();
        let video = dir.path().join(format!("{motion}.mp4"));
        let render = Command::new(env!("CARGO_BIN_EXE_reel"))
            .arg("animatic-render")
            .arg(&variant_manifest)
            .args(["--asset-root", ASSETS, "--silent", "--captions"])
            .arg(&captions_5)
            .arg("--output")
            .arg(&video)
            .args(["--motion-quality", "smooth", "--format", "json"])
            .output()
            .unwrap();
        assert!(render.status.success());
        let artifact: Value =
            serde_json::from_slice(&fs::read(video.with_extension("artifacts.json")).unwrap())
                .unwrap();
        let command = artifact["command_arguments"].as_array().unwrap();
        assert!(command.iter().all(|arg| {
            arg.as_str()
                .is_none_or(|text| !text.contains("perspective="))
        }));
        let analysis = Command::new(env!("CARGO_BIN_EXE_reel"))
            .arg("motion-analyze")
            .arg(&video)
            .args(["--output", "json"])
            .output()
            .unwrap();
        assert!(!analysis.status.success());
        let report: Value = serde_json::from_slice(&analysis.stdout).unwrap();
        assert!(report["near_stationary_fraction"].as_f64().unwrap() > 0.85);
        let manifest_check = Command::new(env!("CARGO_BIN_EXE_reel"))
            .arg("motion-check")
            .arg(&variant_manifest)
            .arg(&video)
            .args(["--output", "json"])
            .output()
            .unwrap();
        assert!(manifest_check.status.success());
        let report: Value = serde_json::from_slice(&manifest_check.stdout).unwrap();
        assert_eq!(report["shots"][0]["expectation"], "stationary");
        assert!(report["shots"][0]["passed"].as_bool().unwrap());
    }

    let corrupt_audio = dir.path().join("corrupt.wav");
    fs::write(&corrupt_audio, b"not an audio stream").unwrap();
    let failed_video = dir.path().join("must-not-publish.mp4");
    let failed = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args([
            "animatic-render",
            MANIFEST,
            "--asset-root",
            ASSETS,
            "--audio",
        ])
        .arg(&corrupt_audio)
        .args(["--captions", CAPTIONS, "--output"])
        .arg(&failed_video)
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(!failed_video.exists());
    assert!(!failed_video.with_extension("artifacts.json").exists());
    assert!(fs::read_dir(dir.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".reel-render-")
    }));
}
