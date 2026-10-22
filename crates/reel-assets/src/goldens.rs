//! Exact historical Golden snapshots. Read-only selection; restore never changes current.
use crate::{Result, active_existing, config, load, selected, valid_oid, versions};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 120
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
fn history(root: &Path) -> Result<Value> {
    let path = root.join("registry/goldens.json");
    if fs::metadata(&path)?.len() > 256 * 1024 {
        return Err("Golden index exceeds budget".into());
    }
    let doc = if let Some((oid, bytes)) = pointer_asset(root)? {
        if crate::generic_objects(root)?.get(&oid) != Some(&bytes) {
            return Err("Golden history control asset is not registered".into());
        }
        let path = active_existing(&config(root)?, &oid, bytes)?
            .ok_or("Golden history absent from active caches; archive not probed")?;
        load(&path)?
    } else {
        load(&path)?
    };
    if doc["schema"] != "caminos.golden-history.v1" {
        return Err("Unknown Golden history schema".into());
    }
    Ok(doc)
}

fn index_bytes(root: &Path) -> Result<Option<Vec<u8>>> {
    let path = root.join("registry/goldens.json");
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_file() => (),
        Ok(_) => return Err("Golden history control must be a regular file".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let path = crate::accounting::relative(root, "registry/goldens.json")?;
    let bytes = crate::tiering::read_bounded(&path, 256 * 1024)?;
    Ok(Some(bytes))
}
fn pointer_asset(root: &Path) -> Result<Option<(String, u64)>> {
    let Some(bytes) = index_bytes(root)? else {
        return Ok(None);
    };
    let asset = crate::registry::parse_pointer(&bytes)?;
    if asset
        .as_ref()
        .is_some_and(|(_, size)| *size == 0 || *size > 256 * 1024)
    {
        return Err("Golden history control asset exceeds budget".into());
    }
    Ok(asset)
}
/// Protect cache-backed control data with either a pointer or hydrated work copy.
pub(crate) fn control_asset(root: &Path) -> Result<Option<(String, u64)>> {
    if let Some(asset) = pointer_asset(root)? {
        return Ok(Some(asset));
    }
    let Some(bytes) = index_bytes(root)? else {
        return Ok(None);
    };
    // Git may leave a hydrated JSON working copy. Its registered backing asset
    // still needs protection for a checkout that leaves the LFS pointer intact.
    let hash = crate::tiering::digest(&bytes);
    let size = bytes.len() as u64;
    Ok((crate::generic_objects(root)?.get(&hash) == Some(&size)).then_some((hash, size)))
}

/// Explicitly retain exact existing index bytes in cache/LFS; never touch media.
pub fn compact(root: &Path) -> Result<Value> {
    history(root)?;
    if let Some((oid, bytes)) = pointer_asset(root)? {
        return Ok(
            json!({"already_compact":true,"sha256":oid,"bytes":bytes,"media_read":false,"archive_probed":false}),
        );
    }
    let path = crate::accounting::relative(root, "registry/goldens.json")?;
    let before = crate::tiering::read_bounded(&path, 256 * 1024)?;
    let asset = crate::ingest(root, &path)?;
    let oid = asset["sha256"]
        .as_str()
        .ok_or("Missing retained index hash")?;
    let bytes = asset["bytes"]
        .as_u64()
        .ok_or("Missing retained index size")?;
    if crate::tiering::digest(&before) != oid || before.len() as u64 != bytes {
        return Err("Golden history changed during compaction".into());
    }
    let parent = path.parent().ok_or("Missing registry directory")?;
    let register_path = crate::unique(parent, "golden-history-register");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&register_path)?;
    let registration = crate::Temporary(register_path);
    file.write_all(&serde_json::to_vec(&json!({"assets":[{"sha256":oid,"bytes":bytes,"cache_uri":format!("cache://sha256/{oid}"),"provenance":"Exact existing Golden-history control bytes","rights":"Project-private metadata; no new creative authority","selection_state":"required-control-metadata"}]}))?)?;
    file.sync_all()?;
    drop(file);
    crate::registry::register(root, &registration.0)?;
    let _lock = crate::admission::worktree_lock(root)?;
    if fs::read(&path)? != before {
        return Err("Golden history changed before pointer publication".into());
    }
    let pointer = crate::registry::pointer(oid, bytes)?;
    let temp_path = crate::unique(parent, "golden-history-index");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)?;
    let temp = crate::Temporary(temp_path);
    file.write_all(pointer.as_bytes())?;
    file.sync_all()?;
    drop(file);
    crate::versions::replace(&temp.0, &path)?;
    Ok(
        json!({"schema":"reel.golden-history-compaction.v1","asset":{"sha256":oid,"bytes":bytes,"cache_uri":format!("cache://sha256/{oid}")},"index_bytes_after":pointer.len(),"current_changed":false,"media_read":false,"archive_probed":false}),
    )
}
fn entry(root: &Path, video: &str, golden: &str) -> Result<Value> {
    if !safe_id(video) || !safe_id(golden) {
        return Err("Invalid video or Golden ID".into());
    }
    let doc = history(root)?;
    let rows = doc["videos"][video].as_array().ok_or("Unknown video ID")?;
    let mut matches = rows.iter().filter(|r| r["golden_id"] == golden);
    let row = matches
        .next()
        .ok_or("Unknown Golden ID; use golden-list")?
        .clone();
    if matches.next().is_some() {
        return Err("Ambiguous Golden ID".into());
    }
    Ok(row)
}

