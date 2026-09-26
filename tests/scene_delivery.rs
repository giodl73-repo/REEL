use reel::scene_delivery;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

#[test]
#[ignore = "requires FFmpeg; synthetic episode and review integration"]
fn real_episode_consumption_boundaries_and_controlled_review() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    for name in ["a", "b"] {
        let dir = root.join(name);
        fs::create_dir(&dir).unwrap();
        let mut j = fixture(&dir);
        let mut c: Value =
            serde_json::from_slice(&fs::read(dir.join("contract.json")).unwrap()).unwrap();
        c["id"] = json!(name);
        write_json(&dir.join("contract.json"), &c);
        j["id"] = json!(name);
        j["contract"] = file(&dir, "contract.json");
        if name == "b" {
            fs::write(
                dir.join("black.ppm"),
                [b"P6\n2 2\n255\n".as_slice(), &[0, 0, 0].repeat(4)].concat(),
            )
            .unwrap();
            j["pictures"][0]["source"] = file(&dir, "black.ppm");
        }
        write_json(&dir.join("job.json"), &j);
        scene_delivery::render(&dir.join("job.json"), &dir, &dir.join("delivery")).unwrap();
    }
    let scene = |name: &str| json!({"id":name,"job":file(root,&format!("{name}/job.json")),"asset_root":name,"receipt":file(root,&format!("{name}/delivery/receipt.json"))});
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(root.join("a/delivery/master.mkv"))
        .arg("-i")
        .arg(root.join("b/delivery/master.mkv"))
        .args([
            "-filter_complex",
            "[0:v][1:v]concat=n=2:v=1:a=0[v];[0:a][1:a]concat=n=2:v=0:a=1[a]",
            "-map",
            "[v]",
            "-map",
            "[a]",
            "-c:v",
            "ffv1",
            "-pix_fmt",
            "yuv444p",
            "-c:a",
            "pcm_s24le",
        ])
        .arg(root.join("master.mkv"))
        .status()
        .unwrap();
    assert!(status.success());
    let contract = root.join("episode.json");
    let mut c = json!({"schema":"reel.episode-delivery.v0.1","id":"episode","scenes":[scene("a"),scene("b")],"master":file(root,"master.mkv"),"layers":[],"boundary_decisions":[]});
    write_json(&contract, &c);
    let report = reel::episode_delivery::check(&contract, root).unwrap();
    assert!(report.content_verified);
    assert!(!report.passed);
    assert_eq!(report.scenes[1].start_frame, 48);
    assert_eq!(report.scenes[1].start_sample, 96000);
    assert!(
        report
            .boundary_findings
            .iter()
            .any(|f| f.code == "black-at-cut")
    );
    fs::write(
        root.join("intent.txt"),
        "Synthetic intentional black opening",
    )
    .unwrap();
    c["boundary_decisions"] = json!(report.boundary_findings.iter().map(|f| json!({"left_scene":"a","right_scene":"b","code":f.code,"left_receipt_sha256":file(root,"a/delivery/receipt.json")["sha256"],"right_receipt_sha256":file(root,"b/delivery/receipt.json")["sha256"],"owner":"test-editor","reason":"Intentional synthetic black opening and contact signal","evidence":file(root,"intent.txt")})).collect::<Vec<_>>());
    write_json(&contract, &c);
    assert!(
        reel::episode_delivery::check(&contract, root)
            .unwrap()
            .passed
    );
    c["scenes"] = json!([scene("b"), scene("a")]);
    write_json(&contract, &c);
    assert!(
        reel::episode_delivery::check(&contract, root)
            .unwrap_err()
            .to_string()
            .contains("does not consume")
    );
    c["scenes"] = json!([scene("a"), scene("b")]);
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(root.join("master.mkv"))
        .args(["-c:v", "copy", "-af", "volume=0", "-c:a", "pcm_s24le"])
        .arg(root.join("muted.mkv"))
        .status()
        .unwrap();
    assert!(status.success());
    c["master"] = file(root, "muted.mkv");
    write_json(&contract, &c);
    assert!(
        reel::episode_delivery::check(&contract, root)
            .unwrap_err()
            .to_string()
            .contains("does not consume")
    );
    let review = root.join("review.json");
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-itsoffset", "0.25", "-i"])
        .arg(root.join("master.mkv"))
        .args(["-c", "copy", "-copyts"])
        .arg(root.join("shifted.mkv"))
        .status()
        .unwrap();
    assert!(status.success());
    c["master"] = file(root, "shifted.mkv");
    write_json(&contract, &c);
    assert!(
        reel::episode_delivery::check(&contract, root)
            .unwrap_err()
            .to_string()
            .contains("timestamp gap/offset")
    );
    write_json(
        &review,
        &json!({"schema":"reel.scene-review.v0.1","baseline":scene("a"),"candidate":scene("b")}),
    );
    let out = root.join("comparison");
    let r = reel::scene_review::render(&review, root, &out).unwrap();
    assert_eq!(r.outputs.len(), 15);
    assert!(out.join("a-dialogue.mp4").is_file());
    assert!(out.join("b-no-score.mp4").is_file());
    reel::scene_review::check(&review, root, &out).unwrap();
    assert!(reel::scene_review::render(&review, root, &out).is_err());
    fs::write(out.join("a-dialogue.mp4"), b"tampered").unwrap();
    assert!(reel::scene_review::check(&review, root, &out).is_err());
    // A resealed candidate with different dialogue must fail before review output.
    wav(&root.join("b/a.wav"), 48001, 100000);
    let mut j: Value = serde_json::from_slice(&fs::read(root.join("b/job.json")).unwrap()).unwrap();
    j["audio"][0]["source"] = file(&root.join("b"), "a.wav");
    write_json(&root.join("b/job.json"), &j);
    scene_delivery::render(
        &root.join("b/job.json"),
        &root.join("b"),
        &root.join("b/recast"),
    )
    .unwrap();
    let mut changed = scene("b");
    changed["receipt"] = file(root, "b/recast/receipt.json");
    write_json(
        &review,
        &json!({"schema":"reel.scene-review.v0.1","baseline":scene("a"),"candidate":changed}),
    );
    assert!(
        reel::scene_review::render(&review, root, &root.join("invalid-review"))
            .unwrap_err()
            .to_string()
            .contains("identical decoded dialogue")
    );
    assert!(!root.join("invalid-review").exists());
}

