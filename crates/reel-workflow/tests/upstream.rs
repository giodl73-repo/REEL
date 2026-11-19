use reel_workflow::{upstream::*, *};
use std::collections::BTreeMap;
fn asset(id: &str, raw: &[u8], media: &str) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: digest(raw),
        bytes: raw.len() as u64,
        media_type: media.into(),
    }
}
fn task(id: &str) -> Task {
    Task {
        id: id.into(),
        native_inputs: None,
        upstream_inputs: None,
        operation: Operation::TextProposal,
        tool: None,
        language: Some("es".into()),
        prompt: "Synthetic exact source consumer.".into(),
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
fn packet(task: Task, dependencies: BTreeMap<String, DependencyResult>) -> Packet {
    make_packet(
        &Plan {
            schema: PLAN_SCHEMA.into(),
            project: "synthetic".into(),
            source_commit: "a".repeat(40),
            tasks: std::iter::once(task.clone())
                .chain(task.depends_on.iter().map(|id| self::task(id)))
                .collect(),
        },
        &task.id,
        dependencies,
    )
    .unwrap()
}
fn returned(packet: &Packet, id: &str) -> ReturnedResult {
    ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: "attempt-1".into(),
        artifacts: BTreeMap::from([("note".into(), asset(id, b"same bytes", "text/plain"))]),
        worker_receipt: None,
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
    }
}
fn child(
    bindings: &Bindings,
    parents: &BTreeMap<String, Parent<'_>>,
    inputs: Vec<Artifact>,
) -> Packet {
    let mut child = task("assemble");
    child.depends_on = parents.keys().cloned().collect();
    child.inputs = inputs;
    child.upstream_inputs = Some(asset(
        "workflow/upstream",
        &bindings.canonical_bytes().unwrap(),
        "application/json",
    ));
    packet(
        child,
        parents
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
            .collect(),
    )
}
#[test]
fn two_parents_preserve_logical_origin_even_when_payload_bytes_match() {
    let a = packet(task("a"), BTreeMap::new());
    let b = packet(task("b"), BTreeMap::new());
    let ar = returned(&a, "part/a");
    let br = returned(&b, "part/b");
    let parents = BTreeMap::from([
        (
            "a".into(),
            Parent {
                packet: &a,
                result: &ar,
            },
        ),
        (
            "b".into(),
            Parent {
                packet: &b,
                result: &br,
            },
        ),
    ]);
    let mut bindings = Bindings {
        schema: "reel.workflow-upstream-inputs.v1".into(),
        inputs: BTreeMap::from([
            (
                "part/a".into(),
                Reference {
                    task_id: "a".into(),
                    artifact: Selector::Output {
                        name: "note".into(),
                    },
                },
            ),
            (
                "part/b".into(),
                Reference {
                    task_id: "b".into(),
                    artifact: Selector::Output {
                        name: "note".into(),
                    },
                },
            ),
        ]),
    };
    let original = child(
        &bindings,
        &parents,
        vec![ar.artifacts["note"].clone(), br.artifacts["note"].clone()],
    );
    assert_eq!(bind(&original, &bindings, &parents).unwrap().len(), 2);
    bindings.inputs.get_mut("part/a").unwrap().task_id = "b".into();
    assert_eq!(
        bind(&original, &bindings, &parents).unwrap_err().kind,
        ErrorKind::Integrity
    );
    let changed = child(&bindings, &parents, original.task.inputs.clone());
    assert_ne!(changed.task_key, original.task_key);
    assert_eq!(
        bind(&changed, &bindings, &parents).unwrap_err().kind,
        ErrorKind::Conflict
    );
}
#[test]
fn changed_attempt_or_project_cannot_reuse_an_identical_parent_artifact() {
    let original = packet(task("a"), BTreeMap::new());
    let result = returned(&original, "part/a");
    let bindings = Bindings {
        schema: "reel.workflow-upstream-inputs.v1".into(),
        inputs: BTreeMap::from([(
            "part/a".into(),
            Reference {
                task_id: "a".into(),
                artifact: Selector::Output {
                    name: "note".into(),
                },
            },
        )]),
    };
    let parents = BTreeMap::from([(
        "a".into(),
        Parent {
            packet: &original,
            result: &result,
        },
    )]);
    let child = child(&bindings, &parents, vec![result.artifacts["note"].clone()]);
    let mut changed = result.clone();
    changed.attempt_id = "attempt-2".into();
    let parents = BTreeMap::from([(
        "a".into(),
        Parent {
            packet: &original,
            result: &changed,
        },
    )]);
    assert_eq!(
        bind(&child, &bindings, &parents).unwrap_err().kind,
        ErrorKind::Stale
    );
    let mut other = Plan {
        schema: PLAN_SCHEMA.into(),
        project: "other".into(),
        source_commit: "a".repeat(40),
        tasks: vec![task("a")],
    };
    let other_packet = make_packet(&other, "a", BTreeMap::new()).unwrap();
    let other_result = returned(&other_packet, "part/a");
    let parents = BTreeMap::from([(
        "a".into(),
        Parent {
            packet: &other_packet,
            result: &other_result,
        },
    )]);
    let mut other_child = child.clone();
    other_child.dependencies.get_mut("a").unwrap().result_sha256 =
        canonical_digest(&other_result).unwrap();
    other.tasks = vec![other_child.task.clone(), task("a")];
    other.project = "synthetic".into();
    let other_child = make_packet(&other, "assemble", other_child.dependencies).unwrap();
    assert_eq!(
        bind(&other_child, &bindings, &parents).unwrap_err().kind,
        ErrorKind::Stale
    );
}
#[test]
fn worker_receipt_requires_its_own_explicit_selector_and_input_identity() {
    let mut producer = task("a");
    let publication = asset("master", b"{}", "application/json");
    let request = asset("request", b"{}", "application/json");
    let executable = asset("tool", b"tool", "application/octet-stream");
    producer.operation = Operation::DeterministicTool;
    producer.prompt.clear();
    producer.inputs = vec![publication.clone(), request.clone()];
    producer.tool = Some(ToolContract {
        auxiliary_executables: BTreeMap::new(),
        schema: "reel.workflow-tool.v1".into(),
        capability: ToolCapability::ComicRender,
        executable: executable.clone(),
        publication,
        request,
        edition_id: "es".into(),
        source_bindings: BTreeMap::new(),
        part: None,
    });
    let producer = packet(producer, BTreeMap::new());
    let mut result = returned(&producer, "part/a");
    result.provenance = Provenance {
        mode: Mode::Tool,
        provider: None,
        model: None,
        executable: Some(executable),
    };
    let receipt = asset("part/a/receipt", b"{}", "application/json");
    result.worker_receipt = Some(receipt.clone());
    let parents = BTreeMap::from([(
        "a".into(),
        Parent {
            packet: &producer,
            result: &result,
        },
    )]);
    let bindings = Bindings {
        schema: "reel.workflow-upstream-inputs.v1".into(),
        inputs: BTreeMap::from([(
            "part/a/receipt".into(),
            Reference {
                task_id: "a".into(),
                artifact: Selector::WorkerReceipt,
            },
        )]),
    };
    let child = child(&bindings, &parents, vec![receipt.clone()]);
    assert_eq!(
        bind(&child, &bindings, &parents).unwrap()["part/a/receipt"].artifact,
        receipt
    );
}
