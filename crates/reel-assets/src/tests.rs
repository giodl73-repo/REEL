use super::*;
use tempfile::TempDir;

#[test]
fn cache_backed_version_records_preserve_locks_and_fail_closed() {
    let f = Fixture::new();
    let root = f.dir.path();
    fs::create_dir(root.join("registry")).unwrap();
    let v = versions::Version {
        logical_id: "book/one".into(),
        version_id: "v1".into(),
        manifest_sha256: "a".repeat(64),
        payloads: (0..2100).map(|n| format!("{n:064x}")).collect(),
        dependencies: vec![],
        supersedes: None,
        deprecated: false,
        dependency_contract: None,
    };
    let bytes = serde_json::to_vec(&v).unwrap();
    let oid = format!("{:x}", Sha256::digest(&bytes));
    let index = json!({"generation":1,"versions":{},"version_records":{"book/one@v1":oid},"current":{"book/one":"v1"},"pins":["book/one@v1"],"leases":{},"coverage_complete":false});
    fs::write(root.join("registry/versions.json"), index.to_string()).unwrap();
    fs::write(
        root.join("registry/objects.tsv"),
        format!("{oid}\t{}\n", bytes.len()),
    )
    .unwrap();
    let d = object(&f.config.cache_roots[1], &oid);
    fs::create_dir_all(d.parent().unwrap()).unwrap();
    fs::write(&d, &bytes).unwrap();
    assert_eq!(
        versions::read(root).unwrap().versions["book/one@v1"].manifest_sha256,
        "a".repeat(64)
    );
    versions::transact(root, |s| {
        s.select("book/one", "v1")?;
        Ok(())
    })
    .unwrap();
    assert!(!root.join("registry/version-records").exists());
    assert!(versions::read(root).unwrap().pins.contains("book/one@v1"));
    let e = object(&f.config.cache_roots[0], &oid);
    fs::create_dir_all(e.parent().unwrap()).unwrap();
    fs::write(&e, vec![b'x'; bytes.len()]).unwrap();
    assert!(versions::read(root).is_err()); // corrupt E cannot fall through to valid D
    fs::remove_file(e).unwrap();
    let m = object(&f.config.archive_roots[0], &oid);
    fs::create_dir_all(m.parent().unwrap()).unwrap();
    fs::write(&m, &bytes).unwrap();
    fs::remove_file(d).unwrap();
    assert!(versions::read(root).is_err()); // archive-only control data is not auto-restored
    assert_eq!(fs::read(m).unwrap(), bytes);
}

struct Fixture {
    dir: TempDir,
    config: Config,
    oid: String,
}

#[test]
fn generic_path_policy_is_explicit_and_rejects_unknown_policy_names() {
    let f = Fixture::new();
    let path = f.dir.path().join(".local/storage-rust.json");
    let mut cfg = load(&path).unwrap();
    cfg["path_policy"] = json!("configured-local-roots");
    fs::write(&path, cfg.to_string()).unwrap();
    assert!(matches!(
        config(f.dir.path()).unwrap().path_policy,
        PathPolicy::ConfiguredLocalRoots
    ));
    cfg["path_policy"] = json!("allow-anything");
    fs::write(&path, cfg.to_string()).unwrap();
    assert!(config(f.dir.path()).is_err());
}

#[cfg(windows)]
#[test]
fn generic_project_can_use_any_local_drive_letter_without_changing_legacy_consumers() {
    let f = Fixture::new();
    let path = f.dir.path().join(".local/storage-rust.json");
    let mut cfg = json!({"cache_roots":["G:/example-assets","M:/example-history"],"archive_roots":["X:/example-archive"]});
    fs::write(&path, cfg.to_string()).unwrap();
    assert!(config(f.dir.path()).is_err());
    cfg["path_policy"] = json!("configured-local-roots");
    fs::write(&path, cfg.to_string()).unwrap();
    assert!(config(f.dir.path()).is_ok());
    cfg["cache_roots"] = json!(["//server/share"]);
    fs::write(&path, cfg.to_string()).unwrap();
    assert!(config(f.dir.path()).is_err());
}

