//! Public signals and authored fixture text only; no acoustic/creative approval.
use super::*;
use reel::{
    scene_delivery::{FileRef, InputFiles},
    spoken_programme::{
        checker,
        reading::{self, Audio, Chapter, Column, Edition, Request, Span},
    },
};
use std::{path::Path, time::Duration};

struct Assets {
    root: PathBuf,
    runtime: FileRef,
    executable: PathBuf,
}
impl InputFiles for Assets {
    fn verified_path(&self, pin: &FileRef) -> Result<PathBuf> {
        if pin.path == self.runtime.path {
            assert_eq!(pin.sha256, self.runtime.sha256);
            assert_eq!(pin.bytes, self.runtime.bytes);
            Ok(self.executable.clone())
        } else {
            self.root.verified_path(pin)
        }
    }
}
fn pinned(path: impl Into<PathBuf>, raw: &[u8]) -> FileRef {
    FileRef {
        path: path.into(),
        sha256: hash(raw),
        bytes: raw.len() as u64,
    }
}
fn save(root: &Path, name: &str, raw: &[u8]) -> FileRef {
    fs::write(root.join(name), raw).unwrap();
    pinned(name, raw)
}
pub(super) struct ReadingFixture {
    pub(super) f: Fixture,
    assets: Assets,
    dialogue: PathBuf,
    access: PathBuf,
    print: PathBuf,
    layers: BTreeMap<String, reading::Layered<'static>>,
    request: Request,
}
impl ReadingFixture {
    pub(super) fn new(version_two: bool) -> Self {
        let mut f = Fixture::new();
        if version_two {
            let module = &mut f.document.modules[1];
            let script = module.cues[0]
                .scripts
                .iter_mut()
                .find(|s| s.language == "es")
                .unwrap();
            script.text.push_str(" Versión dos.");
            script.sha256 = hash(script.text.as_bytes());
            for p in &mut f.document.performances {
                if p.module_id == module.id && p.script_id == script.id {
                    p.script_sha256 = script.sha256.clone();
                }
            }
            f.signals[4][5000] += 1;
            f.sync();
        }
        // The reading fixture consumes technical EN/ES only. Do not produce and
        // recheck unrelated executive parents while an older version is retained.
        // M07 integration below separately retains the original four-edition parent.
        f.document.editions.retain(|e| e.id.ends_with("-technical"));
        for performance in &mut f.document.performances {
            performance
                .scope
                .edition_ids
                .retain(|id| id.ends_with("-technical"));
        }
        Self::from_fixture(f)
    }
    fn from_fixture(f: Fixture) -> Self {
        let root = f.temp.path().to_path_buf();
        let runtime = producer::current_runtime().unwrap();
        let producer_pin = FileRef {
            path: "original-producer".into(),
            sha256: runtime.sha256.clone(),
            bytes: runtime.bytes,
        };
        let assets = Assets {
            root: root.clone(),
            runtime: producer_pin.clone(),
            executable: std::env::current_exe().unwrap(),
        };
        let produced = if f.output().exists() {
            serde_json::from_slice(&fs::read(f.output().join("receipt.json")).unwrap()).unwrap()
        } else {
            f.produce(&ProcessingPolicy::identity()).unwrap()
        };
        let dialogue = f.output();
        let mut checked = checker::check(
            checker::CheckRequest {
                document: &f.document,
                plan_bytes: &f.plan(),
                receipt_bytes: &fs::read(dialogue.join("receipt.json")).unwrap(),
                processing: &ProcessingPolicy::identity(),
                expected_runtime: &runtime,
            },
            &assets,
            &dialogue,
            producer::ProductionControl {
                timeout: Duration::from_secs(30),
                cancellation: || Ok(()),
            },
        )
        .unwrap();
        let access = root.join("access");
        checker::write_access(&mut checked, &access, || Ok(())).unwrap();
        let (font, family) = [
            ("C:/Windows/Fonts/arial.ttf", "Arial"),
            (
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "DejaVu Sans",
            ),
            (
                "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
                "Liberation Sans",
            ),
        ]
        .into_iter()
        .find(|(p, _)| Path::new(p).is_file())
        .expect("declared test font required");
        let font_pin = save(&root, "font.ttf", &fs::read(font).unwrap());
        let mut entries = Vec::new();
        for (i, module) in f.document.modules.iter().enumerate() {
            let pins: Vec<_> = ["en", "es"]
                .iter()
                .map(|language| {
                    let script = module.cues[0]
                        .scripts
                        .iter()
                        .find(|s| s.language == *language)
                        .unwrap();
                    save(
                        &root,
                        &format!("source-{i}-{language}.txt"),
                        script.text.as_bytes(),
                    )
                })
                .collect();
            entries.push(json!({"kind":"parallel-text","id":format!("chapter-{i}"),"title":format!("Chapter {}",i+1),"left":pins[0],"right":pins[1],"left_label":"English","right_label":"Español"}));
        }
        let print_bytes = serde_json::to_vec(&json!({"schema":"reel.print-collection.v1","title":"Reading fixture","author":"Synthetic example","notice":"Technical fixture only","private_only":true,"fonts":[font_pin],"body_font":family,"entries":entries,"limits":{"input_bytes":16777216,"output_bytes":16777216,"pages":16}})).unwrap();
        let print_request = save(&root, "print-request.json", &print_bytes);
        let print = root.join("print");
        reel_print::render_with_inputs(
            &print_bytes,
            |p| {
                assets.control_bytes(&FileRef {
                    path: p.path.clone(),
                    sha256: p.sha256.clone(),
                    bytes: p.bytes,
                })
            },
            &print,
        )
        .unwrap();
        let plan = compiler::compile_with_inputs(
            &f.document,
            &assets,
            &runtime,
            CompilationProfile::Engineering,
        )
        .unwrap();
        let editions = ["en-technical", "es-technical"]
            .iter()
            .map(|id| {
                let compiled = plan
                    .editions
                    .iter()
                    .find(|e| e.recipe.edition_id == *id)
                    .unwrap();
                let output = produced
                    .editions
                    .iter()
                    .find(|e| e.edition_id == *id)
                    .unwrap();
                let chapters = compiled
                    .chapters
                    .iter()
                    .enumerate()
                    .map(|(i, chapter)| Chapter {
                        module_id: chapter.module_id.clone(),
                        print_entry_id: format!("chapter-{i}"),
                        spans: compiled
                            .recipe
                            .cues
                            .iter()
                            .filter(|c| c.module_id == chapter.module_id)
                            .map(|cue| Span {
                                performance_id: cue.performance.id.clone(),
                                start_byte: 0,
                                end_byte: cue.script.text.len(),
                            })
                            .collect(),
                    })
                    .collect();
                Edition {
                    edition_id: id.to_string(),
                    language: compiled.recipe.language.clone(),
                    compiled_recipe_sha256: compiled.recipe_sha256.clone(),
                    execution_recipe_sha256: output.execution_recipe_sha256.clone(),
                    audio: Audio::Dialogue,
                    chapters,
                }
            })
            .collect();
        let request = Request {
            schema: reading::SCHEMA.into(),
            id: "public-engineering-reading".into(),
            declaration: save(
                &root,
                "declaration.json",
                &serde_json::to_vec(&f.document).unwrap(),
            ),
            producer: producer_pin,
            processing: None,
            dialogue_receipt: pinned(
                "receipt.json",
                &fs::read(dialogue.join("receipt.json")).unwrap(),
            ),
            access: pinned(
                "access.json",
                &fs::read(access.join("access.json")).unwrap(),
            ),
            print_request,
            print_receipt: pinned(
                "receipt.json",
                &fs::read(print.join("receipt.json")).unwrap(),
            ),
            language_columns: BTreeMap::from([
                ("en".into(), Column::Left),
                ("es".into(), Column::Right),
            ]),
            editions,
        };
        Self {
            f,
            assets,
            dialogue,
            access,
            print,
            layers: BTreeMap::new(),
            request,
        }
    }
    fn inputs(&self) -> reading::Inputs<'_> {
        reading::Inputs {
            assets: &self.assets,
            dialogue: &self.dialogue,
            access: &self.access,
            print: &self.print,
            layered: &self.layers,
        }
    }
    pub(super) fn prepare(&self) -> Result<reading::Prepared<'_>> {
        reading::prepare(self.request.clone(), self.inputs(), Duration::from_secs(30))
    }
}
fn publish(prepared: &reading::Prepared<'_>, output: &Path) -> Result<reading::Summary> {
    reading::produce(
        prepared,
        output,
        producer::ProductionControl {
            timeout: Duration::from_secs(30),
            cancellation: || Ok(()),
        },
    )
}

