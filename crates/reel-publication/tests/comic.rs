use reel_publication::comic::*;
use resvg::tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn pin(root: &Path, name: &str, raw: &[u8]) -> Pin {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    assert!(!path.exists());
    fs::write(&path, raw).unwrap();
    let obj = object(raw);
    Pin {
        path: name.into(),
        sha256: obj.sha256,
        bytes: obj.bytes,
    }
}
fn json(root: &Path, name: &str, value: &impl serde::Serialize) -> Pin {
    pin(root, name, &serde_json::to_vec_pretty(value).unwrap())
}
fn shape(pixels: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, color: [u8; 4]) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    let mut path = PathBuilder::new();
    path.push_rect(resvg::tiny_skia::Rect::from_xywh(x, y, w, h).unwrap());
    pixels.fill_path(
        &path.finish().unwrap(),
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}
fn artwork() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let mut background = Pixmap::new(520, 330).unwrap();
    background.fill(Color::from_rgba8(211, 232, 238, 255));
    shape(
        &mut background,
        0.0,
        220.0,
        520.0,
        110.0,
        [180, 197, 192, 255],
    );
    shape(
        &mut background,
        330.0,
        140.0,
        150.0,
        115.0,
        [249, 239, 206, 255],
    );
    let mut person = Pixmap::new(100, 160).unwrap();
    shape(&mut person, 30.0, 0.0, 40.0, 42.0, [188, 124, 89, 255]);
    shape(&mut person, 12.0, 46.0, 76.0, 74.0, [38, 84, 115, 255]);
    shape(&mut person, 17.0, 120.0, 25.0, 40.0, [44, 57, 75, 255]);
    shape(&mut person, 58.0, 120.0, 25.0, 40.0, [44, 57, 75, 255]);
    let mut lamp = Pixmap::new(80, 100).unwrap();
    shape(&mut lamp, 15.0, 0.0, 50.0, 42.0, [250, 192, 70, 255]);
    shape(&mut lamp, 36.0, 42.0, 8.0, 45.0, [94, 86, 74, 255]);
    shape(&mut lamp, 10.0, 87.0, 60.0, 10.0, [94, 86, 74, 255]);
    (
        background.encode_png().unwrap(),
        person.encode_png().unwrap(),
        lamp.encode_png().unwrap(),
    )
}
fn font() -> (Vec<u8>, Vec<u8>, String) {
    if let Ok(path) = std::env::var("REEL_COMIC_TEST_FONT") {
        return (
            fs::read(path).unwrap(),
            fs::read(
                std::env::var("REEL_COMIC_TEST_FONT_LICENSE")
                    .expect("explicit fixture font license"),
            )
            .unwrap(),
            std::env::var("REEL_COMIC_TEST_FONT_FAMILY").expect("explicit fixture font family"),
        );
    }
    for (path, family) in [
        ("C:/Windows/Fonts/arial.ttf", "Arial"),
        (
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "DejaVu Sans",
        ),
        (
            "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
            "Liberation Sans",
        ),
    ] {
        if Path::new(path).is_file() {
            return (fs::read(path).unwrap(),b"Synthetic private test uses an existing system font. This fixture grants no font redistribution rights.".to_vec(),family.into());
        }
    }
    panic!("explicit installed fixture font required")
}
fn page(id: &str, lines: [&str; 2], art: &[Pin; 3]) -> PageSource {
    let panels = vec![
        Panel {
            id: "panel-a".into(),
            rect: [40, 40, 520, 330],
        },
        Panel {
            id: "panel-b".into(),
            rect: [40, 430, 520, 330],
        },
    ];
    let mut layers = Vec::new();
    let mut utterances = Vec::new();
    for (index, panel) in panels.iter().enumerate() {
        let y = panel.rect[1];
        for (name, role, source, rect) in [
            (
                "background",
                LayerRole::Background,
                art[0].clone(),
                panel.rect,
            ),
            (
                "character",
                LayerRole::Character,
                art[1].clone(),
                [95, y + 140, 110, 176],
            ),
            (
                "lamp",
                LayerRole::Prop,
                art[2].clone(),
                [440, y + 210, 72, 90],
            ),
        ] {
            layers.push(Layer {
                id: format!("{}-{name}", panel.id),
                panel_id: panel.id.clone(),
                role,
                source,
                rect,
            });
        }
        utterances.push(Utterance {
            id: format!("line-{}", index + 1),
            panel_id: panel.id.clone(),
            speaker: "Guide".into(),
            lines: vec![lines[index].into()],
            rect: [60, y + 20, 480, 76],
            font_size: 24,
            tail: Some([[110, y + 90], [140, y + 90], [155, y + 142]]),
        });
    }
    PageSource {
        schema: "reel.comic-page.v1".into(),
        id: id.into(),
        width: 600,
        height: 820,
        panels,
        layers,
        utterances,
    }
}
fn request(
    root: &Path,
    edition: &str,
    language: &str,
    family: &str,
    font: &Pin,
    license: &Pin,
    pages: Vec<Pin>,
) -> PathBuf {
    let build = Request {
        schema: "reel.comic-build.v1".into(),
        publication_id: "synthetic-lamp".into(),
        edition_id: edition.into(),
        language: language.into(),
        font_family: family.into(),
        font: font.clone(),
        font_license: license.clone(),
        pages,
        limits: Limits {
            input_bytes: 16 * 1024 * 1024,
            output_bytes: 32 * 1024 * 1024,
        },
    };
    let name = format!("{edition}.json");
    json(root, &name, &build);
    root.join(name)
}

