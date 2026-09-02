use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::{
    AuthorityRef,
    hash::{canonical_sha256, sha256_path},
    model::{self, MusicModel, PartRole, Review},
    nonempty, source, status_requires_decision, unique_nonempty, validate_authority,
    validate_sha256,
};

pub const SCHEMA: &str = "reel.music-perspective-comparison.v0.1";
pub const REPORT_SCHEMA: &str = "reel.music-perspective-comparison-report.v0.1";
const REQUIRED_ROLES: &[&str] = &[
    "music-reconstruction-engineer",
    "score-arrangement-director",
    "sound-designer",
    "editor",
    "rights-provenance-steward",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PerspectiveComparison {
    pub schema: String,
    pub comparison_id: String,
    pub recovered_model: ModelBinding,
    pub piano_model: ModelBinding,
    pub recovered_melody_part_id: String,
    pub piano_melody_part_id: String,
    pub policy: MatchPolicy,
    pub authority: AuthorityRef,
    pub review: Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelBinding {
    pub manifest: PathBuf,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub model_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MatchPolicy {
    pub onset_tolerance_ticks: u64,
    pub duration_tolerance_ticks: u64,
    pub pitch_tolerance_semitones: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteMatch {
    pub recovered_note_id: String,
    pub piano_note_id: String,
    pub onset_delta_ticks: u64,
    pub duration_delta_ticks: u64,
    pub pitch_delta_semitones: u8,
    pub exact: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonReport {
    pub schema: String,
    pub comparison_id: String,
    pub comparison_manifest_sha256: String,
    pub comparison_contract_sha256: String,
    pub recovered_model_contract_sha256: String,
    pub piano_model_contract_sha256: String,
    pub pulses_per_quarter: u32,
    pub duration_ticks: u64,
    pub tempo_map_equal: bool,
    pub meter_map_equal: bool,
    pub form_equal: bool,
    pub recovered_notes: usize,
    pub piano_notes: usize,
    pub exact_matches: usize,
    pub tolerance_matches: usize,
    pub agreement_millionths: u32,
    pub matches: Vec<NoteMatch>,
    pub recovered_only_note_ids: Vec<String>,
    pub piano_only_note_ids: Vec<String>,
    pub limitations: Vec<String>,
    pub shareable: bool,
    pub verified: bool,
}

pub fn load(path: &Path) -> Result<PerspectiveComparison> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_slice(&bytes).with_context(|| {
        format!(
            "music perspective comparison is not valid YAML: {}",
            path.display()
        )
    })
}

pub fn build(path: &Path) -> Result<ComparisonReport> {
    let manifest = load(path)?;
    if manifest.schema != SCHEMA {
        bail!("music perspective comparison schema must be {SCHEMA}");
    }
    nonempty("comparison_id", &manifest.comparison_id)?;
    validate_authority(&manifest.authority)?;
    validate_review(&manifest.review)?;
    let (recovered_report, recovered) =
        validate_model(path, "recovered_model", &manifest.recovered_model)?;
    let (piano_report, piano) = validate_model(path, "piano_model", &manifest.piano_model)?;
    if recovered.musical_timebase != piano.musical_timebase
        || recovered.duration_ticks != piano.duration_ticks
    {
        bail!("recovered and piano perspectives require the same musical timebase and duration");
    }
    let ppq = u64::from(recovered.musical_timebase.pulses_per_quarter);
    if manifest.policy.onset_tolerance_ticks > ppq
        || manifest.policy.duration_tolerance_ticks > ppq
        || manifest.policy.pitch_tolerance_semitones > 12
    {
        bail!("perspective match tolerances may not exceed one quarter note or one octave");
    }
    let recovered_part = melody_part(
        &recovered,
        "recovered_melody_part_id",
        &manifest.recovered_melody_part_id,
    )?;
    let piano_part = melody_part(
        &piano,
        "piano_melody_part_id",
        &manifest.piano_melody_part_id,
    )?;
    let mut candidates = Vec::new();
    for (left_index, left) in recovered_part.notes.iter().enumerate() {
        for (right_index, right) in piano_part.notes.iter().enumerate() {
            let onset = left.start_tick.abs_diff(right.start_tick);
            let duration = left.duration_ticks.abs_diff(right.duration_ticks);
            let pitch = left.midi_note.abs_diff(right.midi_note);
            if onset <= manifest.policy.onset_tolerance_ticks
                && duration <= manifest.policy.duration_tolerance_ticks
                && pitch <= manifest.policy.pitch_tolerance_semitones
            {
                candidates.push((
                    pitch,
                    onset,
                    duration,
                    left.id.as_str(),
                    right.id.as_str(),
                    left_index,
                    right_index,
                ));
            }
        }
    }
    candidates.sort();
    let mut used_left = BTreeSet::new();
    let mut used_right = BTreeSet::new();
    let mut matches = Vec::new();
    for (pitch, onset, duration, _, _, left_index, right_index) in candidates {
        if used_left.contains(&left_index) || used_right.contains(&right_index) {
            continue;
        }
        used_left.insert(left_index);
        used_right.insert(right_index);
        let left = &recovered_part.notes[left_index];
        let right = &piano_part.notes[right_index];
        matches.push(NoteMatch {
            recovered_note_id: left.id.clone(),
            piano_note_id: right.id.clone(),
            onset_delta_ticks: onset,
            duration_delta_ticks: duration,
            pitch_delta_semitones: pitch,
            exact: onset == 0 && duration == 0 && pitch == 0,
        });
    }
    matches.sort_by(|left, right| {
        (&left.recovered_note_id, &left.piano_note_id)
            .cmp(&(&right.recovered_note_id, &right.piano_note_id))
    });
    let recovered_only_note_ids = recovered_part
        .notes
        .iter()
        .enumerate()
        .filter(|(index, _)| !used_left.contains(index))
        .map(|(_, note)| note.id.clone())
        .collect::<Vec<_>>();
    let piano_only_note_ids = piano_part
        .notes
        .iter()
        .enumerate()
        .filter(|(index, _)| !used_right.contains(index))
        .map(|(_, note)| note.id.clone())
        .collect::<Vec<_>>();
    let exact_matches = matches.iter().filter(|item| item.exact).count();
    let tolerance_matches = matches.len() - exact_matches;
    let denominator = recovered_part.notes.len().max(piano_part.notes.len());
    let agreement_millionths = if denominator == 0 {
        0
    } else {
        u32::try_from(
            (matches.len() as u64 * 1_000_000 + denominator as u64 / 2) / denominator as u64,
        )?
    };
    Ok(ComparisonReport {
        schema: REPORT_SCHEMA.into(),
        comparison_id: manifest.comparison_id.clone(),
        comparison_manifest_sha256: sha256_path(path)?,
        comparison_contract_sha256: canonical_sha256(&manifest)?,
        recovered_model_contract_sha256: recovered_report.contract_sha256,
        piano_model_contract_sha256: piano_report.contract_sha256,
        pulses_per_quarter: recovered.musical_timebase.pulses_per_quarter,
        duration_ticks: recovered.duration_ticks,
        tempo_map_equal: recovered
            .tempo_map
            .iter()
            .map(|item| (item.tick, item.microseconds_per_quarter))
            .eq(piano
                .tempo_map
                .iter()
                .map(|item| (item.tick, item.microseconds_per_quarter))),
        meter_map_equal: recovered
            .meter_map
            .iter()
            .map(|item| (item.tick, item.numerator, item.denominator))
            .eq(piano
                .meter_map
                .iter()
                .map(|item| (item.tick, item.numerator, item.denominator))),
        form_equal: recovered
            .form
            .iter()
            .map(|item| (item.range.start, item.range.end, item.label.as_str()))
            .eq(piano
                .form
                .iter()
                .map(|item| (item.range.start, item.range.end, item.label.as_str()))),
        recovered_notes: recovered_part.notes.len(),
        piano_notes: piano_part.notes.len(),
        exact_matches,
        tolerance_matches,
        agreement_millionths,
        matches,
        recovered_only_note_ids,
        piano_only_note_ids,
        limitations: vec![
            "the piano reduction is a dependent interpretation, not independent proof of the source recording".into(),
            "note agreement measures declared pitch and timing only; it does not establish harmonic, expressive, emotional, or arrangement fidelity".into(),
            "technical comparison does not select, approve, or authorize either model or any resulting score".into(),
        ],
        shareable: false,
        verified: true,
    })
}

pub fn write(comparison_path: &Path, output_path: &Path) -> Result<ComparisonReport> {
    if output_path.exists() {
        bail!(
            "perspective comparison output already exists: {}",
            output_path.display()
        );
    }
    let report = build(comparison_path)?;
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(&serde_json::to_vec_pretty(&report)?)?;
    temporary.flush()?;
    temporary
        .persist(output_path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish {}", output_path.display()))?;
    Ok(report)
}

pub fn check(comparison_path: &Path, report_path: &Path) -> Result<ComparisonReport> {
    let actual: ComparisonReport = serde_json::from_slice(&fs::read(report_path)?)
        .with_context(|| format!("invalid perspective report: {}", report_path.display()))?;
    let expected = build(comparison_path)?;
    if actual != expected {
        bail!("perspective comparison report does not match current models and policy");
    }
    Ok(actual)
}

fn validate_model(
    manifest_path: &Path,
    field: &str,
    binding: &ModelBinding,
) -> Result<(model::ValidationReport, MusicModel)> {
    nonempty(&format!("{field}.model_id"), &binding.model_id)?;
    validate_sha256(
        &format!("{field}.manifest_sha256"),
        &binding.manifest_sha256,
    )?;
    validate_sha256(
        &format!("{field}.contract_sha256"),
        &binding.contract_sha256,
    )?;
    let path = source::resolve(manifest_path, &binding.manifest);
    if sha256_path(&path)? != binding.manifest_sha256.to_lowercase() {
        bail!("{field} manifest sha256 does not match comparison binding");
    }
    let report = model::validate(&path)?;
    if report.model_id != binding.model_id
        || report.contract_sha256 != binding.contract_sha256.to_lowercase()
    {
        bail!("{field} contract or identity does not match comparison binding");
    }
    Ok((report, model::load(&path)?))
}

fn melody_part<'a>(model: &'a MusicModel, field: &str, id: &str) -> Result<&'a crate::model::Part> {
    nonempty(field, id)?;
    let part = model
        .parts
        .iter()
        .find(|part| part.id == id)
        .ok_or_else(|| anyhow!("{field} is unknown"))?;
    if !matches!(part.role, PartRole::Melody | PartRole::Vocal) || part.notes.is_empty() {
        bail!("{field} must identify a non-empty melody or vocal part");
    }
    Ok(part)
}

fn validate_review(review: &Review) -> Result<()> {
    nonempty("review.status", &review.status)?;
    unique_nonempty("review.required_roles", &review.required_roles)?;
    for role in REQUIRED_ROLES {
        if !review.required_roles.iter().any(|value| value == role) {
            bail!("review.required_roles must include {role}");
        }
    }
    if status_requires_decision(&review.status) && review.decision_refs.is_empty() {
        bail!("review.status {} requires decision_refs", review.status);
    }
    let mut decisions = BTreeSet::new();
    for decision in &review.decision_refs {
        nonempty("review.decision_refs[].artifact_id", &decision.artifact_id)?;
        validate_sha256("review.decision_refs[].sha256", &decision.sha256)?;
        if !decisions.insert(decision.artifact_id.as_str()) {
            bail!("review.decision_refs artifact ids must be unique");
        }
    }
    Ok(())
}
