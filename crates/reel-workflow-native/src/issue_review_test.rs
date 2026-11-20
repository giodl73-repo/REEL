//! Read-only native parent borrowing and actual whole-issue control generation.
//! No child admission, retained control ownership, model viewing or approval.
use super::*;
use crate::{handoff, retention, upstream};
use reel_workflow::{publication::InputReader, *};
use std::{fs, io::Write, path::PathBuf};

#[test]
#[ignore = "Exact certified retained corrected issue only; borrow original native lineage and generate producer-bound full-context controls with original Index unchanged; no close, child admission, provider, retention or finding closure"]
fn corrected_retained_issue_emits_bound_context_controls_without_child_admission() {
    let env = |name| PathBuf::from(std::env::var_os(name).expect(name));
    let certificate = env("REEL_CORRECTED_CONTEXT_CERTIFICATE");
    let root = env("REEL_CORRECTED_CONTEXT_ISSUE");
    let evidence = env("REEL_CORRECTED_CONTEXT_EVIDENCE");
    let runtime = env("REEL_CORRECTED_CONTEXT_RUNTIME");
    let raw = fs::read(certificate.join("qualified-report.json")).unwrap();
    assert_eq!(
        digest(&raw),
        "6216438e6376d21237d98806350cc90643206f7edbd31af823831b51a960d0f4"
    );
    let certified: serde_json::Value = decode(&raw).unwrap();
    for pin in certified["files"].as_array().unwrap() {
        let raw = fs::read(pin["path"].as_str().unwrap()).unwrap();
        assert_eq!(raw.len() as u64, pin["bytes"].as_u64().unwrap());
        assert_eq!(digest(&raw), pin["sha256"].as_str().unwrap());
    }
    let load = |name: &str| fs::read(root.join(name)).unwrap();
    let parent: Packet = decode(&load("packet.json")).unwrap();
    let result: ReturnedResult = decode(&load("result.json")).unwrap();
    let profile: handoff::Profile = decode(&load("profile.json")).unwrap();
    let ack: retention::Acknowledgement = decode(&load("retained.json")).unwrap();
    let minimum = ack.index_checkpoint.clone();
    let mut parents: upstream::Parents = decode(&load("parents.json")).unwrap();
    assert_eq!(
        parent.task_key,
        "69d0d1a011e1a088c283dc64d3e29146a4be3c1a2b5fd931d8e334fb9c536ac9"
    );
    let checkpoint = || {
        tig_store::source_claims::protection::Index::open(&profile.admission.index_root, &minimum)
            .unwrap()
            .checkpoint()
    };
    let before = checkpoint();
    let inherited: reel_workflow::upstream::Bindings = decode(&load("upstream.json")).unwrap();
    let bindings = review_control::bindings(&parent, &result, Some(&inherited)).unwrap();
    parents.parents.insert(
        parent.task.id.clone(),
        upstream::ParentControl {
            packet: parent.clone(),
            result: result.clone(),
            profile: decode(&load("profile.json")).unwrap(),
            acknowledgement: ack,
        },
    );
    let mut source: NativeProfile = decode(&serde_json::to_vec(&profile.source).unwrap()).unwrap();
    source.snapshot.upstream = Some(upstream::Frozen::capture(
        &bindings,
        &parents.borrowed().unwrap(),
    ));
    let raw = fs::read(&runtime).unwrap();
    let candidate = Artifact {
        logical_id: "tools/corrected-issue-context-control".into(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
        media_type: "application/octet-stream".into(),
    };
    let mut task = parent.task.clone();
    task.id = "corrected-issue-review-controls".into();
    let declaration = source.snapshot.declaration_bytes().unwrap();
    task.native_inputs = Some(Artifact {
        logical_id: "control/native-input-bindings".into(),
        sha256: digest(&declaration),
        bytes: declaration.len() as u64,
        media_type: "application/json".into(),
    });
    let raw = bindings.canonical_bytes().unwrap();
    task.upstream_inputs = Some(Artifact {
        logical_id: "control/corrected-issue-review-origins".into(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    });
    task.inputs = review_control::inputs(&parent, &result).unwrap();
    task.depends_on = parents.parents.keys().cloned().collect();
    task.outputs = review_control::OUTPUTS
        .iter()
        .map(|name| {
            (
                (*name).into(),
                OutputContract {
                    media_type: "application/json".into(),
                    required: true,
                    max_bytes: MAX_CONTROL_BYTES as u64,
                },
            )
        })
        .collect();
    let tool = task.tool.as_mut().unwrap();
    tool.capability = ToolCapability::ComicReviewControl;
    tool.executable = candidate.clone();
    tool.request = result.worker_receipt.clone().unwrap();
    tool.source_bindings = review_control::bound_source_bindings(&parent, &result).unwrap();
    let mut plan: Plan = decode(&load("plan.json")).unwrap();
    plan.task_snapshots
        .insert(task.id.clone(), parent.source_commit.clone());
    plan.tasks.push(task);
    let dependencies = parents
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
        .collect();
    let child = make_packet(&plan, "corrected-issue-review-controls", dependencies).unwrap();
    drop(crate::SnapshotInputs::open(&child, &source.snapshot).unwrap());
    let mut borrowed =
        upstream::BorrowedInputs::open(&child, &bindings, parents.borrowed().unwrap()).unwrap();
    // These retained-parent fences cover the original producer while this
    // private read-only bridge uses its exact original snapshot, never a child
    // claim fabricated from metadata. Outputs are read by their exact selector.
    let mut inputs = Inputs {
        native: NativeInputs::open_inner(&parent, &profile.source, None).unwrap(),
        bytes: BTreeMap::new(),
    };
    for pin in result
        .artifacts
        .values()
        .chain(result.worker_receipt.iter())
    {
        inputs.bytes.insert(
            pin.logical_id.clone(),
            borrowed.read(pin, 64 * 1024 * 1024).unwrap(),
        );
    }
    let semantic = parents
        .parents
        .iter()
        .map(|(id, p)| {
            (
                id.clone(),
                reel_workflow::upstream::Parent {
                    packet: &p.packet,
                    result: &p.result,
                },
            )
        })
        .collect();
    let generated = review_control::generate_with_issue(
        &child,
        &bindings,
        &semantic,
        &mut inputs,
        capture_review_issue,
    )
    .unwrap();
    let copy: review::Copy = decode(&generated.files["review-copy.json"]).unwrap();
    let request: review::Request = decode(&generated.files["review-request.json"]).unwrap();
    assert_eq!(request.schema, "reel.workflow-review-request.v2");
    assert_eq!(request.production.as_ref().unwrap().packet, parent);
    assert_eq!(request.production.as_ref().unwrap().result, result);
    let retained_copy: review::Copy = decode(&load("retained-review-copy.json")).unwrap();
    assert_eq!(copy.units, retained_copy.units);
    assert_eq!(copy.format, retained_copy.format);
    let images =
        review::comic_reading_inputs(&copy, &generated.files["review-sources.json"]).unwrap();
    assert_eq!(images.len(), 2);
    let report_task = Task {
        id: "corrected-issue-context-report".into(),
        native_inputs: None,
        upstream_inputs: None,
        tool: None,
        operation: Operation::ReviewReport,
        language: parent.task.language.clone(),
        prompt: "Proposed whole corrected issue context; no viewing or approval is asserted."
            .into(),
        inputs: std::iter::once(copy.publication.clone())
            .chain(generated.outputs.values().cloned())
            .chain(images)
            .collect(),
        depends_on: vec![],
        outputs: BTreeMap::from([(
            "report".into(),
            OutputContract {
                media_type: "application/json".into(),
                required: true,
                max_bytes: MAX_CONTROL_BYTES as u64,
            },
        )]),
    };
    let report_plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: parent.project.clone(),
        source_commit: parent.source_commit.clone(),
        task_snapshots: Default::default(),
        tasks: vec![report_task],
    };
    let report_packet = make_packet(
        &report_plan,
        "corrected-issue-context-report",
        BTreeMap::new(),
    )
    .unwrap();
    for (name, pin) in &generated.outputs {
        inputs
            .bytes
            .insert(pin.logical_id.clone(), generated.files[name].clone());
    }
    let bound = review::bind(&report_packet, &mut inputs).unwrap();
    let report = review::Report {
        schema: "reel.workflow-review-report.v1".into(),
        copy_sha256: canonical_digest(&copy).unwrap(),
        findings: vec![],
    };
    let raw = serde_json::to_vec(&report).unwrap();
    let proposal = bound.pack_handoff("context-proposal-only", &raw).unwrap();
    bound
        .check(&proposal, &raw)
        .unwrap()
        .check_production_task(&plan, &parent.task.id)
        .unwrap();
    let mut admission = profile.admission.clone();
    admission.scope.operation =
        tig_store::Key::parse("corrected-issue-review-control-original").unwrap();
    let preview =
        crate::admission::Prepared::open(&child, &source, &admission, "corrected-review-attempt-1")
            .unwrap()
            .declared_preview()
            .unwrap();
    assert_eq!(before, checkpoint());
    let write = |name: &str, raw: &[u8]| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(evidence.join(name))
            .unwrap();
        file.write_all(raw).unwrap();
        file.sync_all().unwrap();
    };
    for (name, raw) in [
        ("packet.json", serde_json::to_vec(&child).unwrap()),
        ("plan.json", serde_json::to_vec(&plan).unwrap()),
        ("source.json", serde_json::to_vec(&source).unwrap()),
        ("parents.json", serde_json::to_vec(&parents).unwrap()),
        ("upstream.json", bindings.canonical_bytes().unwrap()),
        ("admission.json", serde_json::to_vec(&admission).unwrap()),
        (
            "declared-preview.json",
            serde_json::to_vec(&preview).unwrap(),
        ),
        (
            "report-proposal.json",
            serde_json::to_vec(&report_packet).unwrap(),
        ),
    ] {
        write(name, &raw);
    }
    for (name, raw) in &generated.files {
        write(name, raw);
    }
    write("qualification-result.json", &serde_json::to_vec(&serde_json::json!({"schema":"reel.corrected-issue-bound-context-preflight.v1","original_issue":parent.task_key,"original_runtime":profile.runtime,"prospective_runtime":candidate,"control_task_key":child.task_key,"attempt_id":"corrected-review-attempt-1","operation":admission.scope.operation,"checkpoint_sha256":canonical_digest(&before).unwrap(),"actual_index_unchanged":true,"native_lineage_borrowed":true,"original_issue_replayed":true,"producer_bound_request":true,"pages":2,"units":6,"reading_images_declared":2,"requires_original_issue_close":true,"child_admitted":false,"controls_retained":false,"report_retained":false,"viewing_verified":false,"finding_closed":false,"selected":false,"approved":false})).unwrap());
}