#[test]
#[ignore = "bounded native media/font fixture; explicitly invoked on both hosted operating systems"]
fn bilingual_source_order_clocks_pages_and_authored_text() {
    let f = ReadingFixture::new(false);
    let prepared = f.prepare().unwrap();
    let output = f.f.temp.path().join("reading");
    assert!(!output.exists());
    let summary = publish(&prepared, &output).unwrap();
    assert_eq!((summary.editions, summary.chapters), (2, 6));
    assert!(!summary.authority_verified && !summary.release_authorized);
    reading::check(&prepared, &output, Duration::from_secs(30)).unwrap();
    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("reading.json")).unwrap()).unwrap();
    assert_eq!(index["schema"], "reel.narrated-reading-index.v1");
    assert_eq!(index["request"]["schema"], "reel.narrated-reading.v1");
    let mut wrong_schema = f.request.clone();
    wrong_schema.schema = reading::INDEX_SCHEMA.into();
    assert!(reading::prepare(wrong_schema, f.inputs(), Duration::from_secs(30)).is_err());
    for (i, duration) in [508800, 590400].iter().enumerate() {
        assert_eq!(index["editions"][i]["duration_samples"], *duration);
        let clocks = [
            [(0, 144000), (158400, 350400), (364800, 508800)],
            [(0, 168000), (187200, 403200), (422400, 590400)],
        ];
        for chapter in 0..3 {
            assert_eq!(
                index["editions"][i]["chapters"][chapter]["pages"],
                json!([chapter + 2])
            );
            assert_eq!(
                index["editions"][i]["chapters"][chapter]["start_sample"],
                clocks[i][chapter].0
            );
            assert_eq!(
                index["editions"][i]["chapters"][chapter]["end_sample"],
                clocks[i][chapter].1
            );
        }
        let language = if i == 0 { "en" } else { "es" };
        let expected = (0..3)
            .map(|c| {
                fs::read_to_string(f.f.temp.path().join(format!("source-{c}-{language}.txt")))
                    .unwrap()
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        assert_eq!(
            fs::read_to_string(output.join(format!("reading-{i:03}.txt"))).unwrap(),
            expected
        );
    }
    assert!(publish(&prepared, &output).is_err());
    let mut false_index = index;
    false_index["editions"][0]["chapters"][0]["pages"] = json!([999]);
    fs::write(
        output.join("reading.json"),
        serde_json::to_vec_pretty(&false_index).unwrap(),
    )
    .unwrap();
    assert!(reading::check(&prepared, &output, Duration::from_secs(30)).is_err());
}

#[test]
#[ignore = "bounded native media/font fixture; explicitly invoked on both hosted operating systems"]
fn stale_language_recipe_order_spans_and_missing_mix_refuse() {
    let f = ReadingFixture::new(false);
    let mut cases = Vec::new();
    let mut r = f.request.clone();
    r.editions[0].language = "es".into();
    cases.push(r);
    let mut r = f.request.clone();
    let spanish = &f.f.document.modules[1].cues[0]
        .scripts
        .iter()
        .find(|s| s.language == "es")
        .unwrap()
        .text;
    r.editions[1].chapters[1].spans[0].end_byte = spanish
        .char_indices()
        .find(|(_, c)| !c.is_ascii())
        .unwrap()
        .0
        + 1;
    cases.push(r);
    let mut r = f.request.clone();
    r.editions[0].compiled_recipe_sha256 = "0".repeat(64);
    cases.push(r);
    let mut r = f.request.clone();
    r.editions[0].chapters.swap(0, 1);
    cases.push(r);
    let mut r = f.request.clone();
    r.editions[0].chapters.pop();
    cases.push(r);
    let mut r = f.request.clone();
    r.editions[0].chapters[0].spans[0].start_byte = 1;
    cases.push(r);
    let mut r = f.request.clone();
    r.editions[0].chapters[0].spans[0].end_byte = usize::MAX;
    cases.push(r);
    let mut r = f.request.clone();
    r.language_columns.insert("en".into(), Column::Right);
    cases.push(r);
    let mut r = f.request.clone();
    r.editions[0].audio = Audio::Layered {
        mix_identity_sha256: "0".repeat(64),
    };
    cases.push(r);
    let expected = [
        "language or parent recipe",
        "UTF-8 span boundary",
        "language or parent recipe",
        "chapter order",
        "chapter/cue coverage",
        "authored wording",
        "UTF-8 span boundary",
        "authored wording",
        "checked mix missing",
    ];
    for (r, expected) in cases.into_iter().zip(expected) {
        let error = reading::prepare(r, f.inputs(), Duration::from_secs(30))
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(expected), "expected {expected}: {error}");
    }
    let prepared = f.prepare().unwrap();
    let cancelled = f.f.temp.path().join("cancelled");
    assert!(
        reading::produce(
            &prepared,
            &cancelled,
            producer::ProductionControl {
                timeout: Duration::from_secs(30),
                cancellation: || bail!("cancelled")
            }
        )
        .is_err()
    );
    assert!(!cancelled.exists());
    assert!(reading::prepare(f.request.clone(), f.inputs(), Duration::ZERO).is_err());
}

