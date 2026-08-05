use std::{
    collections::HashSet,
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::{
    production::{self, LoadedProductionManifest, NarrationCue, VariantLineage},
    series::{SrtEntry, parse_srt},
};

pub const CUE_IMPORT_SCHEMA: &str = "reel.cue-import.v0.1";

#[derive(Clone, Debug, Default, Deserialize)]
pub struct CueImportMapping {
    pub schema: String,
    pub cues: Vec<CueAssignment>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CueAssignment {
    pub index: usize,
    pub cue_id: String,
    pub speaker_id: String,
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub pause_policy: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CueImportReport {
    pub schema: String,
    pub input_manifest: String,
    pub input_manifest_sha256: String,
    pub input_srt: String,
    pub input_srt_sha256: String,
    pub output_manifest: String,
    pub output_manifest_sha256: String,
    pub cues: usize,
    pub speakers: Vec<String>,
    pub protected_pauses: usize,
    pub duration_ms: u64,
}

pub fn import_srt(
    manifest_path: impl AsRef<Path>,
    srt_path: impl AsRef<Path>,
    speaker: Option<&str>,
    source_refs: &[String],
    mapping_path: Option<&Path>,
    output: impl AsRef<Path>,
) -> Result<CueImportReport> {
    let manifest_path = manifest_path.as_ref();
    let srt_path = srt_path.as_ref();
    let output = output.as_ref();
    if output.exists() {
        bail!(
            "cue import writes a new derivative and refuses to overwrite {}",
            output.display()
        );
    }
    let loaded = production::load(manifest_path)?;
    let validation = production::validate(&loaded)?;
    let total_ms = validation
        .duration_ms
        .ok_or_else(|| anyhow!("timing not conformed: cue import requires a timed manifest"))?;
    let entries = parse_srt(&fs::read_to_string(srt_path)?)?;
    if entries.is_empty() {
        bail!("SRT contains no cues");
    }
    if entries[0].index != 1 {
        bail!("SRT cue indexes must begin at 1");
    }
    if entries.last().map(|entry| entry.end_ms).unwrap_or(0) > total_ms {
        bail!("SRT cue duration exceeds declared work duration");
    }
    let mapping = mapping_path
        .map(|path| -> Result<CueImportMapping> {
            let parsed: CueImportMapping = serde_yaml::from_slice(&fs::read(path)?)?;
            if parsed.schema != CUE_IMPORT_SCHEMA {
                bail!("cue mapping schema must be {CUE_IMPORT_SCHEMA}");
            }
            Ok(parsed)
        })
        .transpose()?;
    if mapping.is_none() && (speaker.unwrap_or_default().is_empty() || source_refs.is_empty()) {
        bail!("cue import requires --speaker and --source-ref, or a complete --mapping file");
    }
    let speaker_ids = loaded
        .manifest
        .speakers
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let source_ids = loaded
        .manifest
        .source_ranges
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    let mut imported = Vec::new();
    for entry in &entries {
        let assignment = mapping
            .as_ref()
            .and_then(|mapping| mapping.cues.iter().find(|item| item.index == entry.index));
        let cue_id = assignment
            .map(|item| item.cue_id.clone())
            .unwrap_or_else(|| format!("srt-{:04}", entry.index));
        let speaker_id = assignment
            .map(|item| item.speaker_id.clone())
            .unwrap_or_else(|| speaker.unwrap_or_default().to_string());
        let cue_sources = assignment
            .map(|item| item.source_refs.clone())
            .unwrap_or_else(|| source_refs.to_vec());
        if !speaker_ids.contains(speaker_id.as_str()) {
            bail!(
                "SRT cue {} references unknown speaker {speaker_id}",
                entry.index
            );
        }
        if cue_sources.is_empty()
            || cue_sources
                .iter()
                .any(|id| !source_ids.contains(id.as_str()))
        {
            bail!("SRT cue {} requires valid source assignment", entry.index);
        }
        imported.push(NarrationCue {
            id: cue_id,
            speaker_id,
            text: entry.text.clone(),
            text_reference: String::new(),
            source_refs: cue_sources,
            shot_ids: overlapping_shots(&loaded, entry)?,
            audio_reference: None,
            pause_policy: assignment
                .map(|item| item.pause_policy.clone())
                .unwrap_or_default(),
            invented: false,
            start_seconds: Some(entry.start_ms as f64 / 1000.0),
            duration_seconds: Some((entry.end_ms - entry.start_ms) as f64 / 1000.0),
        });
    }
    if let Some(mapping) = &mapping {
        if mapping.cues.len() != entries.len() {
            bail!("cue mapping must assign every SRT cue exactly once");
        }
        let indexes = mapping
            .cues
            .iter()
            .map(|item| item.index)
            .collect::<HashSet<_>>();
        if indexes.len() != entries.len() {
            bail!("cue mapping contains duplicate indexes");
        }
    }
    let imported_ids = imported
        .iter()
        .map(|cue| cue.id.as_str())
        .collect::<HashSet<_>>();
    for pause in &loaded.manifest.protected_pauses {
        if !imported_ids.contains(pause.after_cue_id.as_str()) {
            bail!(
                "protected pause {} requires mapping a cue to preserved id {}",
                pause.id,
                pause.after_cue_id
            );
        }
        let cue = imported
            .iter()
            .find(|cue| cue.id == pause.after_cue_id)
            .expect("protected cue checked");
        let cue_end = ((cue.start_seconds.unwrap_or(0.0) + cue.duration_seconds.unwrap_or(0.0))
            * 1000.0)
            .round() as u64;
        let next_start = imported
            .iter()
            .filter_map(|candidate| {
                let start = (candidate.start_seconds? * 1000.0).round() as u64;
                (start >= cue_end && candidate.id != cue.id).then_some(start)
            })
            .min()
            .ok_or_else(|| anyhow!("protected pause {} has no following cue", pause.id))?;
        if next_start - cue_end != pause.duration_ms {
            bail!(
                "protected pause {} must remain exactly {}ms",
                pause.id,
                pause.duration_ms
            );
        }
    }
    let mut manifest = loaded.manifest;
    manifest.narration_cues = imported;
    for shot in &mut manifest.shots {
        shot.narration_cue_ids = manifest
            .narration_cues
            .iter()
            .filter(|cue| cue.shot_ids.contains(&shot.id))
            .map(|cue| cue.id.clone())
            .collect();
    }
    manifest.lineage = Some(VariantLineage {
        parent_manifest: manifest_path.display().to_string(),
        root_work: manifest
            .lineage
            .as_ref()
            .map(|lineage| lineage.root_work.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| manifest.work.clone()),
        scene_key: manifest
            .scenes
            .iter()
            .map(|scene| scene.id.clone())
            .collect::<Vec<_>>()
            .join("+"),
        transformation_reason: "deterministic SRT cue import".to_string(),
        changed_dimensions: vec!["voice".to_string(), "captions".to_string()],
        review_candidate: true,
        principal_approved: false,
        created_unix: unix_now()?,
    });
    manifest.extra.insert(
        "cue_import".to_string(),
        serde_yaml::to_value(serde_json::json!({
            "schema": CUE_IMPORT_SCHEMA,
            "input_srt": srt_path.display().to_string(),
            "input_srt_sha256": production::sha256_path(srt_path)?,
            "mapping": mapping_path.map(|path| path.display().to_string()),
            "mapping_sha256": mapping_path.map(production::sha256_path).transpose()?,
            "tool_version": env!("CARGO_PKG_VERSION"),
        }))?,
    );
    production::validate(&LoadedProductionManifest {
        path: output.to_path_buf(),
        manifest: manifest.clone(),
        bytes: Vec::new(),
    })?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let staging = output.with_extension(format!("reel-import-{}.tmp", std::process::id()));
    if staging.exists() {
        bail!(
            "cue import staging path already exists: {}",
            staging.display()
        );
    }
    fs::write(&staging, serde_yaml::to_string(&manifest)?)?;
    fs::rename(&staging, output).context("failed to atomically publish cue-import derivative")?;
    let mut speakers = manifest
        .narration_cues
        .iter()
        .map(|cue| cue.speaker_id.clone())
        .collect::<Vec<_>>();
    speakers.sort();
    speakers.dedup();
    Ok(CueImportReport {
        schema: CUE_IMPORT_SCHEMA.to_string(),
        input_manifest: manifest_path.display().to_string(),
        input_manifest_sha256: production::sha256_path(manifest_path)?,
        input_srt: srt_path.display().to_string(),
        input_srt_sha256: production::sha256_path(srt_path)?,
        output_manifest: output.display().to_string(),
        output_manifest_sha256: production::sha256_path(output)?,
        cues: entries.len(),
        speakers,
        protected_pauses: manifest.protected_pauses.len(),
        duration_ms: total_ms,
    })
}

fn overlapping_shots(loaded: &LoadedProductionManifest, entry: &SrtEntry) -> Result<Vec<String>> {
    let shots = loaded
        .manifest
        .shots
        .iter()
        .filter(|shot| {
            let start = (shot.start_seconds.unwrap_or(0.0) * 1000.0).round() as u64;
            let end = start + (shot.duration_seconds.unwrap_or(0.0) * 1000.0).round() as u64;
            start < entry.end_ms && end > entry.start_ms
        })
        .map(|shot| shot.id.clone())
        .collect::<Vec<_>>();
    if shots.is_empty() {
        bail!("SRT cue {} does not overlap any declared shot", entry.index);
    }
    Ok(shots)
}

fn unix_now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
