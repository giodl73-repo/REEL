use std::{fs, path::Path};

use reel_music::{
    analysis::ObservationValue,
    hash::{canonical_sha256, sha256_path},
    semantic_import::OriginalTime,
};
use tempfile::tempdir;

fn copy_fixture(root: &Path) -> std::path::PathBuf {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = root.join("manifests/fixtures");
    let intake = fixtures.join("music-interchange-intake");
    let source = fixtures.join("music-repair-foundation");
    fs::create_dir_all(&intake).unwrap();
    fs::create_dir_all(&source).unwrap();
    for name in [
        "semantic-import.yaml",
        "comparison-selected.yaml",
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
    intake.join("semantic-import.yaml")
}

#[test]
fn validates_exact_time_mappings_and_writes_lineage_bound_analysis() {
    let temporary = tempdir().unwrap();
    let import = copy_fixture(temporary.path());
    let report = reel_music::semantic_import::validate(&import).unwrap();
    assert_eq!(report.events, 3);
    assert_eq!(
        report.selected_artifact_id,
        "synthetic-note-events-alternate"
    );
    assert!(!report.shareable);

    let output = import.parent().unwrap().join("analysis.yaml");
    let write = reel_music::semantic_import::write_analysis(&import, &output).unwrap();
    assert_eq!(write.observations, 3);
    assert!(!write.shareable);
    let analysis = reel_music::analysis::validate(&output).unwrap();
    assert_eq!(analysis.imports, 1);
    assert_eq!(analysis.observations, 3);
    assert!(reel_music::semantic_import::write_analysis(&import, &output).is_err());
}

#[test]
fn supports_samples_and_musical_ticks_without_floating_point() {
    let temporary = tempdir().unwrap();
    let import_path = copy_fixture(temporary.path());
    let mut import = reel_music::semantic_import::load(&import_path).unwrap();
    import.events[0].original_time = OriginalTime::Samples {
        start: 0,
        end: 16,
        sample_rate_hz: 8_000,
    };
    import.events[1].original_time = OriginalTime::MusicalTicks {
        start: 2,
        end: 4,
        pulses_per_quarter: 1_000,
        microseconds_per_quarter: 1_000_000,
    };
    fs::write(&import_path, serde_yaml::to_string(&import).unwrap()).unwrap();
    assert!(reel_music::semantic_import::validate(&import_path).is_ok());
}

#[test]
fn rejects_mapping_selection_and_imported_observation_tampering() {
    let temporary = tempdir().unwrap();
    let import_path = copy_fixture(temporary.path());
    let mut import = reel_music::semantic_import::load(&import_path).unwrap();
    import.events[0].mapped_source.end = 15;
    fs::write(&import_path, serde_yaml::to_string(&import).unwrap()).unwrap();
    assert!(reel_music::semantic_import::validate(&import_path).is_err());

    let import_path = copy_fixture(&temporary.path().join("selection"));
    let mut import = reel_music::semantic_import::load(&import_path).unwrap();
    import.selected_artifact_id = "synthetic-note-events".into();
    fs::write(&import_path, serde_yaml::to_string(&import).unwrap()).unwrap();
    assert!(reel_music::semantic_import::validate(&import_path).is_err());

    let import_path = copy_fixture(&temporary.path().join("analysis"));
    let output = import_path.parent().unwrap().join("analysis.yaml");
    reel_music::semantic_import::write_analysis(&import_path, &output).unwrap();
    let mut analysis = reel_music::analysis::load(&output).unwrap();
    analysis.observations[0].value = ObservationValue::Pitch {
        midi_note: 61,
        cents: 0,
    };
    fs::write(&output, serde_yaml::to_string(&analysis).unwrap()).unwrap();
    let error = reel_music::analysis::validate(&output).unwrap_err();
    assert!(error.to_string().contains("does not exactly mirror"));
}

#[test]
fn generated_analysis_rejects_stale_import_bytes() {
    let temporary = tempdir().unwrap();
    let import_path = copy_fixture(temporary.path());
    let output = import_path.parent().unwrap().join("analysis.yaml");
    reel_music::semantic_import::write_analysis(&import_path, &output).unwrap();

    let mut import = reel_music::semantic_import::load(&import_path).unwrap();
    import.limitations.push("Changed after promotion.".into());
    fs::write(&import_path, serde_yaml::to_string(&import).unwrap()).unwrap();
    let error = reel_music::analysis::validate(&output).unwrap_err();
    assert!(error.to_string().contains("manifest sha256 is stale"));
}

#[test]
fn rejects_comparison_rebinding_even_when_manifest_hash_is_updated() {
    let temporary = tempdir().unwrap();
    let import_path = copy_fixture(temporary.path());
    let comparison_path = import_path
        .parent()
        .unwrap()
        .join("comparison-selected.yaml");
    let mut comparison = reel_music::comparison::load(&comparison_path).unwrap();
    comparison.sets[0].selection = None;
    fs::write(
        &comparison_path,
        serde_yaml::to_string(&comparison).unwrap(),
    )
    .unwrap();

    let mut import = reel_music::semantic_import::load(&import_path).unwrap();
    import.comparison.manifest_sha256 = sha256_path(&comparison_path).unwrap();
    import.comparison.contract_sha256 = canonical_sha256(&comparison).unwrap();
    fs::write(&import_path, serde_yaml::to_string(&import).unwrap()).unwrap();
    let error = reel_music::semantic_import::validate(&import_path).unwrap_err();
    assert!(error.to_string().contains("explicit candidate selection"));
}
