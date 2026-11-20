//! Existing retained issue and original ancestors -> newly owned review controls.
//! Guarded original part slot reuse only; preserve runtimes and uncertain work.
use super::*;
use reel_workflow_native::{admission, graph, handoff, retention, upstream};
use std::io::Read;

fn save(root: &Path, name: &str, value: &impl serde::Serialize) {
    let path = root.join(name);
    assert!(!path.exists(), "existing control refused: {name}");
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn load<T: serde::de::DeserializeOwned>(root: &Path, name: &str) -> T {
    decode(&fs::read(root.join(name)).unwrap()).unwrap()
}
fn graph_binding(journal: PathBuf, profile: PathBuf) -> graph::Binding {
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
#[ignore = "Existing retained synthetic issue and exact ancestors only; guarded original part close and explicit same-slot new task/runtime, admits/retains review controls, preserves Waiting jobs and scratch"]
fn retained_issue_and_original_ancestors_drive_owned_review_controls() {
    let original = PathBuf::from(
        std::env::var_os("REEL_OWNED_ISSUE_REVIEW_ROOT").expect("retained issue root"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_OWNED_ISSUE_REVIEW_EVIDENCE").expect("new evidence directory"),
    );
    let runtime = PathBuf::from(
        std::env::var_os("REEL_OWNED_ISSUE_REVIEW_RUNTIME").expect("frozen current runtime"),
    );
    let actor = std::env::var("REEL_OWNED_ISSUE_REVIEW_ACTOR").expect("explicit actor required");
    let waiting = PathBuf::from(
        std::env::var_os("REEL_OWNED_ISSUE_REVIEW_WAITING_ROOT")
            .expect("protected Waiting job root"),
    );
    assert!(
        original.is_absolute()
            && evidence.is_absolute()
            && evidence.is_dir()
            && runtime.is_absolute()
            && waiting.is_absolute()
    );
    assert_eq!(
        actor, "slot2",
        "this bounded fixture qualifies only guarded reuse of original opening slot2"
    );
    let root = evidence.join("job");
    fs::create_dir(&root).unwrap();
    let producer: Packet = load(&original, "issue-packet.json");
    let produced: ReturnedResult = load(&original, "issue-result.json");
    let producer_profile: handoff::Profile = load(&original, "issue-profile.json");
    let producer_ack: retention::Acknowledgement = load(&original, "issue-retained.json");
    assert_eq!(producer.project, "synthetic-comic");
    assert_eq!(producer.task.id, "issue-assemble");
    let waiting_packet: Packet = load(&waiting, "next-packet.json");
    let waiting_profile: handoff::Profile = load(&waiting, "next-handoff-profile.json");
    let waiting_journal = waiting.join("next-journal");
    let check_waiting = || {
        let journal = reel_workflow::handoff::Journal::open(&waiting_journal).unwrap();
        assert_eq!(journal.state(), reel_workflow::handoff::State::Waiting);
        assert_eq!(journal.ticket().packet, waiting_packet);
        drop(
            admission::Prepared::open(
                &waiting_packet,
                &waiting_profile.source,
                &waiting_profile.admission,
                &waiting_profile.binding.admission.attempt_id,
            )
            .unwrap()
            .reopen(&waiting_profile.binding.admission)
            .unwrap(),
        );
    };
    check_waiting();
    let mut preserved = vec![
        original.join("issue-packet.json"),
        original.join("issue-profile.json"),
        original.join("issue-result.json"),
        original.join("issue-retained.json"),
        waiting.join("next-packet.json"),
        waiting.join("next-handoff-profile.json"),
    ];
    let inherited = producer_profile.source.snapshot.upstream.as_ref().unwrap();
    let mut parents = upstream::Parents {
        schema: "reel.workflow-native-parents.v1".into(),
        parents: BTreeMap::new(),
    };
    let mut graph = graph::Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::new(),
    };
    for (id, old) in &inherited.parents {
        let stem = if id == "issue-control" { "control" } else { id };
        let path = original.join(format!("{stem}-profile.json"));
        let profile: handoff::Profile = decode(&fs::read(&path).unwrap()).unwrap();
        let ack_name = format!(
            "retained-{}-{}.json",
            old.packet.task_key, old.result.attempt_id
        );
        let acknowledgement: retention::Acknowledgement =
            decode(&fs::read(profile.acknowledgements.join(ack_name)).unwrap()).unwrap();
        preserved.push(path.clone());
        graph.tasks.insert(
            id.clone(),
            graph_binding(original.join(format!("{stem}-journal")), path),
        );
        parents.parents.insert(
            id.clone(),
            upstream::ParentControl {
                packet: old.packet.clone(),
                result: old.result.clone(),
                profile,
                acknowledgement,
            },
        );
    }
    graph.tasks.insert(
        producer.task.id.clone(),
        graph_binding(
            original.join("issue-journal"),
            original.join("issue-profile.json"),
        ),
    );
    parents.parents.insert(
        producer.task.id.clone(),
        upstream::ParentControl {
            packet: producer.clone(),
            result: produced.clone(),
            profile: load(&original, "issue-profile.json"),
            acknowledgement: producer_ack,
        },
    );
    let pins: BTreeMap<_, _> = preserved
        .iter()
        .map(|path| (path.clone(), digest(&fs::read(path).unwrap())))
        .collect();
    save(&root, "original-control-pins.json", &pins);
    let bindings =
        review_control::bindings(&producer, &produced, Some(&inherited.bindings)).unwrap();
    assert!(review_control::bindings(&producer, &produced, None).is_err());
    let mut changed = inherited.bindings.clone();
    changed.inputs.values_mut().next().unwrap().task_id = "wrong-original".into();
    assert_eq!(
        review_control::bindings(&producer, &produced, Some(&changed))
            .err()
            .unwrap()
            .code,
        "review-control-inherited-bindings-pin"
    );
    let mut source: reel_workflow_native::NativeProfile =
        decode(&serde_json::to_vec(&producer_profile.source).unwrap()).unwrap();
    source.snapshot.upstream = Some(upstream::Frozen::capture(
        &bindings,
        &parents.borrowed().unwrap(),
    ));
    let runtime_raw = fs::read(&runtime).unwrap();
    let candidate = Artifact {
        logical_id: "tools/owned-issue-review-control".into(),
        sha256: digest(&runtime_raw),
        bytes: runtime_raw.len() as u64,
        media_type: "application/octet-stream".into(),
    };
    assert_ne!(
        candidate.sha256, producer_profile.runtime.sha256,
        "new task does not retarget the original producer runtime"
    );
    let bindings_raw = bindings.canonical_bytes().unwrap();
    let mut task = producer.task.clone();
    task.id = "owned-issue-review-controls".into();
    task.native_inputs = Some(Fixture::declaration(&source.snapshot));
    task.upstream_inputs = Some(Artifact {
        logical_id: "control/owned-issue-review-origins".into(),
        sha256: digest(&bindings_raw),
        bytes: bindings_raw.len() as u64,
        media_type: "application/json".into(),
    });
    task.inputs = review_control::inputs(&producer, &produced).unwrap();
    task.depends_on = parents.parents.keys().cloned().collect();
    task.outputs = review_control::OUTPUTS
        .iter()
        .map(|name| {
            (
                (*name).into(),
                OutputContract {
                    media_type: "application/json".into(),
                    max_bytes: MAX_CONTROL_BYTES as u64,
                    required: true,
                },
            )
        })
        .collect();
    let tool = task.tool.as_mut().unwrap();
    tool.capability = ToolCapability::ComicReviewControl;
    tool.executable = candidate.clone();
    tool.request = produced.worker_receipt.clone().unwrap();
    tool.source_bindings = review_control::source_bindings(&producer, &produced).unwrap();
    let mut plan: Plan = load(&original, "workflow-plan.json");
    plan.tasks.push(task);
    let child = make_packet(
        &plan,
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
    let mut admission = producer_profile.admission.clone();
    admission.scope.actor = tig_store::Key::parse(&actor).unwrap();
    admission.scope.operation =
        tig_store::Key::parse("owned-issue-review-control-original").unwrap();
    assert_ne!(
        admission.scope.actor,
        producer_profile.admission.scope.actor
    );
    assert_ne!(admission.scope.actor, waiting_profile.admission.scope.actor);
    save(&root, "plan.json", &plan);
    save(&root, "packet.json", &child);
    save(&root, "source.json", &source);
    save(&root, "parents.json", &parents);
    save(&root, "upstream.json", &bindings);
    save(&root, "admission-profile.json", &admission);
    save(
        &root,
        "original-operation.json",
        &json!({"schema":"reel.owned-issue-review-original.v1","task_key":child.task_key,"attempt":"owned-review-attempt-1","scope":admission.scope,"runtime":runtime,"runtime_sha256":candidate.sha256,"original_issue":producer.task_key,"new_task":true,"original_runtime_adoption":false}),
    );
    drop(SnapshotInputs::open(&child, &source.snapshot).unwrap());
    let opening = &parents.parents["opening"];
    assert_eq!(opening.profile.admission.scope.actor, admission.scope.actor);
    drop(
        retention::verify(
            &opening.packet,
            &opening.profile.source,
            &opening.profile.admission,
            &opening.profile.binding.admission,
            &opening.result,
            &opening.acknowledgement,
        )
        .unwrap(),
    );
    let close = root.join("opening-close");
    fs::create_dir(&close).unwrap();
    save(&close, "target.json", &opening.profile.binding.target);
    let ticket = close.join("ticket.json");
    let prepare = Command::new(&opening.profile.runtime.path)
        .arg("prepare-close")
        .arg(original.join("opening-journal"))
        .arg(original.join("opening-profile.json"))
        .arg(close.join("target.json"))
        .arg(&ticket)
        .arg("--apply")
        .output()
        .unwrap();
    save(
        &close,
        "prepare-result.json",
        &json!({"exit_code":prepare.status.code(),"stdout":String::from_utf8_lossy(&prepare.stdout),"stderr":String::from_utf8_lossy(&prepare.stderr)}),
    );
    assert!(
        prepare.status.success(),
        "{} {}",
        String::from_utf8_lossy(&prepare.stdout),
        String::from_utf8_lossy(&prepare.stderr)
    );
    let ticket_pin = digest(&fs::read(&ticket).unwrap());
    save(&close, "ticket-pin.json", &ticket_pin);
    let outcome = close.join("closed.json");
    let apply = Command::new(&opening.profile.runtime.path)
        .arg("close-job")
        .arg(&ticket)
        .arg(&ticket_pin)
        .arg(&outcome)
        .arg("--apply")
        .output()
        .unwrap();
    save(
        &close,
        "apply-result.json",
        &json!({"exit_code":apply.status.code(),"stdout":String::from_utf8_lossy(&apply.stdout),"stderr":String::from_utf8_lossy(&apply.stderr)}),
    );
    assert!(
        apply.status.success(),
        "{} {}",
        String::from_utf8_lossy(&apply.stdout),
        String::from_utf8_lossy(&apply.stderr)
    );
    let closed: reel_workflow_native::closing::Outcome =
        decode(&fs::read(&outcome).unwrap()).unwrap();
    assert!(closed.slot_idle && closed.original_clear_complete);
    let prepared = upstream::Admission::prepare(
        &child,
        &source,
        &admission,
        "owned-review-attempt-1",
        parents.borrowed().unwrap(),
    )
    .unwrap();
    let preview = prepared.preview().unwrap();
    assert!(preview.declared_objects + 4 <= preview.changed_path_limit);
    save(&root, "admission-preview.json", &preview);
    let receipt = prepared.accept().unwrap();
    save(&root, "admission-receipt.json", &receipt);
    let acknowledgements = root.join("acknowledgements");
    fs::create_dir(&acknowledgements).unwrap();
    let profile = handoff::Profile {
        schema: "reel.workflow-native-handoff.v1".into(),
        source,
        admission,
        binding: handoff::Binding {
            schema: "reel.workflow-native-handoff-binding.v1".into(),
            retained_owner_policy: None,
            admission: receipt,
            target: producer_profile.binding.target.clone(),
        },
        runtime: Tool {
            path: runtime.clone(),
            sha256: candidate.sha256,
            bytes: candidate.bytes,
        },
        acknowledgements,
    };
    let profile_path = root.join("profile.json");
    save(&root, "profile.json", &profile);
    let journal = root.join("journal");
    let create = Command::new(&runtime)
        .arg("create-handoff")
        .arg(root.join("packet.json"))
        .arg(&profile_path)
        .arg(&journal)
        .arg("--apply")
        .output()
        .unwrap();
    save(
        &root,
        "create-handoff.json",
        &json!({"exit_code":create.status.code(),"stdout":String::from_utf8_lossy(&create.stdout),"stderr":String::from_utf8_lossy(&create.stderr)}),
    );
    assert!(
        create.status.success(),
        "{} {}",
        String::from_utf8_lossy(&create.stdout),
        String::from_utf8_lossy(&create.stderr)
    );
    graph.tasks.insert(
        child.task.id.clone(),
        graph_binding(journal.clone(), profile_path),
    );
    let mut controls = Vec::new();
    automatic_dispatch::run_graph_root(
        &root,
        &child,
        &profile,
        &runtime,
        &root.join("result.json"),
        &mut controls,
        &BTreeMap::new(),
        &plan,
        &graph,
    );
    for (name, raw) in controls {
        fs::write(root.join(name), raw).unwrap();
    }
    let result: ReturnedResult = load(&root, "result.json");
    let ack: retention::Acknowledgement = decode(
        &fs::read(profile.acknowledgements.join(format!(
            "retained-{}-{}.json",
            child.task_key, result.attempt_id
        )))
        .unwrap(),
    )
    .unwrap();
    save(&root, "retained.json", &ack);
    assert_eq!(
        reel_workflow::handoff::Journal::open(&journal)
            .unwrap()
            .state(),
        reel_workflow::handoff::State::Retained
    );
    drop(
        retention::verify(
            &child,
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
    let replay = Command::new(&runtime)
        .arg("dispatch-task")
        .arg(root.join("automatic-plan.json"))
        .arg(root.join("automatic-bindings.json"))
        .arg(&child.task.id)
        .arg(root.join("automatic-config.json"))
        .arg(root.join("automatic-dispatch"))
        .arg("--apply")
        .output()
        .unwrap();
    save(
        &root,
        "checkpoint-replay.json",
        &json!({"exit_code":replay.status.code(),"stdout":String::from_utf8_lossy(&replay.stdout),"stderr":String::from_utf8_lossy(&replay.stderr)}),
    );
    assert!(replay.status.success());
    assert_eq!(before, checkpoint());
    let mut bytes = BTreeMap::new();
    for (name, pin) in &result.artifacts {
        let mut stream = tig_git::annex::workflow::reader::open(
            &profile.source.source,
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
        assert_eq!(raw.len() as u64, pin.bytes);
        assert_eq!(digest(&raw), pin.sha256);
        fs::write(root.join(name), &raw).unwrap();
        bytes.insert(name.clone(), raw);
    }
    let copy: review::Copy = decode(&bytes["review-copy.json"]).unwrap();
    let request: review::Request = decode(&bytes["review-request.json"]).unwrap();
    assert_eq!(copy.units.len(), 6);
    assert_eq!(copy.source_commit, producer.source_commit);
    assert_eq!(
        copy.format,
        review::Format::Comic {
            page_order: vec!["page01".into(), "page02".into()]
        }
    );
    assert_eq!(copy.review, result.artifacts["review-sources.json"]);
    assert_eq!(
        request.copy_input,
        result.artifacts["review-copy.json"].logical_id
    );
    assert_eq!(request.production_commit, producer.source_commit);
    for (path, pin) in &pins {
        assert_eq!(&digest(&fs::read(path).unwrap()), pin);
    }
    check_waiting();
    assert_eq!(before, checkpoint());
    save(
        &evidence,
        "qualification-result.json",
        &json!({"schema":"reel.owned-issue-review-qualification.v1","job":root,"producer_task_key":producer.task_key,"child_task_key":child.task_key,"attempt_id":result.attempt_id,"controls_retained":true,"task_completed":true,"pages":2,"units":copy.units.len(),"original_ancestor_selectors_preserved":true,"actual_replay_checkpoint_unchanged":true,"checkpoint_sha256":before,"original_controls_unchanged":true,"other_waiting_generation_reopened":true,"runtime_adopted":false,"provider_called":false,"approved":false,"selected":false,"scratch_retired":false}),
    );
}
