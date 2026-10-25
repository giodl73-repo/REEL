//! Actual offline delivery compilation after applying native cel-editor requests.
//! No real media/cache access, renderer, server, selection or publication.
#[path = "../crates/reel-assets/tests/fixtures/cel_native.rs"]
mod fixtures;
use reel::scene_delivery_compile::{CompileManifest, compile_to_dir};
use reel_assets::cel_source::{editor, project::Project, sequence::Sequence};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
fn get(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn put(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().into(),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}
fn ready_project(root: &Path, mixed: bool) -> PathBuf {
    let descriptor = fixtures::multi_fixture(root);
    let mut scene = get(&root.join("scene.json"));
    scene.as_object_mut().unwrap().remove("presentation");
    scene["source_scope_ids"] = scene["canonical_cue_ids"].clone();
    put(&root.join("scene.json"), &scene);
    let mut bindings = get(&root.join("scene-bindings.json"));
    for (name, key, rate) in [
        ("first", "clock", 48000),
        ("second", "clock2", if mixed { 44100 } else { 48000 }),
    ] {
        let path = root.join(format!("timing/{name}.json"));
        let mut alignment = get(&path);
        alignment["sample_rate"] = json!(rate);
        alignment["cue_end_sample"] = json!(alignment["cue_end_sample"].as_u64().unwrap() * 1000);
        for marker in alignment["semantic_markers"]
            .as_object_mut()
            .unwrap()
            .values_mut()
        {
            *marker = json!(marker.as_u64().unwrap() * 1000);
        }
        put(&path, &alignment);
        let mut asset = fixtures::asset(&fs::read(path).unwrap());
        asset["logical_id"] = json!(key);
        bindings["assets"][key] = asset;
    }
    put(&root.join("scene-bindings.json"), &bindings);
    let mut graph = get(&root.join("selected-graph.json"));
    for (slot, take) in [("narration", "take"), ("narration2", "take2")] {
        let asset = &bindings["assets"][take];
        graph["slots"].as_array_mut().unwrap().push(json!({"slot_id":slot,"beat_id":"cue","lane":"narration","disposition":"selected","selected_revision_id":"v1","revisions":[{"revision_id":"v1","asset":{"logical_id":asset["logical_id"],"sha256":asset["sha256"],"cache_uri":asset["cache_uri"]}}]}));
        graph["nodes"][0]["slots"]
            .as_array_mut()
            .unwrap()
            .push(json!(slot));
    }
    fixtures::save_graph(root, graph);
    for (role, value) in [
        (
            "catalog.json",
            json!({"schema":"reel.scene-template-catalog.v1","templates":[]}),
        ),
        (
            "episode.json",
            json!({"schema":"reel.episode-authoring.v1","episode_id":"synthetic-episode","season_id":"synthetic-season","authoring_state":"ready-for-private-build","master_template_id":"synthetic-master","scene_policy_id":"synthetic-policy","presentation":[],"score_palette":[],"scene_ids":["synthetic"]}),
        ),
        (
            "policy.json",
            json!({"schema":"reel.scene-policy.v1","policy_id":"synthetic-policy","target_composition_seconds_min":0.1,"target_composition_seconds_max":10.0,"hard_unchanged_composition_seconds_max":10.0,"semantic_cuts_required":true}),
        ),
        (
            "episode-bindings.json",
            json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"synthetic-episode","assets":{}}),
        ),
        (
            "season-bindings.json",
            json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"synthetic-season","assets":{}}),
        ),
        (
            "profile.json",
            json!({"schema":"reel.scene-delivery-profile.v1","width":320,"height":180,"sample_rate":48000,"frame_rate_numerator":24,"frame_rate_denominator":1,"max_composition_samples":480000,"score_gain_db":-18,"score_fade_in_samples":0,"score_fade_out_samples":0}),
        ),
    ] {
        put(&root.join(role), &value);
    }
    let mut config = get(&descriptor);
    for role in [
        "catalog.json",
        "episode.json",
        "policy.json",
        "episode-bindings.json",
        "season-bindings.json",
    ] {
        config["files"][role] = json!(role);
    }
    put(&descriptor, &config);
    descriptor
}
fn edited_candidate(temp: &Path, mixed: bool) -> PathBuf {
    let root = temp.join("source");
    let descriptor = ready_project(&root, mixed);
    let before = snapshot(&root);
    let project = Project::open(&descriptor).unwrap();
    let (docs, locks) = editor::load(&project).unwrap();
    let sequence = Sequence::new(&docs, true).unwrap();
    assert_eq!(
        sequence.session_clock()["delivery_clock"]["state"],
        if mixed {
            "mixed-native-rates"
        } else {
            "matches-profile"
        }
    );
    let mut rows = sequence.rows().as_array().unwrap().clone();
    rows.swap(0, 1);
    rows[0]["samples"] = json!(31000);
    rows[1]["samples"] = json!(65000);
    rows[0]["picture_binding"] = json!("one");
    rows[0]["comment"] = json!("Synthetic compiler check");
    rows.remove(2);
    rows[2]["samples"] = json!(88000);
    let request = temp.join("request.json");
    put(
        &request,
        &json!({"schema":"reel.cel-sorter-edit.v3","scene_id":"synthetic","expected_project":project.descriptor_sha256(),"expected_inputs":locks,"rows":rows}),
    );
    let candidate = temp.join("candidate");
    editor::apply(&descriptor, &request, &candidate).unwrap();
    assert_eq!(snapshot(&root), before);
    let next = Project::open(&candidate.join("cel-sorter-project.json")).unwrap();
    editor::load(&next).unwrap();
    candidate
}
fn manifest() -> CompileManifest {
    serde_json::from_value(json!({"schema":"reel.scene-delivery-compile.v1","catalog":"catalog.json","episode":"episode.json","scene":"scene.json","policy":"policy.json","season_bindings":"season-bindings.json","episode_bindings":"episode-bindings.json","scene_bindings":"scene-bindings.json","graph":"selected-graph.json","pointer":"selected-pointer.json","alignment_paths":"timing/paths.json","profile":"profile.json","language":"es","delivery_id":"synthetic-edit","delivery_title":"Synthetic native edit","output_dir":"delivery"})).unwrap()
}
#[test]
fn inserted_native_cel_compiles_both_languages_and_preserves_existing_slot_revisions() {
    for separate in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let initial = ready_project(&temp.path().join("intake"), false);
        let p = Project::open(&initial).unwrap();
        let (initial_docs, initial_hashes) = editor::load(&p).unwrap();
        let normalization = temp.path().join("normalize.json");
        put(
            &normalization,
            &json!({"schema":"reel.cel-sorter-edit.v3","scene_id":"synthetic","expected_project":p.descriptor_sha256(),"expected_inputs":initial_hashes,"rows":Sequence::for_project(&initial_docs,&p).unwrap().rows()}),
        );
        editor::apply(&initial, &normalization, &source).unwrap();
        let descriptor = source.join("cel-sorter-project.json");
        fixtures::add_english_lane(&source, &descriptor);
        if separate {
            fixtures::separate_language_graphs(&source, &descriptor);
        }
        let before = snapshot(&source);
        let project = Project::open(&descriptor).unwrap();
        let (docs, locks) = editor::load(&project).unwrap();
        let request = temp.path().join("insert.json");
        put(
            &request,
            &json!({"schema":"reel.cel-sorter-insert.v1","scene_id":"synthetic","language":"es","expected_project":project.descriptor_sha256(),"expected_inputs":locks,"after_semantic_id":"a","split_after_samples":5001,
            "shared_event":{"semantic_id":"new-cel","canonical_cue_id":"cue","picture_binding":"two","score":{"disposition":"silence"},"language_ids":["es"]},
            "language_event":{"semantic_id":"new-cel","event_id":"new-event","cue_id":"es-cue","semantic_trigger_id":"new-trigger","picture_slot_id":"new-slot"}}),
        );
        let candidate = temp.path().join("candidate");
        reel_assets::cel_source::insert::apply(&descriptor, &request, &candidate).unwrap();
        assert_eq!(snapshot(&source), before);
        let new_project = Project::open(&candidate.join("cel-sorter-project.json")).unwrap();
        let (after, _) = editor::load(&new_project).unwrap();
        let rows = Sequence::for_project(&after, &new_project).unwrap().rows();
        assert_eq!(rows.as_array().unwrap().len(), 5);
        assert_eq!(rows[0]["samples"], 5001);
        assert_eq!(rows[1]["samples"], 41999);
        let new_graph = &after["selected-graph.json"];
        for slot in docs["selected-graph.json"]["slots"].as_array().unwrap() {
            assert_eq!(
                new_graph["slots"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|s| s["slot_id"] == slot["slot_id"])
                    .unwrap(),
                slot
            );
        }
        for (language, events) in [("es", 5), ("en", 4)] {
            let mut compile = manifest();
            compile.language = language.into();
            compile.output_dir = format!("delivery-{language}");
            if separate {
                compile.graph = format!("{language}-graph.json");
                compile.pointer = format!("{language}-pointer.json");
            }
            if language == "en" {
                compile.alignment_paths = "timing/english-paths.json".into();
            }
            let result = compile_to_dir(&candidate, &compile).unwrap();
            assert_eq!(result["semantic_event_count"], events);
            let contract = get(&candidate.join(format!("delivery-{language}/contract.json")));
            assert_eq!(
                contract["cues"][0]["duration_samples"],
                if language == "es" { 96000 } else { 144000 }
            );
            assert_eq!(
                contract["cues"][1]["duration_samples"],
                if language == "es" { 88000 } else { 136000 }
            );
        }
        assert_eq!(
            after["scene.json"]["language_event_bindings"]["en"],
            docs["scene.json"]["language_event_bindings"]["en"]
        );
    }
}
#[test]
fn explicit_new_picture_compiles_with_append_only_revision_and_protected_english() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let descriptor = ready_project(&source, false);
    fixtures::add_english_lane(&source, &descriptor);
    let mut bindings = get(&source.join("scene-bindings.json"));
    bindings["assets"]["candidate-picture"] = fixtures::asset(b"new-primary-picture");
    put(&source.join("scene-bindings.json"), &bindings);
    let mut config = get(&descriptor);
    config["picture_candidates"] = json!(["candidate-picture"]);
    put(&descriptor, &config);
    let before = snapshot(&source);
    let project = Project::open(&descriptor).unwrap();
    let (docs, locks) = editor::load(&project).unwrap();
    let mut rows = Sequence::for_project(&docs, &project).unwrap().rows();
    rows[0]["picture_binding"] = json!("candidate-picture");
    rows[0]["comment"] = json!("New primary picture, same clock");
    let request = temp.path().join("request.json");
    put(
        &request,
        &json!({"schema":"reel.cel-sorter-edit.v4","language":"es","scene_id":"synthetic","expected_project":project.descriptor_sha256(),"expected_inputs":locks,"rows":rows}),
    );
    let candidate = temp.path().join("candidate");
    editor::apply(&descriptor, &request, &candidate).unwrap();
    assert_eq!(snapshot(&source), before);
    let next_descriptor = candidate.join("cel-sorter-project.json");
    let next = Project::open(&next_descriptor).unwrap();
    assert_eq!(next.picture_candidates(), &["candidate-picture"]);
    editor::load(&next).unwrap();
    let graph = get(&candidate.join("selected-graph.json"));
    let slot = graph["slots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|slot| slot["slot_id"] == "p1")
        .unwrap();
    assert_eq!(slot["revisions"].as_array().unwrap().len(), 2);
    assert_eq!(
        slot["revisions"][0],
        docs["selected-graph.json"]["slots"][0]["revisions"][0]
    );
    assert_eq!(slot["revisions"][1]["supersedes"], "v1");
    assert_eq!(
        get(&candidate.join("scene.json"))["language_event_bindings"]["en"],
        docs["scene.json"]["language_event_bindings"]["en"]
    );
    for language in ["es", "en"] {
        let mut compile = manifest();
        compile.language = language.into();
        compile.output_dir = format!("delivery-{language}");
        if language == "en" {
            compile.alignment_paths = "timing/english-paths.json".into();
        }
        assert_eq!(
            compile_to_dir(&candidate, &compile).unwrap()["cue_count"],
            2
        );
    }
    // A palette change changes the project lock and cannot reuse this edit.
    config["picture_candidates"] = json!([]);
    put(&descriptor, &config);
    let stale = temp.path().join("stale");
    assert!(editor::apply(&descriptor, &request, &stale).is_err());
    assert!(!stale.exists());
}
#[test]
fn edited_multicue_candidate_compiles_to_delivery_with_exact_cue_anchors_and_revision_lineage() {
    let temp = tempfile::tempdir().unwrap();
    let root = edited_candidate(temp.path(), false);
    let receipt = compile_to_dir(&root, &manifest()).unwrap();
    assert_eq!(receipt["cue_count"], 2);
    assert_eq!(receipt["semantic_event_count"], 3);
    let contract = get(&root.join("delivery/contract.json"));
    assert_eq!(
        contract["cues"],
        json!([{"cue_id":"es-cue","duration_samples":96000},{"cue_id":"second-cue","duration_samples":88000}])
    );
    assert!(
        contract["attachments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["start"]["cue_id"] == "es-cue"
                && item["start"]["offset_samples"] == 31000)
    );
    let graph = get(&root.join("selected-graph.json"));
    let slot = graph["slots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|slot| slot["slot_id"] == "p2")
        .unwrap();
    assert_eq!(slot["revisions"].as_array().unwrap().len(), 2);
    let job = get(&root.join("delivery/job.json"));
    assert_eq!(job["pictures"].as_array().unwrap().len(), 3);
    assert_eq!(job["audio"].as_array().unwrap().len(), 2);
    let build = get(&root.join("delivery/build.json"));
    assert_eq!(build["alignment_paths"], "timing/paths.json");
    assert!(root.join("delivery/semantic-delivery.json").is_file());
    let before = snapshot(&root.join("delivery"));
    assert!(compile_to_dir(&root, &manifest()).is_err());
    assert_eq!(snapshot(&root.join("delivery")), before);
    assert_eq!(
        get(&root.join("edit-request.json"))["rows"][0]["comment"],
        "Synthetic compiler check"
    );
}
#[test]
fn mixed_native_rates_remain_editable_but_delivery_compiler_holds_without_resampling_or_output() {
    let temp = tempfile::tempdir().unwrap();
    let root = edited_candidate(temp.path(), true);
    let before = snapshot(&root);
    let error = compile_to_dir(&root, &manifest()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("cue sample rate differs from delivery profile")
    );
    assert!(!root.join("delivery").exists());
    assert_eq!(snapshot(&root), before);
}

#[test]
fn scoped_bilingual_edits_compile_both_languages_with_other_lane_intact() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let descriptor = ready_project(&source, false);
    fixtures::add_english_lane(&source, &descriptor);
    let before = snapshot(&source);
    let project = Project::open(&descriptor).unwrap();
    let (docs, locks) = editor::load(&project).unwrap();
    let sequence = Sequence::for_project(&docs, &project).unwrap();
    let mut rows = sequence.rows().as_array().unwrap().clone();
    rows.swap(0, 1);
    rows[0]["samples"] = json!(31000);
    rows[1]["samples"] = json!(65000);
    rows[0]["picture_binding"] = json!("one");
    rows.remove(2);
    rows[2]["samples"] = json!(88000);
    let request = temp.path().join("request.json");
    put(
        &request,
        &json!({"schema":"reel.cel-sorter-edit.v4","language":"es","expected_project":project.descriptor_sha256(),"expected_inputs":locks,"scene_id":"synthetic","rows":rows}),
    );
    let output = temp.path().join("candidate");
    editor::apply(&descriptor, &request, &output).unwrap();
    assert_eq!(snapshot(&source), before);
    for (language, events) in [("es", 3), ("en", 4)] {
        let mut request = manifest();
        request.language = language.into();
        request.delivery_id = format!("synthetic-{language}");
        request.output_dir = format!("delivery-{language}");
        if language == "en" {
            request.alignment_paths = "timing/english-paths.json".into();
        }
        let receipt = compile_to_dir(&output, &request).unwrap();
        assert_eq!(receipt["cue_count"], 2);
        assert_eq!(receipt["semantic_event_count"], events);
    }
    assert_eq!(
        fs::read(output.join("timing/english-0.json")).unwrap(),
        before[&PathBuf::from("timing/english-0.json")]
    );
    assert_eq!(
        get(&output.join("scene.json"))["language_event_bindings"]["en"],
        docs["scene.json"]["language_event_bindings"]["en"]
    );
}

