use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::production;

pub const CATALOG_SCHEMA: &str = "reel.sonic-asset-catalog.v0.1";
pub const REQUEST_SCHEMA: &str = "reel.sonic-asset-request.v0.1";
pub const RESOLUTION_SCHEMA: &str = "reel.sonic-asset-resolution.v0.1";
pub const RECEIPT_SCHEMA: &str = "reel.sonic-asset-resolution-receipt.v0.1";
pub const MATERIALIZATION_RECEIPT_SCHEMA: &str =
    "reel.sonic-asset-manifest-materialization-receipt.v0.1";

#[derive(Debug)]
pub struct Loaded<T> {
    pub path: PathBuf,
    pub sha256: String,
    pub value: T,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: String,
    pub library_id: String,
    pub library_version: String,
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub pools: Vec<Pool>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthorityState {
    Candidate,
    SelectedPrivateProduction,
    ApprovedPool,
    PrincipalApproved,
    ReleaseCleared,
    Superseded,
    DiagnosticPlaceholder,
    FixtureOnly,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub asset_id: String,
    pub authority_state: AuthorityState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    #[serde(default)]
    pub authority_receipt_sha256: Option<String>,
    pub license: LicenseFacts,
    #[serde(default)]
    pub lineage_sha256: Vec<String>,
    pub variants: Vec<Variant>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LicenseFacts {
    pub license_id: String,
    pub review_status: String,
    pub permits_production_use: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub variant_id: String,
    pub locator: String,
    pub sha256: String,
    pub bytes: u64,
    pub geometry: AudioGeometry,
    #[serde(default)]
    pub loop_region: Option<SampleRegion>,
    #[serde(default)]
    pub sync_markers: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AudioGeometry {
    pub sample_rate_hz: u32,
    pub bit_depth: u16,
    pub channels: u16,
    pub sample_count: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SampleRegion {
    pub start_sample: u64,
    pub end_sample_exclusive: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pool {
    pub pool_id: String,
    pub pool_version: String,
    pub members: Vec<PoolMember>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoolMember {
    pub asset_id: String,
    pub variant_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub request_id: String,
    pub consumer_manifest_sha256: String,
    #[serde(default)]
    pub engineering_fixture: bool,
    pub bindings: Vec<RequestBinding>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequestBinding {
    pub event_id: String,
    pub selection: SelectionMode,
    #[serde(default)]
    pub asset_id: Option<String>,
    #[serde(default)]
    pub variant_id: Option<String>,
    #[serde(default)]
    pub pool_id: Option<String>,
    #[serde(default)]
    pub pool_version: Option<String>,
    #[serde(default)]
    pub selection_key: Option<String>,
    #[serde(default)]
    pub required_geometry: Option<RequiredGeometry>,
    #[serde(default)]
    pub require_loop_region: bool,
    #[serde(default)]
    pub required_sync_markers: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectionMode {
    Exact,
    ApprovedPool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredGeometry {
    pub sample_rate_hz: u32,
    pub bit_depth: u16,
    #[serde(default)]
    pub channels: Option<u16>,
    #[serde(default)]
    pub sample_count: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub schema: String,
    pub library_id: String,
    pub library_version: String,
    pub catalog_sha256: String,
    pub request_sha256: String,
    pub consumer_manifest_sha256: String,
    pub tool_version: String,
    pub shareable: bool,
    pub selections: Vec<ResolvedSelection>,
    pub passed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedSelection {
    pub event_id: String,
    pub selection_mode: String,
    pub asset_id: String,
    pub variant_id: String,
    pub resolved_path: String,
    pub sha256: String,
    pub bytes: u64,
    pub geometry: AudioGeometry,
    #[serde(default)]
    pub loop_region: Option<SampleRegion>,
    #[serde(default)]
    pub sync_markers: BTreeMap<String, u64>,
    pub authority_state: AuthorityState,
    pub authority_receipt_sha256: Option<String>,
    pub license: LicenseFacts,
    pub lineage_sha256: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionReceipt {
    pub schema: String,
    pub library_id: String,
    pub library_version: String,
    pub catalog_sha256: String,
    pub request_sha256: String,
    pub consumer_manifest_sha256: String,
    pub resolution_sha256: String,
    pub tool_version: String,
    pub assets: Vec<ReceiptAsset>,
    pub path_free: bool,
    pub selects_creative_output: bool,
    pub grants_approval: bool,
    pub passed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptAsset {
    pub event_id: String,
    pub asset_id: String,
    pub variant_id: String,
    pub sha256: String,
    pub bytes: u64,
    pub geometry: AudioGeometry,
    pub authority_state: AuthorityState,
    pub authority_receipt_sha256: Option<String>,
    pub license_id: String,
    pub lineage_sha256: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaterializationReceipt {
    pub schema: String,
    pub source_manifest_sha256: String,
    pub resolution_sha256: String,
    pub resolution_receipt_sha256: String,
    pub output_manifest_sha256: String,
    pub bound_events: usize,
    pub tool_version: String,
    pub path_free: bool,
    pub selects_creative_output: bool,
    pub grants_approval: bool,
    pub passed: bool,
}

#[derive(Debug, Serialize)]
pub struct CheckReport {
    pub schema: String,
    pub catalog_sha256: String,
    pub request_sha256: String,
    pub resolution_sha256: String,
    pub receipt_sha256: String,
    pub assets_verified: usize,
    pub passed: bool,
}

pub fn load_catalog(path: impl AsRef<Path>) -> Result<Loaded<Catalog>> {
    load_yaml_or_json(path.as_ref(), "sonic asset catalog")
}

pub fn load_request(path: impl AsRef<Path>) -> Result<Loaded<Request>> {
    load_yaml_or_json(path.as_ref(), "sonic asset request")
}

pub fn resolve(
    catalog: &Loaded<Catalog>,
    request: &Loaded<Request>,
) -> Result<(Resolution, ResolutionReceipt)> {
    validate_catalog(catalog)?;
    validate_request(request)?;
    let assets = index_assets(&catalog.value)?;
    let pools = index_pools(&catalog.value)?;
    let mut selections = Vec::new();
    for binding in &request.value.bindings {
        let (asset, variant, mode) = match binding.selection {
            SelectionMode::Exact => {
                let asset_id = binding.asset_id.as_deref().expect("validated exact asset");
                let asset = assets
                    .get(asset_id)
                    .ok_or_else(|| anyhow!("unknown sonic asset {asset_id}"))?;
                let variant = select_variant(asset, binding.variant_id.as_deref())?;
                (asset, variant, "exact")
            }
            SelectionMode::ApprovedPool => {
                let pool_id = binding.pool_id.as_deref().expect("validated pool id");
                let pool_version = binding
                    .pool_version
                    .as_deref()
                    .expect("validated pool version");
                let selection_key = binding
                    .selection_key
                    .as_deref()
                    .expect("validated selection key");
                let pool = pools
                    .get(pool_id)
                    .ok_or_else(|| anyhow!("unknown sonic asset pool {pool_id}"))?;
                if pool.pool_version != pool_version {
                    bail!("sonic asset pool {pool_id} version mismatch");
                }
                let member = deterministic_member(
                    pool,
                    &catalog.sha256,
                    &request.value.request_id,
                    selection_key,
                )?;
                let asset = assets
                    .get(member.asset_id.as_str())
                    .ok_or_else(|| anyhow!("pool {pool_id} references an unknown asset"))?;
                let variant = select_variant(asset, Some(&member.variant_id))?;
                (asset, variant, "approved-pool")
            }
        };
        validate_authority(asset, &request.value, mode)?;
        validate_binding(binding, variant)?;
        let resolved_path = resolve_locator(&catalog.path, &variant.locator)?;
        let bytes = fs::metadata(&resolved_path)?.len();
        if bytes != variant.bytes || production::sha256_path(&resolved_path)? != variant.sha256 {
            bail!(
                "sonic asset {} variant {} does not match its bytes or sha256",
                asset.asset_id,
                variant.variant_id
            );
        }
        let measured = inspect_pcm_wav(&resolved_path)?;
        if measured != variant.geometry {
            bail!(
                "sonic asset {} variant {} WAV geometry does not match the catalog",
                asset.asset_id,
                variant.variant_id
            );
        }
        selections.push(ResolvedSelection {
            event_id: binding.event_id.clone(),
            selection_mode: mode.to_string(),
            asset_id: asset.asset_id.clone(),
            variant_id: variant.variant_id.clone(),
            resolved_path: resolved_path.display().to_string(),
            sha256: variant.sha256.to_lowercase(),
            bytes,
            geometry: measured,
            loop_region: variant.loop_region.clone(),
            sync_markers: variant.sync_markers.clone(),
            authority_state: asset.authority_state,
            authority_receipt_sha256: asset.authority_receipt_sha256.clone(),
            license: asset.license.clone(),
            lineage_sha256: asset.lineage_sha256.clone(),
        });
    }
    let resolution = Resolution {
        schema: RESOLUTION_SCHEMA.to_string(),
        library_id: catalog.value.library_id.clone(),
        library_version: catalog.value.library_version.clone(),
        catalog_sha256: catalog.sha256.clone(),
        request_sha256: request.sha256.clone(),
        consumer_manifest_sha256: request.value.consumer_manifest_sha256.to_lowercase(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        shareable: false,
        selections,
        passed: true,
    };
    let resolution_bytes = canonical_json(&resolution)?;
    let receipt = receipt_for(&resolution, &digest(&resolution_bytes));
    Ok((resolution, receipt))
}

pub fn write_resolution_packet(
    resolution: &Resolution,
    receipt: &ResolutionReceipt,
    resolution_path: impl AsRef<Path>,
    receipt_path: impl AsRef<Path>,
) -> Result<()> {
    let resolution_path = resolution_path.as_ref();
    let receipt_path = receipt_path.as_ref();
    if resolution_path == receipt_path {
        bail!("resolution and receipt paths must differ");
    }
    if resolution_path.exists() || receipt_path.exists() {
        bail!("refusing to overwrite an existing sonic resolution or receipt");
    }
    write_new(resolution_path, &canonical_json(resolution)?)?;
    if let Err(error) = write_new(receipt_path, &canonical_json(receipt)?) {
        let _ = fs::remove_file(resolution_path);
        return Err(error);
    }
    Ok(())
}

pub fn check(
    catalog_path: impl AsRef<Path>,
    request_path: impl AsRef<Path>,
    resolution_path: impl AsRef<Path>,
    receipt_path: impl AsRef<Path>,
) -> Result<CheckReport> {
    let catalog = load_catalog(catalog_path)?;
    let request = load_request(request_path)?;
    let resolution_bytes = fs::read(resolution_path.as_ref())?;
    let receipt_bytes = fs::read(receipt_path.as_ref())?;
    let resolution: Resolution = serde_json::from_slice(&resolution_bytes)?;
    let receipt: ResolutionReceipt = serde_json::from_slice(&receipt_bytes)?;
    let (expected_resolution, expected_receipt) = resolve(&catalog, &request)?;
    if receipt.resolution_sha256 != digest(&resolution_bytes)
        || canonical_json(&resolution)? != canonical_json(&expected_resolution)?
        || canonical_json(&receipt)? != canonical_json(&expected_receipt)?
    {
        bail!("sonic resolution packet is stale or has been tampered with");
    }
    Ok(CheckReport {
        schema: "reel.sonic-asset-check.v0.1".to_string(),
        catalog_sha256: catalog.sha256,
        request_sha256: request.sha256,
        resolution_sha256: digest(&resolution_bytes),
        receipt_sha256: digest(&receipt_bytes),
        assets_verified: resolution.selections.len(),
        passed: true,
    })
}

pub fn materialize_manifest(
    catalog_path: impl AsRef<Path>,
    request_path: impl AsRef<Path>,
    manifest_path: impl AsRef<Path>,
    resolution_path: impl AsRef<Path>,
    resolution_receipt_path: impl AsRef<Path>,
    output_manifest: impl AsRef<Path>,
    output_receipt: impl AsRef<Path>,
) -> Result<MaterializationReceipt> {
    check(
        catalog_path,
        request_path,
        &resolution_path,
        &resolution_receipt_path,
    )?;
    let manifest_path = manifest_path.as_ref();
    let resolution_path = resolution_path.as_ref();
    let resolution_receipt_path = resolution_receipt_path.as_ref();
    let output_manifest = output_manifest.as_ref();
    let output_receipt = output_receipt.as_ref();
    if output_manifest.exists() || output_receipt.exists() {
        bail!("refusing to overwrite a materialized manifest or receipt");
    }
    let manifest_bytes = fs::read(manifest_path)?;
    let manifest_sha256 = digest(&manifest_bytes);
    let resolution_bytes = fs::read(resolution_path)?;
    let receipt_bytes = fs::read(resolution_receipt_path)?;
    let resolution: Resolution = serde_json::from_slice(&resolution_bytes)?;
    let receipt: ResolutionReceipt = serde_json::from_slice(&receipt_bytes)?;
    if resolution.consumer_manifest_sha256 != manifest_sha256
        || receipt.consumer_manifest_sha256 != manifest_sha256
        || receipt.resolution_sha256 != digest(&canonical_json(&resolution)?)
        || receipt != receipt_for(&resolution, &receipt.resolution_sha256)
    {
        bail!("sonic resolution does not bind the exact source manifest and receipt");
    }
    let mut document: serde_yaml::Value = serde_yaml::from_slice(&manifest_bytes)?;
    let events = document
        .get_mut("audio_events")
        .and_then(serde_yaml::Value::as_sequence_mut)
        .ok_or_else(|| anyhow!("manifest lacks an audio_events sequence"))?;
    let mut by_event = resolution
        .selections
        .iter()
        .map(|item| (item.event_id.as_str(), item))
        .collect::<BTreeMap<_, _>>();
    for event in events.iter_mut() {
        let id = event
            .get("id")
            .and_then(serde_yaml::Value::as_str)
            .ok_or_else(|| anyhow!("audio event lacks an id"))?
            .to_string();
        if let Some(selected) = by_event.remove(id.as_str()) {
            let mapping = event
                .as_mapping_mut()
                .ok_or_else(|| anyhow!("audio event {id} is not a mapping"))?;
            mapping.insert(
                serde_yaml::Value::String("source".to_string()),
                serde_yaml::Value::String(selected.resolved_path.clone()),
            );
        }
    }
    if !by_event.is_empty() {
        bail!("sonic resolution references audio events absent from the manifest");
    }
    let output_bytes = serde_yaml::to_string(&document)?.into_bytes();
    write_new(output_manifest, &output_bytes)?;
    let materialization = MaterializationReceipt {
        schema: MATERIALIZATION_RECEIPT_SCHEMA.to_string(),
        source_manifest_sha256: manifest_sha256,
        resolution_sha256: digest(&resolution_bytes),
        resolution_receipt_sha256: digest(&receipt_bytes),
        output_manifest_sha256: digest(&output_bytes),
        bound_events: resolution.selections.len(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        path_free: true,
        selects_creative_output: false,
        grants_approval: false,
        passed: true,
    };
    if let Err(error) = write_new(output_receipt, &canonical_json(&materialization)?) {
        let _ = fs::remove_file(output_manifest);
        return Err(error);
    }
    Ok(materialization)
}

fn validate_catalog(catalog: &Loaded<Catalog>) -> Result<()> {
    if catalog.value.schema != CATALOG_SCHEMA {
        bail!(
            "unsupported sonic asset catalog schema {}",
            catalog.value.schema
        );
    }
    nonempty("library_id", &catalog.value.library_id)?;
    nonempty("library_version", &catalog.value.library_version)?;
    let assets = index_assets(&catalog.value)?;
    for asset in &catalog.value.assets {
        if asset.variants.is_empty() {
            bail!("sonic asset {} has no variants", asset.asset_id);
        }
        validate_sha_list(asset)?;
        let mut variants = BTreeSet::new();
        for variant in &asset.variants {
            nonempty("variant_id", &variant.variant_id)?;
            nonempty("locator", &variant.locator)?;
            validate_sha("variant sha256", &variant.sha256)?;
            if variant.bytes == 0 || variant.geometry.sample_count == 0 {
                bail!("sonic asset variants require nonzero bytes and samples");
            }
            if !variants.insert(variant.variant_id.as_str()) {
                bail!(
                    "duplicate variant {} in {}",
                    variant.variant_id,
                    asset.asset_id
                );
            }
            if let Some(region) = &variant.loop_region {
                if region.start_sample >= region.end_sample_exclusive
                    || region.end_sample_exclusive > variant.geometry.sample_count
                {
                    bail!("invalid loop region for {}", asset.asset_id);
                }
            }
            if variant
                .sync_markers
                .values()
                .any(|sample| *sample >= variant.geometry.sample_count)
            {
                bail!("sync marker outside {}", asset.asset_id);
            }
        }
        if asset.authority_state == AuthorityState::Superseded {
            let replacement = asset.superseded_by.as_deref().ok_or_else(|| {
                anyhow!("superseded asset {} lacks superseded_by", asset.asset_id)
            })?;
            if !assets.contains_key(replacement) {
                bail!(
                    "superseded asset {} references unknown replacement",
                    asset.asset_id
                );
            }
        } else if asset.superseded_by.is_some() {
            bail!("only superseded assets may declare superseded_by");
        }
        if matches!(
            asset.authority_state,
            AuthorityState::SelectedPrivateProduction
                | AuthorityState::ApprovedPool
                | AuthorityState::PrincipalApproved
                | AuthorityState::ReleaseCleared
        ) && asset.authority_receipt_sha256.is_none()
        {
            bail!(
                "reviewed sonic asset {} requires an authority receipt hash",
                asset.asset_id
            );
        }
    }
    let pools = index_pools(&catalog.value)?;
    for pool in pools.values() {
        if pool.members.is_empty() {
            bail!("sonic asset pool {} is empty", pool.pool_id);
        }
        let mut members = BTreeSet::new();
        for member in &pool.members {
            if !members.insert((member.asset_id.as_str(), member.variant_id.as_str())) {
                bail!("sonic asset pool {} has a duplicate member", pool.pool_id);
            }
            let asset = assets.get(member.asset_id.as_str()).ok_or_else(|| {
                anyhow!("sonic asset pool {} references unknown asset", pool.pool_id)
            })?;
            select_variant(asset, Some(&member.variant_id))?;
            if !matches!(
                asset.authority_state,
                AuthorityState::ApprovedPool
                    | AuthorityState::PrincipalApproved
                    | AuthorityState::ReleaseCleared
            ) {
                bail!(
                    "sonic asset pool {} contains an unapproved member",
                    pool.pool_id
                );
            }
        }
    }
    Ok(())
}

fn validate_request(request: &Loaded<Request>) -> Result<()> {
    if request.value.schema != REQUEST_SCHEMA {
        bail!(
            "unsupported sonic asset request schema {}",
            request.value.schema
        );
    }
    nonempty("request_id", &request.value.request_id)?;
    validate_sha(
        "consumer_manifest_sha256",
        &request.value.consumer_manifest_sha256,
    )?;
    if request.value.bindings.is_empty() {
        bail!("sonic asset request has no bindings");
    }
    let mut events = BTreeSet::new();
    for binding in &request.value.bindings {
        nonempty("event_id", &binding.event_id)?;
        if !events.insert(binding.event_id.as_str()) {
            bail!(
                "duplicate sonic asset binding for event {}",
                binding.event_id
            );
        }
        match binding.selection {
            SelectionMode::Exact => {
                let asset_id = binding
                    .asset_id
                    .as_deref()
                    .ok_or_else(|| anyhow!("exact selection requires asset_id"))?;
                nonempty("asset_id", asset_id)?;
                if binding.pool_id.is_some()
                    || binding.pool_version.is_some()
                    || binding.selection_key.is_some()
                {
                    bail!("exact selection cannot declare pool fields");
                }
            }
            SelectionMode::ApprovedPool => {
                let pool_id = binding
                    .pool_id
                    .as_deref()
                    .ok_or_else(|| anyhow!("approved-pool selection requires pool_id"))?;
                let pool_version = binding
                    .pool_version
                    .as_deref()
                    .ok_or_else(|| anyhow!("approved-pool selection requires pool_version"))?;
                let selection_key = binding
                    .selection_key
                    .as_deref()
                    .ok_or_else(|| anyhow!("approved-pool selection requires selection_key"))?;
                nonempty("pool_id", pool_id)?;
                nonempty("pool_version", pool_version)?;
                nonempty("selection_key", selection_key)?;
                if binding.asset_id.is_some() || binding.variant_id.is_some() {
                    bail!("approved-pool selection cannot declare exact asset fields");
                }
            }
        }
    }
    Ok(())
}

fn validate_binding(binding: &RequestBinding, variant: &Variant) -> Result<()> {
    if let Some(required) = &binding.required_geometry {
        if variant.geometry.sample_rate_hz != required.sample_rate_hz
            || variant.geometry.bit_depth != required.bit_depth
            || required
                .channels
                .is_some_and(|value| value != variant.geometry.channels)
            || required
                .sample_count
                .is_some_and(|value| value != variant.geometry.sample_count)
        {
            bail!(
                "sonic asset for event {} has the wrong required geometry",
                binding.event_id
            );
        }
    }
    if binding.require_loop_region && variant.loop_region.is_none() {
        bail!(
            "sonic asset for event {} lacks a required loop region",
            binding.event_id
        );
    }
    for marker in &binding.required_sync_markers {
        if !variant.sync_markers.contains_key(marker) {
            bail!(
                "sonic asset for event {} lacks sync marker {}",
                binding.event_id,
                marker
            );
        }
    }
    Ok(())
}

fn validate_authority(asset: &Asset, request: &Request, mode: &str) -> Result<()> {
    let allowed = match asset.authority_state {
        AuthorityState::SelectedPrivateProduction
        | AuthorityState::PrincipalApproved
        | AuthorityState::ReleaseCleared => true,
        AuthorityState::ApprovedPool => mode == "approved-pool",
        AuthorityState::FixtureOnly => request.engineering_fixture,
        AuthorityState::Candidate
        | AuthorityState::Superseded
        | AuthorityState::DiagnosticPlaceholder => false,
    };
    if !allowed {
        bail!(
            "sonic asset {} authority state {:?} is not eligible for this request",
            asset.asset_id,
            asset.authority_state
        );
    }
    if !asset.license.permits_production_use && !request.engineering_fixture {
        bail!(
            "sonic asset {} is not license-cleared for production use",
            asset.asset_id
        );
    }
    Ok(())
}

fn receipt_for(resolution: &Resolution, resolution_sha256: &str) -> ResolutionReceipt {
    ResolutionReceipt {
        schema: RECEIPT_SCHEMA.to_string(),
        library_id: resolution.library_id.clone(),
        library_version: resolution.library_version.clone(),
        catalog_sha256: resolution.catalog_sha256.clone(),
        request_sha256: resolution.request_sha256.clone(),
        consumer_manifest_sha256: resolution.consumer_manifest_sha256.clone(),
        resolution_sha256: resolution_sha256.to_string(),
        tool_version: resolution.tool_version.clone(),
        assets: resolution
            .selections
            .iter()
            .map(|item| ReceiptAsset {
                event_id: item.event_id.clone(),
                asset_id: item.asset_id.clone(),
                variant_id: item.variant_id.clone(),
                sha256: item.sha256.clone(),
                bytes: item.bytes,
                geometry: item.geometry.clone(),
                authority_state: item.authority_state,
                authority_receipt_sha256: item.authority_receipt_sha256.clone(),
                license_id: item.license.license_id.clone(),
                lineage_sha256: item.lineage_sha256.clone(),
            })
            .collect(),
        path_free: true,
        selects_creative_output: false,
        grants_approval: false,
        passed: true,
    }
}

fn index_assets(catalog: &Catalog) -> Result<BTreeMap<&str, &Asset>> {
    let mut result = BTreeMap::new();
    for asset in &catalog.assets {
        nonempty("asset_id", &asset.asset_id)?;
        if result.insert(asset.asset_id.as_str(), asset).is_some() {
            bail!("duplicate sonic asset {}", asset.asset_id);
        }
    }
    Ok(result)
}

fn index_pools(catalog: &Catalog) -> Result<BTreeMap<&str, &Pool>> {
    let mut result = BTreeMap::new();
    for pool in &catalog.pools {
        nonempty("pool_id", &pool.pool_id)?;
        nonempty("pool_version", &pool.pool_version)?;
        if result.insert(pool.pool_id.as_str(), pool).is_some() {
            bail!("duplicate sonic asset pool {}", pool.pool_id);
        }
    }
    Ok(result)
}

fn select_variant<'a>(asset: &'a Asset, variant_id: Option<&str>) -> Result<&'a Variant> {
    match variant_id {
        Some(id) => asset
            .variants
            .iter()
            .find(|item| item.variant_id == id)
            .ok_or_else(|| anyhow!("sonic asset {} has no variant {id}", asset.asset_id)),
        None if asset.variants.len() == 1 => Ok(&asset.variants[0]),
        None => bail!("sonic asset {} variant is ambiguous", asset.asset_id),
    }
}

fn deterministic_member<'a>(
    pool: &'a Pool,
    catalog_sha: &str,
    request_id: &str,
    selection_key: &str,
) -> Result<&'a PoolMember> {
    if pool.members.is_empty() {
        bail!("sonic asset pool {} is empty", pool.pool_id);
    }
    let seed = format!(
        "{catalog_sha}\0{}\0{}\0{request_id}\0{selection_key}",
        pool.pool_id, pool.pool_version
    );
    let hash = Sha256::digest(seed.as_bytes());
    let index =
        u64::from_be_bytes(hash[0..8].try_into().expect("eight bytes")) % pool.members.len() as u64;
    Ok(&pool.members[index as usize])
}

fn resolve_locator(catalog_path: &Path, locator: &str) -> Result<PathBuf> {
    let path = Path::new(locator);
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        catalog_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(path)
    };
    candidate
        .canonicalize()
        .with_context(|| format!("failed to resolve sonic asset locator {locator}"))
}

fn inspect_pcm_wav(path: &Path) -> Result<AudioGeometry> {
    let bytes = fs::read(path)?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        bail!("sonic asset {} is not a RIFF/WAVE file", path.display());
    }
    let mut cursor = 12usize;
    let mut format = None;
    let mut data_bytes = None;
    while cursor.checked_add(8).is_some_and(|end| end <= bytes.len()) {
        let id = &bytes[cursor..cursor + 4];
        let size = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        let start = cursor + 8;
        let end = start
            .checked_add(size)
            .ok_or_else(|| anyhow!("WAV chunk length overflow"))?;
        if end > bytes.len() {
            bail!("truncated WAV chunk");
        }
        if id == b"fmt " {
            if size < 16 {
                bail!("invalid WAV fmt chunk");
            }
            format = Some((
                u16::from_le_bytes(bytes[start..start + 2].try_into().unwrap()),
                u16::from_le_bytes(bytes[start + 2..start + 4].try_into().unwrap()),
                u32::from_le_bytes(bytes[start + 4..start + 8].try_into().unwrap()),
                u16::from_le_bytes(bytes[start + 14..start + 16].try_into().unwrap()),
            ));
        } else if id == b"data" {
            data_bytes = Some(size as u64);
        }
        cursor = end + (size & 1);
    }
    let (audio_format, channels, sample_rate_hz, bit_depth) =
        format.ok_or_else(|| anyhow!("WAV lacks fmt chunk"))?;
    if audio_format != 1
        || !matches!(bit_depth, 16 | 24 | 32)
        || channels == 0
        || sample_rate_hz == 0
    {
        bail!("sonic assets must use integer PCM WAV with valid geometry");
    }
    let frame_bytes = u64::from(channels) * u64::from(bit_depth / 8);
    let data_bytes = data_bytes.ok_or_else(|| anyhow!("WAV lacks data chunk"))?;
    if data_bytes == 0 || data_bytes % frame_bytes != 0 {
        bail!("WAV data is not whole sample frames");
    }
    Ok(AudioGeometry {
        sample_rate_hz,
        bit_depth,
        channels,
        sample_count: data_bytes / frame_bytes,
    })
}

fn validate_sha_list(asset: &Asset) -> Result<()> {
    if let Some(value) = &asset.authority_receipt_sha256 {
        validate_sha("authority_receipt_sha256", value)?;
    }
    for value in &asset.lineage_sha256 {
        validate_sha("lineage_sha256", value)?;
    }
    nonempty("license_id", &asset.license.license_id)?;
    nonempty("license review_status", &asset.license.review_status)
}

fn validate_sha(label: &str, value: &str) -> Result<()> {
    if value.len() != 64
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        || value != value.to_ascii_lowercase()
    {
        bail!("{label} must be a lowercase SHA-256 hex digest");
    }
    Ok(())
}

fn nonempty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} must not be empty");
    }
    Ok(())
}

fn load_yaml_or_json<T: for<'de> Deserialize<'de>>(path: &Path, label: &str) -> Result<Loaded<T>> {
    let bytes =
        fs::read(path).with_context(|| format!("failed to read {label} {}", path.display()))?;
    let value = if path.extension().and_then(|value| value.to_str()) == Some("json") {
        serde_json::from_slice(&bytes).with_context(|| format!("failed to parse {label}"))?
    } else {
        serde_yaml::from_slice(&bytes).with_context(|| format!("failed to parse {label}"))?
    };
    Ok(Loaded {
        path: path.to_path_buf(),
        sha256: digest(&bytes),
        value,
    })
}

fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        bail!("refusing to overwrite {}", path.display());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp = NamedTempFile::new_in(parent)?;
    use std::io::Write;
    temp.write_all(bytes)?;
    temp.as_file_mut().sync_all()?;
    temp.persist_noclobber(path).map_err(|error| error.error)?;
    Ok(())
}

impl PartialEq for ResolutionReceipt {
    fn eq(&self, other: &Self) -> bool {
        canonical_json(self).ok() == canonical_json(other).ok()
    }
}