#[test]
#[ignore = "bounded native media/font fixture; explicitly invoked on both hosted operating systems"]
fn earlier_package_reopens_with_retained_sources_after_v2() {
    let old = ReadingFixture::new(false);
    let old_prepared = old.prepare().unwrap();
    let old_output = old.f.temp.path().join("reading-v1");
    publish(&old_prepared, &old_output).unwrap();
    let new = ReadingFixture::new(true);
    let new_prepared = new.prepare().unwrap();
    assert_eq!(
        old.request.editions[0].compiled_recipe_sha256,
        new.request.editions[0].compiled_recipe_sha256
    );
    assert_ne!(
        old.request.editions[1].compiled_recipe_sha256,
        new.request.editions[1].compiled_recipe_sha256
    );
    let new_output = new.f.temp.path().join("reading-v2");
    publish(&new_prepared, &new_output).unwrap();
    assert_ne!(
        fs::read(old_output.join("reading.json")).unwrap(),
        fs::read(new_output.join("reading.json")).unwrap()
    );
    assert_eq!(
        fs::read(old_output.join("reading-000.txt")).unwrap(),
        fs::read(new_output.join("reading-000.txt")).unwrap()
    );
    assert_ne!(
        fs::read(old_output.join("reading-001.txt")).unwrap(),
        fs::read(new_output.join("reading-001.txt")).unwrap()
    );
    reading::check(&old_prepared, &old_output, Duration::from_secs(30)).unwrap();
    assert!(reading::check(&new_prepared, &old_output, Duration::from_secs(30)).is_err());
    fs::write(
        old.f.temp.path().join("source-1-es.txt"),
        b"changed original",
    )
    .unwrap();
    assert!(reading::check(&old_prepared, &old_output, Duration::from_secs(30)).is_err());
    reading::check(&new_prepared, &new_output, Duration::from_secs(30)).unwrap();
}