#[test]
fn separate_language_graph_candidates_compile_independently_without_merging_control_files() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let descriptor = ready_project(&source, false);
    fixtures::add_english_lane(&source, &descriptor);
    fixtures::separate_language_graphs(&source, &descriptor);
    let original = fs::read(source.join("en-graph.json")).unwrap();
    let project = Project::open(&descriptor).unwrap();
    let (docs, locks) = editor::load(&project).unwrap();
    let mut rows = Sequence::for_project(&docs, &project).unwrap().rows();
    rows[0]["picture_binding"] = json!("two");
    let request = temp.path().join("request.json");
    put(
        &request,
        &json!({"schema":"reel.cel-sorter-edit.v4","language":"es","scene_id":"synthetic","expected_project":project.descriptor_sha256(),"expected_inputs":locks,"rows":rows}),
    );
    let output = temp.path().join("candidate");
    editor::apply(&descriptor, &request, &output).unwrap();
    assert_eq!(fs::read(output.join("en-graph.json")).unwrap(), original);
    for language in ["es", "en"] {
        let mut request = manifest();
        request.language = language.into();
        request.graph = format!("{language}-graph.json");
        request.pointer = format!("{language}-pointer.json");
        request.output_dir = format!("delivery-{language}");
        request.delivery_id = format!("synthetic-{language}");
        if language == "en" {
            request.alignment_paths = "timing/english-paths.json".into();
        }
        assert_eq!(
            compile_to_dir(&output, &request).unwrap()["semantic_event_count"],
            4
        );
    }
}

