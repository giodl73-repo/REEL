use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    AuthorityRef, DecisionRef,
    analysis::Review,
    hash::{canonical_sha256, sha256_path},
    interchange::{self, ArtifactPurpose},
    nonempty, status_requires_decision, unique_nonempty, validate_authority, validate_sha256,
};

pub const SCHEMA: &str = "reel.music-evidence-comparison.v0.1";
const REQUIRED_ROLES: &[&str] = &[
    "music-reconstruction-engineer",
    "sound-designer",
    "editor",
    "rights-provenance-steward",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceComparison {
    pub schema: String,
    pub comparison_id: String,
    pub intake: IntakeBinding,
    pub authority: AuthorityRef,
    pub sets: Vec<ComparisonSet>,
    pub review: Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeBinding {
    pub manifest: PathBuf,
    pub manifest_sha256: String,
    pub contract_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonSet {
    pub id: String,
    pub purpose: ArtifactPurpose,
    pub candidates: Vec<CandidateAssessment>,
    pub findings: Vec<ComparisonFinding>,
    #[serde(default)]
    pub corrections: Vec<CorrectionRequest>,
    pub selection: Option<Selection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateAssessment {
    pub artifact_id: String,
    pub coverage_millionths: Option<u32>,
    pub confidence_millionths: Option<u32>,
    pub alignment_error_samples: Option<u64>,
    pub bleed_millionths: Option<u32>,
    pub mixture_consistency_millionths: Option<u32>,
    pub event_count: Option<u64>,
    pub assessment: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingDimension {
    Coverage,
    Timing,
    Pitch,
    Lyrics,
    Form,
    Separation,
    SemanticContent,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingOutcome {
    Agrees,
    Disagrees,
    Inconclusive,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonFinding {
    pub id: String,
    pub dimension: FindingDimension,
    pub artifact_ids: Vec<String>,
    pub outcome: FindingOutcome,
    pub detail: String,
    pub evidence_sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CorrectionCategory {
    Timing,
    Pitch,
    Text,
    Label,
    Structure,
    Separation,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CorrectionRequest {
    pub id: String,
    pub artifact_id: String,
    pub category: CorrectionCategory,
    pub target: String,
    pub description: String,
    pub resolution: Option<DecisionRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub artifact_id: String,
    pub decision: DecisionRef,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueueItem {
    pub set_id: String,
    pub kind: String,
    pub item_id: String,
    pub artifact_id: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub comparison_id: String,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub intake_contract_sha256: String,
    pub sets: usize,
    pub candidates: usize,
    pub findings: usize,
    pub selected_sets: usize,
    pub open_corrections: usize,
    pub queue: Vec<QueueItem>,
    pub reviewed: bool,
    pub shareable: bool,
    pub verified: bool,
}

pub fn load(path: &Path) -> Result<EvidenceComparison> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_slice(&bytes).with_context(|| {
        format!(
            "music evidence comparison is not valid YAML: {}",
            path.display()
        )
    })
}

pub fn validate(path: &Path) -> Result<ValidationReport> {
    let manifest = load(path)?;
    validate_loaded(path, &manifest)
}

fn validate_loaded(path: &Path, manifest: &EvidenceComparison) -> Result<ValidationReport> {
    if manifest.schema != SCHEMA {
        bail!("music evidence comparison schema must be {SCHEMA}");
    }
    nonempty("comparison_id", &manifest.comparison_id)?;
    validate_authority(&manifest.authority)?;

    validate_sha256("intake.manifest_sha256", &manifest.intake.manifest_sha256)?;
    validate_sha256("intake.contract_sha256", &manifest.intake.contract_sha256)?;
    let intake_path = resolve(path, &manifest.intake.manifest);
    if sha256_path(&intake_path)? != manifest.intake.manifest_sha256.to_lowercase() {
        bail!("intake manifest sha256 does not match comparison binding");
    }
    let intake_report = interchange::validate(&intake_path)?;
    if intake_report.contract_sha256 != manifest.intake.contract_sha256.to_lowercase() {
        bail!("intake contract sha256 does not match comparison binding");
    }
    let intake = interchange::load(&intake_path)?;
    let artifacts = intake
        .artifacts
        .iter()
        .map(|artifact| (artifact.id.as_str(), artifact))
        .collect::<BTreeMap<_, _>>();

    if manifest.sets.is_empty() {
        bail!("sets must not be empty");
    }
    let mut set_ids = BTreeSet::new();
    let mut total_candidates = 0;
    let mut total_findings = 0;
    let mut selected_sets = 0;
    let mut open_corrections = 0;
    let mut queue = Vec::new();

    for set in &manifest.sets {
        nonempty("sets[].id", &set.id)?;
        if !set_ids.insert(set.id.as_str()) {
            bail!("sets[].id must be unique");
        }
        if set.candidates.len() < 2 {
            bail!("comparison set {} requires at least two candidates", set.id);
        }
        let mut candidate_ids = BTreeSet::new();
        for candidate in &set.candidates {
            nonempty("sets[].candidates[].artifact_id", &candidate.artifact_id)?;
            if !candidate_ids.insert(candidate.artifact_id.as_str()) {
                bail!(
                    "comparison set {} candidate artifact ids must be unique",
                    set.id
                );
            }
            let artifact = artifacts
                .get(candidate.artifact_id.as_str())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "comparison set {} references unknown artifact {}",
                        set.id,
                        candidate.artifact_id
                    )
                })?;
            if artifact.purpose != set.purpose {
                bail!(
                    "comparison set {} artifact {} does not match its declared purpose",
                    set.id,
                    candidate.artifact_id
                );
            }
            validate_millionths("coverage_millionths", candidate.coverage_millionths)?;
            validate_millionths("confidence_millionths", candidate.confidence_millionths)?;
            validate_millionths("bleed_millionths", candidate.bleed_millionths)?;
            validate_millionths(
                "mixture_consistency_millionths",
                candidate.mixture_consistency_millionths,
            )?;
            nonempty("sets[].candidates[].assessment", &candidate.assessment)?;
        }
        total_candidates += set.candidates.len();

        if set.findings.is_empty() {
            bail!("comparison set {} requires at least one finding", set.id);
        }
        let mut finding_ids = BTreeSet::new();
        for finding in &set.findings {
            nonempty("sets[].findings[].id", &finding.id)?;
            if !finding_ids.insert(finding.id.as_str()) {
                bail!("comparison set {} finding ids must be unique", set.id);
            }
            unique_nonempty("sets[].findings[].artifact_ids", &finding.artifact_ids)?;
            if finding.artifact_ids.len() < 2
                || finding
                    .artifact_ids
                    .iter()
                    .any(|id| !candidate_ids.contains(id.as_str()))
            {
                bail!(
                    "comparison set {} findings must reference at least two candidates in that set",
                    set.id
                );
            }
            nonempty("sets[].findings[].detail", &finding.detail)?;
            if let Some(hash) = &finding.evidence_sha256 {
                validate_sha256("sets[].findings[].evidence_sha256", hash)?;
            }
        }
        total_findings += set.findings.len();

        let mut correction_ids = BTreeSet::new();
        for correction in &set.corrections {
            nonempty("sets[].corrections[].id", &correction.id)?;
            if !correction_ids.insert(correction.id.as_str()) {
                bail!("comparison set {} correction ids must be unique", set.id);
            }
            if !candidate_ids.contains(correction.artifact_id.as_str()) {
                bail!(
                    "comparison set {} correction {} references a non-candidate artifact",
                    set.id,
                    correction.id
                );
            }
            nonempty("sets[].corrections[].target", &correction.target)?;
            nonempty("sets[].corrections[].description", &correction.description)?;
            if let Some(decision) = &correction.resolution {
                validate_decision_ref("sets[].corrections[].resolution", decision)?;
            } else {
                open_corrections += 1;
                queue.push(QueueItem {
                    set_id: set.id.clone(),
                    kind: "correction".into(),
                    item_id: correction.id.clone(),
                    artifact_id: Some(correction.artifact_id.clone()),
                    summary: correction.description.clone(),
                });
            }
        }

        if let Some(selection) = &set.selection {
            if !candidate_ids.contains(selection.artifact_id.as_str()) {
                bail!("comparison set {} selects a non-candidate artifact", set.id);
            }
            validate_decision_ref("sets[].selection.decision", &selection.decision)?;
            nonempty("sets[].selection.rationale", &selection.rationale)?;
            if set
                .corrections
                .iter()
                .any(|item| item.artifact_id == selection.artifact_id && item.resolution.is_none())
            {
                bail!(
                    "comparison set {} cannot select an artifact with open corrections",
                    set.id
                );
            }
            selected_sets += 1;
        } else {
            queue.push(QueueItem {
                set_id: set.id.clone(),
                kind: "selection".into(),
                item_id: format!("{}-selection", set.id),
                artifact_id: None,
                summary: "Human candidate selection remains open; no automatic ranking is applied."
                    .into(),
            });
        }
    }

    validate_review(&manifest.review)?;
    queue.sort_by(|left, right| {
        (&left.set_id, &left.kind, &left.item_id).cmp(&(&right.set_id, &right.kind, &right.item_id))
    });

    Ok(ValidationReport {
        schema: SCHEMA.into(),
        comparison_id: manifest.comparison_id.clone(),
        manifest_sha256: sha256_path(path)?,
        contract_sha256: canonical_sha256(manifest)?,
        intake_contract_sha256: intake_report.contract_sha256,
        sets: manifest.sets.len(),
        candidates: total_candidates,
        findings: total_findings,
        selected_sets,
        open_corrections,
        queue,
        reviewed: status_requires_decision(&manifest.review.status),
        shareable: false,
        verified: true,
    })
}

fn validate_millionths(field: &str, value: Option<u32>) -> Result<()> {
    if value.is_some_and(|value| value > 1_000_000) {
        bail!("{field} must not exceed 1000000");
    }
    Ok(())
}

fn validate_decision_ref(field: &str, decision: &DecisionRef) -> Result<()> {
    nonempty(&format!("{field}.artifact_id"), &decision.artifact_id)?;
    validate_sha256(&format!("{field}.sha256"), &decision.sha256)
}

fn validate_review(review: &Review) -> Result<()> {
    nonempty("review.status", &review.status)?;
    unique_nonempty("review.required_roles", &review.required_roles)?;
    for role in REQUIRED_ROLES {
        if !review.required_roles.iter().any(|value| value == role) {
            bail!("review.required_roles must include {role}");
        }
    }
    for decision in &review.decision_refs {
        validate_decision_ref("review.decision_refs[]", decision)?;
    }
    if status_requires_decision(&review.status) && review.decision_refs.is_empty() {
        bail!("review.status {} requires decision_refs", review.status);
    }
    Ok(())
}

fn resolve(manifest: &Path, child: &Path) -> PathBuf {
    if child.is_absolute() {
        child.to_path_buf()
    } else {
        manifest
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(child)
    }
}
