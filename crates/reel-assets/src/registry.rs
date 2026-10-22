//! Compact per-hash authorization index. Payload metadata stays in LFS manifests.
use crate::{Result, admission, generic_objects, load, selected, unique, valid_oid, versions};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

pub fn pointer(oid: &str, size: u64) -> Result<String> {
    if !valid_oid(oid) {
        return Err("Invalid SHA-256".into());
    }
    Ok(format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {size}\n"
    ))
}

pub fn register(root: &Path, manifest: &Path) -> Result<Value> {
    let doc = load(manifest)?;
    let assets = doc["assets"]
        .as_array()
        .ok_or("Manifest requires assets array")?;
    let _lock = admission::worktree_lock(root)?;
    let mut index = selected(root)?;
    let mut generic = generic_objects(root)?;
    let mut touched: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    // Validate the whole batch before writing any pointers or authorization rows.
    for asset in assets {
        let oid = asset["sha256"].as_str().ok_or("Missing asset SHA-256")?;
        let size = asset["bytes"].as_u64().ok_or("Missing asset byte count")?;
        if !valid_oid(oid) {
            return Err("Invalid asset SHA-256".into());
        }
        for field in ["provenance", "rights", "selection_state"] {
            if asset[field].as_str().is_none_or(|s| s.trim().is_empty()) {
                return Err(format!("Missing asset {field}").into());
            }
        }
        if let Some(old) = index.insert(oid.to_owned(), size)
            && old != size
        {
            return Err("Conflicting object size".into());
        }
        touched
            .entry(oid[..2].to_owned())
            .or_default()
            .insert(oid.to_owned(), size);
        generic.insert(oid.to_owned(), size);
        let target = root.join("assets/objects/sha256").join(&oid[..2]).join(oid);
        if target.exists() && fs::read_to_string(&target)? != pointer(oid, size)? {
            return Err("Existing asset pointer mismatch".into());
        }
    }
    for prefix in touched.keys() {
        let all: BTreeMap<_, _> = generic
            .iter()
            .filter(|(h, _)| h.starts_with(prefix.as_str()))
            .map(|(h, s)| (h.clone(), *s))
            .collect();
        if all.len() * 88 > 256 * 1024 {
            return Err("Object shard would exceed metadata budget".into());
        }
    }
    for asset in assets {
        let oid = asset["sha256"].as_str().unwrap();
        let size = asset["bytes"].as_u64().unwrap();
        let target = root.join("assets/objects/sha256").join(&oid[..2]).join(oid);
        fs::create_dir_all(target.parent().unwrap())?;
        if !target.exists() {
            fs::write(target, pointer(oid, size)?)?;
        }
    }
    let directory = root.join("registry/objects");
    fs::create_dir_all(&directory)?;
    for prefix in touched.keys() {
        let target = directory.join(format!("{prefix}.tsv"));
        let mut rows: BTreeMap<String, u64> = if root.join("registry/object-index.json").exists() {
            // A compact baseline already owns its rows. Keep only this shard's
            // actual overlay instead of re-copying the complete baseline into Git.
            let mut delta = BTreeMap::new();
            if target.exists() {
                for line in fs::read_to_string(&target)?.lines() {
                    let (h, n) = line.split_once('\t').ok_or("Invalid object overlay row")?;
                    delta.insert(h.to_owned(), n.parse()?);
                }
            }
            delta
        } else {
            generic
                .iter()
                .filter(|(h, _)| h.starts_with(prefix.as_str()))
                .map(|(h, n)| (h.clone(), *n))
                .collect()
        };
        rows.extend(touched[prefix].iter().map(|(h, n)| (h.clone(), *n)));
        let text: String = rows.iter().map(|(h, n)| format!("{h}\t{n}\n")).collect();
        let temp = unique(&directory, "shard");
        fs::write(&temp, text)?;
        versions::replace(&temp, &target)?;
    }
    Ok(
        json!({"registered_assets":assets.len(),"touched_shards":touched.len(),"payloads_copied":0,"creative_authority":"not-granted-by-registration"}),
    )
}

