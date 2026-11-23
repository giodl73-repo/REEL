//! Synthetic signals and lexical metadata test packaging, never acoustic fidelity.
use super::*;
use reel::spoken_programme::{
    PerformanceKind,
    checker::{self, CheckRequest, CheckedPackage, EntryKind},
};
use std::time::Duration;

fn checked(f: &Fixture, policy: &ProcessingPolicy) -> Result<CheckedPackage> {
    checker::check(
        CheckRequest {
            document: &f.document,
            plan_bytes: &f.plan(),
            receipt_bytes: &fs::read(f.output().join("receipt.json"))?,
            processing: policy,
            expected_runtime: &producer::current_runtime()?,
        },
        f.temp.path(),
        &f.output(),
        producer::ProductionControl {
            timeout: Duration::from_secs(30),
            cancellation: || Ok(()),
        },
    )
}

fn lexical_fixture() -> Fixture {
    let mut f = Fixture::new();
    for p in &mut f.document.performances {
        let text = &f
            .document
            .modules
            .iter()
            .find(|m| m.id == p.module_id)
            .unwrap()
            .cues
            .iter()
            .find(|c| c.id == p.cue_id)
            .unwrap()
            .scripts
            .iter()
            .find(|s| s.id == p.script_id)
            .unwrap()
            .text;
        p.scope.kind = PerformanceKind::RecordedSpeech;
        p.observed_text_sha256 = hash(text.as_bytes());
        p.markers.clear();
        let begin = if p.language == "es" { 4800 } else { 2400 };
        let words = text
            .split_whitespace()
            .enumerate()
            .map(|(i, word)| {
                let recognized = if i == 0 {
                    word.to_lowercase()
                } else {
                    word.to_owned()
                };
                json!({
                    "word":recognized,"start_sample":begin+i*3600,"end_sample":begin+1200+i*3600,
                })
            })
            .collect::<Vec<_>>();
        let evidence=serde_json::to_vec(&json!({"schema":compiler::ALIGNMENT_SCHEMA,
            "performance_id":p.id,"cue_id":p.cue_id,"language":p.language,"source":p.source,
            "sample_rate":48000,"duration_samples":p.duration_samples,"script_id":p.script_id,
            "script_sha256":p.script_sha256,"markers":[],"word_bindings":[],
            "observation":{"kind":"word-evidence","observed_text":text,"timing":{
                "schema":"reel.scene-word-timing-evidence.v1","language":p.language,"cue_id":p.cue_id,
                "selected_take_sha256":p.source.sha256,"sample_rate":48000,
                "cue_end_sample":p.duration_samples,"words":words}}})).unwrap();
        p.alignment.sha256 = hash(&evidence);
        p.alignment.bytes = evidence.len() as u64;
        fs::write(f.temp.path().join(&p.alignment.path), evidence).unwrap();
    }
    f
}