fn scene_fixture(f: &Fixture) -> PathBuf {
    let root = f.dir.path();
    let scene = json!({"schema":"reel.scene-authoring.v2","scene_id":"one","episode_id":"video-one","authoring_state":"imported-evidence","source_authority_id":"original","languages":{},"holds":["Imported source only"]});
    let bindings = json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"one","assets":{"movie":{"sha256":f.oid,"bytes":3,"cache_uri":format!("cache://sha256/{}",f.oid),"logical_id":"movie","selection_state":"selected-private-production"}}});
    let snapshot = scenes::snapshot(&scene, &bindings).unwrap();
    let oid = format!("{:x}", Sha256::digest(&snapshot));
    let mut state = versions::State::default();
    state
        .register(versions::Version {
            logical_id: "scene/one".into(),
            version_id: "v1".into(),
            manifest_sha256: oid.clone(),
            payloads: std::collections::BTreeSet::from([f.oid.clone()]),
            dependencies: vec![],
            supersedes: None,
            deprecated: false,
            dependency_contract: None,
        })
        .unwrap();
    state.select("scene/one", "v1").unwrap();
    fs::create_dir(root.join("registry")).unwrap();
    fs::write(
        root.join("registry/versions.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    fs::write(root.join("catalog.json"),json!({"videos":[{"scene":"videos/scene.json","bindings":"videos/scene-bindings.json","version_lock":{"logical_id":"scene/one","version_id":"v1","manifest_sha256":oid}}]}).to_string()).unwrap();
    fs::write(
        root.join("videos/scene-bindings.json"),
        bindings.to_string(),
    )
    .unwrap();
    let path = root.join("videos/scene.json");
    fs::write(&path, scene.to_string()).unwrap();
    path
}

fn golden_fixture(f: &Fixture) {
    let path = scene_fixture(f);
    let root = f.dir.path();
    let mut scene = load(&path).unwrap();
    scene["scene_id"] = json!("music-video/one");
    let binding_path = root.join("videos/scene-bindings.json");
    let mut bindings = load(&binding_path).unwrap();
    bindings["scope_id"] = json!("music-video/one");
    let asset = bindings["assets"]["movie"].take();
    bindings["assets"] = json!({"delivery/movie":asset});
    let bytes = scenes::snapshot(&scene, &bindings).unwrap();
    let oid = format!("{:x}", Sha256::digest(&bytes));
    let snap = object(&f.config.cache_roots[1], &oid);
    fs::create_dir_all(snap.parent().unwrap()).unwrap();
    fs::write(snap, &bytes).unwrap();
    fs::write(
        root.join("registry/objects.tsv"),
        format!("{oid}\t{}\n", bytes.len()),
    )
    .unwrap();
    let mut state = versions::read(root).unwrap();
    state
        .versions
        .get_mut("scene/one@v1")
        .unwrap()
        .manifest_sha256 = oid.clone();
    fs::write(
        root.join("registry/versions.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let entry = json!({"golden_id":"golden-1","snapshot":{"sha256":oid,"bytes":bytes.len(),"cache_uri":format!("cache://sha256/{oid}")},"movie_sha256":f.oid,"version_lock":{"logical_id":"scene/one","version_id":"v1","manifest_sha256":oid}});
    fs::write(
        root.join("registry/goldens.json"),
        json!({"schema":"caminos.golden-history.v1","videos":{"one":[entry]}}).to_string(),
    )
    .unwrap();
    // Keep current as it was: a historical restore must never retarget it.
}

fn artifact_fixture(f: &Fixture) -> PathBuf {
    let root = f.dir.path();
    let _ = scene_fixture(f);
    let package = json!({"schema":"caminos.artifact-package.v1","artifact_id":"song/one","kind":"song","version_id":"golden-v1","edition":"full","assets":{"recording":{"sha256":f.oid,"bytes":3,"cache_uri":format!("cache://sha256/{}",f.oid)}}});
    let bytes = serde_json::to_vec(&package).unwrap();
    let oid = format!("{:x}", Sha256::digest(&bytes));
    let path = object(&f.config.cache_roots[1], &oid);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, &bytes).unwrap();
    fs::write(
        root.join("registry/objects.tsv"),
        format!("{oid}\t{}\n", bytes.len()),
    )
    .unwrap();
    let mut state = versions::read(root).unwrap();
    state
        .register(versions::Version {
            logical_id: "song/one".into(),
            version_id: "golden-v1".into(),
            manifest_sha256: oid.clone(),
            payloads: std::collections::BTreeSet::from([f.oid.clone()]),
            dependencies: vec![],
            supersedes: None,
            deprecated: false,
            dependency_contract: None,
        })
        .unwrap();
    state.select("song/one", "golden-v1").unwrap();
    fs::write(
        root.join("registry/versions.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    fs::create_dir_all(root.join("songs/one")).unwrap();
    let record = root.join("songs/one/song.json");
    fs::write(&record,json!({"schema":"caminos.artifact-record.v1","artifact_id":"song/one","kind":"song","current_version":"golden-v1","versions":[{"version_id":"golden-v1","manifest":{"sha256":oid,"bytes":bytes.len(),"cache_uri":format!("cache://sha256/{oid}")},"version_lock":{"logical_id":"song/one","version_id":"golden-v1","manifest_sha256":oid}}]}).to_string()).unwrap();
    record
}

#[test]
fn artifact_rejects_a_valid_lock_under_another_logical_identity() {
    let f = Fixture::new();
    let record = artifact_fixture(&f);
    f.put(1, b"cel");
    let root = f.dir.path();
    let mut state = versions::read(root).unwrap();
    let mut alias = state.versions["song/one@golden-v1"].clone();
    alias.logical_id = "arrangement/other".into();
    state.register(alias).unwrap();
    state.select("arrangement/other", "golden-v1").unwrap();
    fs::write(
        root.join("registry/versions.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let mut doc = load(&record).unwrap();
    doc["versions"][0]["version_lock"]["logical_id"] = json!("arrangement/other");
    fs::write(&record, doc.to_string()).unwrap();
    assert!(artifacts::check(root, &record, None, true).is_err());
    assert!(artifacts::check(root, &record, Some("golden-v1"), true).is_err());
}

#[test]
fn generic_artifact_preserves_explicit_history_and_exact_roles() {
    let f = Fixture::new();
    let record = artifact_fixture(&f);
    f.put(1, b"cel");
    let root = f.dir.path();
    assert_eq!(
        artifacts::check(root, &record, None, true).unwrap()["version_id"],
        "golden-v1"
    );
    assert_eq!(
        artifacts::locate(root, &record, None, "recording").unwrap()["active_tier"],
        1
    );
    assert!(artifacts::locate(root, &record, None, "arrangement").is_err());
    let original = fs::read(&record).unwrap();
    let mut mislabeled = load(&record).unwrap();
    mislabeled["versions"][0]["edition"] = json!("short");
    fs::write(&record, mislabeled.to_string()).unwrap();
    assert!(artifacts::check(root, &record, None, false).is_err());
    fs::write(&record, original).unwrap();
    let mut state = versions::read(root).unwrap();
    state.current.remove("song/one");
    fs::write(
        root.join("registry/versions.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    assert!(artifacts::check(root, &record, None, false).is_err());
    assert!(artifacts::check(root, &record, Some("golden-v1"), true).is_ok());
    assert!(artifacts::check(root, &record, Some("short"), false).is_err());
}

#[test]
fn artifact_verification_never_hides_corruption_or_recovers_archive() {
    let f = Fixture::new();
    let record = artifact_fixture(&f);
    let root = f.dir.path();
    f.put(2, b"cel");
    assert!(artifacts::check(root, &record, None, true).is_err());
    assert!(!f.config.cache_roots[0].exists());
    f.put(1, b"cel");
    f.put(0, b"bad");
    assert!(artifacts::check(root, &record, None, true).is_err());
}

#[test]
fn golden_restore_is_repeatable_and_preserves_current_and_edited_files() {
    let f = Fixture::new();
    golden_fixture(&f);
    f.put(1, b"cel");
    let root = f.dir.path();
    let current = fs::read(root.join("catalog.json")).unwrap();
    let state = fs::read(root.join("registry/versions.json")).unwrap();
    let first = goldens::restore(root, "one", "golden-1").unwrap();
    assert_eq!(first["payload_bytes_copied"], 0);
    assert_eq!(first["current_changed"], false);
    assert_eq!(
        goldens::restore(root, "one", "golden-1").unwrap()["reused_restore"],
        true
    );
    let restored = PathBuf::from(first["directory"].as_str().unwrap()).join("scene.json");
    assert_eq!(load(&restored).unwrap()["scene_id"], "music-video/one");
    fs::write(restored, b"edited").unwrap();
    assert!(goldens::restore(root, "one", "golden-1").is_err());
    assert_eq!(fs::read(root.join("catalog.json")).unwrap(), current);
    assert_eq!(
        fs::read(root.join("registry/versions.json")).unwrap(),
        state
    );
    assert!(goldens::restore(root, "one", "unknown").is_err());
    assert!(goldens::restore(root, "../one", "golden-1").is_err());
}

#[test]
fn golden_restore_fails_before_writing_when_media_is_missing_or_corrupt() {
    let f = Fixture::new();
    golden_fixture(&f);
    f.put(2, b"cel");
    let root = f.dir.path();
    assert!(goldens::check(root, "one", "golden-1", false).is_ok());
    assert!(goldens::restore(root, "one", "golden-1").is_err());
    assert!(!root.join(".local/restored-goldens").exists());
    f.put(1, b"cel");
    f.put(0, b"bad");
    assert!(goldens::restore(root, "one", "golden-1").is_err());
    assert!(!root.join(".local/restored-goldens").exists());
}

#[test]
fn scenes_are_locked_and_metadata_checks_never_probe_media() {
    let f = Fixture::new();
    let path = scene_fixture(&f);
    assert_eq!(
        scenes::check(f.dir.path(), &path, false).unwrap()["fresh_hash_verified"],
        false
    );
    f.put(2, b"cel");
    assert!(scenes::check(f.dir.path(), &path, true).is_err());
    assert!(!f.config.cache_roots[0].exists());
    let d = f.put(1, b"cel");
    let result = scenes::check(f.dir.path(), &path, true).unwrap();
    assert_eq!(result["active_tiers"]["1"], 1);
    assert_eq!(
        scenes::locate(f.dir.path(), &path, "movie").unwrap()["path"],
        json!(d)
    );
    f.put(0, b"bad");
    assert!(scenes::check(f.dir.path(), &path, true).is_err());
    let mut changed = load(&path).unwrap();
    changed["source_authority_id"] = json!("changed");
    fs::write(&path, changed.to_string()).unwrap();
    assert!(scenes::check(f.dir.path(), &path, false).is_err());
}

#[test]
fn scenes_reject_scope_size_and_unknown_bindings() {
    for field in ["scope_id", "bytes", "cache_uri"] {
        let f = Fixture::new();
        let path = scene_fixture(&f);
        let binding = f.dir.path().join("videos/scene-bindings.json");
        let mut data = load(&binding).unwrap();
        match field {
            "scope_id" => data["scope_id"] = json!("another"),
            "bytes" => data["assets"]["movie"][field] = json!(4),
            _ => data["assets"]["movie"][field] = json!("cache://wrong"),
        };
        fs::write(binding, data.to_string()).unwrap();
        assert!(scenes::check(f.dir.path(), &path, false).is_err());
    }
    let f = Fixture::new();
    let path = scene_fixture(&f);
    assert!(scenes::locate(f.dir.path(), &path, "unknown").is_err());
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir(root.join(".local")).unwrap();
        fs::create_dir(root.join("videos")).unwrap();
        let oid = format!("{:x}", Sha256::digest(b"cel"));
        let config = Config {
            cache_roots: vec![root.join("E"), root.join("D")],
            archive_roots: vec![root.join("M")],
            ..Config::default()
        };
        fs::write(
            root.join(".local/storage-rust.json"),
            json!({"cache_roots":config.cache_roots,"archive_roots":config.archive_roots})
                .to_string(),
        )
        .unwrap();
        fs::write(
            root.join("catalog.json"),
            r#"{"videos":[{"record":"videos/video.json"}]}"#,
        )
        .unwrap();
        fs::write(
            root.join("videos/video.json"),
            json!({"assets":{"cel":{"sha256":oid,"bytes":3}}}).to_string(),
        )
        .unwrap();
        Self { dir, config, oid }
    }
    fn put(&self, tier: usize, bytes: &[u8]) -> PathBuf {
        let roots = [
            &self.config.cache_roots[0],
            &self.config.cache_roots[1],
            &self.config.archive_roots[0],
        ];
        let path = object(roots[tier], &self.oid);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
    fn run(&self, input: &str) -> Vec<Value> {
        let mut out = Vec::new();
        protocol(
            &self.dir.path().canonicalize().unwrap(),
            input.as_bytes(),
            &mut out,
        )
        .unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }
}

#[test]
fn active_reuse_keeps_d_in_place_and_never_recovers_archive() {
    let f = Fixture::new();
    f.put(2, b"cel");
    assert!(active_existing(&f.config, &f.oid, 3).unwrap().is_none());
    let d = f.put(1, b"cel");
    assert_eq!(active_existing(&f.config, &f.oid, 3).unwrap(), Some(d));
    assert!(!object(&f.config.cache_roots[0], &f.oid).exists());
    let e = f.put(0, b"cel");
    assert_eq!(active_existing(&f.config, &f.oid, 3).unwrap(), Some(e));
}

#[test]
fn active_reuse_rejects_corrupt_or_nonregular_e_instead_of_hiding_it() {
    let f = Fixture::new();
    f.put(1, b"cel");
    f.put(0, b"BAD");
    assert!(active_existing(&f.config, &f.oid, 3).is_err());
    assert!(active_existing(&f.config, "../outside", 3).is_err());
    let f = Fixture::new();
    f.put(1, b"cel");
    fs::create_dir_all(object(&f.config.cache_roots[0], &f.oid)).unwrap();
    assert!(active_existing(&f.config, &f.oid, 3).is_err());
}

#[test]
fn e_wins_and_never_probes_archive() {
    let f = Fixture::new();
    let e = f.put(0, b"cel");
    f.put(1, b"BAD");
    fs::write(&f.config.archive_roots[0], b"not a directory").unwrap();
    assert_eq!(resolve(f.dir.path(), &f.config, &f.oid, 3).unwrap(), e);
}

#[test]
fn d_fallback_promotes_verified_object_without_archive() {
    let f = Fixture::new();
    let d = f.put(1, b"cel");
    fs::create_dir_all(&f.config.cache_roots[0]).unwrap();
    assert_eq!(
        resolve(f.dir.path(), &f.config, &f.oid, 3).unwrap(),
        object(&f.config.cache_roots[0], &f.oid)
    );
    verify(&d, &f.oid, 3).unwrap();
}

#[test]
fn archive_is_verified_and_promoted_to_e() {
    let f = Fixture::new();
    let m = f.put(2, b"cel");
    fs::create_dir_all(&f.config.cache_roots[0]).unwrap();
    let resolved = resolve(f.dir.path(), &f.config, &f.oid, 3).unwrap();
    assert_eq!(resolved, object(&f.config.cache_roots[0], &f.oid));
    assert_eq!(fs::read(m).unwrap(), b"cel");
    verify(&resolved, &f.oid, 3).unwrap();
    assert!(f.dir.path().join(".local/cache-promotions.jsonl").exists());
}

#[test]
fn corruption_stops_instead_of_hiding_it_with_fallback() {
    let f = Fixture::new();
    f.put(0, b"BAD");
    f.put(1, b"cel");
    assert!(
        resolve(f.dir.path(), &f.config, &f.oid, 3)
            .unwrap_err()
            .to_string()
            .contains("SHA-256")
    );
}

#[test]
fn bad_archive_never_enters_active_cache() {
    let f = Fixture::new();
    f.put(2, b"BAD");
    assert!(resolve(f.dir.path(), &f.config, &f.oid, 3).is_err());
    assert!(!f.config.cache_roots[0].exists());
}

#[test]
fn wrong_size_is_rejected() {
    let f = Fixture::new();
    f.put(0, b"cel extra");
    assert!(
        resolve(f.dir.path(), &f.config, &f.oid, 3)
            .unwrap_err()
            .to_string()
            .contains("size")
    );
}

#[test]
fn upload_refused_before_reading_config() {
    let f = Fixture::new();
    fs::remove_file(f.dir.path().join(".local/storage-rust.json")).unwrap();
    assert!(
        f.run("{\"event\":\"init\",\"operation\":\"upload\"}\n")[0]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Uploads disabled")
    );
}

#[test]
fn exact_protocol_delivers_disposable_link_and_preserves_original() {
    let f = Fixture::new();
    let source = f.put(0, b"cel");
    let input = format!(
        "{{\"event\":\"init\",\"operation\":\"download\"}}\n{{\"event\":\"download\",\"oid\":\"{}\",\"size\":3}}\n{{\"event\":\"terminate\"}}\n",
        f.oid
    );
    let replies = f.run(&input);
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0], json!({}));
    let path = PathBuf::from(replies[1]["path"].as_str().unwrap());
    assert_ne!(path, source);
    fs::remove_file(path).unwrap();
    verify(&source, &f.oid, 3).unwrap();
}

#[test]
fn unregistered_hash_is_rejected_without_touching_caches() {
    let f = Fixture::new();
    let replies = f.run(&format!("{{\"event\":\"init\",\"operation\":\"download\"}}\n{{\"event\":\"download\",\"oid\":\"{}\",\"size\":3}}\n", "0".repeat(64)));
    assert!(
        replies[1]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not a registered")
    );
    assert!(!f.config.cache_roots[0].exists());
}

#[test]
fn malformed_requests_do_not_panic() {
    let f = Fixture::new();
    assert!(f.run("not json\n")[0]["error"].is_object());
    assert!(
        f.run("{\"event\":\"download\",\"oid\":\"../x\",\"size\":3}\n")[0]["error"].is_object()
    );
}

#[test]
fn recovery_cannot_overwrite_existing_bad_object() {
    let f = Fixture::new();
    let m = f.put(2, b"cel");
    fs::create_dir_all(&f.config.cache_roots[0]).unwrap();
    let e = f.put(0, b"BAD");
    assert!(recover(f.dir.path(), &m, &f.config.cache_roots[0], &f.oid, 3).is_err());
    assert_eq!(fs::read(e).unwrap(), b"BAD");
}

#[test]
fn failed_copy_never_publishes_partial_object() {
    let f = Fixture::new();
    let source = f.put(2, b"cel grows unexpectedly");
    assert!(recover(f.dir.path(), &source, &f.config.cache_roots[0], &f.oid, 3).is_err());
    assert!(!object(&f.config.cache_roots[0], &f.oid).exists());
    assert_eq!(fs::read(source).unwrap(), b"cel grows unexpectedly");
}

#[test]
fn concurrent_same_hash_publication_keeps_one_valid_object_and_valid_receipts() {
    let f = Fixture::new();
    let source = f.put(2, b"cel");
    let mut jobs = Vec::new();
    for _ in 0..2 {
        let root = f.dir.path().to_path_buf();
        let source = source.clone();
        let destination = f.config.cache_roots[0].clone();
        let oid = f.oid.clone();
        jobs.push(std::thread::spawn(move || {
            recover(&root, &source, &destination, &oid, 3).map_err(|e| e.to_string())
        }));
    }
    let outcomes: Vec<_> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
    assert!(outcomes.iter().any(|r| r.is_ok()));
    verify(&object(&f.config.cache_roots[0], &f.oid), &f.oid, 3).unwrap();
    verify(&source, &f.oid, 3).unwrap();
    for line in fs::read_to_string(f.dir.path().join(".local/cache-promotions.jsonl"))
        .unwrap()
        .lines()
    {
        assert!(serde_json::from_str::<Value>(line).is_ok());
    }
}
