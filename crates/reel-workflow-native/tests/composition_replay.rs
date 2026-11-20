//! Explicit local replay of previously exported synthetic parts. No native
//! admission, claim changes, runtime adoption, provider or publication effects.
use reel_publication::{
    comic_issue_source::{Assembly, Composition, Plan as IssuePlan},
    comic_source::Pin,
    composition::{Edition, Format, Intent},
};
use reel_workflow::{publication::InputReader, *};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::Path,
};

#[derive(Default)]
struct Inputs(BTreeMap<String, Vec<u8>>);
impl Inputs {
    fn put(&mut self, id: &str, raw: Vec<u8>, media: &str) -> Artifact {
        let asset = Artifact {
            logical_id: id.into(),
            sha256: digest(&raw),
            bytes: raw.len() as u64,
            media_type: media.into(),
        };
        assert!(self.0.insert(id.into(), raw).is_none());
        asset
    }
}
impl InputReader for Inputs {
    fn read(&mut self, asset: &Artifact, maximum: u64) -> Result<Vec<u8>> {
        let raw = &self.0[&asset.logical_id];
        assert!(raw.len() as u64 <= maximum);
        Ok(raw.clone())
    }
}
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    let raw = bounded(path, MAX_CONTROL_BYTES as u64);
    serde_json::from_slice(&raw).unwrap()
}
fn bounded(path: &Path, maximum: u64) -> Vec<u8> {
    let file = fs::File::open(path).unwrap();
    assert!(file.metadata().unwrap().len() <= maximum);
    let mut raw = Vec::new();
    file.take(maximum + 1).read_to_end(&mut raw).unwrap();
    assert!(raw.len() as u64 <= maximum);
    raw
}
fn pin(path: &str, a: &Artifact) -> Pin {
    Pin {
        path: path.into(),
        sha256: a.sha256.clone(),
        bytes: a.bytes,
    }
}
fn write(path: &Path, raw: &[u8]) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(raw).unwrap();
    file.sync_all().unwrap();
}

