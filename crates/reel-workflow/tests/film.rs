#[path = "support/film_fixture.rs"]
mod support;
use reel_workflow::*;
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn auxiliary_program_identity_changes_task_key_and_legacy_contract_omits_empty_map() {
    let f = support::Fixture::new("es");
    let mut task = f.packet.task.clone();
    assert!(
        serde_json::to_value(task.tool.as_ref().unwrap())
            .unwrap()
            .get("auxiliary_executables")
            .is_none()
    );
    let mut runtime = task.tool.as_ref().unwrap().executable.clone();
    runtime.logical_id = "tools/ffmpeg".into();
    task.tool
        .as_mut()
        .unwrap()
        .auxiliary_executables
        .insert("ffmpeg".into(), runtime.clone());
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        task_snapshots: Default::default(),
        project: f.packet.project.clone(),
        source_commit: f.packet.source_commit.clone(),
        tasks: vec![task.clone()],
    };
    let p = make_packet(&plan, &task.id, BTreeMap::new()).unwrap();
    assert_ne!(p.task_key, f.packet.task_key);
    let mut bad = plan.clone();
    bad.tasks[0]
        .tool
        .as_mut()
        .unwrap()
        .auxiliary_executables
        .insert("FFMPEG".into(), runtime.clone());
    assert!(make_packet(&bad, &task.id, BTreeMap::new()).is_err());
    runtime.bytes = 256 * 1024 * 1024 + 1;
    bad = plan;
    bad.tasks[0]
        .tool
        .as_mut()
        .unwrap()
        .auxiliary_executables
        .insert("ffmpeg".into(), runtime);
    assert!(make_packet(&bad, &task.id, BTreeMap::new()).is_err());
}
#[test]
fn language_local_triggers_and_selected_take_clocks_compile_to_graph_and_review_ids() {
    for (lang, cut, end) in [("es", 2400, 8000), ("en", 4500, 12000)] {
        let mut f = support::Fixture::new(lang);
        let packet = f.packet.clone();
        let g = film::generate(&packet, &mut f).unwrap();
        assert_eq!(g.outputs.len(), 5);
        let map: serde_json::Value = serde_json::from_slice(&g.files["review-map.json"]).unwrap();
        assert_eq!(map["events"][0]["start_sample"], 0);
        assert_eq!(map["events"][0]["end_sample"], cut);
        assert_eq!(map["events"][1]["end_sample"], end);
        assert_eq!(map["events"][1]["cel"]["logical_id"], "film/two.svg");
        assert_eq!(map["rendered"], false);
        assert_eq!(map["approved"], false);
        let graph: reel_assembly::Graph = decode(&g.files["graph.json"]).unwrap();
        let pointer: reel_assembly::SelectedPointer = decode(&g.files["pointer.json"]).unwrap();
        reel_assembly::validate_selected_graph(&pointer, &graph).unwrap();
        assert_eq!(graph.events.len(), 2);
        let again = film::generate(&packet, &mut f).unwrap();
        assert_eq!(g.files, again.files);
    }
}
#[test]
fn stale_take_clock_scope_pointer_text_and_trigger_fail_before_output() {
    for case in 0..7 {
        let mut f = support::Fixture::new("es");
        match case {
            0 => f.change_json("words.json", |v| {
                v["selected_take_sha256"] = json!("b".repeat(64))
            }),
            1 => f.change_json("words.json", |v| v["cue_end_sample"] = json!(7999)),
            2 => f.change_json("scene-scope.json", |v| {
                v["assets"]["take"]["selection_state"] = json!("candidate")
            }),
            3 => f.change_json("pointer.json", |v| {
                v["selected_lock"]["sha256"] = json!("b".repeat(64))
            }),
            4 => f.change_json("scene.json", |v| {
                v["languages"]["es"]["cues"][0]["exact_text_sha256"] = json!("c".repeat(64))
            }),
            5 => f.change_json("scene.json", |v| {
                v["language_event_bindings"]["es"][1]["spoken_trigger_phrase"] = json!("absent")
            }),
            _ => {
                f.values.get_mut("take.wav").unwrap().1[24] = 1;
            }
        }
        let p = f.packet.clone();
        assert!(film::generate(&p, &mut f).is_err(), "case {case}");
    }
}