#[test]
#[ignore = "bounded native media/font fixture; explicitly invoked on both hosted operating systems"]
fn rehashed_false_print_audio_and_dangling_visual_refuse() {
    let f = ReadingFixture::new(false);
    let print_receipt_path = f.print.join("receipt.json");
    let original_receipt = fs::read(&print_receipt_path).unwrap();
    let mut receipt: reel_print::Receipt = serde_json::from_slice(&original_receipt).unwrap();
    let pdf_path = f.print.join("collection.pdf");
    let pdf = fs::read(&pdf_path).unwrap();
    let mut false_pdf = pdf.clone();
    false_pdf[0] ^= 1;
    fs::write(&pdf_path, &false_pdf).unwrap();
    receipt.outputs.insert(
        "collection.pdf".into(),
        reel_print::Object {
            sha256: hash(&false_pdf),
            bytes: false_pdf.len() as u64,
        },
    );
    let false_receipt = serde_json::to_vec_pretty(&receipt).unwrap();
    fs::write(&print_receipt_path, &false_receipt).unwrap();
    let mut request = f.request.clone();
    request.print_receipt = pinned("receipt.json", &false_receipt);
    let error = reading::prepare(request, f.inputs(), Duration::from_secs(30))
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("print packet object differs"), "{error}");
    fs::write(&pdf_path, pdf).unwrap();
    fs::write(&print_receipt_path, original_receipt).unwrap();

    let mut visual: serde_json::Value =
        serde_json::from_slice(&fs::read(f.assets.root.join("print-request.json")).unwrap())
            .unwrap();
    visual["entries"][0] = json!({"kind":"retained-pdf","id":"chapter-0","source":{"path":"dangling.pdf","sha256":"0".repeat(64),"bytes":1}});
    let mut request = f.request.clone();
    request.print_request = save(
        &f.assets.root,
        "visual-request.json",
        &serde_json::to_vec(&visual).unwrap(),
    );
    let error = reading::prepare(request, f.inputs(), Duration::from_secs(30))
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("visual reference unsupported"), "{error}");

    let receipt_path = f.dialogue.join("receipt.json");
    let mut receipt: producer::ProductionReceipt =
        serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    let wave = &mut receipt.editions[1].wave;
    let mut bytes = fs::read(f.dialogue.join(&wave.path)).unwrap();
    bytes[44] ^= 1;
    fs::write(f.dialogue.join(&wave.path), &bytes).unwrap();
    wave.sha256 = hash(&bytes);
    let false_receipt = serde_json::to_vec_pretty(&receipt).unwrap();
    fs::write(&receipt_path, &false_receipt).unwrap();
    let mut request = f.request.clone();
    request.dialogue_receipt = pinned("receipt.json", &false_receipt);
    assert!(reading::prepare(request, f.inputs(), Duration::from_secs(30)).is_err());
}

