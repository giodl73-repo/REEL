use std::{fs, process::Command};

use serde_json::Value;
use tempfile::tempdir;

const MANIFEST: &str = "manifests/fixtures/speaker-captions/manifest.yaml";
const ASSETS: &str = "manifests/fixtures/speaker-captions";
const CAPTIONS: &str = "manifests/fixtures/speaker-captions/captions.srt";
const PRESENTATION: &str = "manifests/fixtures/speaker-captions/presentation.yaml";

#[test]
#[ignore = "requires external FFmpeg/ffprobe and renders a 42.155-second fixture"]
fn real_caption_layout_packet_is_bound_private_and_atomic() {
    let dir = tempdir().unwrap();
    let video = dir.path().join("speaker-captions.mp4");
    let rendered = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["animatic-render", MANIFEST, "--asset-root", ASSETS])
        .arg("--silent")
        .args([
            "--captions",
            CAPTIONS,
            "--caption-presentation",
            PRESENTATION,
        ])
        .args(["--speaker-label-policy", "first-entrance"])
        .args(["--caption-profile", "youtube-review"])
        .arg("--output")
        .arg(&video)
        .output()
        .unwrap();
    assert!(
        rendered.status.success(),
        "{}",
        String::from_utf8_lossy(&rendered.stderr)
    );

    let artifacts = video.with_extension("artifacts.json");
    let packet = dir.path().join("layout");
    let layout = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("caption-layout")
        .arg(&artifacts)
        .arg("--output-dir")
        .arg(&packet)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(
        layout.status.success(),
        "{}",
        String::from_utf8_lossy(&layout.stderr)
    );
    let report: Value = serde_json::from_slice(&layout.stdout).unwrap();
    assert_eq!(report["schema"], "reel.caption-layout.v0.1");
    assert_eq!(report["cues"].as_array().unwrap().len(), 11);
    assert_eq!(report["images"].as_array().unwrap().len(), 4);
    assert_eq!(report["images"][0]["srt_index"], 1);
    assert_eq!(report["images"][1]["srt_index"], 6);
    assert_eq!(report["images"][2]["srt_index"], 11);
    assert!(packet.join("first.png").is_file());
    assert!(packet.join("middle.png").is_file());
    assert!(packet.join("last.png").is_file());
    assert!(packet.join("contact-sheet.png").is_file());
    let layout_text = fs::read_to_string(packet.join("layout.json")).unwrap();
    assert!(!layout_text.contains("Guide One"));
    assert!(!layout_text.contains("guide-alpha"));
    assert!(layout_text.contains("no OCR"));

    let refused = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("caption-layout")
        .arg(&artifacts)
        .arg("--output-dir")
        .arg(&packet)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("absent or empty"));

    let tampered = dir.path().join("tampered.artifacts.json");
    let mut artifact: Value = serde_json::from_slice(&fs::read(&artifacts).unwrap()).unwrap();
    artifact["output_sha256"] = Value::String("0".repeat(64));
    fs::write(&tampered, serde_json::to_vec_pretty(&artifact).unwrap()).unwrap();
    let rejected_packet = dir.path().join("rejected-layout");
    let rejected = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("caption-layout")
        .arg(&tampered)
        .arg("--output-dir")
        .arg(&rejected_packet)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(!rejected_packet.exists());
}
