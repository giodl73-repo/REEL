use std::{fs, path::Path, process::Command};

use reel::exposure_sheet::{load, validate, write_report};
use serde_json::Value;
use tempfile::tempdir;

const FIXTURE: &str = "manifests/fixtures/exposure-sheet/simple-shot.yaml";

fn modified_fixture(root: &Path, replacement: (&str, &str)) -> std::path::PathBuf {
    let source = fs::read_to_string(FIXTURE)
        .unwrap()
        .replace(replacement.0, replacement.1);
    let manifest = fs::canonicalize("manifests/fixtures/shared-production/manifest.yaml").unwrap();
    let source = source.replace(
        "../shared-production/manifest.yaml",
        &manifest.display().to_string(),
    );
    let path = root.join("sheet.yaml");
    fs::write(&path, source).unwrap();
    path
}

fn cue_fixture(root: &Path, cue_id: &str) -> std::path::PathBuf {
    let manifest_path = root.join("manifest.yaml");
    let production = fs::read_to_string("manifests/fixtures/shared-production/manifest.yaml")
        .unwrap()
        .replace(
            "duration_seconds: 4.0 }\n  - { id: shot-middle-letter",
            "duration_seconds: 4.0, narration_cue_ids: [cue-fallback] }\n  - { id: shot-middle-letter",
        );
    let manifest = format!(
        "{production}\nspeakers:\n  - {{ id: narrator, display_name: Narrator, language: en, asset_kind: owner-recorded }}\nnarration_cues:\n  - {{ id: cue-primary, speaker_id: narrator, text: primary, shot_ids: [shot-early-window], start_seconds: 0.0, duration_seconds: 4.0 }}\n  - {{ id: cue-fallback, speaker_id: narrator, text: fallback, start_seconds: 0.0, duration_seconds: 4.0 }}\n  - {{ id: cue-middle, speaker_id: narrator, text: middle, shot_ids: [shot-middle-letter], start_seconds: 4.0, duration_seconds: 4.0 }}\n"
    );
    fs::write(&manifest_path, &manifest).unwrap();
    let hash = reel::production::sha256_path(&manifest_path).unwrap();
    let sheet = fs::read_to_string(FIXTURE)
        .unwrap()
        .replace(
            "../shared-production/manifest.yaml",
            &manifest_path.display().to_string(),
        )
        .replace(
            "99957ce88740be4112f54d0c8bfdc166a09c8b292a3409cd8f7b6ea6a3d74823",
            &hash,
        )
        .replace(
            "exposure_id: locked-wide",
            &format!("exposure_id: locked-wide, cue_ids: [{cue_id}]"),
        );
    let sheet_path = root.join("cue-sheet.yaml");
    fs::write(&sheet_path, sheet).unwrap();
    sheet_path
}

fn path_work_fixture(root: &Path) -> std::path::PathBuf {
    let manifest_path = root.join("path-work-manifest.yaml");
    let manifest = fs::read_to_string("manifests/fixtures/shared-production/manifest.yaml")
        .unwrap()
        .replace("work: shared-production-fixture", "work: private/show");
    fs::write(&manifest_path, manifest).unwrap();
    let hash = reel::production::sha256_path(&manifest_path).unwrap();
    let sheet = fs::read_to_string(FIXTURE)
        .unwrap()
        .replace(
            "../shared-production/manifest.yaml",
            &manifest_path.display().to_string(),
        )
        .replace(
            "99957ce88740be4112f54d0c8bfdc166a09c8b292a3409cd8f7b6ea6a3d74823",
            &hash,
        )
        .replace("work: shared-production-fixture", "work: private/show");
    let sheet_path = root.join("path-work-sheet.yaml");
    fs::write(&sheet_path, sheet).unwrap();
    sheet_path
}

#[test]
fn validates_exact_complete_and_sparse_tracks_with_a_path_free_report() {
    let loaded = load(FIXTURE).unwrap();
    let report = validate(&loaded).unwrap();
    assert_eq!(report.schema, "reel.exposure-sheet-report.v0.1");
    assert_eq!(report.shot_id, "shot-early-window");
    assert_eq!(report.duration_frames, 96);
    assert_eq!(report.duration_delta_milli_frames, 0);
    assert_eq!(report.exposure_count, 6);
    assert_eq!(report.declared_asset_hash_exposures, 2);
    assert_eq!(report.planned_exposures, 4);
    assert!(report.tracks[0].gaps.is_empty());
    assert_eq!(report.tracks[2].gaps.len(), 2);
    assert_eq!(report.tracks[2].gaps[0].start_frame, 0);
    assert_eq!(report.tracks[2].gaps[0].end_frame, 35);
    assert!(report.exposures_supplied_by_input);
    assert!(!report.asset_bytes_verified);
    assert!(!report.reel_selected_exposures);
    assert!(!report.rendered_by_reel);
    assert!(!report.dcc_project_mutated);
    assert!(!report.delivery_frame_rate_claimed);

    let directory = tempdir().unwrap();
    let output = directory.path().join("report.json");
    write_report(&loaded, &output).unwrap();
    let bytes = fs::read(&output).unwrap();
    let serialized = String::from_utf8(bytes.clone()).unwrap();
    assert!(!serialized.contains("manifests/"));
    assert!(!serialized.contains("simple-shot.yaml"));
    let persisted: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(persisted["passed"], true);
}

