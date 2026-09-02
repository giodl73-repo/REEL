use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use reel_music::{
    AuthorityRef, DecisionRef,
    arrangement_plan::{self, ArrangementPlan, TransformAction},
    hash::{canonical_sha256, sha256_path},
    model::{self, MusicModel, PartRole},
    model_draft, repair,
    repair_candidate::{ListeningGate, ListeningStatus, SelectionGate, SelectionStatus},
};
use serde::{Deserialize, Serialize};

use crate::music_score::{self, ExportReceipt};

pub const SCHEMA: &str = "reel.music-arrangement-candidate.v0.1";
const REQUIRED_ROLES: &[&str] = &[
    "music-reconstruction-engineer",
    "score-arrangement-director",
    "sound-designer",
    "editor",
    "rights-provenance-steward",
    "platform-audience",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArrangementCandidate {
    pub schema: String,
    pub candidate_id: String,
    pub arrangement: ArrangementBinding,
    pub arranged_model: ModelBinding,
    pub score_export: ScoreExportBinding,
    pub arrangement_audio: ArtifactBinding,
    pub creation: CreationProvenance,
    pub comparison: AudibleComparison,
    pub recognition: RecognitionGate,
    pub authority: AuthorityRef,
    pub selection: SelectionGate,
    pub review: repair::Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArrangementBinding {
    pub manifest: PathBuf,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub arrangement_id: String,
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
pub struct ScoreExportBinding {
    pub plan: PathBuf,
    pub plan_sha256: String,
    pub plan_contract_sha256: String,
    pub receipt: PathBuf,
    pub receipt_sha256: String,
    pub packet_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactBinding {
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreationProvenance {
    pub adapter_id: String,
    pub adapter_version: String,
    pub network_policy: NetworkPolicy,
    pub egress_decision: Option<DecisionRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkPolicy {
    LocalOnly,
    ApprovedExternal,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudibleComparison {
    pub source_reference: ArtifactBinding,
    pub source_authority: AuthorityRef,
    pub source_blind_label: String,
    pub candidate_blind_label: String,
    pub review_dimensions: Vec<ComparisonDimension>,
    pub listening: ListeningGate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComparisonDimension {
    Form,
    Pulse,
    Melody,
    Harmony,
    Hooks,
    EmotionalArc,
    Instrumentation,
    MixBalance,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecognitionGate {
    pub status: RecognitionStatus,
    pub decision: Option<DecisionRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecognitionStatus {
    Pending,
    Recognized,
    NotRecognized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub candidate_id: String,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub arrangement_contract_sha256: String,
    pub model_contract_sha256: String,
    pub score_receipt_sha256: String,
    pub arrangement_audio_sha256: String,
    pub source_reference_sha256: String,
    pub mapped_notes: usize,
    pub arranged_parts: usize,
    pub midi_round_trip: bool,
    pub musicxml_round_trip: bool,
    pub audible_round_trip: bool,
    pub listening_complete: bool,
    pub listening_passed: bool,
    pub recognition_complete: bool,
    pub recognized: bool,
    pub eligible_for_selection: bool,
    pub selected: bool,
    pub rejected: bool,
    pub shareable: bool,
    pub verified: bool,
}

pub fn load(path: &Path) -> Result<ArrangementCandidate> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_slice(&bytes).with_context(|| {
        format!(
            "music arrangement candidate is not valid YAML: {}",
            path.display()
        )
    })
}

pub fn validate(path: &Path) -> Result<ValidationReport> {
    let manifest = load(path)?;
    if manifest.schema != SCHEMA {
        bail!("music arrangement candidate schema must be {SCHEMA}");
    }
    nonempty("candidate_id", &manifest.candidate_id)?;
    validate_authority("authority", &manifest.authority)?;

    let arrangement_path = resolve(path, &manifest.arrangement.manifest);
    validate_binding_hashes(
        "arrangement",
        &arrangement_path,
        &manifest.arrangement.manifest_sha256,
        &manifest.arrangement.contract_sha256,
    )?;
    nonempty(
        "arrangement.arrangement_id",
        &manifest.arrangement.arrangement_id,
    )?;
    let arrangement_report = arrangement_plan::validate(&arrangement_path)?;
    if arrangement_report.contract_sha256 != manifest.arrangement.contract_sha256.to_lowercase()
        || arrangement_report.arrangement_id != manifest.arrangement.arrangement_id
    {
        bail!("arrangement plan contract or identity does not match candidate binding");
    }
    let arrangement = arrangement_plan::load(&arrangement_path)?;

    let model_path = resolve(path, &manifest.arranged_model.manifest);
    validate_binding_hashes(
        "arranged_model",
        &model_path,
        &manifest.arranged_model.manifest_sha256,
        &manifest.arranged_model.contract_sha256,
    )?;
    nonempty("arranged_model.model_id", &manifest.arranged_model.model_id)?;
    let model_report = model::validate(&model_path)?;
    if model_report.contract_sha256 != manifest.arranged_model.contract_sha256.to_lowercase()
        || model_report.model_id != manifest.arranged_model.model_id
    {
        bail!("arranged model contract or identity does not match candidate binding");
    }
    let arranged_model = model::load(&model_path)?;
    let source_model = load_source_model(&arrangement_path, &arrangement)?;
    validate_model_inheritance(&arrangement, &source_model, &arranged_model)?;

    let (score_receipt_sha256, score_receipt) = validate_score_export(
        path,
        &manifest.score_export,
        &model_path,
        &model_report.contract_sha256,
    )?;
    let arrangement_audio_sha256 =
        validate_artifact(path, "arrangement_audio", &manifest.arrangement_audio)?;
    let guide = score_receipt
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == "rehearsal-guide-wav")
        .ok_or_else(|| anyhow!("score export receipt lacks its audible rehearsal guide"))?;
    if arrangement_audio_sha256 != guide.sha256.to_lowercase() {
        bail!("arrangement audio must be the exact score-export audible round trip");
    }
    validate_creation(&manifest.creation, guide)?;
    let source_reference_sha256 = validate_comparison(path, &manifest.comparison)?;
    validate_recognition(&manifest.recognition, &manifest.comparison.listening)?;
    validate_selection(
        &manifest.selection,
        &manifest.comparison.listening,
        &manifest.recognition,
    )?;
    validate_authority_selection(&manifest.authority, &manifest.selection)?;
    validate_review(&manifest.review)?;

    let listening_complete = manifest.comparison.listening.status != ListeningStatus::Pending;
    let listening_passed = manifest.comparison.listening.status == ListeningStatus::Passed;
    let recognition_complete = manifest.recognition.status != RecognitionStatus::Pending;
    let recognized = manifest.recognition.status == RecognitionStatus::Recognized;
    let selected = manifest.selection.status == SelectionStatus::Selected;
    let rejected = manifest.selection.status == SelectionStatus::Rejected;
    let contract_sha256 = canonical_sha256(&manifest)?;
    Ok(ValidationReport {
        schema: SCHEMA.into(),
        candidate_id: manifest.candidate_id.clone(),
        manifest_sha256: sha256_path(path)?,
        contract_sha256,
        arrangement_contract_sha256: arrangement_report.contract_sha256,
        model_contract_sha256: model_report.contract_sha256,
        score_receipt_sha256,
        arrangement_audio_sha256,
        source_reference_sha256,
        mapped_notes: arrangement.note_mappings.len(),
        arranged_parts: arranged_model.parts.len(),
        midi_round_trip: true,
        musicxml_round_trip: true,
        audible_round_trip: true,
        listening_complete,
        listening_passed,
        recognition_complete,
        recognized,
        eligible_for_selection: listening_passed && recognized,
        selected,
        rejected,
        shareable: false,
        verified: true,
    })
}

fn load_source_model(path: &Path, arrangement: &ArrangementPlan) -> Result<MusicModel> {
    let draft_path = resolve(path, &arrangement.model_draft.manifest);
    let draft = model_draft::load(&draft_path)?;
    model::load(&resolve(&draft_path, &draft.model.manifest))
}

fn validate_model_inheritance(
    arrangement: &ArrangementPlan,
    source_model: &MusicModel,
    actual: &MusicModel,
) -> Result<()> {
    let source_binding_equal = source_model.source.manifest_sha256 == actual.source.manifest_sha256
        && source_model.source.contract_sha256 == actual.source.contract_sha256
        && source_model.source.decoded_pcm_sha256 == actual.source.decoded_pcm_sha256;
    let analysis_bindings_equal = source_model.analyses.len() == actual.analyses.len()
        && source_model
            .analyses
            .iter()
            .zip(&actual.analyses)
            .all(|(left, right)| {
                left.manifest_sha256 == right.manifest_sha256
                    && left.contract_sha256 == right.contract_sha256
                    && left.analysis_id == right.analysis_id
            });
    if !source_binding_equal
        || !analysis_bindings_equal
        || source_model.musical_timebase != actual.musical_timebase
        || source_model.duration_ticks != actual.duration_ticks
        || source_model.tempo_map != actual.tempo_map
        || source_model.meter_map != actual.meter_map
        || source_model.form != actual.form
        || source_model.harmony != actual.harmony
        || source_model.rhythm_cells != actual.rhythm_cells
        || source_model.hooks != actual.hooks
        || source_model.lyric_layers != actual.lyric_layers
        || source_model.expressive_timing != actual.expressive_timing
        || source_model.unknowns != actual.unknowns
    {
        bail!("arranged model must preserve every non-note model layer exactly in v0.1");
    }
    if arrangement.element_dispositions.iter().any(|item| {
        !item.model_target_id.starts_with("note:") && item.action != TransformAction::Preserve
    }) {
        bail!("v0.1 arrangement candidates require preserved non-note model dispositions");
    }
    let instruments = arrangement
        .ensemble
        .instruments
        .iter()
        .map(|item| item.id.as_str())
        .collect::<BTreeSet<_>>();
    let expected_parts = arrangement
        .note_mappings
        .iter()
        .map(|item| item.instrument_id.as_str())
        .collect::<BTreeSet<_>>();
    let actual_parts = actual
        .parts
        .iter()
        .map(|part| part.id.as_str())
        .collect::<BTreeSet<_>>();
    if actual_parts.len() != actual.parts.len()
        || actual_parts != expected_parts
        || actual_parts.iter().any(|id| !instruments.contains(id))
    {
        bail!("arranged model parts must equal the instruments used by note mappings");
    }
    let mut source_notes = BTreeMap::new();
    for part in &source_model.parts {
        for note in &part.notes {
            source_notes.insert(note.id.as_str(), (part.role, note));
        }
    }
    let mut expected = BTreeMap::new();
    for mapping in &arrangement.note_mappings {
        let (role, source_note) = source_notes
            .get(mapping.source_note_id.as_str())
            .ok_or_else(|| anyhow!("arrangement mapping references an unknown source note"))?;
        if expected
            .insert(
                mapping.id.as_str(),
                (
                    mapping.instrument_id.as_str(),
                    *role,
                    source_note.voice,
                    mapping.start_tick,
                    mapping.duration_ticks,
                    mapping.midi_note,
                    mapping.velocity,
                ),
            )
            .is_some()
        {
            bail!("arrangement mapping ids must be unique in the candidate score");
        }
    }
    let mut seen = BTreeSet::new();
    for part in &actual.parts {
        let roles = expected
            .values()
            .filter(|entry| entry.0 == part.id)
            .map(|entry| entry.1)
            .fold(Vec::new(), |mut roles, role| {
                if !roles.contains(&role) {
                    roles.push(role);
                }
                roles
            });
        let expected_role = if roles.len() == 1 {
            *roles.first().expect("one role")
        } else {
            PartRole::Other
        };
        if part.role != expected_role {
            bail!("arranged model part role does not match its mapped source roles");
        }
        for note in &part.notes {
            let item = expected
                .get(note.id.as_str())
                .ok_or_else(|| anyhow!("arranged model contains an unmapped note"))?;
            if !seen.insert(note.id.as_str())
                || item.0 != part.id
                || item.2 != note.voice
                || item.3 != note.start_tick
                || item.4 != note.duration_ticks
                || item.5 != note.midi_note
                || item.6 != note.velocity
            {
                bail!("arranged model note does not match its exact plan mapping");
            }
        }
    }
    if seen.len() != expected.len() {
        bail!("arranged model must materialize every mapped note exactly once");
    }
    Ok(())
}

fn validate_score_export(
    manifest_path: &Path,
    binding: &ScoreExportBinding,
    model_path: &Path,
    model_contract_sha256: &str,
) -> Result<(String, ExportReceipt)> {
    validate_sha256("score_export.plan_sha256", &binding.plan_sha256)?;
    validate_sha256(
        "score_export.plan_contract_sha256",
        &binding.plan_contract_sha256,
    )?;
    validate_sha256("score_export.receipt_sha256", &binding.receipt_sha256)?;
    let plan_path = resolve(manifest_path, &binding.plan);
    let receipt_path = resolve(manifest_path, &binding.receipt);
    let packet_dir = resolve(manifest_path, &binding.packet_dir);
    if sha256_path(&plan_path)? != binding.plan_sha256.to_lowercase()
        || sha256_path(&receipt_path)? != binding.receipt_sha256.to_lowercase()
    {
        bail!("score export plan or receipt sha256 does not match candidate binding");
    }
    let plan = reel_music::export::load(&plan_path)?;
    if canonical_sha256(&plan)? != binding.plan_contract_sha256.to_lowercase()
        || plan.model_contract_sha256 != model_contract_sha256
    {
        bail!("score export plan does not bind the exact arranged model contract");
    }
    let report = music_score::check(&receipt_path, &plan_path, model_path, &packet_dir)?;
    if !report.midi_round_trip || !report.musicxml_round_trip || !report.rehearsal_guide_valid {
        bail!("arrangement score export must pass MIDI, MusicXML, and audible round trips");
    }
    let receipt: ExportReceipt = serde_json::from_slice(&fs::read(&receipt_path)?)?;
    if receipt.model_contract_sha256 != model_contract_sha256 {
        bail!("score export receipt does not bind the exact arranged model contract");
    }
    Ok((report.receipt_sha256, receipt))
}

fn validate_creation(
    creation: &CreationProvenance,
    guide: &music_score::ArtifactReceipt,
) -> Result<()> {
    nonempty("creation.adapter_id", &creation.adapter_id)?;
    nonempty("creation.adapter_version", &creation.adapter_version)?;
    if creation.adapter_id != guide.adapter || creation.adapter_version != guide.adapter_version {
        bail!("creation provenance must equal the score-export audible adapter");
    }
    match (creation.network_policy, &creation.egress_decision) {
        (NetworkPolicy::LocalOnly, None) => Ok(()),
        (NetworkPolicy::LocalOnly, Some(_)) => {
            bail!("local-only creation forbids an egress decision")
        }
        (NetworkPolicy::ApprovedExternal, decision) => {
            validate_required_decision("creation.egress_decision", decision)
        }
    }
}

fn validate_comparison(path: &Path, comparison: &AudibleComparison) -> Result<String> {
    let sha = validate_artifact(
        path,
        "comparison.source_reference",
        &comparison.source_reference,
    )?;
    validate_authority("comparison.source_authority", &comparison.source_authority)?;
    nonempty(
        "comparison.source_blind_label",
        &comparison.source_blind_label,
    )?;
    nonempty(
        "comparison.candidate_blind_label",
        &comparison.candidate_blind_label,
    )?;
    if comparison.source_blind_label == comparison.candidate_blind_label {
        bail!("comparison blind labels must be distinct");
    }
    let actual = comparison
        .review_dimensions
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let expected = BTreeSet::from([
        ComparisonDimension::Form,
        ComparisonDimension::Pulse,
        ComparisonDimension::Melody,
        ComparisonDimension::Harmony,
        ComparisonDimension::Hooks,
        ComparisonDimension::EmotionalArc,
        ComparisonDimension::Instrumentation,
        ComparisonDimension::MixBalance,
    ]);
    if actual.len() != comparison.review_dimensions.len() || actual != expected {
        bail!("audible comparison must declare every required review dimension exactly once");
    }
    validate_listening("comparison.listening", &comparison.listening)?;
    Ok(sha)
}

fn validate_recognition(gate: &RecognitionGate, listening: &ListeningGate) -> Result<()> {
    validate_gate_decision(
        "recognition",
        gate.status == RecognitionStatus::Pending,
        &gate.decision,
    )?;
    if gate.status != RecognitionStatus::Pending && listening.status == ListeningStatus::Pending {
        bail!("completed recognition requires completed audible comparison listening");
    }
    if gate.status == RecognitionStatus::Recognized && listening.status != ListeningStatus::Passed {
        bail!("recognized status requires passed audible comparison listening");
    }
    Ok(())
}

fn validate_selection(
    gate: &SelectionGate,
    listening: &ListeningGate,
    recognition: &RecognitionGate,
) -> Result<()> {
    validate_gate_decision(
        "selection",
        gate.status == SelectionStatus::Pending,
        &gate.decision,
    )?;
    match gate.status {
        SelectionStatus::Pending => Ok(()),
        SelectionStatus::Selected
            if listening.status == ListeningStatus::Passed
                && recognition.status == RecognitionStatus::Recognized =>
        {
            Ok(())
        }
        SelectionStatus::Selected => {
            bail!("selection requires passed listening and human recognition")
        }
        SelectionStatus::Rejected
            if listening.status != ListeningStatus::Pending
                || recognition.status != RecognitionStatus::Pending =>
        {
            Ok(())
        }
        SelectionStatus::Rejected => {
            bail!("rejection requires completed listening or recognition evidence")
        }
    }
}

fn validate_authority_selection(authority: &AuthorityRef, selection: &SelectionGate) -> Result<()> {
    let expected = match selection.status {
        SelectionStatus::Pending => "candidate",
        SelectionStatus::Selected => "selected",
        SelectionStatus::Rejected => "rejected",
    };
    if authority.status != expected {
        bail!("arrangement candidate authority status must be {expected}");
    }
    Ok(())
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

fn validate_binding_hashes(
    field: &str,
    path: &Path,
    manifest_sha256: &str,
    contract_sha256: &str,
) -> Result<()> {
    validate_sha256(&format!("{field}.manifest_sha256"), manifest_sha256)?;
    validate_sha256(&format!("{field}.contract_sha256"), contract_sha256)?;
    if sha256_path(path)? != manifest_sha256.to_lowercase() {
        bail!("{field} manifest sha256 does not match candidate binding");
    }
    Ok(())
}

fn validate_artifact(path: &Path, field: &str, binding: &ArtifactBinding) -> Result<String> {
    validate_sha256(&format!("{field}.sha256"), &binding.sha256)?;
    let actual = sha256_path(&resolve(path, &binding.path))?;
    if actual != binding.sha256.to_lowercase() {
        bail!("{field} sha256 does not match candidate binding");
    }
    Ok(actual)
}

fn validate_listening(field: &str, gate: &ListeningGate) -> Result<()> {
    validate_gate_decision(
        field,
        gate.status == ListeningStatus::Pending,
        &gate.decision,
    )
}

fn validate_gate_decision(
    field: &str,
    pending: bool,
    decision: &Option<DecisionRef>,
) -> Result<()> {
    match (pending, decision) {
        (true, None) => Ok(()),
        (true, Some(_)) => bail!("{field} pending status forbids a decision"),
        (false, None) => bail!("{field} completed status requires a decision"),
        (false, Some(decision)) => validate_decision(field, decision),
    }
}

fn validate_required_decision(field: &str, decision: &Option<DecisionRef>) -> Result<()> {
    decision
        .as_ref()
        .ok_or_else(|| anyhow!("{field} requires a decision"))
        .and_then(|decision| validate_decision(field, decision))
}

fn validate_authority(field: &str, authority: &AuthorityRef) -> Result<()> {
    nonempty(&format!("{field}.namespace"), &authority.namespace)?;
    nonempty(&format!("{field}.artifact_id"), &authority.artifact_id)?;
    validate_sha256(
        &format!("{field}.content_sha256"),
        &authority.content_sha256,
    )?;
    nonempty(&format!("{field}.status"), &authority.status)?;
    let roles = authority.required_roles.iter().collect::<BTreeSet<_>>();
    if roles.is_empty() || roles.len() != authority.required_roles.len() {
        bail!("{field}.required_roles must be non-empty and unique");
    }
    for role in &authority.required_roles {
        nonempty(&format!("{field}.required_roles[]"), role)?;
    }
    if status_requires_decision(&authority.status) && authority.decision_refs.is_empty() {
        bail!("{field}.status {} requires decisions", authority.status);
    }
    for decision in &authority.decision_refs {
        validate_decision(&format!("{field}.decision_refs[]"), decision)?;
    }
    Ok(())
}

fn validate_decision(field: &str, decision: &DecisionRef) -> Result<()> {
    nonempty(&format!("{field}.artifact_id"), &decision.artifact_id)?;
    validate_sha256(&format!("{field}.sha256"), &decision.sha256)
}
fn validate_sha256(field: &str, value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{field} must be a 64-character SHA-256 value");
    }
    Ok(())
}
fn nonempty(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(())
}
fn status_requires_decision(status: &str) -> bool {
    matches!(status, "reviewed" | "approved" | "selected" | "released")
}

fn resolve(manifest_path: &Path, value: &Path) -> PathBuf {
    if value.is_absolute() {
        value.to_path_buf()
    } else {
        manifest_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(value)
    }
}
