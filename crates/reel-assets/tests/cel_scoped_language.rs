//! Synthetic scope and exact-byte preservation tests; no media/cache access.
#[path = "fixtures/cel_native.rs"]
mod fixtures;
use reel_assets::cel_source::{editor, project::Project, sequence::Sequence};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn get(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn put(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn fixture(root: &Path) -> PathBuf {
    let descriptor = fixtures::multi_fixture(root);
    fixtures::add_english_lane(root, &descriptor);
    descriptor
}
fn request(descriptor: &Path, rows: Value) -> Value {
    let project = Project::open(descriptor).unwrap();
    let (docs, locks) = editor::load(&project).unwrap();
    json!({"schema":"reel.cel-sorter-edit.v4","language":project.language(),"expected_project":project.descriptor_sha256(),"expected_inputs":locks,"scene_id":docs["scene.json"]["scene_id"],"rows":rows})
}
fn rows(descriptor: &Path) -> Value {
    let project = Project::open(descriptor).unwrap();
    let (docs, _) = editor::load(&project).unwrap();
    Sequence::for_project(&docs, &project).unwrap().rows()
}
fn apply(temp: &Path, descriptor: &Path, request: &Value) -> PathBuf {
    let path = temp.join("request.json");
    put(&path, request);
    let output = temp.join("candidate");
    editor::apply(descriptor, &path, &output).unwrap();
    output
}
#[test]
fn scoped_reorder_substitute_and_remove_preserve_other_language_graph_and_exact_alignments() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let descriptor = fixture(&source);
    let original = get(&source.join("scene.json"));
    let old_bindings = get(&source.join("scene-bindings.json"));
    let old_graph = get(&source.join("selected-graph.json"));
    let protected: Vec<_> = (0..2)
        .map(|i| fs::read(source.join(format!("timing/english-{i}.json"))).unwrap())
        .collect();
    let mut edits = rows(&descriptor).as_array().unwrap().clone();
    edits.swap(0, 1);
    edits[0]["samples"] = json!(31);
    edits[1]["samples"] = json!(65);
    edits[0]["picture_binding"] = json!("one");
    edits[0]["comment"] = json!("Synthetic scoped edit");
    edits.remove(2);
    edits[2]["samples"] = json!(88);
    let output = apply(
        temp.path(),
        &descriptor,
        &request(&descriptor, json!(edits)),
    );
    let new = get(&output.join("scene.json"));
    assert_eq!(new["languages"], original["languages"]);
    assert_eq!(
        new["language_event_bindings"]["en"],
        original["language_event_bindings"]["en"]
    );
    assert_eq!(new["shared_events"][1]["picture_binding"], "two");
    assert_eq!(
        new["language_event_bindings"]["es"][0]["picture_binding_override"],
        "one"
    );
    assert_eq!(new["shared_events"][2]["semantic_id"], "c");
    assert_eq!(new["shared_events"][2]["language_ids"], json!(["en"]));
    for (i, raw) in protected.iter().enumerate() {
        assert_eq!(
            &fs::read(output.join(format!("timing/english-{i}.json"))).unwrap(),
            raw
        );
        let key = format!("english-clock-{i}");
        assert_eq!(
            get(&output.join("scene-bindings.json"))["assets"][&key],
            old_bindings["assets"][&key]
        );
    }
    let graph = get(&output.join("selected-graph.json"));
    for role in ["slots", "events"] {
        for record in old_graph[role].as_array().unwrap().iter().filter(|item| {
            if role == "events" {
                item["language"] == "en"
            } else {
                item["slot_id"].as_str().unwrap().starts_with("en-")
            }
        }) {
            let key = if role == "events" {
                "event_id"
            } else {
                "slot_id"
            };
            assert_eq!(
                graph[role]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|item| item[key] == record[key])
                    .unwrap(),
                record
            );
        }
    }
    let project = Project::open(&output.join("cel-sorter-project.json")).unwrap();
    let (docs, _) = editor::load(&project).unwrap();
    assert_eq!(
        Sequence::for_project(&docs, &project)
            .unwrap()
            .rows()
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(get(&source.join("scene.json")), original);
    assert_eq!(
        get(&output.join("edit-request.json"))["rows"][0]["comment"],
        "Synthetic scoped edit"
    );
}
#[test]
fn reverse_language_edit_keeps_first_lane_and_unchanged_raw_controls() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let descriptor = fixture(&source);
    let mut config = get(&descriptor);
    config["language"] = json!("en");
    put(&descriptor, &config);
    let original = get(&source.join("scene.json"));
    let raw = fs::read(source.join("timing/first.json")).unwrap();
    let mut edits = rows(&descriptor).as_array().unwrap().clone();
    edits.swap(0, 1);
    edits[0]["samples"] = json!(33);
    edits[1]["samples"] = json!(111);
    let output = apply(
        temp.path(),
        &descriptor,
        &request(&descriptor, json!(edits)),
    );
    assert_eq!(
        get(&output.join("scene.json"))["language_event_bindings"]["es"],
        original["language_event_bindings"]["es"]
    );
    assert_eq!(
        get(&output.join("scene.json"))["shared_events"],
        original["shared_events"]
    );
    assert_eq!(fs::read(output.join("timing/first.json")).unwrap(), raw);
    let project = Project::open(&output.join("cel-sorter-project.json")).unwrap();
    editor::load(&project).unwrap();
}
#[test]
fn language_scope_downgrades_foreign_pictures_and_changed_protected_sources_refuse_before_output() {
    let temp = tempfile::tempdir().unwrap();
    let descriptor = fixture(&temp.path().join("source"));
    let original = request(&descriptor, rows(&descriptor));
    let path = temp.path().join("request.json");
    for case in 0..5 {
        let mut changed = original.clone();
        match case {
            0 => changed["language"] = json!("en"),
            1 => {
                changed.as_object_mut().unwrap().remove("language");
            }
            2 => changed["schema"] = json!("reel.cel-sorter-edit.v3"),
            3 => changed["rows"][0]["picture_binding"] = json!("english-image"),
            _ => changed["rows"][0]["cue_id"] = json!("en-es-cue"),
        };
        put(&path, &changed);
        let output = temp.path().join(format!("bad-{case}"));
        assert!(editor::apply(&descriptor, &path, &output).is_err());
        assert!(!output.exists());
    }
    let protected = descriptor.parent().unwrap().join("timing/english-0.json");
    let mut raw = fs::read(&protected).unwrap();
    raw.push(b' ');
    fs::write(&protected, raw).unwrap();
    put(&path, &original);
    assert!(editor::apply(&descriptor, &path, &temp.path().join("changed-source")).is_err());
}
fn shared_slot(root: &Path) {
    let mut scene = get(&root.join("scene.json"));
    scene["language_event_bindings"]["en"][0]["picture_slot_id"] = json!("p1");
    scene["language_event_bindings"]["en"][0]["picture_binding_override"] = json!("one");
    put(&root.join("scene.json"), &scene);
    let mut graph = get(&root.join("selected-graph.json"));
    graph["slots"]
        .as_array_mut()
        .unwrap()
        .retain(|slot| slot["slot_id"] != "en-p1");
    graph["nodes"][0]["slots"]
        .as_array_mut()
        .unwrap()
        .retain(|id| id != "en-p1");
    let picture = graph["events"][0]["picture"].clone();
    for event in graph["events"].as_array_mut().unwrap() {
        if event["event_id"] == "en-event-a" {
            event["picture"] = picture.clone();
        }
    }
    fixtures::save_graph(root, graph);
}
#[test]
fn another_language_reference_protects_a_shared_slot_from_change_or_removal() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let descriptor = fixture(&source);
    shared_slot(&source);
    let mut edits = rows(&descriptor);
    edits[0]["picture_binding"] = json!("two");
    let path = temp.path().join("rejected.json");
    put(&path, &request(&descriptor, edits));
    assert!(
        editor::apply(&descriptor, &path, &temp.path().join("rejected"))
            .unwrap_err()
            .to_string()
            .contains("shared by another language")
    );
    let mut edits = rows(&descriptor).as_array().unwrap().clone();
    edits.remove(0);
    edits[0]["samples"] = json!(96);
    let output = apply(
        temp.path(),
        &descriptor,
        &request(&descriptor, json!(edits)),
    );
    assert!(
        get(&output.join("selected-graph.json"))["slots"]
            .as_array()
            .unwrap()
            .iter()
            .any(|slot| slot["slot_id"] == "p1")
    );
    let project = Project::open(&output.join("cel-sorter-project.json")).unwrap();
    editor::load(&project).unwrap();
}
#[test]
fn omitted_or_invalid_language_and_incomplete_protected_cue_index_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    let descriptor = fixture(&root);
    let original = get(&descriptor);
    for language in [
        Value::Null,
        json!(""),
        json!("missing"),
        json!("x".repeat(65)),
    ] {
        let mut config = original.clone();
        config["language"] = language;
        put(&descriptor, &config);
        assert!(
            Project::open(&descriptor)
                .and_then(|project| editor::load(&project))
                .is_err()
        );
    }
    let mut old = original.clone();
    old["schema"] = json!("reel.cel-sorter-project.v2");
    put(&descriptor, &old);
    assert!(Project::open(&descriptor).is_err());
    put(&descriptor, &original);
    let mut paths = get(&root.join("timing/english-paths.json"));
    paths.as_object_mut().unwrap().remove("en-es-cue");
    put(&root.join("timing/english-paths.json"), &paths);
    assert!(editor::load(&Project::open(&descriptor).unwrap()).is_err());
}