#[test]
fn issue_parts_reassemble_in_order_and_restore_without_original_folders() {
    use reel_publication::comic_issue::{Assembly, Part, Plan, assemble};
    use reel_publication::{
        Artifact, ComicPage, Content, Edition, OutputContract, PageKind, Publication,
        ReadingDirection,
    };
    use std::collections::BTreeMap;
    let temp = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir_in(temp.path()).unwrap();
    let packets = tempfile::tempdir_in(temp.path()).unwrap();
    let root = source.path();
    let (font_bytes, license_bytes, family) = font();
    let font = pin(root, "fonts/body.ttf", &font_bytes);
    let license = pin(root, "fonts/license.txt", &license_bytes);
    let (bg, person, lamp) = artwork();
    let art = [
        pin(root, "art/background.png", &bg),
        pin(root, "art/person.png", &person),
        pin(root, "art/lamp.png", &lamp),
    ];
    let mut pages = Vec::new();
    let mut pins = Vec::new();
    for id in ["page01", "page02", "page03"] {
        let source = page(
            id,
            [
                if id == "page01" {
                    "La luz sigue."
                } else if id == "page02" {
                    "Seguimos."
                } else {
                    "Hasta mañana."
                },
                "Juntos.",
            ],
            &art,
        );
        let pin = json(root, &format!("pages/{id}.json"), &source);
        pages.push(ComicPage {
            id: id.into(),
            kind: PageKind::Story,
            source: Artifact {
                logical_id: format!("sources/{id}"),
                sha256: pin.sha256.clone(),
                bytes: pin.bytes,
                media_type: "application/json".into(),
            },
            panel_order: source.panels.iter().map(|p| p.id.clone()).collect(),
            utterance_order: source.utterances.iter().map(|u| u.id.clone()).collect(),
            spread_id: if id == "page01" {
                None
            } else {
                Some("spread-a".into())
            },
        });
        pins.push(pin);
    }
    let mut plan = Plan {
        schema: "reel.comic-issue-plan.v1".into(),
        publication: Publication {
            schema: "reel.publication.v1".into(),
            id: "synthetic-lamp".into(),
            editions: vec![Edition {
                id: "reading-es".into(),
                language: "es".into(),
                content: Content::Comic {
                    reading_direction: ReadingDirection::RightToLeft,
                    pages,
                },
                outputs: BTreeMap::from([
                    (
                        "reading.pdf".into(),
                        OutputContract {
                            media_type: "application/pdf".into(),
                            max_bytes: 64 * 1024 * 1024,
                            required: true,
                        },
                    ),
                    (
                        "transcript.json".into(),
                        OutputContract {
                            media_type: "application/json".into(),
                            max_bytes: 1024 * 1024,
                            required: true,
                        },
                    ),
                ]),
            }],
        },
        edition_id: "reading-es".into(),
        parts: vec![
            Part {
                id: "opening".into(),
                pages: vec!["page01".into()],
            },
            Part {
                id: "spread".into(),
                pages: vec!["page02".into(), "page03".into()],
            },
        ],
    };
    plan.check().unwrap();
    let mut large = plan.clone();
    let Content::Comic {
        pages: large_pages, ..
    } = &mut large.publication.editions[0].content
    else {
        unreachable!()
    };
    let template = large_pages[0].clone();
    *large_pages = (0..40)
        .map(|i| ComicPage {
            id: format!("page{i:03}"),
            ..template.clone()
        })
        .collect();
    large.parts = vec![
        Part {
            id: "first".into(),
            pages: large_pages[..32].iter().map(|p| p.id.clone()).collect(),
        },
        Part {
            id: "last".into(),
            pages: large_pages[32..].iter().map(|p| p.id.clone()).collect(),
        },
    ];
    large.check().unwrap();
    let next = large.parts[1].pages.remove(0);
    large.parts[0].pages.push(next);
    assert!(large.check().is_err()); // metadata supports larger issues, each worker remains bounded
    assert_eq!(
        plan.part_publication("spread").unwrap().editions[0].language,
        "es"
    );
    let mut invalid = plan.clone();
    invalid.parts[0].pages.push("page02".into());
    invalid.parts[1].pages.remove(0);
    assert!(invalid.check().is_err()); // authored spread cannot be split
    invalid = plan.clone();
    invalid.parts[1].pages.reverse();
    assert!(invalid.check().is_err());
    invalid = plan.clone();
    invalid.parts.pop();
    assert!(invalid.check().is_err());
    invalid = plan.clone();
    invalid.parts[1].id = "OPENING".into();
    assert!(invalid.check().is_err());
    let mut part_receipts = BTreeMap::new();
    for (id, page_pins) in [
        ("opening", vec![pins[0].clone()]),
        ("spread", pins[1..].to_vec()),
    ] {
        // Same edition identity; requests live separately and are captured in packets.
        let request = request(
            root,
            "reading-es",
            "es",
            &family,
            &font,
            &license,
            page_pins,
        );
        let output = packets.path().join(id);
        render(&request, &output).unwrap();
        fs::remove_file(request).unwrap(); // exclusively owned fixture request, reused name
        let receipt = object(&fs::read(output.join("receipt.json")).unwrap());
        part_receipts.insert(
            id.into(),
            Pin {
                path: format!("{id}/receipt.json"),
                sha256: receipt.sha256,
                bytes: receipt.bytes,
            },
        );
    }
    // Only captured sources are available to assembly; the source checkout is gone.
    source.close().unwrap();
    let plan_pin = json(packets.path(), "plan.json", &plan);
    let cli = std::process::Command::new(env!("CARGO_BIN_EXE_reel-publication"))
        .args(["comic-issue-check"])
        .arg(packets.path().join("plan.json"))
        .output()
        .unwrap();
    assert!(cli.status.success());
    let report: serde_json::Value = serde_json::from_slice(&cli.stdout).unwrap();
    assert_eq!(report["outputs_written"], false);
    assert_eq!(report["payloads_verified"], false);
    let assembly = Assembly {
        schema: "reel.comic-issue-assembly.v1".into(),
        plan: plan_pin,
        parts: part_receipts,
        limits: Limits {
            input_bytes: 128 * 1024 * 1024,
            output_bytes: 128 * 1024 * 1024,
        },
    };
    let request_path = packets.path().join("assembly.json");
    fs::write(&request_path, serde_json::to_vec_pretty(&assembly).unwrap()).unwrap();
    let output = temp.path().join("issue");
    let receipt = assemble(&request_path, &output).unwrap();
    assert_eq!(receipt.page_order, ["page01", "page02", "page03"]);
    assert!(
        !receipt.human_approval && !receipt.retained_owner_acknowledged && !receipt.print_ready
    );
    assert_eq!(
        lopdf::Document::load(output.join("reading.pdf"))
            .unwrap()
            .get_pages()
            .len(),
        3
    );
    let pdf = lopdf::Document::load(output.join("reading.pdf")).unwrap();
    for ((_, page_id), (part, page)) in pdf.get_pages().iter().zip([
        ("opening", "page01"),
        ("spread", "page02"),
        ("spread", "page03"),
    ]) {
        let resources = pdf
            .get_dictionary(*page_id)
            .unwrap()
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap();
        let image_id = resources
            .get(b"XObject")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Im0")
            .unwrap()
            .as_reference()
            .unwrap();
        let rgb = pdf
            .get_object(image_id)
            .unwrap()
            .as_stream()
            .unwrap()
            .decompressed_content()
            .unwrap();
        let png = Pixmap::decode_png(
            &fs::read(output.join(format!("parts/{part}/{page}-reading.png"))).unwrap(),
        )
        .unwrap();
        assert_eq!(
            rgb,
            png.data()
                .chunks_exact(4)
                .flat_map(|p| p[..3].iter().copied())
                .collect::<Vec<_>>()
        );
    }
    let transcript: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("transcript.json")).unwrap()).unwrap();
    assert_eq!(transcript["pages"][1]["page_id"], "page02");
    assert_eq!(
        transcript["pages"][0]["utterances"][0]["lines"][0],
        "La luz sigue."
    );
    assert!(assemble(&request_path, &output).is_err());
    // Changed plan/language and changed page source pins refuse before reserving output.
    plan.publication.editions[0].language = "en".into();
    fs::write(
        packets.path().join("plan.json"),
        serde_json::to_vec_pretty(&plan).unwrap(),
    )
    .unwrap();
    let mut wrong = assembly.clone();
    let obj = object(&fs::read(packets.path().join("plan.json")).unwrap());
    wrong.plan.sha256 = obj.sha256;
    wrong.plan.bytes = obj.bytes;
    let wrong_path = packets.path().join("wrong.json");
    fs::write(&wrong_path, serde_json::to_vec_pretty(&wrong).unwrap()).unwrap();
    let refused = temp.path().join("refused");
    assert!(assemble(&wrong_path, &refused).is_err());
    assert!(!refused.exists());
    plan.publication.editions[0].language = "es".into();
    let Content::Comic { pages, .. } = &mut plan.publication.editions[0].content else {
        unreachable!()
    };
    pages[0].source.sha256 = "0".repeat(64);
    let altered = serde_json::to_vec_pretty(&plan).unwrap();
    let obj = object(&altered);
    fs::write(packets.path().join("plan.json"), altered).unwrap();
    wrong.plan.sha256 = obj.sha256;
    wrong.plan.bytes = obj.bytes;
    fs::write(&wrong_path, serde_json::to_vec_pretty(&wrong).unwrap()).unwrap();
    assert!(assemble(&wrong_path, &refused).is_err());
    assert!(!refused.exists());
    packets.close().unwrap();
    // Self-contained issue can be reassembled after both original locations are gone.
    let second = temp.path().join("second");
    let cli = std::process::Command::new(env!("CARGO_BIN_EXE_reel-publication"))
        .arg("comic-issue-assemble")
        .arg(output.join("assembly.json"))
        .arg(&second)
        .output()
        .unwrap();
    assert!(cli.status.success());
    let replay: reel_publication::comic_issue::AssemblyReceipt =
        serde_json::from_slice(&fs::read(second.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt.outputs, replay.outputs);
    let restored = temp.path().join("restored-part");
    let part = output.join("parts/opening");
    let receipt_pin = object(&fs::read(part.join("receipt.json")).unwrap());
    restore_sources(&part, &restored, &receipt_pin.sha256).unwrap();
    assert!(restored.join("pages/page01.json").is_file());
    fs::write(part.join("page01-lettering.svg"), b"altered").unwrap();
    assert!(assemble(&output.join("assembly.json"), &temp.path().join("tampered")).is_err());
    assert!(!temp.path().join("tampered").exists());
    if let Some(path) = std::env::var_os("REEL_COMIC_ISSUE_EVIDENCE_LOCATION") {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        let location = temp.keep();
        file.write_all(&serde_json::to_vec_pretty(&serde_json::json!({"root":location,"verified_issue":second,"editable_restoration":restored,"tampered_issue":output})).unwrap()).unwrap();
        file.sync_all().unwrap();
    }
}

