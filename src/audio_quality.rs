use std::{fs, path::Path};

use anyhow::{Context, Result, anyhow, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::{adapters::ffmpeg::FfmpegAdapter, production};

pub const AUDIO_CHECK_SCHEMA: &str = "reel.audio-check.v0.1";

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum AudioProfile {
    Audiobook,
    Podcast,
    YoutubeAudiobook,
    #[default]
    PrivateReview,
}

impl AudioProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Audiobook => "audiobook",
            Self::Podcast => "podcast",
            Self::YoutubeAudiobook => "youtube-audiobook",
            Self::PrivateReview => "private-review",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "audiobook" => Ok(Self::Audiobook),
            "podcast" => Ok(Self::Podcast),
            "youtube-audiobook" => Ok(Self::YoutubeAudiobook),
            "private-review" => Ok(Self::PrivateReview),
            _ => bail!("unsupported audio profile {value}"),
        }
    }

    fn policy(self) -> AudioPolicy {
        match self {
            Self::Audiobook => AudioPolicy::new(-23.0, -18.0, -3.0, 15.0, 3_000, 2_000, 6.0),
            Self::Podcast => AudioPolicy::new(-19.0, -14.0, -1.0, 12.0, 2_000, 1_500, 6.0),
            Self::YoutubeAudiobook => AudioPolicy::new(-21.0, -16.0, -1.0, 15.0, 2_500, 2_000, 6.0),
            Self::PrivateReview => AudioPolicy::new(-24.0, -16.0, -1.0, 20.0, 4_000, 3_000, 3.0),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudioPolicy {
    pub minimum_integrated_lufs: f64,
    pub maximum_integrated_lufs: f64,
    pub maximum_true_peak_dbtp: f64,
    pub maximum_loudness_range_lu: f64,
    pub maximum_internal_silence_ms: u64,
    pub maximum_edge_silence_ms: u64,
    pub minimum_narration_margin_db: f64,
    pub near_clipping_threshold_dbfs: f64,
}

impl AudioPolicy {
    fn new(
        min_lufs: f64,
        max_lufs: f64,
        max_peak: f64,
        max_lra: f64,
        internal_ms: u64,
        edge_ms: u64,
        margin_db: f64,
    ) -> Self {
        Self {
            minimum_integrated_lufs: min_lufs,
            maximum_integrated_lufs: max_lufs,
            maximum_true_peak_dbtp: max_peak,
            maximum_loudness_range_lu: max_lra,
            maximum_internal_silence_ms: internal_ms,
            maximum_edge_silence_ms: edge_ms,
            minimum_narration_margin_db: margin_db,
            near_clipping_threshold_dbfs: -0.5,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudioFacts {
    pub sha256: String,
    pub duration_ms: u64,
    pub codec: String,
    pub sample_format: Option<String>,
    pub bit_depth: Option<u32>,
    pub sample_rate_hz: u32,
    pub channels: u32,
    pub integrated_lufs: f64,
    pub loudness_range_lu: f64,
    pub true_peak_dbtp: f64,
    pub sample_peak_dbfs: f64,
    pub peak_samples_at_maximum: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SilenceRange {
    pub kind: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudioViolation {
    pub code: String,
    pub measurement: String,
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StemMarginReport {
    pub narration: AudioFacts,
    pub effects_music: AudioFacts,
    pub narration_margin_db: f64,
    pub minimum_margin_db: f64,
    pub passed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudioCheckReport {
    pub schema: String,
    pub profile: String,
    pub policy: AudioPolicy,
    pub audio: AudioFacts,
    pub silence: Vec<SilenceRange>,
    pub stem_margin: Option<StemMarginReport>,
    pub expected_duration_ms: Option<u64>,
    pub violations: Vec<AudioViolation>,
    pub passed: bool,
}

#[derive(Clone, Debug)]
pub struct AudioCheckOptions<'a> {
    pub audio: &'a Path,
    pub narration_stem: Option<&'a Path>,
    pub effects_music_stem: Option<&'a Path>,
    pub manifest: Option<&'a Path>,
    pub profile: AudioProfile,
}

pub fn check(options: AudioCheckOptions<'_>) -> Result<AudioCheckReport> {
    if options.narration_stem.is_some() != options.effects_music_stem.is_some() {
        bail!("provide both narration and effects/music stems or neither");
    }
    let policy = options.profile.policy();
    let (audio, silence) = analyze(options.audio)?;
    let expected_duration_ms = match options.manifest {
        Some(path) => Some(
            production::require_timing_ready(path)?
                .manifest
                .shots
                .iter()
                .map(|shot| (shot.duration_seconds.unwrap_or_default() * 1000.0).round() as u64)
                .sum(),
        ),
        None => None,
    };
    let stem_margin = match (options.narration_stem, options.effects_music_stem) {
        (Some(narration), Some(effects)) => {
            let narration = analyze(narration)?.0;
            let effects_music = analyze(effects)?.0;
            let margin = narration.integrated_lufs - effects_music.integrated_lufs;
            Some(StemMarginReport {
                narration,
                effects_music,
                narration_margin_db: margin,
                minimum_margin_db: policy.minimum_narration_margin_db,
                passed: margin >= policy.minimum_narration_margin_db,
            })
        }
        _ => None,
    };
    let mut violations = violations(&audio, &silence, &policy);
    if let Some(expected) = expected_duration_ms {
        if audio.duration_ms.abs_diff(expected) > 50 {
            violations.push(AudioViolation {
                code: "duration-mismatch".to_string(),
                measurement: format!(
                    "measured={}ms expected={}ms tolerance=50ms",
                    audio.duration_ms, expected
                ),
                start_ms: None,
                end_ms: None,
            });
        }
    }
    if stem_margin.as_ref().is_some_and(|margin| !margin.passed) {
        let margin = stem_margin.as_ref().expect("stem margin exists");
        violations.push(AudioViolation {
            code: "narration-margin".to_string(),
            measurement: format!(
                "measured={:.2}dB minimum={:.2}dB",
                margin.narration_margin_db, margin.minimum_margin_db
            ),
            start_ms: None,
            end_ms: None,
        });
    }
    Ok(AudioCheckReport {
        schema: AUDIO_CHECK_SCHEMA.to_string(),
        profile: options.profile.as_str().to_string(),
        policy,
        audio,
        silence,
        stem_margin,
        expected_duration_ms,
        passed: violations.is_empty(),
        violations,
    })
}

pub fn write_report(path: &Path, report: &AudioCheckReport) -> Result<()> {
    if path.exists() {
        bail!(
            "refusing to overwrite audio-check report {}",
            path.display()
        );
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    fs::write(
        temporary.path(),
        format!("{}\n", serde_json::to_string_pretty(report)?),
    )?;
    temporary
        .persist_noclobber(path)
        .map_err(|error| error.error)?;
    Ok(())
}

fn analyze(path: &Path) -> Result<(AudioFacts, Vec<SilenceRange>)> {
    let path = path
        .canonicalize()
        .with_context(|| format!("failed to resolve audio {}", path.display()))?;
    let adapter = FfmpegAdapter;
    let probe_text = adapter.run_ffprobe(
        &[
            "-v".to_string(), "error".to_string(), "-select_streams".to_string(), "a:0".to_string(),
            "-show_entries".to_string(),
            "stream=codec_name,sample_fmt,bits_per_raw_sample,bits_per_sample,sample_rate,channels:format=duration".to_string(),
            "-of".to_string(), "json".to_string(),
        ],
        &[adapter.path_argument(&path)?],
    )?;
    let probe: Probe =
        serde_json::from_str(&probe_text).context("ffprobe returned invalid audio JSON")?;
    if probe.streams.len() != 1 {
        bail!("expected exactly one primary audio stream");
    }
    let stream = &probe.streams[0];
    let duration_ms = (probe.format.duration.parse::<f64>()? * 1000.0).round() as u64;
    let diagnostics = adapter.run_ffmpeg_diagnostics(
        &[
            "-hide_banner".to_string(),
            "-nostats".to_string(),
            "-i".to_string(),
        ],
        &[
            adapter.path_argument(&path)?,
            "-filter_complex".to_string(),
            "ebur128=peak=true,astats=metadata=0:reset=0,silencedetect=noise=-50dB:d=0.100"
                .to_string(),
            "-f".to_string(),
            "null".to_string(),
            "-".to_string(),
        ],
    )?;
    let integrated_lufs = last_metric(&diagnostics, "I:", "LUFS")?;
    let loudness_range_lu = last_metric(&diagnostics, "LRA:", "LU")?;
    let true_peak_dbtp = last_metric(&diagnostics, "Peak:", "dBFS")?;
    let sample_peak_dbfs = last_key_metric(&diagnostics, "Peak level dB:")?;
    let peak_samples_at_maximum = last_key_metric(&diagnostics, "Peak count:")?.round() as u64;
    let silence = parse_silence(&diagnostics, duration_ms);
    Ok((
        AudioFacts {
            sha256: production::sha256_path(&path)?,
            duration_ms,
            codec: stream.codec_name.clone(),
            sample_format: stream.sample_fmt.clone(),
            bit_depth: stream
                .bits_per_raw_sample
                .as_ref()
                .or(stream.bits_per_sample.as_ref())
                .and_then(json_u32)
                .filter(|value| *value > 0),
            sample_rate_hz: stream.sample_rate.parse()?,
            channels: stream.channels,
            integrated_lufs,
            loudness_range_lu,
            true_peak_dbtp,
            sample_peak_dbfs,
            peak_samples_at_maximum,
        },
        silence,
    ))
}

fn violations(
    audio: &AudioFacts,
    silence: &[SilenceRange],
    policy: &AudioPolicy,
) -> Vec<AudioViolation> {
    let mut result = Vec::new();
    let mut add = |code: &str, measurement: String, range: Option<(u64, u64)>| {
        result.push(AudioViolation {
            code: code.to_string(),
            measurement,
            start_ms: range.map(|value| value.0),
            end_ms: range.map(|value| value.1),
        })
    };
    if audio.integrated_lufs < policy.minimum_integrated_lufs
        || audio.integrated_lufs > policy.maximum_integrated_lufs
    {
        add(
            "integrated-loudness",
            format!(
                "measured={:.2}LUFS range={:.2}..={:.2}LUFS",
                audio.integrated_lufs,
                policy.minimum_integrated_lufs,
                policy.maximum_integrated_lufs
            ),
            None,
        );
    }
    if audio.loudness_range_lu > policy.maximum_loudness_range_lu {
        add(
            "loudness-range",
            format!(
                "measured={:.2}LU maximum={:.2}LU",
                audio.loudness_range_lu, policy.maximum_loudness_range_lu
            ),
            None,
        );
    }
    if audio.true_peak_dbtp > policy.maximum_true_peak_dbtp {
        add(
            "true-peak",
            format!(
                "measured={:.2}dBTP maximum={:.2}dBTP peak_samples_at_maximum={}",
                audio.true_peak_dbtp, policy.maximum_true_peak_dbtp, audio.peak_samples_at_maximum
            ),
            None,
        );
    }
    if audio.sample_peak_dbfs > policy.near_clipping_threshold_dbfs {
        add(
            "near-clipping",
            format!(
                "sample_peak={:.2}dBFS threshold={:.2}dBFS peak_samples_at_maximum={}",
                audio.sample_peak_dbfs,
                policy.near_clipping_threshold_dbfs,
                audio.peak_samples_at_maximum
            ),
            None,
        );
    }
    if audio.channels == 0 || audio.channels > 2 {
        add(
            "channels",
            format!("measured={} expected=1..=2", audio.channels),
            None,
        );
    }
    if audio.sample_rate_hz < 44_100 {
        add(
            "sample-rate",
            format!("measured={}Hz minimum=44100Hz", audio.sample_rate_hz),
            None,
        );
    }
    for item in silence {
        let maximum = if item.kind == "internal" {
            policy.maximum_internal_silence_ms
        } else {
            policy.maximum_edge_silence_ms
        };
        if item.duration_ms > maximum {
            add(
                &format!("{}-silence", item.kind),
                format!("measured={}ms maximum={}ms", item.duration_ms, maximum),
                Some((item.start_ms, item.end_ms)),
            );
        }
    }
    result
}

fn last_metric(text: &str, marker: &str, unit: &str) -> Result<f64> {
    text.lines()
        .rev()
        .find_map(|line| {
            let tail = line.split(marker).nth(1)?;
            let value = tail.trim().strip_suffix(unit)?.trim();
            value.parse().ok()
        })
        .ok_or_else(|| anyhow!("FFmpeg diagnostics omitted {marker} {unit}"))
}

fn last_key_metric(text: &str, marker: &str) -> Result<f64> {
    text.lines()
        .rev()
        .find_map(|line| line.split(marker).nth(1)?.trim().parse().ok())
        .ok_or_else(|| anyhow!("FFmpeg diagnostics omitted {marker}"))
}

fn parse_silence(text: &str, duration_ms: u64) -> Vec<SilenceRange> {
    let mut starts = Vec::new();
    let mut ranges = Vec::new();
    for line in text.lines() {
        if let Some(value) = line
            .split("silence_start:")
            .nth(1)
            .and_then(|tail| tail.split_whitespace().next())
            .and_then(|value| value.parse::<f64>().ok())
        {
            starts.push((value * 1000.0).round().max(0.0) as u64);
        }
        if let Some(tail) = line.split("silence_end:").nth(1) {
            let end = tail
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<f64>().ok())
                .map(|value| (value * 1000.0).round().max(0.0) as u64);
            if let (Some(start), Some(end)) = (starts.pop(), end) {
                let kind = if start <= 10 {
                    "leading"
                } else if end.abs_diff(duration_ms) <= 50 {
                    "trailing"
                } else {
                    "internal"
                };
                ranges.push(SilenceRange {
                    kind: kind.to_string(),
                    start_ms: start,
                    end_ms: end,
                    duration_ms: end.saturating_sub(start),
                });
            }
        }
    }
    ranges
}

#[derive(Deserialize)]
struct Probe {
    streams: Vec<ProbeStream>,
    format: ProbeFormat,
}
#[derive(Deserialize)]
struct ProbeStream {
    codec_name: String,
    sample_fmt: Option<String>,
    bits_per_raw_sample: Option<serde_json::Value>,
    bits_per_sample: Option<serde_json::Value>,
    sample_rate: String,
    channels: u32,
}
#[derive(Deserialize)]
struct ProbeFormat {
    duration: String,
}

fn json_u32(value: &serde_json::Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|number| number.try_into().ok())
        .or_else(|| value.as_str()?.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::parse_silence;

    #[test]
    fn classifies_silence_without_audio_content() {
        let diagnostics = "[silencedetect] silence_start: 0\n\
[silencedetect] silence_end: 0.5 | silence_duration: 0.5\n\
[silencedetect] silence_start: 2\n\
[silencedetect] silence_end: 6.5 | silence_duration: 4.5\n\
[silencedetect] silence_start: 9\n\
[silencedetect] silence_end: 10 | silence_duration: 1";
        let ranges = parse_silence(diagnostics, 10_000);
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0].kind, "leading");
        assert_eq!(ranges[1].kind, "internal");
        assert_eq!(ranges[2].kind, "trailing");
    }
}
