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
    for item in fs::read_dir(root).unwrap() {
        let path = item.unwrap().path();
        if path.is_file() && path.file_name().unwrap() != "job.json" {
            let name = path.file_name().unwrap().to_str().unwrap();
            inputs.insert(name.into(), reference(root, name));
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

#[cfg(all(windows, feature = "tig-publication"))]
fn run_managed_part(executable: &Path, source: &Path, output: &Path) {
    let fixture = tempfile::tempdir().unwrap();
    let project = fixture.path();
    let cache = project.join("cache");
    for dir in [
        cache.as_path(),
        &project.join(".local"),
        &project.join("registry"),
    ] {
        fs::create_dir(dir).unwrap();
    }
    write(
        project,
        ".local/storage-rust.json",
        &json!({
            "cache_roots":[cache],"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}
        }),
    );
    let request = project.join("request.json");
    let mut parameters = json!({
        "schema":"reel.programme-part-job-request.v1",
        "manifest":"manifest.json","range":"range.json","runtime":"runtime.json",
        "budget_bytes":1,"scratch_bytes":16*1024*1024,"timeout_seconds":180,
        "lossless_max_bytes":8*1024*1024,"video_max_bytes":8*1024*1024,
        "pcm_max_bytes":2*1024*1024
    });
    let describe = || {
        cli(
            executable,
            &[
                "describe-part-job",
                path(&request),
                "--asset-root",
                path(source),
            ],
        )
    };
    write(project, "request.json", &parameters);
    let refused = describe();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("does not fit runner limits"));
    parameters["budget_bytes"] = json!(64 * 1024 * 1024);
    write(project, "request.json", &parameters);
    let described = describe();
    assert!(
        described.status.success(),
        "{}",
        String::from_utf8_lossy(&described.stderr)
    );
    let recipe: Value = serde_json::from_slice(&described.stdout).unwrap();
    for (name, pin) in recipe["inputs"].as_object().unwrap() {
        let stored = reel_assets::ingest(project, &source.join(name)).unwrap();
        assert_eq!(stored["sha256"], pin["sha256"]);
        assert_eq!(stored["bytes"], pin["bytes"]);
    }
    write(
        project,
        "catalog.json",
        &json!({"videos":[{"record":"inputs.json"}]}),
    );
    let mut registered_inputs = recipe["inputs"].as_object().unwrap().clone();
    registered_inputs.extend(recipe["installed_tools"].as_object().unwrap().clone());
    write(project, "inputs.json", &json!({"assets":registered_inputs}));
    write(project, "recipe.json", &recipe);
    let result = reel_assets::jobs::run(project, &project.join("recipe.json")).unwrap();
    assert_eq!(result["workspace_retained"], false);
    assert!(!Path::new(result["workspace"].as_str().unwrap()).exists());
    let registered = reel_assets::generic_objects(project).unwrap();
    fs::create_dir(output).unwrap();
    let outputs = result["outputs"].as_object().unwrap();
    assert_eq!(outputs.len(), 4);
    for (name, file) in outputs {
        let oid = file["sha256"].as_str().unwrap();
        let bytes = file["bytes"].as_u64().unwrap();
        let retained = Path::new(file["path"].as_str().unwrap());
        assert_eq!(registered.get(oid), Some(&bytes));
        reel_assets::verify(retained, oid, bytes).unwrap();
        fs::copy(retained, output.join(name.strip_prefix("native/").unwrap())).unwrap();
    }
}

#[test]
fn moving_tone_mixed_clocks_holds_portable_parts_copy_and_continuous_mp3() {
    // Required dependencies: failure to launch is a test failure, never a skip.
    assert!(
        Command::new("ffmpeg")
            .arg("-version")
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        Command::new("ffprobe")
            .arg("-version")
            .output()
            .unwrap()
            .status
            .success()
    );
    let temp = tempfile::tempdir().unwrap();
    let executable = temp.path().join("native-programme-fixture.exe");
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
    for (i, (id, fps, sr)) in [("a", 25, 48000), ("b", 30, 44100), ("c", 24, 48000)]
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
        "output_frame_rate":{"numerator":30,"denominator":1,"policy":"repeat-only"},"audio_frame_conform":"pad-silence-to-picture-boundaries",
        "catalog":reference(&source,"catalog.json"),"master_template_definition":reference(&source,"template.json"),"episode":reference(&source,"episode.json"),"season_bindings":reference(&source,"season.json"),"episode_bindings":reference(&source,"bindings.json"),
        "compact_delivery":{"crf":23,"audio_bitrate_kbps":128,"retain_lossless_master":true},"segments":segments});
    write(&source, "manifest.json", &manifest);
    let baseline = temp.path().join("baseline");
    success(cli(
        &executable,
        &[
            "build",
            path(&source.join("manifest.json")),
            "--input-root",
            path(&source),
            "--asset-root",
            path(&source),
            "--output-dir",
            path(&baseline),
        ],
    ));
    let baseline: Value =
        serde_json::from_slice(&fs::read(baseline.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(baseline["schema"], "reel.episode-conform-receipt.v1");
    let final_root = temp.path().join("final-input");
    fs::create_dir(&final_root).unwrap();
    fs::copy(
        source.join("manifest.json"),
        final_root.join("manifest.json"),
    )
    .unwrap();
    let mut proofs = Vec::new();
    for (i, id) in ["a", "b", "c"].iter().enumerate() {
        let root = temp.path().join(format!("producer-{i}"));
        fs::create_dir(&root).unwrap();
        for name in [
            "manifest.json",
            "catalog.json",
            "template.json",
            "episode.json",
            "season.json",
            "bindings.json",
            &format!("{id}.mkv"),
            &format!("{id}.json"),
        ] {
            fs::copy(source.join(name), root.join(name)).unwrap();
        }
        let expected_range =
            json!({"start":i,"end":i+1,"start_frame":i*90,"start_sample":i*144000});
        let range = if i == 0 {
            expected_range.clone()
        } else {
            let prefix = json!({"schema":"reel.programme-part-set.v2",
                "manifest":reference(&final_root,"manifest.json"),"proofs":proofs});
            write(&final_root, "set.json", &prefix);
            job(&final_root);
            if i == 1 {
                let refused_output = temp.path().join("incomplete-delivery");
                let refused = cli(
                    &executable,
                    &[
                        "finalize-parts",
                        path(&final_root.join("set.json")),
                        "--asset-root",
                        path(&final_root),
                        "--output-dir",
                        path(&refused_output),
                    ],
                );
                assert!(!refused.status.success());
                assert!(
                    String::from_utf8_lossy(&refused.stderr)
                        .contains("incomplete programme part coverage")
                );
                assert!(!refused_output.exists());
            }
            let planned = cli(
                &executable,
                &[
                    "next-part-range",
                    path(&final_root.join("set.json")),
                    "--asset-root",
                    path(&final_root),
                    "--end",
                    &(i + 1).to_string(),
                ],
            );
            assert!(
                planned.status.success(),
                "{}",
                String::from_utf8_lossy(&planned.stderr)
            );
            serde_json::from_slice(&planned.stdout).unwrap()
        };
        assert_eq!(range, expected_range);
        write(&root, "range.json", &range);
        let runtime = root.join("runtime.json");
        if cfg!(windows) {
            use reel::media_runtime::ByteIdentity;
            let program = |name: &str| {
                let search = std::env::var_os("PATH").unwrap();
                let path = std::env::split_paths(&search)
                    .map(|directory| directory.join(name))
                    .find(|path| path.is_file())
                    .unwrap()
                    .canonicalize()
                    .unwrap();
                let pin = ByteIdentity::observe_file(&path).unwrap();
                json!({"path":path,"sha256":pin.sha256,"bytes":pin.bytes})
            };
            write(
                &root,
                "runtime.json",
                &json!({
                    "host":ByteIdentity::observe_file(&executable).unwrap(),
                    "ffmpeg":program("ffmpeg.exe"),
                    "ffprobe":program("ffprobe.exe"),
                    "dependencies":{}
                }),
            );
        }
        job(&root);
        let output = temp.path().join(format!("part-{i}"));
        let manifest_path = root.join("manifest.json");
        let range_path = root.join("range.json");
        let mut args = vec![
            "produce-parts",
            path(&manifest_path),
            "--asset-root",
            path(&root),
            "--range",
            path(&range_path),
            "--output-dir",
            path(&output),
        ];
        if cfg!(windows) {
            args.extend(["--runtime", path(&runtime)]);
        }
        #[cfg(all(windows, feature = "tig-publication"))]
        if i == 0 {
            run_managed_part(&executable, &root, &output);
        } else {
            success(cli(&executable, &args));
        }
        #[cfg(not(all(windows, feature = "tig-publication")))]
        success(cli(&executable, &args));
        let proof: Value =
            serde_json::from_slice(&fs::read(output.join("part-proof.json")).unwrap()).unwrap();
        // Existing v1 lossless normalization is the parity oracle, including
        // resampling, repeat-only frames, full musical endpoints and silent holds.
        assert_eq!(proof["parts"][0]["native"], baseline["segments"][i]);
        assert_eq!(proof["parts"][0]["native"]["tail_hold_samples"], 96000);
        for kind in ["video", "pcm"] {
            let name = proof["parts"][0][kind]["path"].as_str().unwrap();
            fs::copy(output.join(name), final_root.join(name)).unwrap();
        }
        // Lossless files deliberately never enter finalizer input scope.
        let name = format!("proof-{i}.json");
        fs::copy(output.join("part-proof.json"), final_root.join(&name)).unwrap();
        proofs.push(reference(&final_root, &name));
    }
    let set = json!({"schema":"reel.programme-part-set.v2","manifest":reference(&final_root,"manifest.json"),"proofs":proofs});
    write(&final_root, "set.json", &set);
    job(&final_root);
    let final_output = temp.path().join("delivery");
    success(cli(
        &executable,
        &[
            "finalize-parts",
            path(&final_root.join("set.json")),
            "--asset-root",
            path(&final_root),
            "--output-dir",
            path(&final_output),
        ],
    ));
    let receipt: Value =
        serde_json::from_slice(&fs::read(final_output.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["total_frames"], 270);
    assert_eq!(receipt["total_samples"], 432000);
    assert_eq!(receipt["segments"], baseline["segments"]);
    assert_eq!(
        receipt["lossless_master"]["parts"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(receipt["boundary_findings"].as_array().unwrap().len(), 2);
    assert!(receipt.get("master_sha256").is_none());
    assert!(!final_output.join("master.mkv").exists());
    assert_eq!(fs::read_dir(&final_output).unwrap().count(), 2);
    let input_bytes: u64 = fs::read_dir(&final_root)
        .unwrap()
        .map(|p| p.unwrap().metadata().unwrap().len())
        .sum();
    let retained_bytes: u64 = fs::read_dir(&final_output)
        .unwrap()
        .map(|p| p.unwrap().metadata().unwrap().len())
        .sum();
    let pcm_scratch = 432000 * 6;
    let conservative_fixture_bytes = input_bytes + pcm_scratch + 2 * retained_bytes;
    assert!(conservative_fixture_bytes < 8 * 1024 * 1024 * 1024);
    println!(
        "fixture finalization inputs={input_bytes}B continuous_pcm_scratch={pcm_scratch}B retained={retained_bytes}B input+PCM+2retained={conservative_fixture_bytes}B (fixture accounting, not an observed OS peak or production admission)"
    );

    // Reorder, duplicate, omission, and metadata mutations are tested through
    // the real entrypoint with freshly declared controls, not stale hash failures.
    let original_proof = fs::read(final_root.join("proof-0.json")).unwrap();
    for mutation in [
        "reorder",
        "duplicate",
        "gap",
        "language",
        "source",
        "profile",
        "tool",
        "clock",
        "hold",
        "content",
    ] {
        let mut altered = set.clone();
        if mutation == "reorder" {
            altered["proofs"].as_array_mut().unwrap().swap(0, 1);
        } else if mutation == "duplicate" {
            altered["proofs"][1] = altered["proofs"][0].clone();
        } else if mutation == "gap" {
            altered["proofs"].as_array_mut().unwrap().remove(1);
        } else {
            let mut proof: Value = serde_json::from_slice(&original_proof).unwrap();
            match mutation {
                "language" => proof["language"] = "en".into(),
                "source" => {
                    proof["parts"][0]["native"]["selected_master_sha256"] = "0".repeat(64).into()
                }
                "profile" => proof["crf"] = 24.into(),
                "tool" => proof["ffmpeg_sha256"] = "0".repeat(64).into(),
                "clock" => proof["range"]["start_sample"] = 1.into(),
                "hold" => proof["parts"][0]["native"]["tail_hold_frames"] = 59.into(),
                "content" => proof["parts"][0]["compact_yuv420_sha256"] = "0".repeat(64).into(),
                _ => unreachable!(),
            }
            write(&final_root, "proof-0.json", &proof);
            altered["proofs"][0] = reference(&final_root, "proof-0.json");
        }
        write(&final_root, "set.json", &altered);
        job(&final_root);
        let rejected = temp.path().join(format!("rejected-{mutation}"));
        let result = cli(
            &executable,
            &[
                "finalize-parts",
                path(&final_root.join("set.json")),
                "--asset-root",
                path(&final_root),
                "--output-dir",
                path(&rejected),
            ],
        );
        assert!(!result.status.success(), "mutation {mutation} accepted");
        assert!(!rejected.exists());
        fs::write(final_root.join("proof-0.json"), &original_proof).unwrap();
    }
    write(&final_root, "set.json", &set);
    for (name, mode) in [
        ("video-00000.mp4", "mutation"),
        ("audio-00000.pcm", "short"),
        ("audio-00000.pcm", "extra"),
    ] {
        let original = fs::read(final_root.join(name)).unwrap();
        let mut changed = original.clone();
        match mode {
            "mutation" => {
                let end = changed.len() - 1;
                changed[end] ^= 1;
            }
            "short" => {
                changed.truncate(changed.len() - 6);
            }
            "extra" => changed.extend_from_slice(&[0; 6]),
            _ => unreachable!(),
        }
        fs::write(final_root.join(name), changed).unwrap();
        job(&final_root);
        let rejected = temp.path().join(format!("rejected-bytes-{mode}"));
        assert!(
            !cli(
                &executable,
                &[
                    "finalize-parts",
                    path(&final_root.join("set.json")),
                    "--asset-root",
                    path(&final_root),
                    "--output-dir",
                    path(&rejected)
                ]
            )
            .status
            .success()
        );
        assert!(!rejected.exists());
        fs::write(final_root.join(name), original).unwrap();
    }
}
