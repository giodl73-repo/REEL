use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::Path,
};

use anyhow::{Context, Result, anyhow, bail};
use reel_music::{
    export::{self, ScoreExportPlan},
    hash::{canonical_sha256, sha256_path},
    model::{LyricLayerKind, MusicModel, PartRole},
};
use serde::{Deserialize, Serialize};
use tempfile::Builder;

pub const RECEIPT_SCHEMA: &str = "reel.music-score-export-receipt.v0.1";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExportReceipt {
    pub schema: String,
    pub export_id: String,
    pub plan_sha256: String,
    pub plan_contract_sha256: String,
    pub model_manifest_sha256: String,
    pub model_contract_sha256: String,
    pub artifacts: Vec<ArtifactReceipt>,
    pub round_trip: RoundTripReceipt,
    pub limitations: Vec<String>,
    pub shareable: bool,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactReceipt {
    pub kind: String,
    pub filename: String,
    pub sha256: String,
    pub bytes: u64,
    pub adapter: String,
    pub adapter_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoundTripReceipt {
    pub midi: Comparison,
    pub musicxml: Comparison,
    pub rehearsal_guide: GuideComparison,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_sheet: Option<LeadSheetComparison>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LeadSheetComparison {
    pub deterministic_svg_equal: bool,
    pub treble_clef: bool,
    pub melody_notes: usize,
    pub harmony_symbols: usize,
    pub lyric_syllables: usize,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    pub duration_ticks_equal: bool,
    pub tempo_map_equal: bool,
    pub meter_map_equal: bool,
    pub form_equal: bool,
    pub notes_equal: bool,
    pub lyric_layer_bindings_equal: bool,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuideComparison {
    pub riff_wave_header_valid: bool,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub samples_per_channel: u64,
    pub expected_samples_per_channel: u64,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportReport {
    pub schema: String,
    pub output_dir: String,
    pub receipt: String,
    pub receipt_sha256: String,
    pub artifacts: usize,
    pub midi_round_trip: bool,
    pub musicxml_round_trip: bool,
    pub rehearsal_guide_valid: bool,
    pub lead_sheet_valid: Option<bool>,
    pub shareable: bool,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    ppq: u32,
    duration: u64,
    tempos: Vec<(u64, u32)>,
    meters: Vec<(u64, u8, u8)>,
    form: Vec<(u64, String, String)>,
    notes: Vec<NoteSnapshot>,
    lyrics: Vec<(String, String, String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct NoteSnapshot {
    part_id: String,
    voice: u8,
    start: u64,
    duration: u64,
    pitch: u8,
    velocity: u8,
}

pub fn render(plan_path: &Path, model_path: &Path, output_dir: &Path) -> Result<ExportReport> {
    if output_dir.exists() {
        bail!(
            "score export output directory already exists: {}",
            output_dir.display()
        );
    }
    export::validate(plan_path, model_path)?;
    let plan = export::load(plan_path)?;
    let model = reel_music::model::load(model_path)?;
    let parent = output_dir.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = Builder::new()
        .prefix(".reel-score-export-")
        .tempdir_in(parent)?;
    let midi = write_midi(&model)?;
    let xml = write_musicxml(&model)?;
    let wav = write_guide_wav(&model, &plan)?;
    write_bytes(&temporary.path().join("score.mid"), &midi)?;
    write_bytes(&temporary.path().join("score.musicxml"), xml.as_bytes())?;
    write_bytes(&temporary.path().join("rehearsal-guide.wav"), &wav)?;
    if model.lead_sheet.is_some() {
        let lead_sheet = write_lead_sheet_svg(model_path, &model)?;
        write_bytes(
            &temporary.path().join("lead-sheet.svg"),
            lead_sheet.as_bytes(),
        )?;
    }

    let receipt = build_receipt(plan_path, model_path, temporary.path(), &plan, &model)?;
    let receipt_path = temporary.path().join("receipt.json");
    write_bytes(&receipt_path, &serde_json::to_vec_pretty(&receipt)?)?;
    let temporary_path = temporary.keep();
    fs::rename(&temporary_path, output_dir).with_context(|| {
        format!(
            "failed to publish score export {} from {}",
            output_dir.display(),
            temporary_path.display()
        )
    })?;
    report(output_dir, &receipt)
}

pub fn check(
    receipt_path: &Path,
    plan_path: &Path,
    model_path: &Path,
    output_dir: &Path,
) -> Result<ExportReport> {
    export::validate(plan_path, model_path)?;
    let bytes = fs::read(receipt_path)
        .with_context(|| format!("failed to read {}", receipt_path.display()))?;
    let actual: ExportReceipt = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid score export receipt: {}", receipt_path.display()))?;
    let plan = export::load(plan_path)?;
    let model = reel_music::model::load(model_path)?;
    let expected = build_receipt(plan_path, model_path, output_dir, &plan, &model)?;
    if actual != expected {
        bail!("score export receipt or exported artifacts do not match current inputs");
    }
    report(output_dir, &actual)
}

fn build_receipt(
    plan_path: &Path,
    model_path: &Path,
    output_dir: &Path,
    plan: &ScoreExportPlan,
    model: &MusicModel,
) -> Result<ExportReceipt> {
    let expected = snapshot(model);
    let midi_path = output_dir.join("score.mid");
    let xml_path = output_dir.join("score.musicxml");
    let wav_path = output_dir.join("rehearsal-guide.wav");
    let midi_snapshot = parse_midi(&fs::read(&midi_path)?)?;
    let xml_snapshot = parse_musicxml(&fs::read_to_string(&xml_path)?)?;
    let midi_comparison = compare(&expected, &midi_snapshot);
    let xml_comparison = compare(&expected, &xml_snapshot);
    if !midi_comparison.passed || !xml_comparison.passed {
        bail!("score export failed independent round-trip comparison");
    }
    let guide = inspect_wav(&fs::read(&wav_path)?, expected_guide_samples(model, plan)?)?;
    if !guide.passed {
        bail!("rehearsal guide WAV validation failed");
    }
    let requests = plan
        .artifacts
        .iter()
        .map(|request| (request.filename.as_str(), request))
        .collect::<BTreeMap<_, _>>();
    let lead_sheet = if let Some(sheet) = &model.lead_sheet {
        let path = output_dir.join("lead-sheet.svg");
        let actual = fs::read_to_string(&path)?;
        let expected_svg = write_lead_sheet_svg(model_path, model)?;
        let melody = model
            .parts
            .iter()
            .find(|part| part.id == sheet.melody_part_id)
            .expect("validated lead-sheet melody");
        let comparison = LeadSheetComparison {
            deterministic_svg_equal: actual == expected_svg,
            treble_clef: actual.contains("data-clef=\"treble\""),
            melody_notes: melody.notes.len(),
            harmony_symbols: model.harmony.len(),
            lyric_syllables: sheet.underlay.len(),
            passed: actual == expected_svg && actual.contains("data-clef=\"treble\""),
        };
        if !comparison.passed {
            bail!("lead-sheet SVG validation failed");
        }
        Some(comparison)
    } else {
        None
    };
    let mut expected_artifacts = vec![
        ("midi-smf", "score.mid"),
        ("musicxml-score-partwise", "score.musicxml"),
        ("rehearsal-guide-wav", "rehearsal-guide.wav"),
    ];
    if lead_sheet.is_some() {
        expected_artifacts.push(("printable-lead-sheet-svg", "lead-sheet.svg"));
    }
    let mut artifacts = Vec::new();
    for (kind, filename) in expected_artifacts {
        let request = requests
            .get(filename)
            .ok_or_else(|| anyhow!("plan does not request {filename}"))?;
        if request.kind != kind {
            bail!("plan artifact kind mismatch for {filename}");
        }
        let path = output_dir.join(filename);
        artifacts.push(ArtifactReceipt {
            kind: kind.into(),
            filename: filename.into(),
            sha256: sha256_path(&path)?,
            bytes: fs::metadata(&path)?.len(),
            adapter: request.adapter.clone(),
            adapter_version: request.adapter_version.clone(),
        });
    }
    let mut limitations = vec![
        "round-trip proves the declared structural fields, not notation layout or playability".into(),
        "the square-wave guide is for pitch and timing rehearsal, not creative scoring or mastering".into(),
        "technical verification is not human approval, rights clearance, or publication authorization".into(),
    ];
    if lead_sheet.is_some() {
        limitations.push("lead-sheet SVG is deterministic rehearsal engraving; a musician must review spacing, page turns, enharmonic spelling, and playability".into());
    } else {
        limitations.push("lyric bindings preserve layer identities and hashes; no lead-sheet underlay was declared".into());
    }
    Ok(ExportReceipt {
        schema: RECEIPT_SCHEMA.into(),
        export_id: plan.export_id.clone(),
        plan_sha256: sha256_path(plan_path)?,
        plan_contract_sha256: canonical_sha256(plan)?,
        model_manifest_sha256: sha256_path(model_path)?,
        model_contract_sha256: canonical_sha256(model)?,
        artifacts,
        round_trip: RoundTripReceipt {
            midi: midi_comparison,
            musicxml: xml_comparison,
            rehearsal_guide: guide,
            lead_sheet,
        },
        limitations,
        shareable: false,
        verified: true,
    })
}

fn report(output_dir: &Path, receipt: &ExportReceipt) -> Result<ExportReport> {
    let receipt_path = output_dir.join("receipt.json");
    Ok(ExportReport {
        schema: RECEIPT_SCHEMA.into(),
        output_dir: output_dir.display().to_string(),
        receipt: receipt_path.display().to_string(),
        receipt_sha256: sha256_path(&receipt_path)?,
        artifacts: receipt.artifacts.len(),
        midi_round_trip: receipt.round_trip.midi.passed,
        musicxml_round_trip: receipt.round_trip.musicxml.passed,
        rehearsal_guide_valid: receipt.round_trip.rehearsal_guide.passed,
        lead_sheet_valid: receipt
            .round_trip
            .lead_sheet
            .as_ref()
            .map(|value| value.passed),
        shareable: false,
        verified: true,
    })
}

fn snapshot(model: &MusicModel) -> Snapshot {
    let mut notes = model
        .parts
        .iter()
        .flat_map(|part| {
            part.notes.iter().map(|note| NoteSnapshot {
                part_id: part.id.clone(),
                voice: note.voice,
                start: note.start_tick,
                duration: note.duration_ticks,
                pitch: note.midi_note,
                velocity: note.velocity,
            })
        })
        .collect::<Vec<_>>();
    notes.sort();
    Snapshot {
        ppq: model.musical_timebase.pulses_per_quarter,
        duration: model.duration_ticks,
        tempos: model
            .tempo_map
            .iter()
            .map(|event| (event.tick, event.microseconds_per_quarter))
            .collect(),
        meters: model
            .meter_map
            .iter()
            .map(|event| (event.tick, event.numerator, event.denominator))
            .collect(),
        form: model
            .form
            .iter()
            .map(|section| {
                (
                    section.range.start,
                    section.id.clone(),
                    section.label.clone(),
                )
            })
            .collect(),
        notes,
        lyrics: model
            .lyric_layers
            .iter()
            .map(|layer| {
                (
                    layer.id.clone(),
                    lyric_kind(layer.kind).into(),
                    layer.language.clone(),
                    layer.sha256.clone(),
                )
            })
            .collect(),
    }
}

fn write_lead_sheet_svg(model_path: &Path, model: &MusicModel) -> Result<String> {
    let sheet = model
        .lead_sheet
        .as_ref()
        .ok_or_else(|| anyhow!("music model does not declare a lead sheet"))?;
    let melody = model
        .parts
        .iter()
        .find(|part| part.id == sheet.melody_part_id)
        .ok_or_else(|| anyhow!("validated lead-sheet melody is missing"))?;
    let mut notes = melody.notes.iter().collect::<Vec<_>>();
    notes.sort_by_key(|note| (note.start_tick, note.voice, note.midi_note));
    let lyric_text = if let Some(layer_id) = &sheet.lyric_layer_id {
        let layer = model
            .lyric_layers
            .iter()
            .find(|layer| &layer.id == layer_id)
            .ok_or_else(|| anyhow!("validated lead-sheet lyric layer is missing"))?;
        let path = if layer.path.is_absolute() {
            layer.path.clone()
        } else {
            model_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&layer.path)
        };
        Some(fs::read_to_string(path)?)
    } else {
        None
    };
    let mut lyrics = BTreeMap::<&str, (String, bool)>::new();
    if let Some(text) = &lyric_text {
        for item in &sheet.underlay {
            let start = usize::try_from(item.text_start_byte)?;
            let end = usize::try_from(item.text_end_byte)?;
            let syllable = text[start..end].to_string();
            for (index, note_id) in item.note_ids.iter().enumerate() {
                lyrics.insert(
                    note_id,
                    (
                        if index == 0 {
                            syllable.clone()
                        } else {
                            String::new()
                        },
                        index > 0,
                    ),
                );
            }
        }
    }
    let per_system = 12usize;
    let systems = notes.len().div_ceil(per_system).max(1);
    let width = 1_100usize;
    let height = 80 + systems * 150;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" role=\"img\" data-clef=\"treble\">\n"
    );
    svg.push_str("<style>text{font-family:serif;fill:#111}.title{font-size:24px;font-weight:bold}.chord{font-size:16px;font-weight:bold}.lyric{font-size:15px}.meta{font-size:11px;fill:#555}.staff{stroke:#222;stroke-width:1}.stem{stroke:#111;stroke-width:1.5}.note{fill:#111}</style>\n");
    svg.push_str(&format!(
        "<text class=\"title\" x=\"40\" y=\"34\">{}</text>\n",
        xml_escape(&sheet.title)
    ));
    for system in 0..systems {
        let staff_y = 80 + system * 150;
        svg.push_str(&format!(
            "<text x=\"42\" y=\"{}\" font-size=\"42\">𝄞</text>\n",
            staff_y + 32
        ));
        for line in 0..5 {
            let y = staff_y + line * 10;
            svg.push_str(&format!(
                "<line class=\"staff\" x1=\"80\" y1=\"{y}\" x2=\"1060\" y2=\"{y}\"/>\n"
            ));
        }
    }
    let mut last_harmony = None::<&str>;
    for (index, note) in notes.iter().enumerate() {
        let system = index / per_system;
        let column = index % per_system;
        let x = 115 + column * 78;
        let staff_y = 80 + system * 150;
        let y = (staff_y as i32 + 20 - (i32::from(note.midi_note) - 71) * 2)
            .clamp(staff_y as i32 - 18, staff_y as i32 + 58);
        if let Some(section) = model
            .form
            .iter()
            .find(|section| section.range.start == note.start_tick)
        {
            svg.push_str(&format!(
                "<text class=\"meta\" x=\"{x}\" y=\"{}\">{}</text>\n",
                staff_y - 34,
                xml_escape(&section.label)
            ));
        }
        if let Some(harmony) = model.harmony.iter().find(|harmony| {
            harmony.range.start <= note.start_tick && note.start_tick < harmony.range.end
        }) {
            if last_harmony != Some(harmony.id.as_str()) {
                svg.push_str(&format!(
                    "<text class=\"chord\" x=\"{x}\" y=\"{}\">{}</text>\n",
                    staff_y - 12,
                    xml_escape(&harmony.symbol)
                ));
                last_harmony = Some(&harmony.id);
            }
        }
        svg.push_str(&format!("<g data-note-id=\"{}\" data-start-tick=\"{}\" data-duration-ticks=\"{}\" data-midi-note=\"{}\"><ellipse class=\"note\" cx=\"{x}\" cy=\"{y}\" rx=\"7\" ry=\"5\" transform=\"rotate(-18 {x} {y})\"/><line class=\"stem\" x1=\"{}\" y1=\"{y}\" x2=\"{}\" y2=\"{}\"/></g>\n", xml_escape(&note.id), note.start_tick, note.duration_ticks, note.midi_note, x + 6, x + 6, y - 30));
        if let Some((syllable, melisma)) = lyrics.get(note.id.as_str()) {
            if !syllable.is_empty() {
                svg.push_str(&format!(
                    "<text class=\"lyric\" x=\"{x}\" y=\"{}\" text-anchor=\"middle\">{}</text>\n",
                    staff_y + 82,
                    xml_escape(syllable)
                ));
            }
            if *melisma {
                svg.push_str(&format!(
                    "<line class=\"staff\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>\n",
                    x - 18,
                    staff_y + 76,
                    x + 18,
                    staff_y + 76
                ));
            }
        }
    }
    svg.push_str(&format!("<text class=\"meta\" x=\"40\" y=\"{}\">Deterministic rehearsal lead sheet — musician engraving and playability review required.</text>\n</svg>\n", height - 18));
    Ok(svg)
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn compare(expected: &Snapshot, actual: &Snapshot) -> Comparison {
    let result = Comparison {
        duration_ticks_equal: expected.duration == actual.duration && expected.ppq == actual.ppq,
        tempo_map_equal: expected.tempos == actual.tempos,
        meter_map_equal: expected.meters == actual.meters,
        form_equal: expected.form == actual.form,
        notes_equal: expected.notes == actual.notes,
        lyric_layer_bindings_equal: expected.lyrics == actual.lyrics,
        passed: false,
    };
    Comparison {
        passed: result.duration_ticks_equal
            && result.tempo_map_equal
            && result.meter_map_equal
            && result.form_equal
            && result.notes_equal
            && result.lyric_layer_bindings_equal,
        ..result
    }
}

fn lyric_kind(kind: LyricLayerKind) -> &'static str {
    match kind {
        LyricLayerKind::Canonical => "canonical",
        LyricLayerKind::AsSung => "as-sung",
    }
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.flush()?;
    Ok(())
}

#[derive(Clone)]
struct MidiEvent {
    tick: u64,
    priority: u8,
    bytes: Vec<u8>,
}

fn write_midi(model: &MusicModel) -> Result<Vec<u8>> {
    let mut tracks = Vec::new();
    let mut conductor = Vec::new();
    for tempo in &model.tempo_map {
        let value = tempo.microseconds_per_quarter;
        conductor.push(MidiEvent {
            tick: tempo.tick,
            priority: 10,
            bytes: vec![
                0xff,
                0x51,
                0x03,
                (value >> 16) as u8,
                (value >> 8) as u8,
                value as u8,
            ],
        });
    }
    for meter in &model.meter_map {
        conductor.push(MidiEvent {
            tick: meter.tick,
            priority: 20,
            bytes: vec![
                0xff,
                0x58,
                0x04,
                meter.numerator,
                meter.denominator.trailing_zeros() as u8,
                24,
                8,
            ],
        });
    }
    for section in &model.form {
        meta_text(
            &mut conductor,
            section.range.start,
            30,
            0x06,
            &format!(
                "REEL_FORM:{}",
                serde_json::to_string(&(section.id.as_str(), section.label.as_str()))?
            ),
        )?;
    }
    for lyric in &model.lyric_layers {
        meta_text(
            &mut conductor,
            0,
            40,
            0x01,
            &format!(
                "REEL_LYRIC:{}",
                serde_json::to_string(&(
                    lyric.id.as_str(),
                    lyric_kind(lyric.kind),
                    lyric.language.as_str(),
                    lyric.sha256.as_str()
                ))?
            ),
        )?;
    }
    tracks.push(encode_track(conductor, model.duration_ticks)?);
    for part in &model.parts {
        let mut events = Vec::new();
        meta_text(
            &mut events,
            0,
            0,
            0x03,
            &format!(
                "REEL_PART:{}",
                serde_json::to_string(&(part.id.as_str(), part.name.as_str()))?
            ),
        )?;
        for note in &part.notes {
            let channel = note.voice - 1;
            events.push(MidiEvent {
                tick: note.start_tick,
                priority: 100,
                bytes: vec![0x90 | channel, note.midi_note, note.velocity],
            });
            events.push(MidiEvent {
                tick: note.start_tick + note.duration_ticks,
                priority: 50,
                bytes: vec![0x80 | channel, note.midi_note, 0],
            });
        }
        tracks.push(encode_track(events, model.duration_ticks)?);
    }
    let mut out = b"MThd".to_vec();
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&(tracks.len() as u16).to_be_bytes());
    out.extend_from_slice(&(model.musical_timebase.pulses_per_quarter as u16).to_be_bytes());
    for track in tracks {
        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&(track.len() as u32).to_be_bytes());
        out.extend_from_slice(&track);
    }
    Ok(out)
}

fn meta_text(
    events: &mut Vec<MidiEvent>,
    tick: u64,
    priority: u8,
    kind: u8,
    text: &str,
) -> Result<()> {
    let mut bytes = vec![0xff, kind];
    vlq(text.len() as u64, &mut bytes)?;
    bytes.extend_from_slice(text.as_bytes());
    events.push(MidiEvent {
        tick,
        priority,
        bytes,
    });
    Ok(())
}

fn encode_track(mut events: Vec<MidiEvent>, duration: u64) -> Result<Vec<u8>> {
    events.sort_by(|a, b| (a.tick, a.priority, &a.bytes).cmp(&(b.tick, b.priority, &b.bytes)));
    let mut out = Vec::new();
    let mut cursor = 0;
    for event in events {
        if event.tick < cursor || event.tick > duration {
            bail!("MIDI event is outside canonical track duration");
        }
        vlq(event.tick - cursor, &mut out)?;
        out.extend_from_slice(&event.bytes);
        cursor = event.tick;
    }
    vlq(duration - cursor, &mut out)?;
    out.extend_from_slice(&[0xff, 0x2f, 0x00]);
    Ok(out)
}

fn vlq(mut value: u64, out: &mut Vec<u8>) -> Result<()> {
    if value > 0x0fff_ffff {
        bail!("MIDI delta exceeds the four-byte VLQ limit");
    }
    let mut buffer = [0u8; 4];
    let mut index = 3;
    buffer[index] = (value & 0x7f) as u8;
    while {
        value >>= 7;
        value != 0
    } {
        index -= 1;
        buffer[index] = ((value & 0x7f) as u8) | 0x80;
    }
    out.extend_from_slice(&buffer[index..]);
    Ok(())
}

fn parse_midi(bytes: &[u8]) -> Result<Snapshot> {
    if bytes.len() < 14
        || &bytes[..4] != b"MThd"
        || u32::from_be_bytes(bytes[4..8].try_into()?) != 6
    {
        bail!("invalid MIDI header");
    }
    let track_count = u16::from_be_bytes(bytes[10..12].try_into()?) as usize;
    let ppq = u16::from_be_bytes(bytes[12..14].try_into()?) as u32;
    let mut offset: usize = 14;
    let mut tempos = Vec::new();
    let mut meters = Vec::new();
    let mut form = Vec::new();
    let mut lyrics = Vec::new();
    let mut notes = Vec::new();
    let mut duration = None;
    for track_index in 0..track_count {
        let chunk_header_end = offset
            .checked_add(8)
            .ok_or_else(|| anyhow!("MIDI track header offset overflow"))?;
        let chunk_header = bytes
            .get(offset..chunk_header_end)
            .ok_or_else(|| anyhow!("truncated MIDI track header"))?;
        if &chunk_header[..4] != b"MTrk" {
            bail!("missing MIDI track chunk");
        }
        let length = u32::from_be_bytes(chunk_header[4..8].try_into()?) as usize;
        offset = chunk_header_end;
        let track_end = offset
            .checked_add(length)
            .ok_or_else(|| anyhow!("MIDI track length overflow"))?;
        let track = bytes
            .get(offset..track_end)
            .ok_or_else(|| anyhow!("truncated MIDI track"))?;
        offset = track_end;
        let parsed = parse_midi_track(track, track_index)?;
        if track_index == 0 {
            tempos = parsed.tempos;
            meters = parsed.meters;
            form = parsed.form;
            lyrics = parsed.lyrics;
            duration = Some(parsed.duration);
        } else {
            if duration != Some(parsed.duration) {
                bail!("MIDI track durations disagree");
            }
            notes.extend(parsed.notes);
        }
    }
    if offset != bytes.len() {
        bail!("trailing bytes after MIDI tracks");
    }
    notes.sort();
    Ok(Snapshot {
        ppq,
        duration: duration.ok_or_else(|| anyhow!("MIDI has no conductor track"))?,
        tempos,
        meters,
        form,
        notes,
        lyrics,
    })
}

struct ParsedTrack {
    duration: u64,
    tempos: Vec<(u64, u32)>,
    meters: Vec<(u64, u8, u8)>,
    form: Vec<(u64, String, String)>,
    lyrics: Vec<(String, String, String, String)>,
    notes: Vec<NoteSnapshot>,
}

fn parse_midi_track(bytes: &[u8], track_index: usize) -> Result<ParsedTrack> {
    let mut pos = 0;
    let mut tick = 0u64;
    let mut part_id = None;
    let mut active = BTreeMap::<(u8, u8), (u64, u8)>::new();
    let mut parsed = ParsedTrack {
        duration: 0,
        tempos: vec![],
        meters: vec![],
        form: vec![],
        lyrics: vec![],
        notes: vec![],
    };
    while pos < bytes.len() {
        tick = tick
            .checked_add(read_vlq(bytes, &mut pos)?)
            .ok_or_else(|| anyhow!("MIDI tick overflow"))?;
        let status = *bytes
            .get(pos)
            .ok_or_else(|| anyhow!("truncated MIDI event"))?;
        pos += 1;
        if status == 0xff {
            let kind = *bytes
                .get(pos)
                .ok_or_else(|| anyhow!("truncated MIDI meta event"))?;
            pos += 1;
            let length = read_vlq(bytes, &mut pos)? as usize;
            let data = bytes
                .get(pos..pos + length)
                .ok_or_else(|| anyhow!("truncated MIDI meta payload"))?;
            pos += length;
            match kind {
                0x2f => {
                    if length != 0 {
                        bail!("invalid MIDI end-of-track");
                    }
                    parsed.duration = tick;
                    break;
                }
                0x51 if track_index == 0 && length == 3 => parsed.tempos.push((
                    tick,
                    ((data[0] as u32) << 16) | ((data[1] as u32) << 8) | data[2] as u32,
                )),
                0x58 if track_index == 0 && length == 4 => parsed.meters.push((
                    tick,
                    data[0],
                    1u8.checked_shl(data[1] as u32)
                        .ok_or_else(|| anyhow!("invalid MIDI meter denominator"))?,
                )),
                0x06 if track_index == 0 => {
                    let text = std::str::from_utf8(data)?;
                    let payload = text
                        .strip_prefix("REEL_FORM:")
                        .ok_or_else(|| anyhow!("invalid REEL MIDI form marker"))?;
                    let (id, label): (String, String) = serde_json::from_str(payload)?;
                    parsed.form.push((tick, id, label));
                }
                0x01 if track_index == 0 => {
                    let text = std::str::from_utf8(data)?;
                    let payload = text
                        .strip_prefix("REEL_LYRIC:")
                        .ok_or_else(|| anyhow!("invalid REEL MIDI lyric binding"))?;
                    parsed.lyrics.push(serde_json::from_str(payload)?);
                }
                0x03 if track_index > 0 => {
                    let text = std::str::from_utf8(data)?;
                    let payload = text
                        .strip_prefix("REEL_PART:")
                        .ok_or_else(|| anyhow!("invalid REEL MIDI part marker"))?;
                    let (id, _name): (String, String) = serde_json::from_str(payload)?;
                    part_id = Some(id);
                }
                _ => {}
            }
        } else if status & 0xf0 == 0x90 || status & 0xf0 == 0x80 {
            let pitch = *bytes
                .get(pos)
                .ok_or_else(|| anyhow!("truncated MIDI note"))?;
            let velocity = *bytes
                .get(pos + 1)
                .ok_or_else(|| anyhow!("truncated MIDI note"))?;
            pos += 2;
            let key = (status & 0x0f, pitch);
            if status & 0xf0 == 0x90 && velocity != 0 {
                if active.insert(key, (tick, velocity)).is_some() {
                    bail!("overlapping identical MIDI note/channel pair");
                }
            } else {
                let (start, start_velocity) = active
                    .remove(&key)
                    .ok_or_else(|| anyhow!("MIDI note-off has no note-on"))?;
                parsed.notes.push(NoteSnapshot {
                    part_id: part_id
                        .clone()
                        .ok_or_else(|| anyhow!("MIDI note track lacks part marker"))?,
                    voice: key.0 + 1,
                    start,
                    duration: tick - start,
                    pitch,
                    velocity: start_velocity,
                });
            }
        } else {
            bail!("unsupported MIDI event status {status:#x}");
        }
    }
    if parsed.duration == 0 || !active.is_empty() {
        bail!("incomplete MIDI track");
    }
    Ok(parsed)
}

fn read_vlq(bytes: &[u8], pos: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    for _ in 0..4 {
        let byte = *bytes
            .get(*pos)
            .ok_or_else(|| anyhow!("truncated MIDI VLQ"))?;
        *pos += 1;
        value = (value << 7) | (byte & 0x7f) as u64;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    bail!("MIDI VLQ exceeds four bytes")
}

fn bpm_string(microseconds_per_quarter: u32) -> String {
    let denominator = microseconds_per_quarter as u64;
    let scaled = (60_000_000u64 * 1_000_000 + denominator / 2) / denominator;
    format!("{}.{:06}", scaled / 1_000_000, scaled % 1_000_000)
}

fn write_musicxml(model: &MusicModel) -> Result<String> {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<score-partwise version=\"4.0\" xmlns:reel=\"https://github.com/giodl73-repo/REEL/ns/music-score/v0.1\" reel:duration-ticks=\"");
    xml.push_str(&model.duration_ticks.to_string());
    xml.push_str("\">\n  <identification><encoding><software>REEL MusicXML adapter 0.1.0</software></encoding></identification>\n  <miscellaneous>\n");
    for lyric in &model.lyric_layers {
        let binding = serde_json::to_string(&(
            lyric.id.as_str(),
            lyric_kind(lyric.kind),
            lyric.language.as_str(),
            lyric.sha256.as_str(),
        ))?;
        xml.push_str(&format!(
            "    <miscellaneous-field name=\"reel:lyric-layer\">{}</miscellaneous-field>\n",
            escape(&binding)
        ));
    }
    xml.push_str("  </miscellaneous>\n  <part-list>\n");
    for (index, part) in model.parts.iter().enumerate() {
        xml.push_str(&format!("    <score-part id=\"P{}\" reel:part-id=\"{}\"><part-name>{}</part-name></score-part>\n", index + 1, escape(&part.id), escape(&part.name)));
    }
    xml.push_str("  </part-list>\n");
    for (index, part) in model.parts.iter().enumerate() {
        xml.push_str(&format!("  <part id=\"P{}\" reel:part-id=\"{}\">\n    <measure number=\"1\" implicit=\"yes\" non-controlling=\"yes\">\n", index + 1, escape(&part.id)));
        xml.push_str(&format!("      <attributes reel:tick=\"0\"><divisions>{}</divisions><time><beats>{}</beats><beat-type>{}</beat-type></time></attributes>\n", model.musical_timebase.pulses_per_quarter, model.meter_map[0].numerator, model.meter_map[0].denominator));
        if index == 0 {
            for tempo in &model.tempo_map {
                xml.push_str(&format!("      <direction reel:kind=\"tempo\" reel:tick=\"{}\" reel:microseconds-per-quarter=\"{}\"><offset>{}</offset><sound tempo=\"{}\"/></direction>\n", tempo.tick, tempo.microseconds_per_quarter, tempo.tick, bpm_string(tempo.microseconds_per_quarter)));
            }
            for meter in &model.meter_map {
                if meter.tick != 0 {
                    xml.push_str(&format!("      <attributes reel:tick=\"{}\"><time><beats>{}</beats><beat-type>{}</beat-type></time></attributes>\n", meter.tick, meter.numerator, meter.denominator));
                }
            }
            for section in &model.form {
                xml.push_str(&format!("      <direction reel:kind=\"form\" reel:tick=\"{}\" reel:id=\"{}\"><direction-type><rehearsal>{}</rehearsal></direction-type><offset>{}</offset></direction>\n", section.range.start, escape(&section.id), escape(&section.label), section.range.start));
            }
        }
        let voices = part
            .notes
            .iter()
            .map(|note| note.voice)
            .collect::<BTreeSet<_>>();
        for (voice_index, voice) in voices.iter().enumerate() {
            if voice_index > 0 {
                xml.push_str(&format!(
                    "      <backup><duration>{}</duration></backup>\n",
                    model.duration_ticks
                ));
            }
            let mut cursor = 0;
            for note in part.notes.iter().filter(|note| note.voice == *voice) {
                if note.start_tick > cursor {
                    xml.push_str(&format!(
                        "      <forward><duration>{}</duration></forward>\n",
                        note.start_tick - cursor
                    ));
                }
                let (step, alter, octave) = midi_pitch(note.midi_note);
                xml.push_str(&format!("      <note reel:start-tick=\"{}\" reel:velocity=\"{}\"><pitch><step>{}</step>{}<octave>{}</octave></pitch><duration>{}</duration><voice>{}</voice></note>\n", note.start_tick, note.velocity, step, if alter == 0 { String::new() } else { format!("<alter>{alter}</alter>") }, octave, note.duration_ticks, note.voice));
                cursor = note.start_tick + note.duration_ticks;
            }
            if cursor < model.duration_ticks {
                xml.push_str(&format!(
                    "      <forward><duration>{}</duration></forward>\n",
                    model.duration_ticks - cursor
                ));
            }
        }
        xml.push_str("    </measure>\n  </part>\n");
    }
    xml.push_str("</score-partwise>\n");
    Ok(xml)
}

fn parse_musicxml(xml: &str) -> Result<Snapshot> {
    if !xml.starts_with("<?xml") || !xml.contains("<score-partwise") {
        bail!("invalid REEL MusicXML document");
    }
    let root = tag_open(xml, "score-partwise", 0)?.1;
    let ppq = text_between(xml, "divisions", 0)?.0.parse()?;
    let duration = attr(root, "reel:duration-ticks")?.parse()?;
    let mut tempos = Vec::new();
    let mut meters = Vec::new();
    let mut form = Vec::new();
    let mut notes = Vec::new();
    let mut lyrics = Vec::new();
    let mut cursor = 0;
    while let Some(start) = xml[cursor..].find("<miscellaneous-field") {
        let at = cursor + start;
        let (body, open, end) = element(xml, "miscellaneous-field", at)?;
        if attr(open, "name")? == "reel:lyric-layer" {
            lyrics.push(serde_json::from_str(&unescape(body))?);
        }
        cursor = end;
    }
    cursor = 0;
    while let Some(start) = xml[cursor..].find("<attributes") {
        let at = cursor + start;
        let (body, open, end) = element(xml, "attributes", at)?;
        if body.contains("<time>") {
            let tick = attr(open, "reel:tick")?.parse()?;
            let numerator = text_between(body, "beats", 0)?.0.parse()?;
            let denominator = text_between(body, "beat-type", 0)?.0.parse()?;
            if !meters.contains(&(tick, numerator, denominator)) {
                meters.push((tick, numerator, denominator));
            }
        }
        cursor = end;
    }
    cursor = 0;
    while let Some(start) = xml[cursor..].find("<direction ") {
        let at = cursor + start;
        let (body, open, end) = element(xml, "direction", at)?;
        match attr(open, "reel:kind")? {
            "tempo" => tempos.push((
                attr(open, "reel:tick")?.parse()?,
                attr(open, "reel:microseconds-per-quarter")?.parse()?,
            )),
            "form" => form.push((
                attr(open, "reel:tick")?.parse()?,
                unescape(attr(open, "reel:id")?),
                unescape(&text_between(body, "rehearsal", 0)?.0),
            )),
            other => bail!("unsupported REEL MusicXML direction {other}"),
        }
        cursor = end;
    }
    cursor = 0;
    while let Some(start) = xml[cursor..].find("<part ") {
        let at = cursor + start;
        let (body, open, end) = element(xml, "part", at)?;
        let part_id = unescape(attr(open, "reel:part-id")?);
        let mut note_cursor = 0;
        while let Some(note_start) = body[note_cursor..].find("<note ") {
            let note_at = note_cursor + note_start;
            let (note_body, note_open, note_end) = element(body, "note", note_at)?;
            let step = text_between(note_body, "step", 0)?.0;
            let alter: i8 = if note_body.contains("<alter>") {
                text_between(note_body, "alter", 0)?.0.parse()?
            } else {
                0
            };
            let octave: i8 = text_between(note_body, "octave", 0)?.0.parse()?;
            notes.push(NoteSnapshot {
                part_id: part_id.clone(),
                voice: text_between(note_body, "voice", 0)?.0.parse()?,
                start: attr(note_open, "reel:start-tick")?.parse()?,
                duration: text_between(note_body, "duration", 0)?.0.parse()?,
                pitch: xml_pitch(&step, alter, octave)?,
                velocity: attr(note_open, "reel:velocity")?.parse()?,
            });
            note_cursor = note_end;
        }
        cursor = end;
    }
    tempos.sort();
    meters.sort();
    form.sort_by_key(|entry| entry.0);
    notes.sort();
    Ok(Snapshot {
        ppq,
        duration,
        tempos,
        meters,
        form,
        notes,
        lyrics,
    })
}

fn tag_open<'a>(xml: &'a str, tag: &str, from: usize) -> Result<(usize, &'a str)> {
    let start = xml[from..]
        .find(&format!("<{tag}"))
        .map(|value| from + value)
        .ok_or_else(|| anyhow!("missing <{tag}>"))?;
    let end = xml[start..]
        .find('>')
        .map(|value| start + value)
        .ok_or_else(|| anyhow!("unterminated <{tag}>"))?;
    Ok((start, &xml[start..=end]))
}

fn element<'a>(xml: &'a str, tag: &str, from: usize) -> Result<(&'a str, &'a str, usize)> {
    let (start, open) = tag_open(xml, tag, from)?;
    let body_start = start + open.len();
    let close = format!("</{tag}>");
    let body_end = xml[body_start..]
        .find(&close)
        .map(|value| body_start + value)
        .ok_or_else(|| anyhow!("missing {close}"))?;
    Ok((&xml[body_start..body_end], open, body_end + close.len()))
}

fn text_between(xml: &str, tag: &str, from: usize) -> Result<(String, usize)> {
    let (body, _, end) = element(xml, tag, from)?;
    Ok((unescape(body), end))
}

fn attr<'a>(open: &'a str, name: &str) -> Result<&'a str> {
    let needle = format!("{name}=\"");
    let start = open
        .find(&needle)
        .map(|value| value + needle.len())
        .ok_or_else(|| anyhow!("missing XML attribute {name}"))?;
    let end = open[start..]
        .find('"')
        .map(|value| start + value)
        .ok_or_else(|| anyhow!("unterminated XML attribute {name}"))?;
    Ok(&open[start..end])
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

