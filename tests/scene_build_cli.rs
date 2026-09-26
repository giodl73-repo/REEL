use std::{fs, process::Command};

fn write_json(path: &std::path::Path, value: &serde_json::Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn reference(root: &std::path::Path, name: &str) -> serde_json::Value {
    use sha2::{Digest, Sha256};
    let bytes = fs::read(root.join(name)).unwrap();
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    serde_json::json!({"path":name,"sha256":sha,"bytes":bytes.len()})
}

fn wav(path: &std::path::Path) {
    let samples = 48_000u32;
    let size = samples * 6;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + size).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(48_000u32.to_le_bytes());
    bytes.extend(288_000u32.to_le_bytes());
    bytes.extend(6u16.to_le_bytes());
    bytes.extend(24u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(size.to_le_bytes());
    bytes.extend(vec![0u8; size as usize]);
    fs::write(path, bytes).unwrap();
}

#[test]
fn one_command_build_refuses_unrendered_poem_template() {
    let root = tempfile::tempdir().unwrap();
    let from = format!(
        "{}/tests/fixtures/scene-authoring",
        env!("CARGO_MANIFEST_DIR")
    );
    for name in [
        "catalog",
        "episode",
        "scene",
        "policy",
        "season-bindings",
        "episode-bindings",
        "scene-bindings",
    ] {
        fs::copy(
            format!("{from}/{name}.json"),
            root.path().join(format!("{name}.json")),
        )
        .unwrap();
    }
    fs::write(root.path().join("semantic-delivery.json"), "{}").unwrap();
    let manifest = serde_json::json!({
        "schema":"reel.scene-build.v1", "scene_id":"scene-poem", "language":"es",
        "catalog":"catalog.json", "episode":"episode.json", "scene":"scene.json",
        "policy":"policy.json", "season_bindings":"season-bindings.json",
        "episode_bindings":"episode-bindings.json", "scene_bindings":"scene-bindings.json",
        "alignment_paths":"alignment-paths.json", "semantic_delivery":"semantic-delivery.json"
    });
    fs::write(
        root.path().join("build.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let output_dir = root.path().join("output");
    let result = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("build")
        .arg(root.path())
        .arg("build.json")
        .arg("--asset-root")
        .arg(root.path())
        .arg("--output-dir")
        .arg(&output_dir)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("template presentation requires"));
    assert!(!output_dir.exists());
}

#[test]
fn one_command_build_renders_and_checks_an_independent_scene() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(
        root.join("red.ppm"),
        [b"P6\n2 2\n255\n".as_slice(), &[255, 0, 0].repeat(4)].concat(),
    )
    .unwrap();
    wav(&root.join("voice.wav"));
    write_json(
        &root.join("production.json"),
        &serde_json::json!({
            "manifest_version":"reel.manifest.v0.2","profile":"animatic","timing_status":"conformed",
            "work":"synthetic","title":"One scene","scenes":[{"id":"scene"}],
            "shots":[{"id":"shot","scene_id":"scene"}],
            "narration_cues":[{"id":"cue","speaker_id":"speaker"}],
            "audio_events":[{"id":"cue","role":"narration","source":"voice.wav","start_seconds":0}]
        }),
    );
    write_json(
        &root.join("contract.json"),
        &serde_json::json!({
            "schema":"reel.cue-relative-assembly.v0.1","id":"scene","production_manifest":"production.json",
            "sample_rate":48000,"frame_rate":{"numerator":24,"denominator":1},
            "cues":[{"cue_id":"cue","duration_samples":48000}],
            "attachments":[
                {"id":"p","target":{"kind":"cel","shot_id":"shot","cel_id":"red"},"start":{"kind":"cue-start","cue_id":"cue"},"end":{"kind":"cue-end","cue_id":"cue"}},
                {"id":"d","target":{"kind":"audio","audio_event_id":"cue"},"start":{"kind":"cue-start","cue_id":"cue"},"end":{"kind":"cue-end","cue_id":"cue"}}
            ]
        }),
    );
    let picture = reference(root, "red.ppm");
    let voice = reference(root, "voice.wav");
    write_json(
        &root.join("alignment.json"),
        &serde_json::json!({
            "schema":"reel.scene-native-alignment.v1","language":"es","cue_id":"cue",
            "selected_take_sha256":voice["sha256"],"sample_rate":48000,
            "cue_end_sample":48000,"semantic_markers":{"first":0}
        }),
    );
    write_json(
        &root.join("alignment-paths.json"),
        &serde_json::json!({"cue":"alignment.json"}),
    );
    write_json(
        &root.join("job.json"),
        &serde_json::json!({
            "schema":"reel.scene-delivery.v0.1","id":"scene","contract":reference(root,"contract.json"),
            "production_manifest_sha256":reference(root,"production.json")["sha256"],
            "width":64,"height":64,"max_composition_samples":480000,
            "pictures":[{"attachment_id":"p","source":picture,"kind":"still","attention":"source beat"}],
            "audio":[{"attachment_id":"d","source":voice,"bus":"D","cue_id":"cue"}],
            "buses":{"D":{"state":"present","reason":"selected take"},"M":{"state":"intentional-silence","reason":"scene choice"},"E":{"state":"intentional-silence","reason":"scene choice"}}
        }),
    );
    let picture_sha = picture["sha256"].as_str().unwrap();
    let voice_sha = voice["sha256"].as_str().unwrap();
    let graph = serde_json::json!({
        "schema":"reel.semantic-assembly.v1","lock":{"logical_id":"lock","sha256":"a".repeat(64)},
        "slots":[
            {"slot_id":"narration","beat_id":"beat","lane":"narration","disposition":"selected","selected_revision_id":"r1","revisions":[{"revision_id":"r1","asset":{"logical_id":"voice","cache_uri":format!("cache://sha256/{voice_sha}"),"sha256":voice_sha}}]},
            {"slot_id":"picture","beat_id":"beat","lane":"picture","disposition":"selected","selected_revision_id":"r1","revisions":[{"revision_id":"r1","asset":{"logical_id":"red","cache_uri":format!("cache://sha256/{picture_sha}"),"sha256":picture_sha}}]}
        ],
        "events":[{"event_id":"event","scene_id":"scene","language":"es","narration":{"logical_id":"voice","sha256":voice_sha},"picture":{"logical_id":"red","sha256":picture_sha},"phrase_start_seconds":0.0,"phrase_end_seconds":1.0}],
        "nodes":[{"id":"scene","inputs":[],"slots":["narration","picture"],"events":["event"]}],
        "presentation_targets":[]
    });
    let pointer = serde_json::json!({"schema":"reel.selected-pointer.v1","logical_id":"current","selected_lock":graph["lock"]});
    write_json(
        &root.join("semantic.json"),
        &serde_json::json!({
            "schema":"reel.semantic-delivery.v1","id":"scene-es","pointer":pointer,"graph":graph,"target":"scene",
            "scene_delivery_job":reference(root,"job.json"),
            "event_bindings":[{"event_id":"event","narration_attachment_id":"d","picture_attachment_id":"p","audio_attachment_ids":[],"external_layer_attachment_ids":[]}]
        }),
    );
    write_json(
        &root.join("master-template.json"),
        &serde_json::json!({"schema":"reel.episode-master-template.v1","template_id":"master","ordered_roles":["chapter-scenes"],"optional_roles":[]}),
    );
    write_json(
        &root.join("catalog.json"),
        &serde_json::json!({"schema":"reel.scene-template-catalog.v1","templates":[
            {"template_id":"master","kind":"episode-master","definition_sha256":reference(root,"master-template.json")["sha256"],"required_content_keys":[]}
        ]}),
    );
    write_json(
        &root.join("episode.json"),
        &serde_json::json!({"schema":"reel.episode-authoring.v1","episode_id":"ep","season_id":"season","authoring_state":"ready-for-private-build","master_template_id":"master","scene_policy_id":"narrative","presentation":[],"score_palette":[],"scene_ids":["scene"]}),
    );
    write_json(
        &root.join("scene.json"),
        &serde_json::json!({
            "schema":"reel.scene-authoring.v1","scene_id":"scene","episode_id":"ep","authoring_state":"ready-for-private-build",
            "source_scope_ids":["beat"],"source_authority_id":"source","languages":{"es":{"cues":[{"cue_id":"cue","source_id":"beat","exact_text_sha256":"b".repeat(64),"narration_slot_id":"narration","take_binding":"voice","phrase_alignment_binding":"alignment"}],
            "events":[{"event_id":"event","cue_id":"cue","semantic_trigger_id":"first","picture_slot_id":"picture","picture_binding":"red","score":{"disposition":"silence"},"sonic_bindings":[],"vfx_bindings":[]}]}}
        }),
    );
    write_json(
        &root.join("policy.json"),
        &serde_json::json!({"schema":"reel.scene-policy.v1","policy_id":"narrative","target_composition_seconds_min":0.5,"target_composition_seconds_max":10.0,"hard_unchanged_composition_seconds_max":10.0,"semantic_cuts_required":true}),
    );
    let empty =
        serde_json::json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"empty","assets":{}});
    write_json(&root.join("season-bindings.json"), &empty);
    write_json(&root.join("episode-bindings.json"), &empty);
    let asset = |logical: &str, reference: &serde_json::Value| serde_json::json!({"logical_id":logical,"sha256":reference["sha256"],"bytes":reference["bytes"],"cache_uri":format!("cache://sha256/{}",reference["sha256"].as_str().unwrap()),"selection_state":"selected-private-production"});
    write_json(
        &root.join("scene-bindings.json"),
        &serde_json::json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"scene","assets":{"voice":asset("voice",&voice),"red":asset("red",&picture),"alignment":asset("alignment",&reference(root,"alignment.json"))}}),
    );
    write_json(
        &root.join("build.json"),
        &serde_json::json!({"schema":"reel.scene-build.v1","scene_id":"scene","language":"es","catalog":"catalog.json","episode":"episode.json","scene":"scene.json","policy":"policy.json","season_bindings":"season-bindings.json","episode_bindings":"episode-bindings.json","scene_bindings":"scene-bindings.json","alignment_paths":"alignment-paths.json","semantic_delivery":"semantic.json"}),
    );
    let output_dir = root.join("output");
    let result = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("build")
        .arg(root)
        .arg("build.json")
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(&output_dir)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(
        &fs::read(output_dir.join("scene-authoring-build-receipt.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["scene_id"], "scene");
    assert_eq!(receipt["language"], "es");
    assert_eq!(receipt["publication"], "not-authorized");

    // Changing a selected alignment and rebinding its exact new bytes cannot
    // leave the old graph phrase clock accepted by the scene build.
    let original_alignment = fs::read(root.join("alignment.json")).unwrap();
    let original_bindings = fs::read(root.join("scene-bindings.json")).unwrap();
    let mut alignment: serde_json::Value = serde_json::from_slice(&original_alignment).unwrap();
    alignment["cue_end_sample"] = 40000.into();
    write_json(&root.join("alignment.json"), &alignment);
    let mut bindings: serde_json::Value = serde_json::from_slice(&original_bindings).unwrap();
    bindings["assets"]["alignment"] = asset("alignment", &reference(root, "alignment.json"));
    write_json(&root.join("scene-bindings.json"), &bindings);
    let stale_clock = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("build")
        .arg(root)
        .arg("build.json")
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("stale-clock-output"))
        .output()
        .unwrap();
    assert!(!stale_clock.status.success());
    assert!(
        String::from_utf8_lossy(&stale_clock.stderr)
            .contains("phrase clock differs from selected alignment")
    );
    fs::write(root.join("alignment.json"), original_alignment).unwrap();
    fs::write(root.join("scene-bindings.json"), original_bindings).unwrap();

    // The scene author can select a score role, but delivery must actually
    // bind the selected score on M for that event.
    let original_scene = fs::read(root.join("scene.json")).unwrap();
    let original_episode = fs::read(root.join("episode.json")).unwrap();
    let original_episode_bindings = fs::read(root.join("episode-bindings.json")).unwrap();
    let mut scene_with_score: serde_json::Value = serde_json::from_slice(&original_scene).unwrap();
    scene_with_score["languages"]["es"]["events"][0]["score"] =
        serde_json::json!({"disposition":"role","role":"poem-main"});
    write_json(&root.join("scene.json"), &scene_with_score);
    let mut episode_with_score: serde_json::Value =
        serde_json::from_slice(&original_episode).unwrap();
    episode_with_score["score_palette"] = serde_json::json!([{
        "role":"poem-main","theme_id":"poem","source_poem_id":"poem",
        "arrangement_id":"arrangement","asset_binding":"score"
    }]);
    write_json(&root.join("episode.json"), &episode_with_score);
    let mut score_bindings: serde_json::Value =
        serde_json::from_slice(&original_episode_bindings).unwrap();
    score_bindings["assets"]["score"] = asset("score", &voice);
    write_json(&root.join("episode-bindings.json"), &score_bindings);
    let omitted_score = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("build")
        .arg(root)
        .arg("build.json")
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("omitted-score-output"))
        .output()
        .unwrap();
    assert!(!omitted_score.status.success());
    assert!(
        String::from_utf8_lossy(&omitted_score.stderr)
            .contains("M/E attachments differ from authored score or Sonic")
    );
    fs::write(root.join("scene.json"), original_scene).unwrap();
    fs::write(root.join("episode.json"), original_episode).unwrap();
    fs::write(
        root.join("episode-bindings.json"),
        original_episode_bindings,
    )
    .unwrap();

    // Conform the selected scene through the same generic episode boundary.
    // This reopens and independently checks every upstream scene-delivery
    // output against its selected job, rather than trusting only receipt text.
    let scene_segment = serde_json::json!({
        "kind":"scene","id":"scene","master":reference(root,"output/master.mkv"),
        "source_receipt":reference(root,"output/scene-authoring-build-receipt.json"),
        "delivery_job":reference(root,"job.json"),
        "delivery_receipt":reference(root,"output/receipt.json")
    });
    write_json(
        &root.join("conform.json"),
        &serde_json::json!({
            "schema":"reel.episode-conform.v1","episode_id":"ep","language":"es",
            "output_sample_rate":48000,"catalog":reference(root,"catalog.json"),
            "master_template_definition":reference(root,"master-template.json"),
            "episode":reference(root,"episode.json"),
            "season_bindings":reference(root,"season-bindings.json"),
            "episode_bindings":reference(root,"episode-bindings.json"),
            "segments":[scene_segment]
        }),
    );
    let conform = Command::new(env!("CARGO_BIN_EXE_reel-episode-conform"))
        .arg("build")
        .arg(root.join("conform.json"))
        .arg("--input-root")
        .arg(root)
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("conformed"))
        .output()
        .unwrap();
    assert!(
        conform.status.success(),
        "{}",
        String::from_utf8_lossy(&conform.stderr)
    );
    let conform_receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("conformed/receipt.json")).unwrap()).unwrap();
    assert_eq!(
        conform_receipt["upstream_delivery_recheck_state"],
        "verified-for-all-scene-segments"
    );
    assert_eq!(
        conform_receipt["segments"][0]["upstream_delivery_verified"],
        true
    );
    let episode_resolved = Command::new(env!("CARGO_BIN_EXE_reel-scene-authoring"))
        .arg("resolve-language")
        .args([
            "catalog.json",
            "episode.json",
            "scene.json",
            "policy.json",
            "season-bindings.json",
            "episode-bindings.json",
            "scene-bindings.json",
            "es",
            "--output",
        ])
        .arg(root.join("episode-resolved.json"))
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        episode_resolved.status.success(),
        "{}",
        String::from_utf8_lossy(&episode_resolved.stderr)
    );
    write_json(
        &root.join("episode-index.json"),
        &serde_json::json!({
            "schema":"reel.scene-build-index.v1","graph_id":"episode-execution-test",
            "project_root":".","jobs":[{"node_id":"scene-es","build_manifest":"build.json",
                "resolved_language":"episode-resolved.json","semantic_delivery":"semantic.json"}]
        }),
    );
    write_json(
        &root.join("episode-state-empty.json"),
        &serde_json::json!({
            "schema":"reel.changed-only-state.v0.1","graph_id":"episode-execution-test","nodes":[]
        }),
    );
    let episode_scene_run = root.join("episode-scene-run");
    let scene_result = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("execute-changed-only")
        .arg(root.join("episode-index.json"))
        .arg(root.join("episode-state-empty.json"))
        .arg("--asset-root")
        .arg(root)
        .arg("--output-root")
        .arg(&episode_scene_run)
        .output()
        .unwrap();
    assert!(
        scene_result.status.success(),
        "{}",
        String::from_utf8_lossy(&scene_result.stderr)
    );
    let mut conform_template: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("conform.json")).unwrap()).unwrap();
    let scene_segment = &mut conform_template["segments"][0];
    for field in ["master", "source_receipt", "delivery_receipt"] {
        scene_segment.as_object_mut().unwrap().remove(field);
    }
    scene_segment["scene_node_id"] = serde_json::json!("scene-es");
    write_json(&root.join("conform-template.json"), &conform_template);
    write_json(
        &root.join("episode-build.json"),
        &serde_json::json!({
            "schema":"reel.episode-build.v1", "scene_build_index":"episode-index.json",
            "conform_template":"conform-template.json"
        }),
    );
    let mut stale_template = conform_template.clone();
    stale_template["segments"][0]["master"] = reference(root, "output/master.mkv");
    write_json(&root.join("stale-conform-template.json"), &stale_template);
    write_json(
        &root.join("stale-episode-build.json"),
        &serde_json::json!({
            "schema":"reel.episode-build.v1", "scene_build_index":"episode-index.json",
            "conform_template":"stale-conform-template.json"
        }),
    );
    let rejected_episode = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("execute-episode")
        .arg(root)
        .arg("stale-episode-build.json")
        .arg(episode_scene_run.join("final-state.json"))
        .arg("--asset-root")
        .arg(root)
        .arg("--output-root")
        .arg(root.join("rejected-episode-run"))
        .output()
        .unwrap();
    assert!(!rejected_episode.status.success());
    assert!(!root.join("rejected-episode-run").exists());
    let episode_run = root.join("episode-run");
    let episode_result = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("execute-episode")
        .arg(root)
        .arg("episode-build.json")
        .arg(episode_scene_run.join("final-state.json"))
        .arg("--asset-root")
        .arg(root)
        .arg("--output-root")
        .arg(&episode_run)
        .output()
        .unwrap();
    assert!(
        episode_result.status.success(),
        "{}",
        String::from_utf8_lossy(&episode_result.stderr)
    );
    assert!(episode_run.join("episode/master.mkv").exists());
    assert!(!episode_run.join("scenes/scene-es").exists());
    let episode_receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(episode_run.join("episode/receipt.json")).unwrap())
            .unwrap();
    assert_eq!(
        episode_receipt["upstream_delivery_recheck_state"],
        "verified-for-all-scene-segments"
    );
    let fresh_episode_run = root.join("episode-run-fresh");
    let fresh = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("execute-episode")
        .arg(root)
        .arg("episode-build.json")
        .arg(root.join("episode-state-empty.json"))
        .arg("--asset-root")
        .arg(root)
        .arg("--output-root")
        .arg(&fresh_episode_run)
        .output()
        .unwrap();
    assert!(
        fresh.status.success(),
        "{}",
        String::from_utf8_lossy(&fresh.stderr)
    );
    assert!(
        fresh_episode_run
            .join("scenes/scene-es/master.mkv")
            .exists()
    );
    assert!(fresh_episode_run.join("episode/master.mkv").exists());
    let fresh_receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(fresh_episode_run.join("episode/receipt.json")).unwrap())
            .unwrap();
    assert_eq!(
        fresh_receipt["upstream_delivery_recheck_state"],
        "verified-for-all-scene-segments"
    );
    let dialogue_bytes = fs::read(root.join("output/D.wav")).unwrap();
    fs::write(root.join("output/D.wav"), b"tampered dialogue stem").unwrap();
    let rejected_conform = Command::new(env!("CARGO_BIN_EXE_reel-episode-conform"))
        .arg("build")
        .arg(root.join("conform.json"))
        .arg("--input-root")
        .arg(root)
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("rejected-conform"))
        .output()
        .unwrap();
    assert!(!rejected_conform.status.success());
    assert!(!root.join("rejected-conform").exists());
    fs::write(root.join("output/D.wav"), dialogue_bytes).unwrap();

    // Promote the same synthetic scene to a selected poem-panel production.
    // The compiler owns layout and native line timing; the scene names source
    // text, a font, and selected ASS/receipt bindings only.
    let font_path = [
        "C:/Windows/Fonts/arial.ttf",
        "C:/Windows/Fonts/ARIAL.TTF",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ]
    .into_iter()
    .map(std::path::Path::new)
    .find(|path| path.is_file());
    let Some(font_path) = font_path else { return };
    fs::copy(font_path, root.join("font.ttf")).unwrap();
    let font_family = if cfg!(windows) {
        "Arial"
    } else {
        "DejaVu Sans"
    };
    let mut contract: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("contract.json")).unwrap()).unwrap();
    contract["attachments"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id":"title","target":{"kind":"title","title_id":"poem"},
            "start":{"kind":"cue-start","cue_id":"cue"},
            "end":{"kind":"cue-end","cue_id":"cue"}
        }));
    write_json(&root.join("contract.json"), &contract);
    let definition = serde_json::json!({
        "schema":"reel.editable-text-template.v1","template_id":"poem-test","kind":"opening-poem",
        "canvas_width":64,"canvas_height":64,"font_name":font_family,"title_size":14,"body_size":12,
        "title_x":34,"title_y":4,"body_x":34,"body_y":24,"line_spacing":14,
        "future_rgb":[120,127,133],"active_rgb":[216,227,232],"completed_rgb":[255,255,255],
        "panel":{"x":32,"width":32,"background_rgb":[20,18,16],"divider_rgb":[211,178,107],"divider_alpha":185},
        "fixed_duration_seconds":null
    });
    write_json(&root.join("definition.json"), &definition);
    let mut catalog: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("catalog.json")).unwrap()).unwrap();
    catalog["templates"] = serde_json::json!([{"template_id":"poem-test","kind":"opening-poem","definition_sha256":reference(root,"definition.json")["sha256"],"required_content_keys":["poem_id","line_cue_ids","titles","lines_by_language","source_text_bindings"]}]);
    write_json(&root.join("catalog.json"), &catalog);
    let source = serde_json::json!({
        "schema":"reel.presentation-source-text.v1","source_authority_id":"source",
        "source_document_sha256":"b".repeat(64),"source_scope_ids":["beat"],
        "language":"es","text_state":"canonical-original","title":"P","chapter_number":null,
        "lines":[{"text":"A","cue_id":"cue","stanza_break_before":false}]
    });
    write_json(&root.join("source.json"), &source);
    let alignment = serde_json::json!({
        "schema":"reel.scene-native-alignment.v1","language":"es","cue_id":"cue",
        "selected_take_sha256":voice_sha,"sample_rate":48000,"cue_end_sample":48000,
        "semantic_markers":{"first":0}
    });
    write_json(&root.join("alignment.json"), &alignment);
    write_json(
        &root.join("alignment-paths.json"),
        &serde_json::json!({"cue":"alignment.json"}),
    );
    let mut scene: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("scene.json")).unwrap()).unwrap();
    scene["presentation"] = serde_json::json!({
        "role":"opening-poem","template_id":"poem-test","asset_binding":"font",
        "content":{"poem_id":"synthetic-poem","line_cue_ids":{"es":["cue"]},
            "titles":{"es":"P"},"lines_by_language":{"es":[{"text":"A","cue_id":"cue","semantic_trigger_id":"first"}]},
            "source_text_bindings":{"es":"source-text"},"ass_layer_bindings":{"es":"ass-layer"},
            "template_receipt_bindings":{"es":"template-receipt"}}
    });
    write_json(&root.join("scene.json"), &scene);
    let mut bindings: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("scene-bindings.json")).unwrap()).unwrap();
    bindings["assets"]["alignment"] = asset("alignment", &reference(root, "alignment.json"));
    bindings["assets"]["source-text"] = asset("source-text", &reference(root, "source.json"));
    bindings["assets"]["font"] = asset("font", &reference(root, "font.ttf"));
    write_json(&root.join("scene-bindings.json"), &bindings);
    write_json(
        &root.join("season-bindings.json"),
        &serde_json::json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"season","assets":{}}),
    );
    write_json(
        &root.join("episode-bindings.json"),
        &serde_json::json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"ep","assets":{}}),
    );
    let compiled = Command::new(env!("CARGO_BIN_EXE_reel-scene-template"))
        .arg("compile")
        .arg(root.join("catalog.json"))
        .arg(root.join("definition.json"))
        .arg(root.join("scene.json"))
        .arg("es")
        .arg(root.join("season-bindings.json"))
        .arg(root.join("episode-bindings.json"))
        .arg(root.join("scene-bindings.json"))
        .arg(root.join("alignment-paths.json"))
        .arg(root.join("source.json"))
        .arg("--output-ass")
        .arg(root.join("poem.ass"))
        .arg("--receipt")
        .arg(root.join("template-receipt.json"))
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    bindings["assets"]["ass-layer"] = asset("ass-layer", &reference(root, "poem.ass"));
    bindings["assets"]["template-receipt"] = asset(
        "template-receipt",
        &reference(root, "template-receipt.json"),
    );
    write_json(&root.join("scene-bindings.json"), &bindings);
    let mut job: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("job.json")).unwrap()).unwrap();
    job["contract"] = reference(root, "contract.json");
    job["external_layers"] = serde_json::json!([{
        "attachment_id":"title","reason":"Selected editable poem panel","evidence":reference(root,"poem.ass"),
        "render_mode":"ass-overlay","font":reference(root,"font.ttf")
    }]);
    write_json(&root.join("job.json"), &job);
    let mut semantic: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("semantic.json")).unwrap()).unwrap();
    for (slot_id, logical_id, file_name) in [
        ("ass-slot", "ass-layer", "poem.ass"),
        ("font-slot", "font", "font.ttf"),
    ] {
        let hash = reference(root, file_name)["sha256"]
            .as_str()
            .unwrap()
            .to_owned();
        semantic["graph"]["slots"].as_array_mut().unwrap().push(serde_json::json!({
            "slot_id":slot_id,"beat_id":"beat","lane":"presentation","disposition":"selected",
            "selected_revision_id":"r1","revisions":[{"revision_id":"r1","asset":{"logical_id":logical_id,"cache_uri":format!("cache://sha256/{hash}"),"sha256":hash}}]
        }));
        semantic["graph"]["nodes"][0]["slots"]
            .as_array_mut()
            .unwrap()
            .push(slot_id.into());
    }
    semantic["scene_delivery_job"] = reference(root, "job.json");
    semantic["event_bindings"][0]["external_layer_attachment_ids"] = serde_json::json!(["title"]);
    write_json(&root.join("semantic.json"), &semantic);
    let mut build: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("build.json")).unwrap()).unwrap();
    build["template_receipt"] = "template-receipt.json".into();
    write_json(&root.join("build.json"), &build);
    let template_output = root.join("template-output");
    let built = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("build")
        .arg(root)
        .arg("build.json")
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(&template_output)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(
        &fs::read(template_output.join("scene-authoring-build-receipt.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        receipt["template_ass_sha256"],
        reference(root, "poem.ass")["sha256"]
    );
    assert_eq!(receipt["source_text_state"], "canonical-original");
    assert!(template_output.join("clean-picture.mkv").exists());

    let resolved = Command::new(env!("CARGO_BIN_EXE_reel-scene-authoring"))
        .arg("resolve-language")
        .args([
            "catalog.json",
            "episode.json",
            "scene.json",
            "policy.json",
            "season-bindings.json",
            "episode-bindings.json",
            "scene-bindings.json",
            "es",
            "--output",
        ])
        .arg(root.join("resolved.json"))
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        resolved.status.success(),
        "{}",
        String::from_utf8_lossy(&resolved.stderr)
    );
    write_json(
        &root.join("index.json"),
        &serde_json::json!({
            "schema":"reel.scene-build-index.v1","graph_id":"synthetic-episode",
            "project_root":".","jobs":[{"node_id":"scene-es","build_manifest":"build.json",
                "resolved_language":"resolved.json","semantic_delivery":"semantic.json"}]
        }),
    );
    write_json(
        &root.join("state-empty.json"),
        &serde_json::json!({
            "schema":"reel.changed-only-state.v0.1","graph_id":"synthetic-episode","nodes":[]
        }),
    );
    let run_one = root.join("run-one");
    let first = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("execute-changed-only")
        .arg(root.join("index.json"))
        .arg(root.join("state-empty.json"))
        .arg("--asset-root")
        .arg(root)
        .arg("--output-root")
        .arg(&run_one)
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(run_one.join("scene-es/master.mkv").exists());
    assert!(run_one.join("result-receipt-000.json").exists());
    let run_two = root.join("run-two");
    let second = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("execute-changed-only")
        .arg(root.join("index.json"))
        .arg(run_one.join("final-state.json"))
        .arg("--asset-root")
        .arg(root)
        .arg("--output-root")
        .arg(&run_two)
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        String::from_utf8_lossy(&second.stdout).contains("rebuilt 0 scene languages; reused 1")
    );
    assert!(!run_two.join("scene-es").exists());
    let mut changed_policy: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("policy.json")).unwrap()).unwrap();
    changed_policy["target_composition_seconds_min"] = serde_json::json!(0.6);
    write_json(&root.join("policy.json"), &changed_policy);
    fs::remove_file(root.join("resolved.json")).unwrap();
    let refreshed = Command::new(env!("CARGO_BIN_EXE_reel-scene-authoring"))
        .arg("resolve-language")
        .args([
            "catalog.json",
            "episode.json",
            "scene.json",
            "policy.json",
            "season-bindings.json",
            "episode-bindings.json",
            "scene-bindings.json",
            "es",
            "--output",
        ])
        .arg(root.join("resolved.json"))
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        refreshed.status.success(),
        "{}",
        String::from_utf8_lossy(&refreshed.stderr)
    );
    let run_three = root.join("run-three");
    let third = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("execute-changed-only")
        .arg(root.join("index.json"))
        .arg(run_one.join("final-state.json"))
        .arg("--asset-root")
        .arg(root)
        .arg("--output-root")
        .arg(&run_three)
        .output()
        .unwrap();
    assert!(
        third.status.success(),
        "{}",
        String::from_utf8_lossy(&third.stderr)
    );
    assert!(String::from_utf8_lossy(&third.stdout).contains("rebuilt 1 scene languages; reused 0"));
    assert!(run_three.join("scene-es/master.mkv").exists());

    // The selected cel bytes remain identical, but a second picture attachment
    // is introduced without an authored semantic event. Hash checks alone pass.
    let mut contract: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("contract.json")).unwrap()).unwrap();
    contract["attachments"][0]["end"] =
        serde_json::json!({"kind":"cue-progress","cue_id":"cue","numerator":1,"denominator":2});
    contract["attachments"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id":"p-tail","target":{"kind":"cel","shot_id":"shot","cel_id":"red"},
            "start":{"kind":"cue-progress","cue_id":"cue","numerator":1,"denominator":2},
            "end":{"kind":"cue-end","cue_id":"cue"}
        }));
    write_json(&root.join("contract.json"), &contract);
    let mut job: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("job.json")).unwrap()).unwrap();
    job["contract"] = reference(root, "contract.json");
    let selected_picture = job["pictures"][0]["source"].clone();
    job["pictures"].as_array_mut().unwrap().push(serde_json::json!({
        "attachment_id":"p-tail","source":selected_picture,"kind":"still","attention":"source beat"
    }));
    write_json(&root.join("job.json"), &job);
    let mut semantic: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("semantic.json")).unwrap()).unwrap();
    semantic["scene_delivery_job"] = reference(root, "job.json");
    write_json(&root.join("semantic.json"), &semantic);
    let shifted = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("build")
        .arg(root)
        .arg("build.json")
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("shifted-output"))
        .output()
        .unwrap();
    assert!(!shifted.status.success());
    assert!(
        String::from_utf8_lossy(&shifted.stderr).contains("not bound to a selected semantic event")
    );
}

