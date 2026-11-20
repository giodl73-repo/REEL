//! A whole-publication report must identify its producer when a plan contains
//! several snapshots/languages. No fixture here confers native custody/viewing.
use super::*;

fn versioned() -> (
    Plan,
    Plan,
    Memory,
    review::Copy,
    review::Report,
    Packet,
    ReturnedResult,
) {
    let (mut child, parent, result, bindings, mut memory) = review_control_fixture();
    let parents = BTreeMap::from([(
        parent.task.id.clone(),
        upstream::Parent {
            packet: &parent,
            result: &result,
        },
    )]);
    let legacy = review_control::generate(&child, &bindings, &parents, &mut memory).unwrap();
    #[derive(serde::Serialize)]
    struct LegacyRequest<'a> {
        schema: &'a str,
        production_commit: &'a str,
        copy_input: &'a str,
        report_output: &'a str,
    }
    let legacy_request: review::Request = decode(&legacy.files["review-request.json"]).unwrap();
    assert!(legacy_request.production.is_none());
    assert_eq!(
        legacy.files["review-request.json"],
        serde_json::to_vec(&LegacyRequest {
            schema: "reel.workflow-review-request.v1",
            production_commit: &parent.source_commit,
            copy_input: &legacy.outputs["review-copy.json"].logical_id,
            report_output: "report",
        })
        .unwrap()
    );
    child.task.tool.as_mut().unwrap().source_bindings =
        review_control::bound_source_bindings(&parent, &result).unwrap();
    let control_plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: child.project.clone(),
        source_commit: child.source_commit.clone(),
        task_snapshots: Default::default(),
        tasks: vec![parent.task.clone(), child.task.clone()],
    };
    let child = make_packet(&control_plan, &child.task.id, child.dependencies.clone()).unwrap();
    let generated = review_control::generate(&child, &bindings, &parents, &mut memory).unwrap();
    let request: review::Request = decode(&generated.files["review-request.json"]).unwrap();
    assert_eq!(request.schema, "reel.workflow-review-request.v2");
    let witness = request.production.as_ref().unwrap();
    assert_eq!(witness.packet, parent);
    assert_eq!(witness.result, result);
    let copy: review::Copy = decode(&generated.files["review-copy.json"]).unwrap();
    let report = review::Report { schema: "reel.workflow-review-report.v1".into(), copy_sha256: canonical_digest(&copy).unwrap(), findings: vec![review::Finding {
        code: "synthetic-target".into(), unit: copy.units.keys().next().unwrap().clone(), frames: None,
        modality: review::Modality::Text, route: review::Route::Project,
        requested_change: "Synthetic exact-producer correspondence only, no claimed creative defect or viewing.".into(), evidence: vec![copy.review.clone()],
    }] };
    let review_task = Task {
        id: "report".into(),
        native_inputs: None,
        upstream_inputs: None,
        operation: Operation::ReviewReport,
        tool: None,
        language: Some(copy.language.clone()),
        prompt: "Synthetic producer-bound report, no viewing or approval claim.".into(),
        inputs: std::iter::once(copy.publication.clone())
            .chain(generated.outputs.values().cloned())
            .collect(),
        depends_on: vec![],
        outputs: BTreeMap::from([(
            "report".into(),
            OutputContract {
                media_type: "application/json".into(),
                max_bytes: MAX_CONTROL_BYTES as u64,
                required: true,
            },
        )]),
    };
    for (name, pin) in &generated.outputs {
        memory
            .bytes
            .insert(pin.logical_id.clone(), generated.files[name].clone());
    }
    let report_plan = Plan {
        tasks: vec![review_task],
        ..control_plan.clone()
    };
    let mut other = parent.task.clone();
    other.id = "other-edition-producer".into();
    let control_result = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: child.task.id.clone(),
        task_key: child.task_key.clone(),
        attempt_id: "control-attempt-1".into(),
        artifacts: generated.outputs.clone(),
        provenance: Provenance {
            mode: Mode::Tool,
            provider: None,
            model: None,
            executable: Some(child.task.tool.as_ref().unwrap().executable.clone()),
        },
        worker_receipt: Some(memory.put(
            "control/worker-receipt",
            "application/json",
            generated.files["receipt.json"].clone(),
        )),
    };
    let baseline = Plan {
        schema: SCOPED_PLAN_SCHEMA.into(),
        task_snapshots: BTreeMap::from([
            (parent.task.id.clone(), parent.source_commit.clone()),
            (other.id.clone(), "b".repeat(40)),
        ]),
        source_commit: "c".repeat(40),
        tasks: vec![parent.task.clone(), other],
        ..control_plan
    };
    (
        baseline,
        report_plan,
        memory,
        copy,
        report,
        child,
        control_result,
    )
}

fn checked(plan: &Plan, memory: &mut Memory, report: &review::Report) -> review::CheckedReport {
    let packet = make_packet(plan, "report", BTreeMap::new()).unwrap();
    let bound = review::bind(&packet, memory).unwrap();
    let raw = serde_json::to_vec(report).unwrap();
    let returned = bound.pack_handoff("report-attempt-1", &raw).unwrap();
    bound.check(&returned, &raw).unwrap()
}

