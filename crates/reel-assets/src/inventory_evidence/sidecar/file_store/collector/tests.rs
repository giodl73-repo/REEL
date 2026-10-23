use super::*;
use crate::tiering::tests::Fixture;

pub(super) fn setup() -> (Fixture, Request) {
    let mut f = Fixture::new();
    f.state.coverage_complete = true;
    f.save();
    f.baseline();
    fs::write(
        f.root().join("policy.json"),
        serde_json::to_vec(&f.policy).unwrap(),
    )
    .unwrap();
    let cache = f.root().join("fast").canonicalize().unwrap();
    for name in ["index-staging", "index-retained"] {
        fs::create_dir(cache.join(name)).unwrap();
    }
    let request = Request {
        schema: "reel.collector-protection-scope.v1".into(),
        caches: vec![CacheScope {
            cache: cache.clone(),
            staging_parent: cache.join("index-staging"),
            retained_parent: cache.join("index-retained"),
            catalogs: vec![],
            job_parent: None,
        }],
        consumers: vec![Consumer {
            root: f.root().canonicalize().unwrap(),
            policy: f.root().join("policy.json"),
        }],
        limits: Limits::default(),
    };
    (f, request)
}

#[cfg(windows)]
#[test]
fn enrolled_staging_is_held_and_invalid_fence_identity_never_becomes_empty_scope() {
    let (_fixture, request) = setup();
    let scope = &request.caches[0];
    let directory = scope.cache.join("publication-fence");
    fs::create_dir(&directory).unwrap();
    let domain = fence::Domain::initialize(&directory).unwrap();
    let mut writer = FileStore::begin_fenced(
        &scope.cache,
        &scope.staging_parent,
        "enrolled",
        super::super::Limits::default(),
        &domain,
    )
    .unwrap();
    let node = writer.publish(b"enrolled stage").unwrap();
    let captured = capture(&request).unwrap();
    assert!(matches!(
        captured.classify(&node.sha256).unwrap(),
        Membership::Held(_)
    ));
    drop((writer, domain));
    let control = directory.join("asset-publication-fence.v1.lock");
    fs::rename(&control, scope.cache.join("old-publication-control")).unwrap();
    assert!(capture(&request).is_err());
    assert!(captured.revalidate().is_err());
    let _replacement = fence::Domain::initialize(&directory).unwrap();
    assert!(capture(&request).is_err());
    assert!(captured.revalidate().is_err());
    assert!(crate::object(&scope.cache, &node.sha256).exists());
}

