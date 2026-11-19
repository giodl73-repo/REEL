//! Frontier protocol fixtures, not native protection or storage qualification.
use reel_workflow::{
    graph::{self, Job, Observer, Status},
    handoff::{State, Ticket},
    *,
};
use std::collections::BTreeMap;
fn plan() -> Plan {
    let mut tasks: Vec<_> = ["a", "b", "c"]
        .into_iter()
        .map(|id| Task {
            id: id.into(),
            upstream_inputs: None,
            native_inputs: None,
            operation: Operation::TextProposal,
            tool: None,
            language: Some("es".into()),
            prompt: "Synthetic private prompt.".into(),
            inputs: vec![],
            depends_on: vec![],
            outputs: BTreeMap::from([(
                "note".into(),
                OutputContract {
                    media_type: "text/plain".into(),
                    max_bytes: 100,
                    required: true,
                },
            )]),
        })
        .collect();
    tasks[2].depends_on = vec!["a".into()];
    Plan {
        schema: PLAN_SCHEMA.into(),
        project: "synthetic".into(),
        source_commit: "a".repeat(40),
        tasks,
    }
}
fn artifact(id: &str) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: digest(b"synthetic"),
        bytes: 9,
        media_type: "application/json".into(),
    }
}
fn job(packet: Packet, state: State) -> Job {
    let result = matches!(state, State::Retained | State::ImportPending).then(|| ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: "attempt-1".into(),
        worker_receipt: None,
        artifacts: BTreeMap::from([(
            "note".into(),
            Artifact {
                media_type: "text/plain".into(),
                ..artifact("candidate/note")
            },
        )]),
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
    });
    Job {
        ticket: Ticket {
            packet,
            attempt_id: "attempt-1".into(),
            native_binding: artifact("fixture/binding"),
            lifecycle_tool: artifact("fixture/tool"),
        },
        state,
        result,
        acknowledgement: (state == State::Retained).then(|| artifact("fixture/ack")),
    }
}
#[derive(Default)]
struct Fixture {
    jobs: BTreeMap<String, Job>,
    refused: BTreeMap<String, Error>,
    verified: Vec<String>,
}
impl Observer for Fixture {
    fn observe(&mut self, id: &str) -> Result<Option<Job>> {
        Ok(self.jobs.get(id).cloned())
    }
    fn verify(&mut self, job: &Job) -> Result<()> {
        let id = job.ticket.packet.task.id.clone();
        self.verified.push(id.clone());
        if let Some(error) = self.refused.get(&id) {
            Err(error.clone())
        } else {
            Ok(())
        }
    }
}

