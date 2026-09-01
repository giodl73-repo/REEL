use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    AuthorityRef, DecisionRef, evidence,
    hash::{canonical_sha256, sha256_path},
    nonempty, repair, repair_intent, source, status_requires_decision, validate_authority,
    validate_sha256,
};

pub const SCHEMA: &str = "reel.music-repair-candidate.v0.1";
const REQUIRED_ROLES: &[&str] = &[
    "music-reconstruction-engineer",
    "sound-designer",
    "editor",
    "rights-provenance-steward",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepairCandidateManifest {
    pub schema: String,
    pub candidate_id: String,
    pub intent: IntentBinding,
    pub candidate_pcm: CandidateBinding,
    pub evidence: EvidenceBinding,
    pub authority: AuthorityRef,
    pub listening: ListeningGate,
    pub selection: SelectionGate,
    pub review: repair::Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IntentBinding {
    pub manifest: PathBuf,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub intent_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateBinding {
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceBinding {
    pub manifest: PathBuf,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub edl: PathBuf,
    pub repair: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ListeningGate {
    pub status: ListeningStatus,
    pub decision: Option<DecisionRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ListeningStatus {
    Pending,
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionGate {
    pub status: SelectionStatus,
    pub decision: Option<DecisionRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectionStatus {
    Pending,
    Selected,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub candidate_id: String,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub intent_contract_sha256: String,
    pub evidence_contract_sha256: String,
    pub candidate_pcm_sha256: String,
    pub technical_passed: bool,
    pub listening_complete: bool,
    pub listening_passed: bool,
    pub eligible_for_selection: bool,
    pub selected: bool,
    pub rejected: bool,
    pub shareable: bool,
    pub verified: bool,
}

pub fn load(path: &Path) -> Result<RepairCandidateManifest> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_slice(&bytes).with_context(|| {
        format!(
            "music repair candidate is not valid YAML: {}",
            path.display()
        )
    })
}

pub fn validate(path: &Path) -> Result<ValidationReport> {
    let manifest = load(path)?;
    if manifest.schema != SCHEMA {
        bail!("music repair candidate schema must be {SCHEMA}");
    }
    nonempty("candidate_id", &manifest.candidate_id)?;
    validate_authority(&manifest.authority)?;

    validate_sha256("intent.manifest_sha256", &manifest.intent.manifest_sha256)?;
    validate_sha256("intent.contract_sha256", &manifest.intent.contract_sha256)?;
    nonempty("intent.intent_id", &manifest.intent.intent_id)?;
    let intent_path = source::resolve(path, &manifest.intent.manifest);
    if sha256_path(&intent_path)? != manifest.intent.manifest_sha256.to_lowercase() {
        bail!("repair intent manifest sha256 does not match candidate binding");
    }
    let intent_report = repair_intent::validate(&intent_path)?;
    let intent = repair_intent::load(&intent_path)?;
    if intent_report.contract_sha256 != manifest.intent.contract_sha256.to_lowercase()
        || intent_report.intent_id != manifest.intent.intent_id
    {
        bail!("repair intent contract or identity does not match candidate binding");
    }

    validate_sha256("candidate_pcm.sha256", &manifest.candidate_pcm.sha256)?;
    let candidate_path = source::resolve(path, &manifest.candidate_pcm.path);
    let candidate_sha256 = sha256_path(&candidate_path)?;
    if candidate_sha256 != manifest.candidate_pcm.sha256.to_lowercase() {
        bail!("candidate PCM sha256 does not match candidate binding");
    }

    validate_sha256(
        "evidence.manifest_sha256",
        &manifest.evidence.manifest_sha256,
    )?;
    validate_sha256(
        "evidence.contract_sha256",
        &manifest.evidence.contract_sha256,
    )?;
    let evidence_path = source::resolve(path, &manifest.evidence.manifest);
    let edl_path = source::resolve(path, &manifest.evidence.edl);
    let repair_path = source::resolve(path, &manifest.evidence.repair);
    if sha256_path(&evidence_path)? != manifest.evidence.manifest_sha256.to_lowercase() {
        bail!("repair evidence manifest sha256 does not match candidate binding");
    }
    if sha256_path(&repair_path)? != intent.repair.manifest_sha256.to_lowercase() {
        bail!("candidate evidence repair does not match the governed intent repair");
    }
    let repair_report = repair::validate(&repair_path)?;
    if repair_report.contract_sha256 != intent.repair.contract_sha256.to_lowercase()
        || repair_report.repair_id != intent.repair.repair_id
    {
        bail!("candidate evidence repair contract or identity does not match intent");
    }
    let checked = evidence::check(&evidence_path, &edl_path, &repair_path, &candidate_path)?;
    if checked.evidence_sha256 != manifest.evidence.manifest_sha256.to_lowercase()
        || checked.evidence_contract_sha256 != manifest.evidence.contract_sha256.to_lowercase()
        || checked.candidate_pcm_sha256 != candidate_sha256
    {
        bail!("candidate evidence identity does not match candidate binding");
    }

    validate_human_gates(checked.passed, &manifest.listening, &manifest.selection)?;
    validate_review(&manifest.review)?;
    let listening_complete = manifest.listening.status != ListeningStatus::Pending;
    let listening_passed = manifest.listening.status == ListeningStatus::Passed;
    let selected = manifest.selection.status == SelectionStatus::Selected;
    let rejected = manifest.selection.status == SelectionStatus::Rejected;

    Ok(ValidationReport {
        schema: SCHEMA.into(),
        candidate_id: manifest.candidate_id.clone(),
        manifest_sha256: sha256_path(path)?,
        contract_sha256: canonical_sha256(&manifest)?,
        intent_contract_sha256: intent_report.contract_sha256,
        evidence_contract_sha256: checked.evidence_contract_sha256,
        candidate_pcm_sha256: candidate_sha256,
        technical_passed: checked.passed,
        listening_complete,
        listening_passed,
        eligible_for_selection: checked.passed && listening_passed,
        selected,
        rejected,
        shareable: false,
        verified: true,
    })
}

fn validate_human_gates(
    technical_passed: bool,
    listening: &ListeningGate,
    selection: &SelectionGate,
) -> Result<()> {
    validate_gate_decision(
        "listening",
        listening.status == ListeningStatus::Pending,
        &listening.decision,
    )?;
    validate_gate_decision(
        "selection",
        selection.status == SelectionStatus::Pending,
        &selection.decision,
    )?;
    match selection.status {
        SelectionStatus::Pending => {}
        SelectionStatus::Selected => {
            if !technical_passed || listening.status != ListeningStatus::Passed {
                bail!("candidate selection requires passing technical evidence and listening");
            }
        }
        SelectionStatus::Rejected => {
            if listening.status == ListeningStatus::Pending {
                bail!("candidate rejection requires a completed listening gate");
            }
        }
    }
    Ok(())
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
        (false, Some(decision)) => {
            nonempty(
                &format!("{field}.decision.artifact_id"),
                &decision.artifact_id,
            )?;
            validate_sha256(&format!("{field}.decision.sha256"), &decision.sha256)
        }
    }
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
    let mut ids = BTreeSet::new();
    for decision in &review.decision_refs {
        nonempty("review.decision_refs[].artifact_id", &decision.artifact_id)?;
        validate_sha256("review.decision_refs[].sha256", &decision.sha256)?;
        if !ids.insert(decision.artifact_id.as_str()) {
            bail!("review.decision_refs artifact ids must be unique");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision() -> Option<DecisionRef> {
        Some(DecisionRef {
            artifact_id: "decision".into(),
            sha256: "1".repeat(64),
        })
    }

    #[test]
    fn selected_requires_both_technical_and_listening_passes() {
        let passed = ListeningGate {
            status: ListeningStatus::Passed,
            decision: decision(),
        };
        let selected = SelectionGate {
            status: SelectionStatus::Selected,
            decision: decision(),
        };
        assert!(validate_human_gates(true, &passed, &selected).is_ok());
        assert!(validate_human_gates(false, &passed, &selected).is_err());
        let failed = ListeningGate {
            status: ListeningStatus::Failed,
            decision: decision(),
        };
        assert!(validate_human_gates(true, &failed, &selected).is_err());
    }

    #[test]
    fn pending_and_rejected_states_preserve_separate_decisions() {
        let pending_listening = ListeningGate {
            status: ListeningStatus::Pending,
            decision: None,
        };
        let pending_selection = SelectionGate {
            status: SelectionStatus::Pending,
            decision: None,
        };
        assert!(validate_human_gates(false, &pending_listening, &pending_selection).is_ok());
        let failed = ListeningGate {
            status: ListeningStatus::Failed,
            decision: decision(),
        };
        let rejected = SelectionGate {
            status: SelectionStatus::Rejected,
            decision: decision(),
        };
        assert!(validate_human_gates(false, &failed, &rejected).is_ok());
    }
}
