//! Cache bridge for native REEL scene.json + scoped bindings. No renderer or daemon.
use crate::{Result, active_existing, config, load, object, selected, valid_oid, versions};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn read_small(path: &Path) -> Result<Value> {
    if fs::metadata(path)?.len() > 256 * 1024 {
        return Err("Scene control file exceeds 256 KiB".into());
    }
    load(path)
}

fn local(root: &Path, path: &Path) -> Result<PathBuf> {
    let path = root.join(path).canonicalize()?;
    if !path.starts_with(root.canonicalize()?) {
        return Err("Scene path escapes repository".into());
    }
    Ok(path)
}

/// Canonical snapshot: immutable scene and binding bytes independent of indentation.
pub fn snapshot(scene: &Value, bindings: &Value) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(
        &json!({"scene":scene,"bindings":bindings}),
    )?)
}

fn inspect(root: &Path, scene_path: &Path) -> Result<(Value, BTreeMap<String, Value>)> {
    let index = selected(root)?;
    let state = versions::read(root)?;
    inspect_with(root, scene_path, &index, &state)
}
pub(crate) fn inspect_with(
    root: &Path,
    scene_path: &Path,
    index: &std::collections::HashMap<String, u64>,
    state: &versions::State,
) -> Result<(Value, BTreeMap<String, Value>)> {
    let scene_path = local(root, scene_path)?;
    let scene = read_small(&scene_path)?;
    let catalog = read_small(&root.join("catalog.json"))?;
    let mut rows = catalog["videos"]
        .as_array()
        .ok_or("Missing scene catalog")?
        .iter()
        .filter(|row| {
            row["scene"]
                .as_str()
                .is_some_and(|p| local(root, Path::new(p)).ok().as_ref() == Some(&scene_path))
        });
    let row = rows.next().ok_or("Scene is not catalogued")?;
    if rows.next().is_some() {
        return Err("Ambiguous scene catalog entry".into());
    }
    let bindings_path = local(
        root,
        Path::new(row["bindings"].as_str().ok_or("Missing scoped bindings")?),
    )?;
    let bindings = read_small(&bindings_path)?;
    let schema = scene["schema"]
        .as_str()
        .ok_or("Missing REEL scene schema")?;
    if !matches!(
        schema,
        "reel.scene-authoring.v1" | "reel.scene-authoring.v2"
    ) || bindings["schema"] != "reel.scene-asset-bindings.v1"
        || scene["scene_id"].as_str().is_none_or(str::is_empty)
        || scene["scene_id"] != bindings["scope_id"]
    {
        return Err("REEL scene/binding scope mismatch".into());
    }
    if !matches!(
        scene["authoring_state"].as_str(),
        Some("imported-evidence" | "ready-for-private-build")
    ) {
        return Err("Unsupported scene authoring state".into());
    }
    let assets = bindings["assets"]
        .as_object()
        .ok_or("Missing scoped assets")?;
    if assets.is_empty() || assets.len() > 512 {
        return Err("Scene binding count exceeds budget".into());
    }
    let mut unique = BTreeMap::new();
    for (key, asset) in assets {
        let oid = asset["sha256"].as_str().ok_or("Missing binding hash")?;
        let size = asset["bytes"]
            .as_u64()
            .ok_or("Missing binding byte count")?;
        if !valid_oid(oid)
            || asset["cache_uri"] != format!("cache://sha256/{oid}")
            || asset["logical_id"].as_str().is_none_or(str::is_empty)
            || asset["selection_state"].as_str().is_none_or(str::is_empty)
            || key.is_empty()
            || index.get(oid) != Some(&size)
        {
            return Err("Invalid or unregistered exact scene binding".into());
        }
        unique.insert(oid.to_owned(), asset.clone());
    }
    let lock: versions::Dependency = serde_json::from_value(row["version_lock"].clone())?;
    let version = state
        .versions
        .get(&format!("{}@{}", lock.logical_id, lock.version_id))
        .ok_or("Scene version is missing")?;
    let digest = format!("{:x}", Sha256::digest(snapshot(&scene, &bindings)?));
    let protected = versions::payload_closure(state, &lock)?;
    if let Some(contract) = &version.dependency_contract
        && let Some(movie) = assets.get("delivery/movie")
        && !contract
            .outputs
            .contains(movie["sha256"].as_str().ok_or("Missing movie hash")?)
    {
        return Err("Classified scene movie must be a declared compiled output".into());
    }
    if lock.manifest_sha256 != digest
        || version.manifest_sha256 != digest
        || state.current.get(&lock.logical_id) != Some(&lock.version_id)
        || !unique.keys().all(|oid| protected.contains(oid))
    {
        return Err("Scene snapshot or immutable version lock is stale".into());
    }
    Ok((
        json!({"scene_id":scene["scene_id"],"authoring_state":scene["authoring_state"],
        "snapshot_sha256":digest,"bindings":assets.len(),"unique_assets":unique.len(),
        "version_lock":lock,
        "metadata_valid":true,"reel_semantic_build_validated":false,"production_cutover":false,
        "archive_probed":false,"payload_bytes_copied":0,"daemon_started":false}),
        unique,
    ))
}

