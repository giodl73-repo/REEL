use reel::{imported_scene_proof, scene_delivery};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn write(root: &Path, name: &str, value: &Value) {
    fs::write(root.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn reference(root: &Path, name: &str) -> Value {
    let bytes = fs::read(root.join(name)).unwrap();
    json!({"path":name,"bytes":bytes.len(),"sha256":Sha256::digest(bytes).iter()
        .map(|b| format!("{b:02x}")).collect::<String>()})
}
fn parsed_ref(root: &Path, name: &str) -> scene_delivery::FileRef {
    serde_json::from_value(reference(root, name)).unwrap()
}
fn wav(root: &Path, name: &str, samples: u32, signal: i32) {
    let size = samples * 6;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + size).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(48000u32.to_le_bytes());
    bytes.extend(288000u32.to_le_bytes());
    bytes.extend(6u16.to_le_bytes());
    bytes.extend(24u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(size.to_le_bytes());
    for _ in 0..samples {
        for _ in 0..2 {
            bytes.extend(&signal.to_le_bytes()[..3]);
        }
    }
    fs::write(root.join(name), bytes).unwrap();
}

fn fixture(root: &Path) -> Value {
    for (name, color) in [("red.ppm", [255, 0, 0]), ("blue.ppm", [0, 0, 255])] {
        fs::write(
            root.join(name),
            [b"P6\n2 2\n255\n".as_slice(), &color.repeat(4)].concat(),
        )
        .unwrap();
    }
    wav(root, "a.wav", 48000, 500000);
    wav(root, "b.wav", 48000, 600000);
    wav(root, "score.wav", 96000, 10000);
    wav(root, "effect.wav", 2400, 200000);
    fs::write(root.join("panel.ass"),"[Script Info]\nScriptType: v4.00+\nPlayResX: 64\nPlayResY: 64\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,-1,0,0,0,100,100,0,0,1,1,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,TEST\n").unwrap();
    write(
        root,
        "production.json",
        &json!({"manifest_version":"reel.manifest.v0.2",
        "profile":"animatic","timing_status":"conformed","work":"synthetic","title":"Imported fixture",
        "scenes":[{"id":"scene"}],"shots":[{"id":"shot","scene_id":"scene"}],
        "narration_cues":[{"id":"a","speaker_id":"narrator"},{"id":"b","speaker_id":"narrator"}],
        "audio_events":[{"id":"a","role":"narration","source":"a.wav","start_seconds":0},
        {"id":"b","role":"narration","source":"b.wav","start_seconds":0},
        {"id":"m","role":"music","source":"score.wav","start_seconds":0},
        {"id":"e","role":"effect","source":"effect.wav","start_seconds":0}]}),
    );
    let anchor = |cue: &str, edge: &str| json!({"kind":format!("cue-{edge}"),"cue_id":cue});
    write(
        root,
        "contract.json",
        &json!({"schema":"reel.cue-relative-assembly.v0.1","id":"scene",
        "production_manifest":"production.json","sample_rate":48000,"frame_rate":{"numerator":24,"denominator":1},
        "cues":[{"cue_id":"a","duration_samples":48000},{"cue_id":"b","duration_samples":48000}],
        "attachments":[
        {"id":"p-a","target":{"kind":"cel","shot_id":"shot","cel_id":"red"},"start":anchor("a","start"),"end":anchor("a","end")},
        {"id":"p-b","target":{"kind":"cel","shot_id":"shot","cel_id":"blue"},"start":anchor("b","start"),"end":anchor("b","end")},
        {"id":"d-a","target":{"kind":"audio","audio_event_id":"a"},"start":anchor("a","start"),"end":anchor("a","end")},
        {"id":"d-b","target":{"kind":"audio","audio_event_id":"b"},"start":anchor("b","start"),"end":anchor("b","end")},
        {"id":"m","target":{"kind":"audio","audio_event_id":"m"},"start":anchor("a","start"),"end":anchor("b","end")},
        {"id":"e","target":{"kind":"sonic","audio_event_id":"e"},"start":{"kind":"cue-start","cue_id":"a","offset_samples":19200},"end":{"kind":"cue-start","cue_id":"a","offset_samples":21600}},
        {"id":"overlay","target":{"kind":"title","title_id":"overlay"},"start":anchor("a","start"),"end":anchor("b","end")}
        ]}),
    );
    write(
        root,
        "job.json",
        &json!({"schema":"reel.scene-delivery.v0.1","id":"scene",
        "contract":reference(root,"contract.json"),"production_manifest_sha256":reference(root,"production.json")["sha256"],
        "width":64,"height":64,"max_composition_samples":480000,"pictures":[
        {"attachment_id":"p-a","source":reference(root,"red.ppm"),"kind":"still","attention":"First beat"},
        {"attachment_id":"p-b","source":reference(root,"blue.ppm"),"kind":"still","attention":"Second beat"}],
        "audio":[{"attachment_id":"d-a","source":reference(root,"a.wav"),"bus":"D","cue_id":"a"},
        {"attachment_id":"d-b","source":reference(root,"b.wav"),"bus":"D","cue_id":"b"},
        {"attachment_id":"m","source":reference(root,"score.wav"),"bus":"M","gain_db":-20},
        {"attachment_id":"e","source":reference(root,"effect.wav"),"bus":"E","gain_db":-12}],
        "external_layers":[{"attachment_id":"overlay","reason":"Editable overlay across two cels",
            "evidence":reference(root,"panel.ass"),"render_mode":"ass-overlay"}],
        "buses":{"D":{"state":"present","reason":"Fixture dialogue"},"M":{"state":"present","reason":"Quiet score"},"E":{"state":"present","reason":"Contact"}}}),
    );
    let files = [
        ("red.ppm", "picture"),
        ("blue.ppm", "picture"),
        ("a.wav", "narration"),
        ("b.wav", "narration"),
        ("score.wav", "score"),
        ("effect.wav", "sonic"),
        ("panel.ass", "vfx"),
    ];
    let slots=files.iter().map(|(name,lane)| {let r=reference(root,name);json!({"slot_id":name,"beat_id":"beat",
        "lane":lane,"disposition":"selected","selected_revision_id":"r1","revisions":[{"revision_id":"r1",
        "asset":{"logical_id":name,"sha256":r["sha256"],"cache_uri":format!("cache://sha256/{}",r["sha256"].as_str().unwrap())}}]})}).collect::<Vec<_>>();
    let events=[("a","red.ppm"),("b","blue.ppm")].map(|(cue,picture)|json!({"event_id":cue,"scene_id":"scene","language":"es",
        "narration":{"logical_id":format!("{cue}.wav"),"sha256":reference(root,&format!("{cue}.wav"))["sha256"]},
        "picture":{"logical_id":picture,"sha256":reference(root,picture)["sha256"]},
        "phrase_start_seconds":0,"phrase_end_seconds":1}));
    let graph = json!({"schema":"reel.semantic-assembly.v1","lock":{"logical_id":"original","sha256":"a".repeat(64)},
        "slots":slots,"events":events,"nodes":[{"id":"scene","inputs":[],"slots":files.map(|(file,_)|file),"events":["a","b"]}]});
    write(root, "graph.json", &graph);
    let pointer = json!({"schema":"reel.selected-pointer.v1","logical_id":"current","selected_lock":graph["lock"]});
    write(root, "pointer.json", &pointer);
    write(
        root,
        "semantic.json",
        &json!({"schema":"reel.semantic-delivery.v1","id":"import","pointer":pointer,"graph":graph,
        "target":"scene","scene_id":"scene","language":"es","scene_delivery_job":reference(root,"job.json"),
        "event_bindings":[{"event_id":"a","narration_attachment_id":"d-a","picture_attachment_id":"p-a",
            "audio_attachment_ids":["m","e"],"external_layer_attachment_ids":["overlay"]},
        {"event_id":"b","narration_attachment_id":"d-b","picture_attachment_id":"p-b",
            "audio_attachment_ids":["m"],"external_layer_attachment_ids":["overlay"]}]}),
    );
    write(
        root,
        "capture.json",
        &json!({"schema":"reel.imported-scene-source-capture.v1","episode_id":"episode","scene_id":"scene",
        "source_graph":reference(root,"graph.json"),"selected_pointer_sha256":reference(root,"pointer.json")["sha256"],
        "selected_delivery_jobs":{"es":reference(root,"job.json")},"selected_semantic_deliveries":{"es":reference(root,"semantic.json")},
        "event_cue_ids":{"a":"a","b":"b"}}),
    );
    let manifest = json!({"schema":imported_scene_proof::MANIFEST_SCHEMA,"episode_id":"episode","scene_id":"scene","language":"es",
        "source_target":"scene","source_graph":reference(root,"graph.json"),"source_selection":reference(root,"pointer.json"),
        "source_capture":reference(root,"capture.json"),"semantic_delivery":reference(root,"semantic.json"),
        "job":reference(root,"job.json"),"production":reference(root,"production.json")});
    write(root, "import.json", &manifest);
    manifest
}

#[test]
fn granular_import_checks_render_and_episode_consumption() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let mut manifest = fixture(root);
    let output = root.join("delivery");
    let native = scene_delivery::render(&root.join("job.json"), root, &output).unwrap();
    manifest["source_evidence"] = json!([
        reference(root, "semantic.json"),
        reference(root, "delivery/receipt.json")
    ]);
    write(root, "import.json", &manifest);
    assert_eq!(native.plan.rendered_external_layers, vec!["overlay"]);
    let proof =
        imported_scene_proof::verify_render(root, &parsed_ref(root, "import.json"), root, &output)
            .unwrap();
    let selected = serde_json::to_value(proof).unwrap();
    imported_scene_proof::recheck(
        root,
        &parsed_ref(root, "import.json"),
        root,
        &output,
        &selected,
    )
    .unwrap();
    assert!(
        imported_scene_proof::verify_render(root, &parsed_ref(root, "import.json"), root, &output)
            .is_err()
    );
    let mut stale = selected.clone();
    stale["master_sha256"] = json!("b".repeat(64));
    assert!(
        imported_scene_proof::recheck(
            root,
            &parsed_ref(root, "import.json"),
            root,
            &output,
            &stale
        )
        .is_err()
    );

    write(
        root,
        "template.json",
        &json!({"schema":"reel.episode-master-template.v1","template_id":"master","ordered_roles":["chapter-scenes"],"optional_roles":[]}),
    );
    write(
        root,
        "catalog.json",
        &json!({"schema":"reel.scene-template-catalog.v1","templates":[{"template_id":"master","kind":"episode-master","definition_sha256":reference(root,"template.json")["sha256"],"required_content_keys":[]}]}),
    );
    write(
        root,
        "episode.json",
        &json!({"schema":"reel.episode-authoring.v1","episode_id":"episode","season_id":"season",
        "authoring_state":"ready-for-private-build","master_template_id":"master","scene_policy_id":"ordinary","score_palette":[],"scene_ids":["scene"],"presentation":[]}),
    );
    for (name, scope) in [("season.json", "season"), ("bindings.json", "episode")] {
        write(
            root,
            name,
            &json!({"schema":"reel.scene-asset-bindings.v1","scope_id":scope,"assets":{}}),
        );
    }
    let mut conform = json!({"schema":"reel.episode-conform.v1","episode_id":"episode","language":"es","output_sample_rate":48000,
        "catalog":reference(root,"catalog.json"),"master_template_definition":reference(root,"template.json"),
        "episode":reference(root,"episode.json"),"season_bindings":reference(root,"season.json"),"episode_bindings":reference(root,"bindings.json"),
        "segments":[{"kind":"scene","id":"scene","master":reference(root,"delivery/master.mkv"),
        "source_receipt":reference(root,"delivery/imported-scene-source-proof.json"),"delivery_job":reference(root,"job.json"),
        "delivery_receipt":reference(root,"delivery/receipt.json"),"imported_source_manifest":reference(root,"import.json")}]});
    write(root, "conform.json", &conform);
    let episode = reel::episode_conform::build(
        &root.join("conform.json"),
        root,
        root,
        &root.join("episode-output"),
    )
    .unwrap();
    assert_eq!(episode.total_frames, 48);
    assert_eq!(episode.total_samples, 96000);
    assert!(episode.decoded_master_matches_ordered_segments);
    let accepted_conform = conform.clone();
    exercise_encoding_successor(root, &manifest, &accepted_conform);
    conform["segments"][0]
        .as_object_mut()
        .unwrap()
        .remove("imported_source_manifest");
    write(root, "no-gate.json", &conform);
    assert!(
        reel::episode_conform::build(
            &root.join("no-gate.json"),
            root,
            root,
            &root.join("invalid-episode")
        )
        .is_err()
    );

    let original_capture: Value =
        serde_json::from_slice(&fs::read(root.join("capture.json")).unwrap()).unwrap();
    let mut wrong_cue = original_capture.clone();
    wrong_cue["event_cue_ids"]["a"] = json!("b");
    write(root, "wrong-cue-capture.json", &wrong_cue);
    let mut wrong_cue_manifest = manifest.clone();
    wrong_cue_manifest["source_capture"] = reference(root, "wrong-cue-capture.json");
    write(root, "wrong-cue-import.json", &wrong_cue_manifest);
    let error = imported_scene_proof::verify_render(
        root,
        &parsed_ref(root, "wrong-cue-import.json"),
        root,
        &output,
    )
    .unwrap_err();
    assert!(error.to_string().contains("original selected D cue"));

    let mut foreign_proof = selected.clone();
    foreign_proof["episode_id"] = json!("another-episode");
    write(root, "foreign-proof.json", &foreign_proof);
    let mut foreign_conform = accepted_conform.clone();
    foreign_conform["segments"][0]["source_receipt"] = reference(root, "foreign-proof.json");
    write(root, "foreign-conform.json", &foreign_conform);
    assert!(
        reel::episode_conform::build(
            &root.join("foreign-conform.json"),
            root,
            root,
            &root.join("foreign-episode")
        )
        .is_err()
    );

    let original_semantic: Value =
        serde_json::from_slice(&fs::read(root.join("semantic.json")).unwrap()).unwrap();
    let mut changed = original_semantic.clone();
    changed["event_bindings"][0]["audio_attachment_ids"] = json!(["m"]);
    changed["event_bindings"][1]["audio_attachment_ids"] = json!(["m", "e"]);
    write(root, "changed-semantic.json", &changed);
    let mut changed_manifest = manifest.clone();
    changed_manifest["semantic_delivery"] = reference(root, "changed-semantic.json");
    write(root, "changed-import.json", &changed_manifest);
    assert!(
        imported_scene_proof::verify_inputs(root, &parsed_ref(root, "changed-import.json"))
            .is_err()
    );
    changed = original_semantic.clone();
    changed["event_bindings"].as_array_mut().unwrap().pop();
    write(root, "missing-event-semantic.json", &changed);
    let mut changed_capture = original_capture.clone();
    changed_capture["selected_semantic_deliveries"]["es"] =
        reference(root, "missing-event-semantic.json");
    write(root, "missing-event-capture.json", &changed_capture);
    changed_manifest = manifest.clone();
    changed_manifest["semantic_delivery"] = reference(root, "missing-event-semantic.json");
    changed_manifest["source_capture"] = reference(root, "missing-event-capture.json");
    write(root, "missing-event-import.json", &changed_manifest);
    assert!(
        imported_scene_proof::verify_inputs(root, &parsed_ref(root, "missing-event-import.json"))
            .is_err()
    );

    let mut job: Value = serde_json::from_slice(&fs::read(root.join("job.json")).unwrap()).unwrap();
    job["audio"][3]["source_start_sample"] = json!(100);
    write(root, "changed-job.json", &job);
    changed_manifest = manifest.clone();
    changed_manifest["job"] = reference(root, "changed-job.json");
    write(root, "changed-job-import.json", &changed_manifest);
    assert!(
        imported_scene_proof::verify_inputs(root, &parsed_ref(root, "changed-job-import.json"))
            .is_err()
    );
    fs::write(output.join("master.mkv"), b"tampered output").unwrap();
    assert!(
        imported_scene_proof::recheck(
            root,
            &parsed_ref(root, "import.json"),
            root,
            &output,
            &selected
        )
        .is_err()
    );
}

fn exercise_encoding_successor(root: &Path, original_manifest: &Value, original_conform: &Value) {
    let original_job: Value =
        serde_json::from_slice(&fs::read(root.join("job.json")).unwrap()).unwrap();
    let original_semantic: Value =
        serde_json::from_slice(&fs::read(root.join("semantic.json")).unwrap()).unwrap();
    let mut derived = original_job.clone();
    derived["still_sequence_encoding"] = json!("h264-lossless");
    write(root, "derived-job.json", &derived);
    let mut semantic = original_semantic.clone();
    semantic["scene_delivery_job"] = reference(root, "derived-job.json");
    write(root, "derived-semantic.json", &semantic);
    let descriptor = json!({"schema":"reel.imported-scene-encoding-successor.v1",
        "original_manifest":reference(root,"import.json"),
        "original_receipt":reference(root,"delivery/receipt.json"),
        "original_cached_semantic":reference(root,"semantic.json")});
    write(root, "encoding.json", &descriptor);
    let mut manifest = original_manifest.clone();
    manifest["schema"] = json!(imported_scene_proof::ENCODING_MANIFEST_SCHEMA);
    manifest["encoding_successor"] = reference(root, "encoding.json");
    manifest["job"] = reference(root, "derived-job.json");
    manifest["semantic_delivery"] = reference(root, "derived-semantic.json");
    write(root, "encoding-import.json", &manifest);
    imported_scene_proof::verify_inputs(root, &parsed_ref(root, "encoding-import.json")).unwrap();
    imported_scene_proof::check_inputs(root, &parsed_ref(root, "encoding-import.json"), root)
        .unwrap();
    let output = root.join("derived-delivery");
    scene_delivery::render(&root.join("derived-job.json"), root, &output).unwrap();
    let proof = imported_scene_proof::verify_render(
        root,
        &parsed_ref(root, "encoding-import.json"),
        root,
        &output,
    )
    .unwrap();
    assert_eq!(
        proof.original_selected_job_sha256.as_deref(),
        reference(root, "job.json")["sha256"].as_str()
    );
    assert_eq!(
        proof.derived_render_job_sha256.as_deref(),
        reference(root, "derived-job.json")["sha256"].as_str()
    );
    assert!(proof.encoding_successor_sha256.is_some());
    let selected = serde_json::to_value(proof).unwrap();
    imported_scene_proof::recheck(
        root,
        &parsed_ref(root, "encoding-import.json"),
        root,
        &output,
        &selected,
    )
    .unwrap();
    let mut partial = selected.clone();
    partial
        .as_object_mut()
        .unwrap()
        .remove("original_selected_job_sha256");
    assert!(
        imported_scene_proof::recheck(
            root,
            &parsed_ref(root, "encoding-import.json"),
            root,
            &output,
            &partial
        )
        .is_err()
    );
    let mut conform = original_conform.clone();
    conform["segments"][0]["master"] = reference(root, "derived-delivery/master.mkv");
    conform["segments"][0]["source_receipt"] =
        reference(root, "derived-delivery/imported-scene-source-proof.json");
    conform["segments"][0]["delivery_job"] = reference(root, "derived-job.json");
    conform["segments"][0]["delivery_receipt"] = reference(root, "derived-delivery/receipt.json");
    conform["segments"][0]["imported_source_manifest"] = reference(root, "encoding-import.json");
    write(root, "derived-conform.json", &conform);
    let film = reel::episode_conform::build(
        &root.join("derived-conform.json"),
        root,
        root,
        &root.join("derived-episode"),
    )
    .unwrap();
    assert_eq!(film.total_frames, 48);
    assert!(film.decoded_master_matches_ordered_segments);

    for (index, pointer, value) in [
        (0, "/audio/0/gain_db", json!(-6)),
        (1, "/audio/3/source_start_sample", json!(100)),
        (2, "/audio/2/bus", json!("E")),
        (3, "/pictures/0/source/sha256", json!("b".repeat(64))),
        (4, "/contract/sha256", json!("b".repeat(64))),
        (5, "/production_manifest_sha256", json!("b".repeat(64))),
    ] {
        let mut changed = derived.clone();
        if let Some(target) = changed.pointer_mut(pointer) {
            *target = value;
        } else {
            changed["audio"][0]["gain_db"] = value;
        }
        let job_name = format!("bad-encoding-job-{index}.json");
        write(root, &job_name, &changed);
        let mut bad_semantic = semantic.clone();
        bad_semantic["scene_delivery_job"] = reference(root, &job_name);
        let semantic_name = format!("bad-encoding-semantic-{index}.json");
        write(root, &semantic_name, &bad_semantic);
        let mut bad = manifest.clone();
        bad["job"] = reference(root, &job_name);
        bad["semantic_delivery"] = reference(root, &semantic_name);
        let name = format!("bad-encoding-import-{index}.json");
        write(root, &name, &bad);
        assert!(imported_scene_proof::verify_inputs(root, &parsed_ref(root, &name)).is_err());
    }
    for field in ["language", "scene_id"] {
        let mut bad = manifest.clone();
        bad[field] = json!("foreign");
        write(root, "foreign-encoding.json", &bad);
        assert!(
            imported_scene_proof::verify_inputs(root, &parsed_ref(root, "foreign-encoding.json"))
                .is_err()
        );
    }
    let mut stale_semantic = original_semantic.clone();
    let mut fabricated_semantic = original_semantic.clone();
    fabricated_semantic["id"] = json!("unretained-historical-semantic");
    write(
        root,
        "unretained-original-semantic.json",
        &fabricated_semantic,
    );
    let mut fabricated_receipt: Value =
        serde_json::from_slice(&fs::read(root.join("delivery/receipt.json")).unwrap()).unwrap();
    fabricated_receipt["tool_version"] = json!("unretained-history");
    write(
        root,
        "unretained-original-receipt.json",
        &fabricated_receipt,
    );
    for (key, name) in [
        (
            "original_cached_semantic",
            "unretained-original-semantic.json",
        ),
        ("original_receipt", "unretained-original-receipt.json"),
    ] {
        let mut d = descriptor.clone();
        d[key] = reference(root, name);
        write(root, "unretained-encoding.json", &d);
        let mut bad = manifest.clone();
        bad["encoding_successor"] = reference(root, "unretained-encoding.json");
        write(root, "unretained-encoding-import.json", &bad);
        let error = imported_scene_proof::verify_inputs(
            root,
            &parsed_ref(root, "unretained-encoding-import.json"),
        )
        .err()
        .unwrap();
        assert!(
            error
                .to_string()
                .contains("not pinned by original manifest")
        );
    }
    stale_semantic["event_bindings"][0]["audio_attachment_ids"] = json!([]);
    write(root, "stale-original-semantic.json", &stale_semantic);
    let mut stale_receipt: Value =
        serde_json::from_slice(&fs::read(root.join("delivery/receipt.json")).unwrap()).unwrap();
    stale_receipt["plan"]["job_sha256"] = json!("b".repeat(64));
    write(root, "stale-original-receipt.json", &stale_receipt);
    for (key, name) in [
        ("original_cached_semantic", "stale-original-semantic.json"),
        ("original_receipt", "stale-original-receipt.json"),
    ] {
        let mut bad_descriptor = descriptor.clone();
        bad_descriptor[key] = reference(root, name);
        write(root, "stale-encoding.json", &bad_descriptor);
        let mut bad = manifest.clone();
        bad["encoding_successor"] = reference(root, "stale-encoding.json");
        write(root, "stale-encoding-import.json", &bad);
        assert!(
            imported_scene_proof::verify_inputs(
                root,
                &parsed_ref(root, "stale-encoding-import.json")
            )
            .is_err()
        );
    }
    stale_receipt["plan"]["job_sha256"] = reference(root, "job.json")["sha256"].clone();
    stale_receipt["plan"]["compiled_sha256"] = json!("b".repeat(64));
    write(root, "stale-compiled-receipt.json", &stale_receipt);
    let mut bad_descriptor = descriptor.clone();
    bad_descriptor["original_receipt"] = reference(root, "stale-compiled-receipt.json");
    write(root, "stale-compiled-encoding.json", &bad_descriptor);
    let mut bad = manifest.clone();
    bad["encoding_successor"] = reference(root, "stale-compiled-encoding.json");
    write(root, "stale-compiled-import.json", &bad);
    assert!(
        imported_scene_proof::verify_render(
            root,
            &parsed_ref(root, "stale-compiled-import.json"),
            root,
            &output
        )
        .is_err()
    );
}
