use reel_assembly::scene_authoring::{
    Episode, NATIVE_ALIGNMENT_SCHEMA, NativeAlignment, Scene, ScenePolicy, ScopedBindings,
    TemplateCatalog, compile_native_event_spans, resolve_episode_presentation, resolve_scene,
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
