use super::*;
use reel_publication::comic_issue_source::{Assembly, AssemblyReceipt, Part, Plan as IssuePlan};

fn fixture() -> (Plan, Memory, AssemblyReceipt) {
    let (mut workflow, mut memory, mut receipt) = build();
    let task = &mut workflow.tasks[0];
    let original = task.tool.as_ref().unwrap().clone();
    let mut publication: Publication =
        serde_json::from_slice(&memory.bytes[&original.publication.logical_id]).unwrap();
    publication.editions[0]
        .outputs
        .retain(|name, _| name == "reading.pdf" || name == "transcript.json");
    let master = memory.put(
        &original.publication.logical_id,
        "application/json",
        serde_json::to_vec(&publication).unwrap(),
    );
    let plan = IssuePlan {
        schema: "reel.comic-issue-plan.v1".into(),
        publication,
        edition_id: "es".into(),
        parts: vec![Part {
            id: "opening".into(),
            pages: vec!["page01".into()],
        }],
    };
    let plan_asset = memory.put(
        "issue/plan",
        "application/json",
        serde_json::to_vec(&plan).unwrap(),
    );
    let mut request: Request =
        serde_json::from_slice(&memory.bytes[&original.request.logical_id]).unwrap();
    request.part_id = Some("opening".into());
    let part_request = memory.put(
        &original.request.logical_id,
        "application/json",
        serde_json::to_vec(&request).unwrap(),
    );
    receipt.part_id = Some("opening".into());
    receipt.request = object(&part_request);
    receipt
        .source_files
        .insert("request.json".into(), object(&part_request));
    for (name, artifact) in &mut receipt.outputs {
        artifact.logical_id = format!("issue-demo/es/parts/opening/{name}");
        memory.put(
            &artifact.logical_id,
            &artifact.media_type,
            format!("synthetic output {name}").into_bytes(),
        );
    }
    let part_receipt = memory.put(
        "part/opening/receipt",
        "application/json",
        serde_json::to_vec(&receipt).unwrap(),
    );
    let assembly = Assembly {
        composition: None,
        bundles: BTreeMap::new(),
        schema: "reel.comic-issue-assembly.v1".into(),
        plan: pin("plan.json", &plan_asset),
        parts: BTreeMap::from([(
            "opening".into(),
            pin("parts/opening/receipt.json", &part_receipt),
        )]),
        limits: Limits {
            input_bytes: 1_000_000,
            output_bytes: 1_000_000,
        },
    };
    let assembly_asset = memory.put(
        "issue/request",
        "application/json",
        serde_json::to_vec(&assembly).unwrap(),
    );
    let mut paths = BTreeMap::from([
        ("plan.json".into(), plan_asset.logical_id.clone()),
        (
            "parts/opening/receipt.json".into(),
            part_receipt.logical_id.clone(),
        ),
    ]);
    let mut inputs = BTreeMap::from([
        (master.logical_id.clone(), master.clone()),
        (plan_asset.logical_id.clone(), plan_asset.clone()),
        (part_receipt.logical_id.clone(), part_receipt.clone()),
        (assembly_asset.logical_id.clone(), assembly_asset.clone()),
    ]);
    for (path, obj) in &receipt.source_files {
        let id = if path == "request.json" {
            &part_request.logical_id
        } else {
            &original.source_bindings[path]
        };
        let raw = memory.bytes[id].clone();
        let artifact = Artifact {
            logical_id: id.clone(),
            sha256: digest(&raw),
            bytes: raw.len() as u64,
            media_type: if path == "request.json" {
                "application/json".into()
            } else {
                task.inputs
                    .iter()
                    .find(|a| &a.logical_id == id)
                    .unwrap()
                    .media_type
                    .clone()
            },
        };
        assert_eq!(object(&artifact), *obj);
        paths.insert(format!("parts/opening/sources/{path}"), id.clone());
        inputs.insert(id.clone(), artifact);
    }
    for (name, a) in &receipt.outputs {
        paths.insert(format!("parts/opening/{name}"), a.logical_id.clone());
        inputs.insert(a.logical_id.clone(), a.clone());
    }
    task.tool = Some(ToolContract {
        auxiliary_executables: BTreeMap::new(),
        schema: original.schema,
        capability: ToolCapability::ComicIssueAssembly,
        executable: original.executable,
        publication: master,
        edition_id: "es".into(),
        request: assembly_asset.clone(),
        source_bindings: paths,
        part: None,
    });
    task.inputs = inputs.into_values().collect();
    task.outputs = plan.publication.editions[0].outputs.clone();
    let outputs = receipt
        .outputs
        .iter()
        .filter(|(name, _)| name.as_str() == "reading.pdf" || name.as_str() == "transcript.json")
        .map(|(name, a)| {
            let mut a = a.clone();
            a.logical_id = format!("issue-demo/es/{name}");
            (name.clone(), a)
        })
        .collect();
    let expected = AssemblyReceipt {
        schema: "reel.comic-issue-receipt.v1".into(),
        request: object(&assembly_asset),
        plan: object(&plan_asset),
        parts: BTreeMap::from([("opening".into(), object(&part_receipt))]),
        page_order: vec!["page01".into()],
        outputs,
        human_approval: false,
        retained_owner_acknowledged: false,
        print_ready: false,
    };
    (workflow, memory, expected)
}
#[test]
fn complete_issue_binds_every_editable_source_and_checks_original_receipt() {
    let (workflow, mut memory, receipt) = fixture();
    let packet = packet(&workflow);
    let bound = reel_workflow::issue::bind(&packet, &mut memory).unwrap();
    assert_eq!(memory.reads.len(), packet.task.inputs.len());
    let raw = serde_json::to_vec(&receipt).unwrap();
    let result = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: "issue-attempt".into(),
        artifacts: receipt.outputs.clone(),
        provenance: Provenance {
            mode: Mode::Tool,
            provider: None,
            model: None,
            executable: packet.task.tool.as_ref().map(|t| t.executable.clone()),
        },
        worker_receipt: Some(Artifact {
            logical_id: "issue/worker-receipt".into(),
            sha256: digest(&raw),
            bytes: raw.len() as u64,
            media_type: "application/json".into(),
        }),
    };
    reel_workflow::issue::check_receipt(&bound, &result, &raw).unwrap();
    let mut changed = receipt;
    changed.page_order.reverse();
    changed.page_order.push("page02".into());
    let raw = serde_json::to_vec(&changed).unwrap();
    let mut result = result;
    let pin = result.worker_receipt.as_mut().unwrap();
    pin.sha256 = digest(&raw);
    pin.bytes = raw.len() as u64;
    assert_eq!(
        reel_workflow::issue::check_receipt(&bound, &result, &raw)
            .unwrap_err()
            .code,
        "issue-receipt-correspondence"
    );
}

