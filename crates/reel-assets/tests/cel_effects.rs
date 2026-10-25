#[path = "fixtures/cel_native.rs"]
mod fixtures;
use reel_assets::cel_source::{
    editor, effects, insert,
    project::{FILES, Project},
    sequence::Sequence,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
fn get(p: &Path) -> Value {
    serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
}
fn put(p: &Path, v: &Value) {
    fs::write(p, serde_json::to_vec(v).unwrap()).unwrap();
}
fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(r: &Path, p: &Path, m: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for e in fs::read_dir(p).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(r, &p, m)
            } else {
                m.insert(p.strip_prefix(r).unwrap().into(), fs::read(p).unwrap());
            }
        }
    }
    let mut m = BTreeMap::new();
    walk(root, root, &mut m);
    m
}
fn request(input: &Path) -> Value {
    let p = Project::open(input).unwrap();
    let (d, h) = editor::load(&p).unwrap();
    let s = Sequence::for_project(&d, &p).unwrap();
    let row = s.rows()[0].clone();
    let id = row["semantic_id"].as_str().unwrap();
    let e = d["scene.json"]["shared_events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["semantic_id"] == id)
        .unwrap();
    let mut r = json!({"schema":"reel.cel-sorter-insert.v1","scene_id":d["scene.json"]["scene_id"],"language":s.language,"expected_inputs":h,"after_semantic_id":id,"shared_event":{"semantic_id":"effect-new","canonical_cue_id":e["canonical_cue_id"],"picture_binding":"two","score":{"disposition":"silence"},"language_ids":[s.language],"sonic_bindings":["sound"],"vfx_bindings":["motion"]},"language_event":{"semantic_id":"effect-new","event_id":"effect-new-event","cue_id":s.cells[id].cue_id,"semantic_trigger_id":"effect-new-marker","picture_slot_id":"effect-new-slot"},"human_approval":false});
    if let Some(h) = p.descriptor_sha256() {
        r["expected_project"] = json!(h);
    }
    r[if p.is_multi_cue() {
        "split_after_samples"
    } else {
        "split_after_frames"
    }] = json!(7);
    r
}
fn bindings(root: &Path) {
    let mut b = get(&root.join("scene-bindings.json"));
    for key in ["sound", "motion", "english-only", "unlisted"] {
        b["assets"][key] = fixtures::asset(key.as_bytes());
    }
    put(&root.join("scene-bindings.json"), &b);
}
fn legacy(root: &Path) {
    fixtures::fixture(root);
    bindings(root);
    let mut s = get(&root.join("scene.json"));
    s["shared_events"][0]["sonic_bindings"] = json!(["sound"]);
    s["shared_events"][1]["vfx_bindings"] = json!(["motion"]);
    put(&root.join("scene.json"), &s);
}
#[test]
fn legacy_current_effects_are_explicit_and_source_bytes_stay_exact() {
    let t = tempfile::tempdir().unwrap();
    let src = t.path().join("src");
    legacy(&src);
    let before = files(&src);
    let p = Project::open(&src).unwrap();
    let (d, _) = editor::load(&p).unwrap();
    let seq = Sequence::for_project(&d, &p).unwrap();
    let choices = effects::choices(&d, &p, &seq).unwrap();
    assert_eq!(choices.sonic.choices.len(), 1);
    assert_eq!(choices.vfx.choices.len(), 1);
    let req = t.path().join("request.json");
    put(&req, &request(&src));
    let out = t.path().join("out");
    insert::apply(&src, &req, &out).unwrap();
    assert_eq!(files(&src), before);
    let scene = get(&out.join("scene.json"));
    assert_eq!(
        scene["shared_events"][1]["sonic_bindings"],
        json!(["sound"])
    );
    assert_eq!(scene["shared_events"][1]["vfx_bindings"], json!(["motion"]));
}
#[test]
fn unlisted_wrong_kind_duplicate_override_and_unselected_effects_fail_before_output() {
    for mode in 0..10 {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        legacy(&src);
        let mut r = request(&src);
        if mode >= 7 {
            let mut b = get(&src.join("scene-bindings.json"));
            match mode {
                7 => b["assets"]["sound"]["selection_state"] = json!("held"),
                8 => b["assets"]["motion"]["cache_uri"] = json!("cache://sha256/wrong"),
                _ => b["assets"]["motion"]["bytes"] = json!(0),
            }
            put(&src.join("scene-bindings.json"), &b);
        }
        // Invalid native identities reject during intake, before choices exist.
        // For valid-but-held metadata, capture current locks so admission is tested.
        if mode < 8 {
            r = request(&src);
        }
        match mode {
            0 => r["shared_event"]["sonic_bindings"] = json!(["unlisted"]),
            1 => r["shared_event"]["sonic_bindings"] = json!(["motion"]),
            2 => r["shared_event"]["vfx_bindings"] = json!(["sound"]),
            3 => r["shared_event"]["sonic_bindings"] = json!(["sound", "sound"]),
            4 => r["language_event"]["vfx_bindings_override"] = json!(["unlisted"]),
            5 => {
                r["shared_event"]["sonic_bindings"] = json!(["unlisted"]);
                r["language_event"]["sonic_bindings_override"] = json!([]);
            }
            6 => r["language_event"]["sonic_bindings_override"] = json!(["sound", "sound"]),
            _ => {}
        }
        let before = files(&src);
        let req = t.path().join("request.json");
        put(&req, &r);
        let out = t.path().join("out");
        assert!(insert::apply(&src, &req, &out).is_err(), "mode {mode}");
        assert!(!out.exists());
        assert_eq!(files(&src), before);
    }
}
#[test]
fn scoped_candidates_preserve_protected_graphs_and_cannot_import_another_language_effect() {
    for separate in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        let descriptor = fixtures::multi_fixture(&src);
        fixtures::add_english_lane(&src, &descriptor);
        if separate {
            fixtures::separate_language_graphs(&src, &descriptor);
        }
        bindings(&src);
        let mut scene = get(&src.join("scene.json"));
        scene["language_event_bindings"]["en"][0]["sonic_bindings_override"] =
            json!(["english-only"]);
        put(&src.join("scene.json"), &scene);
        let p = Project::open(&descriptor).unwrap();
        let (d, h) = editor::load(&p).unwrap();
        let seq = Sequence::for_project(&d, &p).unwrap();
        let edit = t.path().join("normalize.json");
        put(
            &edit,
            &json!({"schema":"reel.cel-sorter-edit.v4","language":"es","expected_project":p.descriptor_sha256(),"expected_inputs":h,"scene_id":"synthetic","rows":seq.rows()}),
        );
        let base = t.path().join("base");
        editor::apply(&descriptor, &edit, &base).unwrap();
        let input = base.join("cel-sorter-project.json");
        let mut config = get(&input);
        config["sonic_candidates"] = json!(["sound", "missing"]);
        config["vfx_candidates"] = json!(["motion"]);
        put(&input, &config);
        let p = Project::open(&input).unwrap();
        let (d, _) = editor::load(&p).unwrap();
        let seq = Sequence::for_project(&d, &p).unwrap();
        let opts = effects::choices(&d, &p, &seq).unwrap();
        assert_eq!(opts.sonic.choices.len(), 1);
        assert_eq!(opts.sonic.held.len(), 1);
        assert!(
            !opts
                .sonic
                .choices
                .iter()
                .any(|c| c.binding == "english-only")
        );
        let before = files(&base);
        let req = t.path().join("insert.json");
        let mut r = request(&input);
        put(&req, &r);
        let out = t.path().join("out");
        insert::apply(&input, &req, &out).unwrap();
        assert_eq!(files(&base), before);
        let copied = get(&out.join("cel-sorter-project.json"));
        assert_eq!(copied["sonic_candidates"], config["sonic_candidates"]);
        assert_eq!(copied["vfx_candidates"], config["vfx_candidates"]);
        r["shared_event"]["sonic_bindings"] = json!(["english-only"]);
        put(&req, &r);
        assert!(insert::apply(&input, &req, &t.path().join("foreign")).is_err());
        r = request(&input);
        put(&req, &r);
        config["sonic_candidates"] = json!([]);
        put(&input, &config);
        assert!(insert::apply(&input, &req, &t.path().join("stale")).is_err());
    }
}
#[test]
fn invalid_candidate_controls_reject_without_loading_media() {
    let t = tempfile::tempdir().unwrap();
    let src = t.path().join("src");
    fixtures::fixture(&src);
    let descriptor = t.path().join("project.json");
    let base = json!({"schema":"reel.cel-sorter-project.v1","root":"src","files":FILES.iter().map(|f|((*f).to_owned(),json!(f))).collect::<serde_json::Map<_,_>>()});
    for candidate in [
        json!([""]),
        json!(["x", "x"]),
        json!(["bad\nkey"]),
        json!([" spaced"]),
        json!(["x".repeat(257)]),
        json!((0..129).map(|i| format!("effect-{i}")).collect::<Vec<_>>()),
    ] {
        for field in ["sonic_candidates", "vfx_candidates"] {
            let mut value = base.clone();
            value[field] = candidate.clone();
            put(&descriptor, &value);
            assert!(Project::open(&descriptor).is_err());
        }
    }
}