#[test]
fn native_score_insertion_compiles_only_the_selected_language_and_exact_music_primary() {
    for scope in [
        "scene-bindings.json",
        "episode-bindings.json",
        "season-bindings.json",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let descriptor = ready_project(&source, false);
        fixtures::add_english_lane(&source, &descriptor);
        if scope == "season-bindings.json" {
            fixtures::separate_language_graphs(&source, &descriptor);
        }
        let mut episode = get(&source.join("episode.json"));
        episode["score_palette"] = json!([{"role":"theme","theme_id":"theme","source_poem_id":"poem","arrangement_id":"arrangement","asset_binding":"score.theme"}]);
        put(&source.join("episode.json"), &episode);
        let score = fixtures::asset(b"original-score-primary");
        let mut bindings = get(&source.join(scope));
        bindings["assets"]["score.theme"] = score.clone();
        put(&source.join(scope), &bindings);
        let p = Project::open(&descriptor).unwrap();
        let (docs, locks) = editor::load(&p).unwrap();
        let sequence = Sequence::for_project(&docs, &p).unwrap();
        let request = temp.path().join("normalize.json");
        put(
            &request,
            &json!({"schema":"reel.cel-sorter-edit.v4","language":"es","expected_project":p.descriptor_sha256(),"expected_inputs":locks,"scene_id":"synthetic","rows":sequence.rows()}),
        );
        let normalized = temp.path().join("normalized");
        editor::apply(&descriptor, &request, &normalized).unwrap();
        let input = normalized.join("cel-sorter-project.json");
        let p = Project::open(&input).unwrap();
        let (docs, locks) = editor::load(&p).unwrap();
        let shared = docs["scene.json"]["shared_events"][0].clone();
        let local = docs["scene.json"]["language_event_bindings"]["es"][0].clone();
        let insertion = json!({"schema":"reel.cel-sorter-insert.v1","scene_id":"synthetic","language":"es","expected_project":p.descriptor_sha256(),"expected_inputs":locks,"after_semantic_id":shared["semantic_id"],"split_after_samples":24000,"shared_event":{"semantic_id":"scored-new","canonical_cue_id":shared["canonical_cue_id"],"picture_binding":"two","score":{"disposition":"role","role":"theme"},"language_ids":["es"]},"language_event":{"semantic_id":"scored-new","event_id":"scored-new-event","cue_id":local["cue_id"],"semantic_trigger_id":"scored-new-marker","picture_slot_id":"scored-new-slot"},"human_approval":false});
        let req = temp.path().join("insert.json");
        put(&req, &insertion);
        let out = temp.path().join("candidate");
        let before = snapshot(&normalized);
        reel_assets::cel_source::insert::apply(&input, &req, &out).unwrap();
        assert_eq!(snapshot(&normalized), before);
        for language in ["es", "en"] {
            let mut m = manifest();
            m.language = language.into();
            m.output_dir = format!("compiled-{language}");
            m.delivery_id = format!("scored-{language}");
            if language == "en" {
                m.alignment_paths = "timing/english-paths.json".into();
            }
            if scope == "season-bindings.json" {
                m.graph = format!("{language}-graph.json");
                m.pointer = format!("{language}-pointer.json");
            }
            let receipt = compile_to_dir(&out, &m).unwrap();
            let job = get(&out.join(format!("compiled-{language}/job.json")));
            let music = job["audio"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|a| a["bus"] == "M")
                .collect::<Vec<_>>();
            if language == "es" {
                assert_eq!(music.len(), 1);
                assert_eq!(music[0]["source"]["sha256"], score["sha256"]);
                assert_eq!(receipt["score_roles"], json!(["theme"]));
            } else {
                assert!(music.is_empty());
                assert_eq!(receipt["score_roles"], json!([]));
            }
        }
    }
}

