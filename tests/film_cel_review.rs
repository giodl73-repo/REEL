use reel::{
    film_presentation_overlay::review,
    scene_delivery::{self, FileRef, InputFiles, MediaProgram},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
fn save(root: &Path, name: &str, value: &impl serde::Serialize) {
    fs::write(root.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn reference(root: &Path, name: &str) -> Value {
    let path = root.join(name);
    let hash: String = Sha256::digest(fs::read(&path).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    json!({"path":name,"sha256":hash,"bytes":fs::metadata(path).unwrap().len()})
}
fn wav(path: &Path, samples: u32) {
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
        b.extend([0; 6]);
    }
    fs::write(path, b).unwrap();
}
fn fixture(root: &Path) -> Value {
    for (name, color) in [("red", [180u8, 30, 30]), ("blue", [20, 40, 170])] {
        fs::write(
            root.join(format!("{name}.ppm")),
            [b"P6\n2 2\n255\n".as_slice(), &color.repeat(4)].concat(),
        )
        .unwrap();
    }
    wav(&root.join("a.wav"), 48001);
    wav(&root.join("b.wav"), 47999);
    save(
        root,
        "production.json",
        &json!({"manifest_version":"reel.manifest.v0.2","profile":"animatic","timing_status":"conformed","work":"synthetic","title":"Synthetic review","scenes":[{"id":"scene"}],"shots":[{"id":"shot","scene_id":"scene"}],"narration_cues":[{"id":"a","speaker_id":"narrator"},{"id":"b","speaker_id":"narrator"}],"audio_events":[{"id":"a","role":"narration","source":"a.wav","start_seconds":0},{"id":"b","role":"narration","source":"b.wav","start_seconds":0}]}),
    );
    let anchor = |cue: &str, edge: &str| json!({"kind":format!("cue-{edge}"),"cue_id":cue});
    save(
        root,
        "contract.json",
        &json!({"schema":"reel.cue-relative-assembly.v0.1","id":"scene","production_manifest":"production.json","sample_rate":48000,"frame_rate":{"numerator":24,"denominator":1},"cues":[{"cue_id":"a","duration_samples":48001},{"cue_id":"b","duration_samples":47999}],"attachments":[
            {"id":"p-a","target":{"kind":"cel","shot_id":"shot","cel_id":"first"},"start":anchor("a","start"),"end":anchor("a","end")},
            {"id":"p-b","target":{"kind":"cel","shot_id":"shot","cel_id":"second"},"start":anchor("b","start"),"end":anchor("b","end")},
            {"id":"d-a","target":{"kind":"audio","audio_event_id":"a"},"start":anchor("a","start"),"end":anchor("a","end")},
            {"id":"d-b","target":{"kind":"audio","audio_event_id":"b"},"start":anchor("b","start"),"end":anchor("b","end")}
        ]}),
    );
    let job = json!({"schema":"reel.scene-delivery.v0.1","id":"scene","contract":reference(root,"contract.json"),"production_manifest_sha256":reference(root,"production.json")["sha256"],"width":320,"height":180,"max_composition_samples":96000,
        "pictures":[{"attachment_id":"p-a","source":reference(root,"red.ppm"),"kind":"still","attention":"Synthetic first cel","delivery_frame_count":24},{"attachment_id":"p-b","source":reference(root,"blue.ppm"),"kind":"still","attention":"Synthetic second cel","delivery_frame_count":24}],
        "audio":[{"attachment_id":"d-a","source":reference(root,"a.wav"),"bus":"D","cue_id":"a"},{"attachment_id":"d-b","source":reference(root,"b.wav"),"bus":"D","cue_id":"b"}],
        "buses":{"D":{"state":"present","reason":"Synthetic silence"},"M":{"state":"intentional-silence","reason":"No music"},"E":{"state":"intentional-silence","reason":"No effects"}}});
    save(root, "job.json", &job);
    scene_delivery::render(&root.join("job.json"), root, &root.join("source")).unwrap();
    let font = [
        "C:/Windows/Fonts/arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.is_file())
    .unwrap();
    fs::copy(font, root.join("selected-font.ttf")).unwrap();
    let request = json!({"schema":"reel.film-cel-review.v1","project":"synthetic-film","source_commit":"a".repeat(40),"language":"es","film":reference(root,"source/review.mp4"),"delivery_receipt":reference(root,"source/receipt.json"),"job":reference(root,"job.json"),"font":reference(root,"selected-font.ttf"),"font_size":18,
        "identities":{"p-a":{"story_id":"story-1","cel_id":"C100","semantic_id":"arrival"},"p-b":{"story_id":"story-1","cel_id":"C101","semantic_id":"departure"}},
        "limits":{"timeout_seconds":120,"input_bytes":32*1024*1024,"output_bytes":16*1024*1024,"probe_bytes":4*1024*1024,"frames":100}});
    save(root, "request.json", &request);
    request
}
fn manifest(root: &Path) -> FileRef {
    let mut file: FileRef = serde_json::from_value(reference(root, "request.json")).unwrap();
    file.path = root.join("request.json");
    file
}
fn frame(path: &Path, frame: u64) -> Vec<u8> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args([
            "-vf",
            &format!("select=eq(n\\,{frame})"),
            "-frames:v",
            "1",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), 320 * 180 * 3);
    output.stdout
}
#[test]
fn real_native_receipt_drives_visible_frame_exact_ids_with_unchanged_audio_and_timing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fixture(root);
    let request = manifest(root);
    let files = review::Local {
        request: &request,
        root,
    };
    let output = root.join("review");
    let receipt = match review::build_with_inputs(&request, &files, &output) {
        Ok(receipt) => receipt,
        Err(error) => {
            let evidence = temp.keep();
            panic!(
                "synthetic review failure preserved at {}: {error:#}",
                evidence.display()
            );
        }
    };
    review::check_with_inputs(&request, &files, &output).unwrap();
    assert_eq!(receipt.frame_count, 48);
    assert_eq!(receipt.source_clocks_sha256, receipt.output_clocks_sha256);
    assert!(
        !receipt.visual_reviewed
            && !receipt.approvals_verified
            && !receipt.native_retention_verified
    );
    let map: review::ReviewMap =
        serde_json::from_slice(&fs::read(output.join("review-map.json")).unwrap()).unwrap();
    assert_eq!(
        (
            map.targets[0].start_frame,
            map.targets[0].end_frame,
            map.targets[1].start_frame,
            map.targets[1].end_frame
        ),
        (0, 24, 24, 48)
    );
    let frames: Vec<_> = [0, 23, 24, 47]
        .into_iter()
        .map(|n| frame(&output.join("review.mp4"), n))
        .collect();
    // Frame 23 still carries C100; frame 24 carries C101, even though the cue
    // sample boundary is not exactly on frame 24. Inspect visible white glyphs.
    let mask = |raw: &Vec<u8>| {
        raw[..320 * 34 * 3]
            .chunks_exact(3)
            .map(|p| p.iter().all(|v| *v > 180))
            .collect::<Vec<_>>()
    };
    let masks: Vec<_> = frames.iter().map(mask).collect();
    let difference = |a: &Vec<bool>, b: &Vec<bool>| a.iter().zip(b).filter(|(a, b)| a != b).count();
    assert!(masks.iter().all(|m| m.iter().filter(|v| **v).count() > 100));
    assert!(difference(&masks[0], &masks[1]) < 20);
    assert!(difference(&masks[2], &masks[3]) < 20);
    assert!(difference(&masks[1], &masks[2]) > 20);
    if let Ok(path) = std::env::var("REEL_CEL_REVIEW_EVIDENCE") {
        let evidence = Path::new(&path);
        fs::create_dir(evidence).unwrap();
        for (n, pixels) in [0, 23, 24, 47].into_iter().zip(&frames) {
            image::save_buffer(
                evidence.join(format!("frame-{n}.png")),
                pixels,
                320,
                180,
                image::ColorType::Rgb8,
            )
            .unwrap();
        }
        fs::copy(
            output.join("review-map.json"),
            evidence.join("review-map.json"),
        )
        .unwrap();
        fs::copy(output.join("receipt.json"), evidence.join("receipt.json")).unwrap();
    }
    let original = fs::read(output.join("review.mp4")).unwrap();
    assert!(review::build_with_inputs(&request, &files, &output).is_err());
    assert_eq!(fs::read(output.join("review.mp4")).unwrap(), original);
    fs::write(output.join("label-0.txt"), "C999 | story-1").unwrap();
    assert!(review::check_with_inputs(&request, &files, &output).is_err());
}
#[test]
fn wrong_film_binding_missing_ids_and_declared_budget_refuse_before_creating_output() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let request = fixture(root);
    for case in [
        "output-namespace",
        "film-subject",
        "missing-id",
        "frames",
        "probe",
    ] {
        let mut bad = request.clone();
        match case {
            "output-namespace" => bad["film"]["path"] = json!("lookalike.mp4"),
            "film-subject" => bad["film"]["sha256"] = json!("f".repeat(64)),
            "missing-id" => {
                bad["identities"].as_object_mut().unwrap().remove("p-b");
            }
            "frames" => bad["limits"]["frames"] = json!(47),
            "probe" => bad["limits"]["probe_bytes"] = json!(1024),
            _ => unreachable!(),
        }
        save(root, "request.json", &bad);
        let pin = manifest(root);
        let files = review::Local {
            request: &pin,
            root,
        };
        let output = root.join(case);
        assert!(
            review::build_with_inputs(&pin, &files, &output).is_err(),
            "{case}"
        );
        assert!(!output.exists(), "{case}");
    }
}
#[test]
fn explicit_provider_tool_denial_has_no_ambient_fallback_and_cli_rejects_wrong_pin() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fixture(root);
    let pin = manifest(root);
    struct Denied<'a>(review::Local<'a>);
    impl InputFiles for Denied<'_> {
        fn verified_path(&self, item: &FileRef) -> anyhow::Result<PathBuf> {
            self.0.verified_path(item)
        }
        fn validate_reference_name(&self, item: &FileRef) -> anyhow::Result<()> {
            self.0.validate_reference_name(item)
        }
        fn media_program(&self, _: MediaProgram) -> anyhow::Result<PathBuf> {
            anyhow::bail!("synthetic denied program")
        }
    }
    let files = Denied(review::Local {
        request: &pin,
        root,
    });
    let output = root.join("denied");
    assert!(
        review::build_with_inputs(&pin, &files, &output)
            .unwrap_err()
            .to_string()
            .contains("synthetic denied program")
    );
    assert!(!output.exists());
    let result = Command::new(env!("CARGO_BIN_EXE_reel-film-review"))
        .arg("build")
        .arg(root.join("request.json"))
        .arg("f".repeat(64))
        .arg("--root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("cli-refused"))
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!root.join("cli-refused").exists());
    assert!(
        !String::from_utf8(result.stderr)
            .unwrap()
            .contains(root.to_str().unwrap())
    );
}