fn write_json(path: &Path, v: &Value) {
    fs::write(path, serde_json::to_vec_pretty(v).unwrap()).unwrap();
}
fn file(root: &Path, name: &str) -> Value {
    let b = fs::read(root.join(name)).unwrap();
    json!({"path":name,"sha256":Sha256::digest(&b).iter().map(|b| format!("{b:02x}")).collect::<String>(),"bytes":b.len()})
}
fn wav(path: &Path, samples: u32, signal: i32) {
    let size = samples * 6;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((36 + size).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(48000u32.to_le_bytes());
    b.extend(288000u32.to_le_bytes());
    b.extend(6u16.to_le_bytes());
    b.extend(24u16.to_le_bytes());
    b.extend(b"data");
    b.extend(size.to_le_bytes());
    for _ in 0..samples {
        for _ in 0..2 {
            b.extend(&signal.to_le_bytes()[..3]);
        }
    }
    fs::write(path, b).unwrap();
}
fn fixture(root: &Path) -> Value {
    fs::write(
        root.join("red.ppm"),
        [b"P6\n2 2\n255\n".as_slice(), &[255, 0, 0].repeat(4)].concat(),
    )
    .unwrap();
    fs::write(
        root.join("blue.ppm"),
        [b"P6\n2 2\n255\n".as_slice(), &[0, 0, 255].repeat(4)].concat(),
    )
    .unwrap();
    wav(&root.join("a.wav"), 48001, 0);
    wav(&root.join("b.wav"), 47999, 0);
    wav(&root.join("effect.wav"), 16, 2000000);
    write_json(
        &root.join("production.json"),
        &json!({"manifest_version":"reel.manifest.v0.2","profile":"animatic","timing_status":"conformed","work":"synthetic","title":"Synthetic scene","scenes":[{"id":"scene"}],"shots":[{"id":"shot","scene_id":"scene"}],"narration_cues":[{"id":"a","speaker_id":"narrator"},{"id":"b","speaker_id":"narrator"}],"audio_events":[{"id":"a","role":"narration","source":"a.wav","start_seconds":0},{"id":"b","role":"narration","source":"b.wav","start_seconds":0},{"id":"e","role":"effect","source":"effect.wav","start_seconds":0}]}),
    );
    let anchor = |id: &str, edge: &str| json!({"kind":format!("cue-{edge}"),"cue_id":id});
    let c = json!({"schema":"reel.cue-relative-assembly.v0.1","id":"scene","production_manifest":"production.json","sample_rate":48000,"frame_rate":{"numerator":24,"denominator":1},"cues":[{"cue_id":"a","duration_samples":48001},{"cue_id":"b","duration_samples":47999}],"attachments":[
        {"id":"p-a","target":{"kind":"cel","shot_id":"shot","cel_id":"red"},"start":anchor("a","start"),"end":anchor("a","end")},
        {"id":"p-b","target":{"kind":"cel","shot_id":"shot","cel_id":"blue"},"start":anchor("b","start"),"end":anchor("b","end")},
        {"id":"d-a","target":{"kind":"audio","audio_event_id":"a"},"start":anchor("a","start"),"end":anchor("a","end")},
        {"id":"d-b","target":{"kind":"audio","audio_event_id":"b"},"start":anchor("b","start"),"end":anchor("b","end")},
        {"id":"e","target":{"kind":"sonic","audio_event_id":"e"},"start":{"kind":"cue-start","cue_id":"a","offset_samples":123},"end":{"kind":"cue-start","cue_id":"a","offset_samples":139}}
    ]});
    write_json(&root.join("contract.json"), &c);
    let j = json!({"schema":"reel.scene-delivery.v0.1","id":"scene","contract":file(root,"contract.json"),"production_manifest_sha256":file(root,"production.json")["sha256"],"width":64,"height":64,"max_composition_samples":480000,"pictures":[{"attachment_id":"p-a","source":file(root,"red.ppm"),"kind":"still","attention":"Establish the space"},{"attachment_id":"p-b","source":file(root,"blue.ppm"),"kind":"still","attention":"Redirect attention at the native turn"}],"audio":[{"attachment_id":"d-a","source":file(root,"a.wav"),"bus":"D","cue_id":"a"},{"attachment_id":"d-b","source":file(root,"b.wav"),"bus":"D","cue_id":"b"},{"attachment_id":"e","source":file(root,"effect.wav"),"bus":"E"}],"buses":{"D":{"state":"present","reason":"Native dialogue"},"M":{"state":"intentional-silence","reason":"No music in this control"},"E":{"state":"present","reason":"Exact contact event"}}});
    write_json(&root.join("job.json"), &j);
    j
}

#[test]
fn selected_ass_layer_changes_rendered_pixels_and_is_checked() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let mut job = fixture(root);
    let mut contract: Value =
        serde_json::from_slice(&fs::read(root.join("contract.json")).unwrap()).unwrap();
    contract["attachments"].as_array_mut().unwrap().push(json!({
        "id":"editable-title", "target":{"kind":"title","title_id":"example"},
        "start":{"kind":"cue-start","cue_id":"a","offset_samples":0},
        "end":{"kind":"cue-end","cue_id":"b","offset_samples":0}
    }));
    write_json(&root.join("contract.json"), &contract);
    fs::write(root.join("panel.ass"), "[Script Info]\nScriptType: v4.00+\nPlayResX: 64\nPlayResY: 64\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,30,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,-1,0,0,0,100,100,0,0,1,1,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,TEST\n").unwrap();
    job["contract"] = file(root, "contract.json");
    job["external_layers"] = json!([{
        "attachment_id":"editable-title", "reason":"Selected editable title",
        "evidence":file(root,"panel.ass"), "render_mode":"ass-overlay"
    }]);
    write_json(&root.join("job.json"), &job);
    let output = root.join("rendered");
    let receipt = scene_delivery::render(&root.join("job.json"), root, &output).unwrap();
    assert_eq!(
        receipt.plan.rendered_external_layers,
        vec!["editable-title"]
    );
    assert!(output.join("clean-picture.mkv").exists());
    assert!(output.join("presentation.ass").exists());
    scene_delivery::check(&root.join("job.json"), root, &output).unwrap();
    fs::write(output.join("presentation.ass"), b"tampered").unwrap();
    assert!(scene_delivery::check(&root.join("job.json"), root, &output).is_err());
}

