//! Explicit native asset projections from checked current/prior source packages.
use crate::tig_native_registration::{
    DefinitionFiles, Package, Prepared, filename, read_local, read_metadata,
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
};
use tig_assets::native::{Definition, Instance};
use tig_core::{Object, identity};
use tig_store::existing::{ByteSource, Profile as ByteProfile, ReadCache};

pub const SCHEMA: &str = "reel.tig-package-assets-registration.experimental-1";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub filename: String,
    pub definition: DefinitionFiles,
    pub authored: Map<String, Value>,
    /// Exactly every display version; null explicitly records absence then.
    pub assets: BTreeMap<String, Option<String>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub byte_source: ByteProfile,
    pub source_record: Object,
    pub parent_definition: DefinitionFiles,
    pub parent_filename: String,
    #[serde(default)]
    pub parent_authored: Map<String, Value>,
    pub entries: BTreeMap<String, Entry>,
    pub max_output_bytes: u64,
}
pub fn prepare(profile: &Profile, source: &mut impl ByteSource) -> Result<Prepared> {
    if profile.schema != SCHEMA
        || profile.entries.is_empty()
        || profile.entries.len() > 128
        || profile.max_output_bytes == 0
        || profile.max_output_bytes > 16 * 1024 * 1024
        || profile.source_record.bytes > 65_536
        || profile.parent_authored.contains_key("asset_ids")
    {
        bail!("unsupported package asset registration profile");
    }
    filename(&profile.parent_filename)?;
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut references = Map::new();
    for (name, entry) in &profile.entries {
        filename(&entry.filename)?;
        if name.is_empty()
            || name.len() > 256
            || name.chars().any(char::is_control)
            || !ids.insert(entry.id.clone())
            || !names.insert(entry.filename.to_ascii_lowercase())
        {
            bail!("duplicate or invalid native asset mapping");
        }
        references.insert(name.clone(), json!({"id":entry.id}));
    }
    let definition = read_local(&profile.parent_definition.definition)?;
    let schema = read_local(&profile.parent_definition.authored_schema)?;
    let mut fields = profile.parent_authored.clone();
    fields.insert("asset_ids".into(), Value::Object(references));
    let mut remaining = 16 * 1024 * 1024;
    let raw = read_metadata(source, &profile.source_record, &mut remaining)?;
    let parent_bytes = crate::tig_versioned_registration::prepare_with_fields(
        &profile.source_record,
        &raw,
        &definition,
        &schema,
        &fields,
    )?;
    let mut parent: Value = serde_json::from_slice(&parent_bytes)?;
    let parent_id = parent["id"].as_str().context("source identity missing")?;
    if ids.contains(parent_id) {
        bail!("leaf identity collides with parent");
    }
    let current = parent["authored"]["current_version"]
        .as_str()
        .context("current version missing")?;
    let mut packages = BTreeMap::new();
    for stream in parent["streams"]
        .as_array()
        .context("version streams missing")?
    {
        if stream["group"] != "version" {
            continue;
        }
        let version = stream["slot"]
            .as_str()
            .and_then(|s| s.strip_prefix("version/"))
            .context("version slot missing")?;
        let pin: Object = serde_json::from_value(stream["object"].clone())?;
        let package: Package =
            serde_json::from_slice(&read_metadata(source, &pin, &mut remaining)?)?;
        if package.schema != "caminos.artifact-package.v1"
            || package.artifact_id != parent_id
            || Some(package.version_id.as_str()) != stream["provenance"]["version_lock_id"].as_str()
        {
            bail!("package scope or immutable lock differs");
        }
        packages.insert(version.to_owned(), (pin, package));
    }
    let versions: BTreeSet<_> = packages.keys().cloned().collect();
    let mut records = BTreeMap::new();
    for entry in profile.entries.values() {
        if entry.assets.keys().cloned().collect::<BTreeSet<_>>() != versions {
            bail!("every source version needs an explicit asset mapping or absence");
        }
        let mut selected = None;
        let mut streams = Vec::new();
        for (version, role) in &entry.assets {
            let Some(role) = role else { continue };
            let (package_pin, package) = &packages[version];
            let pin = package
                .assets
                .get(role)
                .context("mapped package asset missing")?
                .object()?;
            if version == current {
                selected = Some(pin.clone());
            }
            streams.push(json!({"slot":format!("version/{version}"),"group":"version","role":"source/asset-history","object":pin,
                "provenance":{"source_package":package_pin,"source_asset":role,"version_lock_id":package.version_id}}));
        }
        let selected = selected.context("current asset cannot be absent")?;
        let leaf_def = read_local(&entry.definition.definition)?;
        let leaf_schema = read_local(&entry.definition.authored_schema)?;
        let declared = Definition::parse(&leaf_def)?;
        let bytes = serde_json::to_vec_pretty(
            &json!({"schema":"tig.instance.design-1","id":entry.id,
            "definition":{"id":declared.value().id,"sha256":Object::from_bytes(&leaf_def).sha256,"bytes":leaf_def.len()},
            "authored":entry.authored,"selected":selected,"streams":streams,
            "status":{"lifecycle":"registered-native-asset","production_state":"unclassified"},
            "metadata":{"package_assets_registration":{"source_record":profile.source_record,"artifact_id":parent_id}}}),
        )?;
        Instance::parse(&bytes)?.bind(&leaf_def, &leaf_schema)?;
        records.insert(format!("inputs/{}", entry.filename), bytes);
    }
    parent["status"]["lifecycle"] = json!("registered-native-package");
    let bytes = serde_json::to_vec_pretty(&parent)?;
    Instance::parse(&bytes)?.bind(&definition, &schema)?;
    records.insert(profile.parent_filename.clone(), bytes);
    let bytes = records.values().try_fold(0u64, |sum, bytes| {
        sum.checked_add(bytes.len() as u64)
            .context("output size overflow")
    })?;
    if bytes > profile.max_output_bytes {
        bail!("native candidate set exceeds output budget");
    }
    Ok(Prepared { records, bytes })
}
fn run_inner(input: &Path, output: &Path) -> Result<Value> {
    let mut bytes = Vec::new();
    fs::File::open(input)?
        .take(65_537)
        .read_to_end(&mut bytes)?;
    let profile: Profile = serde_json::from_value(identity::parse_json(&bytes)?)?;
    if !output.is_absolute()
        || output.symlink_metadata().is_ok()
        || !output.parent().is_some_and(|p| p.is_dir())
    {
        bail!("new absolute candidate directory required");
    }
    let mut source = ReadCache::new(profile.byte_source.clone())?;
    let prepared = prepare(&profile, &mut source)?;
    let mut result = prepared.write(output)?;
    result["schema"] = json!("reel.tig-package-assets-registration-result.experimental-1");
    result["mapped_assets"] = json!(profile.entries.len());
    result["authorizes_eviction"] = json!(false);
    Ok(result)
}
pub fn run(input: &Path, output: &Path) -> Result<Value> {
    run_inner(input, output).map_err(|_|anyhow::anyhow!("package asset registration refused; verify source, mappings, definitions, bounds and new destination"))
}
