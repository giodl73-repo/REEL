use super::*;
use std::path::PathBuf;
fn input(path: &Path) -> Input {
    let p = object(path, 2 * 1024 * 1024 * 1024).unwrap();
    Input {
        path: path.into(),
        sha256: p.sha256,
        bytes: p.bytes,
    }
}
fn palette() -> Palette {
    Palette {
        paper: "#F5EAD4".into(),
        ink: "#3D2B24".into(),
        accent: "#2C6462".into(),
        secondary: "#A95F46".into(),
        rule: "#C4AE87".into(),
    }
}
fn style(family: &str) -> Style {
    Style {
        body_family: family.into(),
        heading_family: family.into(),
        heading_bold: false,
        track_title_bold: false,
        footer: "PRIVATE FIXTURE".into(),
        byline: "".into(),
        left_heading: "ESPAÑOL".into(),
        right_heading: "ENGLISH".into(),
        instrumental_left: "Instrumental".into(),
        instrumental_right: "Music without words".into(),
        wordless_left: "Coro sin palabras".into(),
        wordless_right: "Wordless chorus".into(),
    }
}
fn save(path: &Path, value: &Value) -> Input {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    input(path)
}
#[test]
fn wrapping_preserves_source_and_layout_controls_refuse() {
    let pair = json!({"es":"La palma canta suavemente","en":"The palm sings softly","display_lines":{"es":["La palma","canta suavemente"],"en":["The palm","sings softly"]}});
    assert_eq!(layout::lines(&pair, "es").unwrap().len(), 2);
    let mut altered = pair.clone();
    altered["display_lines"]["es"] = json!(["La palma cambia"]);
    assert!(layout::lines(&altered, "es").is_err());
    let bad = json!({"es":"texto {\\pos(0,0)}"});
    assert!(layout::lines(&bad, "es").is_err());
}
#[test]
fn palette_and_integer_label_clock_are_stable() {
    assert_eq!(layout::color("#F5EAD4").unwrap(), "F5EAD4");
    assert!(layout::color("red:filters").is_err());
    assert_eq!(layout::clock(28800), "0:00:00.60");
    assert_eq!(layout::clock(3600 * RATE), "1:00:00.00");
}
fn frame(q: &Request, movie: &Path, seconds: &str) -> Vec<u8> {
    let mut c = cmd(q);
    c.args(["-v", "error", "-ss", seconds]);
    media_input(&mut c, movie);
    c.args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]);
    capture(
        &mut c,
        1920 * 1080 * 3,
        Instant::now() + Duration::from_secs(30),
    )
    .unwrap()
    .0
}
fn ink(raw: &[u8], x: usize, y0: usize, y1: usize) -> usize {
    let mut n = 0;
    for y in y0..y1 {
        for xx in x..x + 792 {
            let p = (y * 1920 + xx) * 3;
            if raw[p] < 100 && raw[p + 1] < 90 && raw[p + 2] < 80 {
                n += 1;
            }
        }
    }
    n
}
#[test]
fn real_bilingual_video_preserves_audio_chapters_and_phrase_pages() {
    let (Some(ff), Some(fp), Some(font), Some(family)) = (
        std::env::var_os("REEL_TEST_FFMPEG"),
        std::env::var_os("REEL_TEST_FFPROBE"),
        std::env::var_os("REEL_TEST_FONT"),
        std::env::var_os("REEL_TEST_FONT_FAMILY"),
    ) else {
        return;
    };
    let t = tempfile::tempdir().unwrap();
    let ff = PathBuf::from(ff);
    let fp = PathBuf::from(fp);
    let font = PathBuf::from(font);
    let family = family.to_string_lossy().to_string();
    let mut command = Command::new(&ff);
    command
        .args([
            "-v",
            "error",
            "-nostdin",
            "-n",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=330:sample_rate=48000:duration=6.01",
            "-af",
            "atrim=end_sample=288001",
            "-ac",
            "2",
            "-c:a",
            "libmp3lame",
            "-b:a",
            "320k",
        ])
        .arg(t.path().join("album.mp3"));
    capture(&mut command, 0, Instant::now() + Duration::from_secs(30)).unwrap();
    fs::write(t.path().join("chapters.ffmetadata"),";FFMETADATA1\ntitle=Álbum del sueño\n[CHAPTER]\nTIMEBASE=1/48000\nSTART=0\nEND=144000\ntitle=Primero\n[CHAPTER]\nTIMEBASE=1/48000\nSTART=144000\nEND=288001\ntitle=Después\n").unwrap();
    let audio = input(&t.path().join("album.mp3"));
    let chapters = input(&t.path().join("chapters.ffmetadata"));
    let ledger = json!({"schema":"reel.local-song-album-master-check.v1","title":"Álbum del sueño","frames":288001,"sample_rate":48000,"channels":2,
        "tracks":[{"song_id":"one","title":"Primero","start_frame":0,"frames":144000},{"song_id":"two","title":"Después","start_frame":144000,"frames":144001}],
        "outputs":{"master.mp3":pin(&audio),"chapters.ffmetadata":pin(&chapters)}});
    let album_receipt = save(&t.path().join("album.json"), &ledger);
    let reading = json!({"schema":"reel.album-bilingual-reading.v1","mode":"editorial-reading-pages","source_album_check":pin(&album_receipt),"palette":palette(),
        "tracks":[{"song_id":"one","mode":"sung","pages":[{"start_frame":0,"end_frame":144000,"lines":[{"es":"Un recuerdo delicado de una mañana junto al mar de Cuba","en":"A delicate memory of the quiet and beautiful Cuban countryside","display_lines":{"es":["Un recuerdo delicado","de una mañana junto","al mar de Cuba"],"en":["A delicate memory of","the quiet and beautiful","Cuban countryside"]}},{"es":"La palma canta","en":"The palm sings"}]}]},
        {"song_id":"two","mode":"instrumental","pages":[]}]});
    let reading_pin = save(&t.path().join("reading.json"), &reading);
    let mut q = Request {
        schema: "reel.album-video.v1".into(),
        title: "Álbum del sueño".into(),
        ffmpeg: input(&ff),
        ffprobe: input(&fp),
        audio,
        chapters,
        album_receipt,
        album_request: None,
        reading_plan: Some(reading_pin),
        fonts: vec![Font {
            file: input(&font),
            family: family.clone(),
        }],
        style: style(&family),
        palette: None,
        permissions: Permissions {
            private_only: true,
            external_upload: false,
            public_release: false,
        },
        limits: Limits {
            max_input_bytes: 1024 * 1024 * 1024,
            max_output_bytes: 64 * 1024 * 1024,
            timeout_seconds: 120,
        },
    };
    let request = t.path().join("video.json");
    fs::write(&request, serde_json::to_vec(&q).unwrap()).unwrap();
    plan(&request).unwrap();
    let first = render(&request, Path::new("first")).unwrap();
    assert_eq!(first["observation"]["audio_packet_payloads_exact"], true);
    assert_eq!(first["observation"]["decoded_pcm"]["bytes"], 288001 * 8);
    assert_eq!(first["observation"]["source_discard_padding_frames"], 46);
    assert!(matches!(
        first["observation"]["decoder_exposed_padding_frames"].as_u64(),
        Some(0 | 46)
    ));
    check(&request, Path::new("first")).unwrap();
    let pixels = frame(&q, &t.path().join("first/master.mp4"), "1");
    assert_eq!(pixels.len(), 1920 * 1080 * 3);
    for left in [118, 1010] {
        assert!(ink(&pixels, left, 335, 932) > 1000);
        let mut groups = 0;
        let mut previous = None;
        for row in 335..932 {
            if ink(&pixels, left, row, row + 1) > 3 {
                if previous.is_none_or(|p| row - p > 12) {
                    groups += 1;
                }
                previous = Some(row);
            }
        }
        assert_eq!(
            groups, 4,
            "both reading columns must preserve four distinct wrapped rows"
        );
    }
    if let Some(directory) = std::env::var_os("REEL_TEST_ALBUM_VIDEO_ARTIFACTS") {
        let directory = PathBuf::from(directory);
        assert!(directory.is_absolute() && directory.is_dir());
        let preview = directory.join("editorial.png");
        assert!(!preview.exists());
        image::RgbImage::from_raw(1920, 1080, pixels.clone())
            .unwrap()
            .save(preview)
            .unwrap();
    }
    let original_movie = fs::read(t.path().join("first/master.mp4")).unwrap();
    let original_receipt = fs::read(t.path().join("first/receipt.json")).unwrap();
    let shifted = t.path().join("shifted.mp4");
    let mut delayed = cmd(&q);
    delayed
        .args(["-v", "error", "-i"])
        .arg(t.path().join("first/master.mp4"))
        .args(["-itsoffset", "0.2", "-i"])
        .arg(t.path().join("first/master.mp4"))
        .args([
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-map_metadata",
            "0",
            "-map_chapters",
            "0",
            "-c",
            "copy",
        ])
        .arg(&shifted);
    capture(&mut delayed, 0, Instant::now() + Duration::from_secs(30)).unwrap();
    fs::write(
        t.path().join("first/master.mp4"),
        fs::read(&shifted).unwrap(),
    )
    .unwrap();
    let mut forged: Value = serde_json::from_slice(&original_receipt).unwrap();
    forged["outputs"]["master.mp4"] =
        serde_json::to_value(object(&shifted, q.limits.max_output_bytes).unwrap()).unwrap();
    fs::write(
        t.path().join("first/receipt.json"),
        serde_json::to_vec(&forged).unwrap(),
    )
    .unwrap();
    let refusal = check(&request, Path::new("first")).unwrap_err();
    assert!(
        refusal.to_string().contains("timeline"),
        "unexpected shifted-audio refusal: {refusal:#}"
    );
    fs::write(t.path().join("first/master.mp4"), original_movie).unwrap();
    fs::write(t.path().join("first/receipt.json"), original_receipt).unwrap();
    let mut crowded = reading.clone();
    crowded["tracks"][0]["pages"][0]["lines"] =
        json!(vec![json!({"es":"Una palabra","en":"One word"}); 8]);
    let (_, _, _held, a, _) = load(&request).unwrap();
    assert!(layout::compile(&q, &a, Some(&crowded)).is_err());
    q.style.heading_bold = true;
    assert!(
        layout::compile(&q, &a, Some(&reading)).is_err(),
        "a regular file must not silently supply an undeclared bold face"
    );
    q.style.heading_bold = false;
    let mut performed = reading.clone();
    performed["mode"] = json!("performed-phrase-pages");
    performed["tracks"][0]["pages"] = json!([
        {"start_frame":28800,"end_frame":67200,"lines":[{"es":"Primera frase","en":"First phrase"}],"phrase_evidence":[{"start":0.8,"end":1.3}]},
        {"start_frame":105600,"end_frame":134400,"lines":[{"es":"Segunda frase","en":"Second phrase"}],"phrase_evidence":[{"start":2.4,"end":2.7}]}]);
    q.reading_plan = Some(save(&t.path().join("performed.json"), &performed));
    let request2 = t.path().join("video2.json");
    fs::write(&request2, serde_json::to_vec(&q).unwrap()).unwrap();
    assert!(
        check(&request2, Path::new("first")).is_err(),
        "revised text/clock plan must invalidate old packet"
    );
    render(&request2, Path::new("performed")).unwrap();
    for (time, visible) in [("0.4", false), ("0.6", true), ("1.8", false), ("2.2", true)] {
        let image = frame(&q, &t.path().join("performed/master.mp4"), time);
        for left in [118, 1010] {
            assert_eq!(ink(&image, left, 335, 440) > 500, visible, "{time} {left}");
        }
    }
    fs::write(t.path().join("performed/labels.ass"), b"changed").unwrap();
    assert!(check(&request2, Path::new("performed")).is_err());
    q.reading_plan = None;
    q.palette = Some(palette());
    let request3 = t.path().join("labels-only.json");
    fs::write(&request3, serde_json::to_vec(&q).unwrap()).unwrap();
    assert_eq!(plan(&request3).unwrap()["canvas"], json!([1280, 720]));
    render(&request3, Path::new("labels-only")).unwrap();
    check(&request3, Path::new("labels-only")).unwrap();
}
