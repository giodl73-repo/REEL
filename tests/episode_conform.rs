use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

fn write(path: &Path, value: &serde_json::Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn reference(root: &Path, name: &str) -> serde_json::Value {
    let bytes = fs::read(root.join(name)).unwrap();
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    serde_json::json!({"path":name,"sha256":sha,"bytes":bytes.len()})
}

fn ffmpeg() -> Command {
    #[allow(unused_mut)] // Windows adds CREATE_NO_WINDOW to this command.
    let mut cmd = Command::new("ffmpeg");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

fn source(root: &Path, name: &str, color: &str, sample_rate: u32) {
    let status = ffmpeg()
        .args(["-v", "error", "-f", "lavfi", "-i"])
        .arg(format!("color=c={color}:s=64x64:r=24:d=1"))
        .args(["-f", "lavfi", "-i"])
        .arg(format!(
            "sine=frequency=400:sample_rate={sample_rate}:duration=1"
        ))
        .args([
            "-ac",
            "2",
            "-shortest",
            "-c:v",
            if name == "credits.mkv" {
                "libx264"
            } else {
                "ffv1"
            },
            "-pix_fmt",
            "yuv444p",
            "-c:a",
            "pcm_s24le",
        ])
        .arg(root.join(name))
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn lossless_episode_conform_handles_presentation_and_explicit_audio_normalization() {
    let temp = tempfile::tempdir().unwrap();
    let work = temp.path().join("John's episode");
    fs::create_dir(&work).unwrap();
    let root = work.as_path();
    source(root, "opening.mkv", "black", 48_000);
    source(root, "scene-a.mkv", "red", 24_000);
    source(root, "chapter.mkv", "yellow", 48_000);
    source(root, "scene-b.mkv", "blue", 48_000);
    source(root, "credits.mkv", "green", 48_000);
    write(
        &root.join("source-master.json"),
        &serde_json::json!({
            "schema":"owner.episode-master.v1","ordered_roles":["series-opening","opening-poem","chapter-title","chapter-scenes","end-credits"]
        }),
    );
    let source_master_sha = reference(root, "source-master.json")["sha256"].clone();
    write(
        &root.join("master-template.json"),
        &serde_json::json!({
        "schema":"reel.episode-master-template.v1","template_id":"master",
        "ordered_roles":["series-opening","opening-poem","chapter-title","chapter-scenes","end-credits"],
        "optional_roles":[],"source_template_sha256":source_master_sha
        }),
    );
    let master_definition_sha = reference(root, "master-template.json")["sha256"].clone();
    write(
        &root.join("catalog.json"),
        &serde_json::json!({
            "schema":"reel.scene-template-catalog.v1","templates":[
            {"template_id":"master","kind":"episode-master","definition_sha256":master_definition_sha,"required_content_keys":[]},
                {"template_id":"opening","kind":"series-opening","definition_sha256":"b".repeat(64),"required_content_keys":["opening_id"]},
                {"template_id":"chapter","kind":"chapter-title","definition_sha256":"d".repeat(64),"required_content_keys":["chapter_number"]},
                {"template_id":"credits","kind":"end-credits","definition_sha256":"c".repeat(64),"required_content_keys":["billing_id"]}
            ]
        }),
    );
    write(
        &root.join("episode.json"),
        &serde_json::json!({
            "schema":"reel.episode-authoring.v1","episode_id":"episode","season_id":"season",
            "authoring_state":"ready-for-private-build","master_template_id":"master",
            "scene_policy_id":"narrative","score_palette":[],"scene_ids":["scene-a","scene-b"],
            "presentation":[
                {"role":"series-opening","template_id":"opening","content":{"opening_id":"season"},"asset_binding":"opening-master"},
                {"role":"chapter-title","template_id":"chapter","content":{"chapter_number":"1"},"asset_binding":"chapter-master"},
                {"role":"end-credits","template_id":"credits","content":{"billing_id":"episode"},"asset_binding":"credits-master"}
            ]
        }),
    );
    write(
        &root.join("season.json"),
        &serde_json::json!({
            "schema":"reel.scene-asset-bindings.v1","scope_id":"season","assets":{}
        }),
    );
    let asset = |name: &str, path: &str| {
        let r = reference(root, path);
        serde_json::json!({"logical_id":name,"sha256":r["sha256"],"bytes":r["bytes"],
            "cache_uri":format!("cache://sha256/{}",r["sha256"].as_str().unwrap()),
            "selection_state":"selected-private-production"})
    };
    write(
        &root.join("episode-bindings.json"),
        &serde_json::json!({
            "schema":"reel.scene-asset-bindings.v1","scope_id":"episode","assets":{
                "opening-master":asset("opening-master","opening.mkv"),
                "chapter-master":asset("chapter-master","chapter.mkv"),
                "credits-master":asset("credits-master","credits.mkv")
            }
        }),
    );
    for (kind, id, path, template) in [
        (
            "episode-presentation",
            "series-opening",
            "opening.mkv",
            Some("opening"),
        ),
        ("scene", "scene-a", "scene-a.mkv", None),
        (
            "episode-presentation",
            "chapter-title",
            "chapter.mkv",
            Some("chapter"),
        ),
        ("scene", "scene-b", "scene-b.mkv", None),
        (
            "episode-presentation",
            "end-credits",
            "credits.mkv",
            Some("credits"),
        ),
    ] {
        let r = reference(root, path);
        let receipt_name = format!("{id}-receipt.json");
        let mut receipt = serde_json::json!({
            "schema":if kind == "scene" {"reel.scene-build-receipt.v1"} else {"reel.presentation-master-receipt.v1"},
            "language":"es","master_sha256":r["sha256"],"master_bytes":r["bytes"]
        });
        if kind == "scene" {
            receipt["scene_id"] = id.into();
            if id == "scene-a" {
                receipt["presentation_role"] = "opening-poem".into();
            }
        } else {
            receipt["role"] = id.into();
            receipt["template_id"] = template.unwrap().into();
            receipt["template_definition_sha256"] = match id {
                "series-opening" => "b".repeat(64),
                "chapter-title" => "d".repeat(64),
                "end-credits" => "c".repeat(64),
                _ => unreachable!(),
            }
            .into();
            receipt["episode_id"] = "episode".into();
            receipt["frames"] = 24.into();
            receipt["samples"] = 48000.into();
            receipt["timestamps_verified"] = true.into();
            receipt["technical_validation_state"] = "rendered-and-checked".into();
        }
        write(&root.join(receipt_name), &receipt);
    }
    let segments = [
        ("episode-presentation", "series-opening", "opening.mkv"),
        ("scene", "scene-a", "scene-a.mkv"),
        ("episode-presentation", "chapter-title", "chapter.mkv"),
        ("scene", "scene-b", "scene-b.mkv"),
        ("episode-presentation", "end-credits", "credits.mkv"),
    ]
    .map(|(kind, id, path)| {
        let mut segment = serde_json::json!({
            "kind":kind,"id":id,"master":reference(root,path),
            "source_receipt":reference(root,&format!("{id}-receipt.json"))
        });
        if id == "scene-a" {
            segment["role"] = "opening-poem".into();
        }
        segment
    });
    let mut manifest = serde_json::json!({
        "schema":"reel.episode-conform.v1","episode_id":"episode","language":"es",
        "output_sample_rate":48000,"catalog":reference(root,"catalog.json"),
        "master_template_definition":reference(root,"master-template.json"),
        "source_master_template":reference(root,"source-master.json"),
        "episode":reference(root,"episode.json"),"season_bindings":reference(root,"season.json"),
        "episode_bindings":reference(root,"episode-bindings.json"),"segments":segments
    });
    write(&root.join("manifest.json"), &manifest);
    let output = root.join("output");
    let result = Command::new(env!("CARGO_BIN_EXE_reel-episode-conform"))
        .arg("build")
        .arg(root.join("manifest.json"))
        .arg("--input-root")
        .arg(root)
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["total_frames"], 120);
    assert_eq!(receipt["total_samples"], 240000);
    assert_eq!(receipt["segments"][1]["input_sample_rate"], 24000);
    assert_eq!(receipt["segments"][1]["output_sample_rate"], 48000);
    assert_eq!(receipt["decoded_master_matches_ordered_segments"], true);
    assert_eq!(receipt["timestamps_verified"], true);
    assert!(
        receipt["boundary_findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["code"] == "black-at-cut")
    );
    let mut compact = manifest.clone();
    compact["compact_delivery"] = serde_json::json!({"crf":18,"audio_bitrate_kbps":320,"retain_lossless_master":false,"intermediate_video_encoding":"h264-lossless"});
    write(&root.join("compact.json"), &compact);
    let compact_output = root.join("compact-output");
    let compact_result = Command::new(env!("CARGO_BIN_EXE_reel-episode-conform"))
        .arg("build")
        .arg(root.join("compact.json"))
        .arg("--input-root")
        .arg(root)
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(&compact_output)
        .output()
        .unwrap();
    assert!(
        compact_result.status.success(),
        "{}",
        String::from_utf8_lossy(&compact_result.stderr)
    );
    assert!(!compact_output.join("master.mkv").exists());
    assert!(compact_output.join("episode.mp4").exists());
    let compact_receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(compact_output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(compact_receipt["encoded_delivery"]["decoded_frames"], 120);
    assert_eq!(
        compact_receipt["encoded_delivery"]["full_decode_verified"],
        true
    );
    assert_eq!(
        compact_receipt["encoded_delivery"]["lossless_master_retained"],
        false
    );
    // Retention affects only this conform's generated movie, never the sources.
    assert!(output.join("master.mkv").exists());
    // A completed assembly can be consumed read-only after all current sources
    // and its entire ordered decoded picture/audio are checked again.
    let original_master = fs::read(output.join("master.mkv")).unwrap();
    let mut resumed = compact.clone();
    resumed["preassembled_master"] = reference(root, "output/master.mkv");
    write(&root.join("resumed.json"), &resumed);
    let resumed_output = root.join("resumed-output");
    let run = |manifest: &str, target: &Path| {
        Command::new(env!("CARGO_BIN_EXE_reel-episode-conform"))
            .arg("build")
            .arg(root.join(manifest))
            .arg("--input-root")
            .arg(root)
            .arg("--asset-root")
            .arg(root)
            .arg("--output-dir")
            .arg(target)
            .output()
            .unwrap()
    };
    let resumed_result = run("resumed.json", &resumed_output);
    assert!(
        resumed_result.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed_result.stderr)
    );
    assert_eq!(
        fs::read(output.join("master.mkv")).unwrap(),
        original_master
    );
    assert!(!resumed_output.join("master.mkv").exists());
    let resumed_receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(resumed_output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(resumed_receipt["master_sha256"], receipt["master_sha256"]);
    assert_eq!(
        resumed_receipt["preassembled_master"],
        resumed["preassembled_master"]
    );
    assert_eq!(
        resumed_receipt["encoded_delivery"]["full_decode_verified"],
        true
    );
    let mut retained_resume = resumed.clone();
    retained_resume["compact_delivery"]["retain_lossless_master"] = true.into();
    write(&root.join("retained-resume.json"), &retained_resume);
    let retained_output = root.join("retained-resume-output");
    assert!(
        run("retained-resume.json", &retained_output)
            .status
            .success()
    );
    assert_eq!(
        fs::read(retained_output.join("master.mkv")).unwrap(),
        original_master
    );
    assert_eq!(
        fs::read(output.join("master.mkv")).unwrap(),
        original_master
    );
    // Same byte count cannot substitute for the declared output geometry.
    assert!(
        ffmpeg()
            .args(["-v", "error", "-i"])
            .arg(output.join("master.mkv"))
            .args([
                "-vf",
                "scale=32:128",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuv444p",
                "-c:a",
                "copy"
            ])
            .arg(root.join("reshaped.mkv"))
            .status()
            .unwrap()
            .success()
    );
    resumed["preassembled_master"] = reference(root, "reshaped.mkv");
    write(&root.join("reshaped-resume.json"), &resumed);
    let rejected_shape = run("reshaped-resume.json", &root.join("reshaped-resume-output"));
    assert!(!rejected_shape.status.success());
    assert!(String::from_utf8_lossy(&rejected_shape.stderr).contains("geometry"));
    assert!(
        ffmpeg()
            .args(["-v", "error", "-i"])
            .arg(output.join("master.mkv"))
            .args([
                "-vf", "negate", "-c:v", "ffv1", "-pix_fmt", "yuv444p", "-c:a", "copy"
            ])
            .arg(root.join("changed-picture.mkv"))
            .status()
            .unwrap()
            .success()
    );
    resumed["preassembled_master"] = reference(root, "changed-picture.mkv");
    write(&root.join("changed-picture-resume.json"), &resumed);
    let rejected_picture = run(
        "changed-picture-resume.json",
        &root.join("changed-picture-resume-output"),
    );
    assert!(!rejected_picture.status.success());
    assert!(
        String::from_utf8_lossy(&rejected_picture.stderr).contains("differs from selected source")
    );
    assert!(
        ffmpeg()
            .args(["-v", "error", "-i"])
            .arg(output.join("master.mkv"))
            .args(["-c:v", "copy", "-af", "volume=0.5", "-c:a", "pcm_s24le"])
            .arg(root.join("changed-audio.mkv"))
            .status()
            .unwrap()
            .success()
    );
    resumed["preassembled_master"] = reference(root, "changed-audio.mkv");
    write(&root.join("changed-audio-resume.json"), &resumed);
    let rejected_audio = run(
        "changed-audio-resume.json",
        &root.join("changed-audio-resume-output"),
    );
    assert!(!rejected_audio.status.success());
    assert!(
        String::from_utf8_lossy(&rejected_audio.stderr).contains("differs from selected source")
    );
    resumed["preassembled_master"] = reference(root, "scene-b.mkv");
    write(&root.join("short-resume.json"), &resumed);
    assert!(
        !run("short-resume.json", &root.join("short-resume-output"))
            .status
            .success()
    );
    manifest["segments"].as_array_mut().unwrap().swap(1, 3);
    write(&root.join("wrong-order.json"), &manifest);
    let rejected = Command::new(env!("CARGO_BIN_EXE_reel-episode-conform"))
        .arg("build")
        .arg(root.join("wrong-order.json"))
        .arg("--input-root")
        .arg(root)
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("rejected-output"))
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(!root.join("rejected-output").exists());
    manifest["segments"].as_array_mut().unwrap().swap(1, 3);
    manifest["segments"].as_array_mut().unwrap().swap(0, 4);
    write(&root.join("wrong-presentation-order.json"), &manifest);
    let misplaced = Command::new(env!("CARGO_BIN_EXE_reel-episode-conform"))
        .arg("build")
        .arg(root.join("wrong-presentation-order.json"))
        .arg("--input-root")
        .arg(root)
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("misplaced-output"))
        .output()
        .unwrap();
    assert!(!misplaced.status.success());
    assert!(String::from_utf8_lossy(&misplaced.stderr).contains("order differs"));
    assert!(!root.join("misplaced-output").exists());
    let mut padded = compact.clone();
    padded["output_sample_rate"] = 44100.into();
    padded["audio_frame_conform"] = "pad-silence-to-picture-boundaries".into();
    for (index, id, rate) in [(1, "scene-a", 24000), (3, "scene-b", 48000)] {
        let path = format!("{id}.mkv");
        let shortened = format!("{id}-short.mkv");
        let status = ffmpeg()
            .args(["-v", "error", "-i"])
            .arg(root.join(&path))
            .args(["-map", "0:v:0", "-map", "0:a:0", "-c:v", "copy", "-af"])
            .arg(format!("atrim=end_sample={}", rate * 97 / 100))
            .args(["-c:a", "pcm_s24le"])
            .arg(root.join(&shortened))
            .status()
            .unwrap();
        assert!(status.success());
        let receipt_path = format!("{id}-short-receipt.json");
        let mut proof: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join(format!("{id}-receipt.json"))).unwrap())
                .unwrap();
        let master = reference(root, &shortened);
        proof["master_sha256"] = master["sha256"].clone();
        proof["master_bytes"] = master["bytes"].clone();
        write(&root.join(&receipt_path), &proof);
        padded["segments"][index]["master"] = master;
        padded["segments"][index]["source_receipt"] = reference(root, &receipt_path);
    }
    write(&root.join("padded.json"), &padded);
    let padded_result = Command::new(env!("CARGO_BIN_EXE_reel-episode-conform"))
        .arg("build")
        .arg(root.join("padded.json"))
        .arg("--input-root")
        .arg(root)
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("padded-output"))
        .output()
        .unwrap();
    assert!(
        padded_result.status.success(),
        "{}",
        String::from_utf8_lossy(&padded_result.stderr)
    );
    let padded_receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("padded-output/receipt.json")).unwrap())
            .unwrap();
    assert_eq!(padded_receipt["total_frames"], 120);
    assert_eq!(padded_receipt["total_samples"], 220500);
    assert_eq!(padded_receipt["timestamps_verified"], true);
    assert_eq!(
        padded_receipt["decoded_master_matches_ordered_segments"],
        true
    );
    for index in [1, 3] {
        assert_eq!(
            padded_receipt["segments"][index]["audio_padding_samples"],
            1323
        );
    }
    for index in [0, 2, 4] {
        assert_eq!(
            padded_receipt["segments"][index]["audio_padding_samples"],
            0
        );
    }

    // Execute native preparation and finish as separate bounded steps. The
    // preparation record is not a final verification or publication receipt.
    let phased = root.join("phased");
    fs::create_dir(&phased).unwrap();
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), phased.join(entry.file_name())).unwrap();
        }
    }
    let mut phased_manifest = compact.clone();
    // Square-pixel sources omit the false assumption flag in geometry records.
    // Preparation must deserialize those records before issuing its checkpoint.
    phased_manifest["output_geometry"] = serde_json::json!({
        "width":64,"height":64,"policy":"preserve-aspect-lanczos",
        "assume_square_for_unspecified_sar":true
    });
    write(&phased.join("conform.json"), &phased_manifest);
    let mut declared = serde_json::Map::new();
    for entry in fs::read_dir(&phased).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_str().unwrap().to_owned();
        let r = reference(&phased, &name);
        declared.insert(
            name,
            serde_json::json!({"sha256":r["sha256"],"bytes":r["bytes"]}),
        );
    }
    write(
        &phased.join("job.json"),
        &serde_json::json!({"inputs":declared,"tools":{},"files":{}}),
    );
    let run_phase = |command: &str, input: &Path, output: &Path| {
        Command::new(env!("CARGO_BIN_EXE_reel-episode-conform"))
            .arg(command)
            .arg(input)
            .arg("--input-root")
            .arg(&phased)
            .arg("--asset-root")
            .arg(&phased)
            .arg("--output-dir")
            .arg(output)
            .output()
            .unwrap()
    };
    let prepared = phased.join("prepared");
    let prepared_result = run_phase("prepare", &phased.join("conform.json"), &prepared);
    assert!(
        prepared_result.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared_result.stderr)
    );
    assert!(!prepared.join("receipt.json").exists());
    assert!(!prepared.join("episode.mp4").exists());
    let checkpoint_bytes = fs::read(prepared.join("prepared.json")).unwrap();
    let mut checkpoint: serde_json::Value = serde_json::from_slice(&checkpoint_bytes).unwrap();
    assert_eq!(checkpoint["full_episode_verified"], false);
    let geometry = &checkpoint["records"][0]["geometry_verification"];
    assert!(geometry.is_object());
    assert!(geometry.get("sample_aspect_ratio_assumed_square").is_none());
    let assembly_ref = checkpoint["master"]["file"].clone();
    assert_eq!(checkpoint["master"]["root"], "prepared");
    let assembled = prepared.join(assembly_ref["path"].as_str().unwrap());
    let original_assembly = fs::read(&assembled).unwrap();
    let finished = phased.join("finished");
    let finished_result = run_phase("finish", &prepared, &finished);
    assert!(
        finished_result.status.success(),
        "{}",
        String::from_utf8_lossy(&finished_result.stderr)
    );
    let final_receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(finished.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(
        final_receipt["decoded_master_matches_ordered_segments"],
        true
    );
    assert_eq!(final_receipt["timestamps_verified"], true);
    assert_eq!(
        final_receipt["encoded_delivery"]["full_decode_verified"],
        true
    );
    assert_eq!(final_receipt["total_frames"], 120);
    assert_eq!(
        final_receipt["preparation_sha256"],
        reference(&prepared, "prepared.json")["sha256"]
    );
    assert_eq!(fs::read(&assembled).unwrap(), original_assembly);
    assert_eq!(
        fs::read(finished.join("preparation.json")).unwrap(),
        checkpoint_bytes
    );
    let catalog_before = fs::read(phased.join("catalog.json")).unwrap();
    fs::write(phased.join("catalog.json"), b"changed source control").unwrap();
    assert!(
        !run_phase("finish", &prepared, &phased.join("stale-phased-output"))
            .status
            .success()
    );
    fs::write(phased.join("catalog.json"), catalog_before).unwrap();
    checkpoint["records"][0]["decoded_picture_sha256"] = "0".repeat(64).into();
    write(&prepared.join("prepared.json"), &checkpoint);
    let changed_expected = run_phase(
        "finish-consume-normalized",
        &prepared,
        &phased.join("changed-prepared-pixels"),
    );
    assert!(!changed_expected.status.success());
    assert!(
        String::from_utf8_lossy(&changed_expected.stderr).contains("differs from selected source")
    );
    fs::write(prepared.join("prepared.json"), &checkpoint_bytes).unwrap();
    let mut changed_assembly = original_assembly.clone();
    let last = changed_assembly.len() - 1;
    changed_assembly[last] ^= 1;
    fs::write(&assembled, changed_assembly).unwrap();
    assert!(
        !run_phase("finish", &prepared, &phased.join("changed-prepared-master"))
            .status
            .success()
    );
    fs::write(&assembled, &original_assembly).unwrap();
    checkpoint = serde_json::from_slice(&checkpoint_bytes).unwrap();
    checkpoint["validator_sha256"] = "0".repeat(64).into();
    write(&prepared.join("prepared.json"), &checkpoint);
    assert!(
        !run_phase(
            "finish",
            &prepared,
            &phased.join("changed-prepared-validator")
        )
        .status
        .success()
    );
    fs::write(prepared.join("prepared.json"), &checkpoint_bytes).unwrap();

    // The explicit consuming finish must retain all existing validation gates,
    // preserve every original input/master and remove only generated files.
    let issued: serde_json::Value = serde_json::from_slice(&checkpoint_bytes).unwrap();
    let generated = issued["normalized"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["root"] == "prepared")
        .map(|f| prepared.join(f["file"]["path"].as_str().unwrap()))
        .collect::<Vec<_>>();
    assert!(!generated.is_empty());
    assert!(generated.iter().all(|f| f.is_file())); // Prior failed validation removed nothing.
    let original_inputs = issued["source_scope"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let path = phased.join(f["path"].as_str().unwrap());
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect::<Vec<_>>();
    assert!(
        !run_phase(
            "finish-consume-normalized",
            &root.join("output"),
            &phased.join("outside-consume")
        )
        .status
        .success()
    );
    assert!(generated.iter().all(|f| f.is_file()));
    let consumed = phased.join("consumed-finished");
    let consume_result = run_phase("finish-consume-normalized", &prepared, &consumed);
    assert!(
        consume_result.status.success(),
        "{}",
        String::from_utf8_lossy(&consume_result.stderr)
    );
    let consumed_receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(consumed.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(
        consumed_receipt["decoded_master_matches_ordered_segments"],
        true
    );
    assert_eq!(consumed_receipt["timestamps_verified"], true);
    assert_eq!(
        consumed_receipt["encoded_delivery"]["full_decode_verified"],
        true
    );
    assert_eq!(consumed_receipt["segments"], final_receipt["segments"]);
    assert_eq!(
        consumed_receipt["boundary_findings"],
        final_receipt["boundary_findings"]
    );
    assert_eq!(
        consumed_receipt["normalization_retirement"]["files"],
        generated.len()
    );
    assert!(
        consumed_receipt["normalization_retirement"]["bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(final_receipt.get("normalization_retirement").is_none());
    assert!(generated.iter().all(|f| !f.exists()));
    assert_eq!(fs::read(&assembled).unwrap(), original_assembly);
    for (path, bytes) in original_inputs {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    assert_eq!(
        fs::read(prepared.join("prepared.json")).unwrap(),
        checkpoint_bytes
    );
    assert!(
        !run_phase(
            "finish-consume-normalized",
            &prepared,
            &phased.join("repeat-consume")
        )
        .status
        .success()
    );

    // Bounded normalization batches reuse issued intermediates and still feed
    // the ordinary preparation/finish checks; incomplete custody is not a movie.
    let batched = root.join("batched");
    fs::create_dir(&batched).unwrap();
    let closed_job: serde_json::Value =
        serde_json::from_slice(&fs::read(phased.join("job.json")).unwrap()).unwrap();
    for name in closed_job["inputs"].as_object().unwrap().keys() {
        fs::copy(phased.join(name), batched.join(name)).unwrap();
    }
    fs::copy(phased.join("job.json"), batched.join("job.json")).unwrap();
    let batched_prepared = batched.join("prepared");
    let run_batch = || {
        Command::new(env!("CARGO_BIN_EXE_reel-episode-conform"))
            .current_dir(&batched)
            .arg("prepare-batch")
            .arg(batched.join("conform.json"))
            .arg("--input-root")
            .arg(&batched)
            .arg("--asset-root")
            .arg(&batched)
            .arg("--output-dir")
            .arg("prepared")
            .arg("--max-segments")
            .arg("2")
            .output()
            .unwrap()
    };
    let assemble = || {
        Command::new(env!("CARGO_BIN_EXE_reel-episode-conform"))
            .arg("assemble-preparation")
            .arg(&batched_prepared)
            .arg("--asset-root")
            .arg(&batched)
            .output()
            .unwrap()
    };
    let first_batch = run_batch();
    assert!(
        first_batch.status.success(),
        "{}",
        String::from_utf8_lossy(&first_batch.stderr)
    );
    let progress_file = batched_prepared.join("normalization-progress.json");
    let progress_bytes = fs::read(&progress_file).unwrap();
    let mut progress: serde_json::Value = serde_json::from_slice(&progress_bytes).unwrap();
    assert_eq!(progress["records"].as_array().unwrap().len(), 2);
    assert_eq!(progress["full_episode_verified"], false);
    assert!(!batched_prepared.join("prepared.json").exists());
    assert!(!batched_prepared.join("master.mkv").exists());
    assert!(!assemble().status.success());
    let intermediate =
        batched_prepared.join(progress["normalized"][0]["file"]["path"].as_str().unwrap());
    assert_eq!(progress["normalized"][0]["root"], "prepared");
    let normalized_bytes = fs::read(&intermediate).unwrap();
    let mut corrupted = normalized_bytes.clone();
    let last = corrupted.len() - 1;
    corrupted[last] ^= 1;
    fs::write(&intermediate, corrupted).unwrap();
    assert!(!run_batch().status.success());
    fs::write(&intermediate, &normalized_bytes).unwrap();
    let original_mtime = fs::metadata(&intermediate).unwrap().modified().unwrap();
    let catalog_bytes = fs::read(batched.join("catalog.json")).unwrap();
    fs::write(batched.join("catalog.json"), b"changed source").unwrap();
    assert!(!run_batch().status.success());
    fs::write(batched.join("catalog.json"), catalog_bytes).unwrap();
    progress["validator_sha256"] = "0".repeat(64).into();
    write(&progress_file, &progress);
    assert!(!run_batch().status.success());
    fs::write(&progress_file, &progress_bytes).unwrap();
    progress = serde_json::from_slice(&progress_bytes).unwrap();
    progress["records"][0]["start_frame"] = 1.into();
    write(&progress_file, &progress);
    assert!(!run_batch().status.success());
    fs::write(&progress_file, &progress_bytes).unwrap();
    for expected_count in [4, 5] {
        let next_batch = run_batch();
        assert!(
            next_batch.status.success(),
            "{}",
            String::from_utf8_lossy(&next_batch.stderr)
        );
        let next: serde_json::Value =
            serde_json::from_slice(&fs::read(&progress_file).unwrap()).unwrap();
        assert_eq!(next["records"].as_array().unwrap().len(), expected_count);
        assert_eq!(
            fs::metadata(&intermediate).unwrap().modified().unwrap(),
            original_mtime
        );
        assert_eq!(fs::read(&intermediate).unwrap(), normalized_bytes);
    }
    assert!(!run_batch().status.success());
    let assembled_batch = assemble();
    assert!(
        assembled_batch.status.success(),
        "{}",
        String::from_utf8_lossy(&assembled_batch.stderr)
    );
    assert!(!run_batch().status.success());
    let batched_output = batched.join("finished");
    let finish_batch = Command::new(env!("CARGO_BIN_EXE_reel-episode-conform"))
        .arg("finish-consume-normalized")
        .arg(&batched_prepared)
        .arg("--input-root")
        .arg(&batched)
        .arg("--asset-root")
        .arg(&batched)
        .arg("--output-dir")
        .arg(&batched_output)
        .output()
        .unwrap();
    assert!(
        finish_batch.status.success(),
        "{}",
        String::from_utf8_lossy(&finish_batch.stderr)
    );
    let batch_receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(batched_output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(batch_receipt["segments"], final_receipt["segments"]);
    assert_eq!(
        batch_receipt["boundary_findings"],
        final_receipt["boundary_findings"]
    );
    assert_eq!(
        batch_receipt["decoded_master_matches_ordered_segments"],
        true
    );
    assert_eq!(batch_receipt["timestamps_verified"], true);
    assert_eq!(
        batch_receipt["encoded_delivery"]["full_decode_verified"],
        true
    );

    fs::write(root.join("source-master.json"), b"altered template").unwrap();
    let stale_source = Command::new(env!("CARGO_BIN_EXE_reel-episode-conform"))
        .arg("build")
        .arg(root.join("manifest.json"))
        .arg("--input-root")
        .arg(root)
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("stale-source-output"))
        .output()
        .unwrap();
    assert!(!stale_source.status.success());
    assert!(!root.join("stale-source-output").exists());
}
