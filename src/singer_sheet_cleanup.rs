use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const REQUEST_SCHEMA: &str = "reel.singer-sheet-cleanup.request.v1";
const DOCUMENT_SCHEMA: &str = "reel.singer-sheet-cleanup.document.v1";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupRequest {
    pub schema: String,
    pub draft_musicxml_sha256: String,
    pub isolated_vocal_sha256: String,
    pub key_fifths: i8,
    pub tempo_bpm: u32,
    pub divisions_per_quarter: u32,
    pub canonical: Vec<CanonicalSyllable>,
    pub occurrences: Vec<PerformedOccurrence>,
    pub vocal_observations: Vec<VocalObservation>,
    pub draft_notes: Vec<DraftNote>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalSyllable {
    pub index: u32,
    pub word_id: String,
    pub printed: String,
    pub word_position: String,
    pub vowel: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformedOccurrence {
    pub occurrence_id: String,
    pub canonical_index: u32,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VocalObservation {
    pub occurrence_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub median_midi_micros: i64,
    pub energy_micros: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DraftNote {
    pub note_id: String,
    pub occurrence_id: Option<String>,
    pub midi: Option<i16>,
    pub duration_divisions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CleanupDocument {
    pub schema: String,
    pub draft_musicxml_sha256: String,
    pub isolated_vocal_sha256: String,
    pub exact_performance_duration_ms: u64,
    pub notation: Vec<NotationEvent>,
    pub ledger: Vec<LedgerEntry>,
    pub metrics: CleanupMetrics,
    pub review_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotationEvent {
    pub event_id: String,
    pub occurrence_id: Option<String>,
    pub canonical_index: Option<u32>,
    pub performance_start_ms: u64,
    pub performance_end_ms: u64,
    pub pitch_midi: Option<i16>,
    pub pitch_spelling: Option<String>,
    pub vocal_energy_micros: Option<u32>,
    pub duration_divisions: u32,
    pub duration_name: String,
    pub lyric: Option<String>,
    pub syllabic: Option<String>,
    pub held_vowel: Option<String>,
    pub melisma: bool,
    pub pause_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LedgerEntry {
    pub occurrence_id: String,
    pub draft_note_ids: Vec<String>,
    pub draft_duration_divisions: u32,
    pub recommended_duration_divisions: u32,
    pub changes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CleanupMetrics {
    pub occurrence_count: usize,
    pub exact_clock_coverage_micros: u32,
    pub tiny_durations_before: usize,
    pub tiny_durations_after: usize,
    pub explicit_melismas: usize,
    pub held_vowels: usize,
    pub written_rests: usize,
    pub breath_marks: usize,
    pub draft_pitch_changes: usize,
}

pub fn cleanup_file(input: &Path, output: &Path) -> Result<CleanupDocument> {
    if output.exists() {
        bail!("refusing to overwrite existing singer-sheet cleanup output");
    }
    let request: CleanupRequest = serde_json::from_slice(
        &fs::read(input).with_context(|| format!("read {}", input.display()))?,
    )
    .context("parse singer-sheet cleanup request")?;
    let document = cleanup(&request)?;
    let bytes = serde_json::to_vec_pretty(&document)?;
    fs::write(output, [bytes.as_slice(), b"\n"].concat())?;
    Ok(document)
}

pub fn cleanup(request: &CleanupRequest) -> Result<CleanupDocument> {
    validate(request)?;
    let canonical: BTreeMap<_, _> = request
        .canonical
        .iter()
        .map(|item| (item.index, item))
        .collect();
    let observations: BTreeMap<_, _> = request
        .vocal_observations
        .iter()
        .map(|item| (item.occurrence_id.as_str(), item))
        .collect();
    let draft_by_occurrence: BTreeMap<_, Vec<_>> = request
        .draft_notes
        .iter()
        .filter_map(|note| note.occurrence_id.as_deref().map(|id| (id, note)))
        .fold(BTreeMap::new(), |mut map, (id, note)| {
            map.entry(id).or_default().push(note);
            map
        });
    let quarter_ms = 60_000u64 / request.tempo_bpm as u64;
    let mut notation = Vec::new();
    let mut ledger = Vec::new();
    let mut prior_end = 0;
    let mut pitch_changes = 0;
    for occurrence in &request.occurrences {
        if occurrence.start_ms > prior_end {
            let gap = occurrence.start_ms - prior_end;
            let written = gap >= quarter_ms;
            notation.push(NotationEvent {
                event_id: format!("pause:{}", occurrence.occurrence_id),
                occurrence_id: None,
                canonical_index: None,
                performance_start_ms: prior_end,
                performance_end_ms: occurrence.start_ms,
                pitch_midi: None,
                pitch_spelling: None,
                vocal_energy_micros: None,
                duration_divisions: quantize(gap, quarter_ms, request.divisions_per_quarter),
                duration_name: if written { "rest" } else { "breath" }.into(),
                lyric: None,
                syllabic: None,
                held_vowel: None,
                melisma: false,
                pause_kind: Some(
                    if written {
                        "written-rest"
                    } else {
                        "breath-mark"
                    }
                    .into(),
                ),
            });
        }
        let syllable = canonical[&occurrence.canonical_index];
        let observation = observations[occurrence.occurrence_id.as_str()];
        let midi = ((observation.median_midi_micros + 500_000) / 1_000_000) as i16;
        let draft = draft_by_occurrence
            .get(occurrence.occurrence_id.as_str())
            .cloned()
            .unwrap_or_default();
        let draft_duration = draft.iter().map(|note| note.duration_divisions).sum();
        let duration = quantize(
            occurrence.end_ms - occurrence.start_ms,
            quarter_ms,
            request.divisions_per_quarter,
        );
        let melisma = draft.len() > 1 || occurrence.end_ms - occurrence.start_ms >= quarter_ms * 2;
        if draft.iter().filter_map(|note| note.midi).next() != Some(midi) {
            pitch_changes += 1;
        }
        let mut changes = Vec::new();
        if draft_duration != duration {
            changes.push("duration-normalized".into());
        }
        if draft.iter().filter_map(|note| note.midi).next() != Some(midi) {
            changes.push("pitch-from-isolated-vocal".into());
        }
        if melisma {
            changes.push("explicit-melisma".into());
        }
        notation.push(NotationEvent {
            event_id: format!("lyric:{}", occurrence.occurrence_id),
            occurrence_id: Some(occurrence.occurrence_id.clone()),
            canonical_index: Some(occurrence.canonical_index),
            performance_start_ms: occurrence.start_ms,
            performance_end_ms: occurrence.end_ms,
            pitch_midi: Some(midi),
            pitch_spelling: Some(spell_pitch(midi, request.key_fifths)),
            vocal_energy_micros: Some(observation.energy_micros),
            duration_divisions: duration,
            duration_name: duration_name(duration, request.divisions_per_quarter),
            lyric: Some(syllable.printed.clone()),
            syllabic: Some(syllable.word_position.clone()),
            held_vowel: (occurrence.end_ms - occurrence.start_ms >= quarter_ms * 2)
                .then(|| syllable.vowel.clone()),
            melisma,
            pause_kind: None,
        });
        ledger.push(LedgerEntry {
            occurrence_id: occurrence.occurrence_id.clone(),
            draft_note_ids: draft.iter().map(|note| note.note_id.clone()).collect(),
            draft_duration_divisions: draft_duration,
            recommended_duration_divisions: duration,
            changes,
        });
        prior_end = occurrence.end_ms;
    }
    let minimum = request.divisions_per_quarter / 2;
    let exact_performance_duration_ms = request.occurrences.last().map_or(0, |item| item.end_ms);
    Ok(CleanupDocument {
        schema: DOCUMENT_SCHEMA.into(),
        draft_musicxml_sha256: request.draft_musicxml_sha256.clone(),
        isolated_vocal_sha256: request.isolated_vocal_sha256.clone(),
        exact_performance_duration_ms,
        metrics: CleanupMetrics {
            occurrence_count: request.occurrences.len(),
            exact_clock_coverage_micros: 1_000_000,
            tiny_durations_before: request
                .draft_notes
                .iter()
                .filter(|note| note.duration_divisions < minimum)
                .count(),
            tiny_durations_after: notation
                .iter()
                .filter(|event| event.occurrence_id.is_some() && event.duration_divisions < minimum)
                .count(),
            explicit_melismas: notation.iter().filter(|event| event.melisma).count(),
            held_vowels: notation
                .iter()
                .filter(|event| event.held_vowel.is_some())
                .count(),
            written_rests: notation
                .iter()
                .filter(|event| event.pause_kind.as_deref() == Some("written-rest"))
                .count(),
            breath_marks: notation
                .iter()
                .filter(|event| event.pause_kind.as_deref() == Some("breath-mark"))
                .count(),
            draft_pitch_changes: pitch_changes,
        },
        notation,
        ledger,
        review_state: "machine-recommendation-musician-review-required".into(),
    })
}

fn quantize(ms: u64, quarter_ms: u64, divisions: u32) -> u32 {
    let raw = ms.saturating_mul(divisions as u64) / quarter_ms.max(1);
    let allowed = [
        divisions / 2,
        divisions,
        divisions * 3 / 2,
        divisions * 2,
        divisions * 3,
        divisions * 4,
    ];
    *allowed
        .iter()
        .min_by_key(|value| u64::from(**value).abs_diff(raw))
        .unwrap()
}

fn duration_name(value: u32, divisions: u32) -> String {
    match value {
        value if value == divisions / 2 => "eighth",
        value if value == divisions => "quarter",
        value if value == divisions * 3 / 2 => "dotted-quarter",
        value if value == divisions * 2 => "half",
        value if value == divisions * 3 => "dotted-half",
        _ => "whole",
    }
    .into()
}

fn spell_pitch(midi: i16, key_fifths: i8) -> String {
    const SHARP: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    const FLAT: [&str; 12] = [
        "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
    ];
    let names = if key_fifths < 0 { FLAT } else { SHARP };
    format!("{}{}", names[midi.rem_euclid(12) as usize], midi / 12 - 1)
}

fn validate(request: &CleanupRequest) -> Result<()> {
    if request.schema != REQUEST_SCHEMA {
        bail!("unsupported schema: {}", request.schema);
    }
    for hash in [
        &request.draft_musicxml_sha256,
        &request.isolated_vocal_sha256,
    ] {
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("input hashes must be SHA-256");
        }
    }
    if request.tempo_bpm == 0 || request.divisions_per_quarter < 2 {
        bail!("tempo and divisions must be positive");
    }
    let canonical: BTreeMap<_, _> = request
        .canonical
        .iter()
        .map(|item| (item.index, item))
        .collect();
    let observations: BTreeMap<_, _> = request
        .vocal_observations
        .iter()
        .map(|item| (item.occurrence_id.as_str(), item))
        .collect();
    let mut prior_end = 0;
    for occurrence in &request.occurrences {
        if occurrence.end_ms <= occurrence.start_ms || occurrence.start_ms < prior_end {
            bail!("performed occurrence clock is not chronological");
        }
        if !canonical.contains_key(&occurrence.canonical_index)
            || !observations.contains_key(occurrence.occurrence_id.as_str())
        {
            bail!("occurrence lacks canonical or vocal evidence");
        }
        let observation = observations[occurrence.occurrence_id.as_str()];
        if observation.start_ms != occurrence.start_ms
            || observation.end_ms != occurrence.end_ms
            || observation.energy_micros > 1_000_000
        {
            bail!("vocal observation disagrees with the exact performed clock");
        }
        prior_end = occurrence.end_ms;
    }
    Ok(())
}
