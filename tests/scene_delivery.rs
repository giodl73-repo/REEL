use reel::scene_delivery;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

fn assert_picture_frames_match(before: &Path, after: &Path, pictures: &[&str]) {
    for name in pictures {
        for format in ["yuv444p", "rgb24"] {
            let decode = |folder: &Path| {
                let output = Command::new("ffmpeg")
                    .args(["-v", "error", "-nostdin", "-i"])
                    .arg(folder.join(name))
                    .args([
                        "-map",
                        "0:v:0",
                        "-fps_mode",
                        "passthrough",
                        "-pix_fmt",
                        format,
                        "-f",
                        "rawvideo",
                        "-",
                    ])
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                output.stdout
            };
            assert_eq!(
                decode(before),
                decode(after),
                "decoded {format} differs for {name}"
            );
        }
    }
}

fn assert_lossless_composition_matches(before: &Path, after: &Path, pictures: &[&str]) {
    assert_picture_frames_match(before, after, pictures);
    for name in ["D.wav", "M.wav", "E.wav", "mix.wav"] {
        assert!(
            fs::read(before.join(name)).unwrap() == fs::read(after.join(name)).unwrap(),
            "stem differs: {name}"
        );
    }
}

#[test]
#[ignore = "requires native FFmpeg; focal and pan camera execution"]
fn focal_and_pan_camera_render_real_pixels_and_preserve_native_audio() {
    use reel_assembly::motioncraft::{self, Point};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let mut job = fixture(root);
    let mut pixels = Vec::new();
    for y in 0..64 {
        for x in 0..64 {
            pixels.extend([
                if (x / 8 + y / 8) % 2 == 0 { 220u8 } else { 30 },
                (x * 3) as u8,
                (y * 3) as u8,
            ]);
        }
    }
    fs::write(
        root.join("pattern.ppm"),
        [b"P6\n64 64\n255\n".as_slice(), &pixels].concat(),
    )
    .unwrap();
    job["pictures"][0]["source"] = file(root, "pattern.ppm");
    job["still_sequence_encoding"] = json!("h264-lossless");
    let mut direction = phased_direction();
    for phase in &mut direction.elements[0].phases {
        phase.zoom_from = 1.2;
        phase.zoom_to = 1.2;
    }
    direction.elements[0].phases[1].curve = motioncraft::Curve::Linear;
    direction.elements[0].phases[1].pan_from = Some(Point::default());
    direction.elements[0].phases[1].pan_to = Some(Point { x: 0.08, y: 0.0 });
    let render = |name: &str, direction: &motioncraft::Direction| {
        let mut job = job.clone();
        let plan =
            motioncraft::compile(direction, &motioncraft::full_canvas(), 48001, 48000, 24, 1)
                .unwrap();
        job["pictures"][0]["motion"] = json!({"kind":"phased-camera","plan":plan});
        let job_path = root.join(format!("{name}.json"));
        write_json(&job_path, &job);
        let output = root.join(name);
        scene_delivery::render(&job_path, root, &output).unwrap();
        let (job, plan) = scene_delivery::plan(&job_path, root).unwrap();
        let cadence = reel::motioncraft_cadence::analyze(&job, &plan, &output).unwrap();
        assert_eq!(cadence["passed"], true, "{cadence}");
        output
    };
    let pan = render("pan", &direction);
    assert_ne!(
        fs::read(pan.join("motioncraft/frame-00000012.png")).unwrap(),
        fs::read(pan.join("motioncraft/frame-00000023.png")).unwrap()
    );
    let mut anchor = phased_direction();
    anchor.focal_anchor = Some(Point { x: 0.8, y: 0.4 });
    let anchored = render("anchor", &anchor);
    let centered = render("center", &phased_direction());
    assert_ne!(
        fs::read(anchored.join("motioncraft/frame-00000023.png")).unwrap(),
        fs::read(centered.join("motioncraft/frame-00000023.png")).unwrap()
    );
    direction.reduced_motion = true;
    direction.focal_anchor = Some(Point { x: 0.8, y: 0.4 });
    let reduced = render("reduced", &direction);
    assert_eq!(
        fs::read(reduced.join("motioncraft/frame-00000012.png")).unwrap(),
        fs::read(reduced.join("motioncraft/frame-00000023.png")).unwrap()
    );
    assert_eq!(
        fs::read(reduced.join("motioncraft/frame-00000012.png")).unwrap(),
        fs::read(centered.join("motioncraft/frame-00000012.png")).unwrap()
    );
    for stem in ["D.wav", "E.wav", "M.wav", "mix.wav"] {
        assert_eq!(
            fs::read(pan.join(stem)).unwrap(),
            fs::read(reduced.join(stem)).unwrap()
        );
        assert_eq!(
            fs::read(anchored.join(stem)).unwrap(),
            fs::read(centered.join(stem)).unwrap()
        );
    }
}

#[test]
#[ignore = "requires native FFmpeg; portable Motioncraft comparison"]
fn matched_motioncraft_package_rehydrates_and_rejects_tamper() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let assets = root.join("selected");
    fs::create_dir(&assets).unwrap();
    let mut job = fixture(&assets);
    let mut pixels = Vec::new();
    for y in 0..64 {
        for x in 0..64 {
            pixels.extend([
                if (x / 8 + y / 8) % 2 == 0 { 220u8 } else { 30 },
                (x * 3) as u8,
                (y * 3) as u8,
            ]);
        }
    }
    fs::write(
        assets.join("pattern.ppm"),
        [b"P6\n64 64\n255\n".as_slice(), &pixels].concat(),
    )
    .unwrap();
    job["pictures"][0]["source"] = file(&assets, "pattern.ppm");
    add_phased_camera(&mut job);
    job["still_sequence_encoding"] = json!("h264-lossless");
    write_json(&assets.join("job.json"), &job);
    let request = root.join("request.json");
    write_json(
        &request,
        &json!({"schema":"reel.motioncraft-comparison-request.v1", "source_job":file(root,"selected/job.json"),
        "asset_root":"selected","engine_commit":"0000000000000000000000000000000000000000"}),
    );
    let output = root.join("package");
    let report = reel::motioncraft_comparison::render(&request, &output).unwrap();
    assert_eq!(report["native_clock_verified"], true);
    assert_eq!(report["variants"].as_object().unwrap().len(), 3);
    assert_eq!(report["all_camera_cadence_passed"], true);
    assert_ne!(
        fs::read(output.join("still/comparison-frames/frame-00000023.png")).unwrap(),
        fs::read(output.join("directed/comparison-frames/frame-00000023.png")).unwrap()
    );
    assert_eq!(
        fs::read(output.join("still/comparison-frames/frame-00000023.png")).unwrap(),
        fs::read(output.join("reduced/comparison-frames/frame-00000023.png")).unwrap()
    );
    assert_eq!(
        fs::read(output.join("original-job.json")).unwrap(),
        fs::read(assets.join("job.json")).unwrap()
    );
    assert!(reel::motioncraft_comparison::render(&request, &output).is_err());
    fs::rename(&assets, root.join("unavailable-original")).unwrap();
    // Moving the entire package preserves all contract and asset-relative locators.
    let hydrated = root.join("hydrated");
    fs::rename(&output, &hydrated).unwrap();
    reel::motioncraft_comparison::check(&hydrated).unwrap();
    fs::write(hydrated.join("reduced/D.wav"), b"wrong native stem").unwrap();
    assert!(reel::motioncraft_comparison::check(&hydrated).is_err());
}

#[test]
fn comparison_rejects_a_stale_request_before_creating_output() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let assets = root.join("selected");
    fs::create_dir(&assets).unwrap();
    let job = fixture(&assets);
    write_json(&assets.join("job.json"), &job);
    let request = root.join("request.json");
    write_json(
        &request,
        &json!({"schema":"reel.motioncraft-comparison-request.v1", "source_job":file(root,"selected/job.json"),
        "asset_root":"selected","engine_commit":"0000000000000000000000000000000000000000"}),
    );
    fs::write(assets.join("job.json"), b"stale").unwrap();
    let output = root.join("package");
    assert!(reel::motioncraft_comparison::render(&request, &output).is_err());
    assert!(!output.exists());
}

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