#[test]
#[ignore = "requires pinned FFmpeg; explicitly invoked on both hosted operating systems"]
fn checked_layered_audio_keeps_reading_clock_and_refuses_false_parent() {
    use reel::spoken_programme::layers as api;
    let (f, sources, runtime) = super::layers::fixture();
    let f = ReadingFixture::from_fixture(f);
    let policy = ProcessingPolicy::identity();
    let request = super::layers::request(&f.f, &sources, "en-technical");
    let mixed = api::prepare(
        api::Inputs {
            document: &f.f.document,
            processing: &policy,
            sources: &f.assets,
            dialogue_candidate: &f.dialogue,
            layers: &sources,
        },
        &request,
        runtime,
    )
    .unwrap();
    let mix_output = f.f.temp.path().join("checked-mix");
    let receipt = api::produce(&mixed, &mix_output, Duration::from_secs(30), || Ok(())).unwrap();
    let layers = BTreeMap::from([(
        "en-technical".into(),
        reading::Layered {
            prepared: &mixed,
            directory: &mix_output,
        },
    )]);
    let inputs = reading::Inputs {
        layered: &layers,
        ..f.inputs()
    };
    let mut request = f.request.clone();
    request.editions[0].audio = Audio::Layered {
        mix_identity_sha256: receipt.mix_identity_sha256.clone(),
    };
    let prepared = reading::prepare(request.clone(), inputs, Duration::from_secs(30)).unwrap();
    let output = f.f.temp.path().join("layered-reading");
    publish(&prepared, &output).unwrap();
    reading::check(&prepared, &output, Duration::from_secs(30)).unwrap();
    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("reading.json")).unwrap()).unwrap();
    assert_eq!(index["editions"][0]["audio_provider"], "layered");
    assert_eq!(
        index["editions"][0]["audio"]["sha256"],
        receipt.outputs["mix.wav"].sha256
    );
    assert_eq!(index["editions"][0]["duration_samples"], 508800);
    request.editions[0].audio = Audio::Layered {
        mix_identity_sha256: "0".repeat(64),
    };
    assert!(reading::prepare(request, inputs, Duration::from_secs(30)).is_err());
}