#[test]
fn native_effect_insertion_compiles_exact_inputs_only_in_selected_language() {
    for scope in [
        "scene-bindings.json",
        "episode-bindings.json",
        "season-bindings.json",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let descriptor = ready_project(&source, false);
        fixtures::add_english_lane(&source, &descriptor);
        if scope == "season-bindings.json" {
            fixtures::separate_language_graphs(&source, &descriptor);
        }
        let sonic = fixtures::asset(b"sonic-primary");
        let recipe = fixtures::asset(br#"{"schema":"reel.timed-picture-transform.v1","zoom_step":0.0001,"zoom_max":1.01,"translation":{"amplitude_x_pixels":1.0,"amplitude_y_pixels":1.0,"period_frames":24}}"#);
        let mut bindings = get(&source.join(scope));
        bindings["assets"]["sonic.test"] = sonic.clone();
        bindings["assets"]["vfx.test"] = recipe.clone();
        put(&source.join(scope), &bindings);
        let mut config = get(&descriptor);
        config["sonic_candidates"] = json!(["sonic.test"]);
        config["vfx_candidates"] = json!(["vfx.test"]);
        put(&descriptor, &config);
        let mut profile = get(&source.join("profile.json"));
        profile["vfx_render_mode_by_binding"] = json!({"vfx.test":"timed-picture-transform"});
        put(&source.join("profile.json"), &profile);
        let p = Project::open(&descriptor).unwrap();
        let (docs, locks) = editor::load(&p).unwrap();
        let request = temp.path().join("normalize.json");
        put(
            &request,
            &json!({"schema":"reel.cel-sorter-edit.v4","language":"es","expected_project":p.descriptor_sha256(),"expected_inputs":locks,"scene_id":"synthetic","rows":Sequence::for_project(&docs,&p).unwrap().rows()}),
        );
        let normalized = temp.path().join("normalized");
        editor::apply(&descriptor, &request, &normalized).unwrap();
        let input = normalized.join("cel-sorter-project.json");
        let p = Project::open(&input).unwrap();
        let (docs, locks) = editor::load(&p).unwrap();
        let shared = &docs["scene.json"]["shared_events"][0];
        let local = &docs["scene.json"]["language_event_bindings"]["es"][0];
        let req = temp.path().join("insert.json");
        put(
            &req,
            &json!({"schema":"reel.cel-sorter-insert.v1","scene_id":"synthetic","language":"es","expected_project":p.descriptor_sha256(),"expected_inputs":locks,"after_semantic_id":shared["semantic_id"],"split_after_samples":24000,"shared_event":{"semantic_id":"effect-new","canonical_cue_id":shared["canonical_cue_id"],"picture_binding":"two","score":{"disposition":"silence"},"language_ids":["es"],"sonic_bindings":["sonic.test"],"vfx_bindings":["vfx.test"]},"language_event":{"semantic_id":"effect-new","event_id":"effect-new-event","cue_id":local["cue_id"],"semantic_trigger_id":"effect-new-marker","picture_slot_id":"effect-new-slot"},"human_approval":false}),
        );
        let before = snapshot(&normalized);
        let out = temp.path().join("candidate");
        reel_assets::cel_source::insert::apply(&input, &req, &out).unwrap();
        assert_eq!(snapshot(&normalized), before);
        for language in ["es", "en"] {
            let mut m = manifest();
            m.language = language.into();
            m.output_dir = format!("compiled-{language}");
            m.delivery_id = format!("effects-{language}");
            if language == "en" {
                m.alignment_paths = "timing/english-paths.json".into();
            }
            if scope == "season-bindings.json" {
                m.graph = format!("{language}-graph.json");
                m.pointer = format!("{language}-pointer.json");
            }
            compile_to_dir(&out, &m).unwrap();
            let job = get(&out.join(format!("compiled-{language}/job.json")));
            let effects = job["audio"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|a| a["bus"] == "E")
                .collect::<Vec<_>>();
            let layers = job["external_layers"].as_array().unwrap();
            if language == "es" {
                assert_eq!(effects.len(), 1);
                assert_eq!(effects[0]["source"]["sha256"], sonic["sha256"]);
                assert_eq!(layers.len(), 1);
                assert_eq!(layers[0]["evidence"]["sha256"], recipe["sha256"]);
                assert_eq!(layers[0]["render_mode"], "timed-picture-transform");
            } else {
                assert!(effects.is_empty());
                assert!(layers.is_empty());
            }
        }
    }
}
