//! Actual native producers through the opt-in executable boundary, using only
//! synthetic temporary project/cache trees. No installed runtime or live caches.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    cache: PathBuf,
    profile: PathBuf,
    source_file: PathBuf,
    primary: String,
    unknown: String,
}
impl Fixture {
    fn new(named: bool, gzip: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        for name in [
            ".local",
            "registry",
            "cache",
            "history",
            "source",
            "coordination",
        ] {
            fs::create_dir(root.join(name)).unwrap();
        }
        let cache = root.join("cache");
        let source_cache = root.join("source");
        for name in ["staging", "retained", "catalog"] {
            fs::create_dir(cache.join(name)).unwrap();
        }
        fs::write(root.join("catalog.json"), r#"{"videos":[]}"#).unwrap();
        fs::write(
            root.join("registry/versions.json"),
            serde_json::to_vec(&reel_assets::versions::State::default()).unwrap(),
        )
        .unwrap();
        let storage = json!({"path_policy":"configured-local-roots","cache_roots":[cache,root.join("history")],"archive_roots":[source_cache],
            "coordination_root":root.join("coordination"),"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}});
        fs::write(root.join(".local/storage-rust.json"), storage.to_string()).unwrap();
        fs::write(root.join("asset-project.json"),json!({"schema":"reel.asset-project.v1","project_id":"synthetic",
            "storage_config":".local/storage-rust.json","selections":[],"scan_cas":true,"measure_unregistered_physical":true,
            "max_inventory_objects":100,"lfs_namespaces":[]}).to_string()).unwrap();
        let primary = hash(b"synthetic primary");
        let unknown = hash(b"unregistered");
        for tier in [&cache, &root.join("history")] {
            for (id, data) in [
                (&primary, b"synthetic primary".as_slice()),
                (
                    &unknown,
                    if tier == &cache {
                        b"unregistered".as_slice()
                    } else {
                        b"short".as_slice()
                    },
                ),
            ] {
                let path = reel_assets::object(tier, id);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, data).unwrap();
            }
        }
        fs::write(
            root.join("registry/objects.tsv"),
            format!("{primary}\t17\n"),
        )
        .unwrap();
        let report = if named {
            reel_assets::accounting::observe_named(&root, &BTreeSet::from([primary.clone()]))
                .unwrap()
        } else {
            reel_assets::accounting::report(&root).unwrap()
        };
        let decoded = serde_json::to_vec(&report).unwrap();
        let encoded = if gzip {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            encoder.write_all(&decoded).unwrap();
            encoder.finish().unwrap()
        } else {
            decoded.clone()
        };
        let source_file = reel_assets::object(&source_cache, &hash(&encoded));
        fs::create_dir_all(source_file.parent().unwrap()).unwrap();
        fs::write(&source_file, &encoded).unwrap();
        // A missing unrelated archive must not be probed by the managed commands.
        let mut storage = storage;
        storage["archive_roots"]
            .as_array_mut()
            .unwrap()
            .push(json!(root.join("UNAVAILABLE_UNRELATED_ARCHIVE")));
        fs::write(
            root.join("private-storage.json"),
            serde_json::to_vec_pretty(&storage).unwrap(),
        )
        .unwrap();
        let profile = root.join("private-index-job.json");
        fs::write(&profile,serde_json::to_vec_pretty(&json!({"schema":"reel.inventory-index-job.v1","storage_config":"private-storage.json",
            "cache":cache,"staging_parent":cache.join("staging"),"retained_parent":cache.join("retained"),"catalog_directory":cache.join("catalog"),
            "catalog_id":hash(b"fixture catalog"),"key":"PRIVATE_KEY_DO_NOT_EXPORT","expected_revision":0,"session_id":"first",
            "source":{"schema":"reel.asset-cache-report.v1","project_id":"synthetic","authority_namespace":"PRIVATE_AUTHORITY_DO_NOT_EXPORT",
                "adapter_version":1,"encoding":if gzip {"gzip"}else{"identity"},"cache":source_cache,
                "encoded":{"sha256":hash(&encoded),"bytes":encoded.len()},"decoded":{"sha256":hash(&decoded),"bytes":decoded.len()}},
            "budgets":{"encoded_bytes":1024*1024,"decoded_bytes":1024*1024,"objects":100,"copies":200,"index_bytes":8*1024*1024,"index_nodes":64}})).unwrap()).unwrap();
        Self {
            _temp: temp,
            root,
            cache,
            profile,
            source_file,
            primary,
            unknown,
        }
    }
    fn command(&self, operation: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_reel-assets"));
        command
            .current_dir(&self.root)
            .arg("--root")
            .arg(&self.root)
            .arg(operation)
            .arg(&self.profile)
            .env_remove("CAMINOS_TRANSFER_CACHE")
            .env_remove("GIT_LFS_SKIP_SMUDGE");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        command
    }
    fn run(&self, operation: &str) -> Output {
        self.command(operation).output().unwrap()
    }
    fn success(&self, operation: &str) -> Value {
        let output = self.run(operation);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn update(&self, edit: impl FnOnce(&mut Value)) {
        let mut profile: Value = serde_json::from_slice(&fs::read(&self.profile).unwrap()).unwrap();
        edit(&mut profile);
        fs::write(&self.profile, serde_json::to_vec_pretty(&profile).unwrap()).unwrap();
    }
    fn queries(&self, subjects: Value) -> PathBuf {
        let path = self.root.join("private-subjects.json");
        fs::write(&path, serde_json::to_vec(&subjects).unwrap()).unwrap();
        path
    }
    fn shadow(&self, path: &Path) -> Output {
        self.command("inventory-index-shadow")
            .arg(path)
            .output()
            .unwrap()
    }
}
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), out)
            } else {
                out.insert(
                    entry.path().strip_prefix(root).unwrap().to_owned(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}
fn assert_private(summary: &Value, f: &Fixture) {
    for (field, value) in summary.as_object().unwrap() {
        assert!(
            field == "schema" || value.is_number() || value.is_boolean(),
            "non-count private summary field"
        );
    }
    let wire = summary.to_string();
    for sentinel in [
        "PRIVATE_KEY_DO_NOT_EXPORT",
        "PRIVATE_AUTHORITY_DO_NOT_EXPORT",
        f.primary.as_str(),
        f.unknown.as_str(),
        f.root.to_str().unwrap(),
    ] {
        assert!(!wire.contains(sentinel), "private content in summary");
    }
}

#[test]
fn native_census_build_and_named_shadow_have_exact_read_only_parity() {
    let f = Fixture::new(false, false);
    let original = fs::read(&f.source_file).unwrap();
    let init = f.success("inventory-index-init");
    assert_eq!(init["catalog_revision"], 0);
    let built = f.success("inventory-index-build");
    assert_eq!(built["catalog_revision"], 1);
    assert_eq!(built["objects"], 2);
    assert_eq!(built["copies"], 4);
    assert_eq!(built["staging_retained"], true);
    assert_eq!(built["media_payloads_copied"], 0);
    assert_private(&built, &f);
    let queries = f.queries(json!([f.primary, f.unknown, hash(b"absent")]));
    let before = snapshot(&f.root);
    let output = f.shadow(&queries);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["subjects_checked"], 3);
    assert_eq!(summary["listed"], 2);
    assert_eq!(summary["not_listed_at_capture"], 1);
    assert_eq!(summary["parity"], true);
    assert_eq!(summary["writes"], 0);
    assert_eq!(summary["removal_authorized"], false);
    assert_private(&summary, &f);
    assert!(before == snapshot(&f.root));
    assert_eq!(fs::read(&f.source_file).unwrap(), original);
    assert!(f.cache.join("staging/first/root.json").is_file());
    assert!(!f.cache.join("staging/first/retirement.json").exists());
}
#[test]
fn named_and_gzip_native_sources_flow_without_inventing_capture_membership() {
    for gzip in [false, true] {
        let f = Fixture::new(true, gzip);
        let original = fs::read(&f.source_file).unwrap();
        f.success("inventory-index-init");
        f.success("inventory-index-build");
        let queries = f.queries(json!([f.primary, f.unknown]));
        let output = f.shadow(&queries);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(summary["listed"], 1);
        assert_eq!(summary["not_listed_at_capture"], 1);
        assert_eq!(fs::read(&f.source_file).unwrap(), original);
        assert_private(&summary, &f);
    }
}
#[test]
fn stale_revision_fails_before_creating_a_new_session_or_reading_corrupt_source() {
    let f = Fixture::new(false, false);
    f.success("inventory-index-init");
    f.success("inventory-index-build");
    f.update(|v| v["session_id"] = json!("stale-writer"));
    fs::write(&f.source_file, b"corrupt source").unwrap();
    let output = f.run("inventory-index-build");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("revision conflict"));
    assert!(!f.cache.join("staging/stale-writer").exists());
}
#[test]
fn corrupt_source_is_rejected_before_any_managed_session_or_selection_write() {
    let f = Fixture::new(false, false);
    f.success("inventory-index-init");
    let mut raw = fs::read(&f.source_file).unwrap();
    raw[0] = b'[';
    fs::write(&f.source_file, raw).unwrap();
    let before = snapshot(&f.root);
    assert!(!f.run("inventory-index-build").status.success());
    assert!(before == snapshot(&f.root));
    assert!(!f.cache.join("staging/first").exists());
}
#[test]
fn changed_source_or_invalid_shadow_subjects_never_emit_success_or_mutate_state() {
    let f = Fixture::new(false, false);
    f.success("inventory-index-init");
    f.success("inventory-index-build");
    for subjects in [json!([]), json!([f.primary, f.primary]), json!(["invalid"])] {
        let path = f.queries(subjects);
        let before = snapshot(&f.root);
        assert!(!f.shadow(&path).status.success());
        assert!(before == snapshot(&f.root));
    }
    let path = f.queries(json!([f.primary]));
    f.update(|v| v["source"]["encoded"]["sha256"] = json!(hash(b"stale evidence")));
    let before = snapshot(&f.root);
    assert!(!f.shadow(&path).status.success());
    assert!(before == snapshot(&f.root));
}
#[test]
fn shadow_query_limits_are_aggregate_and_failure_is_read_only() {
    let f = Fixture::new(false, false);
    f.success("inventory-index-init");
    f.success("inventory-index-build");
    let path = f.queries(json!([f.primary, f.unknown]));
    for (field, limit) in [
        ("shadow_subjects", 1),
        ("shadow_bytes", 1),
        ("shadow_nodes", 1),
    ] {
        f.update(|v| {
            v["budgets"]["shadow_subjects"] = json!(4096);
            v["budgets"]["shadow_bytes"] = json!(256 * 1024 * 1024);
            v["budgets"]["shadow_nodes"] = json!(16384);
            v["budgets"][field] = json!(limit);
        });
        let before = snapshot(&f.root);
        assert!(!f.shadow(&path).status.success());
        assert!(before == snapshot(&f.root));
    }
}
#[test]
fn failed_partial_publication_keeps_staging_without_admitting_or_auto_retiring() {
    let f = Fixture::new(false, false);
    f.success("inventory-index-init");
    f.update(|v| v["budgets"]["index_nodes"] = json!(1));
    assert!(!f.run("inventory-index-build").status.success());
    let staged = f.cache.join("staging/first");
    assert!(staged.join("capacity.json").is_file());
    assert_eq!(fs::read_dir(staged.join("objects")).unwrap().count(), 1);
    assert!(!staged.join("root.json").exists());
    assert!(!staged.join("retirement.json").exists());
    assert_eq!(f.success("inventory-index-recover")["catalog_revision"], 0);
}
#[test]
fn initialization_and_recovery_do_not_probe_an_offline_named_source() {
    let f = Fixture::new(false, false);
    let offline = f.root.join("UNAVAILABLE_UNRELATED_ARCHIVE");
    f.update(|v| v["source"]["cache"] = json!(offline));
    assert_eq!(f.success("inventory-index-init")["catalog_revision"], 0);
    assert_eq!(f.success("inventory-index-recover")["catalog_revision"], 0);
    assert!(!f.run("inventory-index-build").status.success());
    assert!(!f.cache.join("staging/first").exists());
    assert!(!offline.exists());
}
#[test]
fn reinitialization_preserves_an_existing_selection_and_current_recovery_is_read_only() {
    let f = Fixture::new(false, false);
    f.success("inventory-index-init");
    f.success("inventory-index-build");
    assert_eq!(f.success("inventory-index-init")["catalog_revision"], 1);
    let before = snapshot(&f.root);
    let result = f.success("inventory-index-recover");
    assert_eq!(result["catalog_revision"], 1);
    assert_private(&result, &f);
    assert!(before == snapshot(&f.root));
}
#[test]
fn unknown_profile_fields_bad_directories_and_formats_do_not_create_a_catalog() {
    for case in ["unknown", "schema", "overlap", "source-format", "budget"] {
        let f = Fixture::new(false, false);
        f.update(|v| match case {
            "unknown" => v["start_daemon"] = json!(true),
            "schema" => v["schema"] = json!("unknown"),
            "overlap" => v["retained_parent"] = v["staging_parent"].clone(),
            "source-format" => v["source"]["schema"] = json!("unknown"),
            _ => v["budgets"]["index_nodes"] = json!(0),
        });
        assert!(!f.run("inventory-index-init").status.success());
        assert!(!f.cache.join("catalog/catalog.json").exists());
    }
}
#[test]
fn named_cache_normalization_preserves_configured_tier_floor() {
    let f = Fixture::new(false, false);
    let raw = f.cache.join(".");
    let config = f.root.join("private-storage.json");
    let mut storage: Value = serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    storage["cache_roots"][0] = json!(raw);
    storage["capacity"]["tier_minimum_free_bytes"] =
        json!({raw.to_string_lossy().to_string():u64::MAX});
    fs::write(&config, serde_json::to_vec(&storage).unwrap()).unwrap();
    f.update(|v| v["cache"] = json!(raw));
    assert!(!f.run("inventory-index-init").status.success());
    assert!(!f.cache.join("catalog/catalog.json").exists());
}
#[test]
fn two_managed_producers_never_force_overwrite_a_catalog_revision() {
    let f = Fixture::new(false, false);
    f.success("inventory-index-init");
    let other = f.root.join("second-profile.json");
    let mut profile: Value = serde_json::from_slice(&fs::read(&f.profile).unwrap()).unwrap();
    profile["session_id"] = json!("second");
    fs::write(&other, serde_json::to_vec(&profile).unwrap()).unwrap();
    let mut one = f.command("inventory-index-build");
    one.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut two = Command::new(env!("CARGO_BIN_EXE_reel-assets"));
    two.current_dir(&f.root)
        .arg("--root")
        .arg(&f.root)
        .arg("inventory-index-build")
        .arg(&other)
        .env_remove("CAMINOS_TRANSFER_CACHE")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        two.creation_flags(0x08000000);
    }
    let first = one.spawn().unwrap();
    let second = two.spawn().unwrap();
    let results = [
        first.wait_with_output().unwrap(),
        second.wait_with_output().unwrap(),
    ];
    assert_eq!(results.iter().filter(|o| o.status.success()).count(), 1);
    let query = f.queries(json!([f.primary]));
    assert!(f.shadow(&query).status.success());
    assert_eq!(f.success("inventory-index-recover")["catalog_revision"], 1);
    for session in ["first", "second"] {
        let path = f.cache.join("staging").join(session);
        if path.exists() {
            assert!(path.join("capacity.json").is_file());
            assert!(!path.join("retirement.json").exists());
        }
    }
}

