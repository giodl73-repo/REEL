use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn reference(root: &Path, name: &str) -> Value {
    let bytes = fs::read(root.join(name)).unwrap();
    json!({
        "path":name,
        "sha256":Sha256::digest(&bytes).iter().map(|b|format!("{b:02x}")).collect::<String>(),
        "bytes":bytes.len()
    })
}

fn ffmpeg() -> Command {
    #[allow(unused_mut)] // Windows adds CREATE_NO_WINDOW to this command.
    let mut command = Command::new("ffmpeg");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

#[test]
fn selected_existing_opening_becomes_verified_lossless_presentation_master() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let status = ffmpeg()
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=64x64:r=24:d=2",
        ])
        .args([
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=2",
        ])
        .args([
            "-filter:a",
            "pan=stereo|c0=c0|c1=c0",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-shortest",
        ])
        .arg(root.join("opening.mp4"))
        .status()
        .unwrap();
    assert!(status.success());
    write(
        &root.join("template.json"),
        &json!({
            "schema":"reel.selected-presentation-master-template.v1",
            "template_id":"opening","kind":"series-opening","width":64,"height":64,
            "fps_numerator":24,"fps_denominator":1,"sample_rate":48000,"duration_frames":48
        }),
    );
    write(
        &root.join("catalog.json"),
        &json!({
            "schema":"reel.scene-template-catalog.v1","templates":[{
                "template_id":"opening","kind":"series-opening",
                "definition_sha256":reference(root,"template.json")["sha256"],
                "required_content_keys":[]
            }]
        }),
    );
    let selected = reference(root, "opening.mp4");
    let hash = selected["sha256"].as_str().unwrap();
    let asset = json!({
        "logical_id":"selected-opening","sha256":hash,"bytes":selected["bytes"],
        "cache_uri":format!("cache://sha256/{hash}"),
        "selection_state":"selected-private-production"
    });
    write(
        &root.join("season.json"),
        &json!({
        "schema":"reel.scene-asset-bindings.v1","scope_id":"S1",
            "assets":{"series-opening":asset}
        }),
    );
    write(
        &root.join("episode.json"),
        &json!({
        "schema":"reel.scene-asset-bindings.v1","scope_id":"S1E04","assets":{}
        }),
    );
    write(
        &root.join("legacy-receipt.json"),
        &json!({
            "schema":"source.private-opening.v1",
            "series_opening":{"cache_uri":format!("cache://sha256/{hash}")}
        }),
    );
    let mut manifest = json!({
        "schema":"reel.presentation-adopt.v1","role":"series-opening",
        "language":"es","season_id":"S1","episode_id":"S1E04","template_id":"opening",
        "catalog":reference(root,"catalog.json"),
        "template_definition":reference(root,"template.json"),
        "season_bindings":reference(root,"season.json"),
        "episode_bindings":reference(root,"episode.json"),
        "source_binding":"series-opening","source":selected,
        "selection_evidence":reference(root,"legacy-receipt.json"),
        "evidence_hash_pointer":"/series_opening/cache_uri"
    });
    write(&root.join("manifest.json"), &manifest);
    let output = Command::new(env!("CARGO_BIN_EXE_reel-presentation-adopt"))
        .arg("build")
        .arg(root.join("manifest.json"))
        .arg("--input-root")
        .arg(root)
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("adopted"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value =
        serde_json::from_slice(&fs::read(root.join("adopted/receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["schema"], "reel.presentation-master-receipt.v1");
    assert_eq!(receipt["frames"], 48);
    assert_eq!(receipt["samples"], 96000);
    assert_eq!(receipt["source_content_matches_lossless_master"], true);
    assert_eq!(receipt["publication"], "not-authorized");
    assert_eq!(
        receipt["technical_validation_state"],
        "decoded-source-equivalent"
    );

    // Consume the actual adopted master and receipt through the generic
    // episode conform, beside one independent scene master.
    let scene_status = ffmpeg()
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=64x64:r=24:d=1",
        ])
        .args(["-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo:d=1"])
        .args([
            "-shortest",
            "-c:v",
            "ffv1",
            "-pix_fmt",
            "yuv444p",
            "-c:a",
            "pcm_s24le",
        ])
        .arg(root.join("scene.mkv"))
        .status()
        .unwrap();
    assert!(scene_status.success());
    write(
        &root.join("master-order.json"),
        &json!({
            "schema":"reel.episode-master-template.v1","template_id":"episode-master",
            "ordered_roles":["series-opening","chapter-scenes"],"optional_roles":[]
        }),
    );
    write(
        &root.join("conform-catalog.json"),
        &json!({
            "schema":"reel.scene-template-catalog.v1","templates":[
                {"template_id":"episode-master","kind":"episode-master",
                 "definition_sha256":reference(root,"master-order.json")["sha256"],"required_content_keys":[]},
                {"template_id":"opening","kind":"series-opening",
                 "definition_sha256":reference(root,"template.json")["sha256"],"required_content_keys":[]}
            ]
        }),
    );
    let adopted_master = reference(root, "adopted/master.mkv");
    let adopted_hash = adopted_master["sha256"].as_str().unwrap();
    write(
        &root.join("conform-episode-bindings.json"),
        &json!({
            "schema":"reel.scene-asset-bindings.v1","scope_id":"S1E04","assets":{
                "opening-master":{"logical_id":"opening-lossless","sha256":adopted_hash,
                    "bytes":adopted_master["bytes"],
                    "cache_uri":format!("cache://sha256/{adopted_hash}"),
                    "selection_state":"selected-private-production"}
            }
        }),
    );
    write(
        &root.join("episode-authoring.json"),
        &json!({
            "schema":"reel.episode-authoring.v1","episode_id":"S1E04","season_id":"S1",
            "authoring_state":"ready-for-private-build","master_template_id":"episode-master",
            "scene_policy_id":"narrative","scene_ids":["scene"],"score_palette":[],
            "presentation":[{"role":"series-opening","template_id":"opening",
                "content":{},"asset_binding":"opening-master"}]
        }),
    );
    let scene = reference(root, "scene.mkv");
    write(
        &root.join("scene-receipt.json"),
        &json!({
            "schema":"reel.scene-build-receipt.v1","scene_id":"scene","language":"es",
            "master_sha256":scene["sha256"],"master_bytes":scene["bytes"]
        }),
    );
    write(
        &root.join("conform.json"),
        &json!({
            "schema":"reel.episode-conform.v1","episode_id":"S1E04","language":"es",
            "output_sample_rate":48000,"catalog":reference(root,"conform-catalog.json"),
            "master_template_definition":reference(root,"master-order.json"),
            "episode":reference(root,"episode-authoring.json"),
            "season_bindings":reference(root,"season.json"),
            "episode_bindings":reference(root,"conform-episode-bindings.json"),
            "segments":[
            {"kind":"episode-presentation","id":"series-opening",
                "master":adopted_master,"source_receipt":reference(root,"adopted/receipt.json"),
                "adoption_manifest":reference(root,"manifest.json")},
                {"kind":"scene","id":"scene","master":scene,
                    "source_receipt":reference(root,"scene-receipt.json")}
            ]
        }),
    );
    let conformed = Command::new(env!("CARGO_BIN_EXE_reel-episode-conform"))
        .arg("build")
        .arg(root.join("conform.json"))
        .arg("--input-root")
        .arg(root)
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("episode-output"))
        .output()
        .unwrap();
    assert!(
        conformed.status.success(),
        "{}",
        String::from_utf8_lossy(&conformed.stderr)
    );
    let conformed_receipt: Value =
        serde_json::from_slice(&fs::read(root.join("episode-output/receipt.json")).unwrap())
            .unwrap();
    assert_eq!(conformed_receipt["total_frames"], 72);
    assert_eq!(conformed_receipt["total_samples"], 144000);
    assert_eq!(
        conformed_receipt["upstream_presentation_recheck_state"],
        "verified-for-all-presentation-segments"
    );

    fs::create_dir(root.join("retimed")).unwrap();
    let shifted = ffmpeg()
        .args(["-v", "error", "-itsoffset", "0.25", "-i"])
        .arg(root.join("adopted/master.mkv"))
        .args(["-map", "0:v:0", "-map", "0:a:0", "-c", "copy", "-copyts"])
        .arg(root.join("retimed/master.mkv"))
        .status()
        .unwrap();
    assert!(shifted.success());
    let shifted_master = reference(root, "retimed/master.mkv");
    let mut shifted_receipt = receipt.clone();
    shifted_receipt["master_sha256"] = shifted_master["sha256"].clone();
    shifted_receipt["master_bytes"] = shifted_master["bytes"].clone();
    write(&root.join("retimed/receipt.json"), &shifted_receipt);
    let rejected_timing = Command::new(env!("CARGO_BIN_EXE_reel-presentation-adopt"))
        .arg("check")
        .arg(root.join("manifest.json"))
        .arg("--input-root")
        .arg(root)
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("retimed"))
        .output()
        .unwrap();
    assert!(!rejected_timing.status.success());

    manifest["evidence_hash_pointer"] = "/wrong/hash".into();
    write(&root.join("bad-manifest.json"), &manifest);
    let rejected = Command::new(env!("CARGO_BIN_EXE_reel-presentation-adopt"))
        .arg("build")
        .arg(root.join("bad-manifest.json"))
        .arg("--input-root")
        .arg(root)
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("rejected"))
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(!root.join("rejected").exists());
}
