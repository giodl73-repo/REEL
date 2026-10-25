//! Native authoring/lock boundaries on synthetic controls; no media or cache I/O.
#[path = "fixtures/cel_native.rs"]
mod fixtures;
use reel_assets::cel_source::{editor, insert, project::Project, sequence::Sequence};
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
    fn visit(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                visit(root, &p, out)
            } else {
                out.insert(p.strip_prefix(root).unwrap().into(), fs::read(p).unwrap());
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}
fn request(input: &Path) -> Value {
    let project = Project::open(input).unwrap();
    let (docs, hashes) = editor::load(&project).unwrap();
    let sequence = Sequence::for_project(&docs, &project).unwrap();
    let first = sequence.rows()[0].clone();
    let id = first["semantic_id"].as_str().unwrap();
    let cell = &sequence.cells[id];
    let shared = docs["scene.json"]["shared_events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["semantic_id"] == id)
        .unwrap();
    let mut r = json!({"schema":"reel.cel-sorter-insert.v1","scene_id":docs["scene.json"]["scene_id"],"language":sequence.language,"expected_inputs":hashes,"after_semantic_id":id,
      "shared_event":{"semantic_id":"new-cel","canonical_cue_id":shared["canonical_cue_id"],"picture_binding":"two","score":{"disposition":"silence"},"language_ids":[sequence.language]},
      "language_event":{"semantic_id":"new-cel","event_id":"new-event","cue_id":cell.cue_id,"semantic_trigger_id":"new-trigger","picture_slot_id":"new-slot"},"notes":"Explicit producer operation; not approval","human_approval":false});
    if let Some(lock) = project.descriptor_sha256() {
        r["expected_project"] = json!(lock);
    }
    if project.is_multi_cue() {
        r["split_after_samples"] = json!(7);
    } else {
        r["split_after_frames"] = json!(12);
    }
    r
}
fn normalized(input: &Path, output: &Path, request_path: &Path) -> PathBuf {
    let p = Project::open(input).unwrap();
    let (docs, hashes) = editor::load(&p).unwrap();
    let sequence = Sequence::for_project(&docs, &p).unwrap();
    let mut r = json!({"schema":p.session_schema().replace("session","edit"),"scene_id":docs["scene.json"]["scene_id"],"expected_inputs":hashes,"rows":sequence.rows()});
    if let Some(hash) = p.descriptor_sha256() {
        r["expected_project"] = json!(hash);
    }
    if let Some(lang) = p.language() {
        r["language"] = json!(lang);
    }
    put(request_path, &r);
    editor::apply(input, request_path, output).unwrap();
    output.join("cel-sorter-project.json")
}

#[test]
fn legacy_split_preserves_source_revisions_and_full_clock_and_requires_new_editor_locks() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fixtures::fixture(&source);
    let before = snapshot(&source);
    let r = request(&source);
    let req = temp.path().join("insert.json");
    put(&req, &r);
    let out = temp.path().join("new-source");
    insert::apply(&source, &req, &out).unwrap();
    assert_eq!(snapshot(&source), before);
    let p = Project::open(&out).unwrap();
    let (docs, hashes) = editor::load(&p).unwrap();
    let sequence = Sequence::for_project(&docs, &p).unwrap();
    assert_eq!(
        sequence.rows(),
        json!([{"semantic_id":"a","picture_binding":"one","frames":12},{"semantic_id":"new-cel","picture_binding":"two","frames":12},{"semantic_id":"b","picture_binding":"two","frames":24}])
    );
    assert_eq!(
        docs["alignment.json"]["semantic_markers"],
        json!({"a":0,"new-trigger":24,"b":48})
    );
    assert_eq!(
        docs["scene-bindings.json"]["assets"]["clock"]["selection_state"],
        "selected-private-production"
    );
    assert_eq!(
        docs["scene.json"]["languages"],
        get(&source.join("scene.json"))["languages"]
    );
    let original_graph = get(&source.join("selected-graph.json"));
    assert_eq!(
        &docs["selected-graph.json"]["slots"].as_array().unwrap()[..2],
        original_graph["slots"].as_array().unwrap()
    );
    assert_eq!(
        docs["selected-graph.json"]["slots"][2]["revisions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let repeat = temp.path().join("same-candidate");
    insert::apply(&source, &req, &repeat).unwrap();
    assert_eq!(
        get(&repeat.join("selected-graph.json")),
        docs["selected-graph.json"]
    );
    assert!(insert::apply(&out, &req, &temp.path().join("stale-insert")).is_err());
    assert!(!temp.path().join("stale-insert").exists());
    let edit = temp.path().join("old-edit.json");
    put(
        &edit,
        &json!({"schema":"reel.cel-sorter-edit.v1","scene_id":"synthetic","expected_inputs":r["expected_inputs"],"rows":[{"semantic_id":"a","picture_binding":"one","frames":24},{"semantic_id":"b","picture_binding":"two","frames":24}]}),
    );
    assert!(editor::apply(&out, &edit, &temp.path().join("old-session")).is_err());
    assert!(!temp.path().join("old-session").exists());
    put(
        &edit,
        &json!({"schema":"reel.cel-sorter-edit.v1","scene_id":"synthetic","expected_inputs":hashes,"rows":sequence.rows()}),
    );
    editor::apply(&out, &edit, &temp.path().join("fresh-edit")).unwrap();
    let retained = snapshot(&out);
    assert!(insert::apply(&source, &req, &out).is_err());
    assert_eq!(snapshot(&out), retained);
}

#[test]
fn invalid_insertion_requests_reject_before_writing_or_modifying_sources() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fixtures::fixture(&source);
    let before = snapshot(&source);
    let original = request(&source);
    let req = temp.path().join("insert.json");
    for case in 0..23 {
        let mut r = original.clone();
        match case {
            0 => r["expected_inputs"]["scene.json"] = json!("0".repeat(64)),
            1 => r["language"] = json!("en"),
            2 => r["expected_project"] = json!("0".repeat(64)),
            3 => r["human_approval"] = json!(true),
            4 => r["split_after_frames"] = json!(0),
            5 => r["split_after_frames"] = json!(24),
            6 => r["split_after_frames"] = json!(u64::MAX),
            7 => r["split_after_samples"] = json!(7),
            8 => {
                r.as_object_mut().unwrap().remove("split_after_frames");
            }
            9 => {
                r["shared_event"]["semantic_id"] = json!("a");
                r["language_event"]["semantic_id"] = json!("a");
            }
            10 => r["language_event"]["event_id"] = json!("event-a"),
            11 => r["language_event"]["picture_slot_id"] = json!("p1"),
            12 => r["language_event"]["semantic_trigger_id"] = json!("b"),
            13 => r["shared_event"]["picture_binding"] = json!("take"),
            14 => r["shared_event"]["canonical_cue_id"] = json!("other"),
            15 => r["language_event"]["cue_id"] = json!("other"),
            16 => r["shared_event"]["language_ids"] = json!(["es", "en"]),
            17 => r["language_event"]["supersedes_event_id"] = json!("event-a"),
            18 => r["shared_event"]["unknown"] = json!(true),
            19 => r["unknown"] = json!(true),
            20 => r["after_semantic_id"] = json!("missing"),
            21 => r["language_event"]["semantic_trigger_id"] = json!(""),
            _ => r["language_event"]["picture_binding_override"] = json!("unadmitted"),
        }
        put(&req, &r);
        let out = temp.path().join(format!("reject-{case}"));
        assert!(insert::apply(&source, &req, &out).is_err(), "case {case}");
        assert!(!out.exists(), "case {case}");
        assert_eq!(snapshot(&source), before);
    }
    let r = serde_json::to_string(&original).unwrap();
    fs::write(
        &req,
        r.replace(
            "\"expected_inputs\":{",
            "\"expected_inputs\":{\"scene.json\":\"duplicate\",",
        ),
    )
    .unwrap();
    assert!(insert::apply(&source, &req, &temp.path().join("duplicate-lock")).is_err());
}

#[test]
fn scoped_sample_split_preserves_english_and_separate_graph_files_and_blocks_other_lane_ids() {
    for separate in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let intake = fixtures::multi_fixture(&temp.path().join("intake"));
        let descriptor = normalized(&intake, &source, &temp.path().join("normalize.json"));
        fixtures::add_english_lane(&source, &descriptor);
        if separate {
            fixtures::separate_language_graphs(&source, &descriptor);
        }
        let before = snapshot(&source);
        let r = request(&descriptor);
        let req = temp.path().join("insert.json");
        put(&req, &r);
        let out = temp.path().join("new-source");
        insert::apply(&descriptor, &req, &out).unwrap();
        assert_eq!(snapshot(&source), before);
        let project = Project::open(&out.join("cel-sorter-project.json")).unwrap();
        let (docs, _) = editor::load(&project).unwrap();
        let sequence = Sequence::for_project(&docs, &project).unwrap();
        let rows = sequence.rows();
        assert_eq!(rows[0]["samples"], 7);
        assert_eq!(rows[1]["semantic_id"], "new-cel");
        assert_eq!(rows[1]["samples"], 40);
        assert_eq!(sequence.session_clock()["total_seconds"], 4.0);
        assert_eq!(
            docs["scene.json"]["language_event_bindings"]["en"],
            get(&source.join("scene.json"))["language_event_bindings"]["en"]
        );
        for (file, bytes) in &before {
            if file.to_string_lossy().contains("english")
                || file.to_string_lossy().contains("en-graph")
                || file.to_string_lossy().contains("en-pointer")
            {
                assert_eq!(fs::read(out.join(file)).unwrap(), *bytes);
            }
        }
        let mut collision = r.clone();
        collision["language_event"]["event_id"] =
            get(&source.join("scene.json"))["language_event_bindings"]["en"][0]["event_id"].clone();
        put(&req, &collision);
        assert!(insert::apply(&descriptor, &req, &temp.path().join("other-lane-id")).is_err());
        assert!(!temp.path().join("other-lane-id").exists());
        collision = r.clone();
        collision["language_event"]["semantic_trigger_id"] = json!("unrelated");
        put(&req, &collision);
        assert!(insert::apply(&descriptor, &req, &temp.path().join("unrelated-marker")).is_err());
        put(&req, &r);
        fs::write(
            &descriptor,
            format!("{}\n", fs::read_to_string(&descriptor).unwrap()),
        )
        .unwrap();
        assert!(insert::apply(&descriptor, &req, &temp.path().join("stale-descriptor")).is_err());
        assert!(!temp.path().join("stale-descriptor").exists());
    }
}

