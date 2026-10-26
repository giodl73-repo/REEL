use reel::episode_captions::assemble;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
fn sha(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn write(root: &Path, name: &str, v: &Value) -> Value {
    let b = serde_json::to_vec(v).unwrap();
    fs::write(root.join(name), &b).unwrap();
    json!({"path":name,"bytes":b.len(),"sha256":sha(&b)})
}
fn fixture(root: &Path, lang: &str, target_fps: u64) -> (Value, Value, Value) {
    let dummy = json!({"path":"unused-master","sha256":"a".repeat(64),"bytes":123});
    let provenance = write(
        root,
        "selection.json",
        &json!({"owner":"fixture","approval":false}),
    );
    let mut segments = vec![];
    let mut records = vec![];
    let mut tracks = vec![];
    let mut frame = 0;
    let mut sample = 0;
    for i in 0..27 {
        let id = format!("unit-{i}");
        let host = i % 2 == 0;
        let input_fps = if host { 25 } else { 24 };
        // 1.04s hosts -> 32 repeated frames, original audio 49920 samples; films 1s.
        let input_frames = if host { 26 } else { 24 };
        let frames = (input_frames * target_fps).div_ceil(input_fps);
        let per_frame = 48000 / target_fps;
        let hold = 2 * target_fps;
        let input_samples = if host { 49920 } else { 48000 };
        let pad = frames * per_frame - input_samples;
        let text = if lang == "es" {
            "¡Canción del niño!\n  Texto exacto."
        } else {
            "The child's song!\n  Exact text."
        };
        let source = format!("1\n00:00:00,000 --> 00:00:01,000\n{text}\n");
        let name = format!("{i}.srt");
        fs::write(root.join(&name), &source).unwrap();
        let caption = json!({"path":name,"bytes":source.len(),"sha256":sha(source.as_bytes())});
        let kind = if host {
            "scene"
        } else {
            "episode-presentation"
        };
        segments.push(json!({"kind":kind,"id":id,"master":dummy,"source_receipt":dummy,"tail_hold_frames":hold}));
        records.push(json!({"kind":kind,"id":id,"selected_master_sha256":dummy["sha256"],"selected_master_bytes":123,"source_receipt_sha256":dummy["sha256"],"upstream_delivery_verified":host,"upstream_presentation_verified":!host,"input_sample_rate":48000,"output_sample_rate":48000,"input_frames":input_frames,"input_samples":input_samples,"frames":frames+hold,"samples":(frames+hold)*per_frame,"audio_padding_samples":pad,"tail_hold_frames":hold,"tail_hold_samples":96000,"start_frame":frame,"start_sample":sample,"decoded_picture_sha256":"b".repeat(64),"decoded_audio_sha256":"c".repeat(64),"frame_rate_verification":{"input_fps":format!("{input_fps}/1"),"output_fps":format!("{target_fps}/1"),"input_frames":input_frames,"output_frames":frames,"repeated_frames":frames-input_frames,"transformed_source_matches_selected":true}}));
        tracks.push(json!({"segment_id":id,"language":lang,"selected_master_sha256":dummy["sha256"],"source_caption":caption,"source_words_sha256":sha(&serde_json::to_vec(&vec![text]).unwrap()),"selection_evidence":provenance,"source_frame_rate":{"numerator":input_fps,"denominator":1,"policy":"repeat-only"},"vtt_line_percent":if host {json!(70)} else {Value::Null}}));
        frame += frames + hold;
        sample += (frames + hold) * per_frame;
    }
    let conform = json!({"schema":"reel.episode-conform.v1","episode_id":"episode","language":lang,"output_sample_rate":48000,"output_frame_rate":{"numerator":target_fps,"denominator":1,"policy":"repeat-only"},"catalog":dummy,"master_template_definition":dummy,"episode":dummy,"season_bindings":dummy,"episode_bindings":dummy,"audio_frame_conform":"pad-silence-to-picture-boundaries","segments":segments});
    let cref = write(root, "conform.json", &conform);
    let receipt = json!({"schema":"reel.episode-conform-receipt.v1","episode_id":"episode","language":lang,"manifest_sha256":cref["sha256"],"episode_presentation_fingerprint_sha256":"d".repeat(64),"ffmpeg_version":"fixture","output_sample_rate":48000,"total_frames":frame,"total_samples":sample,"master_sha256":"e".repeat(64),"master_bytes":123,"decoded_master_matches_ordered_segments":true,"timestamps_verified":true,"boundary_findings":[],"boundary_review_state":"open","external_layer_review_state":"open","upstream_delivery_recheck_state":"verified-for-all-scene-segments","upstream_presentation_recheck_state":"verified-for-all-presentation-segments","creative_review_state":"open","segments":records,"publication":"not-authorized"});
    let rref = write(root, "receipt.json", &receipt);
    let manifest = json!({"schema":"reel.episode-captions.v1","language":lang,"conform_manifest":cref,"conform_receipt":rref,"tracks":tracks});
    write(root, "captions.json", &manifest);
    (manifest, conform, receipt)
}
#[test]
fn bilingual_27_segments_use_native_sample_clock_and_preserve_words_and_style() {
    for lang in ["es", "en"] {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        let (m, _, r) = fixture(root, lang, 30);
        let output = root.join("out");
        assemble(&root.join("captions.json"), root, &output).unwrap();
        let srt = fs::read_to_string(output.join("captions.srt")).unwrap();
        let cues = reel::series::parse_srt(&srt).unwrap();
        assert_eq!(cues.len(), 27);
        for (i, cue) in cues.iter().enumerate() {
            let start = r["segments"][i]["start_sample"].as_u64().unwrap();
            assert_eq!(cue.start_ms, (start * 1000).div_ceil(48000));
            assert!(cue.text.contains(if lang == "es" {
                "¡Canción del niño!"
            } else {
                "The child's song!"
            }));
            assert!(cue.text.contains("\n  "));
        }
        let vtt = fs::read_to_string(output.join("captions.vtt")).unwrap();
        assert_eq!(vtt.matches("line:70%").count(), 14);
        let report: Value =
            serde_json::from_slice(&fs::read(output.join("caption-timing.json")).unwrap()).unwrap();
        assert_eq!(report["total_samples"], r["total_samples"]);
        assert_eq!(report["conform_receipt"], m["conform_receipt"]);
        assert!(assemble(&root.join("captions.json"), root, &output).is_err());
    }
}
#[test]
fn rejects_corruption_scope_order_clock_and_hold_leaks_before_writing() {
    for case in 0..18 {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        let (mut m, mut c, mut r) = fixture(root, "es", 30);
        match case {
            0 => m["language"] = json!("en"),
            1 => {
                m["tracks"].as_array_mut().unwrap().pop();
            }
            2 => m["tracks"][0]["source_caption"]["sha256"] = json!("f".repeat(64)),
            3 => m["tracks"][0]["source_words_sha256"] = json!("f".repeat(64)),
            4 => m["tracks"][0]["source_caption"]["path"] = json!("../0.srt"),
            5 => r["segments"][1]["start_sample"] = json!(1),
            6 => r["segments"][0]["tail_hold_samples"] = json!(0),
            7 => m["tracks"][0]["source_frame_rate"]["numerator"] = json!(24),
            8 => r["manifest_sha256"] = json!("f".repeat(64)),
            9 => r["segments"][0]["upstream_delivery_verified"] = json!(false),
            10 => {
                m["tracks"].as_array_mut().unwrap().swap(0, 1);
            }
            11 => {
                let bytes = b"1\n00:00:00,000 --> 00:00:02,000\nunchanged\n";
                fs::write(root.join("0.srt"), bytes).unwrap();
                m["tracks"][0]["source_caption"] =
                    json!({"path":"0.srt","bytes":bytes.len(),"sha256":sha(bytes)});
                m["tracks"][0]["source_words_sha256"] =
                    json!(sha(&serde_json::to_vec(&vec!["unchanged"]).unwrap()));
            }
            12 => {
                c["segments"][0]["tail_hold_frames"] = json!(0);
                m["conform_manifest"] = write(root, "conform.json", &c);
                r["manifest_sha256"] = m["conform_manifest"]["sha256"].clone();
            }
            13 => {
                let bytes=b"1\n00:00:00,000 --> 00:00:00,700\none\n\n2\n00:00:00,500 --> 00:00:01,000\ntwo\n";
                fs::write(root.join("0.srt"), bytes).unwrap();
                m["tracks"][0]["source_caption"] =
                    json!({"path":"0.srt","bytes":bytes.len(),"sha256":sha(bytes)});
                m["tracks"][0]["source_words_sha256"] =
                    json!(sha(&serde_json::to_vec(&vec!["one", "two"]).unwrap()));
            }
            14 => m["tracks"][0]["source_frame_rate"]["denominator"] = json!(0),
            15 => m["tracks"][0]["selected_master_sha256"] = json!("f".repeat(64)),
            16 => m["tracks"][0]["language"] = json!("en"),
            17 => r["total_samples"] = json!(1),
            _ => unreachable!(),
        }
        m["conform_receipt"] = write(root, "receipt.json", &r);
        write(root, "captions.json", &m);
        let out = root.join("out");
        assert!(
            assemble(&root.join("captions.json"), root, &out).is_err(),
            "case {case}"
        );
        assert!(!out.exists(), "case {case} wrote output");
    }
}

#[test]
fn repeat_only_24_to_25_preserves_full_source_and_two_second_holds() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fixture(root, "es", 25);
    assemble(&root.join("captions.json"), root, &root.join("out")).unwrap();
    let report: Value =
        serde_json::from_slice(&fs::read(root.join("out/caption-timing.json")).unwrap()).unwrap();
    assert_eq!(report["tracks"][1]["tail_hold_samples"], 96000);
    assert_eq!(report["tracks"][1]["content_samples"], 48000);
}

