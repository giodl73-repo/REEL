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
    let doc = load(&path)?;
    if doc["schema"] != "caminos.golden-history.v1" {
        return Err("Unknown Golden history schema".into());
    }
    Ok(doc)
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
    let protected = versions::payload_closure(&versions::read(root)?, &lock)?;
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
    if fresh {
        if total > cfg.capacity.max_object_bytes {
            return Err("Golden verification exceeds read budget".into());
        }
        for (oid, size) in unique {
            let path = active_existing(&cfg, &oid, size)?
                .ok_or("Golden asset absent from active caches; archive not probed")?;
            paths.insert(oid, path);
        }
    }
    let receipt = json!({"video_id":video,"golden_id":golden,"snapshot_sha256":oid,
        "bindings":assets.len(),"fresh_hash_verified":fresh,"current_changed":false,
        "payload_bytes_copied":0,"archive_probed":false,"movies_rebuilt":0,
        "reel_semantic_build_validated":false,"creative_authority":"not-granted-by-this-receipt"});
    Ok((scene.clone(), bindings.clone(), receipt, paths))
}

pub fn check(root: &Path, video: &str, golden: &str, fresh: bool) -> Result<Value> {
    Ok(snapshot(root, video, golden, fresh)?.2)
}

pub fn restore(root: &Path, video: &str, golden: &str) -> Result<Value> {
    // Verify all declared assets before writing any restored control files.
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