fn midi_pitch(note: u8) -> (&'static str, i8, i8) {
    const PITCHES: [(&str, i8); 12] = [
        ("C", 0),
        ("C", 1),
        ("D", 0),
        ("D", 1),
        ("E", 0),
        ("F", 0),
        ("F", 1),
        ("G", 0),
        ("G", 1),
        ("A", 0),
        ("A", 1),
        ("B", 0),
    ];
    let (step, alter) = PITCHES[(note % 12) as usize];
    (step, alter, (note / 12) as i8 - 1)
}

fn xml_pitch(step: &str, alter: i8, octave: i8) -> Result<u8> {
    let base = match step {
        "C" => 0,
        "D" => 2,
        "E" => 4,
        "F" => 5,
        "G" => 7,
        "A" => 9,
        "B" => 11,
        _ => bail!("invalid MusicXML pitch step"),
    };
    let value = (octave as i16 + 1) * 12 + base + alter as i16;
    if !(0..=127).contains(&value) {
        bail!("MusicXML pitch outside MIDI range");
    }
    Ok(value as u8)
}

fn selected_part(model: &MusicModel) -> &reel_music::model::Part {
    model
        .parts
        .iter()
        .find(|part| matches!(part.role, PartRole::Melody | PartRole::Vocal))
        .unwrap_or(&model.parts[0])
}

