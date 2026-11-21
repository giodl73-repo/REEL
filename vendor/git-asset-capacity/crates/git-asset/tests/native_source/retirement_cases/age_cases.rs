use super::*;
use git_asset_store::{clock, ledger::CapacityLedger, source_claims::age};

fn digest(v: &impl serde::Serialize) -> Digest {
    Digest::blob(&identity::canonical_json(&serde_json::to_value(v).unwrap()).unwrap())
}
// Real OS sampling may temporarily refuse its admitted uncertainty window.
// Retry only this explicit hold, with fresh CLI invocations and finite bounds;
// malformed evidence, admission failures and all other errors remain failures.
fn with_clock_retry(mut run: impl FnMut() -> std::process::Output) -> std::process::Output {
    let started = std::time::Instant::now();
    for attempt in 0..4 {
        let out = run();
        let held = !out.status.success()
            && serde_json::from_slice::<Value>(&out.stdout)
                .is_ok_and(|v| v["code"] == "clock_held");
        if !held || attempt == 3 || started.elapsed() >= std::time::Duration::from_secs(5) {
            return out;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    unreachable!("finite clock attempt loop returns on its final attempt")
}

#[test]
fn aged_native_cli_holds_unknown_and_recent_then_emits_only_exact_proven_block() {
    let (f, mut snapshot) = fixture();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut store = metadata(&root.join("metadata"));
    let names = roots(&snapshot);
    let selection = retirement::build(
        &mut snapshot,
        names,
        profiles(&f),
        &mut store,
        BindingLimits::default(),
    )
    .unwrap();
    store.finish().unwrap();
    let mut ns = namespace(&root.join("namespace"), &store, &selection);
    let ns_min = ns.checkpoint();
    let mut registry = Namespace::create(
        &root.join("registry"),
        key("ages"),
        key("epoch"),
        ns.policy().clone(),
    )
    .unwrap();
    registry
        .apply(
            key("enroll"),
            SourceCommand::Enroll {
                source: key("records"),
                owner: key("owner"),
                class: Kind::Retention,
            },
            &store,
            &protection_limits(),
        )
        .unwrap();
    let registry_min = registry.checkpoint();
    let journal = CapacityLedger::create_clock(
        &root.join("clock"),
        key("clock"),
        key("epoch"),
        0,
        clock::Policy {
            authority: d("authority"),
            epoch: d("epoch"),
            boundary: d("native-os"),
            max_sample_millis: 500,
            max_drift_millis: 100,
            max_session_millis: 60000,
        },
    )
    .unwrap();
    let clock_enrollment = journal.clock_enrollment().unwrap();
    let clock_min = journal.checkpoint();
    let clock = clock::LocalClock::new(journal, clock_enrollment.clone()).unwrap();
    let e = age::Enrollment {
        authority: d("authority"),
        epoch: d("epoch"),
        metadata: store.checkpoint().identity().clone(),
        registry: registry.identity().clone(),
        registry_policy: digest(registry.policy()),
        source: key("records"),
        owner: key("owner"),
        clock: clock_enrollment.id().unwrap(),
        observers: vec![age::Observer {
            namespace: ns.identity().clone(),
            policy: digest(ns.policy()),
            owners: vec![key("owner")],
            classes: [Kind::Branch, Kind::Draft].into_iter().collect(),
        }],
    };
    let limits = age::Limits::default();
    age::Capture {
        enrollment: &e,
        registry: &mut registry,
        minimum: &registry_min,
        metadata: &mut store,
        limits: &limits,
    }
    .initialize()
    .unwrap();
    let source_before = ns.checkpoint();
    let mut profile = json!({"schema":"tig.native-retirement-aged-profile.design-1",
        "selection":{"schema":"tig.native-retirement-select-profile.design-1","metadata":{"root":root.join("metadata"),"minimum":store.checkpoint()},
        "namespace":{"root":root.join("namespace"),"minimum":ns_min},"source":"main","owner":"owner","predecessor_revision":3,"header":selection.context,"categories":category(&snapshot)},
        "age":{"enrollment":e,"registry":{"root":root.join("registry"),"minimum":registry_min},"clock":{"root":root.join("clock"),"minimum":clock_min,"enrollment":clock_enrollment},"required_age_millis":10000}});
    let request =
        json!({"assets":[snapshot.resolve_id("asset/a").unwrap().reference()],"categories":[]});
    let call = |profile: &Value, request: &Value| {
        fs::write(
            root.join("profile.json"),
            serde_json::to_vec(profile).unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("request.json"),
            serde_json::to_vec(request).unwrap(),
        )
        .unwrap();
        let out = with_clock_retry(|| {
            Command::new(env!("CARGO_BIN_EXE_git-asset-retirement"))
                .arg("select-aged")
                .arg(root.join("profile.json"))
                .arg(root.join("request.json"))
                .output()
                .unwrap()
        });
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(out.stderr.is_empty(), "{out:?}");
        assert!(!String::from_utf8_lossy(&out.stdout).contains("PRIVATE AUTHORING TEXT"));
        (out.status.success(), v)
    };
    let (ok, unknown) = call(&profile, &request);
    assert!(ok, "{unknown}");
    assert!(unknown["selection"].is_null());
    assert_eq!(unknown["age_assessment"]["hold"], "unknown_age");
    assert_eq!(
        CapacityLedger::open(&root.join("clock"), &clock_min)
            .unwrap()
            .checkpoint(),
        clock_min
    );
    let r = age::Request {
        namespace: ns.identity().clone(),
        source: key("main"),
        predecessor_revision: 3,
    };
    let captured = age::Capture {
        enrollment: &e,
        registry: &mut registry,
        minimum: &registry_min,
        metadata: &mut store,
        limits: &limits,
    }
    .capture(&r, &mut ns, &ns_min, &clock)
    .unwrap();
    profile["selection"]["metadata"]["minimum"] = serde_json::to_value(store.checkpoint()).unwrap();
    let before = store.checkpoint();
    let registry_before = registry.checkpoint();
    let (ok, recent) = call(&profile, &request);
    assert!(ok, "{recent}");
    assert!(recent["selection"].is_null());
    assert_eq!(recent["age_assessment"]["hold"], "not_old_enough");
    profile["age"]["required_age_millis"] = json!(0);
    // Wait only for the conservative native observation interval, bounded by
    // this fixture's policy; never fake age with a supplied Git/file timestamp.
    let latest = captured.observed.unwrap().latest;
    let wait = std::time::Instant::now();
    loop {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        if now > u128::from(latest) {
            break;
        }
        assert!(wait.elapsed() < std::time::Duration::from_secs(5));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    fs::rename(f.root().join(".git"), f.root().join("git-offline")).unwrap();
    let (ok, eligible) = call(&profile, &request);
    assert!(ok, "{eligible}");
    assert_eq!(eligible["age_assessment"]["old_enough"], true);
    let block: git_asset_store::final_retirement::bulk::Selection =
        serde_json::from_value(eligible["selection"].clone()).unwrap();
    assert_eq!(block.request.paths.len(), 4);
    assert_eq!(block.request.predecessor_revision, 3);
    assert_eq!(eligible["authorizes_removal"], false);
    let mut injected = request.clone();
    injected["min_deprecated_millis"] = json!(0);
    assert!(!call(&profile, &injected).0);
    let mut wrong = profile.clone();
    wrong["selection"]["header"] = json!(d("foreign"));
    assert!(!call(&wrong, &request).0);
    assert_eq!(
        Namespace::open(&root.join("namespace"), &source_before)
            .unwrap()
            .checkpoint(),
        source_before
    );
    assert_eq!(
        Namespace::open(&root.join("registry"), &registry_before)
            .unwrap()
            .checkpoint(),
        registry_before
    );
    assert_eq!(
        MetadataReader::open(&root.join("metadata"), &before)
            .unwrap()
            .checkpoint(),
        &before
    );
    // The indexed CLI uses the same real enrolled clock/registry and detached
    // native archive, with an explicit positive age threshold and staging permit.
    use git_asset::source::native::retirement::paged::{self as paged, Occurrence, Session};
    let session = Session::new(BindingLimits::default()).unwrap();
    let occurrences = paged::occurrences(
        None,
        vec![Occurrence {
            namespace: ns.identity().clone(),
            source: key("main"),
            predecessor_revision: 3,
            header: selection.context.clone(),
        }],
        &mut store,
        &session,
    )
    .unwrap();
    let minimum = store.finish().unwrap();
    let paged_profile = json!({"schema":"tig.native-paged-profile.design-1","allow_stage":true,
        "enrollment":{"metadata":e.metadata,"owner":"owner","categories":category(&snapshot),"age":{"enrollment":e,"required_age_millis":1}},
        "metadata":{"root":root.join("metadata"),"minimum":minimum},
        "namespaces":[{"root":root.join("namespace"),"minimum":ns_min}],
        "age":{"registry":{"root":root.join("registry"),"minimum":registry_min},"clock":profile["age"]["clock"]}});
    fs::write(
        root.join("paged-profile.json"),
        serde_json::to_vec(&paged_profile).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("paged-request.json"),
        serde_json::to_vec(&json!({"schema":"tig.native-paged-select-request.design-1",
        "occurrences":occurrences,"filter":request}))
        .unwrap(),
    )
    .unwrap();
    let out = with_clock_retry(|| {
        Command::new(env!("CARGO_BIN_EXE_git-asset-retirement"))
            .arg("select-paged")
            .arg(root.join("paged-profile.json"))
            .arg(root.join("paged-request.json"))
            .output()
            .unwrap()
    });
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(out.status.success(), "{result}");
    assert_eq!(result["preview"]["selected_roots"], 1);
    assert_eq!(result["preview"]["held_roots"], 0);
    assert_eq!(result["authorizes_removal"], false);
    assert_eq!(
        Namespace::open(&root.join("namespace"), &source_before)
            .unwrap()
            .checkpoint(),
        source_before
    );
    assert_eq!(
        Namespace::open(&root.join("registry"), &registry_before)
            .unwrap()
            .checkpoint(),
        registry_before
    );
}
