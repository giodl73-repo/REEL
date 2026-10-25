use std::{fs, path::Path, process::Command};

use reel_music::model::{
    InstrumentalClef, InstrumentalScore, InstrumentalScorePart, PartRole, PianoVocalScore,
};

use tempfile::tempdir;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(args)
        .output()
        .expect("REEL command runs")
}

fn instrumental_fixture(root: &Path) -> std::path::PathBuf {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = repository.join("manifests/fixtures/music-model-corrected/model.yaml");
    let parent = source.parent().unwrap();
    let mut model = reel_music::model::load(&source).unwrap();
    model.source.manifest = parent.join(&model.source.manifest);
    for a in &mut model.analyses {
        a.manifest = parent.join(&a.manifest);
    }
    for layer in &mut model.lyric_layers {
        layer.path = parent.join(&layer.path);
    }
    let mut right = model.parts[0].clone();
    right.id = "keys-right".into();
    right.name = "Keys right".into();
    right.role = PartRole::Harmony;
    for note in &mut right.notes {
        note.id = format!("right-{}", note.id);
        note.midi_note += 12;
    }
    right.notes[0].voice = 2;
    right.notes[0].duration_ticks = 4800;
    let mut chord_note = right.notes[0].clone();
    chord_note.id = "right-chord-extra".into();
    chord_note.midi_note += 4;
    right.notes.insert(1, chord_note);
    let mut left = model.parts[0].clone();
    left.id = "keys-left".into();
    left.name = "Keys left".into();
    left.role = PartRole::Bass;
    for note in &mut left.notes {
        note.id = format!("left-{}", note.id);
        note.midi_note -= 12;
    }
    let mut drums = model.parts[0].clone();
    drums.id = "percussion".into();
    drums.name = "Percussion".into();
    drums.role = PartRole::Rhythm;
    for note in &mut drums.notes {
        note.id = format!("drum-{}", note.id);
        note.midi_note = 38;
    }
    model.parts.extend([right, left, drums]);
    let mut meter = model.meter_map[0].clone();
    meter.tick = 4800;
    meter.numerator = 3;
    model.meter_map.push(meter);
    model.instrumental_score = Some(InstrumentalScore {
        title: "Synthetic instrumental score".into(),
        pickup_ticks: 960,
        key_fifths: Some(-1),
        parts: vec![
            InstrumentalScorePart {
                id: "lead".into(),
                name: "Reed".into(),
                staff_part_ids: vec!["melody".into()],
                clef: InstrumentalClef::Treble,
            },
            InstrumentalScorePart {
                id: "keys".into(),
                name: "Keyboard".into(),
                staff_part_ids: vec!["keys-right".into(), "keys-left".into()],
                clef: InstrumentalClef::Grand,
            },
            InstrumentalScorePart {
                id: "drums".into(),
                name: "Percussion".into(),
                staff_part_ids: vec!["percussion".into()],
                clef: InstrumentalClef::Percussion,
            },
        ],
    });
    let path = root.join("instrumental-model.json");
    fs::write(&path, serde_json::to_vec_pretty(&model).unwrap()).unwrap();
    reel_music::model::validate(&path).unwrap();
    path
}

#[test]
fn measured_instrumental_export_preserves_parts_ties_and_old_packets() {
    let temp = tempdir().unwrap();
    let path = instrumental_fixture(temp.path());
    let plan = temp.path().join("plan.json");
    reel_music::export::write(&path, &plan).unwrap();
    let packet = temp.path().join("packet");
    let report = reel::music_score::render(&plan, &path, &packet).unwrap();
    assert_eq!(report.instrumental_score_valid, Some(true));
    let xml = fs::read_to_string(packet.join("instrumental-score.musicxml")).unwrap();
    assert!(xml.contains("reel:layout=\"instrumental\""));
    assert!(xml.contains("<fifths>-1</fifths>"));
    assert!(xml.contains("<staves>2</staves>"));
    assert!(xml.contains("<unpitched>"));
    assert!(xml.contains("<midi-unpitched>39</midi-unpitched>"));
    assert!(xml.contains("<tie type=\"start\"/>"));
    assert!(xml.contains("<rest/>"));
    assert!(!xml.contains("<lyric>"));
    let tied_note = xml
        .lines()
        .find(|line| line.contains("reel:note-id=\"right-") && line.contains("<tie "))
        .unwrap();
    assert!(tied_note.find("<tie ").unwrap() < tied_note.find("<voice>").unwrap());
    assert!(xml.rfind("</score-instrument>").unwrap() < xml.find("<midi-instrument ").unwrap());
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(packet.join("receipt.json")).unwrap()).unwrap();
    let comparison = &receipt["round_trip"]["instrumental_score"];
    assert_eq!(comparison["source_notes"], 17);
    assert_eq!(comparison["score_parts"], 3);
    assert_eq!(comparison["measures"], 3);
    assert!(comparison["written_fragments"].as_u64().unwrap() > 17);
    assert!(xml.contains("<chord/>"));
    let mut legacy = reel_music::model::load(&path).unwrap();
    legacy.instrumental_score = None;
    let legacy_path = temp.path().join("legacy.json");
    fs::write(&legacy_path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();
    let legacy_plan = temp.path().join("legacy-plan.json");
    reel_music::export::write(&legacy_path, &legacy_plan).unwrap();
    let old_packet = temp.path().join("old-packet");
    reel::music_score::render(&legacy_plan, &legacy_path, &old_packet).unwrap();
    for name in [
        "score.mid",
        "score.musicxml",
        "lead-sheet.svg",
        "rehearsal-guide.wav",
    ] {
        assert_eq!(
            fs::read(packet.join(name)).unwrap(),
            fs::read(old_packet.join(name)).unwrap()
        );
    }
    fs::write(
        packet.join("instrumental-score.musicxml"),
        xml.replacen("<step>C</step>", "<step>D</step>", 1),
    )
    .unwrap();
    assert!(reel::music_score::check(&packet.join("receipt.json"), &plan, &path, &packet).is_err());
}

