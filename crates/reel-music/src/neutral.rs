use std::{fs, io::Write, path::Path};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::{
    hash::{canonical_sha256, sha256_path},
    source,
    time::{AudioTimebase, SampleRange},
};

pub const SCHEMA: &str = "reel.music-neutral-plan.v0.1";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NeutralPlan {
    pub schema: String,
    pub source_manifest_sha256: String,
    pub source_contract_sha256: String,
    pub source_id: String,
    pub timebase: AudioTimebase,
    pub decoded_pcm_sha256: String,
    pub operations: Vec<NeutralKeep>,
    pub locks: Vec<SampleRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NeutralKeep {
    pub kind: String,
    pub range: SampleRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanReport {
    pub schema: String,
    pub plan: String,
    pub plan_sha256: String,
    pub plan_contract_sha256: String,
    pub source_contract_sha256: String,
    pub samples_per_channel: u64,
    pub locked_samples: u64,
    pub shareable: bool,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckReport {
    pub schema: String,
    pub plan_sha256: String,
    pub plan_contract_sha256: String,
    pub candidate_pcm_sha256: String,
    pub samples_per_channel: u64,
    pub decoded_pcm_equal: bool,
    pub shareable: bool,
    pub verified: bool,
}

pub fn write_plan(source_path: &Path, output: &Path) -> Result<PlanReport> {
    if output.exists() {
        bail!("neutral plan output already exists: {}", output.display());
    }
    let source_report = source::validate(source_path)?;
    let source_manifest = source::load(source_path)?;
    let full = SampleRange {
        start: 0,
        end: source_manifest.media.timebase.samples_per_channel,
    };
    let plan = NeutralPlan {
        schema: SCHEMA.into(),
        source_manifest_sha256: source_report.manifest_sha256,
        source_contract_sha256: source_report.contract_sha256.clone(),
        source_id: source_manifest.source_id,
        timebase: source_manifest.media.timebase,
        decoded_pcm_sha256: source_report.decoded_pcm_sha256,
        operations: vec![NeutralKeep {
            kind: "keep".into(),
            range: full,
        }],
        locks: vec![full],
    };
    let bytes = serde_json::to_vec_pretty(&plan)?;
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.flush()?;
    temporary
        .persist(output)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish {}", output.display()))?;
    Ok(PlanReport {
        schema: SCHEMA.into(),
        plan: output.display().to_string(),
        plan_sha256: sha256_path(output)?,
        plan_contract_sha256: canonical_sha256(&plan)?,
        source_contract_sha256: source_report.contract_sha256,
        samples_per_channel: full.len(),
        locked_samples: full.len(),
        shareable: false,
        verified: true,
    })
}

pub fn check(plan_path: &Path, source_path: &Path, candidate_pcm: &Path) -> Result<CheckReport> {
    let bytes =
        fs::read(plan_path).with_context(|| format!("failed to read {}", plan_path.display()))?;
    let plan: NeutralPlan = serde_json::from_slice(&bytes)?;
    let source_report = source::validate(source_path)?;
    let source_manifest = source::load(source_path)?;
    let full = SampleRange {
        start: 0,
        end: source_manifest.media.timebase.samples_per_channel,
    };
    if plan.schema != SCHEMA
        || plan.source_manifest_sha256 != source_report.manifest_sha256
        || plan.source_contract_sha256 != source_report.contract_sha256
        || plan.source_id != source_manifest.source_id
        || plan.timebase != source_manifest.media.timebase
        || plan.decoded_pcm_sha256 != source_report.decoded_pcm_sha256
        || plan.operations
            != vec![NeutralKeep {
                kind: "keep".into(),
                range: full,
            }]
        || plan.locks != vec![full]
    {
        bail!("neutral plan does not match the current source and full lock");
    }
    let candidate_hash = sha256_path(candidate_pcm)?;
    let candidate_bytes = fs::metadata(candidate_pcm)?.len();
    if candidate_hash != source_report.decoded_pcm_sha256 {
        bail!("neutral candidate decoded PCM does not equal the source");
    }
    if candidate_bytes != source_report.bytes {
        bail!("neutral candidate byte count does not equal the source");
    }
    Ok(CheckReport {
        schema: "reel.music-neutral-check.v0.1".into(),
        plan_sha256: sha256_path(plan_path)?,
        plan_contract_sha256: canonical_sha256(&plan)?,
        candidate_pcm_sha256: candidate_hash,
        samples_per_channel: full.len(),
        decoded_pcm_equal: true,
        shareable: false,
        verified: true,
    })
}

pub fn contract_hash(path: &Path) -> Result<String> {
    let plan: NeutralPlan = serde_json::from_slice(&fs::read(path)?)?;
    canonical_sha256(&plan)
}
