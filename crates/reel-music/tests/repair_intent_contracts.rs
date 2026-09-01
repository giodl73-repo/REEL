use std::{
    fs,
    path::{Path, PathBuf},
};

use reel_music::repair_intent::RepairIntentManifest;
use tempfile::tempdir;

fn copy_fixture(root: &Path) -> PathBuf {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = root.join("manifests/fixtures");
    for (directory, names) in [
        ("music-repair-intent", &["intent.yaml"][..]),
        (
            "music-model-corrected",
            &["draft.yaml", "model.yaml", "analysis.yaml"][..],
        ),
        (
            "music-repair-foundation",
            &["repair.yaml", "source.yaml", "source.u8"][..],
        ),
    ] {
        let destination = fixtures.join(directory);
        fs::create_dir_all(&destination).unwrap();
        for name in names {
            fs::copy(
                repository
                    .join("manifests/fixtures")
                    .join(directory)
                    .join(name),
                destination.join(name),
            )
            .unwrap();
        }
    }
    fixtures.join("music-repair-intent/intent.yaml")
}

fn write(path: &Path, manifest: &RepairIntentManifest) {
    fs::write(path, serde_yaml::to_string(manifest).unwrap()).unwrap();
}

#[test]
fn validates_model_bound_repair_intent_and_candidate_gate() {
    let temporary = tempdir().unwrap();
    let path = copy_fixture(temporary.path());
    let report = reel_music::repair_intent::validate(&path).unwrap();
    assert_eq!(report.intents, 1);
    assert_eq!(report.mutating_operations, 1);
    assert_eq!(report.model_targets, 1);
    assert_eq!(report.candidate_checks, 6);
    assert!(report.complete_operation_coverage);
    assert!(report.source_lineage_matches);
    assert!(!report.shareable);
}

#[test]
fn rejects_unknown_model_target_and_missing_operation_intent() {
    let temporary = tempdir().unwrap();
    let path = copy_fixture(temporary.path());
    let mut manifest = reel_music::repair_intent::load(&path).unwrap();
    manifest.intents[0].model_target_refs[0] = "note:not-present".into();
    write(&path, &manifest);
    assert!(reel_music::repair_intent::validate(&path).is_err());

    let path = copy_fixture(&temporary.path().join("missing"));
    let mut manifest = reel_music::repair_intent::load(&path).unwrap();
    manifest.intents.clear();
    write(&path, &manifest);
    assert!(reel_music::repair_intent::validate(&path).is_err());
}

#[test]
fn rejects_incomplete_candidate_gate_and_tampered_binding() {
    let temporary = tempdir().unwrap();
    let path = copy_fixture(temporary.path());
    let mut manifest = reel_music::repair_intent::load(&path).unwrap();
    manifest.candidate_gate.required_checks.pop();
    write(&path, &manifest);
    assert!(reel_music::repair_intent::validate(&path).is_err());

    let path = copy_fixture(&temporary.path().join("tampered"));
    let mut manifest = reel_music::repair_intent::load(&path).unwrap();
    manifest.repair.manifest_sha256 = "0".repeat(64);
    write(&path, &manifest);
    assert!(reel_music::repair_intent::validate(&path).is_err());
}

#[test]
fn rejects_missing_decision_and_duplicate_operation_link() {
    let temporary = tempdir().unwrap();
    let path = copy_fixture(temporary.path());
    let mut manifest = reel_music::repair_intent::load(&path).unwrap();
    manifest.intents[0].decision.artifact_id.clear();
    write(&path, &manifest);
    assert!(reel_music::repair_intent::validate(&path).is_err());

    let path = copy_fixture(&temporary.path().join("duplicate"));
    let mut manifest = reel_music::repair_intent::load(&path).unwrap();
    let mut duplicate = manifest.intents[0].clone();
    duplicate.id = "duplicate-intent".into();
    manifest.intents.push(duplicate);
    write(&path, &manifest);
    assert!(reel_music::repair_intent::validate(&path).is_err());
}
