//! Read-only native parent borrowing and actual whole-issue control generation.
//! No child admission, retained control ownership, model viewing or approval.
use super::*;
use crate::{handoff, retention, upstream};
use reel_workflow::{publication::InputReader, *};
use std::{fs, io::Write, path::PathBuf};

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
