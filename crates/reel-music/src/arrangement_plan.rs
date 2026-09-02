use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    AuthorityRef, DecisionRef,
    hash::{canonical_sha256, sha256_path},
    language_adaptation::DraftBinding,
    model::{self, MusicModel, Note},
    model_draft, nonempty, repair, source, status_requires_decision, validate_authority,
    validate_sha256,
};

pub const SCHEMA: &str = "reel.music-arrangement-plan.v0.1";
const REQUIRED_ROLES: &[&str] = &[
    "music-reconstruction-engineer",
    "score-arrangement-director",
    "sound-designer",
    "editor",
    "rights-provenance-steward",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArrangementPlan {
    pub schema: String,
    pub arrangement_id: String,
    pub model_draft: DraftBinding,
    pub direction: Direction,
    pub ensemble: Ensemble,
    pub element_dispositions: Vec<ElementDisposition>,
    pub part_assignments: Vec<PartAssignment>,
    pub note_mappings: Vec<NoteMapping>,
    pub candidate_gate: CandidateGate,
    pub authority: AuthorityRef,
    pub review: repair::Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Direction {
    pub label: String,
    pub objective: String,
    pub constraints: Vec<String>,
    pub decision: DecisionRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Ensemble {
    pub maximum_instruments: u16,
    pub instruments: Vec<Instrument>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Instrument {
    pub id: String,
    pub family: String,
    pub function: String,
    pub midi_low: u8,
    pub midi_high: u8,
    pub maximum_simultaneous_notes: u16,
    pub techniques: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ElementDisposition {
    pub model_target_id: String,
    pub action: TransformAction,
    pub rationale: String,
    pub decision: Option<DecisionRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransformAction {
    Preserve,
    Develop,
    Replace,
    Omit,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PartAssignment {
    pub source_part_id: String,
    pub action: TransformAction,
    pub instrument_ids: Vec<String>,
    pub rationale: String,
    pub decision: Option<DecisionRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteMapping {
    pub id: String,
    pub source_note_id: String,
    pub instrument_id: String,
    pub action: TransformAction,
    pub start_tick: u64,
    pub duration_ticks: u64,
    pub midi_note: u8,
    pub velocity: u8,
    pub rationale: String,
    pub decision: Option<DecisionRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateGate {
    pub required_checks: Vec<CandidateCheck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateCheck {
    ExactPlanBinding,
    ModelInheritance,
    RangeAndPolyphony,
    EditableScoreRoundTrip,
    AudibleComparison,
    HumanRecognition,
    HumanSelection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub arrangement_id: String,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub model_contract_sha256: String,
    pub model_targets: usize,
    pub preserved_targets: usize,
    pub developed_targets: usize,
    pub replaced_targets: usize,
    pub omitted_targets: usize,
    pub source_parts: usize,
    pub instruments: usize,
    pub mapped_notes: usize,
    pub candidate_checks: usize,
    pub complete_element_coverage: bool,
    pub complete_part_coverage: bool,
    pub playable_ranges: bool,
    pub polyphony_within_limits: bool,
    pub shareable: bool,
    pub verified: bool,
}

pub fn load(path: &Path) -> Result<ArrangementPlan> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_slice(&bytes).with_context(|| {
        format!(
            "music arrangement plan is not valid YAML: {}",
            path.display()
        )
    })
}

pub fn validate(path: &Path) -> Result<ValidationReport> {
    let manifest = load(path)?;
    if manifest.schema != SCHEMA {
        bail!("music arrangement plan schema must be {SCHEMA}");
    }
    nonempty("arrangement_id", &manifest.arrangement_id)?;
    validate_authority(&manifest.authority)?;
    validate_direction(&manifest.direction)?;
    let (draft_report, music_model) = validate_draft(path, &manifest.model_draft)?;
    let instruments = validate_ensemble(&manifest.ensemble)?;
    let counts = validate_elements(&manifest.element_dispositions, &music_model)?;
    let part_instruments = validate_parts(&manifest.part_assignments, &music_model, &instruments)?;
    validate_notes(
        &manifest.note_mappings,
        &manifest.element_dispositions,
        &music_model,
        &instruments,
        &part_instruments,
    )?;
    validate_candidate_gate(&manifest.candidate_gate)?;
    validate_review(&manifest.review)?;

    Ok(ValidationReport {
        schema: SCHEMA.into(),
        arrangement_id: manifest.arrangement_id.clone(),
        manifest_sha256: sha256_path(path)?,
        contract_sha256: canonical_sha256(&manifest)?,
        model_contract_sha256: draft_report.model_contract_sha256,
        model_targets: manifest.element_dispositions.len(),
        preserved_targets: counts[0],
        developed_targets: counts[1],
        replaced_targets: counts[2],
        omitted_targets: counts[3],
        source_parts: manifest.part_assignments.len(),
        instruments: manifest.ensemble.instruments.len(),
        mapped_notes: manifest.note_mappings.len(),
        candidate_checks: manifest.candidate_gate.required_checks.len(),
        complete_element_coverage: true,
        complete_part_coverage: true,
        playable_ranges: true,
        polyphony_within_limits: true,
        shareable: false,
        verified: true,
    })
}

fn validate_direction(direction: &Direction) -> Result<()> {
    nonempty("direction.label", &direction.label)?;
    nonempty("direction.objective", &direction.objective)?;
    if direction.constraints.is_empty() {
        bail!("direction.constraints must not be empty");
    }
    let mut constraints = BTreeSet::new();
    for constraint in &direction.constraints {
        nonempty("direction.constraints[]", constraint)?;
        if !constraints.insert(constraint) {
            bail!("direction constraints must be unique");
        }
    }
    validate_decision("direction.decision", &direction.decision)
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
        bail!("model draft manifest sha256 does not match arrangement binding");
    }
    let report = model_draft::validate(&draft_path)?;
    if report.contract_sha256 != binding.contract_sha256.to_lowercase()
        || report.draft_id != binding.draft_id
    {
        bail!("model draft contract or identity does not match arrangement binding");
    }
    let draft = model_draft::load(&draft_path)?;
    let model_path = source::resolve(&draft_path, &draft.model.manifest);
    Ok((report, model::load(&model_path)?))
}

fn validate_ensemble(ensemble: &Ensemble) -> Result<BTreeMap<&str, &Instrument>> {
    if ensemble.maximum_instruments == 0
        || ensemble.instruments.is_empty()
        || ensemble.instruments.len() > usize::from(ensemble.maximum_instruments)
    {
        bail!("ensemble must contain one to maximum_instruments instruments");
    }
    let mut instruments = BTreeMap::new();
    for instrument in &ensemble.instruments {
        nonempty("ensemble.instruments[].id", &instrument.id)?;
        nonempty("ensemble.instruments[].family", &instrument.family)?;
        nonempty("ensemble.instruments[].function", &instrument.function)?;
        if instrument.midi_low > instrument.midi_high
            || instrument.maximum_simultaneous_notes == 0
            || instrument.techniques.is_empty()
        {
            bail!("instruments require a valid range, polyphony, and techniques");
        }
        let mut techniques = BTreeSet::new();
        for technique in &instrument.techniques {
            nonempty("ensemble.instruments[].techniques[]", technique)?;
            if !techniques.insert(technique) {
                bail!("instrument techniques must be unique");
            }
        }
        if instruments
            .insert(instrument.id.as_str(), instrument)
            .is_some()
        {
            bail!("instrument ids must be unique");
        }
    }
    Ok(instruments)
}

fn validate_elements(items: &[ElementDisposition], model: &MusicModel) -> Result<[usize; 4]> {
    let expected = model_draft::model_targets(model)?
        .into_keys()
        .collect::<BTreeSet<_>>();
    let actual = items
        .iter()
        .map(|item| item.model_target_id.clone())
        .collect::<BTreeSet<_>>();
    if actual.len() != items.len() || actual != expected {
        bail!("element dispositions must classify every governed model target exactly once");
    }
    let mut counts = [0; 4];
    for item in items {
        nonempty("element_dispositions[].rationale", &item.rationale)?;
        validate_transform_decision(
            "element_dispositions[].decision",
            item.action,
            &item.decision,
        )?;
        counts[action_index(item.action)] += 1;
    }
    Ok(counts)
}

fn validate_parts<'a>(
    assignments: &[PartAssignment],
    model: &MusicModel,
    instruments: &BTreeMap<&'a str, &'a Instrument>,
) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let expected = model
        .parts
        .iter()
        .map(|part| part.id.clone())
        .collect::<BTreeSet<_>>();
    let actual = assignments
        .iter()
        .map(|item| item.source_part_id.clone())
        .collect::<BTreeSet<_>>();
    if actual.len() != assignments.len() || actual != expected {
        bail!("part assignments must cover every model part exactly once");
    }
    let mut output = BTreeMap::new();
    for item in assignments {
        nonempty("part_assignments[].rationale", &item.rationale)?;
        validate_transform_decision("part_assignments[].decision", item.action, &item.decision)?;
        let ids = item.instrument_ids.iter().cloned().collect::<BTreeSet<_>>();
        if ids.len() != item.instrument_ids.len()
            || ids.iter().any(|id| !instruments.contains_key(id.as_str()))
        {
            bail!("part-assignment instruments must be unique and known");
        }
        if item.action == TransformAction::Omit {
            if !ids.is_empty() {
                bail!("omitted parts forbid instrument assignments");
            }
        } else if ids.is_empty() {
            bail!("non-omitted parts require at least one instrument");
        }
        output.insert(item.source_part_id.clone(), ids);
    }
    Ok(output)
}

fn validate_notes(
    mappings: &[NoteMapping],
    dispositions: &[ElementDisposition],
    model: &MusicModel,
    instruments: &BTreeMap<&str, &Instrument>,
    part_instruments: &BTreeMap<String, BTreeSet<String>>,
) -> Result<()> {
    let note_actions = dispositions
        .iter()
        .filter_map(|item| {
            item.model_target_id
                .strip_prefix("note:")
                .map(|id| (id, item.action))
        })
        .collect::<BTreeMap<_, _>>();
    let mut notes = BTreeMap::<&str, (&str, &Note)>::new();
    for part in &model.parts {
        for note in &part.notes {
            notes.insert(note.id.as_str(), (part.id.as_str(), note));
        }
    }
    let expected = note_actions
        .iter()
        .filter(|(_, action)| **action != TransformAction::Omit)
        .map(|(id, _)| *id)
        .collect::<BTreeSet<_>>();
    let actual = mappings
        .iter()
        .map(|mapping| mapping.source_note_id.as_str())
        .collect::<BTreeSet<_>>();
    if actual.len() != mappings.len() || actual != expected {
        bail!("note mappings must cover every non-omitted source note exactly once");
    }
    let mut ids = BTreeSet::new();
    let mut intervals = BTreeMap::<&str, Vec<(u64, u64)>>::new();
    for mapping in mappings {
        nonempty("note_mappings[].id", &mapping.id)?;
        nonempty("note_mappings[].rationale", &mapping.rationale)?;
        if !ids.insert(mapping.id.as_str()) {
            bail!("note mapping ids must be unique");
        }
        let (part_id, source_note) = notes
            .get(mapping.source_note_id.as_str())
            .ok_or_else(|| anyhow::anyhow!("note mapping references unknown source note"))?;
        let expected_action = note_actions[mapping.source_note_id.as_str()];
        if mapping.action != expected_action {
            bail!("note mapping action must equal its element disposition");
        }
        let instrument = instruments
            .get(mapping.instrument_id.as_str())
            .ok_or_else(|| anyhow::anyhow!("note mapping references unknown instrument"))?;
        if !part_instruments[*part_id].contains(&mapping.instrument_id) {
            bail!("note mapping instrument must belong to its source-part assignment");
        }
        let end = mapping
            .start_tick
            .checked_add(mapping.duration_ticks)
            .ok_or_else(|| anyhow::anyhow!("note mapping timing overflows u64"))?;
        if mapping.duration_ticks == 0
            || end > model.duration_ticks
            || mapping.midi_note < instrument.midi_low
            || mapping.midi_note > instrument.midi_high
            || mapping.velocity == 0
            || mapping.velocity > 127
        {
            bail!("note mappings must be timed, playable, and use MIDI velocity 1..127");
        }
        validate_transform_decision(
            "note_mappings[].decision",
            mapping.action,
            &mapping.decision,
        )?;
        if mapping.action == TransformAction::Preserve
            && (mapping.start_tick != source_note.start_tick
                || mapping.duration_ticks != source_note.duration_ticks
                || mapping.midi_note != source_note.midi_note
                || mapping.velocity != source_note.velocity)
        {
            bail!("preserved notes must retain exact timing, pitch, and velocity");
        }
        intervals
            .entry(mapping.instrument_id.as_str())
            .or_default()
            .push((mapping.start_tick, end));
    }
    for (instrument_id, spans) in intervals {
        let limit = usize::from(instruments[instrument_id].maximum_simultaneous_notes);
        for (start, _) in &spans {
            let simultaneous = spans
                .iter()
                .filter(|(other_start, other_end)| other_start <= start && start < other_end)
                .count();
            if simultaneous > limit {
                bail!("note mappings exceed instrument polyphony");
            }
        }
    }
    Ok(())
}

fn validate_candidate_gate(gate: &CandidateGate) -> Result<()> {
    let actual = gate
        .required_checks
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let expected = BTreeSet::from([
        CandidateCheck::ExactPlanBinding,
        CandidateCheck::ModelInheritance,
        CandidateCheck::RangeAndPolyphony,
        CandidateCheck::EditableScoreRoundTrip,
        CandidateCheck::AudibleComparison,
        CandidateCheck::HumanRecognition,
        CandidateCheck::HumanSelection,
    ]);
    if actual.len() != gate.required_checks.len() || actual != expected {
        bail!(
            "candidate gate must declare every required technical, score, listening, and selection check exactly once"
        );
    }
    Ok(())
}

fn action_index(action: TransformAction) -> usize {
    match action {
        TransformAction::Preserve => 0,
        TransformAction::Develop => 1,
        TransformAction::Replace => 2,
        TransformAction::Omit => 3,
    }
}

fn validate_transform_decision(
    field: &str,
    action: TransformAction,
    decision: &Option<DecisionRef>,
) -> Result<()> {
    match (action, decision) {
        (TransformAction::Preserve, None) => Ok(()),
        (TransformAction::Preserve, Some(_)) => {
            bail!("{field} is forbidden when the source element is preserved")
        }
        (_, None) => bail!("{field} is required for develop, replace, or omit"),
        (_, Some(decision)) => validate_decision(field, decision),
    }
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