#[test]
fn checker_reopens_media_and_rejects_rehashed_tamper() {
    let f = Fixture::new();
    let policy = ProcessingPolicy::identity();
    f.produce(&policy).unwrap();
    let valid = checked(&f, &policy).unwrap();
    assert_eq!(valid.summary().editions, 4);
    assert!(valid.summary().independent_pcm_check_passed);
    assert!(!valid.summary().acoustic_word_fidelity_verified);
    assert!(
        valid
            .access()
            .editions
            .iter()
            .all(|e| e.word_evidence.is_empty())
    );
    assert!(valid.access().editions.iter().all(|e| {
        e.entries
            .iter()
            .all(|c| !matches!(c.kind, EntryKind::Spoken))
    }));
    let receipt_path = f.output().join("receipt.json");
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    let wave_path = f
        .output()
        .join(receipt["editions"][0]["wave"]["path"].as_str().unwrap());
    let mut wave = fs::read(&wave_path).unwrap();
    wave[90000] ^= 1;
    fs::write(wave_path, &wave).unwrap();
    receipt["editions"][0]["wave"]["sha256"] = json!(hash(&wave));
    receipt["editions"][0]["pcm_sha256"] = json!(hash(&wave[44..]));
    fs::write(receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let error = match checked(&f, &policy) {
        Ok(_) => panic!("false waveform accepted"),
        Err(e) => e,
    };
    assert!(format!("{error:#}").contains("independent PCM comparison failed"));
}

#[test]
fn four_transcripts_and_chapters_follow_edition_clock() {
    let f = lexical_fixture();
    let policy = ProcessingPolicy::identity();
    f.produce(&policy).unwrap();
    let mut result = checked(&f, &policy).unwrap();
    let expected = [
        ("en-executive", vec![(0, 144000), (156000, 300000)]),
        (
            "en-technical",
            vec![(0, 144000), (158400, 350400), (364800, 508800)],
        ),
        ("es-executive", vec![(0, 168000), (184800, 352800)]),
        (
            "es-technical",
            vec![(0, 168000), (187200, 403200), (422400, 590400)],
        ),
    ];
    for (id, spans) in expected {
        let e = result
            .access()
            .editions
            .iter()
            .find(|e| e.edition_id == id)
            .unwrap();
        let spoken = e
            .entries
            .iter()
            .filter(|c| matches!(c.kind, EntryKind::Spoken))
            .collect::<Vec<_>>();
        assert_eq!(
            spoken
                .iter()
                .map(|c| (c.start_sample, c.end_sample))
                .collect::<Vec<_>>(),
            spans
        );
        assert_eq!(
            e.chapters
                .iter()
                .map(|c| (c.start_sample, c.end_sample))
                .collect::<Vec<_>>(),
            spans
        );
        let selected = &f
            .document
            .editions
            .iter()
            .find(|e| e.id == id)
            .unwrap()
            .selections;
        for (entry, selection) in spoken.iter().zip(selected) {
            let p = f
                .document
                .performances
                .iter()
                .find(|p| p.id == selection.takes[0].performance_id)
                .unwrap();
            let text = &f
                .document
                .modules
                .iter()
                .find(|m| m.id == p.module_id)
                .unwrap()
                .cues[0]
                .scripts
                .iter()
                .find(|s| s.id == p.script_id)
                .unwrap()
                .text;
            assert_eq!(&entry.text, text);
            assert_eq!(entry.text_basis.as_deref(), Some("selected-script"));
            let offset = if e.language == "es" { 4800 } else { 2400 };
            assert!(e.word_evidence.iter().any(|word| word.start_sample
                == entry.start_sample + offset
                && Some(word.speaker) == entry.speaker
                && word.evidence_text == text.split_whitespace().next().unwrap().to_lowercase()));
        }
        assert_eq!(spoken.first().unwrap().speaker, Some(1));
        assert_eq!(spoken.last().unwrap().speaker, Some(1));
        if spoken.len() == 3 {
            assert_eq!(spoken[1].speaker, Some(2));
        }
        assert_eq!(
            e.entries
                .iter()
                .filter(|c| matches!(c.kind, EntryKind::AddedPause))
                .count(),
            spans.len() - 1
        );
    }
    let root = f.temp.path().join("access");
    checker::write_access(&mut result, &root, || Ok(())).unwrap();
    assert!(result.summary().access_checked);
    checker::verify_access(
        &mut result,
        &fs::read(root.join("access.json")).unwrap(),
        &root,
        || Ok(()),
    )
    .unwrap();
    for e in &result.access().editions {
        let text = fs::read_to_string(root.join(&e.transcript.path)).unwrap();
        assert!(text.contains("Speaker 1:"));
        assert!(!text.contains("en-opening"));
    }
}

#[test]
fn access_rehashes_cannot_change_words_roles_chapters_or_bindings() {
    let f = lexical_fixture();
    let policy = ProcessingPolicy::identity();
    f.produce(&policy).unwrap();
    let mut result = checked(&f, &policy).unwrap();
    let root = f.temp.path().join("access");
    checker::write_access(&mut result, &root, || Ok(())).unwrap();
    let original: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("access.json")).unwrap()).unwrap();
    let mut mutations = Vec::new();
    let mut bad = original.clone();
    bad["editions"][0]["entries"][0]["text"] = json!("wrong words");
    mutations.push(bad);
    let mut bad = original.clone();
    bad["editions"][0]["entries"][0]["speaker"] = json!(2);
    mutations.push(bad);
    let mut bad = original.clone();
    bad["editions"][0]["entries"][0]["start_sample"] = json!(1);
    mutations.push(bad);
    let mut bad = original.clone();
    bad["editions"][0]["chapters"][0]["end_sample"] = json!(144001);
    mutations.push(bad);
    let mut bad = original.clone();
    bad["editions"][0]["word_evidence"][0]["start_sample"] = json!(1);
    mutations.push(bad);
    let mut bad = original.clone();
    bad["editions"][0]["wave"]["sha256"] = json!(hash(b"wrong"));
    mutations.push(bad);
    let mut bad = original.clone();
    bad["editions"].as_array_mut().unwrap().swap(0, 1);
    mutations.push(bad);
    let first = &result.access().editions[0].transcript.path;
    let changed = b"changed transcript";
    fs::write(root.join(first), changed).unwrap();
    let mut bad = original.clone();
    bad["editions"][0]["transcript"]["sha256"] = json!(hash(changed));
    bad["editions"][0]["transcript"]["bytes"] = json!(changed.len());
    mutations.push(bad);
    for bad in mutations {
        let manifest = serde_json::to_vec_pretty(&bad).unwrap();
        // The changed transcript's pin above is consistent with its bytes;
        // semantic comparison still uses the independently derived content.
        assert!(checker::verify_access(&mut result, &manifest, &root, || Ok(())).is_err());
        assert!(!result.summary().access_checked);
    }
    assert!(
        checker::verify_access(
            &mut result,
            &serde_json::to_vec(&original).unwrap(),
            &root,
            || Ok(())
        )
        .is_err()
    );
}

