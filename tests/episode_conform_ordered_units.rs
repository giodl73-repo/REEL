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

fn source(root: &Path, name: &str, color: &str) {
    let mut cmd = Command::new("ffmpeg");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let status = cmd
        .args(["-v", "error", "-f", "lavfi", "-i"])
        .arg(format!("color=c={color}:s=64x64:r=24:d=1"))
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
            source(root, &media_name, color);
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
        let manifest_name = format!("manifest-{lang}.json");
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
        assert_eq!(receipt["total_samples"], 192_000);
        assert_eq!(receipt["decoded_master_matches_ordered_segments"], true);

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
