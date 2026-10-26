//! Bounded authorization metadata: a V1 leaf or up to sixteen V2 prefix leaves.
use crate::{Result, load, valid_oid};
use serde_json::Value;
use std::{collections::BTreeSet, fs, path::Path};

pub(crate) const MAX_CONTROL_BYTES: u64 = 4096;
pub(crate) const MAX_LEAF_BYTES: u64 = 4 * 1024 * 1024;

pub(crate) fn assets(root: &Path) -> Result<Vec<(Option<u8>, Value)>> {
    let path = root.join("registry/object-index.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    if fs::metadata(&path)?.len() > MAX_CONTROL_BYTES {
        return Err("Object index control exceeds budget".into());
    }
    descriptor_assets(&load(&path)?)
}

pub(crate) fn descriptor_assets(descriptor: &Value) -> Result<Vec<(Option<u8>, Value)>> {
    let assets = match descriptor["schema"].as_str() {
        Some("caminos.object-index.v1") => vec![(None, descriptor["asset"].clone())],
        Some("caminos.object-index.v2") => {
            if descriptor.as_object().is_none_or(|object| {
                object
                    .keys()
                    .any(|k| !matches!(k.as_str(), "schema" | "leaves" | "rows" | "policy"))
            }) || descriptor["rows"].as_u64().is_none()
            {
                return Err("Invalid sharded object index control".into());
            }
            let leaves = descriptor["leaves"]
                .as_object()
                .ok_or("Missing object index leaves")?;
            if leaves.is_empty() || leaves.len() > 16 {
                return Err("Object index leaf count exceeds budget".into());
            }
            let mut assets = Vec::with_capacity(leaves.len());
            for (prefix, asset) in leaves {
                if prefix.len() != 1
                    || !prefix
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err("Invalid object index leaf prefix".into());
                }
                assets.push((Some(prefix.as_bytes()[0]), asset.clone()));
            }
            assets
        }
        _ => return Err("Invalid cache-backed object index".into()),
    };
    let mut seen = BTreeSet::new();
    for (_, asset) in &assets {
        let oid = asset["sha256"]
            .as_str()
            .ok_or("Missing object index hash")?;
        if !valid_oid(oid)
            || asset["bytes"]
                .as_u64()
                .is_none_or(|size| size > MAX_LEAF_BYTES)
            || asset["cache_uri"] != format!("cache://sha256/{oid}")
            || !seen.insert(oid)
        {
            return Err("Invalid cache-backed object index leaf".into());
        }
    }
    Ok(assets)
}

pub(crate) fn metadata_roots(root: &Path) -> Result<BTreeSet<String>> {
    Ok(assets(root)?
        .into_iter()
        .map(|(_, asset)| asset["sha256"].as_str().unwrap().to_owned())
        .collect())
}