#[test]
fn self_consistent_changed_source_and_placement_do_not_replace_original_controls() {
    let f = Fixture::new();
    let policy = ProcessingPolicy::identity();
    let runtime = producer::current_runtime().unwrap();
    let mut changed_pause = f.document.clone();
    changed_pause.editions[0].joins[0].added_pause_samples += 1;
    let mut changed_source = f.document.clone();
    let alternate = changed_source.performances[2].source.clone();
    let p = &mut changed_source.performances[0];
    let mut metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(f.temp.path().join(&p.alignment.path)).unwrap()).unwrap();
    p.source = alternate;
    metadata["source"] = json!(p.source);
    let bytes = serde_json::to_vec(&metadata).unwrap();
    p.alignment.path = "alternate-source-metadata.json".into();
    p.alignment.sha256 = hash(&bytes);
    p.alignment.bytes = bytes.len() as u64;
    fs::write(f.temp.path().join(&p.alignment.path), bytes).unwrap();
    for (index, changed) in [changed_pause, changed_source].iter().enumerate() {
        let plan = serde_json::to_vec_pretty(
            &compiler::compile_with_inputs(
                changed,
                f.temp.path(),
                &runtime,
                CompilationProfile::Engineering,
            )
            .unwrap(),
        )
        .unwrap();
        let candidate = f.temp.path().join(format!("self-consistent-{index}"));
        // A real producer recomputes every recipe, source, selected/processed,
        // container and PCM hash for the adversarial alternate controls.
        producer::produce(
            changed,
            &plan,
            &policy,
            f.temp.path(),
            &candidate,
            || Ok(()),
        )
        .unwrap();
        let receipt = fs::read(candidate.join("receipt.json")).unwrap();
        let error = match checker::check(
            CheckRequest {
                document: &f.document,
                plan_bytes: &plan,
                receipt_bytes: &receipt,
                processing: &policy,
                expected_runtime: &runtime,
            },
            f.temp.path(),
            &candidate,
            producer::ProductionControl {
                timeout: Duration::from_secs(30),
                cancellation: || Ok(()),
            },
        ) {
            Ok(_) => panic!("alternate controls replaced original contract"),
            Err(error) => error,
        };
        assert!(format!("{error:#}").contains("plan differs from controls/runtime"));
    }
}

