use std::{fs, process::Command};

use serde_json::Value;
use tempfile::tempdir;

const MANIFEST: &str = "manifests/fixtures/speaker-captions/manifest.yaml";
const ASSETS: &str = "manifests/fixtures/speaker-captions";
const CAPTIONS: &str = "manifests/fixtures/speaker-captions/captions.srt";
const PRESENTATION: &str = "manifests/fixtures/speaker-captions/presentation.yaml";

fn render_command(output: &std::path::Path, policy: &str, width: &str, height: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_reel"));
    command
        .args(["animatic-render", MANIFEST, "--asset-root", ASSETS])
        .arg("--silent")
        .args(["--captions", CAPTIONS])
        .args(["--speaker-label-policy", policy])
        .args(["--width", width, "--height", height])
        .arg("--output")
        .arg(output)
        .args(["--dry-run", "--format", "json"]);
    if policy != "none" {
        command.args(["--caption-presentation", PRESENTATION]);
    }
    if height.parse::<u32>().unwrap() > width.parse::<u32>().unwrap() {
        command.args(["--caption-profile", "phone-review"]);
    } else {
        command.args(["--caption-profile", "youtube-review"]);
    }
    command
}

#[test]
fn speaker_caption_policies_preserve_srt_and_timing_at_both_aspects() {
    let dir = tempdir().unwrap();
    let mut caption_hash = None;
    for (policy, expected_events) in [("none", 0_u64), ("first-entrance", 3), ("persistent", 11)] {
        for (width, height) in [("1280", "720"), ("720", "1280")] {
            let output = dir.path().join(format!("{policy}-{width}x{height}.mp4"));
            let result = render_command(&output, policy, width, height)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let reports: Value = serde_json::from_slice(&result.stdout).unwrap();
            let report = &reports[0];
            assert_eq!(report["duration_ms"], 42_155);
            assert_eq!(report["captions"]["check"]["cues"], 11);
            assert!(report["captions"]["check"]["passed"].as_bool().unwrap());
            assert_eq!(
                report["captions"]["label_events"].as_array().unwrap().len() as u64,
                expected_events
            );
            let current_hash = report["captions"]["captions_sha256"]
                .as_str()
                .unwrap()
                .to_string();
            if let Some(expected_hash) = &caption_hash {
                assert_eq!(&current_hash, expected_hash);
            } else {
                caption_hash = Some(current_hash);
            }
            let caption_region = &report["captions"]["style"]["caption_region"];
            let badge_region = &report["captions"]["style"]["badge_region"];
            assert!(
                badge_region["y"].as_u64().unwrap() + badge_region["height"].as_u64().unwrap()
                    <= caption_region["y"].as_u64().unwrap()
            );
            let command = report["command_arguments"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ");
            assert!(!command.contains("Internal Alpha Guide"));
            assert_eq!(command.contains("Guide One"), policy != "none");
            assert!(!output.exists());
            assert!(output.with_extension("artifacts.json").is_file());
        }
    }
}

#[test]
fn speaker_caption_preflight_failures_publish_nothing() {
    let dir = tempdir().unwrap();

    let mut unknown: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(PRESENTATION).unwrap()).unwrap();
    unknown["speakers"][0]["speaker_id"] = serde_yaml::Value::String("unknown".to_string());
    let unknown_path = dir.path().join("unknown.yaml");
    fs::write(&unknown_path, serde_yaml::to_string(&unknown).unwrap()).unwrap();
    let unknown_output = dir.path().join("unknown.mp4");
    let unknown_result = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["animatic-render", MANIFEST, "--asset-root", ASSETS])
        .arg("--silent")
        .args(["--captions", CAPTIONS, "--caption-presentation"])
        .arg(&unknown_path)
        .args(["--speaker-label-policy", "persistent", "--output"])
        .arg(&unknown_output)
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(!unknown_result.status.success());
    assert!(String::from_utf8_lossy(&unknown_result.stderr).contains("unknown speaker"));
    assert!(!unknown_output.exists());
    assert!(!unknown_output.with_extension("artifacts.json").exists());

    let unreadable = dir.path().join("unreadable.srt");
    fs::write(
        &unreadable,
        "1\n00:00:00,000 --> 00:00:00,500\nThis deliberately overlong caption line cannot be read in time.\n",
    )
    .unwrap();
    let unreadable_output = dir.path().join("unreadable.mp4");
    let unreadable_result = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["animatic-render", MANIFEST, "--asset-root", ASSETS])
        .arg("--silent")
        .arg("--captions")
        .arg(&unreadable)
        .args(["--output"])
        .arg(&unreadable_output)
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(!unreadable_result.status.success());
    assert!(
        String::from_utf8_lossy(&unreadable_result.stderr)
            .contains("caption accessibility preflight failed")
    );
    assert!(!unreadable_output.exists());
    assert!(!unreadable_output.with_extension("artifacts.json").exists());

    let infeasible_output = dir.path().join("infeasible.mp4");
    let infeasible_result = render_command(&infeasible_output, "none", "320", "180")
        .output()
        .unwrap();
    assert!(!infeasible_result.status.success());
    assert!(String::from_utf8_lossy(&infeasible_result.stderr).contains("at least 640x360"));
    assert!(!infeasible_output.exists());
    assert!(!infeasible_output.with_extension("artifacts.json").exists());

    let override_output = dir.path().join("undocumented-override.mp4");
    let override_result = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["animatic-render", MANIFEST, "--asset-root", ASSETS])
        .arg("--silent")
        .args(["--captions", CAPTIONS, "--max-caption-chars-per-line", "50"])
        .arg("--output")
        .arg(&override_output)
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(!override_result.status.success());
    assert!(String::from_utf8_lossy(&override_result.stderr).contains("policy-note"));
    assert!(!override_output.with_extension("artifacts.json").exists());
}

