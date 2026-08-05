use std::{fs, process::Command};

use tempfile::tempdir;

const FIXTURE: &str = "manifests/fixtures/two-speaker-untimed/planning.yaml";
const CUES: &str = "manifests/fixtures/two-speaker-untimed/cue-measurements.yaml";

#[test]
fn cli_validates_plans_and_conforms_the_sanitized_fixture() {
    let binary = env!("CARGO_BIN_EXE_reel");
    let validate = Command::new(binary)
        .args(["validate", FIXTURE, "--output", "json"])
        .output()
        .expect("validate command runs");
    assert!(
        validate.status.success(),
        "{}",
        String::from_utf8_lossy(&validate.stderr)
    );
    assert!(String::from_utf8_lossy(&validate.stdout).contains("\"timing_status\": \"untimed\""));

    let plan = Command::new(binary)
        .args(["plan", FIXTURE, "--output", "json"])
        .output()
        .expect("plan command runs");
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    assert!(String::from_utf8_lossy(&plan.stdout).contains("\"duration_ms\": null"));

    let gated = Command::new(binary)
        .args(["scene-preview", FIXTURE, "scene-01", "private-review"])
        .output()
        .expect("gated preview command runs");
    assert!(!gated.status.success());
    assert!(String::from_utf8_lossy(&gated.stderr).contains("timing not conformed"));

    let dir = tempdir().unwrap();
    let packet = dir.path().join("packet");
    let conform = Command::new(binary)
        .arg("conform")
        .arg(FIXTURE)
        .args([
            "--cues",
            CUES,
            "--speaker-tempo",
            "narrator=85",
            "--output-dir",
        ])
        .arg(&packet)
        .args(["--output", "json"])
        .output()
        .expect("conform command runs");
    assert!(
        conform.status.success(),
        "{}",
        String::from_utf8_lossy(&conform.stderr)
    );
    assert!(packet.join("manifest.yaml").is_file());
    assert!(packet.join("captions.srt").is_file());
    assert!(packet.join("lineage.json").is_file());
    let exported_captions = dir.path().join("exported.srt");
    let caption_export = Command::new(binary)
        .arg("caption-export")
        .arg(packet.join("manifest.yaml"))
        .arg("--output")
        .arg(&exported_captions)
        .output()
        .expect("caption export runs");
    assert!(caption_export.status.success());
    assert_eq!(
        fs::read_to_string(&exported_captions).unwrap(),
        fs::read_to_string(packet.join("captions.srt")).unwrap()
    );
    let report = fs::read_to_string(packet.join("conform-report.json")).unwrap();
    assert!(report.contains("\"duration_ms\": 7500"));

    fs::write(dir.path().join("frame-01.png"), b"fixture-image-one").unwrap();
    fs::write(dir.path().join("frame-02.png"), b"fixture-image-two").unwrap();
    let audio = dir.path().join("guide.wav");
    fs::write(&audio, b"fixture-audio").unwrap();
    let video = dir.path().join("private-review.mp4");
    let render = Command::new(binary)
        .arg("animatic-render")
        .arg(packet.join("manifest.yaml"))
        .arg("--asset-root")
        .arg(dir.path())
        .arg("--audio")
        .arg(&audio)
        .arg("--narration-only-audio")
        .arg(&audio)
        .arg("--effects-music-audio")
        .arg(&audio)
        .arg("--captions")
        .arg(packet.join("captions.srt"))
        .arg("--output")
        .arg(&video)
        .args(["--dry-run", "--format", "json"])
        .output()
        .expect("dry-run animatic render runs");
    assert!(
        render.status.success(),
        "{}",
        String::from_utf8_lossy(&render.stderr)
    );
    assert!(video.with_extension("artifacts.json").is_file());
    assert!(
        dir.path()
            .join("private-review.narration-only.artifacts.json")
            .is_file()
    );
    assert!(
        dir.path()
            .join("private-review.effects-music.artifacts.json")
            .is_file()
    );
    assert!(!video.exists());
}