pub fn check(root: &Path, scene: &Path, fresh: bool) -> Result<Value> {
    let index = selected(root)?;
    let state = versions::read(root)?;
    let (mut receipt, assets) = inspect_with(root, scene, &index, &state)?;
    let lock: versions::Dependency = serde_json::from_value(receipt["version_lock"].clone())?;
    let active = versions::verification_payloads(&state, &lock, &index)?;
    receipt["verification_scope"] = json!(if active.is_some() {
        "active-primary-input-and-output-closure"
    } else {
        "declared-assets"
    });
    let verification = active.unwrap_or_else(|| {
        assets
            .iter()
            .map(|(hash, a)| (hash.clone(), a["bytes"].as_u64().unwrap()))
            .collect()
    });
    receipt["deferred_reference_objects"] = json!(
        assets
            .keys()
            .filter(|h| !verification.contains_key(*h))
            .count()
    );
    receipt["fresh_hash_verified"] = json!(fresh);
    if fresh {
        let cfg = config(root)?;
        let total = verification.values().try_fold(0u64, |n, bytes| {
            n.checked_add(*bytes).ok_or("Scene read budget overflow")
        })?;
        if total > cfg.capacity.max_object_bytes {
            return Err("Scene verification exceeds configured read budget".into());
        }
        let mut tiers = BTreeMap::<usize, usize>::new();
        receipt["verified_objects"] = json!(verification.len());
        for (oid, bytes) in verification {
            let path = active_existing(&cfg, &oid, bytes)?
                .ok_or("Named scene asset absent from active caches; archive not probed")?;
            let tier = cfg
                .cache_roots
                .iter()
                .position(|cache| object(cache, &oid) == path)
                .ok_or("Unknown active tier")?;
            *tiers.entry(tier).or_default() += 1;
        }
        receipt["verified_bytes"] = json!(total);
        receipt["active_tiers"] = json!(tiers);
    }
    Ok(receipt)
}

pub fn locate(root: &Path, scene: &Path, binding: &str) -> Result<Value> {
    inspect(root, scene)?;
    let path = local(root, scene)?;
    let catalog = read_small(&root.join("catalog.json"))?;
    let row = catalog["videos"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["scene"]
                .as_str()
                .is_some_and(|p| local(root, Path::new(p)).ok().as_ref() == Some(&path))
        })
        .unwrap();
    let bindings = read_small(&local(root, Path::new(row["bindings"].as_str().unwrap()))?)?;
    let asset = bindings["assets"]
        .get(binding)
        .ok_or("Unknown scene binding")?;
    let oid = asset["sha256"].as_str().unwrap();
    for (tier, cache) in config(root)?.cache_roots.iter().enumerate() {
        let path = object(cache, oid);
        match fs::symlink_metadata(&path) {
            Ok(meta) => {
                if !meta.file_type().is_file() || meta.len() != asset["bytes"].as_u64().unwrap() {
                    return Err(
                        "Active scene binding is not a regular file of the declared size".into(),
                    );
                }
                return Ok(
                    json!({"binding":binding,"asset":asset,"path":path,"active_tier":tier,
                    "fresh_hash_verified":false,"archive_probed":false,"payload_bytes_copied":0}),
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Err("Named scene asset absent from active caches; archive not probed".into())
}