#[test]
fn rejects_overlaps_complete_track_gaps_and_duration_drift() {
    let directory = tempdir().unwrap();

    let overlap = modified_fixture(
        directory.path(),
        (
            "start_frame: 24, end_frame: 47",
            "start_frame: 23, end_frame: 47",
        ),
    );
    assert!(
        validate(&load(&overlap).unwrap())
            .unwrap_err()
            .to_string()
            .contains("overlap")
    );

    let gap = modified_fixture(
        directory.path(),
        (
            "start_frame: 24, end_frame: 47",
            "start_frame: 25, end_frame: 47",
        ),
    );
    assert!(
        validate(&load(&gap).unwrap())
            .unwrap_err()
            .to_string()
            .contains("uncovered frame range 24-24")
    );

    let drift = modified_fixture(
        directory.path(),
        ("duration_frames: 96", "duration_frames: 94"),
    );
    assert!(
        validate(&load(&drift).unwrap())
            .unwrap_err()
            .to_string()
            .contains("more than half a frame")
    );
}

#[test]
fn rejects_noncanonical_or_untrusted_evidence() {
    let directory = tempdir().unwrap();

    let stale = modified_fixture(
        directory.path(),
        (
            "99957ce88740be4112f54d0c8bfdc166a09c8b292a3409cd8f7b6ea6a3d74823",
            "0000000000000000000000000000000000000000000000000000000000000000",
        ),
    );
    assert!(
        validate(&load(&stale).unwrap())
            .unwrap_err()
            .to_string()
            .contains("hash mismatch")
    );

    let duplicate = modified_fixture(
        directory.path(),
        (
            "start_frame: 24, end_frame: 47, exposure_id: action",
            "start_frame: 24, end_frame: 47, exposure_id: anticipation, asset_sha256: 1111111111111111111111111111111111111111111111111111111111111111",
        ),
    );
    assert!(
        validate(&load(&duplicate).unwrap())
            .unwrap_err()
            .to_string()
            .contains("merge the frame spans")
    );

    let uppercase_hash = modified_fixture(
        directory.path(),
        (
            "2222222222222222222222222222222222222222222222222222222222222222",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ),
    );
    assert!(
        validate(&load(&uppercase_hash).unwrap())
            .unwrap_err()
            .to_string()
            .contains("lowercase hexadecimal")
    );

    let path_work = path_work_fixture(directory.path());
    assert!(
        validate(&load(&path_work).unwrap())
            .unwrap_err()
            .to_string()
            .contains("bound production work id")
    );
}

#[test]
fn accepts_same_shot_cues_and_rejects_unknown_or_cross_shot_cues() {
    let directory = tempdir().unwrap();
    let primary = cue_fixture(directory.path(), "cue-primary");
    let report = validate(&load(&primary).unwrap()).unwrap();
    assert_eq!(report.cue_bindings, 1);

    let fallback = cue_fixture(directory.path(), "cue-fallback");
    let report = validate(&load(&fallback).unwrap()).unwrap();
    assert_eq!(report.cue_bindings, 1);

    let middle = cue_fixture(directory.path(), "cue-middle");
    assert!(
        validate(&load(&middle).unwrap())
            .unwrap_err()
            .to_string()
            .contains("is not bound to exposure sheet shot")
    );

    let missing = cue_fixture(directory.path(), "cue-missing");
    assert!(
        validate(&load(&missing).unwrap())
            .unwrap_err()
            .to_string()
            .contains("unknown narration cue")
    );
}

#[test]
fn cli_publishes_once_without_overwriting() {
    let directory = tempdir().unwrap();
    let output = directory.path().join("report.json");
    let first = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("exposure-sheet-check")
        .arg(FIXTURE)
        .arg("--output-path")
        .arg(&output)
        .arg("--output")
        .arg("json")
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let report: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(report["passed"], true);
    assert!(output.exists());

    let second = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("exposure-sheet-check")
        .arg(FIXTURE)
        .arg("--output-path")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("refusing to overwrite"));
}
