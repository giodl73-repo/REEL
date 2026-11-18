//! Synthetic contract tests. Placeholder font/image bytes intentionally do not
//! establish renderer readiness; real rendering is the publication worker fixture.
use reel_publication::{
    ComicPage, Content, Edition, PageKind, Publication, ReadingDirection,
    comic_source::{
        Layer, LayerRole, Limits, Object, PageSource, Panel, Pin, Receipt, Request, Utterance,
    },
};
use reel_workflow::{publication::*, *};
use std::collections::BTreeMap;

#[derive(Clone, Default)]
struct Memory {
    bytes: BTreeMap<String, Vec<u8>>,
    reads: BTreeMap<String, usize>,
    stale: bool,
}
impl Memory {
    fn put(&mut self, id: &str, media: &str, raw: Vec<u8>) -> Artifact {
        let asset = Artifact {
            logical_id: id.into(),
            sha256: digest(&raw),
            bytes: raw.len() as u64,
            media_type: media.into(),
        };
        self.bytes.insert(id.into(), raw);
        asset
    }
}
impl InputReader for Memory {
    fn read(&mut self, asset: &Artifact, max: u64) -> Result<Vec<u8>> {
        *self.reads.entry(asset.logical_id.clone()).or_default() += 1;
        if self.stale {
            return Err(Error {
                kind: ErrorKind::Stale,
                code: "synthetic-reader-stale".into(),
            });
        }
        let raw = self.bytes.get(&asset.logical_id).unwrap().clone();
        assert!(raw.len() as u64 <= max);
        Ok(raw)
    }
}
fn pin(path: &str, asset: &Artifact) -> Pin {
    Pin {
        path: path.into(),
        sha256: asset.sha256.clone(),
        bytes: asset.bytes,
    }
}
fn object(asset: &Artifact) -> Object {
    Object {
        sha256: asset.sha256.clone(),
        bytes: asset.bytes,
    }
}
fn build() -> (Plan, Memory, Receipt) {
    let mut memory = Memory::default();
    let font = memory.put("input/font", "font/ttf", b"synthetic-font".to_vec());
    let license = memory.put(
        "input/license",
        "text/plain",
        b"No rights inferred.".to_vec(),
    );
    let image = memory.put("input/image", "image/png", b"synthetic-image".to_vec());
    let source = PageSource {
        schema: "reel.comic-page.v1".into(),
        id: "page01".into(),
        width: 600,
        height: 820,
        panels: vec![Panel {
            id: "panel-a".into(),
            rect: [40, 40, 520, 330],
        }],
        layers: vec![Layer {
            id: "background".into(),
            panel_id: "panel-a".into(),
            role: LayerRole::Background,
            source: pin("art/picture.png", &image),
            rect: [40, 40, 520, 330],
        }],
        utterances: vec![Utterance {
            id: "line-a".into(),
            panel_id: "panel-a".into(),
            speaker: "Guide".into(),
            lines: vec!["Hola.".into()],
            rect: [60, 60, 400, 80],
            font_size: 24,
            tail: None,
        }],
    };
    let page = memory.put(
        "page/page01-source",
        "application/json",
        serde_json::to_vec(&source).unwrap(),
    );
    let request = Request {
        schema: "reel.comic-build.v1".into(),
        publication_id: "issue-demo".into(),
        edition_id: "es".into(),
        language: "es".into(),
        font_family: "Synthetic".into(),
        font: pin("fonts/body.ttf", &font),
        font_license: pin("fonts/license.txt", &license),
        pages: vec![pin("pages/page01.json", &page)],
        limits: Limits {
            input_bytes: 1_000_000,
            output_bytes: 1_000_000,
        },
    };
    let request_asset = memory.put(
        "job/request",
        "application/json",
        serde_json::to_vec(&request).unwrap(),
    );
    let mut contracts = OutputContracts::new();
    let mut artifacts = ArtifactSet::new();
    for (name, media) in [
        ("page01-clean.svg", "image/svg+xml"),
        ("page01-clean.png", "image/png"),
        ("page01-lettering.svg", "image/svg+xml"),
        ("page01-reading.svg", "image/svg+xml"),
        ("page01-reading.png", "image/png"),
        ("reading.pdf", "application/pdf"),
        ("transcript.json", "application/json"),
    ] {
        contracts.insert(
            name.into(),
            OutputContract {
                media_type: media.into(),
                max_bytes: 1024,
                required: true,
            },
        );
        let raw = format!("synthetic output {name}");
        artifacts.insert(
            name.into(),
            Artifact {
                logical_id: format!("issue-demo/es/{name}"),
                sha256: digest(raw.as_bytes()),
                bytes: raw.len() as u64,
                media_type: media.into(),
            },
        );
    }
    let publication = Publication {
        schema: reel_publication::PUBLICATION_SCHEMA.into(),
        id: "issue-demo".into(),
        editions: vec![Edition {
            id: "es".into(),
            language: "es".into(),
            content: Content::Comic {
                reading_direction: ReadingDirection::LeftToRight,
                pages: vec![ComicPage {
                    id: "page01".into(),
                    kind: PageKind::Story,
                    source: page.clone(),
                    panel_order: vec!["panel-a".into()],
                    utterance_order: vec!["line-a".into()],
                    spread_id: None,
                }],
            },
            outputs: contracts.clone(),
        }],
    };
    let publication_asset = memory.put(
        "publication/issue-demo",
        "application/json",
        serde_json::to_vec(&publication).unwrap(),
    );
    let executable = Artifact {
        logical_id: "tool/publication".into(),
        sha256: digest(b"synthetic-worker"),
        bytes: 16,
        media_type: "application/octet-stream".into(),
    };
    let bindings = BTreeMap::from([
        ("fonts/body.ttf".into(), font.logical_id.clone()),
        ("fonts/license.txt".into(), license.logical_id.clone()),
        ("art/picture.png".into(), image.logical_id.clone()),
        ("pages/page01.json".into(), page.logical_id.clone()),
    ]);
    let task = Task {
        native_inputs: None,
        id: "render".into(),
        operation: Operation::DeterministicTool,
        tool: Some(ToolContract {
            schema: "reel.workflow-tool.v1".into(),
            capability: ToolCapability::ComicRender,
            executable,
            publication: publication_asset.clone(),
            edition_id: "es".into(),
            request: request_asset.clone(),
            source_bindings: bindings,
        }),
        language: Some("es".into()),
        prompt: String::new(),
        inputs: vec![
            font.clone(),
            license.clone(),
            image.clone(),
            page.clone(),
            request_asset.clone(),
            publication_asset,
        ],
        depends_on: vec![],
        outputs: contracts,
    };
    let receipt = Receipt {
        schema: "reel.comic-build-receipt.v1".into(),
        publication_id: "issue-demo".into(),
        edition_id: "es".into(),
        language: "es".into(),
        request: object(&request_asset),
        page_order: vec!["page01".into()],
        outputs: artifacts,
        source_files: BTreeMap::from([
            ("request.json".into(), object(&request_asset)),
            ("fonts/body.ttf".into(), object(&font)),
            ("fonts/license.txt".into(), object(&license)),
            ("art/picture.png".into(), object(&image)),
            ("pages/page01.json".into(), object(&page)),
        ]),
        human_approval: false,
        retained_owner_acknowledged: false,
        print_ready: false,
    };
    (
        Plan {
            schema: PLAN_SCHEMA.into(),
            project: "synthetic-comic".into(),
            source_commit: "a".repeat(40),
            tasks: vec![task],
        },
        memory,
        receipt,
    )
}
fn packet(plan: &Plan) -> Packet {
    make_packet(plan, "render", BTreeMap::new()).unwrap()
}
fn result(packet: &Packet, receipt: &Receipt) -> ReturnedResult {
    ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        worker_receipt: None,
        task_id: "render".into(),
        task_key: packet.task_key.clone(),
        attempt_id: "attempt-1".into(),
        artifacts: receipt.outputs.clone(),
        provenance: Provenance {
            mode: Mode::Tool,
            provider: None,
            model: None,
            executable: Some(packet.task.tool.as_ref().unwrap().executable.clone()),
        },
    }
}
fn revise_publication(plan: &mut Plan, memory: &mut Memory, change: impl FnOnce(&mut Publication)) {
    let id = plan.tasks[0]
        .tool
        .as_ref()
        .unwrap()
        .publication
        .logical_id
        .clone();
    let mut publication: Publication = serde_json::from_slice(&memory.bytes[&id]).unwrap();
    change(&mut publication);
    let next = memory.put(
        &id,
        "application/json",
        serde_json::to_vec(&publication).unwrap(),
    );
    *plan.tasks[0]
        .inputs
        .iter_mut()
        .find(|a| a.logical_id == id)
        .unwrap() = next.clone();
    plan.tasks[0].tool.as_mut().unwrap().publication = next;
}

