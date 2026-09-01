use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::{
    hash::{canonical_sha256, sha256_path},
    repair::{self, Operation},
    source::{self, RawPcmFormat},
    time::{AudioTimebase, SampleRange},
};

pub const SCHEMA: &str = "reel.music-repair-edl.v0.1";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EditDecisionList {
    pub schema: String,
    pub repair_manifest: PathBuf,
    pub repair_manifest_sha256: String,
    pub repair_contract_sha256: String,
    pub source_manifest: PathBuf,
    pub source_manifest_sha256: String,
    pub source_contract_sha256: String,
    pub source_id: String,
    pub source_pcm: PathBuf,
    pub source_pcm_sha256: String,
    pub format: RawPcmFormat,
    pub timebase: AudioTimebase,
    pub output_samples_per_channel: u64,
    pub segments: Vec<KeepSegment>,
    pub cuts: Vec<CutDecision>,
    pub evidence_policy: EvidencePolicy,
    pub shareable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KeepSegment {
    pub id: String,
    pub source: SampleRange,
    pub output: SampleRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CutDecision {
    pub operation_id: String,
    pub source: SampleRange,
    pub output_join_sample: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidencePolicy {
    pub profile: String,
    pub window_samples: u32,
    pub max_boundary_delta_millionths: u32,
    pub max_rms_delta_millidb: u32,
    pub min_window_correlation_millionths: i32,
    pub max_spectral_distance_millionths: u32,
    pub min_exact_right_tail_samples: u64,
}

impl Default for EvidencePolicy {
    fn default() -> Self {
        Self {
            profile: "strict-cut-seam-v0.1".into(),
            window_samples: 256,
            max_boundary_delta_millionths: 150_000,
            max_rms_delta_millidb: 2_000,
            min_window_correlation_millionths: 800_000,
            max_spectral_distance_millionths: 200_000,
            min_exact_right_tail_samples: 16,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompileReport {
    pub schema: String,
    pub edl: String,
    pub edl_sha256: String,
    pub edl_contract_sha256: String,
    pub segments: usize,
    pub cuts: usize,
    pub source_samples_per_channel: u64,
    pub output_samples_per_channel: u64,
    pub removed_samples_per_channel: u64,
    pub adapter: String,
    pub shareable: bool,
    pub verified: bool,
}

pub fn write(repair_path: &Path, output: &Path) -> Result<CompileReport> {
    if output.exists() {
        bail!("repair EDL output already exists: {}", output.display());
    }
    let edl = build(repair_path)?;
    let bytes = serde_json::to_vec_pretty(&edl)?;
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.flush()?;
    temporary
        .persist(output)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish {}", output.display()))?;
    report(output, &edl)
}

pub fn load(path: &Path) -> Result<EditDecisionList> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("music repair EDL is not valid JSON: {}", path.display()))
}

pub fn validate(path: &Path, repair_path: &Path) -> Result<CompileReport> {
    let actual = load(path)?;
    let expected = build(repair_path)?;
    if actual != expected {
        bail!("repair EDL does not match the current repair, source, or evidence policy");
    }
    report(path, &actual)
}

fn report(path: &Path, edl: &EditDecisionList) -> Result<CompileReport> {
    Ok(CompileReport {
        schema: SCHEMA.into(),
        edl: path.display().to_string(),
        edl_sha256: sha256_path(path)?,
        edl_contract_sha256: canonical_sha256(edl)?,
        segments: edl.segments.len(),
        cuts: edl.cuts.len(),
        source_samples_per_channel: edl.timebase.samples_per_channel,
        output_samples_per_channel: edl.output_samples_per_channel,
        removed_samples_per_channel: edl.timebase.samples_per_channel
            - edl.output_samples_per_channel,
        adapter: "ffmpeg".into(),
        shareable: false,
        verified: true,
    })
}

fn build(repair_path: &Path) -> Result<EditDecisionList> {
    let repair_report = repair::validate(repair_path)?;
    let repair_manifest = repair::load(repair_path)?;
    let repair_manifest_path = fs::canonicalize(repair_path)?;
    let source_manifest_path = fs::canonicalize(source::resolve(
        repair_path,
        &repair_manifest.source.manifest,
    ))?;
    let source_report = source::validate(&source_manifest_path)?;
    let source_manifest = source::load(&source_manifest_path)?;
    let source_pcm = fs::canonicalize(source::resolve(
        &source_manifest_path,
        &source_manifest.media.path,
    ))?;

    let mut cuts = Vec::new();
    for operation in &repair_manifest.operations {
        match operation {
            Operation::Cut { id, range } => cuts.push((id.clone(), *range)),
            Operation::Keep { .. } | Operation::Lock { .. } => {}
            _ => bail!(
                "operation {} is planned but not executable in cut-only EDL v0.1",
                operation.id()
            ),
        }
    }
    if cuts.is_empty() {
        bail!("cut-only EDL requires at least one cut operation");
    }
    let mut prior_end = 0;
    for (index, (_, range)) in cuts.iter().enumerate() {
        if range.start == 0 || range.end == repair_manifest.timebase.samples_per_channel {
            bail!("cut-only EDL requires signal on both sides of every cut");
        }
        if index > 0 && range.start <= prior_end {
            bail!(
                "cut operations must be declared in ascending order with retained signal between them"
            );
        }
        prior_end = range.end;
    }
    let cut_ranges = cuts.iter().map(|(_, range)| *range).collect::<Vec<_>>();
    if cut_ranges != repair_manifest.changed_envelopes {
        bail!("cut-only EDL requires each changed envelope to equal one cut");
    }

    let mut segments = Vec::new();
    let mut decisions = Vec::new();
    let mut source_cursor = 0;
    let mut output_cursor = 0;
    for (index, (operation_id, cut)) in cuts.into_iter().enumerate() {
        let kept = SampleRange {
            start: source_cursor,
            end: cut.start,
        };
        let output = SampleRange {
            start: output_cursor,
            end: output_cursor + kept.len(),
        };
        segments.push(KeepSegment {
            id: format!("keep-{:03}", index + 1),
            source: kept,
            output,
        });
        output_cursor = output.end;
        decisions.push(CutDecision {
            operation_id,
            source: cut,
            output_join_sample: output_cursor,
        });
        source_cursor = cut.end;
    }
    let trailing = SampleRange {
        start: source_cursor,
        end: repair_manifest.timebase.samples_per_channel,
    };
    let trailing_output = SampleRange {
        start: output_cursor,
        end: output_cursor + trailing.len(),
    };
    segments.push(KeepSegment {
        id: format!("keep-{:03}", segments.len() + 1),
        source: trailing,
        output: trailing_output,
    });

    Ok(EditDecisionList {
        schema: SCHEMA.into(),
        repair_manifest: repair_manifest_path,
        repair_manifest_sha256: repair_report.manifest_sha256,
        repair_contract_sha256: repair_report.contract_sha256,
        source_manifest: source_manifest_path,
        source_manifest_sha256: source_report.manifest_sha256,
        source_contract_sha256: source_report.contract_sha256,
        source_id: source_manifest.source_id,
        source_pcm,
        source_pcm_sha256: source_report.decoded_pcm_sha256,
        format: source_manifest.media.format,
        timebase: source_manifest.media.timebase,
        output_samples_per_channel: trailing_output.end,
        segments,
        cuts: decisions,
        evidence_policy: EvidencePolicy::default(),
        shareable: false,
    })
}