#[test]
fn actual_cli_creates_new_native_source_then_exports_a_fresh_controls_session() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fixtures::fixture(&source);
    let req = temp.path().join("insert.json");
    put(&req, &request(&source));
    let out = temp.path().join("candidate");
    let tool = env!("CARGO_BIN_EXE_reel-cel-sorter");
    let result = std::process::Command::new(tool)
        .arg("insert-cel")
        .arg(&source)
        .arg(&req)
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let spec = temp.path().join("export.json");
    let export = temp.path().join("fresh-editor");
    put(&spec, &json!({"project":out,"output":export}));
    assert!(
        std::process::Command::new(tool)
            .arg("export-controls")
            .arg(spec)
            .output()
            .unwrap()
            .status
            .success()
    );
    let session = get(&export.join("session.json"));
    assert_eq!(session["rows"].as_array().unwrap().len(), 3);
    assert!(
        session["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["semantic_id"] == "new-cel")
    );
    assert_eq!(session["media_mode"], "controls-only");
    assert!(!export.join("media").exists());
}

#[test]
fn stale_source_graph_clocks_are_not_silently_repaired_during_insertion() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    fixtures::fixture(&source);
    let mut graph = get(&source.join("selected-graph.json"));
    graph["events"][0]["phrase_end_seconds"] = json!(0.5);
    fixtures::save_graph(&source, graph);
    let before = snapshot(&source);
    let req = temp.path().join("insert.json");
    put(&req, &request(&source));
    let out = temp.path().join("candidate");
    let error = insert::apply(&source, &req, &out).unwrap_err().to_string();
    assert!(error.contains("Source graph clock differs"), "{error}");
    assert!(!out.exists());
    assert_eq!(snapshot(&source), before);
}