#[test]
fn deterministic_task_binds_complete_declared_edition_and_worker_receipt() {
    let (plan, mut memory, receipt) = build();
    let packet = packet(&plan);
    let bound = bind_comic(&packet, &mut memory).unwrap();
    assert_eq!(bound.task_key(), packet.task_key);
    assert_eq!(bound.input_objects(), 6);
    assert_eq!(
        bound.logical_bytes(),
        plan.tasks[0].inputs.iter().map(|a| a.bytes).sum::<u64>()
    );
    assert!(memory.reads.values().all(|count| *count == 1));
    check_tool_bytes(&packet, b"synthetic-worker").unwrap();
    assert!(check_tool_bytes(&packet, b"different-worker").is_err());
    let result = result(&packet, &receipt);
    check_result(&packet, &result).unwrap();
    check_comic_receipt(&bound, &result, &serde_json::to_vec(&receipt).unwrap()).unwrap();
}

#[test]
fn same_bytes_under_another_logical_page_id_do_not_satisfy_selection() {
    let (mut plan, mut memory, _) = build();
    revise_publication(&mut plan, &mut memory, |publication| {
        let Content::Comic { pages, .. } = &mut publication.editions[0].content else {
            panic!()
        };
        pages[0].source.logical_id = "page/alias-with-same-bytes".into();
    });
    assert_eq!(
        bind_comic(&packet(&plan), &mut memory).unwrap_err().code,
        "selected-page-logical-binding"
    );
}

