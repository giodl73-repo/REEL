use reel_assembly::scene_authoring::{
    TRIGGER_TEXT_SCHEMA, TextMarker, TimedWord, TriggerTextSpec, WORD_TIMING_SCHEMA,
    WordTimingEvidence, resolve_text_triggers,
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
fn resolves_exact_source_phrases_to_selected_native_word_samples() {
    let (evidence, spec) = fixture();
    let alignment = resolve_text_triggers(&evidence, &spec).unwrap();
    assert_eq!(alignment.semantic_markers["start"], 0);
    assert_eq!(alignment.semantic_markers["wall"], 7200);
    assert_eq!(alignment.semantic_markers["mark"], 21600);
    assert_eq!(alignment.cue_end_sample, 30000);
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