#[test]
fn versioned_issue_binds_intent_to_project_task_and_named_input() {
    use reel_publication::{
        comic_issue_source::Composition,
        composition::{Edition, Format, Intent},
    };
    let (mut workflow, mut memory, _) = fixture();
    let task = &mut workflow.tasks[0];
    let tool = task.tool.as_mut().unwrap();
    let mut assembly: Assembly =
        serde_json::from_slice(&memory.bytes[&tool.request.logical_id]).unwrap();
    let mut intent = Intent {
        schema: "reel.workflow-composition-intent.v1".into(),
        project: workflow.project.clone(),
        format: Format::ComicIssue,
        editions: BTreeMap::from([(
            "es".into(),
            Edition {
                language: "es".into(),
                components: vec!["draw-opening".into()],
                controls: vec![],
                final_task: task.id.clone(),
            },
        )]),
    };
    let intent_asset = memory.put(
        "issue/intent",
        "application/json",
        serde_json::to_vec(&intent).unwrap(),
    );
    assembly.schema = "reel.comic-issue-assembly.v2".into();
    assembly.composition = Some(Composition {
        intent: pin("intent.json", &intent_asset),
        producers: BTreeMap::from([("opening".into(), "draw-opening".into())]),
    });
    tool.source_bindings
        .insert("intent.json".into(), intent_asset.logical_id.clone());
    task.inputs.push(intent_asset);
    let request = memory.put(
        &tool.request.logical_id,
        "application/json",
        serde_json::to_vec(&assembly).unwrap(),
    );
    task.inputs
        .iter_mut()
        .filter(|a| a.logical_id == request.logical_id)
        .for_each(|a| *a = request.clone());
    tool.request = request;
    let bound = reel_workflow::issue::bind(&packet(&workflow), &mut memory).unwrap();
    let origins = bound.composition_origins().unwrap();
    assert_eq!(
        origins.parents,
        std::collections::BTreeSet::from(["draw-opening".into()])
    );
    assert_eq!(
        origins.inputs["part/opening/receipt"].task_id,
        "draw-opening"
    );
    assert_eq!(
        origins.inputs["part/opening/receipt"].artifact,
        reel_workflow::upstream::Selector::WorkerReceipt
    );
    for change in ["project", "task", "language"] {
        let mut bad_workflow = workflow.clone();
        let mut bad_memory = memory.clone();
        let original = serde_json::to_vec(&intent).unwrap();
        match change {
            "project" => intent.project = "another-project".into(),
            "task" => intent.editions.get_mut("es").unwrap().final_task = "another-task".into(),
            _ => intent.editions.get_mut("es").unwrap().language = "en".into(),
        }
        let asset = bad_memory.put(
            "issue/intent",
            "application/json",
            serde_json::to_vec(&intent).unwrap(),
        );
        let mut request = assembly.clone();
        request.composition.as_mut().unwrap().intent = pin("intent.json", &asset);
        let task = &mut bad_workflow.tasks[0];
        let tool = task.tool.as_mut().unwrap();
        let request_asset = bad_memory.put(
            &tool.request.logical_id,
            "application/json",
            serde_json::to_vec(&request).unwrap(),
        );
        for a in &mut task.inputs {
            if a.logical_id == asset.logical_id {
                *a = asset.clone();
            }
            if a.logical_id == request_asset.logical_id {
                *a = request_asset.clone();
            }
        }
        tool.request = request_asset;
        assert!(reel_workflow::issue::bind(&packet(&bad_workflow), &mut bad_memory).is_err());
        intent = serde_json::from_slice(&original).unwrap();
    }
    workflow.tasks[0]
        .tool
        .as_mut()
        .unwrap()
        .source_bindings
        .remove("intent.json");
    assert_eq!(
        reel_workflow::issue::bind(&packet(&workflow), &mut memory)
            .unwrap_err()
            .code,
        "issue-source-binding"
    );
}
#[test]
fn complete_issue_rejects_unused_inputs_and_same_byte_output_aliases() {
    let (mut workflow, mut memory, _) = fixture();
    workflow.tasks[0].inputs.push(memory.put(
        "unrelated/source",
        "text/plain",
        b"unrelated".to_vec(),
    ));
    assert_eq!(
        reel_workflow::issue::bind(&packet(&workflow), &mut memory)
            .unwrap_err()
            .code,
        "issue-unused-input"
    );
    let (mut workflow, mut memory, _) = fixture();
    let task = &mut workflow.tasks[0];
    let tool = task.tool.as_mut().unwrap();
    let old = tool.source_bindings["parts/opening/reading.pdf"].clone();
    let artifact = task
        .inputs
        .iter_mut()
        .find(|a| a.logical_id == old)
        .unwrap();
    artifact.logical_id = "alias/reading".into();
    memory
        .bytes
        .insert(artifact.logical_id.clone(), memory.bytes[&old].clone());
    tool.source_bindings.insert(
        "parts/opening/reading.pdf".into(),
        artifact.logical_id.clone(),
    );
    assert_eq!(
        reel_workflow::issue::bind(&packet(&workflow), &mut memory)
            .unwrap_err()
            .code,
        "issue-part-output-logical-binding"
    );
}