/// Explicitly freeze the authorization rows in CAS, preserving local originals.
/// Ordinary TSVs remain supported as small deltas after compaction.
pub fn compact(root: &Path) -> Result<Value> {
    let before = generic_objects(root)?;
    let has_delta = root.join("registry/objects.tsv").exists()
        || (root.join("registry/objects").is_dir()
            && fs::read_dir(root.join("registry/objects"))?
                .any(|e| e.is_ok_and(|e| e.path().extension().is_some_and(|s| s == "tsv"))));
    if !has_delta && root.join("registry/object-index.json").exists() {
        return Ok(
            json!({"unchanged":true,"authorized_objects":before.len(),"media_deleted":false}),
        );
    }
    let ordered: BTreeMap<_, _> = before.iter().collect();
    let bytes: String = ordered.iter().map(|(h, s)| format!("{h}\t{s}\n")).collect();
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Object index snapshot exceeds budget".into());
    }
    let local = root.join(".local/object-index-staging");
    fs::create_dir_all(&local)?;
    let staging = unique(&local, "index.tsv");
    fs::write(&staging, &bytes)?;
    // Admission has its own lock. Freeze first, then compare under the writer lock.
    let asset = crate::ingest(root, &staging)?;
    let _lock = admission::worktree_lock(root)?;
    if generic_objects(root)? != before {
        return Err("Object registry changed during compaction; no index replaced".into());
    }
    let oid = asset["sha256"]
        .as_str()
        .ok_or("Missing ingested index hash")?;
    let size = asset["bytes"]
        .as_u64()
        .ok_or("Missing ingested index size")?;
    let descriptor = json!({"schema":"caminos.object-index.v1","asset":{
        "sha256":oid,"bytes":size,"cache_uri":format!("cache://sha256/{oid}")},
        "rows":before.len(),"policy":"Explicit snapshot; active E/D only; original rows retained locally and previous snapshots remain authorized"});
    let pointer_path = root.join("assets/objects/sha256").join(&oid[..2]).join(oid);
    fs::create_dir_all(pointer_path.parent().ok_or("Missing pointer parent")?)?;
    let pointer_bytes = pointer(oid, size)?;
    if pointer_path.exists() && fs::read_to_string(&pointer_path)? != pointer_bytes {
        return Err("Existing index LFS pointer differs".into());
    }
    fs::write(&pointer_path, pointer_bytes)?;
    let directory = root.join("registry");
    fs::create_dir_all(&directory)?;
    let temporary = unique(&directory, "object-index");
    fs::write(&temporary, serde_json::to_vec(&descriptor)?)?;
    versions::replace(&temporary, &directory.join("object-index.json"))?;
    // Publishing the verified complete snapshot is the commit point. An
    // interruption before/during retirement merely leaves duplicate equal rows.
    let originals = root.join(".local/object-index-originals").join(oid);
    fs::create_dir_all(&originals)?;
    let mut retired = Vec::new();
    let legacy = directory.join("objects.tsv");
    if legacy.exists() {
        retired.push(legacy);
    }
    let shards = directory.join("objects");
    if shards.is_dir() {
        for entry in fs::read_dir(&shards)? {
            let path = entry?.path();
            if path.extension().is_some_and(|s| s == "tsv") {
                retired.push(path);
            }
        }
    }
    for path in &retired {
        let name = path.file_name().ok_or("Missing registry filename")?;
        fs::copy(path, originals.join(name))?;
        fs::remove_file(path)?;
    }
    let after = generic_objects(root)?;
    if before.iter().any(|(h, s)| after.get(h) != Some(s)) {
        return Err("Compacted index lost authorization".into());
    }
    Ok(
        json!({"snapshot":descriptor,"authorized_objects":after.len(),
        "retired_control_files":retired.len(),"original_rows_preserved":true,
        "media_deleted":false,"archive_probed":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn compact_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let r = t.path().canonicalize().unwrap();
        for p in [".local", "E", "D", "M"] {
            fs::create_dir(r.join(p)).unwrap();
        }
        fs::write(
            r.join(".local/storage-rust.json"),
            json!({
                "cache_roots":[r.join("E"),r.join("D")],"archive_roots":[r.join("M")],
                "capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}
            })
            .to_string(),
        )
        .unwrap();
        fs::write(r.join("catalog.json"), r#"{"videos":[]}"#).unwrap();
        (t, r)
    }
    #[test]
    fn compact_preserves_rows_deltas_snapshots_and_idempotence() {
        let (_t, r) = compact_fixture();
        let manifest = r.join("manifest.json");
        let first = "a".repeat(64);
        fs::write(
            &manifest,
            json!({"assets":[{"sha256":first,"bytes":5,
            "provenance":"fixture","rights":"fixture","selection_state":"retained"}]})
            .to_string(),
        )
        .unwrap();
        register(&r, &manifest).unwrap();
        let result = compact(&r).unwrap();
        let snapshot = result["snapshot"]["asset"]["sha256"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(generic_objects(&r).unwrap()[&first], 5);
        assert!(
            r.join(".local/object-index-originals")
                .join(&snapshot)
                .join("aa.tsv")
                .is_file()
        );
        assert!(!r.join("registry/objects/aa.tsv").exists());
        assert_eq!(compact(&r).unwrap()["unchanged"], true);
        let second = "b".repeat(64);
        fs::write(
            &manifest,
            json!({"assets":[{"sha256":second,"bytes":7,
            "provenance":"fixture","rights":"fixture","selection_state":"retained"}]})
            .to_string(),
        )
        .unwrap();
        register(&r, &manifest).unwrap();
        assert_eq!(generic_objects(&r).unwrap()[&second], 7);
        compact(&r).unwrap();
        let objects = generic_objects(&r).unwrap();
        assert_eq!(objects[&first], 5);
        assert_eq!(objects[&second], 7);
        assert!(objects.contains_key(&snapshot));
        // An interrupted retirement leaves identical rows, never lost authorization.
        fs::write(r.join("registry/objects/aa.tsv"), format!("{first}\t5\n")).unwrap();
        assert_eq!(generic_objects(&r).unwrap()[&first], 5);
        fs::write(r.join("registry/objects/aa.tsv"), format!("{first}\t6\n")).unwrap();
        assert!(generic_objects(&r).is_err());
    }
    #[test]
    fn compact_index_uses_active_fallback_but_rejects_corruption_and_archive_only() {
        let (_t, r) = compact_fixture();
        fs::create_dir_all(r.join("registry/objects")).unwrap();
        fs::write(
            r.join("registry/objects/aa.tsv"),
            format!("{}\t5\n", "a".repeat(64)),
        )
        .unwrap();
        let result = compact(&r).unwrap();
        let oid = result["snapshot"]["asset"]["sha256"].as_str().unwrap();
        let e = crate::object(&r.join("E"), oid);
        let d = crate::object(&r.join("D"), oid);
        fs::create_dir_all(d.parent().unwrap()).unwrap();
        fs::copy(&e, &d).unwrap();
        fs::remove_file(&e).unwrap();
        assert!(generic_objects(&r).is_ok());
        fs::write(&e, b"corrupt").unwrap();
        assert!(generic_objects(&r).is_err());
        fs::remove_file(&e).unwrap();
        let m = crate::object(&r.join("M"), oid);
        fs::create_dir_all(m.parent().unwrap()).unwrap();
        fs::copy(&d, &m).unwrap();
        fs::remove_file(&d).unwrap();
        assert!(generic_objects(&r).is_err());
        let mut descriptor = load(&r.join("registry/object-index.json")).unwrap();
        descriptor["asset"]["bytes"] = json!(4 * 1024 * 1024 + 1);
        fs::write(r.join("registry/object-index.json"), descriptor.to_string()).unwrap();
        assert!(generic_objects(&r).is_err());
    }
    #[test]
    fn registration_after_compaction_writes_only_new_rows_in_the_same_prefix() {
        let (_t, r) = compact_fixture();
        let manifest = r.join("manifest.json");
        let first = "a".repeat(64);
        let second = format!("aa{}", "b".repeat(62));
        let third = format!("aa{}", "c".repeat(62));
        for (n, oid) in [&first, &second, &third].into_iter().enumerate() {
            fs::write(
                &manifest,
                json!({"assets":[{"sha256":oid,"bytes":5,
                "provenance":"fixture","rights":"fixture","selection_state":"retained"}]})
                .to_string(),
            )
            .unwrap();
            register(&r, &manifest).unwrap();
            if n == 0 {
                compact(&r).unwrap();
            }
        }
        let shard = fs::read_to_string(r.join("registry/objects/aa.tsv")).unwrap();
        assert!(!shard.contains(&first));
        assert_eq!(shard.lines().count(), 2);
        let index = generic_objects(&r).unwrap();
        for oid in [first, second, third] {
            assert_eq!(index[&oid], 5);
        }
        compact(&r).unwrap();
        let compacted = generic_objects(&r).unwrap();
        for (oid, bytes) in index {
            assert_eq!(compacted[&oid], bytes);
        }
    }
    #[test]
    fn generic_payload_needs_provenance_and_cannot_change_size() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path().canonicalize().unwrap();
        fs::create_dir(r.join(".local")).unwrap();
        fs::create_dir(r.join("E")).unwrap();
        fs::write(
            r.join(".local/storage-rust.json"),
            json!({"cache_roots":[r.join("E")]}).to_string(),
        )
        .unwrap();
        fs::write(r.join("catalog.json"), r#"{"videos":[]}"#).unwrap();
        // A legacy video hash sharing a shard must not become a generic import.
        let legacy = format!("aa{}", "b".repeat(62));
        fs::create_dir(r.join("video")).unwrap();
        fs::write(
            r.join("catalog.json"),
            r#"{"videos":[{"record":"video/video.json"}]}"#,
        )
        .unwrap();
        fs::write(
            r.join("video/video.json"),
            json!({"assets":{"movie":{"sha256":legacy,"bytes":9}}}).to_string(),
        )
        .unwrap();
        let manifest = r.join("manifest.json");
        let oid = "a".repeat(64);
        let mut doc = json!({"assets":[{"sha256":oid,"bytes":5,"provenance":"synthetic fixture","rights":"private fixture","selection_state":"retained-candidate"}]});
        fs::write(&manifest, doc.to_string()).unwrap();
        register(&r, &manifest).unwrap();
        assert_eq!(selected(&r).unwrap()[&oid], 5);
        assert!(r.join("assets/objects/sha256/aa").join(&oid).is_file());
        assert!(!generic_objects(&r).unwrap().contains_key(&legacy));
        assert!(selected(&r).unwrap().contains_key(&legacy));
        doc["assets"][0]["bytes"] = json!(6);
        fs::write(&manifest, doc.to_string()).unwrap();
        assert!(register(&r, &manifest).is_err());
        doc["assets"][0]["provenance"] = json!("");
        fs::write(&manifest, doc.to_string()).unwrap();
        assert!(register(&r, &manifest).is_err());
    }
}
