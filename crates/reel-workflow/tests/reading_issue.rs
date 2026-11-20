use reel_publication::{
    ComicPage, Content, Edition, PageKind, Publication, ReadingDirection, comic_source::Pin,
};
use reel_workflow::*;
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
fn asset(id: &str, b: &[u8], media: &str) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: digest(b),
        bytes: b.len() as u64,
        media_type: media.into(),
    }
}
fn fixture(mode: u8) -> (Packet, Inputs) {
    let mut data = BTreeMap::new();
    let mut inputs = Vec::new();
    let mut bindings = BTreeMap::new();
    let mut pages = Vec::new();
    let mut specs = Vec::new();
    let mut selected = Vec::new();
    for i in 1..=2 {
        let id = format!("book-page-{i}");
        let source=serde_json::to_vec(&serde_json::json!({"schema":"bbj.layered-page-source.v1","canvas":[2,2],"title":"Selected page","layers":[]})).unwrap();
        let panel = if mode == 3 && i == 1 {
            "different-panel".into()
        } else {
            format!("panel-{i}")
        };
        let author=serde_json::to_vec(&serde_json::json!({"schema":"refraction.native-scene-authoring.v1","page":{"reading_order":[panel]},"dialogue":[{"id":format!("line-{i}"),"panel":format!("panel-{i}")}]})).unwrap();
        let mut pixels = resvg::tiny_skia::Pixmap::new(2, 2).unwrap();
        pixels.fill(resvg::tiny_skia::Color::WHITE);
        let png = pixels.encode_png().unwrap();
        let mut add = |name: &str, raw: Vec<u8>, media: &str| {
            let path = format!("page-{i}/{name}");
            let a = asset(&format!("assets/{path}"), &raw, media);
            let pin = Pin {
                path: path.clone(),
                sha256: a.sha256.clone(),
                bytes: a.bytes,
            };
            bindings.insert(path, a.logical_id.clone());
            data.insert(a.logical_id.clone(), raw);
            inputs.push(a.clone());
            (pin, a)
        };
        let (source_pin, source_asset) = add("source.json", source, "application/json");
        let (author_pin, _) = add("authoring.json", author, "application/json");
        let (png_pin, _) = add("reading.png", png, "image/png");
        let declared =
            |p: &Pin| serde_json::json!({"file":p.path,"sha256":p.sha256,"bytes":p.bytes});
        selected.push(serde_json::json!({"sheet":i,"source":declared(&source_pin),"authoring":declared(&author_pin),"png":declared(&png_pin)}));
        specs.push(reading_issue::Page {
            id: id.clone(),
            sheet: i,
            source: source_pin,
            authoring: author_pin,
            png: png_pin,
        });
        pages.push(ComicPage {
            id,
            kind: PageKind::Story,
            source: source_asset,
            panel_order: vec![format!("panel-{i}")],
            utterance_order: vec![format!("line-{i}")],
            spread_id: None,
        });
    }
    if mode == 2 {
        selected[0]["png"]["file"] = "wrong/page.png".into();
    }
    let contracts = [
        ("reading.pdf", "application/pdf", 32 * 1024 * 1024),
        (
            "reading-manifest.json",
            "application/json",
            MAX_CONTROL_BYTES as u64,
        ),
        (
            "source-index.json",
            "application/json",
            MAX_CONTROL_BYTES as u64,
        ),
    ]
    .into_iter()
    .map(|(n, m, b)| {
        (
            n.into(),
            OutputContract {
                media_type: m.into(),
                max_bytes: b,
                required: true,
            },
        )
    })
    .collect::<OutputContracts>();
    let publication = Publication {
        schema: "reel.publication.v1".into(),
        id: "book".into(),
        editions: vec![Edition {
            id: "en".into(),
            language: "en".into(),
            content: Content::Comic {
                reading_direction: ReadingDirection::LeftToRight,
                pages,
            },
            outputs: contracts.clone(),
        }],
    };
    let raw = serde_json::to_vec(
        &serde_json::json!({"schema":"refraction.book-preview.v1","sheets":selected}),
    )
    .unwrap();
    let a = asset("control/selection", &raw, "application/json");
    let selection = Pin {
        path: "selected-book.json".into(),
        sha256: a.sha256.clone(),
        bytes: a.bytes,
    };
    bindings.insert(selection.path.clone(), a.logical_id.clone());
    data.insert(a.logical_id.clone(), raw);
    inputs.push(a);
    if mode == 1 {
        specs.pop();
    }
    let req = reading_issue::Request {
        schema: "reel.workflow-reading-issue.v1".into(),
        selection,
        pages: specs,
        input_bytes: 32 * 1024 * 1024,
        output_bytes: 32 * 1024 * 1024,
    };
    let pubraw = serde_json::to_vec(&publication).unwrap();
    let reqraw = serde_json::to_vec(&req).unwrap();
    let pubasset = asset("control/publication", &pubraw, "application/json");
    let reqasset = asset("control/request", &reqraw, "application/json");
    data.insert(pubasset.logical_id.clone(), pubraw);
    data.insert(reqasset.logical_id.clone(), reqraw);
    inputs.extend([pubasset.clone(), reqasset.clone()]);
    let task = Task {
        id: "assemble".into(),
        native_inputs: None,
        upstream_inputs: None,
        operation: Operation::DeterministicTool,
        language: Some("en".into()),
        prompt: String::new(),
        inputs,
        depends_on: vec![],
        outputs: contracts,
        tool: Some(ToolContract {
            schema: "reel.workflow-tool.v1".into(),
            capability: ToolCapability::ComicReadingAssembly,
            executable: asset("tool/native", b"runtime", "application/octet-stream"),
            auxiliary_executables: BTreeMap::new(),
            publication: pubasset,
            edition_id: "en".into(),
            request: reqasset,
            source_bindings: bindings,
            part: None,
        }),
    };
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: "reading-fixture".into(),
        source_commit: "a".repeat(40),
        task_snapshots: BTreeMap::new(),
        tasks: vec![task],
    };
    (
        make_packet(&plan, "assemble", BTreeMap::new()).unwrap(),
        Inputs(data),
    )
}
#[test]
fn complete_reading_issue_replays_pages_and_preserves_source_review_targets() {
    let (packet, mut inputs) = fixture(0);
    let generated = reading_issue::generate(&packet, &mut inputs).unwrap();
    assert!(generated.files["reading.pdf"].starts_with(b"%PDF-"));
    let result = ReturnedResult {
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
    };
    let exported = generated
        .bound
        .review_copy(&result, &generated.files["receipt.json"])
        .unwrap();
    assert_eq!(exported.copy.units.len(), 6);
    let reading =
        review::comic_reading_inputs(&exported.copy, exported.manifest.as_ref().unwrap()).unwrap();
    assert_eq!(reading.len(), 2);
    assert_eq!(reading[0].logical_id, "assets/page-1/reading.png");
    for (name, pin) in &generated.outputs {
        inputs
            .0
            .insert(pin.logical_id.clone(), generated.files[name].clone());
    }
    inputs.0.insert(
        result.worker_receipt.as_ref().unwrap().logical_id.clone(),
        generated.files["receipt.json"].clone(),
    );
    let bindings = review_control::bindings(&packet, &result, None).unwrap();
    let raw = bindings.canonical_bytes().unwrap();
    let producer = packet.task.tool.as_ref().unwrap();
    let child_task = Task {
        id: "review-controls".into(),
        native_inputs: None,
        upstream_inputs: Some(asset("control/review-upstream", &raw, "application/json")),
        operation: Operation::DeterministicTool,
        language: packet.task.language.clone(),
        prompt: String::new(),
        inputs: review_control::inputs(&packet, &result).unwrap(),
        depends_on: vec![packet.task.id.clone()],
        outputs: review_control::OUTPUTS
            .iter()
            .map(|n| {
                (
                    (*n).into(),
                    OutputContract {
                        media_type: "application/json".into(),
                        max_bytes: MAX_CONTROL_BYTES as u64,
                        required: true,
                    },
                )
            })
            .collect(),
        tool: Some(ToolContract {
            schema: "reel.workflow-tool.v1".into(),
            capability: ToolCapability::ComicReviewControl,
            executable: producer.executable.clone(),
            auxiliary_executables: BTreeMap::new(),
            publication: producer.publication.clone(),
            edition_id: producer.edition_id.clone(),
            request: result.worker_receipt.clone().unwrap(),
            source_bindings: review_control::bound_source_bindings(&packet, &result).unwrap(),
            part: None,
        }),
    };
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: packet.project.clone(),
        source_commit: packet.source_commit.clone(),
        task_snapshots: BTreeMap::new(),
        tasks: vec![packet.task.clone(), child_task],
    };
    let child = make_packet(
        &plan,
        "review-controls",
        BTreeMap::from([(
            packet.task.id.clone(),
            DependencyResult {
                result_sha256: canonical_digest(&result).unwrap(),
                artifacts: result.artifacts.clone(),
            },
        )]),
    )
    .unwrap();
    let parents = BTreeMap::from([(
        packet.task.id.clone(),
        upstream::Parent {
            packet: &packet,
            result: &result,
        },
    )]);
    let controls = review_control::generate(&child, &bindings, &parents, &mut inputs).unwrap();
    let copy: review::Copy = decode(&controls.files["review-copy.json"]).unwrap();
    let request: review::Request = decode(&controls.files["review-request.json"]).unwrap();
    assert_eq!(copy.units, exported.copy.units);
    assert_eq!(request.production.unwrap().packet.task_key, packet.task_key);
    assert_eq!(
        review::comic_reading_inputs(&copy, &controls.files["review-sources.json"]).unwrap(),
        reading
    );
    assert!(child.task.inputs.iter().any(|pin| pin == &reading[0]));
    let again = reading_issue::generate(&packet, &mut inputs).unwrap();
    assert_eq!(generated.files, again.files);
}
#[test]
fn reading_issue_rejects_missing_pages_changed_associations_and_review_order() {
    for (mode, code) in [
        (1, "reading-issue-complete-book-coverage"),
        (2, "reading-issue-selected-record-association"),
        (3, "reading-issue-review-order"),
    ] {
        // Build honest pins and task keys around each invalid semantic input.
        let (packet, mut inputs) = fixture(mode);
        let error = reading_issue::bind(&packet, &mut inputs).err().unwrap();
        assert_eq!(error.kind, ErrorKind::Conflict);
        assert_eq!(error.code, code);
    }
}
