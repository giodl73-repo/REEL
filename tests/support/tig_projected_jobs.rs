use super::*;
use reel::{
    media_runtime::{Bindings, BoundPrograms, ByteIdentity},
    tig_scene_production::{self, jobs::JobPlan},
};
use tig_core::tree::{MemoryNodes, TreeLimits};
use tig_git::source::native::job::Role;

fn programs(real: bool) -> BoundPrograms {
    let pin = |path: PathBuf| {
        let path = path.canonicalize().unwrap();
        let identity = ByteIdentity::observe_file(&path).unwrap();
        reel::scene_delivery::FileRef {
            path,
            sha256: identity.sha256,
            bytes: identity.bytes,
        }
    };
    let host = pin(std::env::current_exe().unwrap());
    let program = |name: &str| {
        if real {
            let path = std::env::split_paths(&std::env::var_os("PATH").unwrap())
                .map(|dir| dir.join(format!("{name}.exe")))
                .find(|path| path.is_file())
                .expect("explicit external test requires FFmpeg tools on PATH");
            pin(path)
        } else {
            host.clone()
        }
    };
    BoundPrograms::bind(&Bindings {
        host: ByteIdentity {
            sha256: host.sha256.clone(),
            bytes: host.bytes,
        },
        ffmpeg: program("ffmpeg"),
        ffprobe: program("ffprobe"),
        dependencies: Default::default(),
    })
    .unwrap()
}

#[test]
#[ignore = "requires Windows FFmpeg and tig-propose; compiled CLI workflow"]
fn compiled_cli_plans_renders_and_hands_off_records_with_source_git_offline() {
    compiled_rehearsal(None);
}
#[test]
#[ignore = "requires source-pinned older producer, FFmpeg and tig-propose"]
fn retained_producer_and_new_checker_preserve_claim_and_source_currency() {
    let producer = std::env::var_os("REEL_TEST_TIG_ORIGINAL_PRODUCER")
        .map(PathBuf::from)
        .expect("explicit older qualified producer required");
    assert_ne!(
        reel::media_runtime::ByteIdentity::observe_file(&producer).unwrap(),
        reel::media_runtime::ByteIdentity::observe_file(Path::new(env!(
            "CARGO_BIN_EXE_reel-tig-job"
        )))
        .unwrap()
    );
    compiled_rehearsal(Some(&producer));
}
fn compiled_rehearsal(producer: Option<&Path>) {
    use tig_store::metadata::MetadataReader;
    let (mut f, request) = native_scene_fixture();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    native_cli::seed(&mut f);
    if producer.is_some() {
        crate::producer_reuse::seed_scene_scope(&mut f);
    }
    let programs = programs(true);
    let capture = crate::capture::profile(&f, &request);
    let result = crate::capture::run(&f, &capture, "compiled-cli-fragment.json");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let fragment: Value =
        serde_json::from_slice(&fs::read(f.root().join("compiled-cli-fragment.json")).unwrap())
            .unwrap();
    let metadata = crate::capture::metadata_root(f.root());
    let witness = serde_json::from_value(fragment["metadata_minimum"].clone()).unwrap();
    let mut nodes = MetadataReader::open(&metadata, &witness).unwrap();
    let profile = fragment["source"].clone();
    let (mut source, context) = reel::tig_scene::restore_render_context(
        serde_json::from_value(profile.clone()).unwrap(),
        &request,
        &serde_json::from_value(fragment["context"].clone()).unwrap(),
        &f.root().join("compiled-cli-context"),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    drop(nodes);
    let reference = tig_scene_production::render_projected_with_programs(
        &mut source,
        &request,
        &context,
        &programs,
        &f.root().join("compiled-cli-reference-job"),
        &f.root().join("compiled-cli-reference-output"),
    )
    .unwrap();
    if let Some(producer) = producer {
        native_cli::exercise_retained_producer(
            f.root(),
            &request,
            context.selection(),
            (&metadata, &witness),
            profile,
            reference.receipt(),
            producer,
        );
    } else {
        native_cli::exercise_compiled(
            f.root(),
            &request,
            context.selection(),
            (&metadata, &witness),
            profile,
            reference.receipt(),
        );
    }
    // The helper renders/plans with Git hidden, then restores it for the
    // explicit proposal/apply assertions at the end of the lifecycle.
    assert!(f.root().join(".git").is_dir());
    assert!(!f.root().join("cli-active").read_dir().unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("native-prepare-")
    }));
}