#[test]
#[ignore = "Explicit exported synthetic issue only; local v2 control generation and deterministic part replay, no native admission or lifecycle effects"]
fn generated_v2_control_replays_actual_exported_parts_and_preserves_reading_outputs() {
    let locations =
        std::env::var_os("REEL_COMPOSITION_REPLAY_LOCATIONS").expect("explicit locations control");
    let evidence = std::path::PathBuf::from(
        std::env::var_os("REEL_COMPOSITION_REPLAY_EVIDENCE").expect("new evidence directory"),
    );
    assert!(evidence.is_absolute() && evidence.is_dir());
    let locations: serde_json::Value = read(Path::new(&locations));
    let root = Path::new(locations["root"].as_str().unwrap());
    let exported = Path::new(locations["exported"].as_str().unwrap());
    let original: Packet = read(&root.join("control-packet.json"));
    check_packet(&original).unwrap();
    let original_final: Packet = read(&root.join("issue-packet.json"));
    check_packet(&original_final).unwrap();
    let mut workflow: Plan = read(&root.join("control-plan.json"));
    let assembly: Assembly = read(&exported.join("assembly.json"));
    assembly.check_version().unwrap();
    assert_eq!(assembly.plan.path, "issue-plan.json");
    let plan_raw = bounded(
        &exported.join(&assembly.plan.path),
        MAX_CONTROL_BYTES as u64,
    );
    assert_eq!(digest(&plan_raw), assembly.plan.sha256);
    assert_eq!(plan_raw.len() as u64, assembly.plan.bytes);
    let issue: IssuePlan = serde_json::from_slice(&plan_raw).unwrap();
    issue.check().unwrap();
    let components = issue.parts.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
    assert_eq!(
        components.iter().collect::<std::collections::BTreeSet<_>>(),
        original.dependencies.keys().collect()
    );
    let intent = Intent {
        schema: "reel.workflow-composition-intent.v1".into(),
        project: original.project.clone(),
        format: Format::ComicIssue,
        editions: BTreeMap::from([(
            issue.edition_id.clone(),
            Edition {
                language: original.task.language.clone().unwrap(),
                components,
                controls: vec![original.task.id.clone()],
                final_task: original_final.task.id.clone(),
            },
        )]),
    };
    let mut inputs = Inputs::default();
    let mut task = original.task.clone();
    task.native_inputs = None;
    task.upstream_inputs = None;
    let tool = task.tool.as_mut().unwrap();
    let mut descriptors = Vec::new();
    for (path, id) in &tool.source_bindings {
        let expected = original
            .task
            .inputs
            .iter()
            .find(|a| &a.logical_id == id)
            .unwrap();
        assert!(expected.bytes <= 64 * 1024 * 1024);
        let raw = bounded(&exported.join(path), expected.bytes);
        assert_eq!(digest(&raw), expected.sha256);
        assert_eq!(raw.len() as u64, expected.bytes);
        descriptors.push(inputs.put(id, raw, &expected.media_type));
    }
    let publication = inputs.put(
        "composition-replay/publication",
        serde_json::to_vec(&issue.publication).unwrap(),
        "application/json",
    );
    let intent_raw = serde_json::to_vec(&intent).unwrap();
    let intent_asset = inputs.put(
        "composition-replay/intent",
        intent_raw.clone(),
        "application/json",
    );
    let request = reel_workflow::issue_control::Request {
        schema: "reel.workflow-issue-control.v2".into(),
        plan_path: assembly.plan.path,
        parts: assembly
            .parts
            .iter()
            .map(|(id, pin)| {
                (
                    id.clone(),
                    reel_workflow::issue_control::PartPaths {
                        receipt: pin.path.clone(),
                        bundle: assembly.bundles[id].path.clone(),
                    },
                )
            })
            .collect(),
        limits: assembly.limits,
        composition: Some(Composition {
            intent: pin("intent.json", &intent_asset),
            producers: issue
                .parts
                .iter()
                .map(|p| (p.id.clone(), p.id.clone()))
                .collect(),
        }),
    };
    let request_raw = serde_json::to_vec(&request).unwrap();
    let request_asset = inputs.put(
        "composition-replay/request",
        request_raw.clone(),
        "application/json",
    );
    tool.publication = publication.clone();
    tool.request = request_asset.clone();
    tool.source_bindings
        .insert("intent.json".into(), intent_asset.logical_id.clone());
    let executable_raw = bounded(&std::env::current_exe().unwrap(), 256 * 1024 * 1024);
    tool.executable = Artifact {
        logical_id: "composition-replay/fixture".into(),
        sha256: digest(&executable_raw),
        bytes: executable_raw.len() as u64,
        media_type: "application/octet-stream".into(),
    };
    task.inputs = descriptors
        .into_iter()
        .chain([publication, intent_asset, request_asset])
        .collect();
    *workflow
        .tasks
        .iter_mut()
        .find(|t| t.id == original.task.id)
        .unwrap() = task;
    let packet = make_packet(&workflow, &original.task.id, original.dependencies.clone()).unwrap();
    let generated = reel_workflow::issue_control::generate(&packet, &mut inputs).unwrap();
    assert_eq!(
        generated.composition_origins.as_ref().unwrap().inputs.len(),
        4
    );
    let tool = packet.task.tool.as_ref().unwrap();
    let (receipt, files) = reel_publication::comic_issue::capture_with(
        &generated.files["assembly.json"],
        |path, _| Ok(inputs.0[&tool.source_bindings[path]].clone()),
    )
    .unwrap();
    let golden: reel_publication::comic_issue_source::AssemblyReceipt =
        read(&exported.join("receipt.json"));
    assert_eq!(receipt.page_order, golden.page_order);
    assert_eq!(receipt.outputs, golden.outputs);
    let replay = reel_publication::comic_issue::capture_with(&files["assembly.json"], |path, _| {
        Ok(files[path].clone())
    })
    .unwrap();
    assert_eq!(replay.0.outputs, golden.outputs);
    let mut wrong: Assembly = serde_json::from_slice(&generated.files["assembly.json"]).unwrap();
    let producers = &mut wrong.composition.as_mut().unwrap().producers;
    producers.insert("opening".into(), "closing".into());
    producers.insert("closing".into(), "opening".into());
    assert!(
        reel_publication::comic_issue::capture_with(
            &serde_json::to_vec(&wrong).unwrap(),
            |path, _| Ok(inputs.0[&tool.source_bindings[path]].clone())
        )
        .is_err()
    );
    write(&evidence.join("intent.json"), &intent_raw);
    write(&evidence.join("request.json"), &request_raw);
    write(
        &evidence.join("packet.json"),
        &serde_json::to_vec(&packet).unwrap(),
    );
    write(
        &evidence.join("assembly.json"),
        &generated.files["assembly.json"],
    );
    write(
        &evidence.join("receipt.json"),
        &serde_json::to_vec(&receipt).unwrap(),
    );
    write(&evidence.join("report.json"),&serde_json::to_vec_pretty(&serde_json::json!({
        "schema":"reel.composition-control-replay.v1", "scope":"local-exported-synthetic-parts",
        "pages":receipt.page_order.len(),"parts":receipt.parts.len(),"reading_outputs":receipt.outputs,
        "generated_assembly_sha256":digest(&generated.files["assembly.json"]),
        "intent_sha256":digest(&intent_raw),"input_descriptors":packet.task.inputs,
        "captured_intent":files.contains_key("composition-intent.json"),"replay_equal":true,"swapped_order_refused":true,
        "native_byte_access_verified":false,"native_lifecycle_verified":false,"task_completed":false,"selected":false,
    })).unwrap());
}
