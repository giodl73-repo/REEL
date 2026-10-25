use super::*;
use tempfile::TempDir;

#[test]
fn completed_output_waits_for_metadata_without_recopied_media_or_broken_lock() {
    let f = Fixture::new();
    let source = f.put(2, b"cel");
    let guard = admission::worktree_lock(f.dir.path()).unwrap();
    let root = f.dir.path().to_path_buf();
    let cache = f.config.cache_roots[0].clone();
    let oid = f.oid.clone();
    let original_source = source.clone();
    let task = std::thread::spawn(move || {
        recover_job_output(&root, &original_source, &cache, &oid, 3).map_err(|e| e.to_string())
    });
    let canonical = object(&f.config.cache_roots[0], &f.oid);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !canonical.exists() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // A publication wait must not restart the media copy after its verified
    // immutable destination already exists.
    fs::rename(&source, source.with_extension("retained-fixture")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2200));
    assert!(!task.is_finished());
    assert!(!f.dir.path().join(".local/cache-promotions.jsonl").exists());
    assert!(admission::lock(&f.dir.path().join(".local/metadata-control")).is_err());
    drop(guard);
    assert_eq!(task.join().unwrap().unwrap(), canonical);
    verify(&canonical, &f.oid, 3).unwrap();
    let receipt = fs::read_to_string(f.dir.path().join(".local/cache-promotions.jsonl")).unwrap();
    assert_eq!(receipt.lines().count(), 1);
    assert_eq!(
        serde_json::from_str::<Value>(&receipt).unwrap()["sha256"],
        f.oid
    );
}

