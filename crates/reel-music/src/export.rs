use std::{collections::BTreeMap, fs, io::Write, path::Path};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::{
    hash::{canonical_sha256, sha256_path},
    model,
};

pub const SCHEMA: &str = "reel.music-score-export-plan.v0.1";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreExportPlan {
    pub schema: String,
    pub export_id: String,
    pub model_id: String,
    pub model_manifest_sha256: String,
    pub model_contract_sha256: String,
    pub pulses_per_quarter: u32,
    pub duration_ticks: u64,
    pub quantization: Quantization,
    pub artifacts: Vec<ArtifactRequest>,
    pub rehearsal_guide: RehearsalGuide,
    pub lyric_layers: Vec<LyricLayerBinding>,
    pub shareable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Quantization {
    pub tick_policy: String,
    pub expressive_timing_applied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRequest {
    pub kind: String,
    pub filename: String,
    pub adapter: String,
    pub adapter_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RehearsalGuide {
    pub filename: String,
    pub sample_rate_hz: u32,
    pub channels: u8,
    pub sample_format: String,
    pub waveform: String,
    pub part_selection: String,
    pub purpose: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LyricLayerBinding {
    pub id: String,
    pub kind: String,
    pub language: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanReport {
    pub schema: String,
    pub plan: String,
    pub plan_sha256: String,
    pub plan_contract_sha256: String,
    pub model_contract_sha256: String,
    pub artifacts: usize,
    pub lyric_layers: usize,
    pub shareable: bool,
    pub verified: bool,
}

pub fn build(model_path: &Path) -> Result<ScoreExportPlan> {
    let report = model::validate(model_path)?;
    let music = model::load(model_path)?;
    if music.musical_timebase.pulses_per_quarter > u16::MAX as u32 {
        bail!("MIDI export requires pulses_per_quarter <= 65535");
    }
    if music
        .parts
        .iter()
        .any(|part| part.notes.iter().any(|note| note.voice > 16))
    {
        bail!("MIDI v0.1 export supports note voices 1 through 16");
    }
    for part in &music.parts {
        let mut voice_ends = BTreeMap::new();
        for note in &part.notes {
            let end = note.start_tick + note.duration_ticks;
            if voice_ends
                .get(&note.voice)
                .is_some_and(|prior_end| note.start_tick < *prior_end)
            {
                bail!(
                    "MusicXML v0.1 export requires non-overlapping notes within part {} voice {}",
                    part.id,
                    note.voice
                );
            }
            voice_ends.insert(note.voice, end);
        }
    }
    let lyric_layers = music
        .lyric_layers
        .iter()
        .map(|layer| LyricLayerBinding {
            id: layer.id.clone(),
            kind: serde_json::to_value(layer.kind)
                .expect("enum serialization cannot fail")
                .as_str()
                .expect("lyric kind serializes as a string")
                .to_string(),
            language: layer.language.clone(),
            sha256: layer.sha256.clone(),
        })
        .collect();
    let mut artifacts = vec![
        ArtifactRequest {
            kind: "midi-smf".into(),
            filename: "score.mid".into(),
            adapter: "reel-midi-smf".into(),
            adapter_version: "0.1.0".into(),
        },
        ArtifactRequest {
            kind: "musicxml-score-partwise".into(),
            filename: "score.musicxml".into(),
            adapter: "reel-musicxml".into(),
            adapter_version: "0.1.0".into(),
        },
        ArtifactRequest {
            kind: "rehearsal-guide-wav".into(),
            filename: "rehearsal-guide.wav".into(),
            adapter: "reel-square-guide".into(),
            adapter_version: "0.1.0".into(),
        },
    ];
    if music.lead_sheet.is_some() {
        artifacts.push(ArtifactRequest {
            kind: "printable-lead-sheet-svg".into(),
            filename: "lead-sheet.svg".into(),
            adapter: "reel-lead-sheet-svg".into(),
            adapter_version: "0.1.0".into(),
        });
    }
    if music.piano_vocal_score.is_some() {
        artifacts.push(ArtifactRequest {
            kind: "piano-vocal-musicxml".into(),
            filename: "piano-vocal.musicxml".into(),
            adapter: "reel-piano-vocal-musicxml".into(),
            adapter_version: "0.1.0".into(),
        });
    }
    Ok(ScoreExportPlan {
        schema: SCHEMA.into(),
        export_id: format!("{}-score-export", music.model_id),
        model_id: music.model_id,
        model_manifest_sha256: report.manifest_sha256,
        model_contract_sha256: report.contract_sha256,
        pulses_per_quarter: music.musical_timebase.pulses_per_quarter,
        duration_ticks: music.duration_ticks,
        quantization: Quantization {
            tick_policy: "exact-model-ticks".into(),
            expressive_timing_applied: false,
        },
        artifacts,
        rehearsal_guide: RehearsalGuide {
            filename: "rehearsal-guide.wav".into(),
            sample_rate_hz: 48_000,
            channels: 1,
            sample_format: "signed-16-bit-little-endian-pcm".into(),
            waveform: "band-unlimited-square".into(),
            part_selection: "first-melody-or-vocal-else-first-part".into(),
            purpose: "timing-and-pitch-rehearsal-only-not-a-performance-master".into(),
        },
        lyric_layers,
        shareable: false,
    })
}

pub fn write(model_path: &Path, output: &Path) -> Result<PlanReport> {
    if output.exists() {
        bail!(
            "score export plan output already exists: {}",
            output.display()
        );
    }
    let plan = build(model_path)?;
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
    report(output, &plan)
}

pub fn load(path: &Path) -> Result<ScoreExportPlan> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("score export plan is not valid JSON: {}", path.display()))
}

pub fn validate(plan_path: &Path, model_path: &Path) -> Result<PlanReport> {
    let actual = load(plan_path)?;
    let expected = build(model_path)?;
    if actual != expected {
        bail!("score export plan does not match the current validated music model");
    }
    report(plan_path, &actual)
}

fn report(path: &Path, plan: &ScoreExportPlan) -> Result<PlanReport> {
    Ok(PlanReport {
        schema: SCHEMA.into(),
        plan: path.display().to_string(),
        plan_sha256: sha256_path(path)?,
        plan_contract_sha256: canonical_sha256(plan)?,
        model_contract_sha256: plan.model_contract_sha256.clone(),
        artifacts: plan.artifacts.len(),
        lyric_layers: plan.lyric_layers.len(),
        shareable: false,
        verified: true,
    })
}
