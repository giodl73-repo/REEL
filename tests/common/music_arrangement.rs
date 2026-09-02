#![allow(dead_code)]

use std::{
    fs,
    path::{Path, PathBuf},
};

use reel_music::{
    arrangement_plan::{
        ArrangementPlan, CandidateCheck, CandidateGate, Direction, ElementDisposition, Ensemble,
        Instrument, NoteMapping, PartAssignment, TransformAction,
    },
    language_adaptation::DraftBinding,
};

use super::music_language::{authority, decision};

pub struct Fixture {
    pub arrangement: PathBuf,
    pub source_model: PathBuf,
}

pub fn build(root: &Path) -> Fixture {
    fs::create_dir_all(root).unwrap();
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let draft_path = repository.join("manifests/fixtures/music-model-corrected/draft.yaml");
    let draft_report = reel_music::model_draft::validate(&draft_path).unwrap();
    let source_model = repository.join("manifests/fixtures/music-model-corrected/model.yaml");
    let model = reel_music::model::load(&source_model).unwrap();
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
            id: note.id.clone(),
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
    let arrangement = root.join("arrangement.yaml");
    fs::write(&arrangement, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    Fixture {
        arrangement,
        source_model,
    }
}
