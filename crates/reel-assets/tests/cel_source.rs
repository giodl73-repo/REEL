use reel_assets::cel_source;
use serde_json::{Value, json};
use std::{fs, process::Command};

fn fixture(v2: bool) -> (Value, Value) {
    let mut scene: Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/scene-authoring/scene.json"
    ))
    .unwrap();
    let mut bindings: Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/scene-authoring/scene-bindings.json"
    ))
    .unwrap();
    let mut alternate = bindings["assets"]["picture.first-line"].clone();
    alternate["logical_id"] = json!("alternate");
    alternate["sha256"] = json!("8".repeat(64));
    alternate["cache_uri"] = json!(format!("cache://sha256/{}", "8".repeat(64)));
    bindings["assets"]["picture.alternate"] = alternate;
    for lang in ["es", "en"] {
        let mut cue = scene["languages"][lang]["cues"][0].clone();
        cue["cue_id"] = json!(format!("{lang}-2"));
        cue["source_id"] = json!("source-block-2");
        scene["languages"][lang]["cues"]
            .as_array_mut()
            .unwrap()
            .push(cue);
        let mut event = scene["languages"][lang]["events"][0].clone();
        event["event_id"] = json!(format!("{lang}-event-2"));
        event["cue_id"] = json!(format!("{lang}-2"));
        // Reusing the same selected picture slot is legal and retains event identity.
        scene["languages"][lang]["events"]
            .as_array_mut()
            .unwrap()
            .push(event);
    }
    scene["source_scope_ids"] = json!(["source-block-1", "source-block-2"]);
    if v2 {
        scene["schema"] = json!("reel.scene-authoring.v2");
        scene["canonical_cue_ids"] = scene["source_scope_ids"].clone();
        scene["shared_events"] = json!([
            {"semantic_id":"first", "canonical_cue_id":"source-block-1", "picture_binding":"picture.first-line", "score":{"disposition":"silence"}},
            {"semantic_id":"second", "canonical_cue_id":"source-block-2", "picture_binding":"picture.first-line", "score":{"disposition":"silence"}}
        ]);
        for lang in ["es", "en"] {
            scene["languages"][lang]["events"] = json!([]);
            scene["language_event_bindings"][lang] = json!([
                {"semantic_id":"first", "event_id":format!("{lang}-event-1"), "cue_id":format!("{lang}-1"), "semantic_trigger_id":"first-line", "picture_slot_id":"slot-1"},
                {"semantic_id":"second", "event_id":format!("{lang}-event-2"), "cue_id":format!("{lang}-2"), "semantic_trigger_id":"first-line", "picture_slot_id":"slot-2"}
            ]);
        }
        scene["language_event_bindings"]["en"][0]["picture_binding_override"] =
            json!("picture.alternate");
    }
    (scene, bindings)
}
fn inspect(scene: &Value, bindings: &Value) -> reel_assets::Result<cel_source::Report> {
    cel_source::inspect(&serde_json::to_vec(scene)?, &serde_json::to_vec(bindings)?)
}

#[test]
fn v1_bilingual_multicue_discovery_needs_no_movie_or_cache() {
    let (scene, bindings) = fixture(false);
    let report = inspect(&scene, &bindings).unwrap();
    assert_eq!(report.languages, ["en", "es"]);
    assert_eq!(report.cels.len(), 4);
    assert_eq!(report.cels[0].event_id, "en-event-1");
    assert_eq!(report.cels[1].cue_id, "en-2");
    assert_eq!(
        report.cels[0].picture_slot_id,
        report.cels[1].picture_slot_id
    );
    assert_eq!(report.summary()["render_ready"], false);
}

#[test]
fn v2_uses_native_materialization_and_language_override_without_mutation() {
    let (scene, bindings) = fixture(true);
    let original = (scene.clone(), bindings.clone());
    let report = inspect(&scene, &bindings).unwrap();
    assert_eq!(report.cels.len(), 4);
    assert_eq!(report.cels[0].picture_binding, "picture.alternate");
    assert_eq!(report.cels[0].shared_semantic_id.as_deref(), Some("first"));
    assert_eq!(report.cels[2].picture_binding, "picture.first-line");
    assert_eq!(original, (scene.clone(), bindings.clone()));
    assert_eq!(
        inspect(&scene, &bindings).unwrap().scene_sha256,
        report.scene_sha256
    );
}

