//! Complete the pinned corrected issue under one original managed operation.
use super::repair_control_native::check_waiting;
use super::repair_part_native::{
    binding, load, native_bytes, phase, raw_save, save, success, verify_parent,
};
use super::*;
use reel_workflow::handoff::{Journal, State};
use reel_workflow_native::{
    NativeProfile, admission, dispatch, graph, handoff, retention, staging, upstream, worker,
};
use tig_store::source_claims::protection::Index;

fn verify_certificate(root: &Path) {
    let raw = fs::read(root.join("qualified-report.json")).unwrap();
    assert_eq!(
        digest(&raw),
        "d58310ce59f099bcc942c14197a467be21cfee59ca82cf1d65a86ae2ffc0054e"
    );
    let report: serde_json::Value = decode(&raw).unwrap();
    for pin in report["files"].as_array().unwrap() {
        let raw = fs::read(Path::new(pin["path"].as_str().unwrap())).unwrap();
        assert_eq!(raw.len() as u64, pin["bytes"].as_u64().unwrap());
        assert_eq!(digest(&raw), pin["sha256"].as_str().unwrap());
    }
}

#[test]
#[ignore = "Existing qualified corrected issue controls only; one exact slot2 admission and original 264 worker, reconcile any exact legacy owner-limit stop through retained recovery, verify full native issue/review targets and replay; preserves every root and Waiting job"]
fn pinned_corrected_issue_admits_executes_retains_and_replays_without_rebuilding_parents() {
    let env = |key| PathBuf::from(std::env::var_os(key).expect(key));
    let preflight = env("REEL_REPAIR_ISSUE_PREFLIGHT");
    let evidence = env("REEL_REPAIR_ISSUE_EXECUTION_EVIDENCE");
    let prepared_root = preflight.join("job");
    verify_certificate(&preflight);
    let qualified: serde_json::Value = load(&preflight, "qualification-result.json");
    let marker: serde_json::Value = load(&prepared_root, "original-operation.json");
    assert_eq!(marker["issue_admitted"], false);
    assert_eq!(marker["issue_worker_invoked"], false);
    assert_eq!(qualified["whole_corrected_issue_retained"], false);
    let packet: Packet = load(&prepared_root, "packet.json");
    let source: NativeProfile = load(&prepared_root, "source.json");
    let admission: admission::Profile = load(&prepared_root, "admission.json");
    let parents: upstream::Parents = load(&prepared_root, "parents.json");
    let runtime: Tool = decode(&serde_json::to_vec(&marker["runtime"]).unwrap()).unwrap();
    let waiting = PathBuf::from(marker["waiting"].as_str().unwrap());
    let control_root = PathBuf::from(marker["legacy_control"].as_str().unwrap());
    let attempt = marker["attempt_id"].as_str().unwrap();
    assert_eq!(packet.task_key, qualified["task_key"]);
    assert_eq!(
        packet.task_key,
        "69d0d1a011e1a088c283dc64d3e29146a4be3c1a2b5fd931d8e334fb9c536ac9"
    );
    assert_eq!(
        runtime.sha256,
        packet.task.tool.as_ref().unwrap().executable.sha256
    );
    assert_eq!(digest(&fs::read(&runtime.path).unwrap()), runtime.sha256);
    assert_eq!(
        admission.scope.operation.as_str(),
        "rooted-issue-assembly-original"
    );
    assert_eq!(attempt, "rooted-issue-attempt-1");
    let root = evidence.join("job");
    fs::create_dir(&root).unwrap();
    check_waiting(&waiting);
    for p in parents.parents.values() {
        verify_parent(p);
    }
    let before = Index::open(&admission.index_root, &admission.index_minimum)
        .unwrap()
        .checkpoint();
    assert_eq!(
        canonical_digest(&before).unwrap(),
        marker["after_close_checkpoint_sha256"]
    );
    let policy = retention::capacity::Policy {
        schema: "reel.workflow-retained-owner-policy.v1".into(),
        objects: 1024,
        logical_bytes: 1024 * 1024 * 1024,
    };
    save(&root, "declared-owner-recovery-policy.json", &policy);
    save(
        &root,
        "original-operation.json",
        &json!({"schema":"reel.corrected-issue-original-operation.v1","task_key":packet.task_key,"attempt_id":attempt,"scope":admission.scope,"runtime":runtime,"source_commit":packet.source_commit,"preflight":preflight,"waiting":waiting,"declared_owner_recovery_policy":policy,"no_fresh_identity_on_uncertainty":true}),
    );
    for name in [
        "packet.json",
        "source.json",
        "admission.json",
        "plan.json",
        "parents.json",
        "candidate.json",
        "revision.json",
        "upstream.json",
    ] {
        raw_save(&root, name, &fs::read(prepared_root.join(name)).unwrap());
    }
    phase(
        &root,
        "accept-one-original-corrected-issue-with-live-parent-custody",
    );
    let prepared = upstream::Admission::prepare(
        &packet,
        &source,
        &admission,
        attempt,
        parents.borrowed().unwrap(),
    )
    .unwrap();
    assert_eq!(
        prepared.preview().unwrap(),
        load::<admission::Preview>(&prepared_root, "admission-preview.json")
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
            schema: "reel.workflow-native-handoff-binding.v1".into(),
            retained_owner_policy: None,
            admission: receipt.clone(),
            target: parents.parents["issue-control"]
                .profile
                .binding
                .target
                .clone(),
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
    let mut bindings: graph::Bindings = load(&control_root, "bindings.json");
    bindings.tasks.insert(
        packet.task.id.clone(),
        binding(root.join("journal"), root.join("profile.json")),
    );
    save(&root, "bindings.json", &bindings);
    let staging_profile = |name: &str, maximum_bytes| {
        let parent = root.join(name);
        fs::create_dir(&parent).unwrap();
        staging::Profile {
            schema: "reel.workflow-staging.v1".into(),
            parent,
            maximum_bytes,
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
        input_staging: staging_profile("input-workspaces", 64 * 1024 * 1024),
        worker_staging: Some(staging_profile("worker-workspaces", 512 * 1024 * 1024)),
        provider: None,
        auxiliary_programs: BTreeMap::new(),
    };
    save(&root, "dispatch-config.json", &config);
    phase(&root, "dispatch-once-with-original-qualified-264-worker");
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
    save(
        &root,
        "first-dispatch-result.json",
        &json!({"exit_code":out.status.code(),"stdout":String::from_utf8_lossy(&out.stdout),"stderr":String::from_utf8_lossy(&out.stderr)}),
    );
    finish(
        &root,
        &evidence,
        &preflight,
        &out,
        &root.join("dispatch/worker-ticket.json"),
    );
}

#[test]
#[ignore = "Only the exact recorded pre-worker reservation refusal; reopens the existing Waiting admission, preserves empty failed ticket/config, prepares its first owned worker with explicit 512 MiB profile using original 264 runtime; no readmission or replacement"]
fn original_waiting_issue_recovers_preworker_reservation_refusal_without_replacing_dispatch() {
    let original = PathBuf::from(std::env::var_os("REEL_REPAIR_ISSUE_REFUSED_EXECUTION").unwrap());
    let evidence = PathBuf::from(std::env::var_os("REEL_REPAIR_ISSUE_WORKER_RECOVERY").unwrap());
    let root = original.join("job");
    let process: serde_json::Value = load(&original, "process-result.json");
    assert_eq!(process["exit_code"], 101);
    let failed: serde_json::Value = load(&root, "first-dispatch-result.json");
    let refusal: serde_json::Value = decode(failed["stdout"].as_str().unwrap().as_bytes()).unwrap();
    assert_eq!(
        refusal["error"]["code"],
        "worker-output-and-evidence-reservation"
    );
    assert_eq!(refusal["error"]["kind"], "budget");
    assert!(
        fs::read(root.join("dispatch/worker-ticket.json"))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fs::read_dir(root.join("worker-workspaces"))
            .unwrap()
            .count(),
        0
    );
    let marker: serde_json::Value = load(&root, "original-operation.json");
    let preflight = PathBuf::from(marker["preflight"].as_str().unwrap());
    verify_certificate(&preflight);
    let packet: Packet = load(&root, "packet.json");
    assert_eq!(
        packet.task_key,
        "69d0d1a011e1a088c283dc64d3e29146a4be3c1a2b5fd931d8e334fb9c536ac9"
    );
    let profile: handoff::Profile = load(&root, "profile.json");
    let journal = Journal::open(&root.join("journal")).unwrap();
    assert_eq!(journal.state(), State::Waiting);
    assert!(journal.result().is_none());
    drop(
        admission::Prepared::open(
            &packet,
            &profile.source,
            &profile.admission,
            &profile.binding.admission.attempt_id,
        )
        .unwrap()
        .reopen(&profile.binding.admission)
        .unwrap(),
    );
    check_waiting(&PathBuf::from(marker["waiting"].as_str().unwrap()));
    let mut pins = BTreeMap::new();
    for name in [
        "packet.json",
        "source.json",
        "admission.json",
        "receipt.json",
        "profile.json",
        "plan.json",
        "parents.json",
        "dispatch-config.json",
        "first-dispatch-result.json",
        "dispatch/intent.json",
        "dispatch/worker-ticket.json",
        "dispatch/ready.json",
        "dispatch/ready-pin.json",
        "dispatch/input-ticket.json",
        "dispatch/input-ticket-pin.json",
        "dispatch/plan.json",
        "dispatch/bindings.json",
        "journal/00000.json",
    ] {
        pins.insert(name.to_owned(), digest(&fs::read(root.join(name)).unwrap()));
    }
    save(&evidence, "original-pins.json", &pins);
    let before = Index::open(
        &profile.admission.index_root,
        &profile.binding.admission.index_checkpoint,
    )
    .unwrap()
    .checkpoint();
    let config: dispatch::Config = load(&root, "dispatch-config.json");
    let mut worker = config.worker_staging.unwrap();
    assert_eq!(worker.maximum_bytes, 64 * 1024 * 1024);
    worker.maximum_bytes = 512 * 1024 * 1024;
    save(&evidence, "worker-profile.json", &worker);
    save(
        &evidence,
        "original-worker-recovery-intent.json",
        &json!({"schema":"reel.preworker-reservation-recovery.v1","original_job":root,"task_key":packet.task_key,"attempt_id":profile.binding.admission.attempt_id,"operation":profile.admission.scope.operation,"activation":profile.binding.admission.activation,"work":profile.binding.admission.work,"runtime":profile.runtime,"before_checkpoint_sha256":canonical_digest(&before).unwrap(),"original_worker_ticket_empty":true,"original_worker_parent_empty":true,"existing_dispatch_config_preserved":true,"readmission":false,"worker_profile":worker,"first_worker_reservation_only":true}),
    );
    let batch: graph::PinnedFile = load(&root.join("dispatch"), "ready-pin.json");
    let ticket = evidence.join("worker-ticket.json");
    let out = Command::new(&profile.runtime.path)
        .arg("prepare-worker")
        .arg(root.join("dispatch/plan.json"))
        .arg(root.join("dispatch/bindings.json"))
        .arg(&batch.path)
        .arg(&batch.sha256)
        .arg(&packet.task.id)
        .arg(evidence.join("worker-profile.json"))
        .arg(&ticket)
        .arg("--apply")
        .output()
        .unwrap();
    success(&evidence, "prepare-worker-result.json", &out);
    let ticket_pin = digest(&fs::read(&ticket).unwrap());
    let out = Command::new(&profile.runtime.path)
        .arg("run-worker")
        .arg(root.join("dispatch/plan.json"))
        .arg(root.join("dispatch/bindings.json"))
        .arg(&ticket)
        .arg(&ticket_pin)
        .arg("--apply")
        .output()
        .unwrap();
    save(
        &evidence,
        "worker-result.json",
        &json!({"exit_code":out.status.code(),"stdout":String::from_utf8_lossy(&out.stdout),"stderr":String::from_utf8_lossy(&out.stderr)}),
    );
    finish(&root, &evidence, &preflight, &out, &ticket);
    for (name, pin) in pins {
        assert_eq!(digest(&fs::read(root.join(name)).unwrap()), pin);
    }
    assert!(
        fs::read(root.join("dispatch/worker-ticket.json"))
            .unwrap()
            .is_empty()
    );
    save(
        &evidence,
        "original-controls-after-recovery.json",
        &json!({"all_original_pins_unchanged":true,"empty_dispatch_worker_ticket_preserved":true,"original_task_attempt_operation_activation_and_runtime_preserved":true,"readmission":false,"original_worker_execution_started_once":true}),
    );
}

fn finish(
    root: &Path,
    evidence: &Path,
    preflight: &Path,
    out: &std::process::Output,
    ticket_path: &Path,
) {
    let packet: Packet = load(root, "packet.json");
    let profile: handoff::Profile = load(root, "profile.json");
    let receipt: admission::Receipt = load(root, "receipt.json");
    let parents: upstream::Parents = load(root, "parents.json");
    let marker: serde_json::Value = load(root, "original-operation.json");
    let waiting = PathBuf::from(marker["waiting"].as_str().unwrap());
    let prepared_root = preflight.join("job");
    let runtime = &profile.runtime;
    let attempt = profile.binding.admission.attempt_id.as_str();
    let policy: retention::capacity::Policy = load(root, "declared-owner-recovery-policy.json");
    let journal = Journal::open(&root.join("journal")).unwrap();
    let result = journal
        .result()
        .expect("original worker result recorded before import effects")
        .clone();
    check_result(&packet, &result).unwrap();
    assert_eq!(result.attempt_id, attempt);
    save(root, "result.json", &result);
    let ticket: worker::Ticket = decode(&fs::read(ticket_path).unwrap()).unwrap();
    assert_eq!(
        load::<ReturnedResult>(&ticket.directory(), "result.json"),
        result
    );
    let original_worker_result = digest(&fs::read(ticket.directory().join("result.json")).unwrap());
    let mut recovery_used = false;
    if !out.status.success() {
        let failure: serde_json::Value = decode(&out.stdout).unwrap();
        assert_eq!(failure["error"]["kind"], "budget");
        assert_eq!(
            failure["error"]["code"],
            "handoff-retain:retention-apply:retention-resource-bound"
        );
        assert_eq!(journal.state(), State::ImportPending);
        assert!(journal.acknowledgement().is_none());
        phase(root, "stage-explicit-original-result-owner-recovery");
        let preview = retention::capacity_preview(
            &packet,
            &profile.source,
            &profile.admission,
            &receipt,
            &result,
            &profile.binding.target,
        )
        .unwrap();
        assert!(!preview.resource_bound_ok && !preview.original_retention_work_exists);
        save(root, "retention-capacity.json", &preview);
        let recovery = retention::capacity::prepare(
            &packet,
            &profile.source,
            &profile.admission,
            &receipt,
            &result,
            &profile.binding.target,
            policy,
        )
        .unwrap();
        let recovery_raw = recovery.canonical_bytes().unwrap();
        let recovery_pin = digest(&recovery_raw);
        raw_save(root, "retained-owner-plan.json", &recovery_raw);
        let staged = retention::capacity::stage(
            &recovery,
            &recovery_pin,
            &packet,
            &profile.source,
            &profile.admission,
            &receipt,
            &result,
            &profile.binding.target,
        )
        .unwrap();
        save(root, "staged-retention.json", &staged);
        let out = Command::new(&runtime.path)
            .arg("resume-handoff")
            .arg(root.join("journal"))
            .arg(root.join("profile.json"))
            .arg("-")
            .arg("--apply")
            .output()
            .unwrap();
        success(root, "original-retention-resume.json", &out);
        recovery_used = true;
    } else {
        assert_eq!(journal.state(), State::Retained);
    }
    let complete = Journal::open(&root.join("journal")).unwrap();
    assert_eq!(complete.state(), State::Retained);
    assert_eq!(complete.result(), Some(&result));
    let ack: retention::Acknowledgement = load(
        &profile.acknowledgements,
        &format!("retained-{}-{attempt}.json", packet.task_key),
    );
    save(root, "retained.json", &ack);
    phase(
        root,
        "verify-complete-native-issue-and-private-review-targets",
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
    for name in ["reading.pdf", "transcript.json"] {
        let raw = native_bytes(&profile.source, &result.artifacts[name]);
        assert_eq!(
            raw,
            fs::read(prepared_root.join("preflight").join(name)).unwrap()
        );
        raw_save(&root.join("verified"), name, &raw);
    }
    let review = retention::review_copy(
        &packet,
        &profile.source,
        &profile.admission,
        &receipt,
        &result,
        &ack,
    )
    .unwrap();
    assert_eq!(review.copy().units.len(), 6);
    match &review.copy().format {
        review::Format::Comic { page_order } => assert_eq!(page_order.len(), 2),
        _ => panic!("comic review required"),
    };
    save(root, "retained-review-copy.json", review.copy());
    raw_save(
        root,
        "retained-review-manifest.json",
        review.manifest().expect("complete ordered comic manifest"),
    );
    drop(review);
    for p in parents.parents.values() {
        verify_parent(p);
    }
    check_waiting(&waiting);
    verify_certificate(preflight);
    let checkpoint = Index::open(&profile.admission.index_root, &ack.index_checkpoint)
        .unwrap()
        .checkpoint();
    let out = Command::new(&runtime.path)
        .arg("resume-handoff")
        .arg(root.join("journal"))
        .arg(root.join("profile.json"))
        .arg("-")
        .arg("--apply")
        .output()
        .unwrap();
    success(root, "completed-replay.json", &out);
    assert_eq!(
        Index::open(&profile.admission.index_root, &checkpoint)
            .unwrap()
            .checkpoint(),
        checkpoint
    );
    assert_eq!(
        digest(&fs::read(ticket.directory().join("result.json")).unwrap()),
        original_worker_result
    );
    save(
        evidence,
        "qualification-result.json",
        &json!({"schema":"reel.native-corrected-whole-issue-qualification.v1","job":root,"task_key":packet.task_key,"attempt_id":attempt,"operation":profile.admission.scope.operation,"activation":receipt.activation,"work":receipt.work,"runtime":runtime,"source_commit":packet.source_commit,"retention_work":ack.work,"explicit_owner_recovery_used":recovery_used,"original_worker_result_sha256":original_worker_result,"complete_issue_bytes_match_preflight":true,"parent_history_preserved":true,"waiting_claims_reopened":true,"review_pages":2,"review_units":6,"whole_corrected_issue_retained":true,"completed_replay_index_unchanged":true,"after_checkpoint_sha256":canonical_digest(&checkpoint).unwrap(),"contextual_report_retained":false,"finding_closed":false,"selected":false,"approved":false,"scratch_retired":false}),
    );
    phase(
        root,
        "retained-corrected-whole-issue-awaiting-contextual-report",
    );
}
