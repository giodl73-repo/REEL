//! Explicit current/history projections from an immutable binding manifest.
//! No media reads, inferred versions, approval, execution or cache mutation.
use crate::tig_native_registration::{
    DefinitionFiles, Prepared, filename, read_local, read_metadata,
};
use anyhow::{Context, Result, bail};
use reel_assembly::scene_authoring::AssetRef;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
};
use tig_assets::native::{Definition, Instance};
use tig_core::{Digest, Object, identity};
use tig_store::existing::{Profile as ByteProfile, ReadCache};

pub const SCHEMA: &str = "reel.tig-binding-assets-registration.experimental-1";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub filename: String,
    pub definition: DefinitionFiles,
    pub authored: Map<String, Value>,
    /// Caller names every retained slot explicitly; no timestamp/version guessing.
    pub history: BTreeMap<String, String>,
    /// Original decision/evidence binding keys, preserved as pins only.
    pub authorities: BTreeSet<String>,
    /// Complete explicit source-ID map when adopting a stable canonical ID.
    #[serde(default)]
    pub source_ids: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub source: Object,
    pub scope_id: String,
    pub byte_source: ByteProfile,
    /// Map current binding key to independent native asset registration.
    pub entries: BTreeMap<String, Entry>,
    pub max_output_bytes: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bindings {
    schema: String,
    scope_id: String,
    #[serde(deserialize_with = "crate::tig_native_registration::unique_map")]
    assets: BTreeMap<String, AssetRef>,
}
fn object(asset: &AssetRef) -> Result<Object> {
    let object = Object {
        sha256: Digest::parse(&asset.sha256)?,
        bytes: asset.bytes,
    };
    if asset.cache_uri != format!("cache://sha256/{}", object.sha256.as_str())
        || object.bytes > 9_007_199_254_740_991
        || asset.logical_id.is_empty()
        || asset.logical_id.len() > 256
        || asset.logical_id.chars().any(char::is_control)
        || asset.selection_state.is_empty()
        || asset.selection_state.len() > 256
        || asset.selection_state.chars().any(char::is_control)
    {
        bail!("invalid source binding identity");
    }
    Ok(object)
}
fn prepare(p: &Profile) -> Result<Prepared> {
    if p.schema != SCHEMA
        || p.entries.is_empty()
        || p.entries.len() > 128
        || p.max_output_bytes == 0
        || p.max_output_bytes > 16 * 1024 * 1024
    {
        bail!("unsupported binding registration scope");
    }
    let mut source = ReadCache::new(p.byte_source.clone())?;
    let mut remaining = 16 * 1024 * 1024;
    let bytes = read_metadata(&mut source, &p.source, &mut remaining)?;
    // Legacy binding manifests can exceed Tig's 64-KiB canonical record limit.
    // Their exact bytes stay asset-side. Bounded intake and unique asset keys
    // remain enforced independently; no canonical-record limit is relaxed.
    let bindings: Bindings = serde_json::from_slice(&bytes)?;
    if bindings.schema != "reel.scene-asset-bindings.v1"
        || bindings.scope_id != p.scope_id
        || bindings.assets.len() > 4096
    {
        bail!("source binding scope differs");
    }
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut records = BTreeMap::new();
    let mut sizes = BTreeMap::new();
    let mut checked = |key: &str| -> Result<(&AssetRef, Object)> {
        let asset = bindings
            .assets
            .get(key)
            .context("mapped source binding missing")?;
        let object = object(asset)?;
        if sizes
            .insert(object.sha256.clone(), object.bytes)
            .is_some_and(|old| old != object.bytes)
        {
            bail!("source binding lengths conflict");
        }
        Ok((asset, object))
    };
    for (key, entry) in &p.entries {
        filename(&entry.filename)?;
        if !ids.insert(&entry.id)
            || !names.insert(entry.filename.to_lowercase())
            || entry.history.len() > 256
            || entry.history.contains_key("current")
            || entry.history.contains_key("source-bindings")
            || entry.authorities.len() > 128
        {
            bail!("duplicate or unbounded binding mapping");
        }
        let (current, selected) = checked(key)?;
        let expected: BTreeSet<_> = std::iter::once(key).chain(entry.history.values()).collect();
        if !entry.source_ids.is_empty()
            && entry.source_ids.keys().collect::<BTreeSet<_>>() != expected
        {
            bail!("canonical mapping must name every current/historical binding");
        }
        if current.logical_id != *entry.source_ids.get(key).unwrap_or(&entry.id) {
            bail!("binding identity cannot be silently renamed");
        }
        let mut streams = vec![
            json!({"slot":"current","group":"current","role":key,"object":selected,"provenance":{"source_binding":key,"source_logical_id":current.logical_id,"source_selection_state":current.selection_state}}),
        ];
        let mut seen = BTreeSet::new();
        for (slot, binding) in &entry.history {
            if !seen.insert(binding) || binding == key {
                bail!("repeated historical binding");
            }
            let (asset, pin) = checked(binding)?;
            if asset.logical_id != *entry.source_ids.get(binding).unwrap_or(&entry.id) {
                bail!("historical binding belongs to another asset");
            }
            streams.push(json!({"slot":slot,"group":"history","role":key,"object":pin,"provenance":{"source_binding":binding,"source_logical_id":asset.logical_id,"source_selection_state":asset.selection_state,"version_identity":"explicit source binding; no invented version ID"}}));
        }
        streams.push(json!({"slot":"source-bindings","group":"source","role":"evidence/source-bindings","object":p.source}));
        let mut authorities = BTreeMap::new();
        for key in &entry.authorities {
            let (asset, pin) = checked(key)?;
            authorities.insert(key,json!({"object":pin,"source_logical_id":asset.logical_id,"source_selection_state":asset.selection_state}));
        }
        let definition = read_local(&entry.definition.definition)?;
        let schema = read_local(&entry.definition.authored_schema)?;
        let declared = Definition::parse(&definition)?;
        let bytes = serde_json::to_vec_pretty(
            &json!({"schema":"tig.instance.design-1","id":entry.id,
            "definition":{"id":declared.value().id,"sha256":Object::from_bytes(&definition).sha256,"bytes":definition.len()},
            "authored":entry.authored,"selected":selected,"streams":streams,
            "status":{"selection":"retained-source-selection","subject":selected,"human_approval":"not-reasserted"},
            "metadata":{"binding_assets_registration":{"source":p.source,"scope_id":p.scope_id,"source_binding":key,"source_selection_state":current.selection_state,"canonical_source_ids":entry.source_ids,"complete_reference_coverage":false,"native_build":"not-established","public_release":false},"source_authorities":authorities}}),
        )?;
        Instance::parse(&bytes)?.bind(&definition, &schema)?;
        records.insert(entry.filename.clone(), bytes);
    }
    let bytes = records.values().try_fold(0u64, |sum, b| {
        sum.checked_add(b.len() as u64)
            .context("candidate size overflow")
    })?;
    if bytes > p.max_output_bytes {
        bail!("candidate set exceeds admitted budget");
    }
    Ok(Prepared { records, bytes })
}
fn run_inner(path: &Path, output: &Path) -> Result<Value> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 128 * 1024 {
        bail!("binding profile exceeds bound");
    }
    let p: Profile = serde_json::from_value(identity::parse_json(&bytes)?)?;
    if !output.is_absolute()
        || output.symlink_metadata().is_ok()
        || !output.parent().is_some_and(|p| p.is_dir())
    {
        bail!("new absolute candidate directory required");
    }
    let prepared = prepare(&p)?;
    let mut report = prepared.write(output)?;
    report["schema"] = json!("reel.tig-binding-assets-registration-result.experimental-1");
    report["mapped_assets"] = json!(p.entries.len());
    report["payloads_verified"] = json!(false);
    report["authorizes_eviction"] = json!(false);
    Ok(report)
}
pub fn run(path: &Path, output: &Path) -> Result<Value> {
    run_inner(path,output).map_err(|_|anyhow::anyhow!("binding asset registration refused; verify source, scope, mappings, definitions, bounds and new destination"))
}
