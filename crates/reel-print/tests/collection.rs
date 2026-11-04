use lopdf::{Document, Object, Stream, dictionary};
use reel_print::{Entry, Limits, Pin, Request, check, render};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn font() -> (PathBuf, String) {
    for (path, name) in [
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
            return (path.into(), name.into());
        }
    }
    panic!("fixture requires an explicit installed Arial, DejaVu Sans or Liberation Sans font");
}
fn pin(path: &Path) -> Pin {
    let raw = fs::read(path).unwrap();
    Pin {
        path: path.into(),
        sha256: Sha256::digest(&raw)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        bytes: raw.len() as u64,
    }
}
fn write(root: &Path, name: &str, raw: impl AsRef<[u8]>) -> Pin {
    let path = root.join(name);
    fs::write(&path, raw).unwrap();
    pin(&path)
}
fn request(root: &Path, entries: Vec<Entry>) -> PathBuf {
    let (font, family) = font();
    let q = Request {
        schema: "reel.print-collection.v1".into(),
        title: "Private notation fixture".into(),
        author: "Synthetic fixture".into(),
        notice: "Technical checks only; musical review pending.".into(),
        private_only: true,
        fonts: vec![pin(&font)],
        body_font: family,
        entries,
        limits: Limits {
            input_bytes: 16 * 1024 * 1024,
            output_bytes: 64 * 1024 * 1024,
            pages: 64,
        },
    };
    let path = root.join("request.json");
    fs::write(&path, serde_json::to_vec(&q).unwrap()).unwrap();
    path
}
fn change(path: &Path, f: impl FnOnce(&mut Request)) {
    let mut q: Request = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    f(&mut q);
    fs::write(path, serde_json::to_vec(&q).unwrap()).unwrap();
}
fn grid(systems: usize) -> String {
    let mut s = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1100\" height=\"{}\">",
        150 * systems + 80
    );
    for i in 0..systems {
        for d in (0..=40).step_by(10) {
            let y = 80 + 150 * i + d;
            s += &format!(
                "<line class=\"staff\" x1=\"80\" x2=\"1060\" y1=\"{y}\" y2=\"{y}\" stroke=\"black\"/>"
            );
        }
        s += &format!(
            "<circle cx=\"150\" cy=\"{}\" r=\"6\"/><text x=\"200\" y=\"{}\">System {}</text>",
            100 + 150 * i,
            100 + 150 * i,
            i + 1
        );
    }
    // Native exporters reuse the staff class for short pitch ledger lines.
    s + "<line class=\"staff\" x1=\"97\" x2=\"133\" y1=\"130\" y2=\"130\" stroke=\"black\"/></svg>"
}
#[test]
fn unicode_parallel_readings_and_every_native_system_survive_deterministic_packet_checks() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let left = write(
        root,
        "es.txt",
        "Español: niñez, canción y corazón.\nUna línea más.",
    );
    let right = write(
        root,
        "en.txt",
        "English: childhood, song and heart.\nAnother line.",
    );
    let score = write(root, "score.svg", grid(9));
    let path = request(
        root,
        vec![
            Entry::ParallelText {
                id: "readings".into(),
                title: "Parallel readings".into(),
                left,
                right,
                left_label: "Español".into(),
                right_label: "English".into(),
            },
            Entry::NativeScore {
                id: "notation".into(),
                title: "Nine systems".into(),
                source: score,
                systems_per_page: 4,
            },
        ],
    );
    let output = root.join("packet");
    let receipt = render(&path, &output).unwrap();
    assert_eq!(receipt.pages.len(), 5);
    assert_eq!(
        receipt
            .pages
            .iter()
            .filter_map(|p| p.source_band)
            .collect::<Vec<_>>(),
        vec![[40., 640.], [640., 1240.], [1240., 1390.]]
    );
    assert_eq!(check(&path, &output).unwrap(), receipt);
    assert!(!receipt.human_approval && !receipt.published && !receipt.authorizes_eviction);
    let pdf = Document::load(output.join("collection.pdf")).unwrap();
    assert_eq!(pdf.get_pages().len(), 5);
    // The independent reader inserts a newline for each positioned glyph.
    // Remove only those separators; retain spaces, accents and character order.
    let text = pdf.extract_text(&[2]).unwrap().replace('\n', "");
    assert!(
        text.contains("niñez") && text.contains("corazón") && text.contains("English"),
        "{text}"
    );
    assert!(!text.contains('\u{fffd}'));
    let before = fs::read(output.join("collection.pdf")).unwrap();
    assert!(render(&path, &output).is_err());
    assert_eq!(fs::read(output.join("collection.pdf")).unwrap(), before);
    fs::write(output.join("page-002.png"), b"tamper").unwrap();
    assert!(check(&path, &output).is_err());
}
#[test]
fn long_poems_continue_without_losing_last_lines_or_overlapping_footers() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let body = (1..=120)
        .map(|n| format!("Canonical line {n}: canción y corazón.\n"))
        .collect::<String>();
    let source = write(root, "poem.txt", body);
    let path = request(
        root,
        vec![Entry::Text {
            id: "poem".into(),
            title: "A complete poem".into(),
            source,
        }],
    );
    let out = root.join("packet");
    let receipt = render(&path, &out).unwrap();
    assert!(receipt.pages.len() > 3);
    let pdf = Document::load(out.join("collection.pdf")).unwrap();
    let pages = (1..=pdf.get_pages().len() as u32).collect::<Vec<_>>();
    let text = pdf.extract_text(&pages).unwrap().replace('\n', "");
    assert!(text.contains("Canonical line 1:") && text.contains("Canonical line 120:"));
    assert_eq!(text.matches("Canonical line ").count(), 120);
}
#[test]
fn all_limits_stale_pins_duplicate_ids_and_active_svg_refuse_before_output_creation() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let source = write(root, "poem.txt", "A bounded fixture");
    let path = request(
        root,
        vec![Entry::Text {
            id: "poem".into(),
            title: "Fixture".into(),
            source: source.clone(),
        }],
    );
    let original = fs::read(&path).unwrap();
    for (index, kind) in ["input", "output", "pages", "duplicate", "stale", "privacy"]
        .iter()
        .enumerate()
    {
        fs::write(&path, &original).unwrap();
        change(&path, |q| match *kind {
            "input" => q.limits.input_bytes = 1,
            "output" => q.limits.output_bytes = 1,
            "pages" => q.limits.pages = 1,
            "duplicate" => q.entries.push(q.entries[0].clone()),
            "stale" => {
                if let Entry::Text { source, .. } = &mut q.entries[0] {
                    source.sha256 = "0".repeat(64)
                }
            }
            "privacy" => q.private_only = false,
            _ => unreachable!(),
        });
        let out = root.join(format!("refused-{index}"));
        assert!(render(&path, &out).is_err(), "{kind}");
        assert!(!out.exists());
    }
    for (index, svg) in [
        grid(2).replace("y1=\"230\"", "y1=\"231\""),
        grid(1).replace("</svg>", "<image href=\"file:///private.png\"/></svg>"),
        grid(1).replace(
            "</svg>",
            "<style>@import 'https://example.invalid/style';</style></svg>",
        ),
    ]
    .iter()
    .enumerate()
    {
        let source = write(root, &format!("bad-{index}.svg"), svg);
        let path = request(
            root,
            vec![Entry::NativeScore {
                id: "score".into(),
                title: "Fixture".into(),
                source,
                systems_per_page: 4,
            }],
        );
        let out = root.join(format!("bad-svg-{index}"));
        assert!(render(&path, &out).is_err());
        assert!(!out.exists());
    }
}
fn inherited_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages = doc.new_object_id();
    let font =
        doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let resources = dictionary! {"Font"=>dictionary!{"F1"=>font}};
    let a = doc.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 20 40 Td (Alpha) Tj ET".to_vec(),
    ));
    let b = doc.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 20 40 Td (Beta) Tj ET".to_vec(),
    ));
    let first = doc.add_object(dictionary! {"Type"=>"Page","Parent"=>pages,"Contents"=>a});
    let second = doc.add_object(dictionary! {"Type"=>"Page","Parent"=>pages,"Contents"=>b});
    doc.objects.insert(pages,dictionary!{"Type"=>"Pages","Count"=>2,"Kids"=>vec![Object::Reference(second),Object::Reference(first)],"Resources"=>resources,"MediaBox"=>vec![0.into(),0.into(),300.into(),200.into()],"Rotate"=>0}.into());
    let root = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    doc.trailer.set("Root", root);
    let mut bytes = vec![];
    doc.save_to(&mut bytes).unwrap();
    bytes
}
#[test]
fn retained_pdf_pages_keep_order_decoded_commands_and_inherited_resources() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let bytes = inherited_pdf();
    let source = write(root, "source.pdf", &bytes);
    let path = request(
        root,
        vec![Entry::RetainedPdf {
            id: "retained".into(),
            source,
        }],
    );
    let out = root.join("packet");
    let receipt = render(&path, &out).unwrap();
    assert_eq!(receipt.pages.len(), 3);
    assert_eq!(check(&path, &out).unwrap(), receipt);
    let old = Document::load_mem(&bytes).unwrap();
    let new = Document::load(out.join("collection.pdf")).unwrap();
    let old_pages = old.get_pages();
    let new_pages = new.get_pages();
    for n in 1..=2 {
        assert_eq!(
            old.get_page_content(old_pages[&n]).unwrap(),
            new.get_page_content(new_pages[&(n + 1)]).unwrap()
        );
        assert!(
            new.get_object(new_pages[&(n + 1)])
                .unwrap()
                .as_dict()
                .unwrap()
                .has(b"Resources")
        );
    }
    assert!(new.extract_text(&[2]).unwrap().contains("Beta"));
    assert!(new.extract_text(&[3]).unwrap().contains("Alpha"));
    assert_eq!(receipt.pages[1].width, 300.0);
    assert_eq!(receipt.pages[1].height, 200.0);
    assert_eq!(fs::read(root.join("source.pdf")).unwrap(), bytes);
    let mut reordered = old.clone();
    reordered.renumber_objects_with(100);
    let mut bytes = vec![];
    reordered.save_to(&mut bytes).unwrap();
    let different_ids = write(root, "renumbered.pdf", bytes);
    change(&path, |q| {
        q.entries[0] = Entry::RetainedPdf {
            id: "retained".into(),
            source: different_ids,
        }
    });
    let second = root.join("renumbered-packet");
    render(&path, &second).unwrap();
    assert_eq!(
        fs::read(out.join("collection.pdf")).unwrap(),
        fs::read(second.join("collection.pdf")).unwrap(),
        "object numbering must not create a new print byte version"
    );
}

#[cfg(unix)]
#[test]
fn relative_sources_cannot_escape_through_a_directory_link() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let source = write(
        outside.path(),
        "source.txt",
        "Exact bytes do not authorize following a directory link",
    );
    std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
    let mut aliased = source;
    aliased.path = "linked/source.txt".into();
    let request = request(
        root.path(),
        vec![Entry::Text {
            id: "source".into(),
            title: "Fixture".into(),
            source: aliased,
        }],
    );
    let output = root.path().join("refused");
    assert!(render(&request, &output).is_err());
    assert!(!output.exists());
}