#[test]
fn projected_job_freeze_retains_controls_runtime_and_primary_inputs_without_execution() {
    let (f, request) = bilingual();
    // The test executable is a held runtime stand-in; it is never invoked as media software.
    let programs = programs(false);
    let mut source = f.source();
    let mut nodes = MemoryNodes::default();
    let context = reel::tig_scene::freeze_render_context(
        &mut source,
        &request,
        &f.root().join("context"),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let plan = JobPlan::from_render_context(
        &mut source,
        &request,
        &context,
        &programs,
        &f.root().join("preparation"),
    )
    .unwrap();
    assert_eq!(source.byte_requests(), 12);
    let frozen = plan.freeze(&mut nodes, TreeLimits::default()).unwrap();
    assert_eq!(frozen.of(Role::Primary), context.requirements().primary());
    assert_eq!(frozen.of(Role::Control), context.requirements().controls());
    assert!(
        frozen
            .of(Role::Runtime)
            .contains(&plan.recipe().operation().producer)
    );
    for object in plan.runtime().values() {
        assert!(frozen.of(Role::Runtime).contains(object));
    }
    assert_eq!(
        plan.recipe().operation().parameters,
        Object::from_bytes(context.descriptor())
    );
    let mut readonly = ReadOnlyMetadata {
        inner: &nodes,
        reads: 0.into(),
    };
    let ready = plan
        .readback(frozen.selection(), &mut readonly, TreeLimits::default())
        .unwrap();
    assert!(!ready.resources().complete_claim_coverage());
    let reads = source.byte_requests();
    assert!(JobPlan::from_context(&mut source, &request, &context, &programs).is_err());
    assert_eq!(source.byte_requests(), reads);
    let existing = f.root().join("already-present");
    fs::create_dir(&existing).unwrap();
    let output = f.root().join("no-render");
    assert!(
        tig_scene_production::render_projected_with_programs(
            &mut source,
            &request,
            &context,
            &programs,
            &existing,
            &output
        )
        .is_err()
    );
    assert_eq!(source.byte_requests(), reads);
    assert!(!output.exists());
}

#[test]
#[ignore = "requires Windows FFmpeg; projected managed render/publication/claim replay"]
fn projected_managed_render_publishes_and_replays_checked_claim() {
    use tig_assets::{
        graph::{
            Reference,
            recipe::produced::{BoundSubject, Selector},
        },
        native::Instance,
    };
    use tig_store::{
        Key,
        metadata::{Limits, MetadataReader, MetadataStore},
        source_claims::SourceJournal,
    };
    let (mut f, request) = bilingual();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    let programs = programs(true);
    let producer = Object::from_bytes(programs.manifest_bytes());
    let root = f.root().join("projected-managed");
    fs::create_dir(&root).unwrap();
    let metadata = root.join("metadata");
    let mut nodes = MetadataStore::create(
        &metadata,
        Limits {
            nodes: 16384,
            bytes: 16 * 1024 * 1024,
            database_bytes: 32 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let mut source = f.source();
    let context = reel::tig_scene::freeze_render_context(
        &mut source,
        &request,
        &root.join("prepared-context"),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let profile = archived_profile(&f, context.archive().clone(), source.checkpoint().clone());
    let plan = JobPlan::from_render_context(
        &mut source,
        &request,
        &context,
        &programs,
        &root.join("prepared-plan"),
    )
    .unwrap();
    let witness = nodes.finish().unwrap();
    drop(nodes);
    let key = |s: &str| Key::parse(s).unwrap();
    let mut owners =
        SourceJournal::create(&root.join("owners"), key("projected-fixture"), key("epoch"))
            .unwrap();
    let schema = fs::read(f.root().join("definitions/authored.json")).unwrap();
    let definition=serde_json::to_vec(&json!({"schema":"tig.definition.design-1","id":"types/output","authored_schema":Object::from_bytes(&schema),"authoring":{"properties":[],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}})).unwrap();
    let admission = Reference {
        id: "types/output".into(),
        object: Object::from_bytes(&definition),
    };
    let bind_as = |id: &str, pin: &Object| {
        let record=serde_json::to_vec(&json!({"schema":"tig.instance.design-1","id":id,"definition":{"id":admission.id,"sha256":admission.object.sha256,"bytes":admission.object.bytes},"authored":{},"selected":pin,"streams":[],"status":{},"metadata":{}})).unwrap();
        let bound = Instance::parse(&record)
            .unwrap()
            .bind(&definition, &schema)
            .unwrap();
        BoundSubject::bind(&bound, &admission, Selector::Selected {})
            .unwrap()
            .unwrap()
    };
    let claim = native_job::exercise(
        &plan,
        &mut source,
        &mut owners,
        &root.join("owners"),
        &root,
        (&metadata, &witness),
        bind_as,
    );
    assert_eq!(claim.recipe(), plan.recipe().fingerprint());
    assert_eq!(source.active_readers(), 0);
    drop(owners);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut reader = MetadataReader::open(&metadata, &witness).unwrap();
    let (_, replayed) = reel::tig_scene::restore_render_claim(
        profile,
        &request,
        &producer,
        &claim,
        &root.join("claim-replay"),
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(replayed.checked().claim().object(), claim.object());
    assert!(!replayed.context().requirements().controls().is_empty());
}
