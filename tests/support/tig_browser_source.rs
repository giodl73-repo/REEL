use super::*;
use reel::tig_browser::{self as adapter, Bounds, Request};
use std::collections::BTreeSet;

fn edit(f: &Fixture, name: &str, action: impl FnOnce(&mut Value)) {
    let path = format!("records/{name}.tig");
    let mut value = serde_json::from_slice(&fs::read(f.root().join(&path)).unwrap()).unwrap();
    action(&mut value);
    write(f.root(), &path, &value);
}
fn commit(f: &mut Fixture) {
    git(f.root(), &["add", "records", "definitions"]);
    git(f.root(), &["commit", "-m", "synthetic browser revision"]);
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
}
fn fixture() -> (Fixture, Request) {
    let mut f = Fixture::new();
    for name in ["cel-a", "cel-b", "cue-main"] {
        edit(&f, name, |v| {
            v["status"]["selection_state"] = json!("selected-private-production")
        });
    }
    let mut svg: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/cel-a.tig")).unwrap()).unwrap();
    svg["id"] = json!("svg/main");
    svg["selected"] = serde_json::to_value(Object::from_bytes(b"<svg />")).unwrap();
    write(f.root(), "records/svg.tig", &svg);
    let mut definition: Value =
        serde_json::from_slice(&fs::read(f.root().join("definitions/media.json")).unwrap())
            .unwrap();
    definition["id"] = json!("types/browser");
    definition["authoring"]["properties"] = json!(["document"]);
    let pin = write(f.root(), "definitions/browser.json", &definition);
    f.profile["admitted"].as_array_mut().unwrap().push(json!({
        "id":"types/browser","object":pin,"payload":{"kind":"selected"}
    }));
    write(
        f.root(),
        "records/browser.tig",
        &json!({
            "schema":"tig.instance.design-1","id":"browser/source/es",
            "definition":{"id":"types/browser","sha256":pin.sha256,"bytes":pin.bytes},
            "authored":{"document":{"source":"opening.svg","audio":"audio/main.wav",
                "inputs":{"opening.svg":{"id":"svg/main"},"pictures/a.ppm":{"id":"cel/a"},
                    "pictures/b.ppm":{"id":"cel/b"},"audio/main.wav":{"id":"cue/main"}},
                "width":8,"height":8,"fps":24,"frame_count":24}},
            "selected":null,"streams":[],"status":{"selection_state":"selected-private-production"},"metadata":{}
        }),
    );
    commit(&mut f);
    (
        f,
        Request {
            schema: adapter::SCHEMA.into(),
            root_id: "browser/source/es".into(),
            root_definition: "types/browser".into(),
            primary_definitions: BTreeSet::from(["types/media".into()]),
            bounds: Bounds {
                max_inputs: 8,
                max_input_bytes: 1024 * 1024,
                max_frames: 48,
                max_canvas_pixels: 128,
            },
            private_review: None,
        },
    )
}

#[test]
fn native_browser_lowers_exact_primary_ids_without_media_or_output_creation() {
    let (f, request) = fixture();
    let mut source = f.source();
    let prepared = adapter::lower(&mut source, &request).unwrap();
    assert_eq!(prepared.manifest.source.path, "opening.svg");
    assert_eq!(prepared.manifest.assets.len(), 2);
    assert_eq!(prepared.inputs.len(), 4);
    assert_eq!(prepared.inputs["pictures/a.ppm"].path, Path::new("cel/a"));
    assert_eq!(prepared.unique_objects, 4);
    assert_eq!(prepared.alias_bytes, prepared.unique_bytes);
    assert_eq!(prepared.manifest.frame_count, 24);
    assert_eq!(source.byte_requests(), 0);
    assert!(!f.root().join("exports").exists());
}

