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
    opening_fixture("libx264", "yuv420p", false, false, false, false, false);
}

#[test]
fn selected_native_opening_preserves_exact_bytes_and_rechecks_timing() {
    opening_fixture("ffv1", "yuv444p", false, false, false, false, false);
}

#[test]
fn source_excerpt_preserves_exact_selected_frames_and_samples() {
    opening_fixture("libx264", "yuv420p", true, false, false, false, false);
}

#[test]
fn excerpt_rejects_inherited_picture_timestamp_gaps() {
    opening_fixture("ffv1", "yuv444p", true, true, false, false, false);
}

#[test]
fn excerpt_rejects_audio_timestamp_fault_without_explicit_repair() {
    opening_fixture("ffv1", "yuv444p", true, false, true, false, false);
}

#[test]
fn evidenced_audio_clock_repair_preserves_exact_samples_and_conform_consumption() {
    opening_fixture("ffv1", "yuv444p", true, false, true, true, false);
}

#[test]
fn audio_clock_repair_does_not_hide_picture_timestamp_gaps() {
    opening_fixture("ffv1", "yuv444p", true, true, true, true, false);
}

fn opening_fixture(
    codec: &str,
    pixel_format: &str,
    excerpt: bool,
    gapped: bool,
    audio_gap: bool,
    repair: bool,
    origin: bool,
) {
    opening_fixture_with_reset(
        codec,
        pixel_format,
        excerpt,
        gapped,
        audio_gap,
        repair,
        origin,
        None,
    );
}

