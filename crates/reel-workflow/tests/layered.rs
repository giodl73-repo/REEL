use reel_publication::comic_source::Pin;
use reel_publication::{ComicPage, Content, Edition, PageKind, Publication, ReadingDirection};
use reel_workflow::{layered, *};
use std::collections::BTreeMap;

struct Inputs(BTreeMap<String, Vec<u8>>);
impl publication::InputReader for Inputs {
    fn read(&mut self, a: &Artifact, _: u64) -> Result<Vec<u8>> {
        self.0.get(&a.logical_id).cloned().ok_or(Error {
            kind: ErrorKind::Missing,
            code: "fixture-input".into(),
        })
    }
}
fn pin(path: &str, raw: &[u8]) -> Pin {
    Pin {
        path: path.into(),
        sha256: digest(raw),
        bytes: raw.len() as u64,
    }
}
fn asset(id: &str, raw: &[u8], media: &str) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: digest(raw),
        bytes: raw.len() as u64,
        media_type: media.into(),
    }
}
fn fixture(shadow: bool, wrong_association: bool, invalid_png: bool) -> (Packet, Inputs) {
    let svg=br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><rect width="2" height="2" fill="white"/></svg>"#.to_vec();
    let expected=br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2" viewBox="0 0 2 2"><title>Fixture</title><g id="ink" data-role="art" opacity="1" transform="translate(0 0)"><rect width="2" height="2" fill="white"/></g></svg>"#;
    let mut png = resvg::tiny_skia::Pixmap::new(2, 2)
        .unwrap()
        .encode_png()
        .unwrap();
    if invalid_png {
        let i = png.windows(4).position(|w| w == b"IDAT").unwrap() + 4;
        png[i] = 0;
    }
    let source=serde_json::to_vec(&serde_json::json!({"schema":"bbj.layered-page-source.v1","canvas":[2,2],"title":"Fixture","layers":[{"id":"ink","role":"art","asset":{"file":"layers/ink.svg","sha256":digest(&svg),"bytes":svg.len()}}]})).unwrap();
    let author=serde_json::to_vec(&serde_json::json!({"schema":"refraction.native-scene-authoring.v1","page":{"reading_order":["panel-1"]},"dialogue":[{"id":"line-1","panel":"panel-1"}]})).unwrap();
    let source_pin = pin("page/source.json", &source);
    let author_pin = pin("page/author.json", &author);
    let png_pin = pin("reading.png", &png);
    let typed =
        |pin: &Pin| serde_json::json!({"file":pin.path,"sha256":pin.sha256,"bytes":pin.bytes});
    let mut selected_png = typed(&png_pin);
    if wrong_association {
        selected_png["file"] = "another.png".into();
    }
    let selection=serde_json::to_vec(&serde_json::json!({"schema":"refraction.book-preview.v1","sheets":[{"sheet":1,"source":typed(&source_pin),"authoring":typed(&author_pin),"png":selected_png,"svg":{"file":"selected.svg","sha256":digest(expected),"bytes":expected.len()}}]})).unwrap();
    let request = layered::Request {
        schema: "reel.workflow-layered-comic.v1".into(),
        page: reel_publication::layered_source::Request {
            schema: "reel.layered-comic-request.v1".into(),
            page_id: "book-story-001".into(),
            language: "es".into(),
            source: source_pin,
            provenance: vec![author_pin.clone()],
            input_bytes: 1024 * 1024,
            output_bytes: 1024 * 1024,
        },
        authoring: author_pin,
        selection: pin("book/selected.json", &selection),
        sheet: 1,
        selected_svg: reel_publication::comic::object(expected),
        reading_png: png_pin,
    };
    let data = BTreeMap::from([
        ("page/source.json".to_owned(), (source, "application/json")),
        ("page/author.json".into(), (author, "application/json")),
        ("layers/ink.svg".into(), (svg, "image/svg+xml")),
        ("book/selected.json".into(), (selection, "application/json")),
        ("reading.png".into(), (png, "image/png")),
    ]);
    let sources: BTreeMap<_, _> = data
        .iter()
        .map(|(path, (raw, media))| (path.clone(), asset(&format!("assets/{path}"), raw, media)))
        .collect();
    let outputs: BTreeMap<_, _> = [
        ("page.svg", "image/svg+xml"),
        ("page.png", "image/png"),
        ("editable.packet", "application/vnd.reel.editable-packet.v1"),
    ]
    .into_iter()
    .map(|(name, media)| {
        (
            name.into(),
            OutputContract {
                media_type: media.into(),
                max_bytes: 1024 * 1024,
                required: true,
            },
        )
    })
    .collect();
    let mut selected_source = sources["page/source.json"].clone();
    if shadow {
        selected_source.logical_id = "shadow/source".into();
    }
    let publication = Publication {
        schema: "reel.publication.v1".into(),
        id: "book".into(),
        editions: vec![Edition {
            id: "es".into(),
            language: "es".into(),
            content: Content::Comic {
                reading_direction: ReadingDirection::LeftToRight,
                pages: vec![ComicPage {
                    id: "book-story-001".into(),
                    kind: PageKind::Story,
                    source: selected_source,
                    panel_order: vec!["panel-1".into()],
                    utterance_order: vec!["line-1".into()],
                    spread_id: None,
                }],
            },
            outputs: outputs.clone(),
        }],
    };
    let pubraw = serde_json::to_vec(&publication).unwrap();
    let reqraw = serde_json::to_vec(&request).unwrap();
    let pubasset = asset("control/publication", &pubraw, "application/json");
    let reqasset = asset("control/request", &reqraw, "application/json");
    let mut inputs: Vec<_> = sources.values().cloned().collect();
    inputs.extend([pubasset.clone(), reqasset.clone()]);
    let tool = ToolContract {
        schema: "reel.workflow-tool.v1".into(),
        capability: ToolCapability::LayeredComicCompose,
        executable: asset("tool/engine", b"runtime", "application/octet-stream"),
        auxiliary_executables: BTreeMap::new(),
        publication: pubasset,
        edition_id: "es".into(),
        request: reqasset,
        source_bindings: sources
            .iter()
            .map(|(p, a)| (p.clone(), a.logical_id.clone()))
            .collect(),
        part: None,
    };
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: "synthetic-layered-contract".into(),
        source_commit: "a".repeat(40),
        task_snapshots: BTreeMap::new(),
        tasks: vec![Task {
            id: "compose-page".into(),
            native_inputs: None,
            upstream_inputs: None,
            operation: Operation::DeterministicTool,
            tool: Some(tool),
            language: Some("es".into()),
            prompt: String::new(),
            inputs,
            depends_on: vec![],
            outputs,
        }],
    };
    let mut bytes: BTreeMap<_, _> = data
        .into_iter()
        .map(|(path, (raw, _))| (format!("assets/{path}"), raw))
        .collect();
    bytes.insert("control/publication".into(), pubraw);
    bytes.insert("control/request".into(), reqraw);
    (
        make_packet(&plan, "compose-page", BTreeMap::new()).unwrap(),
        Inputs(bytes),
    )
}
fn returned(packet: &Packet, generated: &layered::Generated) -> ReturnedResult {
    ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: "attempt-1".into(),
        artifacts: generated.outputs.clone(),
        provenance: Provenance {
            mode: Mode::Tool,
            provider: None,
            model: None,
            executable: Some(packet.task.tool.as_ref().unwrap().executable.clone()),
        },
        worker_receipt: Some(asset(
            "worker/receipt",
            &generated.files["receipt.json"],
            "application/json",
        )),
    }
}