#[test]
fn explicitly_listed_picture_candidate_and_native_override_are_supported_without_media_lookup() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let intake = fixtures::multi_fixture(&temp.path().join("intake"));
    let descriptor = normalized(&intake, &source, &temp.path().join("normalize.json"));
    let mut bindings = get(&source.join("scene-bindings.json"));
    bindings["assets"]["candidate"] = fixtures::asset(b"new-primary-picture");
    bindings["assets"]["not-listed"] = fixtures::asset(b"not-admitted");
    put(&source.join("scene-bindings.json"), &bindings);
    let mut config = get(&descriptor);
    config["picture_candidates"] = json!(["candidate"]);
    put(&descriptor, &config);
    let mut r = request(&descriptor);
    r["shared_event"]["picture_binding"] = json!("one");
    r["language_event"]["picture_binding_override"] = json!("candidate");
    let req = temp.path().join("insert.json");
    put(&req, &r);
    let out = temp.path().join("candidate-source");
    insert::apply(&descriptor, &req, &out).unwrap();
    let p = Project::open(&out.join("cel-sorter-project.json")).unwrap();
    let (docs, _) = editor::load(&p).unwrap();
    assert_eq!(
        Sequence::for_project(&docs, &p).unwrap().rows()[1]["picture_binding"],
        "candidate"
    );
    assert_eq!(
        docs["scene.json"]["shared_events"][1]["picture_binding"],
        "one"
    );
    assert!(!out.join("media").exists());
    r["language_event"]["picture_binding_override"] = json!("not-listed");
    put(&req, &r);
    let reject = temp.path().join("unadmitted");
    assert!(insert::apply(&descriptor, &req, &reject).is_err());
    assert!(!reject.exists());
}

