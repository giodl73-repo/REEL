//! Execute one rooted repair against an actual new native source snapshot.
//! Reuse only the completed report's slot through its original runtime/close
//! transaction. Descendants remain pending, not admitted under guessed inputs.
use super::*;
use reel_workflow::publication::InputReader;
use reel_workflow_native::{
    NativeInputs, NativeProfile, admission, graph, handoff, retention, worker,
};
use std::io::{Read, Write};

struct Captured<'a>(&'a BTreeMap<String, Vec<u8>>);
impl InputReader for Captured<'_> {
    fn read(&mut self, pin: &Artifact, maximum: u64) -> Result<Vec<u8>> {
        let raw = self.0.get(&pin.logical_id).expect("named captured input");
        assert!(raw.len() as u64 <= maximum);
        Ok(raw.clone())
    }
}

fn load<T: serde::de::DeserializeOwned>(root: &Path, name: &str) -> T {
    decode(&fs::read(root.join(name)).unwrap()).unwrap()
}
fn raw_save(root: &Path, name: &str, raw: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .unwrap();
    file.write_all(raw).unwrap();
    file.sync_all().unwrap();
}
fn save(root: &Path, name: &str, value: &impl serde::Serialize) {
    raw_save(root, name, &serde_json::to_vec(value).unwrap());
}
fn phase(root: &Path, value: &str) {
    fs::write(
        root.join("phase.json"),
        serde_json::to_vec(&json!({"phase":value})).unwrap(),
    )
    .unwrap();
}
fn repin(before: &Artifact, raw: &[u8]) -> Artifact {
    Artifact {
        sha256: digest(raw),
        bytes: raw.len() as u64,
        ..before.clone()
    }
}
fn native_bytes(source: &NativeProfile, pin: &Artifact) -> Vec<u8> {
    assert!(pin.bytes <= 32 * 1024 * 1024);
    let mut stream = tig_git::annex::workflow::reader::open(
        &source.source,
        &Object {
            sha256: tig_core::Digest::parse(&pin.sha256).unwrap(),
            bytes: pin.bytes,
        },
    )
    .unwrap();
    let mut raw = Vec::new();
    Read::by_ref(&mut stream)
        .take(pin.bytes + 1)
        .read_to_end(&mut raw)
        .unwrap();
    assert_eq!(repin(pin, &raw), *pin);
    raw
}
fn success(root: &Path, name: &str, output: &std::process::Output) {
    save(
        root,
        name,
        &json!({"exit_code":output.status.code(),"stdout":String::from_utf8_lossy(&output.stdout),"stderr":String::from_utf8_lossy(&output.stderr)}),
    );
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
fn verify_parent(parent: &reel_workflow_native::upstream::ParentControl) {
    drop(
        retention::verify(
            &parent.packet,
            &parent.profile.source,
            &parent.profile.admission,
            &parent.profile.binding.admission,
            &parent.result,
            &parent.acknowledgement,
        )
        .unwrap(),
    );
}
fn binding(journal: PathBuf, profile: PathBuf) -> graph::Binding {
    graph::Binding {
        journal,
        profile: graph::PinnedFile {
            sha256: digest(&fs::read(&profile).unwrap()),
            path: profile,
        },
        staging_ticket: None,
    }
}

#[test]
#[ignore = "Existing qualified synthetic rooted candidate/report only; actual .tig snapshot, original report guarded close and same-slot corrected part retention; preserves every operation and Waiting job, no issue assembly/adoption/selection"]
fn rooted_candidate_executes_one_corrected_part_without_rebuilding_the_other_part() {
    let env = |key| PathBuf::from(std::env::var_os(key).expect(key));
    let qualified = env("REEL_NATIVE_PART_REPAIR_CANDIDATE");
    let report_root = env("REEL_NATIVE_PART_REPAIR_REPORT");
    let controls = env("REEL_NATIVE_PART_REPAIR_CONTROLS");
    let original = env("REEL_NATIVE_PART_REPAIR_ORIGINAL");
    let waiting = env("REEL_NATIVE_PART_REPAIR_WAITING");
    let evidence = env("REEL_NATIVE_PART_REPAIR_EVIDENCE");
    let runtime = env("REEL_NATIVE_HANDOFF_TOOL");
    let root = evidence.join("job");
    fs::create_dir(&root).unwrap();
    let qualification: serde_json::Value = load(&qualified, "qualification-result.json");
    assert_eq!(qualification["new_native_admission"], false);
    let candidate_root = qualified.join("candidate");
    let baseline: Plan = load(&candidate_root, "baseline.json");
    let mut repair: review::Repair = load(&candidate_root, "repair.json");
    assert_eq!(repair.schema, "reel.workflow-repair.v2");
    assert_eq!(repair.roots, Some(vec!["opening".into()]));
    let parents: reel_workflow_native::upstream::Parents = load(&controls, "parents.json");
    let opening = &parents.parents["opening"];
    let closing = &parents.parents["closing"];
    let report_packet: Packet = load(&report_root, "packet.json");
    let report_profile: handoff::Profile = load(&report_root, "profile.json");
    let report_result: ReturnedResult = load(&report_root, "result.json");
    let report_ack: retention::Acknowledgement = load(&report_root, "retained.json");
    assert_eq!(report_profile.admission.scope.actor.as_str(), "slot2");
    assert_eq!(
        report_profile.admission.scope.operation.as_str(),
        "whole-issue-report-original"
    );
    assert_eq!(baseline.project, "synthetic-comic");
    let check_waiting = || {
        let packet: Packet = load(&waiting, "next-packet.json");
        let profile: handoff::Profile = load(&waiting, "next-handoff-profile.json");
        let journal = reel_workflow::handoff::Journal::open(&waiting.join("next-journal")).unwrap();
        assert_eq!(journal.state(), reel_workflow::handoff::State::Waiting);
        assert_eq!(journal.ticket().packet, packet);
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
    };
    check_waiting();
    let original_paths = [
        report_root.join("packet.json"),
        report_root.join("profile.json"),
        report_root.join("result.json"),
        report_root.join("retained.json"),
        controls.join("parents.json"),
        original.join("closing-profile.json"),
        original.join("closing-packet.json"),
        original.join("closing-result.json"),
        waiting.join("next-packet.json"),
        waiting.join("next-handoff-profile.json"),
    ];
    let original_pins: BTreeMap<_, _> = original_paths
        .into_iter()
        .map(|path| {
            let sha = digest(&fs::read(&path).unwrap());
            (path, sha)
        })
        .collect();
    save(&root, "original-pins.json", &original_pins);
    let guard = retention::verify(
        &report_packet,
        &report_profile.source,
        &report_profile.admission,
        &report_profile.binding.admission,
        &report_result,
        &report_ack,
    )
    .unwrap();
    let checked = guard.review_report().unwrap().clone();
    drop(guard);
    assert_eq!(repair.report_sha256, checked.report_sha256());
    verify_parent(opening);
    verify_parent(closing);
    assert_eq!(
        load::<Packet>(&original, "closing-packet.json"),
        closing.packet
    );
    assert_eq!(
        reel_workflow::handoff::Journal::open(&original.join("closing-journal"))
            .unwrap()
            .ticket()
            .packet,
        closing.packet
    );
    let mut old_inputs = NativeInputs::open(&opening.packet, &opening.profile.source).unwrap();
    reel_workflow::publication::bind_comic(&opening.packet, &mut old_inputs).unwrap();
    let mut bytes = BTreeMap::new();
    for pin in &opening.packet.task.inputs {
        bytes.insert(
            pin.logical_id.clone(),
            old_inputs.read(pin, 16 * 1024 * 1024).unwrap(),
        );
    }
    drop(old_inputs);
    let candidate_repository = PathBuf::from(
        qualification["source_candidate_repository"]
            .as_str()
            .unwrap(),
    );
    let mut changed = BTreeMap::new();
    for edit in &repair.edits {
        assert!(opening.packet.task.inputs.contains(&edit.before));
        let raw = fs::read(
            candidate_repository.join(format!("candidate/{}.json", edit.after.logical_id)),
        )
        .unwrap();
        assert_eq!(repin(&edit.after, &raw), edit.after);
        bytes.insert(edit.after.logical_id.clone(), raw.clone());
        changed.insert(edit.after.logical_id.clone(), raw);
    }
    assert_eq!(changed.len(), 4);
    // A distinct new job explicitly chooses this runtime. No old operation is
    // retargeted, and the retained report closes only with its own runtime.
    let runtime_raw = fs::read(&runtime).unwrap();
    let original_tool = &opening.packet.task.tool.as_ref().unwrap().executable;
    let runtime_asset = repin(original_tool, &runtime_raw);
    assert_ne!(*original_tool, runtime_asset);
    repair.edits.push(review::Edit {
        before: original_tool.clone(),
        after: runtime_asset.clone(),
    });
    let mut fixture = Fixture::new();
    fixture.temp.disable_cleanup(true);
    assert_eq!(
        fixture.profile.git.sha256,
        opening.profile.source.snapshot.git.sha256
    );
    fixture.profile.git = opening.profile.source.snapshot.git.clone();
    let template: serde_json::Value = load(fixture.temp.path(), "records/input.tig");
    fixture.profile.records.clear();
    fixture.profile.bindings.clear();
    fixture.profile.input_streams.clear();
    for (i, (id, raw)) in bytes.iter().enumerate() {
        let object = Object::from_bytes(raw);
        let record = format!("records/rooted-{i}.tig");
        let payload = format!("payloads/rooted-{i}.bin");
        let mut value = template.clone();
        value["id"] = json!(id);
        value["selected"] = serde_json::to_value(&object).unwrap();
        value["streams"] = json!([]);
        write(fixture.temp.path(), &record, value);
        fs::write(
            fixture.temp.path().join(&payload),
            Key::from_object(&object).unlocked_pointer(),
        )
        .unwrap();
        fixture.profile.records.push(record);
        fixture.profile.bindings.insert(
            id.clone(),
            Binding {
                selected: LogicalPath::parse(&payload).unwrap(),
                streams: BTreeMap::new(),
            },
        );
        fixture.profile.input_streams.insert(id.clone(), None);
    }
    git(fixture.temp.path(), &["add", "."]);
    git(
        fixture.temp.path(),
        &[
            "commit",
            "-m",
            "synthetic rooted part repair exact native source",
        ],
    );
    fixture.profile.commit = git(fixture.temp.path(), &["rev-parse", "HEAD"]);
    let impact = review::repair_impact(&baseline, &checked, &repair).unwrap();
    assert_eq!(impact.unaffected, ["closing"]);
    let mut revision = repair::Revision {
        task_snapshots: BTreeMap::new(),
        schema: "reel.workflow-repair-revision.v2".into(),
        source_commit: fixture.profile.commit.clone(),
        native_inputs: BTreeMap::new(),
    };
    for task in baseline
        .tasks
        .iter()
        .filter(|t| impact.rebuild.contains(&t.id))
    {
        if let Some(before) = &task.native_inputs {
            let raw = if task.id == "opening" {
                fixture.profile.declaration_bytes().unwrap()
            } else {
                serde_json::to_vec(&json!({"schema":"reel.synthetic-unqualified-repair-declaration.v1","task":task.id,"source_commit":revision.source_commit,"admitted":false})).unwrap()
            };
            raw_save(&root, &format!("declarations/{}.json", task.id), &raw);
            revision.native_inputs.insert(
                task.id.clone(),
                review::Edit {
                    before: before.clone(),
                    after: repin(before, &raw),
                },
            );
        }
    }
    let candidate = repair::materialize(&baseline, &checked, &repair, &revision).unwrap();
    let packet = make_packet(&candidate.plan, "opening", BTreeMap::new()).unwrap();
    assert_ne!(packet.task_key, opening.packet.task_key);
    assert_eq!(
        make_packet(&candidate.plan, "closing", BTreeMap::new()).unwrap(),
        closing.packet
    );
    // Fail authored correspondence before closing the completed source job.
    // This local check does not replace the subsequent exact native snapshot
    // check, protected ingestion or admitted guarded byte checks.
    reel_workflow::publication::bind_comic(&packet, &mut Captured(&bytes)).unwrap();
    let mut source: NativeProfile =
        decode(&serde_json::to_vec(&opening.profile.source).unwrap()).unwrap();
    source.snapshot = fixture.profile.clone();
    let mut admission = report_profile.admission.clone();
    admission.scope.operation = tig_store::Key::parse("rooted-opening-repair-original").unwrap();
    let attempt = "rooted-opening-attempt-1";
    for (name, raw) in [
        ("baseline.json", serde_json::to_vec(&baseline).unwrap()),
        ("repair.json", serde_json::to_vec(&repair).unwrap()),
        ("revision.json", serde_json::to_vec(&revision).unwrap()),
        ("candidate.json", serde_json::to_vec(&candidate).unwrap()),
        ("plan.json", serde_json::to_vec(&candidate.plan).unwrap()),
        ("packet.json", serde_json::to_vec(&packet).unwrap()),
        ("source.json", serde_json::to_vec(&source).unwrap()),
        ("admission.json", serde_json::to_vec(&admission).unwrap()),
    ] {
        raw_save(&root, name, &raw);
    }
    save(
        &root,
        "original-operation.json",
        &json!({"schema":"reel.native-rooted-part-repair-original.v1","task_key":packet.task_key,"attempt_id":attempt,"scope":admission.scope,"source_repository":fixture.temp.path(),"report":report_root,"original":original,"runtime":runtime,"runtime_sha256":runtime_asset.sha256,"new_job":true,"original_runtime_adopted":false}),
    );
    drop(SnapshotInputs::open(&packet, &source.snapshot).unwrap());
    let close = root.join("report-close");
    fs::create_dir(&close).unwrap();
    save(&close, "target.json", &report_profile.binding.target);
    let ticket = close.join("ticket.json");
    phase(&root, "prepare-original-report-close");
    let out = Command::new(&report_profile.runtime.path)
        .arg("prepare-close")
        .arg(report_root.join("journal"))
        .arg(report_root.join("profile.json"))
        .arg(close.join("target.json"))
        .arg(&ticket)
        .arg("--apply")
        .output()
        .unwrap();
    success(&close, "prepare-result.json", &out);
    let ticket_pin = digest(&fs::read(&ticket).unwrap());
    save(&close, "ticket-pin.json", &ticket_pin);
    phase(&root, "close-original-report-into-retained-coverage");
    let out = Command::new(&report_profile.runtime.path)
        .arg("close-job")
        .arg(&ticket)
        .arg(&ticket_pin)
        .arg(close.join("closed.json"))
        .arg("--apply")
        .output()
        .unwrap();
    success(&close, "apply-result.json", &out);
    let closed: reel_workflow_native::closing::Outcome = load(&close, "closed.json");
    assert!(closed.slot_idle && closed.original_clear_complete);
    let prepared = admission::Prepared::open(&packet, &source, &admission, attempt).unwrap();
    let preview = prepared.preview().unwrap();
    assert!(preview.declared_objects + 4 <= preview.changed_path_limit);
    save(&root, "admission-preview.json", &preview);
    phase(&root, "accept-corrected-opening-original-operation");
    let receipt = prepared.accept().unwrap();
    save(&root, "receipt.json", &receipt);
    let ingress = root.join("source-ingress");
    fs::create_dir(&ingress).unwrap();
    for (i, (id, raw)) in changed.iter().enumerate() {
        let name = format!("changed-{i}.json");
        raw_save(&ingress, &name, raw);
        save(
            &root,
            &format!("source-ingress-{i}.json"),
            &json!({"logical_id":id,"source":name,"object":Object::from_bytes(raw)}),
        );
        phase(&root, &format!("publish-protected-input-{i}"));
        let publication = tig_git::annex::workflow::publication::publish(
            &tig_git::annex::workflow::publication::Profile {
                schema: "tig.annex-file-publication.experimental-1".into(),
                pool: source.source.pool.clone(),
                store: source.source.store.clone(),
                source_root: ingress.clone(),
                source: LogicalPath::parse(&name).unwrap(),
                source_owned: true,
                expected: Some(Object::from_bytes(raw)),
                include_private_location: false,
                maximum_bytes: 16 * 1024 * 1024,
                minimum_free_bytes: 100 * 1024 * 1024,
            },
        )
        .unwrap();
        assert!(publication.report().reopened_verified);
        assert_eq!(publication.report().object, Object::from_bytes(raw));
        save(
            &root,
            &format!("source-published-{i}.json"),
            publication.report(),
        );
    }
    let acknowledgements = root.join("acknowledgements");
    fs::create_dir(&acknowledgements).unwrap();
    let profile = handoff::Profile {
        schema: "reel.workflow-native-handoff.v1".into(),
        source,
        admission,
        binding: handoff::Binding {
            schema: "reel.workflow-native-handoff-binding.v1".into(),
            admission: receipt.clone(),
            target: report_profile.binding.target.clone(),
        },
        runtime: Tool {
            path: runtime.clone(),
            sha256: runtime_asset.sha256,
            bytes: runtime_asset.bytes,
        },
        acknowledgements,
    };
    save(&root, "profile.json", &profile);
    let journal = root.join("journal");
    phase(&root, "create-corrected-part-handoff");
    let out = Command::new(&runtime)
        .arg("create-handoff")
        .arg(root.join("packet.json"))
        .arg(root.join("profile.json"))
        .arg(&journal)
        .arg("--apply")
        .output()
        .unwrap();
    success(&root, "create-result.json", &out);
    let mut inputs =
        NativeInputs::open_admitted(&packet, &profile.source, &profile.admission, &receipt)
            .unwrap();
    reel_workflow::publication::bind_comic(&packet, &mut inputs).unwrap();
    drop(inputs);
    let bindings = graph::Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::from([
            (
                "opening".into(),
                binding(journal.clone(), root.join("profile.json")),
            ),
            (
                "closing".into(),
                binding(
                    original.join("closing-journal"),
                    original.join("closing-profile.json"),
                ),
            ),
        ]),
    };
    save(&root, "bindings.json", &bindings);
    phase(&root, "dispatch-corrected-part-under-real-scoped-plan");
    let mut generated = Vec::new();
    automatic_dispatch::run_graph_root(
        &root,
        &packet,
        &profile,
        &runtime,
        &root.join("result.json"),
        &mut generated,
        &BTreeMap::new(),
        &candidate.plan,
        &bindings,
    );
    for (name, raw) in generated {
        // Returned controls include newly staged bindings. Preserve the exact
        // original dispatch inputs and record those observations separately.
        raw_save(&root.join("generated"), name, &raw);
    }
    verify_completed_part(
        &root,
        &evidence,
        &candidate_root,
        &report_root,
        &controls,
        &waiting,
    );
}

