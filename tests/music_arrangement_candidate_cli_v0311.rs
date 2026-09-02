mod common;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use common::{
    music_arrangement,
    music_language::{authority, decision},
};
use reel::arrangement_candidate::{
    ArrangementBinding, ArrangementCandidate, ArtifactBinding, AudibleComparison,
    ComparisonDimension, CreationProvenance, ModelBinding, NetworkPolicy, RecognitionGate,
    RecognitionStatus, ScoreExportBinding,
};
use reel_music::{
    hash::{canonical_sha256, sha256_path},
    repair_candidate::{ListeningGate, ListeningStatus, SelectionGate, SelectionStatus},
};
use tempfile::tempdir;

fn write(path: &Path, manifest: &ArrangementCandidate) {
    fs::write(path, serde_yaml::to_string(manifest).unwrap()).unwrap();
}

fn build_fixture(root: &Path) -> PathBuf {
    fs::create_dir_all(root).unwrap();
    let fixture = music_arrangement::build(root);
    let arrangement_report = reel_music::arrangement_plan::validate(&fixture.arrangement).unwrap();

    let mut model = reel_music::model::load(&fixture.source_model).unwrap();
    let source_parent = fixture.source_model.parent().unwrap();
    model.source.manifest = source_parent.join(&model.source.manifest);
    for analysis in &mut model.analyses {
        analysis.manifest = source_parent.join(&analysis.manifest);
    }
    model.model_id = "synthetic-arranged-score".into();
    model.authority = authority("synthetic-arranged-score", '4', "candidate", false);
    model.parts[0].id = "plucked-voice".into();
    model.parts[0].name = "Synthetic plucked voice".into();
    model.lead_sheet.as_mut().unwrap().melody_part_id = "plucked-voice".into();
    model.review.status = "not-reviewed".into();
    model.review.decision_refs.clear();
    fs::copy(source_parent.join("lyrics.txt"), root.join("lyrics.txt")).unwrap();
    let model_path = root.join("arranged-model.yaml");
    fs::write(&model_path, serde_yaml::to_string(&model).unwrap()).unwrap();
    let model_report = reel_music::model::validate(&model_path).unwrap();

    let export_plan = root.join("score-plan.json");
    let plan_report = reel_music::export::write(&model_path, &export_plan).unwrap();
    let packet = root.join("score-packet");
    let score_report = reel::music_score::render(&export_plan, &model_path, &packet).unwrap();
    let guide = packet.join("rehearsal-guide.wav");
    let reference = root.join("source-reference.wav");
    fs::copy(&guide, &reference).unwrap();

    let manifest = ArrangementCandidate {
        schema: reel::arrangement_candidate::SCHEMA.into(),
        candidate_id: "synthetic-arrangement-candidate".into(),
        arrangement: ArrangementBinding {
            manifest: fixture.arrangement.clone(),
            manifest_sha256: arrangement_report.manifest_sha256,
            contract_sha256: arrangement_report.contract_sha256,
            arrangement_id: arrangement_report.arrangement_id,
        },
        arranged_model: ModelBinding {
            manifest: model_path,
            manifest_sha256: model_report.manifest_sha256,
            contract_sha256: model_report.contract_sha256,
            model_id: model_report.model_id,
        },
        score_export: ScoreExportBinding {
            plan: export_plan,
            plan_sha256: plan_report.plan_sha256,
            plan_contract_sha256: plan_report.plan_contract_sha256,
            receipt: packet.join("receipt.json"),
            receipt_sha256: score_report.receipt_sha256,
            packet_dir: packet,
        },
        arrangement_audio: ArtifactBinding {
            path: guide.clone(),
            sha256: sha256_path(&guide).unwrap(),
        },
        creation: CreationProvenance {
            adapter_id: "reel-square-guide".into(),
            adapter_version: "0.1.0".into(),
            network_policy: NetworkPolicy::LocalOnly,
            egress_decision: None,
        },
        comparison: AudibleComparison {
            source_reference: ArtifactBinding {
                path: reference.clone(),
                sha256: sha256_path(&reference).unwrap(),
            },
            source_authority: authority("synthetic-source-reference", '5', "fixture-only", false),
            source_blind_label: "A".into(),
            candidate_blind_label: "B".into(),
            review_dimensions: vec![
                ComparisonDimension::Form,
                ComparisonDimension::Pulse,
                ComparisonDimension::Melody,
                ComparisonDimension::Harmony,
                ComparisonDimension::Hooks,
                ComparisonDimension::EmotionalArc,
                ComparisonDimension::Instrumentation,
                ComparisonDimension::MixBalance,
            ],
            listening: ListeningGate {
                status: ListeningStatus::Pending,
                decision: None,
            },
        },
        recognition: RecognitionGate {
            status: RecognitionStatus::Pending,
            decision: None,
        },
        authority: authority("synthetic-arrangement-candidate", '6', "candidate", false),
        selection: SelectionGate {
            status: SelectionStatus::Pending,
            decision: None,
        },
        review: reel_music::repair::Review {
            status: "not-reviewed".into(),
            required_roles: vec![
                "music-reconstruction-engineer".into(),
                "score-arrangement-director".into(),
                "sound-designer".into(),
                "editor".into(),
                "rights-provenance-steward".into(),
                "platform-audience".into(),
            ],
            decision_refs: vec![],
        },
    };
    let path = root.join("candidate.yaml");
    write(&path, &manifest);
    path
}