#[test]
fn completed_output_registration_waits_without_changing_ordinary_transaction_bound() {
    let f = Fixture::new();
    let manifest = f.dir.path().join("registration.json");
    fs::write(&manifest, json!({"assets":[{"sha256":f.oid,"bytes":3,"provenance":"test","rights":"fixture","selection_state":"candidate"}]}).to_string()).unwrap();
    let guard = admission::worktree_lock(f.dir.path()).unwrap();
    assert!(registry::register(f.dir.path(), &manifest).is_err());
    let root = f.dir.path().to_path_buf();
    let task = std::thread::spawn(move || {
        registry::register_job_outputs(&root, &manifest).map_err(|e| e.to_string())
    });
    std::thread::sleep(std::time::Duration::from_millis(2200));
    assert!(!task.is_finished());
    assert!(!f.dir.path().join("assets/objects/sha256").exists());
    drop(guard);
    task.join().unwrap().unwrap();
    assert_eq!(selected(f.dir.path()).unwrap()[&f.oid], 3);
}

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
fn artifact_export_restores_an_unselected_version_without_retargeting_or_writable_aliases() {
    let f = Fixture::new();
    let record = artifact_fixture(&f);
    let source = f.put(1, b"cel");
    let root = f.dir.path();
    let mut state = versions::read(root).unwrap();
    state.current.remove("song/one");
    fs::write(
        root.join("registry/versions.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let before = fs::read(root.join("registry/versions.json")).unwrap();
    let record_before = fs::read(&record).unwrap();
    let hash = load(&record).unwrap()["versions"][0]["manifest"]["sha256"].clone();
    let out = root.join("restored");
    let spec = root.join("export.json");
    fs::write(&spec,json!({"schema":"reel.artifact-export.v1","record":"songs/one/song.json","version_id":"golden-v1","manifest_sha256":hash,"output":out,"max_copy_bytes":6,"files":[{"role":"recording","path":"recording.wav"},{"role":"recording","path":"another/copy.wav"}]}).to_string()).unwrap();
    let plan = artifact_export::plan(root, &spec).unwrap();
    assert_eq!(plan["planned_copy_bytes"], 6);
    assert_eq!(plan["payload_bytes_copied"], 0);
    assert!(!out.exists());
    let result = artifact_export::export(root, &spec).unwrap();
    assert_eq!(result["payload_bytes_copied"], 6);
    assert_eq!(result["source_objects_verified"], 1);
    assert_eq!(fs::read(out.join("recording.wav")).unwrap(), b"cel");
    fs::write(out.join("recording.wav"), b"edit").unwrap();
    assert_eq!(fs::read(source).unwrap(), b"cel");
    assert_eq!(fs::read(&record).unwrap(), record_before);
    assert_eq!(
        fs::read(root.join("registry/versions.json")).unwrap(),
        before
    );
    assert_eq!(
        load(&out.join("artifact-export-receipt.json")).unwrap()["selection_changed"],
        false
    );
    assert!(artifact_export::export(root, &spec).is_err());
}
#[test]
fn artifact_export_rejects_bad_locks_paths_modes_and_budgets_before_output() {
    let f = Fixture::new();
    let record = artifact_fixture(&f);
    let root = f.dir.path();
    f.put(1, b"cel");
    let hash = load(&record).unwrap()["versions"][0]["manifest"]["sha256"].clone();
    let spec = root.join("export.json");
    let base = json!({"schema":"reel.artifact-export.v1","record":"songs/one/song.json","version_id":"golden-v1","manifest_sha256":hash,"output":root.join("new"),"max_copy_bytes":3,"files":[{"role":"recording","path":"recording.wav"}]});
    let mut bad = vec![];
    for path in [
        "../outside",
        "/outside",
        "C:/outside",
        "folder\\outside",
        "a/../outside",
        "AUX.wav",
        "file:stream",
        ".git/config",
        "file.",
        "folder//name",
        "artifact-export-receipt.json",
        "artifact-export-receipt.json/nested",
    ] {
        let mut x = base.clone();
        x["files"][0]["path"] = json!(path);
        bad.push(x);
    }
    for value in [
        json!([{ "role":"recording","path":"a"},{"role":"recording","path":"a/child"}]),
        json!([{ "role":"recording","path":"A.wav"},{"role":"recording","path":"a.wav"}]),
        json!([{ "role":"recording","path":"É.wav"},{"role":"recording","path":"é.wav"}]),
    ] {
        let mut x = base.clone();
        x["files"] = value;
        bad.push(x);
    }
    for (key, value) in [
        ("manifest_sha256", json!("f".repeat(64))),
        ("version_id", json!("unknown")),
        ("max_copy_bytes", json!(2)),
        ("files", json!([])),
        ("read_roots", json!([])),
        ("output", json!(f.config.cache_roots[1].join("working"))),
        ("unexpected", json!(true)),
    ] {
        let mut x = base.clone();
        x[key] = value;
        bad.push(x);
    }
    let mut x = base.clone();
    x["files"][0]["role"] = json!("unknown");
    bad.push(x);
    for x in bad {
        fs::write(&spec, x.to_string()).unwrap();
        assert!(
            artifact_export::export(root, &spec).is_err(),
            "accepted {x}"
        );
        assert!(!root.join("new").exists());
    }
}
#[test]
fn artifact_export_fails_on_missing_or_corrupt_first_copy_without_archive_recovery() {
    let f = Fixture::new();
    let record = artifact_fixture(&f);
    let root = f.dir.path();
    f.put(2, b"cel");
    let spec = root.join("export.json");
    let hash = load(&record).unwrap()["versions"][0]["manifest"]["sha256"].clone();
    let request = json!({"schema":"reel.artifact-export.v1","record":"songs/one/song.json","version_id":"golden-v1","manifest_sha256":hash,"output":root.join("new"),"max_copy_bytes":3,"files":[{"role":"recording","path":"take.wav"}]});
    fs::write(&spec, request.to_string()).unwrap();
    assert!(artifact_export::export(root, &spec).is_err());
    assert!(!root.join("new").exists());
    f.put(1, b"cel");
    f.put(0, b"bad");
    assert!(artifact_export::export(root, &spec).is_err());
    assert!(!root.join("new").exists());
    let mut explicit = request;
    explicit["read_roots"] = json!([f.config.cache_roots[1]]);
    fs::write(&spec, explicit.to_string()).unwrap();
    assert!(artifact_export::export(root, &spec).is_ok());
    assert_eq!(
        fs::read(object(&f.config.cache_roots[0], &f.oid)).unwrap(),
        b"bad"
    );
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
fn classified_scene_verifies_primary_closure_without_archived_movie_provenance() {
    let f = Fixture::new();
    let scene = scene_fixture(&f);
    let root = f.dir.path();
    f.put(1, b"cel");
    let old = f.retain(b"previous compiled movie", true);
    let cue = f.retain(b"source cue", false);
    let cue_manifest = f.retain(b"cue manifest", false);
    let bindings_path = root.join("videos/scene-bindings.json");
    let mut bindings = load(&bindings_path).unwrap();
    bindings["assets"]["history/movie"] = json!({"logical_id":"old/movie","selection_state":"historical-reference","sha256":old,"bytes":23,"cache_uri":format!("cache://sha256/{old}")});
    fs::write(&bindings_path, bindings.to_string()).unwrap();
    let snapshot = scenes::snapshot(&load(&scene).unwrap(), &bindings).unwrap();
    let manifest = f.retain(&snapshot, false);
    let mut catalog = load(&root.join("catalog.json")).unwrap();
    catalog["videos"][0]["version_lock"]["manifest_sha256"] = json!(manifest);
    fs::write(root.join("catalog.json"), catalog.to_string()).unwrap();
    let mut state = versions::read(root).unwrap();
    state
        .register(versions::Version {
            logical_id: "cue".into(),
            version_id: "v1".into(),
            manifest_sha256: cue_manifest.clone(),
            payloads: [cue.clone()].into(),
            dependencies: vec![],
            supersedes: None,
            deprecated: false,
            dependency_contract: Some(versions::DependencyContract {
                primary_inputs: [cue.clone()].into(),
                outputs: Default::default(),
                references: Default::default(),
            }),
        })
        .unwrap();
    let version = state.versions.get_mut("scene/one@v1").unwrap();
    version.manifest_sha256 = manifest;
    version.payloads.insert(old.clone());
    version.dependencies.push(versions::Dependency {
        logical_id: "cue".into(),
        version_id: "v1".into(),
        manifest_sha256: cue_manifest,
    });
    version.dependency_contract = Some(versions::DependencyContract {
        primary_inputs: Default::default(),
        outputs: [f.oid.clone()].into(),
        references: [old.clone()].into(),
    });
    fs::write(
        root.join("registry/versions.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let result = scenes::check(root, &scene, true).unwrap();
    assert_eq!(result["verified_objects"], 4);
    assert_eq!(result["deferred_reference_objects"], 1);
    assert_eq!(result["archive_probed"], false);
    assert_eq!(
        result["verification_scope"],
        "active-primary-input-and-output-closure"
    );
    assert!(scenes::locate(root, &scene, "history/movie").is_err());
    assert!(!object(&f.config.cache_roots[0], &old).exists());
    let bad = object(&f.config.cache_roots[0], &cue);
    fs::create_dir_all(bad.parent().unwrap()).unwrap();
    fs::write(&bad, b"bad source").unwrap();
    assert!(scenes::check(root, &scene, true).is_err());
    fs::remove_file(bad).unwrap();
    fs::remove_file(object(&f.config.cache_roots[1], &cue)).unwrap();
    assert!(scenes::check(root, &scene, true).is_err());
    assert!(scenes::check(root, &scene, false).is_ok());
}

#[test]
fn classified_artifact_verifies_transitive_inputs_and_defers_archived_references() {
    let f = Fixture::new();
    let record = artifact_fixture(&f);
    let root = f.dir.path();
    f.put(1, b"cel");
    let retain = |bytes: &[u8], archive: bool| f.retain(bytes, archive);
    let cue = retain(b"cue", false);
    let source_manifest = retain(b"primary input manifest", false);
    let historical = retain(b"old compiled movie", true);
    let mut doc = load(&record).unwrap();
    let old_package = doc["versions"][0]["manifest"]["sha256"].as_str().unwrap();
    let mut package = load(&object(&f.config.cache_roots[1], old_package)).unwrap();
    package["assets"]["history/movie"] =
        json!({"sha256":historical,"bytes":18,"cache_uri":format!("cache://sha256/{historical}")});
    let package_bytes = serde_json::to_vec(&package).unwrap();
    let package_hash = retain(&package_bytes, false);
    let mut state = versions::read(root).unwrap();
    state
        .register(versions::Version {
            logical_id: "inputs/one".into(),
            version_id: "v1".into(),
            manifest_sha256: source_manifest.clone(),
            payloads: [cue.clone()].into(),
            dependencies: vec![],
            supersedes: None,
            deprecated: false,
            dependency_contract: Some(versions::DependencyContract {
                primary_inputs: [cue.clone()].into(),
                outputs: Default::default(),
                references: Default::default(),
            }),
        })
        .unwrap();
    let version = state.versions.get_mut("song/one@golden-v1").unwrap();
    version.manifest_sha256 = package_hash.clone();
    version.payloads.insert(historical.clone());
    version.dependencies.push(versions::Dependency {
        logical_id: "inputs/one".into(),
        version_id: "v1".into(),
        manifest_sha256: source_manifest,
    });
    version.dependency_contract = Some(versions::DependencyContract {
        primary_inputs: Default::default(),
        outputs: [f.oid.clone()].into(),
        references: [historical.clone()].into(),
    });
    fs::write(
        root.join("registry/versions.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    doc["versions"][0]["manifest"] = json!({"sha256":package_hash,"bytes":package_bytes.len(),"cache_uri":format!("cache://sha256/{package_hash}")});
    doc["versions"][0]["version_lock"]["manifest_sha256"] = json!(package_hash);
    fs::write(&record, doc.to_string()).unwrap();

    let report = artifacts::check(root, &record, None, true).unwrap();
    assert_eq!(
        report["verification_scope"],
        "active-primary-input-and-output-closure"
    );
    assert_eq!(report["verified_objects"], 4);
    assert_eq!(report["deferred_reference_objects"], 1);
    assert_eq!(report["archive_probed"], false);
    assert!(artifacts::locate(root, &record, None, "history/movie").is_err());
    assert!(!object(&f.config.cache_roots[0], &historical).exists());
    assert!(!object(&f.config.cache_roots[1], &historical).exists());
    // A transitive primary input not listed in the output package must still
    // be freshly verified, and corrupt E cannot fall through to valid D.
    let corrupt = object(&f.config.cache_roots[0], &cue);
    fs::create_dir_all(corrupt.parent().unwrap()).unwrap();
    fs::write(&corrupt, b"bad").unwrap();
    assert!(artifacts::check(root, &record, None, true).is_err());
    fs::remove_file(corrupt).unwrap();
    fs::remove_file(object(&f.config.cache_roots[1], &cue)).unwrap();
    assert!(artifacts::check(root, &record, None, true).is_err());
    assert!(artifacts::check(root, &record, None, false).is_ok());
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
fn golden_history_compacts_exact_bytes_and_never_recovers_archive_implicitly() {
    let f = Fixture::new();
    golden_fixture(&f);
    let root = f.dir.path();
    let mut catalog = load(&root.join("catalog.json")).unwrap();
    catalog["videos"][0]["video_id"] = json!("one");
    fs::write(root.join("catalog.json"), catalog.to_string()).unwrap();
    let cfg_path = root.join(".local/storage-rust.json");
    let mut cfg = load(&cfg_path).unwrap();
    cfg["capacity"] = json!({"minimum_free_bytes":0,"minimum_free_basis_points":0});
    fs::write(cfg_path, cfg.to_string()).unwrap();
    fs::create_dir_all(&f.config.cache_roots[0]).unwrap();
    let index = root.join("registry/goldens.json");
    let before = fs::read(&index).unwrap();
    let listed = goldens::list(root, "one").unwrap();
    let receipt = goldens::compact(root).unwrap();
    let (oid, size) = goldens::control_asset(root).unwrap().unwrap();
    assert_eq!(size, before.len() as u64);
    assert_eq!(
        fs::read(&index).unwrap(),
        registry::pointer(&oid, size).unwrap().as_bytes()
    );
    assert_eq!(
        fs::read(object(&f.config.cache_roots[0], &oid)).unwrap(),
        before
    );
    assert_eq!(goldens::list(root, "one").unwrap(), listed);
    assert_eq!(receipt["media_read"], false);
    assert_eq!(goldens::compact(root).unwrap()["already_compact"], true);
    fs::write(&index, &before).unwrap(); // normal Git LFS smudge leaves JSON
    assert_eq!(
        goldens::control_asset(root).unwrap(),
        Some((oid.clone(), size))
    );
    assert_eq!(goldens::list(root, "one").unwrap(), listed);
    goldens::compact(root).unwrap();
    assert!(!object(&f.config.cache_roots[1], &f.oid).exists()); // no movie was needed
    let e = object(&f.config.cache_roots[0], &oid);
    let d = object(&f.config.cache_roots[1], &oid);
    fs::create_dir_all(d.parent().unwrap()).unwrap();
    fs::write(&d, &before).unwrap();
    fs::write(&e, vec![b'x'; before.len()]).unwrap();
    assert!(goldens::list(root, "one").is_err()); // corrupt E cannot hide behind D
    fs::remove_file(e).unwrap();
    assert_eq!(goldens::list(root, "one").unwrap(), listed);
    let m = object(&f.config.archive_roots[0], &oid);
    fs::create_dir_all(m.parent().unwrap()).unwrap();
    fs::write(&m, &before).unwrap();
    fs::remove_file(d).unwrap();
    assert!(goldens::list(root, "one").is_err());
    assert_eq!(fs::read(m).unwrap(), before);
    assert!(!object(&f.config.cache_roots[0], &oid).exists());
    fs::write(&index, registry::pointer(&oid, 256 * 1024 + 1).unwrap()).unwrap();
    assert!(goldens::control_asset(root).is_err());
    fs::write(
        &index,
        b"version https://git-lfs.github.com/spec/v1\noid sha256:bad\nsize 1\n",
    )
    .unwrap();
    assert!(goldens::control_asset(root).is_err());
}

#[test]
fn classified_golden_restore_keeps_provenance_cold_and_checks_primary_inputs() {
    let f = Fixture::new();
    golden_fixture(&f);
    let root = f.dir.path();
    f.put(1, b"cel");
    let path = root.join("registry/goldens.json");
    let mut history = load(&path).unwrap();
    let row = &mut history["videos"]["one"][0];
    let old_hash = row["snapshot"]["sha256"].as_str().unwrap();
    let mut snapshot = load(&object(&f.config.cache_roots[1], old_hash)).unwrap();
    let prior = f.retain(b"historical compiled movie", true);
    let cue = f.retain(b"primary cue", false);
    snapshot["bindings"]["assets"]["history/movie"] =
        json!({"sha256":prior,"bytes":25,"cache_uri":format!("cache://sha256/{prior}")});
    let bytes = serde_json::to_vec(&snapshot).unwrap();
    let hash = f.retain(&bytes, false);
    row["snapshot"] =
        json!({"sha256":hash,"bytes":bytes.len(),"cache_uri":format!("cache://sha256/{hash}")});
    row["version_lock"]["manifest_sha256"] = json!(hash);
    fs::write(&path, history.to_string()).unwrap();
    let mut state = versions::read(root).unwrap();
    let v = state.versions.get_mut("scene/one@v1").unwrap();
    v.manifest_sha256 = hash;
    v.payloads.extend([prior.clone(), cue.clone()]);
    v.dependency_contract = Some(versions::DependencyContract {
        primary_inputs: [cue.clone()].into(),
        outputs: [f.oid.clone()].into(),
        references: [prior.clone()].into(),
    });
    fs::write(
        root.join("registry/versions.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let control = fs::read(root.join("catalog.json")).unwrap();
    let report = goldens::restore(root, "one", "golden-1").unwrap();
    assert_eq!(report["verified_objects"], 3);
    assert_eq!(report["deferred_reference_objects"], 1);
    assert_eq!(
        report["verification_scope"],
        "active-primary-input-and-output-closure"
    );
    assert_eq!(report["archive_probed"], false);
    assert_eq!(fs::read(root.join("catalog.json")).unwrap(), control);
    assert!(!object(&f.config.cache_roots[0], &prior).exists());
    fs::remove_file(object(&f.config.cache_roots[1], &cue)).unwrap();
    assert!(goldens::restore(root, "one", "golden-1").is_err());
    assert!(goldens::check(root, "one", "golden-1", false).is_ok());
    let v = state.versions.get_mut("scene/one@v1").unwrap();
    v.dependency_contract.as_mut().unwrap().outputs.clear();
    v.dependency_contract
        .as_mut()
        .unwrap()
        .references
        .insert(f.oid.clone());
    fs::write(
        root.join("registry/versions.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    assert!(goldens::check(root, "one", "golden-1", false).is_err());
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
    fn retain(&self, bytes: &[u8], archive: bool) -> String {
        let oid = format!("{:x}", Sha256::digest(bytes));
        let cache = if archive {
            &self.config.archive_roots[0]
        } else {
            &self.config.cache_roots[1]
        };
        let path = object(cache, &oid);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        let manifest = self.dir.path().join(".local/register-object.json");
        fs::write(&manifest, json!({"assets":[{"sha256":oid,"bytes":bytes.len(),"provenance":"test","rights":"fixture","selection_state":"fixture"}]}).to_string()).unwrap();
        registry::register(self.dir.path(), &manifest).unwrap();
        oid
    }
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir(root.join(".local")).unwrap();
        fs::create_dir(root.join("videos")).unwrap();
        let oid = format!("{:x}", Sha256::digest(b"cel"));
        let config = Config {
            cache_roots: vec![root.join("E"), root.join("D")],
            archive_roots: vec![root.join("M")],
            capacity: admission::Policy {
                minimum_free_bytes: 0,
                minimum_free_basis_points: 0,
                ..admission::Policy::default()
            },
            ..Config::default()
        };
        fs::write(
            root.join(".local/storage-rust.json"),
            json!({"cache_roots":config.cache_roots,"archive_roots":config.archive_roots,"capacity":config.capacity})
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

#[test]
fn publication_receipt_is_independent_of_another_metadata_operation() {
    let f = Fixture::new();
    let root = f.dir.path();
    fs::create_dir_all(&f.config.cache_roots[0]).unwrap();
    let source = root.join("completed-output.bin");
    fs::write(&source, b"cel").unwrap();
    let control = admission::control(root, &f.config).unwrap();
    let _metadata_guard = admission::lock(&control).unwrap();
    let target = recover(root, &source, &f.config.cache_roots[0], &f.oid, 3).unwrap();
    verify(&target, &f.oid, 3).unwrap();
    let journal = fs::read_to_string(root.join(".local/cache-promotions.jsonl")).unwrap();
    let row: Value = serde_json::from_str(journal.trim()).unwrap();
    assert_eq!(row["sha256"], f.oid);
    assert_eq!(row["sha256_verified"], true);
}

#[cfg(windows)]
#[test]
fn installed_runtime_job_retains_output_and_rejects_changed_tools_and_failures() {
    let f = Fixture::new();
    let root = f.dir.path();
    for cache in &f.config.cache_roots {
        fs::create_dir_all(cache).unwrap();
    }
    let tool = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let asset = ingest(root, &tool).unwrap();
    let register = root.join("registration.json");
    let mut row = asset.clone();
    row["provenance"] = json!("Synthetic installed-runtime fixture");
    row["rights"] = json!("Local test executable; no distribution");
    row["selection_state"] = json!("synthetic-test");
    fs::write(&register, json!({"assets":[row]}).to_string()).unwrap();
    registry::register(root, &register).unwrap();
    let mut job = json!({"budget_bytes":1048576,"scratch_bytes":65536,
        "inputs":{},"tools":{},"installed_tools":{"runtime":{"path":tool,"sha256":asset["sha256"],"bytes":asset["bytes"]}},
        "steps":[{"tool":"runtime","args":["-NoProfile","-Command","Write-Output synthetic"],"timeout_seconds":5}],
        "outputs":{"log":{"path":"step-0.log","max_bytes":4096}}});
    let manifest = root.join("job.json");
    fs::write(&manifest, job.to_string()).unwrap();
    {
        let control = admission::control(root, &f.config).unwrap();
        let _guard = admission::lock(&control).unwrap();
        assert!(
            jobs::run(root, &manifest)
                .unwrap_err()
                .to_string()
                .contains("Job not started; admission:")
        );
    }
    let result = jobs::run(root, &manifest).unwrap();
    let log = PathBuf::from(result["outputs"]["log"]["path"].as_str().unwrap());
    assert!(fs::read_to_string(log).unwrap().contains("synthetic"));
    job["installed_tools"]["runtime"]["sha256"] = json!("0".repeat(64));
    fs::write(&manifest, job.to_string()).unwrap();
    assert!(
        jobs::run(root, &manifest)
            .unwrap_err()
            .to_string()
            .contains("registered")
    );
    job["installed_tools"]["runtime"]["sha256"] = asset["sha256"].clone();
    job["installed_tools"]["runtime"]["path"] = json!(root.join("absent.exe"));
    fs::write(&manifest, job.to_string()).unwrap();
    assert!(jobs::run(root, &manifest).is_err());
    job["installed_tools"]["runtime"]["path"] = json!(tool);
    job["steps"][0]["args"] = json!(["-NoProfile", "-Command", "exit 7"]);
    fs::write(&manifest, job.to_string()).unwrap();
    assert!(jobs::run(root, &manifest).is_err());
    job["steps"][0]["args"] = json!(["-NoProfile", "-Command", "Start-Sleep -Seconds 10"]);
    job["steps"][0]["timeout_seconds"] = json!(1);
    fs::write(&manifest, job.to_string()).unwrap();
    assert!(
        jobs::run(root, &manifest)
            .unwrap_err()
            .to_string()
            .contains("timed out")
    );
}