/// Private retained controls are staged explicitly; this test never discovers cache roots,
/// invokes a renderer, or copies media. Source JSON cues are converted only inside this test.
#[test]
#[ignore = "requires explicitly staged hash-verified private source controls"]
fn retained_native_caption_family_comparison() {
    let input = std::path::PathBuf::from(
        std::env::var_os("REEL_CAPTION_PRIVATE_FIXTURE").expect("explicit fixture root"),
    );
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let conform: Value =
        serde_json::from_slice(&fs::read(input.join("conform.json")).unwrap()).unwrap();
    let receipt: Value =
        serde_json::from_slice(&fs::read(input.join("receipt.json")).unwrap()).unwrap();
    let hosts: Value =
        serde_json::from_slice(&fs::read(input.join("hosts.json")).unwrap()).unwrap();
    let films: Value =
        serde_json::from_slice(&fs::read(input.join("films.json")).unwrap()).unwrap();
    let lang = conform["language"].as_str().unwrap();
    let journey = std::env::var("REEL_CAPTION_PRIVATE_JOURNEY").unwrap();
    let provenance = write(
        root,
        "provenance.json",
        &json!({"fixture":"read-only retained source controls; human approval not inferred"}),
    );
    let mut tracks = vec![];
    for (i, native) in receipt["segments"].as_array().unwrap().iter().enumerate() {
        let id = native["id"].as_str().unwrap();
        let host = native["kind"] == "scene";
        let entries = if host {
            let unit = id.split('/').next_back().unwrap();
            let take = format!("{journey}-{unit}-{lang}");
            hosts["takes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["take_id"] == take)
                .unwrap()["cues"]
                .as_array()
                .unwrap()
        } else {
            films["tracks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| {
                    t["video_id"] == id.strip_prefix("film-").unwrap() && t["language"] == lang
                })
                .unwrap()["cues"]
                .as_array()
                .unwrap()
        };
        let cues: Vec<reel::series::SrtEntry> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| reel::series::SrtEntry {
                index: i + 1,
                start_ms: (e["start"].as_f64().unwrap() * 1000.).round() as u64,
                end_ms: (e["end"].as_f64().unwrap() * 1000.).round() as u64,
                text: e["text"].as_str().unwrap().to_owned(),
            })
            .collect();
        let source = reel::series::render_srt(&cues);
        let name = format!("{i}.srt");
        fs::write(root.join(&name), &source).unwrap();
        let input_fps = native["frame_rate_verification"]["input_fps"]
            .as_str()
            .unwrap_or_else(|| if journey == "j01" { "30/1" } else { "25/1" });
        let (n, d) = input_fps.split_once('/').unwrap();
        tracks.push(json!({"segment_id":id,"language":lang,"selected_master_sha256":native["selected_master_sha256"],"source_caption":{"path":name,"bytes":source.len(),"sha256":sha(source.as_bytes())},"source_words_sha256":sha(&serde_json::to_vec(&cues.iter().map(|e|&e.text).collect::<Vec<_>>()).unwrap()),"selection_evidence":provenance,"source_frame_rate":{"numerator":n.parse::<u32>().unwrap(),"denominator":d.parse::<u32>().unwrap(),"policy":"repeat-only"},"vtt_line_percent":if host {json!(70)} else {Value::Null}}));
    }
    let cref = write(root, "conform.json", &conform); // preserve exact conform bytes: receipt pins them
    let original = fs::read(input.join("conform.json")).unwrap();
    fs::write(root.join("conform.json"), &original).unwrap();
    let cref = json!({"path":cref["path"],"bytes":original.len(),"sha256":sha(&original)});
    let rref = write(root, "receipt.json", &receipt);
    write(
        root,
        "captions.json",
        &json!({"schema":"reel.episode-captions.v1","language":lang,"conform_manifest":cref,"conform_receipt":rref,"tracks":tracks}),
    );
    assemble(&root.join("captions.json"), root, &root.join("out")).unwrap();
    let actual =
        reel::series::parse_srt(&fs::read_to_string(root.join("out/captions.srt")).unwrap())
            .unwrap();
    let expected =
        reel::series::parse_srt(&fs::read_to_string(input.join("expected.srt")).unwrap()).unwrap();
    assert_eq!(actual.len(), expected.len());
    for (a, e) in actual.iter().zip(&expected) {
        assert_eq!(a.text, e.text);
        assert!(a.start_ms.abs_diff(e.start_ms) <= 1);
        assert!(a.end_ms.abs_diff(e.end_ms) <= 1);
    }
    let vtt = fs::read_to_string(root.join("out/captions.vtt")).unwrap();
    let old_vtt = fs::read_to_string(input.join("expected.vtt")).unwrap();
    assert_eq!(
        vtt.matches(" line:70%").count(),
        old_vtt.matches(" line:70%").count()
    );
    eprintln!(
        "retained native {journey}-{lang}: {} exact cue payloads, timing <=1ms, inherited host VTT line:70% preserved",
        actual.len()
    );
}
