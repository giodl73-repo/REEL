mod common;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use common::music_language::{authority, decision};
use reel_music::arrangement_plan::{
    ArrangementPlan, CandidateCheck, CandidateGate, Direction, ElementDisposition, Ensemble,
    Instrument, NoteMapping, PartAssignment, TransformAction,
};
use reel_music::language_adaptation::DraftBinding;
use tempfile::tempdir;

fn build_fixture(root: &Path) -> PathBuf {
    fs::create_dir_all(root).unwrap();
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let draft_path = repository.join("manifests/fixtures/music-model-corrected/draft.yaml");
    let draft_report = reel_music::model_draft::validate(&draft_path).unwrap();
    let model_path = repository.join("manifests/fixtures/music-model-corrected/model.yaml");
    let model = reel_music::model::load(&model_path).unwrap();
    let target_ids = [
        "tempo:0",
        "meter:0",
        "form:phrase-a",
        "form:phrase-a-prime",
        "note:note-1",
        "note:note-2",
        "note:note-3",
        "note:note-4",
        "harmony:tonic-span",
        "rhythm:quarter-cell",
        "hook:opening-hook",
    ];
    let notes = model.parts[0]
        .notes
        .iter()
        .map(|note| NoteMapping {
            id: format!("arranged-{}", note.id),
            source_note_id: note.id.clone(),
            instrument_id: "plucked-voice".into(),
            action: TransformAction::Preserve,
            start_tick: note.start_tick,
            duration_ticks: note.duration_ticks,
            midi_note: note.midi_note,
            velocity: note.velocity,
            rationale: "Retain the synthetic melody exactly in the recast timbre.".into(),
            decision: None,
        })
        .collect();
    let manifest = ArrangementPlan {
        schema: reel_music::arrangement_plan::SCHEMA.into(),
        arrangement_id: "synthetic-limited-ensemble-plan".into(),
        model_draft: DraftBinding {
            manifest: draft_path,
            manifest_sha256: draft_report.manifest_sha256,
            contract_sha256: draft_report.contract_sha256,
            draft_id: draft_report.draft_id,
        },
        direction: Direction {
            label: "Inspectable single-voice recast".into(),
            objective: "Test score-driven timbre reassignment without changing the composition."
                .into(),
            constraints: vec![
                "Preserve the four-note hook exactly.".into(),
                "Use one playable monophonic instrument.".into(),
            ],
            decision: decision("synthetic-arrangement-direction", '1'),
        },
        ensemble: Ensemble {
            maximum_instruments: 2,
            instruments: vec![Instrument {
                id: "plucked-voice".into(),
                family: "plucked-string".into(),
                function: "primary melody".into(),
                midi_low: 48,
                midi_high: 84,
                maximum_simultaneous_notes: 1,
                techniques: vec!["single-note".into(), "gentle-accent".into()],
            }],
        },
        element_dispositions: target_ids
            .into_iter()
            .map(|id| ElementDisposition {
                model_target_id: id.into(),
                action: TransformAction::Preserve,
                rationale: "Preserve governed composition identity in the first arrangement proof."
                    .into(),
                decision: None,
            })
            .collect(),
        part_assignments: vec![PartAssignment {
            source_part_id: "melody".into(),
            action: TransformAction::Develop,
            instrument_ids: vec!["plucked-voice".into()],
            rationale: "Recast the model melody into the limited synthetic ensemble.".into(),
            decision: Some(decision("synthetic-timbre-assignment", '2')),
        }],
        note_mappings: notes,
        candidate_gate: CandidateGate {
            required_checks: vec![
                CandidateCheck::ExactPlanBinding,
                CandidateCheck::ModelInheritance,
                CandidateCheck::RangeAndPolyphony,
                CandidateCheck::EditableScoreRoundTrip,
                CandidateCheck::AudibleComparison,
                CandidateCheck::HumanRecognition,
                CandidateCheck::HumanSelection,
            ],
        },
        authority: authority("synthetic-arrangement-plan", '3', "reviewed", true),
        review: reel_music::repair::Review {
            status: "not-reviewed".into(),
            required_roles: vec![
                "music-reconstruction-engineer".into(),
                "score-arrangement-director".into(),
                "sound-designer".into(),
                "editor".into(),
                "rights-provenance-steward".into(),
            ],
            decision_refs: vec![],
        },
    };
    let path = root.join("arrangement.yaml");
    write(&path, &manifest);
    path
}

