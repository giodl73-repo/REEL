use super::*;
fn input(path: &Path) -> Input {
    let o = object(path, 2 * 1024 * 1024 * 1024).unwrap();
    Input {
        path: path.into(),
        sha256: o.sha256,
        bytes: o.bytes,
    }
}
fn request(ffmpeg: Input, tracks: Vec<Track>) -> Request {
    Request {
        schema: "reel.music-album-master.v1".into(),
        title: "Álbum de prueba".into(),
        delivery_stem: "Álbum".into(),
        ffmpeg,
        permissions: Permissions {
            private_only: true,
            external_upload: false,
            public_release: false,
        },
        gap_frames: 2 * RATE,
        target_lufs: -16.0,
        source_peak_ceiling_dbtp: -2.0,
        limits: Limits {
            max_input_bytes: 1024 * 1024 * 1024,
            max_track_frames: 6 * RATE,
            max_total_frames: 12 * RATE,
            max_output_bytes: 16 * 1024 * 1024,
            timeout_seconds: 120,
        },
        tracks,
    }
}
fn fixture_input() -> Input {
    Input {
        path: PathBuf::from("/fixture"),
        sha256: "a".repeat(64),
        bytes: 1,
    }
}
fn track() -> Track {
    Track {
        asset_id: "song/one".into(),
        revision: "v1".into(),
        title: "Canción".into(),
        source: fixture_input(),
    }
}
#[test]
fn scope_order_limits_and_gain_are_explicit() {
    let mut q = request(fixture_input(), vec![track()]);
    validate(&q).unwrap();
    q.permissions.external_upload = true;
    assert!(validate(&q).is_err());
    q.permissions.external_upload = false;
    q.tracks.push(track());
    assert!(validate(&q).is_err());
    q.tracks.pop();
    q.limits.max_output_bytes = 10;
    assert!(validate(&q).is_err());
    q.limits.max_output_bytes = 16 * 1024 * 1024;
    q.limits.max_input_bytes = 0;
    assert!(validate(&q).is_err());
    assert_eq!(gain(-25.0, -3.0, -16.0, -2.0), 1.0);
    assert_eq!(gain(-12.0, -1.0, -16.0, -2.0), -4.0);
}
#[test]
fn quantization_has_declared_ties_and_rejects_overflow() {
    assert_eq!(pcm24(0.0, 1.0).unwrap(), [0, 0, 0]);
    assert_eq!(pcm24(0.5, 1.0).unwrap(), [0, 0, 64]);
    assert_eq!(pcm24(-0.5, 1.0).unwrap(), [0, 0, 192]);
    assert_eq!(pcm24(0.5 / 8388608.0, 1.0).unwrap(), [0, 0, 0]);
    assert_eq!(pcm24(1.5 / 8388608.0, 1.0).unwrap(), [2, 0, 0]);
    assert!(pcm24(f32::NAN, 1.0).is_err());
    assert!(pcm24(1.0, 1.0).is_err());
    assert!(pcm24(0.99999994, 1.0000001).is_err());
}
#[test]
fn duplicate_keys_and_stale_input_refuse_before_output() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("request.json");
    fs::write(&p, b"{\"schema\":\"a\",\"schema\":\"b\"}").unwrap();
    assert!(plan(&p).is_err());
    let q = request(fixture_input(), vec![track()]);
    fs::write(&p, serde_json::to_vec(&q).unwrap()).unwrap();
    let output = t.path().join("output");
    assert!(render(&p, &output).is_err());
    assert!(!output.exists());
}
fn wave(path: &Path, hz: f64) {
    let rate = 44100u32;
    let frames = rate * 5;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + frames * 4).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 4).to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(frames * 4).to_le_bytes());
    for i in 0..frames {
        let t = i as f64 / rate as f64;
        let pulse = if i > frames - rate / 4 {
            0.025 * (t * 880.0 * std::f64::consts::TAU).sin()
        } else {
            0.0
        };
        let n = ((0.09 * (t * hz * std::f64::consts::TAU).sin() + pulse) * 32767.0).round() as i16;
        bytes.extend_from_slice(&n.to_le_bytes());
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    fs::write(path, bytes).unwrap();
}
#[test]
fn real_ffmpeg_complete_tail_gap_recheck_tamper_and_timeout() {
    let Some(ff) = std::env::var_os("REEL_TEST_FFMPEG") else {
        return;
    };
    let ff = PathBuf::from(ff);
    assert!(
        ff.is_absolute(),
        "explicit absolute FFmpeg fixture required"
    );
    let t = tempfile::tempdir().unwrap();
    let one = t.path().join("one.wav");
    let two = t.path().join("two.wav");
    wave(&one, 330.0);
    wave(&two, 440.0);
    let mut tracks = vec![
        Track {
            source: input(&one),
            ..track()
        },
        Track {
            asset_id: "song/two".into(),
            source: input(&two),
            ..track()
        },
    ];
    tracks[0].source.path = PathBuf::from("one.wav");
    let q = request(input(&ff), tracks);
    let p = t.path().join("album.json");
    fs::write(&p, serde_json::to_vec(&q).unwrap()).unwrap();
    let output = t.path().join("packet");
    plan(&p).unwrap();
    let report = render(&p, &output).unwrap();
    assert_eq!(report["frames"], 12 * RATE);
    assert!(!output.join("master.pcm").exists());
    let receipt: Receipt =
        serde_json::from_slice(&fs::read(output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt.tracks[0].start_frame, 0);
    assert_eq!(receipt.tracks[1].start_frame, 7 * RATE);
    assert_eq!(receipt.tracks[0].frames, 5 * RATE);
    assert_ne!(
        receipt.tracks[0].mastered_pcm24_sha256,
        receipt.tracks[1].mastered_pcm24_sha256
    );
    check(&p, &output).unwrap();
    assert!(render(&p, &output).is_err());
    let original = fs::read(output.join("receipt.json")).unwrap();
    let mut forged: Value = serde_json::from_slice(&original).unwrap();
    forged["tracks"][0]["static_gain_db"] = json!(0);
    fs::write(
        output.join("receipt.json"),
        serde_json::to_vec(&forged).unwrap(),
    )
    .unwrap();
    assert!(check(&p, &output).is_err());
    fs::write(output.join("receipt.json"), original).unwrap();
    let original_receipt = fs::read(output.join("receipt.json")).unwrap();
    let original_chapters = fs::read(output.join("chapters.ffmetadata")).unwrap();
    fs::write(
        output.join("chapters.ffmetadata"),
        b";FFMETADATA1\ntitle=forged\n",
    )
    .unwrap();
    let mut forged: Value = serde_json::from_slice(&original_receipt).unwrap();
    forged["outputs"]["chapters.ffmetadata"] =
        serde_json::to_value(object(&output.join("chapters.ffmetadata"), 1024).unwrap()).unwrap();
    fs::write(
        output.join("receipt.json"),
        serde_json::to_vec(&forged).unwrap(),
    )
    .unwrap();
    assert!(
        check(&p, &output).is_err(),
        "a matching forged output hash must not bless false chapter metadata"
    );
    fs::write(output.join("receipt.json"), original_receipt).unwrap();
    fs::write(output.join("chapters.ffmetadata"), original_chapters).unwrap();
    assert!(
        fs::read_to_string(output.join("chapters.cue"))
            .unwrap()
            .contains("FILE \"master.flac\" WAVE")
    );
    OpenOptions::new()
        .append(true)
        .open(output.join("master.mp3"))
        .unwrap()
        .write_all(b"tamper")
        .unwrap();
    assert!(check(&p, &output).is_err());
    let mut c = command(&q);
    c.args([
        "-v",
        "error",
        "-re",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440",
        "-f",
        "null",
        "-",
    ]);
    let start = Instant::now();
    assert!(capture(&mut c, 0, start + Duration::from_millis(200)).is_err());
    assert!(start.elapsed() < Duration::from_secs(5));
}
