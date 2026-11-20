//! Retain the original affected consumer under its original ID. No task
//! supersession, reexecution of completed producers or inferred finding closure.
use super::repair_control_native::check_waiting;
use super::repair_part_native::{
    binding, load, native_bytes, phase, raw_save, save, success, verify_parent,
};
use super::*;
use reel_workflow::handoff::{Journal, State};
use reel_workflow_native::{
    NativeProfile, admission, closing, dispatch, graph, handoff, retention, staging, upstream,
    worker,
};
use tig_store::source_claims::{Checkpoint, protection::Index};

macro_rules! invoke { ($tool:expr, $($arg:expr),+ $(,)?) => { Command::new($tool)$(.arg($arg))+.output().unwrap() }; }
fn observe(profile: &admission::Profile) -> Checkpoint {
    Index::open(&profile.index_root, &profile.index_minimum)
        .unwrap()
        .checkpoint()
}
fn certificate(root: &Path) {
    let raw = fs::read(root.join("qualified-report.json")).unwrap();
    assert_eq!(
        digest(&raw),
        "9c0e5eed1c34e28b138f8d409f003a84f736c340c2da82b6fb21716ed7150541"
    );
    let value: serde_json::Value = decode(&raw).unwrap();
    for pin in value["files"].as_array().unwrap() {
        let raw = fs::read(pin["path"].as_str().unwrap()).unwrap();
        assert_eq!(raw.len() as u64, pin["bytes"].as_u64().unwrap());
        assert_eq!(digest(&raw), pin["sha256"].as_str().unwrap());
    }
}
fn copy<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) -> T {
    decode(&serde_json::to_vec(value).unwrap()).unwrap()
}

