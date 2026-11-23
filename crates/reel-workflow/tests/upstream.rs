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
    packet_at(task, dependencies, "a".repeat(40))
}
fn packet_at(
    task: Task,
    dependencies: BTreeMap<String, DependencyResult>,
    commit: String,
) -> Packet {
    make_packet(
        &Plan {
            schema: PLAN_SCHEMA.into(),
            task_snapshots: Default::default(),
            project: "synthetic".into(),
            source_commit: commit,
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
#[test]
fn versioned_upstream_accepts_exact_cross_snapshot_parent_and_refuses_substitution() {
    let parent = packet(task("a"), BTreeMap::new());
    let result = returned(&parent, "part/a");
    let parents = BTreeMap::from([(
        "a".into(),
        Parent {
            packet: &parent,
            result: &result,
        },
    )]);
    for version in ["v1", "v2"] {
        let bindings = Bindings {
            streaming: None,
            schema: format!("reel.workflow-upstream-inputs.{version}"),
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
        let mut consumer = child(&bindings, &parents, vec![result.artifacts["note"].clone()]);
        consumer = packet_at(consumer.task, consumer.dependencies, "b".repeat(40));
        if version == "v1" {
            assert_eq!(
                bind(&consumer, &bindings, &parents).unwrap_err().kind,
                ErrorKind::Stale
            );
            continue;
        }
        assert_eq!(bind(&consumer, &bindings, &parents).unwrap().len(), 1);
        let wrong_parent = packet_at(
            parent.task.clone(),
            parent.dependencies.clone(),
            "c".repeat(40),
        );
        let mut wrong_result = result.clone();
        wrong_result.task_key = wrong_parent.task_key.clone();
        // Identical output bytes and logical IDs do not satisfy the pinned result.
        let substituted = BTreeMap::from([(
            "a".into(),
            Parent {
                packet: &wrong_parent,
                result: &wrong_result,
            },
        )]);
        assert_eq!(
            bind(&consumer, &bindings, &substituted).unwrap_err().kind,
            ErrorKind::Stale
        );
        let inconsistent = BTreeMap::from([(
            "a".into(),
            Parent {
                packet: &wrong_parent,
                result: &result,
            },
        )]);
        assert!(bind(&consumer, &bindings, &inconsistent).is_err());
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
        streaming: None,
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
        streaming: None,
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
        task_snapshots: Default::default(),
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
        streaming: None,
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

#[test]
fn explicit_streaming_binds_seven_large_intermediates_and_keeps_legacy_limits() {
    let sizes = [
        3_360_000_000u64,
        2_900_000_000,
        400_000_000,
        400_000_000,
        400_000_000,
        400_000_000,
        76_104_893,
    ];
    assert_eq!(sizes.iter().sum::<u64>(), 7_936_104_893);
    let producers: Vec<_> = sizes
        .iter()
        .enumerate()
        .map(|(i, bytes)| {
            let mut producer = task(&format!("part-{i}"));
            producer.outputs.get_mut("note").unwrap().max_bytes = *bytes;
            let packet = packet_at(producer, BTreeMap::new(), format!("{i:040x}"));
            let mut result = returned(&packet, &format!("part/{i}"));
            result.artifacts.get_mut("note").unwrap().bytes = *bytes;
            (packet, result)
        })
        .collect();
    let parents: BTreeMap<_, _> = producers
        .iter()
        .map(|(p, r)| {
            (
                p.task.id.clone(),
                Parent {
                    packet: p,
                    result: r,
                },
            )
        })
        .collect();
    let mut bindings = Bindings {
        schema: "reel.workflow-upstream-inputs.v2".into(),
        streaming: None,
        inputs: producers
            .iter()
            .map(|(p, r)| {
                (
                    r.artifacts["note"].logical_id.clone(),
                    Reference {
                        task_id: p.task.id.clone(),
                        artifact: Selector::Output {
                            name: "note".into(),
                        },
                    },
                )
            })
            .collect(),
    };
    let inputs = producers
        .iter()
        .map(|(_, r)| r.artifacts["note"].clone())
        .collect::<Vec<_>>();
    let legacy = bindings.canonical_bytes().unwrap();
    assert!(
        !serde_json::from_slice::<serde_json::Value>(&legacy)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("streaming")
    );
    let old = child(&bindings, &parents, inputs.clone());
    assert_eq!(
        bind(&old, &bindings, &parents).unwrap_err().kind,
        ErrorKind::Budget
    );
    bindings.schema = "reel.workflow-upstream-inputs.v3".into();
    bindings.streaming = Some(Streaming {
        schema: "reel.workflow-upstream-streaming.v1".into(),
        input_bytes: 8 * 1024 * 1024 * 1024,
        input_object_bytes: 4 * 1024 * 1024 * 1024,
    });
    let current = child(&bindings, &parents, inputs.clone());
    assert_ne!(current.task_key, old.task_key);
    assert_eq!(bind(&current, &bindings, &parents).unwrap().len(), 7);
    bindings.streaming.as_mut().unwrap().input_bytes = 7_936_104_892;
    assert_eq!(
        bind(&current, &bindings, &parents).unwrap_err().kind,
        ErrorKind::Integrity
    );
    let changed = child(&bindings, &parents, inputs.clone());
    assert_eq!(
        bind(&changed, &bindings, &parents).unwrap_err().kind,
        ErrorKind::Budget
    );
    bindings.streaming.as_mut().unwrap().input_bytes = 8 * 1024 * 1024 * 1024;
    bindings.streaming.as_mut().unwrap().input_object_bytes = 3_359_999_999;
    let changed = child(&bindings, &parents, inputs);
    assert_eq!(
        bind(&changed, &bindings, &parents).unwrap_err().kind,
        ErrorKind::Budget
    );
    bindings.streaming = None;
    bindings.schema = "reel.workflow-upstream-inputs.v2".into();
    assert_eq!(bindings.canonical_bytes().unwrap(), legacy);
}

#[test]
fn streaming_schema_and_numeric_envelopes_cannot_expand_legacy_bindings() {
    let mut bindings = Bindings {
        schema: "reel.workflow-upstream-inputs.v3".into(),
        inputs: BTreeMap::new(),
        streaming: None,
    };
    assert!(bindings.allowance().is_err());
    bindings.streaming = Some(Streaming {
        schema: "reel.workflow-upstream-streaming.v1".into(),
        input_bytes: 64 * 1024 * 1024 * 1024,
        input_object_bytes: 16 * 1024 * 1024 * 1024,
    });
    bindings.allowance().unwrap();
    for version in ["v1", "v2"] {
        bindings.schema = format!("reel.workflow-upstream-inputs.{version}");
        assert!(bindings.allowance().is_err());
    }
    bindings.schema = "reel.workflow-upstream-inputs.v3".into();
    bindings.streaming.as_mut().unwrap().input_object_bytes += 1;
    assert_eq!(bindings.allowance().unwrap_err().kind, ErrorKind::Budget);
    bindings.streaming.as_mut().unwrap().input_object_bytes = 1;
    bindings.streaming.as_mut().unwrap().input_bytes += 1;
    assert_eq!(bindings.allowance().unwrap_err().kind, ErrorKind::Budget);
}

#[test]
fn explicit_empty_inputs_are_only_for_root_candidate_jobs() {
    let mut bindings = Bindings {
        schema: "reel.workflow-upstream-empty.v1".into(),
        inputs: BTreeMap::new(),
        streaming: None,
    };
    let mut job = task("external-source-intake");
    job.operation = Operation::ExternalSourceIntake;
    job.upstream_inputs = Some(asset(
        "control/empty",
        &bindings.canonical_bytes().unwrap(),
        "application/json",
    ));
    let plan = |task: Task| Plan {
        schema: PLAN_SCHEMA.into(),
        project: "synthetic".into(),
        source_commit: "a".repeat(40),
        task_snapshots: BTreeMap::new(),
        tasks: vec![task],
    };
    let make = |task: Task| make_packet(&plan(task), "external-source-intake", BTreeMap::new());
    let accepted = make(job.clone()).unwrap();
    bind(&accepted, &bindings, &BTreeMap::new()).unwrap();
    job.inputs
        .push(asset("hidden/source", b"hidden", "text/plain"));
    assert_eq!(
        make(job.clone()).unwrap_err().code,
        "source-intake-root-contract"
    );
    job.inputs.clear();
    job.operation = Operation::TextProposal;
    assert_eq!(
        make(job.clone()).unwrap_err().code,
        "upstream-input-declaration"
    );
    job.operation = Operation::ExternalSourceIntake;
    bindings.schema = "reel.workflow-upstream-inputs.v2".into();
    job.upstream_inputs = Some(asset(
        "control/empty",
        &bindings.canonical_bytes().unwrap(),
        "application/json",
    ));
    assert_eq!(
        bind(&make(job).unwrap(), &bindings, &BTreeMap::new())
            .unwrap_err()
            .code,
        "upstream-binding-contract"
    );
    let mut result = returned(&accepted, "external/note");
    result.provenance.mode = Mode::Direct;
    assert_eq!(
        check_result(&accepted, &result).unwrap_err().code,
        "source-intake-handoff-provenance"
    );
    bindings.schema = "reel.workflow-upstream-empty.v1".into();
    bindings.streaming = Some(Streaming {
        schema: "reel.workflow-upstream-streaming.v1".into(),
        input_bytes: 1,
        input_object_bytes: 1,
    });
    assert_eq!(
        bindings.allowance().unwrap_err().code,
        "upstream-version-envelope"
    );
}