#[cfg(windows)]
#[test]
fn original_source_demand_overlaps_build_handoff_and_retirement_without_selecting_observed_media() {
    let (mut fixture, request) = setup();
    let observed = fixture.put(b"inventory observation only", false);
    fixture.baseline();
    let raw = fs::read(fixture.root().join("baseline.json")).unwrap();
    let blob = Blob {
        sha256: hash(&raw),
        bytes: raw.len() as u64,
    };
    let source = Source {
        encoded: blob.clone(),
        decoded: blob,
        encoding: Encoding::Identity,
        schema: SOURCE_SCHEMA.into(),
        project_id: "fixture".into(),
        authority_namespace: "fixture".into(),
        adapter_version: ADAPTER_VERSION,
    };
    let scope = &request.caches[0];
    let directory = scope.cache.join("publication-fence");
    fs::create_dir(&directory).unwrap();
    let domain = fence::Domain::initialize(&directory).unwrap();
    let mut writer = FileStore::begin_fenced_source(
        &scope.cache,
        &scope.staging_parent,
        "source",
        super::super::Limits::default(),
        &domain,
        &source,
    )
    .unwrap();
    // Demand does not establish custody: the original isn't in this CAS at all.
    assert!(!crate::object(&scope.cache, &source.encoded.sha256).exists());
    let before_build = capture(&request).unwrap();
    assert!(
        matches!(before_build.classify(&source.encoded.sha256).unwrap(), Membership::Held(reasons) if reasons.contains(&Reason::RequiredSource))
    );
    assert!(matches!(
        before_build.classify(&observed).unwrap(),
        Membership::OutsideDeclaredScope
    ));
    let prepared = prepare(
        raw.as_slice(),
        &source,
        crate::inventory_evidence::Limits::default(),
    )
    .unwrap();
    let root =
        super::super::super::publish(&mut writer, &prepared, BuildLimits::default()).unwrap();
    writer.seal(&root).unwrap();
    writer
        .handoff(&scope.retained_parent, &root, &source)
        .unwrap();
    drop(writer);
    let mut recovered = FileStore::resume(&scope.cache, &scope.staging_parent, "source").unwrap();
    let plan = recovered
        .plan_retirement(&scope.retained_parent, &root, &source)
        .unwrap();
    recovered.apply_retirement(&plan, usize::MAX).unwrap();
    assert_eq!(recovered.staged_nodes(), 0);
    let after = capture(&request).unwrap();
    assert!(
        matches!(after.classify(&source.encoded.sha256).unwrap(), Membership::Held(reasons) if reasons.contains(&Reason::RequiredSource))
    );
    assert!(
        matches!(after.classify(&root.sha256).unwrap(), Membership::Held(reasons) if reasons.contains(&Reason::RetainedIndex))
    );
    assert!(matches!(
        after.classify(&observed).unwrap(),
        Membership::OutsideDeclaredScope
    ));
    let path = recovered.staging_path().join("session.json");
    let original = fs::read(&path).unwrap();
    let mut damaged: Value = serde_json::from_slice(&original).unwrap();
    damaged["source_demand"] = json!({});
    fs::write(&path, serde_json::to_vec(&damaged).unwrap()).unwrap();
    assert!(capture(&request).is_err());
    assert!(after.revalidate().is_err());
    fs::write(path, original).unwrap();
    capture(&request).unwrap();
}
#[test]
fn unprotected_rows_keep_reference_proposals_and_shadow_is_not_an_executable_plan() {
    let (mut f, request) = setup();
    let h = f.put(b"historical", true);
    let lock = f.version("old", &h);
    f.policy["history_versions"] = json!([lock]);
    f.baseline();
    fs::write(
        f.root().join("policy.json"),
        serde_json::to_vec(&f.policy).unwrap(),
    )
    .unwrap();
    let report = compare(
        f.root(),
        &f.root().join("policy.json"),
        &f.root().join("baseline.json"),
        &request,
        1000,
    )
    .unwrap();
    assert_eq!(report.summary().held_proposals, 0);
    assert_eq!(report.baseline()["objects"], report.shadow()["objects"]);
    assert_eq!(
        report.shadow()["schema"],
        "reel.asset-collector-shadow-plan.v1"
    );
    fs::write(
        f.root().join("shadow.json"),
        serde_json::to_vec(report.shadow()).unwrap(),
    )
    .unwrap();
    assert!(
        tiering::check_plan(
            f.root(),
            &f.root().join("policy.json"),
            &f.root().join("shadow.json"),
            1001
        )
        .is_err()
    );
}
#[test]
fn live_session_staging_is_protection_without_requiring_its_owner_lock() {
    let (f, request) = setup();
    let scope = &request.caches[0];
    let mut writer = FileStore::begin(
        &scope.cache,
        &scope.staging_parent,
        "live",
        super::super::Limits::default(),
    )
    .unwrap();
    let blob = writer.publish(b"active metadata node").unwrap();
    let snapshot = capture(&request).unwrap();
    assert!(
        matches!(snapshot.classify(&blob.sha256).unwrap(),Membership::Held(reasons) if reasons.contains(&Reason::Staging))
    );
    assert!(writer.publish(b"other node").is_ok());
    assert!(snapshot.revalidate().is_err());
    assert!(!f.root().join("offline-cold").exists());
}
#[test]
fn partial_staged_bytes_are_a_hold_not_a_verified_backup() {
    let (_f, request) = setup();
    let scope = &request.caches[0];
    let writer = FileStore::begin(
        &scope.cache,
        &scope.staging_parent,
        "partial",
        super::super::Limits::default(),
    )
    .unwrap();
    let id = hash(b"complete metadata");
    fs::write(writer.staging_path().join("objects").join(&id), b"part").unwrap();
    let snapshot = capture(&request).unwrap();
    assert!(matches!(
        snapshot.classify(&id).unwrap(),
        Membership::Held(_)
    ));
    assert!(FileStore::resume(&scope.cache, &scope.staging_parent, "partial").is_err()); // live ownership stays intact
}
#[test]
fn incomplete_or_missing_provider_controls_fail_without_an_empty_snapshot() {
    for mode in [
        "missing-intent",
        "bad-intent",
        "unknown-file",
        "pending-retention",
    ] {
        let (_f, request) = setup();
        let scope = &request.caches[0];
        let writer = FileStore::begin(
            &scope.cache,
            &scope.staging_parent,
            "broken",
            super::super::Limits::default(),
        )
        .unwrap();
        match mode {
            "missing-intent" => {
                fs::remove_file(writer.staging_path().join("session.json")).unwrap()
            }
            "bad-intent" => {
                fs::write(writer.staging_path().join("session.json"), b"partial").unwrap()
            }
            "unknown-file" => fs::write(writer.staging_path().join("unrecognized"), b"x").unwrap(),
            _ => fs::create_dir(
                scope
                    .retained_parent
                    .join(hash(b"unpublished retained root")),
            )
            .unwrap(),
        }
        assert!(capture(&request).is_err(), "{mode}");
    }
}
#[test]
fn staging_hold_restricts_unknown_cold_proposal_without_refilling_the_batch() {
    let (mut f, request) = setup();
    let scope = &request.caches[0];
    let mut writer = FileStore::begin(
        &scope.cache,
        &scope.staging_parent,
        "held",
        super::super::Limits::default(),
    )
    .unwrap();
    let blob = writer.publish(b"unregistered staged").unwrap();
    f.baseline();
    fs::write(
        f.root().join("policy.json"),
        serde_json::to_vec(&f.policy).unwrap(),
    )
    .unwrap();
    let report = compare(
        f.root(),
        &f.root().join("policy.json"),
        &f.root().join("baseline.json"),
        &request,
        1000,
    )
    .unwrap();
    let row = |plan: &Value| {
        plan["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["sha256"] == blob.sha256)
            .unwrap()["proposal"]
            .clone()
    };
    assert_eq!(row(report.baseline()), "register-and-archive-first");
    assert_eq!(row(report.shadow()), "hold");
    assert_eq!(report.summary().held_proposals, 1);
}
#[test]
fn native_job_leases_and_incomplete_graph_coverage_are_not_inferred_from_inventory() {
    let (mut f, request) = setup();
    let h = f.put(b"leased primary", true);
    f.version("job-input", &h);
    f.state
        .leases
        .insert("job".into(), BTreeSet::from(["job-input@v1".into()]));
    f.save();
    let snapshot = capture(&request).unwrap();
    assert!(
        matches!(snapshot.classify(&h).unwrap(),Membership::Held(reasons) if reasons.contains(&Reason::JobLease))
    );
    f.state.coverage_complete = false;
    f.save();
    assert!(capture(&request).is_err());
    assert!(snapshot.revalidate().is_err());
}
#[test]
fn unknown_subjects_never_return_delete_permission_and_budgets_reject() {
    let (_f, request) = setup();
    let snapshot = capture(&request).unwrap();
    assert!(matches!(
        snapshot.classify(&hash(b"outside")).unwrap(),
        Membership::OutsideDeclaredScope
    ));
    assert!(snapshot.classify("invalid").is_err());
    let scope = &request.caches[0];
    let mut writer = FileStore::begin(
        &scope.cache,
        &scope.staging_parent,
        "budget-demand",
        super::super::Limits::default(),
    )
    .unwrap();
    writer.publish(b"bounded hold").unwrap();
    for limits in [
        Limits {
            files: 0,
            ..Limits::default()
        },
        Limits {
            metadata_bytes: 1,
            ..Limits::default()
        },
        Limits {
            holds: 0,
            ..Limits::default()
        },
    ] {
        let mut limited = request.clone();
        limited.limits = limits;
        assert!(capture(&limited).is_err());
    }
}

fn indexed(f: &Fixture, request: &mut Request) -> (FileStore, Source, Blob, catalog::Catalog) {
    let raw = fs::read(f.root().join("baseline.json")).unwrap();
    let blob = Blob {
        sha256: hash(&raw),
        bytes: raw.len() as u64,
    };
    let source = Source {
        encoded: blob.clone(),
        decoded: blob,
        encoding: Encoding::Identity,
        schema: SOURCE_SCHEMA.into(),
        project_id: "fixture".into(),
        authority_namespace: "collector-fixture".into(),
        adapter_version: ADAPTER_VERSION,
    };
    let prepared = prepare(
        raw.as_slice(),
        &source,
        crate::inventory_evidence::Limits::default(),
    )
    .unwrap();
    let scope = &mut request.caches[0];
    let mut writer = FileStore::begin(
        &scope.cache,
        &scope.staging_parent,
        "index",
        super::super::Limits::default(),
    )
    .unwrap();
    let root =
        super::super::super::publish(&mut writer, &prepared, BuildLimits::default()).unwrap();
    writer.seal(&root).unwrap();
    writer
        .handoff(&scope.retained_parent, &root, &source)
        .unwrap();
    let catalog_dir = scope.cache.join("index-catalog");
    fs::create_dir(&catalog_dir).unwrap();
    let mut cfg = crate::config(f.root()).unwrap();
    let coord = f.root().join("index-control");
    fs::create_dir(&coord).unwrap();
    cfg.coordination_root = Some(coord);
    let context = catalog::MutationContext {
        project_root: f.root(),
        storage: &cfg,
        overhead_bytes: 64 * 1024,
    };
    let expected = catalog::Scope {
        project_id: "fixture".into(),
        authority_namespace: "collector-fixture".into(),
        catalog_id: hash(b"catalog"),
    };
    let catalog =
        catalog::Catalog::initialize(&context, &scope.cache, &catalog_dir, &expected).unwrap();
    catalog.create_key(&context, "current").unwrap();
    let candidate = catalog::Candidate::verify(
        &scope.cache,
        &scope.retained_parent,
        &root,
        &source,
        ReadLimits::default(),
    )
    .unwrap();
    catalog.admit(&context, "current", 0, &candidate).unwrap();
    scope.catalogs.push(CatalogScope {
        directory: catalog_dir,
        project_id: expected.project_id,
        authority_namespace: expected.authority_namespace,
        catalog_id: expected.catalog_id,
    });
    (writer, source, root, catalog)
}
#[test]
fn catalog_and_retained_history_protect_metadata_but_not_observed_media_rows() {
    let (mut f, mut request) = setup();
    let observed = f.put(b"observed but unregistered", false);
    f.baseline();
    fs::write(
        f.root().join("policy.json"),
        serde_json::to_vec(&f.policy).unwrap(),
    )
    .unwrap();
    let (mut writer, source, root, catalog) = indexed(&f, &mut request);
    let snapshot = capture(&request).unwrap();
    assert!(
        matches!(snapshot.classify(&root.sha256).unwrap(),Membership::Held(reasons)
        if reasons.contains(&Reason::CatalogIndex) && reasons.contains(&Reason::RetainedIndex) && reasons.contains(&Reason::Staging))
    );
    assert!(matches!(
        snapshot.classify(&observed).unwrap(),
        Membership::OutsideDeclaredScope
    ));
    assert!(
        matches!(snapshot.classify(&source.encoded.sha256).unwrap(),Membership::Held(reasons) if reasons.contains(&Reason::RequiredSource))
    );
    let plan = writer
        .plan_retirement(&request.caches[0].retained_parent, &root, &source)
        .unwrap();
    writer.apply_retirement(&plan, 100).unwrap();
    assert!(snapshot.revalidate().is_err());
    let mut cfg = crate::config(f.root()).unwrap();
    cfg.coordination_root = Some(f.root().join("index-control"));
    catalog
        .tombstone(
            &catalog::MutationContext {
                project_root: f.root(),
                storage: &cfg,
                overhead_bytes: 64 * 1024,
            },
            "current",
            1,
        )
        .unwrap();
    let historical = capture(&request).unwrap();
    assert!(
        matches!(historical.classify(&root.sha256).unwrap(),Membership::Held(reasons)
        if reasons.contains(&Reason::RetainedIndex) && !reasons.contains(&Reason::CatalogIndex) && !reasons.contains(&Reason::Staging))
    );
}
#[test]
fn corrupt_or_missing_retained_metadata_blocks_combined_capture() {
    for failure in [
        "missing-node",
        "corrupt-node",
        "unknown-source",
        "extra-file",
    ] {
        let (f, mut request) = setup();
        let (_writer, _source, root, _catalog) = indexed(&f, &mut request);
        let path = request.caches[0].retained_parent.join(&root.sha256);
        match failure {
            "missing-node" => fs::remove_file(path.join("objects").join(&root.sha256)).unwrap(),
            "corrupt-node" => {
                let file = path.join("objects").join(&root.sha256);
                fs::remove_file(&file).unwrap();
                fs::write(file, b"corrupt").unwrap();
            }
            "unknown-source" => {
                let file = path.join("root.json");
                let mut value: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
                value["source"]["adapter_version"] = json!(99);
                fs::write(file, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            _ => fs::write(path.join("unknown-file"), b"extra").unwrap(),
        }
        assert!(capture(&request).is_err(), "{failure}");
    }
}
#[test]
fn graph_and_catalog_budgets_accumulate_across_staging_retained_and_current_views() {
    let (f, mut request) = setup();
    let (_writer, _source, _root, _catalog) = indexed(&f, &mut request);
    for limits in [
        Limits {
            graph_nodes: 5,
            ..Limits::default()
        },
        Limits {
            catalog_keys: 0,
            ..Limits::default()
        },
        Limits {
            data_bytes: 1,
            ..Limits::default()
        },
        Limits {
            retained_roots: 0,
            ..Limits::default()
        },
        Limits {
            sessions: 0,
            ..Limits::default()
        },
    ] {
        let mut limited = request.clone();
        limited.limits = limits;
        assert!(capture(&limited).is_err());
    }
}
#[test]
fn another_consumer_primary_or_job_hold_prevents_reference_history_demotion() {
    for lease in [false, true] {
        let (mut owner, mut request) = setup();
        let h = owner.put(b"shared input", true);
        let lock = owner.version("old-use", &h);
        owner.policy["history_versions"] = json!([lock]);
        owner.baseline();
        fs::write(
            owner.root().join("policy.json"),
            serde_json::to_vec(&owner.policy).unwrap(),
        )
        .unwrap();
        let mut other = Fixture::new();
        let same = other.put(b"shared input", true);
        assert_eq!(h, same);
        other.version("live-input", &h);
        other.state.coverage_complete = true;
        if lease {
            other
                .state
                .leases
                .insert("render".into(), BTreeSet::from(["live-input@v1".into()]));
        } else {
            other.state.current.insert("live-input".into(), "v1".into());
        }
        other.save();
        // Explicitly share the owner's caches, as independent worktrees do.
        let mut cfg: Value = serde_json::from_slice(
            &fs::read(other.root().join(".local/storage-rust.json")).unwrap(),
        )
        .unwrap();
        cfg["cache_roots"] = json!([
            owner.root().join("fast").canonicalize().unwrap(),
            owner.root().join("history").canonicalize().unwrap()
        ]);
        fs::write(
            other.root().join(".local/storage-rust.json"),
            serde_json::to_vec(&cfg).unwrap(),
        )
        .unwrap();
        other.baseline();
        let policy = other.root().join("policy.json");
        fs::write(&policy, serde_json::to_vec(&other.policy).unwrap()).unwrap();
        request.consumers.push(Consumer {
            root: other.root().canonicalize().unwrap(),
            policy,
        });
        let result = compare(
            owner.root(),
            &owner.root().join("policy.json"),
            &owner.root().join("baseline.json"),
            &request,
            1000,
        )
        .unwrap();
        let proposal = |p: &Value| {
            p["objects"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["sha256"] == h)
                .unwrap()["proposal"]
                .clone()
        };
        assert_eq!(proposal(result.baseline()), "verify-and-demote");
        assert_eq!(proposal(result.shadow()), "hold");
        assert_eq!(result.shadow()["candidate_batch_objects"], 0);
    }
}
#[test]
fn combined_coverage_never_refills_a_freed_batch_slot_or_changes_unrelated_rows() {
    let (mut f, request) = setup();
    let first = f.put(b"first old", true);
    let second = f.put(b"second old", true);
    let a = f.version("first", &first);
    let b = f.version("second", &second);
    f.policy["history_versions"] = json!([a, b]);
    f.policy["max_batch_objects"] = json!(1);
    f.baseline();
    fs::write(
        f.root().join("policy.json"),
        serde_json::to_vec(&f.policy).unwrap(),
    )
    .unwrap();
    let before = tiering::plan(
        f.root(),
        &f.root().join("policy.json"),
        &f.root().join("baseline.json"),
        1000,
    )
    .unwrap();
    let selected = before["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["proposal"] == "verify-and-demote")
        .unwrap()["sha256"]
        .as_str()
        .unwrap();
    let scope = &request.caches[0];
    let writer = FileStore::begin(
        &scope.cache,
        &scope.staging_parent,
        "pin",
        super::super::Limits::default(),
    )
    .unwrap();
    // A staged input demand can retain an already observed CAS subject too.
    fs::write(
        writer.staging_path().join("objects").join(selected),
        b"in flight",
    )
    .unwrap();
    let result = compare(
        f.root(),
        &f.root().join("policy.json"),
        &f.root().join("baseline.json"),
        &request,
        1000,
    )
    .unwrap();
    assert_eq!(result.shadow()["candidate_batch_objects"], 0);
    assert_eq!(result.summary().held_proposals, 1);
    for original in result.baseline()["objects"].as_array().unwrap() {
        if original["sha256"] != selected {
            let actual = result.shadow()["objects"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["sha256"] == original["sha256"])
                .unwrap();
            assert_eq!(actual, original);
        }
    }
}

fn job_fixture(request: &mut Request) -> (PathBuf, String, String) {
    let parent = request.caches[0].cache.join("native-jobs");
    fs::create_dir(&parent).unwrap();
    request.caches[0].job_parent = Some(parent.clone());
    let workspace = parent.join("job-123-456");
    fs::create_dir(&workspace).unwrap();
    let input = hash(b"primary cel");
    let output = hash(b"compiled output");
    let job = json!({"budget_bytes":4096,"scratch_bytes":0,"inputs":{"cel.bin":{"sha256":input,"bytes":11}},
        "tools":{"tool.bin":{"sha256":hash(b"tool"),"bytes":4}},"files":{},"steps":[{"tool":"tool.bin","args":[],"timeout_seconds":1}],
        "outputs":{"preview":{"path":"out.bin","max_bytes":1}}});
    let raw = serde_json::to_vec(&job).unwrap();
    fs::write(workspace.join("job.json"), &raw).unwrap();
    fs::write(workspace.join("result.json"),json!({"schema":"caminos.production-job-result.v1","job_id":"job-123-456",
        "manifest_sha256":hash(&raw),"cache_root":request.caches[0].cache,"workspace":workspace,
        "outputs":{"preview":{"sha256":output,"bytes":15,"path":"THIS_DATA_PATH_MUST_NEVER_BE_OPENED"}},"workspace_retained":true}).to_string()).unwrap();
    (workspace, input, output)
}
#[test]
fn native_workspace_inputs_and_published_outputs_are_demands_without_running_or_reading_payloads() {
    let (_f, mut request) = setup();
    let (workspace, input, output) = job_fixture(&mut request);
    let snapshot = capture(&request).unwrap();
    assert!(
        matches!(snapshot.classify(&input).unwrap(),Membership::Held(reasons) if reasons.contains(&Reason::JobWorkspaceInput))
    );
    assert!(
        matches!(snapshot.classify(&output).unwrap(),Membership::Held(reasons) if reasons.contains(&Reason::JobWorkspaceOutput) && !reasons.contains(&Reason::JobWorkspaceInput))
    );
    assert!(!workspace.join("cel.bin").exists());
    assert!(!workspace.join("tool.bin").exists());
    assert!(!workspace.join("out.bin").exists());
    fs::remove_file(workspace.join("result.json")).unwrap();
    assert!(snapshot.revalidate().is_err());
}
#[test]
fn native_job_receipt_scope_or_manifest_errors_block_capture() {
    for case in ["manifest", "scope", "schema", "request"] {
        let (_f, mut request) = setup();
        let (workspace, _input, _output) = job_fixture(&mut request);
        if case == "request" {
            fs::write(workspace.join("job.json"), b"partial").unwrap();
        } else {
            let path = workspace.join("result.json");
            let mut result: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            match case {
                "manifest" => result["manifest_sha256"] = json!(hash(b"another request")),
                "scope" => result["workspace"] = json!(request.caches[0].cache),
                _ => result["schema"] = json!("unknown"),
            }
            fs::write(path, serde_json::to_vec(&result).unwrap()).unwrap();
        }
        assert!(capture(&request).is_err(), "{case}");
    }
}