#[test]
#[ignore = "Exact original-consumer preflight only; checked supplemental-report close, one original-ID consumer admission with finite owner policy and pinned 5c worker, retention/replay and exact next original-report derivation; preserves all producers/Waiting/scratch, no report admission or closure"]
fn original_review_consumer_retains_under_its_original_id_and_rebinds_the_original_report() {
    let env = |key| PathBuf::from(std::env::var_os(key).expect(key));
    let preflight = env("REEL_ORIGINAL_EXEC_PREFLIGHT");
    let old_report = env("REEL_ORIGINAL_EXEC_SLOT_OWNER");
    let issue_root = env("REEL_ORIGINAL_EXEC_ISSUE");
    let baseline_report = env("REEL_ORIGINAL_EXEC_BASELINE_REPORT");
    let old_control = env("REEL_ORIGINAL_EXEC_BASELINE_CONTROL");
    let waiting = env("REEL_ORIGINAL_EXEC_WAITING");
    let evidence = env("REEL_ORIGINAL_EXEC_EVIDENCE");
    let executable = env("REEL_ORIGINAL_EXEC_RUNTIME");
    certificate(&preflight);
    let ready = preflight.join("job");
    let packet: Packet = load(&ready, "packet.json");
    let source: NativeProfile = load(&ready, "source.json");
    let admission: admission::Profile = load(&ready, "admission.json");
    let parents: upstream::Parents = load(&ready, "parents.json");
    assert_eq!(packet.task.id, "owned-issue-review-controls");
    assert_eq!(
        packet.task_key,
        "3d51fe03fab26d996b9357d3de3ea645fd6b6a79a0ee265579de1756e07c9b96"
    );
    let raw = fs::read(&executable).unwrap();
    let runtime = Tool {
        path: executable,
        sha256: digest(&raw),
        bytes: raw.len() as u64,
    };
    assert_eq!(
        runtime.sha256,
        packet.task.tool.as_ref().unwrap().executable.sha256
    );
    let old: handoff::Profile = load(&old_report, "profile.json");
    assert!(old.binding.retained_owner_policy.is_some());
    assert_eq!(
        Journal::open(&old_report.join("journal")).unwrap().state(),
        State::Retained
    );
    assert_eq!(
        canonical_digest(&observe(&admission)).unwrap(),
        "cc4fd86ef00d4aeeefb022c1b73e7aff3d3a3f8597c032a1899c012bc965396e"
    );
    check_waiting(&waiting);
    let root = evidence.join("job");
    fs::create_dir(&root).unwrap();
    let policy = retention::capacity::Policy {
        schema: "reel.workflow-retained-owner-policy.v1".into(),
        objects: 1024,
        logical_bytes: 1024 * 1024 * 1024,
    };
    save(&root, "owner-policy.json", &policy);
    save(
        &root,
        "original-operation.json",
        &json!({"schema":"reel.original-consumer-execution.v1","task_id":packet.task.id,"task_key":packet.task_key,"attempt_id":"original-review-rebuild-attempt-1","scope":admission.scope,"runtime":runtime,"source_commit":packet.source_commit,"preflight":preflight,"old_slot_owner":old_report,"waiting":waiting,"owner_policy":policy,"no_fresh_identity_on_uncertainty":true}),
    );
    for name in [
        "baseline.json",
        "repair.json",
        "revision.json",
        "candidate.json",
        "plan.json",
        "packet.json",
        "source.json",
        "admission.json",
        "parents.json",
        "upstream.json",
    ] {
        raw_save(&root, name, &fs::read(ready.join(name)).unwrap());
    }
    phase(
        &root,
        "checked-close-of-supplemental-report-before-original-consumer-admission",
    );
    let close = root.join("old-report-close");
    fs::create_dir(&close).unwrap();
    save(&close, "target.json", &old.binding.target);
    let out = invoke!(
        &runtime.path,
        "prepare-close",
        old_report.join("journal"),
        old_report.join("profile.json"),
        close.join("target.json"),
        close.join("ticket.json"),
        "--apply"
    );
    success(&close, "prepare-result.json", &out);
    let pin = digest(&fs::read(close.join("ticket.json")).unwrap());
    let out = invoke!(
        &runtime.path,
        "stage-close",
        close.join("ticket.json"),
        &pin,
        close.join("staged.json"),
        "--apply"
    );
    success(&close, "stage-result.json", &out);
    let staged: closing::Outcome = load(&close, "staged.json");
    assert!(staged.handoff.is_some() && !staged.slot_idle);
    let out = invoke!(
        &runtime.path,
        "recover-close",
        close.join("ticket.json"),
        &pin,
        close.join("closed.json"),
        "--apply"
    );
    success(&close, "recover-result.json", &out);
    let closed: closing::Outcome = load(&close, "closed.json");
    assert!(closed.slot_idle && closed.original_clear_complete);
    assert_eq!(staged.handoff, closed.handoff);
    let after_close = observe(&admission);
    let out = invoke!(
        &old.runtime.path,
        "resume-handoff",
        old_report.join("journal"),
        old_report.join("profile.json"),
        "-",
        "--apply"
    );
    success(&close, "historical-replay.json", &out);
    assert_eq!(after_close, observe(&admission));
    phase(&root, "accept-one-original-id-review-consumer");
    let prepared = upstream::Admission::prepare(
        &packet,
        &source,
        &admission,
        "original-review-rebuild-attempt-1",
        parents.borrowed().unwrap(),
    )
    .unwrap();
    assert_eq!(
        prepared.preview().unwrap(),
        load::<admission::Preview>(&ready, "declared-preview.json")
    );
    let receipt = prepared.accept().unwrap();
    save(&root, "receipt.json", &receipt);
    let acknowledgements = root.join("acknowledgements");
    fs::create_dir(&acknowledgements).unwrap();
    let profile = handoff::Profile {
        schema: "reel.workflow-native-handoff.v1".into(),
        source,
        admission,
        binding: handoff::Binding {
            schema: "reel.workflow-native-handoff-binding.v2".into(),
            retained_owner_policy: Some(policy),
            admission: receipt.clone(),
            target: old.binding.target.clone(),
        },
        runtime: runtime.clone(),
        acknowledgements,
    };
    save(&root, "profile.json", &profile);
    let out = invoke!(
        &runtime.path,
        "create-handoff",
        root.join("packet.json"),
        root.join("profile.json"),
        root.join("journal"),
        "--apply"
    );
    success(&root, "create-result.json", &out);
    let mut bindings: graph::Bindings = load(&issue_root, "bindings.json");
    bindings.tasks.insert(
        packet.task.id.clone(),
        binding(root.join("journal"), root.join("profile.json")),
    );
    save(&root, "bindings.json", &bindings);
    let stage = |name| {
        let parent = root.join(name);
        fs::create_dir(&parent).unwrap();
        staging::Profile {
            schema: "reel.workflow-staging.v1".into(),
            parent,
            maximum_bytes: 64 * 1024 * 1024,
            capacity_policy: reel_assets::admission::Policy {
                minimum_free_bytes: 65536,
                minimum_free_basis_points: 0,
                max_object_bytes: 512 * 1024 * 1024,
                ..Default::default()
            },
        }
    };
    let config = dispatch::Config {
        schema: "reel.workflow-dispatch.v1".into(),
        input_staging: stage("input-workspaces"),
        worker_staging: Some(stage("worker-workspaces")),
        provider: None,
        auxiliary_programs: Default::default(),
    };
    save(&root, "dispatch-config.json", &config);
    phase(
        &root,
        "dispatch-once-under-original-consumer-id-and-pinned-runtime",
    );
    let out = invoke!(
        &runtime.path,
        "dispatch-task",
        root.join("plan.json"),
        root.join("bindings.json"),
        &packet.task.id,
        root.join("dispatch-config.json"),
        root.join("dispatch"),
        "--apply"
    );
    success(&root, "dispatch-result.json", &out);
    let journal = Journal::open(&root.join("journal")).unwrap();
    assert_eq!(journal.state(), State::Retained);
    let result = journal.result().unwrap().clone();
    save(&root, "result.json", &result);
    let ack: retention::Acknowledgement = load(
        &profile.acknowledgements,
        &format!("retained-{}-{}.json", packet.task_key, receipt.attempt_id),
    );
    save(&root, "retained.json", &ack);
    phase(
        &root,
        "verify-original-consumer-retention-and-prepare-original-report",
    );
    drop(
        retention::verify(
            &packet,
            &profile.source,
            &profile.admission,
            &receipt,
            &result,
            &ack,
        )
        .unwrap(),
    );
    for name in review_control::OUTPUTS {
        raw_save(
            &root.join("verified"),
            name,
            &native_bytes(&profile.source, &result.artifacts[name]),
        );
    }
    let request: review::Request = load(&root.join("verified"), "review-request.json");
    assert_eq!(request.schema, "reel.workflow-review-request.v1");
    assert!(request.production.is_none());
    let copy: review::Copy = load(&root.join("verified"), "review-copy.json");
    assert_eq!(copy.units.len(), 6);
    let worker: worker::Ticket = load(&root.join("dispatch"), "worker-ticket.json");
    let worker_pin = digest(&fs::read(worker.directory().join("result.json")).unwrap());
    let complete = observe(&profile.admission);
    let out = invoke!(
        &runtime.path,
        "resume-handoff",
        root.join("journal"),
        root.join("profile.json"),
        "-",
        "--apply"
    );
    success(&root, "completed-replay.json", &out);
    assert_eq!(complete, observe(&profile.admission));
    assert_eq!(
        worker_pin,
        digest(&fs::read(worker.directory().join("result.json")).unwrap())
    );
    for parent in parents.parents.values() {
        verify_parent(parent);
    }
    check_waiting(&waiting);
    certificate(&preflight);
    save(
        &root,
        "consumer-retention-checkpoint.json",
        &json!({"task_id":packet.task.id,"task_key":packet.task_key,"controls_retained":true,"actual_index_sha256":canonical_digest(&complete).unwrap(),"original_task_id_preserved":true,"completed_replay_index_and_worker_result_unchanged":true}),
    );
    let next = prepare_report(
        &root,
        &baseline_report,
        &old_control,
        &packet,
        &profile,
        &result,
        &ack,
        &parents,
    );
    assert_eq!(complete, observe(&profile.admission));
    save(
        &evidence,
        "qualification-result.json",
        &json!({"schema":"reel.original-consumer-execution-qualification.v1","job":root,"task_id":packet.task.id,"task_key":packet.task_key,"attempt_id":receipt.attempt_id,"operation":profile.admission.scope.operation,"activation":receipt.activation,"work":receipt.work,"retention_work":ack.work,"runtime":runtime,"old_report_close":closed,"controls_retained":true,"original_task_id_preserved":true,"completed_producer_packets_unchanged":true,"legacy_request_protocol_preserved":true,"pages":2,"units":6,"completed_replay_index_and_worker_result_unchanged":true,"waiting_claims_reopened":true,"after_checkpoint_sha256":canonical_digest(&complete).unwrap(),"next_original_report_task_key":next.task_key,"next_report_preflight":root.join("original-report-preflight"),"original_report_consumer_retained":false,"viewing_verified":false,"finding_closed":false,"selected":false,"approved":false,"scratch_retired":false}),
    );
    phase(
        &root,
        "retained-original-review-consumer-awaiting-original-report-rebuild",
    );
}

