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
    model::{self, EvidenceRef, MusicModel, Provenance, ProvenanceState},
    nonempty, source, status_requires_decision, unique_nonempty, validate_authority,
    validate_sha256,
};

pub const SCHEMA: &str = "reel.music-model-draft.v0.1";
const REQUIRED_ROLES: &[&str] = &[
    "music-reconstruction-engineer",
    "score-arrangement-director",
    "sound-designer",
    "editor",
    "rights-provenance-steward",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDraft {
    pub schema: String,
    pub draft_id: String,
    pub model: ModelBinding,
    pub authority: AuthorityRef,
    pub dispositions: Vec<ObservationDisposition>,
    pub review: model::Review,
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
pub struct ObservationDisposition {
    pub analysis_id: String,
    pub observation_id: String,
    pub outcome: DispositionOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DispositionOutcome {
    Mapped {
        targets: Vec<TargetMapping>,
    },
    Omitted {
        rationale: String,
        decision: DecisionRef,
    },
    Unknown {
        rationale: String,
        unknown_text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetMapping {
    pub target_ref: String,
    pub state: ProvenanceState,
    pub rationale: String,
    pub correction_ref: Option<DecisionRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub draft_id: String,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub model_contract_sha256: String,
    pub observations: usize,
    pub mapped_observations: usize,
    pub mapped_targets: usize,
    pub omitted_observations: usize,
    pub unknown_observations: usize,
    pub human_corrected_targets: usize,
    pub reviewed: bool,
    pub shareable: bool,
    pub verified: bool,
}

pub fn load(path: &Path) -> Result<ModelDraft> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_slice(&bytes)
        .with_context(|| format!("music model draft is not valid YAML: {}", path.display()))
}

pub fn validate(path: &Path) -> Result<ValidationReport> {
    let draft = load(path)?;
    validate_loaded(path, &draft)
}

fn validate_loaded(path: &Path, draft: &ModelDraft) -> Result<ValidationReport> {
    if draft.schema != SCHEMA {
        bail!("music model draft schema must be {SCHEMA}");
    }
    nonempty("draft_id", &draft.draft_id)?;
    validate_authority(&draft.authority)?;
    validate_sha256("model.manifest_sha256", &draft.model.manifest_sha256)?;
    validate_sha256("model.contract_sha256", &draft.model.contract_sha256)?;
    nonempty("model.model_id", &draft.model.model_id)?;
    let model_path = source::resolve(path, &draft.model.manifest);
    if sha256_path(&model_path)? != draft.model.manifest_sha256.to_lowercase() {
        bail!("model manifest sha256 does not match draft binding");
    }
    let model_report = model::validate(&model_path)?;
    let model = model::load(&model_path)?;
    if model_report.contract_sha256 != draft.model.contract_sha256.to_lowercase()
        || model_report.model_id != draft.model.model_id
    {
        bail!("model contract or identity does not match draft binding");
    }

    let observations = load_observation_census(&model_path, &model)?;
    let targets = model_targets(&model)?;
    if draft.dispositions.len() != observations.len() {
        bail!("dispositions must cover every bound analysis observation exactly once");
    }

    let mut disposition_refs = BTreeSet::new();
    let mut declared_mappings = BTreeSet::new();
    let mut mapped_observations = 0;
    let mut mapped_targets = 0;
    let mut omitted = 0;
    let mut unknown = 0;
    let mut corrected = 0;
    for disposition in &draft.dispositions {
        nonempty("dispositions[].analysis_id", &disposition.analysis_id)?;
        nonempty("dispositions[].observation_id", &disposition.observation_id)?;
        let evidence = EvidenceRef {
            analysis_id: disposition.analysis_id.clone(),
            observation_id: disposition.observation_id.clone(),
        };
        if !disposition_refs.insert(evidence.clone()) {
            bail!("each analysis observation may have only one disposition");
        }
        if !observations.contains(&evidence) {
            bail!("disposition references an unknown analysis observation");
        }
        match &disposition.outcome {
            DispositionOutcome::Mapped { targets: mappings } => {
                if mappings.is_empty() {
                    bail!("mapped disposition targets must not be empty");
                }
                mapped_observations += 1;
                let mut local_targets = BTreeSet::new();
                for mapping in mappings {
                    nonempty("dispositions[].targets[].target_ref", &mapping.target_ref)?;
                    nonempty("dispositions[].targets[].rationale", &mapping.rationale)?;
                    if !local_targets.insert(mapping.target_ref.as_str()) {
                        bail!("mapped target refs must be unique within a disposition");
                    }
                    let provenance = targets.get(mapping.target_ref.as_str()).ok_or_else(|| {
                        anyhow::anyhow!(
                            "disposition references unknown model target {}",
                            mapping.target_ref
                        )
                    })?;
                    if provenance.state != mapping.state
                        || provenance.correction_ref != mapping.correction_ref
                        || !provenance.evidence_refs.contains(&evidence)
                    {
                        bail!(
                            "model target {} does not match its declared disposition",
                            mapping.target_ref
                        );
                    }
                    validate_mapping_correction(mapping)?;
                    if mapping.state == ProvenanceState::HumanCorrected {
                        corrected += 1;
                    }
                    declared_mappings.insert((evidence.clone(), mapping.target_ref.clone()));
                    mapped_targets += 1;
                }
            }
            DispositionOutcome::Omitted {
                rationale,
                decision,
            } => {
                nonempty("dispositions[].outcome.rationale", rationale)?;
                validate_decision("dispositions[].outcome.decision", decision)?;
                omitted += 1;
            }
            DispositionOutcome::Unknown {
                rationale,
                unknown_text,
            } => {
                nonempty("dispositions[].outcome.rationale", rationale)?;
                nonempty("dispositions[].outcome.unknown_text", unknown_text)?;
                if !model.unknowns.contains(unknown_text) {
                    bail!("unknown disposition text must be preserved exactly in model.unknowns");
                }
                unknown += 1;
            }
        }
    }
    if disposition_refs != observations {
        bail!("dispositions do not exactly match the bound analysis observation census");
    }

    for (target_ref, provenance) in &targets {
        for evidence in &provenance.evidence_refs {
            if !declared_mappings.contains(&(evidence.clone(), (*target_ref).to_string())) {
                bail!("model evidence citation lacks a matching target disposition");
            }
        }
    }
    validate_review(&draft.review)?;

    Ok(ValidationReport {
        schema: SCHEMA.into(),
        draft_id: draft.draft_id.clone(),
        manifest_sha256: sha256_path(path)?,
        contract_sha256: canonical_sha256(draft)?,
        model_contract_sha256: model_report.contract_sha256,
        observations: observations.len(),
        mapped_observations,
        mapped_targets,
        omitted_observations: omitted,
        unknown_observations: unknown,
        human_corrected_targets: corrected,
        reviewed: status_requires_decision(&draft.review.status),
        shareable: false,
        verified: true,
    })
}

fn load_observation_census(model_path: &Path, model: &MusicModel) -> Result<BTreeSet<EvidenceRef>> {
    let mut census = BTreeSet::new();
    for binding in &model.analyses {
        let analysis_path = source::resolve(model_path, &binding.manifest);
        let analysis = analysis::load(&analysis_path)?;
        for observation in analysis.observations {
            census.insert(EvidenceRef {
                analysis_id: binding.analysis_id.clone(),
                observation_id: observation.id,
            });
        }
    }
    Ok(census)
}

fn model_targets(model: &MusicModel) -> Result<BTreeMap<String, &Provenance>> {
    let mut targets = BTreeMap::new();
    for item in &model.tempo_map {
        insert_target(
            &mut targets,
            format!("tempo:{}", item.tick),
            &item.provenance,
        )?;
    }
    for item in &model.meter_map {
        insert_target(
            &mut targets,
            format!("meter:{}", item.tick),
            &item.provenance,
        )?;
    }
    for item in &model.form {
        insert_target(&mut targets, format!("form:{}", item.id), &item.provenance)?;
    }
    for part in &model.parts {
        for note in &part.notes {
            insert_target(&mut targets, format!("note:{}", note.id), &note.provenance)?;
        }
    }
    for item in &model.harmony {
        insert_target(
            &mut targets,
            format!("harmony:{}", item.id),
            &item.provenance,
        )?;
    }
    for item in &model.rhythm_cells {
        insert_target(
            &mut targets,
            format!("rhythm:{}", item.id),
            &item.provenance,
        )?;
    }
    for item in &model.hooks {
        insert_target(&mut targets, format!("hook:{}", item.id), &item.provenance)?;
    }
    for item in &model.expressive_timing {
        insert_target(
            &mut targets,
            format!("expressive:{}", item.note_id),
            &item.provenance,
        )?;
    }
    Ok(targets)
}

fn insert_target<'a>(
    targets: &mut BTreeMap<String, &'a Provenance>,
    id: String,
    provenance: &'a Provenance,
) -> Result<()> {
    if targets.insert(id.clone(), provenance).is_some() {
        bail!("model target reference is ambiguous: {id}");
    }
    Ok(())
}

fn validate_mapping_correction(mapping: &TargetMapping) -> Result<()> {
    match mapping.state {
        ProvenanceState::HumanCorrected => {
            let decision = mapping.correction_ref.as_ref().ok_or_else(|| {
                anyhow::anyhow!("human-corrected target mapping requires correction_ref")
            })?;
            validate_decision("dispositions[].targets[].correction_ref", decision)
        }
        ProvenanceState::Observed | ProvenanceState::Inferred => {
            if mapping.correction_ref.is_some() {
                bail!("observed/inferred target mapping forbids correction_ref");
            }
            Ok(())
        }
    }
}

fn validate_decision(field: &str, decision: &DecisionRef) -> Result<()> {
    nonempty(&format!("{field}.artifact_id"), &decision.artifact_id)?;
    validate_sha256(&format!("{field}.sha256"), &decision.sha256)
}

fn validate_review(review: &model::Review) -> Result<()> {
    nonempty("review.status", &review.status)?;
    unique_nonempty("review.required_roles", &review.required_roles)?;
    for role in REQUIRED_ROLES {
        if !review.required_roles.iter().any(|value| value == role) {
            bail!("review.required_roles must include {role}");
        }
    }
    let mut decisions = BTreeSet::new();
    for decision in &review.decision_refs {
        validate_decision("review.decision_refs[]", decision)?;
        if !decisions.insert(decision.artifact_id.as_str()) {
            bail!("review.decision_refs artifact ids must be unique");
        }
    }
    if status_requires_decision(&review.status) && review.decision_refs.is_empty() {
        bail!("review.status {} requires decision_refs", review.status);
    }
    Ok(())
}
