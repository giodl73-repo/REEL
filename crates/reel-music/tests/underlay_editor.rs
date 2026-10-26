use reel_music::{
    hash::sha256_bytes,
    model,
    underlay_editor::{self, Edit},
};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};
fn fixture(root: &Path) -> (PathBuf, PathBuf, Edit) {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../manifests/fixtures/music-model-corrected");
    let path = root.join("model.yaml");
    let lyric = root.join("lyrics.txt");
    fs::copy(source.join("model.yaml"), &path).unwrap();
    fs::copy(source.join("lyrics.txt"), &lyric).unwrap();
    let edit = underlay_editor::template(&path, &lyric).unwrap();
    (path, lyric, edit)
}
fn request(root: &Path, edit: &Edit) -> PathBuf {
    let path = root.join("edit.json");
    fs::write(&path, serde_json::to_vec(edit).unwrap()).unwrap();
    path
}
fn merge(edit: &mut Edit) {
    edit.underlay[0].note_ids.push("note-2".into());
    edit.underlay[0].text_end_byte = 5;
    edit.underlay.remove(1);
    edit.rationale = "One source phrase across two notes.".into();
}
#[test]
fn explicit_lyric_editor_preserves_text_notes_authority_and_all_original_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let (path, lyric, mut edit) = fixture(temp.path());
    let originals = [fs::read(&path).unwrap(), fs::read(&lyric).unwrap()];
    let output = temp.path().join("editor");
    assert_eq!(underlay_editor::export(&path, &lyric, &output).unwrap(), 4);
    assert!(underlay_editor::export(&path, &lyric, &output).is_err());
    let session: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("session.json")).unwrap()).unwrap();
    assert_eq!(session["lyric_bytes_verified"], true);
    assert_eq!(session["audio_resolved"], false);
    assert_eq!(session["native_validation"], "not-run");
    merge(&mut edit);
    let output = temp.path().join("proposal.json");
    let req = request(temp.path(), &edit);
    assert_eq!(
        underlay_editor::apply(&path, &lyric, &req, &output).unwrap(),
        3
    );
    let source = model::load(&path).unwrap();
    let candidate = model::load(&output).unwrap();
    assert_eq!(candidate.parts, source.parts);
    assert_eq!(candidate.source, source.source);
    assert_eq!(candidate.analyses, source.analyses);
    assert_eq!(candidate.lyric_layers, source.lyric_layers);
    assert_eq!(
        candidate.lead_sheet.as_ref().unwrap().title,
        source.lead_sheet.as_ref().unwrap().title
    );
    assert_eq!(candidate.review.status, "not-reviewed");
    assert!(candidate.review.decision_refs.is_empty());
    assert_eq!(candidate.authority.status, "not-reviewed");
    assert!(
        candidate
            .review
            .required_roles
            .iter()
            .any(|role| role == "lyrics-vocal-adaptation-editor")
    );
    assert_eq!(fs::read(&path).unwrap(), originals[0]);
    assert_eq!(fs::read(&lyric).unwrap(), originals[1]);
    let bytes = fs::read(&output).unwrap();
    assert!(underlay_editor::apply(&path, &lyric, &req, &output).is_err());
    assert_eq!(fs::read(output).unwrap(), bytes);
}
#[test]
fn invalid_domains_coverage_order_and_first_melisma_reverse_refuse_before_output() {
    let temp = tempfile::tempdir().unwrap();
    let (path, lyric, original) = fixture(temp.path());
    for case in 0..11 {
        let mut edit = original.clone();
        edit.rationale = "Synthetic rejection fixture.".into();
        match case {
            0 => edit.expected_model_sha256 = "0".repeat(64),
            1 => edit.expected_lyric_sha256 = "0".repeat(64),
            2 => edit.lyric_layer_id = "foreign".into(),
            3 => {
                edit.underlay.pop();
            }
            4 => edit.underlay[1].note_ids = edit.underlay[0].note_ids.clone(),
            5 => edit.underlay[0].note_ids = vec!["foreign".into()],
            6 => edit.underlay[0].text_end_byte = 20,
            7 => edit.underlay[1].text_start_byte = 0,
            8 => {
                merge(&mut edit);
                edit.underlay[0].note_ids.reverse();
            }
            9 => {
                merge(&mut edit);
                edit.rationale.clear();
            }
            _ => edit.underlay[1].id = edit.underlay[0].id.clone(),
        }
        let output = temp.path().join(format!("bad-{case}.json"));
        assert!(
            underlay_editor::apply(&path, &lyric, &request(temp.path(), &edit), &output).is_err(),
            "case {case}"
        );
        assert!(!output.exists());
    }
}
#[test]
fn unicode_crlf_and_incomplete_source_maps_can_be_repaired_without_rewriting_text() {
    let temp = tempfile::tempdir().unwrap();
    let (path, lyric, _) = fixture(temp.path());
    let raw = "Á😀\r\nñ fin".as_bytes();
    fs::write(&lyric, raw).unwrap();
    let mut model = model::load(&path).unwrap();
    model.lyric_layers[0].sha256 = sha256_bytes(raw);
    model.lead_sheet.as_mut().unwrap().underlay.clear();
    fs::write(&path, serde_json::to_vec(&model).unwrap()).unwrap();
    assert_eq!(
        underlay_editor::export(&path, &lyric, &temp.path().join("incomplete")).unwrap(),
        0
    );
    let mut edit = underlay_editor::template(&path, &lyric).unwrap();
    edit.underlay = vec![model::LyricUnderlay {
        id: "phrase".into(),
        note_ids: vec![
            "note-1".into(),
            "note-2".into(),
            "note-3".into(),
            "note-4".into(),
        ],
        text_start_byte: 0,
        text_end_byte: raw.len() as u64,
        syllabic: model::Syllabic::Single,
    }];
    edit.rationale = "Whole exact source phrase across the melody.".into();
    let output = temp.path().join("unicode.json");
    underlay_editor::apply(&path, &lyric, &request(temp.path(), &edit), &output).unwrap();
    assert_eq!(fs::read(&lyric).unwrap(), raw);
    edit.underlay[0].text_start_byte = 1;
    let output = temp.path().join("split-character.json");
    assert!(underlay_editor::apply(&path, &lyric, &request(temp.path(), &edit), &output).is_err());
    assert!(!output.exists());
}
#[test]
fn stale_bytes_wrong_paths_unsafe_outputs_and_authority_fields_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let (path, lyric, edit) = fixture(temp.path());
    let req = request(temp.path(), &edit);
    let alias = temp.path().join("same-bytes.txt");
    fs::copy(&lyric, &alias).unwrap();
    assert!(underlay_editor::template(&path, &alias).is_err());
    let other = temp.path().join("other");
    fs::create_dir(&other).unwrap();
    assert!(underlay_editor::apply(&path, &lyric, &req, &other.join("bad.json")).is_err());
    let mut raw = fs::read(&path).unwrap();
    raw.push(b'\n');
    fs::write(&path, raw).unwrap();
    assert!(
        underlay_editor::apply(&path, &lyric, &req, &temp.path().join("stale-model.json")).is_err()
    );
    let mut value =
        serde_json::to_value(underlay_editor::template(&path, &lyric).unwrap()).unwrap();
    value["human_approval"] = json!(true);
    fs::write(&req, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        underlay_editor::apply(&path, &lyric, &req, &temp.path().join("approval.json")).is_err()
    );
    fs::write(&lyric, b"changed").unwrap();
    assert!(underlay_editor::export(&path, &lyric, &temp.path().join("stale-lyric")).is_err());
    assert!(!temp.path().join("stale-lyric").exists());
}
#[test]
fn real_underlay_cli_emits_only_counts_and_validation_states() {
    let temp = tempfile::tempdir().unwrap();
    let (path, lyric, mut edit) = fixture(temp.path());
    let binary = env!("CARGO_BIN_EXE_reel-music-editor");
    let result = std::process::Command::new(binary)
        .arg("export-underlay")
        .arg(&path)
        .arg(&lyric)
        .arg(temp.path().join("editor"))
        .output()
        .unwrap();
    assert!(result.status.success());
    let summary: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        summary,
        json!({"spans":4,"lyric_bytes_verified":true,"audio_resolved":false,"native_validation":"not-run"})
    );
    merge(&mut edit);
    let result = std::process::Command::new(binary)
        .arg("apply-underlay")
        .arg(&path)
        .arg(&lyric)
        .arg(request(temp.path(), &edit))
        .arg(temp.path().join("proposal.json"))
        .output()
        .unwrap();
    assert!(result.status.success());
    let summary: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        summary,
        json!({"spans":3,"review":"not-reviewed","lyric_bytes_verified":true,"audio_resolved":false,"native_validation":"not-run"})
    );
}