pub fn list(root: &Path, video: &str) -> Result<Value> {
    if !safe_id(video) {
        return Err("Invalid video ID".into());
    }
    let doc = history(root)?;
    let rows = doc["videos"][video].as_array().ok_or("Unknown video ID")?;
    let catalog = load(&root.join("catalog.json"))?;
    let current = catalog["videos"]
        .as_array()
        .ok_or("Missing catalog")?
        .iter()
        .find(|r| r["video_id"] == video)
        .ok_or("Unknown catalog video")?;
    let rows: Vec<_> = rows
        .iter()
        .map(|row| {
            let mut row = row.clone();
            row["current"] =
                json!(row["snapshot"]["sha256"] == current["version_lock"]["manifest_sha256"]);
            row
        })
        .collect();
    Ok(json!({"video_id":video,"goldens":rows,"media_read":false,"current_changed":false}))
}

fn snapshot(
    root: &Path,
    video: &str,
    golden: &str,
    fresh: bool,
) -> Result<(Value, Value, Value, BTreeMap<String, PathBuf>)> {
    let row = entry(root, video, golden)?;
    let asset = &row["snapshot"];
    let oid = asset["sha256"].as_str().ok_or("Missing snapshot hash")?;
    let size = asset["bytes"].as_u64().ok_or("Missing snapshot bytes")?;
    let index = selected(root)?;
    if !valid_oid(oid)
        || size > 256 * 1024
        || index.get(oid) != Some(&size)
        || asset["cache_uri"] != format!("cache://sha256/{oid}")
    {
        return Err("Invalid Golden snapshot descriptor".into());
    }
    let cfg = config(root)?;
    let path = active_existing(&cfg, oid, size)?
        .ok_or("Golden snapshot absent from active cache; archive not probed")?;
    let doc = load(&path)?;
    let scene = &doc["scene"];
    let bindings = &doc["bindings"];
    if !matches!(
        scene["schema"].as_str(),
        Some("reel.scene-authoring.v1" | "reel.scene-authoring.v2")
    ) || scene["scene_id"] != format!("music-video/{video}")
        || bindings["schema"] != "reel.scene-asset-bindings.v1"
        || scene["scene_id"] != bindings["scope_id"]
    {
        return Err("Golden scene/binding scope mismatch".into());
    }
    let lock: versions::Dependency = serde_json::from_value(row["version_lock"].clone())?;
    if lock.manifest_sha256 != oid {
        return Err("Golden snapshot lock differs".into());
    }
    let state = versions::read(root)?;
    let protected = versions::payload_closure(&state, &lock)?;
    let active = versions::verification_payloads(&state, &lock, &index)?;
    let classified = active.is_some();
    if let Some(contract) =
        &state.versions[&format!("{}@{}", lock.logical_id, lock.version_id)].dependency_contract
        && !contract.outputs.contains(
            row["movie_sha256"]
                .as_str()
                .ok_or("Missing Golden movie hash")?,
        )
    {
        return Err("Classified Golden movie must be a declared compiled output".into());
    }
    let assets = bindings["assets"]
        .as_object()
        .ok_or("Missing Golden asset bindings")?;
    if assets.is_empty() || assets.len() > 512 {
        return Err("Golden binding count exceeds budget".into());
    }
    let mut unique = BTreeMap::new();
    let mut total = 0u64;
    for asset in assets.values() {
        let oid = asset["sha256"].as_str().ok_or("Missing binding hash")?;
        let bytes = asset["bytes"].as_u64().ok_or("Missing binding bytes")?;
        if !valid_oid(oid)
            || asset["cache_uri"] != format!("cache://sha256/{oid}")
            || index.get(oid) != Some(&bytes)
            || !protected.contains(oid)
        {
            return Err("Golden binding is not registered and version-protected".into());
        }
        if unique.insert(oid.to_owned(), bytes).is_none() {
            total = total
                .checked_add(bytes)
                .ok_or("Golden read budget overflow")?;
        }
    }
    if assets
        .get("delivery/movie")
        .is_none_or(|a| a["sha256"] != row["movie_sha256"])
    {
        return Err("Golden movie differs from index".into());
    }
    let mut paths = BTreeMap::new();
    let verification = active.unwrap_or_else(|| unique.clone());
    let deferred = unique
        .keys()
        .filter(|hash| !verification.contains_key(*hash))
        .count();
    if classified {
        total = verification.values().try_fold(0u64, |n, bytes| {
            n.checked_add(*bytes).ok_or("Golden read budget overflow")
        })?;
    }
    if fresh {
        if total > cfg.capacity.max_object_bytes {
            return Err("Golden verification exceeds read budget".into());
        }
        for (oid, size) in &verification {
            let path = active_existing(&cfg, oid, *size)?
                .ok_or("Golden asset absent from active caches; archive not probed")?;
            paths.insert(oid.clone(), path);
        }
    }
    let receipt = json!({"video_id":video,"golden_id":golden,"snapshot_sha256":oid,
        "bindings":assets.len(),"fresh_hash_verified":fresh,"current_changed":false,
        "verification_scope":if classified {"active-primary-input-and-output-closure"}else{"declared-assets"},
        "deferred_reference_objects":deferred,"verified_objects":if fresh {Some(verification.len())}else{None},
        "payload_bytes_copied":0,"archive_probed":false,"movies_rebuilt":0,
        "reel_semantic_build_validated":false,"creative_authority":"not-granted-by-this-receipt"});
    Ok((scene.clone(), bindings.clone(), receipt, paths))
}