#[test]
fn scope_unknown_fields_unsupported_schema_and_unbound_assets_reject() {
    let (scene, bindings) = fixture(true);
    for (mut s, mut b, case) in (0..5).map(|case| (scene.clone(), bindings.clone(), case)) {
        match case {
            0 => b["scope_id"] = json!("other-scene"),
            1 => s["schema"] = json!("future"),
            2 => s["unknown"] = json!("do not silently discard"),
            3 => {
                b["assets"]
                    .as_object_mut()
                    .unwrap()
                    .remove("picture.alternate");
            }
            _ => b["assets"]["picture.first-line"]["cache_uri"] = json!("file:///unexpected"),
        }
        assert!(inspect(&s, &b).is_err(), "case {case}");
    }
}

#[test]
fn duplicate_events_unknown_cues_and_incomplete_v2_lanes_reject() {
    for v2 in [false, true] {
        let (scene, bindings) = fixture(v2);
        for case in 0..3 {
            let mut s = scene.clone();
            if v2 {
                match case {
                    0 => {
                        s["language_event_bindings"]["es"][1]["event_id"] =
                            s["language_event_bindings"]["es"][0]["event_id"].clone()
                    }
                    1 => s["language_event_bindings"]["es"][0]["cue_id"] = json!("foreign"),
                    _ => {
                        s["language_event_bindings"]
                            .as_object_mut()
                            .unwrap()
                            .remove("en");
                    }
                }
            } else {
                match case {
                    0 => {
                        s["languages"]["es"]["events"][1]["event_id"] =
                            s["languages"]["es"]["events"][0]["event_id"].clone()
                    }
                    1 => s["languages"]["es"]["events"][0]["cue_id"] = json!("foreign"),
                    _ => s["languages"]["es"]["events"][0]["picture_slot_id"] = json!(""),
                }
            }
            assert!(inspect(&s, &bindings).is_err(), "v2={v2}, case={case}");
        }
    }
}

#[test]
fn imported_evidence_stays_explicit_and_input_byte_locks_change() {
    let (mut scene, bindings) = fixture(false);
    scene["authoring_state"] = json!("imported-evidence");
    let a = serde_json::to_vec(&scene).unwrap();
    let mut b = a.clone();
    b.push(b'\n');
    let bindings = serde_json::to_vec(&bindings).unwrap();
    let one = cel_source::inspect(&a, &bindings).unwrap();
    let two = cel_source::inspect(&b, &bindings).unwrap();
    assert!(one.imported_evidence);
    assert_ne!(one.scene_sha256, two.scene_sha256);
    assert_eq!(one.bindings_sha256, two.bindings_sha256);
    assert!(cel_source::inspect(&vec![b' '; 1024 * 1024 + 1], &bindings).is_err());
}

#[test]
fn cli_writes_private_report_once_and_only_counts_on_stdout() {
    let temp = tempfile::tempdir().unwrap();
    let (scene, bindings) = fixture(true);
    let source = temp.path().join("scene.json");
    let assets = temp.path().join("bindings.json");
    let output = temp.path().join("report.json");
    fs::write(&source, serde_json::to_vec(&scene).unwrap()).unwrap();
    fs::write(&assets, serde_json::to_vec(&bindings).unwrap()).unwrap();
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_reel-assets"))
            .arg("--root")
            .arg(temp.path())
            .arg("cel-inspect")
            .arg(&source)
            .arg(&assets)
            .arg(&output)
            .output()
            .unwrap()
    };
    let result = invoke();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8(result.stdout).unwrap();
    let summary: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(summary["cels"], 4);
    for secret in [
        "scene-poem",
        "alternate",
        "picture",
        "source-block",
        "report.json",
    ] {
        assert!(!stdout.contains(secret));
    }
    let original = fs::read(&output).unwrap();
    assert!(!invoke().status.success());
    assert_eq!(fs::read(&output).unwrap(), original);
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 3);
}