fn packed_fixture() -> (Plan, Memory) {
    let (mut workflow, mut memory, _) = fixture();
    let task = &mut workflow.tasks[0];
    let tool = task.tool.as_mut().unwrap();
    let receipt_id = tool.source_bindings["parts/opening/receipt.json"].clone();
    let mut receipt: Receipt = serde_json::from_slice(&memory.bytes[&receipt_id]).unwrap();
    // This is metadata correspondence only; native production must decode/replay.
    let carrier = memory.put(
        "issue-demo/es/parts/opening/editable.packet",
        "application/vnd.reel.editable-packet.v1",
        b"synthetic opaque carrier".to_vec(),
    );
    receipt
        .outputs
        .insert("editable.packet".into(), carrier.clone());
    let receipt_asset = memory.put(
        &receipt_id,
        "application/json",
        serde_json::to_vec(&receipt).unwrap(),
    );
    let mut assembly: Assembly =
        serde_json::from_slice(&memory.bytes[&tool.request.logical_id]).unwrap();
    assembly.parts.insert(
        "opening".into(),
        pin("parts/opening/receipt.json", &receipt_asset),
    );
    assembly.bundles.insert(
        "opening".into(),
        pin("parts/opening/editable.packet", &carrier),
    );
    let request = memory.put(
        &tool.request.logical_id,
        "application/json",
        serde_json::to_vec(&assembly).unwrap(),
    );
    tool.source_bindings
        .retain(|path, _| path == "plan.json" || path == "parts/opening/receipt.json");
    tool.source_bindings.insert(
        "parts/opening/editable.packet".into(),
        carrier.logical_id.clone(),
    );
    let plan = task
        .inputs
        .iter()
        .find(|a| a.logical_id == tool.source_bindings["plan.json"])
        .unwrap()
        .clone();
    task.inputs = vec![
        tool.publication.clone(),
        plan,
        request.clone(),
        receipt_asset,
        carrier,
    ];
    tool.request = request;
    (workflow, memory)
}
#[test]
fn packed_issue_declares_carrier_and_receipt_without_undeclared_loose_reads() {
    let (workflow, mut memory) = packed_fixture();
    reel_workflow::issue::bind(&packet(&workflow), &mut memory).unwrap();
    assert_eq!(memory.reads.len(), 5);
    assert!(!memory.reads.contains_key("input/font"));
    assert!(!memory.reads.contains_key("input/image"));
}
#[test]
fn packed_issue_rejects_same_byte_carrier_with_different_logical_origin() {
    let (mut workflow, mut memory) = packed_fixture();
    let task = &mut workflow.tasks[0];
    let tool = task.tool.as_mut().unwrap();
    let id = tool.source_bindings["parts/opening/editable.packet"].clone();
    let carrier = task.inputs.iter_mut().find(|a| a.logical_id == id).unwrap();
    carrier.logical_id = "alias/carrier".into();
    memory
        .bytes
        .insert(carrier.logical_id.clone(), memory.bytes[&id].clone());
    tool.source_bindings.insert(
        "parts/opening/editable.packet".into(),
        carrier.logical_id.clone(),
    );
    assert_eq!(
        reel_workflow::issue::bind(&packet(&workflow), &mut memory)
            .unwrap_err()
            .code,
        "issue-part-bundle-binding"
    );
}