fn planning_fixture() -> (
    tempfile::TempDir,
    bbj::Config,
    BTreeMap<std::path::PathBuf, Vec<u8>>,
) {
    let directory = tempfile::tempdir().unwrap();
    let prepared = directory.path().join("prepared");
    let (packet, inputs) = fixture(false, false, false);
    let tool = packet.task.tool.as_ref().unwrap();
    let request: layered::Request = decode(&inputs.0[&tool.request.logical_id]).unwrap();
    let mut data = BTreeMap::new();
    let mut bindings = BTreeMap::new();
    for (i, (path, id)) in tool
        .source_bindings
        .iter()
        .filter(|(p, _)| **p != request.selection.path && **p != request.reading_png.path)
        .enumerate()
    {
        let absolute = directory.path().join(format!("input-{i}"));
        bindings.insert(path.clone(), absolute.clone());
        data.insert(absolute, inputs.0[id].clone());
    }
    let reading = directory.path().join("reading.png");
    data.insert(
        reading.clone(),
        inputs.0[&tool.source_bindings[&request.reading_png.path]].clone(),
    );
    data.insert(
        prepared.join("reading-bindings.json"),
        serde_json::to_vec(&BTreeMap::from([(
            request.reading_png.path.clone(),
            reading,
        )]))
        .unwrap(),
    );
    data.insert(
        prepared.join("sheet-001/bindings.json"),
        serde_json::to_vec(&bindings).unwrap(),
    );
    data.insert(
        prepared.join("sheet-001/request.json"),
        serde_json::to_vec(&request.page).unwrap(),
    );
    data.insert(
        prepared.join("publication.json"),
        inputs.0[&tool.publication.logical_id].clone(),
    );
    data.insert(
        prepared.join("selected-book.json"),
        inputs.0[&tool.source_bindings[&request.selection.path]].clone(),
    );
    let selection = &inputs.0[&tool.source_bindings[&request.selection.path]];
    data.insert(prepared.join("book.json"),serde_json::to_vec(&serde_json::json!({"schema":"reel.bbj-book-import.v1","publication_id":"book","selection":{"sha256":digest(selection),"bytes":selection.len()},"pages":[{"sheet":1,"request":"sheet-001/request.json","bindings":"sheet-001/bindings.json"}]})).unwrap());
    let exe = directory.path().join("engine.exe");
    data.insert(exe.clone(), b"runtime".to_vec());
    let config = bbj::Config {
        schema: "reel.workflow-bbj-page-plan.v1".into(),
        project: "planner-fixture".into(),
        source_commit: "a".repeat(40),
        task_prefix: "page-v2".into(),
        attempt_id: "attempt-1".into(),
        executable: pin(exe.to_str().unwrap(), b"runtime"),
        native_inputs: asset("control/native", b"{}", "application/json"),
    };
    (directory, config, data)
}

