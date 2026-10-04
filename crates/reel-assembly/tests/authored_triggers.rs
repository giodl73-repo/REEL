use reel_assembly::scene_authoring::{
    Scene, WordTimingEvidence, authored_trigger_text, resolve_text_triggers,
};
use serde_json::json;
use sha2::{Digest, Sha256};

fn hash(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn scene() -> Scene {
    serde_json::from_value(json!({
        "schema":"reel.scene-authoring.v2", "scene_id":"scene-1", "episode_id":"episode-1",
        "authoring_state":"imported-evidence", "source_authority_id":"source-1",
        "source_scope":"One source cue with bilingual semantic entrances",
        "canonical_cue_ids":["cue-1"],
        "shared_events":[
            {"semantic_id":"greeting","canonical_cue_id":"cue-1","picture_binding":"picture.greeting","score":{"disposition":"silence"}},
            {"semantic_id":"look","canonical_cue_id":"cue-1","picture_binding":"picture.look","score":{"disposition":"silence"}}
        ],
        "languages":{
            "es":{"cues":[{"cue_id":"cue-1","source_id":"cue-1","exact_text_sha256":hash("Hola Sara mira"),"narration_slot_id":"d-es"}]},
            "en":{"cues":[{"cue_id":"cue-1","source_id":"cue-1","exact_text_sha256":hash("Hello Sara look"),"narration_slot_id":"d-en"}]}
        },
        "language_event_bindings":{
            "es":[
                {"semantic_id":"greeting","event_id":"greet-es","cue_id":"cue-1","semantic_trigger_id":"greeting","picture_slot_id":"p-1","spoken_trigger_phrase":"Hola"},
                {"semantic_id":"look","event_id":"look-es","cue_id":"cue-1","semantic_trigger_id":"look","picture_slot_id":"p-2","spoken_trigger_phrase":"mira"}],
            "en":[
                {"semantic_id":"greeting","event_id":"greet-en","cue_id":"cue-1","semantic_trigger_id":"greeting","picture_slot_id":"p-1","spoken_trigger_phrase":"Hello"},
                {"semantic_id":"look","event_id":"look-en","cue_id":"cue-1","semantic_trigger_id":"look","picture_slot_id":"p-2","spoken_trigger_phrase":"look"}]
        }
    })).unwrap()
}

fn evidence(language: &str, text: &str, second_cut: u64) -> WordTimingEvidence {
    serde_json::from_value(json!({
        "schema":"reel.scene-word-timing-evidence.v1", "language":language,"cue_id":"cue-1",
        "selected_take_sha256":"a".repeat(64),"sample_rate":44100,"cue_end_sample":10000,
        "words":text.split_whitespace().enumerate().map(|(i, word)| {
            let start = [500,1500,second_cut][i];
            json!({"word":word,"start_sample":start,"end_sample":start+400})
        }).collect::<Vec<_>>()
    }))
    .unwrap()
}

#[test]
fn one_manifest_resolves_language_local_phrases_to_different_native_clocks() {
    let scene = scene();
    for (language, text, cut) in [
        ("es", "Hola Sara mira", 3000),
        ("en", "Hello Sara look", 4100),
    ] {
        let words = evidence(language, text, cut);
        let proposal = authored_trigger_text(&scene, &words, text).unwrap();
        let resolved = resolve_text_triggers(&words, &proposal).unwrap();
        assert_eq!(resolved.semantic_markers["greeting"], 0);
        assert_eq!(resolved.semantic_markers["look"], cut);
    }
}

#[test]
fn later_first_phrase_is_measured_instead_of_silently_moved_to_zero() {
    let mut scene = scene();
    scene.language_event_bindings.get_mut("es").unwrap()[0].spoken_trigger_phrase =
        Some("Sara".into());
    let words = evidence("es", "Hola Sara mira", 3000);
    let proposal = authored_trigger_text(&scene, &words, "Hola Sara mira").unwrap();
    let resolved = resolve_text_triggers(&words, &proposal).unwrap();
    assert_eq!(resolved.semantic_markers["cue-start"], 0);
    assert_eq!(resolved.semantic_markers["greeting"], 1500);
}

#[test]
fn wrong_text_language_and_cue_fail_before_clock_resolution() {
    let scene = scene();
    let mut words = evidence("es", "Hola Sara mira", 3000);
    assert!(authored_trigger_text(&scene, &words, "Hello Sara look").is_err());
    words.language = "fr".into();
    assert!(authored_trigger_text(&scene, &words, "Hola Sara mira").is_err());
    words.language = "es".into();
    words.cue_id = "other-cue".into();
    assert!(authored_trigger_text(&scene, &words, "Hola Sara mira").is_err());
}

#[test]
fn manifest_roundtrip_preserves_authored_phrase_and_source_provenance() {
    let mut scene = scene();
    let cue = &mut scene.languages.get_mut("es").unwrap().cues[0];
    cue.source_text_overlay = Some("spoken-overlay.json".into());
    cue.raw_source_text_sha256 = Some("b".repeat(64));
    let again: Scene = serde_json::from_slice(&serde_json::to_vec(&scene).unwrap()).unwrap();
    assert_eq!(
        again.language_event_bindings["es"][1]
            .spoken_trigger_phrase
            .as_deref(),
        Some("mira")
    );
    assert_eq!(
        again.languages["es"].cues[0].source_text_overlay.as_deref(),
        Some("spoken-overlay.json")
    );
}

#[test]
fn repeated_opening_phrase_preserves_explicit_later_occurrence() {
    let text = "Hola Sara Hola";
    let mut scene = scene();
    scene.languages.get_mut("es").unwrap().cues[0].exact_text_sha256 = hash(text);
    let bindings = scene.language_event_bindings.get_mut("es").unwrap();
    bindings.truncate(1);
    bindings[0].spoken_trigger_occurrence = Some(2);
    let words = evidence("es", text, 3000);
    let proposal = authored_trigger_text(&scene, &words, text).unwrap();
    let resolved = resolve_text_triggers(&words, &proposal).unwrap();
    assert_eq!(resolved.semantic_markers["cue-start"], 0);
    assert_eq!(resolved.semantic_markers["greeting"], 3000);
    bindings_no_phrase_occurrence_fails();
}

fn bindings_no_phrase_occurrence_fails() {
    let mut scene = scene();
    let first = &mut scene.language_event_bindings.get_mut("es").unwrap()[0];
    first.spoken_trigger_phrase = None;
    first.spoken_trigger_occurrence = Some(2);
    assert!(
        authored_trigger_text(
            &scene,
            &evidence("es", "Hola Sara mira", 3000),
            "Hola Sara mira"
        )
        .is_err()
    );
}

#[test]
fn opening_phrase_cannot_bypass_ambiguity_or_word_evidence() {
    let mut scene = scene();
    let repeated = "Hola Sara Hola";
    scene.languages.get_mut("es").unwrap().cues[0].exact_text_sha256 = hash(repeated);
    assert!(authored_trigger_text(&scene, &evidence("es", repeated, 3000), repeated).is_err());
    let scene = super_scene_with_one_event();
    assert!(
        authored_trigger_text(
            &scene,
            &evidence("es", "Wrong words here", 3000),
            "Hola Sara mira"
        )
        .is_err()
    );
}

fn super_scene_with_one_event() -> Scene {
    let mut scene = scene();
    scene
        .language_event_bindings
        .get_mut("es")
        .unwrap()
        .truncate(1);
    scene
}

#[test]
fn imported_candidates_still_enforce_canonical_and_shared_event_scope() {
    let mut scene = scene();
    scene.canonical_cue_ids = vec!["other-cue".into()];
    assert!(
        authored_trigger_text(
            &scene,
            &evidence("es", "Hola Sara mira", 3000),
            "Hola Sara mira"
        )
        .is_err()
    );
    scene.canonical_cue_ids = vec!["cue-1".into()];
    scene.shared_events[0].canonical_cue_id = "other-cue".into();
    assert!(
        authored_trigger_text(
            &scene,
            &evidence("es", "Hola Sara mira", 3000),
            "Hola Sara mira"
        )
        .is_err()
    );
}
