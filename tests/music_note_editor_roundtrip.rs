//! Real native exporter round trip over synthetic, isolated controls and PCM.
use reel_music::{export, model, note_editor, underlay_editor};
use std::{fs, path::PathBuf};
fn fixture(root: &std::path::Path) -> [&'static str; 5] {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifests/fixtures");
    for family in ["music-model-corrected", "music-repair-foundation"] {
        fs::create_dir(root.join(family)).unwrap();
    }
    let files = [
        "music-model-corrected/model.yaml",
        "music-model-corrected/analysis.yaml",
        "music-model-corrected/lyrics.txt",
        "music-repair-foundation/source.yaml",
        "music-repair-foundation/source.u8",
    ];
    for name in files {
        fs::copy(fixtures.join(name), root.join(name)).unwrap();
    }
    files
}
#[test]
fn unreviewed_note_candidate_passes_native_model_and_score_packet_checks() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let files = fixture(root);
    let original = files.map(|name| fs::read(root.join(name)).unwrap());
    let source = root.join(files[0]);
    model::validate(&source).unwrap();
    let mut edit = note_editor::template(&source).unwrap();
    edit.rows[0].midi_note = 61;
    edit.rows[0].duration_ticks = 900;
    edit.rows[0].rationale = "Synthetic pitch/release proposal for round-trip testing.".into();
    let request = root.join("edit.json");
    fs::write(&request, serde_json::to_vec(&edit).unwrap()).unwrap();
    let candidate = root.join("music-model-corrected/proposal.json");
    assert_eq!(
        note_editor::apply(&source, &request, &candidate).unwrap(),
        1
    );
    let validation = model::validate(&candidate).unwrap();
    assert!(validation.verified);
    assert_eq!(validation.notes, 4);
    assert_eq!(validation.human_corrected_events, 0);
    let plan = root.join("plan.json");
    export::write(&candidate, &plan).unwrap();
    let packet = root.join("packet");
    reel::music_score::render(&plan, &candidate, &packet).unwrap();
    reel::music_score::check(&packet.join("receipt.json"), &plan, &candidate, &packet).unwrap();
    assert!(packet.join("lead-sheet.svg").is_file());
    for (i, name) in files.iter().enumerate() {
        assert_eq!(fs::read(root.join(name)).unwrap(), original[i]);
    }
    assert!(note_editor::apply(&source, &request, &candidate).is_err());
}

#[test]
fn exact_lyric_melisma_candidate_passes_native_validation_and_export_without_changing_source() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let files = fixture(root);
    let original = files.map(|name| fs::read(root.join(name)).unwrap());
    let source = root.join(files[0]);
    let lyric = root.join(files[2]);
    let mut edit = underlay_editor::template(&source, &lyric).unwrap();
    edit.underlay[0].note_ids.push("note-2".into());
    edit.underlay[0].text_end_byte = 5;
    edit.underlay.remove(1);
    edit.rationale = "One exact source phrase sung across two notes.".into();
    let request = root.join("underlay-edit.json");
    fs::write(&request, serde_json::to_vec(&edit).unwrap()).unwrap();
    let candidate = root.join("music-model-corrected/underlay-proposal.json");
    assert_eq!(
        underlay_editor::apply(&source, &lyric, &request, &candidate).unwrap(),
        3
    );
    let validation = model::validate(&candidate).unwrap();
    assert!(validation.verified);
    assert_eq!(validation.notes, 4);
    let before = model::load(&source).unwrap();
    let after = model::load(&candidate).unwrap();
    assert_eq!(before.parts, after.parts);
    assert_eq!(before.lyric_layers, after.lyric_layers);
    let plan = root.join("underlay-plan.json");
    export::write(&candidate, &plan).unwrap();
    let packet = root.join("underlay-packet");
    reel::music_score::render(&plan, &candidate, &packet).unwrap();
    reel::music_score::check(&packet.join("receipt.json"), &plan, &candidate, &packet).unwrap();
    for (i, name) in files.iter().enumerate() {
        assert_eq!(fs::read(root.join(name)).unwrap(), original[i]);
    }
    let mut invalid = after;
    invalid.lead_sheet.as_mut().unwrap().underlay[0]
        .note_ids
        .reverse();
    let rejected = root.join("music-model-corrected/reversed.json");
    fs::write(&rejected, serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert!(model::validate(&rejected).is_err());
}
