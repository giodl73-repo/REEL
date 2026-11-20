//! Retain the exact preflighted corrected-issue review controls, then declare
//! their report with actual retained origins. Never readmit an original job.
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
use tig_store::source_claims::protection::Index;

fn certificate(root: &Path) {
    let raw = fs::read(root.join("qualified-report.json")).unwrap();
    assert_eq!(
        digest(&raw),
        "31146215a2de7d16c949b213ad1b0a67756246c6ef6569f573cf4150f8cb6d36"
    );
    let value: serde_json::Value = decode(&raw).unwrap();
    for pin in value["files"].as_array().unwrap() {
        let raw = fs::read(pin["path"].as_str().unwrap()).unwrap();
        assert_eq!(raw.len() as u64, pin["bytes"].as_u64().unwrap());
        assert_eq!(digest(&raw), pin["sha256"].as_str().unwrap());
    }
}
fn observe(profile: &admission::Profile) -> tig_store::source_claims::Checkpoint {
    Index::open(&profile.index_root, &profile.index_minimum)
        .unwrap()
        .checkpoint()
}

#[test]
#[ignore = "Exact certified corrected-issue context controls only; checked original-policy issue close then one slot2 control admission with original pinned 3fa runtime and finite owner policy, actual retention/replay and report-origin preflight; preserves every root and Waiting job; no report admission/provider/viewing/closure"]
fn pinned_corrected_issue_controls_retain_and_declare_their_exact_context_report() {
    let env = |key| PathBuf::from(std::env::var_os(key).expect(key));
    let preflight = env("REEL_CORRECTED_CONTROL_PREFLIGHT");
    let original = env("REEL_CORRECTED_CONTROL_ISSUE");
    let evidence = env("REEL_CORRECTED_CONTROL_EVIDENCE");
    let waiting = env("REEL_CORRECTED_CONTROL_WAITING");
    certificate(&preflight);
    let ready = preflight.join("job");
    let packet: Packet = load(&ready, "packet.json");
    let source: NativeProfile = load(&ready, "source.json");
    let admission: admission::Profile = load(&ready, "admission.json");
    let parents: upstream::Parents = load(&ready, "parents.json");
    let runtime: Tool = decode(&serde_json::to_vec(&json!({"path":env("REEL_CORRECTED_CONTROL_RUNTIME"),"sha256":packet.task.tool.as_ref().unwrap().executable.sha256,"bytes":packet.task.tool.as_ref().unwrap().executable.bytes})).unwrap()).unwrap();
    assert_eq!(
        packet.task_key,
        "ffed8eb7773a4dcf1e914fa3659ba4a9341c2846e3cd8efc60e78894e3a8760c"
    );
    assert_eq!(
        runtime.sha256,
        "3fa07fff5f99eeaeb60bcbcafd57b6c1b809d265bdbd8b1c0c3ebaac309d244e"
    );
    assert_eq!(digest(&fs::read(&runtime.path).unwrap()), runtime.sha256);
    let old_packet: Packet = load(&original, "packet.json");
    let old_profile: handoff::Profile = load(&original, "profile.json");
    assert_eq!(
        Journal::open(&original.join("journal")).unwrap().state(),
        State::Retained
    );
    assert_eq!(parents.parents[&old_packet.task.id].packet, old_packet);
    assert_eq!(admission.scope.actor, old_profile.admission.scope.actor);
    assert_ne!(
        admission.scope.operation,
        old_profile.admission.scope.operation
    );
    let before = observe(&admission);
    assert_eq!(
        canonical_digest(&before).unwrap(),
        "b4135560a030589390191369b56652b56051493c9d467e1f63e52625afb3152e"
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
        &json!({"schema":"reel.corrected-context-control-original.v1","task_key":packet.task_key,"attempt_id":"corrected-review-attempt-1","operation":admission.scope.operation,"actor":admission.scope.actor,"source_commit":packet.source_commit,"runtime":runtime,"original_issue":old_packet.task_key,"original_issue_runtime":old_profile.runtime,"preflight":preflight,"waiting":waiting,"owner_policy":policy,"no_fresh_identity_on_uncertainty":true}),
    );
    for name in [
        "packet.json",
        "plan.json",
        "source.json",
        "parents.json",
        "upstream.json",
        "admission.json",
    ] {
        raw_save(&root, name, &fs::read(ready.join(name)).unwrap());
    }
    phase(
        &root,
        "checked-original-issue-policy-close-before-new-control-admission",
    );
    let close = root.join("issue-close");
    fs::create_dir(&close).unwrap();
    save(&close, "target.json", &old_profile.binding.target);
    let retained_policy: retention::capacity::Plan = load(&original, "retained-owner-plan.json");
    let raw = retained_policy.canonical_bytes().unwrap();
    let policy_pin = digest(&raw);
    raw_save(&close, "policy.json", &raw);
    let out = Command::new(&runtime.path)
        .arg("prepare-close-with-policy")
        .arg(original.join("journal"))
        .arg(original.join("profile.json"))
        .arg(close.join("target.json"))
        .arg(close.join("policy.json"))
        .arg(&policy_pin)
        .arg(close.join("ticket.json"))
        .arg("--apply")
        .output()
        .unwrap();
    success(&close, "prepare-result.json", &out);
    let ticket_pin = digest(&fs::read(close.join("ticket.json")).unwrap());
    let out = Command::new(&runtime.path)
        .arg("stage-close")
        .arg(close.join("ticket.json"))
        .arg(&ticket_pin)
        .arg(close.join("staged.json"))
        .arg("--apply")
        .output()
        .unwrap();
    success(&close, "stage-result.json", &out);
    let staged: closing::Outcome = load(&close, "staged.json");
    assert!(staged.handoff.is_some() && !staged.slot_idle);
    let out = Command::new(&runtime.path)
        .arg("recover-close")
        .arg(close.join("ticket.json"))
        .arg(&ticket_pin)
        .arg(close.join("closed.json"))
        .arg("--apply")
        .output()
        .unwrap();
    success(&close, "recover-result.json", &out);
    let closed: closing::Outcome = load(&close, "closed.json");
    assert!(closed.slot_idle && closed.original_clear_complete);
    assert_eq!(closed.handoff, staged.handoff);
    let after_close = observe(&admission);
    let out = Command::new(&old_profile.runtime.path)
        .arg("resume-handoff")
        .arg(original.join("journal"))
        .arg(original.join("profile.json"))
        .arg("-")
        .arg("--apply")
        .output()
        .unwrap();
    success(&close, "original-runtime-historical-replay.json", &out);
    assert_eq!(after_close, observe(&admission));
    phase(
        &root,
        "accept-one-pinned-control-with-explicit-finite-owner-policy",
    );
    let prepared = upstream::Admission::prepare(
        &packet,
        &source,
        &admission,
        "corrected-review-attempt-1",
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
            target: old_profile.binding.target.clone(),
        },
        runtime: runtime.clone(),
        acknowledgements,
    };
    save(&root, "profile.json", &profile);
    let out = Command::new(&runtime.path)
        .arg("create-handoff")
        .arg(root.join("packet.json"))
        .arg(root.join("profile.json"))
        .arg(root.join("journal"))
        .arg("--apply")
        .output()
        .unwrap();
    success(&root, "create-result.json", &out);
    let mut bindings: graph::Bindings = load(&original, "bindings.json");
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
    phase(&root, "dispatch-once-with-original-preflighted-3fa-runtime");
    let out = Command::new(&runtime.path)
        .arg("dispatch-task")
        .arg(root.join("plan.json"))
        .arg(root.join("bindings.json"))
        .arg(&packet.task.id)
        .arg(root.join("dispatch-config.json"))
        .arg(root.join("dispatch"))
        .arg("--apply")
        .output()
        .unwrap();
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
    phase(&root, "verify-native-v2-controls-and-producer-chain");
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
        let raw = native_bytes(&profile.source, &result.artifacts[name]);
        assert_eq!(raw, fs::read(ready.join(name)).unwrap());
        raw_save(&root.join("verified"), name, &raw);
    }
    let request: review::Request = load(&root.join("verified"), "review-request.json");
    assert_eq!(request.schema, "reel.workflow-review-request.v2");
    assert_eq!(request.production.as_ref().unwrap().packet, old_packet);
    let ticket: worker::Ticket = load(&root.join("dispatch"), "worker-ticket.json");
    let worker_result = digest(&fs::read(ticket.directory().join("result.json")).unwrap());
    let complete = observe(&profile.admission);
    let out = Command::new(&runtime.path)
        .arg("resume-handoff")
        .arg(root.join("journal"))
        .arg(root.join("profile.json"))
        .arg("-")
        .arg("--apply")
        .output()
        .unwrap();
    success(&root, "completed-replay.json", &out);
    assert_eq!(complete, observe(&profile.admission));
    assert_eq!(
        digest(&fs::read(ticket.directory().join("result.json")).unwrap()),
        worker_result
    );
    for p in parents.parents.values() {
        verify_parent(p);
    }
    check_waiting(&waiting);
    certificate(&preflight);
    let report = report_preflight(&root, &ready, &packet, &profile, &result, &ack, &parents);
    assert_eq!(complete, observe(&profile.admission));
    save(
        &evidence,
        "qualification-result.json",
        &json!({"schema":"reel.corrected-context-control-qualification.v1","job":root,"task_key":packet.task_key,"attempt_id":receipt.attempt_id,"operation":profile.admission.scope.operation,"activation":receipt.activation,"work":receipt.work,"runtime":runtime,"retention_work":ack.work,"original_issue_close":closed,"after_checkpoint_sha256":canonical_digest(&complete).unwrap(),"controls_retained":true,"native_control_bytes_equal_preflight":true,"producer_bound_request":true,"completed_replay_index_and_worker_result_unchanged":true,"parent_history_preserved":true,"waiting_claims_reopened":true,"report_task_key":report.task_key,"report_preflight":root.join("report-preflight"),"report_admitted":false,"report_retained":false,"viewing_verified":false,"finding_closed":false,"selected":false,"approved":false,"scratch_retired":false}),
    );
    phase(
        &root,
        "retained-bound-controls-awaiting-context-report-admission",
    );
}

