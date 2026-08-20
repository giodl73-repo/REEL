use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::production;

pub const RESOLVED_BINDING_SCHEMA: &str = "reel.production-binding.v0.1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionBinding {
    pub manifest: String,
    pub manifest_sha256: String,
    pub work: String,
    #[serde(default)]
    pub shots: BTreeMap<String, String>,
    #[serde(default)]
    pub beats: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResolvedProductionBinding {
    pub schema: String,
    pub manifest_sha256: String,
    pub work: String,
    pub shots: BTreeMap<String, ResolvedShotBinding>,
    pub beats: BTreeMap<String, ResolvedBeatBinding>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResolvedShotBinding {
    pub shot_id: String,
    pub start_ms: u64,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResolvedBeatBinding {
    pub beat_id: String,
    pub time_ms: u64,
}

pub struct BoundProduction {
    pub resolved: ResolvedProductionBinding,
    pub loaded: production::LoadedProductionManifest,
}

pub fn resolve(sidecar_path: &Path, binding: &ProductionBinding) -> Result<BoundProduction> {
    require_text("production binding manifest", &binding.manifest)?;
    require_hash(&binding.manifest_sha256)?;
    require_text("production binding work", &binding.work)?;
    let manifest_path = resolve_relative(sidecar_path, &binding.manifest);
    let loaded = production::load(&manifest_path)?;
    let actual_hash = production::sha256_bytes(&loaded.bytes);
    if actual_hash != binding.manifest_sha256 {
        bail!(
            "production binding manifest hash mismatch: expected {}, found {}",
            binding.manifest_sha256,
            actual_hash
        );
    }
    production::validate(&loaded)?;
    if loaded.manifest.work != binding.work {
        bail!(
            "production binding work mismatch: expected {}, found {}",
            binding.work,
            loaded.manifest.work
        );
    }
    let shots_by_id = loaded
        .manifest
        .shots
        .iter()
        .map(|shot| (shot.id.as_str(), shot))
        .collect::<BTreeMap<_, _>>();
    let beats_by_id = loaded
        .manifest
        .beat_markers
        .iter()
        .map(|beat| (beat.id.as_str(), beat))
        .collect::<BTreeMap<_, _>>();
    let mut shots = BTreeMap::new();
    for (local_ref, shot_id) in &binding.shots {
        validate_id("production binding shot reference", local_ref)?;
        let shot = shots_by_id
            .get(shot_id.as_str())
            .ok_or_else(|| anyhow!("production binding references unknown shot {shot_id}"))?;
        let start_ms = required_ms(shot.start_seconds, &format!("bound shot {shot_id} start"))?;
        let duration_ms = required_ms(
            shot.duration_seconds,
            &format!("bound shot {shot_id} duration"),
        )?;
        shots.insert(
            local_ref.clone(),
            ResolvedShotBinding {
                shot_id: shot_id.clone(),
                start_ms,
                duration_ms,
            },
        );
    }
    let mut beats = BTreeMap::new();
    for (local_ref, beat_id) in &binding.beats {
        validate_id("production binding beat reference", local_ref)?;
        let beat = beats_by_id
            .get(beat_id.as_str())
            .ok_or_else(|| anyhow!("production binding references unknown beat {beat_id}"))?;
        beats.insert(
            local_ref.clone(),
            ResolvedBeatBinding {
                beat_id: beat_id.clone(),
                time_ms: seconds_to_ms(beat.time_seconds)?,
            },
        );
    }
    Ok(BoundProduction {
        resolved: ResolvedProductionBinding {
            schema: RESOLVED_BINDING_SCHEMA.to_string(),
            manifest_sha256: actual_hash,
            work: binding.work.clone(),
            shots,
            beats,
        },
        loaded,
    })
}

pub fn require_shot<'a>(
    resolved: &'a ResolvedProductionBinding,
    local_ref: &str,
) -> Result<&'a ResolvedShotBinding> {
    resolved
        .shots
        .get(local_ref)
        .ok_or_else(|| anyhow!("production binding has no shot mapping for {local_ref}"))
}

pub fn require_beat<'a>(
    resolved: &'a ResolvedProductionBinding,
    local_ref: &str,
) -> Result<&'a ResolvedBeatBinding> {
    resolved
        .beats
        .get(local_ref)
        .ok_or_else(|| anyhow!("production binding has no beat mapping for {local_ref}"))
}

fn resolve_relative(sidecar_path: &Path, value: &str) -> PathBuf {
    let candidate = PathBuf::from(value);
    if candidate.is_absolute() {
        candidate
    } else {
        sidecar_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(candidate)
    }
}

fn required_ms(value: Option<f64>, label: &str) -> Result<u64> {
    let value = value.ok_or_else(|| anyhow!("{label} is untimed"))?;
    if !value.is_finite() || value < 0.0 {
        bail!("{label} must be finite and non-negative");
    }
    Ok((value * 1_000.0).round() as u64)
}

fn seconds_to_ms(value: f64) -> Result<u64> {
    if !value.is_finite() || value < 0.0 {
        bail!("bound beat time must be finite and non-negative");
    }
    Ok((value * 1_000.0).round() as u64)
}

fn require_hash(value: &str) -> Result<()> {
    if value.len() != 64 || !value.chars().all(|character| character.is_ascii_hexdigit()) {
        bail!("production binding manifest_sha256 must be a 64-character hexadecimal hash");
    }
    Ok(())
}

fn require_text(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(())
}

fn validate_id(kind: &str, id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        bail!("{kind} id {id:?} must use ASCII letters, numbers, hyphens, or underscores");
    }
    Ok(())
}