#[test]
fn bilingual_pages_fix_preserves_art_and_restores_editable_sources_without_originals() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::env::var_os("REEL_COMIC_FIXTURE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| temp.path().join("comic-fixture"));
    assert!(root.is_absolute() && !root.exists());
    fs::create_dir(&root).unwrap();
    let input = root.join("inputs");
    fs::create_dir(&input).unwrap();
    let (font_bytes, license_bytes, family) = font();
    let font = pin(&input, "fonts/body.ttf", &font_bytes);
    let license = pin(&input, "fonts/license.txt", &license_bytes);
    let (bg, person, lamp) = artwork();
    let art = [
        pin(&input, "art/background.png", &bg),
        pin(&input, "art/person.png", &person),
        pin(&input, "art/lamp.png", &lamp),
    ];
    let es01 = page("page01", ["¿Ves la luz?", "Sí. Está encendida."], &art);
    let es02 = page("page02", ["Entonces, seguimos.", "Juntos."], &art);
    let en01 = page("page01", ["Do you see the light?", "Yes. It is on."], &art);
    let en02 = page("page02", ["Then let's go.", "Together."], &art);
    let es_pages = vec![
        json(&input, "pages/es01.json", &es01),
        json(&input, "pages/es02.json", &es02),
    ];
    let en_pages = vec![
        json(&input, "pages/en01.json", &en01),
        json(&input, "pages/en02.json", &en02),
    ];
    let es_request = request(
        &input,
        "reading-es",
        "es",
        &family,
        &font,
        &license,
        es_pages.clone(),
    );
    let en_request = request(
        &input,
        "reading-en",
        "en",
        &family,
        &font,
        &license,
        en_pages,
    );
    let es_output = root.join("es");
    let en_output = root.join("en");
    let es = render(&es_request, &es_output).unwrap();
    let replayed = check_with(
        &fs::read(&es_request).unwrap(),
        |pin| Ok(fs::read(input.join(&pin.path))?),
        |name, _| Ok(fs::read(es_output.join(name))?),
    )
    .unwrap();
    assert_eq!(replayed.outputs, es.outputs);
    assert!(
        check_with(
            &fs::read(&es_request).unwrap(),
            |pin| Ok(fs::read(input.join(&pin.path))?),
            |name, _| {
                let mut raw = fs::read(es_output.join(name))?;
                if name == "reading.pdf" {
                    raw.push(0);
                }
                Ok(raw)
            }
        )
        .is_err()
    );
    let en = render(&en_request, &en_output).unwrap();
    assert_eq!(es.page_order, vec!["page01", "page02"]);
    assert!(!es.human_approval && !es.retained_owner_acknowledged && !es.print_ready);
    assert_eq!(
        lopdf::Document::load(es_output.join("reading.pdf"))
            .unwrap()
            .get_pages()
            .len(),
        2
    );
    check(&es_request, &es_output).unwrap();
    check(&en_request, &en_output).unwrap();
    let cli = std::process::Command::new(env!("CARGO_BIN_EXE_reel-publication"))
        .current_dir(&root)
        .arg("comic-check")
        .arg("inputs/reading-es.json")
        .arg("es")
        .output()
        .unwrap();
    assert!(cli.status.success());
    let cli_report: serde_json::Value = serde_json::from_slice(&cli.stdout).unwrap();
    assert_eq!(cli_report["rendered"], true);
    assert_eq!(cli_report["outputs_written"], false);
    assert!(render(&es_request, &es_output).is_err());
    let mut fixed = es01.clone();
    fixed.utterances[0].lines = vec!["Mira, la luz.".into()];
    let fixed_pin = json(&input, "pages/es01-fixed.json", &fixed);
    let fixed_request = request(
        &input,
        "reading-es-fixed",
        "es",
        &family,
        &font,
        &license,
        vec![fixed_pin, es_pages[1].clone()],
    );
    let fixed_output = root.join("es-fixed");
    let fixed_receipt = render(&fixed_request, &fixed_output).unwrap();
    for name in ["page01-clean.png", "page02-clean.png", "page02-reading.png"] {
        assert_eq!(
            fs::read(es_output.join(name)).unwrap(),
            fs::read(fixed_output.join(name)).unwrap()
        );
    }
    assert_ne!(
        fs::read(es_output.join("page01-reading.png")).unwrap(),
        fs::read(fixed_output.join("page01-reading.png")).unwrap()
    );
    assert_eq!(en.outputs, check(&en_request, &en_output).unwrap().outputs);
    assert!(check(&es_request, &fixed_output).is_err());
    let receipt_pin = object(&fs::read(fixed_output.join("receipt.json")).unwrap());
    let restored = root.join("restored");
    assert!(restore_sources(&fixed_output, &restored, &"0".repeat(64)).is_err());
    assert!(!restored.exists());
    // Only this fixture's owned source directory is made unavailable.
    let history = root.join("inputs-history");
    let owned = root.canonicalize().unwrap();
    assert!(
        input.canonicalize().unwrap().starts_with(&owned)
            && history.parent().unwrap().canonicalize().unwrap() == owned
            && !history.exists()
    );
    fs::rename(&input, &history).unwrap();
    assert!(!input.exists());
    restore_sources(&fixed_output, &restored, &receipt_pin.sha256).unwrap();
    let rebuilt = root.join("rebuilt");
    let rebuilt_receipt = render(&restored.join("request.json"), &rebuilt).unwrap();
    assert_eq!(rebuilt_receipt.outputs, fixed_receipt.outputs);
    let mut restored_page: PageSource =
        serde_json::from_slice(&fs::read(restored.join("pages/es01-fixed.json")).unwrap()).unwrap();
    restored_page.utterances[0].lines = vec!["La luz sigue aquí.".into()];
    let changed = json(&restored, "pages/es01-new-edit.json", &restored_page);
    let mut edit: Request =
        serde_json::from_slice(&fs::read(restored.join("request.json")).unwrap()).unwrap();
    edit.pages[0] = changed;
    edit.edition_id = "reading-es-new-edit".into();
    json(&restored, "new-edit.json", &edit);
    let edited = root.join("restored-edit");
    render(&restored.join("new-edit.json"), &edited).unwrap();
    assert_eq!(
        fs::read(edited.join("page01-clean.png")).unwrap(),
        fs::read(fixed_output.join("page01-clean.png")).unwrap()
    );
    assert_ne!(
        fs::read(edited.join("page01-reading.png")).unwrap(),
        fs::read(fixed_output.join("page01-reading.png")).unwrap()
    );
    println!("comic fixture: {}", root.display());
}