#[test]
fn changing_one_leaf_changes_lowered_manifest_without_parent_churn() {
    let (mut f, request) = fixture();
    let parent = fs::read(f.root().join("records/browser.tig")).unwrap();
    let before = adapter::lower(&mut f.source(), &request).unwrap();
    edit(&f, "cel-a", |v| {
        v["selected"] = serde_json::to_value(Object::from_bytes(b"replacement picture")).unwrap()
    });
    commit(&mut f);
    let after = adapter::lower(&mut f.source(), &request).unwrap();
    assert_ne!(before.manifest_bytes, after.manifest_bytes);
    assert_eq!(
        before.inputs["pictures/b.ppm"],
        after.inputs["pictures/b.ppm"]
    );
    assert_eq!(before.source_record, after.source_record);
    assert_ne!(before.source_checkpoint, after.source_checkpoint);
    assert_eq!(
        fs::read(f.root().join("records/browser.tig")).unwrap(),
        parent
    );
}

#[test]
fn browser_aliases_preserve_paths_and_separate_unique_from_declared_bytes() {
    let (mut f, mut request) = fixture();
    edit(&f, "browser", |v| {
        v["authored"]["document"]["inputs"]["pictures/a-copy.ppm"] = json!({"id":"cel/a"})
    });
    commit(&mut f);
    let prepared = adapter::lower(&mut f.source(), &request).unwrap();
    assert_eq!(prepared.inputs.len(), 5);
    assert_eq!(prepared.unique_objects, 4);
    assert!(prepared.alias_bytes > prepared.unique_bytes);
    request.bounds.max_input_bytes = prepared.unique_bytes;
    assert!(adapter::lower(&mut f.source(), &request).is_err());
}

#[test]
fn browser_refuses_case_prefix_and_escaping_resource_paths() {
    for path in ["../outside", "/outside", "pictures/A.ppm", "pictures"] {
        let (mut f, request) = fixture();
        edit(&f, "browser", |v| {
            v["authored"]["document"]["inputs"][path] = json!({"id":"cel/a"})
        });
        commit(&mut f);
        let mut source = f.source();
        assert!(adapter::lower(&mut source, &request).is_err(), "{path}");
        assert_eq!(source.byte_requests(), 0);
    }
}

#[test]
fn browser_bounds_and_undeclared_or_pinned_authoring_references_refuse() {
    for (key, value) in [
        ("width", json!(0)),
        ("width", json!(7)),
        ("height", json!(64)),
        ("fps", json!(0)),
        ("frame_count", json!(49)),
        ("source", json!("undeclared.svg")),
    ] {
        let (mut f, request) = fixture();
        edit(&f, "browser", |v| v["authored"]["document"][key] = value);
        commit(&mut f);
        assert!(adapter::lower(&mut f.source(), &request).is_err());
    }
    let (mut f, request) = fixture();
    edit(&f, "browser", |v| {
        v["authored"]["document"]["inputs"]["opening.svg"]["sha256"] =
            json!("a competing parent hash")
    });
    commit(&mut f);
    assert!(adapter::lower(&mut f.source(), &request).is_err());
}

#[test]
fn browser_cannot_consume_an_unadmitted_definition_or_an_unbuilt_primary() {
    let (mut f, request) = fixture();
    edit(&f, "browser", |v| {
        v["authored"]["document"]["inputs"]["opening.svg"] = json!({"id":"alignment/main"})
    });
    commit(&mut f);
    assert!(adapter::lower(&mut f.source(), &request).is_err());
    let (mut f, request) = fixture();
    edit(&f, "svg", |v| v["selected"] = Value::Null);
    commit(&mut f);
    assert!(adapter::lower(&mut f.source(), &request).is_err());
}