#[test]
#[ignore = "Explicit retained synthetic issue only; borrow original native lineage and replay into prospective owned controls, no child admission or lifecycle effects"]
fn retained_issue_generates_complete_review_task_inputs_without_admission() {
    let root = PathBuf::from(
        std::env::var_os("REEL_ISSUE_REVIEW_CONTROL_ROOT").expect("retained issue root"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_ISSUE_REVIEW_CONTROL_EVIDENCE").expect("new evidence directory"),
    );
    let runtime = PathBuf::from(
        std::env::var_os("REEL_ISSUE_REVIEW_CONTROL_RUNTIME").expect("frozen candidate runtime"),
    );
    assert!(
        root.is_absolute() && evidence.is_absolute() && evidence.is_dir() && runtime.is_absolute()
    );
    let load = |name: &str| fs::read(root.join(name)).unwrap();
    let parent: Packet = decode(&load("issue-packet.json")).unwrap();
    let produced: ReturnedResult = decode(&load("issue-result.json")).unwrap();
    let profile: handoff::Profile = decode(&load("issue-profile.json")).unwrap();
    let ack: retention::Acknowledgement = decode(&load("issue-retained.json")).unwrap();
    assert_eq!(parent.project, "synthetic-comic");
    assert_eq!(parent.task.id, "issue-assemble");
    let before = || {
        let index = tig_store::source_claims::protection::Index::open(
            &profile.admission.index_root,
            &profile.binding.admission.index_checkpoint,
        )
        .unwrap();
        canonical_digest(&index.checkpoint()).unwrap()
    };
    let checkpoint = before();
    let bindings = reel_workflow::upstream::Bindings {
        schema: "reel.workflow-upstream-inputs.v1".into(),
        streaming: None,
        inputs: produced
            .artifacts
            .iter()
            .map(|(name, pin)| {
                (
                    pin.logical_id.clone(),
                    reel_workflow::upstream::Reference {
                        task_id: parent.task.id.clone(),
                        artifact: reel_workflow::upstream::Selector::Output { name: name.clone() },
                    },
                )
            })
            .chain(produced.worker_receipt.iter().map(|pin| {
                (
                    pin.logical_id.clone(),
                    reel_workflow::upstream::Reference {
                        task_id: parent.task.id.clone(),
                        artifact: reel_workflow::upstream::Selector::WorkerReceipt,
                    },
                )
            }))
            .collect(),
    };
    let binding_raw = bindings.canonical_bytes().unwrap();
    let runtime_raw = fs::read(&runtime).unwrap();
    let mut task = parent.task.clone();
    task.id = "whole-issue-review-controls".into();
    task.native_inputs = None;
    task.upstream_inputs = Some(Artifact {
        logical_id: "control/whole-issue-review-origins".into(),
        sha256: digest(&binding_raw),
        bytes: binding_raw.len() as u64,
        media_type: "application/json".into(),
    });
    task.depends_on = vec![parent.task.id.clone()];
    task.inputs = review_control::inputs(&parent, &produced).unwrap();
    task.outputs = review_control::OUTPUTS
        .iter()
        .map(|name| {
            (
                (*name).into(),
                OutputContract {
                    media_type: "application/json".into(),
                    required: true,
                    max_bytes: MAX_CONTROL_BYTES as u64,
                },
            )
        })
        .collect();
    let tool = task.tool.as_mut().unwrap();
    tool.capability = ToolCapability::ComicReviewControl;
    tool.executable = Artifact {
        logical_id: "tools/whole-issue-review-candidate".into(),
        sha256: digest(&runtime_raw),
        bytes: runtime_raw.len() as u64,
        media_type: "application/octet-stream".into(),
    };
    tool.request = produced.worker_receipt.clone().unwrap();
    tool.source_bindings = review_control::source_bindings(&parent, &produced).unwrap();
    let mut plan: Plan = decode(&load("workflow-plan.json")).unwrap();
    assert_eq!(
        plan.tasks.iter().find(|t| t.id == parent.task.id),
        Some(&parent.task)
    );
    plan.tasks.push(task);
    let child = make_packet(
        &plan,
        "whole-issue-review-controls",
        BTreeMap::from([(
            parent.task.id.clone(),
            DependencyResult {
                result_sha256: canonical_digest(&produced).unwrap(),
                artifacts: produced.artifacts.clone(),
            },
        )]),
    )
    .unwrap();
    let mut borrowed = upstream::BorrowedInputs::open(
        &child,
        &bindings,
        BTreeMap::from([(
            parent.task.id.clone(),
            upstream::Parent {
                packet: &parent,
                result: &produced,
                source: &profile.source,
                admission: &profile.admission,
                receipt: &profile.binding.admission,
                acknowledgement: &ack,
            },
        )]),
    )
    .unwrap();
    // The live borrowed parent session fences retained coverage. NativeInputs
    // checks the original parent's exact snapshot/records, not a rewritten task.
    let mut inputs = Inputs {
        native: NativeInputs::open_inner(&parent, &profile.source, None).unwrap(),
        bytes: BTreeMap::new(),
    };
    for asset in produced
        .artifacts
        .values()
        .chain(produced.worker_receipt.iter())
    {
        let raw = borrowed.read(asset, 64 * 1024 * 1024).unwrap();
        inputs.bytes.insert(asset.logical_id.clone(), raw);
    }
    let parents = BTreeMap::from([(
        parent.task.id.clone(),
        reel_workflow::upstream::Parent {
            packet: &parent,
            result: &produced,
        },
    )]);
    let generated = review_control::generate_with_issue(
        &child,
        &bindings,
        &parents,
        &mut inputs,
        capture_review_issue,
    )
    .unwrap();
    let copy: review::Copy = decode(&generated.files["review-copy.json"]).unwrap();
    let request: review::Request = decode(&generated.files["review-request.json"]).unwrap();
    let sources: serde_json::Value = decode(&generated.files["review-sources.json"]).unwrap();
    assert_eq!(copy.units.len(), 6);
    assert_eq!(
        copy.format,
        review::Format::Comic {
            page_order: vec!["page01".into(), "page02".into()]
        }
    );
    assert_eq!(copy.review, generated.outputs["review-sources.json"]);
    assert_eq!(request.production_commit, parent.source_commit);
    assert_eq!(
        request.copy_input,
        generated.outputs["review-copy.json"].logical_id
    );
    for (page, part) in [("page01", "opening"), ("page02", "closing")] {
        assert_eq!(
            sources["reading"][page]["logical_id"],
            format!("synthetic-issue/es/parts/{part}/{page}-reading.png")
        );
    }
    let report_task = Task {
        id: "whole-issue-review".into(),
        native_inputs: None,
        upstream_inputs: None,
        operation: Operation::ReviewReport,
        tool: None,
        language: parent.task.language.clone(),
        prompt: "Synthetic whole issue review request; no provider call.".into(),
        inputs: std::iter::once(copy.publication.clone())
            .chain(generated.outputs.values().cloned())
            .collect(),
        depends_on: vec![],
        outputs: BTreeMap::from([(
            "report".into(),
            OutputContract {
                media_type: "application/json".into(),
                required: true,
                max_bytes: MAX_CONTROL_BYTES as u64,
            },
        )]),
    };
    for (name, pin) in &generated.outputs {
        inputs
            .bytes
            .insert(pin.logical_id.clone(), generated.files[name].clone());
    }
    let report_plan = Plan {
        schema: PLAN_SCHEMA.into(),
        task_snapshots: Default::default(),
        project: parent.project.clone(),
        source_commit: parent.source_commit.clone(),
        tasks: vec![report_task],
    };
    let report_packet = make_packet(&report_plan, "whole-issue-review", BTreeMap::new()).unwrap();
    review::bind(&report_packet, &mut inputs).unwrap();
    // This uses the exact same callback as native production/replay. A changed
    // original receipt cannot be excused by a successful deterministic render.
    let mut wrong = produced.clone();
    let receipt = wrong.worker_receipt.as_mut().unwrap();
    let mut declared: reel_publication::comic_issue_source::AssemblyReceipt =
        decode(&inputs.bytes[&receipt.logical_id]).unwrap();
    declared.page_order.reverse();
    let changed = serde_json::to_vec(&declared).unwrap();
    receipt.sha256 = digest(&changed);
    receipt.bytes = changed.len() as u64;
    inputs.bytes.insert(receipt.logical_id.clone(), changed);
    assert_eq!(
        capture_review_issue(&parent, &wrong, &mut inputs)
            .err()
            .unwrap()
            .code,
        "review-issue-receipt-replay"
    );
    assert_eq!(checkpoint, before());
    let write_new = |name: &str, raw: &[u8]| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(evidence.join(name))
            .unwrap();
        file.write_all(raw).unwrap();
        file.sync_all().unwrap();
    };
    write_new("packet.json", &serde_json::to_vec(&child).unwrap());
    write_new(
        "report-packet.json",
        &serde_json::to_vec(&report_packet).unwrap(),
    );
    for (name, raw) in &generated.files {
        write_new(name, raw)
    }
    write_new("result.json",&serde_json::to_vec(&serde_json::json!({"schema":"reel.native-issue-review-control-qualification.v1","parent_task_key":parent.task_key,"child_task_key":child.task_key,"units":copy.units.len(),"pages":2,"actual_index_checkpoint_unchanged":true,"native_parent_verified":true,"producer_replayed":true,"child_admitted":false,"controls_retained":false,"viewing_verified":false,"approved":false,"selected":false})).unwrap());
}
