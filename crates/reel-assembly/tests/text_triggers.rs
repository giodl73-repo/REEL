use reel_assembly::scene_authoring::{
    CacheObjectRef, TRIGGER_TEXT_SCHEMA, TextMarker, TimedWord, TriggerTextSpec,
    WHISPERCPP_WORD_IMPORT_SCHEMA, WORD_TIMING_SCHEMA, WhisperCppWordImport, WordTimingEvidence,
    import_whispercpp_words, resolve_text_triggers,
};
use sha2::{Digest, Sha256};

fn text_sha(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fixture() -> (WordTimingEvidence, TriggerTextSpec) {
    let spoken = "Bertha María, en una pared de su cuarto, marcó cada día.";
    let take = "a".repeat(64);
    let words = [
        "Bertha", "María,", "en", "una", "pared", "de", "su", "cuarto,", "marcó", "cada", "día.",
    ]
    .iter()
    .enumerate()
    .map(|(index, word)| TimedWord {
        word: (*word).into(),
        start_sample: (index as u64 + 1) * 2400,
        end_sample: (index as u64 + 1) * 2400 + 1200,
    })
    .collect();
    (
        WordTimingEvidence {
            schema: WORD_TIMING_SCHEMA.into(),
            language: "es".into(),
            cue_id: "cue".into(),
            selected_take_sha256: take.clone(),
            sample_rate: 24000,
            cue_end_sample: 30000,
            words,
        },
        TriggerTextSpec {
            schema: TRIGGER_TEXT_SCHEMA.into(),
            language: "es".into(),
            cue_id: "cue".into(),
            selected_take_sha256: take,
            spoken_text: spoken.into(),
            spoken_text_sha256: text_sha(spoken),
            markers: vec![
                TextMarker {
                    id: "start".into(),
                    phrase: None,
                    occurrence: None,
                },
                TextMarker {
                    id: "wall".into(),
                    phrase: Some("en una pared".into()),
                    occurrence: None,
                },
                TextMarker {
                    id: "mark".into(),
                    phrase: Some("marcó cada día".into()),
                    occurrence: None,
                },
            ],
        },
    )
}

#[test]
fn native_whisper_token_fragments_resolve_a_source_phrase() {
    let take = "a".repeat(64);
    let spec = WhisperCppWordImport {
        schema: WHISPERCPP_WORD_IMPORT_SCHEMA.into(),
        language: "es".into(),
        cue_id: "cue".into(),
        sample_rate: 24_000,
        cue_end_sample: 24_000,
        take: CacheObjectRef {
            cache_uri: format!("cache://sha256/{take}"),
            sha256: take.clone(),
            bytes: 100,
        },
        model: CacheObjectRef {
            cache_uri: format!("cache://sha256/{}", "b".repeat(64)),
            sha256: "b".repeat(64),
            bytes: 100,
        },
        tool: CacheObjectRef {
            cache_uri: format!("cache://sha256/{}", "c".repeat(64)),
            sha256: "c".repeat(64),
            bytes: 100,
        },
        transcription_path: "native.json".into(),
        transcription_sha256: "d".repeat(64),
        transcription_bytes: 100,
    };
    let output = serde_json::json!({"transcription":[{"tokens":[
        {"text":"[_BEG_]","offsets":{"from":0,"to":0}},
        {"text":" se","offsets":{"from":100,"to":180}},
        {"text":" instal","offsets":{"from":180,"to":330}},
        {"text":"aron","offsets":{"from":330,"to":450}},
        {"text":" en","offsets":{"from":450,"to":510}},
        {"text":" Villa","offsets":{"from":510,"to":620}},
        {"text":" Bert","offsets":{"from":620,"to":710}},
        {"text":"ha.","offsets":{"from":710,"to":790}},
        {"text":",","offsets":{"from":800,"to":800}},
        {"text":".","offsets":{"from":1000,"to":1100}}
    ]}]});
    let (evidence, diagnostics) =
        import_whispercpp_words(&serde_json::to_vec(&output).unwrap(), &spec).unwrap();
    assert_eq!(evidence.words.len(), 5);
    assert_eq!(diagnostics.ignored_zero_offset_tokens, 1);
    assert_eq!(diagnostics.ignored_out_of_take_punctuation, 1);
    assert_eq!(evidence.words[1].word, "instalaron");
    let spoken = "se instalaron en Villa Bertha.";
    let trigger = TriggerTextSpec {
        schema: TRIGGER_TEXT_SCHEMA.into(),
        language: "es".into(),
        cue_id: "cue".into(),
        selected_take_sha256: take,
        spoken_text: spoken.into(),
        spoken_text_sha256: text_sha(spoken),
        markers: vec![
            TextMarker {
                id: "start".into(),
                phrase: None,
                occurrence: None,
            },
            TextMarker {
                id: "home".into(),
                phrase: Some("se instalaron en Villa Bertha".into()),
                occurrence: Some(1),
            },
        ],
    };
    assert_eq!(
        resolve_text_triggers(&evidence, &trigger)
            .unwrap()
            .semantic_markers["home"],
        2400
    );
}

#[test]
fn native_whisper_import_rejects_tokens_outside_the_take() {
    let mut spec = WhisperCppWordImport {
        schema: WHISPERCPP_WORD_IMPORT_SCHEMA.into(),
        language: "es".into(),
        cue_id: "cue".into(),
        sample_rate: 24_000,
        cue_end_sample: 24_000,
        take: CacheObjectRef {
            cache_uri: format!("cache://sha256/{}", "a".repeat(64)),
            sha256: "a".repeat(64),
            bytes: 100,
        },
        model: CacheObjectRef {
            cache_uri: format!("cache://sha256/{}", "b".repeat(64)),
            sha256: "b".repeat(64),
            bytes: 100,
        },
        tool: CacheObjectRef {
            cache_uri: format!("cache://sha256/{}", "c".repeat(64)),
            sha256: "c".repeat(64),
            bytes: 100,
        },
        transcription_path: "native.json".into(),
        transcription_sha256: "d".repeat(64),
        transcription_bytes: 100,
    };
    let output = serde_json::json!({"transcription":[{"tokens":[{"text":" palabra","offsets":{"from":900,"to":1100}}]}]});
    assert!(import_whispercpp_words(&serde_json::to_vec(&output).unwrap(), &spec).is_err());
    spec.cue_end_sample = 30_000;
    assert!(import_whispercpp_words(&serde_json::to_vec(&output).unwrap(), &spec).is_ok());
}

#[test]
fn resolves_exact_source_phrases_to_selected_native_word_samples() {
    let (evidence, spec) = fixture();
    let alignment = resolve_text_triggers(&evidence, &spec).unwrap();
    assert_eq!(alignment.semantic_markers["start"], 0);
    assert_eq!(alignment.semantic_markers["wall"], 7200);
    assert_eq!(alignment.semantic_markers["mark"], 21600);
    assert_eq!(alignment.cue_end_sample, 30000);
}

#[test]
fn resolves_spanish_diacritic_variants_without_changing_source_hash() {
    let (mut evidence, mut spec) = fixture();
    spec.spoken_text = "Se rompió en añicos".into();
    spec.spoken_text_sha256 = text_sha(&spec.spoken_text);
    evidence.words = ["Se", "rompio", "en", "anicos"]
        .iter()
        .enumerate()
        .map(|(i, word)| TimedWord {
            word: (*word).into(),
            start_sample: i as u64 * 6000,
            end_sample: i as u64 * 6000 + 3000,
        })
        .collect();
    spec.markers = vec![
        TextMarker {
            id: "start".into(),
            phrase: None,
            occurrence: None,
        },
        TextMarker {
            id: "shattered".into(),
            phrase: Some("añicos".into()),
            occurrence: Some(1),
        },
    ];
    let alignment = resolve_text_triggers(&evidence, &spec).unwrap();
    assert_eq!(alignment.semantic_markers["shattered"], 18000);
    assert!(alignment.semantic_markers["shattered"] < alignment.cue_end_sample);
}

#[test]
fn repeated_phrase_requires_explicit_occurrence() {
    let (mut evidence, mut spec) = fixture();
    spec.spoken_text = "la casa y la casa".into();
    spec.spoken_text_sha256 = text_sha(&spec.spoken_text);
    evidence.words = ["la", "casa", "y", "la", "casa"]
        .iter()
        .enumerate()
        .map(|(i, word)| TimedWord {
            word: (*word).into(),
            start_sample: (i as u64 + 1) * 3000,
            end_sample: (i as u64 + 1) * 3000 + 1000,
        })
        .collect();
    spec.markers = vec![
        TextMarker {
            id: "start".into(),
            phrase: None,
            occurrence: None,
        },
        TextMarker {
            id: "second".into(),
            phrase: Some("la casa".into()),
            occurrence: None,
        },
    ];
    assert!(resolve_text_triggers(&evidence, &spec).is_err());
    spec.markers[1].occurrence = Some(2);
    assert_eq!(
        resolve_text_triggers(&evidence, &spec)
            .unwrap()
            .semantic_markers["second"],
        12000
    );
}

#[test]
fn rejects_stale_take_or_invalid_word_clock() {
    let (mut evidence, spec) = fixture();
    evidence.selected_take_sha256 = "b".repeat(64);
    assert!(resolve_text_triggers(&evidence, &spec).is_err());
    evidence.selected_take_sha256 = spec.selected_take_sha256.clone();
    evidence.words[2].end_sample = evidence.words[2].start_sample;
    assert!(resolve_text_triggers(&evidence, &spec).is_err());
}
