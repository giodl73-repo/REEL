use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    AuthorityRef, DecisionRef,
    hash::{canonical_sha256, sha256_path},
    model, model_draft, nonempty, repair, source, status_requires_decision, validate_authority,
    validate_sha256,
};

pub const SCHEMA: &str = "reel.music-repair-intent.v0.1";
const REQUIRED_ROLES: &[&str] = &[
    "music-reconstruction-engineer",
    "sound-designer",
    "editor",
    "rights-provenance-steward",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepairIntentManifest {
    pub schema: String,
    pub intent_id: String,
    pub model_draft: DraftBinding,
    pub repair: RepairBinding,
    pub authority: AuthorityRef,
    pub intents: Vec<RepairIntent>,
    pub candidate_gate: CandidateGate,
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
pub struct RepairBinding {
    pub manifest: PathBuf,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub repair_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepairIntent {
    pub id: String,
    pub objective: RepairObjective,
    pub operation_ids: Vec<String>,
    pub model_target_refs: Vec<String>,
    pub rationale: String,
    pub decision: DecisionRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepairObjective {
    RemoveMistake,
    Timing,
    Pitch,
    Structure,
    Noise,
    Performance,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateGate {
    pub required_checks: Vec<CandidateCheck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateCheck {
    OutsideRegionsExact,
    BoundaryContinuity,
    RightTailIdentity,
    OutputDuration,
    HumanListening,
    HumanSelection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub intent_id: String,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub model_contract_sha256: String,
    pub repair_contract_sha256: String,
    pub intents: usize,
    pub mutating_operations: usize,
    pub model_targets: usize,
    pub candidate_checks: usize,
    pub complete_operation_coverage: bool,
    pub source_lineage_matches: bool,
    pub shareable: bool,
    pub verified: bool,
}

pub fn load(path: &Path) -> Result<RepairIntentManifest> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_slice(&bytes)
        .with_context(|| format!("music repair intent is not valid YAML: {}", path.display()))
}

pub fn validate(path: &Path) -> Result<ValidationReport> {
    let manifest = load(path)?;
    if manifest.schema != SCHEMA {
        bail!("music repair intent schema must be {SCHEMA}");
    }
    nonempty("intent_id", &manifest.intent_id)?;
    validate_authority(&manifest.authority)?;

    validate_sha256(
        "model_draft.manifest_sha256",
        &manifest.model_draft.manifest_sha256,
    )?;
    validate_sha256(
        "model_draft.contract_sha256",
        &manifest.model_draft.contract_sha256,
    )?;
    nonempty("model_draft.draft_id", &manifest.model_draft.draft_id)?;
    let draft_path = source::resolve(path, &manifest.model_draft.manifest);
    if sha256_path(&draft_path)? != manifest.model_draft.manifest_sha256.to_lowercase() {
        bail!("model draft manifest sha256 does not match intent binding");
    }
    let draft_report = model_draft::validate(&draft_path)?;
    if draft_report.contract_sha256 != manifest.model_draft.contract_sha256.to_lowercase()
        || draft_report.draft_id != manifest.model_draft.draft_id
    {
        bail!("model draft contract or identity does not match intent binding");
    }
    let draft = model_draft::load(&draft_path)?;
    let model_path = source::resolve(&draft_path, &draft.model.manifest);
    let music_model = model::load(&model_path)?;
    let model_targets = model_draft::model_targets(&music_model)?;

    validate_sha256("repair.manifest_sha256", &manifest.repair.manifest_sha256)?;
    validate_sha256("repair.contract_sha256", &manifest.repair.contract_sha256)?;
    nonempty("repair.repair_id", &manifest.repair.repair_id)?;
    let repair_path = source::resolve(path, &manifest.repair.manifest);
    if sha256_path(&repair_path)? != manifest.repair.manifest_sha256.to_lowercase() {
        bail!("repair manifest sha256 does not match intent binding");
    }
    let repair_report = repair::validate(&repair_path)?;
    let repair_manifest = repair::load(&repair_path)?;
    if repair_report.contract_sha256 != manifest.repair.contract_sha256.to_lowercase()
        || repair_report.repair_id != manifest.repair.repair_id
    {
        bail!("repair contract or identity does not match intent binding");
    }
    if music_model.source.contract_sha256 != repair_report.source_contract_sha256
        || music_model.source.decoded_pcm_sha256 != repair_manifest.decoded_pcm_sha256
        || music_model.source.manifest_sha256 != repair_manifest.source.sha256
    {
        bail!("model and repair must bind the same immutable source lineage");
    }

    let mut mutating = BTreeSet::new();
    for operation in &repair_manifest.operations {
        if !matches!(
            operation,
            repair::Operation::Keep { .. } | repair::Operation::Lock { .. }
        ) {
            mutating.insert(operation.id().to_string());
        }
    }
    let mut intent_ids = BTreeSet::new();
    let mut linked_operations = BTreeSet::new();
    let mut linked_targets = BTreeSet::new();
    if manifest.intents.is_empty() {
        bail!("intents must not be empty");
    }
    for intent in &manifest.intents {
        nonempty("intents[].id", &intent.id)?;
        if !intent_ids.insert(intent.id.as_str()) {
            bail!("intent ids must be unique");
        }
        nonempty("intents[].rationale", &intent.rationale)?;
        validate_decision(&intent.decision)?;
        if intent.operation_ids.is_empty() || intent.model_target_refs.is_empty() {
            bail!("each intent requires operation_ids and model_target_refs");
        }
        let mut local_operations = BTreeSet::new();
        for operation_id in &intent.operation_ids {
            nonempty("intents[].operation_ids[]", operation_id)?;
            if !local_operations.insert(operation_id.as_str()) {
                bail!("operation ids must be unique within an intent");
            }
            if !mutating.contains(operation_id) {
                bail!("intent references unknown or non-mutating operation {operation_id}");
            }
            if !linked_operations.insert(operation_id.as_str()) {
                bail!("mutating operation {operation_id} may have only one repair intent");
            }
        }
        let mut local_targets = BTreeSet::new();
        for target_ref in &intent.model_target_refs {
            nonempty("intents[].model_target_refs[]", target_ref)?;
            if !local_targets.insert(target_ref.as_str()) {
                bail!("model target refs must be unique within an intent");
            }
            if !model_targets.contains_key(target_ref.as_str()) {
                bail!("intent references unknown model target {target_ref}");
            }
            linked_targets.insert(target_ref.as_str());
        }
    }
    if linked_operations.len() != mutating.len() {
        bail!("repair intents must cover every mutating operation exactly once");
    }

    let required_checks = BTreeSet::from([
        CandidateCheck::OutsideRegionsExact,
        CandidateCheck::BoundaryContinuity,
        CandidateCheck::RightTailIdentity,
        CandidateCheck::OutputDuration,
        CandidateCheck::HumanListening,
        CandidateCheck::HumanSelection,
    ]);
    let declared_checks = manifest
        .candidate_gate
        .required_checks
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if declared_checks.len() != manifest.candidate_gate.required_checks.len()
        || declared_checks != required_checks
    {
        bail!(
            "candidate_gate.required_checks must declare each technical, listening, and selection gate exactly once"
        );
    }
    validate_review(&manifest.review)?;

    Ok(ValidationReport {
        schema: SCHEMA.into(),
        intent_id: manifest.intent_id.clone(),
        manifest_sha256: sha256_path(path)?,
        contract_sha256: canonical_sha256(&manifest)?,
        model_contract_sha256: draft_report.model_contract_sha256,
        repair_contract_sha256: repair_report.contract_sha256,
        intents: manifest.intents.len(),
        mutating_operations: mutating.len(),
        model_targets: linked_targets.len(),
        candidate_checks: declared_checks.len(),
        complete_operation_coverage: true,
        source_lineage_matches: true,
        shareable: false,
        verified: true,
    })
}

fn validate_decision(decision: &DecisionRef) -> Result<()> {
    nonempty("intents[].decision.artifact_id", &decision.artifact_id)?;
    validate_sha256("intents[].decision.sha256", &decision.sha256)
}

fn validate_review(review: &repair::Review) -> Result<()> {
    nonempty("review.status", &review.status)?;
    let declared = review
        .required_roles
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if declared.len() != review.required_roles.len() {
        bail!("review.required_roles must be unique");
    }
    for role in REQUIRED_ROLES {
        if !declared.contains(role) {
            bail!("review.required_roles must include {role}");
        }
    }
    if status_requires_decision(&review.status) && review.decision_refs.is_empty() {
        bail!("review status {} requires decision_refs", review.status);
    }
    let mut decision_ids = BTreeSet::new();
    for decision in &review.decision_refs {
        nonempty("review.decision_refs[].artifact_id", &decision.artifact_id)?;
        validate_sha256("review.decision_refs[].sha256", &decision.sha256)?;
        if !decision_ids.insert(decision.artifact_id.as_str()) {
            bail!("review.decision_refs artifact ids must be unique");
        }
    }
    Ok(())
}
