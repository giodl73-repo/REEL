use super::*;
fn save(root: &Path, name: &str, v: &Value) -> Pin {
    let raw = serde_json::to_vec_pretty(v).unwrap();
    fs::write(root.join(name), &raw).unwrap();
    pin(&raw)
}
fn fixture(root: &Path) -> Value {
    let text = "Alpha beta\nDelta echo\n";
    fs::write(root.join("source.txt"), text).unwrap();
    // Independently constructed PCM16 WAVE: 30 seconds, mono, quarter scale.
    let size = 30 * 8000 * 2u32;
    let mut raw = b"RIFF".to_vec();
    raw.extend((size + 36).to_le_bytes());
    raw.extend(b"WAVEfmt ");
    raw.extend(16u32.to_le_bytes());
    raw.extend(1u16.to_le_bytes());
    raw.extend(1u16.to_le_bytes());
    raw.extend(8000u32.to_le_bytes());
    raw.extend(16000u32.to_le_bytes());
    raw.extend(2u16.to_le_bytes());
    raw.extend(16u16.to_le_bytes());
    raw.extend(b"data");
    raw.extend(size.to_le_bytes());
    for _ in 0..30 * 8000 {
        raw.extend(8192i16.to_le_bytes());
    }
    fs::write(root.join("source.wav"), &raw).unwrap();
    let recording = pin(&raw);
    let original = json!({"schema":"reel.local-song-request.v1","task":"text2music","language":"es",
        "candidate_id":"opaque-original","duration_seconds":30,"seed":1,"caption":"Invented original musical brief",
        "canonical_lyrics":text,"render_lyrics":"[Verse]\nAlpha beta\nDelta echo\n[Outro]",
        "source":pin(text.as_bytes()),"source_lock":{"logical_id":"opaque-text","version_id":"v1","manifest_sha256":"a".repeat(64)},
        "permissions":{"private_only":true,"external_upload":false,"identifiable_imitation":false,"public_release":false}});
    let request = save(root, "source-request.json", &original);
    let prior = json!({"schema":"reel.song-lyric-diagnostic.v1","request":request,"recording":recording,
        "candidate_id":"opaque-original","recognition_settings":{"canonical_prompt":false},
        "lines":[{"canonical":"Alpha beta"},{"canonical":"Delta echo"}],
        "passes":[{"name":"opaque-pass","model":"synthetic-fixture-only","tokens":[
            {"word":"alpha","span":[0.25,0.5],"confidence":0.9},
            {"word":"beta","span":[0.75,1.0]},
            {"word":"noise","span":[1.74,1.8]},
            {"word":"delta","span":[3.25,3.5]},
            {"word":"echo","span":[4.0,4.25]}]}]});
    let analysis = save(root, "source-analysis.json", &prior);
    let generation = save(
        root,
        "source-generation.json",
        &json!({"schema":"reel.local-song-generation.v1", "request":request,"recording":recording,"candidate_id":"opaque-original"}),
    );
    let recipe = json!({"schema":"reel.song-short-edit.v1","task":"short-edit","candidate_id":"opaque-edition",
        "source_recording":recording,"source_request":request,"source_analysis":analysis,"source_generation":generation,
        "source":original["source"],"source_lock":original["source_lock"],"permissions":original["permissions"],
        "caption":original["caption"],"source_generation_duration_seconds":30,"duration_seconds":3.75,
        "segments":[[0.0,2.0],[3.0,5.0]],"crossfade_seconds":0.25,"fade_in_seconds":0.125,"fade_out_seconds":0.125,"max_duration_seconds":4});
    save(root, "edit.json", &recipe);
    recipe
}
#[test]
fn retained_source_recipe_produces_float_wave_and_exact_original_word_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fixture(root);
    let request = root.join("edit.json");
    let output = root.join("edition");
    let before = pin(&fs::read(root.join("source.wav")).unwrap());
    assert_eq!(plan(&request).unwrap()["output_frames"], 30000);
    let result = render(&request, &output).unwrap();
    assert_eq!(result["verified"], true);
    assert_eq!(result["lyric_gate_passed"], true);
    assert_eq!(result["new_recognition"], false);
    assert_eq!(result["human_approval"], false);
    let wav = fs::read(output.join("recording.wav")).unwrap();
    assert_eq!(&wav[20..22], &3u16.to_le_bytes());
    assert_eq!(wav.len(), 120056);
    assert_eq!(&wav[56 + 1000 * 4..56 + 1001 * 4], &0.25f32.to_le_bytes());
    let analysis = parse(&fs::read(output.join("analysis.json")).unwrap()).unwrap();
    assert_eq!(analysis["passes"][0]["text"], "alpha beta delta echo");
    assert_eq!(
        analysis["passes"][0]["tokens"][2]["span"],
        json!([2.0, 2.25])
    );
    assert_eq!(
        analysis["passes"][0]["source_pass_metadata"]["model"],
        "synthetic-fixture-only"
    );
    assert_eq!(analysis["continuous_order_passed"], true);
    assert!(check(&request, &output).is_ok());
    assert!(render(&request, &output).is_err());
    assert_eq!(before, pin(&fs::read(root.join("source.wav")).unwrap()));
}
#[test]
fn immutable_lineage_permissions_and_prompt_provenance_fail_before_output_creation() {
    for case in 0..5 {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let mut recipe = fixture(root);
        match case {
            0 => recipe["permissions"]["public_release"] = json!(true),
            1 => recipe["caption"] = json!("Different actual generation brief"),
            2 => recipe["source_lock"]["version_id"] = json!("v2"),
            3 => {
                let path = root.join("source-analysis.json");
                let mut prior = parse(&fs::read(path).unwrap()).unwrap();
                prior["recognition_settings"]["canonical_prompt"] = json!(true);
                recipe["source_analysis"] =
                    serde_json::to_value(save(root, "source-analysis.json", &prior)).unwrap();
            }
            _ => {
                fs::write(root.join("source-request.json"), b"{}").unwrap();
            }
        }
        save(root, "edit.json", &recipe);
        let output = root.join("refused");
        assert!(
            render(&root.join("edit.json"), &output).is_err(),
            "case {case}"
        );
        assert!(!output.exists());
    }
}
#[test]
fn output_audio_analysis_and_receipt_tampering_never_requalify() {
    for case in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fixture(root);
        let request = root.join("edit.json");
        let output = root.join("edition");
        render(&request, &output).unwrap();
        match case {
            0 => {
                let path = output.join("recording.wav");
                let mut raw = fs::read(&path).unwrap();
                raw[56 + 1000 * 4] ^= 1;
                fs::write(path, raw).unwrap();
            }
            1 => {
                let path = output.join("analysis.json");
                let mut raw = fs::read(&path).unwrap();
                raw.push(b' ');
                fs::write(path, raw).unwrap();
            }
            _ => {
                let path = output.join("edit-receipt.json");
                let mut value = parse(&fs::read(&path).unwrap()).unwrap();
                value["new_human_approval"] = json!(true);
                save(&output, "edit-receipt.json", &value);
            }
        }
        assert!(check(&request, &output).is_err(), "case {case}");
    }
}
#[test]
fn missing_whole_lines_remain_failed_candidates_without_inventing_recognition() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let mut recipe = fixture(root);
    recipe["segments"] = json!([[0.0, 2.0]]);
    recipe["duration_seconds"] = json!(2.0);
    recipe["crossfade_seconds"] = json!(0.0);
    save(root, "edit.json", &recipe);
    let r = render(&root.join("edit.json"), &root.join("held")).unwrap();
    assert_eq!(r["verified"], true);
    assert_eq!(r["lyric_gate_passed"], false);
    assert_eq!(r["qualifying_lines"], 1);
    assert_eq!(r["new_performance"], false);
    assert_eq!(r["human_approval"], false);
}

