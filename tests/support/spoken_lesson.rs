//! Public engineering package only. No played repeat or human approval.
use super::*;
#[path = "../../crates/reel-music/tests/support/lesson_fixture.rs"]
mod score_fixture;
use reel::{
    music_score,
    scene_delivery::{FileRef, InputFiles},
    spoken_programme::{lesson as api, reading as reading_api},
};
use reel_music::{export, model, time::RoundingMode, validation_inputs::ValidationInputs};
use score_fixture::ScoreFixture;
use std::{path::Path, time::Duration};

fn pin(path: impl Into<PathBuf>, raw: &[u8]) -> FileRef {
    FileRef {
        path: path.into(),
        sha256: hash(raw),
        bytes: raw.len() as u64,
    }
}
fn rendered_score(ppq: u64, tempo: u64) -> ScoreFixture {
    let mut f = ScoreFixture::with_clock(ppq, tempo);
    let plan = f.temp.path().join("plan.json");
    export::write(&f.model(), &plan).unwrap();
    let packet = f.temp.path().join("packet");
    music_score::render(&plan, &f.model(), &packet).unwrap();
    f.files.insert("plan.json".into(), fs::read(plan).unwrap());
    let receipt: music_score::ExportReceipt =
        serde_json::from_slice(&fs::read(packet.join("receipt.json")).unwrap()).unwrap();
    for name in receipt
        .artifacts
        .iter()
        .map(|a| a.filename.as_str())
        .chain(["receipt.json"])
    {
        f.files.insert(
            PathBuf::from("packet").join(name),
            fs::read(packet.join(name)).unwrap(),
        );
    }
    f
}
fn request(f: &mut ScoreFixture, book: &Path, ppq: u32) -> api::Request {
    let score_files = f.files.iter().map(|(name, raw)| pin(name, raw)).collect();
    let mut instructions = Vec::new();
    for (language, text) in [
        ("en", "Play the pickup once, then practice bar A twice."),
        (
            "es",
            "Toca la anacrusa una vez y practica el compás A dos veces.",
        ),
    ] {
        let name = format!("instructions-{language}.txt");
        let authority = format!("instruction-scope-{language}.txt");
        let declaration=b"Original public engineering fixture only; no performance, language or release approval.";
        fs::write(f.temp.path().join(&name), text).unwrap();
        fs::write(f.temp.path().join(&authority), declaration).unwrap();
        f.files
            .insert(name.clone().into(), text.as_bytes().to_vec());
        f.files
            .insert(authority.clone().into(), declaration.to_vec());
        instructions.push(api::Instruction {
            language: language.into(),
            source: pin(name, text.as_bytes()),
            authority: pin(authority, declaration),
        });
    }
    let report = model::validate(&f.model()).unwrap();
    api::Request {
        schema: api::SCHEMA.into(),
        id: "fixture-private-lesson-canary".into(),
        reading_index: pin(
            "reading.json",
            &fs::read(book.join("reading.json")).unwrap(),
        ),
        edition_ids: vec!["en-technical".into(), "es-technical".into()],
        instructions,
        score_files,
        model: "model.json".into(),
        plan: "plan.json".into(),
        receipt: "packet/receipt.json".into(),
        packet: "packet".into(),
        score_contract_sha256: report.contract_sha256,
        ppq,
        frame_rounding: RoundingMode::HalfAwayFromZero,
        occurrences: vec![
            api::Occurrence {
                id: "pickup-once".into(),
                section_id: "pickup".into(),
            },
            api::Occurrence {
                id: "A-pass-1".into(),
                section_id: "A".into(),
            },
            api::Occurrence {
                id: "A-pass-2".into(),
                section_id: "A".into(),
            },
        ],
        anchors: vec![
            api::Anchor {
                id: "first-entry".into(),
                occurrence_id: "A-pass-1".into(),
                note_id: "A-two".into(),
                source_tick: 720,
            },
            api::Anchor {
                id: "second-entry".into(),
                occurrence_id: "A-pass-2".into(),
                note_id: "A-two".into(),
                source_tick: 720,
            },
        ],
    }
}
fn book(f: &reading::ReadingFixture, prepared: &reading_api::Prepared<'_>) -> PathBuf {
    let output = f.f.temp.path().join("lesson-reading");
    reading_api::produce(
        prepared,
        &output,
        producer::ProductionControl {
            timeout: Duration::from_secs(30),
            cancellation: || Ok(()),
        },
    )
    .unwrap();
    output
}
fn publish(prepared: &api::Prepared<'_>, output: &Path) -> Result<api::Summary> {
    api::produce(
        prepared,
        output,
        producer::ProductionControl {
            timeout: Duration::from_secs(30),
            cancellation: || Ok(()),
        },
    )
}

