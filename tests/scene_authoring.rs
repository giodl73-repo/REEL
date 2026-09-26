use reel_assembly::scene_authoring::{
    Episode, LanguageEventBinding, NATIVE_ALIGNMENT_SCHEMA, NativeAlignment, SCENE_SCHEMA_V2,
    Scene, ScenePolicy, ScopedBindings, ScoreUse, SharedEvent, TemplateCatalog,
    compile_native_event_spans, compile_selected_event_request, materialize_scene,
    resolve_episode_presentation, resolve_scene,
};
use serde::de::DeserializeOwned;

fn fixture<T: DeserializeOwned>(name: &str) -> T {
    let path = format!(
        "{}/tests/fixtures/scene-authoring/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn subject() -> (
    TemplateCatalog,
    Episode,
    Scene,
    ScenePolicy,
    ScopedBindings,
    ScopedBindings,
    ScopedBindings,
) {
    (
        fixture("catalog"),
        fixture("episode"),
        fixture("scene"),
        fixture("policy"),
        fixture("season-bindings"),
        fixture("episode-bindings"),
        fixture("scene-bindings"),
    )
}

#[test]
fn v2_owns_effects_once_and_compiles_language_local_triggers() {
    let (catalog, episode, mut scene, policy, season, episode_bindings, mut scene_bindings) =
        subject();
    scene.schema = SCENE_SCHEMA_V2.into();
    scene.canonical_cue_ids = vec!["source-block-1".into()];
    scene.source_scope_ids.clear();
    scene.shared_events = vec![SharedEvent {
        semantic_id: "poem-image".into(),
        canonical_cue_id: "source-block-1".into(),
        picture_binding: "picture.first-line".into(),
        score: ScoreUse::Role {
            role: "poem.intimate".into(),
        },
        sonic_bindings: vec!["sonic.wind".into()],
        vfx_bindings: vec!["vfx.dust".into()],
    }];
    for (language, lane) in &mut scene.languages {
        let old = lane.events.remove(0);
        scene.language_event_bindings.insert(
            language.clone(),
            vec![LanguageEventBinding {
                semantic_id: "poem-image".into(),
                event_id: old.event_id,
                cue_id: old.cue_id,
                semantic_trigger_id: format!("{language}-native-line"),
                picture_slot_id: old.picture_slot_id,
                picture_binding_override: None,
                supersedes_event_id: None,
            }],
        );
    }
    for (key, digit) in [("sonic.wind", '8'), ("vfx.dust", '9')] {
        let mut asset = scene_bindings.assets["picture.first-line"].clone();
        asset.logical_id = key.into();
        asset.sha256 = digit.to_string().repeat(64);
        asset.cache_uri = format!("cache://sha256/{}", asset.sha256);
        scene_bindings.assets.insert(key.into(), asset);
    }
    let resolved = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    assert!(resolved.selected_inputs.contains_key("sonic.wind"));
    assert!(resolved.selected_inputs.contains_key("vfx.dust"));
    let native = materialize_scene(&scene).unwrap();
    for (language, lane) in &native.languages {
        assert_eq!(lane.events.len(), 1);
        assert_eq!(lane.events[0].sonic_bindings, vec!["sonic.wind"]);
        assert_eq!(lane.events[0].vfx_bindings, vec!["vfx.dust"]);
        let cue_id = lane.cues[0].cue_id.clone();
        let span = compile_native_event_spans(
            language,
            lane,
            &std::collections::BTreeMap::from([(
                cue_id.clone(),
                NativeAlignment {
                    schema: NATIVE_ALIGNMENT_SCHEMA.into(),
                    language: language.clone(),
                    cue_id,
                    selected_take_sha256: if language == "es" {
                        "3".repeat(64)
                    } else {
                        "5".repeat(64)
                    },
                    sample_rate: 24_000,
                    cue_end_sample: if language == "es" { 48_000 } else { 72_000 },
                    semantic_markers: std::collections::BTreeMap::from([(
                        format!("{language}-native-line"),
                        0,
                    )]),
                },
            )]),
        )
        .unwrap();
        assert_eq!(
            span[0].end_sample,
            if language == "es" { 48_000 } else { 72_000 }
        );
    }
    let mut duplicate = scene;
    duplicate.languages.get_mut("es").unwrap().events = native.languages["es"].events.clone();
    assert!(materialize_scene(&duplicate).is_err());
}

#[test]
fn presentation_source_scope_change_invalidates_both_language_lanes() {
    let (catalog, episode, mut scene, policy, season, episode_bindings, scene_bindings) = subject();
    scene.presentation_source_scope_ids = vec!["poem-title".into(), "poet-credit".into()];
    let first = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    scene.presentation_source_scope_ids[1] = "revised-poet-credit".into();
    let second = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    assert_ne!(
        first.language_fingerprints["es"],
        second.language_fingerprints["es"]
    );
    assert_ne!(
        first.language_fingerprints["en"],
        second.language_fingerprints["en"]
    );
}

#[test]
fn one_language_take_rebind_only_invalidates_its_lane() {
    let (catalog, episode, scene, policy, season, episode_bindings, mut scene_bindings) = subject();
    let first = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    let take = scene_bindings.assets.get_mut("es.take").unwrap();
    take.sha256 = "8".repeat(64);
    take.cache_uri = format!("cache://sha256/{}", take.sha256);
    let second = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    assert_ne!(
        first.language_fingerprints["es"],
        second.language_fingerprints["es"]
    );
    assert_eq!(
        first.language_fingerprints["en"],
        second.language_fingerprints["en"]
    );
}

#[test]
fn poem_score_rebind_invalidates_both_scored_lanes() {
    let (catalog, episode, scene, policy, season, mut episode_bindings, scene_bindings) = subject();
    let first = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    let score = episode_bindings
        .assets
        .get_mut("score.poem.intimate")
        .unwrap();
    score.sha256 = "9".repeat(64);
    score.cache_uri = format!("cache://sha256/{}", score.sha256);
    let second = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    assert_ne!(
        first.language_fingerprints["es"],
        second.language_fingerprints["es"]
    );
    assert_ne!(
        first.language_fingerprints["en"],
        second.language_fingerprints["en"]
    );
}

#[test]
fn season_opening_rebind_only_invalidates_episode_presentation() {
    let (catalog, episode, scene, policy, mut season, episode_bindings, scene_bindings) = subject();
    let scene_first = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    let presentation_first =
        resolve_episode_presentation(&catalog, &episode, &season, &episode_bindings).unwrap();
    let opening = season.assets.get_mut("opening.selected").unwrap();
    opening.sha256 = "0".repeat(64);
    opening.cache_uri = format!("cache://sha256/{}", opening.sha256);
    let scene_second = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    let presentation_second =
        resolve_episode_presentation(&catalog, &episode, &season, &episode_bindings).unwrap();
    assert_eq!(
        scene_first.fingerprint_sha256,
        scene_second.fingerprint_sha256
    );
    assert_ne!(
        presentation_first.fingerprint_sha256,
        presentation_second.fingerprint_sha256
    );
}

#[test]
fn english_poem_line_map_change_only_invalidates_english_scene_lane() {
    let (catalog, episode, mut scene, policy, season, episode_bindings, scene_bindings) = subject();
    scene.presentation.as_mut().unwrap().content["titles"] =
        serde_json::json!({"es":"Recuerdos","en":"Memories"});
    scene.presentation.as_mut().unwrap().content["lines_by_language"] =
        serde_json::json!({"es":[{"text":"Línea"}],"en":[{"text":"Line"}]});
    let first = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    scene.presentation.as_mut().unwrap().content["line_cue_ids"]["en"] =
        serde_json::json!(["en-1", "en-2"]);
    scene.presentation.as_mut().unwrap().content["titles"]["en"] = "New title".into();
    scene.presentation.as_mut().unwrap().content["lines_by_language"]["en"] =
        serde_json::json!([{"text":"New line"}]);
    let second = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    assert_eq!(
        first.language_fingerprints["es"],
        second.language_fingerprints["es"]
    );
    assert_ne!(
        first.language_fingerprints["en"],
        second.language_fingerprints["en"]
    );
}

#[test]
fn moving_episode_season_changes_presentation_fingerprint() {
    let (catalog, mut episode, _, _, mut season, episode_bindings, _) = subject();
    let first =
        resolve_episode_presentation(&catalog, &episode, &season, &episode_bindings).unwrap();
    episode.season_id = "another-season".into();
    season.scope_id = episode.season_id.clone();
    let second =
        resolve_episode_presentation(&catalog, &episode, &season, &episode_bindings).unwrap();
    assert_ne!(first.fingerprint_sha256, second.fingerprint_sha256);
}

#[test]
fn template_layout_is_owned_by_template_and_only_its_users_rebuild() {
    let (mut catalog, episode, scene, policy, season, episode_bindings, scene_bindings) = subject();
    let first = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    catalog
        .templates
        .iter_mut()
        .find(|t| t.template_id == "chapter-v1")
        .unwrap()
        .definition_sha256 = "1".repeat(64);
    let unrelated = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    assert_eq!(first.fingerprint_sha256, unrelated.fingerprint_sha256);
    catalog
        .templates
        .iter_mut()
        .find(|t| t.template_id == "poem-v1")
        .unwrap()
        .definition_sha256 = "2".repeat(64);
    let changed = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    assert_ne!(first.fingerprint_sha256, changed.fingerprint_sha256);
    let mut bad_scene = scene;
    bad_scene.presentation.as_mut().unwrap().content["layout"] =
        serde_json::json!({"panel_width":427});
    assert!(
        resolve_scene(
            &catalog,
            &episode,
            &bad_scene,
            &policy,
            &season,
            &episode_bindings,
            &scene_bindings
        )
        .is_err()
    );
}

#[test]
fn unknown_score_role_is_rejected() {
    let (catalog, episode, mut scene, policy, season, episode_bindings, scene_bindings) = subject();
    if let reel_assembly::scene_authoring::ScoreUse::Role { role } =
        &mut scene.languages.get_mut("es").unwrap().events[0].score
    {
        *role = "another-song".into();
    }
    assert!(
        resolve_scene(
            &catalog,
            &episode,
            &scene,
            &policy,
            &season,
            &episode_bindings,
            &scene_bindings
        )
        .is_err()
    );
}

#[test]
fn historical_evidence_cannot_be_resolved_as_a_selected_scene() {
    let (catalog, episode, mut scene, policy, season, episode_bindings, scene_bindings) = subject();
    scene.authoring_state = "imported-evidence".into();
    scene.languages.clear();
    assert!(
        resolve_scene(
            &catalog,
            &episode,
            &scene,
            &policy,
            &season,
            &episode_bindings,
            &scene_bindings
        )
        .is_err()
    );
}

#[test]
fn one_ready_scene_builds_inside_an_unfinished_episode_context() {
    let (catalog, mut episode, scene, policy, season, episode_bindings, scene_bindings) = subject();
    episode.authoring_state = "scene-build-context".into();
    resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .expect("ready scene resolves independently of incomplete episode presentation");
    assert!(resolve_episode_presentation(&catalog, &episode, &season, &episode_bindings).is_err());
    episode.authoring_state = "imported-evidence".into();
    assert!(
        resolve_scene(
            &catalog,
            &episode,
            &scene,
            &policy,
            &season,
            &episode_bindings,
            &scene_bindings,
        )
        .is_err()
    );
}

#[test]
fn selected_bindings_must_match_their_owning_scopes() {
    let (catalog, episode, scene, policy, mut season, mut episode_bindings, mut scene_bindings) =
        subject();
    season.scope_id = "another-season".into();
    assert!(
        resolve_scene(
            &catalog,
            &episode,
            &scene,
            &policy,
            &season,
            &episode_bindings,
            &scene_bindings
        )
        .is_err()
    );
    assert!(resolve_episode_presentation(&catalog, &episode, &season, &episode_bindings).is_err());
    season.scope_id = episode.season_id.clone();
    episode_bindings.scope_id = "another-episode".into();
    assert!(
        resolve_scene(
            &catalog,
            &episode,
            &scene,
            &policy,
            &season,
            &episode_bindings,
            &scene_bindings
        )
        .is_err()
    );
    assert!(resolve_episode_presentation(&catalog, &episode, &season, &episode_bindings).is_err());
    episode_bindings.scope_id = episode.episode_id.clone();
    scene_bindings.scope_id = "another-scene".into();
    assert!(
        resolve_scene(
            &catalog,
            &episode,
            &scene,
            &policy,
            &season,
            &episode_bindings,
            &scene_bindings
        )
        .is_err()
    );
}

#[test]
fn scene_fingerprint_tracks_scope_and_consumed_score_provenance() {
    let (catalog, mut episode, scene, policy, mut season, episode_bindings, scene_bindings) =
        subject();
    let baseline = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    episode.season_id = "another-season".into();
    season.scope_id = episode.season_id.clone();
    let moved = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    assert_ne!(baseline.language_fingerprints, moved.language_fingerprints);
    episode.season_id = "season-1".into();
    season.scope_id = episode.season_id.clone();
    episode.score_palette[0].source_poem_id = "another-poem".into();
    let rescored = resolve_scene(
        &catalog,
        &episode,
        &scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )
    .unwrap();
    assert_ne!(
        baseline.language_fingerprints,
        rescored.language_fingerprints
    );
}

#[test]
fn semantic_markers_compile_on_each_native_language_clock() {
    let (_, _, scene, _, _, _, _) = subject();
    for (language_id, end_sample) in [("es", 240_000), ("en", 168_000)] {
        let lane = &scene.languages[language_id];
        let alignment = NativeAlignment {
            schema: NATIVE_ALIGNMENT_SCHEMA.into(),
            language: language_id.into(),
            cue_id: lane.cues[0].cue_id.clone(),
            selected_take_sha256: "a".repeat(64),
            sample_rate: 24_000,
            cue_end_sample: end_sample,
            semantic_markers: std::collections::BTreeMap::from([("first-line".into(), 0)]),
        };
        let spans = compile_native_event_spans(
            language_id,
            lane,
            &std::collections::BTreeMap::from([(alignment.cue_id.clone(), alignment)]),
        )
        .unwrap();
        assert_eq!(spans[0].end_sample, end_sample);
    }
}

#[test]
fn native_clock_rejects_unmeasured_semantic_entrance() {
    let (_, _, scene, _, _, _, _) = subject();
    let lane = &scene.languages["es"];
    let alignment = NativeAlignment {
        schema: NATIVE_ALIGNMENT_SCHEMA.into(),
        language: "es".into(),
        cue_id: lane.cues[0].cue_id.clone(),
        selected_take_sha256: "a".repeat(64),
        sample_rate: 24_000,
        cue_end_sample: 240_000,
        semantic_markers: Default::default(),
    };
    assert!(
        compile_native_event_spans(
            "es",
            lane,
            &std::collections::BTreeMap::from([(alignment.cue_id.clone(), alignment)])
        )
        .is_err()
    );
}

#[test]
fn native_markers_bind_only_to_selected_reel_slots() {
    use reel_assembly::{
        Asset, Disposition, GRAPH_SCHEMA, Graph, ImmutableRef, Lane, Node, POINTER_SCHEMA,
        Revision, SelectedPointer, Slot,
    };
    let (_, episode, scene, _, season, episode_bindings, scene_bindings) = subject();
    let make_slot = |slot_id: &str, lane: Lane, key: &str| {
        let selected = &scene_bindings.assets[key];
        Slot {
            slot_id: slot_id.into(),
            beat_id: "beat-1".into(),
            lane,
            disposition: Disposition::Selected,
            selected_revision_id: Some("r1".into()),
            revisions: vec![Revision {
                revision_id: "r1".into(),
                supersedes: None,
                asset: Asset {
                    logical_id: selected.logical_id.clone(),
                    cache_uri: selected.cache_uri.clone(),
                    sha256: selected.sha256.clone(),
                },
            }],
        }
    };
    let lock = ImmutableRef {
        logical_id: "lock-1".into(),
        sha256: "a".repeat(64),
    };
    let graph = Graph {
        schema: GRAPH_SCHEMA.into(),
        lock: lock.clone(),
        slots: vec![
            make_slot("es-narration-slot", Lane::Narration, "es.take"),
            make_slot("es-picture-slot", Lane::Picture, "picture.first-line"),
        ],
        events: vec![],
        nodes: vec![Node {
            id: "scene-poem".into(),
            inputs: vec![],
            slots: vec!["es-narration-slot".into(), "es-picture-slot".into()],
            events: vec![],
        }],
        presentation_targets: vec![],
    };
    let pointer = SelectedPointer {
        schema: POINTER_SCHEMA.into(),
        logical_id: "current".into(),
        selected_lock: lock,
    };
    let lane = &scene.languages["es"];
    let alignment = NativeAlignment {
        schema: NATIVE_ALIGNMENT_SCHEMA.into(),
        language: "es".into(),
        cue_id: lane.cues[0].cue_id.clone(),
        selected_take_sha256: scene_bindings.assets["es.take"].sha256.clone(),
        sample_rate: 24_000,
        cue_end_sample: 240_000,
        semantic_markers: std::collections::BTreeMap::from([("first-line".into(), 0)]),
    };
    let alignments = std::collections::BTreeMap::from([(alignment.cue_id.clone(), alignment)]);
    let request = compile_selected_event_request(
        &graph,
        &pointer,
        &episode,
        &scene,
        "es",
        &[&scene_bindings, &episode_bindings, &season],
        &alignments,
        "lock-2",
    )
    .unwrap();
    assert_eq!(request.bindings[0].event.phrase_end_seconds, 10.0);
    let mut wrong_season = season.clone();
    wrong_season.scope_id = "other-season".into();
    assert!(
        compile_selected_event_request(
            &graph,
            &pointer,
            &episode,
            &scene,
            "es",
            &[&scene_bindings, &episode_bindings, &wrong_season],
            &alignments,
            "lock-2",
        )
        .is_err()
    );
    let mut prior = graph.clone();
    prior.events = ["old-a", "old-b"]
        .into_iter()
        .enumerate()
        .map(|(index, id)| reel_assembly::SemanticEvent {
            event_id: id.into(),
            scene_id: scene.scene_id.clone(),
            language: "es".into(),
            narration: ImmutableRef {
                logical_id: scene_bindings.assets["es.take"].logical_id.clone(),
                sha256: scene_bindings.assets["es.take"].sha256.clone(),
            },
            picture: ImmutableRef {
                logical_id: scene_bindings.assets["picture.first-line"]
                    .logical_id
                    .clone(),
                sha256: scene_bindings.assets["picture.first-line"].sha256.clone(),
            },
            phrase_start_seconds: index as f64 * 5.0,
            phrase_end_seconds: (index + 1) as f64 * 5.0,
        })
        .collect();
    prior.nodes[0].events = vec!["old-a".into(), "old-b".into()];
    let replacement = compile_selected_event_request(
        &prior,
        &pointer,
        &episode,
        &scene,
        "es",
        &[&scene_bindings, &episode_bindings, &season],
        &alignments,
        "lock-3",
    )
    .unwrap();
    assert_eq!(replacement.retirements.len(), 2);
    let selected = reel_assembly::append_semantic_events(&prior, &replacement).unwrap();
    assert_eq!(
        selected.nodes[0].events,
        vec![lane.events[0].event_id.as_str()]
    );
    assert_eq!(selected.events.len(), 3);
    let mut stale = graph.clone();
    stale.slots[0].revisions[0].asset.sha256 = "f".repeat(64);
    assert!(
        compile_selected_event_request(
            &stale,
            &pointer,
            &episode,
            &scene,
            "es",
            &[&scene_bindings, &episode_bindings, &season],
            &alignments,
            "lock-2"
        )
        .is_err()
    );
}
