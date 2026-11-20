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
    let staging_profile = |name: &str| {
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
        input_staging: staging_profile("input-workspaces"),
        worker_staging: Some(staging_profile("worker-workspaces")),
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
    let journal = Journal::open(&root.join("journal")).unwrap();
    let result = journal
        .result()
        .expect("original worker result recorded before import effects")
        .clone();
    check_result(&packet, &result).unwrap();
    assert_eq!(result.attempt_id, attempt);
    save(&root, "result.json", &result);
    let ticket: worker::Ticket = load(&root.join("dispatch"), "worker-ticket.json");
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
        phase(&root, "stage-explicit-original-result-owner-recovery");
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
        save(&root, "retention-capacity.json", &preview);
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
        raw_save(&root, "retained-owner-plan.json", &recovery_raw);
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
        save(&root, "staged-retention.json", &staged);
        let out = Command::new(&runtime.path)
            .arg("resume-handoff")
            .arg(root.join("journal"))
            .arg(root.join("profile.json"))
            .arg("-")
            .arg("--apply")
            .output()
            .unwrap();
        success(&root, "original-retention-resume.json", &out);
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
    save(&root, "retained.json", &ack);
    phase(
        &root,
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
    save(&root, "retained-review-copy.json", review.copy());
    raw_save(
        &root,
        "retained-review-manifest.json",
        review.manifest().expect("complete ordered comic manifest"),
    );
    drop(review);
    for p in parents.parents.values() {
        verify_parent(p);
    }
    check_waiting(&waiting);
    verify_certificate(&preflight);
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
    success(&root, "completed-replay.json", &out);
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
        &evidence,
        "qualification-result.json",
        &json!({"schema":"reel.native-corrected-whole-issue-qualification.v1","job":root,"task_key":packet.task_key,"attempt_id":attempt,"operation":profile.admission.scope.operation,"activation":receipt.activation,"work":receipt.work,"runtime":runtime,"source_commit":packet.source_commit,"retention_work":ack.work,"explicit_owner_recovery_used":recovery_used,"original_worker_result_sha256":original_worker_result,"complete_issue_bytes_match_preflight":true,"parent_history_preserved":true,"waiting_claims_reopened":true,"review_pages":2,"review_units":6,"whole_corrected_issue_retained":true,"completed_replay_index_unchanged":true,"after_checkpoint_sha256":canonical_digest(&checkpoint).unwrap(),"contextual_report_retained":false,"finding_closed":false,"selected":false,"approved":false,"scratch_retired":false}),
    );
    phase(
        &root,
        "retained-corrected-whole-issue-awaiting-contextual-report",
    );
}