fn recipe_fixture() -> (recipe::Recipe, Fixture) {
    let base = plan();
    let mut one = expansion_template(&base).task;
    one.id = "one".into();
    let mut two = one.clone();
    two.id = "two".into();
    let mut three = one.clone();
    three.id = "three".into();
    three.depends_on = vec!["one".into()];
    three.inputs = vec![expansion::Input::Parent {
        reference: upstream::Reference {
            task_id: "one".into(),
            artifact: upstream::Selector::Output {
                name: "note".into(),
            },
        },
    }];
    let mut observer = Fixture::default();
    observer.jobs.insert(
        "a".into(),
        job(
            make_packet(&base, "a", BTreeMap::new()).unwrap(),
            State::Retained,
        ),
    );
    (
        recipe::Recipe {
            schema: "reel.workflow-recipe.v1".into(),
            base,
            steps: vec![one, two, three],
        },
        observer,
    )
}
#[test]
fn recipe_proposes_bounded_waves_without_unlocking_unretained_descendants() {
    let (recipe, mut observer) = recipe_fixture();
    let wave = recipe::inspect(&recipe, &recipe.base, 1, &mut observer).unwrap();
    assert_eq!(wave.proposals.len(), 1);
    assert_eq!(wave.proposals[0].packet.task.id, "one");
    assert_eq!(wave.rows[0].state, recipe::State::Ready);
    assert_eq!(wave.rows[1].state, recipe::State::Deferred);
    assert_eq!(wave.rows[2].state, recipe::State::Blocked);
    assert_eq!(wave.rows[2].waiting_on, ["one"]);
    assert_eq!(observer.verified, ["a"]);
    assert_eq!(
        wave.proposals[0].template.base_plan_sha256,
        canonical_digest(&recipe.base).unwrap()
    );
}
#[test]
fn recipe_keeps_waiting_steps_and_unlocks_children_only_after_verified_retention() {
    let (mut recipe, mut observer) = recipe_fixture();
    let wave = recipe::inspect(&recipe, &recipe.base, 1, &mut observer).unwrap();
    let mut current = recipe.base.clone();
    current.tasks.push(wave.proposals[0].packet.task.clone());
    observer.jobs.insert(
        "one".into(),
        job(wave.proposals[0].packet.clone(), State::Waiting),
    );
    let wave = recipe::inspect(&recipe, &current, 16, &mut observer).unwrap();
    assert_eq!(wave.rows[0].state, recipe::State::Existing);
    assert_eq!(wave.rows[0].existing_status, Some(Status::Waiting));
    assert_eq!(wave.rows[2].state, recipe::State::Blocked);
    assert!(wave.proposals.iter().all(|p| p.packet.task.id != "one"));
    let packet = observer.jobs["one"].ticket.packet.clone();
    observer
        .jobs
        .insert("one".into(), job(packet, State::Retained));
    let wave = recipe::inspect(&recipe, &current, 16, &mut observer).unwrap();
    assert_eq!(wave.rows[2].state, recipe::State::Ready);
    recipe.steps.swap(0, 2);
    let wave = recipe::inspect(&recipe, &current, 1, &mut observer).unwrap();
    assert_eq!(wave.proposals[0].packet.task.id, "three");
    recipe.steps.swap(0, 2);
    recipe.steps[0].prompt.push_str(" edited");
    let wave = recipe::inspect(&recipe, &current, 16, &mut observer).unwrap();
    assert_eq!(wave.rows[0].state, recipe::State::Refused);
    assert_eq!(wave.rows[0].error.as_ref().unwrap().kind, ErrorKind::Stale);
    assert_eq!(wave.rows[2].state, recipe::State::Blocked);
    assert!(wave.proposals.iter().all(|p| p.packet.task.id != "three"));
    recipe.steps.swap(0, 2);
    let wave = recipe::inspect(&recipe, &current, 1, &mut observer).unwrap();
    assert_eq!(wave.rows[0].state, recipe::State::Blocked);
    assert_eq!(wave.proposals[0].packet.task.id, "two");
    assert_eq!(observer.jobs["one"].ticket.packet.task, current.tasks[3]);
}
#[test]
fn recipe_refuses_cycles_bad_selectors_limits_and_replaced_base_tasks() {
    let (mut recipe, mut observer) = recipe_fixture();
    assert_eq!(
        recipe::inspect(&recipe, &recipe.base, 17, &mut observer)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Budget
    );
    assert!(observer.verified.is_empty());
    let mut current = recipe.base.clone();
    current.tasks[0].prompt.push_str(" changed");
    assert_eq!(
        recipe::inspect(&recipe, &current, 1, &mut observer)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Stale
    );
    recipe.steps[0].inputs = vec![expansion::Input::Parent {
        reference: upstream::Reference {
            task_id: "a".into(),
            artifact: upstream::Selector::Output {
                name: "absent".into(),
            },
        },
    }];
    assert_eq!(
        recipe::check(&recipe).unwrap_err().code,
        "recipe-parent-selector"
    );
    recipe.steps[0].inputs.clear();
    recipe.steps[0].depends_on = vec!["three".into()];
    assert_eq!(
        recipe::check(&recipe).unwrap_err().code,
        "recipe-dependency-cycle"
    );
}