#[test]
fn bbj_planning_captures_replayable_sources_and_defers_parent_resolution() {
    let (dir, config, data) = planning_fixture();
    let files = bbj::prepare(
        &config,
        &dir.path().join("prepared"),
        1,
        &dir.path().join("capsule"),
        |p, max| {
            let raw = data.get(p).unwrap();
            assert!(raw.len() as u64 <= max);
            Ok(raw.clone())
        },
    )
    .unwrap();
    let preview: Packet = decode(&files["local-preview-packet.json"]).unwrap();
    let mut inputs = Inputs(
        preview
            .task
            .inputs
            .iter()
            .map(|a| {
                (
                    a.logical_id.clone(),
                    files[&format!("incoming/{}", a.logical_id.rsplit('/').next().unwrap())]
                        .clone(),
                )
            })
            .collect(),
    );
    let generated = layered::generate(&preview, &mut inputs).unwrap();
    assert_eq!(
        generated.outputs["page.png"].sha256,
        digest(&files["incoming/reading.png"])
    );
    let recipe: recipe::Recipe = decode(&files["recipe.json"]).unwrap();
    recipe::check(&recipe).unwrap();
    assert_eq!(recipe.steps[0].depends_on, vec!["page-v2-intake"]);
    assert!(
        recipe.steps[0]
            .inputs
            .iter()
            .all(|i| matches!(i, expansion::Input::Parent { .. }))
    );
    let proposed: ReturnedResult = decode(&files["proposed-return.json"]).unwrap();
    let intake: Packet = decode(&files["intake-packet.json"]).unwrap();
    check_result(&intake, &proposed).unwrap();
    // A contract observer only: this unit test grants no actual native custody.
    struct Observed(graph::Job);
    impl graph::Observer for Observed {
        fn observe(&mut self, id: &str) -> Result<Option<graph::Job>> {
            Ok((id == self.0.ticket.packet.task.id).then(|| self.0.clone()))
        }
        fn verify(&mut self, _: &graph::Job) -> Result<()> {
            Ok(())
        }
    }
    let mut observed = Observed(graph::Job {
        ticket: handoff::Ticket {
            packet: intake,
            attempt_id: proposed.attempt_id.clone(),
            native_binding: asset("control/fixture-binding", b"binding", "application/json"),
            lifecycle_tool: preview.task.tool.as_ref().unwrap().executable.clone(),
        },
        state: handoff::State::Retained,
        result: Some(proposed.clone()),
        acknowledgement: Some(asset("control/fixture-ack", b"ack", "application/json")),
    });
    let expanded = expansion::prepare(
        &recipe.base,
        &expansion::Template {
            schema: recipe.steps[0].expansion_schema().into(),
            base_plan_sha256: canonical_digest(&recipe.base).unwrap(),
            task: recipe.steps[0].clone(),
        },
        &mut observed,
    )
    .unwrap();
    assert_eq!(
        expanded.packet.dependencies["page-v2-intake"].result_sha256,
        canonical_digest(&proposed).unwrap()
    );
    assert_eq!(expanded.packet.task.inputs, preview.task.inputs);
    let intent: serde_json::Value = decode(&files["intent.json"]).unwrap();
    assert_eq!(intent["native_admitted"], false);
    assert_eq!(intent["retained_owner_acknowledged"], false);
    assert!(!dir.path().join("capsule").exists());
}

