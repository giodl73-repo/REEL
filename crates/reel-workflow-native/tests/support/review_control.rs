//! Actual retained comic -> owned derived review controls -> protected LLM handoff.
use super::*;
use reel_workflow_native::{admission, closing, dispatch, graph, handoff, staging, upstream};
fn save(root: &Path, name: &str, value: &impl serde::Serialize) {
    fs::write(root.join(name), serde_json::to_vec(value).unwrap()).unwrap();
}
fn invoke(exe: &Path, args: &[&std::ffi::OsStr]) {
    let out = Command::new(exe).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "{} {} {}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
fn close(root: &Path, journal: &Path, profile_file: &Path, profile: &handoff::Profile) {
    fs::create_dir(root).unwrap();
    save(root, "target.json", &profile.binding.target);
    let ticket = root.join("ticket.json");
    invoke(
        &profile.runtime.path,
        &[
            "prepare-close".as_ref(),
            journal.as_os_str(),
            profile_file.as_os_str(),
            root.join("target.json").as_os_str(),
            ticket.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let pin = digest(&fs::read(&ticket).unwrap());
    save(root, "ticket-pin.json", &pin);
    let outcome = root.join("closed.json");
    invoke(
        &profile.runtime.path,
        &[
            "close-job".as_ref(),
            ticket.as_os_str(),
            pin.as_ref(),
            outcome.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let outcome: closing::Outcome = decode(&fs::read(outcome).unwrap()).unwrap();
    assert!(outcome.slot_idle && outcome.original_clear_complete);
}
fn origin_map(id: &str, result: &ReturnedResult) -> reel_workflow::upstream::Bindings {
    reel_workflow::upstream::Bindings {
        schema: "reel.workflow-upstream-inputs.v1".into(),
        inputs: result
            .artifacts
            .iter()
            .map(|(name, a)| {
                (
                    a.logical_id.clone(),
                    reel_workflow::upstream::Reference {
                        task_id: id.into(),
                        artifact: reel_workflow::upstream::Selector::Output { name: name.clone() },
                    },
                )
            })
            .chain(result.worker_receipt.iter().map(|a| {
                (
                    a.logical_id.clone(),
                    reel_workflow::upstream::Reference {
                        task_id: id.into(),
                        artifact: reel_workflow::upstream::Selector::WorkerReceipt,
                    },
                )
            }))
            .collect(),
    }
}
fn binding(pin: &reel_workflow::upstream::Bindings) -> Artifact {
    let raw = pin.canonical_bytes().unwrap();
    Artifact {
        logical_id: "control/review-origins".into(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    }
}
fn dep(result: &ReturnedResult) -> DependencyResult {
    DependencyResult {
        result_sha256: canonical_digest(result).unwrap(),
        artifacts: result.artifacts.clone(),
    }
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
#[ignore = "Existing retained synthetic comic only; closes/reuses original slot for owned derived controls and protected review handoff, preserves all roots"]
fn retained_comic_drives_owned_review_controls_and_a_protected_structured_review_handoff() {
    let original = PathBuf::from(
        std::env::var_os("REEL_REVIEW_CONTROL_PARENT").expect("Set retained comic root"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_REVIEW_CONTROL_EVIDENCE_DIRECTORY").expect("Set evidence directory"),
    );
    let exe = PathBuf::from(
        std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("Set qualified current CLI"),
    );
    let producer: Packet = decode(&fs::read(original.join("comic-packet.json")).unwrap()).unwrap();
    let produced: ReturnedResult =
        decode(&fs::read(original.join("comic-result.json")).unwrap()).unwrap();
    let producer_profile: handoff::Profile =
        decode(&fs::read(original.join("comic-profile.json")).unwrap()).unwrap();
    let producer_ack: reel_workflow_native::retention::Acknowledgement = decode(
        &fs::read(producer_profile.acknowledgements.join(format!(
            "retained-{}-{}.json",
            producer.task_key, produced.attempt_id
        )))
        .unwrap(),
    )
    .unwrap();
    let mut temp = tempfile::tempdir().unwrap();
    temp.disable_cleanup(true);
    let root = temp.path().to_path_buf();
    let marker = json!({"root":root,"original":original,"producer":producer.task_key,"producer_admission":producer_profile.binding.admission,"runtime":exe});
    save(&root, "original-operation.json", &marker);
    save(&evidence, "original-operation-roots.json", &marker);
    save(
        &root,
        "phase.json",
        &json!({"phase":"close-original-producer"}),
    );
    save(&root, "producer-profile.json", &producer_profile);
    close(
        &root.join("producer-close"),
        &original.join("comic-journal"),
        &root.join("producer-profile.json"),
        &producer_profile,
    );
    let origins = origin_map(&producer.task.id, &produced);
    let parent = upstream::Parent {
        packet: &producer,
        result: &produced,
        source: &producer_profile.source,
        admission: &producer_profile.admission,
        receipt: &producer_profile.binding.admission,
        acknowledgement: &producer_ack,
    };
    let parents = BTreeMap::from([(producer.task.id.clone(), parent)]);
    let mut source: reel_workflow_native::NativeProfile =
        serde_json::from_value(serde_json::to_value(&producer_profile.source).unwrap()).unwrap();
    source.snapshot.schema = "reel.workflow-native-composite-snapshot.v1".into();
    source.snapshot.upstream = Some(upstream::Frozen::capture(&origins, &parents));
    let raw_exe = fs::read(&exe).unwrap();
    let tool = producer.task.tool.as_ref().unwrap();
    let task = Task {
        id: "comic-review-controls".into(),
        native_inputs: Some(Fixture::declaration(&source.snapshot)),
        upstream_inputs: Some(binding(&origins)),
        operation: Operation::DeterministicTool,
        tool: Some(ToolContract {
            schema: "reel.workflow-tool.v1".into(),
            capability: ToolCapability::ComicReviewControl,
            executable: Artifact {
                logical_id: "tools/review-control".into(),
                sha256: digest(&raw_exe),
                bytes: raw_exe.len() as u64,
                media_type: "application/octet-stream".into(),
            },
            auxiliary_executables: BTreeMap::new(),
            publication: tool.publication.clone(),
            edition_id: tool.edition_id.clone(),
            request: produced.worker_receipt.clone().unwrap(),
            source_bindings: reel_workflow::review_control::source_bindings(&producer, &produced)
                .unwrap(),
            part: None,
        }),
        language: producer.task.language.clone(),
        prompt: String::new(),
        inputs: reel_workflow::review_control::inputs(&producer, &produced).unwrap(),
        depends_on: vec![producer.task.id.clone()],
        outputs: reel_workflow::review_control::OUTPUTS
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
            .collect(),
    };
    let mut plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: producer.project.clone(),
        source_commit: producer.source_commit.clone(),
        tasks: vec![producer.task.clone(), task],
    };
    let child = make_packet(
        &plan,
        "comic-review-controls",
        BTreeMap::from([(producer.task.id.clone(), dep(&produced))]),
    )
    .unwrap();
    let mut profile = producer_profile.admission.clone();
    profile.scope.operation = tig_store::Key::parse("comic-review-controls").unwrap();
    let case = AdmissionCase {
        temp,
        profile,
        source,
    };
    save(&root, "packet.json", &child);
    save(&root, "source.json", &case.source);
    save(&root, "admission-profile.json", &case.profile);
    save(
        &root,
        "phase.json",
        &json!({"phase":"accept-derived-controls"}),
    );
    let receipt =
        upstream::Admission::prepare(&child, &case.source, &case.profile, "attempt-2", parents)
            .unwrap()
            .accept()
            .unwrap();
    save(&root, "admission-receipt.json", &receipt);
    assert!(
        admission::Prepared::open(
            &producer,
            &producer_profile.source,
            &producer_profile.admission,
            &produced.attempt_id
        )
        .unwrap()
        .reopen(&producer_profile.binding.admission)
        .is_err()
    );
    let handoff = handoff_profile(&case, receipt.clone(), exe.clone());
    let profile_file = root.join("profile.json");
    save(&root, "profile.json", &handoff);
    let journal = root.join("journal");
    invoke(
        &exe,
        &[
            "create-handoff".as_ref(),
            root.join("packet.json").as_os_str(),
            profile_file.as_os_str(),
            journal.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let mut bindings = graph::Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::from([
            (
                producer.task.id.clone(),
                graph_binding(
                    original.join("comic-journal"),
                    root.join("producer-profile.json"),
                ),
            ),
            (
                child.task.id.clone(),
                graph_binding(journal.clone(), profile_file.clone()),
            ),
        ]),
    };
    save(
        &root,
        "phase.json",
        &json!({"phase":"dispatch-owned-derived-controls"}),
    );
    let mut controls = Vec::new();
    automatic_dispatch::run_graph(
        &case,
        &child,
        &handoff,
        &exe,
        &root.join("result.json"),
        &mut controls,
        &BTreeMap::new(),
        &plan,
        &bindings,
    );
    for (name, raw) in controls {
        fs::write(root.join(name), raw).unwrap();
    }
    let derived: ReturnedResult = decode(&fs::read(root.join("result.json")).unwrap()).unwrap();
    assert_eq!(derived.artifacts.len(), 3);
    let ack: reel_workflow_native::retention::Acknowledgement = decode(
        &fs::read(
            handoff
                .acknowledgements
                .join(format!("retained-{}-attempt-2.json", child.task_key)),
        )
        .unwrap(),
    )
    .unwrap();
    save(&root, "ack.json", &ack);
    save(
        &root,
        "phase.json",
        &json!({"phase":"close-derived-control-slot"}),
    );
    close(
        &root.join("control-close"),
        &journal,
        &profile_file,
        &handoff,
    );
    let mut review_origins = origin_map(&producer.task.id, &produced);
    review_origins
        .inputs
        .extend(origin_map(&child.task.id, &derived).inputs);
    let parents = BTreeMap::from([
        (
            producer.task.id.clone(),
            upstream::Parent {
                packet: &producer,
                result: &produced,
                source: &producer_profile.source,
                admission: &producer_profile.admission,
                receipt: &producer_profile.binding.admission,
                acknowledgement: &producer_ack,
            },
        ),
        (
            child.task.id.clone(),
            upstream::Parent {
                packet: &child,
                result: &derived,
                source: &handoff.source,
                admission: &handoff.admission,
                receipt: &handoff.binding.admission,
                acknowledgement: &ack,
            },
        ),
    ]);
    let mut review_source: reel_workflow_native::NativeProfile =
        serde_json::from_value(serde_json::to_value(&producer_profile.source).unwrap()).unwrap();
    review_source.snapshot.schema = "reel.workflow-native-composite-snapshot.v1".into();
    review_source.snapshot.upstream = Some(upstream::Frozen::capture(&review_origins, &parents));
    let mut review_inputs = child.task.inputs.clone();
    review_inputs.extend(derived.artifacts.values().cloned());
    review_inputs.push(derived.worker_receipt.clone().unwrap());
    let review=Task {id:"structured-comic-review".into(),native_inputs:Some(Fixture::declaration(&review_source.snapshot)),upstream_inputs:Some(binding(&review_origins)),operation:Operation::ReviewReport,tool:None,language:child.task.language.clone(),prompt:"Review the pinned synthetic comic reading images. Return the structured report named by the pinned request; preserve source wording and actual review modality.".into(),inputs:review_inputs,depends_on:vec![producer.task.id.clone(),child.task.id.clone()],outputs:BTreeMap::from([("report".into(),OutputContract {media_type:"application/json".into(),max_bytes:MAX_CONTROL_BYTES as u64,required:true})])};
    plan.tasks.push(review);
    let packet = make_packet(
        &plan,
        "structured-comic-review",
        BTreeMap::from([
            (producer.task.id.clone(), dep(&produced)),
            (child.task.id.clone(), dep(&derived)),
        ]),
    )
    .unwrap();
    let mut review_profile = handoff.admission.clone();
    review_profile.scope.operation = tig_store::Key::parse("structured-comic-review").unwrap();
    let review_root = root.join("review");
    fs::create_dir(&review_root).unwrap();
    save(&review_root, "packet.json", &packet);
    save(&review_root, "source.json", &review_source);
    save(&review_root, "admission.json", &review_profile);
    save(
        &root,
        "phase.json",
        &json!({"phase":"accept-protected-review-from-derived-controls"}),
    );
    let receipt = upstream::Admission::prepare(
        &packet,
        &review_source,
        &review_profile,
        "attempt-3",
        parents,
    )
    .unwrap()
    .accept()
    .unwrap();
    save(&review_root, "receipt.json", &receipt);
    let ack_dir = review_root.join("acknowledgements");
    fs::create_dir(&ack_dir).unwrap();
    let review_handoff = handoff::Profile {
        schema: "reel.workflow-native-handoff.v1".into(),
        source: review_source,
        admission: review_profile,
        binding: handoff::Binding {
            schema: "reel.workflow-native-handoff-binding.v1".into(),
            admission: receipt,
            target: retained_target(),
        },
        runtime: handoff.runtime.clone(),
        acknowledgements: ack_dir,
    };
    save(&review_root, "profile.json", &review_handoff);
    let review_journal = review_root.join("journal");
    invoke(
        &exe,
        &[
            "create-handoff".as_ref(),
            review_root.join("packet.json").as_os_str(),
            review_root.join("profile.json").as_os_str(),
            review_journal.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    bindings.tasks.insert(
        packet.task.id.clone(),
        graph_binding(review_journal.clone(), review_root.join("profile.json")),
    );
    let stage = review_root.join("inputs");
    fs::create_dir(&stage).unwrap();
    let config = dispatch::Config {
        schema: "reel.workflow-dispatch.v1".into(),
        input_staging: staging::Profile {
            schema: "reel.workflow-staging.v1".into(),
            parent: stage,
            maximum_bytes: 64 * 1024 * 1024,
            capacity_policy: reel_assets::admission::Policy {
                minimum_free_bytes: 65536,
                minimum_free_basis_points: 0,
                max_object_bytes: 512 * 1024 * 1024,
                ..Default::default()
            },
        },
        worker_staging: None,
        provider: None,
        auxiliary_programs: BTreeMap::new(),
    };
    save(&review_root, "plan.json", &plan);
    save(&review_root, "bindings.json", &bindings);
    save(&review_root, "config.json", &config);
    save(
        &root,
        "phase.json",
        &json!({"phase":"stage-protected-structured-review-handoff"}),
    );
    invoke(
        &exe,
        &[
            "dispatch-task".as_ref(),
            review_root.join("plan.json").as_os_str(),
            review_root.join("bindings.json").as_os_str(),
            packet.task.id.as_ref(),
            review_root.join("config.json").as_os_str(),
            review_root.join("dispatch").as_os_str(),
            "--apply".as_ref(),
        ],
    );
    assert_eq!(
        reel_workflow::handoff::Journal::open(&review_journal)
            .unwrap()
            .state(),
        reel_workflow::handoff::State::Waiting
    );
    let outcome = json!({"root":root,"producer":original,"parent_and_control_slots_closed":true,"derived_controls_retained_and_completed":true,"derived_controls_replay_claims_unchanged":true,"review_request_from_retained_dependency":true,"review_inputs_independently_admitted":true,"structured_review_staged_waiting":true,"source_commit_unchanged":true,"provider_called":false,"selected":false,"scratch_retired":false});
    save(&root, "qualification-result.json", &outcome);
    save(&evidence, "qualification-result.json", &outcome);
    save(&root, "phase.json", &json!({"phase":"verified"}));
}