#[test]
fn runtime_source_order_and_processing_tamper_refuse() {
    let f = Fixture::new();
    let policy = ProcessingPolicy::identity();
    f.produce(&policy).unwrap();
    let path = f.output().join("receipt.json");
    let original = fs::read(&path).unwrap();
    let receipt: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let mut mutations = Vec::new();
    let mut bad = receipt.clone();
    bad["producer"]["sha256"] = json!(hash(b"wrong runtime"));
    mutations.push(bad);
    let mut bad = receipt.clone();
    bad["editions"].as_array_mut().unwrap().swap(0, 1);
    mutations.push(bad);
    let mut bad = receipt.clone();
    bad["editions"][0]["takes"][0]["source"]["sha256"] = json!(hash(b"wrong source"));
    mutations.push(bad);
    let mut bad = receipt.clone();
    bad["editions"][0]["takes"][0]["processing"]["gain"]["numerator"] = json!(2);
    mutations.push(bad);
    for bad in mutations {
        fs::write(&path, serde_json::to_vec_pretty(&bad).unwrap()).unwrap();
        assert!(checked(&f, &policy).is_err());
    }
    fs::write(&path, original).unwrap();
    assert!(checked(&f, &policy).is_ok());
    fs::remove_file(f.output().join("edition-000.wav")).unwrap();
    assert!(checked(&f, &policy).is_err());
    fs::remove_file(&path).unwrap();
    assert!(checked(&f, &policy).is_err());
}

#[test]
fn access_publication_interruption_and_existing_output_preserve_files() {
    let f = Fixture::new();
    let policy = ProcessingPolicy::identity();
    f.produce(&policy).unwrap();
    let mut result = checked(&f, &policy).unwrap();
    let root = f.temp.path().join("access");
    assert!(
        checker::write_access(&mut result, &root, || {
            if root.join("edition-000.txt").exists() {
                fs::write(root.join("foreign"), b"preserve")?;
                bail!("interrupted");
            }
            Ok(())
        })
        .is_err()
    );
    assert!(!root.join("access.json").exists());
    assert!(!root.join("edition-000.txt").exists());
    assert_eq!(fs::read(root.join("foreign")).unwrap(), b"preserve");
    assert!(checker::write_access(&mut result, &root, || Ok(())).is_err());
    let receipt = fs::read(f.output().join("receipt.json")).unwrap();
    assert!(
        checker::check(
            CheckRequest {
                document: &f.document,
                plan_bytes: &f.plan(),
                receipt_bytes: &receipt,
                processing: &policy,
                expected_runtime: &producer::current_runtime().unwrap()
            },
            f.temp.path(),
            &f.output(),
            producer::ProductionControl {
                timeout: Duration::ZERO,
                cancellation: || Ok(())
            }
        )
        .is_err()
    );
}

#[test]
fn independent_checker_accepts_declared_envelopes_and_overlap() {
    let mut f = Fixture::new();
    let join = &mut f
        .document
        .editions
        .iter_mut()
        .find(|e| e.id == "en-executive")
        .unwrap()
        .joins[0];
    join.added_pause_samples = 0;
    join.overlap_samples = 1000;
    f.document.performances[0]
        .protected
        .retain(|r| !matches!(r.kind, reel::spoken_programme::ProtectedKind::Tail));
    f.signals[0].fill(101);
    f.signals[2].fill(-203);
    f.sync();
    let policy = ProcessingPolicy {
        schema: producer::PROCESSING_SCHEMA.into(),
        operations: vec![
            CueProcessing {
                edition_id: "en-executive".into(),
                performance_id: "en-opening".into(),
                gain: RationalGain {
                    numerator: 1,
                    denominator: 2,
                },
                fade_in_samples: 100,
                fade_out_samples: 1000,
            },
            CueProcessing {
                edition_id: "en-executive".into(),
                performance_id: "en-closing".into(),
                gain: RationalGain {
                    numerator: 1,
                    denominator: 3,
                },
                fade_in_samples: 0,
                fade_out_samples: 0,
            },
        ],
    };
    f.produce(&policy).unwrap();
    let mut result = checked(&f, &policy).unwrap();
    let edition = result
        .access()
        .editions
        .iter()
        .find(|e| e.edition_id == "en-executive")
        .unwrap();
    assert_eq!(edition.entries[1].overlap_before_samples, 1000);
    assert_eq!(edition.entries[1].start_sample, 143000);
    let actual = decode(&fs::read(f.output().join(&edition.wave.path)).unwrap());
    assert_eq!(
        [actual[50 * 2], actual[143500 * 2], actual[145000 * 2]],
        [25, -43, -67]
    );
    let root = f.temp.path().join("access");
    checker::write_access(&mut result, &root, || Ok(())).unwrap();
    assert!(
        fs::read_to_string(root.join("edition-000.txt"))
            .unwrap()
            .contains("Intentional overlap: 1000 samples")
    );
}

