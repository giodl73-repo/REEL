use reel_workflow::*;
use std::collections::BTreeMap;

fn task(id: &str) -> Task {
    Task {
        upstream_inputs: None,
        native_inputs: None,
        id: id.into(),
        operation: Operation::TextProposal,
        tool: None,
        language: Some("es".into()),
        prompt: "Return one synthetic editorial note.".into(),
        inputs: vec![],
        depends_on: vec![],
        outputs: BTreeMap::from([(
            "note".into(),
            OutputContract {
                media_type: "text/plain".into(),
                max_bytes: 1024,
                required: true,
            },
        )]),
    }
}
fn plan() -> Plan {
    Plan {
        schema: PLAN_SCHEMA.into(),
        project: "synthetic-film".into(),
        source_commit: "a".repeat(40),
        tasks: vec![task("note")],
    }
}
fn packet() -> Packet {
    make_packet(&plan(), "note", BTreeMap::new()).unwrap()
}
fn returned(packet: &Packet, mode: Mode) -> ReturnedResult {
    ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        worker_receipt: None,
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: "attempt-1".into(),
        artifacts: BTreeMap::from([(
            "note".into(),
            Artifact {
                logical_id: "candidate/note".into(),
                sha256: digest(b"hello"),
                bytes: 5,
                media_type: "text/plain".into(),
            },
        )]),
        provenance: Provenance {
            mode,
            provider: None,
            model: None,
            executable: None,
        },
    }
}

#[test]
fn plans_are_project_neutral_and_language_specific() {
    let first = packet();
    let mut other = plan();
    other.project = "synthetic-game".into();
    assert_ne!(
        first.task_key,
        make_packet(&other, "note", BTreeMap::new())
            .unwrap()
            .task_key
    );
    other = plan();
    other.tasks[0].language = Some("en".into());
    assert_ne!(
        first.task_key,
        make_packet(&other, "note", BTreeMap::new())
            .unwrap()
            .task_key
    );
}

#[test]
fn additive_tool_contract_preserves_existing_v2_intelligence_fingerprint() {
    let original = packet();
    assert_eq!(
        original.task_key,
        "dd09506181be2b4ae2a5b98a9d916affc785788fc1e82795b701cd16d13c8524"
    );
    assert!(
        serde_json::to_value(&original.task)
            .unwrap()
            .get("tool")
            .is_none()
    );
}

#[test]
fn native_input_declarations_are_task_bound_and_require_bounded_json_pins() {
    let mut graph = plan();
    let original = packet().task_key;
    graph.tasks[0].native_inputs = Some(Artifact {
        logical_id: "control/native-bindings".into(),
        sha256: digest(b"{}"),
        bytes: 2,
        media_type: "application/json".into(),
    });
    let declared = make_packet(&graph, "note", BTreeMap::new()).unwrap();
    assert_ne!(declared.task_key, original);
    graph.tasks[0].native_inputs.as_mut().unwrap().sha256 = digest(b"changed");
    assert_ne!(
        make_packet(&graph, "note", BTreeMap::new())
            .unwrap()
            .task_key,
        declared.task_key
    );
    graph.tasks[0].native_inputs.as_mut().unwrap().media_type = "text/plain".into();
    assert!(check_plan(&graph).is_err());
    graph.tasks[0].native_inputs.as_mut().unwrap().media_type = "application/json".into();
    graph.tasks[0].native_inputs.as_mut().unwrap().bytes = MAX_CONTROL_BYTES as u64 + 1;
    assert!(check_plan(&graph).is_err());
}

#[test]
fn svg_and_structured_json_are_valid_output_types() {
    for media_type in ["image/svg+xml", "application/vnd.reel.source+json"] {
        let mut graph = plan();
        graph.tasks[0].outputs.get_mut("note").unwrap().media_type = media_type.into();
        let packet = make_packet(&graph, "note", BTreeMap::new()).unwrap();
        let mut result = returned(&packet, Mode::Handoff);
        result.artifacts.get_mut("note").unwrap().media_type = media_type.into();
        check_output(&packet, &result, "note", b"hello").unwrap();
    }
}