#[test]
fn bbj_planning_refuses_changed_runtime_and_incomplete_source_closure() {
    let (dir, mut config, mut data) = planning_fixture();
    config.executable.sha256 = "0".repeat(64);
    let error = bbj::prepare(
        &config,
        &dir.path().join("prepared"),
        1,
        &dir.path().join("capsule"),
        |p, _| Ok(data[p].clone()),
    )
    .unwrap_err();
    assert_eq!(error.code, "bbj-planning-runtime-pin");
    config.executable.sha256 = digest(b"runtime");
    let key = dir.path().join("prepared/sheet-001/bindings.json");
    let mut bindings: BTreeMap<String, std::path::PathBuf> = decode(&data[&key]).unwrap();
    bindings.insert("unused/source.json".into(), dir.path().join("extra"));
    data.insert(key, serde_json::to_vec(&bindings).unwrap());
    let error = bbj::prepare(
        &config,
        &dir.path().join("prepared"),
        1,
        &dir.path().join("capsule"),
        |p, _| Ok(data[p].clone()),
    )
    .unwrap_err();
    assert_eq!(error.code, "bbj-planning-binding-closure");
    assert!(!dir.path().join("capsule").exists());
}

#[test]
fn bbj_planning_refuses_null_item_lists_before_capturing_media() {
    let (dir, config, mut data) = planning_fixture();
    let path = dir.path().join("prepared/publication.json");
    let mut publication: serde_json::Value = decode(&data[&path]).unwrap();
    publication["editions"][0]["content"]["pages"][0]["panel_order"] = serde_json::Value::Null;
    data.insert(path.clone(), serde_json::to_vec(&publication).unwrap());
    let mut opened = Vec::new();
    let error = bbj::prepare(
        &config,
        &dir.path().join("prepared"),
        1,
        &dir.path().join("capsule"),
        |p, _| {
            opened.push(p.to_owned());
            Ok(data[p].clone())
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "bbj-planning-publication-control");
    assert_eq!(opened, vec![path]);
    assert!(!dir.path().join("capsule").exists());
}
#[test]
fn managed_layered_replay_preserves_selected_pixels_editability_and_review_units() {
    let (packet, mut inputs) = fixture(false, false, false);
    let generated = layered::generate(&packet, &mut inputs).unwrap();
    let result = returned(&packet, &generated);
    generated
        .bound
        .check_receipt(&result, &generated.files["receipt.json"])
        .unwrap();
    let export = generated
        .bound
        .review_copy(&result, &generated.files["receipt.json"])
        .unwrap();
    assert_eq!(export.copy.units.len(), 3);
    assert_eq!(
        review::comic_reading_inputs(&export.copy, export.manifest.as_ref().unwrap()).unwrap(),
        vec![result.artifacts["page.png"].clone()]
    );
    let bundle = reel_publication::packet::decode(&generated.files["editable.packet"]).unwrap();
    assert_eq!(
        bundle["sources/layers/ink.svg"],
        inputs.0["assets/layers/ink.svg"]
    );
    assert_eq!(
        bundle["sources/book/selected.json"],
        inputs.0["assets/book/selected.json"]
    );
    let mut wrong = result;
    wrong.artifacts.get_mut("page.png").unwrap().sha256 = "b".repeat(64);
    assert_eq!(
        generated
            .bound
            .check_receipt(&wrong, &generated.files["receipt.json"])
            .unwrap_err()
            .code,
        "layered-selected-output-bytes"
    );
}
#[test]
fn equal_source_bytes_do_not_substitute_for_selected_logical_identity_or_record_association() {
    let (packet, mut inputs) = fixture(true, false, false);
    assert_eq!(
        layered::bind(&packet, &mut inputs).err().unwrap().code,
        "layered-selected-source-identity"
    );
    let (packet, mut inputs) = fixture(false, true, false);
    assert_eq!(
        layered::bind(&packet, &mut inputs).err().unwrap().code,
        "layered-selected-record-association"
    );
}
#[test]
fn a_png_with_honest_pins_but_invalid_payload_is_not_a_reading_result() {
    let (packet, mut inputs) = fixture(false, false, true);
    assert_eq!(
        layered::generate(&packet, &mut inputs).err().unwrap().code,
        "layered-reading-png"
    );
}
