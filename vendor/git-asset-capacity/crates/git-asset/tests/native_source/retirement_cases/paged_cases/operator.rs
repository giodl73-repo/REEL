//! Actual native selector -> generic operator handoff in isolated fixture roots.
use super::*;
use git_asset_store::{
    AckLevel, Request, accounted, clock,
    final_retirement::{bulk, paged::control},
    ledger::CapacityLedger,
    local::LocalStore,
    local_capacity::LocalCapacityProbe,
    orchestration::Place,
    retention,
    store_protection::{Binding, Member},
};
use serde_json::Value;
use std::{ffi::OsStr, process::Command};

#[track_caller]
fn invoke(cli: &OsStr, root: &Path, prefix: &[&str], profile: &Value, request: &Value) -> Value {
    let profile_path = root.join("operator-profile.json");
    let request_path = root.join("operator-request.json");
    std::fs::write(&profile_path, identity::canonical_json(profile).unwrap()).unwrap();
    std::fs::write(&request_path, identity::canonical_json(request).unwrap()).unwrap();
    let output = Command::new(cli)
        .args(prefix)
        .arg(profile_path)
        .arg(request_path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{prefix:?}: {output:?}");
    assert!(output.stderr.is_empty());
    assert!(output.stdout.len() <= 65536);
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
#[ignore = "requires explicitly fresh git-asset-storage executable"]
fn native_cli_selection_reaches_storage_operator_and_preserves_shared_bytes() {
    let storage =
        std::env::var_os("GIT_ASSET_PAGED_CLI").expect("explicit fresh storage executable");
    let native_cli = OsStr::new(env!("CARGO_BIN_EXE_git-asset-retirement"));
    let f = Setup::new(17);
    let root = f.temp.path();
    let mut native_profile = json!({"schema":"tig.native-paged-profile.design-1",
        "enrollment":f.enrollment,"allow_stage":true,
        "metadata":{"root":root.join("metadata"),"minimum":f.metadata.checkpoint()},
        "namespaces":[{"root":root.join("namespace"),"minimum":f.namespaces[0].checkpoint()}]});
    let indexed = invoke(
        native_cli,
        root,
        &["index-paged"],
        &native_profile,
        &json!({"schema":"tig.native-paged-index-request.design-1","occurrences":f.roots}),
    );
    assert_eq!(indexed["count"], 17);
    native_profile["metadata"]["minimum"] = indexed["metadata_checkpoint"].clone();
    let selected = invoke(
        native_cli,
        root,
        &["select-paged"],
        &native_profile,
        &json!({"schema":"tig.native-paged-select-request.design-1","occurrences":indexed["occurrences"],
            "filter":filter(&f.snapshot, &["asset/a", "asset/c"]),
            "paging":{"max_bindings":4,"max_bytes":49152}}),
    );
    assert_eq!(selected["preview"]["selected_roots"], 17);
    assert_eq!(selected["authorizes_removal"], false);
    let candidate: native::Candidate =
        serde_json::from_value(selected["candidate"].clone()).unwrap();
    let roster = claims::Roster::flat(
        d("final-authority"),
        d("epoch"),
        Some(d("owned-complete-scope")),
        vec![claims::NamespacePin::current(&f.namespaces[0]).unwrap()],
        protection_limits(),
    )
    .unwrap();
    let clock_ledger = CapacityLedger::create_clock(
        &root.join("clock"),
        key("clock"),
        key("epoch"),
        0,
        clock::Policy {
            authority: d("clock-authority"),
            epoch: d("clock-epoch"),
            boundary: d("fixture-os"),
            max_sample_millis: 5000,
            max_drift_millis: 1000,
            max_session_millis: 300000,
        },
    )
    .unwrap();
    let clock_enrollment = clock_ledger.clock_enrollment().unwrap();
    let clock_id = clock_enrollment.id().unwrap();
    let store = LocalStore::create_final(
        &root.join("M"),
        key("M"),
        Binding::new(
            1,
            d("fixture-boundary"),
            vec![Member {
                namespace: f.namespaces[0].identity().clone(),
                places: vec![key("M")],
            }],
        )
        .unwrap(),
        provider::Policy::new(
            d("final-authority"),
            d("epoch"),
            roster.scope().unwrap(),
            clock_id.clone(),
            vec![key("owner")],
            300000,
        )
        .unwrap(),
    )
    .unwrap();
    let mut ledger =
        CapacityLedger::create(&root.join("ledger"), key("capacity"), key("epoch"), 0).unwrap();
    LocalCapacityProbe::bind(&store, &mut ledger)
        .unwrap()
        .observe(&store, &mut ledger, key("probe"), key("sample"))
        .unwrap();
    let job = ledger
        .reserve(key("job"), key("owner"), 128 * 1024 * 1024)
        .unwrap()
        .reference()
        .clone();
    let mut copies = Vec::new();
    for name in ["shared", "unique"] {
        let publication = Request {
            operation: key(&format!("publish-{name}")),
            owner: key("owner"),
            object: Object::from_bytes(name.as_bytes()),
            required: AckLevel::ProcessRestart,
        };
        let plan = accounted::plan(&store, &publication).unwrap();
        accounted::publish_accounted(
            &mut ledger,
            &store,
            &job,
            &plan,
            &mut std::io::Cursor::new(name.as_bytes()),
        )
        .unwrap();
        let grant = retention::plan(
            &store,
            &retention::RetentionRequest {
                operation: key(&format!("retain-{name}")),
                owner: key("owner"),
                publication: publication.clone(),
                minimum_ack: AckLevel::ProcessRestart,
            },
        )
        .unwrap();
        retention::retain(&mut ledger, &store, &job, &grant).unwrap();
        copies.push(bulk::Copy {
            store: store.identity().clone(),
            capacity: ledger.identity().clone(),
            publication,
        });
    }
    let witness = LocalStore::create(&root.join("witness"), key("witness")).unwrap();
    let mut witness_ledger = CapacityLedger::create(
        &root.join("witness-ledger"),
        key("witness-capacity"),
        key("epoch"),
        0,
    )
    .unwrap();
    LocalCapacityProbe::bind(&witness, &mut witness_ledger)
        .unwrap()
        .observe(&witness, &mut witness_ledger, key("probe"), key("sample"))
        .unwrap();
    let witness_job = witness_ledger
        .reserve(key("job"), key("owner"), 512 * 1024 * 1024)
        .unwrap()
        .reference()
        .clone();
    let enrollment = control::Enrollment {
        age: None,
        selection: selection::Enrollment {
            authority: d("final-authority"),
            epoch: d("epoch"),
            owner: key("owner"),
            metadata: f.enrollment.metadata.clone(),
            scope: roster.scope().unwrap(),
            classes: [Kind::Branch].into_iter().collect(),
        },
        sources: vec![source_claims::subset::Enrollment {
            authority: d("final-authority"),
            namespace: f.namespaces[0].identity().clone(),
            policy: digest(f.namespaces[0].policy()),
            owner: key("owner"),
            classes: vec![Kind::Branch],
            limits: protection_limits(),
            writes: 10000,
            write_bytes: 8 * 1024 * 1024,
        }],
        context: d("native-operator-context"),
        clock: clock_id.clone(),
        witness: Place::local(&witness, 1).unwrap(),
        capacity: witness_ledger.identity().clone(),
        max_window: 300000,
        max_metadata_bytes: 512 * 1024 * 1024,
    };
    let mut profile = json!({"schema":"tig.paged-operator-profile.design-1","enrollment":enrollment,
        "roster":roster,"admitted_context":enrollment.context,
        "metadata":{"root":root.join("metadata"),"minimum":selected["metadata_checkpoint"]},
        "namespaces":[{"root":root.join("namespace"),"minimum":roster.members[0].minimum}],
        "stores":[{"root":root.join("M"),"ledger_root":root.join("ledger"),"minimum":ledger.checkpoint(),"job":job,"place":Place::local(&store,1).unwrap()}],
        "witness":{"root":root.join("witness"),"ledger_root":root.join("witness-ledger"),"minimum":witness_ledger.checkpoint(),"job":witness_job,"place":enrollment.witness},
        "clock":{"root":root.join("clock"),"minimum":clock_ledger.checkpoint(),"enrollment":clock_enrollment},
        "permissions":{"stage":true,"apply":true,"cancel":true},"run_limits":{"pages":8,"actions":1}});
    let copied = invoke(
        &storage,
        root,
        &["paged", "copies"],
        &profile,
        &json!({"schema":"tig.paged-copy-request.design-1","base":null,"copies":copies}),
    );
    profile["metadata"]["minimum"] = copied["minimum"].clone();
    let port = clock::LocalClock::new(clock_ledger, clock_enrollment).unwrap();
    let now = port.sample(&clock_id).unwrap();
    let planned = invoke(
        &storage,
        root,
        &["paged", "plan"],
        &profile,
        &json!({"schema":"tig.paged-plan-request.design-1","operation":"native-cli-block",
            "selection":candidate.request().unwrap(),"copies":copied["copies"],
            "window":{"not_before":now.earliest - 1000,"expires":now.latest + 240000}}),
    );
    profile["metadata"]["minimum"] = planned["minimum"].clone();
    profile["claim_release"] = planned["decision_id"].clone();
    profile["final_loss"] = planned["decision_id"].clone();
    let decision = &planned["decision"];
    let explained = invoke(&storage, root, &["paged", "subjects"], &profile, decision);
    assert!(
        explained["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["object"] == json!(Object::from_bytes(b"shared"))
                && s["hold"] == "surviving_claims")
    );
    let before = f.namespaces[0].checkpoint();
    let first = invoke(&storage, root, &["paged", "apply"], &profile, decision);
    assert_eq!(first["phase"], "intent");
    assert_eq!(first["completed_pages"], 8);
    assert!(store.read_object(&Object::from_bytes(b"unique")).is_ok());
    let mut done = first;
    for _ in 0..32 {
        done = invoke(&storage, root, &["paged", "resume"], &profile, decision);
        if done["phase"] == "completed" {
            break;
        }
    }
    assert_eq!(done["phase"], "completed", "{done}");
    assert!(done["completed_pages"].as_u64().unwrap() > 16);
    assert_eq!(done["historical_deleted_actions"], 1);
    assert!(matches!(
        store.read_object(&Object::from_bytes(b"unique")),
        Err(git_asset_store::Error::AuthorizedAbsence)
    ));
    assert_eq!(done["effects"][0]["receipt"]["observed"], "absent");
    assert_eq!(done["effects"][0]["receipt"]["logical_bytes_removed"], 6);
    let mut bytes = Vec::new();
    store
        .read_object(&Object::from_bytes(b"shared"))
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, b"shared");
    let current = Namespace::open(&root.join("namespace"), &before).unwrap();
    assert_ne!(current.checkpoint(), before);
    let status = invoke(&storage, root, &["paged", "status"], &profile, decision);
    assert_eq!(status["phase"], "completed");
    assert_eq!(status["historical_deleted_actions"], 1);
}
