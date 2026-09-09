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
