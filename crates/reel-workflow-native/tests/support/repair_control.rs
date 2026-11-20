//! Real repaired part and historical sibling -> new protected assembly control.
//! Private synthetic fixtures only. Rebind declared producer slots; retain
//! original operations/runtimes and pending descendants without prediction.
use super::repair_part_native::{
    binding, load, native_bytes, phase, raw_save, save, success, verify_parent,
};
use super::*;
use reel_workflow::publication::InputReader;
use reel_workflow_native::{NativeProfile, admission, graph, handoff, retention, upstream};
use std::collections::BTreeSet;

struct Captured(BTreeMap<String, Vec<u8>>);
impl InputReader for Captured {
    fn read(&mut self, a: &Artifact, max: u64) -> Result<Vec<u8>> {
        let raw = self.0.get(&a.logical_id).expect("declared captured input");
        assert!(raw.len() as u64 <= max);
        assert_eq!(digest(raw), a.sha256);
        assert_eq!(raw.len() as u64, a.bytes);
        Ok(raw.clone())
    }
}
fn repin(before: &Artifact, raw: &[u8]) -> Artifact {
    Artifact {
        sha256: digest(raw),
        bytes: raw.len() as u64,
        ..before.clone()
    }
}
fn witness(p: &upstream::ParentControl) -> repair::ResultWitness {
    repair::ResultWitness {
        packet: p.packet.clone(),
        result: p.result.clone(),
    }
}
fn parent(root: &Path) -> upstream::ParentControl {
    upstream::ParentControl {
        packet: load(root, "packet.json"),
        profile: load(root, "profile.json"),
        result: load(root, "result.json"),
        acknowledgement: load(root, "retained.json"),
    }
}
pub(super) fn check_waiting(root: &Path) {
    let packet: Packet = load(root, "next-packet.json");
    let p: handoff::Profile = load(root, "next-handoff-profile.json");
    let journal = reel_workflow::handoff::Journal::open(&root.join("next-journal")).unwrap();
    assert_eq!(journal.state(), reel_workflow::handoff::State::Waiting);
    assert_eq!(journal.ticket().packet, packet);
    drop(
        admission::Prepared::open(
            &packet,
            &p.source,
            &p.admission,
            &p.binding.admission.attempt_id,
        )
        .unwrap()
        .reopen(&p.binding.admission)
        .unwrap(),
    );
}

