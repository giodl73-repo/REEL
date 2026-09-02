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
    language_adaptation::{self, LanguageAdaptation, TextUnit},
    nonempty, repair,
    repair_candidate::{ListeningGate, ListeningStatus, SelectionGate, SelectionStatus},
    source::{self, RawPcmFormat},
    time::AudioTimebase,
    validate_authority, validate_sha256,
};

pub const SCHEMA: &str = "reel.music-language-performance.v0.1";
const REQUIRED_ROLES: &[&str] = &[
    "music-reconstruction-engineer",
    "score-arrangement-director",
    "lyrics-vocal-adaptation-editor",
    "sound-designer",
    "editor",
    "rights-provenance-steward",
    "platform-audience",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LanguagePerformance {
    pub schema: String,
    pub performance_id: String,
    pub adaptation: AdaptationBinding,
    pub vocal_take: PcmBinding,
    pub performed_text: PerformedText,
    pub unit_audit: Vec<UnitAudit>,
    pub lyric_listening: ListeningGate,
    pub provenance: PerformanceProvenance,
    pub consent: ConsentBinding,
    pub comparison: BilingualComparison,
    pub authority: AuthorityRef,
    pub selection: SelectionGate,
    pub review: repair::Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AdaptationBinding {
    pub manifest: PathBuf,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub adaptation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PcmBinding {
    pub path: PathBuf,
    pub sha256: String,
    pub decoded_pcm_sha256: String,
    pub format: RawPcmFormat,
    pub timebase: AudioTimebase,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PerformedText {
    pub language: String,
    pub path: PathBuf,
    pub sha256: String,
    pub authority: AuthorityRef,
    pub units: Vec<TextUnit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UnitAudit {
    pub target_unit_id: String,
    pub performed_unit_ids: Vec<String>,
    pub outcome: UnitAuditOutcome,
    pub rationale: String,
    pub decision: Option<DecisionRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnitAuditOutcome {
    Matched,
    Changed,
    Omitted,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PerformanceProvenance {
    pub method: PerformanceMethod,
    pub adapter_id: String,
    pub adapter_version: String,
    pub model_checkpoint: Option<ModelCheckpoint>,
    pub seed: Option<String>,
    pub creation_egress: CreationEgress,
    pub egress_decision: Option<DecisionRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PerformanceMethod {
    HumanRecorded,
    SyntheticVoice,
    NonIdentifiableFixtureTone,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCheckpoint {
    pub artifact_id: String,
    pub sha256: String,
    pub license: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CreationEgress {
    LocalPrivate,
    ApprovedExternal,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsentBinding {
    pub subject_id: String,
    pub status: ConsentStatus,
    pub operation: String,
    pub service_runtime: String,
    pub audience: String,
    pub retention: String,
    pub reuse_scope: String,
    pub decision: Option<DecisionRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConsentStatus {
    Pending,
    Granted,
    Denied,
    NotApplicableFixture,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BilingualComparison {
    pub source_reference: PcmBinding,
    pub source_authority: AuthorityRef,
    pub source_language: String,
    pub target_language: String,
    pub model_contract_sha256: String,
    pub source_blind_label: String,
    pub target_blind_label: String,
    pub review_dimensions: Vec<ComparisonDimension>,
    pub listening: ListeningGate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComparisonDimension {
    LyricFidelity,
    Prosody,
    CompositionRecognition,
    AccompanimentContinuity,
    MixBalance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub performance_id: String,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub adaptation_contract_sha256: String,
    pub model_contract_sha256: String,
    pub vocal_take_sha256: String,
    pub source_reference_sha256: String,
    pub performed_text_sha256: String,
    pub target_units: usize,
    pub performed_units: usize,
    pub matched_units: usize,
    pub exception_units: usize,
    pub technical_passed: bool,
    pub lyric_listening_passed: bool,
    pub comparison_listening_passed: bool,
    pub consent_satisfied: bool,
    pub eligible_for_selection: bool,
    pub selected: bool,
    pub rejected: bool,
    pub shareable: bool,
    pub verified: bool,
}

pub fn load(path: &Path) -> Result<LanguagePerformance> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_slice(&bytes).with_context(|| {
        format!(
            "music language performance is not valid YAML: {}",
            path.display()
        )
    })
}

pub fn validate(path: &Path) -> Result<ValidationReport> {
    let manifest = load(path)?;
    if manifest.schema != SCHEMA {
        bail!("music language performance schema must be {SCHEMA}");
    }
    nonempty("performance_id", &manifest.performance_id)?;
    validate_authority(&manifest.authority)?;

    let (adaptation_path, adaptation, adaptation_report) =
        validate_adaptation(path, &manifest.adaptation)?;
    let vocal_take_sha256 = validate_pcm(
        path,
        "vocal_take",
        &manifest.vocal_take,
        adaptation.accompaniment.format,
        &adaptation.accompaniment.timebase,
    )?;
    let performed = validate_performed_text(path, &manifest.performed_text)?;
    if manifest.performed_text.language != adaptation.target_text.language {
        bail!("performed text language must equal the approved target language");
    }
    let target = text_units(
        &adaptation_path,
        "adaptation.target_text",
        &adaptation.target_text.path,
        &adaptation.target_text.units,
    )?;
    let (matched_units, exception_units) =
        validate_unit_audit(&manifest.unit_audit, &target, &performed)?;
    validate_listening_gate("lyric_listening", &manifest.lyric_listening)?;
    validate_provenance(&manifest.provenance)?;
    let consent_satisfied = validate_consent(&manifest.consent, manifest.provenance.method)?;
    let source_reference_sha256 = validate_comparison(
        path,
        &manifest.comparison,
        &adaptation,
        &adaptation_report.model_contract_sha256,
    )?;
    validate_review(&manifest.review)?;

    let lyric_passed = manifest.lyric_listening.status == ListeningStatus::Passed;
    let comparison_passed = manifest.comparison.listening.status == ListeningStatus::Passed;
    validate_selection(
        &manifest.selection,
        lyric_passed,
        comparison_passed,
        consent_satisfied,
        &manifest.lyric_listening,
        &manifest.comparison.listening,
        manifest.consent.status,
    )?;
    validate_authority_selection(&manifest.authority, &manifest.selection)?;
    let selected = manifest.selection.status == SelectionStatus::Selected;
    let rejected = manifest.selection.status == SelectionStatus::Rejected;

    Ok(ValidationReport {
        schema: SCHEMA.into(),
        performance_id: manifest.performance_id.clone(),
        manifest_sha256: sha256_path(path)?,
        contract_sha256: canonical_sha256(&manifest)?,
        adaptation_contract_sha256: adaptation_report.contract_sha256,
        model_contract_sha256: adaptation_report.model_contract_sha256,
        vocal_take_sha256,
        source_reference_sha256,
        performed_text_sha256: manifest.performed_text.sha256.to_lowercase(),
        target_units: target.len(),
        performed_units: performed.len(),
        matched_units,
        exception_units,
        technical_passed: true,
        lyric_listening_passed: lyric_passed,
        comparison_listening_passed: comparison_passed,
        consent_satisfied,
        eligible_for_selection: lyric_passed && comparison_passed && consent_satisfied,
        selected,
        rejected,
        shareable: false,
        verified: true,
    })
}

fn validate_adaptation(
    path: &Path,
    binding: &AdaptationBinding,
) -> Result<(
    PathBuf,
    LanguageAdaptation,
    language_adaptation::ValidationReport,
)> {
    validate_sha256("adaptation.manifest_sha256", &binding.manifest_sha256)?;
    validate_sha256("adaptation.contract_sha256", &binding.contract_sha256)?;
    nonempty("adaptation.adaptation_id", &binding.adaptation_id)?;
    let resolved = source::resolve(path, &binding.manifest);
    if sha256_path(&resolved)? != binding.manifest_sha256.to_lowercase() {
        bail!("adaptation manifest sha256 does not match performance binding");
    }
    let report = language_adaptation::validate(&resolved)?;
    if report.contract_sha256 != binding.contract_sha256.to_lowercase()
        || report.adaptation_id != binding.adaptation_id
    {
        bail!("adaptation contract or identity does not match performance binding");
    }
    Ok((
        resolved.clone(),
        language_adaptation::load(&resolved)?,
        report,
    ))
}

fn validate_pcm(
    path: &Path,
    field: &str,
    binding: &PcmBinding,
    expected_format: RawPcmFormat,
    expected_timebase: &AudioTimebase,
) -> Result<String> {
    validate_sha256(&format!("{field}.sha256"), &binding.sha256)?;
    validate_sha256(
        &format!("{field}.decoded_pcm_sha256"),
        &binding.decoded_pcm_sha256,
    )?;
    binding.timebase.validate()?;
    if binding.format != expected_format || &binding.timebase != expected_timebase {
        bail!("{field} must use the exact adaptation accompaniment format and duration");
    }
    let resolved = source::resolve(path, &binding.path);
    let sha256 = sha256_path(&resolved)?;
    if sha256 != binding.sha256.to_lowercase()
        || sha256 != binding.decoded_pcm_sha256.to_lowercase()
    {
        bail!("{field} raw PCM hashes do not match");
    }
    let expected_bytes = binding
        .timebase
        .samples_per_channel
        .checked_mul(u64::from(binding.timebase.channels))
        .and_then(|value| value.checked_mul(binding.format.bytes_per_sample()))
        .ok_or_else(|| anyhow::anyhow!("{field} byte count overflows u64"))?;
    if fs::metadata(resolved)?.len() != expected_bytes {
        bail!("{field} byte count does not match its timebase");
    }
    Ok(sha256)
}

fn validate_performed_text(path: &Path, text: &PerformedText) -> Result<Vec<(String, String)>> {
    nonempty("performed_text.language", &text.language)?;
    validate_sha256("performed_text.sha256", &text.sha256)?;
    validate_authority(&text.authority)?;
    let resolved = source::resolve(path, &text.path);
    if sha256_path(&resolved)? != text.sha256.to_lowercase() {
        bail!("performed text sha256 does not match");
    }
    text_units(path, "performed_text", &text.path, &text.units)
}

fn text_units(
    manifest_path: &Path,
    field: &str,
    path: &Path,
    units: &[TextUnit],
) -> Result<Vec<(String, String)>> {
    let resolved = source::resolve(manifest_path, path);
    let text = fs::read_to_string(resolved)?;
    if units.is_empty() {
        bail!("{field}.units must not be empty");
    }
    let mut ids = BTreeSet::new();
    let mut cursor = 0_usize;
    let mut output = Vec::new();
    for unit in units {
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
            || !text[cursor..start].chars().all(char::is_whitespace)
            || text[start..end].trim().is_empty()
        {
            bail!("{field} units must cover ordered non-whitespace UTF-8 text");
        }
        output.push((unit.id.clone(), text[start..end].to_owned()));
        cursor = end;
    }
    if !text[cursor..].chars().all(char::is_whitespace) {
        bail!("{field} units must cover every non-whitespace character exactly once");
    }
    Ok(output)
}

fn validate_unit_audit(
    audit: &[UnitAudit],
    target: &[(String, String)],
    performed: &[(String, String)],
) -> Result<(usize, usize)> {
    if audit.len() != target.len() {
        bail!("unit audit must cover every approved target unit exactly once");
    }
    let performed_map = performed
        .iter()
        .map(|(id, value)| (id.as_str(), value.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut flattened = Vec::new();
    let mut matched = 0;
    let mut exceptions = 0;
    for (item, (target_id, target_value)) in audit.iter().zip(target) {
        if &item.target_unit_id != target_id {
            bail!("unit audit must follow approved target-unit order");
        }
        nonempty("unit_audit[].rationale", &item.rationale)?;
        let unique = item
            .performed_unit_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if unique.len() != item.performed_unit_ids.len()
            || unique.iter().any(|id| !performed_map.contains_key(id))
        {
            bail!("unit audit performed-unit references must be unique and known");
        }
        flattened.extend(item.performed_unit_ids.iter().cloned());
        match item.outcome {
            UnitAuditOutcome::Matched => {
                if item.performed_unit_ids.len() != 1
                    || performed_map[item.performed_unit_ids[0].as_str()] != target_value
                    || item.decision.is_some()
                {
                    bail!("matched unit audit requires one exact performed unit and no decision");
                }
                matched += 1;
            }
            UnitAuditOutcome::Changed => {
                if item.performed_unit_ids.is_empty() {
                    bail!("changed unit audit requires performed text");
                }
                validate_required_decision("unit_audit[].decision", &item.decision)?;
                exceptions += 1;
            }
            UnitAuditOutcome::Omitted => {
                if !item.performed_unit_ids.is_empty() {
                    bail!("omitted unit audit forbids performed text");
                }
                validate_required_decision("unit_audit[].decision", &item.decision)?;
                exceptions += 1;
            }
            UnitAuditOutcome::Uncertain => {
                validate_required_decision("unit_audit[].decision", &item.decision)?;
                exceptions += 1;
            }
        }
    }
    let expected = performed
        .iter()
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    if flattened != expected {
        bail!("unit audit must cover every performed-text unit exactly once in order");
    }
    Ok((matched, exceptions))
}

fn validate_provenance(provenance: &PerformanceProvenance) -> Result<()> {
    nonempty("provenance.adapter_id", &provenance.adapter_id)?;
    nonempty("provenance.adapter_version", &provenance.adapter_version)?;
    if let Some(checkpoint) = &provenance.model_checkpoint {
        nonempty(
            "provenance.model_checkpoint.artifact_id",
            &checkpoint.artifact_id,
        )?;
        validate_sha256("provenance.model_checkpoint.sha256", &checkpoint.sha256)?;
        nonempty("provenance.model_checkpoint.license", &checkpoint.license)?;
    }
    if provenance.method == PerformanceMethod::SyntheticVoice
        && provenance.model_checkpoint.is_none()
    {
        bail!("synthetic voice provenance requires a model checkpoint and license");
    }
    if provenance
        .seed
        .as_ref()
        .is_some_and(|seed| seed.trim().is_empty())
    {
        bail!("provenance seed must not be empty when present");
    }
    match (provenance.creation_egress, &provenance.egress_decision) {
        (CreationEgress::LocalPrivate, None) => {}
        (CreationEgress::LocalPrivate, Some(_)) => {
            bail!("local-private creation forbids an external-egress decision")
        }
        (CreationEgress::ApprovedExternal, decision) => {
            validate_required_decision("provenance.egress_decision", decision)?;
        }
    }
    Ok(())
}

fn validate_consent(consent: &ConsentBinding, method: PerformanceMethod) -> Result<bool> {
    nonempty("consent.subject_id", &consent.subject_id)?;
    nonempty("consent.operation", &consent.operation)?;
    nonempty("consent.service_runtime", &consent.service_runtime)?;
    nonempty("consent.audience", &consent.audience)?;
    nonempty("consent.retention", &consent.retention)?;
    nonempty("consent.reuse_scope", &consent.reuse_scope)?;
    match consent.status {
        ConsentStatus::Pending => {
            if consent.decision.is_some() {
                bail!("pending consent forbids a decision");
            }
        }
        ConsentStatus::Granted | ConsentStatus::Denied | ConsentStatus::NotApplicableFixture => {
            validate_required_decision("consent.decision", &consent.decision)?;
        }
    }
    if method == PerformanceMethod::NonIdentifiableFixtureTone {
        if consent.status != ConsentStatus::NotApplicableFixture {
            bail!("non-identifiable fixture tone requires not-applicable-fixture consent");
        }
    } else if consent.status == ConsentStatus::NotApplicableFixture {
        bail!("human or synthetic voice performance cannot waive speaker consent");
    }
    Ok(matches!(
        consent.status,
        ConsentStatus::Granted | ConsentStatus::NotApplicableFixture
    ))
}

fn validate_comparison(
    path: &Path,
    comparison: &BilingualComparison,
    adaptation: &LanguageAdaptation,
    model_contract_sha256: &str,
) -> Result<String> {
    validate_authority(&comparison.source_authority)?;
    nonempty("comparison.source_language", &comparison.source_language)?;
    nonempty("comparison.target_language", &comparison.target_language)?;
    nonempty(
        "comparison.source_blind_label",
        &comparison.source_blind_label,
    )?;
    nonempty(
        "comparison.target_blind_label",
        &comparison.target_blind_label,
    )?;
    validate_sha256(
        "comparison.model_contract_sha256",
        &comparison.model_contract_sha256,
    )?;
    if comparison.source_language != adaptation.source_text.language
        || comparison.target_language != adaptation.target_text.language
        || comparison.source_language == comparison.target_language
    {
        bail!("comparison languages must match the distinct adaptation languages");
    }
    if comparison.model_contract_sha256.to_lowercase() != model_contract_sha256.to_lowercase() {
        bail!("comparison model contract must match the recursively checked adaptation model");
    }
    if comparison.source_blind_label == comparison.target_blind_label {
        bail!("comparison blind labels must be distinct");
    }
    let dimensions = comparison
        .review_dimensions
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let required_dimensions = BTreeSet::from([
        ComparisonDimension::LyricFidelity,
        ComparisonDimension::Prosody,
        ComparisonDimension::CompositionRecognition,
        ComparisonDimension::AccompanimentContinuity,
        ComparisonDimension::MixBalance,
    ]);
    if dimensions.len() != comparison.review_dimensions.len() || dimensions != required_dimensions {
        bail!(
            "comparison review dimensions must include each required listening lens exactly once"
        );
    }
    validate_listening_gate("comparison.listening", &comparison.listening)?;
    validate_pcm(
        path,
        "comparison.source_reference",
        &comparison.source_reference,
        adaptation.accompaniment.format,
        &adaptation.accompaniment.timebase,
    )
}

fn validate_authority_selection(authority: &AuthorityRef, selection: &SelectionGate) -> Result<()> {
    let expected = match selection.status {
        SelectionStatus::Pending => "candidate",
        SelectionStatus::Selected => "selected",
        SelectionStatus::Rejected => "rejected",
    };
    if authority.status != expected {
        bail!("performance authority status must be {expected} for its selection state");
    }
    Ok(())
}

fn validate_selection(
    selection: &SelectionGate,
    lyric_passed: bool,
    comparison_passed: bool,
    consent_satisfied: bool,
    lyric: &ListeningGate,
    comparison: &ListeningGate,
    consent: ConsentStatus,
) -> Result<()> {
    validate_gate_decision(
        "selection",
        selection.status == SelectionStatus::Pending,
        &selection.decision,
    )?;
    match selection.status {
        SelectionStatus::Pending => {}
        SelectionStatus::Selected => {
            if !lyric_passed || !comparison_passed || !consent_satisfied {
                bail!("selection requires passed lyric/comparison listening and satisfied consent");
            }
        }
        SelectionStatus::Rejected => {
            if lyric.status == ListeningStatus::Pending
                && comparison.status == ListeningStatus::Pending
                && consent != ConsentStatus::Denied
            {
                bail!("rejection requires completed listening or denied consent");
            }
        }
    }
    Ok(())
}

fn validate_listening_gate(field: &str, gate: &ListeningGate) -> Result<()> {
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
    let decision = decision
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("{field} requires a decision"))?;
    validate_decision(field, decision)
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
    if crate::status_requires_decision(&review.status) && review.decision_refs.is_empty() {
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