#[test]
fn browser_private_review_is_exact_scoped_and_does_not_promote_selection() {
    let (mut f, mut request) = fixture();
    edit(&f, "cel-a", |v| {
        v["status"]["selection_state"] = json!("draft")
    });
    commit(&mut f);
    let raw = fs::read(f.root().join("records/cel-a.tig")).unwrap();
    let mut source = f.source();
    assert!(adapter::lower(&mut source, &request).is_err());
    request.private_review = Some(reel::tig_delivery_template::PrivateReview {
        checkpoint: source.checkpoint().clone(),
        records: BTreeMap::from([("cel/a".into(), Object::from_bytes(&raw))]),
    });
    adapter::lower(&mut source, &request).unwrap();
    assert_eq!(source.byte_requests(), 0);
    assert_eq!(fs::read(f.root().join("records/cel-a.tig")).unwrap(), raw);
    request.private_review.as_mut().unwrap().records.insert(
        "cel/b".into(),
        Object::from_bytes(&fs::read(f.root().join("records/cel-b.tig")).unwrap()),
    );
    assert!(adapter::lower(&mut source, &request).is_err());
}

#[test]
fn browser_recipe_tracks_leaf_bytes_but_not_annotations_or_resource_limits() {
    use tig_core::tree::MemoryNodes;
    let (mut f, mut request) = fixture();
    let producer = Object::from_bytes(b"synthetic admitted browser producer");
    let mut nodes = MemoryNodes::default();
    let mut source = f.source();
    let (_, original) =
        adapter::freeze_context(&mut source, &request, &mut nodes, Default::default()).unwrap();
    let first = adapter::recipe(&mut source, &request, &original, &producer).unwrap();
    assert_eq!(original.requirements().primary().len(), 4);
    edit(&f, "cel-a", |v| {
        v["metadata"]["note"] = json!("annotation only")
    });
    commit(&mut f);
    let mut current = f.source();
    let (_, annotated) =
        adapter::freeze_context(&mut current, &request, &mut nodes, Default::default()).unwrap();
    let annotation_recipe = adapter::recipe(&mut current, &request, &annotated, &producer).unwrap();
    assert_ne!(original.selection(), annotated.selection());
    assert_eq!(first.fingerprint(), annotation_recipe.fingerprint());
    request.bounds.max_frames = 72;
    let mut current = f.source();
    let (_, bounded) =
        adapter::freeze_context(&mut current, &request, &mut nodes, Default::default()).unwrap();
    let bounded_recipe = adapter::recipe(&mut current, &request, &bounded, &producer).unwrap();
    // Resource admission belongs to the retained job evidence, not pixel identity.
    assert_eq!(annotated.descriptor(), bounded.descriptor());
    assert_eq!(first.fingerprint(), bounded_recipe.fingerprint());
    edit(&f, "cel-a", |v| {
        v["selected"] =
            serde_json::to_value(Object::from_bytes(b"changed primary picture")).unwrap()
    });
    commit(&mut f);
    let mut current = f.source();
    let (_, changed) =
        adapter::freeze_context(&mut current, &request, &mut nodes, Default::default()).unwrap();
    let changed_recipe = adapter::recipe(&mut current, &request, &changed, &producer).unwrap();
    assert_ne!(first.fingerprint(), changed_recipe.fingerprint());
    assert_eq!(source.byte_requests() + current.byte_requests(), 0);
}

#[test]
fn frozen_browser_context_reopens_without_original_git_or_loose_controls() {
    use tig_core::tree::MemoryNodes;
    let (f, request) = fixture();
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let (prepared, context) =
        adapter::freeze_context(&mut source, &request, &mut nodes, Default::default()).unwrap();
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let (mut restored, replay) = adapter::restore_context(
        profile,
        &request,
        context.selection(),
        &mut nodes,
        Default::default(),
    )
    .unwrap();
    assert_eq!(context.selection(), replay.selection());
    assert_eq!(
        prepared.manifest_bytes,
        adapter::lower(&mut restored, &request)
            .unwrap()
            .manifest_bytes
    );
    assert_eq!(restored.byte_requests(), 0);
    assert!(!f.root().join("exports").exists());
}

