#[cfg(windows)]
use super::*;

#[cfg(windows)]
#[test]
#[ignore = "requires native FFmpeg; verified runtime, in-memory controls and candidate publication"]
fn guarded_delivery_candidate_uses_existing_claim_and_publication_contracts() {
    delivery_candidate(false);
}

#[cfg(windows)]
#[test]
#[ignore = "requires native FFmpeg; exact private-review draft admission through managed worker"]
fn private_review_candidate_uses_managed_capture_render_retention_and_git_handoff() {
    delivery_candidate(true);
}

#[cfg(windows)]
fn delivery_candidate(private_review: bool) {
    use reel::media_runtime::{Bindings, BoundPrograms, ByteIdentity};
    use reel::tig_scene_production::{self, OutputBinding};
    use tig_assets::{
        graph::Reference,
        graph::recipe::produced::{BoundSubject, Selector},
        native::Instance,
    };
    let (mut f, mut request) = fixture();
    #[cfg(feature = "tig-publication")]
    {
        let _ = native_cli::require_propose_tool();
        native_cli::seed(&mut f);
    }
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    if private_review {
        for name in [
            "root",
            "control-job",
            "control-contract",
            "control-production",
            "cel-a",
            "cel-b",
            "cue-main",
        ] {
            record(&f, name, |r| {
                r["status"]["selection_state"] = json!("draft")
            });
        }
        // Exercise the registered-primary case too: payload present, no selection label.
        record(&f, "cue-main", |r| {
            r["status"]
                .as_object_mut()
                .unwrap()
                .remove("selection_state");
        });
        commit(&mut f);
    }
    let file = |path: PathBuf| {
        let path = path.canonicalize().unwrap();
        let pin = ByteIdentity::observe_file(&path).unwrap();
        reel::scene_delivery::FileRef {
            path,
            sha256: pin.sha256,
            bytes: pin.bytes,
        }
    };
    let locate = |name: &str| {
        std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join(format!("{name}.exe")))
            .find(|p| p.is_file())
            .unwrap()
    };
    let host = ByteIdentity::observe_file(&std::env::current_exe().unwrap()).unwrap();
    let programs = BoundPrograms::bind(&Bindings {
        host,
        ffmpeg: file(locate("ffmpeg")),
        ffprobe: file(locate("ffprobe")),
        dependencies: Default::default(),
    })
    .unwrap();
    let mut source = f.source();
    if private_review {
        admit_review(&f, &source, &mut request);
    }
    let mut nodes = MemoryNodes::default();
    let (_, context) =
        adapter::freeze_context(&mut source, &request, &mut nodes, Default::default()).unwrap();
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    let schema = fs::read(f.root().join("definitions/authored.json")).unwrap();
    let definition = serde_json::to_vec(&json!({"schema":"tig.definition.design-1","id":"types/output",
        "authored_schema":Object::from_bytes(&schema),"authoring":{"properties":[],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}})).unwrap();
    let admission = Reference {
        id: "types/output".into(),
        object: Object::from_bytes(&definition),
    };
    let bind = |id: &str, pin: &Object| {
        let record = serde_json::to_vec(&json!({"schema":"tig.instance.design-1","id":id,
            "definition":{"id":admission.id,"sha256":admission.object.sha256,"bytes":admission.object.bytes},
            "authored":{},"selected":pin,"streams":[],"status":{},"metadata":{}})).unwrap();
        let bound = Instance::parse(&record)
            .unwrap()
            .bind(&definition, &schema)
            .unwrap();
        BoundSubject::bind(&bound, &admission, Selector::Selected {})
            .unwrap()
            .unwrap()
    };
    drop(source);
    for name in [
        ".git",
        "records",
        "definitions",
        "job.json",
        "contract.json",
        "production.json",
    ] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let (mut source, context) = adapter::restore_context(
        profile,
        &request,
        context.selection(),
        &mut nodes,
        Default::default(),
    )
    .unwrap();
    let output = f.root().join("rendered-native-output");
    let candidate = tig_scene_production::render_delivery_with_programs(
        &mut source,
        &request,
        &context,
        &programs,
        &output,
    )
    .unwrap();
    assert_eq!(
        &candidate.recipe().operation().producer,
        &Object::from_bytes(programs.manifest_bytes())
    );
    assert!(!f.root().join("job.json").exists());
    assert!(!f.root().join("contract.json").exists());
    let selected = Object::from_bytes(&fs::read(output.join("master.mkv")).unwrap());
    let subject = bind("delivery/recording", &selected);
    let claim = candidate
        .claim(&[OutputBinding {
            output_name: "master.mkv",
            subject: &subject,
        }])
        .unwrap();
    claim.check_plan(candidate.recipe()).unwrap();
    let wrong = bind("delivery/recording", &Object::from_bytes(b"foreign bytes"));
    assert!(
        candidate
            .claim(&[OutputBinding {
                output_name: "master.mkv",
                subject: &wrong
            }])
            .is_err()
    );
    let before = fs::read(output.join("receipt.json")).unwrap();
    assert!(
        tig_scene_production::render_delivery_with_programs(
            &mut source,
            &request,
            &context,
            &programs,
            &output
        )
        .is_err()
    );
    assert_eq!(before, fs::read(output.join("receipt.json")).unwrap());
    #[cfg(feature = "tig-publication")]
    {
        let _ = candidate_publication::exercise(&candidate, &subject, f.root(), bind);
        // The SAME ownership/handoff fixture must work for delivery templates;
        // no new delivery journal, fencing or publication implementation.
        use tig_store::{
            Key,
            metadata::{Limits, MetadataStore},
            source_claims::SourceJournal,
        };
        let metadata = f.root().join("delivery-job-metadata");
        let mut writer = MetadataStore::create(
            &metadata,
            Limits {
                nodes: 8192,
                bytes: 8 * 1024 * 1024,
                database_bytes: 16 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let (_, managed_context) =
            adapter::freeze_context(&mut source, &request, &mut writer, Default::default())
                .unwrap();
        let checkpoint = writer.finish().unwrap();
        drop(writer);
        let journal_root = f.root().join("job-owner");
        let mut owners = SourceJournal::create(
            &journal_root,
            Key::parse("fixture").unwrap(),
            Key::parse("epoch").unwrap(),
        )
        .unwrap();
        let plan = tig_scene_production::jobs::JobPlan::from_delivery_context(
            &mut source,
            &request,
            &managed_context,
            &programs,
        )
        .unwrap();
        let scene_only = tig_scene_production::jobs::reference::ReferencePlan::compile(
            &mut source,
            &plan,
            "delivery/recording",
            "types/output",
            &f.root().join("must-not-create-scene-controls"),
        )
        .err()
        .unwrap();
        assert!(scene_only.to_string().contains("not a speech-scene job"));
        assert!(!f.root().join("must-not-create-scene-controls").exists());
        let claim = native_job::exercise(
            &plan,
            &mut source,
            &mut owners,
            &journal_root,
            f.root(),
            (&metadata, &checkpoint),
            bind,
        );
        claim.check_plan(plan.recipe()).unwrap();
        assert!(
            !f.root().join("managed-native-job").exists(),
            "delivery controls must remain in memory"
        );
        for name in [".git", "records", "definitions"] {
            fs::rename(
                f.root().join(format!("offline-{name}")),
                f.root().join(name),
            )
            .unwrap();
        }
        let capture_cache = f.root().join("cache");
        fs::create_dir_all(capture_cache.join("objects/sha256")).unwrap();
        let coordination = f.root().join("delivery-capture-coordination");
        fs::create_dir(&coordination).unwrap();
        let capture = json!({"schema":"reel.tig-delivery-capture-profile.experimental-1",
            "source":f.profile,"request":request,"metadata_root":metadata,"metadata_minimum":checkpoint,
            "metadata_cache":capture_cache,
            "cache":{"path_policy":"configured-local-roots","cache_roots":[capture_cache],
                "coordination_root":coordination,"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}}});
        let capture_path = f.root().join("delivery-capture.json");
        fs::write(&capture_path, serde_json::to_vec(&capture).unwrap()).unwrap();
        let fragment = f.root().join("delivery-fragment.json");
        let captured = Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
            .arg("capture-context")
            .arg(&capture_path)
            .arg(&fragment)
            .output()
            .unwrap();
        assert!(
            captured.status.success(),
            "{}",
            String::from_utf8_lossy(&captured.stderr)
        );
        let summary: Value = serde_json::from_slice(&captured.stdout).unwrap();
        assert_eq!(summary["context_kind"], "delivery");
        assert_eq!(summary["media_rendered"], false);
        assert_eq!(summary["job_adopted"], false);
        let captured: Value = serde_json::from_slice(&fs::read(fragment).unwrap()).unwrap();
        assert_eq!(captured["context_mode"], "delivery");
        assert_eq!(
            captured["context"],
            serde_json::to_value(managed_context.selection()).unwrap()
        );
        native_cli::exercise_delivery(
            f.root(),
            &request,
            &serde_json::from_value(captured["context"].clone()).unwrap(),
            (
                &metadata,
                &serde_json::from_value(captured["metadata_minimum"].clone()).unwrap(),
            ),
            captured["source"].clone(),
            candidate.receipt(),
        );
    }
}