#[test]
fn shadow_detects_a_validly_hashed_but_wrong_derived_observation() {
    use reel_assets::inventory_evidence::sidecar::{
        Store,
        file_store::{
            FileStore, Limits,
            catalog::{Candidate, Catalog, MutationContext, Scope},
        },
    };
    let f = Fixture::new(false, false);
    f.success("inventory-index-init");
    f.success("inventory-index-build");
    let config: reel_assets::Config =
        serde_json::from_slice(&fs::read(f.root.join("private-storage.json")).unwrap()).unwrap();
    let scope = Scope {
        project_id: "synthetic".into(),
        authority_namespace: "PRIVATE_AUTHORITY_DO_NOT_EXPORT".into(),
        catalog_id: hash(b"fixture catalog"),
    };
    let catalog = Catalog::open(&f.cache, &f.cache.join("catalog"), &scope).unwrap();
    let selected = catalog.read("PRIVATE_KEY_DO_NOT_EXPORT").unwrap();
    let source = selected.source().unwrap().unwrap();
    let old = selected.root().unwrap();
    let mut header: Value =
        serde_json::from_slice(&fs::read(reel_assets::object(&f.cache, &old.sha256)).unwrap())
            .unwrap();
    let leafid = header["tree"]["sha256"].as_str().unwrap();
    let mut leaf: Value =
        serde_json::from_slice(&fs::read(reel_assets::object(&f.cache, leafid)).unwrap()).unwrap();
    leaf["body"]["rows"][0]["observation"]["copies"][0]["allocated_bytes"] = json!(123456789);
    let context = MutationContext {
        project_root: &f.root,
        storage: &config,
        overhead_bytes: 64 * 1024,
    };
    let mut writer = FileStore::begin_admitted(
        &f.root,
        &config,
        &f.cache,
        &f.cache.join("staging"),
        "wrong-transform",
        Limits {
            nodes: 64,
            staged_bytes: 8 * 1024 * 1024,
            ..Limits::default()
        },
        64 * 8192,
    )
    .unwrap();
    let leaf = writer.publish(&serde_json::to_vec(&leaf).unwrap()).unwrap();
    header["tree"] = json!({"sha256":leaf.sha256,"bytes":leaf.bytes});
    let root = writer
        .publish(&serde_json::to_vec(&header).unwrap())
        .unwrap();
    writer.seal(&root).unwrap();
    writer
        .handoff(&f.cache.join("retained"), &root, &source)
        .unwrap();
    // Deliberately admit a simulated faulty trusted transform: source binding
    // and internal closure consistency are NOT source-inclusion proofs.
    let candidate = Candidate::verify(
        &f.cache,
        &f.cache.join("retained"),
        &root,
        &source,
        reel_assets::inventory_evidence::sidecar::ReadLimits::default(),
    )
    .unwrap();
    catalog
        .admit(&context, "PRIVATE_KEY_DO_NOT_EXPORT", 1, &candidate)
        .unwrap();
    drop(writer);
    let path = f.queries(json!([f.primary, f.unknown]));
    let before = snapshot(&f.root);
    let output = f.shadow(&path);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("parity mismatch"));
    assert!(before == snapshot(&f.root));
}

#[test]
fn explicit_profile_ignores_legacy_transfer_overrides_and_requires_no_external_tools() {
    let f = Fixture::new(false, false);
    f.success("inventory-index-init");
    let wrong = f.root.join("MUST_NOT_CREATE_TRANSFER_CACHE");
    let output = f
        .command("inventory-index-build")
        .env("CAMINOS_TRANSFER_CACHE", &wrong)
        .env("GIT_LFS_SKIP_SMUDGE", "0")
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!wrong.exists());
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["media_payloads_copied"], 0);
    assert_private(&summary, &f);
}