#[test]
fn thousand_browser_aliases_keep_one_primary_authority_and_no_media_reads() {
    let (mut f, mut request) = fixture();
    edit(&f, "browser", |v| {
        for index in 0..1000 {
            v["authored"]["document"]["inputs"][format!("pictures/alias-{index:04}.ppm")] =
                json!({"id":"cel/a"});
        }
    });
    commit(&mut f);
    request.bounds.max_inputs = 1024;
    let mut source = f.source();
    let prepared = adapter::lower(&mut source, &request).unwrap();
    assert_eq!(prepared.inputs.len(), 1004);
    assert_eq!(prepared.unique_objects, 4);
    assert_eq!(
        adapter::requirements(&mut source, &request)
            .unwrap()
            .primary()
            .len(),
        4
    );
    assert_eq!(source.byte_requests(), 0);
}

#[test]
fn browser_named_payload_admission_does_not_substitute_selected_bytes() {
    let (mut f, request) = fixture();
    for name in ["cel-a", "cel-b", "cue-main", "svg"] {
        edit(&f, name, |v| {
            v["streams"] = json!([{"slot":"primary","role":"source/primary","group":"input","object":v["selected"]}]);
            v["selected"] =
                serde_json::to_value(Object::from_bytes(b"unconsumed selected evidence")).unwrap();
        });
    }
    f.profile["admitted"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|v| v["id"] == "types/media")
        .unwrap()["payload"] = json!({"kind":"stream","slot":"primary"});
    commit(&mut f);
    let mut source = f.source();
    let prepared = adapter::lower(&mut source, &request).unwrap();
    assert_eq!(
        prepared.inputs["pictures/a.ppm"].sha256,
        Object::from_bytes(&picture([255, 0, 0])).sha256.as_str()
    );
    assert_eq!(source.byte_requests(), 0);
    f.profile["admitted"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|v| v["id"] == "types/media")
        .unwrap()["payload"] = json!({"kind":"stream","slot":"missing"});
    assert!(adapter::lower(&mut f.source(), &request).is_err());
}

#[test]
fn browser_plan_cli_is_metadata_only_and_redacts_ids_paths_and_errors() {
    let (f, request) = fixture();
    let profile = f.root().join("browser-profile.json");
    let request_path = f.root().join("browser-request.json");
    fs::write(&profile, serde_json::to_vec(&f.profile).unwrap()).unwrap();
    fs::write(&request_path, serde_json::to_vec(&request).unwrap()).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_reel-tig-scene"))
            .arg("--profile")
            .arg(&profile)
            .arg("plan-browser")
            .arg(&request_path)
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["input_aliases"], 4);
    assert_eq!(summary["unique_objects"], 4);
    assert_eq!(summary["primary_media_opened"], 0);
    assert_eq!(summary["production_executed"], false);
    assert_eq!(summary["authorizes_eviction"], false);
    let raw = String::from_utf8(output.stdout).unwrap();
    for secret in ["browser/source/es", "pictures/a.ppm", "cue/main"] {
        assert!(!raw.contains(secret));
    }
    let mut value = serde_json::to_value(request).unwrap();
    value["root_id"] = json!("private-secret-unavailable-record");
    fs::write(&request_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let output = run();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap().trim(),
        "browser source plan refused; verify explicit metadata and admission"
    );
    assert!(!f.root().join("exports").exists());
}