fn tick_to_sample(tick: u64, model: &MusicModel, sample_rate: u32) -> Result<u64> {
    let mut weighted_ticks = 0u128;
    for (index, tempo) in model.tempo_map.iter().enumerate() {
        if tick <= tempo.tick {
            break;
        }
        let end = model
            .tempo_map
            .get(index + 1)
            .map(|next| next.tick)
            .unwrap_or(tick)
            .min(tick);
        let span = end - tempo.tick;
        weighted_ticks = weighted_ticks
            .checked_add(span as u128 * tempo.microseconds_per_quarter as u128)
            .ok_or_else(|| anyhow!("guide duration overflow"))?;
    }
    let numerator = weighted_ticks * sample_rate as u128;
    let denominator = model.musical_timebase.pulses_per_quarter as u128 * 1_000_000u128;
    Ok(((numerator + denominator / 2) / denominator).try_into()?)
}

fn expected_guide_samples(model: &MusicModel, plan: &ScoreExportPlan) -> Result<u64> {
    tick_to_sample(
        model.duration_ticks,
        model,
        plan.rehearsal_guide.sample_rate_hz,
    )
}

fn write_guide_wav(model: &MusicModel, plan: &ScoreExportPlan) -> Result<Vec<u8>> {
    let rate = plan.rehearsal_guide.sample_rate_hz;
    let samples = expected_guide_samples(model, plan)? as usize;
    let mut pcm = vec![0i32; samples];
    for note in &selected_part(model).notes {
        let start = tick_to_sample(note.start_tick, model, rate)? as usize;
        let end = tick_to_sample(note.start_tick + note.duration_ticks, model, rate)? as usize;
        const OCTAVE_ZERO_MILLIHERTZ: [u64; 12] = [
            8_176, 8_662, 9_177, 9_723, 10_301, 10_913, 11_562, 12_250, 12_978, 13_750, 14_568,
            15_434,
        ];
        let frequency_millihertz =
            OCTAVE_ZERO_MILLIHERTZ[(note.midi_note % 12) as usize] << (note.midi_note / 12);
        let increment = (((frequency_millihertz as u128) << 32) / (rate as u128 * 1000)) as u64;
        let amplitude = 1500 + note.velocity as i32 * 90;
        let mut phase = 0u64;
        for sample in pcm
            .iter_mut()
            .take(end.min(samples))
            .skip(start.min(samples))
        {
            let value = if phase & 0x8000_0000 != 0 {
                amplitude
            } else {
                -amplitude
            };
            *sample = (*sample + value).clamp(i16::MIN as i32, i16::MAX as i32);
            phase = phase.wrapping_add(increment);
        }
    }
    let data_bytes = (pcm.len() * 2) as u32;
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for sample in pcm {
        wav.extend_from_slice(&(sample as i16).to_le_bytes());
    }
    Ok(wav)
}

