//! Exact corrected whole-issue inputs and original legacy-control slot handoff.
use super::repair_control_native::check_waiting;
use super::repair_part_native::{
    load, native_bytes, phase, raw_save, save, success, verify_parent,
};
use super::*;
use reel_workflow::publication::InputReader;
use reel_workflow_native::{NativeProfile, closing, retention, upstream};
use std::collections::BTreeSet;
use tig_store::source_claims::protection::Index;

struct Captured(BTreeMap<String, Vec<u8>>);
impl InputReader for Captured {
    fn read(&mut self, pin: &Artifact, max: u64) -> Result<Vec<u8>> {
        let raw = self.0.get(&pin.logical_id).expect("captured exact input");
        assert!(raw.len() as u64 <= max);
        assert_eq!(raw.len() as u64, pin.bytes);
        assert_eq!(digest(raw), pin.sha256);
        Ok(raw.clone())
    }
}
fn witness(parent: &upstream::ParentControl) -> repair::ResultWitness {
    repair::ResultWitness {
        packet: parent.packet.clone(),
        result: parent.result.clone(),
    }
}
fn copy<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) -> T {
    decode(&serde_json::to_vec(value).unwrap()).unwrap()
}
fn parent(root: &Path) -> upstream::ParentControl {
    upstream::ParentControl {
        packet: load(root, "packet.json"),
        profile: load(root, "profile.json"),
        result: load(root, "result.json"),
        acknowledgement: load(root, "retained.json"),
    }
}
fn repin(before: &Artifact, raw: &[u8]) -> Artifact {
    Artifact {
        sha256: digest(raw),
        bytes: raw.len() as u64,
        ..before.clone()
    }
}

