use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn reference(root: &Path, name: &str) -> serde_json::Value {
    let bytes = fs::read(root.join(name)).unwrap();
    let sha = hash(&bytes);
    json!({"path":name,"sha256":sha,"bytes":bytes.len(),
        "cache_uri":format!("cache://sha256/{sha}")})
}

fn run(root: &Path, action: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_reel-presentation-still-sequence"))
        .arg(action)
        .arg(root.join("manifest.json"))
        .arg("--root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("render"))
        .output()
        .unwrap()
}

#[test]
fn two_stills_render_exact_picture_and_audio_clocks() {
    still_fixture(false, false);
}

#[test]
fn selected_audio_envelope_fades_actual_samples_and_rejects_font_tampering() {
    still_fixture(true, false);
}

#[test]
fn semantic_song_clock_renders_variable_picture_boundaries_without_restarting_audio() {
    still_fixture(true, true);
}

#[test]
fn portrait_fit_keeps_square_canvas_pixels() {
    still_geometry_fixture(false, false, true, false);
}

#[test]
fn non_square_source_is_fit_by_display_aspect_before_square_canvas() {
    still_geometry_fixture(false, false, true, true);
}

fn still_fixture(with_audio: bool, song_clock: bool) {
    still_geometry_fixture(with_audio, song_clock, false, false);
}

fn still_geometry_fixture(with_audio: bool, song_clock: bool, portrait: bool, non_square: bool) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for (name, color) in [("red.png", "red"), ("blue.png", "blue")] {
        let status = Command::new("ffmpeg")
            .args(["-v", "error", "-f", "lavfi", "-i"])
            .arg(format!(
                "color=c={color}:s={}:r=24:d=1",
                if portrait { "84x124" } else { "64x64" }
            ))
            .args(if non_square {
                vec!["-vf", "setsar=2/1"]
            } else {
                vec![]
            })
            .args(["-frames:v", "1"])
            .arg(root.join(name))
            .status()
            .unwrap();
        assert!(status.success());
    }
    fs::write(
        root.join("template.json"),
        serde_json::to_vec(&json!({
            "schema":"reel.presentation-still-template.v1",
            "template_id":"fixture","width":64,"height":64,
            "frame_rate":24,"sample_rate":48000,
            "picture_count":2,"frames_per_picture":24
        }))
        .unwrap(),
    )
    .unwrap();
    let ass = [
        "[Script Info]",
        "ScriptType: v4.00+",
        "PlayResX: 64",
        "PlayResY: 64",
        "[V4+ Styles]",
        "Format: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Outline,Shadow,Alignment,MarginL,MarginR,MarginV,Encoding",
        "Style: Default,Arial,12,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,2,2,2,2,1",
        "[Events]",
        "Format: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text",
        "Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,Caption",
    ]
    .join("\n");
    fs::write(root.join("layer.ass"), ass).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&json!({
            "schema":"reel.presentation-still-sequence.v1",
            "episode_id":"fixture-episode","language":"es","role":"captions",
            "template":reference(root,"template.json"),
            "pictures":[reference(root,"red.png"),reference(root,"blue.png")],
            "editable_layer":reference(root,"layer.ass")
        }))
        .unwrap(),
    )
    .unwrap();
    if with_audio {
        assert!(
            Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000:duration=2",
                    "-ac",
                    "2",
                    "-c:a",
                    "pcm_s24le"
                ])
                .arg(root.join("audio.wav"))
                .status()
                .unwrap()
                .success()
        );
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
        manifest["audio"] = reference(root, "audio.wav");
        manifest["audio_treatment"] =
            json!({"gain_db":-6.0,"fade_in_samples":24000,"fade_out_samples":24000});
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    }
    if with_audio {
        let mut template: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("template.json")).unwrap()).unwrap();
        template["audio_required"] = json!(true);
        fs::write(
            root.join("template.json"),
            serde_json::to_vec(&template).unwrap(),
        )
        .unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
        manifest["template"] = reference(root, "template.json");
        let mut silent = manifest.clone();
        silent.as_object_mut().unwrap().remove("audio");
        silent.as_object_mut().unwrap().remove("audio_treatment");
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&silent).unwrap(),
        )
        .unwrap();
        let rejected = run(root, "build");
        assert!(!rejected.status.success());
        assert!(String::from_utf8_lossy(&rejected.stderr).contains("requires a selected audio"));
        assert!(!root.join("render").exists());
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    }
    if song_clock {
        fs::write(
            root.join("anchor-evidence.json"),
            b"fixture measured source anchors",
        )
        .unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
        fs::write(root.join("clock.json"),serde_json::to_vec(&json!({"schema":"reel.presentation-source-clock.v1","source_sha256":manifest["audio"]["sha256"],"sample_rate":48000,"total_samples":96000,"evidence":reference(root,"anchor-evidence.json"),"anchors":[{"id":"intro","sample":0},{"id":"word","sample":12000},{"id":"outro","sample":96000}]})).unwrap()).unwrap();
        fs::write(root.join("timing.json"),serde_json::to_vec(&json!({"schema":"reel.presentation-picture-timing.v1","clock":reference(root,"clock.json"),"spans":[{"start_anchor":"intro","end_anchor":"word"},{"start_anchor":"word","end_anchor":"outro"}]})).unwrap()).unwrap();
        fs::write(root.join("template.json"),serde_json::to_vec(&json!({"schema":"reel.presentation-still-template.v2","template_id":"fixture","width":64,"height":64,"frame_rate":24,"sample_rate":48000,"picture_count":2,"total_frames":48})).unwrap()).unwrap();
        manifest["schema"] = json!("reel.presentation-still-sequence.v2");
        manifest.as_object_mut().unwrap().remove("audio_treatment");
        manifest["template"] = reference(root, "template.json");
        manifest["picture_timing"] = reference(root, "timing.json");
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    }
    let built = run(root, "build");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let checked = run(root, "check");
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("render/receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["frames"], 48);
    assert_eq!(receipt["samples"], 96_000);
    if portrait {
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height,sample_aspect_ratio",
                "-of",
                "json",
            ])
            .arg(root.join("render/master.mkv"))
            .output()
            .unwrap();
        assert!(probe.status.success());
        let facts: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
        assert_eq!(facts["streams"][0]["width"], 64);
        assert_eq!(facts["streams"][0]["height"], 64);
        assert_eq!(facts["streams"][0]["sample_aspect_ratio"], "1:1");
        let pixels = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(root.join("render/master.mkv"))
            .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
            .output()
            .unwrap();
        assert!(pixels.status.success());
        let border = if non_square { 32 * 3 } else { (10 * 64) * 3 };
        let center = (20 * 64 + 32) * 3;
        assert!(
            pixels.stdout[border..border + 3]
                .iter()
                .all(|value| *value < 5)
        );
        assert!(pixels.stdout[center] > 200);
    }
    if song_clock {
        assert_eq!(receipt["picture_frames"], json!([6, 42]));
        // Sample both sides of the semantic cut, away from the bottom caption.
        let decoded = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(root.join("render/master.mkv"))
            .args([
                "-vf",
                "crop=8:8:0:0",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "-",
            ])
            .output()
            .unwrap();
        assert!(decoded.status.success());
        let frame_bytes = 8 * 8 * 3;
        let red = &decoded.stdout[5 * frame_bytes..6 * frame_bytes];
        let blue = &decoded.stdout[6 * frame_bytes..7 * frame_bytes];
        assert!(red[0] > 200 && red[2] < 30);
        assert!(blue[2] > 200 && blue[0] < 30);
        let pcm = |path: &Path| {
            let output = Command::new("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(path)
                .args(["-map", "0:a:0", "-f", "s24le", "-acodec", "pcm_s24le", "-"])
                .output()
                .unwrap();
            assert!(output.status.success());
            output.stdout
        };
        assert_eq!(
            pcm(&root.join("audio.wav")),
            pcm(&root.join("render/master.mkv")),
            "picture cuts must preserve one continuous selected source recording"
        );
        fs::write(root.join("anchor-evidence.json"), b"tampered anchors").unwrap();
        assert!(!run(root, "check").status.success());
        fs::write(
            root.join("anchor-evidence.json"),
            b"fixture measured source anchors",
        )
        .unwrap();
    }
    if with_audio && !song_clock {
        let decoded = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(root.join("render/master.mkv"))
            .args(["-map", "0:a", "-f", "f32le", "-ac", "1", "-"])
            .output()
            .unwrap();
        assert!(decoded.status.success());
        let samples: Vec<f32> = decoded
            .stdout
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let rms = |slice: &[f32]| {
            (slice.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / slice.len() as f64).sqrt()
        };
        let center = rms(&samples[36000..48000]);
        assert!(
            center > 0.04 && center < 0.05,
            "gain must change actual PCM: {center}"
        );
        assert!(rms(&samples[..2400]) < center * 0.1);
        assert!(rms(&samples[93600..]) < center * 0.1);
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("manifest.json")).unwrap()).unwrap();
        fs::write(root.join("font.ttf"), b"fixture-font").unwrap();
        manifest["fonts"] = json!([reference(root, "font.ttf")]);
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(root.join("font.ttf"), b"tampered").unwrap();
        let rejected = run(root, "check");
        assert!(!rejected.status.success());
        assert!(
            String::from_utf8_lossy(&rejected.stderr).contains("hash, bytes or cache URI mismatch")
        );
    }
    fs::write(root.join("render/editable-layer.ass"), b"tampered").unwrap();
    assert!(!run(root, "check").status.success());
}