#[test]
fn browser_proposal_from_exported_native_session_applies_and_requires_a_fresh_session() {
    let tool = env!("CARGO_BIN_EXE_reel-cel-sorter");
    for mode in 0..4 {
        let temp = tempfile::tempdir().unwrap();
        let intake = temp.path().join("intake");
        let input = if mode == 0 {
            fixtures::fixture(&intake);
            intake.clone()
        } else {
            let descriptor = fixtures::multi_fixture(&intake);
            if mode >= 2 {
                fixtures::add_english_lane(&intake, &descriptor);
            }
            if mode == 3 {
                fixtures::separate_language_graphs(&intake, &descriptor);
            }
            normalized(
                &descriptor,
                &temp.path().join("source"),
                &temp.path().join("normalize.json"),
            )
        };
        let source_root = if input.is_dir() {
            input.as_path()
        } else {
            input.parent().unwrap()
        };
        add_score_palette(
            source_root,
            [
                "scene-bindings.json",
                "episode-bindings.json",
                "season-bindings.json",
            ][mode % 3],
        );
        if !input.is_dir() {
            let mut config = get(&input);
            for role in [
                "episode.json",
                "episode-bindings.json",
                "season-bindings.json",
            ] {
                config["files"][role] = json!(role);
            }
            put(&input, &config);
        }
        let before = snapshot(&intake);
        let export = temp.path().join("editor");
        let spec = temp.path().join("export.json");
        put(&spec, &json!({"project":input,"output":export}));
        let run = |args: &[&std::ffi::OsStr]| {
            let result = std::process::Command::new(tool)
                .args(args)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        };
        run(&["export-controls".as_ref(), spec.as_os_str()]);
        let session_path = export.join("session.json");
        let session = get(&session_path);
        assert_eq!(
            session["insertion"]["score_palette"]["choices"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            session["insertion"]["score_palette"]["held_roles"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let request_path = temp.path().join("browser-insert.json");
        let result = std::process::Command::new("node")
            .arg("-e")
            .arg("const fs=require('fs'),ui=require(process.argv[1]),s=JSON.parse(fs.readFileSync(process.argv[2],'utf8'));const first=s.rows[0],sample=first.samples!==undefined;const r=ui.insertion(s,s.rows,'',{after:first.semantic_id,split:Math.floor(first[sample?'samples':'frames']/2),picture:s.picture_choices.at(-1),id:'browser-new',score:'role:theme'});fs.writeFileSync(process.argv[3],JSON.stringify(r));")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cel_sorter.js"))
            .arg(&session_path).arg(&request_path).output().expect("Node is required for browser/native contract integration");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let out = temp.path().join("new-source");
        run(&[
            "insert-cel".as_ref(),
            input.as_os_str(),
            request_path.as_os_str(),
            out.as_os_str(),
        ]);
        let new_input = if mode == 0 {
            out.clone()
        } else {
            out.join("cel-sorter-project.json")
        };
        let fresh = temp.path().join("fresh-editor");
        put(&spec, &json!({"project":new_input,"output":fresh}));
        run(&["export-controls".as_ref(), spec.as_os_str()]);
        let current = get(&fresh.join("session.json"));
        assert_eq!(
            current["rows"].as_array().unwrap().len(),
            session["rows"].as_array().unwrap().len() + 1
        );
        assert_ne!(current["expected_inputs"], session["expected_inputs"]);
        let stale = std::process::Command::new(tool)
            .arg("insert-cel")
            .arg(&new_input)
            .arg(&request_path)
            .arg(temp.path().join("stale"))
            .output()
            .unwrap();
        assert!(!stale.status.success());
        assert!(!temp.path().join("stale").exists());
        assert_eq!(snapshot(&intake), before);
        assert!(!export.join("media").exists());
        assert!(!fresh.join("media").exists());
    }
}

fn add_score_palette(root: &Path, scope: &str) {
    let mut episode = json!({"schema":"reel.episode-authoring.v1","episode_id":"synthetic-episode","season_id":"synthetic-season","authoring_state":"ready-for-private-build","master_template_id":"synthetic","scene_policy_id":"synthetic","presentation":[],"scene_ids":["synthetic"]});
    episode["episode_id"] = json!("synthetic-episode");
    episode["season_id"] = json!("synthetic-season");
    episode["score_palette"] = json!([
        {"role":"theme","theme_id":"theme-id","source_poem_id":"poem-id","arrangement_id":"arrangement-id","asset_binding":"score.theme"},
        {"role":"unavailable","theme_id":"held-theme","source_poem_id":"poem-id","arrangement_id":"arrangement-id","asset_binding":"missing-score"}
    ]);
    put(&root.join("episode.json"), &episode);
    for (name, id) in [
        ("episode-bindings.json", "synthetic-episode"),
        ("season-bindings.json", "synthetic-season"),
    ] {
        put(
            &root.join(name),
            &json!({"schema":"reel.scene-asset-bindings.v1","scope_id":id,"assets":{}}),
        );
    }
    let mut bindings = get(&root.join(scope));
    bindings["assets"]["score.theme"] = fixtures::asset(b"synthetic-score-primary");
    put(&root.join(scope), &bindings);
}

#[test]
fn selected_native_score_roles_are_explicit_and_unavailable_roles_cannot_be_forged() {
    for scope in [
        "scene-bindings.json",
        "episode-bindings.json",
        "season-bindings.json",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fixtures::fixture(&source);
        add_score_palette(&source, scope);
        let before = snapshot(&source);
        let p = Project::open(&source).unwrap();
        let (docs, _) = editor::load(&p).unwrap();
        let choices = reel_assets::cel_source::score::choices(&docs).unwrap();
        assert_eq!(choices.choices.len(), 1);
        assert_eq!(choices.choices[0].role, "theme");
        assert_eq!(choices.held_roles.len(), 1);
        let mut r = request(&source);
        r["shared_event"]["score"] = json!({"disposition":"role","role":"theme"});
        let req = temp.path().join("request.json");
        put(&req, &r);
        let out = temp.path().join("out");
        insert::apply(&source, &req, &out).unwrap();
        assert_eq!(
            get(&out.join("scene.json"))["shared_events"][1]["score"],
            r["shared_event"]["score"]
        );
        assert_eq!(snapshot(&source), before);
        for role in ["unavailable", "invented"] {
            r["shared_event"]["score"]["role"] = json!(role);
            put(&req, &r);
            let bad = temp.path().join(role);
            assert!(insert::apply(&source, &req, &bad).is_err());
            assert!(!bad.exists());
        }
    }
}

#[test]
fn score_admission_rejects_unselected_ambiguous_invalid_and_stale_bindings_before_output() {
    for mode in 0..10 {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fixtures::fixture(&source);
        add_score_palette(&source, "episode-bindings.json");
        let mut bindings = get(&source.join("episode-bindings.json"));
        match mode {
            0 => bindings["assets"]["score.theme"]["selection_state"] = json!("held"),
            1 => {
                bindings["assets"]
                    .as_object_mut()
                    .unwrap()
                    .remove("score.theme");
            }
            2 => {
                let mut duplicate = get(&source.join("season-bindings.json"));
                duplicate["assets"]["score.theme"] = bindings["assets"]["score.theme"].clone();
                put(&source.join("season-bindings.json"), &duplicate);
            }
            3 => bindings["assets"]["score.theme"]["cache_uri"] = json!("cache://sha256/wrong"),
            4 => bindings["assets"]["score.theme"]["bytes"] = json!(0),
            5 => bindings["scope_id"] = json!("wrong-episode"),
            6 => bindings["assets"]["score.theme"]["logical_id"] = json!("x".repeat(257)),
            7 => {
                let mut episode = get(&source.join("episode.json"));
                let role = episode["score_palette"][0].clone();
                episode["score_palette"].as_array_mut().unwrap().push(role);
                put(&source.join("episode.json"), &episode);
            }
            9 => {
                let mut episode = get(&source.join("episode.json"));
                episode["scene_ids"] = json!(["different-scene"]);
                put(&source.join("episode.json"), &episode);
            }
            _ => {}
        }
        put(&source.join("episode-bindings.json"), &bindings);
        let mut r = request(&source);
        r["shared_event"]["score"] = json!({"disposition":"role","role":"theme"});
        if mode == 8 {
            bindings["assets"]["score.theme"] = fixtures::asset(b"changed-selected-score");
            put(&source.join("episode-bindings.json"), &bindings);
        }
        let before = snapshot(&source);
        let req = temp.path().join("request.json");
        put(&req, &r);
        let out = temp.path().join("out");
        assert!(insert::apply(&source, &req, &out).is_err(), "mode {mode}");
        assert!(!out.exists());
        if [5, 7, 9].contains(&mode) {
            let export = temp.path().join("rejected-export");
            let spec = temp.path().join("export.json");
            put(&spec, &json!({"project":source,"output":export}));
            let result = std::process::Command::new(env!("CARGO_BIN_EXE_reel-cel-sorter"))
                .arg("export-controls")
                .arg(&spec)
                .output()
                .unwrap();
            assert!(!result.status.success());
            assert!(!export.exists());
        }
        assert_eq!(snapshot(&source), before);
    }
}
