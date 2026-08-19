use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::{
    choreography::ChoreographyAssets,
    production,
    sprite_materializer::{MaterializationReceipt, RECEIPT_SCHEMA},
};

pub const BINDING_SCHEMA: &str = "reel.sprite-choreography-binding.v0.1";
pub const STAGING_REPORT_SCHEMA: &str = "reel.sprite-choreography-staging-report.v0.1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpriteChoreographyBinding {
    pub schema: String,
    pub choreography_sha256: String,
    pub base_assets_sha256: String,
    pub materialization_receipt_sha256: String,
    pub performers: BTreeMap<String, PerformerBinding>,
    #[serde(default)]
    pub preserve_unmapped_performers: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformerBinding {
    pub character: String,
    pub default_request: String,
    #[serde(default)]
    pub poses: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct StagingReport {
    pub schema: String,
    pub choreography_sha256: String,
    pub base_assets_sha256: String,
    pub materialization_receipt_sha256: String,
    pub output_sha256: String,
    pub cache_bound_performers: usize,
    pub preserved_performers: usize,
    pub cache_bindings: Vec<CacheBindingReport>,
    pub passed: bool,
}

#[derive(Debug, Serialize)]
pub struct CacheBindingReport {
    pub performer: String,
    pub pose: String,
    pub character: String,
    pub request: String,
    pub raster_cache_key: String,
    pub sha256: String,
}

pub fn stage_assets(
    binding_path: impl AsRef<Path>,
    receipt_path: impl AsRef<Path>,
    base_assets_path: impl AsRef<Path>,
    cache_root: impl AsRef<Path>,
    output: impl AsRef<Path>,
) -> Result<StagingReport> {
    let binding_path = binding_path.as_ref();
    let receipt_path = receipt_path.as_ref();
    let base_assets_path = base_assets_path.as_ref();
    let output = output.as_ref();
    if output.exists() {
        bail!("refusing to overwrite {}", output.display());
    }
    let binding_bytes = fs::read(binding_path)
        .with_context(|| format!("failed to read {}", binding_path.display()))?;
    let binding: SpriteChoreographyBinding = serde_yaml::from_slice(&binding_bytes)?;
    if binding.schema != BINDING_SCHEMA {
        bail!(
            "unsupported sprite choreography binding schema {}",
            binding.schema
        );
    }
    let receipt_bytes = fs::read(receipt_path)?;
    let base_assets_bytes = fs::read(base_assets_path)?;
    let receipt_sha = digest_bytes(&receipt_bytes);
    let base_assets_sha = digest_bytes(&base_assets_bytes);
    if receipt_sha != binding.materialization_receipt_sha256 {
        bail!("materialization receipt hash does not match choreography binding");
    }
    if base_assets_sha != binding.base_assets_sha256 {
        bail!("base choreography assets hash does not match binding");
    }
    let receipt: MaterializationReceipt = serde_json::from_slice(&receipt_bytes)?;
    if receipt.schema != RECEIPT_SCHEMA || !receipt.passed {
        bail!("choreography staging requires a passing materialization receipt");
    }
    let mut assets: ChoreographyAssets = serde_yaml::from_slice(&base_assets_bytes)?;
    if assets.choreography_sha256 != binding.choreography_sha256 {
        bail!("base asset choreography hash does not match binding");
    }
    let mapped = binding.performers.keys().cloned().collect::<BTreeSet<_>>();
    let preserved = binding
        .preserve_unmapped_performers
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if mapped.intersection(&preserved).next().is_some() {
        bail!("performer cannot be both cache-bound and preserved");
    }
    let all = assets.performers.keys().cloned().collect::<BTreeSet<_>>();
    if mapped.union(&preserved).cloned().collect::<BTreeSet<_>>() != all {
        bail!("binding must explicitly cache-bind or preserve every performer");
    }
    let cache_root = cache_root.as_ref();
    let mut receipt_keys = BTreeSet::new();
    for item in &receipt.outputs {
        if !receipt_keys.insert((item.character.as_str(), item.request.as_str())) {
            bail!(
                "materialization receipt duplicates {}/{}",
                item.character,
                item.request
            );
        }
    }
    let by_request = receipt
        .outputs
        .iter()
        .map(|item| ((item.character.as_str(), item.request.as_str()), item))
        .collect::<BTreeMap<_, _>>();
    let mut cache_bindings = Vec::new();
    for (performer, performer_binding) in &binding.performers {
        let target = assets
            .performers
            .get_mut(performer)
            .ok_or_else(|| anyhow::anyhow!("binding references unknown performer {performer}"))?;
        target.default_asset = cache_asset(
            cache_root,
            &by_request,
            performer,
            "default",
            &performer_binding.character,
            &performer_binding.default_request,
            &mut cache_bindings,
        )?;
        let required_poses = target.poses.keys().cloned().collect::<BTreeSet<_>>();
        let mapped_poses = performer_binding
            .poses
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        if required_poses != mapped_poses {
            bail!("performer {performer} pose mapping does not match base assets");
        }
        for (pose, request) in &performer_binding.poses {
            target.poses.insert(
                pose.clone(),
                cache_asset(
                    cache_root,
                    &by_request,
                    performer,
                    pose,
                    &performer_binding.character,
                    request,
                    &mut cache_bindings,
                )?,
            );
        }
    }
    let base_directory = base_assets_path.parent().unwrap_or_else(|| Path::new("."));
    assets.background = absolute_asset(base_directory, &assets.background)?;
    for performer in &preserved {
        let target = assets
            .performers
            .get_mut(performer)
            .expect("set equality checked");
        target.default_asset = absolute_asset(base_directory, &target.default_asset)?;
        for asset in target.poses.values_mut() {
            *asset = absolute_asset(base_directory, asset)?;
        }
    }
    for prop in assets.props.values_mut() {
        prop.asset = absolute_asset(base_directory, &prop.asset)?;
    }
    let bytes = format!("{}\n", serde_yaml::to_string(&assets)?).into_bytes();
    write_atomic(output, &bytes)?;
    Ok(StagingReport {
        schema: STAGING_REPORT_SCHEMA.to_string(),
        choreography_sha256: binding.choreography_sha256,
        base_assets_sha256: base_assets_sha,
        materialization_receipt_sha256: receipt_sha,
        output_sha256: production::sha256_path(output)?,
        cache_bound_performers: binding.performers.len(),
        preserved_performers: preserved.len(),
        cache_bindings,
        passed: true,
    })
}

fn cache_asset(
    cache_root: &Path,
    by_request: &BTreeMap<(&str, &str), &crate::sprite_materializer::MaterializedOutput>,
    performer: &str,
    pose: &str,
    character: &str,
    request: &str,
    reports: &mut Vec<CacheBindingReport>,
) -> Result<String> {
    let item = by_request.get(&(character, request)).ok_or_else(|| {
        anyhow::anyhow!(
            "performer {performer} pose {pose} has no materialized {character}/{request}"
        )
    })?;
    let output = cache_root
        .join(logical_path(&item.raster_cache_key)?)
        .with_extension("png");
    let output = output.canonicalize()?;
    let sha = digest_bytes(&fs::read(&output)?);
    if sha != item.sha256 {
        bail!("cache hash mismatch for performer {performer} pose {pose}");
    }
    reports.push(CacheBindingReport {
        performer: performer.to_string(),
        pose: pose.to_string(),
        character: character.to_string(),
        request: request.to_string(),
        raster_cache_key: item.raster_cache_key.clone(),
        sha256: sha,
    });
    Ok(normalize_path(&output))
}

fn absolute_asset(base: &Path, value: &str) -> Result<String> {
    let relative = Path::new(value);
    if relative.is_absolute() {
        if !relative.is_file() {
            bail!("base asset does not exist");
        }
        return Ok(normalize_path(&relative.canonicalize()?));
    }
    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        bail!("base asset path cannot traverse parents");
    }
    let resolved = base.join(relative);
    if !resolved.is_file() {
        bail!("base asset does not exist");
    }
    Ok(normalize_path(&resolved.canonicalize()?))
}

fn logical_path(value: &str) -> Result<PathBuf> {
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("logical cache key cannot escape cache root");
    }
    Ok(path.to_path_buf())
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn write_atomic(output: &Path, bytes: &[u8]) -> Result<()> {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary
        .persist_noclobber(output)
        .map_err(|error| error.error)?;
    Ok(())
}

fn digest_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