#[test]
fn declared_word_gap_cap_is_distinct_from_line_presence_and_cannot_be_ignored() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let mut recipe = fixture(root);
    let text = "Alpha beta gamma\nDelta echo\n";
    fs::write(root.join("source.txt"), text).unwrap();
    let mut request = parse(&fs::read(root.join("source-request.json")).unwrap()).unwrap();
    request["source"] = serde_json::to_value(pin(text.as_bytes())).unwrap();
    request["canonical_lyrics"] = json!(text);
    request["render_lyrics"] = json!(text);
    let source_request = save(root, "source-request.json", &request);
    let mut prior = parse(&fs::read(root.join("source-analysis.json")).unwrap()).unwrap();
    prior["request"] = serde_json::to_value(&source_request).unwrap();
    prior["lines"][0]["canonical"] = json!("Alpha beta gamma");
    let mut generation = parse(&fs::read(root.join("source-generation.json")).unwrap()).unwrap();
    generation["request"] = serde_json::to_value(&source_request).unwrap();
    recipe["source"] = request["source"].clone();
    recipe["source_request"] = serde_json::to_value(source_request).unwrap();
    recipe["source_analysis"] =
        serde_json::to_value(save(root, "source-analysis.json", &prior)).unwrap();
    recipe["source_generation"] =
        serde_json::to_value(save(root, "source-generation.json", &generation)).unwrap();
    recipe["max_one_word_gap_lines"] = json!(0);
    save(root, "edit.json", &recipe);
    let result = render(&root.join("edit.json"), &root.join("held")).unwrap();
    assert_eq!(result["line_gate_passed"], true);
    assert_eq!(result["lyric_gate_passed"], false);
    assert_eq!(result["human_approval"], false);
    assert_eq!(
        plan(&root.join("edit.json")).unwrap()["lyric_gate_passed"],
        false
    );
}
