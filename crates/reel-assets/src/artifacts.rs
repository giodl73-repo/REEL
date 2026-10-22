//! Family-neutral immutable artifact packages: songs now, arrangements/documents later.
use crate::{Result, active_existing, config, load, selected, valid_oid, versions};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

fn small(path: &Path) -> Result<Value> {
    if fs::metadata(path)?.len() > 256 * 1024 {
        return Err("Artifact control file exceeds budget".into());
    }
    load(path)
}

pub(crate) fn inspect(
    root: &Path,
    record: &Path,
    version: Option<&str>,
    fresh: bool,
) -> Result<(Value, BTreeMap<String, Value>)> {
    let index = selected(root)?;
    let state = versions::read(root)?;
    let cfg = config(root)?;
    inspect_with(root, record, version, fresh, &index, &state, &cfg)
}

pub(crate) fn inspect_with(
    root: &Path,
    record: &Path,
    version: Option<&str>,
    fresh: bool,
    index: &std::collections::HashMap<String, u64>,
    state: &versions::State,
    cfg: &crate::Config,
) -> Result<(Value, BTreeMap<String, Value>)> {
    let root = root.canonicalize()?;
    let path = root.join(record).canonicalize()?;
    if !path.starts_with(&root) {
        return Err("Artifact record escapes repository".into());
    }
    let doc = small(&path)?;
    if doc["schema"] != "caminos.artifact-record.v1"
        || doc["artifact_id"].as_str().is_none_or(str::is_empty)
        || doc["kind"].as_str().is_none_or(str::is_empty)
    {
        return Err("Invalid artifact record".into());
    }
    let wanted = version
        .or_else(|| doc["current_version"].as_str())
        .ok_or("Missing artifact version")?;
    let rows = doc["versions"]
        .as_array()
        .ok_or("Missing artifact versions")?;
    if rows.len() > 128 {
        return Err("Artifact version index exceeds budget".into());
    }
    let mut matches = rows.iter().filter(|v| v["version_id"] == wanted);
    let row = matches.next().ok_or("Unknown artifact version")?;
    if matches.next().is_some() {
        return Err("Ambiguous artifact version".into());
    }
    let lock: versions::Dependency = serde_json::from_value(row["version_lock"].clone())?;
    let manifest = &row["manifest"];
    let oid = manifest["sha256"].as_str().ok_or("Missing package hash")?;
    let size = manifest["bytes"]
        .as_u64()
        .ok_or("Missing package byte count")?;
    if !valid_oid(oid)
        || size > 256 * 1024
        || manifest["cache_uri"] != format!("cache://sha256/{oid}")
        || index.get(oid) != Some(&size)
        || lock.manifest_sha256 != oid
        || doc["artifact_id"].as_str() != Some(lock.logical_id.as_str())
    {
        return Err("Invalid artifact package descriptor or lock".into());
    }
    let protected = versions::payload_closure(state, &lock)?;
    let classified = state.versions[&format!("{}@{}", lock.logical_id, lock.version_id)]
        .dependency_contract
        .is_some();
    // Provenance remains authorized and recoverable, but typed active verification
    // must not require historical outputs to be promoted out of the archive.
    let active = if classified {
        Some(crate::accounting::expand(
            state,
            [format!("{}@{}", lock.logical_id, lock.version_id)],
        )?)
    } else {
        None
    };
    if version.is_none() && state.current.get(&lock.logical_id) != Some(&lock.version_id) {
        return Err("Artifact current selection is stale".into());
    }
    let package_path = active_existing(cfg, oid, size)?
        .ok_or("Artifact package absent from active caches; archive not probed")?;
    let package = small(&package_path)?;
    if package["schema"] != "caminos.artifact-package.v1"
        || package["artifact_id"] != doc["artifact_id"]
        || package["version_id"] != wanted
        || package["kind"] != doc["kind"]
    {
        return Err("Artifact package scope/version mismatch".into());
    }
    if let Some(edition) = row.get("edition")
        && package.get("edition") != Some(edition)
    {
        return Err("Artifact edition differs from its selected package".into());
    }
    let assets = package["assets"]
        .as_object()
        .ok_or("Missing artifact assets")?;
    if assets.is_empty() || assets.len() > 512 {
        return Err("Artifact asset count exceeds budget".into());
    }
    let mut total = 0u64;
    let mut unique = BTreeMap::new();
    for a in assets.values() {
        let hash = a["sha256"].as_str().ok_or("Missing artifact asset hash")?;
        let bytes = a["bytes"].as_u64().ok_or("Missing artifact asset bytes")?;
        if !valid_oid(hash)
            || index.get(hash) != Some(&bytes)
            || a["cache_uri"] != format!("cache://sha256/{hash}")
            || !protected.contains(hash)
        {
            return Err("Unregistered or unprotected artifact asset".into());
        }
        if unique.insert(hash.to_owned(), bytes).is_none() {
            total = total
                .checked_add(bytes)
                .ok_or("Artifact read budget overflow")?;
        }
    }
    let mut verification = unique.clone();
    if let Some(active) = &active {
        verification = active
            .iter()
            .map(|hash| {
                let bytes = index
                    .get(hash)
                    .ok_or("Unregistered active artifact dependency")?;
                Ok((hash.clone(), *bytes))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        total = verification.values().try_fold(0u64, |sum, bytes| {
            sum.checked_add(*bytes)
                .ok_or("Artifact read budget overflow")
        })?;
    }
    let deferred_references = unique
        .keys()
        .filter(|hash| !verification.contains_key(*hash))
        .count();
    if fresh {
        if total > cfg.capacity.max_object_bytes {
            return Err("Artifact verification exceeds read budget".into());
        }
        for (hash, bytes) in &verification {
            active_existing(cfg, hash, *bytes)?
                .ok_or("Artifact asset absent from active caches; archive not probed")?;
        }
    }
    Ok((
        json!({"artifact_id":doc["artifact_id"],"kind":doc["kind"],"version_id":wanted,
        "assets":assets.len(),"unique_assets":unique.len(),"fresh_hash_verified":fresh,
        "verification_scope":if classified {"active-primary-input-and-output-closure"}else{"declared-assets"},
        "verified_objects":if fresh {Some(verification.len())}else{None},
        "deferred_reference_objects":deferred_references,
        "package_sha256":oid,"verified_bytes":if fresh {Some(total)}else{None},"archive_probed":false,
        "payload_bytes_copied":0,"creative_authority":"not-granted-by-this-receipt"}),
        assets.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
    ))
}

pub fn check(root: &Path, record: &Path, version: Option<&str>, fresh: bool) -> Result<Value> {
    Ok(inspect(root, record, version, fresh)?.0)
}

pub fn locate(root: &Path, record: &Path, version: Option<&str>, role: &str) -> Result<Value> {
    let (receipt, assets) = inspect(root, record, version, false)?;
    let a = assets.get(role).ok_or("Unknown artifact role")?;
    let oid = a["sha256"].as_str().unwrap();
    let size = a["bytes"].as_u64().unwrap();
    for (tier, cache) in config(root)?.cache_roots.iter().enumerate() {
        let path = crate::object(cache, oid);
        match fs::symlink_metadata(&path) {
            Ok(meta) => {
                if !meta.file_type().is_file() || meta.len() != size {
                    return Err("Invalid active artifact object".into());
                }
                return Ok(
                    json!({"artifact_id":receipt["artifact_id"],"version_id":receipt["version_id"],"role":role,"asset":a,"path":path,"active_tier":tier,"fresh_hash_verified":false,"archive_probed":false}),
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Err("Artifact absent from active caches; archive not probed".into())
}