pub(super) fn check_shared_programme_lesson(
    reading: &reading_api::Prepared<'_>,
    book: &Path,
    output: &Path,
) {
    let mut score = rendered_score(480, 500_000);
    let request = request(&mut score, book, 480);
    let book_sha256 = hash(&fs::read(book.join("reading.json")).unwrap());
    let assets = score.temp.path().to_path_buf();
    let prepared = api::prepare(
        request,
        api::Inputs {
            reading,
            reading_directory: book,
            assets: &assets,
        },
        Duration::from_secs(30),
    )
    .unwrap();
    let summary = publish(&prepared, output).unwrap();
    api::check(&prepared, output, Duration::from_secs(30)).unwrap();
    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("lesson.json")).unwrap()).unwrap();
    assert_eq!(index["request"]["reading_index"]["sha256"], book_sha256);
    assert_eq!((summary.editions, summary.occurrences), (2, 3));
}

#[test]
#[ignore = "bounded reading/notation fixture; explicitly invoked on both hosted operating systems"]
fn actual_score_exports_and_bilingual_lesson_follow_pickup_repeat_goldens() {
    let mut score = rendered_score(480, 500000);
    let f = reading::ReadingFixture::new(false);
    let reading = f.prepare().unwrap();
    let book = book(&f, &reading);
    let request = request(&mut score, &book, 480);
    let assets = score.temp.path().to_path_buf();
    let inputs = api::Inputs {
        reading: &reading,
        reading_directory: &book,
        assets: &assets,
    };
    let prepared = api::prepare(request, inputs, Duration::from_secs(30)).unwrap();
    let output = f.f.temp.path().join("lesson");
    assert!(!output.exists());
    let summary = publish(&prepared, &output).unwrap();
    api::check(&prepared, &output, Duration::from_secs(30)).unwrap();
    assert_eq!(
        (summary.editions, summary.occurrences, summary.anchors),
        (2, 3, 2)
    );
    let report = serde_json::to_string(&summary).unwrap();
    assert!(!report.contains("canary"));
    assert!(
        !summary.authority_verified
            && !summary.musical_accuracy_verified
            && !summary.device_playback_checked
            && !summary.release_authorized
    );
    let raw = fs::read(output.join("lesson.json")).unwrap();
    let index: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(index["schema"], "reel.lesson-index.v1");
    assert_eq!(index["exercise_samples"], 204000);
    assert_eq!(
        index["reference"]["sha256"],
        hash(&score.files[&PathBuf::from("packet/rehearsal-guide.wav")])
    );
    for (i, (source, exercise)) in [
        ([0, 12000], [0, 12000]),
        ([12000, 108000], [12000, 108000]),
        ([12000, 108000], [108000, 204000]),
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(index["occurrences"][i]["source_samples"], json!(source));
        assert_eq!(index["occurrences"][i]["exercise_samples"], json!(exercise));
    }
    assert_eq!(index["anchors"][0]["exercise_sample"], 36000);
    assert_eq!(index["anchors"][1]["exercise_sample"], 132000);
    assert_eq!(
        index["instructions"][1]["text"],
        "Toca la anacrusa una vez y practica el compás A dos veces."
    );
    assert!(publish(&prepared, &output).is_err());
    assert_eq!(fs::read(output.join("lesson.json")).unwrap(), raw);
    let mut false_index = index;
    false_index["exercise_samples"] = json!(1);
    fs::write(
        output.join("lesson.json"),
        serde_json::to_vec(&false_index).unwrap(),
    )
    .unwrap();
    assert!(api::check(&prepared, &output, Duration::from_secs(30)).is_err());
}

