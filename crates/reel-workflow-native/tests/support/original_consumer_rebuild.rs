//! Complete the original affected consumer declarations without renaming tasks
//! or retargeting any completed producer. Native preflight only, no admission.
use super::repair_control_native::check_waiting;
use super::repair_part_native::{load, raw_save, save};
use super::*;
use reel_workflow_native::{NativeProfile, admission, retention, upstream};
use tig_store::source_claims::protection::Index;

fn copy<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) -> T {
    decode(&serde_json::to_vec(value).unwrap()).unwrap()
}
fn parent(root: &Path) -> upstream::ParentControl {
    upstream::ParentControl {
        packet: load(root, "packet.json"),
        result: load(root, "result.json"),
        profile: load(root, "profile.json"),
        acknowledgement: load(root, "retained.json"),
    }
}
fn witness(parent: &upstream::ParentControl) -> repair::ResultWitness {
    repair::ResultWitness {
        packet: parent.packet.clone(),
        result: parent.result.clone(),
    }
}
#[test]
#[ignore = "Exact retained repaired issue/parents and baseline report only; materialize v4 original consumer declaration with explicit new-job tool edit and original task ID, verify completed producer packets and exact native origins, actual Index/Waiting unchanged; no close/admission/publication"]
fn original_review_consumer_rebinds_to_retained_producers_without_changing_their_packets() {
    let env = |key| PathBuf::from(std::env::var_os(key).expect(key));
    let issue_root = env("REEL_ORIGINAL_CONSUMER_ISSUE");
    let prior = env("REEL_ORIGINAL_CONSUMER_PREFLIGHT");
    let control_root = env("REEL_ORIGINAL_CONSUMER_CONTROL");
    let old_root = env("REEL_ORIGINAL_CONSUMER_OLD_PARENTS");
    let old_consumer_root = env("REEL_ORIGINAL_CONSUMER_OLD_CONTROL");
    let baseline_report_root = env("REEL_ORIGINAL_CONSUMER_BASELINE_REPORT");
    let waiting = env("REEL_ORIGINAL_CONSUMER_WAITING");
    let evidence = env("REEL_ORIGINAL_CONSUMER_EVIDENCE");
    let runtime = env("REEL_ORIGINAL_CONSUMER_RUNTIME");
    let issue = parent(&issue_root);
    assert_eq!(
        issue.packet.task_key,
        "69d0d1a011e1a088c283dc64d3e29146a4be3c1a2b5fd931d8e334fb9c536ac9"
    );
    let mut parents: upstream::Parents = load(&issue_root, "parents.json");
    parents
        .parents
        .insert(issue.packet.task.id.clone(), copy(&issue));
    let before = Index::open(
        &issue.profile.admission.index_root,
        &issue.acknowledgement.index_checkpoint,
    )
    .unwrap()
    .checkpoint();
    check_waiting(&waiting);
    let baseline_report = parent(&baseline_report_root);
    let guard = retention::verify(
        &baseline_report.packet,
        &baseline_report.profile.source,
        &baseline_report.profile.admission,
        &baseline_report.profile.binding.admission,
        &baseline_report.result,
        &baseline_report.acknowledgement,
    )
    .unwrap();
    let checked = guard.review_report().unwrap().clone();
    drop(guard);
    let baseline: Plan = load(&control_root, "baseline.json");
    let mut repair: review::Repair = load(&control_root, "repair.json");
    let mut revision: repair::Revision = load(&prior, "revision.json");
    let old: upstream::Parents = load(&old_root, "parents.json");
    let original_consumer = parent(&old_consumer_root);
    assert_eq!(
        original_consumer.packet.task.id,
        "owned-issue-review-controls"
    );
    let before_bindings = original_consumer
        .profile
        .source
        .snapshot
        .upstream
        .as_ref()
        .unwrap()
        .bindings
        .clone();
    let inherited: reel_workflow::upstream::Bindings = load(&issue_root, "upstream.json");
    let after_bindings =
        review_control::bindings(&issue.packet, &issue.result, Some(&inherited)).unwrap();
    let mut source: NativeProfile = copy(&issue.profile.source);
    source.snapshot.upstream = Some(upstream::Frozen::capture(
        &after_bindings,
        &parents.borrowed().unwrap(),
    ));
    let declaration = source.snapshot.declaration_bytes().unwrap();
    let native = revision
        .native_inputs
        .get_mut("owned-issue-review-controls")
        .unwrap();
    native.after = Artifact {
        sha256: digest(&declaration),
        bytes: declaration.len() as u64,
        ..native.before.clone()
    };
    revision.task_snapshots.insert(
        "owned-issue-review-controls".into(),
        issue.packet.source_commit.clone(),
    );
    let runtime_raw = fs::read(&runtime).unwrap();
    let original_tool = baseline
        .tasks
        .iter()
        .find(|t| t.id == "owned-issue-review-controls")
        .unwrap()
        .tool
        .as_ref()
        .unwrap();
    let tool = Artifact {
        sha256: digest(&runtime_raw),
        bytes: runtime_raw.len() as u64,
        ..original_tool.executable.clone()
    };
    assert_eq!(
        tool.sha256,
        "5c237208ccf8bc9fe13e2cc2ccdc154430c4d4f8ef07c51a613f5ab41a40abe1"
    );
    repair.edits.push(review::Edit {
        before: original_tool.executable.clone(),
        after: tool.clone(),
    });
    let derivation = revision.derivation.as_mut().unwrap();
    derivation.parents.insert(
        "issue-assemble".into(),
        repair::ParentChange {
            before: witness(&old.parents["issue-assemble"]),
            after: witness(&issue),
        },
    );
    derivation.consumers.insert(
        "owned-issue-review-controls".into(),
        repair::ConsumerBindings {
            before: before_bindings,
            after: after_bindings.clone(),
        },
    );
    let candidate = repair::materialize(&baseline, &checked, &repair, &revision).unwrap();
    for p in parents.parents.values() {
        let packet = make_packet(
            &candidate.plan,
            &p.packet.task.id,
            p.packet.dependencies.clone(),
        )
        .unwrap();
        assert_eq!(
            packet, p.packet,
            "completed producers cannot be reinterpreted"
        );
    }
    let packet = make_packet(
        &candidate.plan,
        "owned-issue-review-controls",
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
    assert_eq!(packet.task.id, original_consumer.packet.task.id);
    assert_eq!(packet.task.tool.as_ref().unwrap().executable, tool);
    assert_eq!(
        packet.task.tool.as_ref().unwrap().source_bindings,
        review_control::source_bindings(&issue.packet, &issue.result).unwrap()
    );
    let actual_inputs: BTreeMap<_, _> = packet
        .task
        .inputs
        .iter()
        .map(|p| (&p.logical_id, p))
        .collect();
    let expected = review_control::inputs(&issue.packet, &issue.result).unwrap();
    assert_eq!(
        actual_inputs,
        expected.iter().map(|p| (&p.logical_id, p)).collect()
    );
    drop(SnapshotInputs::open(&packet, &source.snapshot).unwrap());
    // Reopen each exact retained producer once, then retain the borrowed
    // declaration fences while checking original result selectors and pins.
    let borrowed =
        upstream::BorrowedInputs::open(&packet, &after_bindings, parents.borrowed().unwrap())
            .unwrap();
    let mut admission = issue.profile.admission.clone();
    admission.scope.operation =
        tig_store::Key::parse("rooted-original-review-consumer-rebuild").unwrap();
    let preview = admission::Prepared::open(
        &packet,
        &source,
        &admission,
        "original-review-rebuild-attempt-1",
    )
    .unwrap()
    .declared_preview()
    .unwrap();
    assert!(preview.declared_objects + 4 <= preview.changed_path_limit);
    drop(borrowed);
    check_waiting(&waiting);
    assert_eq!(
        before,
        Index::open(&issue.profile.admission.index_root, &before)
            .unwrap()
            .checkpoint()
    );
    let root = evidence.join("job");
    fs::create_dir(&root).unwrap();
    for (name, raw) in [
        ("baseline.json", serde_json::to_vec(&baseline).unwrap()),
        ("repair.json", serde_json::to_vec(&repair).unwrap()),
        ("revision.json", serde_json::to_vec(&revision).unwrap()),
        ("candidate.json", serde_json::to_vec(&candidate).unwrap()),
        ("plan.json", serde_json::to_vec(&candidate.plan).unwrap()),
        ("packet.json", serde_json::to_vec(&packet).unwrap()),
        ("source.json", serde_json::to_vec(&source).unwrap()),
        ("parents.json", serde_json::to_vec(&parents).unwrap()),
        ("upstream.json", after_bindings.canonical_bytes().unwrap()),
        ("admission.json", serde_json::to_vec(&admission).unwrap()),
        (
            "declared-preview.json",
            serde_json::to_vec(&preview).unwrap(),
        ),
    ] {
        raw_save(&root, name, &raw);
    }
    save(
        &evidence,
        "qualification-result.json",
        &json!({"schema":"reel.original-review-consumer-preflight.v1","task_id":packet.task.id,"task_key":packet.task_key,"attempt_id":"original-review-rebuild-attempt-1","operation":admission.scope.operation,"runtime":tool,"source_commit":packet.source_commit,"original_task_id_preserved":true,"completed_producer_packets_unchanged":true,"exact_retained_origins_borrowed":true,"explicit_new_job_runtime_edit":true,"legacy_review_request_protocol_preserved":true,"native_declaration_complete":true,"actual_index_unchanged":true,"checkpoint_sha256":canonical_digest(&before).unwrap(),"waiting_claims_reopened":true,"child_admitted":false,"controls_retained":false,"original_report_consumer_rebuilt":false,"finding_closed":false,"selected":false,"approved":false}),
    );
}