#[test]
fn spoken_profile_check_uses_the_native_pcm_full_scale() {
    let mut f = Fixture::new();
    f.signals[0][6000] = 7_476_354;
    f.sync();
    let runtime = producer::current_runtime().unwrap();
    let policy = ProcessingPolicy::identity();
    let plan = serde_json::to_vec_pretty(
        &compiler::compile_with_inputs(
            &f.document,
            f.temp.path(),
            &runtime,
            CompilationProfile::Spoken,
        )
        .unwrap(),
    )
    .unwrap();
    producer::produce(
        &f.document,
        &plan,
        &policy,
        f.temp.path(),
        &f.output(),
        || Ok(()),
    )
    .unwrap();
    let result = checker::check(
        CheckRequest {
            document: &f.document,
            plan_bytes: &plan,
            receipt_bytes: &fs::read(f.output().join("receipt.json")).unwrap(),
            processing: &policy,
            expected_runtime: &runtime,
        },
        f.temp.path(),
        &f.output(),
        producer::ProductionControl {
            timeout: Duration::from_secs(30),
            cancellation: || Ok(()),
        },
    )
    .unwrap();
    assert!(result.summary().independent_pcm_check_passed);
}

#[test]
fn portable_output_redacts_canaries() {
    let mut f = Fixture::new();
    f.document.id = "PRIVATE-CHECK-PROGRAMME-CANARY".into();
    let declaration = f.temp.path().join("PRIVATE-CHECK-PATH-CANARY.json");
    fs::write(&declaration, serde_json::to_vec(&f.document).unwrap()).unwrap();
    let plan = f.temp.path().join("plan.json");
    let compile = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("spoken-edition-compile")
        .arg(&declaration)
        .arg("--output")
        .arg(&plan)
        .output()
        .unwrap();
    assert!(compile.status.success());
    let produce = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("spoken-edition-produce")
        .arg(&plan)
        .arg("--declaration")
        .arg(&declaration)
        .arg("--asset-root")
        .arg(f.temp.path())
        .arg("--output")
        .arg(f.output())
        .output()
        .unwrap();
    assert!(
        produce.status.success(),
        "{}",
        String::from_utf8_lossy(&produce.stderr)
    );
    let access = f.temp.path().join("access");
    let invoke = |mode: &str| {
        Command::new(env!("CARGO_BIN_EXE_reel"))
            .arg("spoken-edition-check")
            .arg(&declaration)
            .arg(&plan)
            .arg("--asset-root")
            .arg(f.temp.path())
            .arg("--candidate")
            .arg(f.output())
            .arg(mode)
            .arg(&access)
            .output()
            .unwrap()
    };
    let first = invoke("--write-access");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let summary: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(summary["access_checked"], true);
    let verify = invoke("--access");
    assert!(verify.status.success());
    let fail = invoke("--write-access");
    assert!(!fail.status.success());
    for output in [first, verify, fail] {
        let normal = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for canary in [
            "PRIVATE-CHECK",
            f.temp.path().to_str().unwrap(),
            "en-opening",
            "en-executive",
        ] {
            assert!(!normal.contains(canary));
        }
    }
}