#[test]
fn stale_pins_path_escapes_and_unreadable_lettering_refuse_before_outputs() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (font_bytes, license_bytes, family) = font();
    let font = pin(root, "font.ttf", &font_bytes);
    let license = pin(root, "license.txt", &license_bytes);
    let (bg, person, lamp) = artwork();
    let art = [
        pin(root, "bg.png", &bg),
        pin(root, "person.png", &person),
        pin(root, "lamp.png", &lamp),
    ];
    let mut source = page("page01", ["A short line.", "Another line."], &art);
    source.layers[0].source.sha256 = "0".repeat(64);
    let source_pin = json(root, "bad-page.json", &source);
    let bad = request(
        root,
        "bad-pin",
        "en",
        &family,
        &font,
        &license,
        vec![source_pin],
    );
    let output = root.join("refused");
    assert!(render(&bad, &output).is_err());
    assert!(!output.exists());
    source.layers[0].source = art[0].clone();
    source.layers[0].source.path = "../bg.png".into();
    let source_pin = json(root, "escape-page.json", &source);
    let bad = request(
        root,
        "bad-path",
        "en",
        &family,
        &font,
        &license,
        vec![source_pin],
    );
    assert!(render(&bad, &output).is_err());
    assert!(!output.exists());
    source.layers[0].source = art[0].clone();
    source.utterances[0].lines = vec!["Far too much text for this balloon ".repeat(20)];
    let source_pin = json(root, "crowded-page.json", &source);
    let bad = request(
        root,
        "bad-lettering",
        "en",
        &family,
        &font,
        &license,
        vec![source_pin],
    );
    assert!(render(&bad, &output).is_err());
    assert!(!output.exists());
}