#[cfg(all(windows, feature = "tig-publication"))]
#[test]
fn browser_capture_cli_freezes_to_existing_stores_and_replays_with_source_offline() {
    let (f, request) = fixture();
    let mut profile = super::capture::profile(&f, &request);
    profile["schema"] = json!("reel.tig-browser-capture-profile.experimental-1");
    profile.as_object_mut().unwrap().remove("preparation_cache");
    let output = super::capture::run(&f, &profile, "browser-captured.json");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["context_kind"], "browser");
    assert_eq!(summary["readback_equal"], true);
    assert_eq!(summary["primary"], 4);
    assert_eq!(summary["media_rendered"], false);
    assert_eq!(summary["job_adopted"], false);
    let fragment: Value =
        serde_json::from_slice(&fs::read(f.root().join("browser-captured.json")).unwrap()).unwrap();
    assert_eq!(fragment["context_mode"], "browser");
    for name in [".git", "records", "definitions"] {
        let from = f.root().join(name);
        let to = f.root().join(format!("offline-{name}"));
        assert!(from.is_absolute() && from.starts_with(f.root()) && to.starts_with(f.root()));
        fs::rename(from, to).unwrap();
    }
    let mut reader = tig_store::metadata::MetadataReader::open(
        Path::new(fragment["metadata_root"].as_str().unwrap()),
        &serde_json::from_value(fragment["metadata_minimum"].clone()).unwrap(),
    )
    .unwrap();
    let (mut restored, context) = adapter::restore_context(
        serde_json::from_value(fragment["source"].clone()).unwrap(),
        &request,
        &serde_json::from_value(fragment["context"].clone()).unwrap(),
        &mut reader,
        Default::default(),
    )
    .unwrap();
    assert_eq!(context.requirements().primary().len(), 4);
    assert_eq!(
        adapter::recipe(
            &mut restored,
            &request,
            &context,
            &Object::from_bytes(b"fixture producer")
        )
        .unwrap()
        .operation()
        .parameters,
        Object::from_bytes(context.descriptor()),
    );
    assert_eq!(
        adapter::lower(&mut restored, &request)
            .unwrap()
            .inputs
            .len(),
        4
    );
    assert_eq!(restored.byte_requests(), 0);
    assert!(!f.root().join("native").exists());
}