fn opening_fixture_with_reset(
    codec: &str,
    pixel_format: &str,
    excerpt: bool,
    gapped: bool,
    audio_gap: bool,
    repair: bool,
    origin: bool,
    reset_sample: Option<u32>,
) {
    let prerange_reset = reset_sample.is_some();
    let filter = if audio_gap {
        "pan=stereo|c0=c0|c1=c0,asetpts=PTS+if(gte(N\\,24000)\\,0.1/TB\\,0)".to_string()
    } else if let Some(sample) = reset_sample {
        format!("pan=stereo|c0=c0|c1=c0,asetpts=PTS-if(gte(N\\,{sample})\\,1024/SR/TB\\,0)")
    } else {
        "pan=stereo|c0=c0|c1=c0".to_string()
    };
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let status = ffmpeg()
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=64x64:r=24:d=2",
        ])
        .args([
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=2",
        ])
        .args([
            "-filter:a",
            &filter,
            "-c:v",
            codec,
            "-pix_fmt",
            pixel_format,
            "-c:a",
            if origin { "aac" } else { "pcm_s24le" },
            "-shortest",
        ])
        .args(if gapped && origin {
            vec![
                "-vf",
                "select=not(eq(n\\,8)),setpts=PTS+2/(24*TB)",
                "-fps_mode",
                "passthrough",
            ]
        } else if gapped {
            vec!["-vf", "select=not(eq(n\\,24))", "-fps_mode", "passthrough"]
        } else if origin {
            vec!["-vf", "setpts=PTS+2/(24*TB)", "-fps_mode", "passthrough"]
        } else {
            vec![]
        })
        .arg(root.join("opening.mkv"))
        .status()
        .unwrap();
    assert!(status.success());
    write(
        &root.join("template.json"),
        &json!({
            "schema":"reel.selected-presentation-master-template.v1",
            "template_id":"opening","kind":"series-opening","width":64,"height":64,
            "fps_numerator":24,"fps_denominator":1,"sample_rate":48000,"duration_frames":if excerpt {24} else {48}
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
    let selected = reference(root, "opening.mkv");
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
            "series_opening":{"cache_uri":format!("cache://sha256/{hash}")},
            "source_range":{"start_frame":12,"frame_count":24},
            "audio_clock_policy":"decoded-sample-count",
            "audio_range_origin_policy":"selected-decoded-video-pts"
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
    if excerpt {
        manifest["source_range"] = json!({"start_frame":12,"frame_count":24});
        manifest["evidence_range_pointer"] = "/source_range".into();
    }
    if repair {
        manifest["audio_clock_repair"] =
            json!({"policy":"decoded-sample-count", "evidence_pointer":"/audio_clock_policy"});
    }
    if origin {
        manifest["audio_range_origin"] = json!({"policy":"selected-decoded-video-pts","evidence_pointer":"/audio_range_origin_policy"});
    }
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
    if gapped || (audio_gap && (!repair || origin)) || matches!(reset_sample, Some(28672 | 48000)) {
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("timestamp gap")
                || String::from_utf8_lossy(&output.stderr).contains("timestamp overlap"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!root.join("adopted").exists());
        return;
    }
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value =
        serde_json::from_slice(&fs::read(root.join("adopted/receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["schema"], "reel.presentation-master-receipt.v1");
    assert_eq!(receipt["frames"], if excerpt { 24 } else { 48 });
    assert_eq!(receipt["samples"], if excerpt { 48000 } else { 96000 });
    assert_eq!(receipt["source_content_matches_lossless_master"], true);
    assert_eq!(receipt["publication"], "not-authorized");
    if origin {
        assert_eq!(
            receipt["audio_sample_window"]["audio_anchor_sample"],
            if prerange_reset { 28672 } else { 27648 }
        );
        assert_eq!(
            receipt["audio_sample_window"]["selected_audio_anchor_pts"]["ticks"],
            576
        );
        assert_eq!(
            receipt["audio_sample_window"]["start_sample"],
            if prerange_reset { 29008 } else { 27984 }
        );
        assert_eq!(
            receipt["audio_sample_window"]["end_sample"],
            if prerange_reset { 77008 } else { 75984 }
        );
        assert_eq!(
            receipt["audio_sample_window"]["selected_video_pts"]["ticks"],
            583
        );
        assert_eq!(
            receipt["audio_sample_window"]["first_audio_pts"]["ticks"],
            0
        );
        // Timestamp normalization can pass while selecting the wrong source
        // window. Demonstrate that the legacy policy is distinct, not silently
        // changed to the new source-origin policy.
        let mut repair_only = manifest.clone();
        repair_only
            .as_object_mut()
            .unwrap()
            .remove("audio_range_origin");
        write(&root.join("repair-only.json"), &repair_only);
        let output = Command::new(env!("CARGO_BIN_EXE_reel-presentation-adopt"))
            .arg("build")
            .arg(root.join("repair-only.json"))
            .arg("--input-root")
            .arg(root)
            .arg("--asset-root")
            .arg(root)
            .arg("--output-dir")
            .arg(root.join("repair-only"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let old: Value =
            serde_json::from_slice(&fs::read(root.join("repair-only/receipt.json")).unwrap())
                .unwrap();
        assert_eq!(old["timestamps_verified"], true);
        assert!(old.get("audio_sample_window").is_none());
        assert_ne!(old["decoded_audio_sha256"], receipt["decoded_audio_sha256"]);
    }
    if codec == "ffv1" && !excerpt {
        assert_eq!(receipt["source_sha256"], receipt["master_sha256"]);
        assert_eq!(receipt["source_bytes"], receipt["master_bytes"]);
    }
    if excerpt {
        assert_eq!(receipt["source_range"], manifest["source_range"]);
        if repair {
            assert_eq!(
                receipt["audio_clock_repair"],
                manifest["audio_clock_repair"]
            );
            for (name, policy) in [
                (
                    "unevidenced-repair",
                    json!({"policy":"decoded-sample-count","evidence_pointer":"/missing"}),
                ),
                (
                    "unknown-repair",
                    json!({"policy":"resample","evidence_pointer":"/audio_clock_policy"}),
                ),
            ] {
                let mut bad = manifest.clone();
                bad["audio_clock_repair"] = policy;
                write(&root.join(format!("{name}.json")), &bad);
                let o = Command::new(env!("CARGO_BIN_EXE_reel-presentation-adopt"))
                    .arg("build")
                    .arg(root.join(format!("{name}.json")))
                    .arg("--input-root")
                    .arg(root)
                    .arg("--asset-root")
                    .arg(root)
                    .arg("--output-dir")
                    .arg(root.join(name))
                    .output()
                    .unwrap();
                assert!(!o.status.success());
                assert!(
                    String::from_utf8_lossy(&o.stderr).contains("exact selected policy evidence")
                );
                assert!(!root.join(name).exists());
            }
        }
        // Independent oracle: decode the whole source, then slice raw bytes
        // in the test. Do not reuse the production trim filters.
        for picture in [true, false] {
            let raw = |file: &str| {
                let mut c = ffmpeg();
                c.args(["-v", "error", "-i"]).arg(root.join(file));
                if picture {
                    c.args([
                        "-map",
                        "0:v:0",
                        "-fps_mode",
                        "passthrough",
                        "-pix_fmt",
                        "yuv444p",
                        "-f",
                        "rawvideo",
                    ]);
                } else {
                    c.args(["-map", "0:a:0", "-c:a", "pcm_s24le", "-f", "s24le"]);
                }
                let o = c.arg("-").output().unwrap();
                assert!(o.status.success());
                o.stdout
            };
            let source = raw("opening.mkv");
            let adopted = raw("adopted/master.mkv");
            let (start, end) = if picture {
                (12 * 64 * 64 * 3, 36 * 64 * 64 * 3)
            } else {
                if origin {
                    (
                        if prerange_reset { 29008 * 6 } else { 27984 * 6 },
                        if prerange_reset { 77008 * 6 } else { 75984 * 6 },
                    )
                } else {
                    (24000 * 6, 72000 * 6)
                }
            };
            assert_eq!(adopted.len(), end - start, "picture={picture}");
            assert_eq!(
                Sha256::digest(&adopted),
                Sha256::digest(&source[start..end]),
                "picture={picture}; source PCM/frame slice differs"
            );
        }
        for (name, range, pointer) in [
            (
                "missing-range-evidence",
                json!({"start_frame":12,"frame_count":24}),
                Value::Null,
            ),
            (
                "changed-range",
                json!({"start_frame":13,"frame_count":24}),
                json!("/source_range"),
            ),
            (
                "zero-range",
                json!({"start_frame":12,"frame_count":0}),
                json!("/source_range"),
            ),
        ] {
            let mut bad = manifest.clone();
            bad["source_range"] = range;
            bad["evidence_range_pointer"] = pointer;
            write(&root.join(format!("{name}.json")), &bad);
            let o = Command::new(env!("CARGO_BIN_EXE_reel-presentation-adopt"))
                .arg("build")
                .arg(root.join(format!("{name}.json")))
                .arg("--input-root")
                .arg(root)
                .arg("--asset-root")
                .arg(root)
                .arg("--output-dir")
                .arg(root.join(name))
                .output()
                .unwrap();
            assert!(!o.status.success());
            assert!(!root.join(name).exists());
        }
    }
    assert_eq!(
        receipt["technical_validation_state"],
        "decoded-source-equivalent"
    );

    if origin {
        assert_eq!(
            receipt["audio_sample_window"]["start_sample"],
            if prerange_reset { 29008 } else { 27984 }
        );
        assert_eq!(
            receipt["audio_sample_window"]["end_sample"],
            if prerange_reset { 77008 } else { 75984 }
        );
        assert_eq!(
            receipt["audio_range_origin"],
            manifest["audio_range_origin"]
        );
        for (name, policy) in [
            (
                "missing-origin-evidence",
                json!({"policy":"selected-decoded-video-pts","evidence_pointer":"/missing"}),
            ),
            (
                "unknown-origin",
                json!({"policy":"manual-offset","evidence_pointer":"/audio_range_origin_policy"}),
            ),
        ] {
            let mut bad = manifest.clone();
            bad["audio_range_origin"] = policy;
            write(&root.join(format!("{name}.json")), &bad);
            let o = Command::new(env!("CARGO_BIN_EXE_reel-presentation-adopt"))
                .args(["build"])
                .arg(root.join(format!("{name}.json")))
                .arg("--input-root")
                .arg(root)
                .arg("--asset-root")
                .arg(root)
                .arg("--output-dir")
                .arg(root.join(name))
                .output()
                .unwrap();
            assert!(!o.status.success());
            assert!(String::from_utf8_lossy(&o.stderr).contains("exact selected policy evidence"));
            assert!(!root.join(name).exists());
        }
    }

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
    assert_eq!(
        conformed_receipt["total_frames"],
        if excerpt { 48 } else { 72 }
    );
    assert_eq!(
        conformed_receipt["total_samples"],
        if excerpt { 96000 } else { 144000 }
    );
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

    if excerpt {
        // A range can match evidence yet exceed source availability. The
        // failed build must leave no adopted output or successful receipt.
        let mut evidence: Value =
            serde_json::from_slice(&fs::read(root.join("legacy-receipt.json")).unwrap()).unwrap();
        evidence["source_range"] = json!({"start_frame":40,"frame_count":24});
        write(&root.join("past-end-evidence.json"), &evidence);
        let mut past_end = manifest.clone();
        past_end["source_range"] = evidence["source_range"].clone();
        past_end["selection_evidence"] = reference(root, "past-end-evidence.json");
        write(&root.join("past-end.json"), &past_end);
        let o = Command::new(env!("CARGO_BIN_EXE_reel-presentation-adopt"))
            .arg("build")
            .arg(root.join("past-end.json"))
            .arg("--input-root")
            .arg(root)
            .arg("--asset-root")
            .arg(root)
            .arg("--output-dir")
            .arg(root.join("past-end-output"))
            .output()
            .unwrap();
        assert!(!o.status.success());
        assert!(!root.join("past-end-output").exists());
    }

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

#[test]
fn h264_aac_excerpt_uses_decoded_picture_origin_and_continuous_sample_clock() {
    opening_fixture("libx264", "yuv420p", true, false, false, true, true);
}

#[test]
fn decoded_origin_rejects_a_picture_gap_before_the_selected_range() {
    opening_fixture("libx264", "yuv420p", true, true, false, true, true);
}

#[test]
fn decoded_origin_rejects_audio_gaps_despite_output_clock_repair() {
    opening_fixture("libx264", "yuv420p", true, false, true, true, true);
}

#[test]
fn decoded_origin_maps_a_prerange_audio_reset_to_local_samples() {
    opening_fixture_with_reset(
        "libx264",
        "yuv420p",
        true,
        false,
        false,
        true,
        true,
        Some(12000),
    );
}

#[test]
fn decoded_origin_rejects_an_overlap_at_the_selected_anchor() {
    opening_fixture_with_reset(
        "libx264",
        "yuv420p",
        true,
        false,
        false,
        true,
        true,
        Some(28672),
    );
}

#[test]
fn decoded_origin_rejects_a_reset_inside_the_selected_range() {
    opening_fixture_with_reset(
        "libx264",
        "yuv420p",
        true,
        false,
        false,
        true,
        true,
        Some(48000),
    );
}
