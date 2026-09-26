use std::{fs, process::Command};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/scene-authoring/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn write(path: &std::path::Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

#[test]
fn compiles_selected_poem_from_scene_content_and_rejects_changed_definition() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path();
    let mut catalog = fixture("catalog");
    let mut scene = fixture("scene");
    scene["presentation_source_scope_ids"] = json!(["poem-title", "poet-credit"]);
    let mut scene_bindings = fixture("scene-bindings");
    let definition = json!({
        "schema":"reel.editable-text-template.v1", "template_id":"poem-v1",
        "kind":"opening-poem", "canvas_width":1280, "canvas_height":720,
        "font_name":"Monotype Corsiva", "title_size":42, "body_size":31,
        "title_x":880, "title_y":32, "body_x":880, "body_y":94,
        "line_spacing":42, "future_rgb":[120,127,133],
        "active_rgb":[216,227,232], "completed_rgb":[255,255,255],
        "byline":{"x":880,"y":660,"font_size":21,"rgb":[255,255,255]},
        "panel":{"x":853,"width":427,"background_rgb":[20,18,16],"divider_rgb":[211,178,107],"divider_alpha":185},
        "fixed_duration_seconds":null
    });
    let definition_bytes = serde_json::to_vec_pretty(&definition).unwrap();
    fs::write(dir.join("definition.json"), &definition_bytes).unwrap();
    catalog["templates"][2]["definition_sha256"] = sha(&definition_bytes).into();
    scene["presentation"]["content"]["titles"] = json!({"es":"Recuerdos","en":"Memories"});
    scene["presentation"]["content"]["bylines"] =
        json!({"es":"por Andrés Alarcón García","en":"by Andrés Alarcón García"});
    scene["presentation"]["content"]["lines_by_language"] = json!({
        "es":[{"text":"Línea original","cue_id":"es-1","semantic_trigger_id":"first-line"}],
        "en":[{"text":"Draft line","cue_id":"en-1","semantic_trigger_id":"first-line"}]
    });
    scene["presentation"]["content"]["source_text_bindings"] =
        json!({"es":"es.poem.source","en":"en.poem.source"});
    let source_text = json!({
        "schema":"reel.presentation-source-text.v1",
        "source_authority_id":"source-1",
        "source_document_sha256":"a".repeat(64),
        "source_scope_ids":["source-block-1","poem-title","poet-credit"],
        "language":"es", "text_state":"canonical-original",
        "title":"Recuerdos", "byline":"por Andrés Alarcón García", "chapter_number":null,
        "lines":[{"text":"Línea original","cue_id":"es-1","stanza_break_before":false}]
    });
    let source_bytes = serde_json::to_vec_pretty(&source_text).unwrap();
    fs::write(dir.join("source-text.json"), &source_bytes).unwrap();
    let source_sha = sha(&source_bytes);
    scene_bindings["assets"]["es.poem.source"] = json!({
        "logical_id":"es-poem-source", "sha256":source_sha,
        "bytes":source_bytes.len(), "cache_uri":format!("cache://sha256/{source_sha}"),
        "selection_state":"selected-private-production"
    });
    let alignment = json!({
        "schema":"reel.scene-native-alignment.v1", "language":"es", "cue_id":"es-1",
        "selected_take_sha256":"3".repeat(64), "sample_rate":24000,
        "cue_end_sample":24000, "semantic_markers":{"first-line":0}
    });
    let alignment_bytes = serde_json::to_vec_pretty(&alignment).unwrap();
    fs::write(dir.join("es-alignment.json"), &alignment_bytes).unwrap();
    let alignment_binding = &mut scene_bindings["assets"]["es.alignment"];
    alignment_binding["sha256"] = sha(&alignment_bytes).into();
    alignment_binding["bytes"] = (alignment_bytes.len() as u64).into();
    alignment_binding["cache_uri"] = format!("cache://sha256/{}", sha(&alignment_bytes)).into();
    write(&dir.join("catalog.json"), &catalog);
    write(&dir.join("episode-authoring.json"), &fixture("episode"));
    write(&dir.join("scene.json"), &scene);
    write(&dir.join("season.json"), &fixture("season-bindings"));
    write(
        &dir.join("episode-bindings.json"),
        &fixture("episode-bindings"),
    );
    write(&dir.join("bindings.json"), &scene_bindings);
    write(
        &dir.join("paths.json"),
        &json!({"es-1":"es-alignment.json"}),
    );
    let exe = env!("CARGO_BIN_EXE_reel-scene-template");
    let run = |ass: &str, receipt: &str| {
        Command::new(exe)
            .arg("compile")
            .arg(dir.join("catalog.json"))
            .arg(dir.join("definition.json"))
            .arg(dir.join("episode-authoring.json"))
            .arg(dir.join("scene.json"))
            .arg("es")
            .arg(dir.join("season.json"))
            .arg(dir.join("episode-bindings.json"))
            .arg(dir.join("bindings.json"))
            .arg(dir.join("paths.json"))
            .arg(dir.join("source-text.json"))
            .arg("--output-ass")
            .arg(dir.join(ass))
            .arg("--receipt")
            .arg(dir.join(receipt))
            .output()
            .unwrap()
    };
    let good = run("poem.ass", "receipt.json");
    assert!(
        good.status.success(),
        "{}",
        String::from_utf8_lossy(&good.stderr)
    );
    let ass = fs::read_to_string(dir.join("poem.ass")).unwrap();
    assert!(ass.contains("Línea original"));
    assert!(ass.contains("por Andrés Alarcón García"));
    assert!(ass.contains("0:00:01.00"));
    let receipt: Value =
        serde_json::from_slice(&fs::read(dir.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["ass_sha256"], sha(ass.as_bytes()));
    assert_eq!(receipt["source_text_state"], "canonical-original");
    scene["schema"] = json!("reel.scene-authoring.v2");
    scene.as_object_mut().unwrap().remove("source_scope_ids");
    scene["canonical_cue_ids"] = json!(["source-block-1"]);
    scene["shared_events"] = json!([{"semantic_id":"poem-image","canonical_cue_id":"source-block-1","picture_binding":"picture.first-line","score":{"disposition":"role","role":"poem.intimate"},"sonic_bindings":[],"vfx_bindings":[]}]);
    scene["language_event_bindings"] = json!({"es":[{"semantic_id":"poem-image","event_id":"es-event-1","cue_id":"es-1","semantic_trigger_id":"first-line","picture_slot_id":"es-picture-slot"}],"en":[{"semantic_id":"poem-image","event_id":"en-event-1","cue_id":"en-1","semantic_trigger_id":"first-line","picture_slot_id":"en-picture-slot"}]});
    scene["languages"]["es"]
        .as_object_mut()
        .unwrap()
        .remove("events");
    scene["languages"]["en"]
        .as_object_mut()
        .unwrap()
        .remove("events");
    scene["presentation"]["content"]
        .as_object_mut()
        .unwrap()
        .remove("line_cue_ids");
    catalog["templates"][2]["required_content_keys"] = json!(["poem_id", "lines_by_language"]);
    write(&dir.join("scene.json"), &scene);
    write(&dir.join("catalog.json"), &catalog);
    let v2 = run("poem-v2.ass", "receipt-v2.json");
    assert!(
        v2.status.success(),
        "{}",
        String::from_utf8_lossy(&v2.stderr)
    );
    assert_eq!(
        fs::read(dir.join("poem.ass")).unwrap(),
        fs::read(dir.join("poem-v2.ass")).unwrap()
    );
    let mut wrong_season = fixture("season-bindings");
    wrong_season["scope_id"] = "another-season".into();
    write(&dir.join("season.json"), &wrong_season);
    let rejected = run("wrong-season.ass", "wrong-season-receipt.json");
    assert!(!rejected.status.success());
    write(&dir.join("season.json"), &fixture("season-bindings"));
    fs::write(dir.join("definition.json"), b"{}").unwrap();
    let stale = run("stale.ass", "stale-receipt.json");
    assert!(!stale.status.success());
    assert!(!dir.join("stale.ass").exists());
}
