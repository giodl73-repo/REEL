use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use tempfile::NamedTempFile;

use crate::{
    DecisionRef,
    hash::{canonical_sha256, sha256_path},
    nonempty,
    repair::{self, Operation},
    source::{self, NetworkPolicy, RawPcmFormat},
    time::{AudioTimebase, SampleRange},
    validate_sha256,
};

pub const REQUEST_SCHEMA: &str = "reel.music-external-repair-request.v0.1";
pub const PLAN_RECEIPT_SCHEMA: &str = "reel.music-external-repair-plan-receipt.v0.1";
pub const LYRIC_EVIDENCE_SCHEMA: &str = "reel.music-external-lyric-evidence.v0.1";
pub const CANDIDATE_SCHEMA: &str = "reel.music-external-repair-candidate.v0.1";

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequestManifest {
    pub schema: String,
    pub request_id: String,
    pub repair: RepairBinding,
    pub operation_id: String,
    pub region: SampleRange,
    pub target: TargetPerformance,
    pub retained_music: FileBinding,
    pub adapter: Adapter,
    pub permissions: Permissions,
    pub candidate_policy: CandidatePolicy,
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
pub struct FileBinding {
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PerformanceMode {
    ReSing,
    Repaint,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPerformance {
    pub mode: PerformanceMode,
    pub language: String,
    pub text: FileBinding,
    pub exact_text_authority_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Adapter {
    pub kind: String,
    pub version: String,
    pub executable: String,
    pub model_id: String,
    pub checkpoint_sha256: String,
    pub model_license: String,
    pub seed: u64,
    pub local_only: bool,
    pub network_policy: NetworkPolicy,
    pub auto_download: bool,
    #[serde(default)]
    pub parameters: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Permissions {
    pub voice_consent_status: String,
    pub voice_consent_evidence: Vec<DecisionRef>,
    pub third_party_upload: bool,
    pub public_release: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidatePolicy {
    pub maximum_boundary_delta_millionths: u32,
    pub maximum_region_loudness_delta_millidb: u32,
    pub minimum_lyric_coverage_millionths: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlanReceipt {
    pub schema: String,
    pub tool_version: String,
    pub request_manifest_sha256: String,
    pub request_contract_sha256: String,
    pub repair_contract_sha256: String,
    pub target_text_sha256: String,
    pub retained_music_sha256: String,
    pub region: SampleRange,
    pub adapter_kind: String,
    pub adapter_version: String,
    pub model_id: String,
    pub checkpoint_sha256: String,
    pub seed: u64,
    pub local_only: bool,
    pub network_denied: bool,
    pub auto_download: bool,
    pub independent_lyric_evidence_required: bool,
    pub human_listening_required: bool,
    pub selected: bool,
    pub released: bool,
    pub shareable: bool,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LyricEvidence {
    pub schema: String,
    pub candidate_pcm_sha256: String,
    pub target_text_sha256: String,
    pub analyzer_id: String,
    pub analyzer_version: String,
    pub coverage_millionths: u32,
    pub exact_text_matched: bool,
    pub passed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateDisposition {
    Pending,
    AuditionReady,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateManifest {
    pub schema: String,
    pub candidate_id: String,
    pub request: FileBinding,
    pub plan_receipt: FileBinding,
    pub candidate_pcm: FileBinding,
    pub format: RawPcmFormat,
    pub timebase: AudioTimebase,
    pub lyric_evidence: FileBinding,
    pub disposition: CandidateDisposition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition_decision: Option<DecisionRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub request_manifest_sha256: String,
    pub request_contract_sha256: String,
    pub repair_contract_sha256: String,
    pub target_text_sha256: String,
    pub retained_music_sha256: String,
    pub local_only: bool,
    pub independent_lyric_evidence_required: bool,
    pub human_listening_required: bool,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateReport {
    pub schema: String,
    pub candidate_manifest_sha256: String,
    pub request_contract_sha256: String,
    pub candidate_pcm_sha256: String,
    pub duration_exact: bool,
    pub outside_region_exact: bool,
    pub boundary_delta_millionths: u32,
    pub region_loudness_delta_millidb: u32,
    pub independent_lyric_evidence: bool,
    pub lyric_passed: bool,
    pub technical_passed: bool,
    pub audition_ready: bool,
    pub rejected: bool,
    pub selected: bool,
    pub released: bool,
    pub shareable: bool,
    pub verified: bool,
}

pub fn validate_request(path: &Path) -> Result<ValidationReport> {
    let (manifest, repair_report) = load_and_validate_request(path)?;
    Ok(ValidationReport {
        schema: REQUEST_SCHEMA.into(),
        request_manifest_sha256: sha256_path(path)?,
        request_contract_sha256: canonical_sha256(&manifest)?,
        repair_contract_sha256: repair_report.contract_sha256,
        target_text_sha256: manifest.target.text.sha256.to_lowercase(),
        retained_music_sha256: manifest.retained_music.sha256.to_lowercase(),
        local_only: true,
        independent_lyric_evidence_required: true,
        human_listening_required: true,
        verified: true,
    })
}

pub fn write_plan(
    path: &Path,
    receipt_path: &Path,
    tool_version: &str,
) -> Result<ValidationReport> {
    nonempty("tool_version", tool_version)?;
    if receipt_path.exists() {
        bail!(
            "external repair plan receipt already exists: {}",
            receipt_path.display()
        );
    }
    let report = validate_request(path)?;
    let manifest = load_request(path)?;
    let receipt = PlanReceipt {
        schema: PLAN_RECEIPT_SCHEMA.into(),
        tool_version: tool_version.into(),
        request_manifest_sha256: report.request_manifest_sha256.clone(),
        request_contract_sha256: report.request_contract_sha256.clone(),
        repair_contract_sha256: report.repair_contract_sha256.clone(),
        target_text_sha256: report.target_text_sha256.clone(),
        retained_music_sha256: report.retained_music_sha256.clone(),
        region: manifest.region,
        adapter_kind: manifest.adapter.kind,
        adapter_version: manifest.adapter.version,
        model_id: manifest.adapter.model_id,
        checkpoint_sha256: manifest.adapter.checkpoint_sha256,
        seed: manifest.adapter.seed,
        local_only: true,
        network_denied: true,
        auto_download: false,
        independent_lyric_evidence_required: true,
        human_listening_required: true,
        selected: false,
        released: false,
        shareable: false,
        verified: true,
    };
    publish(receipt_path, &serde_json::to_vec_pretty(&receipt)?)?;
    Ok(report)
}

pub fn validate_candidate(path: &Path) -> Result<CandidateReport> {
    let manifest: CandidateManifest = serde_yaml::from_slice(&fs::read(path)?)?;
    if manifest.schema != CANDIDATE_SCHEMA {
        bail!("external repair candidate schema must be {CANDIDATE_SCHEMA}");
    }
    nonempty("candidate_id", &manifest.candidate_id)?;
    let request_path = source::resolve(path, &manifest.request.path);
    verify_file("request", &request_path, &manifest.request.sha256)?;
    let (request, _) = load_and_validate_request(&request_path)?;
    let request_contract = canonical_sha256(&request)?;
    let receipt_path = source::resolve(path, &manifest.plan_receipt.path);
    verify_file("plan_receipt", &receipt_path, &manifest.plan_receipt.sha256)?;
    let receipt: PlanReceipt = serde_json::from_slice(&fs::read(&receipt_path)?)?;
    if receipt.schema != PLAN_RECEIPT_SCHEMA || receipt.request_contract_sha256 != request_contract
    {
        bail!("external repair plan receipt is stale or invalid");
    }
    let repair_path = source::resolve(&request_path, &request.repair.manifest);
    let repair_manifest = repair::load(&repair_path)?;
    let source_path = source::resolve(&repair_path, &repair_manifest.source.manifest);
    let source_manifest = source::load(&source_path)?;
    let source_pcm_path = source::resolve(&source_path, &source_manifest.media.path);
    let source_bytes = fs::read(source_pcm_path)?;
    let candidate_path = source::resolve(path, &manifest.candidate_pcm.path);
    verify_file(
        "candidate_pcm",
        &candidate_path,
        &manifest.candidate_pcm.sha256,
    )?;
    let candidate_bytes = fs::read(candidate_path)?;
    if manifest.format != source_manifest.media.format
        || manifest.timebase != source_manifest.media.timebase
    {
        bail!("candidate PCM format and timebase must equal the source");
    }
    let duration_exact = candidate_bytes.len() == source_bytes.len();
    let frame_bytes = u64::from(manifest.timebase.channels) * manifest.format.bytes_per_sample();
    let before = byte_slice(
        &source_bytes,
        SampleRange {
            start: 0,
            end: request.region.start,
        },
        frame_bytes,
    )?;
    let after = byte_slice(
        &source_bytes,
        SampleRange {
            start: request.region.end,
            end: manifest.timebase.samples_per_channel,
        },
        frame_bytes,
    )?;
    let outside_region_exact = duration_exact
        && byte_slice(
            &candidate_bytes,
            SampleRange {
                start: 0,
                end: request.region.start,
            },
            frame_bytes,
        )? == before
        && byte_slice(
            &candidate_bytes,
            SampleRange {
                start: request.region.end,
                end: manifest.timebase.samples_per_channel,
            },
            frame_bytes,
        )? == after;
    let source_region = decode(
        byte_slice(&source_bytes, request.region, frame_bytes)?,
        manifest.format,
    )?;
    let candidate_region = decode(
        byte_slice(&candidate_bytes, request.region, frame_bytes)?,
        manifest.format,
    )?;
    let loudness_delta = (dbfs(rms(&source_region)) - dbfs(rms(&candidate_region))).abs();
    let boundary = boundary_delta(
        &candidate_bytes,
        request.region,
        frame_bytes,
        manifest.format,
        manifest.timebase.channels,
    )?;
    let lyric_path = source::resolve(path, &manifest.lyric_evidence.path);
    verify_file(
        "lyric_evidence",
        &lyric_path,
        &manifest.lyric_evidence.sha256,
    )?;
    let lyric: LyricEvidence = serde_yaml::from_slice(&fs::read(lyric_path)?)?;
    if lyric.schema != LYRIC_EVIDENCE_SCHEMA
        || lyric.candidate_pcm_sha256 != manifest.candidate_pcm.sha256.to_lowercase()
        || lyric.target_text_sha256 != request.target.text.sha256.to_lowercase()
        || lyric.analyzer_id == request.adapter.kind
        || lyric.coverage_millionths > 1_000_000
    {
        bail!("lyric evidence is stale, self-authored, or invalid");
    }
    nonempty("lyric_evidence.analyzer_id", &lyric.analyzer_id)?;
    nonempty("lyric_evidence.analyzer_version", &lyric.analyzer_version)?;
    let lyric_passed = lyric.passed
        && lyric.exact_text_matched
        && lyric.coverage_millionths >= request.candidate_policy.minimum_lyric_coverage_millionths;
    let technical_passed = duration_exact
        && outside_region_exact
        && boundary <= request.candidate_policy.maximum_boundary_delta_millionths
        && (loudness_delta * 1_000.0).round() as u32
            <= request
                .candidate_policy
                .maximum_region_loudness_delta_millidb
        && lyric_passed;
    validate_disposition(
        technical_passed,
        manifest.disposition,
        &manifest.disposition_decision,
    )?;
    Ok(CandidateReport {
        schema: CANDIDATE_SCHEMA.into(),
        candidate_manifest_sha256: sha256_path(path)?,
        request_contract_sha256: request_contract,
        candidate_pcm_sha256: manifest.candidate_pcm.sha256.to_lowercase(),
        duration_exact,
        outside_region_exact,
        boundary_delta_millionths: boundary,
        region_loudness_delta_millidb: (loudness_delta * 1_000.0).round() as u32,
        independent_lyric_evidence: true,
        lyric_passed,
        technical_passed,
        audition_ready: manifest.disposition == CandidateDisposition::AuditionReady,
        rejected: manifest.disposition == CandidateDisposition::Rejected,
        selected: false,
        released: false,
        shareable: false,
        verified: true,
    })
}

fn load_request(path: &Path) -> Result<RequestManifest> {
    serde_yaml::from_slice(&fs::read(path)?)
        .with_context(|| format!("invalid external repair request {}", path.display()))
}

fn load_and_validate_request(path: &Path) -> Result<(RequestManifest, repair::ValidationReport)> {
    let manifest = load_request(path)?;
    if manifest.schema != REQUEST_SCHEMA {
        bail!("external repair request schema must be {REQUEST_SCHEMA}");
    }
    nonempty("request_id", &manifest.request_id)?;
    validate_sha256("repair.manifest_sha256", &manifest.repair.manifest_sha256)?;
    validate_sha256("repair.contract_sha256", &manifest.repair.contract_sha256)?;
    let repair_path = source::resolve(path, &manifest.repair.manifest);
    verify_file("repair", &repair_path, &manifest.repair.manifest_sha256)?;
    let repair_report = repair::validate(&repair_path)?;
    let repair_manifest = repair::load(&repair_path)?;
    if repair_report.contract_sha256 != manifest.repair.contract_sha256.to_lowercase()
        || repair_report.repair_id != manifest.repair.repair_id
    {
        bail!("repair binding is stale");
    }
    manifest
        .region
        .validate(repair_manifest.timebase.samples_per_channel, "region")?;
    let operation = repair_manifest
        .operations
        .iter()
        .find(|operation| operation.id() == manifest.operation_id)
        .ok_or_else(|| anyhow::anyhow!("operation_id is not in the repair"))?;
    if matches!(operation, Operation::Keep { .. } | Operation::Lock { .. }) {
        bail!("external repair must bind a mutating repair operation");
    }
    if !operation_regions(operation)
        .iter()
        .any(|range| range.contains(manifest.region))
    {
        bail!("external repair region must be contained by the bound operation");
    }
    nonempty("target.language", &manifest.target.language)?;
    validate_sha256(
        "target.exact_text_authority_sha256",
        &manifest.target.exact_text_authority_sha256,
    )?;
    verify_bound_file(path, "target.text", &manifest.target.text)?;
    if fs::read(source::resolve(path, &manifest.target.text.path))?.is_empty() {
        bail!("target text must not be empty");
    }
    verify_bound_file(path, "retained_music", &manifest.retained_music)?;
    for value in [
        &manifest.adapter.kind,
        &manifest.adapter.version,
        &manifest.adapter.executable,
        &manifest.adapter.model_id,
        &manifest.adapter.model_license,
    ] {
        nonempty("adapter field", value)?;
    }
    validate_sha256(
        "adapter.checkpoint_sha256",
        &manifest.adapter.checkpoint_sha256,
    )?;
    if !manifest.adapter.local_only
        || manifest.adapter.network_policy != NetworkPolicy::Denied
        || manifest.adapter.auto_download
    {
        bail!(
            "external repair adapter must be local-only, network-denied, and forbid auto-download"
        );
    }
    if manifest.permissions.voice_consent_status != "recorded"
        || manifest.permissions.voice_consent_evidence.is_empty()
    {
        bail!("external vocal repair requires recorded speaker-specific consent evidence");
    }
    for evidence in &manifest.permissions.voice_consent_evidence {
        nonempty("voice consent artifact_id", &evidence.artifact_id)?;
        validate_sha256("voice consent sha256", &evidence.sha256)?;
    }
    if manifest.permissions.third_party_upload || manifest.permissions.public_release {
        bail!("external repair request cannot authorize upload or release");
    }
    if manifest.candidate_policy.maximum_boundary_delta_millionths > 2_000_000
        || manifest
            .candidate_policy
            .maximum_region_loudness_delta_millidb
            > 24_000
        || manifest.candidate_policy.minimum_lyric_coverage_millionths > 1_000_000
    {
        bail!("candidate policy values are outside supported bounds");
    }
    Ok((manifest, repair_report))
}

fn operation_regions(operation: &Operation) -> Vec<SampleRange> {
    match operation {
        Operation::Keep { range, .. }
        | Operation::Cut { range, .. }
        | Operation::Crossfade { range, .. }
        | Operation::MatchGain { range, .. }
        | Operation::MatchEq { range, .. }
        | Operation::ExtendBars { range, .. }
        | Operation::Lock { range, .. } => vec![*range],
        Operation::Insert { destination, .. }
        | Operation::Replace { destination, .. }
        | Operation::Repeat { destination, .. }
        | Operation::PreserveTail { destination, .. } => vec![*destination],
        Operation::Move {
            source,
            destination,
            ..
        } => vec![*source, *destination],
    }
}

fn validate_disposition(
    passed: bool,
    disposition: CandidateDisposition,
    decision: &Option<DecisionRef>,
) -> Result<()> {
    match disposition {
        CandidateDisposition::Pending if decision.is_some() => {
            bail!("pending candidate forbids disposition decision")
        }
        CandidateDisposition::Pending => Ok(()),
        CandidateDisposition::AuditionReady if !passed => {
            bail!("audition-ready requires passing independent technical evidence")
        }
        CandidateDisposition::AuditionReady | CandidateDisposition::Rejected => {
            let decision = decision.as_ref().ok_or_else(|| {
                anyhow::anyhow!("completed candidate disposition requires a decision")
            })?;
            nonempty("disposition decision artifact_id", &decision.artifact_id)?;
            validate_sha256("disposition decision sha256", &decision.sha256)
        }
    }
}

fn verify_bound_file(base: &Path, field: &str, binding: &FileBinding) -> Result<()> {
    verify_file(
        field,
        &source::resolve(base, &binding.path),
        &binding.sha256,
    )
}
fn verify_file(field: &str, path: &Path, expected: &str) -> Result<()> {
    validate_sha256(&format!("{field}.sha256"), expected)?;
    if sha256_path(path)? != expected.to_lowercase() {
        bail!("{field} sha256 does not match");
    }
    Ok(())
}

fn byte_slice(bytes: &[u8], range: SampleRange, frame_bytes: u64) -> Result<&[u8]> {
    let start = usize::try_from(
        range
            .start
            .checked_mul(frame_bytes)
            .ok_or_else(|| anyhow::anyhow!("byte offset overflow"))?,
    )?;
    let end = usize::try_from(
        range
            .end
            .checked_mul(frame_bytes)
            .ok_or_else(|| anyhow::anyhow!("byte offset overflow"))?,
    )?;
    bytes
        .get(start..end)
        .ok_or_else(|| anyhow::anyhow!("PCM range exceeds buffer"))
}

fn decode(bytes: &[u8], format: RawPcmFormat) -> Result<Vec<f64>> {
    let width = usize::try_from(format.bytes_per_sample())?;
    bytes
        .chunks_exact(width)
        .map(|chunk| {
            Ok(match format {
                RawPcmFormat::RawPcmU8 => (f64::from(chunk[0]) - 128.0) / 128.0,
                RawPcmFormat::RawPcmS16le => {
                    f64::from(i16::from_le_bytes([chunk[0], chunk[1]])) / 32_768.0
                }
                RawPcmFormat::RawPcmS24le => {
                    let raw =
                        i32::from(chunk[0]) | i32::from(chunk[1]) << 8 | i32::from(chunk[2]) << 16;
                    f64::from(if raw & 0x80_0000 != 0 {
                        raw | !0xff_ffff
                    } else {
                        raw
                    }) / 8_388_608.0
                }
                RawPcmFormat::RawPcmS32le => {
                    f64::from(i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                        / 2_147_483_648.0
                }
                RawPcmFormat::RawPcmF32le => {
                    let value = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    if !value.is_finite() {
                        bail!("PCM float must be finite");
                    }
                    f64::from(value)
                }
            })
        })
        .collect()
}

fn rms(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        (values.iter().map(|v| v * v).sum::<f64>() / values.len() as f64).sqrt()
    }
}
fn dbfs(value: f64) -> f64 {
    20.0 * value.max(1e-12).log10()
}

fn boundary_delta(
    bytes: &[u8],
    region: SampleRange,
    frame_bytes: u64,
    format: RawPcmFormat,
    channels: u16,
) -> Result<u32> {
    let samples = decode(bytes, format)?;
    let channels = usize::from(channels);
    let frames = bytes.len() as u64 / frame_bytes;
    let mut maximum = 0.0_f64;
    for point in [region.start, region.end] {
        if point == 0 || point >= frames {
            continue;
        }
        let left = (usize::try_from(point)? - 1) * channels;
        let right = usize::try_from(point)? * channels;
        maximum = maximum.max(
            (0..channels)
                .map(|channel| (samples[left + channel] - samples[right + channel]).abs())
                .fold(0.0, f64::max),
        );
    }
    Ok((maximum.clamp(0.0, 4_294.0) * 1_000_000.0).round() as u32)
}

fn publish(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary
        .persist_noclobber(path)
        .map_err(|error| error.error)?;
    Ok(())
}
