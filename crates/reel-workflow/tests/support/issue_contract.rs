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
