use reel_music::{
    model,
    note_editor::{self, Edit},
};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../manifests/fixtures/music-model-corrected/model.yaml")
}
fn setup(root: &Path) -> (PathBuf, Edit) {
    let path = root.join("model.yaml");
    fs::copy(fixture(), &path).unwrap();
    let edit = note_editor::template(&path).unwrap();
    (path, edit)
}
fn save(root: &Path, edit: &Edit) -> PathBuf {
    let path = root.join("request.json");
    fs::write(&path, serde_json::to_vec(edit).unwrap()).unwrap();
    path
}
#[test]
fn export_and_apply_work_without_any_media_or_dependency_files() {
    let temp = tempfile::tempdir().unwrap();
    let (path, mut edit) = setup(temp.path());
    let before = fs::read(&path).unwrap();
    let editor = temp.path().join("editor");
    assert_eq!(note_editor::export(&path, &editor).unwrap(), 4);
    assert!(note_editor::export(&path, &editor).is_err());
    let session: serde_json::Value =
        serde_json::from_slice(&fs::read(editor.join("session.json")).unwrap()).unwrap();
    assert_eq!(session["media_resolved"], false);
    assert_eq!(session["native_validation"], "not-run");
    edit.rows[0].midi_note = 61;
    edit.rows[0].duration_ticks = 900;
    edit.rows[0].rationale = "Compare this pitch and shorter release.".into();
    let output = temp.path().join("proposal.json");
    assert_eq!(
        note_editor::apply(&path, &save(temp.path(), &edit), &output).unwrap(),
        1
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    let original = model::load(&path).unwrap();
    let candidate = model::load(&output).unwrap();
    assert_eq!(candidate.source, original.source);
    assert_eq!(candidate.analyses, original.analyses);
    assert_eq!(candidate.lyric_layers, original.lyric_layers);
    assert_eq!(candidate.lead_sheet, original.lead_sheet);
    assert_eq!(candidate.parts[0].notes[1..], original.parts[0].notes[1..]);
    assert_eq!(candidate.parts[0].notes[0].midi_note, 61);
    assert_eq!(
        candidate.parts[0].notes[0].provenance.state,
        model::ProvenanceState::Inferred
    );
    assert_eq!(candidate.parts[0].notes[0].provenance.correction_ref, None);
    assert_eq!(candidate.review.status, "not-reviewed");
    assert!(candidate.review.decision_refs.is_empty());
    assert_eq!(candidate.authority.status, "not-reviewed");
    assert!(candidate.authority.decision_refs.is_empty());
    let bytes = fs::read(&output).unwrap();
    assert!(note_editor::apply(&path, &save(temp.path(), &edit), &output).is_err());
    assert_eq!(fs::read(output).unwrap(), bytes);
}
#[test]
fn invalid_proposals_refuse_before_creating_an_output() {
    let temp = tempfile::tempdir().unwrap();
    let (path, original) = setup(temp.path());
    for case in 0..11 {
        let mut edit = original.clone();
        match case {
            0 => edit.expected_model_sha256 = "0".repeat(64),
            1 => edit.rows[0].note_id = "foreign".into(),
            2 => {
                edit.rows.pop();
            }
            3 => edit.rows[1] = edit.rows[0].clone(),
            4 => edit.rows[0].midi_note = 128,
            5 => edit.rows[0].duration_ticks = 0,
            6 => edit.rows[0].duration_ticks = 9600,
            7 => {
                edit.rows[0].duration_ticks = 961;
                edit.rows[0].rationale = "Overlap".into();
            }
            8 => edit.candidate_model_id = "synthetic-corrected-editable-model".into(),
            9 => edit.rows[0].midi_note = 61,
            _ => {
                edit.rows[0].duration_ticks = u64::MAX;
                edit.rows[0].rationale = "Overflow".into();
            }
        }
        let output = temp.path().join(format!("bad-{case}.json"));
        assert!(
            note_editor::apply(&path, &save(temp.path(), &edit), &output).is_err(),
            "case {case}"
        );
        assert!(!output.exists());
    }
}
#[test]
fn stale_bytes_unsafe_destinations_unknown_fields_and_unbounded_inputs_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let (path, edit) = setup(temp.path());
    let request = save(temp.path(), &edit);
    let other = temp.path().join("other");
    fs::create_dir(&other).unwrap();
    assert!(note_editor::apply(&path, &request, &other.join("proposal.json")).is_err());
    let mut raw = fs::read(&path).unwrap();
    raw.push(b'\n');
    fs::write(&path, raw).unwrap();
    assert!(note_editor::apply(&path, &request, &temp.path().join("stale.json")).is_err());
    let mut value = serde_json::to_value(note_editor::template(&path).unwrap()).unwrap();
    value["human_approval"] = json!(true);
    fs::write(&request, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(note_editor::apply(&path, &request, &temp.path().join("authority.json")).is_err());
    fs::write(&path, vec![b' '; 2 * 1024 * 1024 + 1]).unwrap();
    assert!(note_editor::export(&path, &temp.path().join("oversized")).is_err());
    assert!(!temp.path().join("oversized").exists());
}
#[test]
fn identity_proposals_keep_native_content_but_do_not_inherit_review() {
    let temp = tempfile::tempdir().unwrap();
    let (path, edit) = setup(temp.path());
    let output = temp.path().join("identity.json");
    assert_eq!(
        note_editor::apply(&path, &save(temp.path(), &edit), &output).unwrap(),
        0
    );
    let mut original = model::load(&path).unwrap();
    original.model_id = edit.candidate_model_id;
    original.review.status = "not-reviewed".into();
    original.review.decision_refs.clear();
    original.authority.status = "not-reviewed".into();
    original.authority.decision_refs.clear();
    assert_eq!(model::load(&output).unwrap(), original);
}

#[test]
fn proposals_cannot_invent_evidence_or_inherit_source_decisions() {
    let temp = tempfile::tempdir().unwrap();
    let (path, _) = setup(temp.path());
    let mut value = serde_json::to_value(model::load(&path).unwrap()).unwrap();
    let decision = json!({"artifact_id":"synthetic-old-approval","sha256":"3".repeat(64)});
    value["authority"]["status"] = json!("approved");
    value["authority"]["decision_refs"] = json!([decision.clone()]);
    value["review"]["status"] = json!("approved");
    value["review"]["decision_refs"] = json!([decision]);
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let mut edit = note_editor::template(&path).unwrap();
    edit.rows[0].midi_note = 61;
    edit.rows[0].rationale = "Compare pitch".into();
    let output = temp.path().join("proposal.json");
    note_editor::apply(&path, &save(temp.path(), &edit), &output).unwrap();
    let candidate = model::load(&output).unwrap();
    assert!(candidate.authority.decision_refs.is_empty());
    assert!(candidate.review.decision_refs.is_empty());
    value["parts"][0]["notes"][0]["provenance"]["evidence_refs"] = json!([]);
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let mut edit = note_editor::template(&path).unwrap();
    edit.rows[0].midi_note = 61;
    edit.rows[0].rationale = "No evidence available".into();
    let output = temp.path().join("unsupported.json");
    assert!(note_editor::apply(&path, &save(temp.path(), &edit), &output).is_err());
    assert!(!output.exists());
}

#[test]
fn real_cli_front_door_keeps_private_identity_out_of_its_summary() {
    let temp = tempfile::tempdir().unwrap();
    let (path, mut edit) = setup(temp.path());
    let binary = env!("CARGO_BIN_EXE_reel-music-editor");
    let editor = temp.path().join("editor");
    let result = std::process::Command::new(binary)
        .arg("export")
        .arg(&path)
        .arg(&editor)
        .output()
        .unwrap();
    assert!(result.status.success());
    let summary: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        summary,
        json!({"notes":4,"media_resolved":false,"native_validation":"not-run"})
    );
    edit.rows[0].midi_note = 61;
    edit.rows[0].rationale = "Synthetic CLI proposal".into();
    let output = temp.path().join("proposal.json");
    let result = std::process::Command::new(binary)
        .arg("apply")
        .arg(&path)
        .arg(save(temp.path(), &edit))
        .arg(&output)
        .output()
        .unwrap();
    assert!(result.status.success());
    let summary: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        summary,
        json!({"changed_notes":1,"review":"not-reviewed","media_resolved":false,"native_validation":"not-run"})
    );
}