#[test]
fn cli_validates_exact_score_and_audible_round_trip_candidate() {
    let temporary = tempdir().unwrap();
    let path = build_fixture(temporary.path());
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("music-arrangement-candidate-check")
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
    assert_eq!(report["mapped_notes"], 4);
    assert_eq!(report["arranged_parts"], 1);
    assert_eq!(report["midi_round_trip"], true);
    assert_eq!(report["musicxml_round_trip"], true);
    assert_eq!(report["audible_round_trip"], true);
    assert_eq!(report["eligible_for_selection"], false);
    assert_eq!(report["shareable"], false);
}

#[test]
fn rejects_plan_model_score_and_audio_tampering() {
    let temporary = tempdir().unwrap();
    let path = build_fixture(temporary.path());
    let mut manifest = reel::arrangement_candidate::load(&path).unwrap();
    manifest.arrangement.contract_sha256 = "0".repeat(64);
    write(&path, &manifest);
    assert!(reel::arrangement_candidate::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("model"));
    let mut manifest = reel::arrangement_candidate::load(&path).unwrap();
    let model_path = manifest.arranged_model.manifest.clone();
    let mut model = reel_music::model::load(&model_path).unwrap();
    model.parts[0].notes[0].midi_note += 1;
    fs::write(&model_path, serde_yaml::to_string(&model).unwrap()).unwrap();
    manifest.arranged_model.manifest_sha256 = sha256_path(&model_path).unwrap();
    manifest.arranged_model.contract_sha256 = canonical_sha256(&model).unwrap();
    write(&path, &manifest);
    assert!(reel::arrangement_candidate::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("score"));
    let manifest = reel::arrangement_candidate::load(&path).unwrap();
    fs::write(
        manifest.score_export.packet_dir.join("score.musicxml"),
        b"tampered",
    )
    .unwrap();
    assert!(reel::arrangement_candidate::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("audio"));
    let mut manifest = reel::arrangement_candidate::load(&path).unwrap();
    manifest.arrangement_audio.sha256 = "f".repeat(64);
    write(&path, &manifest);
    assert!(reel::arrangement_candidate::validate(&path).is_err());
}

#[test]
fn human_recognition_and_selection_cannot_be_shortcut() {
    let temporary = tempdir().unwrap();
    let path = build_fixture(temporary.path());
    let mut manifest = reel::arrangement_candidate::load(&path).unwrap();
    manifest.recognition.status = RecognitionStatus::Recognized;
    manifest.recognition.decision = Some(decision("recognition", '7'));
    write(&path, &manifest);
    assert!(reel::arrangement_candidate::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("selection"));
    let mut manifest = reel::arrangement_candidate::load(&path).unwrap();
    manifest.selection.status = SelectionStatus::Selected;
    manifest.selection.decision = Some(decision("selection", '8'));
    manifest.authority.status = "selected".into();
    manifest.authority.decision_refs = vec![decision("candidate-selection", '9')];
    write(&path, &manifest);
    assert!(reel::arrangement_candidate::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("review"));
    let mut manifest = reel::arrangement_candidate::load(&path).unwrap();
    manifest
        .review
        .required_roles
        .retain(|role| role != "platform-audience");
    write(&path, &manifest);
    assert!(reel::arrangement_candidate::validate(&path).is_err());
}
