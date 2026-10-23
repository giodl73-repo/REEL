//! Real Windows execution in synthetic active caches, without production assets.
use super::*;
use crate::inventory_evidence::sidecar::file_store::collector::{self, fence::Mode};
use crate::tiering::tests::Fixture;
use std::{process::Command, sync::OnceLock};

fn enrolled(mode: &str) -> (Fixture, PathBuf, Value, Domain) {
    let mut fixture = Fixture::new();
    fixture.state.coverage_complete = true;
    fixture.save();
    let (manifest, mut job) = prepare(&fixture, mode);
    job["retain_workspace"] = json!(true);
    if mode == "mutate" {
        job["writable_inputs"] = json!(["in.bin"]);
    }
    fs::write(&manifest, job.to_string()).unwrap();
    let directory = fixture.root().join("fast/fence");
    fs::create_dir(&directory).unwrap();
    let domain = Domain::initialize(&directory).unwrap();
    (fixture, manifest, job, domain)
}
fn job_scope(fixture: &mut Fixture) -> collector::Request {
    fixture.baseline();
    fs::write(
        fixture.root().join("policy.json"),
        fixture.policy.to_string(),
    )
    .unwrap();
    let cache = fixture.root().join("fast").canonicalize().unwrap();
    for name in ["index-staging", "index-retained"] {
        fs::create_dir_all(cache.join(name)).unwrap();
    }
    collector::Request {
        schema: "reel.collector-protection-scope.v1".into(),
        caches: vec![collector::CacheScope {
            cache: cache.clone(),
            staging_parent: cache.join("index-staging"),
            retained_parent: cache.join("index-retained"),
            catalogs: vec![],
            job_parent: Some(cache.join("staging/caminos")),
        }],
        consumers: vec![collector::Consumer {
            root: fixture.root().canonicalize().unwrap(),
            policy: fixture.root().join("policy.json"),
        }],
        limits: collector::Limits::default(),
    }
}
fn only_workspace(fixture: &Fixture) -> PathBuf {
    let mut entries = fs::read_dir(fixture.root().join("fast/staging/caminos")).unwrap();
    let workspace = entries.next().unwrap().unwrap().path();
    assert!(entries.next().is_none());
    workspace
}

#[test]
fn enrolled_job_publishes_distinct_input_output_demand_and_count_only_summary() {
    let (mut fixture, manifest, job, domain) = enrolled("mutate");
    let result = run_fenced(fixture.root(), &manifest, &domain).unwrap();
    let workspace = PathBuf::from(result["workspace"].as_str().unwrap());
    assert!(workspace.join("job-publication.json").exists());
    assert!(workspace.join("job-output-intent.json").exists());
    assert_eq!(result["workspace_retained"], true);
    let input = job["inputs"]["in.bin"]["sha256"].as_str().unwrap();
    let output = result["outputs"]["copy"]["sha256"].as_str().unwrap();
    assert_ne!(input, output);
    let scope = job_scope(&mut fixture);
    let snapshot = collector::capture(&scope).unwrap();
    assert!(
        matches!(snapshot.classify(input).unwrap(),collector::Membership::Held(reasons) if reasons.contains(&collector::Reason::JobWorkspaceInput))
    );
    assert!(
        matches!(snapshot.classify(output).unwrap(),collector::Membership::Held(reasons) if reasons.contains(&collector::Reason::JobWorkspaceOutput) && !reasons.contains(&collector::Reason::JobWorkspaceInput))
    );
    let summary = publication_summary(&result).unwrap();
    for (key, value) in summary.as_object().unwrap() {
        assert!(key == "schema" || value.is_number() || value.is_boolean());
    }
    assert!(!summary.to_string().contains(output));
    assert!(!summary.to_string().contains(input));
    let mut bad = result.clone();
    bad["linked_input_bytes"] = json!("PRIVATE_BAD_COUNTER");
    assert!(publication_summary(&bad).is_err());
    fs::remove_file(workspace.join("result.json")).unwrap();
    assert!(snapshot.revalidate().is_err());
    let intent_only = collector::capture(&scope).unwrap();
    assert!(matches!(
        intent_only.classify(output).unwrap(),
        collector::Membership::Held(_)
    ));
    assert_eq!(
        fs::read(object(&fixture.root().join("fast"), input)).unwrap(),
        b"source"
    );
}

