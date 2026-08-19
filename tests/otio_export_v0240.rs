use std::{fs, path::Path, process::Command};

use serde_json::Value;
use tempfile::tempdir;

fn manifest(timing_status: &str) -> String {
    format!(
        r#"manifest_version: reel.manifest.v0.2
profile: animatic
timing_status: {timing_status}
work: otio-fixture
title: OTIO fixture
source_scenario: {{}}
format: short-film
style: storyboard-animatic
audience: {{}}
platforms:
  - {{ name: private-review, aspect_ratio: "16:9", target_duration_seconds: 2.01, sound_optional: false }}
continuity: {{}}
scenes:
  - id: scene-01
    duration_seconds: 2.01
shots:
  - id: shot-01
    scene_id: scene-01
    start_seconds: 0
    duration_seconds: 1.005
    visual_prompt: "private prompt must not export"
    visual_asset: "C:\\private\\candidate.png"
    visual_asset_status: candidate-unreviewed
    media_kind: still
    source_in_seconds: 0
    transition_out: measured cut
  - id: shot-02
    scene_id: scene-01
    start_seconds: 1.005
    duration_seconds: 1.005
    visual_prompt: "second private prompt"
    visual_asset_status: planned-unrendered
    media_kind: video
    source_in_seconds: 2.5
    transition_out: hold into cut
speakers: []
narration_cues: []
protected_pauses: []
audio_events: []
beat_markers: []
source_ranges: []
omissions: []
audio: {{}}
captions: {{}}
renderer_assumptions: {{}}
exports:
  - {{ id: private-review, filename: fixture.mp4, aspect_ratio: "16:9", duration_seconds: 2.01 }}
review: {{}}
"#
    )
}

fn write_manifest(path: &Path, timing_status: &str) {
    fs::write(path, manifest(timing_status)).unwrap();
}

#[test]
fn exports_exact_offline_picture_timeline_without_private_media_or_authority() {
    let directory = tempdir().unwrap();
    let manifest_path = directory.path().join("manifest.yaml");
    let output_path = directory.path().join("timeline.otio");
    write_manifest(&manifest_path, "conformed");

    let report = reel::otio_export::export(&manifest_path, &output_path).unwrap();
    assert_eq!(report.timebase_rate, 1000);
    assert_eq!(report.track_count, 1);
    assert_eq!(report.clip_count, 2);
    assert_eq!(report.duration_ms, 2010);
    assert_eq!(report.offline_media_references, 2);
    assert!(report.picture_track_only);
    assert!(!report.media_paths_exported);
    assert!(!report.transitions_mapped);
    assert!(!report.audio_exported);
    assert!(report.human_authority_required);
    assert!(!report.creative_approved);
    assert!(!report.rights_approved);
    assert!(!report.publication_approved);
    assert!(!report.release_approved);

    let text = fs::read_to_string(&output_path).unwrap();
    assert!(!text.contains("C:\\\\private"));
    assert!(!text.contains("private prompt"));
    assert!(!text.contains("measured cut"));
    assert!(!text.contains("hold into cut"));
    assert!(!text.contains("\"target_url\""));
    let timeline: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(timeline["OTIO_SCHEMA"], "Timeline.1");
    assert_eq!(timeline["tracks"]["OTIO_SCHEMA"], "Stack.1");
    let track = &timeline["tracks"]["children"][0];
    assert_eq!(track["OTIO_SCHEMA"], "Track.1");
    assert_eq!(track["kind"], "Video");
    let clips = track["children"].as_array().unwrap();
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[0]["OTIO_SCHEMA"], "Clip.2");
    assert_eq!(clips[0]["source_range"]["duration"]["rate"], 1000);
    assert_eq!(clips[0]["source_range"]["duration"]["value"], 1005);
    assert_eq!(clips[1]["source_range"]["start_time"]["value"], 2500);
    assert_eq!(
        clips[0]["media_references"]["DEFAULT_MEDIA"]["OTIO_SCHEMA"],
        "MissingReference.1"
    );
    assert_eq!(
        clips[0]["metadata"]["reel"]["visual_asset_status"],
        "candidate"
    );
    assert_eq!(
        clips[0]["metadata"]["reel"]["transition_intent_present"],
        true
    );
    assert_eq!(
        clips[0]["metadata"]["reel"]["transition_intent_sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(
        clips[1]["metadata"]["reel"]["visual_asset_status"],
        "planned-unrendered"
    );
    assert_eq!(clips[1]["metadata"]["reel"]["timeline_start_ms"], 1005);
    assert_eq!(
        timeline["metadata"]["reel"]["source_manifest_sha256"],
        report.source_manifest_sha256
    );
}

#[test]
fn rejects_provisional_timing_wrong_extension_and_existing_output() {
    let directory = tempdir().unwrap();
    let guide_path = directory.path().join("guide.yaml");
    write_manifest(&guide_path, "guide");
    assert!(
        reel::otio_export::export(&guide_path, directory.path().join("guide.otio"))
            .unwrap_err()
            .to_string()
            .contains("conformed or locked")
    );

    let conformed_path = directory.path().join("conformed.yaml");
    write_manifest(&conformed_path, "conformed");
    assert!(
        reel::otio_export::export(&conformed_path, directory.path().join("timeline.json"))
            .unwrap_err()
            .to_string()
            .contains(".otio extension")
    );

    let output_path = directory.path().join("timeline.otio");
    reel::otio_export::export(&conformed_path, &output_path).unwrap();
    assert!(
        reel::otio_export::export(&conformed_path, &output_path)
            .unwrap_err()
            .to_string()
            .contains("overwrite")
    );
}

#[test]
fn invalid_source_timeline_fails_before_export() {
    let directory = tempdir().unwrap();
    let manifest_path = directory.path().join("invalid.yaml");
    let invalid = manifest("conformed").replace("start_seconds: 1.005", "start_seconds: 1.006");
    fs::write(&manifest_path, invalid).unwrap();
    assert!(
        reel::otio_export::export(&manifest_path, directory.path().join("invalid.otio"))
            .unwrap_err()
            .to_string()
            .contains("expected 1005ms")
    );
}

#[test]
fn overflowing_source_timeline_fails_without_panicking_or_wrapping() {
    let directory = tempdir().unwrap();
    let manifest_path = directory.path().join("overflow.yaml");
    let overflow = manifest("conformed")
        .replace(
            "duration_seconds: 2.01",
            "duration_seconds: 20000000000000000",
        )
        .replace(
            "duration_seconds: 1.005",
            "duration_seconds: 10000000000000000",
        )
        .replace("start_seconds: 1.005", "start_seconds: 10000000000000000");
    fs::write(&manifest_path, overflow).unwrap();
    assert!(
        reel::otio_export::export(&manifest_path, directory.path().join("overflow.otio"))
            .unwrap_err()
            .to_string()
            .contains("duration exceeds supported range")
    );
}

#[test]
fn cli_help_exposes_otio_export() {
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("otio-export"));
}