#[test]
fn changed_only_graph_plans_each_scene_language_independently() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write_json(&root.join("alignment-paths.json"), &serde_json::json!({}));
    fs::write(
        root.join("es-delivery.json"),
        b"scene_delivery_job:\n  path: es-job.yaml\n",
    )
    .unwrap();
    fs::write(
        root.join("en-delivery.json"),
        b"scene_delivery_job:\n  path: en-job.yaml\n",
    )
    .unwrap();
    let fixture = format!(
        "{}/tests/fixtures/scene-authoring",
        env!("CARGO_MANIFEST_DIR")
    );
    for (source, destination) in [
        ("catalog", "catalog"),
        ("episode", "episode"),
        ("scene", "scene"),
        ("policy", "policy"),
        ("season-bindings", "season"),
        ("episode-bindings", "episode-bindings"),
        ("scene-bindings", "scene-bindings"),
    ] {
        fs::copy(
            format!("{fixture}/{source}.json"),
            root.join(format!("{destination}.json")),
        )
        .unwrap();
    }
    for language in ["es", "en"] {
        let resolved = Command::new(env!("CARGO_BIN_EXE_reel-scene-authoring"))
            .arg("resolve-language")
            .args([
                "catalog.json",
                "episode.json",
                "scene.json",
                "policy.json",
                "season.json",
                "episode-bindings.json",
                "scene-bindings.json",
                language,
                "--output",
            ])
            .arg(root.join(format!("{language}-resolved.json")))
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            resolved.status.success(),
            "{}",
            String::from_utf8_lossy(&resolved.stderr)
        );
        write_json(
            &root.join(format!("{language}-job.yaml")),
            &serde_json::json!({
                "schema":"reel.scene-delivery.v0.1","id":language,
                "contract":reference(root,"catalog.json"),"production_manifest_sha256":"a".repeat(64),
                "width":64,"height":64,"max_composition_samples":480000,
                "pictures":[],"audio":[],"buses":{}
            }),
        );
        write_json(
            &root.join(format!("{language}-build.json")),
            &serde_json::json!({
                "schema":"reel.scene-build.v1","scene_id":"scene-poem","language":language,
                "catalog":"catalog.json","episode":"episode.json","scene":"scene.json",
                "policy":"policy.json","season_bindings":"season.json",
                "episode_bindings":"episode-bindings.json","scene_bindings":"scene-bindings.json",
                "alignment_paths":"alignment-paths.json", "semantic_delivery":format!("{language}-delivery.json")
            }),
        );
    }
    write_json(
        &root.join("index.json"),
        &serde_json::json!({
            "schema":"reel.scene-build-index.v1","graph_id":"episode-one","project_root":".",
            "jobs":[
                {"node_id":"scene-001-es","build_manifest":"es-build.json","resolved_language":"es-resolved.json","semantic_delivery":"es-delivery.json"},
                {"node_id":"scene-001-en","build_manifest":"en-build.json","resolved_language":"en-resolved.json","semantic_delivery":"en-delivery.json"}
            ]
        }),
    );
    let graph = root.join("graph.json");
    let result = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("emit-changed-only-graph")
        .arg(root.join("index.json"))
        .arg("--asset-root")
        .arg(root)
        .arg("--output")
        .arg(&graph)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut escaped_index: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("index.json")).unwrap()).unwrap();
    escaped_index["jobs"][0]["resolved_language"] =
        serde_json::json!(root.join("es-resolved.json").to_string_lossy());
    write_json(&root.join("escaped-index.json"), &escaped_index);
    let escaped = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("emit-changed-only-graph")
        .arg(root.join("escaped-index.json"))
        .arg("--asset-root")
        .arg(root)
        .arg("--output")
        .arg(root.join("escaped-graph.json"))
        .output()
        .unwrap();
    assert!(!escaped.status.success());
    assert!(!root.join("escaped-graph.json").exists());
    write_json(
        &root.join("state.json"),
        &serde_json::json!({
            "schema":"reel.changed-only-state.v0.1","graph_id":"episode-one","nodes":[]
        }),
    );
    reel::changed_only::write_changed_only_plan(
        &graph,
        &root.join("state.json"),
        &root.join("plan.json"),
    )
    .unwrap();
    let plan: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("plan.json")).unwrap()).unwrap();
    assert_eq!(plan["summary"]["rebuild_count"], 2);
    assert_eq!(plan["summary"]["blocked_dependency_count"], 0);
    let graph_before = fs::read(&graph).unwrap();
    let mut changed_job: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("en-job.yaml")).unwrap()).unwrap();
    changed_job["id"] = "en-changed".into();
    write_json(&root.join("en-job.yaml"), &changed_job);
    let graph_after = root.join("graph-after.json");
    let rebuilt = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("emit-changed-only-graph")
        .arg(root.join("index.json"))
        .arg("--asset-root")
        .arg(root)
        .arg("--output")
        .arg(&graph_after)
        .output()
        .unwrap();
    assert!(
        rebuilt.status.success(),
        "{}",
        String::from_utf8_lossy(&rebuilt.stderr)
    );
    let before: serde_json::Value = serde_json::from_slice(&graph_before).unwrap();
    let after: serde_json::Value = serde_json::from_slice(&fs::read(graph_after).unwrap()).unwrap();
    assert_eq!(before["nodes"][0]["inputs"], after["nodes"][0]["inputs"]);
    assert_ne!(before["nodes"][1]["inputs"], after["nodes"][1]["inputs"]);

    // A season opening rebind belongs to episode presentation; it does not
    // change either scene-language action key.
    let mut season: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("season.json")).unwrap()).unwrap();
    season["assets"]["opening.selected"]["sha256"] = "f".repeat(64).into();
    season["assets"]["opening.selected"]["cache_uri"] =
        format!("cache://sha256/{}", "f".repeat(64)).into();
    write_json(&root.join("season.json"), &season);
    let unrelated_graph = root.join("graph-unrelated.json");
    let unrelated = Command::new(env!("CARGO_BIN_EXE_reel-scene-build"))
        .arg("emit-changed-only-graph")
        .arg(root.join("index.json"))
        .arg("--asset-root")
        .arg(root)
        .arg("--output")
        .arg(&unrelated_graph)
        .output()
        .unwrap();
    assert!(
        unrelated.status.success(),
        "{}",
        String::from_utf8_lossy(&unrelated.stderr)
    );
    let stable: serde_json::Value =
        serde_json::from_slice(&fs::read(unrelated_graph).unwrap()).unwrap();
    assert_eq!(after["nodes"][0]["inputs"], stable["nodes"][0]["inputs"]);
    assert_eq!(after["nodes"][1]["inputs"], stable["nodes"][1]["inputs"]);
}