fn inspect_wav(bytes: &[u8], expected_samples: u64) -> Result<GuideComparison> {
    let valid = bytes.len() >= 44
        && &bytes[0..4] == b"RIFF"
        && &bytes[8..12] == b"WAVE"
        && &bytes[12..16] == b"fmt "
        && &bytes[36..40] == b"data";
    if !valid {
        bail!("invalid rehearsal guide RIFF/WAVE header");
    }
    let channels = u16::from_le_bytes(bytes[22..24].try_into()?);
    let rate = u32::from_le_bytes(bytes[24..28].try_into()?);
    let bits = u16::from_le_bytes(bytes[34..36].try_into()?);
    let data = u32::from_le_bytes(bytes[40..44].try_into()?) as usize;
    let audio_format = u16::from_le_bytes(bytes[20..22].try_into()?);
    if audio_format != 1
        || channels == 0
        || bits != 16
        || data.checked_add(44) != Some(bytes.len())
        || data % (channels as usize * 2) != 0
    {
        bail!("unsupported or inconsistent rehearsal guide WAV");
    }
    let samples = (data / (channels as usize * 2)) as u64;
    Ok(GuideComparison {
        riff_wave_header_valid: true,
        sample_rate_hz: rate,
        channels,
        samples_per_channel: samples,
        expected_samples_per_channel: expected_samples,
        passed: rate == 48_000 && channels == 1 && samples == expected_samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reel_music::hash::sha256_bytes;
    use std::path::PathBuf;

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("manifests/fixtures/music-model-corrected/model.yaml")
    }

    #[test]
    fn midi_and_musicxml_round_trip_the_corrected_model() {
        let model = reel_music::model::load(&fixture()).unwrap();
        assert_eq!(
            snapshot(&model),
            parse_midi(&write_midi(&model).unwrap()).unwrap()
        );
        assert_eq!(
            snapshot(&model),
            parse_musicxml(&write_musicxml(&model).unwrap()).unwrap()
        );
    }

    #[test]
    fn reimport_detects_pitch_tampering() {
        let model = reel_music::model::load(&fixture()).unwrap();
        let mut midi = write_midi(&model).unwrap();
        let position = midi
            .windows(3)
            .position(|window| window == [0x90, 60, 80])
            .unwrap();
        midi[position + 1] = 61;
        assert!(parse_midi(&midi).is_err());
        let xml = write_musicxml(&model)
            .unwrap()
            .replacen("<step>C</step>", "<step>D</step>", 1);
        assert!(!compare(&snapshot(&model), &parse_musicxml(&xml).unwrap()).notes_equal);
    }

    #[test]
    fn guide_is_deterministic_and_has_expected_duration() {
        let model = reel_music::model::load(&fixture()).unwrap();
        let plan = export::build(&fixture()).unwrap();
        let first = write_guide_wav(&model, &plan).unwrap();
        let second = write_guide_wav(&model, &plan).unwrap();
        assert_eq!(sha256_bytes(&first), sha256_bytes(&second));
        assert!(
            inspect_wav(&first, expected_guide_samples(&model, &plan).unwrap())
                .unwrap()
                .passed
        );
    }

    #[test]
    fn malformed_export_bytes_fail_without_panicking() {
        let model = reel_music::model::load(&fixture()).unwrap();
        let midi = write_midi(&model).unwrap();
        assert!(parse_midi(&midi[..15]).is_err());

        let plan = export::build(&fixture()).unwrap();
        let mut wav = write_guide_wav(&model, &plan).unwrap();
        wav[22..24].copy_from_slice(&0u16.to_le_bytes());
        assert!(inspect_wav(&wav, expected_guide_samples(&model, &plan).unwrap()).is_err());
    }
}