fn control_fixture() -> (Plan, Memory, Vec<u8>) {
    let (mut workflow, mut memory) = packed_fixture();
    let task = &mut workflow.tasks[0];
    let tool = task.tool.as_mut().unwrap();
    let original = memory.bytes[&tool.request.logical_id].clone();
    let assembly: Assembly = serde_json::from_slice(&original).unwrap();
    let request = reel_workflow::issue_control::Request {
        schema: "reel.workflow-issue-control.v1".into(),
        plan_path: assembly.plan.path,
        parts: BTreeMap::from([(
            "opening".into(),
            reel_workflow::issue_control::PartPaths {
                receipt: assembly.parts["opening"].path.clone(),
                bundle: assembly.bundles["opening"].path.clone(),
            },
        )]),
        limits: assembly.limits,
    };
    let control = memory.put(
        "issue/control-request",
        "application/json",
        serde_json::to_vec(&request).unwrap(),
    );
    task.inputs.retain(|a| a != &tool.request);
    task.inputs.push(control.clone());
    tool.request = control;
    tool.capability = ToolCapability::ComicIssueControl;
    task.outputs = BTreeMap::from([(
        "assembly.json".into(),
        reel_publication::OutputContract {
            media_type: "application/json".into(),
            required: true,
            max_bytes: MAX_CONTROL_BYTES as u64,
        },
    )]);
    (workflow, memory, original)
}

