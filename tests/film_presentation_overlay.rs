use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

fn command(name: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(name);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

fn write(root: &Path, name: &str, value: &Value) {
    fs::write(root.join(name), serde_json::to_vec(value).unwrap()).unwrap();
}

fn reference(root: &Path, name: &str) -> Value {
    let bytes = fs::read(root.join(name)).unwrap();
    json!({"path":name,"sha256":Sha256::digest(&bytes).iter().map(|b|format!("{b:02x}")).collect::<String>(),"bytes":bytes.len()})
}

fn fixture(root: &Path) -> Value {
    let result = command("ffmpeg")
        .current_dir(root)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=navy:s=160x90:r=24:d=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=330:sample_rate=48000:duration=10",
            "-vf",
            "settb=1/12288,setpts=PTS+262",
            "-fps_mode:v",
            "passthrough",
            "-enc_time_base:v",
            "1:12288",
            "-video_track_timescale",
            "12288",
            "-movie_timescale",
            "12288",
            "-c:v",
            "libx264",
            "-bf",
            "0",
            "-c:a",
            "aac",
            "-avoid_negative_ts",
            "disabled",
            "film.mp4",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    // A fixture font is cache-independent. Prefer a standard TrueType font on each CI host.
    let font = [
        "C:/Windows/Fonts/arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ]
    .iter()
    .map(Path::new)
    .find(|p| p.exists())
    .expect("fixture TrueType font");
    fs::copy(font, root.join("font.ttf")).unwrap();
    fs::write(root.join("chapter.ass"), "[Script Info]\nScriptType: v4.00+\nPlayResX: 160\nPlayResY: 90\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,14,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,2,4,4,4,1\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:04.00,Default,,0,0,0,,Chapter — 1927\n").unwrap();
    let film = reference(root, "film.mp4");
    write(
        root,
        "timeline.json",
        &json!({"schema":"reel.source-film-timeline.v1","source_film_sha256":film["sha256"],"frame_count":240,"fps_numerator":24,"fps_denominator":1,"units":[{"scene_id":"scene-two","first_picture_slot_id":"picture-first","start_frame":24,"frame_count":120}]}),
    );
    write(
        root,
        "target.json",
        &json!({"scene_id":"scene-two","language_event_bindings":{"es":[{"picture_slot_id":"picture-first"}]}}),
    );
    write(
        root,
        "presentation.json",
        &json!({"scene_id":"chapter-unit","presentation":{"template_id":"chapter-master","placement":{"mode":"overlay-on-target-scene-start","target_scene_id":"scene-two","target_picture_slot_id":"picture-first"}}}),
    );
    write(
        root,
        "template.json",
        &json!({"template_id":"chapter-master","kind":"chapter-title","fixed_duration_seconds":4}),
    );
    let layer = reference(root, "chapter.ass");
    let template = reference(root, "template.json");
    write(
        root,
        "layer-receipt.json",
        &json!({"schema":"reel.editable-layer-compile-receipt.v1","scene_id":"chapter-unit","language":"es","template_id":"chapter-master","template_definition_sha256":template["sha256"],"ass_sha256":layer["sha256"],"ass_bytes":layer["bytes"],"sample_rate":100,"duration_samples":400}),
    );
    json!({"schema":"reel.film-presentation-overlay.v1","language":"es","film":film,"timeline":reference(root,"timeline.json"),"overlays":[{"presentation_scene":reference(root,"presentation.json"),"target_scene":reference(root,"target.json"),"template":template,"template_receipt":reference(root,"layer-receipt.json"),"layer":layer,"font":reference(root,"font.ttf")}]})
}

fn pixels(path: &Path, frame: u64) -> Vec<u8> {
    let result = command("ffmpeg")
        .args(["-v", "error", "-i"])
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
    assert!(result.status.success());
    result.stdout
}

#[test]
fn chapter_overlay_preserves_nonzero_picture_origin_and_aac_packets() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let manifest = fixture(root);
    write(root, "manifest.json", &manifest);
    reel::film_presentation_overlay::build(&root.join("manifest.json"), root, &root.join("render"))
        .unwrap();
    reel::film_presentation_overlay::check(&root.join("manifest.json"), root, &root.join("render"))
        .unwrap();
    let receipt: Value =
        serde_json::from_slice(&fs::read(root.join("render/receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["frame_count"], 240);
    assert_eq!(receipt["placements"][0]["start_frame"], 24);
    assert_eq!(receipt["placements"][0]["end_frame"], 120);
    assert_eq!(receipt["placements"][0]["start_centiseconds"], 102);
    assert_eq!(receipt["placements"][0]["end_centiseconds"], 502);
    let probe = command("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_frames",
            "-show_entries",
            "frame=best_effort_timestamp",
            "-of",
            "json",
        ])
        .arg(root.join("film.mp4"))
        .output()
        .unwrap();
    assert!(probe.status.success());
    let frames: Value = serde_json::from_slice(&probe.stdout).unwrap();
    assert_eq!(frames["frames"][0]["best_effort_timestamp"], 262);
    assert_eq!(
        receipt["source_clocks_sha256"],
        receipt["output_clocks_sha256"]
    );
    let pcm_hash = |path: &Path| {
        let result = command("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(path)
            .args([
                "-map",
                "0:a:0",
                "-c:a",
                "pcm_s24le",
                "-f",
                "hash",
                "-hash",
                "SHA256",
                "-",
            ])
            .output()
            .unwrap();
        assert!(result.status.success());
        result.stdout
    };
    assert_eq!(
        pcm_hash(&root.join("film.mp4")),
        pcm_hash(&root.join("render/review.mp4"))
    );
    for (frame, visible) in [
        (23, false),
        (24, true),
        (72, true),
        (119, true),
        (120, false),
    ] {
        let a = pixels(&root.join("film.mp4"), frame);
        let b = pixels(&root.join("render/review.mp4"), frame);
        let changed = a
            .iter()
            .zip(b)
            .filter(|(x, y)| (i16::from(**x) - i16::from(*y)).abs() > 30)
            .count();
        assert_eq!(
            changed > 100,
            visible,
            "title boundary frame {frame}: {changed} changed channels"
        );
    }
    let mut tampered = receipt;
    tampered["placements"][0]["start_frame"] = json!(25);
    write(&root.join("render"), "receipt.json", &tampered);
    assert!(
        reel::film_presentation_overlay::check(
            &root.join("manifest.json"),
            root,
            &root.join("render")
        )
        .is_err()
    );
}

#[test]
fn wrong_target_scope_and_stale_source_are_rejected_before_render() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut manifest = fixture(root);
    write(
        root,
        "target.json",
        &json!({"scene_id":"scene-two","language_event_bindings":{"es":[{"picture_slot_id":"wrong-picture"}]}}),
    );
    manifest["overlays"][0]["target_scene"] = reference(root, "target.json");
    write(root, "manifest.json", &manifest);
    assert!(
        reel::film_presentation_overlay::build(
            &root.join("manifest.json"),
            root,
            &root.join("render")
        )
        .is_err()
    );
    assert!(!root.join("render").exists());
    manifest["film"]["sha256"] = json!("0".repeat(64));
    write(root, "manifest.json", &manifest);
    assert!(
        reel::film_presentation_overlay::build(
            &root.join("manifest.json"),
            root,
            &root.join("render")
        )
        .is_err()
    );
    assert!(!root.join("render").exists());
}

#[test]
fn local_layer_timing_cannot_escape_the_chapter_master() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut manifest = fixture(root);
    let ass = fs::read_to_string(root.join("chapter.ass"))
        .unwrap()
        .replace("0:00:04.00", "0:00:05.00");
    fs::write(root.join("chapter.ass"), ass).unwrap();
    manifest["overlays"][0]["layer"] = reference(root, "chapter.ass");
    let mut compiled: Value =
        serde_json::from_slice(&fs::read(root.join("layer-receipt.json")).unwrap()).unwrap();
    compiled["ass_sha256"] = manifest["overlays"][0]["layer"]["sha256"].clone();
    compiled["ass_bytes"] = manifest["overlays"][0]["layer"]["bytes"].clone();
    write(root, "layer-receipt.json", &compiled);
    manifest["overlays"][0]["template_receipt"] = reference(root, "layer-receipt.json");
    write(root, "manifest.json", &manifest);
    let error = reel::film_presentation_overlay::build(
        &root.join("manifest.json"),
        root,
        &root.join("render"),
    )
    .err()
    .unwrap();
    assert!(
        error
            .to_string()
            .contains("chapter ASS must contain only local 0-4"),
        "{error:#}"
    );
    assert!(!root.join("render").exists());
}

#[test]
fn multiple_chapter_units_use_their_own_scene_frame_scopes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut manifest = fixture(root);
    let mut timeline: Value =
        serde_json::from_slice(&fs::read(root.join("timeline.json")).unwrap()).unwrap();
    timeline["units"].as_array_mut().unwrap().push(json!({"scene_id":"scene-three","first_picture_slot_id":"picture-other","start_frame":144,"frame_count":96}));
    write(root, "timeline.json", &timeline);
    manifest["timeline"] = reference(root, "timeline.json");
    write(
        root,
        "target-other.json",
        &json!({"scene_id":"scene-three","language_event_bindings":{"es":[{"picture_slot_id":"picture-other"}]}}),
    );
    write(
        root,
        "presentation-other.json",
        &json!({"scene_id":"chapter-other","presentation":{"template_id":"chapter-master","placement":{"mode":"overlay-on-target-scene-start","target_scene_id":"scene-three","target_picture_slot_id":"picture-other"}}}),
    );
    let mut compiled: Value =
        serde_json::from_slice(&fs::read(root.join("layer-receipt.json")).unwrap()).unwrap();
    compiled["scene_id"] = json!("chapter-other");
    write(root, "layer-receipt-other.json", &compiled);
    let mut other = manifest["overlays"][0].clone();
    other["presentation_scene"] = reference(root, "presentation-other.json");
    other["target_scene"] = reference(root, "target-other.json");
    other["template_receipt"] = reference(root, "layer-receipt-other.json");
    manifest["overlays"].as_array_mut().unwrap().push(other);
    write(root, "manifest.json", &manifest);
    reel::film_presentation_overlay::build(&root.join("manifest.json"), root, &root.join("render"))
        .unwrap();
    reel::film_presentation_overlay::check(&root.join("manifest.json"), root, &root.join("render"))
        .unwrap();
    let receipt: Value =
        serde_json::from_slice(&fs::read(root.join("render/receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["placements"][1]["start_frame"], 144);
    assert_eq!(receipt["placements"][1]["end_frame"], 240);
    for (frame, visible) in [(120, false), (143, false), (144, true), (239, true)] {
        let a = pixels(&root.join("film.mp4"), frame);
        let b = pixels(&root.join("render/review.mp4"), frame);
        let changed = a
            .iter()
            .zip(b)
            .filter(|(x, y)| (i16::from(**x) - i16::from(*y)).abs() > 30)
            .count();
        assert_eq!(changed > 100, visible, "frame {frame}");
    }
}

#[test]
fn changed_audio_timestamps_are_rejected_even_after_refreshing_movie_hash() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let manifest = fixture(root);
    write(root, "manifest.json", &manifest);
    reel::film_presentation_overlay::build(&root.join("manifest.json"), root, &root.join("render"))
        .unwrap();
    let movie = root.join("render/review.mp4");
    let shifted = root.join("shifted.mp4");
    let output = command("ffmpeg")
        .args(["-v", "error", "-copyts", "-i"])
        .arg(&movie)
        .args(["-itsoffset", "0.125", "-i"])
        .arg(&movie)
        .args([
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-c",
            "copy",
            "-avoid_negative_ts",
            "disabled",
        ])
        .arg(&shifted)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::copy(&shifted, &movie).unwrap();
    let mut receipt: Value =
        serde_json::from_slice(&fs::read(root.join("render/receipt.json")).unwrap()).unwrap();
    let media = reference(root, "render/review.mp4");
    receipt["output_sha256"] = media["sha256"].clone();
    receipt["output_bytes"] = media["bytes"].clone();
    write(&root.join("render"), "receipt.json", &receipt);
    let error = reel::film_presentation_overlay::check(
        &root.join("manifest.json"),
        root,
        &root.join("render"),
    )
    .err()
    .unwrap();
    assert!(
        error
            .to_string()
            .contains("film overlay receipt/output closure mismatch"),
        "{error:#}"
    );
}