#[test]
fn temporal_lossless_scene_encoding_preserves_every_decoded_pixel_and_sample() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let mut job = fixture(root);
    scene_delivery::render(&root.join("job.json"), root, &root.join("ffv1")).unwrap();
    job["still_sequence_encoding"] = json!("h264-lossless");
    write_json(&root.join("job-compact.json"), &job);
    scene_delivery::render(&root.join("job-compact.json"), root, &root.join("compact")).unwrap();
    for (stream, format) in [("0:v:0", "rawvideo"), ("0:a:0", "s24le")] {
        let decode = |folder: &str| {
            let mut command = Command::new("ffmpeg");
            command
                .args(["-v", "error", "-i"])
                .arg(root.join(folder).join("master.mkv"))
                .args(["-map", stream]);
            if format == "rawvideo" {
                command.args(["-pix_fmt", "yuv444p"]);
            }
            let output = command.args(["-f", format, "-"]).output().unwrap();
            assert!(output.status.success());
            output.stdout
        };
        assert_eq!(decode("ffv1"), decode("compact"));
    }
}
fn file(root: &Path, name: &str) -> Value {
    let b = fs::read(root.join(name)).unwrap();
    json!({"path":name,"sha256":Sha256::digest(&b).iter().map(|b| format!("{b:02x}")).collect::<String>(),"bytes":b.len()})
}

#[test]
#[ignore = "requires native FFmpeg; explicit mono channel mapping"]
fn duplicate_mono_preserves_source_level_and_rejects_stereo() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let mut job = fixture(root);
    for (name, samples) in [("a.wav", 48001u32), ("b.wav", 47999)] {
        let size = samples * 3;
        let mut bytes = Vec::new();
        bytes.extend(b"RIFF");
        bytes.extend((36 + size).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(48000u32.to_le_bytes());
        bytes.extend(144000u32.to_le_bytes());
        bytes.extend(3u16.to_le_bytes());
        bytes.extend(24u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(size.to_le_bytes());
        for _ in 0..samples {
            bytes.extend(&1_000_000i32.to_le_bytes()[..3]);
        }
        fs::write(root.join(name), bytes).unwrap();
    }
    job["audio"][0]["source"] = file(root, "a.wav");
    job["audio"][1]["source"] = file(root, "b.wav");
    write_json(&root.join("legacy.json"), &job);
    scene_delivery::render(&root.join("legacy.json"), root, &root.join("legacy")).unwrap();
    for event in job["audio"].as_array_mut().unwrap().iter_mut().take(2) {
        event["channel_mapping"] = json!("duplicate-mono");
    }
    write_json(&root.join("duplicate.json"), &job);
    scene_delivery::render(&root.join("duplicate.json"), root, &root.join("duplicate")).unwrap();
    scene_delivery::render_audio(
        &root.join("duplicate.json"),
        root,
        &root.join("duplicate-audio"),
    )
    .unwrap();
    for name in ["D.wav", "M.wav", "E.wav", "mix.wav"] {
        assert_eq!(
            fs::read(root.join("duplicate").join(name)).unwrap(),
            fs::read(root.join("duplicate-audio").join(name)).unwrap()
        );
    }
    let samples = |folder: &str| {
        let output = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(root.join(folder).join("D.wav"))
            .args(["-f", "s32le", "-"])
            .output()
            .unwrap();
        assert!(output.status.success());
        output
            .stdout
            .chunks_exact(4)
            .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap()) / 256)
            .collect::<Vec<_>>()
    };
    let legacy = samples("legacy");
    let duplicate = samples("duplicate");
    assert_eq!(duplicate.len(), 96000 * 2);
    assert!(duplicate.iter().all(|sample| *sample == 1_000_000));
    assert!(
        legacy
            .iter()
            .all(|sample| (707105..=707108).contains(sample))
    );
    for stem in ["M.wav", "E.wav"] {
        assert_eq!(
            fs::read(root.join("legacy").join(stem)).unwrap(),
            fs::read(root.join("duplicate").join(stem)).unwrap()
        );
    }
    job["audio"][2]["channel_mapping"] = json!("duplicate-mono");
    write_json(&root.join("stereo.json"), &job);
    assert!(
        scene_delivery::render(&root.join("stereo.json"), root, &root.join("stereo"))
            .unwrap_err()
            .to_string()
            .contains("requires a mono source")
    );
    job["audio"][0]["channel_mapping"] = json!("unknown");
    write_json(&root.join("unknown.json"), &job);
    assert!(scene_delivery::plan(&root.join("unknown.json"), root).is_err());
}
#[test]
#[ignore = "requires native FFmpeg; independent audio qualification"]
fn audio_only_matches_full_scene_and_rejects_tamper_stale_job_and_overwrite() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let mut job = fixture(root);
    wav(&root.join("a.wav"), 48001, 900000);
    wav(&root.join("b.wav"), 47999, -700000);
    wav(&root.join("effect.wav"), 32, 2000000);
    for (index, name) in [(0, "a.wav"), (1, "b.wav"), (2, "effect.wav")] {
        job["audio"][index]["source"] = file(root, name);
    }
    job["audio"][0]["gain_db"] = json!(-1.25);
    job["audio"][0]["fade_in_samples"] = json!(128);
    job["audio"][0]["fade_out_samples"] = json!(256);
    job["audio"][2]["source_start_sample"] = json!(4);
    job["audio"][2]["fade_out_samples"] = json!(4);
    let mut production: Value =
        serde_json::from_slice(&fs::read(root.join("production.json")).unwrap()).unwrap();
    production["audio_events"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"m","role":"music","source":"a.wav","start_seconds":0}));
    write_json(&root.join("production.json"), &production);
    let mut contract: Value =
        serde_json::from_slice(&fs::read(root.join("contract.json")).unwrap()).unwrap();
    contract["attachments"].as_array_mut().unwrap().push(json!({"id":"m","target":{"kind":"audio","audio_event_id":"m"},"start":{"kind":"cue-start","cue_id":"a"},"end":{"kind":"cue-end","cue_id":"a"}}));
    write_json(&root.join("contract.json"), &contract);
    job["production_manifest_sha256"] = file(root, "production.json")["sha256"].clone();
    job["contract"] = file(root, "contract.json");
    job["buses"]["M"] = json!({"state":"present","reason":"Bounded selected music window"});
    job["audio"].as_array_mut().unwrap().push(json!({"attachment_id":"m","bus":"M","source":file(root,"a.wav"),"gain_db":-18,"fade_in_samples":64,"fade_out_samples":128}));
    let path = root.join("job.json");
    write_json(&path, &job);
    scene_delivery::render(&path, root, &root.join("full")).unwrap();
    let receipt = scene_delivery::render_audio(&path, root, &root.join("audio")).unwrap();
    assert_eq!(receipt.content_samples, 96000);
    assert_eq!(receipt.outputs.len(), 4);
    assert_eq!(receipt.schema, "reel.scene-audio-receipt.v0.1");
    assert!(!root.join("audio/picture.mkv").exists());
    assert!(!root.join("audio/receipt.json").exists());
    assert!(scene_delivery::check(&path, root, &root.join("audio")).is_err());
    for name in ["D.wav", "M.wav", "E.wav", "mix.wav"] {
        assert_eq!(
            fs::read(root.join("full").join(name)).unwrap(),
            fs::read(root.join("audio").join(name)).unwrap()
        );
    }
    scene_delivery::check_audio(&path, root, &root.join("audio")).unwrap();
    assert!(scene_delivery::render_audio(&path, root, &root.join("audio")).is_err());
    let original = fs::read(root.join("audio/D.wav")).unwrap();
    fs::write(root.join("audio/D.wav"), b"tampered").unwrap();
    assert!(scene_delivery::check_audio(&path, root, &root.join("audio")).is_err());
    fs::write(root.join("audio/D.wav"), original).unwrap();
    job["audio"][0]["gain_db"] = json!(-2);
    write_json(&path, &job);
    assert!(scene_delivery::check_audio(&path, root, &root.join("audio")).is_err());
    job["audio"][0]["channel_mapping"] = json!("duplicate-mono");
    write_json(&path, &job);
    assert!(scene_delivery::render_audio(&path, root, &root.join("rejected")).is_err());
    assert!(!root.join("rejected").exists());
}