#[test]
fn enrolled_input_fence_refusal_precedes_workspace_creation_and_input_access() {
    let (fixture, manifest, job, domain) = enrolled("copy");
    let input = job["inputs"]["in.bin"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let _held = domain
        .acquire(std::slice::from_ref(&input), Mode::Remove, Duration::ZERO)
        .unwrap();
    for cache in ["fast", "history"] {
        fs::remove_file(object(&fixture.root().join(cache), &input)).unwrap();
    }
    let error = run_fenced(fixture.root(), &manifest, &domain)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Publication fence unavailable"), "{error}");
    no_scratch(&fixture);
}

#[test]
fn enrolled_missing_input_keeps_demand_without_a_lifetime_lock_or_custody_claim() {
    let (mut fixture, manifest, job, domain) = enrolled("copy");
    let input = job["inputs"]["in.bin"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    for cache in ["fast", "history"] {
        fs::remove_file(object(&fixture.root().join(cache), &input)).unwrap();
    }
    assert!(run_fenced(fixture.root(), &manifest, &domain).is_err());
    let workspace = only_workspace(&fixture);
    assert!(workspace.join("job-publication.json").exists());
    assert!(!workspace.join("in.bin").exists());
    domain
        .acquire(std::slice::from_ref(&input), Mode::Remove, Duration::ZERO)
        .unwrap();
    let snapshot = collector::capture(&job_scope(&mut fixture)).unwrap();
    assert!(matches!(
        snapshot.classify(&input).unwrap(),
        collector::Membership::Held(_)
    ));
}

#[test]
fn enrolled_output_fence_refuses_before_cas_publication_and_explicit_retry_can_succeed() {
    let (fixture, manifest, _job, domain) = enrolled("mutate");
    let output = format!("{:x}", Sha256::digest(b"changed"));
    let held = domain
        .acquire(std::slice::from_ref(&output), Mode::Remove, Duration::ZERO)
        .unwrap();
    let error = run_fenced(fixture.root(), &manifest, &domain)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Publication fence unavailable"), "{error}");
    let workspace = only_workspace(&fixture);
    assert_eq!(fs::read(workspace.join("out.bin")).unwrap(), b"changed");
    assert!(!workspace.join("job-output-intent.json").exists());
    assert!(!object(&fixture.root().join("fast"), &output).exists());
    assert!(!object(&fixture.root().join("history"), &output).exists());
    drop(held);
    let result = run_fenced(fixture.root(), &manifest, &domain).unwrap();
    assert_eq!(result["outputs"]["copy"]["sha256"], output);
    assert!(workspace.exists()); // the first failed job is never auto-cleaned/replayed
}

#[test]
fn enrolled_collection_rejects_damaged_controls_and_replaced_domain() {
    for case in 0..5 {
        let (mut fixture, manifest, _job, domain) = enrolled("copy");
        let result = run_fenced(fixture.root(), &manifest, &domain).unwrap();
        let workspace = PathBuf::from(result["workspace"].as_str().unwrap());
        let scope = job_scope(&mut fixture);
        let snapshot = collector::capture(&scope).unwrap();
        match case {
            0 => fs::remove_file(workspace.join("job-publication.json")).unwrap(),
            1 => fs::write(workspace.join("job-output-intent.json"), b"{}").unwrap(),
            2 => {
                let mut changed = result.clone();
                changed["outputs"]["copy"]["bytes"] = json!(999);
                fs::write(workspace.join("result.json"), changed.to_string()).unwrap();
            }
            3 => {
                let path = workspace.join("job-publication.json");
                let mut changed: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                changed["manifest_sha256"] = json!("e".repeat(64));
                fs::write(path, changed.to_string()).unwrap();
            }
            _ => {
                drop(domain);
                fs::rename(
                    fixture
                        .root()
                        .join("fast/fence/asset-publication-fence.v1.lock"),
                    fixture.root().join("old-domain"),
                )
                .unwrap();
                let _replacement = Domain::initialize(&fixture.root().join("fast/fence")).unwrap();
            }
        }
        assert!(collector::capture(&scope).is_err(), "case {case}");
        assert!(snapshot.revalidate().is_err());
    }
}

#[test]
fn enrolled_jobs_require_control_budget_in_both_retention_modes() {
    let (fixture, manifest, mut job, domain) = enrolled("copy");
    job["scratch_bytes"] = json!(0);
    job["budget_bytes"] = json!(tool().len() as u64 + 6 + 2048);
    for retained in [false, true] {
        job["retain_workspace"] = json!(retained);
        fs::write(&manifest, job.to_string()).unwrap();
        assert!(
            run_fenced(fixture.root(), &manifest, &domain)
                .unwrap_err()
                .to_string()
                .contains("publication controls")
        );
    }
    no_scratch(&fixture);
}

#[test]
fn enrolled_automatic_success_publishes_evidence_and_retires_only_workspace() {
    let (fixture, manifest, mut job, domain) = enrolled("copy");
    job["retain_workspace"] = json!(false);
    fs::write(&manifest, job.to_string()).unwrap();
    let result = run_fenced(fixture.root(), &manifest, &domain).unwrap();
    assert_eq!(result["workspace_retained"], false);
    let summary = publication_summary(&result).unwrap();
    assert_eq!(summary["retirement_completed"], true);
    let retained = evidence(
        &fixture,
        result["retirement_evidence"]["sha256"].as_str().unwrap(),
    );
    assert_eq!(
        serde_json::from_str::<Value>(retained["request_utf8"].as_str().unwrap()).unwrap(),
        job
    );
    assert_eq!(
        fs::read(result["outputs"]["copy"]["path"].as_str().unwrap()).unwrap(),
        b"source"
    );
    assert!(
        selected(fixture.root())
            .unwrap()
            .contains_key(result["retirement_evidence"]["sha256"].as_str().unwrap())
    );
    no_scratch(&fixture);
}

#[test]
fn enrolled_automatic_failure_salvages_declared_output_and_preserves_private_diagnostics() {
    let (fixture, manifest, mut job, domain) = enrolled("fail");
    job["retain_workspace"] = json!(false);
    fs::write(&manifest, job.to_string()).unwrap();
    let error = run_fenced(fixture.root(), &manifest, &domain)
        .unwrap_err()
        .to_string();
    assert!(error.contains("exited 23"), "{error}");
    let retained = failed_evidence(&fixture, &error);
    assert!(
        retained["diagnostics"]["step-0.log"]["text"]
            .as_str()
            .unwrap()
            .contains("synthetic failure detail")
    );
    let candidate = &retained["declared_retained_outputs"]["copy"];
    let path = object(
        &fixture.root().join("fast"),
        candidate["sha256"].as_str().unwrap(),
    );
    assert_eq!(fs::read(path).unwrap(), b"candidate");
    no_scratch(&fixture);
}

#[test]
fn enrolled_retirement_fence_refusal_precedes_salvage_publication_and_preserves_intent() {
    let (mut fixture, manifest, mut job, domain) = enrolled("fail");
    job["retain_workspace"] = json!(false);
    fs::write(&manifest, job.to_string()).unwrap();
    let candidate = format!("{:x}", Sha256::digest(b"candidate"));
    let _held = domain
        .acquire(
            std::slice::from_ref(&candidate),
            Mode::Remove,
            Duration::ZERO,
        )
        .unwrap();
    let error = run_fenced(fixture.root(), &manifest, &domain)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Publication fence unavailable"), "{error}");
    let workspace = only_workspace(&fixture);
    assert!(workspace.join("job-retirement-request.json").exists());
    assert!(!workspace.join("job-retirement-intent.json").exists());
    assert!(!object(&fixture.root().join("fast"), &candidate).exists());
    let raw = fs::read(workspace.join("job.json")).unwrap();
    let snapshot = collector::capture(&job_scope(&mut fixture)).unwrap();
    for input in protection_inputs(&raw).unwrap() {
        assert!(matches!(
            snapshot.classify(&input).unwrap(),
            collector::Membership::Held(_)
        ));
    }
}

#[test]
fn enrolled_pre_unlink_interruption_preserves_all_demand_then_verified_discard_completes() {
    let (mut fixture, manifest, mut job, domain) = enrolled("copy");
    // Prepare the same registered inputs and owned workspace without replaying
    // execution during recovery. A synthetic output is already an exact input.
    job["retain_workspace"] = json!(false);
    let raw = job.to_string().into_bytes();
    fs::write(&manifest, &raw).unwrap();
    let parsed = decode(&raw).unwrap();
    let cfg = config(fixture.root()).unwrap();
    let reservation = admission::reserve(fixture.root(), &cfg, parsed.budget_bytes).unwrap();
    let workspace = publication::prepare(&cfg, &reservation, &domain, &raw).unwrap();
    fs::write(workspace.join("out.bin"), b"source").unwrap();
    let aliases = workspace.join("temporary-aliases");
    fs::create_dir(&aliases).unwrap();
    fs::hard_link(
        object(
            &reservation.cache_root,
            job["inputs"]["in.bin"]["sha256"].as_str().unwrap(),
        ),
        aliases.join("input.bin"),
    )
    .unwrap();
    let other_directory = fixture.root().join("fast/foreign-fence");
    fs::create_dir(&other_directory).unwrap();
    let other_domain = Domain::initialize(&other_directory).unwrap();
    let output = publication::Descriptor {
        sha256: job["inputs"]["in.bin"]["sha256"].as_str().unwrap().into(),
        bytes: 6,
    };
    assert!(
        publication::outputs(
            &workspace,
            &reservation.id,
            &raw,
            &other_domain,
            BTreeMap::from([("copy".into(), output)])
        )
        .is_err()
    );
    assert!(!workspace.join("job-output-intent.json").exists());
    let failure = json!({"schema":"reel.failed-job-result.v1","job_id":reservation.id,"manifest_sha256":format!("{:x}",Sha256::digest(&raw)),"outputs":{}});
    let error = lifecycle::publish_and_retire_inner(
        fixture.root(),
        &cfg,
        &reservation.cache_root,
        &workspace,
        &raw,
        &failure,
        &parsed,
        Some(&domain),
        || {
            let intent = publication::check_retirement(
                &workspace,
                &raw,
                &fs::read(workspace.join("job-retirement-intent.json"))?,
            )?;
            let canonical = object(&reservation.cache_root, &intent.evidence.sha256);
            fs::remove_dir_all(&aliases).expect(
                "owned temporary aliases must be removable while canonical custody remains pinned",
            );
            let input_canonical = object(
                &reservation.cache_root,
                job["inputs"]["in.bin"]["sha256"].as_str().unwrap(),
            );
            assert!(
                crate::demotion::platform::open(&input_canonical, true).is_err(),
                "canonical eviction handle was not excluded"
            );
            assert!(
                fs::remove_file(canonical).is_err(),
                "custody copy was deletable before scratch unlink"
            );
            Err("injected pre-unlink interruption".into())
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("pre-unlink"));
    assert!(workspace.exists());
    drop(reservation);
    let scope = job_scope(&mut fixture);
    let snapshot = collector::capture(&scope).unwrap();
    let intent = publication::check_retirement(
        &workspace,
        &raw,
        &fs::read(workspace.join("job-retirement-intent.json")).unwrap(),
    )
    .unwrap();
    for descriptor in [&intent.manifest, &intent.evidence] {
        assert!(
            matches!(snapshot.classify(&descriptor.sha256).unwrap(),collector::Membership::Held(reasons) if reasons.contains(&collector::Reason::RegistryControl))
        );
    }
    assert!(
        matches!(snapshot.classify(&intent.outputs["copy"].sha256).unwrap(),collector::Membership::Held(reasons) if reasons.contains(&collector::Reason::JobWorkspaceOutput))
    );
    let intent_path = workspace.join("job-retirement-intent.json");
    let saved = fs::read(&intent_path).unwrap();
    fs::write(&intent_path, b"{}").unwrap();
    assert!(collector::capture(&scope).is_err());
    assert!(
        discard(
            fixture.root(),
            workspace.file_name().unwrap().to_str().unwrap(),
            &intent.manifest.sha256,
            true
        )
        .is_err()
    );
    fs::write(&intent_path, saved).unwrap();
    let evidence_path = object(&fixture.root().join("fast"), &intent.evidence.sha256);
    let evidence_bytes = fs::read(&evidence_path).unwrap();
    fs::remove_file(&evidence_path).unwrap();
    assert!(
        discard(
            fixture.root(),
            workspace.file_name().unwrap().to_str().unwrap(),
            &intent.manifest.sha256,
            true
        )
        .is_err()
    );
    assert!(workspace.exists());
    fs::write(&evidence_path, evidence_bytes).unwrap();
    let cleaned = discard(
        fixture.root(),
        workspace.file_name().unwrap().to_str().unwrap(),
        &intent.manifest.sha256,
        true,
    )
    .unwrap();
    assert_eq!(cleaned["cache_objects_removed"], 0);
    assert!(!workspace.exists());
    for descriptor in [&intent.manifest, &intent.evidence] {
        verify(
            &object(&fixture.root().join("fast"), &descriptor.sha256),
            &descriptor.sha256,
            descriptor.bytes,
        )
        .unwrap();
    }
}

#[test]
fn killed_automatic_retirement_writer_requires_reconciliation_before_explicit_discard() {
    use std::os::windows::process::CommandExt;
    use std::{process::Stdio, time::Instant};
    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let (mut fixture, manifest, mut job, _domain) = enrolled("copy");
    job["retain_workspace"] = json!(false);
    fs::write(&manifest, job.to_string()).unwrap();
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "jobs::lifecycle_tests::subprocess_retirement_commit",
                "--nocapture",
            ])
            .env("REEL_TEST_RETIREMENT_ROOT", fixture.root())
            .env("REEL_TEST_RETIREMENT_MANIFEST", &manifest)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .spawn()
            .unwrap(),
    );
    let ready = fixture.root().join(".local/retirement-ready.json");
    let started = Instant::now();
    while !ready.exists() {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "retirement fixture exited before commit boundary"
        );
        assert!(started.elapsed() < Duration::from_secs(15));
        std::thread::sleep(Duration::from_millis(5));
    }
    let record: Value = serde_json::from_slice(&fs::read(ready).unwrap()).unwrap();
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let id = record["job_id"].as_str().unwrap();
    let manifest_id = record["manifest_sha256"].as_str().unwrap();
    let workspace = fixture.root().join("fast/staging/caminos").join(id);
    assert!(workspace.exists());
    let snapshot = collector::capture(&job_scope(&mut fixture)).unwrap();
    assert!(matches!(
        snapshot.classify(manifest_id).unwrap(),
        collector::Membership::Held(_)
    ));
    // Process death releases ownership, not the explicit recorded reservation.
    assert!(discard(fixture.root(), id, manifest_id, true).is_err());
    admission::release_stale(fixture.root(), &config(fixture.root()).unwrap(), id).unwrap();
    let result = discard(fixture.root(), id, manifest_id, true).unwrap();
    assert_eq!(result["cache_objects_removed"], 0);
    assert!(!workspace.exists());
    let evidence_hash = record["evidence_sha256"].as_str().unwrap();
    assert!(
        selected(fixture.root())
            .unwrap()
            .contains_key(evidence_hash)
    );
    assert_eq!(
        evidence(&fixture, evidence_hash)["result"]["schema"],
        "reel.failed-job-result.v1"
    );
}