pub fn check(root: &Path, video: &str, golden: &str, fresh: bool) -> Result<Value> {
    Ok(snapshot(root, video, golden, fresh)?.2)
}

pub fn restore(root: &Path, video: &str, golden: &str) -> Result<Value> {
    // Verify the declared legacy scope or classified active closure before
    // writing any restored control files. Historical references remain cold.
    let (scene, bindings, mut receipt, paths) = snapshot(root, video, golden, true)?;
    let root = root.canonicalize()?;
    let base = root.join(".local/restored-goldens");
    fs::create_dir_all(&base)?;
    let base = base.canonicalize()?;
    if !base.starts_with(root.join(".local")) {
        return Err("Restore directory escapes private workspace".into());
    }
    let name = format!(
        "{video}-{golden}-{}",
        &receipt["snapshot_sha256"].as_str().unwrap()[..12]
    );
    let target = base.join(name);
    let contents = [
        ("scene.json", serde_json::to_vec_pretty(&scene)?),
        ("scene-bindings.json", serde_json::to_vec_pretty(&bindings)?),
    ];
    if target.exists() {
        if target.symlink_metadata()?.file_type().is_symlink() || !target.is_dir() {
            return Err("Restore target is not an owned regular directory".into());
        }
        for (name, bytes) in &contents {
            let file = target.join(name);
            if !file.symlink_metadata()?.file_type().is_file() || fs::read(file)? != *bytes {
                return Err("Existing restored files changed; refusing overwrite".into());
            }
        }
        receipt["reused_restore"] = json!(true);
    } else {
        let staging = crate::unique(&base, "restore");
        fs::create_dir(&staging)?;
        for (name, bytes) in contents {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(staging.join(name))?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        fs::rename(&staging, &target)?;
        receipt["reused_restore"] = json!(false);
    }
    receipt["directory"] = json!(target);
    let movie = bindings["assets"]["delivery/movie"]["sha256"]
        .as_str()
        .unwrap();
    receipt["movie_path"] = json!(paths[movie]);
    Ok(receipt)
}