#[test]
fn effect_placement_preserves_semantic_anchors_and_rejects_invalid_evidence_and_bounds() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let original = fixture(root);
    let (_, baseline) = scene_delivery::plan(&root.join("job.json"), root).unwrap();
    assert!(baseline.audio_anchor_spans.is_empty());
    let offset = json!({"samples":8,"reason":"Measured historical millisecond placement","evidence":file(root,"contract.json")});
    let mut job = original.clone();
    job["audio"][2]["placement_offset"] = offset.clone();
    write_json(&root.join("placed.json"), &job);
    let (_, placed) = scene_delivery::plan(&root.join("placed.json"), root).unwrap();
    assert_eq!(
        serde_json::to_value(&placed.audio_anchor_spans).unwrap(),
        serde_json::to_value(&baseline.audio).unwrap()
    );
    assert_eq!(placed.audio[2].start_sample, 131);
    assert_eq!(placed.audio[2].end_sample, 147);
    assert_eq!(placed.duration_samples, baseline.duration_samples);
    assert_eq!(placed.compiled_sha256, baseline.compiled_sha256);
    for bad in [json!(-124), json!(96000), json!(i64::MIN), json!(i64::MAX)] {
        job["audio"][2]["placement_offset"]["samples"] = bad;
        write_json(&root.join("bad.json"), &job);
        assert!(scene_delivery::plan(&root.join("bad.json"), root).is_err());
    }
    job["audio"][2]["placement_offset"] = offset.clone();
    job["audio"][2]["placement_offset"]["evidence"]["sha256"] = json!("0".repeat(64));
    write_json(&root.join("bad.json"), &job);
    assert!(scene_delivery::plan(&root.join("bad.json"), root).is_err());
    job["audio"][2]["placement_offset"] = offset.clone();
    job["audio"][2]["placement_offset"]["reason"] = json!(" ");
    write_json(&root.join("bad.json"), &job);
    assert!(scene_delivery::plan(&root.join("bad.json"), root).is_err());
    job = original;
    job["audio"][0]["placement_offset"] = offset;
    write_json(&root.join("bad.json"), &job);
    assert!(scene_delivery::plan(&root.join("bad.json"), root).is_err());
    job["audio"][0]
        .as_object_mut()
        .unwrap()
        .remove("placement_offset");
    job["audio"][0]["channel_mapping"] = json!("downmix-mono-duplicate");
    write_json(&root.join("bad.json"), &job);
    assert!(scene_delivery::plan(&root.join("bad.json"), root).is_err());
}

#[test]
#[ignore = "requires native FFmpeg; selected Sonic channel and placement compatibility"]
fn explicit_sonic_mono_and_placement_match_historical_mix_without_changing_pictures_or_dialogue() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let mut job = fixture(root);
    wav(&root.join("effect.wav"), 32, 2000000);
    let mut effect = fs::read(root.join("effect.wav")).unwrap();
    for sample in 0..32usize {
        effect[44 + sample * 6..47 + sample * 6]
            .copy_from_slice(&(2000000i32 + sample as i32 * 1000).to_le_bytes()[..3]);
        effect[47 + sample * 6..50 + sample * 6]
            .copy_from_slice(&(500000i32 - sample as i32 * 4000).to_le_bytes()[..3]);
    }
    fs::write(root.join("effect.wav"), effect).unwrap();
    job["audio"][2]["source"] = file(root, "effect.wav");
    job["audio"][2]["gain_db"] = json!(-18);
    job["audio"][2]["fade_out_samples"] = json!(4);
    job["audio"][2]["source_start_sample"] = json!(4);
    write_json(&root.join("legacy.json"), &job);
    scene_delivery::render(&root.join("legacy.json"), root, &root.join("legacy")).unwrap();
    job["audio"][2]["channel_mapping"] = json!("downmix-mono-duplicate");
    job["audio"][2]["placement_offset"] = json!({"samples":8,"reason":"Explicit selected historical placement","evidence":file(root,"contract.json")});
    let path = root.join("mapped.json");
    write_json(&path, &job);
    let full = scene_delivery::render(&path, root, &root.join("mapped")).unwrap();
    let audio = scene_delivery::render_audio(&path, root, &root.join("audio")).unwrap();
    assert_eq!(full.plan.audio[2].start_sample, 131);
    assert_eq!(audio.plan.audio_anchor_spans[2].start_sample, 123);
    for name in ["D.wav", "M.wav", "E.wav", "mix.wav"] {
        assert_eq!(
            fs::read(root.join("mapped").join(name)).unwrap(),
            fs::read(root.join("audio").join(name)).unwrap()
        );
    }
    for name in ["D.wav", "M.wav"] {
        assert_eq!(
            fs::read(root.join("mapped").join(name)).unwrap(),
            fs::read(root.join("legacy").join(name)).unwrap()
        );
    }
    assert_picture_frames_match(&root.join("legacy"), &root.join("mapped"), &["picture.mkv"]);
    let status = Command::new("ffmpeg").args(["-v","error","-f","lavfi","-i","anullsrc=r=48000:cl=mono","-i"]).arg(root.join("effect.wav"))
        .args(["-filter_complex","[1:a]atrim=start_sample=4:end_sample=20,asetpts=PTS-STARTPTS,volume=-18dB,afade=t=out:ss=12:ns=4,adelay=131S:all=1[e];[0:a][e]amix=inputs=2:duration=first:normalize=0,atrim=end_sample=96000,pan=stereo|c0=c0|c1=c0[out]","-map","[out]","-c:a","pcm_s24le"])
        .arg(root.join("historical.wav")).status().unwrap();
    assert!(status.success());
    let pcm = |path: &Path| {
        let decoded = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(path)
            .args(["-f", "s32le", "-"])
            .output()
            .unwrap();
        assert!(decoded.status.success());
        decoded
            .stdout
            .chunks_exact(4)
            .map(|b| i32::from_le_bytes(b.try_into().unwrap()) / 256)
            .collect::<Vec<_>>()
    };
    let expected = pcm(&root.join("historical.wav"));
    let actual = pcm(&root.join("mapped/mix.wav"));
    assert_eq!(actual.len(), expected.len());
    assert!(
        actual
            .iter()
            .zip(&expected)
            .all(|(a, b)| (a - b).abs() <= 1)
    );
    assert!(actual[..131 * 2].iter().all(|v| *v == 0));
    assert_ne!(actual[131 * 2], 0);
    assert!(actual.chunks_exact(2).all(|pair| pair[0] == pair[1]));
    scene_delivery::check_audio(&path, root, &root.join("audio")).unwrap();
    job["audio"][2]["placement_offset"]["samples"] = json!(-3);
    write_json(&path, &job);
    let negative = scene_delivery::render_audio(&path, root, &root.join("negative")).unwrap();
    assert_eq!(negative.plan.audio[2].start_sample, 120);
    assert!(scene_delivery::check_audio(&path, root, &root.join("audio")).is_err());
    let mut mono = fs::read(root.join("effect.wav")).unwrap();
    // A valid existing mono take must not silently enter the stereo-only route.
    mono[22..24].copy_from_slice(&1u16.to_le_bytes());
    mono[28..32].copy_from_slice(&144000u32.to_le_bytes());
    mono[32..34].copy_from_slice(&3u16.to_le_bytes());
    fs::write(root.join("mono.wav"), mono).unwrap();
    job["audio"][2]["source"] = file(root, "mono.wav");
    write_json(&path, &job);
    let error = scene_delivery::render_audio(&path, root, &root.join("rejected")).unwrap_err();
    assert!(error.to_string().contains("requires a stereo source"));
    assert!(!root.join("rejected").exists());
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

fn phased_direction() -> reel_assembly::motioncraft::Direction {
    serde_json::from_slice(include_bytes!(
        "../manifests/fixtures/motioncraft/read-then-push.json"
    ))
    .unwrap()
}

fn add_phased_camera(job: &mut Value) {
    let safe = reel_assembly::motioncraft::Rect {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };
    let plan = reel_assembly::motioncraft::compile(&phased_direction(), &safe, 48001, 48000, 24, 1)
        .unwrap();
    job["pictures"][0]["motion"] = json!({"kind":"phased-camera","plan":plan});
}