fn expansion_template(plan: &Plan) -> expansion::Template {
    expansion::Template {
        schema: "reel.workflow-expansion-template.v1".into(),
        base_plan_sha256: canonical_digest(plan).unwrap(),
        task: expansion::TaskTemplate {
            id: "late-child".into(),
            operation: Operation::TextProposal,
            language: Some("es".into()),
            prompt: "Use the exact retained parent input.".into(),
            depends_on: vec!["a".into()],
            inputs: vec![expansion::Input::Parent {
                reference: upstream::Reference {
                    task_id: "a".into(),
                    artifact: upstream::Selector::Output {
                        name: "note".into(),
                    },
                },
            }],
            outputs: plan.tasks[0].outputs.clone(),
            tool: None,
        },
    }
}
#[test]
fn expansion_uses_exact_verified_parent_outputs_and_preserves_existing_tasks() {
    let plan = plan();
    let template = expansion_template(&plan);
    let mut fixture = Fixture::default();
    let parent = job(
        make_packet(&plan, "a", BTreeMap::new()).unwrap(),
        State::Retained,
    );
    let result = parent.result.as_ref().unwrap().clone();
    fixture.jobs.insert("a".into(), parent);
    let expanded = expansion::prepare(&plan, &template, &mut fixture).unwrap();
    assert_eq!(
        &expanded.plan.tasks[..plan.tasks.len()],
        plan.tasks.as_slice()
    );
    assert_eq!(
        expanded.packet.task.inputs,
        [result.artifacts["note"].clone()]
    );
    assert_eq!(
        expanded.packet.dependencies["a"].result_sha256,
        canonical_digest(&result).unwrap()
    );
    assert_eq!(
        expanded.upstream.as_ref().unwrap().inputs["candidate/note"].task_id,
        "a"
    );
    assert!(expanded.packet.task.native_inputs.is_none());
    assert_eq!(fixture.verified, ["a"]);
    let raw = expanded.upstream.unwrap().canonical_bytes().unwrap();
    assert_eq!(
        expanded.packet.task.upstream_inputs.unwrap().sha256,
        digest(&raw)
    );
}
#[test]
fn expansion_refuses_waiting_pending_cancelled_or_unverified_parent_jobs() {
    let plan = plan();
    let template = expansion_template(&plan);
    for state in [
        State::Waiting,
        State::ImportPending,
        State::Cancelled,
        State::Retained,
    ] {
        let mut fixture = Fixture::default();
        fixture.jobs.insert(
            "a".into(),
            job(make_packet(&plan, "a", BTreeMap::new()).unwrap(), state),
        );
        if state == State::Retained {
            fixture.refused.insert(
                "a".into(),
                Error {
                    kind: ErrorKind::Denied,
                    code: "fixture-lost-retained-coverage".into(),
                },
            );
        }
        let error = expansion::prepare(&plan, &template, &mut fixture)
            .err()
            .unwrap();
        assert!(matches!(error.kind, ErrorKind::Busy | ErrorKind::Denied));
    }
}
#[test]
fn expansion_preserves_origin_even_for_identical_bytes_and_logical_ids() {
    let plan = plan();
    let mut template = expansion_template(&plan);
    let mut fixture = Fixture::default();
    for id in ["a", "b"] {
        fixture.jobs.insert(
            id.into(),
            job(
                make_packet(&plan, id, BTreeMap::new()).unwrap(),
                State::Retained,
            ),
        );
    }
    template.task.depends_on.push("b".into());
    template.task.inputs.push(expansion::Input::Parent {
        reference: upstream::Reference {
            task_id: "b".into(),
            artifact: upstream::Selector::Output {
                name: "note".into(),
            },
        },
    });
    assert_eq!(
        expansion::prepare(&plan, &template, &mut fixture)
            .err()
            .unwrap()
            .code,
        "expansion-logical-origin-alias"
    );
    let mut template = expansion_template(&plan);
    template.task.inputs.push(expansion::Input::Fixed {
        artifact: fixture.jobs["a"].result.as_ref().unwrap().artifacts["note"].clone(),
    });
    assert_eq!(
        expansion::prepare(&plan, &template, &mut fixture)
            .err()
            .unwrap()
            .code,
        "expansion-logical-origin-alias"
    );
}
#[test]
fn expansion_refuses_stale_bases_changed_original_parents_and_missing_selectors() {
    let mut plan = plan();
    let mut template = expansion_template(&plan);
    let mut fixture = Fixture::default();
    fixture.jobs.insert(
        "a".into(),
        job(
            make_packet(&plan, "a", BTreeMap::new()).unwrap(),
            State::Retained,
        ),
    );
    template.base_plan_sha256 = "0".repeat(64);
    assert_eq!(
        expansion::prepare(&plan, &template, &mut fixture)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Stale
    );
    assert!(fixture.verified.is_empty());
    template = expansion_template(&plan);
    template.task.inputs[0] = expansion::Input::Parent {
        reference: upstream::Reference {
            task_id: "a".into(),
            artifact: upstream::Selector::WorkerReceipt,
        },
    };
    assert_eq!(
        expansion::prepare(&plan, &template, &mut fixture)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Missing
    );
    plan.tasks[0].prompt.push_str(" Changed authored input.");
    template = expansion_template(&plan);
    assert_eq!(
        expansion::prepare(&plan, &template, &mut fixture)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Stale
    );
}

