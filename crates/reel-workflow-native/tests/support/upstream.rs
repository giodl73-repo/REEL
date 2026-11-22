//! Shared retained-child admission tests; payload/producer qualification is explicit.
use super::*;
use reel_workflow::upstream as contract;
use reel_workflow_native::{NativeProfile, admission, upstream};

fn clone_source(source: &NativeProfile) -> NativeProfile {
    serde_json::from_slice(&serde_json::to_vec(source).unwrap()).unwrap()
}
fn child(
    packet_id: &str,
    source: &NativeProfile,
    parents: BTreeMap<String, upstream::Parent<'_>>,
) -> (Packet, NativeProfile) {
    let inputs: Vec<_> = parents
        .values()
        .flat_map(|p| p.result.artifacts.values().cloned())
        .collect();
    let bindings = contract::Bindings {
        schema: "reel.workflow-upstream-inputs.v2".into(),
        streaming: None,
        inputs: parents
            .iter()
            .flat_map(|(id, p)| {
                p.result.artifacts.iter().map(move |(name, a)| {
                    (
                        a.logical_id.clone(),
                        contract::Reference {
                            task_id: id.clone(),
                            artifact: contract::Selector::Output { name: name.clone() },
                        },
                    )
                })
            })
            .collect(),
    };
    let mut child_source = clone_source(source);
    child_source.snapshot.schema = "reel.workflow-native-snapshot.v3".into();
    child_source.snapshot.budget = Some(reel_workflow_native::SnapshotBudget {
        schema: "reel.workflow-native-input-budget.v1".into(),
        input_bytes: 64 * 1024 * 1024,
        input_object_bytes: 64 * 1024 * 1024,
    });
    child_source.snapshot.records.clear();
    child_source.snapshot.definitions.clear();
    child_source.snapshot.bindings.clear();
    child_source.snapshot.input_streams.clear();
    child_source.snapshot.upstream = Some(upstream::Frozen::capture(&bindings, &parents));
    let raw = bindings.canonical_bytes().unwrap();
    let task = Task {
        id: packet_id.into(),
        native_inputs: Some(Fixture::declaration(&child_source.snapshot)),
        upstream_inputs: Some(Artifact {
            logical_id: "control/retained-origins".into(),
            sha256: digest(&raw),
            bytes: raw.len() as u64,
            media_type: "application/json".into(),
        }),
        operation: Operation::TextProposal,
        tool: None,
        language: Some("es".into()),
        prompt: "Synthetic bounded retained-input consumer.".into(),
        inputs,
        depends_on: parents.keys().cloned().collect(),
        outputs: BTreeMap::from([(
            "control".into(),
            OutputContract {
                media_type: "application/json".into(),
                max_bytes: 4096,
                required: true,
            },
        )]),
    };
    let plan = Plan {
        schema: SCOPED_PLAN_SCHEMA.into(),
        project: parents.values().next().unwrap().packet.project.clone(),
        source_commit: child_source.snapshot.commit.clone(),
        task_snapshots: parents
            .iter()
            .map(|(id, p)| (id.clone(), p.packet.source_commit.clone()))
            .chain(std::iter::once((
                packet_id.into(),
                child_source.snapshot.commit.clone(),
            )))
            .collect(),
        tasks: parents
            .values()
            .map(|p| p.packet.task.clone())
            .chain(std::iter::once(task))
            .collect(),
    };
    let dependencies = parents
        .iter()
        .map(|(id, p)| {
            (
                id.clone(),
                DependencyResult {
                    result_sha256: canonical_digest(p.result).unwrap(),
                    artifacts: p.result.artifacts.clone(),
                },
            )
        })
        .collect();
    (
        make_packet(&plan, packet_id, dependencies).unwrap(),
        child_source,
    )
}
fn parent<'a>(
    fixture: &'a Fixture,
    case: &'a AdmissionCase,
    receipt: &'a admission::Receipt,
    result: &'a ReturnedResult,
    ack: &'a reel_workflow_native::retention::Acknowledgement,
) -> upstream::Parent<'a> {
    upstream::Parent {
        packet: &fixture.packet,
        result,
        source: &case.source,
        admission: &case.profile,
        receipt,
        acknowledgement: ack,
    }
}
#[test]
fn frozen_parent_metadata_cannot_bypass_live_child_admission_and_preserves_legacy_bindings() {
    use reel_workflow_native::{admission::Prepared, response, retention};
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    assert!(
        !String::from_utf8(fixture.profile.declaration_bytes().unwrap())
            .unwrap()
            .contains("upstream")
    );
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let (result, returns) = response_fixture(&fixture.packet, &case.temp.path().join("returns"));
    response::register(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &returns,
    )
    .unwrap();
    let ack = retention::stage(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &retained_target(),
    )
    .unwrap();
    let (packet, source) = child(
        "child",
        &case.source,
        BTreeMap::from([(
            "review".into(),
            parent(&fixture, &case, &receipt, &result, &ack),
        )]),
    );
    let metadata = fs::read(case.profile.metadata_root.join("metadata.json")).unwrap();
    let snapshot = SnapshotInputs::open(&packet, &source.snapshot).unwrap();
    assert_eq!(snapshot.input_count(), 1);
    let prepared = Prepared::open(&packet, &source, &case.profile, "child-attempt").unwrap();
    assert_eq!(prepared.accept().unwrap_err().kind, ErrorKind::Denied);
    assert!(
        upstream::Admission::prepare(
            &packet,
            &source,
            &case.profile,
            "child-attempt",
            BTreeMap::from([(
                "review".into(),
                parent(&fixture, &case, &receipt, &result, &ack)
            )])
        )
        .is_err()
    );
    assert_eq!(
        metadata,
        fs::read(case.profile.metadata_root.join("metadata.json")).unwrap()
    );
}
#[test]
fn native_origin_changes_admission_archive_without_retargeting_logical_parent_identity() {
    use reel_workflow_native::{admission::Prepared, response, retention};
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let (result, returns) = response_fixture(&fixture.packet, &case.temp.path().join("returns"));
    response::register(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &returns,
    )
    .unwrap();
    let ack = retention::stage(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &retained_target(),
    )
    .unwrap();
    let (packet, mut source) = child(
        "child",
        &case.source,
        BTreeMap::from([(
            "review".into(),
            parent(&fixture, &case, &receipt, &result, &ack),
        )]),
    );
    let declaration = source.snapshot.declaration_bytes().unwrap();
    let checkpoint = SnapshotInputs::open(&packet, &source.snapshot)
        .unwrap()
        .checkpoint()
        .to_owned();
    source
        .snapshot
        .upstream
        .as_mut()
        .unwrap()
        .parents
        .get_mut("review")
        .unwrap()
        .origin
        .owner = tig_store::Key::parse("wrong-owner").unwrap();
    assert_eq!(declaration, source.snapshot.declaration_bytes().unwrap());
    assert_ne!(
        checkpoint,
        SnapshotInputs::open(&packet, &source.snapshot)
            .unwrap()
            .checkpoint()
    );
    let error = upstream::Admission::prepare(
        &packet,
        &source,
        &case.profile,
        "child-attempt",
        BTreeMap::from([(
            "review".into(),
            parent(&fixture, &case, &receipt, &result, &ack),
        )]),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "upstream-live-origin-mismatch");
}

