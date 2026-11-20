//! Staged repair edges over checked result metadata; no native custody claim.
use reel_workflow::{repair::*, review::*, *};
use std::collections::BTreeMap;
fn pin(id: &str, raw: &[u8]) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: digest(raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    }
}
fn origin(id: &str, selector: upstream::Selector) -> upstream::Reference {
    upstream::Reference {
        task_id: id.into(),
        artifact: selector,
    }
}
fn output(id: &str, name: &str) -> upstream::Reference {
    origin(id, upstream::Selector::Output { name: name.into() })
}
fn bindings(inputs: BTreeMap<String, upstream::Reference>, new: bool) -> upstream::Bindings {
    upstream::Bindings {
        schema: format!("reel.workflow-upstream-inputs.v{}", if new { 2 } else { 1 }),
        streaming: None,
        inputs,
    }
}
fn binding_pin(id: &str, b: &upstream::Bindings) -> Artifact {
    pin(id, &b.canonical_bytes().unwrap())
}
fn task(id: &str, master: &Artifact, request: &Artifact, deps: &[&str]) -> Task {
    Task {
        id: id.into(),
        native_inputs: Some(pin(&format!("native/{id}"), b"before native")),
        upstream_inputs: None,
        operation: Operation::DeterministicTool,
        language: Some("es".into()),
        prompt: String::new(),
        inputs: vec![master.clone(), request.clone()],
        depends_on: deps.iter().map(|s| (*s).into()).collect(),
        outputs: BTreeMap::from([(
            "output".into(),
            OutputContract {
                media_type: "application/json".into(),
                max_bytes: 4096,
                required: true,
            },
        )]),
        tool: Some(ToolContract {
            schema: "reel.workflow-tool.v1".into(),
            capability: ToolCapability::ComicRender,
            executable: pin("tool/reel", b"synthetic executable"),
            auxiliary_executables: BTreeMap::new(),
            publication: master.clone(),
            request: request.clone(),
            edition_id: "es".into(),
            source_bindings: BTreeMap::new(),
            part: None,
        }),
    }
}
fn deps(parents: &[&ResultWitness]) -> BTreeMap<String, DependencyResult> {
    parents
        .iter()
        .map(|p| {
            (
                p.packet.task.id.clone(),
                DependencyResult {
                    result_sha256: canonical_digest(&p.result).unwrap(),
                    artifacts: p.result.artifacts.clone(),
                },
            )
        })
        .collect()
}
fn result(plan: &Plan, id: &str, parents: &[&ResultWitness], version: &str) -> ResultWitness {
    let packet = make_packet(plan, id, deps(parents)).unwrap();
    let returned = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: id.into(),
        task_key: packet.task_key.clone(),
        attempt_id: format!("attempt-{version}"),
        artifacts: BTreeMap::from([(
            "output".into(),
            pin(
                &format!("derived/{}/output", packet.task_key),
                version.as_bytes(),
            ),
        )]),
        worker_receipt: Some(pin(
            &format!("worker/{}/receipt", packet.task_key),
            version.as_bytes(),
        )),
        provenance: Provenance {
            mode: Mode::Tool,
            provider: None,
            model: None,
            executable: Some(packet.task.tool.as_ref().unwrap().executable.clone()),
        },
    };
    check_result(&packet, &returned).unwrap();
    ResultWitness {
        packet,
        result: returned,
    }
}
struct Fixture {
    baseline: Plan,
    report: CheckedReport,
    repair: Repair,
    revision: Revision,
    first: Plan,
}
fn fixture() -> Fixture {
    let master = pin("publication/master", b"before master");
    let mut baseline = Plan {
        schema: PLAN_SCHEMA.into(),
        task_snapshots: BTreeMap::new(),
        project: "synthetic".into(),
        source_commit: "a".repeat(40),
        tasks: vec![
            task("drawing", &master, &pin("request/drawing", b"drawing"), &[]),
            task("closing", &master, &pin("request/closing", b"closing"), &[]),
        ],
    };
    let old_draw = result(&baseline, "drawing", &[], "old-drawing");
    let closing = result(&baseline, "closing", &[], "unchanged-closing");
    let old_receipt = old_draw.result.worker_receipt.as_ref().unwrap();
    let old_output = &old_draw.result.artifacts["output"];
    let ending = &closing.result.artifacts["output"];
    let ctrl_before = bindings(
        BTreeMap::from([
            (
                old_receipt.logical_id.clone(),
                origin("drawing", upstream::Selector::WorkerReceipt),
            ),
            (old_output.logical_id.clone(), output("drawing", "output")),
            (ending.logical_id.clone(), output("closing", "output")),
        ]),
        false,
    );
    let mut control = task(
        "controls",
        &master,
        &pin("request/controls", b"control template"),
        &["drawing", "closing"],
    );
    control.upstream_inputs = Some(binding_pin("origins/controls", &ctrl_before));
    control
        .inputs
        .extend([old_receipt.clone(), old_output.clone(), ending.clone()]);
    control.tool.as_mut().unwrap().source_bindings = BTreeMap::from([
        ("drawing.receipt".into(), old_receipt.logical_id.clone()),
        ("drawing.packet".into(), old_output.logical_id.clone()),
        ("closing.packet".into(), ending.logical_id.clone()),
    ]);
    baseline.tasks.push(control);
    let old_control = result(
        &baseline,
        "controls",
        &[&old_draw, &closing],
        "old-controls",
    );
    let old_assembly = &old_control.result.artifacts["output"];
    let mut issue = task(
        "issue",
        &master,
        old_assembly,
        &["drawing", "closing", "controls"],
    );
    issue
        .inputs
        .extend([old_receipt.clone(), old_output.clone(), ending.clone()]);
    issue.tool.as_mut().unwrap().source_bindings = BTreeMap::from([
        ("drawing.receipt".into(), old_receipt.logical_id.clone()),
        ("drawing.packet".into(), old_output.logical_id.clone()),
        ("closing.packet".into(), ending.logical_id.clone()),
    ]);
    let mut issue_before = ctrl_before.clone();
    issue_before.inputs.insert(
        old_assembly.logical_id.clone(),
        output("controls", "output"),
    );
    issue.upstream_inputs = Some(binding_pin("origins/issue", &issue_before));
    baseline.tasks.push(issue);
    baseline.tasks.push(task(
        "review",
        &master,
        &pin("request/review", b"contextual review"),
        &["issue"],
    ));
    let copy = Copy {
        schema: "reel.workflow-review-copy.v1".into(),
        project: baseline.project.clone(),
        source_commit: baseline.source_commit.clone(),
        language: "es".into(),
        publication: master.clone(),
        review: pin("review/proof", b"whole issue"),
        format: Format::Comic {
            page_order: vec!["page01".into(), "page02".into()],
        },
        units: BTreeMap::from([(
            "target".into(),
            Unit::Comic {
                page_id: "page01".into(),
                panel_id: None,
                balloon_id: None,
            },
        )]),
    };
    let report = check_report(
        &copy,
        &Report {
            schema: "reel.workflow-review-report.v1".into(),
            copy_sha256: canonical_digest(&copy).unwrap(),
            findings: vec![Finding {
                code: "synthetic-layout".into(),
                unit: "target".into(),
                frames: None,
                modality: Modality::Layout,
                route: Route::Project,
                requested_change: "Synthetic staged repair metadata only".into(),
                evidence: vec![copy.review.clone()],
            }],
        },
    )
    .unwrap();
    let repair = Repair {
        schema: "reel.workflow-repair.v2".into(),
        report_sha256: report.report_sha256().into(),
        finding_keys: report.findings().keys().cloned().collect(),
        edits: vec![Edit {
            before: master,
            after: pin("publication/master", b"after master"),
        }],
        roots: Some(vec!["drawing".into()]),
    };
    let impact = repair_impact(&baseline, &report, &repair).unwrap();
    let mut revision = Revision {
        schema: "reel.workflow-repair-revision.v3".into(),
        source_commit: "c".repeat(40),
        derivation: None,
        task_snapshots: impact
            .rebuild
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    if id == "drawing" {
                        "b".repeat(40)
                    } else {
                        "c".repeat(40)
                    },
                )
            })
            .collect(),
        native_inputs: baseline
            .tasks
            .iter()
            .filter(|t| impact.rebuild.contains(&t.id))
            .map(|t| {
                (
                    t.id.clone(),
                    Edit {
                        before: t.native_inputs.clone().unwrap(),
                        after: pin(&format!("native/{}", t.id), b"after native"),
                    },
                )
            })
            .collect(),
    };
    let initial = materialize(&baseline, &report, &repair, &revision).unwrap();
    let new_draw = result(&initial.plan, "drawing", &[], "new-drawing");
    let mut ctrl_after = ctrl_before.clone();
    ctrl_after.schema = "reel.workflow-upstream-inputs.v2".into();
    ctrl_after.inputs.remove(&old_receipt.logical_id);
    ctrl_after.inputs.remove(&old_output.logical_id);
    ctrl_after.inputs.insert(
        new_draw
            .result
            .worker_receipt
            .as_ref()
            .unwrap()
            .logical_id
            .clone(),
        origin("drawing", upstream::Selector::WorkerReceipt),
    );
    ctrl_after.inputs.insert(
        new_draw.result.artifacts["output"].logical_id.clone(),
        output("drawing", "output"),
    );
    revision.schema = "reel.workflow-repair-revision.v4".into();
    revision.derivation = Some(Derivation {
        parents: BTreeMap::from([
            (
                "drawing".into(),
                ParentChange {
                    before: old_draw,
                    after: new_draw,
                },
            ),
            (
                "closing".into(),
                ParentChange {
                    before: closing.clone(),
                    after: closing,
                },
            ),
        ]),
        consumers: BTreeMap::from([(
            "controls".into(),
            ConsumerBindings {
                before: ctrl_before,
                after: ctrl_after.clone(),
            },
        )]),
    });
    let first = materialize(&baseline, &report, &repair, &revision)
        .unwrap()
        .plan;
    let d = revision.derivation.as_mut().unwrap();
    let new_control = result(
        &first,
        "controls",
        &[&d.parents["drawing"].after, &d.parents["closing"].after],
        "new-controls",
    );
    let mut issue_after = ctrl_after;
    issue_after.inputs.insert(
        new_control.result.artifacts["output"].logical_id.clone(),
        output("controls", "output"),
    );
    d.parents.insert(
        "controls".into(),
        ParentChange {
            before: old_control,
            after: new_control,
        },
    );
    d.consumers.insert(
        "issue".into(),
        ConsumerBindings {
            before: issue_before,
            after: issue_after,
        },
    );
    Fixture {
        baseline,
        report,
        repair,
        revision,
        first,
    }
}
#[test]
fn observed_new_results_rebind_consumer_requests_sources_and_dependencies_in_stages() {
    let f = fixture();
    let candidate = materialize(&f.baseline, &f.report, &f.repair, &f.revision).unwrap();
    let d = f.revision.derivation.as_ref().unwrap();
    let root = &d.parents["drawing"].after;
    let closing = &d.parents["closing"].after;
    let control = &d.parents["controls"].after;
    assert_eq!(
        make_packet(&candidate.plan, "drawing", BTreeMap::new()).unwrap(),
        root.packet
    );
    assert_eq!(
        make_packet(&candidate.plan, "closing", BTreeMap::new()).unwrap(),
        closing.packet
    );
    assert_eq!(
        make_packet(&candidate.plan, "controls", deps(&[root, closing])).unwrap(),
        control.packet
    );
    assert_eq!(candidate.plan.tasks[2], f.first.tasks[2]);
    let issue = make_packet(&candidate.plan, "issue", deps(&[root, closing, control])).unwrap();
    assert_eq!(
        issue.task.tool.as_ref().unwrap().request,
        control.result.artifacts["output"]
    );
    assert_eq!(
        issue.task.tool.as_ref().unwrap().source_bindings["drawing.receipt"],
        root.result.worker_receipt.as_ref().unwrap().logical_id
    );
    let parents = BTreeMap::from([
        (
            "drawing".into(),
            upstream::Parent {
                packet: &root.packet,
                result: &root.result,
            },
        ),
        (
            "closing".into(),
            upstream::Parent {
                packet: &closing.packet,
                result: &closing.result,
            },
        ),
        (
            "controls".into(),
            upstream::Parent {
                packet: &control.packet,
                result: &control.result,
            },
        ),
    ]);
    let bound = upstream::bind(&issue, &d.consumers["issue"].after, &parents).unwrap();
    assert_eq!(bound.len(), 4);
    assert_eq!(
        bound[&control.result.artifacts["output"].logical_id].result_sha256,
        canonical_digest(&control.result).unwrap()
    );
    assert_eq!(candidate.impact.unaffected, ["closing"]);
    assert_eq!(candidate.impact.contextual_review, *f.report.copy());
    assert_eq!(
        candidate.impact.rebuild,
        ["drawing", "controls", "issue", "review"]
    );
}
#[test]
fn derived_edges_refuse_stale_parent_context_wrong_origins_and_ambiguous_edits() {
    let f = fixture();
    let check = |r: &Revision| {
        materialize(&f.baseline, &f.report, &f.repair, r)
            .unwrap_err()
            .code
    };
    let mut bad = f.revision.clone();
    bad.derivation = None;
    assert_eq!(check(&bad), "repair-revision-derived-mode");
    let mut bad = f.revision.clone();
    bad.schema = "reel.workflow-repair-revision.v3".into();
    assert_eq!(check(&bad), "repair-revision-derived-mode");
    let mut bad = f.revision.clone();
    bad.derivation.as_mut().unwrap().parents.remove("closing");
    assert_eq!(check(&bad), "repair-derived-parent-set");
    let mut bad = f.revision.clone();
    let p = bad.derivation.as_ref().unwrap().parents["closing"].clone();
    bad.derivation
        .as_mut()
        .unwrap()
        .parents
        .insert("other".into(), p);
    assert_eq!(check(&bad), "repair-derived-parent-set");
    let mut bad = f.revision.clone();
    let b = bad.derivation.as_ref().unwrap().consumers["controls"].clone();
    bad.derivation
        .as_mut()
        .unwrap()
        .consumers
        .insert("closing".into(), b);
    assert_eq!(check(&bad), "repair-derived-consumer-scope");
    let mut bad = f.revision.clone();
    bad.derivation
        .as_mut()
        .unwrap()
        .consumers
        .get_mut("controls")
        .unwrap()
        .before
        .schema = "reel.workflow-upstream-inputs.v2".into();
    assert_eq!(check(&bad), "upstream-binding-content");
    let mut bad = f.revision.clone();
    let b = &mut bad
        .derivation
        .as_mut()
        .unwrap()
        .consumers
        .get_mut("controls")
        .unwrap()
        .after;
    for r in b.inputs.values_mut() {
        if matches!(r.artifact, upstream::Selector::WorkerReceipt) {
            r.artifact = upstream::Selector::Output {
                name: "output".into(),
            };
            break;
        }
    }
    assert_eq!(check(&bad), "repair-derived-selector-set");
    let mut bad = f.revision.clone();
    bad.derivation
        .as_mut()
        .unwrap()
        .parents
        .get_mut("drawing")
        .unwrap()
        .after
        .packet
        .project = "foreign".into();
    assert!(matches!(
        check(&bad).as_str(),
        "packet-key" | "packet-key-binding"
    ));
    let mut bad = f.revision.clone();
    let mut foreign = f.first.clone();
    foreign
        .task_snapshots
        .insert("drawing".into(), "d".repeat(40));
    let witness = &mut bad
        .derivation
        .as_mut()
        .unwrap()
        .parents
        .get_mut("drawing")
        .unwrap()
        .after;
    witness.packet = make_packet(&foreign, "drawing", BTreeMap::new()).unwrap();
    witness.result.task_key = witness.packet.task_key.clone();
    check_result(&witness.packet, &witness.result).unwrap();
    assert_eq!(check(&bad), "repair-derived-parent-packet");
    let old = &f.revision.derivation.as_ref().unwrap().parents["drawing"]
        .before
        .result
        .artifacts["output"];
    let mut overlapping = f.repair.clone();
    overlapping.edits.push(Edit {
        before: old.clone(),
        after: pin(&old.logical_id, b"manual replacement"),
    });
    assert_eq!(
        materialize(&f.baseline, &f.report, &overlapping, &f.revision)
            .unwrap_err()
            .code,
        "repair-derived-asset-edit-overlap"
    );
    let mut renamed = f.repair.clone();
    renamed.edits.push(Edit {
        before: old.clone(),
        after: pin("new-identity", b"manual replacement"),
    });
    assert_eq!(
        materialize(&f.baseline, &f.report, &renamed, &f.revision)
            .unwrap_err()
            .code,
        "review-repair-edit"
    );
}
#[test]
fn legacy_revisions_omit_derivation_and_derived_inputs_cannot_replace_runtime_code() {
    let f = fixture();
    for schema in [
        "reel.workflow-repair-revision.v1",
        "reel.workflow-repair-revision.v2",
        "reel.workflow-repair-revision.v3",
    ] {
        let mut r = f.revision.clone();
        r.schema = schema.into();
        r.derivation = None;
        let raw = serde_json::to_vec(&r).unwrap();
        let value: serde_json::Value = decode(&raw).unwrap();
        assert!(value.get("derivation").is_none());
        assert_eq!(
            serde_json::to_vec(&decode::<Revision>(&raw).unwrap()).unwrap(),
            raw
        );
    }
    let mut baseline = f.baseline.clone();
    let mut r = f.revision.clone();
    let d = r.derivation.as_mut().unwrap();
    d.consumers.remove("issue");
    d.parents.remove("controls");
    baseline.tasks[2].tool.as_mut().unwrap().executable =
        d.parents["drawing"].before.result.artifacts["output"].clone();
    assert_eq!(
        materialize(&baseline, &f.report, &f.repair, &r)
            .unwrap_err()
            .code,
        "repair-derived-runtime-input"
    );
}