fn verify_completed_part(
    root: &Path,
    evidence: &Path,
    candidate_root: &Path,
    report_root: &Path,
    controls: &Path,
    waiting: &Path,
) {
    let packet: Packet = load(root, "packet.json");
    let profile: handoff::Profile = load(root, "profile.json");
    let receipt: admission::Receipt = load(root, "receipt.json");
    let preview: admission::Preview = load(root, "admission-preview.json");
    let runtime = &profile.runtime.path;
    let attempt = &receipt.attempt_id;
    let source_repository = &profile.source.snapshot.repository;
    let journal = root.join("journal");
    let marker: serde_json::Value = load(root, "original-operation.json");
    assert_eq!(marker["task_key"], packet.task_key);
    assert_eq!(marker["attempt_id"], receipt.attempt_id);
    assert_eq!(marker["runtime_sha256"], profile.runtime.sha256);
    assert_eq!(receipt.task_key, packet.task_key);
    assert_eq!(
        receipt.scope.operation.as_str(),
        "rooted-opening-repair-original"
    );
    assert_eq!(
        reel_workflow::handoff::Journal::open(&root.join("journal"))
            .unwrap()
            .state(),
        reel_workflow::handoff::State::Retained
    );
    let check_waiting = || {
        let packet: Packet = load(waiting, "next-packet.json");
        let profile: handoff::Profile = load(waiting, "next-handoff-profile.json");
        let journal = reel_workflow::handoff::Journal::open(&waiting.join("next-journal")).unwrap();
        assert_eq!(journal.state(), reel_workflow::handoff::State::Waiting);
        assert_eq!(journal.ticket().packet, packet);
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
    };
    check_waiting();
    let index = tig_store::source_claims::protection::Index::open(
        &profile.admission.index_root,
        &receipt.index_checkpoint,
    )
    .unwrap();
    let verification_checkpoint = canonical_digest(&index.checkpoint()).unwrap();
    drop(index);
    save(
        evidence,
        "verification-start.json",
        &json!({"task_key":packet.task_key,"activation":receipt.activation,"work":receipt.work,"checkpoint_sha256":verification_checkpoint,"runtime":profile.runtime}),
    );
    let mut original_pins: BTreeMap<PathBuf, String> = load(root, "original-pins.json");
    for name in [
        "packet.json",
        "profile.json",
        "source.json",
        "admission.json",
        "receipt.json",
        "result.json",
        "original-operation.json",
    ] {
        let path = root.join(name);
        original_pins.insert(path.clone(), digest(&fs::read(path).unwrap()));
    }
    save(evidence, "verification-original-pins.json", &original_pins);
    let parents: reel_workflow_native::upstream::Parents = load(controls, "parents.json");
    let opening = &parents.parents["opening"];
    let closing = &parents.parents["closing"];
    verify_parent(opening);
    let old_clean = native_bytes(
        &opening.profile.source,
        &opening.result.artifacts["page01-clean.png"],
    );
    let old_reading = native_bytes(
        &opening.profile.source,
        &opening.result.artifacts["page01-reading.png"],
    );
    let report_packet: Packet = load(report_root, "packet.json");
    let report_profile: handoff::Profile = load(report_root, "profile.json");
    let report_result: ReturnedResult = load(report_root, "result.json");
    let report_ack: retention::Acknowledgement = load(report_root, "retained.json");
    let guard = retention::verify(
        &report_packet,
        &report_profile.source,
        &report_profile.admission,
        &report_profile.binding.admission,
        &report_result,
        &report_ack,
    )
    .unwrap();
    let checked = guard.review_report().unwrap().clone();
    drop(guard);
    let result: ReturnedResult = load(root, "result.json");
    let ack: retention::Acknowledgement = load(
        &profile.acknowledgements,
        &format!("retained-{}-{}.json", packet.task_key, attempt),
    );
    if root.join("retained.json").exists() {
        let existing: retention::Acknowledgement = load(root, "retained.json");
        assert_eq!(
            canonical_digest(&existing).unwrap(),
            canonical_digest(&ack).unwrap()
        );
    } else {
        save(root, "retained.json", &ack);
    }
    phase(
        root,
        "verify-corrected-part-retained-proof-and-historical-report",
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
    assert_eq!(
        native_bytes(&profile.source, &result.artifacts["page01-clean.png"]),
        old_clean
    );
    assert_ne!(
        native_bytes(&profile.source, &result.artifacts["page01-reading.png"]),
        old_reading
    );
    for name in [
        "page01-clean.png",
        "page01-reading.png",
        "transcript.json",
        "editable.packet",
    ] {
        assert_eq!(
            native_bytes(&profile.source, &result.artifacts[name]),
            fs::read(candidate_root.join(format!("local-reading/parts/opening/{name}"))).unwrap()
        );
    }
    let reviewed = retention::review_copy(
        &packet,
        &profile.source,
        &profile.admission,
        &receipt,
        &result,
        &ack,
    )
    .unwrap();
    let targets: BTreeMap<_, _> = checked
        .copy()
        .units
        .iter()
        .filter(
            |(_, unit)| matches!(unit, review::Unit::Comic { page_id, .. } if page_id == "page01"),
        )
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    assert_eq!(reviewed.copy().units, targets);
    assert_eq!(reviewed.copy().source_commit, packet.source_commit);
    save(root, "corrected-part-review-copy.json", reviewed.copy());
    drop(reviewed);
    let reopened = retention::verify(
        &report_packet,
        &report_profile.source,
        &report_profile.admission,
        &report_profile.binding.admission,
        &report_result,
        &report_ack,
    )
    .unwrap();
    assert_eq!(
        reopened.review_report().unwrap().report_sha256(),
        checked.report_sha256()
    );
    drop(reopened);
    verify_parent(closing);
    let checkpoint = || {
        let index = tig_store::source_claims::protection::Index::open(
            &profile.admission.index_root,
            &receipt.index_checkpoint,
        )
        .unwrap();
        canonical_digest(&index.checkpoint()).unwrap()
    };
    let completed_checkpoint = checkpoint();
    assert_eq!(verification_checkpoint, completed_checkpoint);
    let replay = Command::new(runtime)
        .arg("dispatch-task")
        .arg(root.join("automatic-plan.json"))
        .arg(root.join("automatic-bindings.json"))
        .arg("opening")
        .arg(root.join("automatic-config.json"))
        .arg(root.join("automatic-dispatch"))
        .arg("--apply")
        .output()
        .unwrap();
    success(root, "completed-replay.json", &replay);
    assert_eq!(completed_checkpoint, checkpoint());
    assert_eq!(load::<ReturnedResult>(root, "result.json"), result);
    check_waiting();
    for (path, pin) in &original_pins {
        assert_eq!(&digest(&fs::read(path).unwrap()), pin);
    }
    assert_eq!(
        reel_workflow::handoff::Journal::open(&journal)
            .unwrap()
            .state(),
        reel_workflow::handoff::State::Retained
    );
    let worker: worker::Ticket = load(&root.join("automatic-dispatch"), "worker-ticket.json");
    let outcome = json!({"schema":"reel.native-rooted-part-repair-qualification.v1","job":root,"source_repository":source_repository,"task_key":packet.task_key,"attempt_id":attempt,"source_commit":packet.source_commit,"activation":receipt.activation,"work":receipt.work,"scope":receipt.scope,"runtime":profile.runtime,"declared_objects":preview.declared_objects,"checkpoint_sha256":completed_checkpoint,"real_native_source_verified":true,"original_report_closed_with_original_runtime":true,"same_slot_reused":true,"retained_report_reopened_after_reuse":true,"part_retained":true,"part_task_completed":true,"part_replay_actual_index_unchanged":true,"clean_art_unchanged":true,"reading_pixels_changed":true,"editable_part_matches_local_qualified_candidate":true,"page_panel_balloon_ids_preserved":true,"closing_packet_and_history_preserved":true,"other_waiting_generation_reopened":true,"worker_directory":worker.directory(),"whole_corrected_issue_retained":false,"descendant_source_declarations_pending":true,"contextual_viewing":false,"finding_closed":false,"selected":false,"approved":false,"runtime_adopted":false,"scratch_retired":false});
    save(root, "qualification-result.json", &outcome);
    save(evidence, "qualification-result.json", &outcome);
    phase(
        root,
        "retained-corrected-part-awaiting-fresh-derived-controls-and-complete-issue",
    );
}

#[test]
#[ignore = "Verify only the preserved completed part after fixture binding-observation bookkeeping failed; original runtime/receipt/result, no readmission, input publication or rerender"]
fn verify_original_completed_part_after_binding_observation_recording_failure() {
    let env = |key| PathBuf::from(std::env::var_os(key).expect(key));
    let root = env("REEL_NATIVE_PART_REPAIR_COMPLETED_ROOT");
    let evidence = env("REEL_NATIVE_PART_REPAIR_VERIFICATION_EVIDENCE");
    let qualified = env("REEL_NATIVE_PART_REPAIR_CANDIDATE");
    let controls = env("REEL_NATIVE_PART_REPAIR_CONTROLS");
    let waiting = env("REEL_NATIVE_PART_REPAIR_WAITING");
    let marker: serde_json::Value = load(&root, "original-operation.json");
    let report = PathBuf::from(marker["report"].as_str().unwrap());
    let completed: serde_json::Value = load(&root, "automatic-completion.json");
    assert_eq!(completed["task_completed"], true);
    verify_completed_part(
        &root,
        &evidence,
        &qualified.join("candidate"),
        &report,
        &controls,
        &waiting,
    );
}

#[test]
#[ignore = "Read-only follow-up to the retained corrected-part operation; export revision v3 under the exact original root packet, no new native admission/runtime/selection or effects"]
fn retained_part_packet_survives_native_v3_revision_export() {
    let env = |key| PathBuf::from(std::env::var_os(key).expect(key));
    let original = env("REEL_NATIVE_PART_REPAIR_COMPLETED_ROOT");
    let evidence = env("REEL_NATIVE_REPAIR_V3_EXPORT_EVIDENCE");
    let executable = env("REEL_NATIVE_HANDOFF_TOOL");
    let marker: serde_json::Value = load(&original, "original-operation.json");
    let report = PathBuf::from(marker["report"].as_str().unwrap());
    let packet: Packet = load(&original, "packet.json");
    let plan: Plan = load(&original, "plan.json");
    let profile: handoff::Profile = load(&original, "profile.json");
    let result: ReturnedResult = load(&original, "result.json");
    let ack: retention::Acknowledgement = load(&original, "retained.json");
    assert_eq!(
        reel_workflow::handoff::Journal::open(&original.join("journal"))
            .unwrap()
            .state(),
        reel_workflow::handoff::State::Retained
    );
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
    let checkpoint = || {
        let index = tig_store::source_claims::protection::Index::open(
            &profile.admission.index_root,
            &profile.binding.admission.index_checkpoint,
        )
        .unwrap();
        canonical_digest(&index.checkpoint()).unwrap()
    };
    let before = checkpoint();
    let candidate: serde_json::Value = load(&original, "candidate.json");
    let mut revision: repair::Revision = load(&original, "revision.json");
    assert_eq!(revision.schema, "reel.workflow-repair-revision.v2");
    revision.schema = "reel.workflow-repair-revision.v3".into();
    revision.task_snapshots = candidate["impact"]["rebuild"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| {
            let id = id.as_str().unwrap().to_owned();
            let commit = plan.source_commit_for(&id).unwrap().to_owned();
            (id, commit)
        })
        .collect();
    save(&evidence, "revision-v3.json", &revision);
    let destination = evidence.join("plan-v3.json");
    let mut child = Command::new(&executable);
    child.arg("export-repair-plan");
    for name in [
        "packet.json",
        "source.json",
        "admission.json",
        "receipt.json",
        "result.json",
        "retained.json",
    ] {
        child.arg(report.join(name));
    }
    let output = child
        .arg(original.join("baseline.json"))
        .arg(original.join("repair.json"))
        .arg(evidence.join("revision-v3.json"))
        .arg(&destination)
        .arg("--apply")
        .output()
        .unwrap();
    success(&evidence, "export-result.json", &output);
    let exported: Plan = load(&evidence, "plan-v3.json");
    assert_eq!(exported, plan);
    assert_eq!(
        make_packet(&exported, "opening", BTreeMap::new()).unwrap(),
        packet
    );
    assert_eq!(result.task_key, packet.task_key);
    assert_eq!(before, checkpoint());
    let pins: BTreeMap<PathBuf, String> = load(&original, "original-pins.json");
    for (path, pin) in &pins {
        assert_eq!(&digest(&fs::read(path).unwrap()), pin);
    }
    save(
        &evidence,
        "qualification-result.json",
        &json!({"schema":"reel.native-repair-v3-export-qualification.v1","original_job":original,"retained_part_task_key":packet.task_key,"retained_part_proof_reopened":true,"native_v3_export":true,"plan_byte_semantics_unchanged":true,"original_root_packet_and_result_key_unchanged":true,"actual_index_unchanged":true,"checkpoint_sha256":before,"new_native_admission":false,"runtime_adopted":false,"later_consumer_commit_executed":false,"whole_corrected_issue_retained":false,"selected":false,"approved":false}),
    );
}