#[cfg(all(windows, feature = "tig-publication"))]
#[test]
#[ignore = "explicit enrolled browser/FFmpeg; synthetic managed render and retention"]
fn managed_browser_cli_renders_retains_and_proposes_with_source_offline() {
    use reel::media_runtime::ByteIdentity;
    let (mut f, request) = fixture();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    for (name, bytes) in [
        ("svg", br##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><image href="pictures/a.svg" width="8" height="8"/><rect id="r" width="8" height="8" fill="red"/><script>addEventListener('reelframe',e=>document.getElementById('r').setAttribute('fill',e.detail.frame%2?'blue':'red'))</script></svg>"##.as_slice()),
        ("cel-a", br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="green"/></svg>"#.as_slice()),
        ("cel-b", br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="blue"/></svg>"#.as_slice()),
    ] {
        let object = Object::from_bytes(bytes);
        fs::write(f.root().join("cache/objects").join(object.sha256.as_str()), bytes).unwrap();
        edit(&f, name, |v| v["selected"] = serde_json::to_value(&object).unwrap());
    }
    edit(&f, "browser", |v| {
        let inputs = v["authored"]["document"]["inputs"].as_object_mut().unwrap();
        for key in ["a", "b"] {
            let old = inputs.remove(&format!("pictures/{key}.ppm")).unwrap();
            inputs.insert(format!("pictures/{key}.svg"), old);
        }
        v["authored"]["document"]["frame_count"] = json!(6);
    });
    commit(&mut f);
    super::native_cli::seed(&mut f);
    let original = fs::read(f.root().join("records/native-output.tig")).unwrap();
    let base = git(f.root(), &["rev-parse", "HEAD"]);
    let mut capture = super::capture::profile(&f, &request);
    capture["schema"] = json!("reel.tig-browser-capture-profile.experimental-1");
    capture.as_object_mut().unwrap().remove("preparation_cache");
    let captured = super::capture::run(&f, &capture, "managed-context.json");
    assert!(
        captured.status.success(),
        "{}",
        String::from_utf8_lossy(&captured.stderr)
    );
    let fragment: Value =
        serde_json::from_slice(&fs::read(f.root().join("managed-context.json")).unwrap()).unwrap();
    let stores: Value =
        serde_json::from_slice(&fs::read(f.root().join("capture-stores.json")).unwrap()).unwrap();
    let binary = Path::new(env!("CARGO_BIN_EXE_reel-tig-job"))
        .canonicalize()
        .unwrap();
    let host = ByteIdentity::observe_file(&binary).unwrap();
    let pin = |path: PathBuf| {
        let path = path.canonicalize().unwrap();
        let observed = ByteIdentity::observe_file(&path).unwrap();
        json!({"path":path,"sha256":observed.sha256,"bytes":observed.bytes})
    };
    let explicit = |name| {
        PathBuf::from(std::env::var_os(name).expect("explicit fixture tool enrollment required"))
    };
    let git_exe = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("git.exe"))
        .find(|p| p.is_file())
        .unwrap()
        .canonicalize()
        .unwrap();
    let cache = f.root().join("capture-capacity");
    fs::create_dir_all(cache.join("objects/sha256")).unwrap();
    let profile = json!({"schema":"reel.tig-native-job-profile.experimental-1",
        "worker":{"context_mode":"browser","metadata_root":fragment["metadata_root"],"metadata_minimum":fragment["metadata_minimum"],
            "source":fragment["source"],"context":fragment["context"],"request":request,
            "runtime":{"host":host,"ffmpeg":pin(explicit("REEL_TEST_FFMPEG")),"ffprobe":pin(explicit("REEL_TEST_FFPROBE")),"dependencies":{}},
            "browser":{"program":pin(explicit("REEL_TEST_BROWSER")),"clean_picture":false,
                "limits":{"capture":{"max_requests":32,"max_resource_bytes":1048576,"max_served_bytes":4194304,"max_message_bytes":4194304,
                    "max_messages":4096,"max_png_bytes":1048576,"max_frames":6,"max_canvas_pixels":64,"max_elapsed_ms":60000,"max_io_wait_ms":15000},
                    "max_workspace_bytes":134217728,"max_output_bytes":1048576,"browser_memory_bytes":2147483648_u64,"browser_processes":64,
                    "encoder_memory_bytes":268435456,"encoder_processes":4}},
            "outputs":{"master.mkv":1048576,"receipt.json":65536},
            "record_outputs":[{"output":"master.mkv","template":{"id":"delivery/native-scene","object":Object::from_bytes(&original)},"selector":{"kind":"selected"}}],
            "git_handoff":{"git_executable":git_exe,"repository":f.root(),"base":base,
                "identity":{"name":"Fixture","email":"fixture@example.invalid","unix_seconds":1700000000},"message":"managed browser candidate",
                "paths":{"delivery/native-scene":"records/native-output.tig"},"protection":"prevent-mutation"}},
        "cache":capture["cache"],"journal_root":stores["journal_root"],"journal_minimum":stores["journal_minimum"],
        "source_id":"browser-fixture","owner":"fixture-owner","expected_revision":0,"start_operation":"start","retain_operation":"retain",
        "budget_bytes":268435456,"scratch_bytes":134217728,"timeout_seconds":180});
    let profile_path = f.root().join("browser-job.json");
    let mut invalid = profile.clone();
    invalid["budget_bytes"] = json!(1);
    fs::write(&profile_path, serde_json::to_vec(&invalid).unwrap()).unwrap();
    let refused = Command::new(&binary)
        .arg("run")
        .arg(&profile_path)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("workspace exceeds managed admission")
    );
    let journal = tig_store::source_claims::SourceJournal::open(
        Path::new(stores["journal_root"].as_str().unwrap()),
        &serde_json::from_value(stores["journal_minimum"].clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(journal.checkpoint()).unwrap(),
        stores["journal_minimum"]
    );
    drop(journal);
    fs::write(&profile_path, serde_json::to_vec(&profile).unwrap()).unwrap();
    for name in [".git", "records", "definitions"] {
        let from = f.root().join(name);
        let to = f.root().join(format!("offline-{name}"));
        assert!(from.is_absolute() && from.starts_with(f.root()) && to.starts_with(f.root()));
        fs::rename(from, to).unwrap();
    }
    let result = Command::new(&binary)
        .arg("run")
        .arg(&profile_path)
        .output()
        .unwrap();
    if !result.status.success() {
        use std::io::Read;
        let error = String::from_utf8_lossy(&result.stderr);
        if let Some((_, suffix)) = error.split_once("cache://sha256/") {
            let oid = &suffix[..64];
            let file = fs::File::open(reel_assets::object(&cache, oid)).unwrap();
            let mut evidence = String::new();
            flate2::read::GzDecoder::new(file)
                .take(262144)
                .read_to_string(&mut evidence)
                .unwrap();
            let evidence: Value = serde_json::from_str(&evidence).unwrap();
            panic!(
                "{error}; retained synthetic diagnostic: {}",
                evidence["diagnostics"]
            );
        }
    }
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result: Value = serde_json::from_slice(&result.stdout).unwrap();
    let retained = &result["result"]["outputs"];
    let receipt: Value = serde_json::from_slice(
        &fs::read(retained["native/receipt.json"]["path"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["frames"], 6);
    assert_eq!(receipt["samples_per_channel"], 12000);
    assert_eq!(receipt["approved"], false);
    assert_eq!(
        receipt["outputs"]["master.mkv"]["sha256"],
        retained["native/master.mkv"]["sha256"]
    );
    let candidate: Value = serde_json::from_slice(
        &fs::read(
            result["record_candidates"]["records"][0]["retained_path"]
                .as_str()
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        candidate["selected"]["sha256"],
        retained["native/master.mkv"]["sha256"]
    );
    assert_eq!(
        candidate["streams"],
        serde_json::from_slice::<Value>(&original).unwrap()["streams"]
    );
    assert_eq!(candidate["status"], json!({}));
    assert!(result["record_candidates"]["git_proposal"]["object"].is_object());
    // Operational limits and any exact review grants are retained even though
    // they are deliberately excluded from semantic render recipe identity.
    use tig_core::tree::{NodeStore, Tree};
    let mut journal = tig_store::source_claims::SourceJournal::open(
        Path::new(stores["journal_root"].as_str().unwrap()),
        &serde_json::from_value(stores["journal_minimum"].clone()).unwrap(),
    )
    .unwrap();
    let state = journal
        .source(&tig_store::Key::parse("browser-fixture").unwrap())
        .unwrap()
        .unwrap();
    let mut metadata = tig_store::metadata::MetadataReader::open(
        Path::new(fragment["metadata_root"].as_str().unwrap()),
        &serde_json::from_value(fragment["metadata_minimum"].clone()).unwrap(),
    )
    .unwrap();
    let evidence = Tree::from_root(state.predecessors[0].selection.root.clone())
        .get(
            &tig_core::LogicalPath::parse("input-evidence/review/admission").unwrap(),
            &mut metadata,
            Default::default(),
        )
        .unwrap()
        .0
        .unwrap();
    let evidence: Value =
        serde_json::from_slice(&metadata.get(&evidence).unwrap().unwrap()).unwrap();
    assert_eq!(
        evidence["schema"],
        "reel.tig-browser-job-admission.experimental-1"
    );
    assert_eq!(evidence["request"], serde_json::to_value(&request).unwrap());
    assert_eq!(
        fs::read(f.root().join("offline-records/native-output.tig")).unwrap(),
        original
    );
    assert!(!f.root().join(".git").exists());
    let staging = cache.join("staging/caminos");
    assert!(!staging.exists() || fs::read_dir(staging).unwrap().next().is_none());
}