fn report_preflight(
    root: &Path,
    ready: &Path,
    producer: &Packet,
    profile: &handoff::Profile,
    produced: &ReturnedResult,
    ack: &retention::Acknowledgement,
    inherited: &upstream::Parents,
) -> Packet {
    let next = root.join("report-preflight");
    fs::create_dir(&next).unwrap();
    let copy: review::Copy = load(&root.join("verified"), "review-copy.json");
    let sources = fs::read(root.join("verified/review-sources.json")).unwrap();
    let images = review::comic_reading_inputs(&copy, &sources).unwrap();
    assert_eq!(images.len(), 2);
    let mut parents = upstream::Parents {
        schema: "reel.workflow-native-parents.v1".into(),
        parents: Default::default(),
    };
    let mut origins = reel_workflow::upstream::Bindings {
        schema: "reel.workflow-upstream-inputs.v2".into(),
        streaming: None,
        inputs: Default::default(),
    };
    let mut inputs = vec![copy.publication.clone()];
    for (name, pin) in &produced.artifacts {
        inputs.push(pin.clone());
        origins.inputs.insert(
            pin.logical_id.clone(),
            reel_workflow::upstream::Reference {
                task_id: producer.task.id.clone(),
                artifact: reel_workflow::upstream::Selector::Output { name: name.clone() },
            },
        );
    }
    for image in &images {
        let matches: Vec<_> = inherited
            .parents
            .iter()
            .flat_map(|(id, p)| {
                p.result
                    .artifacts
                    .iter()
                    .filter(move |(_, pin)| *pin == image)
                    .map(move |(name, _)| (id, name))
            })
            .collect();
        assert_eq!(matches.len(), 1);
        let (id, name) = matches[0];
        assert!(matches!(id.as_str(), "opening" | "closing"));
        parents.parents.insert(
            id.clone(),
            decode(&serde_json::to_vec(&inherited.parents[id]).unwrap()).unwrap(),
        );
        inputs.push(image.clone());
        origins.inputs.insert(
            image.logical_id.clone(),
            reel_workflow::upstream::Reference {
                task_id: id.clone(),
                artifact: reel_workflow::upstream::Selector::Output { name: name.clone() },
            },
        );
    }
    parents.parents.insert(
        producer.task.id.clone(),
        upstream::ParentControl {
            packet: producer.clone(),
            result: produced.clone(),
            profile: decode(&serde_json::to_vec(profile).unwrap()).unwrap(),
            acknowledgement: ack.clone(),
        },
    );
    let mut source: NativeProfile = decode(&serde_json::to_vec(&profile.source).unwrap()).unwrap();
    source.snapshot = source
        .snapshot
        .fixed_subset(&std::collections::BTreeSet::from([copy
            .publication
            .logical_id
            .clone()]))
        .unwrap();
    source.snapshot.upstream = Some(upstream::Frozen::capture(
        &origins,
        &parents.borrowed().unwrap(),
    ));
    let declaration = source.snapshot.declaration_bytes().unwrap();
    let raw = origins.canonical_bytes().unwrap();
    let task = Task { id: "corrected-issue-context-report".into(), native_inputs: Some(Artifact { logical_id: "control/native-input-bindings".into(), sha256: digest(&declaration), bytes: declaration.len() as u64, media_type: "application/json".into() }), upstream_inputs: Some(Artifact { logical_id: "control/corrected-context-report-origins".into(), sha256: digest(&raw), bytes: raw.len() as u64, media_type: "application/json".into() }), operation: Operation::ReviewReport, tool: None, language: Some(copy.language.clone()), prompt: "Synthetic full corrected-issue technical context. Preserve exact Spanish wording and all page/panel/balloon IDs. This protocol fixture asserts no model viewing, creative defect, approval or finding resolution.".into(), inputs, depends_on: parents.parents.keys().cloned().collect(), outputs: BTreeMap::from([("report".into(), OutputContract { media_type: "application/json".into(), required: true, max_bytes: MAX_CONTROL_BYTES as u64 })]) };
    let mut plan: Plan = load(ready, "plan.json");
    plan.task_snapshots
        .insert(task.id.clone(), producer.source_commit.clone());
    plan.tasks.push(task);
    let packet = make_packet(
        &plan,
        "corrected-issue-context-report",
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
    let semantic = parents
        .parents
        .iter()
        .map(|(id, p)| {
            (
                id.clone(),
                reel_workflow::upstream::Parent {
                    packet: &p.packet,
                    result: &p.result,
                },
            )
        })
        .collect();
    let request: review::Request = load(&root.join("verified"), "review-request.json");
    review_control::check_request_origin(
        &packet,
        request.production.as_ref().unwrap(),
        &origins,
        &semantic,
    )
    .unwrap();
    let mut admission = profile.admission.clone();
    admission.scope.operation =
        tig_store::Key::parse("corrected-issue-context-report-original").unwrap();
    let preview =
        admission::Prepared::open(&packet, &source, &admission, "corrected-context-attempt-1")
            .unwrap()
            .declared_preview()
            .unwrap();
    for (name, raw) in [
        ("packet.json", serde_json::to_vec(&packet).unwrap()),
        ("plan.json", serde_json::to_vec(&plan).unwrap()),
        ("source.json", serde_json::to_vec(&source).unwrap()),
        ("parents.json", serde_json::to_vec(&parents).unwrap()),
        ("upstream.json", origins.canonical_bytes().unwrap()),
        ("admission.json", serde_json::to_vec(&admission).unwrap()),
        (
            "declared-preview.json",
            serde_json::to_vec(&preview).unwrap(),
        ),
    ] {
        raw_save(&next, name, &raw);
    }
    packet
}