#[test]
#[ignore = "Explicit completed original synthetic parent only; admits a NEW bounded child generation in its existing index, retains exact outputs and closes the child; no cache initialization, project selection or parent retirement"]
fn retained_parent_admits_own_child_claims_and_replays_after_child_slot_close() {
    qualify_child(false);
}
#[test]
#[ignore = "Explicit two completed synthetic parents only; verifies shared retained-owner fence, admits NEW child generation and closes it; no parent release or project adoption"]
fn two_retained_parents_share_fence_and_transfer_exact_input_claims_to_child() {
    qualify_child(true);
}
#[cfg(feature = "film")]
#[test]
#[ignore = "Read-only complete original synthetic film/image parent proofs in their existing index; no new child claims, publication, task, slot or profile adoption"]
fn film_tool_parent_reopens_exact_worker_proof_with_shared_retained_owner() {
    use reel_workflow_native::handoff;
    let film_root = PathBuf::from(
        std::env::var_os("REEL_TOOL_PARENT").expect("Set completed synthetic film child"),
    );
    let image_root = PathBuf::from(
        std::env::var_os("REEL_CHILD_PARENT").expect("Set completed synthetic image parent"),
    );
    let location: serde_json::Value =
        serde_json::from_slice(&fs::read(film_root.join("locations.json")).unwrap()).unwrap();
    let complete: serde_json::Value =
        serde_json::from_slice(&fs::read(film_root.join("complete.json")).unwrap()).unwrap();
    assert_eq!(complete["complete"], true);
    let packet: Packet = serde_json::from_value(location["packet"].clone()).unwrap();
    let source: NativeProfile = serde_json::from_value(location["native"].clone()).unwrap();
    let profile: admission::Profile =
        serde_json::from_value(location["admission_profile"].clone()).unwrap();
    let receipt =
        serde_json::from_slice(&fs::read(film_root.join("admission.json")).unwrap()).unwrap();
    let ack: reel_workflow_native::retention::Acknowledgement =
        serde_json::from_value(complete["retained"].clone()).unwrap();
    let film = upstream::ParentControl {
        packet: packet.clone(),
        result: serde_json::from_value(complete["result"].clone()).unwrap(),
        acknowledgement: ack.clone(),
        profile: handoff::Profile {
            schema: "reel.workflow-native-handoff.v1".into(),
            source: clone_source(&source),
            admission: profile.clone(),
            binding: handoff::Binding {
                schema: "reel.workflow-native-handoff-binding.v1".into(),
                admission: receipt,
                target: ack.target,
            },
            runtime: serde_json::from_value(location["runtime"].clone()).unwrap(),
            acknowledgements: film_root.join("unused-read-only-acknowledgements"),
        },
    };
    let image: reel_workflow_native::image_selection::Request =
        serde_json::from_slice(&fs::read(image_root.join("selection-request.json")).unwrap())
            .unwrap();
    let image_control = upstream::ParentControl {
        packet: image.packet.clone(),
        result: image.result,
        acknowledgement: image.acknowledgement,
        profile: serde_json::from_slice(
            &fs::read(image_root.join("profile-control.json")).unwrap(),
        )
        .unwrap(),
    };
    let parents = upstream::Parents {
        schema: "reel.workflow-native-parents.v1".into(),
        parents: BTreeMap::from([
            (film.packet.task.id.clone(), film),
            (image.packet.task.id.clone(), image_control),
        ]),
    };
    let (child, child_source) = child("tool-parent-review", &source, parents.borrowed().unwrap());
    let mut child_profile = profile.clone();
    child_profile.scope.operation = tig_store::Key::parse("preview-film-parent-v1").unwrap();
    let before = tig_store::source_claims::protection::Index::open(
        &profile.index_root,
        &profile.index_minimum,
    )
    .unwrap()
    .checkpoint();
    let proof = upstream::Admission::prepare(
        &child,
        &child_source,
        &child_profile,
        "read-only-parent-check",
        parents.borrowed().unwrap(),
    )
    .unwrap();
    assert!(proof.preview().unwrap().response_budget_ok);
    drop(proof);
    assert_eq!(
        before,
        tig_store::source_claims::protection::Index::open(
            &profile.index_root,
            &profile.index_minimum
        )
        .unwrap()
        .checkpoint()
    );
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(film_root.join("tool-parent-readback.json"))
        .unwrap();
    file.write_all(&serde_json::to_vec(&json!({"film_task_key":packet.task_key,"exact_film_worker_verified":true,"shared_retained_owner_verified":true,"index_unchanged":true,"child_claims_admitted":false})).unwrap()).unwrap();
    file.sync_all().unwrap();
}
fn qualify_child(two_parents: bool) {
    use reel_workflow_native::{closing, handoff, retention};
    let parent_root = PathBuf::from(
        std::env::var_os("REEL_CHILD_PARENT").expect("Set completed synthetic image qualification"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_CHILD_EVIDENCE").expect("Set NEW child evidence root"),
    );
    let parent_request: reel_workflow_native::image_selection::Request =
        serde_json::from_slice(&fs::read(parent_root.join("selection-request.json")).unwrap())
            .unwrap();
    let parent_complete: serde_json::Value =
        serde_json::from_slice(&fs::read(parent_root.join("complete.json")).unwrap()).unwrap();
    assert_eq!(parent_complete["slot_idle"], true);
    let parent_profile: handoff::Profile =
        serde_json::from_slice(&fs::read(parent_root.join("profile-control.json")).unwrap())
            .unwrap();
    let mut controls = upstream::Parents {
        schema: "reel.workflow-native-parents.v1".into(),
        parents: BTreeMap::from([(
            parent_request.packet.task.id.clone(),
            upstream::ParentControl {
                packet: parent_request.packet.clone(),
                result: parent_request.result.clone(),
                profile: parent_profile,
                acknowledgement: parent_request.acknowledgement.clone(),
            },
        )]),
    };
    if two_parents {
        let extra = PathBuf::from(
            std::env::var_os("REEL_CHILD_EXTRA_PARENT")
                .expect("Set completed original derived-control parent"),
        );
        let complete: serde_json::Value =
            serde_json::from_slice(&fs::read(extra.join("complete.json")).unwrap()).unwrap();
        assert_eq!(complete["child_slot_closed"], true);
        let packet: Packet =
            serde_json::from_slice(&fs::read(extra.join("packet.json")).unwrap()).unwrap();
        let result: ReturnedResult =
            serde_json::from_slice(&fs::read(extra.join("result.json")).unwrap()).unwrap();
        let profile: handoff::Profile =
            serde_json::from_slice(&fs::read(extra.join("handoff.json")).unwrap()).unwrap();
        let ack_path = profile.acknowledgements.join(format!(
            "retained-{}-{}.json",
            profile.binding.admission.task_key, profile.binding.admission.attempt_id
        ));
        let acknowledgement = serde_json::from_slice(&fs::read(ack_path).unwrap()).unwrap();
        controls.parents.insert(
            packet.task.id.clone(),
            upstream::ParentControl {
                packet,
                result,
                profile,
                acknowledgement,
            },
        );
    }
    let packet_id = if two_parents {
        "review-controls"
    } else {
        "derived-controls"
    };
    let (packet, source) = child(
        packet_id,
        &parent_request.source,
        controls.borrowed().unwrap(),
    );
    let mut profile = parent_request.admission.clone();
    profile.scope.operation = tig_store::Key::parse(if two_parents {
        "review-controls-v1"
    } else {
        "derive-controls-v1"
    })
    .unwrap();
    let index_before = tig_store::source_claims::protection::Index::open(
        &profile.index_root,
        &profile.index_minimum,
    )
    .unwrap()
    .checkpoint();
    assert_eq!(
        admission::Prepared::open(&packet, &source, &profile, "child-attempt-1")
            .unwrap()
            .accept()
            .unwrap_err()
            .kind,
        ErrorKind::Denied
    );
    let mut bad: upstream::Parents =
        serde_json::from_slice(&serde_json::to_vec(&controls).unwrap()).unwrap();
    let bad_parent = bad.parents.values_mut().next().unwrap();
    bad_parent.acknowledgement.target.owner = tig_store::Key::parse("wrong-owner").unwrap();
    bad_parent.profile.binding.target = bad_parent.acknowledgement.target.clone();
    let mut wrong_source = clone_source(&source);
    wrong_source.snapshot.upstream = Some(upstream::Frozen::capture(
        &source.snapshot.upstream.as_ref().unwrap().bindings,
        &bad.borrowed().unwrap(),
    ));
    assert!(
        upstream::Admission::prepare(
            &packet,
            &wrong_source,
            &profile,
            "child-attempt-1",
            bad.borrowed().unwrap()
        )
        .is_err()
    );
    assert_eq!(
        index_before,
        tig_store::source_claims::protection::Index::open(
            &profile.index_root,
            &profile.index_minimum
        )
        .unwrap()
        .checkpoint()
    );
    assert!(evidence.is_absolute());
    fs::create_dir(&evidence).unwrap();
    let saved = |name: &str, value: &serde_json::Value| {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(evidence.join(name))
            .unwrap();
        file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
        file.sync_all().unwrap();
    };
    let runtime = evidence.join("reel-workflow-native.exe");
    fs::copy(env!("CARGO_BIN_EXE_reel-workflow-native"), &runtime).unwrap();
    for (name, value) in [
        ("packet.json", serde_json::to_value(&packet).unwrap()),
        ("source.json", serde_json::to_value(&source).unwrap()),
        (
            "admission-profile.json",
            serde_json::to_value(&profile).unwrap(),
        ),
        ("parents.json", serde_json::to_value(&controls).unwrap()),
    ] {
        saved(name, &value);
    }
    let call = |args: Vec<std::ffi::OsString>| {
        let output = Command::new(&runtime).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let admission_args = |command: &str| {
        vec![
            command.into(),
            evidence.join("packet.json").into(),
            evidence.join("source.json").into(),
            evidence.join("admission-profile.json").into(),
            evidence.join("parents.json").into(),
            "child-attempt-1".into(),
        ]
    };
    let preview = call(admission_args("preview-upstream-admission"));
    assert_eq!(preview["child_claims_accepted"], false);
    let mut args = admission_args("accept-upstream-admission");
    args.extend([evidence.join("admission.json").into(), "--apply".into()]);
    call(args);
    let receipt: admission::Receipt =
        serde_json::from_slice(&fs::read(evidence.join("admission.json")).unwrap()).unwrap();
    assert_ne!(receipt.activation, parent_request.receipt.activation);
    assert_ne!(receipt.work, parent_request.receipt.work);
    let claims = admission::Prepared::open(&packet, &source, &profile, "child-attempt-1")
        .unwrap()
        .reopen(&receipt)
        .unwrap();
    let mut inputs =
        reel_workflow_native::NativeInputs::from_claims(&packet, &source, claims).unwrap();
    let selected = packet
        .task
        .inputs
        .iter()
        .find(|a| a.media_type == "image/png")
        .unwrap();
    let image =
        reel_workflow_native::InputReader::read(&mut inputs, selected, 1024 * 1024).unwrap();
    assert!(image.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(digest(&image), selected.sha256);
    if two_parents {
        let control = packet
            .task
            .inputs
            .iter()
            .find(|a| a.media_type == "application/json")
            .unwrap();
        let raw = reel_workflow_native::InputReader::read(&mut inputs, control, 4096).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(value["picture"], serde_json::to_value(selected).unwrap());
        assert_eq!(value["cel_id"], "C101");
    }
    drop(inputs);
    assert!(
        admission::Prepared::open(
            &parent_request.packet,
            &parent_request.source,
            &parent_request.admission,
            &parent_request.receipt.attempt_id
        )
        .unwrap()
        .reopen(&parent_request.receipt)
        .is_err()
    );
    {
        let mut index = tig_store::source_claims::protection::Index::open(
            &profile.index_root,
            &profile.index_minimum,
        )
        .unwrap();
        let mut nodes = tig_store::metadata::MetadataStore::open(
            &profile.metadata_root,
            &profile.metadata_minimum,
        )
        .unwrap();
        let object = Object::from_bytes(&image);
        let state = index
            .inspect(&mut nodes, &object, &profile.scope.store)
            .unwrap();
        assert!(state.protected_by.contains(&profile.scope.actor));
        assert!(
            state
                .protected_by
                .contains(&parent_request.acknowledgement.target.actor)
        );
    }
    let returns_root = evidence.join("returns");
    fs::create_dir(&returns_root).unwrap();
    let control = serde_json::to_vec(
        &json!({"schema":"synthetic.derived-control.v1","picture":selected,"cel_id":"C101"}),
    )
    .unwrap();
    fs::write(returns_root.join("control.json"), &control).unwrap();
    let result = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: receipt.attempt_id.clone(),
        worker_receipt: None,
        artifacts: BTreeMap::from([(
            "control".into(),
            Artifact {
                logical_id: if two_parents {
                    "review/control"
                } else {
                    "derived/control"
                }
                .into(),
                sha256: digest(&control),
                bytes: control.len() as u64,
                media_type: "application/json".into(),
            },
        )]),
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
    };
    let acknowledgements = evidence.join("acknowledgements");
    fs::create_dir(&acknowledgements).unwrap();
    let raw = fs::read(&runtime).unwrap();
    let handoff = handoff::Profile {
        schema: "reel.workflow-native-handoff.v1".into(),
        source: clone_source(&source),
        admission: profile.clone(),
        binding: handoff::Binding {
            schema: "reel.workflow-native-handoff-binding.v1".into(),
            admission: receipt.clone(),
            target: parent_request.acknowledgement.target.clone(),
        },
        runtime: Tool {
            path: runtime.clone(),
            sha256: digest(&raw),
            bytes: raw.len() as u64,
        },
        acknowledgements,
    };
    let returns = reel_workflow_native::response::Returns {
        schema: "reel.workflow-returns.v1".into(),
        root: returns_root,
        files: BTreeMap::from([("control".into(), "control.json".into())]),
    };
    saved("handoff.json", &serde_json::to_value(&handoff).unwrap());
    saved("result.json", &serde_json::to_value(&result).unwrap());
    saved("returns.json", &serde_json::to_value(&returns).unwrap());
    let journal = evidence.join("journal");
    call(vec![
        "create-handoff".into(),
        evidence.join("packet.json").into(),
        evidence.join("handoff.json").into(),
        journal.clone().into(),
        "--apply".into(),
    ]);
    call(vec![
        "import-handoff".into(),
        journal.clone().into(),
        evidence.join("handoff.json").into(),
        evidence.join("result.json").into(),
        evidence.join("returns.json").into(),
        "--apply".into(),
    ]);
    let ack_path = fs::read_dir(&handoff.acknowledgements)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .unwrap();
    let ack: retention::Acknowledgement =
        serde_json::from_slice(&fs::read(ack_path).unwrap()).unwrap();
    drop(retention::verify(&packet, &source, &profile, &receipt, &result, &ack).unwrap());
    let original_lifecycle_tool = reel_workflow::handoff::Journal::open(&journal)
        .unwrap()
        .ticket()
        .lifecycle_tool
        .clone();
    let close = closing::Request {
        schema: "reel.workflow-native-close.v1".into(),
        journal,
        packet: packet.clone(),
        source: clone_source(&source),
        admission: profile.clone(),
        receipt,
        result: result.clone(),
        acknowledgement: ack.clone(),
        operation: tig_store::Key::parse(if two_parents {
            "close-review-controls-v1"
        } else {
            "close-derived-controls-v1"
        })
        .unwrap(),
        original_lifecycle_tool,
        runtime: handoff.runtime.clone(),
    };
    saved("close-request.json", &serde_json::to_value(&close).unwrap());
    let pin = digest(&fs::read(evidence.join("close-request.json")).unwrap());
    call(vec![
        "close-retained".into(),
        evidence.join("close-request.json").into(),
        pin.into(),
        evidence.join("closed.json").into(),
        "--apply".into(),
    ]);
    let closed: closing::Outcome =
        serde_json::from_slice(&fs::read(evidence.join("closed.json")).unwrap()).unwrap();
    assert!(closed.slot_idle && closed.original_clear_complete);
    drop(
        retention::verify(
            &parent_request.packet,
            &parent_request.source,
            &parent_request.admission,
            &parent_request.receipt,
            &parent_request.result,
            &parent_request.acknowledgement,
        )
        .unwrap(),
    );
    drop(retention::verify(&packet, &source, &profile, &close.receipt, &result, &ack).unwrap());
    saved(
        "complete.json",
        &json!({"native_parent_count":controls.parents.len(),"slot_reused_as_new_child_generation":true,"child_input_claims_verified":true,"exact_parent_png_read":true,"exact_parent_control_read":two_parents,"parent_history_preserved":true,"child_retained":true,"child_slot_closed":true,"producer_executed":false,"production_adopted":false}),
    );
}