#[test]
#[ignore = "bounded reading/notation fixture; explicitly invoked on both hosted operating systems"]
fn half_frame_rounding_and_original_score_packet_callbacks() {
    let mut score = rendered_score(960, 500010);
    let legacy = music_score::check(
        &score.temp.path().join("packet/receipt.json"),
        &score.temp.path().join("plan.json"),
        &score.model(),
        &score.temp.path().join("packet"),
    )
    .unwrap();
    let mut read = |path: &Path| score.bytes(path);
    let mut reader = ValidationInputs::admitted(&mut read);
    let checked = music_score::check_with_inputs(
        Path::new("packet/receipt.json"),
        Path::new("plan.json"),
        Path::new("model.json"),
        Path::new("packet"),
        &mut reader,
    )
    .unwrap();
    assert_eq!(legacy.receipt_sha256, checked.receipt_sha256);
    assert!(
        checked.midi_round_trip && checked.musicxml_round_trip && checked.rehearsal_guide_valid
    );
    let f = reading::ReadingFixture::new(false);
    let reading = f.prepare().unwrap();
    let book = book(&f, &reading);
    let mut request = request(&mut score, &book, 960);
    request.anchors = vec![api::Anchor {
        id: "fraction".into(),
        occurrence_id: "A-pass-1".into(),
        note_id: "A-one".into(),
        source_tick: 1000,
    }];
    let assets = score.temp.path().to_path_buf();
    let prepared = api::prepare(
        request,
        api::Inputs {
            reading: &reading,
            reading_directory: &book,
            assets: &assets,
        },
        Duration::from_secs(30),
    )
    .unwrap();
    let output = f.f.temp.path().join("rounded-lesson");
    publish(&prepared, &output).unwrap();
    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("lesson.json")).unwrap()).unwrap();
    assert_eq!(index["anchors"][0]["source_sample"], 25001);
}

#[test]
#[ignore = "bounded reading/notation fixture; explicitly invoked on both hosted operating systems"]
fn malformed_traversal_stale_clock_and_interrupted_publication_refuse() {
    let mut score = rendered_score(480, 500000);
    let f = reading::ReadingFixture::new(false);
    let reading = f.prepare().unwrap();
    let book = book(&f, &reading);
    let valid = request(&mut score, &book, 480);
    let assets = score.temp.path().to_path_buf();
    let inputs = api::Inputs {
        reading: &reading,
        reading_directory: &book,
        assets: &assets,
    };
    let mut cases = Vec::new();
    macro_rules! bad {
        ($message:expr,$edit:expr) => {{
            let mut r = valid.clone();
            ($edit)(&mut r);
            cases.push((r, $message));
        }};
    }
    bad!("duplicated", |r: &mut api::Request| r.occurrences[2].id =
        "A-pass-1".into());
    bad!("skips or rearranges", |r: &mut api::Request| {
        r.occurrences.remove(0);
    });
    bad!("omits written", |r: &mut api::Request| r
        .occurrences
        .truncate(1));
    bad!("unknown score section", |r: &mut api::Request| r
        .occurrences[1]
        .section_id =
        "absent".into());
    bad!("absent or ambiguous", |r: &mut api::Request| r.anchors[0]
        .occurrence_id
        .clear());
    bad!("note is unknown", |r: &mut api::Request| r.anchors[0]
        .note_id =
        "absent".into());
    bad!("outside section or note", |r: &mut api::Request| r
        .anchors[0]
        .source_tick =
        u64::MAX);
    bad!("timebase or rounding", |r: &mut api::Request| r.ppq = 960);
    bad!("timebase or rounding", |r: &mut api::Request| r
        .frame_rounding =
        RoundingMode::Floor);
    bad!("contract is stale", |r: &mut api::Request| r
        .score_contract_sha256 =
        "0".repeat(64));
    bad!("languages differ", |r: &mut api::Request| r.instructions
        [0]
    .language =
        "fr".into());
    bad!("unused inputs", |r: &mut api::Request| r
        .score_files
        .push(pin("unused", b"x")));
    for (r, expected) in cases {
        let error = api::prepare(r, inputs, Duration::from_secs(30))
            .err()
            .unwrap();
        assert!(
            format!("{error:#}").contains(expected),
            "expected {expected}: {error:#}"
        );
    }
    assert!(api::prepare(valid.clone(), inputs, Duration::ZERO).is_err());
    let prepared = api::prepare(valid, inputs, Duration::from_secs(30)).unwrap();
    let output = f.f.temp.path().join("cancelled-lesson");
    assert!(
        api::produce(
            &prepared,
            &output,
            producer::ProductionControl {
                timeout: Duration::from_secs(30),
                cancellation: || {
                    if output.exists() {
                        bail!("interrupt final marker")
                    };
                    Ok(())
                }
            }
        )
        .is_err()
    );
    assert!(!output.exists());
}