#[test]
fn timed_alpha_effect_changes_only_its_selected_frames() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let mut job = fixture(root);
    let mut production: Value =
        serde_json::from_slice(&fs::read(root.join("production.json")).unwrap()).unwrap();
    production["shots"][0]["effect_passes"] = json!([{
        "id":"glow", "color":{"path":"red.ppm","sha256":file(root,"red.ppm")["sha256"]},
        "matte":{"path":"blue.ppm","sha256":file(root,"blue.ppm")["sha256"]},
        "alpha_mode":"separate-matte", "composite_operator":"over", "color_space":"srgb",
        "alpha_mode_detail":"straight", "timing_fps":24, "duration_frames":48,
        "placement":{"space":"normalized","x":0,"y":0,"width":1,"height":1},
        "visible_start_frame":12, "visible_end_frame":36
    }]);
    write_json(&root.join("production.json"), &production);
    job["production_manifest_sha256"] = file(root, "production.json")["sha256"].clone();
    let mut contract: Value =
        serde_json::from_slice(&fs::read(root.join("contract.json")).unwrap()).unwrap();
    contract["attachments"].as_array_mut().unwrap().push(json!({
        "id":"timed-effect", "target":{"kind":"effect","shot_id":"shot","effect_pass_id":"glow"},
        "start":{"kind":"cue-start","cue_id":"a","offset_samples":24000},
        "end":{"kind":"cue-start","cue_id":"b","offset_samples":24000}
    }));
    write_json(&root.join("contract.json"), &contract);
    let effect = root.join("effect.mkv");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "color=c=lime@0.75:s=64x64:r=24:d=1.1,format=yuva444p",
            "-c:v",
            "ffv1",
            "-pix_fmt",
            "yuva444p",
            "-frames:v",
            "25",
            "-y",
        ])
        .arg(&effect)
        .status()
        .unwrap();
    assert!(status.success());
    job["contract"] = file(root, "contract.json");
    job["external_layers"] = json!([{
        "attachment_id":"timed-effect", "reason":"Selected effect span",
        "evidence":file(root,"effect.mkv"), "render_mode":"timed-video-overlay"
    }]);
    write_json(&root.join("job.json"), &job);
    let selected_effect = job["external_layers"][0]["evidence"].clone();
    job["external_layers"][0]["evidence"] = file(root, "red.ppm");
    write_json(&root.join("job.json"), &job);
    assert!(scene_delivery::plan(&root.join("job.json"), root).is_err());
    job["external_layers"][0]["evidence"] = selected_effect;
    write_json(&root.join("job.json"), &job);
    let selected_contract = contract.clone();
    contract["attachments"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()["start"] = json!({"kind":"cue-start","cue_id":"a","offset_samples":24001});
    contract["attachments"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()["end"] = json!({"kind":"cue-start","cue_id":"a","offset_samples":24002});
    write_json(&root.join("contract.json"), &contract);
    job["contract"] = file(root, "contract.json");
    write_json(&root.join("job.json"), &job);
    assert!(
        scene_delivery::plan(&root.join("job.json"), root)
            .unwrap_err()
            .to_string()
            .contains("positive span")
    );
    write_json(&root.join("contract.json"), &selected_contract);
    job["contract"] = file(root, "contract.json");
    write_json(&root.join("job.json"), &job);
    contract = selected_contract.clone();
    contract["attachments"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()["end"] = json!({"kind":"cue-end","cue_id":"b"});
    write_json(&root.join("contract.json"), &contract);
    job["contract"] = file(root, "contract.json");
    job["pictures"][0]["delivery_frame_count"] = json!(24);
    job["pictures"][1]["delivery_frame_count"] = json!(23);
    write_json(&root.join("job.json"), &job);
    let (_, clipped_plan) = scene_delivery::plan(&root.join("job.json"), root).unwrap();
    assert_eq!(clipped_plan.external_layer_spans[0].end_sample, 96000);
    assert_eq!(clipped_plan.external_layer_spans[0].end_frame, 47);
    contract = selected_contract.clone();
    let effect_attachment = contract["attachments"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    effect_attachment["start"] = json!({"kind":"cue-start","cue_id":"b"});
    effect_attachment["end"] = json!({"kind":"cue-start","cue_id":"b","offset_samples":24000});
    write_json(&root.join("contract.json"), &contract);
    job["contract"] = file(root, "contract.json");
    job["pictures"][0]["delivery_frame_count"] = json!(26);
    job["pictures"][1]["delivery_frame_count"] = json!(22);
    write_json(&root.join("job.json"), &job);
    let (_, clipped_start) = scene_delivery::plan(&root.join("job.json"), root).unwrap();
    assert_eq!(clipped_start.external_layer_spans[0].start_sample, 48001);
    assert_eq!(clipped_start.external_layer_spans[0].start_frame, 26);
    contract["attachments"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()["start"] = json!({"kind":"cue-start","cue_id":"b","offset_samples":1});
    write_json(&root.join("contract.json"), &contract);
    job["contract"] = file(root, "contract.json");
    job["pictures"][0]["delivery_frame_count"] = json!(24);
    job["pictures"][1]["delivery_frame_count"] = json!(24);
    write_json(&root.join("job.json"), &job);
    let (_, fractional_start) = scene_delivery::plan(&root.join("job.json"), root).unwrap();
    assert_eq!(fractional_start.external_layer_spans[0].start_sample, 48002);
    assert_eq!(fractional_start.external_layer_spans[0].start_frame, 25);
    contract["attachments"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()["start"] = json!({"kind":"cue-start","cue_id":"a","offset_samples":24000});
    contract["attachments"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()["end"] = json!({"kind":"cue-start","cue_id":"b"});
    write_json(&root.join("contract.json"), &contract);
    job["contract"] = file(root, "contract.json");
    job["pictures"][0]["delivery_frame_count"] = json!(25);
    job["pictures"][1]["delivery_frame_count"] = json!(23);
    write_json(&root.join("job.json"), &job);
    let (_, fractional_end) = scene_delivery::plan(&root.join("job.json"), root).unwrap();
    assert_eq!(fractional_end.external_layer_spans[0].end_sample, 48001);
    assert_eq!(fractional_end.external_layer_spans[0].end_frame, 25);
    write_json(&root.join("contract.json"), &selected_contract);
    job["contract"] = file(root, "contract.json");
    job["pictures"][0]
        .as_object_mut()
        .unwrap()
        .remove("delivery_frame_count");
    job["pictures"][1]
        .as_object_mut()
        .unwrap()
        .remove("delivery_frame_count");
    write_json(&root.join("job.json"), &job);
    let output = root.join("rendered");
    let receipt = scene_delivery::render(&root.join("job.json"), root, &output).unwrap();
    assert_eq!(receipt.plan.rendered_external_layers, vec!["timed-effect"]);
    assert_eq!(receipt.plan.external_layer_spans[0].start_frame, 12);
    assert_eq!(receipt.plan.external_layer_spans[0].end_frame, 37);
    scene_delivery::check(&root.join("job.json"), root, &output).unwrap();
    fs::write(output.join("selected-overlay.mkv"), b"tampered").unwrap();
    assert!(scene_delivery::check(&root.join("job.json"), root, &output).is_err());

    write_json(
        &root.join("selected-effect.json"),
        &json!({"kind":"source-effect"}),
    );
    let derivation = json!({
        "schema":"reel.timed-overlay-derivation.v1",
        "selected_evidence":file(root,"selected-effect.json"),
        "output":file(root,"effect.mkv"),
        "inputs":[file(root,"red.ppm")],
        "recipe":{"kind":"synthetic-alpha-plate","frames":25}
    });
    write_json(&root.join("derivation.json"), &derivation);
    job["external_layers"][0]["evidence"] = file(root, "selected-effect.json");
    job["external_layers"][0]["render_source"] = file(root, "effect.mkv");
    job["external_layers"][0]["derivation_receipt"] = file(root, "derivation.json");
    write_json(&root.join("job.json"), &job);
    let derived_output = root.join("rendered-derived");
    scene_delivery::render(&root.join("job.json"), root, &derived_output).unwrap();
    scene_delivery::check(&root.join("job.json"), root, &derived_output).unwrap();
    let mut bad_recipe = derivation.clone();
    bad_recipe["recipe"] = json!("not a recipe object");
    write_json(&root.join("derivation.json"), &bad_recipe);
    job["external_layers"][0]["derivation_receipt"] = file(root, "derivation.json");
    write_json(&root.join("job.json"), &job);
    assert!(scene_delivery::plan(&root.join("job.json"), root).is_err());
    let mut bad = derivation;
    bad["output"]["sha256"] = json!("0".repeat(64));
    write_json(&root.join("derivation.json"), &bad);
    job["external_layers"][0]["derivation_receipt"] = file(root, "derivation.json");
    write_json(&root.join("job.json"), &job);
    assert!(scene_delivery::plan(&root.join("job.json"), root).is_err());
}
#[test]
fn plan_uses_compiled_samples_and_one_global_frame_partition() {
    let t = tempfile::tempdir().unwrap();
    fixture(t.path());
    let (_, p) = scene_delivery::plan(&t.path().join("job.json"), t.path()).unwrap();
    assert_eq!(p.duration_samples, 96000);
    assert_eq!(p.frame_count, 48);
    assert_eq!(p.pictures[0].end_sample, 48001);
    assert_eq!(p.pictures[0].end_frame, p.pictures[1].start_frame);
    assert_eq!(p.audio[2].start_sample, 123);
}
#[test]
fn recorded_picture_frame_counts_preserve_selected_cut_boundary() {
    let t = tempfile::tempdir().unwrap();
    let mut job = fixture(t.path());
    job["pictures"][0]["delivery_frame_count"] = json!(25);
    job["pictures"][1]["delivery_frame_count"] = json!(23);
    write_json(&t.path().join("job.json"), &job);
    let (_, plan) = scene_delivery::plan(&t.path().join("job.json"), t.path()).unwrap();
    assert_eq!(plan.duration_samples, 96000);
    assert_eq!(plan.frame_count, 48);
    assert_eq!(plan.pictures[0].end_sample, 48001);
    assert_eq!(plan.pictures[0].end_frame, 25);
    assert_eq!(plan.pictures[1].start_frame, 25);
    assert_eq!(plan.pictures[1].end_frame, 48);

    job["pictures"][1]
        .as_object_mut()
        .unwrap()
        .remove("delivery_frame_count");
    write_json(&t.path().join("job.json"), &job);
    assert!(
        scene_delivery::plan(&t.path().join("job.json"), t.path())
            .unwrap_err()
            .to_string()
            .contains("every picture")
    );
}
#[test]
fn rejects_missing_bus_audio_and_held_mix() {
    let t = tempfile::tempdir().unwrap();
    let mut j = fixture(t.path());
    j["audio"].as_array_mut().unwrap().pop();
    write_json(&t.path().join("job.json"), &j);
    assert!(
        scene_delivery::plan(&t.path().join("job.json"), t.path())
            .unwrap_err()
            .to_string()
            .contains("policy")
    );
    let mut j = fixture(t.path());
    j["buses"]["M"]["state"] = json!("held");
    write_json(&t.path().join("job.json"), &j);
    assert!(
        scene_delivery::plan(&t.path().join("job.json"), t.path())
            .unwrap_err()
            .to_string()
            .contains("held")
    );
}
#[test]
fn rejects_fake_composition_changes_and_requires_specific_stillness_exception() {
    let t = tempfile::tempdir().unwrap();
    let mut j = fixture(t.path());
    j["pictures"][1]["source"] = j["pictures"][0]["source"].clone();
    j["max_composition_samples"] = json!(60000);
    write_json(&t.path().join("job.json"), &j);
    assert!(
        scene_delivery::plan(&t.path().join("job.json"), t.path())
            .unwrap_err()
            .to_string()
            .contains("unchanged composition")
    );
    j["pictures"][1]["stillness_exception"] =
        json!({"reason":"Protected afterimage","decision":"consumer-decision-123"});
    write_json(&t.path().join("job.json"), &j);
    scene_delivery::plan(&t.path().join("job.json"), t.path()).unwrap();
}
#[test]
fn selected_still_motion_is_scoped_and_validated() {
    let t = tempfile::tempdir().unwrap();
    let mut j = fixture(t.path());
    let motion = json!({
        "kind":"zoompan", "scale_width":128, "scale_height":128,
        "crop_width":64, "crop_height":64,
        "zoom_step":0.0001, "zoom_max":1.015
    });
    j["pictures"][0]["motion"] = motion.clone();
    write_json(&t.path().join("job.json"), &j);
    scene_delivery::plan(&t.path().join("job.json"), t.path()).unwrap();
    j["pictures"][0]["motion"]["kind"] = json!("centered-zoompan");
    write_json(&t.path().join("job.json"), &j);
    scene_delivery::plan(&t.path().join("job.json"), t.path()).unwrap();
    j["pictures"][0]["crop"] = json!({"x":0,"y":0,"width":64,"height":64});
    write_json(&t.path().join("job.json"), &j);
    assert!(scene_delivery::plan(&t.path().join("job.json"), t.path()).is_err());
    j["pictures"][0].as_object_mut().unwrap().remove("crop");
    j["pictures"][0]["motion"]["zoom_step"] = json!(0);
    write_json(&t.path().join("job.json"), &j);
    assert!(scene_delivery::plan(&t.path().join("job.json"), t.path()).is_err());
}
#[test]
fn continuous_motion_group_requires_the_same_adjacent_still() {
    let t = tempfile::tempdir().unwrap();
    let mut j = fixture(t.path());
    let motion = json!({
        "kind":"centered-zoompan", "scale_width":128, "scale_height":128,
        "crop_width":64, "crop_height":64,
        "zoom_step":0.0001, "zoom_max":1.015
    });
    j["pictures"][0]["motion"] = motion.clone();
    j["pictures"][1]["motion"] = motion;
    j["pictures"][1]["source"] = j["pictures"][0]["source"].clone();
    j["pictures"][0]["motion_group_id"] = json!("continuous-drift");
    j["pictures"][1]["motion_group_id"] = json!("continuous-drift");
    j["max_composition_samples"] = json!(100000);
    write_json(&t.path().join("job.json"), &j);
    scene_delivery::plan(&t.path().join("job.json"), t.path()).unwrap();
    let output = t.path().join("grouped-render");
    scene_delivery::render(&t.path().join("job.json"), t.path(), &output).unwrap();
    scene_delivery::check(&t.path().join("job.json"), t.path(), &output).unwrap();
    j["pictures"][1]["source"] = file(t.path(), "blue.ppm");
    write_json(&t.path().join("job.json"), &j);
    assert!(
        scene_delivery::plan(&t.path().join("job.json"), t.path())
            .unwrap_err()
            .to_string()
            .contains("motion group")
    );
}
#[test]
fn rejects_wrong_bytes_and_unknown_or_duplicate_consumption() {
    let t = tempfile::tempdir().unwrap();
    let mut j = fixture(t.path());
    j["pictures"][0]["source"]["bytes"] = json!(1);
    write_json(&t.path().join("job.json"), &j);
    assert!(scene_delivery::plan(&t.path().join("job.json"), t.path()).is_err());
    let mut j = fixture(t.path());
    j["pictures"][1]["attachment_id"] = json!("p-a");
    write_json(&t.path().join("job.json"), &j);
    assert!(
        scene_delivery::plan(&t.path().join("job.json"), t.path())
            .unwrap_err()
            .to_string()
            .contains("more than once")
    );
}
#[test]
fn native_recast_invalidates_plan_without_retiming_effect_by_ratio() {
    let t = tempfile::tempdir().unwrap();
    let mut j = fixture(t.path());
    let (_, old) = scene_delivery::plan(&t.path().join("job.json"), t.path()).unwrap();
    let mut c: Value =
        serde_json::from_slice(&fs::read(t.path().join("contract.json")).unwrap()).unwrap();
    c["cues"][0]["duration_samples"] = json!(60001);
    write_json(&t.path().join("contract.json"), &c);
    assert!(scene_delivery::plan(&t.path().join("job.json"), t.path()).is_err());
    j["contract"] = file(t.path(), "contract.json");
    wav(&t.path().join("a.wav"), 60001, 0);
    j["audio"][0]["source"] = file(t.path(), "a.wav");
    write_json(&t.path().join("job.json"), &j);
    let (_, new) = scene_delivery::plan(&t.path().join("job.json"), t.path()).unwrap();
    assert_ne!(old.compiled_sha256, new.compiled_sha256);
    assert_eq!(new.pictures[1].start_sample, 60001);
    assert_eq!(new.audio[2].start_sample, 123);
}

#[test]
fn rejects_case_mismatch_and_parent_traversal_before_rendering() {
    let t = tempfile::tempdir().unwrap();
    let mut j = fixture(t.path());
    j["pictures"][0]["source"]["path"] = json!("RED.ppm");
    write_json(&t.path().join("job.json"), &j);
    assert!(scene_delivery::plan(&t.path().join("job.json"), t.path()).is_err());
    j["pictures"][0]["source"]["path"] = json!("../red.ppm");
    write_json(&t.path().join("job.json"), &j);
    assert!(scene_delivery::plan(&t.path().join("job.json"), t.path()).is_err());
}

#[test]
#[ignore = "requires FFmpeg; exercised explicitly in CI"]
fn real_delivery_preserves_sample_offset_stems_frames_and_rejects_tamper() {
    let t = tempfile::tempdir().unwrap();
    fixture(t.path());
    let out = t.path().join("delivery");
    let job = t.path().join("job.json");
    scene_delivery::render(&job, t.path(), &out).unwrap();
    let pcm = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(out.join("E.wav"))
        .args(["-f", "s24le", "-"])
        .output()
        .unwrap();
    assert!(pcm.status.success());
    let active: Vec<_> = pcm
        .stdout
        .chunks_exact(6)
        .enumerate()
        .filter(|(_, s)| s.iter().any(|b| *b != 0))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(active.first(), Some(&123));
    assert_eq!(active.last(), Some(&138));
    let m = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(out.join("M.wav"))
        .args(["-f", "s24le", "-"])
        .output()
        .unwrap();
    assert!(m.stdout.iter().all(|b| *b == 0));
    // D and M are zero in this fixture: mix must reproduce the E role exactly.
    let mix = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(out.join("mix.wav"))
        .args(["-f", "s24le", "-"])
        .output()
        .unwrap();
    assert_eq!(pcm.stdout, mix.stdout);
    let pixels = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(out.join("picture.mkv"))
        .args([
            "-vf",
            "select=eq(n\\,23)+eq(n\\,24)",
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()
        .unwrap();
    assert!(pixels.status.success());
    assert_eq!(pixels.stdout.len(), 2 * 64 * 64 * 3);
    // The final red frame and first blue frame share one compiled join.
    assert!(pixels.stdout[0] > 240 && pixels.stdout[2] < 10);
    assert!(pixels.stdout[64 * 64 * 3] < 10 && pixels.stdout[64 * 64 * 3 + 2] > 240);
    assert!(
        scene_delivery::render(&job, t.path(), &out)
            .unwrap_err()
            .to_string()
            .contains("already exists")
    );
    fs::write(out.join("E.wav"), b"tampered").unwrap();
    assert!(scene_delivery::check(&job, t.path(), &out).is_err());

    let mut video_job = fixture(t.path());
    for i in 0..2 {
        video_job["pictures"][i]["source"] = file(t.path(), "delivery/picture.mkv");
        video_job["pictures"][i]["kind"] = json!("video");
        video_job["pictures"][i]["source_start_frame"] = json!(i * 24);
    }
    write_json(&job, &video_job);
    let video_out = t.path().join("video-delivery");
    scene_delivery::render(&job, t.path(), &video_out).unwrap();
    let video_pixels = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(video_out.join("picture.mkv"))
        .args([
            "-vf",
            "select=eq(n\\,23)+eq(n\\,24)",
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()
        .unwrap();
    assert!(video_pixels.status.success());
    assert_eq!(
        pixels.stdout, video_pixels.stdout,
        "a reused motion clip must not restart its action"
    );

    let mut j = fixture(t.path());
    j["audio"][2]["gain_db"] = json!(20.0);
    write_json(&job, &j);
    let clipped = t.path().join("clipped");
    assert!(
        scene_delivery::render(&job, t.path(), &clipped)
            .unwrap_err()
            .to_string()
            .contains("overload")
    );
    assert!(
        !clipped.exists(),
        "failed render must not publish a result directory"
    );
}
