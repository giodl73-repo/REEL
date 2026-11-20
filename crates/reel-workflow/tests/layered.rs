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