#[test]
fn mixed_snapshot_repair_binds_the_reviewed_producer_instead_of_guessing_from_shared_master() {
    let (mut baseline, report_plan, mut memory, copy, report, ..) = versioned();
    let checked = checked(&report_plan, &mut memory, &report);
    let before = baseline.tasks[0].inputs[0].clone();
    let repair = review::Repair {
        schema: "reel.workflow-repair.v1".into(),
        report_sha256: checked.report_sha256().into(),
        finding_keys: checked.findings().keys().cloned().collect(),
        edits: vec![review::Edit {
            after: Artifact {
                sha256: digest(b"changed source"),
                bytes: 14,
                ..before.clone()
            },
            before,
        }],
        roots: None,
    };
    let unbound = review::check_report(&copy, &report).unwrap();
    assert_eq!(
        review::repair_impact(&baseline, &unbound, &repair)
            .unwrap_err()
            .code,
        "review-repair-baseline"
    );
    assert_eq!(
        review::repair_impact(&baseline, &checked, &repair)
            .unwrap()
            .rebuild
            .len(),
        2
    );
    let task = &checked.production().unwrap().packet.task.id;
    checked.check_production_task(&baseline, task).unwrap();
    assert_eq!(
        checked
            .check_production_task(&baseline, "other-edition-producer")
            .unwrap_err()
            .code,
        "review-production-plan"
    );
    baseline.tasks[1]
        .outputs
        .values_mut()
        .next()
        .unwrap()
        .max_bytes -= 1;
    review::repair_impact(&baseline, &checked, &repair).unwrap();
    baseline.tasks[0]
        .outputs
        .values_mut()
        .next()
        .unwrap()
        .max_bytes -= 1;
    assert_eq!(
        review::repair_impact(&baseline, &checked, &repair)
            .unwrap_err()
            .code,
        "review-production-plan"
    );
}

#[test]
fn request_version_and_producer_correspondence_cannot_be_downgraded_or_omitted() {
    for mode in [
        "legacy-witness",
        "missing-witness",
        "missing-receipt",
        "foreign-commit",
        "wrong-executable",
    ] {
        let (_, mut plan, mut memory, ..) = versioned();
        let mut request: review::Request = decode(&memory.bytes[review::REQUEST_ID]).unwrap();
        match mode {
            "legacy-witness" => request.schema = "reel.workflow-review-request.v1".into(),
            "missing-witness" => request.production = None,
            "missing-receipt" => request.production.as_mut().unwrap().result.worker_receipt = None,
            "foreign-commit" => request.production_commit = "d".repeat(40),
            "wrong-executable" => {
                request
                    .production
                    .as_mut()
                    .unwrap()
                    .result
                    .provenance
                    .executable
                    .as_mut()
                    .unwrap()
                    .sha256 = "e".repeat(64)
            }
            _ => unreachable!(),
        }
        let pin = memory.put(
            review::REQUEST_ID,
            "application/json",
            serde_json::to_vec(&request).unwrap(),
        );
        *plan.tasks[0]
            .inputs
            .iter_mut()
            .find(|a| a.logical_id == review::REQUEST_ID)
            .unwrap() = pin;
        let packet = make_packet(&plan, "report", BTreeMap::new()).unwrap();
        assert!(review::bind(&packet, &mut memory).is_err(), "{mode}");
    }
}

#[test]
fn native_origin_contract_requires_one_control_for_request_copy_and_manifest_and_exact_producer() {
    for mode in [
        "correct",
        "source-authored-request",
        "different-result",
        "wrong-selector",
    ] {
        let (mut plan, report_plan, mut memory, _, _, control, result) = versioned();
        let mut bindings = upstream::Bindings {
            schema: "reel.workflow-upstream-inputs.v2".into(),
            streaming: None,
            inputs: result
                .artifacts
                .iter()
                .map(|(name, pin)| {
                    (
                        pin.logical_id.clone(),
                        upstream::Reference {
                            task_id: control.task.id.clone(),
                            artifact: upstream::Selector::Output { name: name.clone() },
                        },
                    )
                })
                .collect(),
        };
        if mode == "source-authored-request" {
            bindings.inputs.remove(review::REQUEST_ID);
        }
        if mode == "wrong-selector" {
            bindings
                .inputs
                .get_mut(review::REQUEST_ID)
                .unwrap()
                .artifact = upstream::Selector::WorkerReceipt;
        }
        let raw = bindings.canonical_bytes().unwrap();
        let pin = memory.put("control/report-origins", "application/json", raw);
        plan.task_snapshots
            .insert(control.task.id.clone(), control.source_commit.clone());
        plan.tasks.push(control.task.clone());
        let mut task = report_plan.tasks[0].clone();
        task.depends_on = vec![control.task.id.clone()];
        task.upstream_inputs = Some(pin);
        plan.task_snapshots.insert(task.id.clone(), "d".repeat(40));
        plan.tasks.push(task);
        let packet = make_packet(
            &plan,
            "report",
            BTreeMap::from([(
                control.task.id.clone(),
                DependencyResult {
                    result_sha256: canonical_digest(&result).unwrap(),
                    artifacts: result.artifacts.clone(),
                },
            )]),
        )
        .unwrap();
        let bound = review::bind(&packet, &mut memory).unwrap();
        let mut production = bound.production().unwrap().clone();
        if mode == "different-result" {
            production.result.attempt_id = "another-producer-attempt".into();
        }
        let parents = BTreeMap::from([(
            control.task.id.clone(),
            upstream::Parent {
                packet: &control,
                result: &result,
            },
        )]);
        let checked =
            review_control::check_request_origin(&packet, &production, &bindings, &parents);
        if mode == "correct" {
            checked.unwrap();
        } else {
            assert!(checked.is_err(), "{mode}");
        }
    }
}