fn write(path: &Path, manifest: &ArrangementPlan) {
    fs::write(path, serde_yaml::to_string(manifest).unwrap()).unwrap();
}

#[test]
fn cli_validates_complete_playable_limited_ensemble_plan() {
    let temporary = tempdir().unwrap();
    let path = build_fixture(temporary.path());
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("music-arrangement-plan-check")
        .arg(&path)
        .arg("--output")
        .arg("json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["model_targets"], 11);
    assert_eq!(report["preserved_targets"], 11);
    assert_eq!(report["source_parts"], 1);
    assert_eq!(report["instruments"], 1);
    assert_eq!(report["mapped_notes"], 4);
    assert_eq!(report["candidate_checks"], 7);
    assert_eq!(report["playable_ranges"], true);
    assert_eq!(report["polyphony_within_limits"], true);
    assert_eq!(report["shareable"], false);
}

#[test]
fn rejects_incomplete_or_ungoverned_transformations() {
    let temporary = tempdir().unwrap();
    let path = build_fixture(temporary.path());
    let mut manifest = reel_music::arrangement_plan::load(&path).unwrap();
    manifest.element_dispositions.pop();
    write(&path, &manifest);
    assert!(reel_music::arrangement_plan::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("decision"));
    let mut manifest = reel_music::arrangement_plan::load(&path).unwrap();
    manifest.element_dispositions[0].action = TransformAction::Develop;
    write(&path, &manifest);
    assert!(reel_music::arrangement_plan::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("part"));
    let mut manifest = reel_music::arrangement_plan::load(&path).unwrap();
    manifest.part_assignments.clear();
    write(&path, &manifest);
    assert!(reel_music::arrangement_plan::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("note"));
    let mut manifest = reel_music::arrangement_plan::load(&path).unwrap();
    manifest.note_mappings[0].midi_note += 1;
    write(&path, &manifest);
    assert!(reel_music::arrangement_plan::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("instrument"));
    let mut manifest = reel_music::arrangement_plan::load(&path).unwrap();
    manifest.note_mappings[0].instrument_id = "unknown".into();
    write(&path, &manifest);
    assert!(reel_music::arrangement_plan::validate(&path).is_err());
}

#[test]
fn rejects_unplayable_mapping_and_candidate_gate_shortcuts() {
    let temporary = tempdir().unwrap();
    let path = build_fixture(temporary.path());
    let mut manifest = reel_music::arrangement_plan::load(&path).unwrap();
    manifest.ensemble.instruments[0].midi_high = 59;
    write(&path, &manifest);
    assert!(reel_music::arrangement_plan::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("polyphony"));
    let mut manifest = reel_music::arrangement_plan::load(&path).unwrap();
    manifest.element_dispositions[5].action = TransformAction::Develop;
    manifest.element_dispositions[5].decision = Some(decision("develop-note-2", '4'));
    manifest.note_mappings[1].action = TransformAction::Develop;
    manifest.note_mappings[1].decision = Some(decision("develop-note-2-output", '5'));
    manifest.note_mappings[1].start_tick = 0;
    write(&path, &manifest);
    assert!(reel_music::arrangement_plan::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("gate"));
    let mut manifest = reel_music::arrangement_plan::load(&path).unwrap();
    manifest.candidate_gate.required_checks.pop();
    write(&path, &manifest);
    assert!(reel_music::arrangement_plan::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("ensemble"));
    let mut manifest = reel_music::arrangement_plan::load(&path).unwrap();
    manifest.ensemble.maximum_instruments = 0;
    write(&path, &manifest);
    assert!(reel_music::arrangement_plan::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("review"));
    let mut manifest = reel_music::arrangement_plan::load(&path).unwrap();
    manifest
        .review
        .required_roles
        .retain(|role| role != "score-arrangement-director");
    write(&path, &manifest);
    assert!(reel_music::arrangement_plan::validate(&path).is_err());
}