#[test]
fn multi_output_result_must_cover_all_required_outputs() {
    let mut graph = plan();
    graph.tasks[0].outputs.insert(
        "lettering".into(),
        OutputContract {
            media_type: "application/json".into(),
            max_bytes: 100,
            required: true,
        },
    );
    let packet = make_packet(&graph, "note", BTreeMap::new()).unwrap();
    let mut result = returned(&packet, Mode::Handoff);
    assert_eq!(
        check_result(&packet, &result).unwrap_err().kind,
        ErrorKind::Missing
    );
    result.artifacts.insert(
        "lettering".into(),
        Artifact {
            logical_id: "candidate/lettering".into(),
            sha256: digest(b"{}"),
            bytes: 2,
            media_type: "application/json".into(),
        },
    );
    check_output(&packet, &result, "note", b"hello").unwrap();
    check_output(&packet, &result, "lettering", b"{}").unwrap();
    let mut revised = graph.clone();
    revised.tasks[0]
        .outputs
        .get_mut("lettering")
        .unwrap()
        .max_bytes = 99;
    assert_ne!(
        packet.task_key,
        make_packet(&revised, "note", BTreeMap::new())
            .unwrap()
            .task_key
    );
}

#[test]
fn single_output_v1_contracts_are_not_silently_reinterpreted() {
    let mut old = plan();
    old.schema = "reel.workflow-plan.v1".into();
    assert!(check_plan(&old).is_err());
}

#[test]
fn direct_and_handoff_share_task_and_validation_without_automatic_retry() {
    struct Fake {
        calls: usize,
        fail: bool,
    }
    impl DirectAdapter for Fake {
        fn execute(&mut self, packet: &Packet, _: &str) -> Result<ReturnedResult> {
            self.calls += 1;
            if self.fail {
                return Err(Error {
                    kind: ErrorKind::NeedsRecovery,
                    code: "lost-response".into(),
                });
            }
            Ok(returned(packet, Mode::Direct))
        }
    }
    let packet = packet();
    let mut adapter = Fake {
        calls: 0,
        fail: false,
    };
    let direct = call_direct(&mut adapter, &packet, "attempt-1").unwrap();
    let manual = returned(&packet, Mode::Handoff);
    assert_eq!(direct.task_key, manual.task_key);
    check_output(&packet, &direct, "note", b"hello").unwrap();
    check_output(&packet, &manual, "note", b"hello").unwrap();
    adapter.fail = true;
    assert_eq!(
        call_direct(&mut adapter, &packet, "attempt-1")
            .unwrap_err()
            .kind,
        ErrorKind::NeedsRecovery
    );
    assert_eq!(adapter.calls, 2);
}

#[test]
fn changes_invalidate_returns_but_unrelated_tasks_do_not_change_packet() {
    let first = packet();
    let result = returned(&first, Mode::Handoff);
    let mut revised = plan();
    revised.tasks[0].prompt.push_str(" Revised.");
    let next = make_packet(&revised, "note", BTreeMap::new()).unwrap();
    assert_eq!(
        check_result(&next, &result).unwrap_err().kind,
        ErrorKind::Stale
    );
    revised = plan();
    revised.tasks.push(task("unrelated"));
    assert_eq!(
        first,
        make_packet(&revised, "note", BTreeMap::new()).unwrap()
    );
}

#[test]
fn exact_attempt_replays_conflict_on_changed_content_or_provenance() {
    let first = returned(&packet(), Mode::Handoff);
    check_replay(&first, &first).unwrap();
    let mut changed = first.clone();
    changed.artifacts.get_mut("note").unwrap().sha256 = digest(b"other");
    assert_eq!(
        check_replay(&first, &changed).unwrap_err().kind,
        ErrorKind::Conflict
    );
    changed = first.clone();
    changed.provenance.model = Some("different-model".into());
    assert_eq!(
        check_replay(&first, &changed).unwrap_err().kind,
        ErrorKind::Conflict
    );
}

#[test]
fn graph_refuses_cycles_missing_and_duplicate_dependencies() {
    let mut graph = plan();
    graph.tasks[0].depends_on = vec!["missing".into()];
    assert_eq!(check_plan(&graph).unwrap_err().kind, ErrorKind::Missing);
    graph.tasks.push(task("missing"));
    graph.tasks[1].depends_on = vec!["note".into()];
    assert_eq!(check_plan(&graph).unwrap_err().code, "dependency-cycle");
    graph.tasks[1].depends_on.clear();
    assert_eq!(check_plan(&graph).unwrap(), vec!["missing", "note"]);
    graph.tasks[0].depends_on.push("missing".into());
    assert!(check_plan(&graph).is_err());
}

