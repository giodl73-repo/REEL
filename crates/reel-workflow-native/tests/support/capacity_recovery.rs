//! Original-operation recovery of the preserved synthetic capacity boundary.
use super::repair_control_native::check_waiting;
use super::repair_part_native::{
    load, native_bytes, phase, raw_save, save, success, verify_parent,
};
use super::*;
use reel_workflow_native::{handoff, retention, upstream};
use tig_store::{
    metadata::MetadataStore,
    source_claims::protection::{Index, work},
};

#[test]
#[ignore = "Existing original ImportPending control only; pin and stage its finite capacity policy, reconstruct the SAME work after process exit, then resume with original 264 runtime; no render, publication restart, actor enrollment or history release"]
fn staged_capacity_policy_resumes_original_control_with_frozen_runtime_and_preserves_history() {
    let env = |key| PathBuf::from(std::env::var_os(key).expect(key));
    let original = env("REEL_REPAIR_CONTROL_PENDING");
    let planned = env("REEL_REPAIR_CONTROL_PLAN_EVIDENCE");
    let evidence = env("REEL_REPAIR_CONTROL_RECOVERY_EVIDENCE");
    let waiting = env("REEL_REPAIR_CONTROL_WAITING");
    let coordinator = env("REEL_NATIVE_HANDOFF_TOOL");
    check_waiting(&waiting);
    let packet: Packet = load(&original, "packet.json");
    let profile: handoff::Profile = load(&original, "profile.json");
    let journal = reel_workflow::handoff::Journal::open(&original.join("journal")).unwrap();
    assert_eq!(
        journal.state(),
        reel_workflow::handoff::State::ImportPending
    );
    assert!(journal.acknowledgement().is_none());
    assert_eq!(journal.ticket().packet, packet);
    assert_eq!(
        journal.ticket().native_binding,
        profile.binding.artifact().unwrap()
    );
    let result = journal.result().unwrap().clone();
    assert_eq!(
        profile.runtime.sha256,
        "264efd3655caa8f0190ab00548e6daff3a3b2a7a457261fd7d27c68d46e12b9a"
    );
    assert_eq!(
        digest(&fs::read(&profile.runtime.path).unwrap()),
        profile.runtime.sha256
    );
    assert_eq!(
        packet.task.tool.as_ref().unwrap().executable.sha256,
        profile.runtime.sha256
    );
    let plan: retention::capacity::Plan = load(&planned, "plan.json");
    let plan_raw = plan.canonical_bytes().unwrap();
    let plan_pin = digest(&plan_raw);
    let qualified: serde_json::Value = load(&planned, "qualification-result.json");
    assert_eq!(qualified["plan_sha256"], plan_pin);
    retention::capacity::recheck(
        &plan,
        &plan_pin,
        &packet,
        &profile.source,
        &profile.admission,
        &profile.binding.admission,
        &result,
        &profile.binding.target,
    )
    .unwrap();
    raw_save(&evidence, "plan.json", &plan_raw);
    save(&evidence, "result.json", &result);
    save(&evidence, "target.json", &profile.binding.target);
    let controls: BTreeMap<PathBuf, String> = [
        "packet.json",
        "source.json",
        "admission.json",
        "receipt.json",
        "profile.json",
        "original-operation.json",
    ]
    .into_iter()
    .map(|name| {
        let path = original.join(name);
        let raw = fs::read(&path).unwrap();
        raw_save(&evidence.join("before-controls"), name, &raw);
        (path, digest(&raw))
    })
    .collect();
    // Preserve the exact prior journal bytes before the authorized append.
    for entry in fs::read_dir(original.join("journal")).unwrap() {
        let entry = entry.unwrap();
        raw_save(
            &evidence.join("before-journal"),
            &entry.file_name().to_string_lossy(),
            &fs::read(entry.path()).unwrap(),
        );
    }
    let parents: upstream::Parents = load(&original, "parents.json");
    let bytes_before: BTreeMap<String, String> = result
        .artifacts
        .iter()
        .map(|(name, pin)| (name.clone(), digest(&native_bytes(&profile.source, pin))))
        .collect();
    let checkpoint = || {
        let index = Index::open(
            &profile.admission.index_root,
            &profile.binding.admission.index_checkpoint,
        )
        .unwrap();
        canonical_digest(&index.checkpoint()).unwrap()
    };
    let before = checkpoint();
    assert_eq!(qualified["checkpoint_sha256"], before);
    let stage = |output: &str| {
        Command::new(&coordinator)
            .arg("stage-retention-capacity")
            .arg(evidence.join("plan.json"))
            .arg(&plan_pin)
            .arg(original.join("packet.json"))
            .arg(original.join("source.json"))
            .arg(original.join("admission.json"))
            .arg(original.join("receipt.json"))
            .arg(evidence.join("result.json"))
            .arg(evidence.join("target.json"))
            .arg(evidence.join(output))
            .arg("--apply")
            .output()
            .unwrap()
    };
    phase(
        &evidence,
        "stage-original-retention-with-explicit-history-policy",
    );
    let out = stage("staged.json");
    success(&evidence, "stage-result.json", &out);
    let staged: retention::Acknowledgement = load(&evidence, "staged.json");
    assert_eq!(staged.candidate.task_key, result.task_key);
    assert_eq!(staged.candidate.attempt_id, result.attempt_id);
    assert_eq!(staged.target, profile.binding.target);
    let mut index = Index::open(&profile.admission.index_root, &staged.index_checkpoint).unwrap();
    let nodes = MetadataStore::open(
        &profile.admission.metadata_root,
        &staged.metadata_checkpoint,
    )
    .unwrap();
    let record = work::read(&mut index, &staged.work, &nodes).unwrap();
    let operation_raw = tig_core::identity::canonical_json(&json!((
        "reel.workflow-retain-key.v1",
        &profile.admission.index_identity,
        &result.task_key,
        &result.attempt_id
    )))
    .unwrap();
    let expected_operation = format!("workflow-retain-{}", digest(&operation_raw));
    assert_eq!(record.update.operation.as_str(), expected_operation);
    assert!(!work::complete(&mut index, &staged.work).unwrap());
    assert_eq!(record.evidence.as_ref(), Some(&staged.evidence));
    drop(nodes);
    drop(index);
    let after_stage = checkpoint();
    let pending = reel_workflow::handoff::Journal::open(&original.join("journal")).unwrap();
    assert_eq!(
        pending.state(),
        reel_workflow::handoff::State::ImportPending
    );
    assert!(pending.acknowledgement().is_none());
    // A new coordinator process reconstructs the original staged WorkRef. It
    // does not plan against the now-changed Index or allocate another operation.
    phase(
        &evidence,
        "reconstruct-original-staged-work-after-process-exit",
    );
    let out = stage("reconciled.json");
    success(&evidence, "stage-replay-result.json", &out);
    let reconciled: retention::Acknowledgement = load(&evidence, "reconciled.json");
    assert_eq!(staged.work, reconciled.work);
    assert_eq!(staged.candidate, reconciled.candidate);
    assert_eq!(staged.evidence, reconciled.evidence);
    assert_eq!(after_stage, checkpoint());
    phase(&evidence, "resume-original-journal-with-frozen-264-runtime");
    let out = Command::new(&profile.runtime.path)
        .arg("resume-handoff")
        .arg(original.join("journal"))
        .arg(original.join("profile.json"))
        .arg("-")
        .arg("--apply")
        .output()
        .unwrap();
    success(&evidence, "original-resume-result.json", &out);
    let completed = reel_workflow::handoff::Journal::open(&original.join("journal")).unwrap();
    assert_eq!(completed.state(), reel_workflow::handoff::State::Retained);
    assert_eq!(completed.result(), Some(&result));
    let ack: retention::Acknowledgement = load(
        &profile.acknowledgements,
        &format!("retained-{}-{}.json", result.task_key, result.attempt_id),
    );
    assert_eq!(ack.work, staged.work);
    assert_eq!(ack.candidate, staged.candidate);
    assert_eq!(ack.evidence, staged.evidence);
    drop(
        retention::verify(
            &packet,
            &profile.source,
            &profile.admission,
            &profile.binding.admission,
            &result,
            &ack,
        )
        .unwrap(),
    );
    assert_eq!(
        native_bytes(&profile.source, &result.artifacts["assembly.json"]),
        fs::read(original.join("preflight-assembly.json")).unwrap()
    );
    for (name, hash) in bytes_before {
        assert_eq!(
            digest(&native_bytes(&profile.source, &result.artifacts[&name])),
            hash
        );
    }
    for parent in parents.parents.values() {
        verify_parent(parent);
    }
    check_waiting(&waiting);
    for (path, pin) in controls {
        assert_eq!(digest(&fs::read(path).unwrap()), pin);
    }
    for (name, raw) in [
        ("packet.json", serde_json::to_vec(&packet).unwrap()),
        ("profile.json", serde_json::to_vec(&profile).unwrap()),
        ("result.json", serde_json::to_vec(&result).unwrap()),
        ("retained.json", ack.canonical_bytes().unwrap()),
    ] {
        raw_save(&evidence.join("control"), name, &raw);
    }
    let after = checkpoint();
    phase(
        &evidence,
        "replay-completed-original-journal-without-native-effects",
    );
    let out = Command::new(&profile.runtime.path)
        .arg("resume-handoff")
        .arg(original.join("journal"))
        .arg(original.join("profile.json"))
        .arg("-")
        .arg("--apply")
        .output()
        .unwrap();
    success(&evidence, "completed-replay-result.json", &out);
    assert_eq!(after, checkpoint());
    save(
        &evidence,
        "qualification-result.json",
        &json!({
            "schema":"reel.original-retained-capacity-recovery-qualification.v1",
            "original":original,"task_key":result.task_key,"attempt_id":result.attempt_id,
            "activation":profile.binding.admission.activation,"source_work":profile.binding.admission.work,
            "retention_work":ack.work,"retention_operation":expected_operation,"plan_sha256":plan_pin,
            "policy":plan.policy,"runtime":profile.runtime,"before_checkpoint_sha256":before,
            "staged_checkpoint_sha256":after_stage,"after_checkpoint_sha256":after,
            "original_work_reconstructed_after_process_exit":true,"stage_replay_index_unchanged":true,
            "original_runtime_acknowledged":true,"original_result_preserved":true,
            "outputs_and_parent_history_reopened":true,"control_matches_exact_preflight":true,
            "waiting_claims_reopened":true,"completed_replay_index_unchanged":true,
            "task_completed":true,"runtime_adopted":false,"worker_rerendered":false,
            "whole_corrected_issue_retained":false,"finding_closed":false,"selected":false,
            "approved":false,"scratch_retired":false,"history_released":false,
        }),
    );
    phase(&evidence, "retained-control-awaiting-corrected-whole-issue");
}
