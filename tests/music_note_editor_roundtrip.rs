//! Real native exporter round trip over synthetic, isolated controls and PCM.
use reel_music::{export, model, note_editor};
use std::{fs, path::PathBuf};
#[test]
fn unreviewed_note_candidate_passes_native_model_and_score_packet_checks() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
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