#[test]
fn subprocess_retirement_commit() {
    let Some(root) = std::env::var_os("REEL_TEST_RETIREMENT_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let manifest = PathBuf::from(std::env::var_os("REEL_TEST_RETIREMENT_MANIFEST").unwrap());
    let raw = fs::read(manifest).unwrap();
    let parsed = decode(&raw).unwrap();
    let cfg = config(&root).unwrap();
    let domain = Domain::open(&root.join("fast/fence")).unwrap();
    let reservation = admission::reserve(&root, &cfg, parsed.budget_bytes).unwrap();
    let workspace = publication::prepare(&cfg, &reservation, &domain, &raw).unwrap();
    fs::write(workspace.join("out.bin"), b"source").unwrap();
    let failure = json!({"schema":"reel.failed-job-result.v1","job_id":reservation.id,
        "manifest_sha256":format!("{:x}",Sha256::digest(&raw)),"outputs":{}});
    lifecycle::publish_and_retire_inner(&root,&cfg,&reservation.cache_root,&workspace,&raw,&failure,&parsed,Some(&domain),||{
        let intent=publication::check_retirement(&workspace,&raw,&fs::read(workspace.join("job-retirement-intent.json"))?)?;
        let pending=root.join(".local/retirement-ready.pending");
        fs::write(&pending,json!({"job_id":reservation.id,"manifest_sha256":intent.manifest.sha256,"evidence_sha256":intent.evidence.sha256}).to_string())?;
        fs::rename(pending,root.join(".local/retirement-ready.json"))?;
        loop {std::thread::park_timeout(Duration::from_secs(1));}
    }).unwrap();
}

#[test]
fn enrolled_verified_discard_rejects_invalid_binding_and_preserves_cas_after_valid_cleanup() {
    let (fixture, manifest, _job, domain) = enrolled("mutate");
    let result = run_fenced(fixture.root(), &manifest, &domain).unwrap();
    let workspace = PathBuf::from(result["workspace"].as_str().unwrap());
    let manifest_id = fixture.put(&fs::read(manifest).unwrap(), true);
    fixture.put(b"changed", true);
    let control = fixture
        .root()
        .join("fast/fence/asset-publication-fence.v1.lock");
    let original = fs::read(&control).unwrap();
    fs::write(&control, b"partial").unwrap();
    assert!(
        discard(
            fixture.root(),
            result["job_id"].as_str().unwrap(),
            &manifest_id,
            false
        )
        .is_err()
    );
    assert!(workspace.exists());
    assert!(workspace.join("out.bin").exists());
    fs::write(&control, original).unwrap(); // explicit fixture restoration, never runtime repair
    let cleaned = discard(
        fixture.root(),
        result["job_id"].as_str().unwrap(),
        &manifest_id,
        false,
    )
    .unwrap();
    assert_eq!(cleaned["cache_objects_removed"], 0);
    assert!(!workspace.exists());
    assert_eq!(
        fs::read(object(
            &fixture.root().join("fast"),
            result["outputs"]["copy"]["sha256"].as_str().unwrap()
        ))
        .unwrap(),
        b"changed"
    );
}

fn tool() -> &'static [u8] {
    static TOOL: OnceLock<Vec<u8>> = OnceLock::new();
    TOOL.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("fixture.exe");
        let output = Command::new("rustc")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/job_lifecycle.rs"
            ))
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        fs::read(executable).unwrap()
    })
}

