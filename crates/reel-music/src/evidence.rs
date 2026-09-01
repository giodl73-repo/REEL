use std::{f64::consts::TAU, fs, io::Write, path::Path};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::{
    edl,
    hash::{canonical_sha256, sha256_bytes, sha256_path},
    nonempty,
    source::RawPcmFormat,
};

pub const SCHEMA: &str = "reel.music-repair-evidence.v0.1";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceReport {
    pub schema: String,
    pub edl_sha256: String,
    pub edl_contract_sha256: String,
    pub repair_manifest_sha256: String,
    pub source_pcm_sha256: String,
    pub candidate_pcm_sha256: String,
    pub adapter: String,
    pub adapter_version: String,
    pub output_samples_per_channel: u64,
    pub segments: Vec<SegmentEvidence>,
    pub joins: Vec<JoinEvidence>,
    pub violations: Vec<String>,
    pub outside_regions_exact: bool,
    pub passed: bool,
    pub verified: bool,
    pub shareable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentEvidence {
    pub id: String,
    pub source_sha256: String,
    pub output_sha256: String,
    pub samples_per_channel: u64,
    pub exact: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JoinEvidence {
    pub operation_id: String,
    pub output_join_sample: u64,
    pub window_samples: u32,
    pub boundary_delta_millionths: u32,
    pub left_rms_millidbfs: i32,
    pub right_rms_millidbfs: i32,
    pub rms_delta_millidb: u32,
    pub window_correlation_millionths: i32,
    pub spectral_distance_millionths: u32,
    pub dc_offset_delta_millionths: u32,
    pub right_tail_samples: u64,
    pub right_tail_exact: bool,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WriteReport {
    pub schema: String,
    pub evidence: String,
    pub evidence_sha256: String,
    pub evidence_contract_sha256: String,
    pub candidate_pcm_sha256: String,
    pub joins: usize,
    pub outside_regions_exact: bool,
    pub passed: bool,
    pub verified: bool,
    pub shareable: bool,
}

pub fn analyze(
    edl_path: &Path,
    repair_path: &Path,
    candidate_path: &Path,
    adapter: &str,
    adapter_version: &str,
) -> Result<EvidenceReport> {
    nonempty("adapter", adapter)?;
    nonempty("adapter_version", adapter_version)?;
    let edl_report = edl::validate(edl_path, repair_path)?;
    let edl = edl::load(edl_path)?;
    let source = fs::read(&edl.source_pcm)
        .with_context(|| format!("failed to read {}", edl.source_pcm.display()))?;
    let candidate = fs::read(candidate_path)
        .with_context(|| format!("failed to read {}", candidate_path.display()))?;
    let frame_bytes = u64::from(edl.timebase.channels) * edl.format.bytes_per_sample();
    let expected_bytes = edl
        .output_samples_per_channel
        .checked_mul(frame_bytes)
        .ok_or_else(|| anyhow::anyhow!("candidate byte count overflows u64"))?;
    if candidate.len() as u64 != expected_bytes {
        bail!(
            "candidate byte count {} does not match EDL output {}",
            candidate.len(),
            expected_bytes
        );
    }

    let mut segments = Vec::new();
    let mut violations = Vec::new();
    for segment in &edl.segments {
        let source_slice = frame_slice(&source, segment.source, frame_bytes)?;
        let output_slice = frame_slice(&candidate, segment.output, frame_bytes)?;
        let exact = source_slice == output_slice;
        if !exact {
            violations.push(format!(
                "segment {} differs from its source mapping",
                segment.id
            ));
        }
        segments.push(SegmentEvidence {
            id: segment.id.clone(),
            source_sha256: sha256_bytes(source_slice),
            output_sha256: sha256_bytes(output_slice),
            samples_per_channel: segment.source.len(),
            exact,
        });
    }
    let outside_regions_exact = segments.iter().all(|segment| segment.exact);

    let decoded = decode_pcm(&candidate, edl.format, edl.timebase.channels)?;
    let mut joins = Vec::new();
    for (index, cut) in edl.cuts.iter().enumerate() {
        let available_left = cut.output_join_sample;
        let available_right = edl.output_samples_per_channel - cut.output_join_sample;
        let window = u64::from(edl.evidence_policy.window_samples)
            .min(available_left)
            .min(available_right);
        if window < 8 {
            violations.push(format!(
                "join {} has fewer than 8 evidence samples",
                cut.operation_id
            ));
        }
        let left = sample_window(
            &decoded,
            edl.timebase.channels,
            cut.output_join_sample - window,
            cut.output_join_sample,
        )?;
        let right = sample_window(
            &decoded,
            edl.timebase.channels,
            cut.output_join_sample,
            cut.output_join_sample + window,
        )?;
        let boundary = boundary_delta(&decoded, edl.timebase.channels, cut.output_join_sample)?;
        let left_rms = rms(&left);
        let right_rms = rms(&right);
        let left_db = dbfs(left_rms);
        let right_db = dbfs(right_rms);
        let rms_delta = (left_db - right_db).abs();
        let window_correlation = cosine_correlation(&left, &right);
        let spectral = spectral_distance(
            &mono(&left, edl.timebase.channels),
            &mono(&right, edl.timebase.channels),
        );
        let dc_delta = (mean(&left) - mean(&right)).abs();
        let right_segment = &segments[index + 1];
        let mut join_violations = Vec::new();
        if millionths(boundary) > edl.evidence_policy.max_boundary_delta_millionths {
            join_violations.push("boundary-delta");
        }
        if millidb(rms_delta) > edl.evidence_policy.max_rms_delta_millidb {
            join_violations.push("window-rms-delta");
        }
        if signed_millionths(window_correlation)
            < edl.evidence_policy.min_window_correlation_millionths
        {
            join_violations.push("window-correlation");
        }
        if millionths(spectral) > edl.evidence_policy.max_spectral_distance_millionths {
            join_violations.push("spectral-distance");
        }
        if !right_segment.exact
            || right_segment.samples_per_channel < edl.evidence_policy.min_exact_right_tail_samples
        {
            join_violations.push("right-tail-identity");
        }
        for violation in &join_violations {
            violations.push(format!("join {} failed {violation}", cut.operation_id));
        }
        joins.push(JoinEvidence {
            operation_id: cut.operation_id.clone(),
            output_join_sample: cut.output_join_sample,
            window_samples: u32::try_from(window)?,
            boundary_delta_millionths: millionths(boundary),
            left_rms_millidbfs: signed_millidb(left_db),
            right_rms_millidbfs: signed_millidb(right_db),
            rms_delta_millidb: millidb(rms_delta),
            window_correlation_millionths: signed_millionths(window_correlation),
            spectral_distance_millionths: millionths(spectral),
            dc_offset_delta_millionths: millionths(dc_delta),
            right_tail_samples: right_segment.samples_per_channel,
            right_tail_exact: right_segment.exact,
            passed: join_violations.is_empty(),
        });
    }
    let passed = outside_regions_exact && violations.is_empty();
    Ok(EvidenceReport {
        schema: SCHEMA.into(),
        edl_sha256: sha256_path(edl_path)?,
        edl_contract_sha256: edl_report.edl_contract_sha256,
        repair_manifest_sha256: edl.repair_manifest_sha256,
        source_pcm_sha256: edl.source_pcm_sha256,
        candidate_pcm_sha256: sha256_path(candidate_path)?,
        adapter: adapter.into(),
        adapter_version: adapter_version.into(),
        output_samples_per_channel: edl.output_samples_per_channel,
        segments,
        joins,
        violations,
        outside_regions_exact,
        passed,
        verified: true,
        shareable: false,
    })
}

pub fn write(path: &Path, report: &EvidenceReport) -> Result<WriteReport> {
    if path.exists() {
        bail!("repair evidence output already exists: {}", path.display());
    }
    let bytes = serde_json::to_vec_pretty(report)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(&bytes)?;
    temporary.flush()?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish {}", path.display()))?;
    write_report(path, report)
}

pub fn check(
    evidence_path: &Path,
    edl_path: &Path,
    repair_path: &Path,
    candidate_path: &Path,
) -> Result<WriteReport> {
    let bytes = fs::read(evidence_path)
        .with_context(|| format!("failed to read {}", evidence_path.display()))?;
    let saved: EvidenceReport = serde_json::from_slice(&bytes)?;
    if saved.schema != SCHEMA || !saved.verified || saved.shareable {
        bail!("invalid local repair evidence contract");
    }
    let current = analyze(
        edl_path,
        repair_path,
        candidate_path,
        &saved.adapter,
        &saved.adapter_version,
    )?;
    if saved != current {
        bail!("repair evidence does not match current EDL, source, or candidate");
    }
    write_report(evidence_path, &saved)
}

fn write_report(path: &Path, report: &EvidenceReport) -> Result<WriteReport> {
    Ok(WriteReport {
        schema: SCHEMA.into(),
        evidence: path.display().to_string(),
        evidence_sha256: sha256_path(path)?,
        evidence_contract_sha256: canonical_sha256(report)?,
        candidate_pcm_sha256: report.candidate_pcm_sha256.clone(),
        joins: report.joins.len(),
        outside_regions_exact: report.outside_regions_exact,
        passed: report.passed,
        verified: true,
        shareable: false,
    })
}

fn frame_slice(bytes: &[u8], range: crate::time::SampleRange, frame_bytes: u64) -> Result<&[u8]> {
    let start = usize::try_from(
        range
            .start
            .checked_mul(frame_bytes)
            .ok_or_else(|| anyhow::anyhow!("frame offset overflow"))?,
    )?;
    let end = usize::try_from(
        range
            .end
            .checked_mul(frame_bytes)
            .ok_or_else(|| anyhow::anyhow!("frame offset overflow"))?,
    )?;
    bytes
        .get(start..end)
        .ok_or_else(|| anyhow::anyhow!("PCM range exceeds buffer"))
}

fn decode_pcm(bytes: &[u8], format: RawPcmFormat, channels: u16) -> Result<Vec<f64>> {
    let width = usize::try_from(format.bytes_per_sample())?;
    let frame = width * usize::from(channels);
    if bytes.len() % frame != 0 {
        bail!("PCM byte count is not frame aligned");
    }
    bytes
        .chunks_exact(width)
        .map(|sample| decode_sample(sample, format))
        .collect()
}

fn decode_sample(bytes: &[u8], format: RawPcmFormat) -> Result<f64> {
    let value = match format {
        RawPcmFormat::RawPcmU8 => (f64::from(bytes[0]) - 128.0) / 128.0,
        RawPcmFormat::RawPcmS16le => f64::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32_768.0,
        RawPcmFormat::RawPcmS24le => {
            let raw =
                i32::from(bytes[0]) | (i32::from(bytes[1]) << 8) | (i32::from(bytes[2]) << 16);
            let signed = if raw & 0x80_0000 != 0 {
                raw | !0xff_ffff
            } else {
                raw
            };
            f64::from(signed) / 8_388_608.0
        }
        RawPcmFormat::RawPcmS32le => {
            f64::from(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                / 2_147_483_648.0
        }
        RawPcmFormat::RawPcmF32le => {
            let value = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            if !value.is_finite() {
                bail!("PCM float sample must be finite");
            }
            f64::from(value)
        }
    };
    Ok(value)
}

fn sample_window(samples: &[f64], channels: u16, start: u64, end: u64) -> Result<Vec<f64>> {
    let channels = usize::from(channels);
    let start = usize::try_from(start)? * channels;
    let end = usize::try_from(end)? * channels;
    Ok(samples
        .get(start..end)
        .ok_or_else(|| anyhow::anyhow!("evidence window exceeds PCM buffer"))?
        .to_vec())
}

fn boundary_delta(samples: &[f64], channels: u16, join: u64) -> Result<f64> {
    if join == 0 {
        bail!("join cannot be at sample zero");
    }
    let channels = usize::from(channels);
    let left = (usize::try_from(join)? - 1) * channels;
    let right = usize::try_from(join)? * channels;
    Ok((0..channels)
        .map(|channel| (samples[left + channel] - samples[right + channel]).abs())
        .fold(0.0, f64::max))
}

fn rms(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    (values.iter().map(|value| value * value).sum::<f64>() / values.len() as f64).sqrt()
}

fn dbfs(value: f64) -> f64 {
    20.0 * value.max(1e-12).log10()
}
fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn cosine_correlation(left: &[f64], right: &[f64]) -> f64 {
    let dot = left.iter().zip(right).map(|(a, b)| a * b).sum::<f64>();
    let norms = left.iter().map(|v| v * v).sum::<f64>().sqrt()
        * right.iter().map(|v| v * v).sum::<f64>().sqrt();
    if norms <= 1e-12 {
        1.0
    } else {
        (dot / norms).clamp(-1.0, 1.0)
    }
}

fn mono(values: &[f64], channels: u16) -> Vec<f64> {
    let channels = usize::from(channels);
    values
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f64>() / channels as f64)
        .collect()
}

fn spectral_distance(left: &[f64], right: &[f64]) -> f64 {
    let size = left.len().min(right.len()).min(256);
    if size == 0 {
        return 0.0;
    }
    let bins = size / 2 + 1;
    let spectrum = |values: &[f64]| -> Vec<f64> {
        (0..bins)
            .map(|bin| {
                let (real, imaginary) = values.iter().take(size).enumerate().fold(
                    (0.0, 0.0),
                    |(real, imaginary), (index, value)| {
                        let angle = TAU * bin as f64 * index as f64 / size as f64;
                        (real + value * angle.cos(), imaginary - value * angle.sin())
                    },
                );
                (real * real + imaginary * imaginary).sqrt()
            })
            .collect()
    };
    let mut a = spectrum(left);
    let mut b = spectrum(right);
    let sum_a = a.iter().sum::<f64>().max(1e-12);
    let sum_b = b.iter().sum::<f64>().max(1e-12);
    for value in &mut a {
        *value /= sum_a;
    }
    for value in &mut b {
        *value /= sum_b;
    }
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum::<f64>() / 2.0
}

fn millionths(value: f64) -> u32 {
    (value.clamp(0.0, 4_294.0) * 1_000_000.0).round() as u32
}
fn signed_millionths(value: f64) -> i32 {
    (value.clamp(-1.0, 1.0) * 1_000_000.0).round() as i32
}
fn millidb(value: f64) -> u32 {
    (value.clamp(0.0, 4_294_000.0) * 1_000.0).round() as u32
}
fn signed_millidb(value: f64) -> i32 {
    (value.clamp(-2_147_000.0, 2_147_000.0) * 1_000.0).round() as i32
}
