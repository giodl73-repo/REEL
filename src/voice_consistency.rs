use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::production;

pub const PROFILE_SCHEMA: &str = "reel.voice-consistency-profile.v0.1";
pub const MEASUREMENTS_SCHEMA: &str = "reel.voice-consistency-measurements.v0.1";
pub const REPORT_SCHEMA: &str = "reel.voice-consistency-report.v0.1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VoiceMode {
    NarratorSelf,
    Poet,
    CastCharacter,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckScope {
    Audition,
    Scene,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsistencyProfile {
    schema: String,
    profile_id: String,
    approval_reference: String,
    speakers: Vec<SpeakerProfile>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpeakerProfile {
    speaker_id: String,
    continuity_key: String,
    mode: VoiceMode,
    target_wpm: f64,
    minimum_wpm: f64,
    maximum_wpm: f64,
    #[serde(default)]
    minimum_pause_after_ms: u64,
    reference_audio_sha256: String,
    approval_reference: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Measurements {
    schema: String,
    manifest_sha256: String,
    scene_id: String,
    scope: CheckScope,
    cues: Vec<CueMeasurement>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CueMeasurement {
    cue_id: String,
    speaker_id: String,
    continuity_key: String,
    mode: VoiceMode,
    duration_ms: u64,
    #[serde(default)]
    head_silence_ms: u64,
    #[serde(default)]
    tail_silence_ms: u64,
    #[serde(default)]
    pause_after_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CueResult {
    pub cue_id: String,
    pub speaker_id: String,
    pub continuity_key: String,
    pub mode: VoiceMode,
    pub words: usize,
    pub speech_duration_ms: u64,
    pub measured_wpm: f64,
    pub target_wpm: f64,
    pub minimum_wpm: f64,
    pub maximum_wpm: f64,
    pub percent_from_target: f64,
    pub pause_after_ms: u64,
    pub minimum_pause_after_ms: u64,
    pub passed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpeakerResult {
    pub speaker_id: String,
    pub continuity_key: String,
    pub mode: VoiceMode,
    pub cue_count: usize,
    pub words: usize,
    pub speech_duration_ms: u64,
    pub measured_wpm: f64,
    pub target_wpm: f64,
    pub percent_from_target: f64,
    pub passed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsistencyViolation {
    pub code: String,
    pub cue_id: Option<String>,
    pub speaker_id: Option<String>,
    pub measurement: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsistencyReport {
    pub schema: String,
    pub profile_id: String,
    pub scene_id: String,
    pub scope: CheckScope,
    pub manifest_sha256: String,
    pub profile_sha256: String,
    pub measurements_sha256: String,
    pub cues: Vec<CueResult>,
    pub speakers: Vec<SpeakerResult>,
    pub violations: Vec<ConsistencyViolation>,
    pub passed: bool,
}

pub fn check(
    manifest_path: &Path,
    profile_path: &Path,
    measurements_path: &Path,
) -> Result<ConsistencyReport> {
    let loaded = production::load(manifest_path)?;
    production::validate(&loaded)?;
    let manifest_sha256 = production::sha256_path(manifest_path)?;
    let profile_sha256 = production::sha256_path(profile_path)?;
    let measurements_sha256 = production::sha256_path(measurements_path)?;
    let profile: ConsistencyProfile =
        serde_yaml::from_slice(&fs::read(profile_path).with_context(|| {
            format!(
                "failed to read voice consistency profile {}",
                profile_path.display()
            )
        })?)?;
    let measurements: Measurements =
        serde_yaml::from_slice(&fs::read(measurements_path).with_context(|| {
            format!(
                "failed to read voice consistency measurements {}",
                measurements_path.display()
            )
        })?)?;

    validate_profile(&profile, &loaded.manifest)?;
    if measurements.schema != MEASUREMENTS_SCHEMA {
        bail!(
            "unsupported voice consistency measurements schema: {}",
            measurements.schema
        );
    }
    if measurements.manifest_sha256 != manifest_sha256 {
        bail!("voice consistency measurements are stale: manifest_sha256 does not match");
    }
    if measurements.scene_id.trim().is_empty() {
        bail!("voice consistency measurements scene_id must not be empty");
    }
    if measurements.cues.is_empty() {
        bail!("voice consistency measurements must contain at least one cue");
    }

    let profiles: BTreeMap<_, _> = profile
        .speakers
        .iter()
        .map(|item| (item.speaker_id.as_str(), item))
        .collect();
    let manifest_cues: BTreeMap<_, _> = loaded
        .manifest
        .narration_cues
        .iter()
        .map(|cue| (cue.id.as_str(), cue))
        .collect();
    let expected_cue_ids: BTreeSet<_> = manifest_cues.keys().copied().collect();
    let mut measured_cue_ids = BTreeSet::new();
    let mut cue_results = Vec::new();
    let mut violations = Vec::new();

    for measured in &measurements.cues {
        if !measured_cue_ids.insert(measured.cue_id.as_str()) {
            bail!(
                "duplicate voice consistency cue measurement: {}",
                measured.cue_id
            );
        }
        let cue = manifest_cues
            .get(measured.cue_id.as_str())
            .with_context(|| {
                format!(
                    "voice consistency measurement references unknown cue: {}",
                    measured.cue_id
                )
            })?;
        if cue.speaker_id != measured.speaker_id {
            bail!(
                "cue {} speaker mismatch: manifest={} measurements={}",
                measured.cue_id,
                cue.speaker_id,
                measured.speaker_id
            );
        }
        let speaker = profiles
            .get(measured.speaker_id.as_str())
            .with_context(|| {
                format!(
                    "cue {} has no approved speaker profile: {}",
                    measured.cue_id, measured.speaker_id
                )
            })?;
        if measured.continuity_key != speaker.continuity_key {
            bail!(
                "cue {} continuity_key mismatch for speaker {}",
                measured.cue_id,
                measured.speaker_id
            );
        }
        if measured.mode != speaker.mode {
            bail!(
                "cue {} voice mode mismatch for speaker {}",
                measured.cue_id,
                measured.speaker_id
            );
        }
        if cue.text.trim().is_empty() {
            bail!(
                "cue {} has no inline text from which to measure speaking rate",
                cue.id
            );
        }
        let silence_ms = measured
            .head_silence_ms
            .checked_add(measured.tail_silence_ms)
            .context("voice consistency silence duration overflow")?;
        let speech_duration_ms = measured
            .duration_ms
            .checked_sub(silence_ms)
            .with_context(|| format!("cue {} silence exceeds its duration", measured.cue_id))?;
        if speech_duration_ms == 0 {
            bail!("cue {} has zero measured speech duration", measured.cue_id);
        }
        let words = cue.text.split_whitespace().count();
        let measured_wpm = round2(words as f64 * 60_000.0 / speech_duration_ms as f64);
        let percent_from_target =
            round2((measured_wpm - speaker.target_wpm) * 100.0 / speaker.target_wpm);
        let pace_passed =
            measured_wpm >= speaker.minimum_wpm && measured_wpm <= speaker.maximum_wpm;
        let pause_passed = measured.pause_after_ms >= speaker.minimum_pause_after_ms;
        if !pace_passed {
            let code = if measured_wpm > speaker.maximum_wpm {
                "pace-too-fast"
            } else {
                "pace-too-slow"
            };
            violations.push(ConsistencyViolation {
                code: code.to_string(), cue_id: Some(cue.id.clone()), speaker_id: Some(cue.speaker_id.clone()),
                measurement: format!("measured={measured_wpm:.2}wpm target={:.2}wpm allowed={:.2}-{:.2}wpm deviation={percent_from_target:+.2}%", speaker.target_wpm, speaker.minimum_wpm, speaker.maximum_wpm),
            });
        }
        if !pause_passed {
            violations.push(ConsistencyViolation {
                code: "pause-too-short".to_string(),
                cue_id: Some(cue.id.clone()),
                speaker_id: Some(cue.speaker_id.clone()),
                measurement: format!(
                    "measured={}ms minimum={}ms",
                    measured.pause_after_ms, speaker.minimum_pause_after_ms
                ),
            });
        }
        cue_results.push(CueResult {
            cue_id: cue.id.clone(),
            speaker_id: cue.speaker_id.clone(),
            continuity_key: speaker.continuity_key.clone(),
            mode: speaker.mode,
            words,
            speech_duration_ms,
            measured_wpm,
            target_wpm: speaker.target_wpm,
            minimum_wpm: speaker.minimum_wpm,
            maximum_wpm: speaker.maximum_wpm,
            percent_from_target,
            pause_after_ms: measured.pause_after_ms,
            minimum_pause_after_ms: speaker.minimum_pause_after_ms,
            passed: pace_passed && pause_passed,
        });
    }

    enforce_coverage(
        measurements.scope,
        &expected_cue_ids,
        &measured_cue_ids,
        &loaded.manifest.narration_cues,
        &cue_results,
    )?;
    let speaker_results = aggregate_speakers(&cue_results, &profiles);
    for speaker in &speaker_results {
        let profile = profiles[speaker.speaker_id.as_str()];
        if speaker.measured_wpm < profile.minimum_wpm || speaker.measured_wpm > profile.maximum_wpm
        {
            violations.push(ConsistencyViolation {
                code: "speaker-aggregate-pace".to_string(),
                cue_id: None,
                speaker_id: Some(speaker.speaker_id.clone()),
                measurement: format!(
                    "measured={:.2}wpm target={:.2}wpm allowed={:.2}-{:.2}wpm deviation={:+.2}%",
                    speaker.measured_wpm,
                    speaker.target_wpm,
                    profile.minimum_wpm,
                    profile.maximum_wpm,
                    speaker.percent_from_target
                ),
            });
        }
    }
    let passed = violations.is_empty();
    Ok(ConsistencyReport {
        schema: REPORT_SCHEMA.to_string(),
        profile_id: profile.profile_id,
        scene_id: measurements.scene_id,
        scope: measurements.scope,
        manifest_sha256,
        profile_sha256,
        measurements_sha256,
        cues: cue_results,
        speakers: speaker_results,
        violations,
        passed,
    })
}

fn validate_profile(
    profile: &ConsistencyProfile,
    manifest: &production::ProductionManifest,
) -> Result<()> {
    if profile.schema != PROFILE_SCHEMA {
        bail!(
            "unsupported voice consistency profile schema: {}",
            profile.schema
        );
    }
    if profile.profile_id.trim().is_empty() {
        bail!("voice consistency profile_id must not be empty");
    }
    if profile.approval_reference.trim().is_empty() {
        bail!("voice consistency profile approval_reference must not be empty");
    }
    if profile.speakers.is_empty() {
        bail!("voice consistency profile must contain at least one speaker");
    }
    let manifest_speakers: BTreeSet<_> = manifest
        .speakers
        .iter()
        .map(|speaker| speaker.id.as_str())
        .collect();
    let mut speaker_ids = BTreeSet::new();
    let mut continuity_keys = BTreeSet::new();
    for speaker in &profile.speakers {
        if !speaker_ids.insert(speaker.speaker_id.as_str()) {
            bail!(
                "duplicate voice consistency speaker_id: {}",
                speaker.speaker_id
            );
        }
        if !continuity_keys.insert(speaker.continuity_key.as_str()) {
            bail!("duplicate voice continuity_key: {}", speaker.continuity_key);
        }
        if !manifest_speakers.contains(speaker.speaker_id.as_str()) {
            bail!(
                "voice consistency profile references unknown speaker: {}",
                speaker.speaker_id
            );
        }
        if speaker.continuity_key.trim().is_empty() {
            bail!(
                "speaker {} continuity_key must not be empty",
                speaker.speaker_id
            );
        }
        if speaker.approval_reference.trim().is_empty() {
            bail!(
                "speaker {} approval_reference must not be empty",
                speaker.speaker_id
            );
        }
        if speaker.reference_audio_sha256.len() != 64
            || !speaker
                .reference_audio_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            bail!(
                "speaker {} reference_audio_sha256 must be a 64-character hex digest",
                speaker.speaker_id
            );
        }
        if !speaker.minimum_wpm.is_finite()
            || !speaker.target_wpm.is_finite()
            || !speaker.maximum_wpm.is_finite()
            || speaker.minimum_wpm <= 0.0
            || speaker.minimum_wpm > speaker.target_wpm
            || speaker.target_wpm > speaker.maximum_wpm
        {
            bail!(
                "speaker {} requires 0 < minimum_wpm <= target_wpm <= maximum_wpm",
                speaker.speaker_id
            );
        }
    }
    for cue in &manifest.narration_cues {
        if !speaker_ids.contains(cue.speaker_id.as_str()) {
            bail!(
                "manifest cue {} has no voice consistency profile for speaker {}",
                cue.id,
                cue.speaker_id
            );
        }
    }
    Ok(())
}

fn enforce_coverage(
    scope: CheckScope,
    expected: &BTreeSet<&str>,
    measured: &BTreeSet<&str>,
    manifest_cues: &[production::NarrationCue],
    results: &[CueResult],
) -> Result<()> {
    match scope {
        CheckScope::Scene => {
            let missing: Vec<_> = expected.difference(measured).copied().collect();
            if !missing.is_empty() {
                bail!(
                    "scene voice consistency measurements are incomplete; missing cues: {}",
                    missing.join(",")
                );
            }
        }
        CheckScope::Audition => {
            let expected_speakers: BTreeSet<_> = manifest_cues
                .iter()
                .map(|cue| cue.speaker_id.as_str())
                .collect();
            let measured_speakers: BTreeSet<_> =
                results.iter().map(|cue| cue.speaker_id.as_str()).collect();
            let missing: Vec<_> = expected_speakers
                .difference(&measured_speakers)
                .copied()
                .collect();
            if !missing.is_empty() {
                bail!(
                    "audition voice consistency measurements must sample every scene speaker; missing: {}",
                    missing.join(",")
                );
            }
        }
    }
    Ok(())
}

fn aggregate_speakers(
    cues: &[CueResult],
    profiles: &BTreeMap<&str, &SpeakerProfile>,
) -> Vec<SpeakerResult> {
    let mut totals: BTreeMap<&str, (usize, usize, u64)> = BTreeMap::new();
    for cue in cues {
        let total = totals.entry(cue.speaker_id.as_str()).or_default();
        total.0 += 1;
        total.1 += cue.words;
        total.2 += cue.speech_duration_ms;
    }
    totals
        .into_iter()
        .map(|(speaker_id, (cue_count, words, speech_duration_ms))| {
            let profile = profiles[speaker_id];
            let measured_wpm = round2(words as f64 * 60_000.0 / speech_duration_ms as f64);
            let percent_from_target =
                round2((measured_wpm - profile.target_wpm) * 100.0 / profile.target_wpm);
            SpeakerResult {
                speaker_id: speaker_id.to_string(),
                continuity_key: profile.continuity_key.clone(),
                mode: profile.mode,
                cue_count,
                words,
                speech_duration_ms,
                measured_wpm,
                target_wpm: profile.target_wpm,
                percent_from_target,
                passed: measured_wpm >= profile.minimum_wpm && measured_wpm <= profile.maximum_wpm,
            }
        })
        .collect()
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub fn write_report(path: &Path, report: &ConsistencyReport) -> Result<()> {
    if path.exists() {
        bail!(
            "refusing to overwrite voice consistency report {}",
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
