use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

fn write(root: &Path, name: &str, value: &Value) {
    fs::write(root.join(name), serde_json::to_vec(value).unwrap()).unwrap();
}
fn reference(root: &Path, name: &str) -> Value {
    let data = fs::read(root.join(name)).unwrap();
    json!({"path":name,"sha256":Sha256::digest(&data).iter().map(|b| format!("{b:02x}")).collect::<String>(),"bytes":data.len()})
}
fn cli(executable: &Path, args: &[&str]) -> std::process::Output {
    Command::new(executable).args(args).output().unwrap()
}
#[track_caller]
fn success(output: std::process::Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn job(root: &Path) {
    let mut inputs = serde_json::Map::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for item in fs::read_dir(dir).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path == root.join("job.json") {
                continue;
            }
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            inputs.insert(name.clone(), reference(root, &name));
        }
    }
    write(
        root,
        "job.json",
        &json!({"inputs":inputs,"tools":{},"files":{}}),
    );
}
fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
#[ignore = "requires explicit authentic historical producer binary"]
fn authentic_historical_producer_complete_prefix_sets_at_25_and_30() {
    let historical =
        std::env::var("REEL_HISTORICAL_PRODUCER").expect("explicit historical executable");
    for target_fps in [30u64, 25] {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("new-composer.exe");
        fs::copy(env!("CARGO_BIN_EXE_reel-episode-conform"), &executable).unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        let units =
            json!([{"kind":"scene","id":"a"},{"kind":"scene","id":"b"},{"kind":"scene","id":"c"}]);
        write(
            &source,
            "template.json",
            &json!({"schema":"reel.episode-master-template.v2","template_id":"master","ordered_units":units}),
        );
        write(
            &source,
            "catalog.json",
            &json!({"schema":"reel.scene-template-catalog.v1","templates":[{"template_id":"master","kind":"episode-master","definition_sha256":reference(&source,"template.json")["sha256"],"required_content_keys":[]}]}),
        );
        write(
            &source,
            "episode.json",
            &json!({"schema":"reel.episode-authoring.v1","episode_id":"fixture","season_id":"season","authoring_state":"ready-for-private-build","master_template_id":"master","scene_policy_id":"narrative","score_palette":[],"scene_ids":["a","b","c"],"presentation":[]}),
        );
        for (name, id) in [("season.json", "season"), ("bindings.json", "fixture")] {
            write(
                &source,
                name,
                &json!({"schema":"reel.scene-asset-bindings.v1","scope_id":id,"assets":{}}),
            );
        }
        let mut segments = Vec::new();
        for (i, (id, fps, sr)) in [
            ("a", 25, 48000),
            ("b", if target_fps == 25 { 25 } else { 30 }, 44100),
            ("c", 24, 48000),
        ]
        .iter()
        .enumerate()
        {
            let media = format!("{id}.mkv");
            let status = Command::new("ffmpeg")
                .args(["-v", "error", "-f", "lavfi", "-i"])
                .arg(format!("testsrc2=size=64x64:rate={fps}:duration=1"))
                .args(["-f", "lavfi", "-i"])
                .arg(format!(
                    "sine=frequency={}:sample_rate={sr}:duration=1",
                    400 + i * 173
                ))
                .args([
                    "-ac",
                    "2",
                    "-c:v",
                    "ffv1",
                    "-pix_fmt",
                    "yuv444p",
                    "-color_range",
                    "tv",
                    "-colorspace",
                    "bt709",
                    "-color_trc",
                    "bt709",
                    "-color_primaries",
                    "bt709",
                    "-c:a",
                    "pcm_s24le",
                ])
                .arg(source.join(&media))
                .status()
                .unwrap();
            assert!(status.success());
            let receipt = format!("{id}.json");
            let master = reference(&source, &media);
            write(
                &source,
                &receipt,
                &json!({"schema":"reel.scene-build-receipt.v1","scene_id":id,"language":"es","master_sha256":master["sha256"],"master_bytes":master["bytes"]}),
            );
            segments.push(json!({"kind":"scene","id":id,"master":master,"source_receipt":reference(&source,&receipt),"tail_hold_frames":60}));
        }
        let manifest = json!({"schema":"reel.episode-conform.v1","episode_id":"fixture","language":"es","output_sample_rate":48000,
        "output_frame_rate":{"numerator":target_fps,"denominator":1,"policy":"repeat-only"},"audio_frame_conform":"pad-silence-to-picture-boundaries",
        "catalog":reference(&source,"catalog.json"),"master_template_definition":reference(&source,"template.json"),"episode":reference(&source,"episode.json"),"season_bindings":reference(&source,"season.json"),"episode_bindings":reference(&source,"bindings.json"),
        "compact_delivery":{"crf":23,"audio_bitrate_kbps":128,"retain_lossless_master":true},"segments":segments});
        write(&source, "manifest.json", &manifest);
        // Authentic old producer receives fully rechecked adopted presentations, so
        // the resulting actual v3 receipt is also consumable by the caption path.
        let mut manifest = manifest;
        let master = reference(&source, "a.mkv");
        let h = master["sha256"].as_str().unwrap();
        let mut body_catalog: Value =
            serde_json::from_slice(&fs::read(source.join("catalog.json")).unwrap()).unwrap();
        let mut body_episode: Value =
            serde_json::from_slice(&fs::read(source.join("episode.json")).unwrap()).unwrap();
        let mut bindings: Value =
            serde_json::from_slice(&fs::read(source.join("bindings.json")).unwrap()).unwrap();
        let mut uses = vec![];
        let mut segments = vec![];
        let mut units = vec![];
        for id in ["a", "b", "c"] {
            write(
                &source,
                &format!("{id}-template.json"),
                &json!({"schema":"reel.selected-presentation-master-template.v1","template_id":format!("{id}-template"),"kind":id,"width":64,"height":64,"fps_numerator":25,"fps_denominator":1,"sample_rate":48000,"duration_frames":25}),
            );
            body_catalog["templates"].as_array_mut().unwrap().push(json!({"template_id":format!("{id}-template"),"kind":id,"definition_sha256":reference(&source,&format!("{id}-template.json"))["sha256"],"required_content_keys":[]}));
            bindings["assets"][format!("{id}-master")] = json!({"logical_id":format!("{id}-master"),"sha256":h,"bytes":master["bytes"],"cache_uri":format!("cache://sha256/{h}"),"selection_state":"selected-private-production"});
            uses.push(json!({"role":id,"template_id":format!("{id}-template"),"content":{},"asset_binding":format!("{id}-master")}));
            units.push(json!({"kind":"episode-presentation","id":id}));
        }
        write(
            &source,
            "template.json",
            &json!({"schema":"reel.episode-master-template.v2","template_id":"master","ordered_units":units}),
        );
        body_catalog["templates"][0]["definition_sha256"] =
            reference(&source, "template.json")["sha256"].clone();
        write(&source, "catalog.json", &body_catalog);
        body_episode["scene_ids"] = json!([]);
        body_episode["presentation"] = json!(uses);
        write(&source, "episode.json", &body_episode);
        write(&source, "bindings.json", &bindings);
        write(
            &source,
            "selection.json",
            &json!({"master_sha256":h,"approval":false}),
        );
        for id in ["a", "b", "c"] {
            let adopt = format!("{id}-adopt.json");
            write(
                &source,
                &adopt,
                &json!({"schema":"reel.presentation-adopt.v1","role":id,"language":"es","season_id":"season","episode_id":"fixture","template_id":format!("{id}-template"),"catalog":reference(&source,"catalog.json"),"template_definition":reference(&source,&format!("{id}-template.json")),"season_bindings":reference(&source,"season.json"),"episode_bindings":reference(&source,"bindings.json"),"source_binding":format!("{id}-master"),"source":master,"selection_evidence":reference(&source,"selection.json"),"evidence_hash_pointer":"/master_sha256","output_mode":"retain-source"}),
            );
            let out = source.join(format!("adopted-{id}"));
            success(cli(
                Path::new(env!("CARGO_BIN_EXE_reel-presentation-adopt")),
                &[
                    "build",
                    path(&source.join(&adopt)),
                    "--input-root",
                    path(&source),
                    "--asset-root",
                    path(&source),
                    "--output-dir",
                    path(&out),
                ],
            ));
            segments.push(json!({"kind":"episode-presentation","id":id,"master":master,"source_receipt":reference(&source,&format!("adopted-{id}/receipt.json")),"adoption_manifest":reference(&source,&adopt),"tail_hold_frames":2*target_fps}));
        }
        manifest["segments"] = json!(segments);
        for (field, name) in [
            ("catalog", "catalog.json"),
            ("master_template_definition", "template.json"),
            ("episode", "episode.json"),
            ("episode_bindings", "bindings.json"),
        ] {
            manifest[field] = reference(&source, name);
        }
        write(&source, "manifest.json", &manifest);
        // Compose complete immutable source sets in separate namespaces. No old proof
        // or media bytes are rewritten to pretend the prefix existed originally.
        let compose_root = temp.path().join("composition-input");
        fs::create_dir(&compose_root).unwrap();
        let historical_exe = temp.path().join("historical-producer.exe");
        fs::copy(&historical, &historical_exe).unwrap();
        let producer = historical_exe.as_path();
        let mut expected_proofs = vec![];
        let body = compose_root.join("body");
        fs::create_dir(&body).unwrap();
        write(
            &source,
            "range.json",
            &json!({"start":0,"end":3,"start_frame":0,"start_sample":0}),
        );
        job(&source);
        let old_output = temp.path().join("old-produced");
        success(cli(
            producer,
            &[
                "produce-parts",
                path(&source.join("manifest.json")),
                "--asset-root",
                path(&source),
                "--range",
                path(&source.join("range.json")),
                "--output-dir",
                path(&old_output),
            ],
        ));
        let original_proof = fs::read(old_output.join("part-proof.json")).unwrap();
        let proof: Value = serde_json::from_slice(&original_proof).unwrap();
        assert_eq!(
            proof["validator_sha256"],
            "11da1731ddff8ab6df0cf93ad04596a950204b1dcd41699ff11e5b5b43e3b227"
        );
        for part in proof["parts"].as_array().unwrap() {
            for kind in ["video", "pcm"] {
                let name = part[kind]["path"].as_str().unwrap();
                fs::copy(old_output.join(name), body.join(name)).unwrap();
            }
        }
        fs::copy(
            old_output.join("part-proof.json"),
            body.join("proof-0.json"),
        )
        .unwrap();
        expected_proofs.push(reference(&body, "proof-0.json"));
        for name in [
            "manifest.json",
            "catalog.json",
            "template.json",
            "episode.json",
            "season.json",
            "bindings.json",
        ] {
            fs::copy(source.join(name), body.join(name)).unwrap();
        }
        write(
            &body,
            "set.json",
            &json!({"schema":"reel.programme-part-set.v2","manifest":reference(&body,"manifest.json"),"proofs":expected_proofs}),
        );
        fs::copy(producer, compose_root.join("historical.exe")).unwrap();
        let prefix_source = temp.path().join("prefix-source");
        fs::create_dir(&prefix_source).unwrap();
        for name in [
            "catalog.json",
            "template.json",
            "episode.json",
            "season.json",
            "bindings.json",
            "a.mkv",
        ] {
            fs::copy(source.join(name), prefix_source.join(name)).unwrap();
        }
        let mut prefix = manifest.clone();
        prefix["segments"] = json!([segments[0].clone()]);
        prefix["segments"][0]["id"] = "opening".into();
        prefix["segments"][0]["kind"] = "episode-presentation".into();
        write(
            &prefix_source,
            "opening-template.json",
            &json!({"schema":"reel.selected-presentation-master-template.v1","template_id":"opening-template","kind":"opening","width":64,"height":64,"fps_numerator":25,"fps_denominator":1,"sample_rate":48000,"duration_frames":25}),
        );
        write(
            &prefix_source,
            "template.json",
            &json!({"schema":"reel.episode-master-template.v2","template_id":"master","ordered_units":[{"kind":"episode-presentation","id":"opening"}]}),
        );
        let mut catalog: Value =
            serde_json::from_slice(&fs::read(prefix_source.join("catalog.json")).unwrap()).unwrap();
        catalog["templates"][0]["definition_sha256"] =
            reference(&prefix_source, "template.json")["sha256"].clone();
        catalog["templates"].as_array_mut().unwrap().push(json!({"template_id":"opening-template","kind":"opening","definition_sha256":reference(&prefix_source,"opening-template.json")["sha256"],"required_content_keys":[]}));
        write(&prefix_source, "catalog.json", &catalog);
        let master = reference(&prefix_source, "a.mkv");
        let hash = master["sha256"].as_str().unwrap();
        let selected = json!({"logical_id":"opening-master","sha256":hash,"bytes":master["bytes"],"cache_uri":format!("cache://sha256/{hash}"),"selection_state":"selected-private-production"});
        write(
            &prefix_source,
            "bindings.json",
            &json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"fixture","assets":{"opening-master":selected}}),
        );
        let mut episode: Value =
            serde_json::from_slice(&fs::read(prefix_source.join("episode.json")).unwrap()).unwrap();
        episode["scene_ids"] = json!([]);
        episode["presentation"] = json!([{"role":"opening","template_id":"opening-template","content":{},"asset_binding":"opening-master"}]);
        write(&prefix_source, "episode.json", &episode);
        write(
            &prefix_source,
            "selection.json",
            &json!({"master_sha256":hash,"approval":false}),
        );
        write(
            &prefix_source,
            "adopt.json",
            &json!({"schema":"reel.presentation-adopt.v1","role":"opening","language":"es","season_id":"season","episode_id":"fixture","template_id":"opening-template","catalog":reference(&prefix_source,"catalog.json"),"template_definition":reference(&prefix_source,"opening-template.json"),"season_bindings":reference(&prefix_source,"season.json"),"episode_bindings":reference(&prefix_source,"bindings.json"),"source_binding":"opening-master","source":master,"selection_evidence":reference(&prefix_source,"selection.json"),"evidence_hash_pointer":"/master_sha256","output_mode":"retain-source"}),
        );
        let adopted = prefix_source.join("adopted");
        success(cli(
            Path::new(env!("CARGO_BIN_EXE_reel-presentation-adopt")),
            &[
                "build",
                path(&prefix_source.join("adopt.json")),
                "--input-root",
                path(&prefix_source),
                "--asset-root",
                path(&prefix_source),
                "--output-dir",
                path(&adopted),
            ],
        ));
        prefix["segments"][0]["source_receipt"] = reference(&prefix_source, "adopted/receipt.json");
        prefix["segments"][0]["adoption_manifest"] = reference(&prefix_source, "adopt.json");
        for (field, name) in [
            ("catalog", "catalog.json"),
            ("master_template_definition", "template.json"),
            ("episode", "episode.json"),
            ("episode_bindings", "bindings.json"),
        ] {
            prefix[field] = reference(&prefix_source, name);
        }
        write(&prefix_source, "manifest.json", &prefix);
        write(
            &prefix_source,
            "range.json",
            &json!({"start":0,"end":1,"start_frame":0,"start_sample":0}),
        );
        job(&prefix_source);
        let prefix_dir = compose_root.join("prefix");
        success(cli(
            &executable,
            &[
                "produce-parts",
                path(&prefix_source.join("manifest.json")),
                "--asset-root",
                path(&prefix_source),
                "--range",
                path(&prefix_source.join("range.json")),
                "--output-dir",
                path(&prefix_dir),
            ],
        ));
        for name in [
            "manifest.json",
            "catalog.json",
            "template.json",
            "episode.json",
            "season.json",
            "bindings.json",
        ] {
            fs::copy(prefix_source.join(name), prefix_dir.join(name)).unwrap();
        }
        write(
            &prefix_dir,
            "set.json",
            &json!({"schema":"reel.programme-part-set.v2","manifest":reference(&prefix_dir,"manifest.json"),"proofs":[reference(&prefix_dir,"part-proof.json")]}),
        );
        fs::copy(&executable, compose_root.join("producer.exe")).unwrap();
        for name in [
            "catalog.json",
            "template.json",
            "episode.json",
            "season.json",
            "bindings.json",
        ] {
            fs::copy(source.join(name), compose_root.join(name)).unwrap();
        }
        let mut target = manifest.clone();
        let mut target_segments = vec![prefix["segments"][0].clone()];
        target_segments.extend(segments.clone());
        target["segments"] = json!(target_segments);
        let mut target_units = vec![json!({"kind":"episode-presentation","id":"opening"})];
        target_units.extend(units.clone());
        write(
            &compose_root,
            "template.json",
            &json!({"schema":"reel.episode-master-template.v2","template_id":"master","ordered_units":target_units}),
        );
        catalog["templates"][0]["definition_sha256"] =
            reference(&compose_root, "template.json")["sha256"].clone();
        write(&compose_root, "catalog.json", &catalog);
        episode["scene_ids"] = json!([]);
        let mut target_uses = episode["presentation"].as_array().unwrap().clone();
        target_uses.extend(body_episode["presentation"].as_array().unwrap().clone());
        episode["presentation"] = json!(target_uses);
        write(&compose_root, "episode.json", &episode);
        let mut target_bindings = bindings.clone();
        let prefix_bindings: Value =
            serde_json::from_slice(&fs::read(prefix_source.join("bindings.json")).unwrap())
                .unwrap();
        target_bindings["assets"]
            .as_object_mut()
            .unwrap()
            .extend(prefix_bindings["assets"].as_object().unwrap().clone());
        write(&compose_root, "bindings.json", &target_bindings);
        for (field, name) in [
            ("catalog", "catalog.json"),
            ("master_template_definition", "template.json"),
            ("episode", "episode.json"),
            ("season_bindings", "season.json"),
            ("episode_bindings", "bindings.json"),
        ] {
            target[field] = reference(&compose_root, name);
        }
        write(&compose_root, "target.json", &target);
        let composition = json!({"schema":"reel.programme-part-composition.v1","manifest":reference(&compose_root,"target.json"),"sources":[{"id":"prefix","asset_root":"prefix","set":reference(&compose_root,"prefix/set.json"),"producer":reference(&compose_root,"producer.exe")},{"id":"body","asset_root":"body","set":reference(&compose_root,"body/set.json"),"producer":reference(&compose_root,"historical.exe")}],"order":["prefix","body"]});
        write(&compose_root, "composition.json", &composition);
        job(&compose_root);
        let composed = temp.path().join("composed");
        success(cli(
            &executable,
            &[
                "compose-parts",
                path(&compose_root.join("composition.json")),
                "--asset-root",
                path(&compose_root),
                "--output-dir",
                path(&composed),
            ],
        ));
        let r: Value =
            serde_json::from_slice(&fs::read(composed.join("receipt.json")).unwrap()).unwrap();
        assert_eq!(r["schema"], "reel.episode-conform-receipt.v3");
        assert_eq!(r["finalization"]["total_frames"], 12 * target_fps);
        assert_eq!(r["finalization"]["total_samples"], 576000);
        assert_eq!(r["coded_video_packets_verified"], true);
        assert_eq!(r["provenance"]["placements"][1]["source_start_frame"], 0);
        assert_eq!(
            r["provenance"]["placements"][1]["start_frame"],
            3 * target_fps
        );
        assert_eq!(fs::read(body.join("proof-0.json")).unwrap(), original_proof);
        fs::copy(
            composed.join("receipt.json"),
            compose_root.join("composed-receipt.json"),
        )
        .unwrap();
        write(
            &compose_root,
            "caption-selection.json",
            &json!({"approval":false,"fixture":"source words unchanged"}),
        );
        let mut tracks = vec![];
        for (i, segment) in target["segments"].as_array().unwrap().iter().enumerate() {
            let text = if i == 0 {
                ""
            } else {
                "1\n00:00:00,000 --> 00:00:00,500\nExact source words.\n"
            };
            let filename = format!("caption-{i}.srt");
            fs::write(compose_root.join(&filename), text).unwrap();
            let words = if i == 0 {
                json!([])
            } else {
                json!(["Exact source words."])
            };
            tracks.push(json!({"segment_id":segment["id"],"language":"es","selected_master_sha256":segment["master"]["sha256"],"source_caption":reference(&compose_root,&filename),"source_words_sha256":Sha256::digest(serde_json::to_vec(&words).unwrap()).iter().map(|b|format!("{b:02x}")).collect::<String>(),"selection_evidence":reference(&compose_root,"caption-selection.json"),"source_frame_rate":{"numerator":25,"denominator":1,"policy":"repeat-only"}}));
        }
        write(
            &compose_root,
            "captions.json",
            &json!({"schema":"reel.episode-captions.v1","language":"es","conform_manifest":reference(&compose_root,"target.json"),"conform_receipt":reference(&compose_root,"composed-receipt.json"),"tracks":tracks}),
        );
        let captions = temp.path().join("actual-composed-captions");
        reel::episode_captions::assemble(
            &compose_root.join("captions.json"),
            &compose_root,
            &captions,
        )
        .unwrap();
        let cues =
            reel::series::parse_srt(&fs::read_to_string(captions.join("captions.srt")).unwrap())
                .unwrap();
        assert_eq!(cues.len(), 3);
        assert_eq!(cues[0].start_ms, 3000);
        assert_eq!(cues[2].start_ms, 9000);
        for mutation in ["repeat", "omit", "producer", "offset", "selection"] {
            let mut c = composition.clone();
            match mutation {
                "repeat" => c["order"] = json!(["prefix", "prefix"]),
                "omit" => c["order"] = json!(["prefix"]),
                "producer" => {
                    fs::write(compose_root.join("foreign.exe"), b"unqualified").unwrap();
                    c["sources"][1]["producer"] = reference(&compose_root, "foreign.exe");
                }
                "offset" => c["start_frame"] = 1.into(),
                "selection" => {
                    let mut bad = target.clone();
                    bad["segments"][1]["tail_hold_frames"] = 59.into();
                    write(&compose_root, "bad-target.json", &bad);
                    c["manifest"] = reference(&compose_root, "bad-target.json");
                }
                _ => unreachable!(),
            }
            write(&compose_root, "composition.json", &c);
            job(&compose_root);
            let rejected = temp.path().join(format!("composition-rejected-{mutation}"));
            assert!(
                !cli(
                    &executable,
                    &[
                        "compose-parts",
                        path(&compose_root.join("composition.json")),
                        "--asset-root",
                        path(&compose_root),
                        "--output-dir",
                        path(&rejected)
                    ]
                )
                .status
                .success(),
                "accepted {mutation}"
            );
            assert!(!rejected.exists());
        }
    }
}