#[test]
#[ignore = "bounded reading/notation fixture; explicitly invoked on both hosted operating systems"]
fn rehashed_false_midi_and_receipt_cannot_replace_the_original_model() {
    let mut score = rendered_score(480, 500000);
    let f = reading::ReadingFixture::new(false);
    let reading = f.prepare().unwrap();
    let book = book(&f, &reading);
    let mut request = request(&mut score, &book, 480);
    let assets = score.temp.path().to_path_buf();
    let inputs = api::Inputs {
        reading: &reading,
        reading_directory: &book,
        assets: &assets,
    };
    let original = api::prepare(request.clone(), inputs, Duration::from_secs(30)).unwrap();
    let output = f.f.temp.path().join("original-lesson");
    publish(&original, &output).unwrap();
    let path = score.temp.path().join("packet/score.mid");
    let mut midi = fs::read(&path).unwrap();
    let mut changed = 0;
    for i in 0..midi.len() - 2 {
        if (midi[i] & 0xf0 == 0x90 || midi[i] & 0xf0 == 0x80) && midi[i + 1] == 60 {
            midi[i + 1] = 61;
            changed += 1;
        }
    }
    assert!(changed >= 2);
    fs::write(&path, &midi).unwrap();
    let receipt_path = score.temp.path().join("packet/receipt.json");
    let mut receipt: music_score::ExportReceipt =
        serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    receipt
        .artifacts
        .iter_mut()
        .find(|a| a.filename == "score.mid")
        .unwrap()
        .sha256 = hash(&midi);
    let receipt_raw = serde_json::to_vec_pretty(&receipt).unwrap();
    fs::write(&receipt_path, &receipt_raw).unwrap();
    for p in &mut request.score_files {
        if p.path == Path::new("packet/score.mid") {
            *p = pin(&p.path, &midi);
        } else if p.path == Path::new("packet/receipt.json") {
            *p = pin(&p.path, &receipt_raw);
        }
    }
    let error = api::prepare(request, inputs, Duration::from_secs(30))
        .err()
        .unwrap();
    assert!(
        format!("{error:#}").contains("independent round-trip"),
        "{error:#}"
    );
    assert!(api::check(&original, &output, Duration::from_secs(30)).is_err());
}

struct MemoryAssets {
    files: std::cell::RefCell<BTreeMap<PathBuf, Vec<u8>>>,
}
impl InputFiles for MemoryAssets {
    fn verified_path(&self, _: &FileRef) -> Result<PathBuf> {
        bail!("memory fixture offers no filesystem view")
    }
    fn control_bytes(&self, pin: &FileRef) -> Result<Vec<u8>> {
        self.files
            .borrow()
            .get(&pin.path)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("retained fixture byte is missing"))
    }
}
#[test]
#[ignore = "bounded reading/notation fixture; explicitly invoked on both hosted operating systems"]
fn retained_lesson_reopens_without_original_score_directories_after_v2() {
    let mut first = rendered_score(480, 500000);
    let mut second = rendered_score(480, 600000);
    let f = reading::ReadingFixture::new(false);
    let reading = f.prepare().unwrap();
    let book = book(&f, &reading);
    let old_request = request(&mut first, &book, 480);
    let new_request = request(&mut second, &book, 480);
    let old_root = first.temp.path().to_path_buf();
    let new_root = second.temp.path().to_path_buf();
    let old = MemoryAssets {
        files: std::cell::RefCell::new(first.files.clone()),
    };
    let new = MemoryAssets {
        files: std::cell::RefCell::new(second.files.clone()),
    };
    drop(first);
    drop(second);
    assert!(!old_root.exists() && !new_root.exists());
    let inputs = |assets| api::Inputs {
        reading: &reading,
        reading_directory: &book,
        assets,
    };
    let v1 = api::prepare(old_request.clone(), inputs(&old), Duration::from_secs(30)).unwrap();
    let old_output = f.f.temp.path().join("v1");
    publish(&v1, &old_output).unwrap();
    let v2 = api::prepare(new_request, inputs(&new), Duration::from_secs(30)).unwrap();
    let new_output = f.f.temp.path().join("v2");
    publish(&v2, &new_output).unwrap();
    assert_ne!(
        fs::read(old_output.join("lesson.json")).unwrap(),
        fs::read(new_output.join("lesson.json")).unwrap()
    );
    let reopened = api::prepare(old_request, inputs(&old), Duration::from_secs(30)).unwrap();
    api::check(&reopened, &old_output, Duration::from_secs(30)).unwrap();
    assert!(api::check(&v2, &old_output, Duration::from_secs(30)).is_err());
    old.files.borrow_mut().remove(Path::new("source.raw"));
    assert!(api::check(&reopened, &old_output, Duration::from_secs(30)).is_err());
    api::check(&v2, &new_output, Duration::from_secs(30)).unwrap();
}