#[test]
#[ignore = "requires external FFmpeg/ffprobe and renders six 42.155-second variants"]
fn real_speaker_caption_matrix_verifies_artifacts_and_receipts() {
    let dir = tempdir().unwrap();
    let mut caption_hash = None;
    for (policy, expected_events) in [("none", 0_usize), ("first-entrance", 3), ("persistent", 11)]
    {
        for (width, height) in [("1280", "720"), ("720", "1280")] {
            let video = dir.path().join(format!("{policy}-{width}x{height}.mp4"));
            let mut render = render_command(&video, policy, width, height);
            let args = render
                .get_args()
                .map(|arg| arg.to_owned())
                .collect::<Vec<_>>();
            render = Command::new(env!("CARGO_BIN_EXE_reel"));
            for arg in args {
                if arg != "--dry-run" {
                    render.arg(arg);
                }
            }
            let result = render.output().unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let artifacts = video.with_extension("artifacts.json");
            let artifact: Value = serde_json::from_slice(&fs::read(&artifacts).unwrap()).unwrap();
            assert_eq!(
                artifact["captions"]["label_events"]
                    .as_array()
                    .unwrap()
                    .len(),
                expected_events
            );
            let current_hash = artifact["captions"]["captions_sha256"]
                .as_str()
                .unwrap()
                .to_string();
            if let Some(expected) = &caption_hash {
                assert_eq!(&current_hash, expected);
            } else {
                caption_hash = Some(current_hash);
            }
            let checked = Command::new(env!("CARGO_BIN_EXE_reel"))
                .arg("animatic-check")
                .arg(&artifacts)
                .args(["--output", "json"])
                .output()
                .unwrap();
            assert!(
                checked.status.success(),
                "{}",
                String::from_utf8_lossy(&checked.stderr)
            );

            if policy == "first-entrance" {
                let receipt = dir.path().join(format!("{width}x{height}.receipt.json"));
                let receipt_result = Command::new(env!("CARGO_BIN_EXE_reel"))
                    .arg("animatic-receipt")
                    .arg(&artifacts)
                    .arg("--output")
                    .arg(&receipt)
                    .output()
                    .unwrap();
                assert!(receipt_result.status.success());
                let receipt_text = fs::read_to_string(&receipt).unwrap();
                assert!(!receipt_text.contains("Guide One"));
                assert!(!receipt_text.contains("guide-alpha"));
                let receipt_check = Command::new(env!("CARGO_BIN_EXE_reel"))
                    .arg("animatic-receipt-check")
                    .arg(&receipt)
                    .arg(&video)
                    .output()
                    .unwrap();
                assert!(receipt_check.status.success());
            }
        }
    }
}
