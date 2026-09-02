use std::{fs, path::Path};

use reel_music::{
    DecisionRef,
    model_draft::{DispositionOutcome, ModelDraft},
};
use tempfile::tempdir;

fn copy_fixture(root: &Path) -> std::path::PathBuf {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = root.join("manifests/fixtures");
    let model_dir = fixtures.join("music-model-corrected");
    let source_dir = fixtures.join("music-repair-foundation");
    fs::create_dir_all(&model_dir).unwrap();
    fs::create_dir_all(&source_dir).unwrap();
    for name in ["draft.yaml", "model.yaml", "analysis.yaml", "lyrics.txt"] {
        fs::copy(
            repository
                .join("manifests/fixtures/music-model-corrected")
                .join(name),
            model_dir.join(name),
        )
        .unwrap();
    }
    for name in ["source.yaml", "source.u8"] {
        fs::copy(
            repository
                .join("manifests/fixtures/music-repair-foundation")
                .join(name),
            source_dir.join(name),
        )
        .unwrap();
    }
    model_dir.join("draft.yaml")
}

fn write_draft(path: &Path, draft: &ModelDraft) {
    fs::write(path, serde_yaml::to_string(draft).unwrap()).unwrap();
}

fn rebind_model(draft_path: &Path, draft: &mut ModelDraft) {
    let model_path = draft_path.parent().unwrap().join("model.yaml");
    let report = reel_music::model::validate(&model_path).unwrap();
    draft.model.manifest_sha256 = report.manifest_sha256;
    draft.model.contract_sha256 = report.contract_sha256;
}

#[test]
fn validates_complete_bidirectional_observation_dispositions() {
    let temporary = tempdir().unwrap();
    let draft = copy_fixture(temporary.path());
    let report = reel_music::model_draft::validate(&draft).unwrap();
    assert_eq!(report.observations, 7);
    assert_eq!(report.mapped_targets, 11);
    assert_eq!(report.human_corrected_targets, 1);
    assert!(!report.shareable);
}

#[test]
fn rejects_missing_disposition_wrong_correction_and_unmapped_model_citation() {
    let temporary = tempdir().unwrap();
    let draft_path = copy_fixture(temporary.path());
    let mut draft = reel_music::model_draft::load(&draft_path).unwrap();
    draft.dispositions.pop();
    write_draft(&draft_path, &draft);
    assert!(reel_music::model_draft::validate(&draft_path).is_err());

    let draft_path = copy_fixture(&temporary.path().join("correction"));
    let mut draft = reel_music::model_draft::load(&draft_path).unwrap();
    if let DispositionOutcome::Mapped { targets } = &mut draft.dispositions[3].outcome {
        targets[0].correction_ref.as_mut().unwrap().sha256 = "9".repeat(64);
    }
    write_draft(&draft_path, &draft);
    assert!(reel_music::model_draft::validate(&draft_path).is_err());

    let draft_path = copy_fixture(&temporary.path().join("citation"));
    let mut draft = reel_music::model_draft::load(&draft_path).unwrap();
    if let DispositionOutcome::Mapped { targets } = &mut draft.dispositions[3].outcome {
        targets.pop();
    }
    write_draft(&draft_path, &draft);
    let error = reel_music::model_draft::validate(&draft_path).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("lacks a matching target disposition")
    );
}

#[test]
fn accepts_explicit_unknown_and_decision_bound_omission_when_model_matches() {
    let temporary = tempdir().unwrap();
    let draft_path = copy_fixture(temporary.path());
    let model_path = draft_path.parent().unwrap().join("model.yaml");
    let mut model = reel_music::model::load(&model_path).unwrap();
    model.harmony.clear();
    model.hooks.clear();
    let unknown = "Whether the opening material functions as a hook remains unknown.".to_string();
    model.unknowns.push(unknown.clone());
    fs::write(&model_path, serde_yaml::to_string(&model).unwrap()).unwrap();

    let mut draft = reel_music::model_draft::load(&draft_path).unwrap();
    draft.dispositions[4].outcome = DispositionOutcome::Omitted {
        rationale: "The fixture author explicitly omits the harmonic label.".into(),
        decision: DecisionRef {
            artifact_id: "synthetic-harmony-omission".into(),
            sha256: "8".repeat(64),
        },
    };
    draft.dispositions[6].outcome = DispositionOutcome::Unknown {
        rationale: "The evidence does not establish creative hook identity.".into(),
        unknown_text: unknown,
    };
    rebind_model(&draft_path, &mut draft);
    write_draft(&draft_path, &draft);
    let report = reel_music::model_draft::validate(&draft_path).unwrap();
    assert_eq!(report.omitted_observations, 1);
    assert_eq!(report.unknown_observations, 1);
}

#[test]
fn rejects_unknown_text_not_preserved_in_model() {
    let temporary = tempdir().unwrap();
    let draft_path = copy_fixture(temporary.path());
    let mut draft = reel_music::model_draft::load(&draft_path).unwrap();
    draft.dispositions[6].outcome = DispositionOutcome::Unknown {
        rationale: "Keep uncertainty visible.".into(),
        unknown_text: "This exact unknown is absent from the model.".into(),
    };
    write_draft(&draft_path, &draft);
    assert!(reel_music::model_draft::validate(&draft_path).is_err());
}
