//! Lexical metadata only. These records assert no recorded acoustic performance.
use super::*;

#[test]
fn binding_count_refuses_before_provider_access() {
    struct Refusing(std::cell::Cell<usize>);
    impl InputFiles for Refusing {
        fn verified_path(&self, _: &FileRef) -> Result<std::path::PathBuf> {
            self.0.set(self.0.get() + 1);
            anyhow::bail!("unexpected provider access")
        }
    }
    let provider = Refusing(std::cell::Cell::new(0));
    let doc: super::super::Document = serde_json::from_str(include_str!(
        "../../../tests/fixtures/spoken-programme/four-editions.json"
    ))
    .unwrap();
    let runtime = compiler::CompilerIdentity {
        sha256: sha(b"metadata-only"),
        bytes: 1,
    };
    let policy = producer::ProcessingPolicy::identity();
    let inputs = Inputs {
        spoken: checker::CheckRequest {
            document: &doc,
            plan_bytes: &[],
            receipt_bytes: &[],
            processing: &policy,
            expected_runtime: &runtime,
        },
        sources: &provider,
        candidate: &provider,
        visuals: &provider,
        access: &provider,
        access_manifest: &[],
    };
    let request = Request {
        schema: SCHEMA.into(),
        edition_ids: vec!["en-technical".into(), "es-technical".into()],
        svg: pin("fixture.svg", b"fixture"),
        width: 8,
        height: 8,
        events: ["first", "second"]
            .into_iter()
            .map(|id| Event {
                id: id.into(),
                bindings: (0..20)
                    .map(|_| Binding {
                        edition_id: "en-technical".into(),
                        anchor: Anchor::EngineeringMarker {
                            performance_id: "fixture".into(),
                            marker_id: "marker".into(),
                        },
                    })
                    .collect(),
            })
            .collect(),
    };
    let error = prepare(request, inputs, Duration::from_secs(30))
        .err()
        .unwrap();
    assert!(error.to_string().contains("binding closure"));
    assert_eq!(provider.0.get(), 0);
}

fn lexical(es: bool) -> (compiler::AlignmentEvidence, String) {
    let (text, first, second, duration) = if es {
        (
            "Escucha Ahora sigue el ritmo Después vuelve a empezar",
            60000,
            120000,
            168000,
        )
    } else {
        (
            "Listen Now follow the rhythm Then begin again",
            48000,
            96000,
            144000,
        )
    };
    let words: Vec<_> = text
        .split_whitespace()
        .enumerate()
        .map(|(i, word)| {
            let start = match i {
                0 => {
                    if es {
                        4800
                    } else {
                        2400
                    }
                }
                1..=4 => first + (i - 1) as u64 * 1000,
                _ => second + (i - 5) as u64 * 1000,
            };
            json!({"word":word,"start_sample":start,"end_sample":start+1000})
        })
        .collect();
    let raw = json!({"schema":compiler::ALIGNMENT_SCHEMA,"performance_id":"metadata-only",
        "cue_id":"cue","language":if es {"es"} else {"en"},"source":pin("not-a-recording",b"metadata-only"),
        "sample_rate":48000,"duration_samples":duration,"script_id":"source","script_sha256":sha(text.as_bytes()),
        "observation":{"kind":"word-evidence","observed_text":text,"timing":{
            "schema":"reel.scene-word-timing-evidence.v1","language":if es {"es"} else {"en"},
            "cue_id":"cue","selected_take_sha256":sha(b"metadata-only"),"sample_rate":48000,
            "cue_end_sample":duration,"words":words}},
        "markers":[{"id":"first","kind":"phrase","start_sample":first,"end_sample":first+4000},
            {"id":"second","kind":"phrase","start_sample":second,"end_sample":second+(words.len()-5) as u64*1000}],
        "word_bindings":[{"marker_id":"first","first_word_index":1,"end_word_index_exclusive":5},
            {"marker_id":"second","first_word_index":5,"end_word_index_exclusive":words.len()}]});
    (serde_json::from_value(raw).unwrap(), text.into())
}
#[test]
fn lexical_metadata_resolves_independent_english_and_spanish_entrances() {
    for es in [false, true] {
        let (e, text) = lexical(es);
        let phrases = if es {
            ["Ahora sigue el ritmo", "Después vuelve a empezar"]
        } else {
            ["Now follow the rhythm", "Then begin again"]
        };
        let expected = if es { [60000, 120000] } else { [48000, 96000] };
        for ((id, phrase), sample) in ["first", "second"].into_iter().zip(phrases).zip(expected) {
            assert_eq!(phrase_sample(&e, &text, id, phrase, None).unwrap(), sample);
        }
        assert!(
            phrase_sample(&e, &text, "second", phrases[0], None)
                .unwrap_err()
                .to_string()
                .contains("entrance")
        );
        assert!(phrase_sample(&e, &text, "first", "absent phrase", None).is_err());
        assert!(phrase_sample(&e, &text, "first", phrases[0], Some(0)).is_err());
    }
    let (mut e, _) = lexical(false);
    if let compiler::Observation::WordEvidence { timing, .. } = &mut e.observation {
        let first = timing.words[1..5].to_vec();
        let mut second = first.clone();
        for word in &mut second {
            word.start_sample += 72000;
            word.end_sample += 72000;
        }
        timing.words = first.into_iter().chain(second).collect();
    }
    e.markers[1].start_sample = 120000;
    e.markers[1].end_sample = 124000;
    let text = "Now follow the rhythm Now follow the rhythm";
    assert!(
        phrase_sample(&e, text, "first", "Now follow the rhythm", None)
            .unwrap_err()
            .to_string()
            .contains("ambiguous")
    );
    assert_eq!(
        phrase_sample(&e, text, "first", "Now follow the rhythm", Some(1)).unwrap(),
        48000
    );
    assert_eq!(
        phrase_sample(&e, text, "second", "Now follow the rhythm", Some(2)).unwrap(),
        120000
    );
}
#[test]
fn phrase_binding_cannot_turn_engineering_markers_into_spoken_words() {
    let (mut e, text) = lexical(false);
    e.observation = compiler::Observation::EngineeringSignal {};
    assert!(
        phrase_sample(&e, &text, "first", "Now follow the rhythm", None)
            .unwrap_err()
            .to_string()
            .contains("requires original word evidence")
    );
    let (mut e, text) = lexical(false);
    if let compiler::Observation::WordEvidence { timing, .. } = &mut e.observation {
        timing.words[1].word = "different".into();
    }
    assert!(phrase_sample(&e, &text, "first", "Now follow the rhythm", None).is_err());
}