#[test]
fn caption_reservation_reuses_profile_geometry_and_rejects_unsafe_cameras() {
    let t = tempfile::tempdir().unwrap();
    let mut job = fixture(t.path());
    job["width"] = json!(1280);
    job["height"] = json!(720);
    job["caption_picture_layout"] =
        json!({"profile":"youtube-review","layout":"reserve-caption-band"});
    add_phased_camera(&mut job);
    write_json(&t.path().join("job.json"), &job);
    let (parsed, _) = scene_delivery::plan(&t.path().join("job.json"), t.path()).unwrap();
    let layout = scene_delivery::picture_layout(&parsed).unwrap().unwrap();
    assert_eq!(layout.picture_region.height, 520);
    job["post_compose_camera"] = json!({"evidence":file(t.path(),"contract.json"),"zoom_step":0.02,"zoom_max":1.5,"windows":[{"start_frame":0,"end_frame":48}]});
    write_json(&t.path().join("job.json"), &job);
    assert!(
        scene_delivery::plan(&t.path().join("job.json"), t.path())
            .unwrap_err()
            .to_string()
            .contains("reserved caption band")
    );
    job.as_object_mut().unwrap().remove("post_compose_camera");
    job["caption_picture_layout"]["profile"] = json!("phone-review");
    write_json(&t.path().join("job.json"), &job);
    assert!(
        scene_delivery::plan(&t.path().join("job.json"), t.path())
            .unwrap_err()
            .to_string()
            .contains("9:16")
    );
}

#[test]
fn explicit_picture_viewport_rejects_overflow_and_caption_overlap() {
    use reel::caption_presentation::{self, CaptionPictureLayoutConfig, PixelRect};
    let config: CaptionPictureLayoutConfig =
        serde_json::from_value(json!({"profile":"phone-review","layout":"reserve-caption-band"}))
            .unwrap();
    let region = PixelRect {
        x: 0,
        y: 30,
        width: 720,
        height: 405,
    };
    let resolved =
        caption_presentation::resolve_scene_picture_layout(Some(&config), Some(&region), 720, 1280)
            .unwrap()
            .unwrap();
    assert_eq!(resolved.picture_region, region);
    for invalid in [
        PixelRect {
            x: 0,
            y: 899,
            width: 720,
            height: 2,
        },
        PixelRect {
            x: u32::MAX,
            y: 0,
            width: 2,
            height: 2,
        },
        PixelRect {
            x: 0,
            y: 0,
            width: 0,
            height: 2,
        },
    ] {
        assert!(
            caption_presentation::resolve_scene_picture_layout(
                Some(&config),
                Some(&invalid),
                720,
                1280
            )
            .is_err()
        );
    }
    assert!(
        serde_json::from_value::<PixelRect>(json!({"x":0,"y":0,"width":720,"height":405,"typo":1}))
            .is_err()
    );
    assert!(
        caption_presentation::resolve_scene_picture_layout(None, None, 1280, 720)
            .unwrap()
            .is_none()
    );
}