#[test]
fn renamed_inputs_are_applied_simultaneously_and_fixed_input_collisions_refuse() {
    let f = fixture();
    let mut revision = f.revision.clone();
    let d = revision.derivation.as_mut().unwrap();
    d.consumers.remove("issue");
    d.parents.remove("controls");
    let draw = d.parents.get_mut("drawing").unwrap();
    let old_receipt = draw.before.result.worker_receipt.clone().unwrap();
    let old_output = draw.before.result.artifacts["output"].clone();
    draw.after.result.worker_receipt = Some(old_output.clone());
    draw.after
        .result
        .artifacts
        .insert("output".into(), old_receipt.clone());
    check_result(&draw.after.packet, &draw.after.result).unwrap();
    let b = &mut d.consumers.get_mut("controls").unwrap().after;
    b.inputs.retain(|_, r| r.task_id != "drawing");
    b.inputs.insert(
        old_output.logical_id.clone(),
        origin("drawing", upstream::Selector::WorkerReceipt),
    );
    b.inputs
        .insert(old_receipt.logical_id.clone(), output("drawing", "output"));
    let candidate = materialize(&f.baseline, &f.report, &f.repair, &revision).unwrap();
    let tool = candidate.plan.tasks[2].tool.as_ref().unwrap();
    assert_eq!(
        tool.source_bindings["drawing.receipt"],
        old_output.logical_id
    );
    assert_eq!(
        tool.source_bindings["drawing.packet"],
        old_receipt.logical_id
    );
    let mut collision = revision.clone();
    let d = collision.derivation.as_mut().unwrap();
    let draw = d.parents.get_mut("drawing").unwrap();
    draw.after
        .result
        .worker_receipt
        .as_mut()
        .unwrap()
        .logical_id = f.repair.edits[0].after.logical_id.clone();
    let b = &mut d.consumers.get_mut("controls").unwrap().after;
    b.inputs.remove(&old_output.logical_id);
    b.inputs.insert(
        f.repair.edits[0].after.logical_id.clone(),
        origin("drawing", upstream::Selector::WorkerReceipt),
    );
    assert_eq!(
        materialize(&f.baseline, &f.report, &f.repair, &collision)
            .unwrap_err()
            .code,
        "duplicate-input-identity"
    );
    let mut wrong_version = revision;
    wrong_version
        .derivation
        .as_mut()
        .unwrap()
        .consumers
        .get_mut("controls")
        .unwrap()
        .after
        .schema = "reel.workflow-upstream-inputs.v1".into();
    assert_eq!(
        materialize(&f.baseline, &f.report, &f.repair, &wrong_version)
            .unwrap_err()
            .code,
        "upstream-original-parent-result"
    );
}