#[test]
fn expansion_fills_tool_request_and_source_roles_from_exact_parent_selectors() {
    let mut plan = plan();
    plan.tasks[0].outputs.get_mut("note").unwrap().media_type = "application/json".into();
    let runtime = artifact("parent/runtime");
    let publication = artifact("parent/publication");
    let request = artifact("parent/request");
    plan.tasks[0].operation = Operation::DeterministicTool;
    plan.tasks[0].prompt.clear();
    plan.tasks[0].inputs = vec![publication.clone(), request.clone()];
    plan.tasks[0].tool = Some(ToolContract {
        auxiliary_executables: BTreeMap::new(),
        schema: "reel.workflow-tool.v1".into(),
        capability: ToolCapability::ComicRender,
        executable: runtime.clone(),
        publication,
        edition_id: "es".into(),
        request,
        source_bindings: BTreeMap::new(),
        part: None,
    });
    let mut parent = job(
        make_packet(&plan, "a", BTreeMap::new()).unwrap(),
        State::Retained,
    );
    let result = parent.result.as_mut().unwrap();
    result.provenance = Provenance {
        mode: Mode::Tool,
        provider: None,
        model: None,
        executable: Some(runtime),
    };
    result.artifacts.get_mut("note").unwrap().media_type = "application/json".into();
    result.worker_receipt = Some(artifact("parent/worker-receipt"));
    let expected_request = result.artifacts["note"].clone();
    let expected_receipt = result.worker_receipt.clone().unwrap();
    let mut fixture = Fixture::default();
    fixture.jobs.insert("a".into(), parent);
    let mut template = expansion_template(&plan);
    template.task.operation = Operation::DeterministicTool;
    template.task.prompt.clear();
    template.task.tool = Some(expansion::Tool {
        auxiliary_executables: BTreeMap::from([("ffmpeg".into(), artifact("tools/ffmpeg"))]),
        capability: ToolCapability::ComicIssueControl,
        executable: artifact("tool/runtime"),
        publication: expansion::Input::Fixed {
            artifact: artifact("publication/master"),
        },
        edition_id: "es".into(),
        request: template.task.inputs[0].clone(),
        sources: BTreeMap::from([(
            "parts/opening/receipt.json".into(),
            expansion::Input::Parent {
                reference: upstream::Reference {
                    task_id: "a".into(),
                    artifact: upstream::Selector::WorkerReceipt,
                },
            },
        )]),
        part: None,
    });
    let expanded = expansion::prepare(&plan, &template, &mut fixture).unwrap();
    let tool = expanded.packet.task.tool.as_ref().unwrap();
    assert_eq!(
        tool.auxiliary_executables["ffmpeg"].logical_id,
        "tools/ffmpeg"
    );
    assert_eq!(tool.request, expected_request);
    assert_eq!(
        tool.source_bindings["parts/opening/receipt.json"],
        expected_receipt.logical_id
    );
    let bindings = expanded.upstream.as_ref().unwrap();
    assert_eq!(bindings.inputs.len(), 2);
    assert_eq!(
        bindings.inputs[&expected_receipt.logical_id].artifact,
        upstream::Selector::WorkerReceipt
    );
    assert_eq!(expanded.packet.task.inputs.len(), 3);
}
#[test]
fn only_verified_retained_results_unlock_children_with_exact_dependency_fingerprints() {
    let plan = plan();
    let mut fixture = Fixture::default();
    let a = job(
        make_packet(&plan, "a", BTreeMap::new()).unwrap(),
        State::Retained,
    );
    let expected = DependencyResult {
        result_sha256: canonical_digest(a.result.as_ref().unwrap()).unwrap(),
        artifacts: a.result.as_ref().unwrap().artifacts.clone(),
    };
    fixture.jobs.insert("a".into(), a);
    fixture.jobs.insert(
        "b".into(),
        job(
            make_packet(&plan, "b", BTreeMap::new()).unwrap(),
            State::Waiting,
        ),
    );
    let frontier = graph::inspect(&plan, &mut fixture).unwrap();
    assert_eq!(frontier.waiting.len(), 1);
    assert_eq!(frontier.needs_admission.len(), 1);
    assert_eq!(frontier.needs_admission["c"].dependencies["a"], expected);
    assert_eq!(fixture.verified, vec!["a", "b"]);
    assert_eq!(frontier.rows[0].status, Status::RetainedObserved);
}
#[test]
fn storage_refusal_blocks_descendants_but_keeps_independent_work_and_redacts_reasons() {
    let plan = plan();
    let mut fixture = Fixture::default();
    fixture.jobs.insert(
        "a".into(),
        job(
            make_packet(&plan, "a", BTreeMap::new()).unwrap(),
            State::Retained,
        ),
    );
    fixture.refused.insert(
        "a".into(),
        Error {
            kind: ErrorKind::Missing,
            code: "private/path?secret=hidden".into(),
        },
    );
    let frontier = graph::inspect(&plan, &mut fixture).unwrap();
    assert_eq!(frontier.rows[0].status, Status::Refused);
    assert_eq!(frontier.rows[2].status, Status::Blocked);
    assert!(frontier.needs_admission.contains_key("b"));
    assert!(!frontier.needs_admission.contains_key("c"));
    let json = serde_json::to_string(&frontier.rows).unwrap();
    assert!(!json.contains("secret"));
    assert!(!json.contains("prompt"));
}
#[test]
fn changed_parent_attempt_invalidates_an_already_started_child_without_retargeting_it() {
    let plan = plan();
    let mut fixture = Fixture::default();
    let mut a = job(
        make_packet(&plan, "a", BTreeMap::new()).unwrap(),
        State::Retained,
    );
    let old = DependencyResult {
        result_sha256: canonical_digest(a.result.as_ref().unwrap()).unwrap(),
        artifacts: a.result.as_ref().unwrap().artifacts.clone(),
    };
    let child = job(
        make_packet(&plan, "c", BTreeMap::from([("a".into(), old)])).unwrap(),
        State::Waiting,
    );
    fixture.jobs.insert("c".into(), child.clone());
    a.ticket.attempt_id = "attempt-2".into();
    a.result.as_mut().unwrap().attempt_id = "attempt-2".into();
    fixture.jobs.insert("a".into(), a);
    let frontier = graph::inspect(&plan, &mut fixture).unwrap();
    assert_eq!(
        frontier.rows[2].error.as_ref().unwrap().kind,
        ErrorKind::Stale
    );
    assert_eq!(fixture.jobs["c"], child);
    assert!(!fixture.verified.contains(&"c".into()));
}
#[test]
fn pending_and_cancelled_jobs_are_never_dispatched_or_automatically_reconciled() {
    let plan = plan();
    let mut fixture = Fixture::default();
    fixture.jobs.insert(
        "a".into(),
        job(
            make_packet(&plan, "a", BTreeMap::new()).unwrap(),
            State::ImportPending,
        ),
    );
    fixture.jobs.insert(
        "b".into(),
        job(
            make_packet(&plan, "b", BTreeMap::new()).unwrap(),
            State::Cancelled,
        ),
    );
    let frontier = graph::inspect(&plan, &mut fixture).unwrap();
    assert_eq!(frontier.rows[0].status, Status::NeedsRecovery);
    assert_eq!(frontier.rows[1].status, Status::Cancelled);
    assert!(frontier.waiting.is_empty());
    assert!(fixture.verified.is_empty());
}
#[test]
fn local_retained_marker_with_missing_result_or_another_attempt_is_not_completion() {
    let plan = plan();
    let mut fixture = Fixture::default();
    let mut a = job(
        make_packet(&plan, "a", BTreeMap::new()).unwrap(),
        State::Retained,
    );
    a.result = None;
    fixture.jobs.insert("a".into(), a);
    let mut b = job(
        make_packet(&plan, "b", BTreeMap::new()).unwrap(),
        State::Retained,
    );
    b.result.as_mut().unwrap().attempt_id = "another".into();
    fixture.jobs.insert("b".into(), b);
    let frontier = graph::inspect(&plan, &mut fixture).unwrap();
    assert!(
        frontier.rows[..2]
            .iter()
            .all(|r| r.status == Status::Refused)
    );
    assert!(fixture.verified.is_empty());
}