#[test]
#[ignore = "Read-only audit of the preserved pre-admission schema refusal; exact original part activation/WorkRef and actual Index checkpoint, no close or new admission"]
fn preflight_refusal_preserves_the_original_repaired_part_before_control_replanning() {
    let env = |key| PathBuf::from(std::env::var_os(key).expect(key));
    let failed = env("REEL_REPAIR_CONTROL_REFUSED");
    let part_root = env("REEL_REPAIR_CONTROL_PART");
    let process: serde_json::Value = load(&failed, "process-result.json");
    assert_eq!(process["exit_code"], 101);
    assert!(
        fs::read_to_string(failed.join("run.log"))
            .unwrap()
            .contains("code: \"native-task-commit\"")
    );
    let entries: BTreeSet<_> = fs::read_dir(failed.join("job"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        entries,
        BTreeSet::from([
            "pending-native".to_owned(),
            "original-pins.json".into(),
            "source-clone.json".into()
        ])
    );
    let pins: BTreeMap<PathBuf, String> = load(&failed.join("job"), "original-pins.json");
    for (path, pin) in &pins {
        assert_eq!(&digest(&fs::read(path).unwrap()), pin);
    }
    let part = parent(&part_root);
    assert_eq!(
        reel_workflow::handoff::Journal::open(&part_root.join("journal"))
            .unwrap()
            .state(),
        reel_workflow::handoff::State::Retained
    );
    drop(
        admission::Prepared::open(
            &part.packet,
            &part.profile.source,
            &part.profile.admission,
            &part.profile.binding.admission.attempt_id,
        )
        .unwrap()
        .reopen(&part.profile.binding.admission)
        .unwrap(),
    );
    let index = tig_store::source_claims::protection::Index::open(
        &part.profile.admission.index_root,
        &part.profile.binding.admission.index_checkpoint,
    )
    .unwrap();
    let checkpoint = canonical_digest(&index.checkpoint()).unwrap();
    let previous: serde_json::Value = load(&part_root, "qualification-result.json");
    assert_eq!(previous["checkpoint_sha256"], checkpoint);
    save(
        &failed,
        "original-part-state-after-refusal.json",
        &json!({"schema":"reel.repair-control-refusal-audit.v1","task_key":part.packet.task_key,"activation":part.profile.binding.admission.activation,"work":part.profile.binding.admission.work,"checkpoint_sha256":checkpoint,"original_activation_reopened":true,"original_controls_unchanged":true,"close_started":false,"new_admission":false,"runtime_adopted":false}),
    );
}

#[test]
#[ignore = "Read-only original ImportPending control after retention-resource-bound; compute actual owner aggregation and exact native work status, no retry/publication/admission or runtime adoption"]
fn pending_control_reports_exact_retained_capacity_without_changing_original_state() {
    let env = |key| PathBuf::from(std::env::var_os(key).expect(key));
    let original = env("REEL_REPAIR_CONTROL_PENDING");
    let evidence = env("REEL_REPAIR_CONTROL_CAPACITY_EVIDENCE");
    let executable = env("REEL_NATIVE_HANDOFF_TOOL");
    let packet: Packet = load(&original, "packet.json");
    let profile: handoff::Profile = load(&original, "profile.json");
    let receipt: admission::Receipt = load(&original, "receipt.json");
    let journal = reel_workflow::handoff::Journal::open(&original.join("journal")).unwrap();
    assert_eq!(
        journal.state(),
        reel_workflow::handoff::State::ImportPending
    );
    assert_eq!(journal.ticket().packet, packet);
    assert_eq!(
        journal.ticket().native_binding,
        profile.binding.artifact().unwrap()
    );
    let result = journal.result().unwrap().clone();
    assert!(journal.acknowledgement().is_none());
    let checkpoint = || {
        let index = tig_store::source_claims::protection::Index::open(
            &profile.admission.index_root,
            &receipt.index_checkpoint,
        )
        .unwrap();
        canonical_digest(&index.checkpoint()).unwrap()
    };
    let before = checkpoint();
    let mut originals: BTreeMap<PathBuf, String> = [
        "packet.json",
        "profile.json",
        "source.json",
        "receipt.json",
        "original-operation.json",
    ]
    .into_iter()
    .map(|name| {
        let path = original.join(name);
        let hash = digest(&fs::read(&path).unwrap());
        (path, hash)
    })
    .collect();
    for entry in fs::read_dir(original.join("journal")).unwrap() {
        let path = entry.unwrap().path();
        originals.insert(path.clone(), digest(&fs::read(path).unwrap()));
    }
    save(&evidence, "original-pins.json", &originals);
    save(&evidence, "result.json", &result);
    save(&evidence, "target.json", &profile.binding.target);
    let preview = retention::capacity_preview(
        &packet,
        &profile.source,
        &profile.admission,
        &receipt,
        &result,
        &profile.binding.target,
    )
    .unwrap();
    assert!(!preview.resource_bound_ok);
    assert!(!preview.original_retention_work_exists);
    assert_eq!(preview.original_retention_work_complete, None);
    let out = Command::new(&executable)
        .arg("inspect-retention-capacity")
        .arg(original.join("packet.json"))
        .arg(original.join("source.json"))
        .arg(original.join("admission.json"))
        .arg(original.join("receipt.json"))
        .arg(evidence.join("result.json"))
        .arg(evidence.join("target.json"))
        .output()
        .unwrap();
    success(&evidence, "capacity-cli-result.json", &out);
    let inspected: serde_json::Value = decode(&out.stdout).unwrap();
    assert_eq!(
        inspected["capacity"],
        serde_json::to_value(&preview).unwrap()
    );
    assert_eq!(inspected["effects_applied"], false);
    assert_eq!(before, checkpoint());
    for (path, pin) in originals {
        assert_eq!(digest(&fs::read(path).unwrap()), pin);
    }
    save(
        &evidence,
        "qualification-result.json",
        &json!({"schema":"reel.retained-capacity-diagnostic-qualification.v1","original":original,"task_key":packet.task_key,"attempt_id":receipt.attempt_id,"activation":receipt.activation,"work":receipt.work,"capacity":preview,"checkpoint_sha256":before,"original_journal_and_controls_unchanged":true,"actual_index_unchanged":true,"original_retention_work_not_staged":true,"native_payloads_verified":false,"retained_acknowledged":false,"task_completed":false,"effects_applied":false,"runtime_adopted":false}),
    );
}

#[test]
#[ignore = "Read-only finite retained-owner capacity plan over original pending control; rejects edited/stale/foreign plans and preserves real Index, journal, runtime and Waiting claims"]
fn pending_control_exports_and_rechecks_pinned_capacity_plan_without_native_effects() {
    use retention::capacity::{self, Policy};
    let env = |key| PathBuf::from(std::env::var_os(key).expect(key));
    let original = env("REEL_REPAIR_CONTROL_PENDING");
    let evidence = env("REEL_REPAIR_CONTROL_PLAN_EVIDENCE");
    let executable = env("REEL_NATIVE_HANDOFF_TOOL");
    let waiting = env("REEL_REPAIR_CONTROL_WAITING");
    check_waiting(&waiting);
    let packet: Packet = load(&original, "packet.json");
    let profile: handoff::Profile = load(&original, "profile.json");
    let receipt: admission::Receipt = load(&original, "receipt.json");
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
    let checkpoint = || {
        let index = tig_store::source_claims::protection::Index::open(
            &profile.admission.index_root,
            &receipt.index_checkpoint,
        )
        .unwrap();
        canonical_digest(&index.checkpoint()).unwrap()
    };
    let before = checkpoint();
    let mut pins: BTreeMap<PathBuf, String> = [
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
        let pin = digest(&fs::read(&path).unwrap());
        (path, pin)
    })
    .collect();
    for entry in fs::read_dir(original.join("journal")).unwrap() {
        let path = entry.unwrap().path();
        pins.insert(path.clone(), digest(&fs::read(path).unwrap()));
    }
    save(&evidence, "original-pins.json", &pins);
    save(&evidence, "result.json", &result);
    save(&evidence, "target.json", &profile.binding.target);
    let policy = Policy {
        schema: "reel.workflow-retained-owner-policy.v1".into(),
        objects: 1024,
        logical_bytes: 1024 * 1024 * 1024,
    };
    save(&evidence, "policy.json", &policy);
    let plan = capacity::prepare(
        &packet,
        &profile.source,
        &profile.admission,
        &receipt,
        &result,
        &profile.binding.target,
        policy,
    )
    .unwrap();
    assert_eq!(
        (
            plan.observed.combined_objects,
            plan.required_objects,
            plan.required_changed_paths
        ),
        (525, 526, 53)
    );
    assert_eq!(plan.observed.object_limit, 512);
    let pin = digest(&plan.canonical_bytes().unwrap());
    let out = Command::new(&executable)
        .arg("plan-retention-capacity")
        .arg(original.join("packet.json"))
        .arg(original.join("source.json"))
        .arg(original.join("admission.json"))
        .arg(original.join("receipt.json"))
        .arg(evidence.join("result.json"))
        .arg(evidence.join("target.json"))
        .arg(evidence.join("policy.json"))
        .arg(evidence.join("plan.json"))
        .arg("--apply")
        .output()
        .unwrap();
    success(&evidence, "plan-cli-result.json", &out);
    assert_eq!(
        fs::read(evidence.join("plan.json")).unwrap(),
        plan.canonical_bytes().unwrap()
    );
    let exported: serde_json::Value = decode(&out.stdout).unwrap();
    assert_eq!(exported["plan_sha256"], pin);
    assert_eq!(exported["native_effects_applied"], false);
    let check = |name: &str, pin: &str| {
        Command::new(&executable)
            .arg("check-retention-capacity-plan")
            .arg(evidence.join(name))
            .arg(pin)
            .arg(original.join("packet.json"))
            .arg(original.join("source.json"))
            .arg(original.join("admission.json"))
            .arg(original.join("receipt.json"))
            .arg(evidence.join("result.json"))
            .arg(evidence.join("target.json"))
            .output()
            .unwrap()
    };
    let out = check("plan.json", &pin);
    success(&evidence, "recheck-cli-result.json", &out);
    let mut refusals = Vec::new();
    let out = check("plan.json", &"0".repeat(64));
    assert!(!out.status.success());
    let refusal: serde_json::Value = decode(&out.stdout).unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("retained-capacity-plan-pin"));
    refusals.push(refusal);
    for which in ["state", "request", "reservation"] {
        let mut edited = plan.clone();
        match which {
            "state" => edited.state = tig_core::Object::from_bytes(b"stale target revision"),
            "request" => {
                edited.request = tig_core::Object::from_bytes(b"foreign activation or runtime")
            }
            _ => edited.required_objects -= 1,
        }
        let name = format!("{which}-plan.json");
        let raw = edited.canonical_bytes().unwrap();
        raw_save(&evidence, &name, &raw);
        let out = check(&name, &digest(&raw));
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("retained-capacity-plan-stale"));
        refusals.push(decode::<serde_json::Value>(&out.stdout).unwrap());
    }
    let out = Command::new(&executable)
        .arg("plan-retention-capacity")
        .arg(original.join("packet.json"))
        .arg(original.join("source.json"))
        .arg(original.join("admission.json"))
        .arg(original.join("receipt.json"))
        .arg(evidence.join("result.json"))
        .arg(evidence.join("target.json"))
        .arg(evidence.join("policy.json"))
        .arg(evidence.join("plan.json"))
        .arg("--apply")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(
        fs::read(evidence.join("plan.json")).unwrap(),
        plan.canonical_bytes().unwrap()
    );
    refusals.push(decode::<serde_json::Value>(&out.stdout).unwrap());
    save(&evidence, "refusals.json", &refusals);
    capacity::recheck(
        &plan,
        &pin,
        &packet,
        &profile.source,
        &profile.admission,
        &receipt,
        &result,
        &profile.binding.target,
    )
    .unwrap();
    check_waiting(&waiting);
    assert_eq!(before, checkpoint());
    for (path, pin) in pins {
        assert_eq!(digest(&fs::read(path).unwrap()), pin);
    }
    let observed = retention::capacity_preview(
        &packet,
        &profile.source,
        &profile.admission,
        &receipt,
        &result,
        &profile.binding.target,
    )
    .unwrap();
    assert!(!observed.original_retention_work_exists);
    save(
        &evidence,
        "qualification-result.json",
        &json!({
            "schema":"reel.retained-capacity-plan-qualification.v1", "original":original,
            "plan_sha256":pin,"checkpoint_sha256":before,"refusals":refusals.len(),
            "original_controls_and_journal_unchanged":true,"actual_index_unchanged":true,
            "waiting_claims_reopened":true,"original_retention_work_not_staged":true,
            "native_effects_applied":false,"runtime_adopted":false,"retained_acknowledged":false,
            "task_completed":false,"native_payloads_verified":false,
        }),
    );
}

