use reel_assets::{
    Result,
    inventory_evidence::{
        sidecar::{self, BuildLimits, ReadLimits, Store},
        *,
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn blob(bytes: &[u8]) -> Blob {
    Blob {
        sha256: hash(bytes),
        bytes: bytes.len() as u64,
    }
}
fn source(raw: &[u8]) -> Source {
    Source {
        encoded: blob(raw),
        decoded: blob(raw),
        encoding: Encoding::Identity,
        schema: SOURCE_SCHEMA.into(),
        project_id: "synthetic".into(),
        authority_namespace: "fixture".into(),
        adapter_version: ADAPTER_VERSION,
    }
}
fn fixture(count: usize) -> (PreparedInventory, Vec<String>) {
    let ids: Vec<_> = (0..count)
        .map(|i| hash(format!("object-{i}").as_bytes()))
        .collect();
    let rows:Vec<_>=ids.iter().enumerate().map(|(i,id)|json!({"sha256":id,"bytes":i,"registered":true,
        "copies":[{"tier":0,"bytes":i,"physical_id":format!("synthetic-{i}"),"physical_identity_measured":true,
            "descriptor_size_matches":true,"hard_links":2,"allocated_bytes":4096}]})).collect();
    let raw=serde_json::to_vec(&json!({"schema":SOURCE_SCHEMA,"project_id":"synthetic",
        "profile_sha256":hash(b"profile"),"registry_generation":8,"scope":"synthetic census",
        "coverage_complete":false,"census_started_unix_seconds":10,"census_completed_unix_seconds":20,
        "tiers":[{"tier":0,"root":"SYNTHETIC_PRIVATE_PATH","volume_id":"fixture-volume"}],"objects":rows})).unwrap();
    (
        prepare(raw.as_slice(), &source(&raw), Limits::default()).unwrap(),
        ids,
    )
}

/// Test backend only. Exact caller-provided directory, immutable no-clobber
/// publication; no configured project caches, scanning or recovery fallback.
struct Files {
    root: PathBuf,
    reads: usize,
    writes: usize,
    fail_after: Option<usize>,
    lie: bool,
    corrupt_after_publish: bool,
}
impl Files {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            reads: 0,
            writes: 0,
            fail_after: None,
            lie: false,
            corrupt_after_publish: false,
        }
    }
    fn path(&self, b: &Blob) -> PathBuf {
        self.root.join(&b.sha256)
    }
    fn value(&self, b: &Blob) -> Value {
        serde_json::from_slice(&fs::read(self.path(b)).unwrap()).unwrap()
    }
    fn put_value(&mut self, v: &Value) -> Blob {
        self.publish(&serde_json::to_vec(v).unwrap()).unwrap()
    }
}
impl Store for Files {
    fn publish(&mut self, bytes: &[u8]) -> Result<Blob> {
        if self.fail_after == Some(self.writes) {
            return Err("synthetic disk full/interruption".into());
        }
        let b = blob(bytes);
        let mut stage = tempfile::NamedTempFile::new_in(&self.root)?;
        stage.write_all(bytes)?;
        stage.as_file().sync_all()?;
        if let Err(e) = stage.persist_noclobber(self.path(&b)) {
            if e.error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(e.error.into());
            }
            if fs::read(self.path(&b))? != bytes {
                return Err("corrupt existing winner".into());
            }
        }
        self.writes += 1;
        if self.corrupt_after_publish {
            fs::write(self.path(&b), b"corrupt")?;
        }
        if self.lie {
            return Ok(blob(b"wrong identity"));
        }
        Ok(b)
    }
    fn open(&mut self, b: &Blob) -> Result<Box<dyn Read + '_>> {
        self.reads += 1;
        Ok(Box::new(fs::File::open(self.path(b))?))
    }
}
fn setup(
    count: usize,
) -> (
    tempfile::TempDir,
    Files,
    PreparedInventory,
    Vec<String>,
    Blob,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Files::new(dir.path().into());
    let (index, ids) = fixture(count);
    let root = sidecar::publish(&mut store, &index, BuildLimits::default()).unwrap();
    (dir, store, index, ids, root)
}
fn query(store: &mut Files, root: &Blob, s: &Source, id: &str) -> Result<sidecar::IndexedLookup> {
    sidecar::lookup(store, root, s, id, ReadLimits::default())
}
fn tree(store: &Files, root: &Blob) -> Blob {
    let v = store.value(root);
    Blob {
        sha256: v["tree"]["sha256"].as_str().unwrap().into(),
        bytes: v["tree"]["bytes"].as_u64().unwrap(),
    }
}
fn rewrite_tree(store: &mut Files, root: &Blob, edit: impl FnOnce(&mut Value)) -> Blob {
    let mut header = store.value(root);
    let mut node = store.value(&tree(store, root));
    edit(&mut node);
    let b = store.put_value(&node);
    header["tree"] = json!({"sha256":b.sha256,"bytes":b.bytes});
    store.put_value(&header)
}