#[test]
fn instrumental_layout_rejects_omissions_duplicates_bad_clefs_and_keys() {
    let temp = tempdir().unwrap();
    let path = instrumental_fixture(temp.path());
    let model = reel_music::model::load(&path).unwrap();
    for mutation in 0..5 {
        let mut bad = model.clone();
        let score = bad.instrumental_score.as_mut().unwrap();
        match mutation {
            0 => {
                score.parts.pop();
            }
            1 => score.parts[2].staff_part_ids[0] = "melody".into(),
            2 => score.parts[1].staff_part_ids.pop().map(|_| ()).unwrap(),
            3 => score.parts[0].clef = InstrumentalClef::Percussion,
            _ => score.key_fifths = Some(8),
        }
        fs::write(&path, serde_json::to_vec_pretty(&bad).unwrap()).unwrap();
        assert!(reel_music::model::validate(&path).is_err());
    }
}

#[test]
fn cli_exports_rechecks_and_detects_score_tampering() {
    let temporary = tempdir().unwrap();
    let plan = temporary.path().join("plan.json");
    let packet = temporary.path().join("packet");
    let model = "manifests/fixtures/music-model-corrected/model.yaml";

    let planned = run(&[
        "music-score-export-plan",
        model,
        "--output-path",
        plan.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert!(
        planned.status.success(),
        "{}",
        String::from_utf8_lossy(&planned.stderr)
    );

    let rendered = run(&[
        "music-score-export-render",
        plan.to_str().unwrap(),
        model,
        "--output-dir",
        packet.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert!(
        rendered.status.success(),
        "{}",
        String::from_utf8_lossy(&rendered.stderr)
    );
    let rendered_report = String::from_utf8_lossy(&rendered.stdout);
    assert!(rendered_report.contains("\"midi_round_trip\": true"));
    assert!(rendered_report.contains("\"musicxml_round_trip\": true"));
    assert!(rendered_report.contains("\"lead_sheet_valid\": true"));
    assert!(rendered_report.contains("\"shareable\": false"));

    let receipt = packet.join("receipt.json");
    let checked = run(&[
        "music-score-export-check",
        receipt.to_str().unwrap(),
        plan.to_str().unwrap(),
        model,
        packet.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let lead_sheet_path = packet.join("lead-sheet.svg");
    let lead_sheet = fs::read_to_string(&lead_sheet_path).unwrap();
    assert!(lead_sheet.contains("data-clef=\"treble\""));
    assert_eq!(lead_sheet.matches("data-note-id=").count(), 4);
    for syllable in ["la", "le", "li", "lo"] {
        assert!(lead_sheet.contains(&format!(">{syllable}</text>")));
    }

    let musicxml_path = packet.join("score.musicxml");
    let musicxml = fs::read_to_string(&musicxml_path).unwrap();
    fs::write(
        &musicxml_path,
        musicxml.replacen("<step>C</step>", "<step>D</step>", 1),
    )
    .unwrap();
    let tampered = run(&[
        "music-score-export-check",
        receipt.to_str().unwrap(),
        plan.to_str().unwrap(),
        model,
        packet.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert!(!tampered.status.success());
    assert!(String::from_utf8_lossy(&tampered.stderr).contains("round-trip comparison"));

    fs::write(&musicxml_path, musicxml).unwrap();
    fs::write(
        &lead_sheet_path,
        lead_sheet.replace(">la</text>", ">xx</text>"),
    )
    .unwrap();
    let tampered = run(&[
        "music-score-export-check",
        receipt.to_str().unwrap(),
        plan.to_str().unwrap(),
        model,
        packet.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert!(!tampered.status.success());
    assert!(String::from_utf8_lossy(&tampered.stderr).contains("lead-sheet SVG"));
}

#[test]
fn exports_measured_piano_vocal_musicxml_without_changing_legacy_packets() {
    let temporary = tempdir().unwrap();
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_model = repository.join("manifests/fixtures/music-model-corrected/model.yaml");
    let source_parent = source_model.parent().unwrap();
    let mut model = reel_music::model::load(&source_model).unwrap();
    model.source.manifest = source_parent.join(&model.source.manifest);
    for analysis in &mut model.analyses {
        analysis.manifest = source_parent.join(&analysis.manifest);
    }
    model.lyric_layers[0].path = source_parent.join(&model.lyric_layers[0].path);
    model.model_id = "synthetic-piano-vocal-score".into();
    model.parts[0].role = PartRole::Vocal;
    let mut right = model.parts[0].clone();
    right.id = "piano-right".into();
    right.name = "Piano right hand".into();
    right.role = PartRole::Harmony;
    for note in &mut right.notes {
        note.id = format!("rh-{}", note.id);
        note.midi_note += 12;
    }
    right.notes[3].duration_ticks = 2_400;
    let mut left = model.parts[0].clone();
    left.id = "piano-left".into();
    left.name = "Piano left hand".into();
    left.role = PartRole::Bass;
    for note in &mut left.notes {
        note.id = format!("lh-{}", note.id);
        note.midi_note -= 12;
    }
    let mut inner_voice = left.notes[0].clone();
    inner_voice.id = "lh-inner-voice".into();
    inner_voice.voice = 2;
    inner_voice.midi_note += 7;
    left.notes.insert(1, inner_voice);
    model.parts.extend([right, left]);
    let mut meter_change = model.meter_map[0].clone();
    meter_change.tick = 4_800;
    meter_change.numerator = 3;
    model.meter_map.push(meter_change);
    model.piano_vocal_score = Some(PianoVocalScore {
        title: "Synthetic piano and vocal score".into(),
        vocal_part_id: "melody".into(),
        piano_right_hand_part_id: "piano-right".into(),
        piano_left_hand_part_id: "piano-left".into(),
        pickup_ticks: 960,
    });
    let model_path = temporary.path().join("model.yaml");
    fs::write(&model_path, serde_yaml::to_string(&model).unwrap()).unwrap();
    reel_music::model::validate(&model_path).unwrap();
    let mut invalid = model.clone();
    invalid
        .piano_vocal_score
        .as_mut()
        .unwrap()
        .piano_left_hand_part_id = "piano-right".into();
    let invalid_path = temporary.path().join("invalid-duplicate-hand.yaml");
    fs::write(&invalid_path, serde_yaml::to_string(&invalid).unwrap()).unwrap();
    assert!(reel_music::model::validate(&invalid_path).is_err());
    invalid = model.clone();
    invalid.piano_vocal_score.as_mut().unwrap().pickup_ticks = 3_840;
    let invalid_path = temporary.path().join("invalid-pickup.yaml");
    fs::write(&invalid_path, serde_yaml::to_string(&invalid).unwrap()).unwrap();
    assert!(reel_music::model::validate(&invalid_path).is_err());

    let plan_path = temporary.path().join("plan.json");
    let plan = reel_music::export::write(&model_path, &plan_path).unwrap();
    assert_eq!(plan.artifacts, 5);
    let packet = temporary.path().join("packet");
    let report = reel::music_score::render(&plan_path, &model_path, &packet).unwrap();
    assert_eq!(report.piano_vocal_score_valid, Some(true));
    let xml = fs::read_to_string(packet.join("piano-vocal.musicxml")).unwrap();
    assert!(xml.contains("reel:layout=\"piano-vocal\""));
    assert!(xml.contains("<staves>2</staves>"));
    assert!(xml.contains("<staff>1</staff>"));
    assert!(xml.contains("<staff>2</staff>"));
    assert_eq!(xml.matches("<measure number=").count(), 6);
    assert!(xml.contains("<text>la</text>"));
    assert!(xml.contains("<beats>3</beats>"));
    assert!(xml.contains("<tie type=\"start\"/>"));
    assert!(xml.contains("<tie type=\"stop\"/>"));
    assert!(xml.contains("<backup>"));
    assert!(xml.contains("<rest/>"));

    let receipt = packet.join("receipt.json");
    fs::write(
        packet.join("piano-vocal.musicxml"),
        xml.replacen("<text>la</text>", "<text>xx</text>", 1),
    )
    .unwrap();
    assert!(reel::music_score::check(&receipt, &plan_path, &model_path, &packet).is_err());
}