#[test]
fn lexical_metadata_uses_real_compiler_take_placements_without_media() {
    struct Metadata(BTreeMap<std::path::PathBuf, Vec<u8>>);
    impl InputFiles for Metadata {
        fn verified_path(&self, _: &FileRef) -> Result<std::path::PathBuf> {
            anyhow::bail!("metadata-only fixture has no media")
        }
        fn control_bytes(&self, p: &FileRef) -> Result<Vec<u8>> {
            self.0
                .get(&p.path)
                .cloned()
                .context("missing lexical metadata")
        }
    }
    let mut doc: super::super::Document = serde_json::from_str(include_str!(
        "../../../tests/fixtures/spoken-programme/four-editions.json"
    ))
    .unwrap();
    doc.modules.truncate(1);
    doc.performances = vec![doc.performances[0].clone(), doc.performances[3].clone()];
    doc.editions.retain(|e| e.id.ends_with("-technical"));
    for e in &mut doc.editions {
        e.required_modules = vec!["opening".into()];
        e.selections.truncate(1);
        e.joins.clear();
    }
    let mut raw = BTreeMap::new();
    let mut provider = BTreeMap::new();
    for p in &mut doc.performances {
        let es = p.language == "es";
        let (mut evidence, text) = lexical(es);
        let script = doc.modules[0].cues[0]
            .scripts
            .iter_mut()
            .find(|s| s.language == p.language)
            .unwrap();
        script.text = text.clone();
        script.sha256 = sha(text.as_bytes());
        p.script_sha256 = script.sha256.clone();
        p.observed_text_sha256 = sha(text.as_bytes());
        p.scope.kind = super::super::PerformanceKind::RecordedSpeech;
        p.scope.edition_ids.retain(|id| id.ends_with("-technical"));
        p.markers = evidence.markers.clone();
        evidence.performance_id = p.id.clone();
        evidence.cue_id = p.cue_id.clone();
        evidence.source = p.source.clone();
        evidence.script_id = p.script_id.clone();
        evidence.script_sha256 = p.script_sha256.clone();
        if let compiler::Observation::WordEvidence { timing, .. } = &mut evidence.observation {
            timing.cue_id = p.cue_id.clone();
            timing.selected_take_sha256 = p.source.sha256.clone();
        }
        let bytes = serde_json::to_vec(&evidence).unwrap();
        p.alignment = pin(&format!("{}.json", p.id), &bytes);
        raw.insert(p.id.clone(), bytes.clone());
        provider.insert(p.alignment.path.clone(), bytes);
    }
    let provider = Metadata(provider);
    let runtime = compiler::CompilerIdentity {
        sha256: sha(b"metadata compiler"),
        bytes: 1,
    };
    for trim in [0, 2000] {
        let mut selected = doc.clone();
        for e in &mut selected.editions {
            e.selections[0].takes[0].start_sample = trim;
        }
        let plan = compiler::compile(
            &selected,
            &raw,
            &runtime,
            compiler::CompilationProfile::Spoken,
        )
        .unwrap();
        for edition in &plan.editions {
            let es = edition.recipe.language == "es";
            let phrases = if es {
                ["Ahora sigue el ritmo", "Después vuelve a empezar"]
            } else {
                ["Now follow the rhythm", "Then begin again"]
            };
            for ((marker, phrase), sample) in ["first", "second"]
                .into_iter()
                .zip(phrases)
                .zip(if es { [60000, 120000] } else { [48000, 96000] })
            {
                let anchor = Anchor::Phrase {
                    performance_id: edition.cues[0].performance_id.clone(),
                    marker_id: marker.into(),
                    phrase: phrase.into(),
                    occurrence: None,
                };
                let result = resolve(&anchor, edition, &provider).unwrap();
                assert_eq!(result["sample"], sample - trim);
                assert_eq!(result["basis"], "word-evidence-not-listening-approval");
            }
        }
    }
}