#[test]
fn reopened_persistent_lookup_reads_only_a_bounded_path_with_original_witness() {
    let (dir, _store, index, ids, root) = setup(1000);
    let expected = index.source().clone();
    drop(index);
    let mut reopened = Files::new(dir.path().into());
    let result = query(&mut reopened, &root, &expected, &ids[713]).unwrap();
    assert_eq!(result.nodes_read, 3);
    assert_eq!(reopened.reads, 3);
    assert!(result.rows_inspected <= 128);
    assert!(result.bytes_read < expected.decoded.bytes / 2);
    let row = result.observation.unwrap();
    assert_eq!(row.source_record, 713);
    assert_eq!(row.bytes, Some(713));
    assert_eq!(row.copies[0].physical_id.as_deref(), Some("synthetic-713"));
    assert_eq!(row.copies[0].hard_links, Some(2));
    assert_eq!(result.capture.unwrap().completed, 20);
    assert_eq!(result.registry_generation, 8);
    assert_eq!(result.scope.as_deref(), Some("synthetic census"));
    assert_eq!(result.producer_dependency_coverage, Some(false));
    assert_eq!(result.tiers[0].root, "SYNTHETIC_PRIVATE_PATH");
}
#[test]
fn duplicate_publication_has_same_root_and_preserves_existing_nodes() {
    let (_d, mut store, index, ids, root) = setup(1000);
    let second = sidecar::publish(&mut store, &index, BuildLimits::default()).unwrap();
    assert!(root == second);
    assert!(
        query(&mut store, &root, index.source(), &ids[9])
            .unwrap()
            .observation
            .is_some()
    );
}
#[test]
fn old_and_new_sources_coexist_without_refresh_overwriting_old_snapshot() {
    let (_d, mut store, old, ids, oldroot) = setup(50);
    let (new, _) = fixture(51);
    let newroot = sidecar::publish(&mut store, &new, BuildLimits::default()).unwrap();
    assert!(oldroot != newroot);
    assert!(
        query(&mut store, &oldroot, old.source(), &ids[0])
            .unwrap()
            .observation
            .is_some()
    );
    assert!(
        query(&mut store, &newroot, new.source(), &ids[0])
            .unwrap()
            .observation
            .is_some()
    );
    assert!(query(&mut store, &oldroot, new.source(), &ids[0]).is_err());
}
#[test]
fn source_identity_scope_and_interpretation_changes_reject_before_children() {
    let (_d, mut store, index, ids, root) = setup(200);
    for case in 0..9 {
        let mut s = index.source().clone();
        match case {
            0 => s.encoded.sha256 = hash(b"stale"),
            1 => s.decoded.bytes += 1,
            2 => s.encoding = Encoding::Gzip,
            3 => s.project_id = "other".into(),
            4 => s.authority_namespace = "other".into(),
            5 => s.schema = "other".into(),
            6 => s.adapter_version += 1,
            7 => s.decoded.sha256 = hash(b"stale"),
            _ => s.encoded.bytes += 1,
        }
        store.reads = 0;
        assert!(query(&mut store, &root, &s, &ids[0]).is_err());
        assert_eq!(store.reads, if matches!(case, 5 | 6) { 0 } else { 1 });
    }
}
#[test]
fn empty_and_absent_subjects_are_captured_nonmembership_only() {
    for n in [0, 1, 1000] {
        let (_d, mut store, index, _ids, root) = setup(n);
        assert!(
            query(&mut store, &root, index.source(), &hash(b"absent"))
                .unwrap()
                .observation
                .is_none()
        );
    }
}
#[test]
fn missing_referenced_node_is_error_not_nonmembership() {
    let (_d, mut store, index, ids, root) = setup(1);
    let b = tree(&store, &root);
    // Synthetic backend fault injection: never a production cache path.
    fs::remove_file(store.path(&b)).unwrap();
    assert!(query(&mut store, &root, index.source(), &ids[0]).is_err());
}
#[test]
fn corrupt_header_or_leaf_reject_without_repair() {
    for header in [true, false] {
        let (_d, mut store, index, ids, root) = setup(1);
        let b = if header {
            root.clone()
        } else {
            tree(&store, &root)
        };
        fs::write(store.path(&b), b"corrupt").unwrap();
        assert!(query(&mut store, &root, index.source(), &ids[0]).is_err());
        assert_eq!(fs::read(store.path(&b)).unwrap(), b"corrupt");
    }
}
#[test]
fn corrupt_existing_publication_winner_is_preserved_and_rejected() {
    let (_d, mut store, index, _ids, root) = setup(1);
    let b = tree(&store, &root);
    fs::write(store.path(&b), b"corrupt").unwrap();
    assert!(sidecar::publish(&mut store, &index, BuildLimits::default()).is_err());
    assert_eq!(fs::read(store.path(&b)).unwrap(), b"corrupt");
}
#[test]
fn semantic_corruption_with_valid_hash_rejects() {
    for case in 0..9 {
        let (_d, mut store, index, ids, root) = setup(2);
        let rewritten = rewrite_tree(&mut store, &root, |v| match case {
            0 => v["schema"] = json!("unsupported"),
            1 => v["prefix"] = json!("a"),
            2 => v["body"]["rows"][0]["record"] = json!(2),
            3 => v["body"]["rows"][0]["observation"]["sha256"] = json!("invalid"),
            4 => v["body"]["rows"][0]["observation"]["copies"][0]["tier"] = json!(9),
            5 => {
                v["body"]["rows"][0]["observation"]["copies"][0]["descriptor_size_matches"] =
                    json!(false)
            }
            6 => {
                v["body"]["rows"][1] = v["body"]["rows"][0].clone();
            }
            7 => v["body"]["rows"][0]["observation"]["unknown"] = json!(true),
            _ => {
                let a = v["body"]["rows"][0].clone();
                v["body"]["rows"][0] = v["body"]["rows"][1].clone();
                v["body"]["rows"][1] = a;
            }
        });
        assert!(
            query(&mut store, &rewritten, index.source(), &ids[0]).is_err(),
            "case {case}"
        );
    }
}
#[test]
fn malformed_branch_routing_and_descriptors_reject() {
    for case in 0..4 {
        let (_d, mut store, index, ids, root) = setup(1000);
        let rewritten = rewrite_tree(&mut store, &root, |v| {
            let children = v["body"]["children"].as_object_mut().unwrap();
            let key = children.keys().next().unwrap().clone();
            let mut child = children.remove(&key).unwrap();
            match case {
                0 => {
                    children.insert("zz".into(), child);
                }
                1 => {
                    children.insert("Z".into(), child);
                }
                2 => {
                    child["sha256"] = json!("bad");
                    children.insert(key, child);
                }
                _ => {
                    child["bytes"] = json!(u64::MAX);
                    children.insert(key, child);
                }
            }
        });
        assert!(query(&mut store, &rewritten, index.source(), &ids[0]).is_err());
    }
}
#[test]
fn source_header_unknown_schema_and_invalid_metadata_reject() {
    for case in 0..5 {
        let (_d, mut store, index, ids, root) = setup(1);
        let mut v = store.value(&root);
        match case {
            0 => v["schema"] = json!("unsupported"),
            1 => v["profile"] = json!("bad"),
            2 => v["capture"] = json!([20, 10]),
            3 => v["tiers"][0]["root"] = json!(" "),
            _ => v["extra"] = json!(true),
        }
        let b = store.put_value(&v);
        assert!(query(&mut store, &b, index.source(), &ids[0]).is_err());
    }
}
#[test]
fn query_budgets_reject_before_open_and_on_leaf_rows() {
    let (_d, mut store, index, ids, root) = setup(1000);
    for limits in [
        ReadLimits {
            nodes: 0,
            ..ReadLimits::default()
        },
        ReadLimits {
            total_bytes: root.bytes - 1,
            ..ReadLimits::default()
        },
        ReadLimits {
            node_bytes: root.bytes - 1,
            ..ReadLimits::default()
        },
    ] {
        store.reads = 0;
        assert!(sidecar::lookup(&mut store, &root, index.source(), &ids[0], limits).is_err());
        assert_eq!(store.reads, 0);
    }
    for limits in [
        ReadLimits {
            nodes: 1,
            ..ReadLimits::default()
        },
        ReadLimits {
            rows: 0,
            ..ReadLimits::default()
        },
    ] {
        assert!(sidecar::lookup(&mut store, &root, index.source(), &ids[0], limits).is_err());
    }
    assert!(query(&mut store, &root, index.source(), "अ").is_err());
}
#[test]
fn publication_failure_never_returns_a_root_and_header_is_last() {
    for fail in [0, 1, 2, 17] {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Files::new(dir.path().into());
        store.fail_after = Some(fail);
        let (index, _) = fixture(1000);
        assert!(sidecar::publish(&mut store, &index, BuildLimits::default()).is_err());
        for entry in fs::read_dir(dir.path()).unwrap() {
            let v: Value =
                serde_json::from_slice(&fs::read(entry.unwrap().path()).unwrap()).unwrap();
            assert_ne!(v["schema"], "reel.inventory-sidecar.header.v1");
        }
    }
}
#[test]
fn publication_verifies_returned_identity_and_readback() {
    for lie in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Files::new(dir.path().into());
        store.lie = lie;
        store.corrupt_after_publish = !lie;
        let (index, _) = fixture(1);
        assert!(sidecar::publish(&mut store, &index, BuildLimits::default()).is_err());
    }
}
#[test]
fn build_budgets_are_explicit_and_do_not_silently_expand() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Files::new(dir.path().into());
    let (index, _) = fixture(1);
    for limits in [
        BuildLimits {
            nodes: 0,
            ..BuildLimits::default()
        },
        BuildLimits {
            total_bytes: 0,
            ..BuildLimits::default()
        },
        BuildLimits {
            leaf_rows: 0,
            ..BuildLimits::default()
        },
        BuildLimits {
            leaf_rows: 4097,
            ..BuildLimits::default()
        },
        BuildLimits {
            node_bytes: 0,
            ..BuildLimits::default()
        },
        BuildLimits {
            node_bytes: 4 * 1024 * 1024 + 1,
            ..BuildLimits::default()
        },
        BuildLimits {
            node_bytes: 100,
            ..BuildLimits::default()
        },
    ] {
        assert!(sidecar::publish(&mut store, &index, limits).is_err());
    }
}
#[test]
fn deep_shared_prefixes_split_without_unbounded_recursion() {
    // Two valid distinct IDs sharing 63 hex digits stress the full path.
    let ids = [
        format!("{}0", "a".repeat(63)),
        format!("{}1", "a".repeat(63)),
    ];
    let rows: Vec<_> = ids
        .iter()
        .map(|id| json!({"sha256":id,"bytes":0,"registered":true,"copies":[]}))
        .collect();
    let raw = serde_json::to_vec(
        &json!({"schema":SOURCE_SCHEMA,"project_id":"synthetic","profile_sha256":hash(b"profile"),
        "registry_generation":0,"tiers":[],"objects":rows}),
    )
    .unwrap();
    let s = source(&raw);
    let index = prepare(raw.as_slice(), &s, Limits::default()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut store = Files::new(dir.path().into());
    let root = sidecar::publish(
        &mut store,
        &index,
        BuildLimits {
            leaf_rows: 1,
            ..BuildLimits::default()
        },
    )
    .unwrap();
    let result = query(&mut store, &root, &s, &ids[1]).unwrap();
    assert_eq!(result.nodes_read, 66);
    assert_eq!(result.observation.unwrap().source_record, 1);
}

#[test]
fn noncanonical_duplicate_and_omitted_fields_reject_even_with_valid_hashes() {
    let (_d, mut store, index, ids, root) = setup(1);
    let v = store.value(&root);
    let pretty = serde_json::to_vec_pretty(&v).unwrap();
    let b = store.publish(&pretty).unwrap();
    assert!(query(&mut store, &b, index.source(), &ids[0]).is_err());
    let raw = serde_json::to_string(&v).unwrap();
    let duplicate = raw.replacen("{", "{\"schema\":\"ignored\",", 1);
    let b = store.publish(duplicate.as_bytes()).unwrap();
    assert!(query(&mut store, &b, index.source(), &ids[0]).is_err());
    let mut missing = v;
    missing.as_object_mut().unwrap().remove("named_scope");
    let b = store.put_value(&missing);
    assert!(query(&mut store, &b, index.source(), &ids[0]).is_err());
}

#[test]
fn oversized_rows_and_header_metadata_fail_without_returning_a_root() {
    for field in ["row", "header"] {
        let raw=serde_json::to_vec(&json!({"schema":SOURCE_SCHEMA,"project_id":"synthetic",
            "registry_generation":0,"profile_sha256":hash(b"profile"),
            "scope":if field=="header" {"x".repeat(2048)}else{"small".into()},
            "tiers":[{"tier":0,"root":"synthetic"}],
            "objects":[{"sha256":hash(b"a"),"bytes":1,"registered":true,
                "copies":[{"tier":0,"bytes":1,"physical_id":if field=="row" {"x".repeat(2048)}else{"small".into()}}]}]})).unwrap();
        let index = prepare(raw.as_slice(), &source(&raw), Limits::default()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut store = Files::new(dir.path().into());
        assert!(
            sidecar::publish(
                &mut store,
                &index,
                BuildLimits {
                    node_bytes: 1024,
                    ..BuildLimits::default()
                }
            )
            .is_err()
        );
        assert_eq!(store.writes, 0);
    }
}

#[test]
fn actual_census_and_named_reports_roundtrip_through_persistent_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for path in [
        ".local", "registry", "hot", "history", "archive", "sidecars",
    ] {
        fs::create_dir_all(root.join(path)).unwrap();
    }
    fs::write(root.join("catalog.json"), r#"{"videos":[]}"#).unwrap();
    fs::write(
        root.join("registry/versions.json"),
        serde_json::to_vec(&reel_assets::versions::State::default()).unwrap(),
    )
    .unwrap();
    fs::write(root.join(".local/storage-rust.json"),json!({"path_policy":"configured-local-roots",
        "cache_roots":[root.join("hot"),root.join("history")],"archive_roots":[root.join("archive")],
        "capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}}).to_string()).unwrap();
    fs::write(root.join("asset-project.json"),json!({"schema":"reel.asset-project.v1","project_id":"synthetic",
        "storage_config":".local/storage-rust.json","selections":[],"scan_cas":true,"measure_unregistered_physical":true,
        "max_inventory_objects":100,"lfs_namespaces":[]}).to_string()).unwrap();
    let id = hash(b"registered");
    let unknown = hash(b"unregistered");
    fs::write(root.join("registry/objects.tsv"), format!("{id}\t10\n")).unwrap();
    for tier in ["hot", "history"] {
        for (subject, bytes) in [
            (&id, b"registered".as_slice()),
            (
                &unknown,
                if tier == "hot" {
                    b"unregistered".as_slice()
                } else {
                    b"short".as_slice()
                },
            ),
        ] {
            let path = root
                .join(tier)
                .join("objects/sha256")
                .join(&subject[..2])
                .join(subject);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
    }
    let before = fs::read(root.join("registry/versions.json")).unwrap();
    for report in [
        reel_assets::accounting::report(&root).unwrap(),
        reel_assets::accounting::observe_named(
            &root,
            &std::collections::BTreeSet::from([id.clone()]),
        )
        .unwrap(),
    ] {
        let raw = serde_json::to_vec(&report).unwrap();
        let s = source(&raw);
        let prepared = prepare(raw.as_slice(), &s, Limits::default()).unwrap();
        let mut store = Files::new(root.join("sidecars"));
        let indexroot = sidecar::publish(&mut store, &prepared, BuildLimits::default()).unwrap();
        drop(prepared);
        for subject in [&id, &unknown] {
            let result = query(&mut store, &indexroot, &s, subject).unwrap();
            if let Some(row) = result.observation {
                let original = &report["objects"][row.source_record];
                assert_eq!(original["sha256"], *subject);
                assert_eq!(original["bytes"].as_u64(), row.bytes);
                assert_eq!(original["registered"].as_bool(), Some(row.registered));
                assert_eq!(
                    row.copies.len(),
                    original["copies"].as_array().unwrap().len()
                );
                for (copy, original) in row
                    .copies
                    .iter()
                    .zip(original["copies"].as_array().unwrap())
                {
                    assert_eq!(
                        copy.physical_id.as_deref(),
                        original["physical_id"].as_str()
                    );
                    assert_eq!(copy.bytes, original["bytes"].as_u64().unwrap());
                    assert_eq!(
                        copy.descriptor_size_matches,
                        original["descriptor_size_matches"].as_bool()
                    );
                    assert_eq!(copy.hard_links, original["hard_links"].as_u64());
                }
                if subject == &unknown {
                    assert!(!row.registered);
                    assert_ne!(row.copies[0].bytes, row.copies[1].bytes);
                }
            } else {
                assert_eq!(subject, &unknown);
                assert!(report.get("scope").is_none());
            }
            assert_eq!(result.capture.is_some(), report.get("scope").is_some());
            assert_eq!(
                result.named_scope.as_deref(),
                report["inventory_scope"].as_str()
            );
        }
    }
    assert_eq!(
        fs::read(root.join("registry/versions.json")).unwrap(),
        before
    );
}

#[test]
fn stored_descriptor_limits_actual_reads_to_declared_bytes_plus_one() {
    struct Counting {
        count: std::rc::Rc<std::cell::Cell<usize>>,
        reader: std::io::Cursor<Vec<u8>>,
    }
    impl Read for Counting {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.reader.read(buf)?;
            self.count.set(self.count.get() + n);
            Ok(n)
        }
    }
    struct LongStore(std::rc::Rc<std::cell::Cell<usize>>);
    impl Store for LongStore {
        fn publish(&mut self, _: &[u8]) -> Result<Blob> {
            Err("read only".into())
        }
        fn open(&mut self, _: &Blob) -> Result<Box<dyn Read + '_>> {
            Ok(Box::new(Counting {
                count: self.0.clone(),
                reader: std::io::Cursor::new(vec![b'x'; 10000]),
            }))
        }
    }
    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut store = LongStore(count.clone());
    let (index, ids) = fixture(1);
    assert!(
        sidecar::lookup(
            &mut store,
            &blob(b"x"),
            index.source(),
            &ids[0],
            ReadLimits::default()
        )
        .is_err()
    );
    assert_eq!(count.get(), 2);
}

#[test]
fn byte_driven_splitting_works_below_the_row_limit() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Files::new(dir.path().into());
    let (index, ids) = fixture(100);
    let root = sidecar::publish(
        &mut store,
        &index,
        BuildLimits {
            node_bytes: 4096,
            ..BuildLimits::default()
        },
    )
    .unwrap();
    let result = sidecar::lookup(
        &mut store,
        &root,
        index.source(),
        &ids[0],
        ReadLimits {
            node_bytes: 4096,
            ..ReadLimits::default()
        },
    )
    .unwrap();
    assert!(result.nodes_read > 2);
    assert!(result.rows_inspected < 100);
}

#[test]
fn validly_hashed_row_in_the_wrong_leaf_rejects() {
    let (_d, mut store, index, ids, root) = setup(1000);
    let mut header = store.value(&root);
    let mut branch = store.value(&tree(&store, &root));
    let key = &ids[0][..1];
    let child = &branch["body"]["children"][key];
    let b = Blob {
        sha256: child["sha256"].as_str().unwrap().into(),
        bytes: child["bytes"].as_u64().unwrap(),
    };
    let mut leaf = store.value(&b);
    let wrong = format!("{}{}", if key == "0" { "1" } else { "0" }, "0".repeat(63));
    leaf["body"]["rows"][0]["observation"]["sha256"] = json!(wrong);
    let leafb = store.put_value(&leaf);
    branch["body"]["children"][key] = json!({"sha256":leafb.sha256,"bytes":leafb.bytes});
    let branchb = store.put_value(&branch);
    header["tree"] = json!({"sha256":branchb.sha256,"bytes":branchb.bytes});
    let root = store.put_value(&header);
    assert!(query(&mut store, &root, index.source(), &ids[0]).is_err());
}
