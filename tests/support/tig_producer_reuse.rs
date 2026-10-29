//! Actual source-pinned old CLI and a current checker, generated assets only.
use super::*;
use reel::{
    media_runtime::{Bindings, BoundPrograms, ByteIdentity},
    tig_episode,
    tig_scene_production::jobs::JobPlan,
};
use tig_assets::graph::recipe::produced::Claim;
use tig_core::tree::TreeLimits;
use tig_store::metadata::{MetadataReader, MetadataStore};

pub fn seed_scene_scope(f: &mut Fixture) {
    let mut definition: Value =
        serde_json::from_slice(&fs::read(f.root().join("definitions/scene.json")).unwrap())
            .unwrap();
    definition["authoring"]["properties"]
        .as_array_mut()
        .unwrap()
        .push(json!("episode_id"));
    let pin = write(f.root(), "definitions/scene.json", &definition);
    let mut record: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/scene.tig")).unwrap()).unwrap();
    record["definition"]["sha256"] = json!(pin.sha256);
    record["definition"]["bytes"] = json!(pin.bytes);
    record["authored"]["episode_id"] = json!("episode-1");
    write(f.root(), "records/scene.tig", &record);
    for admission in f.profile["admitted"].as_array_mut().unwrap() {
        if admission["id"] == "types/scene" {
            admission["object"] = json!(pin);
        }
    }
    git(f.root(), &["add", "records", "definitions"]);
    git(
        f.root(),
        &[
            "commit",
            "-m",
            "declare independent episode scope before original production",
        ],
    );
    f.profile["commit"] = json!(git(f.root(), &["rev-parse", "HEAD"]));
}

