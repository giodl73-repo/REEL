use std::{fs, path::Path};

use tempfile::tempdir;

fn copy_fixture(root: &Path) -> std::path::PathBuf {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = root.join("manifests/fixtures");
    let intake = fixtures.join("music-interchange-intake");
    let source = fixtures.join("music-repair-foundation");
    fs::create_dir_all(&intake).unwrap();
    fs::create_dir_all(&source).unwrap();
    for name in [
        "comparison.yaml",
        "intake.yaml",
        "note-events.csv",
        "note-events-alt.csv",
        "annotations.jams",
    ] {
        fs::copy(
            repository
                .join("manifests/fixtures/music-interchange-intake")
                .join(name),
            intake.join(name),
        )
        .unwrap();
    }
    for name in ["source.yaml", "source.u8"] {
        fs::copy(
            repository
                .join("manifests/fixtures/music-repair-foundation")
                .join(name),
            source.join(name),
        )
        .unwrap();
    }
    intake.join("comparison.yaml")
}

#[test]
fn emits_deterministic_selection_and_correction_queue() {
    let temporary = tempdir().unwrap();
    let comparison = copy_fixture(temporary.path());
    let report = reel_music::comparison::validate(&comparison).unwrap();
    assert_eq!(report.sets, 1);
    assert_eq!(report.candidates, 2);
    assert_eq!(report.findings, 1);
    assert_eq!(report.selected_sets, 0);
    assert_eq!(report.open_corrections, 1);
    assert_eq!(report.queue.len(), 2);
    assert_eq!(report.queue[0].kind, "correction");
    assert_eq!(report.queue[1].kind, "selection");
    assert!(!report.shareable);
    assert!(report.verified);
}

#[test]
fn rejects_stale_intake_unknown_candidates_and_metric_overflow() {
    let temporary = tempdir().unwrap();
    let comparison = copy_fixture(temporary.path());
    let mut manifest = reel_music::comparison::load(&comparison).unwrap();
    manifest.intake.contract_sha256 = "0".repeat(64);
    fs::write(&comparison, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    assert!(reel_music::comparison::validate(&comparison).is_err());

    let comparison = copy_fixture(&temporary.path().join("unknown"));
    let mut manifest = reel_music::comparison::load(&comparison).unwrap();
    manifest.sets[0].candidates[0].artifact_id = "missing-artifact".into();
    fs::write(&comparison, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    assert!(reel_music::comparison::validate(&comparison).is_err());

    let comparison = copy_fixture(&temporary.path().join("metric"));
    let mut manifest = reel_music::comparison::load(&comparison).unwrap();
    manifest.sets[0].candidates[0].confidence_millionths = Some(1_000_001);
    fs::write(&comparison, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    assert!(reel_music::comparison::validate(&comparison).is_err());
}

#[test]
fn selection_requires_a_decision_and_no_open_correction_on_selected_artifact() {
    use reel_music::{DecisionRef, comparison::Selection};

    let temporary = tempdir().unwrap();
    let comparison = copy_fixture(temporary.path());
    let mut manifest = reel_music::comparison::load(&comparison).unwrap();
    manifest.sets[0].selection = Some(Selection {
        artifact_id: "synthetic-note-events-alternate".into(),
        decision: DecisionRef {
            artifact_id: "human-selection-001".into(),
            sha256: "c".repeat(64),
        },
        rationale: "Fixture decision reference tests selection gating.".into(),
    });
    fs::write(&comparison, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    assert!(reel_music::comparison::validate(&comparison).is_err());

    manifest.sets[0].corrections[0].resolution = Some(DecisionRef {
        artifact_id: "human-correction-001".into(),
        sha256: "d".repeat(64),
    });
    fs::write(&comparison, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    let report = reel_music::comparison::validate(&comparison).unwrap();
    assert_eq!(report.selected_sets, 1);
    assert_eq!(report.open_corrections, 0);
    assert!(report.queue.is_empty());
}