#[test]
fn worker_receipt_pin_is_optional_for_legacy_contracts_but_exact_when_present() {
    let (plan, mut memory, receipt) = build();
    let packet = packet(&plan);
    let bound = bind_comic(&packet, &mut memory).unwrap();
    let mut returned = result(&packet, &receipt);
    assert!(
        !serde_json::to_value(&returned)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("worker_receipt")
    );
    let raw = serde_json::to_vec(&receipt).unwrap();
    returned.worker_receipt = Some(Artifact {
        logical_id: "receipt/comic".into(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    });
    check_result(&packet, &returned).unwrap();
    check_comic_receipt(&bound, &returned, &raw).unwrap();
    returned.worker_receipt.as_mut().unwrap().sha256 = "0".repeat(64);
    assert_eq!(
        check_comic_receipt(&bound, &returned, &raw)
            .unwrap_err()
            .kind,
        ErrorKind::Integrity
    );
    returned.worker_receipt.as_mut().unwrap().media_type = "image/png".into();
    assert_eq!(
        check_result(&packet, &returned).unwrap_err().kind,
        ErrorKind::Invalid
    );
}

#[test]
fn language_and_panel_or_utterance_order_mismatches_refuse() {
    for field in ["language", "panel", "utterance"] {
        let (mut plan, mut memory, _) = build();
        revise_publication(&mut plan, &mut memory, |publication| {
            if field == "language" {
                publication.editions[0].language = "en".into();
                return;
            }
            let Content::Comic { pages, .. } = &mut publication.editions[0].content else {
                panic!()
            };
            if field == "panel" {
                pages[0].panel_order[0] = "different-panel".into()
            } else {
                pages[0].utterance_order[0] = "different-line".into()
            }
        });
        assert_eq!(
            bind_comic(&packet(&plan), &mut memory).unwrap_err().kind,
            ErrorKind::Conflict
        );
    }
}

#[test]
fn comic_tool_refuses_film_content_and_incomplete_worker_output_contracts() {
    let (mut plan, mut memory, _) = build();
    revise_publication(&mut plan, &mut memory, |publication| {
        let Content::Comic { pages, .. } = &publication.editions[0].content else {
            panic!()
        };
        let composition = pages[0].source.clone();
        publication.editions[0].content = Content::Film { composition };
    });
    assert_eq!(
        bind_comic(&packet(&plan), &mut memory).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    let (mut plan, mut memory, _) = build();
    revise_publication(&mut plan, &mut memory, |publication| {
        publication.editions[0].outputs.remove("transcript.json");
    });
    plan.tasks[0].outputs.remove("transcript.json");
    assert_eq!(
        bind_comic(&packet(&plan), &mut memory).unwrap_err().code,
        "comic-worker-output-contract"
    );
}

#[test]
fn missing_extra_and_changed_leaf_bindings_refuse() {
    let (mut plan, mut memory, _) = build();
    plan.tasks[0]
        .tool
        .as_mut()
        .unwrap()
        .source_bindings
        .remove("art/picture.png");
    assert_eq!(
        bind_comic(&packet(&plan), &mut memory).unwrap_err().kind,
        ErrorKind::Missing
    );
    let (mut plan, mut memory, _) = build();
    let unused = memory.put("unused/source", "text/plain", b"unused".to_vec());
    plan.tasks[0].inputs.push(unused);
    assert_eq!(
        bind_comic(&packet(&plan), &mut memory).unwrap_err().code,
        "undeclared-or-unused-input"
    );
    let (plan, mut memory, _) = build();
    memory
        .bytes
        .insert("input/image".into(), b"corrupt".to_vec());
    assert_eq!(
        bind_comic(&packet(&plan), &mut memory).unwrap_err().kind,
        ErrorKind::Integrity
    );
}

#[test]
fn stale_source_observation_propagates_without_retry_or_tool_call() {
    let (plan, mut memory, _) = build();
    memory.stale = true;
    assert_eq!(
        bind_comic(&packet(&plan), &mut memory).unwrap_err().kind,
        ErrorKind::Stale
    );
    assert_eq!(memory.reads.values().sum::<usize>(), 1);
}

#[test]
fn worker_cannot_change_sources_namespace_or_claim_approval_and_retention() {
    let (plan, mut memory, receipt) = build();
    let packet = packet(&plan);
    let bound = bind_comic(&packet, &mut memory).unwrap();
    for field in [
        "source",
        "namespace",
        "approval",
        "retention",
        "language",
        "task",
    ] {
        let mut changed = receipt.clone();
        if field == "source" {
            changed
                .source_files
                .get_mut("art/picture.png")
                .unwrap()
                .sha256 = "f".repeat(64)
        }
        if field == "namespace" {
            changed.outputs.get_mut("reading.pdf").unwrap().logical_id =
                "another/edition/movie".into()
        }
        if field == "approval" {
            changed.human_approval = true
        }
        if field == "retention" {
            changed.retained_owner_acknowledged = true
        }
        if field == "language" {
            changed.language = "en".into()
        }
        let mut result = result(&packet, &changed);
        if field == "task" {
            result.task_id = "another-task".into()
        }
        assert!(
            check_comic_receipt(&bound, &result, &serde_json::to_vec(&changed).unwrap()).is_err()
        );
    }
}

#[test]
fn tool_tasks_never_dispatch_through_an_llm_adapter() {
    struct Never;
    impl DirectAdapter for Never {
        fn execute(&mut self, _: &Packet, _: &str) -> Result<ReturnedResult> {
            panic!("LLM must not be invoked")
        }
    }
    let (plan, _, receipt) = build();
    let packet = packet(&plan);
    assert_eq!(
        call_direct(&mut Never, &packet, "attempt-1")
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    let mut returned = result(&packet, &receipt);
    returned.provenance.mode = Mode::Handoff;
    assert!(check_result(&packet, &returned).is_err());
}

#[test]
fn malformed_tool_prompt_and_stale_result_fail_before_consumption() {
    let (mut plan, _, receipt) = build();
    let old = packet(&plan);
    let result = result(&old, &receipt);
    plan.tasks[0].prompt = "Please run a shell command".into();
    assert!(check_plan(&plan).is_err());
    plan.tasks[0].prompt.clear();
    plan.tasks[0].tool.as_mut().unwrap().executable.sha256 = "b".repeat(64);
    assert_eq!(
        check_result(&packet(&plan), &result).unwrap_err().kind,
        ErrorKind::Stale
    );
}

#[test]
fn local_cli_checks_complete_inputs_tool_and_returned_payloads_without_dispatch() {
    use std::fs;
    let root = tempfile::tempdir().unwrap();
    let (plan, memory, receipt) = build();
    let packet = packet(&plan);
    let result = result(&packet, &receipt);
    let mut paths = BTreeMap::new();
    for (index, (id, bytes)) in memory.bytes.iter().enumerate() {
        let path = root.path().join(format!("input-{index}.json"));
        fs::write(&path, bytes).unwrap();
        paths.insert(id.clone(), path.to_str().unwrap().to_owned());
    }
    let tool = root.path().join("worker.bin");
    fs::write(&tool, b"synthetic-worker").unwrap();
    let packet_path = root.path().join("packet.json");
    fs::write(&packet_path, serde_json::to_vec(&packet).unwrap()).unwrap();
    let paths_path = root.path().join("input-bindings.json");
    fs::write(&paths_path, serde_json::to_vec(&paths).unwrap()).unwrap();
    let checked = std::process::Command::new(env!("CARGO_BIN_EXE_reel-workflow"))
        .arg("check-publication-task")
        .arg(&packet_path)
        .arg(&paths_path)
        .arg(&tool)
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stdout)
    );
    let report: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(report["production_admitted"], false);
    assert_eq!(report["tool_executed"], false);
    assert_eq!(report["snapshot_binding_verified"], false);
    let receipt_path = root.path().join("receipt.json");
    fs::write(&receipt_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
    let result_path = root.path().join("result.json");
    fs::write(&result_path, serde_json::to_vec(&result).unwrap()).unwrap();
    let mut payloads = BTreeMap::new();
    for (index, name) in receipt.outputs.keys().enumerate() {
        let path = root.path().join(format!("output-{index}.dat"));
        fs::write(&path, format!("synthetic output {name}")).unwrap();
        payloads.insert(name.clone(), path.to_str().unwrap().to_owned());
    }
    let payloads_path = root.path().join("payloads.json");
    fs::write(&payloads_path, serde_json::to_vec(&payloads).unwrap()).unwrap();
    let checked = std::process::Command::new(env!("CARGO_BIN_EXE_reel-workflow"))
        .arg("check-publication-result")
        .arg(&packet_path)
        .arg(&paths_path)
        .arg(&tool)
        .arg(&result_path)
        .arg(&receipt_path)
        .arg(&payloads_path)
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stdout)
    );
    let report: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(report["retention_acknowledged"], false);
    assert_eq!(report["task_completed"], false);
    assert!(
        !String::from_utf8(checked.stdout)
            .unwrap()
            .contains("issue-demo")
    );
    fs::write(&tool, b"another-program").unwrap();
    assert!(
        !std::process::Command::new(env!("CARGO_BIN_EXE_reel-workflow"))
            .arg("check-publication-task")
            .arg(&packet_path)
            .arg(&paths_path)
            .arg(&tool)
            .status()
            .unwrap()
            .success()
    );
}