fn seed_assembly(
    root: &Path,
    request: &reel::tig_scene::Request,
    scene: &Value,
    output: &Value,
    profile: &mut Value,
) -> tig_episode::Request {
    let schema = Object::from_bytes(&fs::read(root.join("definitions/authored.json")).unwrap());
    let mut definitions = std::collections::BTreeMap::new();
    for (name, fields) in [
        ("reuse-episode", vec!["episode_id", "scene_ids"]),
        (
            "reuse-assembly",
            vec!["episode", "language", "conform_slot", "components"],
        ),
    ] {
        let pin = write(
            root,
            &format!("definitions/{name}.json"),
            &json!({
                "schema":"tig.definition.design-1","id":format!("types/{name}"),"authored_schema":schema,
                "authoring":{"properties":fields,"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{},
            }),
        );
        profile["admitted"].as_array_mut().unwrap().push(json!({
            "id":format!("types/{name}"),"object":pin,"payload":{"kind":"selected"},
        }));
        definitions.insert(name, pin);
    }
    let cached = |value: Value| {
        let bytes = serde_json::to_vec(&value).unwrap();
        let pin = Object::from_bytes(&bytes);
        fs::write(root.join("cache/objects").join(pin.sha256.as_str()), bytes).unwrap();
        pin
    };
    let native = cached(
        json!({"schema":"reel.episode-authoring.v1","episode_id":"episode-1",
        "season_id":"season-1","authoring_state":"scene-build-context","master_template_id":"master",
        "scene_policy_id":"policy","presentation":[],"score_palette":[],"scene_ids":[scene["authored"]["scene_id"]]}),
    );
    let dummy = Object::from_bytes(b"not consumed by logical planner");
    let control =
        json!({"path":"unconsumed-control.json","sha256":dummy.sha256,"bytes":dummy.bytes});
    let conform = cached(
        json!({"schema":"reel.episode-conform.v1","episode_id":"episode-1","language":request.language,
        "output_sample_rate":48000,"catalog":control,"master_template_definition":control,"episode":control,
        "season_bindings":control,"episode_bindings":control,"segments":[{"id":scene["authored"]["scene_id"],
        "kind":"scene","master":{"path":"master.mkv","sha256":output["selected"]["sha256"],"bytes":output["selected"]["bytes"]},"source_receipt":control}]}),
    );
    for (name, id, authored, selected, streams) in [
        (
            "reuse-episode",
            "reuse/episode",
            json!({"episode_id":"episode-1","scene_ids":[{"id":request.scene_id}]}),
            json!(native),
            json!([{"slot":"retained","role":"evidence/conform","group":"source","object":conform}]),
        ),
        (
            "reuse-assembly",
            "reuse/assembly",
            json!({"episode":{"id":"reuse/episode"},"language":request.language,
            "conform_slot":"retained","components":{scene["authored"]["scene_id"].as_str().unwrap():{"id":"delivery/native-scene"}}}),
            Value::Null,
            json!([]),
        ),
    ] {
        let pin = &definitions[name];
        write(
            root,
            &format!("records/{name}.tig"),
            &json!({"schema":"tig.instance.design-1","id":id,
            "definition":{"id":format!("types/{name}"),"sha256":pin.sha256,"bytes":pin.bytes},"authored":authored,
            "selected":selected,"streams":streams,"status":{},"metadata":{}}),
        );
    }
    git(root, &["add", "records", "definitions"]);
    git(
        root,
        &[
            "commit",
            "-m",
            "declare logical grouping for retained output proof",
        ],
    );
    profile["commit"] = json!(git(root, &["rev-parse", "HEAD"]));
    serde_json::from_value(json!({"schema":tig_episode::SCHEMA,"assembly_id":"reuse/assembly",
        "assembly_definition":"types/reuse-assembly","episode_definition":"types/reuse-episode",
        "scene_definitions":[request.scene_definition],"component_definitions":["types/native-output"]})).unwrap()
}

pub fn exercise(
    root: &Path,
    request: &reel::tig_scene::Request,
    metadata: (&Path, &tig_store::metadata::Checkpoint),
    profile: &Value,
    completed: &Value,
) {
    let producer: Object =
        serde_json::from_value(completed["planning"]["producer"].clone()).unwrap();
    let claim_pin: Object =
        serde_json::from_value(completed["record_candidates"]["claim"].clone()).unwrap();
    let claim_path = Path::new(
        completed["result"]["outputs"]["production-claim.json"]["path"]
            .as_str()
            .unwrap(),
    );
    let claim_bytes = fs::read(claim_path).unwrap();
    let claim = Claim::parse(&claim_pin, &claim_bytes).unwrap();
    assert_eq!(claim.operation().producer, producer);
    let mut nodes = MetadataReader::open(metadata.0, metadata.1).unwrap();
    let (_, original) = reel::tig_scene::restore_render_claim(
        serde_json::from_value(profile["worker"]["source"].clone()).unwrap(),
        request,
        &producer,
        &claim,
        &root.join("old-producer-replay"),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let output_path = root.join("records/native-output.tig");
    let mut output: Value = serde_json::from_slice(&fs::read(&output_path).unwrap()).unwrap();
    let scene: Value =
        serde_json::from_slice(&fs::read(root.join("records/scene.tig")).unwrap()).unwrap();
    output["authored"]["scene"] = json!({"id":request.scene_id});
    output["authored"]["language"] = json!(request.language);
    output["authored"]["component_kind"] = json!("scene");
    output["authored"]["source_segment_id"] = scene["authored"]["scene_id"].clone();
    fs::write(&output_path, serde_json::to_vec(&output).unwrap()).unwrap();
    git(root, &["add", "records"]);
    git(
        root,
        &[
            "commit",
            "-m",
            "bind retained output to its independently versioned logical scene",
        ],
    );
    let mut current = profile["worker"]["source"].clone();
    current["schema"] = json!(reel::tig_inputs::PROFILE_SCHEMA);
    current["git_executable"] = json!(git_executable());
    current["repository"] = json!(root);
    current["commit"] = json!(git(root, &["rev-parse", "HEAD"]));
    current["records"] = json!(["records"]);
    current["definitions"] = json!(["definitions"]);
    current.as_object_mut().unwrap().remove("archive");
    current.as_object_mut().unwrap().remove("checkpoint");
    let assembly = seed_assembly(root, request, &scene, &output, &mut current);
    let source =
        |profile: &Value| Source::open(serde_json::from_value(profile.clone()).unwrap()).unwrap();
    let mut runtime: Bindings =
        serde_json::from_value(profile["worker"]["runtime"].clone()).unwrap();
    runtime.host = ByteIdentity::observe_file(&std::env::current_exe().unwrap()).unwrap();
    let checker = BoundPrograms::bind(&runtime).unwrap();
    assert_ne!(Object::from_bytes(checker.manifest_bytes()), producer);
    let new_metadata = root.join("current-checker-metadata");
    let mut writer = MetadataStore::create(
        &new_metadata,
        tig_store::metadata::Limits {
            nodes: 32768,
            bytes: 16 * 1024 * 1024,
            database_bytes: 32 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let mut fresh = source(&current);
    let context = reel::tig_scene::freeze_render_context(
        &mut fresh,
        request,
        &root.join("current-source-context"),
        &mut writer,
        TreeLimits::default(),
    )
    .unwrap();
    let current_job = JobPlan::from_render_context(
        &mut fresh,
        request,
        &context,
        &checker,
        &root.join("current-checker-plan"),
    )
    .unwrap();
    assert!(
        tig_episode::check_current_scene(&fresh, "delivery/native-scene", &current_job, &original)
            .unwrap_err()
            .to_string()
            .contains("stale current-source recipe")
    );
    let denied = root.join("wrong-producer-reuse");
    assert!(
        tig_episode::check_current_scene_with_retained_producer(
            &mut fresh,
            "delivery/native-scene",
            &current_job,
            &original,
            &Object::from_bytes(b"unadmitted producer"),
            Some(&denied)
        )
        .is_err()
    );
    assert!(!denied.exists());
    let matched = tig_episode::check_current_scene_with_retained_producer(
        &mut fresh,
        "delivery/native-scene",
        &current_job,
        &original,
        &producer,
        Some(&root.join("retained-producer-current-source")),
    )
    .unwrap();
    assert_eq!(matched.matched_producer(), &producer);
    assert_eq!(matched.recipe(), claim.recipe());
    assert_ne!(matched.recipe(), current_job.recipe().fingerprint());
    assert_eq!(matched.claim(), &claim_pin);
    let resources = current_job
        .freeze(&mut writer, TreeLimits::default())
        .unwrap();
    let witness = writer.finish().unwrap();
    drop(writer);
    let mut reader = MetadataReader::open(&new_metadata, &witness).unwrap();
    let child = current_job
        .readback(resources.selection(), &mut reader, TreeLimits::default())
        .unwrap();
    let identity = reel::tig_scene_production::jobs::Identity {
        source: tig_store::Key::parse("retained-child").unwrap(),
        owner: tig_store::Key::parse("checker").unwrap(),
    };
    let children = std::collections::BTreeMap::from([(
        scene["authored"]["scene_id"].as_str().unwrap().to_string(),
        tig_episode::SceneAdmission {
            readback: child,
            identity,
            expected_revision: 0,
            operation: tig_store::Key::parse("protect").unwrap(),
        },
    )]);
    let proofs = std::collections::BTreeMap::from([(
        scene["authored"]["scene_id"].as_str().unwrap().to_string(),
        matched,
    )]);
    let mut journal = tig_store::source_claims::SourceJournal::create(
        &root.join("reuse-owners"),
        tig_store::Key::parse("fixture").unwrap(),
        tig_store::Key::parse("epoch").unwrap(),
    )
    .unwrap();
    let coordination = root.join("reuse-coordination");
    fs::create_dir(&coordination).unwrap();
    let protected = tig_episode::protect_current_scenes(
        &mut fresh,
        &assembly,
        &proofs,
        children,
        &mut journal,
        &reel::tig_scene_production::jobs::Coordination { root: coordination },
        &tig_episode::ProtectionBudget {
            max_objects: 1024,
            max_logical_bytes: 1024 * 1024 * 1024,
        },
    )
    .unwrap();
    protected.verify_current(&fresh, &mut journal).unwrap();
    assert!(!protected.primary_objects().is_empty());
    drop(protected);
    let mut writer = MetadataStore::open(&new_metadata, &witness).unwrap();
    // Change a consumed primary without changing the selected old scene output.
    // The absent new payload must be detected by metadata/compilation, not render.
    let path = root.join("records/cel-b.tig");
    let mut cel: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    cel["selected"] = json!(Object::from_bytes(
        b"changed primary missing from every cache"
    ));
    fs::write(path, serde_json::to_vec(&cel).unwrap()).unwrap();
    git(root, &["add", "records"]);
    git(
        root,
        &[
            "commit",
            "-m",
            "change consumed cel without rebuilding its selected scene",
        ],
    );
    current["commit"] = json!(git(root, &["rev-parse", "HEAD"]));
    let mut changed = source(&current);
    let changed_context = reel::tig_scene::freeze_render_context(
        &mut changed,
        request,
        &root.join("changed-source-context"),
        &mut writer,
        TreeLimits::default(),
    )
    .unwrap();
    let changed_job = JobPlan::from_render_context(
        &mut changed,
        request,
        &changed_context,
        &checker,
        &root.join("changed-checker-plan"),
    )
    .unwrap();
    assert!(
        tig_episode::check_current_scene_with_retained_producer(
            &mut changed,
            "delivery/native-scene",
            &changed_job,
            &original,
            &producer,
            Some(&root.join("changed-retained-source"))
        )
        .unwrap_err()
        .to_string()
        .contains("stale current-source recipe")
    );
    assert_eq!(fs::read(claim_path).unwrap(), claim_bytes);
    writer.finish().unwrap();
}