#[test]
fn dependency_result_and_artifact_are_both_bound() {
    let mut graph = plan();
    graph.tasks.push(task("followup"));
    graph.tasks[1].depends_on.push("note".into());
    assert_eq!(
        make_packet(&graph, "followup", BTreeMap::new())
            .unwrap_err()
            .kind,
        ErrorKind::Missing
    );
    let prior = returned(&packet(), Mode::Handoff);
    let mut descriptors = BTreeMap::from([(
        "note".into(),
        DependencyResult {
            result_sha256: canonical_digest(&prior).unwrap(),
            artifacts: prior.artifacts,
        },
    )]);
    let first = make_packet(&graph, "followup", descriptors.clone()).unwrap();
    descriptors.get_mut("note").unwrap().result_sha256 = "b".repeat(64);
    assert_ne!(
        first.task_key,
        make_packet(&graph, "followup", descriptors)
            .unwrap()
            .task_key
    );
}

#[test]
fn corrupt_bytes_wrong_type_oversize_and_tampered_packets_refuse() {
    let mut packet = packet();
    let mut result = returned(&packet, Mode::Handoff);
    assert_eq!(
        check_output(&packet, &result, "note", b"hallo")
            .unwrap_err()
            .kind,
        ErrorKind::Integrity
    );
    result.artifacts.get_mut("note").unwrap().media_type = "image/png".into();
    assert!(check_result(&packet, &result).is_err());
    result = returned(&packet, Mode::Handoff);
    result.artifacts.get_mut("note").unwrap().bytes = 1025;
    assert_eq!(
        check_result(&packet, &result).unwrap_err().kind,
        ErrorKind::Budget
    );
    packet.task.prompt.push_str(" tampered");
    assert_eq!(
        check_packet(&packet).unwrap_err().kind,
        ErrorKind::Integrity
    );
}

#[test]
fn unknown_fields_and_metadata_bounds_refuse_without_echoing_private_text() {
    let mut value = serde_json::to_value(plan()).unwrap();
    value["secret"] = serde_json::json!("sensitive-content");
    let error = decode::<Plan>(&serde_json::to_vec(&value).unwrap()).unwrap_err();
    assert!(!error.to_string().contains("sensitive-content"));
    assert_eq!(
        decode::<Plan>(&vec![b' '; MAX_CONTROL_BYTES + 1])
            .unwrap_err()
            .kind,
        ErrorKind::Budget
    );
}

#[test]
fn cli_checks_bytes_without_claiming_completion_and_never_overwrites_packets() {
    let root = tempfile::tempdir().unwrap();
    let plan_path = root.path().join("plan.json");
    let packet_path = root.path().join("packet.json");
    let result_path = root.path().join("result.json");
    let payload_path = root.path().join("output.txt");
    let bindings_path = root.path().join("payload-bindings.json");
    std::fs::write(
        &bindings_path,
        serde_json::to_vec(&BTreeMap::from([("note", payload_path.to_str().unwrap())])).unwrap(),
    )
    .unwrap();
    std::fs::write(&plan_path, serde_json::to_vec(&plan()).unwrap()).unwrap();
    let exported = std::process::Command::new(env!("CARGO_BIN_EXE_reel-workflow"))
        .arg("export-task")
        .arg(&plan_path)
        .arg("note")
        .arg(&packet_path)
        .output()
        .unwrap();
    assert!(exported.status.success());
    let original = std::fs::read(&packet_path).unwrap();
    let packet: Packet = decode(&original).unwrap();
    let duplicate = std::process::Command::new(env!("CARGO_BIN_EXE_reel-workflow"))
        .arg("export-task")
        .arg(&plan_path)
        .arg("note")
        .arg(&packet_path)
        .output()
        .unwrap();
    assert!(!duplicate.status.success());
    assert_eq!(std::fs::read(&packet_path).unwrap(), original);
    std::fs::write(
        &result_path,
        serde_json::to_vec(&returned(&packet, Mode::Handoff)).unwrap(),
    )
    .unwrap();
    std::fs::write(&payload_path, b"hello").unwrap();
    let checked = std::process::Command::new(env!("CARGO_BIN_EXE_reel-workflow"))
        .arg("check-result")
        .arg(&packet_path)
        .arg(&result_path)
        .arg(&bindings_path)
        .output()
        .unwrap();
    assert!(checked.status.success());
    let report: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(report["task_completed"], false);
    assert_eq!(report["retention_acknowledged"], false);
    assert!(
        !String::from_utf8(checked.stdout)
            .unwrap()
            .contains("synthetic-film")
    );
    std::fs::write(&payload_path, b"tampered").unwrap();
    assert!(
        !std::process::Command::new(env!("CARGO_BIN_EXE_reel-workflow"))
            .arg("check-result")
            .arg(&packet_path)
            .arg(&result_path)
            .arg(&bindings_path)
            .status()
            .unwrap()
            .success()
    );
}
