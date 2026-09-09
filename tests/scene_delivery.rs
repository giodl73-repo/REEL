use reel::scene_delivery;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

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
