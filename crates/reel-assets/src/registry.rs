//! Compact per-hash authorization index. Payload metadata stays in LFS manifests.
use crate::{Result, admission, generic_objects, load, selected, unique, valid_oid, versions};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

pub fn pointer(oid: &str, size: u64) -> Result<String> {
    if !valid_oid(oid) {
        return Err("Invalid SHA-256".into());
    }
    Ok(format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {size}\n"
    ))
}

/// Recognize the canonical, bounded LFS pointer used by local control assets.
/// Ordinary JSON is left to its native parser; malformed pointers fail closed.
pub fn parse_pointer(bytes: &[u8]) -> Result<Option<(String, u64)>> {
    if !bytes.starts_with(b"version https://git-lfs.github.com/spec/v1") {
        return Ok(None);
    }
    if bytes.len() > 1024 {
        return Err("LFS control pointer exceeds budget".into());
    }
    let text = std::str::from_utf8(bytes)?;
    let mut lines = text.lines();
    if lines.next() != Some("version https://git-lfs.github.com/spec/v1") {
        return Err("Invalid LFS control pointer version".into());
    }
    let oid = lines
        .next()
        .and_then(|s| s.strip_prefix("oid sha256:"))
        .ok_or("Missing control pointer hash")?;
    let size: u64 = lines
        .next()
        .and_then(|s| s.strip_prefix("size "))
        .ok_or("Missing control pointer size")?
        .parse()?;
    if pointer(oid, size)?.as_bytes() != bytes || lines.next().is_some() {
        return Err("Noncanonical LFS control pointer".into());
    }
    Ok(Some((oid.to_owned(), size)))
}

pub fn register(root: &Path, manifest: &Path) -> Result<Value> {
    register_with_publication_wait(root, manifest, false)
}

pub(crate) fn register_job_outputs(root: &Path, manifest: &Path) -> Result<Value> {
    register_with_publication_wait(root, manifest, true)
}