#[allow(clippy::too_many_arguments)]
fn prepare_report(
    root: &Path,
    baseline_report_root: &Path,
    old_control_root: &Path,
    producer: &Packet,
    profile: &handoff::Profile,
    result: &ReturnedResult,
    ack: &retention::Acknowledgement,
    inherited: &upstream::Parents,
) -> Packet {
    let next = root.join("original-report-preflight");
    fs::create_dir(&next).unwrap();
    let baseline: Plan = load(root, "baseline.json");
    let repair: review::Repair = load(root, "repair.json");
    let mut revision: repair::Revision = load(root, "revision.json");
    let before_packet: Packet = load(baseline_report_root, "packet.json");
    let before_result: ReturnedResult = load(baseline_report_root, "result.json");
    let before_profile: handoff::Profile = load(baseline_report_root, "profile.json");
    let before_ack: retention::Acknowledgement = load(baseline_report_root, "retained.json");
    let guard = retention::verify(
        &before_packet,
        &before_profile.source,
        &before_profile.admission,
        &before_profile.binding.admission,
        &before_result,
        &before_ack,
    )
    .unwrap();
    let checked = guard.review_report().unwrap().clone();
    drop(guard);
    let before_bindings = before_profile
        .source
        .snapshot
        .upstream
        .as_ref()
        .unwrap()
        .bindings
        .clone();
    let parents = upstream::Parents {
        schema: "reel.workflow-native-parents.v1".into(),
        parents: BTreeMap::from([
            ("opening".into(), copy(&inherited.parents["opening"])),
            ("closing".into(), copy(&inherited.parents["closing"])),
            (
                producer.task.id.clone(),
                upstream::ParentControl {
                    packet: producer.clone(),
                    result: result.clone(),
                    profile: copy(profile),
                    acknowledgement: ack.clone(),
                },
            ),
        ]),
    };
    let mut bindings = reel_workflow::upstream::Bindings {
        schema: "reel.workflow-upstream-inputs.v2".into(),
        streaming: None,
        inputs: Default::default(),
    };
    for reference in before_bindings.inputs.values() {
        let parent = &parents.parents[&reference.task_id];
        let pin = match &reference.artifact {
            reel_workflow::upstream::Selector::Output { name } => &parent.result.artifacts[name],
            reel_workflow::upstream::Selector::WorkerReceipt => {
                parent.result.worker_receipt.as_ref().unwrap()
            }
        };
        assert!(
            bindings
                .inputs
                .insert(pin.logical_id.clone(), reference.clone())
                .is_none()
        );
    }
    let review_copy: review::Copy = load(&root.join("verified"), "review-copy.json");
    let mut source: NativeProfile =
        reel_workflow_native::read_control(&root.join("source.json")).unwrap();
    source.snapshot = source
        .snapshot
        .fixed_subset(&std::collections::BTreeSet::from([review_copy
            .publication
            .logical_id
            .clone()]))
        .unwrap();
    source.snapshot.upstream = Some(upstream::Frozen::capture(
        &bindings,
        &parents.borrowed().unwrap(),
    ));
    let declaration = source.snapshot.declaration_bytes().unwrap();
    let native = revision
        .native_inputs
        .get_mut("whole-issue-structured-report")
        .unwrap();
    native.after = Artifact {
        sha256: digest(&declaration),
        bytes: declaration.len() as u64,
        ..native.before.clone()
    };
    revision.task_snapshots.insert(
        "whole-issue-structured-report".into(),
        source.snapshot.commit.clone(),
    );
    let old_packet: Packet = load(old_control_root, "packet.json");
    let old_result: ReturnedResult = load(old_control_root, "result.json");
    let derivation = revision.derivation.as_mut().unwrap();
    derivation.parents.insert(
        producer.task.id.clone(),
        repair::ParentChange {
            before: repair::ResultWitness {
                packet: old_packet,
                result: old_result,
            },
            after: repair::ResultWitness {
                packet: producer.clone(),
                result: result.clone(),
            },
        },
    );
    derivation.consumers.insert(
        "whole-issue-structured-report".into(),
        repair::ConsumerBindings {
            before: before_bindings,
            after: bindings.clone(),
        },
    );
    let candidate = repair::materialize(&baseline, &checked, &repair, &revision).unwrap();
    let mut completed: upstream::Parents = copy(inherited);
    completed.parents.insert(
        producer.task.id.clone(),
        upstream::ParentControl {
            packet: producer.clone(),
            result: result.clone(),
            profile: copy(profile),
            acknowledgement: ack.clone(),
        },
    );
    for parent in completed.parents.values() {
        assert_eq!(
            make_packet(
                &candidate.plan,
                &parent.packet.task.id,
                parent.packet.dependencies.clone()
            )
            .unwrap(),
            parent.packet
        );
    }
    let packet = make_packet(
        &candidate.plan,
        "whole-issue-structured-report",
        parents
            .parents
            .iter()
            .map(|(id, p)| {
                (
                    id.clone(),
                    DependencyResult {
                        result_sha256: canonical_digest(&p.result).unwrap(),
                        artifacts: p.result.artifacts.clone(),
                    },
                )
            })
            .collect(),
    )
    .unwrap();
    drop(SnapshotInputs::open(&packet, &source.snapshot).unwrap());
    let expected: std::collections::BTreeSet<_> = parents
        .parents
        .values()
        .flat_map(|p| p.result.artifacts.values())
        .filter(|pin| packet.task.inputs.contains(pin))
        .map(|p| p.logical_id.clone())
        .collect();
    assert_eq!(expected, bindings.inputs.keys().cloned().collect());
    let borrowed =
        upstream::BorrowedInputs::open(&packet, &bindings, parents.borrowed().unwrap()).unwrap();
    drop(borrowed);
    let mut admission = profile.admission.clone();
    admission.scope.operation =
        tig_store::Key::parse("rooted-original-report-consumer-rebuild").unwrap();
    let preview = admission::Prepared::open(
        &packet,
        &source,
        &admission,
        "original-report-rebuild-attempt-1",
    )
    .unwrap()
    .declared_preview()
    .unwrap();
    assert!(preview.declared_objects + 4 <= preview.changed_path_limit);
    for (name, raw) in [
        ("baseline.json", serde_json::to_vec(&baseline).unwrap()),
        ("repair.json", serde_json::to_vec(&repair).unwrap()),
        ("revision.json", serde_json::to_vec(&revision).unwrap()),
        ("candidate.json", serde_json::to_vec(&candidate).unwrap()),
        ("plan.json", serde_json::to_vec(&candidate.plan).unwrap()),
        ("packet.json", serde_json::to_vec(&packet).unwrap()),
        ("source.json", serde_json::to_vec(&source).unwrap()),
        ("parents.json", serde_json::to_vec(&parents).unwrap()),
        ("upstream.json", bindings.canonical_bytes().unwrap()),
        ("admission.json", serde_json::to_vec(&admission).unwrap()),
        (
            "declared-preview.json",
            serde_json::to_vec(&preview).unwrap(),
        ),
    ] {
        raw_save(&next, name, &raw);
    }
    // The saved parent controls remain unmodified. No synthetic result for the
    // original report or task-supersession declaration is manufactured here.
    packet
}