#[test]
#[ignore = "Existing retained repaired part and old sibling only; isolated exact source C, witnessed edge finalization, guarded part close and same-slot assembly-control retention; preserves every root and Waiting job"]
fn retained_repaired_part_and_historical_sibling_drive_fresh_native_assembly_control() {
    let env = |key| PathBuf::from(std::env::var_os(key).expect(key));
    let repaired = env("REEL_REPAIR_CONTROL_PART");
    let original = env("REEL_REPAIR_CONTROL_ORIGINAL");
    let controls = env("REEL_REPAIR_CONTROL_ANCESTORS");
    let waiting = env("REEL_REPAIR_CONTROL_WAITING");
    let evidence = env("REEL_REPAIR_CONTROL_EVIDENCE");
    let root = evidence.join("job");
    fs::create_dir(&root).unwrap();
    let repaired_parent = parent(&repaired);
    let old: upstream::Parents = load(&controls, "parents.json");
    let old_control = &old.parents["issue-control"];
    let old_opening = &old.parents["opening"];
    let old_closing = &old.parents["closing"];
    assert_eq!(repaired_parent.packet.task.id, "opening");
    assert_eq!(
        repaired_parent.profile.admission.scope.actor.as_str(),
        "slot2"
    );
    assert_eq!(
        repaired_parent.profile.admission.scope.operation.as_str(),
        "rooted-opening-repair-original"
    );
    check_waiting(&waiting);
    for p in [&repaired_parent, old_control, old_closing] {
        verify_parent(p);
    }
    let marker: serde_json::Value = load(&repaired, "original-operation.json");
    let report_root = PathBuf::from(marker["report"].as_str().unwrap());
    let report = parent(&report_root);
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
    let baseline: Plan = load(&repaired, "baseline.json");
    let repair: review::Repair = load(&repaired, "repair.json");
    let original_revision: repair::Revision = load(&repaired, "revision.json");
    let impact = review::repair_impact(&baseline, &checked, &repair).unwrap();
    assert_eq!(impact.unaffected, ["closing"]);
    let preserved: BTreeMap<_, _> = [
        repaired.join("packet.json"),
        repaired.join("source.json"),
        repaired.join("profile.json"),
        repaired.join("receipt.json"),
        repaired.join("result.json"),
        repaired.join("retained.json"),
        controls.join("parents.json"),
        original.join("closing-profile.json"),
        waiting.join("next-handoff-profile.json"),
    ]
    .into_iter()
    .map(|path| {
        let hash = digest(&fs::read(&path).unwrap());
        (path, hash)
    })
    .collect();
    save(&root, "original-pins.json", &preserved);
    // Immediate read-only planning, not generic input admission. The complete
    // exact original snapshot establishes record/annex-leaf binding first.
    let snapshot =
        SnapshotInputs::open(&old_control.packet, &old_control.profile.source.snapshot).unwrap();
    let mut fixed = BTreeMap::new();
    for id in ["comic/composition-intent", "comic/assembly"] {
        let pin = old_control
            .packet
            .task
            .inputs
            .iter()
            .find(|a| a.logical_id == id)
            .unwrap();
        fixed.insert(
            id.to_owned(),
            (pin.clone(), native_bytes(&old_control.profile.source, pin)),
        );
    }
    drop(snapshot);
    for id in ["comic/master", "comic/plan"] {
        let pin = repaired_parent
            .packet
            .task
            .inputs
            .iter()
            .find(|a| a.logical_id == id)
            .unwrap();
        // The actual repaired source is fixed-only; its guarded read proves the
        // exact B commit and leaf before authoring new records in source C.
        let mut inputs = reel_workflow_native::NativeInputs::open(
            &repaired_parent.packet,
            &repaired_parent.profile.source,
        )
        .unwrap();
        fixed.insert(
            id.to_owned(),
            (
                pin.clone(),
                inputs.read(pin, MAX_CONTROL_BYTES as u64).unwrap(),
            ),
        );
    }
    let mut temp = tempfile::tempdir().unwrap();
    temp.disable_cleanup(true);
    let history = temp.path().join("source-history");
    let out = Command::new(&repaired_parent.profile.source.snapshot.git.path)
        .arg("clone")
        .arg("--no-hardlinks")
        .arg(&repaired_parent.profile.source.snapshot.repository)
        .arg(&history)
        .output()
        .unwrap();
    success(&root, "source-clone.json", &out);
    git(&history, &["config", "user.name", "Fixture"]);
    git(
        &history,
        &["config", "user.email", "fixture@example.invalid"],
    );
    let mut source: NativeProfile =
        decode(&serde_json::to_vec(&repaired_parent.profile.source).unwrap()).unwrap();
    source.snapshot.repository = history.clone();
    let template: serde_json::Value = load(&history, &source.snapshot.records[0]);
    for (i, id) in ["comic/composition-intent", "comic/assembly"]
        .into_iter()
        .enumerate()
    {
        assert!(!source.snapshot.bindings.contains_key(id));
        let raw = &fixed[id].1;
        let object = Object::from_bytes(raw);
        let record = format!("records/repair-control-{i}.tig");
        let payload = format!("payloads/repair-control-{i}.bin");
        let mut value = template.clone();
        value["id"] = json!(id);
        value["selected"] = serde_json::to_value(&object).unwrap();
        value["streams"] = json!([]);
        write(&history, &record, value);
        fs::write(
            history.join(&payload),
            Key::from_object(&object).unlocked_pointer(),
        )
        .unwrap();
        source.snapshot.records.push(record);
        source.snapshot.bindings.insert(
            id.into(),
            Binding {
                selected: LogicalPath::parse(&payload).unwrap(),
                streams: BTreeMap::new(),
            },
        );
        source.snapshot.input_streams.insert(id.into(), None);
    }
    git(&history, &["add", "."]);
    git(
        &history,
        &[
            "commit",
            "-m",
            "synthetic repaired issue control exact fixed inputs",
        ],
    );
    source.snapshot.commit = git(&history, &["rev-parse", "HEAD"]);
    assert_ne!(source.snapshot.commit, repaired_parent.packet.source_commit);
    source.snapshot = source
        .snapshot
        .fixed_subset(&fixed.keys().cloned().collect::<BTreeSet<_>>())
        .unwrap();
    let parents = upstream::Parents {
        schema: "reel.workflow-native-parents.v1".into(),
        parents: BTreeMap::from([
            ("opening".into(), repaired_parent),
            (
                "closing".into(),
                decode(&serde_json::to_vec(old_closing).unwrap()).unwrap(),
            ),
        ]),
    };
    let before = old_control
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
    for reference in before.inputs.values() {
        let parent = &parents.parents[&reference.task_id];
        let pin = match &reference.artifact {
            reel_workflow::upstream::Selector::Output { name } => &parent.result.artifacts[name],
            reel_workflow::upstream::Selector::WorkerReceipt => {
                parent.result.worker_receipt.as_ref().unwrap()
            }
        };
        assert!(
            after
                .inputs
                .insert(pin.logical_id.clone(), reference.clone())
                .is_none()
        );
    }
    source.snapshot.upstream = Some(upstream::Frozen::capture(
        &after,
        &parents.borrowed().unwrap(),
    ));
    assert!(
        source.snapshot.budget.is_none(),
        "this qualifier is the bounded v1 source lane"
    );
    source.snapshot.schema = "reel.workflow-native-composite-snapshot.v1".into();
    let native_raw = source.snapshot.declaration_bytes().unwrap();
    let mut revision = original_revision;
    revision.schema = "reel.workflow-repair-revision.v4".into();
    revision.source_commit = source.snapshot.commit.clone();
    revision.task_snapshots = impact
        .rebuild
        .iter()
        .map(|id| {
            (
                id.clone(),
                if id == "opening" {
                    parents.parents[id].packet.source_commit.clone()
                } else {
                    source.snapshot.commit.clone()
                },
            )
        })
        .collect();
    for (id, edit) in &mut revision.native_inputs {
        if id == "issue-control" {
            edit.after = repin(&edit.before, &native_raw);
        } else if id != "opening" {
            let raw=serde_json::to_vec(&json!({"schema":"reel.synthetic-unqualified-repair-declaration.v1","task":id,"source_commit":source.snapshot.commit,"admitted":false})).unwrap();
            edit.after = repin(&edit.before, &raw);
            raw_save(&root, &format!("pending-native/{id}.json"), &raw);
        }
    }
    revision.derivation = Some(repair::Derivation {
        parents: BTreeMap::from([
            (
                "opening".into(),
                repair::ParentChange {
                    before: witness(old_opening),
                    after: witness(&parents.parents["opening"]),
                },
            ),
            (
                "closing".into(),
                repair::ParentChange {
                    before: witness(old_closing),
                    after: witness(&parents.parents["closing"]),
                },
            ),
        ]),
        consumers: BTreeMap::from([(
            "issue-control".into(),
            repair::ConsumerBindings {
                before,
                after: after.clone(),
            },
        )]),
    });
    let candidate = repair::materialize(&baseline, &checked, &repair, &revision).unwrap();
    let packet = make_packet(
        &candidate.plan,
        "issue-control",
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
    let part = &parents.parents["opening"];
    assert_eq!(
        make_packet(&candidate.plan, "opening", BTreeMap::new()).unwrap(),
        part.packet
    );
    assert_eq!(
        make_packet(&candidate.plan, "closing", BTreeMap::new()).unwrap(),
        old_closing.packet
    );
    assert_eq!(
        packet.task.tool.as_ref().unwrap().executable.sha256,
        part.profile.runtime.sha256
    );
    let runtime = part.profile.runtime.clone();
    save(&root, "preflight/packet.json", &packet);
    save(&root, "preflight/source.json", &source);
    save(&root, "preflight/revision.json", &revision);
    drop(SnapshotInputs::open(&packet, &source.snapshot).unwrap());
    let mut captured = Captured(
        fixed
            .iter()
            .map(|(id, (_, raw))| (id.clone(), raw.clone()))
            .collect(),
    );
    for (id, reference) in &after.inputs {
        let p = &parents.parents[&reference.task_id];
        let pin = packet
            .task
            .inputs
            .iter()
            .find(|a| &a.logical_id == id)
            .unwrap();
        captured
            .0
            .insert(id.clone(), native_bytes(&p.profile.source, pin));
    }
    let preflight = issue_control::generate(&packet, &mut captured).unwrap();
    assert_eq!(preflight.outputs.len(), 1);
    raw_save(
        &root,
        "preflight-assembly.json",
        &preflight.files["assembly.json"],
    );
    let mut admission = part.profile.admission.clone();
    admission.scope.operation = tig_store::Key::parse("rooted-issue-control-original").unwrap();
    let attempt = "rooted-control-attempt-1";
    for (name, value) in [
        ("baseline.json", serde_json::to_vec(&baseline).unwrap()),
        ("repair.json", serde_json::to_vec(&repair).unwrap()),
        ("revision.json", serde_json::to_vec(&revision).unwrap()),
        ("candidate.json", serde_json::to_vec(&candidate).unwrap()),
        ("plan.json", serde_json::to_vec(&candidate.plan).unwrap()),
        ("packet.json", serde_json::to_vec(&packet).unwrap()),
        ("source.json", serde_json::to_vec(&source).unwrap()),
        ("admission.json", serde_json::to_vec(&admission).unwrap()),
        ("parents.json", serde_json::to_vec(&parents).unwrap()),
        ("upstream.json", after.canonical_bytes().unwrap()),
    ] {
        raw_save(&root, name, &value);
    }
    save(
        &root,
        "original-operation.json",
        &json!({"schema":"reel.native-repair-control-original.v1","task_key":packet.task_key,"attempt_id":attempt,"scope":admission.scope,"source_repository":history,"repaired_part":repaired,"runtime":runtime,"before_after_witnesses_are_not_custody":true,"actual_parent_admission_required":true}),
    );
    let close = root.join("part-close");
    fs::create_dir(&close).unwrap();
    save(&close, "target.json", &part.profile.binding.target);
    let ticket = close.join("ticket.json");
    phase(&root, "prepare-original-part-close");
    let out = Command::new(&runtime.path)
        .arg("prepare-close")
        .arg(repaired.join("journal"))
        .arg(repaired.join("profile.json"))
        .arg(close.join("target.json"))
        .arg(&ticket)
        .arg("--apply")
        .output()
        .unwrap();
    success(&close, "prepare-result.json", &out);
    let pin = digest(&fs::read(&ticket).unwrap());
    save(&close, "ticket-pin.json", &pin);
    phase(&root, "close-original-part-into-retained-coverage");
    let out = Command::new(&runtime.path)
        .arg("close-job")
        .arg(&ticket)
        .arg(&pin)
        .arg(close.join("closed.json"))
        .arg("--apply")
        .output()
        .unwrap();
    success(&close, "apply-result.json", &out);
    let closed: reel_workflow_native::closing::Outcome = load(&close, "closed.json");
    assert!(closed.slot_idle && closed.original_clear_complete);
    let prepared = upstream::Admission::prepare(
        &packet,
        &source,
        &admission,
        attempt,
        parents.borrowed().unwrap(),
    )
    .unwrap();
    let preview = prepared.preview().unwrap();
    assert!(preview.declared_objects + 4 <= preview.changed_path_limit);
    save(&root, "admission-preview.json", &preview);
    phase(
        &root,
        "accept-original-control-with-live-retained-parent-proof",
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
            admission: receipt.clone(),
            target: part.profile.binding.target.clone(),
        },
        runtime: runtime.clone(),
        acknowledgements,
    };
    save(&root, "profile.json", &profile);
    let journal = root.join("journal");
    phase(&root, "create-owned-repaired-control-handoff");
    let out = Command::new(&runtime.path)
        .arg("create-handoff")
        .arg(root.join("packet.json"))
        .arg(root.join("profile.json"))
        .arg(&journal)
        .arg("--apply")
        .output()
        .unwrap();
    success(&root, "create-result.json", &out);
    let bindings = graph::Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::from([
            (
                "opening".into(),
                binding(repaired.join("journal"), repaired.join("profile.json")),
            ),
            (
                "closing".into(),
                binding(
                    original.join("closing-journal"),
                    original.join("closing-profile.json"),
                ),
            ),
            (
                "issue-control".into(),
                binding(journal.clone(), root.join("profile.json")),
            ),
        ]),
    };
    save(&root, "bindings.json", &bindings);
    phase(&root, "dispatch-fresh-native-assembly-control");
    let mut generated = Vec::new();
    automatic_dispatch::run_graph_root(
        &root,
        &packet,
        &profile,
        &runtime.path,
        &root.join("result.json"),
        &mut generated,
        &BTreeMap::new(),
        &candidate.plan,
        &bindings,
    );
    for (name, raw) in generated {
        raw_save(&root.join("generated"), name, &raw);
    }
    let result: ReturnedResult = load(&root, "result.json");
    let ack: retention::Acknowledgement = load(
        &profile.acknowledgements,
        &format!("retained-{}-{attempt}.json", packet.task_key),
    );
    save(&root, "retained.json", &ack);
    phase(&root, "verify-native-control-and-parent-history");
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
    assert_eq!(result.artifacts, preflight.outputs);
    assert_eq!(
        native_bytes(&profile.source, &result.artifacts["assembly.json"]),
        preflight.files["assembly.json"]
    );
    assert_ne!(
        result.artifacts["assembly.json"].logical_id,
        old_control.result.artifacts["assembly.json"].logical_id
    );
    for p in parents.parents.values() {
        verify_parent(p);
    }
    check_waiting(&waiting);
    for (path, pin) in &preserved {
        assert_eq!(&digest(&fs::read(path).unwrap()), pin);
    }
    let checkpoint = || {
        let index = tig_store::source_claims::protection::Index::open(
            &profile.admission.index_root,
            &receipt.index_checkpoint,
        )
        .unwrap();
        canonical_digest(&index.checkpoint()).unwrap()
    };
    let completed = checkpoint();
    let out = Command::new(&runtime.path)
        .arg("dispatch-task")
        .arg(root.join("automatic-plan.json"))
        .arg(root.join("automatic-bindings.json"))
        .arg("issue-control")
        .arg(root.join("automatic-config.json"))
        .arg(root.join("automatic-dispatch"))
        .arg("--apply")
        .output()
        .unwrap();
    success(&root, "completed-replay.json", &out);
    assert_eq!(completed, checkpoint());
    let outcome = json!({"schema":"reel.native-repaired-issue-control-qualification.v1","job":root,"source_repository":history,"source_commit":packet.source_commit,"task_key":packet.task_key,"attempt_id":attempt,"activation":receipt.activation,"work":receipt.work,"runtime":runtime,"declared_objects":preview.declared_objects,"checkpoint_sha256":completed,"repaired_part_packet_preserved":true,"closing_packet_preserved":true,"witnessed_edge_rebinding":true,"live_parent_custody_admitted":true,"original_part_closed_with_original_runtime":true,"same_slot_reused":true,"control_retained":true,"task_completed":true,"assembly_matches_exact_preflight":true,"parents_reopened_after_reuse":true,"other_waiting_generation_reopened":true,"replay_actual_index_unchanged":true,"whole_corrected_issue_retained":false,"descendant_declarations_pending":true,"finding_closed":false,"selected":false,"approved":false,"runtime_adopted":false,"scratch_retired":false});
    save(&root, "qualification-result.json", &outcome);
    save(&evidence, "qualification-result.json", &outcome);
    phase(
        &root,
        "retained-corrected-control-awaiting-full-issue-admission",
    );
}