fn register_with_publication_wait(
    root: &Path,
    manifest: &Path,
    completed_job_outputs: bool,
) -> Result<Value> {
    let doc = load(manifest)?;
    let assets = doc["assets"]
        .as_array()
        .ok_or("Manifest requires assets array")?;
    let _lock = if completed_job_outputs {
        admission::job_publication_lock(root)?
    } else {
        admission::worktree_lock(root)?
    };
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
    let parts = if bytes.len() as u64 <= crate::object_index::MAX_LEAF_BYTES {
        vec![(None, bytes)]
    } else {
        let mut leaves: BTreeMap<String, String> = BTreeMap::new();
        for (hash, size) in &ordered {
            use std::fmt::Write;
            writeln!(
                leaves.entry(hash[..1].to_owned()).or_default(),
                "{hash}\t{size}"
            )?;
        }
        if leaves
            .values()
            .any(|leaf| leaf.len() as u64 > crate::object_index::MAX_LEAF_BYTES)
        {
            return Err("Object index prefix leaf exceeds budget".into());
        }
        leaves
            .into_iter()
            .map(|(prefix, bytes)| (Some(prefix), bytes))
            .collect()
    };
    let local = root.join(".local/object-index-staging");
    fs::create_dir_all(&local)?;
    // Admission has its own lock. Freeze first, then compare under the writer lock.
    let mut leaves = serde_json::Map::new();
    let mut legacy = None;
    for (prefix, bytes) in parts {
        let staging = unique(&local, "index.tsv");
        fs::write(&staging, bytes)?;
        let asset = crate::ingest(root, &staging)?;
        let asset =
            json!({"sha256":asset["sha256"],"bytes":asset["bytes"],"cache_uri":asset["cache_uri"]});
        if let Some(prefix) = prefix {
            leaves.insert(prefix, asset);
        } else {
            legacy = Some(asset);
        }
    }
    let descriptor = if let Some(asset) = legacy {
        json!({"schema":"caminos.object-index.v1","asset":asset,
            "rows":before.len(),"policy":"Explicit snapshot; active E/D only; original rows retained locally and previous snapshots remain authorized"})
    } else {
        json!({"schema":"caminos.object-index.v2","leaves":leaves,"rows":before.len(),
            "policy":"Explicit bounded prefix leaves; configured active caches only; original rows and previous snapshots retained"})
    };
    let encoded = serde_json::to_vec(&descriptor)?;
    if encoded.len() as u64 > crate::object_index::MAX_CONTROL_BYTES {
        return Err("Object index control exceeds budget".into());
    }
    let assets = crate::object_index::descriptor_assets(&descriptor)?;
    let _lock = admission::worktree_lock(root)?;
    if generic_objects(root)? != before {
        return Err("Object registry changed during compaction; no index replaced".into());
    }
    let mut pointers = Vec::new();
    for (_, asset) in assets {
        let oid = asset["sha256"].as_str().unwrap();
        let size = asset["bytes"].as_u64().unwrap();
        let path = root.join("assets/objects/sha256").join(&oid[..2]).join(oid);
        let bytes = pointer(oid, size)?;
        if path.exists() && fs::read_to_string(&path)? != bytes {
            return Err("Existing index LFS pointer differs".into());
        }
        pointers.push((path, bytes));
    }
    for (path, bytes) in pointers {
        fs::create_dir_all(path.parent().ok_or("Missing pointer parent")?)?;
        fs::write(path, bytes)?;
    }
    let directory = root.join("registry");
    fs::create_dir_all(&directory)?;
    let temporary = unique(&directory, "object-index");
    fs::write(&temporary, &encoded)?;
    versions::replace(&temporary, &directory.join("object-index.json"))?;
    // Publishing the verified complete snapshot is the commit point. An
    // interruption before/during retirement merely leaves duplicate equal rows.
    let snapshot_key = descriptor["asset"]["sha256"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{:x}", Sha256::digest(&encoded)));
    let originals = root
        .join(".local/object-index-originals")
        .join(snapshot_key);
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
    fn sharded_snapshot_preserves_large_union_versions_deltas_and_history() {
        let (_t, r) = compact_fixture();
        fs::create_dir_all(r.join("registry")).unwrap();
        let first = "a".repeat(64);
        fs::write(r.join("registry/objects.tsv"), format!("{first}\t5\n")).unwrap();
        let old = compact(&r).unwrap()["snapshot"].clone();
        assert_eq!(old["schema"], "caminos.object-index.v1");
        versions::transact(&r, |state| {
            state.register(serde_json::from_value(
                json!({"logical_id":"fixture","version_id":"v1",
                "manifest_sha256":first,"payloads":[first]}),
            )?)?;
            state.select("fixture", "v1")?;
            state.pins.insert("fixture@v1".into());
            state.lease("fixture-owner", ["fixture@v1".into()].into())?;
            Ok(json!({}))
        })
        .unwrap();
        let versions_before = serde_json::to_value(versions::read(&r).unwrap()).unwrap();
        let rows: String = (0..62_000)
            .map(|n| {
                let hash = format!("{:x}", Sha256::digest(format!("synthetic-{n}")));
                format!("{hash}\t{}\n", n + 1)
            })
            .collect();
        assert!(rows.len() as u64 > crate::object_index::MAX_LEAF_BYTES);
        fs::write(r.join("registry/objects.tsv"), rows).unwrap();
        let before = generic_objects(&r).unwrap();
        let large = compact(&r).unwrap()["snapshot"].clone();
        assert_eq!(large["schema"], "caminos.object-index.v2");
        assert_eq!(large["leaves"].as_object().unwrap().len(), 16);
        assert!(
            fs::metadata(r.join("registry/object-index.json"))
                .unwrap()
                .len()
                <= 4096
        );
        let after = generic_objects(&r).unwrap();
        for (hash, size) in &before {
            assert_eq!(after.get(hash), Some(size));
        }
        let mut state = versions::read(&r).unwrap();
        assert_eq!(serde_json::to_value(&state).unwrap(), versions_before);
        for asset in large["leaves"].as_object().unwrap().values() {
            let hash = asset["sha256"].as_str().unwrap();
            assert!(asset["bytes"].as_u64().unwrap() <= crate::object_index::MAX_LEAF_BYTES);
            assert!(state.metadata_roots.contains(hash));
            assert!(after.contains_key(hash));
        }
        // A retired consumer cannot make a live index leaf unprotected.
        let leaf = large["leaves"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()["sha256"]
            .as_str()
            .unwrap();
        state
            .register(
                serde_json::from_value(json!({"logical_id":"retired","version_id":"v1",
            "manifest_sha256":first,"payloads":[first,leaf],"deprecated":true}))
                .unwrap(),
            )
            .unwrap();
        let plan = state.retirement_plan("retired", "v1").unwrap();
        assert!(
            plan["shared_protected_hashes"]
                .as_array()
                .unwrap()
                .contains(&json!(leaf))
        );
        assert_eq!(compact(&r).unwrap()["unchanged"], true);
        let extra = format!("{:x}", Sha256::digest(b"new-after-large-snapshot"));
        let manifest = r.join("manifest.json");
        fs::write(
            &manifest,
            json!({"assets":[{"sha256":extra,"bytes":7,
            "provenance":"synthetic fixture","rights":"fixture","selection_state":"retained"}]})
            .to_string(),
        )
        .unwrap();
        register(&r, &manifest).unwrap();
        assert_eq!(generic_objects(&r).unwrap()[&extra], 7);
        compact(&r).unwrap();
        let newest = fs::read(r.join("registry/object-index.json")).unwrap();
        let current = generic_objects(&r).unwrap();
        for (hash, size) in after {
            assert_eq!(current.get(&hash), Some(&size));
        }
        assert_eq!(current[&extra], 7);
        // Historical native headers still restore their exact former views.
        fs::write(r.join("registry/object-index.json"), large.to_string()).unwrap();
        let restored = generic_objects(&r).unwrap();
        for (hash, size) in &before {
            assert_eq!(restored.get(hash), Some(size));
        }
        assert!(!restored.contains_key(&extra));
        fs::write(r.join("registry/object-index.json"), old.to_string()).unwrap();
        assert_eq!(generic_objects(&r).unwrap()[&first], 5);
        fs::write(r.join("registry/object-index.json"), newest).unwrap();
    }

    #[test]
    fn sharded_reader_honors_profiles_and_rejects_corruption_missing_and_malformed_leaves() {
        let (_t, r) = compact_fixture();
        let mut leaves = serde_json::Map::new();
        for prefix in ["a", "b"] {
            let f = r.join("leaf.tsv");
            fs::write(&f, format!("{}\t5\n", prefix.repeat(64))).unwrap();
            let asset = crate::ingest(&r, &f).unwrap();
            leaves.insert(prefix.into(), json!({"sha256":asset["sha256"],"bytes":asset["bytes"],"cache_uri":asset["cache_uri"]}));
        }
        fs::create_dir_all(r.join("registry")).unwrap();
        let descriptor = json!({"schema":"caminos.object-index.v2","leaves":leaves,"rows":2});
        let path = r.join("registry/object-index.json");
        fs::write(&path, descriptor.to_string()).unwrap();
        assert_eq!(generic_objects(&r).unwrap()[&"a".repeat(64)], 5);
        for asset in descriptor["leaves"].as_object().unwrap().values() {
            let h = asset["sha256"].as_str().unwrap();
            let e = crate::object(&r.join("E"), h);
            for tier in ["D", "M"] {
                let copy = crate::object(&r.join(tier), h);
                fs::create_dir_all(copy.parent().unwrap()).unwrap();
                fs::copy(&e, copy).unwrap();
            }
            fs::remove_file(e).unwrap();
        }
        let cfg = crate::config(&r).unwrap();
        let profile = crate::Config {
            cache_roots: vec![r.join("D")],
            ..cfg.clone()
        };
        fs::write(
            r.join(".local/storage-rust.json"),
            json!({"cache_roots":[r.join("E")]}).to_string(),
        )
        .unwrap();
        assert!(generic_objects(&r).is_err());
        assert!(crate::generic_objects_with(&r, Some(&profile)).is_ok());
        fs::write(
            r.join(".local/storage-rust.json"),
            json!({"cache_roots":[r.join("E"),r.join("D")],"archive_roots":[r.join("M")]})
                .to_string(),
        )
        .unwrap();
        let hash = descriptor["leaves"]["a"]["sha256"].as_str().unwrap();
        let e = crate::object(&r.join("E"), hash);
        fs::write(&e, b"corrupt").unwrap();
        assert!(generic_objects(&r).is_err());
        fs::remove_file(e).unwrap();
        let d = crate::object(&r.join("D"), hash);
        fs::remove_file(&d).unwrap();
        assert!(generic_objects(&r).is_err()); // Archive-only is never implicit.
        fs::copy(crate::object(&r.join("M"), hash), d).unwrap();
        let mut invalid = descriptor.clone();
        invalid["leaves"]["a"]["bytes"] = json!(crate::object_index::MAX_LEAF_BYTES + 1);
        fs::write(&path, invalid.to_string()).unwrap();
        assert!(generic_objects(&r).is_err());
        invalid = descriptor.clone();
        invalid["leaves"]["a"] = descriptor["leaves"]["b"].clone();
        invalid["leaves"].as_object_mut().unwrap().remove("b");
        fs::write(&path, invalid.to_string()).unwrap();
        assert!(
            generic_objects(&r)
                .unwrap_err()
                .to_string()
                .contains("leaf prefix")
        );
        invalid = descriptor.clone();
        invalid["leaves"]["g"] = descriptor["leaves"]["a"].clone();
        fs::write(&path, invalid.to_string()).unwrap();
        assert!(generic_objects(&r).is_err());
        invalid = descriptor.clone();
        invalid["leaves"]["b"] = descriptor["leaves"]["a"].clone();
        fs::write(&path, invalid.to_string()).unwrap();
        assert!(generic_objects(&r).is_err());
        fs::write(&path, descriptor.to_string()).unwrap();
        assert!(generic_objects(&r).is_ok());
    }

    #[test]
    fn oversized_prefix_leaf_fails_before_snapshot_publication() {
        let (_t, r) = compact_fixture();
        fs::create_dir_all(r.join("registry")).unwrap();
        let rows: String = (0..64_000).map(|n| format!("a{n:063x}\t1\n")).collect();
        assert!(rows.len() as u64 > crate::object_index::MAX_LEAF_BYTES);
        fs::write(r.join("registry/objects.tsv"), &rows).unwrap();
        assert!(
            compact(&r)
                .unwrap_err()
                .to_string()
                .contains("prefix leaf exceeds budget")
        );
        assert!(!r.join("registry/object-index.json").exists());
        assert_eq!(
            fs::read_to_string(r.join("registry/objects.tsv")).unwrap(),
            rows
        );
        assert!(!r.join(".local/object-index-staging").exists());
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