#[test]
#[ignore = "requires native FFmpeg; explicit landscape and portrait picture viewports"]
fn explicit_picture_viewport_contains_native_camera_and_preserves_audio() {
    for (width, height, region, caption) in [
        (
            1280,
            720,
            json!({"x":0,"y":0,"width":853,"height":720}),
            None,
        ),
        (
            720,
            1280,
            json!({"x":0,"y":30,"width":720,"height":405}),
            Some("phone-review"),
        ),
    ] {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        let mut job = fixture(root);
        let pixels: Vec<_> = (0..64)
            .flat_map(|y| {
                (0..64).flat_map(move |x| {
                    [
                        if (x / 8 + y / 8) % 2 == 0 { 220u8 } else { 30 },
                        (x * 3) as u8,
                        (y * 3) as u8,
                    ]
                })
            })
            .collect();
        fs::write(
            root.join("pattern.ppm"),
            [b"P6\n64 64\n255\n".as_slice(), &pixels].concat(),
        )
        .unwrap();
        job["pictures"][0]["source"] = file(root, "pattern.ppm");
        job["width"] = json!(width);
        job["height"] = json!(height);
        job["picture_region"] = region.clone();
        job["still_sequence_encoding"] = json!("h264-lossless");
        if let Some(profile) = caption {
            job["caption_picture_layout"] =
                json!({"profile":profile,"layout":"reserve-caption-band"});
        }
        let original = root.join("still-job.json");
        write_json(&original, &job);
        scene_delivery::render(&original, root, &root.join("still")).unwrap();
        add_phased_camera(&mut job);
        write_json(&root.join("job.json"), &job);
        let output = root.join("directed");
        scene_delivery::render(&root.join("job.json"), root, &output).unwrap();
        scene_delivery::check(&root.join("job.json"), root, &output).unwrap();
        for stem in ["D.wav", "M.wav", "E.wav", "mix.wav"] {
            assert_eq!(
                fs::read(root.join("still").join(stem)).unwrap(),
                fs::read(output.join(stem)).unwrap()
            );
        }
        let decoded = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(output.join("picture.mkv"))
            .args([
                "-vf",
                "select=eq(n\\,0)+eq(n\\,10)+eq(n\\,23)",
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
        assert!(
            decoded.status.success(),
            "{}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        let stride = width as usize * height as usize * 3;
        assert_eq!(decoded.stdout.len(), stride * 3);
        assert_eq!(
            &decoded.stdout[..stride],
            &decoded.stdout[stride..2 * stride]
        );
        assert_ne!(&decoded.stdout[..stride], &decoded.stdout[2 * stride..]);
        let x = region["x"].as_u64().unwrap() as usize;
        let y = region["y"].as_u64().unwrap() as usize;
        let rw = region["width"].as_u64().unwrap() as usize;
        let rh = region["height"].as_u64().unwrap() as usize;
        for frame in decoded.stdout.chunks_exact(stride) {
            for py in 0..height as usize {
                for px in 0..width as usize {
                    if px < x || px >= x + rw || py < y || py >= y + rh {
                        let index = (py * width as usize + px) * 3;
                        assert!(
                            frame[index..index + 3].iter().all(|v| *v <= 2),
                            "camera entered reserved text space"
                        );
                    }
                }
            }
        }
        let report =
            reel::motioncraft_cadence::checked_report(&root.join("job.json"), root, &output)
                .unwrap();
        assert_eq!(report["passed"], true);
        assert_eq!(report["shots"][0]["measurement_region"], region);
    }
}

#[test]
#[ignore = "requires FFmpeg; run explicitly for Motioncraft"]
fn reserved_caption_band_survives_real_phased_camera_in_both_profiles() {
    for (width, height, profile, band_y) in [
        (1280, 720, "youtube-review", 520),
        (720, 1280, "phone-review", 900),
        (900, 1600, "phone-review", 1125),
    ] {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        let mut job = fixture(root);
        let pixels = (0..64)
            .flat_map(|y| {
                (0..64).flat_map(move |x| {
                    [
                        if (x / 8 + y / 8) % 2 == 0 { 220u8 } else { 30 },
                        (x * 3) as u8,
                        (y * 3) as u8,
                    ]
                })
            })
            .collect::<Vec<_>>();
        fs::write(
            root.join("pattern.ppm"),
            [b"P6\n64 64\n255\n".as_slice(), &pixels].concat(),
        )
        .unwrap();
        job["pictures"][0]["source"] = file(root, "pattern.ppm");
        job["width"] = json!(width);
        job["height"] = json!(height);
        job["still_sequence_encoding"] = json!("h264-lossless");
        job["caption_picture_layout"] = json!({"profile":profile,"layout":"reserve-caption-band"});
        let mut direction = phased_direction();
        direction.focal_anchor = Some(reel_assembly::motioncraft::Point { x: 0.8, y: 0.4 });
        direction.elements[0].phases[1].pan_from =
            Some(reel_assembly::motioncraft::Point::default());
        direction.elements[0].phases[1].pan_to =
            Some(reel_assembly::motioncraft::Point { x: 0.02, y: 0.0 });
        let camera = reel_assembly::motioncraft::compile(
            &direction,
            &reel_assembly::motioncraft::full_canvas(),
            48001,
            48000,
            24,
            1,
        )
        .unwrap();
        job["pictures"][0]["motion"] = json!({"kind":"phased-camera","plan":camera});
        write_json(&root.join("job.json"), &job);
        let output = root.join("reserved");
        scene_delivery::render(&root.join("job.json"), root, &output).unwrap();
        scene_delivery::check(&root.join("job.json"), root, &output).unwrap();
        let decoded = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(output.join("picture.mkv"))
            .args([
                "-vf",
                "select=eq(n\\,0)+eq(n\\,10)+eq(n\\,23)",
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
        assert!(decoded.status.success());
        let stride = width as usize * height as usize * 3;
        assert_eq!(decoded.stdout.len(), stride * 3);
        assert_eq!(
            &decoded.stdout[..stride],
            &decoded.stdout[stride..stride * 2],
            "reading hold changed in {profile}"
        );
        assert_ne!(
            &decoded.stdout[..stride],
            &decoded.stdout[stride * 2..],
            "camera did not move in {profile}"
        );
        for frame in decoded.stdout.chunks_exact(stride) {
            let band = &frame[(band_y as usize + 4) * width as usize * 3..];
            assert!(
                band.iter().all(|v| *v <= 2),
                "camera entered caption band in {profile}"
            );
            let center = (band_y as usize / 2 * width as usize + width as usize / 2) * 3;
            assert!(
                frame[center..center + 3].iter().any(|v| *v > 20),
                "picture missing in {profile}"
            );
        }
        let evidence: Value =
            serde_json::from_slice(&fs::read(output.join("motioncraft/evidence.json")).unwrap())
                .unwrap();
        assert_eq!(
            evidence["picture_layout"]["picture_region"]["height"],
            band_y
        );
        assert_eq!(
            evidence["camera_coordinate_space"],
            "normalized contained picture region before output padding"
        );
        let (_, plan) = scene_delivery::plan(&root.join("job.json"), root).unwrap();
        assert_eq!(plan.duration_samples, 96000);
        assert_eq!(plan.frame_count, 48);
        assert_eq!(plan.audio[2].start_sample, 123);
    }
}

#[test]
fn phased_camera_is_bound_to_native_timing_and_compiled_evidence() {
    let t = tempfile::tempdir().unwrap();
    let mut job = fixture(t.path());
    add_phased_camera(&mut job);
    write_json(&t.path().join("job.json"), &job);
    let (_, plan) = scene_delivery::plan(&t.path().join("job.json"), t.path()).unwrap();
    assert_eq!(plan.duration_samples, 96000);
    assert_eq!(plan.audio[2].start_sample, 123);
    job["pictures"][0]["motion"]["plan"]["duration_samples"] = json!(48000);
    write_json(&t.path().join("job.json"), &job);
    assert!(
        scene_delivery::plan(&t.path().join("job.json"), t.path())
            .unwrap_err()
            .to_string()
            .contains("compiled plan")
    );
    add_phased_camera(&mut job);
    job["pictures"][0]["motion"]["plan"]["direction"]["elements"][0]["role"] = json!("headline");
    write_json(&t.path().join("job.json"), &job);
    assert!(
        scene_delivery::plan(&t.path().join("job.json"), t.path())
            .unwrap_err()
            .to_string()
            .contains("camera element")
    );
    let mut cropped = phased_direction();
    cropped.protected_regions = vec![reel_assembly::motioncraft::Rect {
        x: 0.85,
        y: 0.3,
        width: 0.1,
        height: 0.1,
    }];
    let p = reel_assembly::motioncraft::compile(
        &cropped,
        &reel_assembly::motioncraft::full_canvas(),
        48001,
        48000,
        24,
        1,
    )
    .unwrap();
    job["pictures"][0]["motion"] = json!({"kind":"phased-camera","plan":p});
    write_json(&t.path().join("job.json"), &job);
    assert!(
        scene_delivery::plan(&t.path().join("job.json"), t.path())
            .unwrap_err()
            .to_string()
            .contains("crops protected")
    );
}

#[test]
#[ignore = "requires FFmpeg; run explicitly for Motioncraft"]
fn phased_camera_renders_real_hold_push_and_reduced_motion_without_audio_drift() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let mut job = fixture(root);
    let mut pixels = Vec::new();
    for y in 0..64 {
        for x in 0..64 {
            pixels.extend([
                if (x / 8 + y / 8) % 2 == 0 { 220u8 } else { 30 },
                (x * 3) as u8,
                (y * 3) as u8,
            ]);
        }
    }
    fs::write(
        root.join("pattern.ppm"),
        [b"P6\n64 64\n255\n".as_slice(), &pixels].concat(),
    )
    .unwrap();
    job["pictures"][0]["source"] = file(root, "pattern.ppm");
    job["still_sequence_encoding"] = json!("h264-lossless");
    add_phased_camera(&mut job);
    write_json(&root.join("job.json"), &job);
    let output = root.join("directed");
    scene_delivery::render(&root.join("job.json"), root, &output).unwrap();
    let decode = |dir: &Path| {
        let result = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(dir.join("picture.mkv"))
            .args(["-pix_fmt", "rgb24", "-f", "rawvideo", "-"])
            .output()
            .unwrap();
        assert!(result.status.success());
        result.stdout
    };
    let rendered = decode(&output);
    let (cadence_job, cadence_plan) = scene_delivery::plan(&root.join("job.json"), root).unwrap();
    let cadence = reel::motioncraft_cadence::analyze(&cadence_job, &cadence_plan, &output).unwrap();
    assert_eq!(cadence["passed"], true, "{cadence}");
    assert!(
        cadence["analyzer_ffmpeg_version"]
            .as_str()
            .unwrap()
            .starts_with("ffmpeg version ")
    );
    assert_eq!(cadence["analyzer_backend"], "native");
    let mut layered_job = cadence_job.clone();
    layered_job.post_compose_camera = Some(scene_delivery::PostComposeCamera {
        evidence: layered_job.contract.clone(),
        zoom_step: 0.02,
        zoom_max: 1.5,
        windows: vec![scene_delivery::CameraWindow {
            start_frame: 0,
            end_frame: 48,
        }],
    });
    let layered = reel::motioncraft_cadence::analyze(&layered_job, &cadence_plan, &output).unwrap();
    assert_eq!(layered["passed"], false);
    assert_eq!(layered["shots"][0]["passed"], Value::Null);
    assert_eq!(layered["shots"][0]["whole_frame_hold_allowance"], false);
    let frame = 64 * 64 * 3;
    assert_eq!(rendered.len(), 48 * frame);
    assert_eq!(
        &rendered[0..frame],
        &rendered[10 * frame..11 * frame],
        "declared reading hold must be stationary"
    );
    assert_ne!(
        &rendered[0..frame],
        &rendered[23 * frame..24 * frame],
        "declared push must reach rendered pixels"
    );
    let evidence: Value =
        serde_json::from_slice(&fs::read(output.join("motioncraft/evidence.json")).unwrap())
            .unwrap();
    assert!(
        evidence["frames"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["frame_index"] == 23)
    );
    assert!(output.join("motioncraft/contact-sheet.png").is_file());
    assert!(output.join("motioncraft/quarter-speed.mp4").is_file());
    let selected = image::open(output.join("motioncraft/frame-00000023.png"))
        .unwrap()
        .to_rgb8();
    assert_eq!(
        selected.as_raw().as_slice(),
        &rendered[23 * frame..24 * frame],
        "indexed evidence must match an independent full-video frame decode"
    );
    let mut reduced = phased_direction();
    reduced.reduced_motion = true;
    let safe = reel_assembly::motioncraft::Rect {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };
    let p = reel_assembly::motioncraft::compile(&reduced, &safe, 48001, 48000, 24, 1).unwrap();
    job["pictures"][0]["motion"] = json!({"kind":"phased-camera","plan":p});
    write_json(&root.join("reduced-job.json"), &job);
    let reduced_output = root.join("reduced");
    scene_delivery::render(&root.join("reduced-job.json"), root, &reduced_output).unwrap();
    let stationary = decode(&reduced_output);
    let (cadence_job, cadence_plan) =
        scene_delivery::plan(&root.join("reduced-job.json"), root).unwrap();
    let cadence =
        reel::motioncraft_cadence::analyze(&cadence_job, &cadence_plan, &reduced_output).unwrap();
    assert_eq!(cadence["passed"], true, "{cadence}");
    assert_eq!(&stationary[0..frame], &stationary[23 * frame..24 * frame]);
    assert_eq!(
        fs::read(output.join("D.wav")).unwrap(),
        fs::read(reduced_output.join("D.wav")).unwrap()
    );
    assert_eq!(
        fs::read(output.join("E.wav")).unwrap(),
        fs::read(reduced_output.join("E.wav")).unwrap()
    );
}

#[test]
fn selected_ass_layer_changes_rendered_pixels_and_is_checked() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let mut job = fixture(root);
    job["still_sequence_encoding"] = json!("h264-lossless");
    let mut contract: Value =
        serde_json::from_slice(&fs::read(root.join("contract.json")).unwrap()).unwrap();
    contract["attachments"].as_array_mut().unwrap().push(json!({
        "id":"editable-title", "target":{"kind":"title","title_id":"example"},
        "start":{"kind":"cue-start","cue_id":"a","offset_samples":0},
        "end":{"kind":"cue-end","cue_id":"b","offset_samples":0}
    }));
    write_json(&root.join("contract.json"), &contract);
    fs::write(root.join("panel.ass"), "[Script Info]\nScriptType: v4.00+\nPlayResX: 64\nPlayResY: 64\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,30,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,-1,0,0,0,100,100,0,0,1,1,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,TEST\n").unwrap();
    fs::rename(
        root.join("panel.ass"),
        root.join("extensionless-ass-object"),
    )
    .unwrap();
    job["contract"] = file(root, "contract.json");
    job["external_layers"] = json!([{
        "attachment_id":"editable-title", "reason":"Selected editable title",
        "evidence":file(root,"extensionless-ass-object"), "render_mode":"ass-overlay"
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
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(output.join("picture.mkv"))
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .unwrap();
    assert!(decoded.status.success());
    assert_eq!(decoded.stdout.len(), 48 * 64 * 64 * 3);
    for (index, frame) in decoded.stdout.chunks_exact(64 * 64 * 3).enumerate() {
        let visible_text = frame
            .chunks_exact(3)
            .filter(|pixel| pixel.iter().all(|&c| c > 200))
            .count();
        assert!(visible_text > 20, "selected title missing at frame {index}");
    }
    scene_delivery::check(&root.join("job.json"), root, &output).unwrap();
    let mut compact_job = job.clone();
    compact_job["composition_encoding"] = json!("h264-lossless");
    write_json(&root.join("compact-ass.json"), &compact_job);
    let compact = root.join("compact-ass");
    scene_delivery::render(&root.join("compact-ass.json"), root, &compact).unwrap();
    scene_delivery::check(&root.join("compact-ass.json"), root, &compact).unwrap();
    assert_lossless_composition_matches(&output, &compact, &["picture.mkv", "master.mkv"]);
    // Historical per-shot rounding may retain one fewer picture frame while
    // narration and full-scene ASS still end at the same native sample.
    let mut retained_job = compact_job.clone();
    retained_job["pictures"][0]["delivery_frame_count"] = json!(24);
    retained_job["pictures"][1]["delivery_frame_count"] = json!(23);
    let retained_path = root.join("retained-ass.json");
    write_json(&retained_path, &retained_job);
    let retained = root.join("retained-ass");
    let retained_receipt = scene_delivery::render(&retained_path, root, &retained).unwrap();
    assert_eq!(retained_receipt.plan.frame_count, 47);
    assert_eq!(retained_receipt.plan.duration_samples, 96000);
    assert_eq!(retained_receipt.plan.external_layer_spans[0].end_frame, 47);
    assert_eq!(
        retained_receipt.plan.external_layer_spans[0].end_sample,
        96000
    );
    for name in ["D.wav", "M.wav", "E.wav", "mix.wav"] {
        assert_eq!(
            fs::read(compact.join(name)).unwrap(),
            fs::read(retained.join(name)).unwrap()
        );
    }
    let expectations_path = root.join("retained-expectations.json");
    let mut expectations = json!({
        "schema":"reel.motioncraft-layer-expectations.v1",
        "job_sha256":retained_receipt.plan.job_sha256,
        "render_receipt_sha256":file(root,"retained-ass/receipt.json")["sha256"],
        "layers":[{"attachment_id":"editable-title",
            "source_sha256":retained_job["external_layers"][0]["evidence"]["sha256"],
            "intervals":[
                {"start_frame":0,"end_frame":24,"kind":"hold","region":{"x":0.0,"y":0.0,"width":1.0,"height":1.0}},
                {"start_frame":24,"end_frame":47,"kind":"hold","region":{"x":0.0,"y":0.0,"width":1.0,"height":1.0}}
            ]}]
    });
    write_json(&expectations_path, &expectations);
    let report =
        reel::motioncraft_layers::analyze(&retained_path, root, &retained, &expectations_path)
            .unwrap();
    assert_eq!(report["passed"], true, "{report}");
    expectations["layers"][0]["intervals"][1]["end_frame"] = json!(48);
    write_json(&expectations_path, &expectations);
    assert!(
        reel::motioncraft_layers::analyze(&retained_path, root, &retained, &expectations_path)
            .is_err()
    );
    fs::write(output.join("presentation.ass"), b"tampered").unwrap();
    assert!(scene_delivery::check(&root.join("job.json"), root, &output).is_err());
}

#[test]
#[ignore = "requires the hash-bound CAIMITOS portrait poem regression job and native FFmpeg"]
fn selected_poem_ass_keeps_title_and_complete_reading_states() {
    use std::io::Read;
    let job_path = std::env::var_os("REEL_ASS_REGRESSION_JOB")
        .map(std::path::PathBuf::from)
        .expect("set REEL_ASS_REGRESSION_JOB");
    let assets = std::env::var_os("REEL_ASS_REGRESSION_ASSETS")
        .map(std::path::PathBuf::from)
        .expect("set REEL_ASS_REGRESSION_ASSETS");
    let temporary = tempfile::tempdir().unwrap();
    let output = std::env::var_os("REEL_ASS_REGRESSION_OUTPUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| temporary.path().join("rendered"));
    let (job, plan) = scene_delivery::plan(&job_path, &assets).unwrap();
    assert_eq!((job.width, job.height), (720, 1280));
    let viewport = scene_delivery::picture_layout(&job).unwrap().unwrap();
    assert!(viewport.picture_region.y + viewport.picture_region.height < 465);
    assert_eq!(
        plan.frame_count, 605,
        "use the retained Spanish Scene001 fixture"
    );
    let receipt = scene_delivery::render(&job_path, &assets, &output).unwrap();
    assert_eq!(receipt.plan.frame_count, 605);
    let mut child = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(output.join("picture.mkv"))
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut frame = vec![0; 720 * 1280 * 3];
    let mut missing = Vec::new();
    for index in 0..605 {
        stdout.read_exact(&mut frame).unwrap();
        for (name, top, bottom) in [("title", 465, 525), ("first line", 540, 575)] {
            let bright = (top..bottom)
                .flat_map(|y| (40..680).map(move |x| (y * 720 + x) * 3))
                .filter(|&offset| frame[offset..offset + 3].iter().all(|&c| c > 150))
                .count();
            if bright < 100 {
                missing.push((index, name, bright));
            }
        }
    }
    let mut trailing = [0];
    assert_eq!(stdout.read(&mut trailing).unwrap(), 0);
    assert!(child.wait().unwrap().success());
    assert!(missing.is_empty(), "persistent text missing: {missing:?}");
    if let Some(baseline) = std::env::var_os("REEL_ASS_REGRESSION_BASELINE") {
        let baseline = std::path::PathBuf::from(baseline);
        for name in ["D.wav", "M.wav", "E.wav", "mix.wav"] {
            assert_eq!(
                fs::read(baseline.join(name)).unwrap(),
                fs::read(output.join(name)).unwrap(),
                "subtitle composition changed {name}"
            );
        }
    }
    scene_delivery::check(&job_path, &assets, &output).unwrap();
}

#[test]
fn timed_alpha_effect_changes_only_its_selected_frames() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let mut job = fixture(root);
    job["still_sequence_encoding"] = json!("h264-lossless");
    let mut contract: Value =
        serde_json::from_slice(&fs::read(root.join("contract.json")).unwrap()).unwrap();
    contract["attachments"].as_array_mut().unwrap().push(json!({
        "id":"timed-effect", "target":{"kind":"overlay","shot_id":"shot","overlay_id":"storm-glow"},
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
    // One effect begins on the red cel and stays active after the blue-cel cut.
    assert!(receipt.plan.external_layer_spans[0].start_frame < receipt.plan.pictures[0].end_frame);
    assert!(receipt.plan.external_layer_spans[0].end_frame > receipt.plan.pictures[1].start_frame);
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
fn two_timed_overlays_render_and_check_as_independent_semantic_layers() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let mut job = fixture(root);
    let mut contract: Value =
        serde_json::from_slice(&fs::read(root.join("contract.json")).unwrap()).unwrap();
    for (id, cue) in [("storm-light", "a"), ("storm-rain", "b")] {
        contract["attachments"].as_array_mut().unwrap().push(json!({
            "id":id,
            "target":{"kind":"overlay","shot_id":"shot","overlay_id":id},
            "start":{"kind":"cue-start","cue_id":cue,"offset_samples":6000},
            "end":{"kind":"cue-start","cue_id":cue,"offset_samples":18000}
        }));
    }
    write_json(&root.join("contract.json"), &contract);
    let effect = root.join("effect.mkv");
    assert!(
        Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-v",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "color=c=lime@0.75:s=64x64:r=24:d=1",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuva444p",
                "-frames:v",
                "24",
                "-y"
            ])
            .arg(&effect)
            .status()
            .unwrap()
            .success()
    );
    job["contract"] = file(root, "contract.json");
    job["external_layers"] = json!(
        ["storm-light", "storm-rain"]
            .into_iter()
            .map(|id| json!({
                "attachment_id":id,"reason":"Selected semantic storm effect",
                "evidence":file(root,"effect.mkv"),"render_mode":"timed-video-overlay"
            }))
            .collect::<Vec<_>>()
    );
    write_json(&root.join("job.json"), &job);
    let output = root.join("rendered");
    let receipt = scene_delivery::render(&root.join("job.json"), root, &output).unwrap();
    assert_eq!(
        receipt.plan.rendered_external_layers,
        vec!["storm-light", "storm-rain"]
    );
    assert!(receipt.outputs.contains_key("selected-overlay-000.mkv"));
    assert!(receipt.outputs.contains_key("selected-overlay-001.mkv"));
    assert!(receipt.outputs.contains_key("layered-picture-000.mkv"));
    scene_delivery::check(&root.join("job.json"), root, &output).unwrap();
    fs::write(output.join("selected-overlay-001.mkv"), b"tampered").unwrap();
    assert!(scene_delivery::check(&root.join("job.json"), root, &output).is_err());
}

#[test]
fn timed_effect_and_final_ass_preserve_text_timing_and_audio() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let mut job = fixture(root);
    let mut contract: Value =
        serde_json::from_slice(&fs::read(root.join("contract.json")).unwrap()).unwrap();
    contract["attachments"].as_array_mut().unwrap().push(json!({
        "id":"effect", "target":{"kind":"overlay","shot_id":"shot","overlay_id":"effect"},
        "start":{"kind":"cue-start","cue_id":"a","offset_samples":24000},
        "end":{"kind":"cue-start","cue_id":"b","offset_samples":24000}
    }));
    write_json(&root.join("contract.json"), &contract);
    assert!(
        Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "color=c=lime@0.75:s=64x64:r=24:d=2",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuva444p",
                "-n"
            ])
            .arg(root.join("effect.mkv"))
            .status()
            .unwrap()
            .success()
    );
    job["contract"] = file(root, "contract.json");
    job["external_layers"] = json!([{"attachment_id":"effect","reason":"Timed selected effect",
        "evidence":file(root,"effect.mkv"),"render_mode":"timed-video-overlay"}]);
    write_json(&root.join("baseline.json"), &job);
    let baseline = root.join("baseline");
    scene_delivery::render(&root.join("baseline.json"), root, &baseline).unwrap();
    contract["attachments"].as_array_mut().unwrap().push(json!({
        "id":"text", "target":{"kind":"caption","cue_id":"a","caption_id":"text"},
        "start":{"kind":"cue-start","cue_id":"a","offset_samples":0},
        "end":{"kind":"cue-end","cue_id":"b","offset_samples":0}
    }));
    write_json(&root.join("contract.json"), &contract);
    fs::write(root.join("text.ass"), "[Script Info]\nScriptType: v4.00+\nPlayResX: 64\nPlayResY: 64\n[V4+ Styles]\nFormat: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Outline,Shadow,Alignment,MarginL,MarginR,MarginV,Encoding\nStyle: Text,Arial,18,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n[Events]\nFormat: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text\nDialogue: 0,0:00:00.00,0:00:02.00,Text,,0,0,0,,READ\n").unwrap();
    job["contract"] = file(root, "contract.json");
    job["external_layers"].as_array_mut().unwrap().push(json!({"attachment_id":"text",
        "reason":"Final caption presentation above selected effect","evidence":file(root,"text.ass"),"render_mode":"ass-overlay"}));
    let font = [
        "C:/Windows/Fonts/arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ]
    .into_iter()
    .find(|path| Path::new(path).exists());
    if let Some(font) = font {
        fs::copy(font, root.join("selected-font.ttf")).unwrap();
        job["external_layers"][1]["font"] = file(root, "selected-font.ttf");
    }
    write_json(&root.join("mixed.json"), &job);
    let output = root.join("mixed");
    let receipt = scene_delivery::render(&root.join("mixed.json"), root, &output).unwrap();
    assert_eq!(
        receipt.plan.rendered_external_layers,
        vec!["effect", "text"]
    );
    assert_eq!(
        (
            receipt.plan.external_layer_spans[0].start_frame,
            receipt.plan.external_layer_spans[0].end_frame
        ),
        (12, 37)
    );
    assert!(output.join("layered-picture-000.mkv").exists());
    assert!(output.join("presentation-001.ass").exists());
    for name in ["D.wav", "M.wav", "E.wav", "mix.wav"] {
        assert_eq!(
            fs::read(baseline.join(name)).unwrap(),
            fs::read(output.join(name)).unwrap()
        );
    }
    let decode = |path: &Path| {
        let result = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(path)
            .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
            .output()
            .unwrap();
        assert!(result.status.success());
        result.stdout
    };
    let before = decode(&baseline.join("picture.mkv"));
    let after = decode(&output.join("picture.mkv"));
    assert_eq!(before.len(), 48 * 64 * 64 * 3);
    assert_eq!(before.len(), after.len());
    for (index, (a, b)) in before
        .chunks_exact(64 * 64 * 3)
        .zip(after.chunks_exact(64 * 64 * 3))
        .enumerate()
    {
        assert_eq!(
            &a[..64 * 3 * 8],
            &b[..64 * 3 * 8],
            "text changed protected top rows at frame {index}"
        );
        let white = b
            .chunks_exact(3)
            .filter(|pixel| pixel.iter().all(|&c| c > 200))
            .count();
        assert!(white > 20, "text obscured at frame {index}");
        assert_eq!(
            b[1] > 100,
            (12..37).contains(&index),
            "effect clock wrong at frame {index}"
        );
    }
    scene_delivery::check(&root.join("mixed.json"), root, &output).unwrap();
    let mut compact_job = job.clone();
    compact_job["composition_encoding"] = json!("h264-lossless");
    write_json(&root.join("compact-mixed.json"), &compact_job);
    let compact = root.join("compact-mixed");
    scene_delivery::render(&root.join("compact-mixed.json"), root, &compact).unwrap();
    scene_delivery::check(&root.join("compact-mixed.json"), root, &compact).unwrap();
    assert_lossless_composition_matches(
        &output,
        &compact,
        &["layered-picture-000.mkv", "picture.mkv", "master.mkv"],
    );
    let mut reversed = job.clone();
    reversed["external_layers"]
        .as_array_mut()
        .unwrap()
        .reverse();
    write_json(&root.join("reversed.json"), &reversed);
    assert!(
        scene_delivery::plan(&root.join("reversed.json"), root)
            .unwrap_err()
            .to_string()
            .contains("must follow")
    );
    fs::write(output.join("presentation-001.ass"), b"tampered").unwrap();
    assert!(scene_delivery::check(&root.join("mixed.json"), root, &output).is_err());
    if font.is_some() {
        let mut stale = job.clone();
        stale["external_layers"][1]["font"]["sha256"] = json!("0".repeat(64));
        write_json(&root.join("stale-font.json"), &stale);
        assert!(scene_delivery::plan(&root.join("stale-font.json"), root).is_err());
    }
}

#[test]
fn multiple_ass_presentations_require_one_combined_source() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let mut job = fixture(root);
    let mut contract: Value =
        serde_json::from_slice(&fs::read(root.join("contract.json")).unwrap()).unwrap();
    fs::write(
        root.join("text.ass"),
        "[Script Info]\nScriptType: v4.00+\n[Events]\n",
    )
    .unwrap();
    job["external_layers"] = json!([]);
    for id in ["text-a", "text-b"] {
        contract["attachments"].as_array_mut().unwrap().push(json!({
            "id":id,"target":{"kind":"title","title_id":id},
            "start":{"kind":"cue-start","cue_id":"a","offset_samples":0},
            "end":{"kind":"cue-end","cue_id":"b","offset_samples":0}
        }));
        job["external_layers"].as_array_mut().unwrap().push(json!({
            "attachment_id":id,"reason":"Competing presentation source",
            "evidence":file(root,"text.ass"),"render_mode":"ass-overlay"
        }));
    }
    write_json(&root.join("contract.json"), &contract);
    job["contract"] = file(root, "contract.json");
    write_json(&root.join("job.json"), &job);
    assert!(
        scene_delivery::plan(&root.join("job.json"), root)
            .unwrap_err()
            .to_string()
            .contains("combine text presentations")
    );
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
fn unknown_composition_encoding_rejects_before_output() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let mut job = fixture(root);
    let (legacy, _) = scene_delivery::plan(&root.join("job.json"), root).unwrap();
    assert!(
        serde_json::to_value(legacy)
            .unwrap()
            .get("composition_encoding")
            .is_none()
    );
    job["composition_encoding"] = json!("h264-lossy");
    write_json(&root.join("job.json"), &job);
    let output = root.join("invalid-encoding");
    assert!(
        scene_delivery::render(&root.join("job.json"), root, &output)
            .unwrap_err()
            .to_string()
            .contains("unsupported composition encoding")
    );
    assert!(!output.exists());
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
fn post_compose_camera_is_scoped_to_selected_frames_and_checked() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let mut job = fixture(root);
    let mut picture = b"P6\n64 64\n255\n".to_vec();
    for y in 0..64 {
        for x in 0..64 {
            picture.extend(if (x / 8 + y / 8) % 2 == 0 {
                [255, 40, 20]
            } else {
                [20, 40, 255]
            });
        }
    }
    fs::write(root.join("pattern.ppm"), picture).unwrap();
    fs::write(root.join("camera.json"), b"selected camera evidence").unwrap();
    job["pictures"][0]["source"] = file(root, "pattern.ppm");
    job["post_compose_camera"] = json!({
        "evidence":file(root,"camera.json"),
        "zoom_step":0.02,"zoom_max":1.5,
        "windows":[{"start_frame":0,"end_frame":24}]
    });
    write_json(&root.join("job.json"), &job);
    scene_delivery::plan(&root.join("job.json"), root).unwrap();
    let out = root.join("camera-render");
    scene_delivery::render(&root.join("job.json"), root, &out).unwrap();
    scene_delivery::check(&root.join("job.json"), root, &out).unwrap();
    assert!(out.join("pre-camera-picture.mkv").exists());
    let mut compact_job = job.clone();
    compact_job["composition_encoding"] = json!("h264-lossless");
    write_json(&root.join("compact-camera.json"), &compact_job);
    let compact = root.join("compact-camera");
    scene_delivery::render(&root.join("compact-camera.json"), root, &compact).unwrap();
    scene_delivery::check(&root.join("compact-camera.json"), root, &compact).unwrap();
    assert_lossless_composition_matches(
        &out,
        &compact,
        &["pre-camera-picture.mkv", "picture.mkv", "master.mkv"],
    );
    job["post_compose_camera"]["windows"][0]["end_frame"] = json!(49);
    write_json(&root.join("job.json"), &job);
    assert!(scene_delivery::plan(&root.join("job.json"), root).is_err());
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

#[test]
fn short_phased_camera_matches_every_authored_transition() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let mut job = fixture(root);
    let mut pixels = Vec::new();
    for y in 0..64 {
        for x in 0..64 {
            pixels.extend([
                if (x / 8 + y / 8) % 2 == 0 { 220u8 } else { 30 },
                (x * 3) as u8,
                (y * 3) as u8,
            ]);
        }
    }
    fs::write(
        root.join("pattern.ppm"),
        [b"P6\n64 64\n255\n".as_slice(), &pixels].concat(),
    )
    .unwrap();
    job["pictures"][0]["source"] = file(root, "pattern.ppm");
    job["still_sequence_encoding"] = json!("h264-lossless");
    let mut direction = phased_direction();
    direction.working_fps = 24;
    direction.duration_frames = 24;
    direction.elements[0].phases = serde_json::from_value(json!([
        {"id":"settle","kind":"settle","start_frame":0,"end_frame":7,"curve":"linear","zoom_from":1,"zoom_to":1.15},
        {"id":"read","kind":"hold","start_frame":8,"end_frame":23,"curve":"linear","zoom_from":1.15,"zoom_to":1.15}
    ])).unwrap();
    let motion = reel_assembly::motioncraft::compile(
        &direction,
        &reel_assembly::motioncraft::full_canvas(),
        48001,
        48000,
        24,
        1,
    )
    .unwrap();
    job["pictures"][0]["motion"] = json!({"kind":"phased-camera","plan":motion});
    write_json(&root.join("job.json"), &job);
    let output = root.join("directed");
    scene_delivery::render(&root.join("job.json"), root, &output).unwrap();
    // This renderer regression runs on Windows CI without WSL. The separate
    // cadence integration tests exercise the platform-specific luma analyzer.
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(output.join("picture.mkv"))
        .args(["-map", "0:v:0", "-f", "framemd5", "-"])
        .output()
        .unwrap();
    assert!(
        decoded.status.success(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    let hashes: Vec<_> = String::from_utf8(decoded.stdout)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| line.rsplit(',').next().unwrap().trim().to_owned())
        .collect();
    assert_eq!(hashes.len(), 48);
    for frame in 1..=7 {
        assert_ne!(
            hashes[frame - 1],
            hashes[frame],
            "moving transition {frame}"
        );
    }
    for frame in 8..24 {
        assert_eq!(hashes[7], hashes[frame], "declared hold frame {frame}");
    }
}

#[test]
#[ignore = "requires native FFmpeg"]
fn layer_cadence_rejects_frozen_effect_and_accepts_declared_hold() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let mut job = fixture(root);
    let mut contract: Value =
        serde_json::from_slice(&fs::read(root.join("contract.json")).unwrap()).unwrap();
    contract["frame_rate"]["numerator"] = json!(30);
    contract["attachments"].as_array_mut().unwrap().push(json!({
        "id":"timed-effect", "target":{"kind":"overlay","shot_id":"shot","overlay_id":"storm-glow"},
        "start":{"kind":"cue-start","cue_id":"a","offset_samples":0},
        "end":{"kind":"cue-start","cue_id":"a","offset_samples":48000}
    }));
    write_json(&root.join("contract.json"), &contract);
    assert!(
        Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "color=c=lime@0.75:s=64x64:r=30:d=1.0,format=yuva444p",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuva444p",
                "-frames:v",
                "30"
            ])
            .arg(root.join("effect.mkv"))
            .status()
            .unwrap()
            .success()
    );
    job["contract"] = file(root, "contract.json");
    job["external_layers"] = json!([{"attachment_id":"timed-effect","reason":"Layer validation regression",
        "evidence":file(root,"effect.mkv"),"render_mode":"timed-video-overlay"}]);
    let job_path = root.join("job.json");
    write_json(&job_path, &job);
    let render = root.join("render");
    scene_delivery::render(&job_path, root, &render).unwrap();
    let mut request = json!({"schema":"reel.motioncraft-layer-expectations.v1",
        "job_sha256":file(root,"job.json")["sha256"],
        "render_receipt_sha256":file(root,"render/receipt.json")["sha256"],
        "layers":[{"attachment_id":"timed-effect","source_sha256":file(root,"effect.mkv")["sha256"],
            "intervals":[{"start_frame":0,"end_frame":30,"kind":"moving",
                "region":{"x":0.0,"y":0.0,"width":1.0,"height":1.0}}]}]});
    let request_path = root.join("expectations.json");
    write_json(&request_path, &request);
    let report =
        reel::motioncraft_layers::analyze(&job_path, root, &render, &request_path).unwrap();
    assert_eq!(report["passed"], false, "{report}");
    assert_eq!(
        report["layers"][0]["intervals"][0]["status"], "failed-source-temporal-expectation",
        "{report}"
    );
    request["layers"][0]["intervals"][0]["kind"] = json!("hold");
    write_json(&request_path, &request);
    let report =
        reel::motioncraft_layers::analyze(&job_path, root, &render, &request_path).unwrap();
    assert_eq!(report["passed"], true, "{report}");
    request["layers"][0]["source_sha256"] = json!("0".repeat(64));
    write_json(&request_path, &request);
    assert!(
        reel::motioncraft_layers::analyze(&job_path, root, &render, &request_path)
            .unwrap_err()
            .to_string()
            .contains("stale selected carrier")
    );
}