#[test]
fn local_cue_names_can_overlap_between_languages_without_cross_lane_alignment_aliases() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let descriptor = fixture(&source);
    let mut scene = get(&source.join("scene.json"));
    let mut bindings = get(&source.join("scene-bindings.json"));
    let paths = get(&source.join("timing/english-paths.json"));
    let mut updated = json!({});
    let pairs: Vec<_> = scene["languages"]["en"]["cues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cue| {
            let old = cue["cue_id"].as_str().unwrap().to_owned();
            let new = old.strip_prefix("en-").unwrap().to_owned();
            (
                old,
                new,
                cue["phrase_alignment_binding"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    for (old, new, key) in pairs {
        for cue in scene["languages"]["en"]["cues"].as_array_mut().unwrap() {
            if cue["cue_id"] == old {
                cue["cue_id"] = json!(new);
            }
        }
        for event in scene["language_event_bindings"]["en"]
            .as_array_mut()
            .unwrap()
        {
            if event["cue_id"] == old {
                event["cue_id"] = json!(new);
            }
        }
        let filename = paths[&old].as_str().unwrap();
        updated[&new] = json!(filename);
        let path = source.join("timing").join(filename);
        let mut clock = get(&path);
        clock["cue_id"] = json!(new);
        fs::write(&path, serde_json::to_vec_pretty(&clock).unwrap()).unwrap();
        bindings["assets"][&key] = fixtures::asset(&fs::read(path).unwrap());
    }
    put(&source.join("scene.json"), &scene);
    put(&source.join("scene-bindings.json"), &bindings);
    put(&source.join("timing/english-paths.json"), &updated);
    let output = apply(
        temp.path(),
        &descriptor,
        &request(&descriptor, rows(&descriptor)),
    );
    let project = Project::open(&output.join("cel-sorter-project.json")).unwrap();
    editor::load(&project).unwrap();
    assert_eq!(
        get(&output.join("scene.json"))["language_event_bindings"]["en"],
        scene["language_event_bindings"]["en"]
    );
}

#[test]
fn original_native_buffers_reject_duplicate_typed_fields_before_value_normalization() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let descriptor = fixture(&source);
    let path = source.join("scene.json");
    let original = fs::read(&path).unwrap();
    let raw = String::from_utf8(original.clone()).unwrap().replacen(
        "\"scene_id\":",
        "\"scene_id\":\"duplicate\",\"scene_id\":",
        1,
    );
    fs::write(&path, raw).unwrap();
    assert!(
        editor::load(&Project::open(&descriptor).unwrap())
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
    fs::write(path, original).unwrap();
    let path = source.join("timing/english-0.json");
    let original = fs::read(&path).unwrap();
    let raw = String::from_utf8(original).unwrap().replacen(
        "\"sample_rate\":",
        "\"sample_rate\":48,\"sample_rate\":",
        1,
    );
    fs::write(&path, raw).unwrap();
    let mut bindings = get(&source.join("scene-bindings.json"));
    bindings["assets"]["english-clock-0"] = fixtures::asset(&fs::read(path).unwrap());
    put(&source.join("scene-bindings.json"), &bindings);
    assert!(
        editor::load(&Project::open(&descriptor).unwrap())
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
}

#[test]
fn separate_graph_files_preserve_protected_graph_pointer_bytes_and_stale_locks() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let descriptor = fixture(&source);
    fixtures::separate_language_graphs(&source, &descriptor);
    // The same slot label in separate files is not a shared slot domain.
    let mut scene = get(&source.join("scene.json"));
    scene["language_event_bindings"]["en"][0]["picture_slot_id"] = json!("p1");
    put(&source.join("scene.json"), &scene);
    let mut graph = get(&source.join("en-graph.json"));
    for slot in graph["slots"].as_array_mut().unwrap() {
        if slot["slot_id"] == "en-p1" {
            slot["slot_id"] = json!("p1");
        }
    }
    for slot in graph["nodes"][0]["slots"].as_array_mut().unwrap() {
        if *slot == "en-p1" {
            *slot = json!("p1");
        }
    }
    let typed: reel_assembly::Graph = serde_json::from_value(graph.clone()).unwrap();
    graph["lock"] =
        serde_json::to_value(reel_assembly::graph_lock(&typed, "en-draft").unwrap()).unwrap();
    put(&source.join("en-graph.json"), &graph);
    let mut pointer = get(&source.join("en-pointer.json"));
    pointer["selected_lock"] = graph["lock"].clone();
    put(&source.join("en-pointer.json"), &pointer);
    let graph = fs::read(source.join("en-graph.json")).unwrap();
    let pointer = fs::read(source.join("en-pointer.json")).unwrap();
    let mut edits = rows(&descriptor);
    edits[0]["picture_binding"] = json!("two");
    let original = request(&descriptor, edits);
    let output = apply(temp.path(), &descriptor, &original);
    assert_eq!(fs::read(output.join("en-graph.json")).unwrap(), graph);
    assert_eq!(fs::read(output.join("en-pointer.json")).unwrap(), pointer);
    let profile = Project::open(&output.join("cel-sorter-project.json")).unwrap();
    editor::load(&profile).unwrap();
    let mut raw = graph;
    raw.push(b' ');
    fs::write(source.join("en-graph.json"), raw).unwrap();
    let path = temp.path().join("stale.json");
    put(&path, &original);
    assert!(editor::apply(&descriptor, &path, &temp.path().join("stale")).is_err());
    assert!(!temp.path().join("stale").exists());
}
#[test]
fn incomplete_aliased_or_wrong_domain_protected_graph_routes_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let descriptor = fixture(&source);
    fixtures::separate_language_graphs(&source, &descriptor);
    let original = get(&descriptor);
    for case in 0..5 {
        let mut config = original.clone();
        match case {
            0 => config["protected_graphs"] = json!({}),
            1 => {
                config["protected_graphs"] =
                    json!({"es":{"graph":"en-graph.json","pointer":"en-pointer.json"}})
            }
            2 => config["protected_graphs"]["en"]["graph"] = json!("es-graph.json"),
            3 => config["protected_graphs"]["en"]["pointer"] = json!("es-pointer.json"),
            _ => config["protected_graphs"]["en"]["graph"] = json!("../outside.json"),
        };
        put(&descriptor, &config);
        assert!(
            Project::open(&descriptor)
                .and_then(|project| editor::load(&project))
                .is_err(),
            "case {case}"
        );
    }
    put(&descriptor, &original);
    let raw=serde_json::to_string(&original).unwrap().replace("\"en\":{\"graph\":\"en-graph.json\",\"pointer\":\"en-pointer.json\"}","\"en\":{\"graph\":\"en-graph.json\",\"pointer\":\"en-pointer.json\"},\"en\":{\"graph\":\"en-graph.json\",\"pointer\":\"en-pointer.json\"}");
    fs::write(&descriptor, raw).unwrap();
    assert!(Project::open(&descriptor).is_err());
}
