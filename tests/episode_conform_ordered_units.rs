use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn reference(root: &Path, name: &str) -> Value {
    let bytes = fs::read(root.join(name)).unwrap();
    json!({"path":name,"sha256":digest(&bytes),"bytes":bytes.len()})
}

fn source_geometry(
    root: &Path,
    name: &str,
    color: &str,
    changed_properties: bool,
    small: bool,
    unspecified: bool,
    known_matrix: bool,
) {
    let mut cmd = Command::new("ffmpeg");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.args(["-v", "error", "-f", "lavfi", "-i"])
        .arg(if small {
            "testsrc2=s=32x32:r=24:d=1".to_owned()
        } else {
            format!("color=c={color}:s=64x64:r=24:d=1")
        })
        .args(["-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo:d=1"]);
    if unspecified {
        cmd.args(["-vf", "setsar=0"]);
    } else if changed_properties {
        if name.starts_with("non-square") {
            cmd.args(["-vf", "setsar=2/1"]);
        } else {
            cmd.args([
                "-vf",
                "setsar=1744/1743:max=10000",
                "-colorspace",
                "bt470bg",
            ]);
        }
    }
    if known_matrix {
        cmd.args(["-colorspace", "bt470bg"]);
    }
    let status = cmd
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

fn selected_asset(root: &Path, key: &str, path: &str) -> Value {
    let file = reference(root, path);
    json!({"logical_id":key,"sha256":file["sha256"],"bytes":file["bytes"],
        "cache_uri":format!("cache://sha256/{}",file["sha256"].as_str().unwrap()),
        "selection_state":"selected-private-production"})
}

fn run(root: &Path, manifest: &str, output: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_reel-episode-conform"))
        .arg("build")
        .arg(root.join(manifest))
        .arg("--input-root")
        .arg(root)
        .arg("--asset-root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join(output))
        .output()
        .unwrap()
}

#[test]
fn ordered_units_conform_bilingual_displays_and_rejects_bad_sources() {
    ordered_fixture(false, false, false);
}

#[test]
fn adopted_editable_displays_bind_source_text_and_rendered_master() {
    ordered_fixture(true, false, false);
}

#[test]
fn compact_conform_keeps_global_clock_across_display_property_changes() {
    ordered_fixture(true, true, false);
}

#[test]
fn verified_presentation_reuse_rehashes_inputs_and_checks_new_full_episode() {
    ordered_fixture(true, true, true);
}

fn ordered_fixture(adopt: bool, clock_transition: bool, reuse_cache: bool) {
    ordered_geometry_fixture(adopt, clock_transition, reuse_cache, false, false, false);
}

#[test]
fn mixed_geometry_requires_explicit_aspect_preserving_delivery_policy() {
    ordered_geometry_fixture(true, false, false, true, false, false);
}

#[test]
fn unspecified_pixel_aspect_requires_explicit_declaration_and_preserves_content() {
    ordered_geometry_fixture(true, false, false, true, true, false);
}

#[test]
fn unknown_matrix_declaration_requires_matching_decoded_colors() {
    ordered_geometry_fixture(true, false, false, true, false, true);
}

fn ordered_geometry_fixture(
    adopt: bool,
    clock_transition: bool,
    reuse_cache: bool,
    mixed: bool,
    unspecified: bool,
    color_declaration: bool,
) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(root.join("clean-picture.bin"), b"selected clean background").unwrap();
    let text = [
        (
            "es",
            ["Foto de familia", "Papas agridulces", "Güines, 1944"],
        ),
        (
            "en",
            ["Family photograph", "Bittersweet Potatoes", "Güines, 1944"],
        ),
    ];
    let hashes = |lang: &str, indices: &[usize]| {
        let words = text
            .iter()
            .find(|(candidate, _)| *candidate == lang)
            .unwrap()
            .1;
        indices
            .iter()
            .map(|i| digest(words[*i].as_bytes()))
            .collect::<Vec<_>>()
    };
    let ordered_units = json!([
        {"kind":"scene","id":"scene-a"},
        {"kind":"episode-presentation","id":"display-a","source_ids":["B1647"],
            "source_text_sha256_by_language":{"es":hashes("es", &[0]),"en":hashes("en", &[0])}},
        {"kind":"episode-presentation","id":"display-b","source_ids":["B1648","B1649"],
            "source_text_sha256_by_language":{"es":hashes("es", &[1,2]),"en":hashes("en", &[1,2])}},
        {"kind":"scene","id":"scene-b"}
    ]);
    write(
        &root.join("source-master.json"),
        &json!({"ordered_units":ordered_units}),
    );
    write(
        &root.join("master-template.json"),
        &json!({
            "schema":"reel.episode-master-template.v2","template_id":"ordered-master",
            "ordered_units":ordered_units,
            "source_template_sha256":reference(root,"source-master.json")["sha256"]
        }),
    );
    let master_sha = reference(root, "master-template.json")["sha256"].clone();
    write(
        &root.join("catalog.json"),
        &json!({
            "schema":"reel.scene-template-catalog.v1","templates":[
                {"template_id":"ordered-master","kind":"episode-master","definition_sha256":master_sha,"required_content_keys":[]},
                {"template_id":"display-a-template","kind":"display-a","definition_sha256":"a".repeat(64),
                    "required_content_keys":["picture_binding","source_text_evidence_sha256_by_language"]},
                {"template_id":"display-b-template","kind":"display-b","definition_sha256":"b".repeat(64),
                    "required_content_keys":["picture_binding","source_text_evidence_sha256_by_language"]}
            ]
        }),
    );
    write(
        &root.join("season.json"),
        &json!({
            "schema":"reel.scene-asset-bindings.v1","scope_id":"season","assets":{}
        }),
    );
    let mut assets = serde_json::Map::new();
    assets.insert(
        "clean-picture".into(),
        selected_asset(root, "clean-picture", "clean-picture.bin"),
    );
    let picture_sha = reference(root, "clean-picture.bin")["sha256"].clone();
    for (lang, words) in text {
        for (role, indices) in [("display-a", vec![0]), ("display-b", vec![1, 2])] {
            let units = indices
                .iter()
                .map(|i| {
                    json!({
                        "source_id":format!("B{}", 1647+i),"text":words[*i],
                        "sha256":digest(words[*i].as_bytes())
                    })
                })
                .collect::<Vec<_>>();
            let evidence_name = format!("{role}-{lang}-text.json");
            write(
                &root.join(&evidence_name),
                &json!({
                    "schema":"reel.presentation-source-text.v1","episode_id":"episode",
                    "role":role,"language":lang,"picture_binding":"clean-picture",
                    "picture_sha256":picture_sha,
                    "caption_template_id":format!("{role}-template"),"units":units
                }),
            );
        }
        for (id, color) in [
            ("scene-a", "red"),
            ("display-a", "yellow"),
            ("display-b", "green"),
            ("scene-b", "blue"),
        ] {
            let media_name = format!("{id}-{lang}.mkv");
            source_geometry(
                root,
                &media_name,
                color,
                clock_transition && id.starts_with("display"),
                mixed && id == "scene-a",
                unspecified && id == "display-a",
                color_declaration && id == "display-b",
            );
            let selected = reference(root, &media_name);
            if id.starts_with("display") {
                assets.insert(
                    format!("{id}-{lang}-master"),
                    selected_asset(root, &format!("{id}-{lang}-master"), &media_name),
                );
            }
            let receipt_name = format!("{id}-{lang}-receipt.json");
            let mut receipt = json!({
                "schema":if id.starts_with("display") {"reel.presentation-master-receipt.v1"}
                         else {"reel.scene-build-receipt.v1"},
                "language":lang,"master_sha256":selected["sha256"],
                "master_bytes":selected["bytes"]
            });
            if id.starts_with("display") {
                receipt["role"] = id.into();
                receipt["template_id"] = format!("{id}-template").into();
                receipt["template_definition_sha256"] = if id == "display-a" {
                    "a".repeat(64)
                } else {
                    "b".repeat(64)
                }
                .into();
                receipt["episode_id"] = "episode".into();
                receipt["frames"] = 24.into();
                receipt["samples"] = 48_000.into();
                receipt["timestamps_verified"] = true.into();
                receipt["technical_validation_state"] = "rendered-and-checked".into();
                receipt["selection_evidence_sha256"] =
                    reference(root, &format!("{id}-{lang}-text.json"))["sha256"].clone();
            } else {
                receipt["scene_id"] = id.into();
            }
            write(&root.join(receipt_name), &receipt);
        }
    }
    write(
        &root.join("episode-bindings.json"),
        &json!({
            "schema":"reel.scene-asset-bindings.v1","scope_id":"episode","assets":assets
        }),
    );
    if adopt {
        let mut catalog: Value =
            serde_json::from_slice(&fs::read(root.join("catalog.json")).unwrap()).unwrap();
        for (index, role) in ["display-a", "display-b"].iter().enumerate() {
            let name = format!("{role}-template.json");
            write(
                &root.join(&name),
                &json!({
                    "schema":"reel.selected-presentation-master-template.v1",
                    "template_id":format!("{role}-template"),"kind":role,
                    "width":64,"height":64,"fps_numerator":24,"fps_denominator":1,
                    "sample_rate":48000,"duration_frames":24
                }),
            );
            catalog["templates"][index + 1]["definition_sha256"] =
                reference(root, &name)["sha256"].clone();
        }
        write(&root.join("catalog.json"), &catalog);
        for (lang, _) in text {
            for role in ["display-a", "display-b"] {
                let text_name = format!("{role}-{lang}-text.json");
                let media_name = format!("{role}-{lang}.mkv");
                let mut evidence: Value =
                    serde_json::from_slice(&fs::read(root.join(&text_name)).unwrap()).unwrap();
                evidence["rendered_source_master_sha256"] =
                    reference(root, &media_name)["sha256"].clone();
                write(&root.join(&text_name), &evidence);
                let manifest_name = format!("{role}-{lang}-adopt.json");
                write(
                    &root.join(&manifest_name),
                    &json!({
                        "schema":"reel.presentation-adopt.v1","role":role,"language":lang,
                        "season_id":"season","episode_id":"episode",
                        "template_id":format!("{role}-template"),"catalog":reference(root,"catalog.json"),
                        "template_definition":reference(root,&format!("{role}-template.json")),
                        "season_bindings":reference(root,"season.json"),"episode_bindings":reference(root,"episode-bindings.json"),
                        "source_binding":format!("{role}-{lang}-master"),"source":reference(root,&media_name),
                        "selection_evidence":reference(root,&text_name),"evidence_hash_pointer":"/rendered_source_master_sha256"
                    }),
                );
                let output_dir = format!("adopted-{role}-{lang}");
                let output = Command::new(env!("CARGO_BIN_EXE_reel-presentation-adopt"))
                    .arg("build")
                    .arg(root.join(&manifest_name))
                    .arg("--input-root")
                    .arg(root)
                    .arg("--asset-root")
                    .arg(root)
                    .arg("--output-dir")
                    .arg(root.join(&output_dir))
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                fs::copy(
                    root.join(output_dir).join("receipt.json"),
                    root.join(format!("{role}-{lang}-receipt.json")),
                )
                .unwrap();
            }
        }
    }
    for (lang, _) in text {
        let presentation = ["display-a", "display-b"].map(|role| {
            json!({
                "role":role,"template_id":format!("{role}-template"),
                "content":{"picture_binding":"clean-picture",
                    "source_text_evidence_sha256_by_language":{
                        "es":reference(root,&format!("{role}-es-text.json"))["sha256"],
                        "en":reference(root,&format!("{role}-en-text.json"))["sha256"]
                    }},
                "asset_binding":format!("{role}-{lang}-master")
            })
        });
        let episode_name = format!("episode-{lang}.json");
        write(
            &root.join(&episode_name),
            &json!({
                "schema":"reel.episode-authoring.v1","episode_id":"episode",
                "season_id":"season","authoring_state":"ready-for-private-build",
                "master_template_id":"ordered-master","scene_policy_id":"narrative",
                "score_palette":[],"scene_ids":["scene-a","scene-b"],
                "presentation":presentation
            }),
        );
        let segments = ["scene-a", "display-a", "display-b", "scene-b"].map(|id| {
            let mut item = json!({
                "kind":if id.starts_with("display") {"episode-presentation"} else {"scene"},
                "id":id,"master":reference(root,&format!("{id}-{lang}.mkv")),
                "source_receipt":reference(root,&format!("{id}-{lang}-receipt.json"))
            });
            if id.starts_with("display") {
                item["source_text_evidence"] = reference(root, &format!("{id}-{lang}-text.json"));
                if adopt {
                    item["adoption_manifest"] = reference(root, &format!("{id}-{lang}-adopt.json"));
                    item["master"] = reference(root, &format!("adopted-{id}-{lang}/master.mkv"));
                }
            }
            item
        });
        let mut manifest = json!({
            "schema":"reel.episode-conform.v1","episode_id":"episode","language":lang,
            "output_sample_rate":48000,"catalog":reference(root,"catalog.json"),
            "master_template_definition":reference(root,"master-template.json"),
            "source_master_template":reference(root,"source-master.json"),
            "episode":reference(root,&episode_name),
            "season_bindings":reference(root,"season.json"),
            "episode_bindings":reference(root,"episode-bindings.json"),"segments":segments
        });
        if clock_transition {
            manifest["output_sample_rate"] = 44_100.into();
            manifest["audio_frame_conform"] = "pad-silence-to-picture-boundaries".into();
            manifest["compact_delivery"] = json!({
                "crf":18,"audio_bitrate_kbps":320,"retain_lossless_master":false,
                "intermediate_video_encoding":"h264-lossless"
            });
        }
        if reuse_cache {
            manifest["reuse_verified_presentations"] = true.into();
        }
        let manifest_name = format!("manifest-{lang}.json");
        if mixed {
            manifest["audio_frame_conform"] = "pad-silence-to-picture-boundaries".into();
            manifest["compact_delivery"] = json!({"crf":18,"audio_bitrate_kbps":320,
                "retain_lossless_master":false,"intermediate_video_encoding":"h264-lossless"});
            write(&root.join(&manifest_name), &manifest);
            let rejected = run(root, &manifest_name, &format!("no-policy-{lang}"));
            assert!(!rejected.status.success());
            assert!(String::from_utf8_lossy(&rejected.stderr).contains("differ in geometry"));
            manifest["output_geometry"] =
                json!({"width":64,"height":48,"policy":"preserve-aspect-lanczos"});
            write(&root.join(&manifest_name), &manifest);
            let rejected = run(root, &manifest_name, &format!("aspect-change-{lang}"));
            assert!(!rejected.status.success());
            assert!(
                String::from_utf8_lossy(&rejected.stderr).contains("change source aspect ratio")
            );
            manifest["output_geometry"] = json!({"width":64,"height":64,"policy":"stretch"});
            write(&root.join(&manifest_name), &manifest);
            let rejected = run(root, &manifest_name, &format!("bad-policy-{lang}"));
            assert!(!rejected.status.success());
            assert!(
                String::from_utf8_lossy(&rejected.stderr)
                    .contains("invalid explicit output geometry")
            );
            manifest["output_geometry"]["policy"] = "preserve-aspect-lanczos".into();
            if unspecified {
                write(&root.join(&manifest_name), &manifest);
                let rejected = run(root, &manifest_name, &format!("undeclared-sar-{lang}"));
                assert!(!rejected.status.success());
                assert!(
                    String::from_utf8_lossy(&rejected.stderr)
                        .contains("requires square source pixels")
                );
                manifest["output_geometry"]["assume_square_for_unspecified_sar"] = true.into();
            }
            if color_declaration {
                write(&root.join(&manifest_name), &manifest);
                let rejected = run(root, &manifest_name, &format!("undeclared-matrix-{lang}"));
                assert!(!rejected.status.success());
                assert!(
                    String::from_utf8_lossy(&rejected.stderr)
                        .contains("consistent source color properties")
                );
                manifest["output_geometry"]["assume_color_space_for_unspecified"] = "bt709".into();
                write(&root.join(&manifest_name), &manifest);
                let rejected = run(root, &manifest_name, &format!("unsupported-matrix-{lang}"));
                assert!(!rejected.status.success());
                assert!(
                    String::from_utf8_lossy(&rejected.stderr)
                        .contains("unsupported unspecified color-space declaration")
                );
                manifest["output_geometry"]["assume_color_space_for_unspecified"] =
                    "bt470bg".into();
            }
        }
        write(&root.join(&manifest_name), &manifest);
        let output = run(root, &manifest_name, &format!("output-{lang}"));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt: Value = serde_json::from_slice(
            &fs::read(root.join(format!("output-{lang}/receipt.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["total_frames"], 96);
        assert_eq!(
            receipt["total_samples"],
            if clock_transition { 176_400 } else { 192_000 }
        );
        assert_eq!(receipt["decoded_master_matches_ordered_segments"], true);
        assert_eq!(receipt["timestamps_verified"], true);
        if mixed {
            assert_eq!(receipt["output_geometry"], manifest["output_geometry"]);
            if color_declaration {
                assert_eq!(
                    receipt["segments"][0]["geometry_verification"]["assumed_color_space"],
                    "bt470bg"
                );
                assert_eq!(
                    receipt["segments"][0]["geometry_verification"]["color_properties"]["color_space"],
                    "unknown"
                );
                assert!(
                    receipt["segments"][2]["geometry_verification"]
                        .get("assumed_color_space")
                        .is_none()
                );
            }
            if unspecified {
                let verification = &receipt["segments"][1]["geometry_verification"];
                assert_eq!(verification["sample_aspect_ratio_assumed_square"], true);
                assert_eq!(verification["scaling_applied"], false);
                assert_eq!(verification["transformed_source_matches_selected"], true);
                assert_eq!(receipt["segments"][1]["input_frames"], 24);
                assert_eq!(receipt["segments"][1]["input_samples"], 48_000);
                assert_eq!(
                    receipt["segments"][1]["selected_master_sha256"],
                    manifest["segments"][1]["master"]["sha256"]
                );
            }
            let transformed = &receipt["segments"][0];
            assert_eq!(transformed["input_frames"], 24);
            assert_eq!(transformed["frames"], 24);
            assert_eq!(transformed["input_samples"], 48_000);
            assert_eq!(transformed["samples"], 48_000);
            assert_eq!(transformed["geometry_verification"]["input_width"], 32);
            assert_eq!(transformed["geometry_verification"]["output_width"], 64);
            assert_eq!(
                transformed["geometry_verification"]["scaling_applied"],
                true
            );
            assert_eq!(
                transformed["geometry_verification"]["transformed_source_matches_selected"],
                true
            );
            assert_eq!(
                transformed["selected_master_sha256"],
                manifest["segments"][0]["master"]["sha256"]
            );
            assert_eq!(
                receipt["segments"][1]["geometry_verification"]["scaling_applied"],
                false
            );
            let expected = Command::new("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(root.join(format!("scene-a-{lang}.mkv")))
                .args([
                    "-vf",
                    "scale=64:64:flags=lanczos",
                    "-map",
                    "0:v:0",
                    "-pix_fmt",
                    "yuv444p",
                    "-f",
                    "rawvideo",
                    "-",
                ])
                .output()
                .unwrap();
            assert!(expected.status.success());
            assert_eq!(expected.stdout.len(), 24 * 64 * 64 * 3);
            assert_eq!(
                transformed["decoded_picture_sha256"],
                digest(&expected.stdout)
            );
            let non_square_media = format!("non-square-{lang}.mkv");
            source_geometry(root, &non_square_media, "red", true, false, false, false);
            let mut non_square = manifest.clone();
            let selected = reference(root, &non_square_media);
            let mut original_receipt: Value = serde_json::from_slice(
                &fs::read(root.join(format!("scene-a-{lang}-receipt.json"))).unwrap(),
            )
            .unwrap();
            original_receipt["master_sha256"] = selected["sha256"].clone();
            original_receipt["master_bytes"] = selected["bytes"].clone();
            write(&root.join("non-square-receipt.json"), &original_receipt);
            non_square["segments"][0]["master"] = selected;
            non_square["segments"][0]["source_receipt"] =
                reference(root, "non-square-receipt.json");
            write(&root.join("non-square-manifest.json"), &non_square);
            let rejected = run(
                root,
                "non-square-manifest.json",
                &format!("non-square-{lang}"),
            );
            assert!(!rejected.status.success());
            assert!(
                String::from_utf8_lossy(&rejected.stderr).contains("requires square source pixels"),
                "{}",
                String::from_utf8_lossy(&rejected.stderr)
            );
        } else {
            assert!(receipt.get("output_geometry").is_none());
            assert!(
                receipt["segments"][0]
                    .get("geometry_verification")
                    .is_none()
            );
        }
        if reuse_cache {
            assert_eq!(
                receipt["presentation_verification_cache"]["reused_verified_checks"],
                0
            );
            assert_eq!(
                receipt["presentation_verification_cache"]["fresh_full_checks"],
                2
            );
            let warm_dir = format!("warm-{lang}");
            let warm = run(root, &manifest_name, &warm_dir);
            assert!(
                warm.status.success(),
                "{}",
                String::from_utf8_lossy(&warm.stderr)
            );
            let warm_receipt: Value = serde_json::from_slice(
                &fs::read(root.join(&warm_dir).join("receipt.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                warm_receipt["presentation_verification_cache"]["reused_verified_checks"],
                2
            );
            assert_eq!(
                warm_receipt["presentation_verification_cache"]["fresh_full_checks"],
                0
            );
            assert_eq!(
                warm_receipt["decoded_master_matches_ordered_segments"],
                true
            );
            assert_eq!(warm_receipt["timestamps_verified"], true);
            assert_eq!(warm_receipt["total_frames"], 96);
            assert_eq!(warm_receipt["total_samples"], 176_400);
            assert_eq!(
                warm_receipt["encoded_delivery"]["full_decode_verified"],
                true
            );

            // Cached success never permits a current dependency byte change.
            let cache_dir = root.join(".reel-verification-cache/presentation-adoption-v1");
            let cached_count = fs::read_dir(&cache_dir).unwrap().count();
            let adoption_name = format!("display-a-{lang}-adopt.json");
            let adoption: Value =
                serde_json::from_slice(&fs::read(root.join(&adoption_name)).unwrap()).unwrap();
            let mut paths = vec![
                adoption_name,
                format!("display-a-{lang}-receipt.json"),
                format!("adopted-display-a-{lang}/master.mkv"),
            ];
            for field in [
                "catalog",
                "template_definition",
                "season_bindings",
                "episode_bindings",
                "selection_evidence",
                "source",
            ] {
                paths.push(adoption[field]["path"].as_str().unwrap().into());
            }
            for (index, path) in paths.iter().enumerate() {
                let target = root.join(path);
                let original = fs::read(&target).unwrap();
                let mut bad = original.clone();
                bad[0] ^= 1;
                fs::write(&target, bad).unwrap();
                let rejected = run(root, &manifest_name, &format!("mutated-{lang}-{index}"));
                fs::write(&target, original).unwrap();
                assert!(
                    !rejected.status.success(),
                    "cached byte mutation accepted: {path}"
                );
            }
            let adoption_path = root.join(format!("display-a-{lang}-adopt.json"));
            let original_adoption = fs::read(&adoption_path).unwrap();
            for (index, bad_path) in ["../escape.mkv".to_string(), format!("DISPLAY-A-{lang}.mkv")]
                .iter()
                .enumerate()
            {
                let mut bad_adoption = adoption.clone();
                bad_adoption["source"]["path"] = bad_path.clone().into();
                write(&adoption_path, &bad_adoption);
                let mut bad_manifest = manifest.clone();
                bad_manifest["segments"][1]["adoption_manifest"] =
                    reference(root, &format!("display-a-{lang}-adopt.json"));
                let bad_name = format!("bad-cached-path-{lang}-{index}.json");
                write(&root.join(&bad_name), &bad_manifest);
                let rejected = run(root, &bad_name, &format!("bad-cached-path-{lang}-{index}"));
                fs::write(&adoption_path, &original_adoption).unwrap();
                assert!(
                    !rejected.status.success(),
                    "cached invalid path accepted: {bad_path}"
                );
            }
            assert_eq!(
                fs::read_dir(&cache_dir).unwrap().count(),
                cached_count,
                "failed verification published new success records"
            );

            // Corrupt records are misses, never trusted successes.
            for entry in
                fs::read_dir(root.join(".reel-verification-cache/presentation-adoption-v1"))
                    .unwrap()
            {
                fs::write(entry.unwrap().path(), b"partial{").unwrap();
            }
            let recovered_dir = format!("recovered-{lang}");
            let recovered = run(root, &manifest_name, &recovered_dir);
            assert!(
                recovered.status.success(),
                "{}",
                String::from_utf8_lossy(&recovered.stderr)
            );
            let recovered_receipt: Value = serde_json::from_slice(
                &fs::read(root.join(recovered_dir).join("receipt.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                recovered_receipt["presentation_verification_cache"]["reused_verified_checks"],
                0
            );
            assert_eq!(
                recovered_receipt["presentation_verification_cache"]["fresh_full_checks"],
                2
            );
        }

        if adopt {
            let evidence_name = format!("display-a-{lang}-text.json");
            let original = fs::read(root.join(&evidence_name)).unwrap();
            for (name, bad_hash) in [("missing", None), ("wrong", Some("f".repeat(64)))] {
                let mut evidence: Value = serde_json::from_slice(&original).unwrap();
                if let Some(hash) = bad_hash {
                    evidence["rendered_source_master_sha256"] = hash.into();
                } else {
                    evidence
                        .as_object_mut()
                        .unwrap()
                        .remove("rendered_source_master_sha256");
                }
                write(&root.join(&evidence_name), &evidence);
                // The evidence file is now stale; neither adoption nor conform may accept it.
                let rejected = run(
                    root,
                    &manifest_name,
                    &format!("rejected-{lang}-source-{name}"),
                );
                assert!(
                    !rejected.status.success(),
                    "{name} rendered source hash accepted"
                );
            }
            fs::write(root.join(&evidence_name), original).unwrap();
        }

        let cases = [
            ("omit", vec![1_usize], false),
            ("duplicate", vec![1, 1], false),
            ("reverse", vec![2, 1], false),
        ];
        for (name, positions, _) in cases {
            let mut bad = manifest.clone();
            let original = manifest["segments"].as_array().unwrap();
            let mut changed = vec![original[0].clone()];
            changed.extend(positions.iter().map(|i| original[*i].clone()));
            changed.push(original[3].clone());
            bad["segments"] = changed.into();
            let bad_name = format!("{lang}-{name}.json");
            write(&root.join(&bad_name), &bad);
            let rejected = run(root, &bad_name, &format!("rejected-{lang}-{name}"));
            assert!(!rejected.status.success(), "{name} accepted");
        }
        let wrong = if lang == "es" { "en" } else { "es" };
        manifest["segments"][1]["source_text_evidence"] =
            reference(root, &format!("display-a-{wrong}-text.json"));
        let bad_name = format!("{lang}-wrong-language.json");
        write(&root.join(&bad_name), &manifest);
        let rejected = run(root, &bad_name, &format!("rejected-{lang}-wrong-language"));
        assert!(!rejected.status.success());
    }
}