#[test]
#[ignore = "Actual retained corrected part/control and historical closing only; finalize complete issue inputs and byte-exact preflight, then explicit recovered-policy original-control close with unchanged job/runtime and Waiting claims; no new issue admission or render"]
fn corrected_issue_preflight_preserves_parent_packets_and_returns_original_control_slot_with_retained_policy()
 {
    let env = |key| PathBuf::from(std::env::var_os(key).expect(key));
    let control_root = env("REEL_REPAIR_ISSUE_CONTROL");
    let control_proof = env("REEL_REPAIR_ISSUE_CONTROL_PROOF");
    let part_root = env("REEL_REPAIR_ISSUE_PART");
    let old_root = env("REEL_REPAIR_ISSUE_OLD_PARENTS");
    let waiting = env("REEL_REPAIR_ISSUE_WAITING");
    let policy_root = env("REEL_REPAIR_ISSUE_POLICY");
    let local = env("REEL_REPAIR_ISSUE_LOCAL");
    let evidence = env("REEL_REPAIR_ISSUE_EVIDENCE");
    let coordinator = env("REEL_NATIVE_HANDOFF_TOOL");
    let root = evidence.join("job");
    fs::create_dir(&root).unwrap();
    phase(&root, "reopen-actual-retained-producer-chain");
    check_waiting(&waiting);
    let control = parent(&control_proof);
    let opening = parent(&part_root);
    let old: upstream::Parents = load(&old_root, "parents.json");
    for p in [&control, &opening, &old.parents["closing"]] {
        verify_parent(p);
    }
    let policy: retention::capacity::Plan = load(&policy_root, "plan.json");
    let policy_pin = digest(&policy.canonical_bytes().unwrap());
    let certified: serde_json::Value = load(&policy_root, "qualification-result.json");
    assert_eq!(certified["plan_sha256"], policy_pin);
    assert!(control.profile.binding.retained_owner_policy.is_none());
    assert_eq!(
        control.profile.runtime.sha256,
        "264efd3655caa8f0190ab00548e6daff3a3b2a7a457261fd7d27c68d46e12b9a"
    );
    let before = Index::open(
        &control.profile.admission.index_root,
        &control.acknowledgement.index_checkpoint,
    )
    .unwrap()
    .checkpoint();
    let checkpoint_before = canonical_digest(&before).unwrap();
    assert_eq!(
        before, control.acknowledgement.index_checkpoint,
        "pre-admission refusal must preserve original Index"
    );
    save(
        &root,
        "original-state-audit.json",
        &json!({"schema":"reel.corrected-issue-original-state-audit.v1","task_key":control.packet.task_key,"activation":control.profile.binding.admission.activation,"work":control.profile.binding.admission.work,"checkpoint_sha256":checkpoint_before,"original_retained_checkpoint_unchanged":true,"original_runtime":control.profile.runtime,"control_still_retained":true}),
    );
    let pins: BTreeMap<PathBuf, String> = [
        control_root.join("packet.json"),
        control_root.join("profile.json"),
        control_root.join("receipt.json"),
        part_root.join("packet.json"),
        part_root.join("profile.json"),
        part_root.join("result.json"),
        part_root.join("retained.json"),
        old_root.join("parents.json"),
        waiting.join("next-handoff-profile.json"),
    ]
    .into_iter()
    .map(|path| {
        let sha = digest(&fs::read(&path).unwrap());
        (path, sha)
    })
    .collect();
    save(&root, "original-pins.json", &pins);
    let marker: serde_json::Value = load(&part_root, "original-operation.json");
    let report = parent(Path::new(marker["report"].as_str().unwrap()));
    let guard = retention::verify(
        &report.packet,
        &report.profile.source,
        &report.profile.admission,
        &report.profile.binding.admission,
        &report.result,
        &report.acknowledgement,
    )
    .unwrap();
    let checked = guard.review_report().unwrap().clone();
    drop(guard);
    let baseline: Plan = load(&control_root, "baseline.json");
    let repair: review::Repair = load(&control_root, "repair.json");
    let mut revision: repair::Revision = load(&control_root, "revision.json");
    let parents = upstream::Parents {
        schema: "reel.workflow-native-parents.v1".into(),
        parents: BTreeMap::from([
            ("opening".into(), opening),
            ("closing".into(), copy(&old.parents["closing"])),
            ("issue-control".into(), control),
        ]),
    };
    let control = &parents.parents["issue-control"];
    let original_issue = &old.parents["issue-assemble"];
    let before_bindings = original_issue
        .profile
        .source
        .snapshot
        .upstream
        .as_ref()
        .unwrap()
        .bindings
        .clone();
    let mut after = reel_workflow::upstream::Bindings {
        schema: "reel.workflow-upstream-inputs.v2".into(),
        streaming: None,
        inputs: BTreeMap::new(),
    };
    for reference in before_bindings.inputs.values() {
        let producer = &parents.parents[&reference.task_id];
        let pin = match &reference.artifact {
            reel_workflow::upstream::Selector::Output { name } => &producer.result.artifacts[name],
            reel_workflow::upstream::Selector::WorkerReceipt => {
                producer.result.worker_receipt.as_ref().unwrap()
            }
        };
        assert!(
            after
                .inputs
                .insert(pin.logical_id.clone(), reference.clone())
                .is_none()
        );
    }
    let fixed_ids = BTreeSet::from([
        "comic/master".to_owned(),
        "comic/plan".into(),
        "comic/composition-intent".into(),
    ]);
    drop(SnapshotInputs::open(&control.packet, &control.profile.source.snapshot).unwrap());
    let mut captured = Captured(BTreeMap::new());
    for id in &fixed_ids {
        let pin = control
            .packet
            .task
            .inputs
            .iter()
            .find(|a| &a.logical_id == id)
            .unwrap();
        captured
            .0
            .insert(id.clone(), native_bytes(&control.profile.source, pin));
    }
    let mut source: NativeProfile = copy(&control.profile.source);
    source.snapshot = source.snapshot.fixed_subset(&fixed_ids).unwrap();
    source.snapshot.upstream = Some(upstream::Frozen::capture(
        &after,
        &parents.borrowed().unwrap(),
    ));
    assert_eq!(
        source.snapshot.schema,
        "reel.workflow-native-composite-snapshot.v1"
    );
    let native_raw = source.snapshot.declaration_bytes().unwrap();
    let edit = revision.native_inputs.get_mut("issue-assemble").unwrap();
    edit.after = repin(&edit.before, &native_raw);
    revision
        .task_snapshots
        .insert("issue-assemble".into(), source.snapshot.commit.clone());
    let derivation = revision.derivation.as_mut().unwrap();
    derivation.parents.insert(
        "issue-control".into(),
        repair::ParentChange {
            before: witness(&old.parents["issue-control"]),
            after: witness(control),
        },
    );
    derivation.consumers.insert(
        "issue-assemble".into(),
        repair::ConsumerBindings {
            before: before_bindings,
            after: after.clone(),
        },
    );
    let candidate = repair::materialize(&baseline, &checked, &repair, &revision).unwrap();
    let packet = make_packet(
        &candidate.plan,
        "issue-assemble",
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
    for (id, p) in &parents.parents {
        let dependencies = if id == "issue-control" {
            p.packet.dependencies.clone()
        } else {
            BTreeMap::new()
        };
        assert_eq!(
            make_packet(&candidate.plan, id, dependencies).unwrap(),
            p.packet
        );
    }
    drop(SnapshotInputs::open(&packet, &source.snapshot).unwrap());
    for (id, reference) in &after.inputs {
        let producer = &parents.parents[&reference.task_id];
        let pin = packet
            .task
            .inputs
            .iter()
            .find(|a| &a.logical_id == id)
            .unwrap();
        captured
            .0
            .insert(id.clone(), native_bytes(&producer.profile.source, pin));
    }
    let bound = reel_workflow::issue::bind(&packet, &mut captured).unwrap();
    let semantic_parents = parents
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
    let actual = reel_workflow::upstream::bind(&packet, &after, &semantic_parents).unwrap();
    let expected = bound.composition_origins().unwrap();
    assert_eq!(
        actual
            .values()
            .map(|b| b.reference.task_id.clone())
            .collect::<BTreeSet<_>>(),
        expected.parents
    );
    for (id, reference) in &expected.inputs {
        assert_eq!(&actual[id].reference, reference);
    }
    let tool = packet.task.tool.as_ref().unwrap();
    let request = &captured.0[&tool.request.logical_id];
    let (receipt, files) = reel_publication::comic_issue::capture_with(request, |path, max| {
        let id = tool
            .source_bindings
            .get(path)
            .ok_or_else(|| anyhow::anyhow!("declared preflight source required"))?;
        let raw = &captured.0[id];
        anyhow::ensure!(raw.len() as u64 <= max, "bounded preflight source");
        Ok(raw.clone())
    })
    .unwrap();
    assert_eq!(
        files["reading.pdf"],
        fs::read(local.join("reading.pdf")).unwrap()
    );
    assert_eq!(
        files["transcript.json"],
        fs::read(local.join("transcript.json")).unwrap()
    );
    let mut admission = control.profile.admission.clone();
    admission.scope.operation = tig_store::Key::parse("rooted-issue-assembly-original").unwrap();
    let attempt = "rooted-issue-attempt-1";
    let preview =
        reel_workflow_native::admission::Prepared::open(&packet, &source, &admission, attempt)
            .unwrap()
            .declared_preview()
            .unwrap();
    assert!(preview.declared_objects + 4 <= preview.changed_path_limit);
    for (name, raw) in [
        ("packet.json", serde_json::to_vec(&packet).unwrap()),
        ("source.json", serde_json::to_vec(&source).unwrap()),
        ("admission.json", serde_json::to_vec(&admission).unwrap()),
        ("plan.json", serde_json::to_vec(&candidate.plan).unwrap()),
        ("candidate.json", serde_json::to_vec(&candidate).unwrap()),
        ("revision.json", serde_json::to_vec(&revision).unwrap()),
        ("parents.json", serde_json::to_vec(&parents).unwrap()),
        ("upstream.json", after.canonical_bytes().unwrap()),
    ] {
        raw_save(&root, name, &raw);
    }
    save(&root, "admission-preview.json", &preview);
    let inspected = Command::new(&coordinator)
        .arg("inspect-input-closure")
        .arg(root.join("packet.json"))
        .arg(root.join("source.json"))
        .arg(root.join("admission.json"))
        .arg(attempt)
        .output()
        .unwrap();
    success(&root, "declared-preview-cli.json", &inspected);
    let inspected: serde_json::Value = decode(&inspected.stdout).unwrap();
    assert_eq!(
        inspected["resources"],
        serde_json::to_value(&preview).unwrap()
    );
    assert_eq!(inspected["admission_checked"], false);
    assert_eq!(inspected["effects_applied"], false);
    save(&root, "preflight-receipt.json", &receipt);
    for (name, raw) in &files {
        raw_save(&root.join("preflight"), name, raw);
    }
    assert_eq!(
        canonical_digest(
            &Index::open(&admission.index_root, &before)
                .unwrap()
                .checkpoint()
        )
        .unwrap(),
        checkpoint_before
    );
    phase(
        &root,
        "preview-original-legacy-control-close-from-retained-policy",
    );
    let close = root.join("control-close");
    fs::create_dir(&close).unwrap();
    save(&close, "target.json", &control.profile.binding.target);
    raw_save(&close, "policy.json", &policy.canonical_bytes().unwrap());
    let prepare = |plan: &Path, pin: &str, output: &Path| {
        Command::new(&coordinator)
            .arg("prepare-close-with-policy")
            .arg(control_root.join("journal"))
            .arg(control_root.join("profile.json"))
            .arg(close.join("target.json"))
            .arg(plan)
            .arg(pin)
            .arg(output)
            .arg("--apply")
            .output()
            .unwrap()
    };
    let out = prepare(
        &close.join("policy.json"),
        &"0".repeat(64),
        &close.join("wrong-pin-ticket.json"),
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("retained-capacity-plan-pin"));
    let mut foreign = policy.clone();
    foreign.state = Object::from_bytes(b"foreign state with same requested allowance");
    let raw = foreign.canonical_bytes().unwrap();
    raw_save(&close, "foreign-policy.json", &raw);
    let out = prepare(
        &close.join("foreign-policy.json"),
        &digest(&raw),
        &close.join("foreign-ticket.json"),
    );
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("close-original-retained-policy-evidence")
    );
    assert!(
        !close.join("wrong-pin-ticket.json").exists()
            && !close.join("foreign-ticket.json").exists()
    );
    assert_eq!(
        canonical_digest(
            &Index::open(&admission.index_root, &before)
                .unwrap()
                .checkpoint()
        )
        .unwrap(),
        checkpoint_before
    );
    let out = prepare(
        &close.join("policy.json"),
        &policy_pin,
        &close.join("ticket.json"),
    );
    success(&close, "prepare-result.json", &out);
    let ticket_raw = fs::read(close.join("ticket.json")).unwrap();
    let ticket_pin = digest(&ticket_raw);
    let ticket: closing::Ticket = decode(&ticket_raw).unwrap();
    assert_eq!(ticket.schema, "reel.workflow-close-ticket.v2");
    assert_eq!(ticket.retained_owner_plan.as_ref().unwrap(), &policy);
    assert_eq!(
        ticket.profile.runtime.sha256,
        control.profile.runtime.sha256
    );
    phase(
        &root,
        "stage-and-recover-original-control-close-with-retained-policy",
    );
    let out = Command::new(&coordinator)
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
    let out = Command::new(&coordinator)
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
    let prepared = upstream::Admission::prepare(
        &packet,
        &source,
        &admission,
        attempt,
        parents.borrowed().unwrap(),
    )
    .unwrap();
    assert_eq!(prepared.preview().unwrap(), preview);
    drop(prepared);
    for p in parents.parents.values() {
        verify_parent(p);
    }
    check_waiting(&waiting);
    for (path, pin) in pins {
        assert_eq!(digest(&fs::read(path).unwrap()), pin);
    }
    let after_close = Index::open(&admission.index_root, &closed.index_checkpoint)
        .unwrap()
        .checkpoint();
    let out = Command::new(&control.profile.runtime.path)
        .arg("resume-handoff")
        .arg(control_root.join("journal"))
        .arg(control_root.join("profile.json"))
        .arg("-")
        .arg("--apply")
        .output()
        .unwrap();
    success(&close, "original-runtime-historical-replay.json", &out);
    assert_eq!(
        Index::open(&admission.index_root, &after_close)
            .unwrap()
            .checkpoint(),
        after_close
    );
    save(
        &root,
        "original-operation.json",
        &json!({"schema":"reel.corrected-issue-preflight.v1","task_key":packet.task_key,"attempt_id":attempt,"source_commit":packet.source_commit,"scope":admission.scope,"runtime":control.profile.runtime,"parents":control_proof,"legacy_control":control_root,"waiting":waiting,"control_close":close,"before_checkpoint_sha256":checkpoint_before,"after_close_checkpoint_sha256":canonical_digest(&after_close).unwrap(),"issue_admitted":false,"issue_worker_invoked":false,"source_slot_idle":true}),
    );
    save(
        &evidence,
        "qualification-result.json",
        &json!({"schema":"reel.corrected-issue-preflight-and-policy-close-qualification.v1","job":root,"task_key":packet.task_key,"attempt_id":attempt,"source_commit":packet.source_commit,"declared_objects":preview.declared_objects,"logical_bytes":preview.logical_bytes,"parent_packets_preserved":true,"opening_and_closing_byte_inputs_reused":true,"exact_preflight_matches_local_issue":true,"issue_reading_bytes":files["reading.pdf"].len(),"issue_transcript_bytes":files["transcript.json"].len(),"control_closed_from_actual_retained_policy":true,"same_close_work_recovered":true,"original_control_runtime_preserved":true,"historical_replay_index_unchanged":true,"waiting_claims_reopened":true,"after_close_checkpoint_sha256":canonical_digest(&after_close).unwrap(),"issue_admitted":false,"whole_corrected_issue_retained":false,"finding_closed":false,"selected":false,"approved":false,"scratch_retired":false}),
    );
    phase(&root, "complete-issue-inputs-ready-for-own-admission");
}
