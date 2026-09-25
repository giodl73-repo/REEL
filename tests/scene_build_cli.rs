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
        "semantic_delivery":"semantic-delivery.json"
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
        &root.join("catalog.json"),
        &serde_json::json!({"schema":"reel.scene-template-catalog.v1","templates":[]}),
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
        &serde_json::json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"scene","assets":{"voice":asset("voice",&voice),"red":asset("red",&picture),"alignment":{"logical_id":"alignment","sha256":"c".repeat(64),"bytes":1,"cache_uri":format!("cache://sha256/{}","c".repeat(64)),"selection_state":"selected-private-production"}}}),
    );
    write_json(
        &root.join("build.json"),
        &serde_json::json!({"schema":"reel.scene-build.v1","scene_id":"scene","language":"es","catalog":"catalog.json","episode":"episode.json","scene":"scene.json","policy":"policy.json","season_bindings":"season-bindings.json","episode_bindings":"episode-bindings.json","scene_bindings":"scene-bindings.json","semantic_delivery":"semantic.json"}),
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
}
