//! Family-neutral preservation of immutable source package versions.
use crate::tig_native_registration::{DefinitionFiles, read_local};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::Path,
};
use tig_assets::native::{Definition, Instance};
use tig_core::{Object, identity};
use tig_store::existing::{ByteSource, Profile as ByteProfile, ReadCache};

const LIMIT: usize = 65_536;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    source_record: Object,
    byte_source: ByteProfile,
    definition: DefinitionFiles,
}

/// Pure projection; the caller supplies exact source and definition bytes.
/// Packages stay opaque. This is not graph closure, availability or approval.
pub fn prepare(source: &Object, bytes: &[u8], definition: &[u8], schema: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() > LIMIT || Object::from_bytes(bytes) != *source {
        bail!("versioned source differs from its bounded admission");
    }
    let doc = identity::parse_json(bytes)?;
    let text = |value: &Value| -> Result<String> {
        let value = value.as_str().context("versioned identity missing")?;
        if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
            bail!("versioned identity exceeds contract");
        }
        Ok(value.to_owned())
    };
    if doc["schema"] != "caminos.artifact-record.v1" {
        bail!("unsupported versioned source format");
    }
    let id = text(&doc["artifact_id"])?;
    let kind = text(&doc["kind"])?;
    let current = text(&doc["current_version"])?;
    let public = doc["public_release"]
        .as_bool()
        .context("source release declaration missing")?;
    let versions = doc["versions"]
        .as_array()
        .context("source versions missing")?;
    if versions.is_empty() || versions.len() > 255 {
        bail!("version count exceeds bounded record contract");
    }
    let mut names = BTreeSet::new();
    let mut sizes = std::collections::BTreeMap::new();
    let mut selected = None;
    let mut streams = vec![
        json!({"slot":"source-record","role":"evidence/source-record","group":"source","object":source}),
    ];
    for version in versions {
        let name = text(&version["version_id"])?;
        if !names.insert(name.clone()) {
            bail!("duplicate source version");
        }
        let manifest = &version["manifest"];
        let object: Object =
            serde_json::from_value(json!({"sha256":manifest["sha256"],"bytes":manifest["bytes"]}))?;
        if manifest["cache_uri"] != format!("cache://sha256/{}", object.sha256.as_str())
            || version["version_lock"]["logical_id"] != id
            || version["version_lock"]["version_id"] != name
            || version["version_lock"]["manifest_sha256"] != object.sha256.as_str()
            || sizes
                .insert(object.sha256.clone(), object.bytes)
                .is_some_and(|n| n != object.bytes)
        {
            bail!("source version lock, URI or size differs");
        }
        if name == current {
            selected = Some(object.clone());
        }
        streams.push(json!({"slot":format!("version/{name}"),"role":"source/package","group":"version","object":object}));
    }
    let selected = selected.context("source current version is absent")?;
    let definition_value = Definition::parse(definition)?;
    let pin = Object::from_bytes(definition);
    let output = serde_json::to_vec_pretty(&json!({
        "schema":"tig.instance.design-1","id":id,
        "definition":{"id":definition_value.value().id,"sha256":pin.sha256,"bytes":pin.bytes},
        "authored":{"artifact_id":id,"kind":kind,"current_version":current,"public_release":public},
        "selected":selected,"streams":streams,
        "status":{"lifecycle":"registered-source-package","production_state":"unclassified"},
        "metadata":{"versioned_registration":{"source_record":source,"source_schema":"caminos.artifact-record.v1"}}
    }))?;
    Instance::parse(&output)?.bind(definition, schema)?;
    Ok(output)
}

fn run_inner(input: &Path, output: &Path) -> Result<Value> {
    let mut bytes = Vec::new();
    fs::File::open(input)?
        .take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    let profile: Profile = serde_json::from_value(identity::parse_json(&bytes)?)?;
    if profile.schema != "reel.tig-versioned-registration.experimental-1"
        || profile.source_record.bytes > LIMIT as u64
    {
        bail!("unsupported versioned profile");
    }
    let parent = output.parent().context("candidate parent missing")?;
    if !output.is_absolute() || output.symlink_metadata().is_ok() || !parent.is_dir() {
        bail!("candidate must be a new absolute file in an existing directory");
    }
    let definition = read_local(&profile.definition.definition)?;
    let schema = read_local(&profile.definition.authored_schema)?;
    let mut cache = ReadCache::new(profile.byte_source)?;
    let mut source = Vec::new();
    cache
        .open_verified(&profile.source_record)?
        .take(LIMIT as u64 + 1)
        .read_to_end(&mut source)?;
    let candidate = prepare(&profile.source_record, &source, &definition, &schema)?;
    let instance = Instance::parse(&candidate)?;
    let result = json!({"schema":"reel.tig-versioned-registration-result.experimental-1",
        "records":1,"record_bytes":candidate.len(),"versions":instance.value().streams.len()-1,
        "source_records_verified":1,"package_media_opened":0,"adopted":false,
        "complete_semantic_coverage":false,"authorizes_eviction":false});
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&candidate)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(output)
        .map_err(|_| anyhow::anyhow!("candidate publication refused"))?;
    Ok(result)
}

/// Reads named small metadata only; never discovers or publishes cache objects.
pub fn run(input: &Path, output: &Path) -> Result<Value> {
    run_inner(input, output).map_err(|_| {
        anyhow::anyhow!(
            "versioned registration refused; verify source, definition, bounds and destination"
        )
    })
}
