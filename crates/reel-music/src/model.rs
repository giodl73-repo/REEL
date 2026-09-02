use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    AuthorityRef, DecisionRef, analysis,
    hash::{canonical_sha256, sha256_path},
    nonempty, source, status_requires_decision,
    time::MusicalTimebase,
    unique_nonempty, validate_authority, validate_sha256,
};

pub const SCHEMA: &str = "reel.music-model.v0.1";
const REQUIRED_ROLES: &[&str] = &[
    "music-reconstruction-engineer",
    "score-arrangement-director",
    "sound-designer",
    "editor",
    "rights-provenance-steward",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MusicModel {
    pub schema: String,
    pub model_id: String,
    pub source: SourceBinding,
    pub analyses: Vec<AnalysisBinding>,
    pub authority: AuthorityRef,
    pub musical_timebase: MusicalTimebase,
    pub duration_ticks: u64,
    pub tempo_map: Vec<TempoEvent>,
    pub meter_map: Vec<MeterEvent>,
    pub form: Vec<FormSection>,
    pub parts: Vec<Part>,
    #[serde(default)]
    pub harmony: Vec<HarmonyEvent>,
    #[serde(default)]
    pub rhythm_cells: Vec<RhythmCell>,
    #[serde(default)]
    pub hooks: Vec<Hook>,
    #[serde(default)]
    pub lyric_layers: Vec<LyricLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_sheet: Option<LeadSheet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub piano_vocal_score: Option<PianoVocalScore>,
    #[serde(default)]
    pub expressive_timing: Vec<ExpressiveTiming>,
    pub unknowns: Vec<String>,
    pub review: Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBinding {
    pub manifest: PathBuf,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub decoded_pcm_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisBinding {
    pub manifest: PathBuf,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub analysis_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TickRange {
    pub start: u64,
    pub end: u64,
}

impl TickRange {
    fn validate(&self, duration: u64, field: &str) -> Result<()> {
        if self.start >= self.end || self.end > duration {
            bail!("{field} must be a non-empty half-open range within model duration");
        }
        Ok(())
    }

    fn len(&self) -> u64 {
        self.end - self.start
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TempoEvent {
    pub tick: u64,
    pub microseconds_per_quarter: u32,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MeterEvent {
    pub tick: u64,
    pub numerator: u8,
    pub denominator: u8,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FormSection {
    pub id: String,
    pub label: String,
    pub range: TickRange,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub id: String,
    pub role: PartRole,
    pub name: String,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PartRole {
    Melody,
    Vocal,
    Bass,
    Harmony,
    Rhythm,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Note {
    pub id: String,
    pub voice: u8,
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub midi_note: u8,
    pub velocity: u8,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarmonyEvent {
    pub id: String,
    pub range: TickRange,
    pub symbol: String,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RhythmCell {
    pub id: String,
    pub range: TickRange,
    pub onset_offsets_ticks: Vec<u64>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Hook {
    pub id: String,
    pub label: String,
    pub range: TickRange,
    pub element_refs: Vec<String>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LyricLayer {
    pub id: String,
    pub kind: LyricLayerKind,
    pub language: String,
    pub path: PathBuf,
    pub sha256: String,
    pub authority: AuthorityRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LyricLayerKind {
    Canonical,
    AsSung,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LeadSheet {
    pub title: String,
    pub melody_part_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyric_layer_id: Option<String>,
    #[serde(default)]
    pub underlay: Vec<LyricUnderlay>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PianoVocalScore {
    pub title: String,
    pub vocal_part_id: String,
    pub piano_right_hand_part_id: String,
    pub piano_left_hand_part_id: String,
    #[serde(default)]
    pub pickup_ticks: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LyricUnderlay {
    pub id: String,
    pub note_ids: Vec<String>,
    pub text_start_byte: u64,
    pub text_end_byte: u64,
    pub syllabic: Syllabic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Syllabic {
    Single,
    Begin,
    Middle,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpressiveTiming {
    pub note_id: String,
    pub onset_offset_ticks: i32,
    pub duration_offset_ticks: i32,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProvenanceState {
    Observed,
    Inferred,
    HumanCorrected,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub state: ProvenanceState,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    pub rationale: String,
    pub correction_ref: Option<DecisionRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRef {
    pub analysis_id: String,
    pub observation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub status: String,
    pub required_roles: Vec<String>,
    #[serde(default)]
    pub decision_refs: Vec<DecisionRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub model_id: String,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub source_contract_sha256: String,
    pub analysis_contract_sha256s: Vec<String>,
    pub duration_ticks: u64,
    pub tempo_events: usize,
    pub meter_events: usize,
    pub form_sections: usize,
    pub parts: usize,
    pub notes: usize,
    pub harmony_events: usize,
    pub rhythm_cells: usize,
    pub hooks: usize,
    pub lyric_layers: usize,
    pub human_corrected_events: usize,
    pub shareable: bool,
    pub verified: bool,
}

pub fn load(path: &Path) -> Result<MusicModel> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_slice(&bytes)
        .with_context(|| format!("music model is not valid YAML: {}", path.display()))
}

pub fn validate(path: &Path) -> Result<ValidationReport> {
    let model = load(path)?;
    validate_loaded(path, &model)
}

fn validate_loaded(path: &Path, model: &MusicModel) -> Result<ValidationReport> {
    if model.schema != SCHEMA {
        bail!("music model schema must be {SCHEMA}");
    }
    nonempty("model_id", &model.model_id)?;
    validate_authority(&model.authority)?;
    if model.duration_ticks == 0 {
        bail!("duration_ticks must be positive");
    }
    model.musical_timebase.validate()?;
    let source_path = validate_source_binding(path, &model.source)?;
    let source_report = source::validate(&source_path)?;
    let source_manifest = source::load(&source_path)?;
    if model.musical_timebase != source_manifest.musical_timebase {
        bail!("model musical_timebase must match the immutable source timebase");
    }
    let evidence = validate_analysis_bindings(path, model, &source_report.contract_sha256)?;
    let mut human_corrected = 0;

    validate_point_maps(model, &evidence, &mut human_corrected)?;
    let mut element_ids = BTreeSet::new();
    validate_form(model, &evidence, &mut element_ids, &mut human_corrected)?;
    let note_ids = validate_parts(model, &evidence, &mut element_ids, &mut human_corrected)?;
    validate_harmony(model, &evidence, &mut element_ids, &mut human_corrected)?;
    validate_rhythm(model, &evidence, &mut element_ids, &mut human_corrected)?;
    validate_hooks(model, &evidence, &element_ids, &mut human_corrected)?;
    validate_lyrics(path, model)?;
    validate_lead_sheet(path, model)?;
    validate_piano_vocal_score(model)?;
    validate_expressive_timing(model, &evidence, &note_ids, &mut human_corrected)?;
    unique_nonempty("unknowns", &model.unknowns)?;
    validate_review(&model.review)?;

    Ok(ValidationReport {
        schema: SCHEMA.into(),
        model_id: model.model_id.clone(),
        manifest_sha256: sha256_path(path)?,
        contract_sha256: canonical_sha256(model)?,
        source_contract_sha256: source_report.contract_sha256,
        analysis_contract_sha256s: model
            .analyses
            .iter()
            .map(|binding| binding.contract_sha256.clone())
            .collect(),
        duration_ticks: model.duration_ticks,
        tempo_events: model.tempo_map.len(),
        meter_events: model.meter_map.len(),
        form_sections: model.form.len(),
        parts: model.parts.len(),
        notes: model.parts.iter().map(|part| part.notes.len()).sum(),
        harmony_events: model.harmony.len(),
        rhythm_cells: model.rhythm_cells.len(),
        hooks: model.hooks.len(),
        lyric_layers: model.lyric_layers.len(),
        human_corrected_events: human_corrected,
        shareable: false,
        verified: true,
    })
}

fn validate_lead_sheet(path: &Path, model: &MusicModel) -> Result<()> {
    let Some(lead_sheet) = &model.lead_sheet else {
        return Ok(());
    };
    nonempty("lead_sheet.title", &lead_sheet.title)?;
    let part = model
        .parts
        .iter()
        .find(|part| part.id == lead_sheet.melody_part_id)
        .ok_or_else(|| anyhow::anyhow!("lead_sheet.melody_part_id is unknown"))?;
    if !matches!(part.role, PartRole::Melody | PartRole::Vocal) || part.notes.is_empty() {
        bail!("lead_sheet melody part must be a non-empty melody or vocal part");
    }
    let Some(layer_id) = &lead_sheet.lyric_layer_id else {
        if !lead_sheet.underlay.is_empty() {
            bail!("lead_sheet underlay requires lyric_layer_id");
        }
        return Ok(());
    };
    let layer = model
        .lyric_layers
        .iter()
        .find(|layer| &layer.id == layer_id)
        .ok_or_else(|| anyhow::anyhow!("lead_sheet.lyric_layer_id is unknown"))?;
    let lyric_path = source::resolve(path, &layer.path);
    let text = fs::read_to_string(lyric_path)?;
    if lead_sheet.underlay.is_empty() {
        bail!("lead_sheet with lyrics requires underlay");
    }
    let note_starts = part
        .notes
        .iter()
        .map(|note| (note.id.as_str(), note.start_tick))
        .collect::<BTreeMap<_, _>>();
    let mut underlay_ids = BTreeSet::new();
    let mut mapped_notes = BTreeSet::new();
    let mut prior_text_end = 0usize;
    let mut prior_note_tick = 0u64;
    for (index, item) in lead_sheet.underlay.iter().enumerate() {
        register_id(&mut underlay_ids, "lead_sheet.underlay[].id", &item.id)?;
        if item.note_ids.is_empty() {
            bail!("lead_sheet underlay note_ids must not be empty");
        }
        let start = usize::try_from(item.text_start_byte)?;
        let end = usize::try_from(item.text_end_byte)?;
        if start >= end
            || end > text.len()
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
            || text[start..end].trim().is_empty()
            || (index > 0 && start < prior_text_end)
        {
            bail!("lead_sheet underlay text ranges must be ordered UTF-8 syllable spans");
        }
        prior_text_end = end;
        for note_id in &item.note_ids {
            let tick = *note_starts.get(note_id.as_str()).ok_or_else(|| {
                anyhow::anyhow!("lead_sheet underlay references an unknown melody note")
            })?;
            if !mapped_notes.insert(note_id.as_str()) || (index > 0 && tick < prior_note_tick) {
                bail!("lead_sheet underlay notes must be unique and source ordered");
            }
            prior_note_tick = tick;
        }
    }
    if mapped_notes.len() != part.notes.len() {
        bail!("lead_sheet lyric underlay must map every melody note exactly once");
    }
    Ok(())
}

fn validate_piano_vocal_score(model: &MusicModel) -> Result<()> {
    let Some(score) = &model.piano_vocal_score else {
        return Ok(());
    };
    nonempty("piano_vocal_score.title", &score.title)?;
    let ids = [
        score.vocal_part_id.as_str(),
        score.piano_right_hand_part_id.as_str(),
        score.piano_left_hand_part_id.as_str(),
    ];
    if ids.into_iter().collect::<BTreeSet<_>>().len() != 3 {
        bail!("piano_vocal_score requires three distinct vocal, right-hand, and left-hand parts");
    }
    for (field, id) in [
        ("vocal_part_id", &score.vocal_part_id),
        ("piano_right_hand_part_id", &score.piano_right_hand_part_id),
        ("piano_left_hand_part_id", &score.piano_left_hand_part_id),
    ] {
        if !model.parts.iter().any(|part| &part.id == id) {
            bail!("piano_vocal_score.{field} is unknown");
        }
    }
    let vocal = model
        .parts
        .iter()
        .find(|part| part.id == score.vocal_part_id)
        .expect("part existence validated");
    if !matches!(vocal.role, PartRole::Melody | PartRole::Vocal) {
        bail!("piano_vocal_score vocal part must have melody or vocal role");
    }
    let lead_sheet = model
        .lead_sheet
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("piano_vocal_score requires lead_sheet lyric authority"))?;
    if lead_sheet.melody_part_id != score.vocal_part_id {
        bail!("piano_vocal_score vocal part must equal lead_sheet.melody_part_id");
    }
    let first_meter = &model.meter_map[0];
    let numerator = u64::from(model.musical_timebase.pulses_per_quarter)
        .checked_mul(4)
        .and_then(|value| value.checked_mul(u64::from(first_meter.numerator)))
        .ok_or_else(|| anyhow::anyhow!("piano_vocal_score first-measure duration overflow"))?;
    if numerator % u64::from(first_meter.denominator) != 0 {
        bail!("piano_vocal_score meter is not integral at the declared PPQ");
    }
    let full_measure_ticks = numerator / u64::from(first_meter.denominator);
    if score.pickup_ticks >= full_measure_ticks || score.pickup_ticks >= model.duration_ticks {
        bail!(
            "piano_vocal_score pickup_ticks must be zero or shorter than the first full measure and model duration"
        );
    }
    Ok(())
}

fn validate_source_binding(path: &Path, binding: &SourceBinding) -> Result<PathBuf> {
    validate_sha256("source.manifest_sha256", &binding.manifest_sha256)?;
    validate_sha256("source.contract_sha256", &binding.contract_sha256)?;
    validate_sha256("source.decoded_pcm_sha256", &binding.decoded_pcm_sha256)?;
    let source_path = source::resolve(path, &binding.manifest);
    if sha256_path(&source_path)? != binding.manifest_sha256.to_lowercase() {
        bail!("source manifest sha256 does not match model binding");
    }
    let report = source::validate(&source_path)?;
    if report.contract_sha256 != binding.contract_sha256
        || report.decoded_pcm_sha256 != binding.decoded_pcm_sha256
    {
        bail!("model source contract or decoded PCM identity is stale");
    }
    Ok(source_path)
}

fn validate_analysis_bindings(
    path: &Path,
    model: &MusicModel,
    source_contract_sha256: &str,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    if model.analyses.is_empty() {
        bail!("analyses must not be empty");
    }
    let mut evidence = BTreeMap::new();
    for binding in &model.analyses {
        nonempty("analyses[].analysis_id", &binding.analysis_id)?;
        validate_sha256("analyses[].manifest_sha256", &binding.manifest_sha256)?;
        validate_sha256("analyses[].contract_sha256", &binding.contract_sha256)?;
        let analysis_path = source::resolve(path, &binding.manifest);
        if sha256_path(&analysis_path)? != binding.manifest_sha256.to_lowercase() {
            bail!("analysis {} manifest sha256 is stale", binding.analysis_id);
        }
        let report = analysis::validate(&analysis_path)?;
        let manifest = analysis::load(&analysis_path)?;
        if report.analysis_id != binding.analysis_id
            || report.contract_sha256 != binding.contract_sha256
            || report.source_contract_sha256 != source_contract_sha256
        {
            bail!(
                "analysis {} binding or source lineage is stale",
                binding.analysis_id
            );
        }
        let ids = manifest
            .observations
            .into_iter()
            .map(|observation| observation.id)
            .collect();
        if evidence.insert(binding.analysis_id.clone(), ids).is_some() {
            bail!("analyses[].analysis_id must be unique");
        }
    }
    Ok(evidence)
}

fn validate_point_maps(
    model: &MusicModel,
    evidence: &BTreeMap<String, BTreeSet<String>>,
    human_corrected: &mut usize,
) -> Result<()> {
    if model.tempo_map.is_empty() || model.tempo_map[0].tick != 0 {
        bail!("tempo_map must start at tick zero");
    }
    let mut prior = None;
    for event in &model.tempo_map {
        if event.tick >= model.duration_ticks || prior.is_some_and(|tick| event.tick <= tick) {
            bail!("tempo_map ticks must be strictly increasing within duration");
        }
        if !(100_000..=3_000_000).contains(&event.microseconds_per_quarter) {
            bail!("tempo_map microseconds_per_quarter is outside supported bounds");
        }
        validate_provenance(&event.provenance, evidence, human_corrected)?;
        prior = Some(event.tick);
    }
    if model.meter_map.is_empty() || model.meter_map[0].tick != 0 {
        bail!("meter_map must start at tick zero");
    }
    prior = None;
    for event in &model.meter_map {
        if event.tick >= model.duration_ticks || prior.is_some_and(|tick| event.tick <= tick) {
            bail!("meter_map ticks must be strictly increasing within duration");
        }
        if event.numerator == 0
            || event.numerator > 32
            || !event.denominator.is_power_of_two()
            || event.denominator > 64
        {
            bail!("meter_map contains an invalid meter");
        }
        validate_provenance(&event.provenance, evidence, human_corrected)?;
        prior = Some(event.tick);
    }
    Ok(())
}

fn validate_form(
    model: &MusicModel,
    evidence: &BTreeMap<String, BTreeSet<String>>,
    element_ids: &mut BTreeSet<String>,
    human_corrected: &mut usize,
) -> Result<()> {
    if model.form.is_empty() {
        bail!("form must not be empty");
    }
    let mut cursor = 0;
    for section in &model.form {
        register_id(element_ids, "form[].id", &section.id)?;
        nonempty("form[].label", &section.label)?;
        section
            .range
            .validate(model.duration_ticks, "form[].range")?;
        if section.range.start != cursor {
            bail!("form sections must cover the model contiguously from tick zero");
        }
        cursor = section.range.end;
        validate_provenance(&section.provenance, evidence, human_corrected)?;
    }
    if cursor != model.duration_ticks {
        bail!("form sections must cover the complete model duration");
    }
    Ok(())
}

fn validate_parts(
    model: &MusicModel,
    evidence: &BTreeMap<String, BTreeSet<String>>,
    element_ids: &mut BTreeSet<String>,
    human_corrected: &mut usize,
) -> Result<BTreeMap<String, (u64, u64)>> {
    if model.parts.is_empty() {
        bail!("parts must not be empty");
    }
    let mut part_ids = BTreeSet::new();
    let mut note_ids = BTreeSet::new();
    let mut note_ranges = BTreeMap::new();
    for part in &model.parts {
        register_id(&mut part_ids, "parts[].id", &part.id)?;
        nonempty("parts[].name", &part.name)?;
        if part.notes.is_empty() {
            bail!("part {} must contain at least one note", part.id);
        }
        let mut prior_key = None;
        for note in &part.notes {
            register_id(&mut note_ids, "notes[].id", &note.id)?;
            register_id(element_ids, "notes[].id", &note.id)?;
            if note.voice == 0
                || note.duration_ticks == 0
                || note.midi_note > 127
                || note.velocity > 127
            {
                bail!(
                    "note {} has invalid voice, duration, pitch, or velocity",
                    note.id
                );
            }
            let end = note
                .start_tick
                .checked_add(note.duration_ticks)
                .ok_or_else(|| anyhow::anyhow!("note duration overflow"))?;
            if end > model.duration_ticks {
                bail!("note {} ends beyond model duration", note.id);
            }
            let key = (
                note.start_tick,
                note.voice,
                note.midi_note,
                note.id.as_str(),
            );
            if prior_key.is_some_and(|prior| key <= prior) {
                bail!("notes in part {} must use canonical order", part.id);
            }
            prior_key = Some(key);
            note_ranges.insert(note.id.clone(), (note.start_tick, note.duration_ticks));
            validate_provenance(&note.provenance, evidence, human_corrected)?;
        }
    }
    Ok(note_ranges)
}

fn validate_harmony(
    model: &MusicModel,
    evidence: &BTreeMap<String, BTreeSet<String>>,
    element_ids: &mut BTreeSet<String>,
    human_corrected: &mut usize,
) -> Result<()> {
    let mut prior_end = 0;
    for (index, event) in model.harmony.iter().enumerate() {
        register_id(element_ids, "harmony[].id", &event.id)?;
        event
            .range
            .validate(model.duration_ticks, "harmony[].range")?;
        if index > 0 && event.range.start < prior_end {
            bail!("harmony events must be ordered and non-overlapping");
        }
        prior_end = event.range.end;
        nonempty("harmony[].symbol", &event.symbol)?;
        validate_provenance(&event.provenance, evidence, human_corrected)?;
    }
    Ok(())
}

fn validate_rhythm(
    model: &MusicModel,
    evidence: &BTreeMap<String, BTreeSet<String>>,
    element_ids: &mut BTreeSet<String>,
    human_corrected: &mut usize,
) -> Result<()> {
    for cell in &model.rhythm_cells {
        register_id(element_ids, "rhythm_cells[].id", &cell.id)?;
        cell.range
            .validate(model.duration_ticks, "rhythm_cells[].range")?;
        if cell.onset_offsets_ticks.is_empty()
            || cell
                .onset_offsets_ticks
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || cell
                .onset_offsets_ticks
                .iter()
                .any(|offset| *offset >= cell.range.len())
        {
            bail!(
                "rhythm cell {} onsets must be unique, ordered, and within range",
                cell.id
            );
        }
        validate_provenance(&cell.provenance, evidence, human_corrected)?;
    }
    Ok(())
}

fn validate_hooks(
    model: &MusicModel,
    evidence: &BTreeMap<String, BTreeSet<String>>,
    element_ids: &BTreeSet<String>,
    human_corrected: &mut usize,
) -> Result<()> {
    let mut ids = BTreeSet::new();
    for hook in &model.hooks {
        register_id(&mut ids, "hooks[].id", &hook.id)?;
        nonempty("hooks[].label", &hook.label)?;
        hook.range.validate(model.duration_ticks, "hooks[].range")?;
        unique_nonempty("hooks[].element_refs", &hook.element_refs)?;
        if hook.element_refs.is_empty()
            || hook.element_refs.iter().any(|id| !element_ids.contains(id))
        {
            bail!("hook {} must reference known model elements", hook.id);
        }
        validate_provenance(&hook.provenance, evidence, human_corrected)?;
    }
    Ok(())
}

fn validate_lyrics(path: &Path, model: &MusicModel) -> Result<()> {
    let mut ids = BTreeSet::new();
    for layer in &model.lyric_layers {
        register_id(&mut ids, "lyric_layers[].id", &layer.id)?;
        nonempty("lyric_layers[].language", &layer.language)?;
        validate_sha256("lyric_layers[].sha256", &layer.sha256)?;
        validate_authority(&layer.authority)?;
        let lyric_path = source::resolve(path, &layer.path);
        if sha256_path(&lyric_path)? != layer.sha256.to_lowercase() {
            bail!("lyric layer {} sha256 does not match", layer.id);
        }
    }
    if model.parts.iter().any(|part| part.role == PartRole::Vocal) && model.lyric_layers.is_empty()
    {
        bail!("a vocal part requires at least one exact lyric layer");
    }
    Ok(())
}

fn validate_expressive_timing(
    model: &MusicModel,
    evidence: &BTreeMap<String, BTreeSet<String>>,
    note_ranges: &BTreeMap<String, (u64, u64)>,
    human_corrected: &mut usize,
) -> Result<()> {
    let mut timed = BTreeSet::new();
    for timing in &model.expressive_timing {
        let Some((start, duration)) = note_ranges.get(&timing.note_id) else {
            bail!("expressive_timing must reference each known note at most once");
        };
        if !timed.insert(timing.note_id.as_str()) {
            bail!("expressive_timing must reference each known note at most once");
        }
        let adjusted_start = i128::from(*start) + i128::from(timing.onset_offset_ticks);
        let adjusted_duration = i128::from(*duration) + i128::from(timing.duration_offset_ticks);
        if adjusted_start < 0
            || adjusted_duration <= 0
            || adjusted_start + adjusted_duration > i128::from(model.duration_ticks)
        {
            bail!("expressive_timing adjustment must remain within model duration");
        }
        validate_provenance(&timing.provenance, evidence, human_corrected)?;
    }
    Ok(())
}

fn validate_provenance(
    provenance: &Provenance,
    evidence: &BTreeMap<String, BTreeSet<String>>,
    human_corrected: &mut usize,
) -> Result<()> {
    nonempty("provenance.rationale", &provenance.rationale)?;
    let mut refs = BTreeSet::new();
    for reference in &provenance.evidence_refs {
        if !refs.insert(reference) {
            bail!("provenance.evidence_refs must be unique");
        }
        if !evidence
            .get(&reference.analysis_id)
            .is_some_and(|ids| ids.contains(&reference.observation_id))
        {
            bail!("provenance references unknown analysis observation");
        }
    }
    match provenance.state {
        ProvenanceState::Observed | ProvenanceState::Inferred => {
            if provenance.evidence_refs.is_empty() || provenance.correction_ref.is_some() {
                bail!("observed/inferred provenance requires evidence and forbids correction_ref");
            }
        }
        ProvenanceState::HumanCorrected => {
            let correction = provenance.correction_ref.as_ref().ok_or_else(|| {
                anyhow::anyhow!("human-corrected provenance requires correction_ref")
            })?;
            nonempty(
                "provenance.correction_ref.artifact_id",
                &correction.artifact_id,
            )?;
            validate_sha256("provenance.correction_ref.sha256", &correction.sha256)?;
            *human_corrected += 1;
        }
    }
    Ok(())
}

fn validate_review(review: &Review) -> Result<()> {
    nonempty("review.status", &review.status)?;
    unique_nonempty("review.required_roles", &review.required_roles)?;
    for role in REQUIRED_ROLES {
        if !review.required_roles.iter().any(|value| value == role) {
            bail!("review.required_roles must include {role}");
        }
    }
    let mut ids = BTreeSet::new();
    for decision in &review.decision_refs {
        nonempty("review.decision_refs[].artifact_id", &decision.artifact_id)?;
        validate_sha256("review.decision_refs[].sha256", &decision.sha256)?;
        if !ids.insert(decision.artifact_id.as_str()) {
            bail!("review.decision_refs artifact ids must be unique");
        }
    }
    if status_requires_decision(&review.status) && review.decision_refs.is_empty() {
        bail!("review.status {} requires decision_refs", review.status);
    }
    Ok(())
}

fn register_id(ids: &mut BTreeSet<String>, field: &str, id: &str) -> Result<()> {
    nonempty(field, id)?;
    if !ids.insert(id.to_string()) {
        bail!("{field} must be unique");
    }
    Ok(())
}
