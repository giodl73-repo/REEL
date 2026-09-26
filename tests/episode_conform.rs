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
        .arg(format!("anullsrc=r={sample_rate}:cl=stereo:d=1"))
        .args([
            "-shortest",
            "-c:v",
            "ffv1",
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
