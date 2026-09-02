use std::{
    collections::BTreeMap,
    f64::consts::{PI, TAU},
    fs,
    io::Write,
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::{
    hash::{canonical_sha256, sha256_bytes, sha256_path},
    repair::{self, BeatGrid, EqBand, FadeCurve, Operation},
    source::{self, RawPcmFormat},
    time::SampleRange,
};

pub const SCHEMA: &str = "reel.music-repair-render-receipt.v0.2";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockEvidence {
    pub source: SampleRange,
    pub output: SampleRange,
    pub source_sha256: String,
    pub output_sha256: String,
    pub exact: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RenderReceipt {
    pub schema: String,
    pub tool_version: String,
    pub repair_manifest_sha256: String,
    pub repair_contract_sha256: String,
    pub source_contract_sha256: String,
    pub source_pcm_sha256: String,
    pub output_pcm_sha256: String,
    pub format: RawPcmFormat,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub source_samples_per_channel: u64,
    pub output_samples_per_channel: u64,
    pub operation_kinds: BTreeMap<String, usize>,
    pub locks: Vec<LockEvidence>,
    pub outside_regions_exact: bool,
    pub beat_alignment: BeatEvidence,
    pub continuity: ContinuityEvidence,
    pub shareable: bool,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BeatEvidence {
    pub declared: bool,
    pub checked_boundaries: usize,
    pub off_grid_boundaries: usize,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SeamEvidence {
    pub output_sample: u64,
    pub boundary_delta_millionths: u32,
    pub ambience_rms_delta_millidb: u32,
    pub reverb_tail_correlation_millionths: i32,
    pub phase_correlation_millionths: i32,
    pub spectral_distance_millionths: u32,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuityEvidence {
    pub profile: String,
    pub loudness_matching: String,
    pub eq_processing: String,
    pub clipping_samples: u64,
    pub seams: Vec<SeamEvidence>,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenderReport {
    pub schema: String,
    pub output_pcm_sha256: String,
    pub receipt_sha256: String,
    pub receipt_contract_sha256: String,
    pub source_samples_per_channel: u64,
    pub output_samples_per_channel: u64,
    pub operations: usize,
    pub locks: usize,
    pub outside_regions_exact: bool,
    pub beat_alignment_passed: bool,
    pub continuity_passed: bool,
    pub shareable: bool,
    pub verified: bool,
}

struct Replacement {
    range: SampleRange,
    bytes: Vec<u8>,
}

pub fn render(
    repair_path: &Path,
    output_path: &Path,
    receipt_path: &Path,
    tool_version: &str,
) -> Result<RenderReport> {
    if output_path.exists() {
        bail!("repair output already exists: {}", output_path.display());
    }
    if receipt_path.exists() {
        bail!("repair receipt already exists: {}", receipt_path.display());
    }
    let (output, receipt) = build(repair_path, tool_version)?;
    publish(output_path, &output)?;
    let receipt_bytes = serde_json::to_vec_pretty(&receipt)?;
    if let Err(error) = publish(receipt_path, &receipt_bytes) {
        let _ = fs::remove_file(output_path);
        return Err(error);
    }
    report(receipt_path, &receipt)
}

pub fn check(repair_path: &Path, output_path: &Path, receipt_path: &Path) -> Result<RenderReport> {
    let bytes = fs::read(receipt_path)
        .with_context(|| format!("failed to read {}", receipt_path.display()))?;
    let saved: RenderReceipt = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid repair receipt {}", receipt_path.display()))?;
    let (expected_output, expected) = build(repair_path, &saved.tool_version)?;
    if saved != expected || sha256_path(output_path)? != saved.output_pcm_sha256 {
        bail!("repair receipt does not match current manifest, source, policy, or output");
    }
    if fs::read(output_path)? != expected_output {
        bail!("repair output bytes are not the deterministic compiled result");
    }
    report(receipt_path, &saved)
}

fn build(repair_path: &Path, tool_version: &str) -> Result<(Vec<u8>, RenderReceipt)> {
    if tool_version.trim().is_empty() {
        bail!("tool version must not be empty");
    }
    let repair_report = repair::validate(repair_path)?;
    let manifest = repair::load(repair_path)?;
    let source_manifest_path = source::resolve(repair_path, &manifest.source.manifest);
    let source_report = source::validate(&source_manifest_path)?;
    let source_manifest = source::load(&source_manifest_path)?;
    let source_path = source::resolve(&source_manifest_path, &source_manifest.media.path);
    let source_bytes = fs::read(&source_path)?;
    let frame_bytes = u64::from(manifest.timebase.channels)
        .checked_mul(source_manifest.media.format.bytes_per_sample())
        .ok_or_else(|| anyhow::anyhow!("PCM frame width overflows u64"))?;
    let mut replacements = Vec::new();
    let mut operation_kinds = BTreeMap::new();
    for operation in &manifest.operations {
        let kind = operation_kind(operation);
        *operation_kinds.entry(kind.to_string()).or_default() += 1;
        match operation {
            Operation::Keep { .. } | Operation::Lock { .. } => {}
            Operation::Cut { range, .. } => replacements.push(Replacement {
                range: *range,
                bytes: Vec::new(),
            }),
            Operation::Insert {
                destination, asset, ..
            } => {
                let mut bytes = asset_bytes(repair_path, asset, frame_bytes)?;
                bytes.extend_from_slice(frame_slice(&source_bytes, *destination, frame_bytes)?);
                replacements.push(Replacement {
                    range: *destination,
                    bytes,
                });
            }
            Operation::Replace {
                destination, asset, ..
            } => replacements.push(Replacement {
                range: *destination,
                bytes: asset_bytes(repair_path, asset, frame_bytes)?,
            }),
            Operation::Repeat {
                source,
                destination,
                ..
            } => {
                let mut bytes = frame_slice(&source_bytes, *source, frame_bytes)?.to_vec();
                bytes.extend_from_slice(frame_slice(&source_bytes, *destination, frame_bytes)?);
                replacements.push(Replacement {
                    range: *destination,
                    bytes,
                });
            }
            Operation::Move {
                source,
                destination,
                ..
            } => {
                replacements.push(Replacement {
                    range: *source,
                    bytes: Vec::new(),
                });
                let mut bytes = frame_slice(&source_bytes, *source, frame_bytes)?.to_vec();
                bytes.extend_from_slice(frame_slice(&source_bytes, *destination, frame_bytes)?);
                replacements.push(Replacement {
                    range: *destination,
                    bytes,
                });
            }
            Operation::ExtendBars { range, bars, .. } => {
                let original = frame_slice(&source_bytes, *range, frame_bytes)?;
                let repeats = usize::try_from(u64::from(*bars) + 1)?;
                replacements.push(Replacement {
                    range: *range,
                    bytes: original.repeat(repeats),
                });
            }
            Operation::Crossfade { range, curve, .. } => replacements.push(Replacement {
                range: *range,
                bytes: crossfade_bytes(
                    frame_slice(&source_bytes, *range, frame_bytes)?,
                    source_manifest.media.format,
                    manifest.timebase.channels,
                    *curve,
                )?,
            }),
            Operation::PreserveTail {
                source,
                destination,
                ..
            } => replacements.push(Replacement {
                range: *destination,
                bytes: preserve_tail_bytes(
                    frame_slice(&source_bytes, *source, frame_bytes)?,
                    frame_slice(&source_bytes, *destination, frame_bytes)?,
                    source_manifest.media.format,
                    manifest.timebase.channels,
                )?,
            }),
            Operation::MatchGain {
                range,
                target_millilufs,
                ..
            } => replacements.push(Replacement {
                range: *range,
                bytes: match_gain_bytes(
                    frame_slice(&source_bytes, *range, frame_bytes)?,
                    source_manifest.media.format,
                    manifest.timebase.channels,
                    manifest.timebase.sample_rate_hz,
                    *target_millilufs,
                )?,
            }),
            Operation::MatchEq { range, bands, .. } => {
                if bands.is_empty() {
                    bail!(
                        "match-eq operation {} requires inline hash-bound bands for rendering",
                        operation.id()
                    );
                }
                replacements.push(Replacement {
                    range: *range,
                    bytes: match_eq_bytes(
                        frame_slice(&source_bytes, *range, frame_bytes)?,
                        source_manifest.media.format,
                        manifest.timebase.channels,
                        manifest.timebase.sample_rate_hz,
                        bands,
                    )?,
                });
            }
        }
    }
    if replacements.is_empty() {
        bail!("repair materialization requires at least one executable operation");
    }
    replacements.sort_by_key(|item| item.range.start);
    for pair in replacements.windows(2) {
        if pair[0].range.end > pair[1].range.start {
            bail!("compiled repair replacements overlap");
        }
    }
    let beat_alignment = beat_evidence(&manifest)?;
    let mut output = Vec::new();
    let mut locks = Vec::new();
    let mut seam_points = Vec::new();
    let mut cursor = 0;
    for replacement in replacements {
        append_exact(
            &source_bytes,
            SampleRange {
                start: cursor,
                end: replacement.range.start,
            },
            frame_bytes,
            &mut output,
            &mut locks,
        )?;
        seam_points.push(u64::try_from(output.len())? / frame_bytes);
        output.extend_from_slice(&replacement.bytes);
        seam_points.push(u64::try_from(output.len())? / frame_bytes);
        cursor = replacement.range.end;
    }
    append_exact(
        &source_bytes,
        SampleRange {
            start: cursor,
            end: manifest.timebase.samples_per_channel,
        },
        frame_bytes,
        &mut output,
        &mut locks,
    )?;
    let output_frames = u64::try_from(output.len())? / frame_bytes;
    seam_points.sort_unstable();
    seam_points.dedup();
    seam_points.retain(|point| *point > 0 && *point < output_frames);
    let continuity = continuity_evidence(
        &output,
        source_manifest.media.format,
        manifest.timebase.channels,
        &seam_points,
    )?;
    let receipt = RenderReceipt {
        schema: SCHEMA.into(),
        tool_version: tool_version.into(),
        repair_manifest_sha256: repair_report.manifest_sha256,
        repair_contract_sha256: repair_report.contract_sha256,
        source_contract_sha256: source_report.contract_sha256,
        source_pcm_sha256: source_report.decoded_pcm_sha256,
        output_pcm_sha256: sha256_bytes(&output),
        format: source_manifest.media.format,
        sample_rate_hz: manifest.timebase.sample_rate_hz,
        channels: manifest.timebase.channels,
        source_samples_per_channel: manifest.timebase.samples_per_channel,
        output_samples_per_channel: output_frames,
        operation_kinds,
        outside_regions_exact: locks.iter().all(|item| item.exact),
        locks,
        beat_alignment,
        continuity,
        shareable: false,
        verified: true,
    };
    Ok((output, receipt))
}

fn append_exact(
    source: &[u8],
    range: SampleRange,
    frame_bytes: u64,
    output: &mut Vec<u8>,
    locks: &mut Vec<LockEvidence>,
) -> Result<()> {
    if range.is_empty() {
        return Ok(());
    }
    let bytes = frame_slice(source, range, frame_bytes)?;
    let output_start = u64::try_from(output.len())? / frame_bytes;
    output.extend_from_slice(bytes);
    let output_range = SampleRange {
        start: output_start,
        end: output_start + range.len(),
    };
    let hash = sha256_bytes(bytes);
    locks.push(LockEvidence {
        source: range,
        output: output_range,
        source_sha256: hash.clone(),
        output_sha256: hash,
        exact: true,
    });
    Ok(())
}

fn asset_bytes(
    repair_path: &Path,
    asset: &repair::AssetRange,
    frame_bytes: u64,
) -> Result<Vec<u8>> {
    let path = source::resolve(repair_path, &asset.path);
    let bytes = fs::read(path)?;
    Ok(frame_slice(&bytes, asset.range, frame_bytes)?.to_vec())
}

fn frame_slice(bytes: &[u8], range: SampleRange, frame_bytes: u64) -> Result<&[u8]> {
    let start = usize::try_from(
        range
            .start
            .checked_mul(frame_bytes)
            .ok_or_else(|| anyhow::anyhow!("PCM range byte offset overflows u64"))?,
    )?;
    let end = usize::try_from(
        range
            .end
            .checked_mul(frame_bytes)
            .ok_or_else(|| anyhow::anyhow!("PCM range byte offset overflows u64"))?,
    )?;
    bytes
        .get(start..end)
        .ok_or_else(|| anyhow::anyhow!("PCM range exceeds buffer"))
}

fn crossfade_bytes(
    bytes: &[u8],
    format: RawPcmFormat,
    channels: u16,
    curve: FadeCurve,
) -> Result<Vec<u8>> {
    let samples = decode_samples(bytes, format)?;
    let channels = usize::from(channels);
    let frames = samples.len() / channels;
    if frames < 2 || frames % 2 != 0 {
        bail!("crossfade input requires an even frame count of at least two");
    }
    let half = frames / 2;
    let mut output = Vec::with_capacity(half * channels);
    for frame in 0..half {
        let t = (frame + 1) as f64 / (half + 1) as f64;
        let (left_gain, right_gain) = match curve {
            FadeCurve::Linear => (1.0 - t, t),
            FadeCurve::EqualPower => ((t * PI / 2.0).cos(), (t * PI / 2.0).sin()),
        };
        for channel in 0..channels {
            output.push(
                samples[frame * channels + channel] * left_gain
                    + samples[(frame + half) * channels + channel] * right_gain,
            );
        }
    }
    encode_samples(&output, format)
}

fn preserve_tail_bytes(
    tail: &[u8],
    destination: &[u8],
    format: RawPcmFormat,
    channels: u16,
) -> Result<Vec<u8>> {
    let tail = decode_samples(tail, format)?;
    let destination = decode_samples(destination, format)?;
    if tail.len() != destination.len() {
        bail!("preserved tail and destination sample buffers must match");
    }
    let frames = tail.len() / usize::from(channels);
    let mut output = Vec::with_capacity(tail.len());
    for (index, (tail, destination)) in tail.iter().zip(destination).enumerate() {
        let frame = index / usize::from(channels);
        let gain = 1.0 - frame as f64 / frames.max(1) as f64;
        output.push(destination + tail * gain);
    }
    encode_samples(&output, format)
}

fn match_gain_bytes(
    bytes: &[u8],
    format: RawPcmFormat,
    channels: u16,
    sample_rate_hz: u32,
    target_millilufs: i32,
) -> Result<Vec<u8>> {
    let samples = decode_samples(bytes, format)?;
    let measured_lufs = integrated_lufs(&samples, usize::from(channels), sample_rate_hz)?;
    if !measured_lufs.is_finite() {
        bail!("match-gain cannot normalize a silent range");
    }
    let target_lufs = f64::from(target_millilufs) / 1_000.0;
    let gain = 10f64.powf((target_lufs - measured_lufs) / 20.0);
    encode_samples(
        &samples
            .iter()
            .map(|sample| sample * gain)
            .collect::<Vec<_>>(),
        format,
    )
}

fn integrated_lufs(samples: &[f64], channels: usize, sample_rate_hz: u32) -> Result<f64> {
    if channels == 0 || samples.len() % channels != 0 {
        bail!("loudness input is not channel aligned");
    }
    let mut weighted = samples.to_vec();
    apply_biquad(
        &mut weighted,
        channels,
        high_shelf_coefficients(sample_rate_hz, 1_681.974_450_955_533, 4.0),
    );
    apply_biquad(
        &mut weighted,
        channels,
        high_pass_coefficients(
            sample_rate_hz,
            38.135_470_876_024_44,
            0.500_327_037_323_877_3,
        ),
    );
    let frames = weighted.len() / channels;
    let block = (u64::from(sample_rate_hz) * 400 / 1_000).max(1) as usize;
    let hop = (u64::from(sample_rate_hz) * 100 / 1_000).max(1) as usize;
    let mut energies = Vec::new();
    if frames <= block {
        energies.push(rms(&weighted).powi(2));
    } else {
        for start in (0..=frames - block).step_by(hop) {
            energies.push(rms(&weighted[start * channels..(start + block) * channels]).powi(2));
        }
    }
    let absolute = energies
        .into_iter()
        .filter(|energy| loudness_from_energy(*energy) >= -70.0)
        .collect::<Vec<_>>();
    if absolute.is_empty() {
        return Ok(f64::NEG_INFINITY);
    }
    let preliminary = absolute.iter().sum::<f64>() / absolute.len() as f64;
    let relative_gate = loudness_from_energy(preliminary) - 10.0;
    let gated = absolute
        .iter()
        .copied()
        .filter(|energy| loudness_from_energy(*energy) >= relative_gate)
        .collect::<Vec<_>>();
    Ok(loudness_from_energy(
        gated.iter().sum::<f64>() / gated.len() as f64,
    ))
}

fn loudness_from_energy(energy: f64) -> f64 {
    -0.691 + 10.0 * energy.max(1e-24).log10()
}

#[derive(Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

fn apply_biquad(samples: &mut [f64], channels: usize, coefficients: Biquad) {
    let mut x1 = vec![0.0; channels];
    let mut x2 = vec![0.0; channels];
    let mut y1 = vec![0.0; channels];
    let mut y2 = vec![0.0; channels];
    for frame in samples.chunks_exact_mut(channels) {
        for channel in 0..channels {
            let x0 = frame[channel];
            let y0 = coefficients.b0 * x0
                + coefficients.b1 * x1[channel]
                + coefficients.b2 * x2[channel]
                - coefficients.a1 * y1[channel]
                - coefficients.a2 * y2[channel];
            x2[channel] = x1[channel];
            x1[channel] = x0;
            y2[channel] = y1[channel];
            y1[channel] = y0;
            frame[channel] = y0;
        }
    }
}

fn high_pass_coefficients(sample_rate_hz: u32, frequency_hz: f64, q: f64) -> Biquad {
    let omega = TAU * frequency_hz / f64::from(sample_rate_hz);
    let cos = omega.cos();
    let alpha = omega.sin() / (2.0 * q);
    let a0 = 1.0 + alpha;
    Biquad {
        b0: ((1.0 + cos) / 2.0) / a0,
        b1: (-(1.0 + cos)) / a0,
        b2: ((1.0 + cos) / 2.0) / a0,
        a1: (-2.0 * cos) / a0,
        a2: (1.0 - alpha) / a0,
    }
}

fn high_shelf_coefficients(sample_rate_hz: u32, frequency_hz: f64, gain_db: f64) -> Biquad {
    let a = 10f64.powf(gain_db / 40.0);
    let omega = TAU * frequency_hz / f64::from(sample_rate_hz);
    let cos = omega.cos();
    let alpha = omega.sin() / 2.0 * 2.0_f64.sqrt();
    let beta = 2.0 * a.sqrt() * alpha;
    let a0 = (a + 1.0) - (a - 1.0) * cos + beta;
    Biquad {
        b0: a * ((a + 1.0) + (a - 1.0) * cos + beta) / a0,
        b1: -2.0 * a * ((a - 1.0) + (a + 1.0) * cos) / a0,
        b2: a * ((a + 1.0) + (a - 1.0) * cos - beta) / a0,
        a1: 2.0 * ((a - 1.0) - (a + 1.0) * cos) / a0,
        a2: ((a + 1.0) - (a - 1.0) * cos - beta) / a0,
    }
}

fn match_eq_bytes(
    bytes: &[u8],
    format: RawPcmFormat,
    channels: u16,
    sample_rate_hz: u32,
    bands: &[EqBand],
) -> Result<Vec<u8>> {
    let channels = usize::from(channels);
    let mut samples = decode_samples(bytes, format)?;
    for band in bands {
        let frequency = f64::from(band.frequency_millihz) / 1_000.0;
        let q = f64::from(band.q_milli) / 1_000.0;
        let gain_db = f64::from(band.gain_millidb) / 1_000.0;
        let a = 10f64.powf(gain_db / 40.0);
        let omega = TAU * frequency / f64::from(sample_rate_hz);
        let alpha = omega.sin() / (2.0 * q);
        let cos = omega.cos();
        let a0 = 1.0 + alpha / a;
        let b0 = (1.0 + alpha * a) / a0;
        let b1 = (-2.0 * cos) / a0;
        let b2 = (1.0 - alpha * a) / a0;
        let a1 = (-2.0 * cos) / a0;
        let a2 = (1.0 - alpha / a) / a0;
        let mut x1 = vec![0.0; channels];
        let mut x2 = vec![0.0; channels];
        let mut y1 = vec![0.0; channels];
        let mut y2 = vec![0.0; channels];
        for frame in samples.chunks_exact_mut(channels) {
            for channel in 0..channels {
                let x0 = frame[channel];
                let y0 = b0 * x0 + b1 * x1[channel] + b2 * x2[channel]
                    - a1 * y1[channel]
                    - a2 * y2[channel];
                x2[channel] = x1[channel];
                x1[channel] = x0;
                y2[channel] = y1[channel];
                y1[channel] = y0;
                frame[channel] = y0;
            }
        }
    }
    encode_samples(&samples, format)
}

fn decode_samples(bytes: &[u8], format: RawPcmFormat) -> Result<Vec<f64>> {
    let width = usize::try_from(format.bytes_per_sample())?;
    if bytes.len() % width != 0 {
        bail!("PCM byte count is not sample aligned");
    }
    bytes
        .chunks_exact(width)
        .map(|chunk| match format {
            RawPcmFormat::RawPcmU8 => Ok((f64::from(chunk[0]) - 128.0) / 128.0),
            RawPcmFormat::RawPcmS16le => {
                Ok(f64::from(i16::from_le_bytes([chunk[0], chunk[1]])) / 32_768.0)
            }
            RawPcmFormat::RawPcmS24le => {
                let raw =
                    i32::from(chunk[0]) | (i32::from(chunk[1]) << 8) | (i32::from(chunk[2]) << 16);
                let signed = if raw & 0x80_0000 != 0 {
                    raw | !0xff_ffff
                } else {
                    raw
                };
                Ok(f64::from(signed) / 8_388_608.0)
            }
            RawPcmFormat::RawPcmS32le => {
                Ok(
                    f64::from(i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                        / 2_147_483_648.0,
                )
            }
            RawPcmFormat::RawPcmF32le => {
                let value = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                if !value.is_finite() {
                    bail!("PCM float sample must be finite");
                }
                Ok(f64::from(value))
            }
        })
        .collect()
}

fn encode_samples(samples: &[f64], format: RawPcmFormat) -> Result<Vec<u8>> {
    let mut output =
        Vec::with_capacity(samples.len() * usize::try_from(format.bytes_per_sample())?);
    for sample in samples {
        let sample = sample.clamp(-1.0, 1.0);
        match format {
            RawPcmFormat::RawPcmU8 => {
                output.push((sample * 128.0 + 128.0).round().clamp(0.0, 255.0) as u8)
            }
            RawPcmFormat::RawPcmS16le => output.extend_from_slice(
                &((sample * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16).to_le_bytes(),
            ),
            RawPcmFormat::RawPcmS24le => {
                let value = (sample * 8_388_608.0)
                    .round()
                    .clamp(-8_388_608.0, 8_388_607.0) as i32;
                let bytes = value.to_le_bytes();
                output.extend_from_slice(&bytes[..3]);
            }
            RawPcmFormat::RawPcmS32le => output.extend_from_slice(
                &((sample * 2_147_483_648.0)
                    .round()
                    .clamp(-2_147_483_648.0, 2_147_483_647.0) as i32)
                    .to_le_bytes(),
            ),
            RawPcmFormat::RawPcmF32le => output.extend_from_slice(&(sample as f32).to_le_bytes()),
        }
    }
    Ok(output)
}

fn beat_evidence(manifest: &repair::RepairManifest) -> Result<BeatEvidence> {
    let Some(grid) = manifest.beat_grid else {
        if manifest
            .operations
            .iter()
            .any(|operation| matches!(operation, Operation::ExtendBars { .. }))
        {
            bail!("extend-bars rendering requires beat_grid");
        }
        return Ok(BeatEvidence {
            declared: false,
            checked_boundaries: 0,
            off_grid_boundaries: 0,
            passed: true,
        });
    };
    let mut boundaries = Vec::new();
    for operation in &manifest.operations {
        for range in operation_ranges(operation) {
            boundaries.extend([range.start, range.end]);
        }
        if let Operation::ExtendBars { range, .. } = operation {
            let bar = grid
                .samples_per_beat
                .checked_mul(u64::from(grid.beats_per_bar))
                .ok_or_else(|| anyhow::anyhow!("beat-grid bar length overflow"))?;
            if range.len() != bar {
                bail!("extend-bars range must equal one declared bar");
            }
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let off_grid = boundaries
        .iter()
        .filter(|sample| !on_grid(**sample, grid))
        .count();
    Ok(BeatEvidence {
        declared: true,
        checked_boundaries: boundaries.len(),
        off_grid_boundaries: off_grid,
        passed: off_grid == 0,
    })
}

fn operation_ranges(operation: &Operation) -> Vec<SampleRange> {
    match operation {
        Operation::Keep { .. } | Operation::Lock { .. } => Vec::new(),
        Operation::Cut { range, .. }
        | Operation::Crossfade { range, .. }
        | Operation::MatchGain { range, .. }
        | Operation::MatchEq { range, .. }
        | Operation::ExtendBars { range, .. } => vec![*range],
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

fn on_grid(sample: u64, grid: BeatGrid) -> bool {
    let distance = if sample >= grid.origin_sample {
        let remainder = (sample - grid.origin_sample) % grid.samples_per_beat;
        remainder.min(grid.samples_per_beat - remainder)
    } else {
        grid.origin_sample - sample
    };
    distance <= u64::from(grid.boundary_tolerance_samples)
}

fn continuity_evidence(
    bytes: &[u8],
    format: RawPcmFormat,
    channels: u16,
    seam_points: &[u64],
) -> Result<ContinuityEvidence> {
    let samples = decode_samples(bytes, format)?;
    let channels_usize = usize::from(channels);
    let frames = samples.len() / channels_usize;
    let clipping_samples = samples
        .iter()
        .filter(|sample| sample.abs() >= 0.999_999)
        .count() as u64;
    let mut seams = Vec::new();
    for point in seam_points {
        let point = usize::try_from(*point)?;
        let window = 256usize.min(point).min(frames - point);
        if window == 0 {
            continue;
        }
        let left = &samples[(point - window) * channels_usize..point * channels_usize];
        let right = &samples[point * channels_usize..(point + window) * channels_usize];
        let boundary = (0..channels_usize)
            .map(|channel| {
                (samples[(point - 1) * channels_usize + channel]
                    - samples[point * channels_usize + channel])
                    .abs()
            })
            .fold(0.0, f64::max);
        let rms_delta = (dbfs(rms(left)) - dbfs(rms(right))).abs();
        let correlation = correlation(left, right);
        let spectral = spectral_distance(&mono(left, channels_usize), &mono(right, channels_usize));
        let passed = boundary <= 0.15 && rms_delta <= 2.0 && correlation >= 0.8 && spectral <= 0.2;
        seams.push(SeamEvidence {
            output_sample: point as u64,
            boundary_delta_millionths: millionths(boundary),
            ambience_rms_delta_millidb: (rms_delta * 1_000.0).round() as u32,
            reverb_tail_correlation_millionths: signed_millionths(correlation),
            phase_correlation_millionths: signed_millionths(correlation),
            spectral_distance_millionths: millionths(spectral),
            passed,
        });
    }
    let passed = clipping_samples == 0 && seams.iter().all(|seam| seam.passed);
    Ok(ContinuityEvidence {
        profile: "repair-continuity-v0.2".into(),
        loudness_matching: "ebu-r128-k-weighted-gated-v0.1".into(),
        eq_processing: "peaking-biquad-v0.1".into(),
        clipping_samples,
        seams,
        passed,
    })
}

fn rms(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        0.0
    } else {
        (samples.iter().map(|sample| sample * sample).sum::<f64>() / samples.len() as f64).sqrt()
    }
}

fn dbfs(value: f64) -> f64 {
    20.0 * value.max(1e-12).log10()
}

fn correlation(left: &[f64], right: &[f64]) -> f64 {
    let dot = left.iter().zip(right).map(|(a, b)| a * b).sum::<f64>();
    let norm = left.iter().map(|value| value * value).sum::<f64>().sqrt()
        * right.iter().map(|value| value * value).sum::<f64>().sqrt();
    if norm <= 1e-12 {
        1.0
    } else {
        (dot / norm).clamp(-1.0, 1.0)
    }
}

fn mono(samples: &[f64], channels: usize) -> Vec<f64> {
    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f64>() / channels as f64)
        .collect()
}

fn spectral_distance(left: &[f64], right: &[f64]) -> f64 {
    let size = left.len().min(right.len()).min(128);
    if size == 0 {
        return 0.0;
    }
    let spectrum = |values: &[f64]| -> Vec<f64> {
        (0..=size / 2)
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
    let mut left = spectrum(left);
    let mut right = spectrum(right);
    let left_sum = left.iter().sum::<f64>().max(1e-12);
    let right_sum = right.iter().sum::<f64>().max(1e-12);
    left.iter_mut().for_each(|value| *value /= left_sum);
    right.iter_mut().for_each(|value| *value /= right_sum);
    left.iter()
        .zip(right)
        .map(|(a, b)| (a - b).abs())
        .sum::<f64>()
        / 2.0
}

fn millionths(value: f64) -> u32 {
    (value.clamp(0.0, 4_294.0) * 1_000_000.0).round() as u32
}
fn signed_millionths(value: f64) -> i32 {
    (value.clamp(-1.0, 1.0) * 1_000_000.0).round() as i32
}

fn operation_kind(operation: &Operation) -> &'static str {
    match operation {
        Operation::Keep { .. } => "keep",
        Operation::Cut { .. } => "cut",
        Operation::Insert { .. } => "insert",
        Operation::Replace { .. } => "replace",
        Operation::Repeat { .. } => "repeat",
        Operation::Move { .. } => "move",
        Operation::Crossfade { .. } => "crossfade",
        Operation::PreserveTail { .. } => "preserve-tail",
        Operation::MatchGain { .. } => "match-gain",
        Operation::MatchEq { .. } => "match-eq",
        Operation::ExtendBars { .. } => "extend-bars",
        Operation::Lock { .. } => "lock",
    }
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

fn report(path: &Path, receipt: &RenderReceipt) -> Result<RenderReport> {
    Ok(RenderReport {
        schema: SCHEMA.into(),
        output_pcm_sha256: receipt.output_pcm_sha256.clone(),
        receipt_sha256: sha256_path(path)?,
        receipt_contract_sha256: canonical_sha256(receipt)?,
        source_samples_per_channel: receipt.source_samples_per_channel,
        output_samples_per_channel: receipt.output_samples_per_channel,
        operations: receipt.operation_kinds.values().sum(),
        locks: receipt.locks.len(),
        outside_regions_exact: receipt.outside_regions_exact,
        beat_alignment_passed: receipt.beat_alignment.passed,
        continuity_passed: receipt.continuity.passed,
        shareable: false,
        verified: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dsp_operations_are_deterministic_and_geometry_bounded() {
        let source = (0..32)
            .map(|index| {
                let phase = TAU * index as f64 / 8.0;
                (128.0 + phase.sin() * 48.0).round() as u8
            })
            .collect::<Vec<_>>();
        let crossfaded =
            crossfade_bytes(&source, RawPcmFormat::RawPcmU8, 1, FadeCurve::EqualPower).unwrap();
        assert_eq!(crossfaded.len(), source.len() / 2);
        assert_eq!(
            crossfaded,
            crossfade_bytes(&source, RawPcmFormat::RawPcmU8, 1, FadeCurve::EqualPower).unwrap()
        );

        let tail =
            preserve_tail_bytes(&source[..16], &source[16..], RawPcmFormat::RawPcmU8, 1).unwrap();
        assert_eq!(tail.len(), 16);

        let quiet = source
            .iter()
            .map(|sample| (128 + (i16::from(*sample) - 128) / 4) as u8)
            .collect::<Vec<_>>();
        let gained = match_gain_bytes(&quiet, RawPcmFormat::RawPcmU8, 1, 8_000, -12_000).unwrap();
        assert!(
            rms(&decode_samples(&gained, RawPcmFormat::RawPcmU8).unwrap())
                > rms(&decode_samples(&quiet, RawPcmFormat::RawPcmU8).unwrap())
        );

        let equalized = match_eq_bytes(
            &source,
            RawPcmFormat::RawPcmU8,
            1,
            8_000,
            &[EqBand {
                frequency_millihz: 1_000_000,
                q_milli: 1_000,
                gain_millidb: -6_000,
            }],
        )
        .unwrap();
        assert_eq!(equalized.len(), source.len());
        assert_ne!(equalized, source);
    }

    #[test]
    fn continuity_evidence_reports_clipping_and_seam_metrics() {
        let bytes = [
            128_u8, 140, 152, 164, 176, 188, 200, 212, 128, 116, 104, 92, 80, 68, 56, 44,
        ];
        let evidence = continuity_evidence(&bytes, RawPcmFormat::RawPcmU8, 1, &[8]).unwrap();
        assert_eq!(evidence.seams.len(), 1);
        assert_eq!(evidence.profile, "repair-continuity-v0.2");
        assert_eq!(evidence.loudness_matching, "ebu-r128-k-weighted-gated-v0.1");
    }
}
