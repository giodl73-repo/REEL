use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::production::{self, ContinuityEntity, ProductionManifest};

pub const CONTINUITY_SCHEMA: &str = "reel.continuity.v0.1";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContinuityRegistry {
    pub schema: String,
    pub registry_id: String,
    pub version: String,
    pub entities: Vec<ContinuityEntity>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExternalRegistryRef {
    pub path: String,
    pub version: String,
    pub sha256: String,
    pub entity_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContinuityValidationReport {
    pub schema: String,
    pub registry_id: String,
    pub version: String,
    pub entities: usize,
    pub assets: usize,
    pub forbidden_assets: usize,
    pub path: String,
    pub sha256: String,
}

pub fn load(path: impl AsRef<Path>) -> Result<ContinuityRegistry> {
    let path = path.as_ref();
    let registry: ContinuityRegistry = serde_yaml::from_slice(&fs::read(path)?)
        .with_context(|| format!("failed to parse continuity registry {}", path.display()))?;
    validate_registry(path, &registry)?;
    Ok(registry)
}

pub fn validate(path: impl AsRef<Path>) -> Result<ContinuityValidationReport> {
    let path = path.as_ref();
    let registry = load(path)?;
    let assets = registry
        .entities
        .iter()
        .map(|entity| entity.reference_assets.len())
        .sum();
    let forbidden_assets = registry
        .entities
        .iter()
        .flat_map(|entity| &entity.reference_assets)
        .filter(|asset| asset.provider_transfer == production::TransferPolicy::Forbidden)
        .count();
    Ok(ContinuityValidationReport {
        schema: registry.schema,
        registry_id: registry.registry_id,
        version: registry.version,
        entities: registry.entities.len(),
        assets,
        forbidden_assets,
        path: path.display().to_string(),
        sha256: production::sha256_path(path)?,
    })
}

pub fn resolve_for_manifest(
    manifest_path: &Path,
    manifest: &ProductionManifest,
) -> Result<Vec<ContinuityEntity>> {
    let Some(raw_ref) = manifest.continuity.extra.get("external_registry") else {
        return Ok(manifest.continuity.entities.clone());
    };
    let reference: ExternalRegistryRef = serde_yaml::from_value(raw_ref.clone())
        .context("continuity.external_registry is invalid")?;
    let registry_path = resolve_relative(manifest_path, &reference.path);
    let actual_hash = production::sha256_path(&registry_path)?;
    if actual_hash != reference.sha256 {
        bail!(
            "continuity registry hash mismatch for {}",
            registry_path.display()
        );
    }
    let registry = load(&registry_path)?;
    if registry.version != reference.version {
        bail!("continuity registry version mismatch");
    }
    let by_id = registry
        .entities
        .into_iter()
        .map(|entity| (entity.id.clone(), entity))
        .collect::<BTreeMap<_, _>>();
    let requested = reference.entity_ids.into_iter().collect::<HashSet<_>>();
    let mut resolved = Vec::new();
    for id in &requested {
        let shared = by_id
            .get(id)
            .ok_or_else(|| anyhow!("continuity registry has no entity {id}"))?;
        resolved.push(shared.clone());
    }
    for local in &manifest.continuity.entities {
        if let Some(shared) = resolved.iter_mut().find(|entity| entity.id == local.id) {
            apply_override(shared, local);
        } else {
            resolved.push(local.clone());
        }
    }
    resolved.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(resolved)
}

fn validate_registry(path: &Path, registry: &ContinuityRegistry) -> Result<()> {
    if registry.schema != CONTINUITY_SCHEMA {
        bail!(
            "continuity schema must be {CONTINUITY_SCHEMA}, got {}",
            registry.schema
        );
    }
    if registry.registry_id.trim().is_empty() || registry.version.trim().is_empty() {
        bail!("continuity registry id and version must not be empty");
    }
    let mut ids = HashSet::new();
    let mut assets = HashSet::new();
    for entity in &registry.entities {
        if entity.id.trim().is_empty() || !ids.insert(entity.id.as_str()) {
            bail!(
                "continuity registry contains empty or duplicate entity id {}",
                entity.id
            );
        }
        if entity.observations.is_empty() {
            bail!(
                "continuity entity {} requires approved textual observations",
                entity.id
            );
        }
        let observations_approved = entity
            .extra
            .get("observations_approved")
            .and_then(serde_yaml::Value::as_bool)
            .unwrap_or(false);
        let approval_reference = entity
            .extra
            .get("observation_approval_reference")
            .and_then(serde_yaml::Value::as_str)
            .unwrap_or_default();
        if !observations_approved || approval_reference.is_empty() {
            bail!(
                "continuity entity {} observations require explicit approval and a reference",
                entity.id
            );
        }
        for asset in &entity.reference_assets {
            if asset.id.trim().is_empty() || !assets.insert(asset.id.as_str()) {
                bail!(
                    "continuity registry contains empty or duplicate asset id {}",
                    asset.id
                );
            }
            if asset.sha256.trim().is_empty() {
                bail!("continuity asset {} requires a hash", asset.id);
            }
            if asset.local_path.trim().is_empty() {
                bail!("continuity asset {} requires a local path", asset.id);
            }
        }
    }
    if registry.entities.is_empty() {
        bail!("continuity registry {} has no entities", path.display());
    }
    Ok(())
}

fn apply_override(shared: &mut ContinuityEntity, local: &ContinuityEntity) {
    if !local.age_at_scene.is_empty() {
        shared.age_at_scene = local.age_at_scene.clone();
    }
    let local_observations_approved = local
        .extra
        .get("observations_approved")
        .and_then(serde_yaml::Value::as_bool)
        .unwrap_or(false)
        && local
            .extra
            .get("observation_approval_reference")
            .and_then(serde_yaml::Value::as_str)
            .is_some_and(|reference| !reference.is_empty());
    if local_observations_approved {
        shared.observations.extend(local.observations.clone());
    }
    shared.observations.sort();
    shared.observations.dedup();
    if !local.confidence.is_empty() {
        shared.confidence = local.confidence.clone();
    }
    if !local.provenance.is_empty() {
        shared.provenance = local.provenance.clone();
    }
    if !local.human_confirmation_status.is_empty() {
        shared.human_confirmation_status = local.human_confirmation_status.clone();
    }
    shared
        .reference_assets
        .extend(local.reference_assets.clone());
    shared.extra.extend(local.extra.clone());
}

fn resolve_relative(manifest_path: &Path, value: &str) -> PathBuf {
    let candidate = PathBuf::from(value);
    if candidate.is_absolute() {
        candidate
    } else {
        manifest_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(candidate)
    }
}