#[test]
fn derived_control_produces_exact_assembly_then_binds_as_an_ordinary_input() {
    let (workflow, mut memory, original) = control_fixture();
    let packet = packet(&workflow);
    let generated = reel_workflow::issue_control::generate(&packet, &mut memory).unwrap();
    assert_eq!(generated.files["assembly.json"], original);
    assert_eq!(generated.outputs["assembly.json"].sha256, digest(&original));
    assert!(
        generated.outputs["assembly.json"]
            .logical_id
            .contains(&packet.task_key)
    );
    let mut assembly_workflow = workflow.clone();
    let task = &mut assembly_workflow.tasks[0];
    let tool = task.tool.as_mut().unwrap();
    task.inputs.retain(|a| a != &tool.request);
    let a = generated.outputs["assembly.json"].clone();
    memory.bytes.insert(a.logical_id.clone(), original);
    tool.request = a.clone();
    tool.capability = ToolCapability::ComicIssueAssembly;
    task.inputs.push(a);
    let master: Publication =
        serde_json::from_slice(&memory.bytes[&tool.publication.logical_id]).unwrap();
    task.outputs = master.editions[0].outputs.clone();
    reel_workflow::issue::bind(&super::packet(&assembly_workflow), &mut memory).unwrap();
}

#[test]
fn derived_control_refuses_changed_carrier_bytes_and_wrong_scoped_origin() {
    let (workflow, mut memory, _) = control_fixture();
    let id = "issue-demo/es/parts/opening/editable.packet";
    memory.bytes.get_mut(id).unwrap().push(0);
    assert_eq!(
        reel_workflow::issue_control::generate(&packet(&workflow), &mut memory)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Integrity
    );
    let (mut workflow, mut memory, _) = control_fixture();
    let task = &mut workflow.tasks[0];
    let tool = task.tool.as_mut().unwrap();
    let a = task.inputs.iter_mut().find(|a| a.logical_id == id).unwrap();
    a.logical_id = "alias/carrier".into();
    memory
        .bytes
        .insert(a.logical_id.clone(), memory.bytes[id].clone());
    tool.source_bindings
        .insert("parts/opening/editable.packet".into(), a.logical_id.clone());
    assert_eq!(
        reel_workflow::issue_control::generate(&packet(&workflow), &mut memory)
            .err()
            .unwrap()
            .code,
        "issue-control-part-correspondence"
    );
}

#[test]
fn derived_control_recipe_does_not_depend_on_future_part_hashes() {
    let (mut workflow, mut memory, _) = control_fixture();
    let request_before = workflow.tasks[0].tool.as_ref().unwrap().request.clone();
    let task = &mut workflow.tasks[0];
    let tool = task.tool.as_mut().unwrap();
    let receipt_id = tool.source_bindings["parts/opening/receipt.json"].clone();
    let carrier_id = tool.source_bindings["parts/opening/editable.packet"].clone();
    let next = memory.put(
        &carrier_id,
        "application/vnd.reel.editable-packet.v1",
        b"new unknown part bytes".to_vec(),
    );
    let mut receipt: Receipt = serde_json::from_slice(&memory.bytes[&receipt_id]).unwrap();
    receipt
        .outputs
        .insert("editable.packet".into(), next.clone());
    let next_receipt = memory.put(
        &receipt_id,
        "application/json",
        serde_json::to_vec(&receipt).unwrap(),
    );
    for a in &mut task.inputs {
        if a.logical_id == carrier_id {
            *a = next.clone();
        }
        if a.logical_id == receipt_id {
            *a = next_receipt.clone();
        }
    }
    assert_eq!(tool.request, request_before);
    let generated =
        reel_workflow::issue_control::generate(&packet(&workflow), &mut memory).unwrap();
    let assembly: Assembly = serde_json::from_slice(&generated.files["assembly.json"]).unwrap();
    assert_eq!(assembly.bundles["opening"].sha256, next.sha256);
    assert_eq!(assembly.parts["opening"].sha256, next_receipt.sha256);
}
