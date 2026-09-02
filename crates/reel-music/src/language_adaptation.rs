use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    AuthorityRef, DecisionRef,
    hash::{canonical_sha256, sha256_path},
    model::{self, MusicModel, PartRole},
    model_draft, nonempty, repair, source, status_requires_decision, validate_authority,
    validate_sha256,
};

pub const SCHEMA: &str = "reel.music-language-adaptation.v0.1";
const REQUIRED_ROLES: &[&str] = &[
    "music-reconstruction-engineer",
    "score-arrangement-director",
    "lyrics-vocal-adaptation-editor",
    "sound-designer",
    "editor",
    "rights-provenance-steward",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageAdaptation {
    pub schema: String,
    pub adaptation_id: String,
    pub model_draft: DraftBinding,
    pub accompaniment: AudioBinding,
    pub source_text: TextLayer,
    pub target_text: TextLayer,
    pub translation_decision: DecisionRef,
    pub translation_links: Vec<TranslationLink>,
    pub preserved_model_targets: Vec<String>,
    pub underlay: Vec<Underlay>,
    pub prosody_exceptions: Vec<ProsodyException>,
    pub review: repair::Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DraftBinding {
    pub manifest: PathBuf,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub draft_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudioBinding {
    pub path: PathBuf,
    pub sha256: String,
    pub decoded_pcm_sha256: String,
    pub format: source::RawPcmFormat,
    pub timebase: crate::time::AudioTimebase,
    pub source_contract_sha256: String,
    pub derivation_decision: DecisionRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TextLayer {
    pub kind: TextLayerKind,
    pub language: String,
    pub path: PathBuf,
    pub sha256: String,
    pub authority: AuthorityRef,
    pub units: Vec<TextUnit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextLayerKind {
    CanonicalSource,
    ApprovedTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TextUnit {
    pub id: String,
    pub byte_start: u64,
    pub byte_end: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TranslationLink {
    pub id: String,
    pub source_unit_ids: Vec<String>,
    pub target_unit_ids: Vec<String>,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Underlay {
    pub target_unit_id: String,
    pub note_ids: Vec<String>,
    pub stress: Stress,
    pub melisma: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stress {
    Unstressed,
    Secondary,
    Primary,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProsodyException {
    pub id: String,
    pub translation_link_id: String,
    pub kind: ProsodyExceptionKind,
    pub target_unit_ids: Vec<String>,
    pub note_ids: Vec<String>,
    pub rationale: String,
    pub required_review_roles: Vec<String>,
    pub decision: DecisionRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProsodyExceptionKind {
    Onset,
    Duration,
    Pitch,
    Melisma,
    Stress,
    Rest,
    Pickup,
    Cadence,
    PhraseBoundary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub adaptation_id: String,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub model_contract_sha256: String,
    pub accompaniment_sha256: String,
    pub source_text_sha256: String,
    pub target_text_sha256: String,
    pub source_units: usize,
    pub target_units: usize,
    pub translation_links: usize,
    pub underlay_events: usize,
    pub prosody_exceptions: usize,
    pub complete_text_coverage: bool,
    pub complete_model_inheritance: bool,
    pub exact_accompaniment_duration: bool,
    pub shareable: bool,
    pub verified: bool,
}

pub fn load(path: &Path) -> Result<LanguageAdaptation> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_slice(&bytes).with_context(|| {
        format!(
            "music language adaptation is not valid YAML: {}",
            path.display()
        )
    })
}

pub fn validate(path: &Path) -> Result<ValidationReport> {
    let manifest = load(path)?;
    if manifest.schema != SCHEMA {
        bail!("music language adaptation schema must be {SCHEMA}");
    }
    nonempty("adaptation_id", &manifest.adaptation_id)?;
    validate_decision("translation_decision", &manifest.translation_decision)?;

    let (draft_report, music_model) = validate_draft(path, &manifest.model_draft)?;
    let accompaniment_sha256 = validate_accompaniment(path, &manifest.accompaniment, &music_model)?;
    let source_ids = validate_text_layer(
        path,
        "source_text",
        &manifest.source_text,
        TextLayerKind::CanonicalSource,
    )?;
    let target_ids = validate_text_layer(
        path,
        "target_text",
        &manifest.target_text,
        TextLayerKind::ApprovedTarget,
    )?;
    if manifest.source_text.language == manifest.target_text.language {
        bail!("source and target text languages must differ");
    }
    if manifest.target_text.authority.status != "approved" {
        bail!("target text authority must be explicitly approved");
    }
    validate_translation_links(&manifest.translation_links, &source_ids, &target_ids)?;

    let model_targets = model_draft::model_targets(&music_model)?
        .into_keys()
        .collect::<BTreeSet<_>>();
    let preserved = manifest
        .preserved_model_targets
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if preserved.len() != manifest.preserved_model_targets.len() || preserved != model_targets {
        bail!("preserved_model_targets must name every governed model target exactly once");
    }
    let notes = vocal_notes(&music_model)?;
    validate_underlay(&manifest.underlay, &target_ids, &notes)?;
    validate_prosody(
        &manifest.prosody_exceptions,
        &manifest.translation_links,
        &target_ids,
        &notes,
    )?;
    validate_review(&manifest.review)?;

    Ok(ValidationReport {
        schema: SCHEMA.into(),
        adaptation_id: manifest.adaptation_id.clone(),
        manifest_sha256: sha256_path(path)?,
        contract_sha256: canonical_sha256(&manifest)?,
        model_contract_sha256: draft_report.model_contract_sha256,
        accompaniment_sha256,
        source_text_sha256: manifest.source_text.sha256.to_lowercase(),
        target_text_sha256: manifest.target_text.sha256.to_lowercase(),
        source_units: source_ids.len(),
        target_units: target_ids.len(),
        translation_links: manifest.translation_links.len(),
        underlay_events: manifest.underlay.len(),
        prosody_exceptions: manifest.prosody_exceptions.len(),
        complete_text_coverage: true,
        complete_model_inheritance: true,
        exact_accompaniment_duration: true,
        shareable: false,
        verified: true,
    })
}

fn validate_draft(
    path: &Path,
    binding: &DraftBinding,
) -> Result<(model_draft::ValidationReport, MusicModel)> {
    validate_sha256("model_draft.manifest_sha256", &binding.manifest_sha256)?;
    validate_sha256("model_draft.contract_sha256", &binding.contract_sha256)?;
    nonempty("model_draft.draft_id", &binding.draft_id)?;
    let draft_path = source::resolve(path, &binding.manifest);
    if sha256_path(&draft_path)? != binding.manifest_sha256.to_lowercase() {
        bail!("model draft manifest sha256 does not match adaptation binding");
    }
    let report = model_draft::validate(&draft_path)?;
    if report.contract_sha256 != binding.contract_sha256.to_lowercase()
        || report.draft_id != binding.draft_id
    {
        bail!("model draft contract or identity does not match adaptation binding");
    }
    let draft = model_draft::load(&draft_path)?;
    let model_path = source::resolve(&draft_path, &draft.model.manifest);
    Ok((report, model::load(&model_path)?))
}

fn validate_accompaniment(path: &Path, audio: &AudioBinding, model: &MusicModel) -> Result<String> {
    validate_sha256("accompaniment.sha256", &audio.sha256)?;
    validate_sha256(
        "accompaniment.decoded_pcm_sha256",
        &audio.decoded_pcm_sha256,
    )?;
    validate_sha256(
        "accompaniment.source_contract_sha256",
        &audio.source_contract_sha256,
    )?;
    validate_decision(
        "accompaniment.derivation_decision",
        &audio.derivation_decision,
    )?;
    audio.timebase.validate()?;
    if audio.source_contract_sha256.to_lowercase() != model.source.contract_sha256.to_lowercase() {
        bail!("accompaniment must derive from the governed model source contract");
    }
    let resolved = source::resolve(path, &audio.path);
    let hash = sha256_path(&resolved)?;
    if hash != audio.sha256.to_lowercase() || hash != audio.decoded_pcm_sha256.to_lowercase() {
        bail!("raw PCM accompaniment hashes do not match");
    }
    let expected_samples =
        ticks_to_samples(model, model.duration_ticks, audio.timebase.sample_rate_hz)?;
    if audio.timebase.samples_per_channel != expected_samples {
        bail!("accompaniment duration does not match the governed tempo map and model duration");
    }
    let expected_bytes = expected_samples
        .checked_mul(u64::from(audio.timebase.channels))
        .and_then(|value| value.checked_mul(audio.format.bytes_per_sample()))
        .ok_or_else(|| anyhow::anyhow!("accompaniment byte count overflows u64"))?;
    if fs::metadata(resolved)?.len() != expected_bytes {
        bail!("accompaniment byte count does not match its declared timebase");
    }
    Ok(hash)
}

fn ticks_to_samples(model: &MusicModel, target_tick: u64, sample_rate: u32) -> Result<u64> {
    let mut numerator = 0_u128;
    for (index, tempo) in model.tempo_map.iter().enumerate() {
        if tempo.tick >= target_tick {
            break;
        }
        let end = model
            .tempo_map
            .get(index + 1)
            .map(|next| next.tick)
            .unwrap_or(target_tick)
            .min(target_tick);
        let ticks = end - tempo.tick;
        numerator = numerator
            .checked_add(
                u128::from(ticks)
                    * u128::from(tempo.microseconds_per_quarter)
                    * u128::from(sample_rate),
            )
            .ok_or_else(|| anyhow::anyhow!("musical time mapping overflows u128"))?;
    }
    let denominator = u128::from(model.musical_timebase.pulses_per_quarter) * 1_000_000;
    let samples = match model.musical_timebase.rounding {
        crate::time::RoundingMode::Floor => numerator / denominator,
        crate::time::RoundingMode::Ceiling => numerator.div_ceil(denominator),
        crate::time::RoundingMode::HalfAwayFromZero => (numerator + denominator / 2) / denominator,
    };
    u64::try_from(samples).context("musical time mapping exceeds u64")
}

fn validate_text_layer(
    path: &Path,
    field: &str,
    layer: &TextLayer,
    expected: TextLayerKind,
) -> Result<Vec<String>> {
    if layer.kind != expected {
        bail!("{field}.kind is not valid for this adaptation side");
    }
    nonempty(&format!("{field}.language"), &layer.language)?;
    validate_sha256(&format!("{field}.sha256"), &layer.sha256)?;
    validate_authority(&layer.authority)?;
    let resolved = source::resolve(path, &layer.path);
    if sha256_path(&resolved)? != layer.sha256.to_lowercase() {
        bail!("{field} sha256 does not match");
    }
    let text = fs::read_to_string(&resolved)?;
    let mut ids = BTreeSet::new();
    let mut ordered = Vec::new();
    let mut cursor = 0_usize;
    if layer.units.is_empty() {
        bail!("{field}.units must not be empty");
    }
    for unit in &layer.units {
        nonempty(&format!("{field}.units[].id"), &unit.id)?;
        if !ids.insert(unit.id.as_str()) {
            bail!("{field} unit ids must be unique");
        }
        let start = usize::try_from(unit.byte_start)?;
        let end = usize::try_from(unit.byte_end)?;
        if start < cursor
            || start >= end
            || end > text.len()
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
        {
            bail!("{field} units must be ordered non-empty UTF-8 byte ranges");
        }
        if !text[cursor..start].chars().all(char::is_whitespace)
            || text[start..end].trim().is_empty()
        {
            bail!("{field} units must cover every non-whitespace character exactly once");
        }
        cursor = end;
        ordered.push(unit.id.clone());
    }
    if !text[cursor..].chars().all(char::is_whitespace) {
        bail!("{field} units must cover every non-whitespace character exactly once");
    }
    Ok(ordered)
}

fn validate_translation_links(
    links: &[TranslationLink],
    source: &[String],
    target: &[String],
) -> Result<()> {
    if links.is_empty() {
        bail!("translation_links must not be empty");
    }
    let mut ids = BTreeSet::new();
    let mut source_flat = Vec::new();
    let mut target_flat = Vec::new();
    for link in links {
        nonempty("translation_links[].id", &link.id)?;
        nonempty("translation_links[].rationale", &link.rationale)?;
        if !ids.insert(link.id.as_str())
            || link.source_unit_ids.is_empty()
            || link.target_unit_ids.is_empty()
        {
            bail!("translation links require unique ids and non-empty source/target units");
        }
        source_flat.extend(link.source_unit_ids.iter().cloned());
        target_flat.extend(link.target_unit_ids.iter().cloned());
    }
    if source_flat != source || target_flat != target {
        bail!("translation links must cover source and target units exactly once in order");
    }
    Ok(())
}

fn vocal_notes(model: &MusicModel) -> Result<BTreeMap<String, u64>> {
    let mut notes = BTreeMap::new();
    for part in &model.parts {
        if matches!(part.role, PartRole::Melody | PartRole::Vocal) {
            for note in &part.notes {
                if notes.insert(note.id.clone(), note.start_tick).is_some() {
                    bail!("melody/vocal note ids must be unique for underlay");
                }
            }
        }
    }
    if notes.is_empty() {
        bail!("language adaptation requires a melody or vocal part");
    }
    Ok(notes)
}

fn validate_underlay(
    underlay: &[Underlay],
    target: &[String],
    notes: &BTreeMap<String, u64>,
) -> Result<()> {
    if underlay.len() != target.len() {
        bail!("underlay must cover every target text unit exactly once");
    }
    let mut prior_tick = 0;
    for (event, expected_unit) in underlay.iter().zip(target) {
        if &event.target_unit_id != expected_unit || event.note_ids.is_empty() {
            bail!("underlay must follow target text order and reference notes");
        }
        let unique = event.note_ids.iter().collect::<BTreeSet<_>>();
        if unique.len() != event.note_ids.len() || event.melisma != (event.note_ids.len() > 1) {
            bail!("underlay note ids must be unique and melisma must match note count");
        }
        for note_id in &event.note_ids {
            let tick = *notes.get(note_id).ok_or_else(|| {
                anyhow::anyhow!("underlay references unknown melody/vocal note {note_id}")
            })?;
            if tick < prior_tick {
                bail!("underlay notes must not move backward in musical time");
            }
            prior_tick = tick;
        }
    }
    Ok(())
}

fn validate_prosody(
    exceptions: &[ProsodyException],
    links: &[TranslationLink],
    target: &[String],
    notes: &BTreeMap<String, u64>,
) -> Result<()> {
    let target_set = target.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let link_targets = links
        .iter()
        .map(|link| {
            (
                link.id.as_str(),
                link.target_unit_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut ids = BTreeSet::new();
    let mut governed_links = BTreeSet::new();
    for exception in exceptions {
        nonempty("prosody_exceptions[].id", &exception.id)?;
        nonempty(
            "prosody_exceptions[].translation_link_id",
            &exception.translation_link_id,
        )?;
        nonempty("prosody_exceptions[].rationale", &exception.rationale)?;
        validate_decision("prosody_exceptions[].decision", &exception.decision)?;
        if !ids.insert(exception.id.as_str())
            || exception.target_unit_ids.is_empty()
            || exception.note_ids.is_empty()
        {
            bail!("prosody exceptions require unique ids and target/note refs");
        }
        let linked_targets = link_targets
            .get(exception.translation_link_id.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!("prosody exception references unknown translation link")
            })?;
        for unit in &exception.target_unit_ids {
            if !target_set.contains(unit.as_str()) || !linked_targets.contains(unit.as_str()) {
                bail!("prosody exception target units must belong to its translation link");
            }
        }
        for note in &exception.note_ids {
            if !notes.contains_key(note) {
                bail!("prosody exception references unknown melody/vocal note");
            }
        }
        if exception.required_review_roles.is_empty() {
            bail!("prosody exceptions require at least one review role");
        }
        let roles = exception
            .required_review_roles
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if roles.len() != exception.required_review_roles.len()
            || roles.iter().any(|role| !REQUIRED_ROLES.contains(role))
        {
            bail!("prosody exception review roles must be unique known adaptation roles");
        }
        governed_links.insert(exception.translation_link_id.as_str());
    }
    for link in links {
        if link.source_unit_ids.len() != link.target_unit_ids.len()
            && !governed_links.contains(link.id.as_str())
        {
            bail!("translation links with changed unit counts require a prosody exception");
        }
    }
    Ok(())
}

fn validate_decision(field: &str, decision: &DecisionRef) -> Result<()> {
    nonempty(&format!("{field}.artifact_id"), &decision.artifact_id)?;
    validate_sha256(&format!("{field}.sha256"), &decision.sha256)
}

fn validate_review(review: &repair::Review) -> Result<()> {
    nonempty("review.status", &review.status)?;
    let roles = review
        .required_roles
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if roles.len() != review.required_roles.len() {
        bail!("review.required_roles must be unique");
    }
    for role in REQUIRED_ROLES {
        if !roles.contains(role) {
            bail!("review.required_roles must include {role}");
        }
    }
    if status_requires_decision(&review.status) && review.decision_refs.is_empty() {
        bail!("review status {} requires decision_refs", review.status);
    }
    let mut decisions = BTreeSet::new();
    for decision in &review.decision_refs {
        validate_decision("review.decision_refs[]", decision)?;
        if !decisions.insert(decision.artifact_id.as_str()) {
            bail!("review decision artifact ids must be unique");
        }
    }
    Ok(())
}
