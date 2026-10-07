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
    // A title-first prelude has its own display clock and never borrows the
    // first line's narration. Its selected poem score remains mandatory.
    let saved_catalog = catalog.clone();
    let saved_scene = scene.clone();
    let saved_bindings = scene_bindings.clone();
    let mut title_definition = definition.clone();
    title_definition["kind"] = json!("poem-title");
    title_definition["fixed_duration_seconds"] = json!(2);
    title_definition["soundtrack_requirement"] = json!("selected-episode-score");
    write(&dir.join("definition.json"), &title_definition);
    catalog["templates"][2]["kind"] = json!("poem-title");
    catalog["templates"][2]["required_content_keys"] = json!(["poem_id", "score_role"]);
    catalog["templates"][2]["soundtrack_requirement"] = json!("selected-episode-score");
    catalog["templates"][2]["definition_sha256"] =
        sha(&fs::read(dir.join("definition.json")).unwrap()).into();
    write(&dir.join("catalog.json"), &catalog);
    scene["source_scope_ids"] = json!([]);
    for language in ["es", "en"] {
        scene["languages"][language]["cues"] = json!([]);
        scene["languages"][language]["events"] = json!([]);
    }
    scene["presentation"]["role"] = json!("poem-title");
    scene["presentation"]["content"]
        .as_object_mut()
        .unwrap()
        .remove("line_cue_ids");
    scene["presentation"]["content"]
        .as_object_mut()
        .unwrap()
        .remove("lines_by_language");
    scene["presentation"]["content"]["score_role"] = json!("poem.intimate");
    let mut title_source = source_text.clone();
    title_source["source_scope_ids"] = json!(["poem-title", "poet-credit"]);
    title_source["lines"] = json!([]);
    write(&dir.join("source-text.json"), &title_source);
    let title_bytes = fs::read(dir.join("source-text.json")).unwrap();
    let title_sha = sha(&title_bytes);
    scene_bindings["assets"]["es.poem.source"] = json!({
        "logical_id":"es-title-source", "sha256":title_sha,
        "bytes":title_bytes.len(), "cache_uri":format!("cache://sha256/{title_sha}"),
        "selection_state":"selected-private-production"
    });
    write(&dir.join("scene.json"), &scene);
    write(&dir.join("bindings.json"), &scene_bindings);
    let title_only = run("title-first.ass", "title-first-receipt.json");
    assert!(
        title_only.status.success(),
        "{}",
        String::from_utf8_lossy(&title_only.stderr)
    );
    let title_ass = fs::read_to_string(dir.join("title-first.ass")).unwrap();
    assert!(title_ass.contains("Recuerdos"));
    assert!(!title_ass.contains("Línea original"));
    assert!(title_ass.contains("0:00:02.00"));
    let mut unavailable_score = fixture("episode-bindings");
    unavailable_score["assets"]["score.poem.intimate"]["selection_state"] = json!("candidate");
    write(&dir.join("episode-bindings.json"), &unavailable_score);
    assert!(
        !run("title-unselected.ass", "title-unselected.json")
            .status
            .success()
    );
    scene["presentation"]["content"]["poem_id"] = json!("unrelated-poem");
    write(&dir.join("scene.json"), &scene);
    write(
        &dir.join("episode-bindings.json"),
        &fixture("episode-bindings"),
    );
    assert!(
        !run("title-unrelated.ass", "title-unrelated.json")
            .status
            .success()
    );
    catalog = saved_catalog;
    scene = saved_scene;
    scene_bindings = saved_bindings;
    write(&dir.join("catalog.json"), &catalog);
    write(&dir.join("scene.json"), &scene);
    write(&dir.join("bindings.json"), &scene_bindings);
    write(&dir.join("source-text.json"), &source_text);
    let mut scored_definition = definition.clone();
    scored_definition["soundtrack_requirement"] = json!("selected-episode-score");
    write(&dir.join("definition.json"), &scored_definition);
    catalog["templates"][2]["definition_sha256"] =
        sha(&fs::read(dir.join("definition.json")).unwrap()).into();
    catalog["templates"][2]["soundtrack_requirement"] = json!("selected-episode-score");
    write(&dir.join("catalog.json"), &catalog);
    let missing_score = run("missing-score.ass", "missing-score-receipt.json");
    assert!(!missing_score.status.success());
    assert!(String::from_utf8_lossy(&missing_score.stderr).contains("score_role"));
    scene["presentation"]["content"]["score_role"] = json!("poem.intimate");
    write(&dir.join("scene.json"), &scene);
    let scored = run("scored.ass", "scored-receipt.json");
    assert!(
        scored.status.success(),
        "{}",
        String::from_utf8_lossy(&scored.stderr)
    );
    let mut unselected_score = fixture("episode-bindings");
    unselected_score["assets"]["score.poem.intimate"]["selection_state"] = json!("candidate");
    write(&dir.join("episode-bindings.json"), &unselected_score);
    let rejected_score = run("unselected-score.ass", "unselected-score-receipt.json");
    assert!(!rejected_score.status.success());
    write(
        &dir.join("episode-bindings.json"),
        &fixture("episode-bindings"),
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
    let mut tail_definition = scored_definition.clone();
    tail_definition["post_poem_title_duration_ms"] = json!(2_000);
    write(&dir.join("definition.json"), &tail_definition);
    catalog["templates"][2]["definition_sha256"] =
        sha(&fs::read(dir.join("definition.json")).unwrap()).into();
    write(&dir.join("catalog.json"), &catalog);
    scene["presentation"]["content"]["titles"]["es"] = json!("de “Recuerdos”");
    write(&dir.join("scene.json"), &scene);
    let mut tail_source = source_text.clone();
    tail_source["schema"] = json!("reel.scene-presentation-source-text.v2");
    tail_source["text_state"] = json!("project-draft-review-held");
    tail_source["title"] = json!("de “Recuerdos”");
    tail_source["display_units"] = json!([
        {"source_scope_id":"poem-title","role":"poem-title",
         "source_text":"nde “Recuerdos”","source_text_sha256":sha("nde “Recuerdos”".as_bytes()),
         "editable_text":"de “Recuerdos”","editable_text_sha256":sha("de “Recuerdos”".as_bytes())},
        {"source_scope_id":"poet-credit","role":"poet-credit",
         "source_text":"Andrés Alarcón García","source_text_sha256":sha("Andrés Alarcón García".as_bytes()),
         "editable_text":"por Andrés Alarcón García","editable_text_sha256":sha("por Andrés Alarcón García".as_bytes())}
    ]);
    write(&dir.join("source-text.json"), &tail_source);
    let tail_bytes = fs::read(dir.join("source-text.json")).unwrap();
    let tail_sha = sha(&tail_bytes);
    scene_bindings["assets"]["es.poem.source"]["sha256"] = tail_sha.clone().into();
    scene_bindings["assets"]["es.poem.source"]["bytes"] = (tail_bytes.len() as u64).into();
    scene_bindings["assets"]["es.poem.source"]["cache_uri"] =
        format!("cache://sha256/{tail_sha}").into();
    write(&dir.join("bindings.json"), &scene_bindings);
    let tail = run("poem-tail.ass", "receipt-tail.json");
    assert!(
        tail.status.success(),
        "{}",
        String::from_utf8_lossy(&tail.stderr)
    );
    let tail_ass = fs::read_to_string(dir.join("poem-tail.ass")).unwrap();
    assert!(tail_ass.lines().any(
        |line| line.starts_with("Dialogue: 0,0:00:01.00,0:00:03.00,Text")
            && line.contains("de “Recuerdos”")
    ));
    assert!(
        !tail_ass
            .lines()
            .any(|line| line.contains("de “Recuerdos”") && line.contains("0:00:00.00"))
    );
    let tail_receipt: Value =
        serde_json::from_slice(&fs::read(dir.join("receipt-tail.json")).unwrap()).unwrap();
    assert_eq!(tail_receipt["duration_samples"], 72_000);
    write(&dir.join("definition.json"), &scored_definition);
    catalog["templates"][2]["definition_sha256"] =
        sha(&fs::read(dir.join("definition.json")).unwrap()).into();
    write(&dir.join("catalog.json"), &catalog);
    assert!(
        !run("v2-without-tail.ass", "v2-without-tail-receipt.json")
            .status
            .success()
    );
    write(&dir.join("definition.json"), &tail_definition);
    catalog["templates"][2]["definition_sha256"] =
        sha(&fs::read(dir.join("definition.json")).unwrap()).into();
    write(&dir.join("catalog.json"), &catalog);
    tail_source["display_units"][0]["source_scope_id"] = json!("wrong-title-id");
    write(&dir.join("source-text.json"), &tail_source);
    let changed_bytes = fs::read(dir.join("source-text.json")).unwrap();
    let changed_sha = sha(&changed_bytes);
    scene_bindings["assets"]["es.poem.source"]["sha256"] = changed_sha.clone().into();
    scene_bindings["assets"]["es.poem.source"]["bytes"] = (changed_bytes.len() as u64).into();
    scene_bindings["assets"]["es.poem.source"]["cache_uri"] =
        format!("cache://sha256/{changed_sha}").into();
    write(&dir.join("bindings.json"), &scene_bindings);
    assert!(
        !run("wrong-display.ass", "wrong-display-receipt.json")
            .status
            .success()
    );
    tail_source["display_units"][0]["source_scope_id"] = json!("poem-title");
    write(&dir.join("source-text.json"), &tail_source);
    write(&dir.join("bindings.json"), &{
        let restored_bytes = fs::read(dir.join("source-text.json")).unwrap();
        let restored_sha = sha(&restored_bytes);
        scene_bindings["assets"]["es.poem.source"]["sha256"] = restored_sha.clone().into();
        scene_bindings["assets"]["es.poem.source"]["bytes"] = (restored_bytes.len() as u64).into();
        scene_bindings["assets"]["es.poem.source"]["cache_uri"] =
            format!("cache://sha256/{restored_sha}").into();
        scene_bindings.clone()
    });
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

    // Exercise a source cue inside a selected combined performance through the CLI.
    let mut label_definition = definition.clone();
    label_definition["kind"] = json!("semantic-label");
    label_definition["panel"] = Value::Null;
    label_definition["byline"] = Value::Null;
    label_definition["title_x"] = json!(640);
    label_definition["title_y"] = json!(680);
    label_definition["fixed_duration_seconds"] = json!(4);
    write(&dir.join("definition.json"), &label_definition);
    catalog["templates"][2]["kind"] = json!("semantic-label");
    catalog["templates"][2]["required_content_keys"] = json!(["titles", "lines_by_language"]);
    catalog["templates"][2]
        .as_object_mut()
        .unwrap()
        .remove("soundtrack_requirement");
    catalog["templates"][2]["definition_sha256"] =
        sha(&fs::read(dir.join("definition.json")).unwrap()).into();
    write(&dir.join("catalog.json"), &catalog);
    scene["presentation"]["role"] = json!("semantic-label");
    scene
        .as_object_mut()
        .unwrap()
        .remove("presentation_source_scope_ids");
    scene["languages"]["es"]["cues"][0]["source_cue_ids"] = json!(["source-block-1"]);
    scene["presentation"]["content"] = json!({
        "titles":{"es":"1937","en":"1937"},
        "lines_by_language":{"es":[{"text":"1937","cue_id":"source-block-1","audio_cue_id":"es-1","semantic_trigger_id":"first-line"}]},
        "source_text_bindings":{"es":"es.poem.source","en":"en.poem.source"}
    });
    let mut label_source = json!({
        "schema":"reel.presentation-source-text.v1", "source_authority_id":"source-1",
        "source_document_sha256":"a".repeat(64), "source_scope_ids":["source-block-1"],
        "language":"es", "text_state":"project-draft-review-held", "title":"1937",
        "byline":null,"chapter_number":null,
        "lines":[{"text":"1937","cue_id":"source-block-1","audio_cue_id":"es-1","stanza_break_before":false}]
    });
    for (suffix, source_id, success) in [
        ("valid", "source-block-1", true),
        ("unrelated", "unrelated-source", false),
    ] {
        scene["presentation"]["content"]["lines_by_language"]["es"][0]["cue_id"] = json!(source_id);
        label_source["lines"][0]["cue_id"] = json!(source_id);
        write(&dir.join("scene.json"), &scene);
        write(&dir.join("source-text.json"), &label_source);
        let source_bytes = fs::read(dir.join("source-text.json")).unwrap();
        scene_bindings["assets"]["es.poem.source"]["sha256"] = sha(&source_bytes).into();
        scene_bindings["assets"]["es.poem.source"]["bytes"] = json!(source_bytes.len());
        scene_bindings["assets"]["es.poem.source"]["cache_uri"] =
            format!("cache://sha256/{}", sha(&source_bytes)).into();
        write(&dir.join("bindings.json"), &scene_bindings);
        let result = run(
            &format!("label-{suffix}.ass"),
            &format!("label-{suffix}.json"),
        );
        assert_eq!(
            result.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let label_ass = fs::read_to_string(dir.join("label-valid.ass")).unwrap();
    assert!(label_ass.contains("\\pos(640,680)"));
    assert!(label_ass.contains("1937"));
}
