use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    DecisionRef,
    hash::{canonical_sha256, sha256_path},
    nonempty, source, status_requires_decision,
    time::{AudioTimebase, SampleRange, validate_ordered_nonoverlapping},
    unique_nonempty, validate_sha256,
};

pub const SCHEMA: &str = "reel.music-repair.v0.1";
const REQUIRED_ROLES: &[&str] = &[
    "music-reconstruction-engineer",
    "sound-designer",
    "editor",
    "rights-provenance-steward",
];

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepairManifest {
    pub schema: String,
    pub repair_id: String,
    pub source: SourceRef,
    pub source_id: String,
    pub decoded_pcm_sha256: String,
    pub timebase: AudioTimebase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat_grid: Option<BeatGrid>,
    pub operations: Vec<Operation>,
    pub changed_envelopes: Vec<SampleRange>,
    pub locks: Vec<SampleRange>,
    pub review: Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRef {
    pub manifest: PathBuf,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub status: String,
    pub required_roles: Vec<String>,
    #[serde(default)]
    pub decision_refs: Vec<DecisionRef>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Operation {
    Keep {
        id: String,
        range: SampleRange,
    },
    Cut {
        id: String,
        range: SampleRange,
    },
    Insert {
        id: String,
        destination: SampleRange,
        asset: AssetRange,
    },
    Replace {
        id: String,
        destination: SampleRange,
        asset: AssetRange,
    },
    Repeat {
        id: String,
        source: SampleRange,
        destination: SampleRange,
    },
    Move {
        id: String,
        source: SampleRange,
        destination: SampleRange,
    },
    Crossfade {
        id: String,
        range: SampleRange,
        curve: FadeCurve,
    },
    PreserveTail {
        id: String,
        source: SampleRange,
        destination: SampleRange,
    },
    MatchGain {
        id: String,
        range: SampleRange,
        target_millilufs: i32,
    },
    MatchEq {
        id: String,
        range: SampleRange,
        profile_sha256: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        bands: Vec<EqBand>,
    },
    ExtendBars {
        id: String,
        range: SampleRange,
        bars: u32,
    },
    Lock {
        id: String,
        range: SampleRange,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssetRange {
    pub path: PathBuf,
    pub sha256: String,
    pub decoded_pcm_sha256: String,
    pub format: source::RawPcmFormat,
    pub timebase: AudioTimebase,
    pub range: SampleRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FadeCurve {
    Linear,
    EqualPower,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BeatGrid {
    pub origin_sample: u64,
    pub samples_per_beat: u64,
    pub beats_per_bar: u16,
    #[serde(default)]
    pub boundary_tolerance_samples: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EqBand {
    pub frequency_millihz: u32,
    pub q_milli: u32,
    pub gain_millidb: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub repair_id: String,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub source_contract_sha256: String,
    pub operations: usize,
    pub changed_envelopes: usize,
    pub changed_samples: u64,
    pub locks: usize,
    pub locked_samples: u64,
    pub complete_coverage: bool,
    pub required_roles_present: bool,
    pub shareable: bool,
    pub verified: bool,
}

pub fn validate(path: &Path) -> Result<ValidationReport> {
    let manifest = load(path)?;
    validate_loaded(path, &manifest)
}

pub fn load(path: &Path) -> Result<RepairManifest> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_slice(&bytes)
        .with_context(|| format!("music repair is not valid YAML: {}", path.display()))
}

fn validate_loaded(path: &Path, manifest: &RepairManifest) -> Result<ValidationReport> {
    if manifest.schema != SCHEMA {
        bail!("music repair schema must be {SCHEMA}");
    }
    nonempty("repair_id", &manifest.repair_id)?;
    validate_sha256("source.sha256", &manifest.source.sha256)?;
    validate_sha256("decoded_pcm_sha256", &manifest.decoded_pcm_sha256)?;
    manifest.timebase.validate()?;
    validate_beat_grid(manifest.beat_grid, manifest.timebase)?;
    let source_path = source::resolve(path, &manifest.source.manifest);
    if sha256_path(&source_path)? != manifest.source.sha256.to_lowercase() {
        bail!("source manifest sha256 does not match source.sha256");
    }
    let source_report = source::validate(&source_path)?;
    let source_manifest = source::load(&source_path)?;
    if manifest.source_id != source_manifest.source_id
        || manifest.decoded_pcm_sha256 != source_report.decoded_pcm_sha256
        || manifest.timebase != source_manifest.media.timebase
    {
        bail!("repair source identity or timebase does not match source manifest");
    }
    let total = manifest.timebase.samples_per_channel;
    validate_ordered_nonoverlapping(&manifest.changed_envelopes, total, "changed_envelopes")?;
    validate_ordered_nonoverlapping(&manifest.locks, total, "locks")?;
    if manifest.changed_envelopes.is_empty() {
        bail!("changed_envelopes must not be empty");
    }
    if manifest.locks.is_empty() {
        bail!("locks must not be empty");
    }
    for changed in &manifest.changed_envelopes {
        if manifest.locks.iter().any(|lock| changed.intersects(*lock)) {
            bail!("changed_envelopes must not intersect locks");
        }
    }
    validate_complete_coverage(&manifest.changed_envelopes, &manifest.locks, total)?;
    validate_review(&manifest.review)?;
    validate_operations(path, manifest, total, source_manifest.media.format)?;
    Ok(ValidationReport {
        schema: SCHEMA.into(),
        repair_id: manifest.repair_id.clone(),
        manifest_sha256: sha256_path(path)?,
        contract_sha256: canonical_sha256(manifest)?,
        source_contract_sha256: source_report.contract_sha256,
        operations: manifest.operations.len(),
        changed_envelopes: manifest.changed_envelopes.len(),
        changed_samples: manifest
            .changed_envelopes
            .iter()
            .map(|range| range.len())
            .sum(),
        locks: manifest.locks.len(),
        locked_samples: manifest.locks.iter().map(|range| range.len()).sum(),
        complete_coverage: true,
        required_roles_present: true,
        shareable: false,
        verified: true,
    })
}

fn validate_review(review: &Review) -> Result<()> {
    nonempty("review.status", &review.status)?;
    unique_nonempty("review.required_roles", &review.required_roles)?;
    for role in REQUIRED_ROLES {
        if !review.required_roles.iter().any(|value| value == role) {
            bail!("review.required_roles must include {role}");
        }
    }
    let mut decision_ids = BTreeSet::new();
    for decision in &review.decision_refs {
        nonempty("review.decision_refs[].artifact_id", &decision.artifact_id)?;
        validate_sha256("review.decision_refs[].sha256", &decision.sha256)?;
        if !decision_ids.insert(&decision.artifact_id) {
            bail!("review.decision_refs artifact ids must be unique");
        }
    }
    if status_requires_decision(&review.status) && review.decision_refs.is_empty() {
        bail!("review.status {} requires decision_refs", review.status);
    }
    Ok(())
}

fn validate_operations(
    path: &Path,
    manifest: &RepairManifest,
    total: u64,
    source_format: source::RawPcmFormat,
) -> Result<()> {
    if manifest.operations.is_empty() {
        bail!("operations must not be empty");
    }
    let mut ids = BTreeSet::new();
    let mut mutating = Vec::<SampleRange>::new();
    for operation in &manifest.operations {
        let id = operation.id();
        nonempty("operations[].id", id)?;
        if !ids.insert(id) {
            bail!("duplicate operation id: {id}");
        }
        for range in operation.all_ranges() {
            range.validate(total, &format!("operation {id} range"))?;
        }
        match operation {
            Operation::Keep { range, .. }
                if manifest
                    .changed_envelopes
                    .iter()
                    .any(|changed| range.intersects(*changed)) =>
            {
                bail!("keep operation {id} intersects a changed envelope");
            }
            Operation::Lock { range, .. }
                if !manifest.locks.iter().any(|lock| lock.contains(*range)) =>
            {
                bail!("lock operation {id} is not contained by declared locks");
            }
            Operation::Insert {
                destination, asset, ..
            }
            | Operation::Replace {
                destination, asset, ..
            } => {
                validate_asset(path, asset, manifest.timebase, source_format)?;
                if destination.len() != asset.range.len() {
                    bail!("operation {id} destination and asset ranges must have equal length");
                }
            }
            Operation::Repeat {
                source,
                destination,
                ..
            }
            | Operation::PreserveTail {
                source,
                destination,
                ..
            } if source.len() != destination.len() => {
                bail!("operation {id} source and destination ranges must have equal length");
            }
            Operation::Crossfade { range, .. } if range.len() < 2 || range.len() % 2 != 0 => {
                bail!("crossfade operation {id} requires an even range of at least two samples");
            }
            Operation::MatchGain {
                target_millilufs, ..
            } if !(-70_000..=-1_000).contains(target_millilufs) => {
                bail!("match-gain operation {id} target must be -70000..=-1000 millilufs");
            }
            Operation::MatchEq {
                profile_sha256,
                bands,
                ..
            } => {
                validate_sha256("operations[].profile_sha256", profile_sha256)?;
                if !bands.is_empty() {
                    validate_eq_bands(id, bands, manifest.timebase.sample_rate_hz)?;
                    if canonical_sha256(bands)? != profile_sha256.to_lowercase() {
                        bail!("match-eq operation {id} profile hash does not bind its bands");
                    }
                }
            }
            Operation::ExtendBars { bars, .. } if *bars == 0 => {
                bail!("extend-bars operation {id} requires positive bars");
            }
            _ => {}
        }
        for range in operation.changed_ranges() {
            if !manifest
                .changed_envelopes
                .iter()
                .any(|changed| changed.contains(range))
            {
                bail!("operation {id} changes samples outside changed_envelopes");
            }
            if mutating.iter().any(|prior| prior.intersects(range)) {
                bail!("mutating operation {id} overlaps a prior operation");
            }
            mutating.push(range);
        }
    }
    for changed in &manifest.changed_envelopes {
        if !is_covered(*changed, &mutating) {
            bail!(
                "changed envelope {}..{} is not fully covered by operations",
                changed.start,
                changed.end
            );
        }
    }
    Ok(())
}

fn validate_beat_grid(grid: Option<BeatGrid>, timebase: AudioTimebase) -> Result<()> {
    let Some(grid) = grid else {
        return Ok(());
    };
    if grid.origin_sample >= timebase.samples_per_channel
        || grid.samples_per_beat == 0
        || !(1..=32).contains(&grid.beats_per_bar)
        || u64::from(grid.boundary_tolerance_samples) >= grid.samples_per_beat
    {
        bail!(
            "beat_grid requires an in-range origin, positive beat length, 1..=32 beats per bar, and tolerance below one beat"
        );
    }
    grid.samples_per_beat
        .checked_mul(u64::from(grid.beats_per_bar))
        .ok_or_else(|| anyhow::anyhow!("beat_grid bar length overflows u64"))?;
    Ok(())
}

fn validate_eq_bands(id: &str, bands: &[EqBand], sample_rate_hz: u32) -> Result<()> {
    let nyquist_millihz = u64::from(sample_rate_hz) * 500;
    for band in bands {
        if band.frequency_millihz < 20_000
            || u64::from(band.frequency_millihz) >= nyquist_millihz
            || !(100..=24_000).contains(&band.q_milli)
            || !(-24_000..=24_000).contains(&band.gain_millidb)
        {
            bail!(
                "match-eq operation {id} bands require 20Hz..<Nyquist, q 0.1..=24, and gain -24..=24dB"
            );
        }
    }
    Ok(())
}

fn validate_asset(
    path: &Path,
    asset: &AssetRange,
    source_timebase: AudioTimebase,
    source_format: source::RawPcmFormat,
) -> Result<()> {
    validate_sha256("operations[].asset.sha256", &asset.sha256)?;
    validate_sha256(
        "operations[].asset.decoded_pcm_sha256",
        &asset.decoded_pcm_sha256,
    )?;
    asset.timebase.validate()?;
    asset.range.validate(
        asset.timebase.samples_per_channel,
        "operations[].asset.range",
    )?;
    if asset.timebase.sample_rate_hz != source_timebase.sample_rate_hz
        || asset.timebase.channels != source_timebase.channels
        || asset.format != source_format
    {
        bail!("operation asset format, sample rate, and channels must match source");
    }
    let resolved = source::resolve(path, &asset.path);
    let hash = sha256_path(&resolved)?;
    if hash != asset.sha256.to_lowercase() {
        bail!("operation asset sha256 does not match");
    }
    if hash != asset.decoded_pcm_sha256.to_lowercase() {
        bail!("raw PCM operation asset must have identical file and decoded hashes");
    }
    let bytes = fs::metadata(&resolved)?.len();
    let expected = asset
        .timebase
        .samples_per_channel
        .checked_mul(u64::from(asset.timebase.channels))
        .and_then(|value| value.checked_mul(asset.format.bytes_per_sample()))
        .ok_or_else(|| anyhow::anyhow!("operation asset byte count overflows u64"))?;
    if bytes != expected {
        bail!("operation asset byte count does not match its timebase");
    }
    Ok(())
}

fn validate_complete_coverage(
    changed: &[SampleRange],
    locks: &[SampleRange],
    total: u64,
) -> Result<()> {
    let mut ranges = changed.iter().chain(locks).copied().collect::<Vec<_>>();
    ranges.sort_by_key(|range| range.start);
    let mut cursor = 0;
    for range in ranges {
        if range.start != cursor {
            bail!("changed_envelopes and locks must cover every source sample exactly once");
        }
        cursor = range.end;
    }
    if cursor != total {
        bail!("changed_envelopes and locks must cover every source sample exactly once");
    }
    Ok(())
}

fn is_covered(target: SampleRange, ranges: &[SampleRange]) -> bool {
    let mut relevant = ranges
        .iter()
        .copied()
        .filter(|range| target.intersects(*range))
        .collect::<Vec<_>>();
    relevant.sort_by_key(|range| range.start);
    let mut cursor = target.start;
    for range in relevant {
        if range.start > cursor {
            return false;
        }
        cursor = cursor.max(range.end.min(target.end));
        if cursor == target.end {
            return true;
        }
    }
    false
}

impl Operation {
    pub fn id(&self) -> &str {
        match self {
            Self::Keep { id, .. }
            | Self::Cut { id, .. }
            | Self::Insert { id, .. }
            | Self::Replace { id, .. }
            | Self::Repeat { id, .. }
            | Self::Move { id, .. }
            | Self::Crossfade { id, .. }
            | Self::PreserveTail { id, .. }
            | Self::MatchGain { id, .. }
            | Self::MatchEq { id, .. }
            | Self::ExtendBars { id, .. }
            | Self::Lock { id, .. } => id,
        }
    }

    fn all_ranges(&self) -> Vec<SampleRange> {
        match self {
            Self::Keep { range, .. }
            | Self::Cut { range, .. }
            | Self::Crossfade { range, .. }
            | Self::MatchGain { range, .. }
            | Self::MatchEq { range, .. }
            | Self::ExtendBars { range, .. }
            | Self::Lock { range, .. } => vec![*range],
            Self::Insert { destination, .. } | Self::Replace { destination, .. } => {
                vec![*destination]
            }
            Self::Repeat {
                source,
                destination,
                ..
            }
            | Self::Move {
                source,
                destination,
                ..
            }
            | Self::PreserveTail {
                source,
                destination,
                ..
            } => vec![*source, *destination],
        }
    }

    fn changed_ranges(&self) -> Vec<SampleRange> {
        match self {
            Self::Keep { .. } | Self::Lock { .. } => Vec::new(),
            Self::Cut { range, .. }
            | Self::Crossfade { range, .. }
            | Self::MatchGain { range, .. }
            | Self::MatchEq { range, .. }
            | Self::ExtendBars { range, .. } => vec![*range],
            Self::Insert { destination, .. }
            | Self::Replace { destination, .. }
            | Self::Repeat { destination, .. }
            | Self::PreserveTail { destination, .. } => vec![*destination],
            Self::Move {
                source,
                destination,
                ..
            } => vec![*source, *destination],
        }
    }
}
