//! Shared synthetic native controls for CLI and full delivery-compiler tests.
use reel_assets::cel_source::project::{FILES, Project};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn write(path: &Path, value: &Value) -> std::io::Result<()> {
    fs::write(path, serde_json::to_vec(value).unwrap())
}
pub fn asset(body: &[u8]) -> Value {
    let sha256 = hash(body);
    json!({"logical_id":format!("synthetic/{sha256}"),"sha256":sha256,"bytes":body.len(),"cache_uri":format!("cache://sha256/{sha256}"),"selection_state":"selected-private-production"})
}
pub fn graph_asset(body: &[u8]) -> Value {
    let value = asset(body);
    json!({"logical_id":value["logical_id"],"cache_uri":value["cache_uri"],"sha256":value["sha256"]})
}
pub fn save_graph(project: &Path, mut graph: Value) {
    let typed: reel_assembly::Graph = serde_json::from_value(graph.clone()).unwrap();
    graph["lock"] =
        serde_json::to_value(reel_assembly::graph_lock(&typed, &typed.lock.logical_id).unwrap())
            .unwrap();
    let pointer = json!({"schema":reel_assembly::POINTER_SCHEMA,"logical_id":"synthetic-current","selected_lock":graph["lock"]});
    fs::write(
        project.join("selected-graph.json"),
        serde_json::to_vec(&graph).unwrap(),
    )
    .unwrap();
    fs::write(
        project.join("selected-pointer.json"),
        serde_json::to_vec(&pointer).unwrap(),
    )
    .unwrap();
}

pub fn fixture(project: &Path) {
    fs::create_dir_all(project).unwrap();
    for name in FILES {
        write(&project.join(name), &json!({"unknown":{"preserve":true}})).unwrap();
    }
    let alignment = json!({"schema":"reel.scene-native-alignment.v1","language":"es","cue_id":"es-cue","selected_take_sha256":hash(b"voice"),"sample_rate":48,"cue_end_sample":96,"semantic_markers":{"a":0,"b":48}});
    let ab = serde_json::to_vec(&alignment).unwrap();
    let ah = hash(&ab);
    let scene = json!({"schema":"reel.scene-authoring.v2","scene_id":"synthetic","episode_id":"synthetic-episode","authoring_state":"ready-for-private-build","source_authority_id":"synthetic-authority","canonical_cue_ids":["cue"],
            "languages":{"es":{"cues":[{"cue_id":"es-cue","source_id":"cue","exact_text_sha256":hash(b"text"),"narration_slot_id":"narration","take_binding":"take","phrase_alignment_binding":"clock"}]}},
            "shared_events":[{"semantic_id":"a","canonical_cue_id":"cue","picture_binding":"one","score":{"disposition":"silence"}},{"semantic_id":"b","canonical_cue_id":"cue","picture_binding":"two","score":{"disposition":"silence"}}],
            "language_event_bindings":{"es":[{"semantic_id":"a","event_id":"event-a","cue_id":"es-cue","semantic_trigger_id":"a","picture_slot_id":"p1"},{"semantic_id":"b","event_id":"event-b","cue_id":"es-cue","semantic_trigger_id":"b","picture_slot_id":"p2"}]},
            "presentation":{"role":"synthetic","template_id":"synthetic-template","content":{"unknown":{"preserve":true}}}});
    let mut clock = asset(&ab);
    clock["logical_id"] = json!("clock");
    assert_eq!(clock["sha256"], ah);
    let bindings = json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"synthetic","assets":{"clock":clock,"take":asset(b"voice"),"one":asset(b"first"),"two":asset(b"second")}});
    let graph = json!({"schema":reel_assembly::GRAPH_SCHEMA,"nodes":[{"id":"synthetic","slots":["p1","p2"],"events":["event-a","event-b"]}],
            "events":[{"event_id":"event-a","scene_id":"synthetic","language":"es","narration":{"logical_id":asset(b"voice")["logical_id"],"sha256":hash(b"voice")},"picture":{"logical_id":asset(b"first")["logical_id"],"sha256":hash(b"first")},"phrase_start_seconds":0.0,"phrase_end_seconds":1.0},
                {"event_id":"event-b","scene_id":"synthetic","language":"es","narration":{"logical_id":asset(b"voice")["logical_id"],"sha256":hash(b"voice")},"picture":{"logical_id":asset(b"second")["logical_id"],"sha256":hash(b"second")},"phrase_start_seconds":1.0,"phrase_end_seconds":2.0}],
            "lock":{"logical_id":"draft","sha256":hash(b"old")},"slots":[{"slot_id":"p1","beat_id":"cue","lane":"picture","disposition":"selected","selected_revision_id":"v1","revisions":[{"revision_id":"v1","asset":graph_asset(b"first")}]},
                {"slot_id":"p2","beat_id":"cue","lane":"picture","disposition":"selected","selected_revision_id":"v1","revisions":[{"revision_id":"v1","asset":graph_asset(b"second")}]}]});
    let graph_copy = graph.clone();
    for (name, v) in [
        ("scene.json", scene),
        ("scene-bindings.json", bindings),
        ("alignment.json", alignment),
        ("selected-graph.json", graph),
        (
            "profile.json",
            json!({"frame_rate_numerator":24,"frame_rate_denominator":1}),
        ),
    ] {
        fs::write(project.join(name), serde_json::to_vec(&v).unwrap()).unwrap();
    }
    save_graph(project, graph_copy);
}