fn prepare(f: &Fixture, mode: &str) -> (PathBuf, Value) {
    let executable = tool();
    let oid = f.put(executable, true);
    let source = f.put(b"source", true);
    let manifest = f.root().join(format!("{mode}.json"));
    let job = json!({"budget_bytes":16*1024*1024,"scratch_bytes":2*1024*1024,
        "inputs":{"in.bin":{"sha256":source,"bytes":6}},
        "tools":{"tools/tool.exe":{"sha256":oid,"bytes":executable.len()}},
        "steps":[{"tool":"tools/tool.exe","args":[mode,"in.bin","out.bin"],"timeout_seconds":10}],
        "outputs":{"copy":{"path":"out.bin","max_bytes":1024}}});
    fs::write(&manifest, job.to_string()).unwrap();
    (manifest, job)
}

fn evidence(f: &Fixture, oid: &str) -> Value {
    let cfg = config(f.root()).unwrap();
    let path = cfg
        .cache_roots
        .iter()
        .map(|c| object(c, oid))
        .find(|p| p.exists())
        .unwrap();
    let mut text = String::new();
    flate2::read::GzDecoder::new(File::open(path).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

fn failed_evidence(f: &Fixture, error: &str) -> Value {
    let (_, rest) = error.split_once("cache://sha256/").expect(error);
    evidence(f, &rest[..64])
}

fn no_scratch(f: &Fixture) {
    for cache in config(f.root()).unwrap().cache_roots {
        let p = cache.join("staging/caminos");
        assert!(!p.exists() || fs::read_dir(p).unwrap().next().is_none());
    }
    assert!(!f.root().join("offline-cold").exists());
}

#[test]
fn default_success_shares_inputs_publishes_evidence_and_retires_scratch() {
    let f = Fixture::new();
    let (manifest, job) = prepare(&f, "copy");
    let result = run(f.root(), &manifest).unwrap();
    assert_eq!(result["workspace_retained"], false);
    assert_eq!(result["copied_input_bytes"], 0);
    assert_eq!(result["linked_input_bytes"], tool().len() as u64 + 6);
    assert_eq!(
        fs::read(result["outputs"]["copy"]["path"].as_str().unwrap()).unwrap(),
        b"source"
    );
    let e = evidence(
        &f,
        result["retirement_evidence"]["sha256"].as_str().unwrap(),
    );
    assert_eq!(
        serde_json::from_str::<Value>(e["request_utf8"].as_str().unwrap()).unwrap(),
        job
    );
    assert!(e["diagnostics"]["step-0.log"].is_object());
    let registered = selected(f.root()).unwrap();
    assert!(registered.contains_key(result["retirement_evidence"]["sha256"].as_str().unwrap()));
    assert!(registered.contains_key(result["manifest_sha256"].as_str().unwrap()));
    no_scratch(&f);
}

#[test]
fn failed_job_salvages_declared_candidate_and_logs_but_retires_intermediates() {
    let f = Fixture::new();
    let (manifest, _) = prepare(&f, "fail");
    let error = run(f.root(), &manifest).unwrap_err().to_string();
    assert!(error.contains("exited 23"), "{error}");
    let e = failed_evidence(&f, &error);
    assert!(
        e["diagnostics"]["step-0.log"]["text"]
            .as_str()
            .unwrap()
            .contains("synthetic failure detail")
    );
    let output = &e["declared_retained_outputs"]["copy"];
    let oid = output["sha256"].as_str().unwrap();
    assert_eq!(selected(f.root()).unwrap()[oid], 9);
    assert_eq!(
        fs::read(object(&f.root().join("fast"), oid)).unwrap(),
        b"candidate"
    );
    assert!(!e.to_string().contains("intermediate.bin"));
    no_scratch(&f);
}

#[test]
fn installed_runtime_is_pinned_and_runs_without_an_executable_workspace_copy() {
    let f = Fixture::new();
    let (manifest, mut job) = prepare(&f, "copy");
    let installed = f.root().join("installed.exe");
    fs::write(&installed, tool()).unwrap();
    let descriptor = job["tools"]["tools/tool.exe"].clone();
    job["tools"] = json!({});
    job["installed_tools"] = json!({"runtime":{"path":installed,"sha256":descriptor["sha256"],"bytes":descriptor["bytes"]}});
    job["steps"][0]["tool"] = json!("runtime");
    fs::write(&manifest, job.to_string()).unwrap();
    let result = run(f.root(), &manifest).unwrap();
    assert_eq!(result["linked_input_bytes"], 6);
    assert_eq!(result["copied_input_bytes"], 0);
    no_scratch(&f);
    fs::write(&installed, b"changed installation").unwrap();
    assert!(run(f.root(), &manifest).is_err());
    no_scratch(&f);
}

#[test]
fn immutable_input_rejects_mutation_and_explicit_writable_copy_preserves_canonical() {
    let f = Fixture::new();
    let (manifest, mut job) = prepare(&f, "mutate");
    let original = object(
        &f.root().join("fast"),
        job["inputs"]["in.bin"]["sha256"].as_str().unwrap(),
    );
    let error = run(f.root(), &manifest).unwrap_err().to_string();
    assert!(error.contains("staging retired"), "{error}");
    assert_eq!(fs::read(&original).unwrap(), b"source");
    no_scratch(&f);
    job["writable_inputs"] = json!(["in.bin"]);
    fs::write(&manifest, job.to_string()).unwrap();
    let result = run(f.root(), &manifest).unwrap();
    assert_eq!(result["copied_input_bytes"], 6);
    assert_eq!(
        fs::read(result["outputs"]["copy"]["path"].as_str().unwrap()).unwrap(),
        b"changed"
    );
    assert_eq!(fs::read(original).unwrap(), b"source");
    no_scratch(&f);
}

#[test]
fn independent_readers_coexist_and_temporary_links_are_removable_while_pinned() {
    let f = Fixture::new();
    let original = object(&f.root().join("fast"), &f.put(b"immutable", true));
    let link = f.root().join("view");
    let a = lifecycle::pin_read_only(&original).unwrap();
    let b = lifecycle::pin_read_only(&original).unwrap();
    fs::hard_link(&original, &link).unwrap();
    let c = lifecycle::pin_read_only(&link).unwrap();
    assert!(fs::write(&original, b"bad").is_err());
    assert!(fs::write(&link, b"bad").is_err());
    fs::remove_file(link).unwrap();
    assert_eq!(fs::read(&original).unwrap(), b"immutable");
    drop((a, b, c));
}

#[test]
fn concurrent_jobs_share_canonical_inputs_without_a_job_wide_cache_lock() {
    let f = Fixture::new();
    let (manifest, _) = prepare(&f, "hold-copy");
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|s| {
        let execute = || {
            barrier.wait();
            run(f.root(), &manifest).unwrap()
        };
        let a = s.spawn(execute);
        let b = s.spawn(execute);
        for result in [a.join().unwrap(), b.join().unwrap()] {
            assert_eq!(result["workspace_retained"], false);
            assert_eq!(result["copied_input_bytes"], 0);
        }
    });
    no_scratch(&f);
}

#[test]
fn unsafe_salvage_and_registration_failure_hold_the_workspace() {
    let f = Fixture::new();
    let (manifest, _) = prepare(&f, "oversize");
    let error = run(f.root(), &manifest).unwrap_err().to_string();
    assert!(error.contains("staging preserved"), "{error}");
    assert!(error.contains("salvage bound"), "{error}");
    let f = Fixture::new();
    let (manifest, job) = prepare(&f, "copy");
    let oid = job["inputs"]["in.bin"]["sha256"].as_str().unwrap();
    let pointer = f
        .root()
        .join("assets/objects/sha256")
        .join(&oid[..2])
        .join(oid);
    fs::create_dir_all(pointer.parent().unwrap()).unwrap();
    fs::write(pointer, b"corrupt pointer").unwrap();
    let error = run(f.root(), &manifest).unwrap_err().to_string();
    assert!(error.contains("staging preserved"), "{error}");
    let parent = f.root().join("fast/staging/caminos");
    let workspace = fs::read_dir(parent)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(workspace.join("out.bin").exists());
}

#[test]
fn explicit_debug_retention_still_supports_guarded_discard() {
    let f = Fixture::new();
    let (manifest, mut job) = prepare(&f, "copy");
    job["retain_workspace"] = json!(true);
    let bytes = serde_json::to_vec(&job).unwrap();
    fs::write(&manifest, &bytes).unwrap();
    let oid = f.put(&bytes, true);
    let result = run(f.root(), &manifest).unwrap();
    assert_eq!(result["workspace_retained"], true);
    let receipt = discard(f.root(), result["job_id"].as_str().unwrap(), &oid, false).unwrap();
    assert_eq!(receipt["cache_objects_removed"], 0);
    no_scratch(&f);
}

#[test]
fn removal_of_canonical_names_does_not_delete_the_last_remaining_input_view() {
    let f = Fixture::new();
    let (manifest, mut job) = prepare(&f, "unlink-originals");
    let oid = job["inputs"]["in.bin"]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    // Simulate external name removal while the job holds its input handles.
    job["steps"][0]["args"]
        .as_array_mut()
        .unwrap()
        .extend(["fast", "history"].map(|tier| json!(object(&f.root().join(tier), &oid))));
    // Produce different output so publication does not incidentally restore the original.
    job["steps"]
        .as_array_mut()
        .unwrap()
        .push(json!({"tool":"tools/tool.exe",
        "args":["fail","in.bin","out.bin"],"timeout_seconds":10}));
    fs::write(&manifest, job.to_string()).unwrap();
    let error = run(f.root(), &manifest).unwrap_err().to_string();
    assert!(error.contains("Retained original absent"), "{error}");
    assert!(error.contains("staging preserved"), "{error}");
    let workspace = fs::read_dir(f.root().join("fast/staging/caminos"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(fs::read(workspace.join("in.bin")).unwrap(), b"source");
}

#[test]
fn staging_parent_junction_cannot_redirect_job_writes_or_retirement_outside_cache() {
    let f = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    let destination = outside.path().canonicalize().unwrap();
    fs::write(destination.join("preserve.txt"), b"outside").unwrap();
    let staging = f.root().join("fast/staging");
    let cmd_path = |p: &Path| {
        p.to_string_lossy()
            .replace('/', "\\")
            .trim_start_matches("\\\\?\\")
            .to_owned()
    };
    let output = Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(cmd_path(&staging))
        .arg(cmd_path(&destination))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (manifest, _) = prepare(&f, "copy");
    let error = run(f.root(), &manifest).unwrap_err().to_string();
    assert!(
        error.contains("escaped its exact admitted cache"),
        "{error}"
    );
    assert!(!destination.join("caminos").exists());
    assert_eq!(
        fs::read(destination.join("preserve.txt")).unwrap(),
        b"outside"
    );
    // Remove only this exact test junction, not its target tree.
    assert!(staging.starts_with(f.root()));
    fs::remove_dir(staging).unwrap();
}

#[test]
fn timeout_retires_workspace_after_descendants_die_and_preserves_pid_evidence() {
    use windows_sys::Win32::{Foundation::*, System::Threading::*};
    let f = Fixture::new();
    let (manifest, mut job) = prepare(&f, "tree");
    // Allow both synthetic processes to start under parallel Windows load;
    // timeout and retained PID/termination assertions remain mandatory.
    job["steps"][0]["timeout_seconds"] = json!(5);
    fs::write(&manifest, job.to_string()).unwrap();
    let error = run(f.root(), &manifest).unwrap_err().to_string();
    assert!(error.contains("timed out"), "{error}");
    let e = failed_evidence(&f, &error);
    for name in ["parent.pid", "child.pid"] {
        let pid = e["diagnostics"][name]["text"]
            .as_str()
            .unwrap()
            .parse::<u32>()
            .unwrap();
        // A missing PID handle is also evidence of exit; otherwise await termination.
        unsafe {
            let handle = OpenProcess(0x0010_0000 /* SYNCHRONIZE */, 0, pid);
            if !handle.is_null() {
                let status = WaitForSingleObject(handle, 5000);
                CloseHandle(handle);
                assert_eq!(status, WAIT_OBJECT_0);
            }
        }
    }
    no_scratch(&f);
}