pub fn multi_fixture(project: &Path) -> PathBuf {
    use reel_assets::cel_source::project::alignment_role;
    fixture(project);
    let (mut docs, _) =
        reel_assets::cel_source::editor::load(&Project::open(project).unwrap()).unwrap();
    let mut first = docs["alignment.json"].clone();
    first["semantic_markers"] = json!({"ta":0,"tb":47,"unrelated":12});
    let mut second = first.clone();
    second["cue_id"] = json!("second-cue");
    second["selected_take_sha256"] = json!(hash(b"second-voice"));
    second["sample_rate"] = json!(44);
    second["cue_end_sample"] = json!(88);
    second["semantic_markers"] = json!({"tc":0,"td":43,"other":13});
    let scene = docs.get_mut("scene.json").unwrap();
    scene["canonical_cue_ids"] = json!(["cue", "second"]);
    let mut cue = scene["languages"]["es"]["cues"][0].clone();
    cue["cue_id"] = json!("second-cue");
    cue["source_id"] = json!("second");
    cue["take_binding"] = json!("take2");
    cue["phrase_alignment_binding"] = json!("clock2");
    cue["narration_slot_id"] = json!("narration2");
    scene["languages"]["es"]["cues"]
        .as_array_mut()
        .unwrap()
        .push(cue);
    scene["language_event_bindings"]["es"][0]["semantic_trigger_id"] = json!("ta");
    scene["language_event_bindings"]["es"][1]["semantic_trigger_id"] = json!("tb");
    for (index, id, trigger, slot) in [(0, "c", "tc", "p3"), (1, "d", "td", "p4")] {
        let mut event = scene["shared_events"][index].clone();
        event["semantic_id"] = json!(id);
        event["canonical_cue_id"] = json!("second");
        scene["shared_events"].as_array_mut().unwrap().push(event);
        let mut local = scene["language_event_bindings"]["es"][index].clone();
        local["semantic_id"] = json!(id);
        local["cue_id"] = json!("second-cue");
        local["event_id"] = json!(format!("event-{id}"));
        local["semantic_trigger_id"] = json!(trigger);
        local["picture_slot_id"] = json!(slot);
        scene["language_event_bindings"]["es"]
            .as_array_mut()
            .unwrap()
            .push(local);
    }
    let assets = &mut docs.get_mut("scene-bindings.json").unwrap()["assets"];
    assets["clock"] = asset(&serde_json::to_vec(&first).unwrap());
    assets["clock"]["selection_state"] = json!("principal-approved");
    assets["unused-alias"] = assets["clock"].clone();
    assets["clock2"] = asset(&serde_json::to_vec(&second).unwrap());
    assets["take2"] = asset(b"second-voice");
    let mut graph = docs["selected-graph.json"].clone();
    for (index, id, slot) in [(0, "c", "p3"), (1, "d", "p4")] {
        let mut event = graph["events"][index].clone();
        event["event_id"] = json!(format!("event-{id}"));
        event["narration"] = json!({"logical_id":asset(b"second-voice")["logical_id"],"sha256":hash(b"second-voice")});
        graph["events"].as_array_mut().unwrap().push(event);
        let mut picture = graph["slots"][index].clone();
        picture["slot_id"] = json!(slot);
        picture["beat_id"] = json!("second");
        graph["slots"].as_array_mut().unwrap().push(picture);
    }
    graph["nodes"][0]["events"] = json!(["event-a", "event-b", "event-c", "event-d"]);
    graph["nodes"][0]["slots"] = json!(["p1", "p2", "p3", "p4"]);
    fs::create_dir(project.join("timing")).unwrap();
    for (path, value) in [
        ("scene.json", docs["scene.json"].clone()),
        ("scene-bindings.json", docs["scene-bindings.json"].clone()),
        ("timing/first.json", first),
        ("timing/second.json", second),
    ] {
        fs::write(project.join(path), serde_json::to_vec(&value).unwrap()).unwrap();
    }
    save_graph(project, graph);
    fs::write(
        project.join("timing/paths.json"),
        br#"{"es-cue":"first.json","second-cue":"second.json"}"#,
    )
    .unwrap();
    let mut files: BTreeMap<String, String> = FILES
        .iter()
        .take(6)
        .filter(|name| **name != "alignment.json")
        .map(|name| ((*name).into(), (*name).into()))
        .collect();
    files.insert("alignment-paths.json".into(), "timing/paths.json".into());
    let descriptor = project.join("multi.json");
    write(
        &descriptor,
        &json!({"schema":"reel.cel-sorter-project.v2","root":".","files":files}),
    )
    .unwrap();
    let input = Project::open(&descriptor).unwrap();
    let loaded = input.load().unwrap();
    assert!(loaded.controls.contains_key(&alignment_role("second-cue")));
    descriptor
}
