use super::*;
use reel::{
    media_runtime::{Bindings, BoundPrograms, ByteIdentity},
    presentation_still_sequence as producer,
    tig_episode::ProtectionBudget,
    tig_presentation::jobs::JobPlan,
    tig_scene_production::jobs::{Coordination, Identity},
};
use tig_core::tree::TreeLimits;
use tig_git::source::native::job::Role;
use tig_store::{
    Key,
    metadata::{Limits, MetadataReader, MetadataStore},
    source_claims::{Command, SourceJournal},
};

fn programs() -> BoundPrograms {
    let pin = |path: PathBuf| {
        let path = path.canonicalize().unwrap();
        let pin = ByteIdentity::observe_file(&path).unwrap();
        FileRef {
            path,
            sha256: pin.sha256,
            bytes: pin.bytes,
        }
    };
    let host = ByteIdentity::observe_file(&std::env::current_exe().unwrap()).unwrap();
    let external = |name: &str| {
        pin(std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|root| root.join(format!("{name}.exe")))
            .find(|path| path.is_file())
            .unwrap())
    };
    BoundPrograms::bind(&Bindings {
        host,
        ffmpeg: external("ffmpeg"),
        ffprobe: external("ffprobe"),
        dependencies: Default::default(),
    })
    .unwrap()
}
fn key(value: &str) -> Key {
    Key::parse(value).unwrap()
}

// Metadata planning fixture. The genuine native master/receipt come from the
// producer above; this does not claim to execute the legacy adoption route.
fn add_parent(f: &mut Fixture, master: &Object, receipt: &Object) -> reel::tig_episode::Request {
    let schema = Object::from_bytes(&fs::read(f.root().join("definitions/authored.json")).unwrap());
    let mut definitions = BTreeMap::new();
    for (kind, fields) in [
        (
            "assembly",
            vec!["episode", "language", "conform_slot", "components"],
        ),
        ("episode", vec!["episode_id", "scene_ids"]),
        ("scene", vec!["episode_id", "scene_id"]),
    ] {
        let id = format!("types/parent-{kind}");
        let object = write(
            f.root(),
            &format!("definitions/parent-{kind}.json"),
            &json!({"schema":"tig.definition.design-1","id":id,"authored_schema":schema,"authoring":{"properties":fields,"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
        );
        f.profile["admitted"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":id,"object":object,"payload":{"kind":"selected"}}));
        definitions.insert(kind, object);
    }
    let cached = |value: &Value| {
        let raw = serde_json::to_vec(value).unwrap();
        let object = Object::from_bytes(&raw);
        fs::write(
            f.root().join("cache/objects").join(object.sha256.as_str()),
            raw,
        )
        .unwrap();
        object
    };
    let episode = cached(
        &json!({"schema":"reel.episode-authoring.v1","episode_id":"fixture","season_id":"season","authoring_state":"draft","master_template_id":"master","scene_policy_id":"policy","presentation":[{"role":"opening","template_id":"opening","content":{}}],"score_palette":[],"scene_ids":[]}),
    );
    let opaque = cached(&json!({}));
    let conform = cached(
        &json!({"schema":"reel.episode-conform.v1","episode_id":"fixture","language":"es","output_sample_rate":48000,"catalog":reference("catalog.json",&opaque),"master_template_definition":reference("master.json",&opaque),"episode":reference("episode.json",&episode),"season_bindings":reference("season.json",&opaque),"episode_bindings":reference("bindings.json",&opaque),"segments":[{"kind":"episode-presentation","id":"opening","master":reference("master.mkv",master),"source_receipt":reference("receipt.json",receipt)}]}),
    );
    for (kind, authored, selected, streams) in [
        (
            "episode",
            json!({"episode_id":"fixture","scene_ids":[]}),
            json!(episode),
            json!([{"slot":"conform","role":"control","group":"native","object":conform}]),
        ),
        (
            "assembly",
            json!({"episode":{"id":"parent/episode"},"language":"es","conform_slot":"conform","components":{"opening":{"id":"output/main"}}}),
            Value::Null,
            json!([]),
        ),
    ] {
        write(
            f.root(),
            &format!("records/parent-{kind}.tig"),
            &json!({"schema":"tig.instance.design-1","id":format!("parent/{kind}"),"definition":{"id":format!("types/parent-{kind}"),"sha256":definitions[kind].sha256,"bytes":definitions[kind].bytes},"authored":authored,"selected":selected,"streams":streams,"status":{},"metadata":{}}),
        );
    }
    let mut stale: Value = serde_json::from_slice(
        &fs::read(f.root().join("cache/objects").join(conform.sha256.as_str())).unwrap(),
    )
    .unwrap();
    stale["segments"][0]["master"] = reference(
        "master.mkv",
        &Object::from_bytes(b"stale selected baseline"),
    );
    let stale = cached(&stale);
    update(f, "parent-episode", |record| {
        record["streams"]
            .as_array_mut()
            .unwrap()
            .push(json!({"slot":"stale-conform","role":"control","group":"native","object":stale}))
    });
    let mut stale_record: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/parent-assembly.tig")).unwrap())
            .unwrap();
    stale_record["id"] = json!("parent/stale");
    stale_record["authored"]["conform_slot"] = json!("stale-conform");
    write(f.root(), "records/parent-stale.tig", &stale_record);
    serde_json::from_value(json!({"schema":reel::tig_episode::SCHEMA,"assembly_id":"parent/assembly","assembly_definition":"types/parent-assembly","episode_definition":"types/parent-episode","scene_definitions":["types/parent-scene"],"component_definitions":["types/presentation-output"]})).unwrap()
}

#[test]
fn managed_presentation_retains_exact_resources_allows_peers_and_refuses_stale_owners() {
    let (mut f, request) = setup();
    f.profile["byte_source"]["required"] = json!("prevent-mutation");
    let path = "definitions/presentation-output.json";
    let mut definition: Value =
        serde_json::from_slice(&fs::read(f.root().join(path)).unwrap()).unwrap();
    definition["authoring"]["properties"] = json!([
        "document",
        "presentation",
        "episode_id",
        "language",
        "source_segment_id",
        "component_kind",
        "episode"
    ]);
    let output_definition = write(f.root(), path, &definition);
    for admission in f.profile["admitted"].as_array_mut().unwrap() {
        if admission["id"] == "types/presentation-output" {
            admission["object"] = json!(output_definition);
        }
    }
    commit(&mut f);
    let programs = programs();
    let mut source = f.source();
    let compiled = tig_presentation::compile(&source, &request).unwrap();
    let inputs = compiled.prepare(&mut source).unwrap();
    let output = f.root().join("original-output");
    producer::build_with_control_files(
        compiled.manifest(),
        &compiled,
        &programs.with_inputs(&inputs),
        &output,
    )
    .unwrap();
    let selected = Object::from_bytes(&fs::read(output.join("master.mkv")).unwrap());
    let receipt = Object::from_bytes(&fs::read(output.join("receipt.json")).unwrap());
    fs::copy(
        output.join("receipt.json"),
        f.root().join("cache/objects").join(receipt.sha256.as_str()),
    )
    .unwrap();
    fs::copy(
        output.join("master.mkv"),
        f.root()
            .join("cache/objects")
            .join(selected.sha256.as_str()),
    )
    .unwrap();
    drop(inputs);
    drop(source);
    for (id, language) in [("output/main", "es"), ("output/wrong-language", "en")] {
        write(
            f.root(),
            &format!("records/{}.tig", id.replace('/', "-")),
            &json!({"schema":"tig.instance.design-1","id":id,"definition":{"id":"types/presentation-output","sha256":output_definition.sha256,"bytes":output_definition.bytes},"authored":{"document":{},"presentation":{"id":"presentation/source"},"episode":{"id":"parent/episode"},"episode_id":"fixture","language":language,"source_segment_id":"opening","component_kind":"episode-presentation"},"selected":selected,"streams":[],"status":{},"metadata":{}}),
        );
    }
    let parent_request = add_parent(&mut f, &selected, &receipt);
    commit(&mut f);
    let mut source = f.source();
    let metadata = f.root().join("managed-metadata");
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
    let context =
        tig_presentation::freeze_context(&mut source, &request, &mut writer, TreeLimits::default())
            .unwrap();
    assert!(
        JobPlan::from_context(
            &mut source,
            &request,
            &context,
            &programs,
            "output/wrong-language",
            "types/presentation-output"
        )
        .is_err()
    );
    assert_eq!(source.byte_requests(), 0);
    let plan = JobPlan::from_context(
        &mut source,
        &request,
        &context,
        &programs,
        "output/main",
        "types/presentation-output",
    )
    .unwrap();
    let frozen = plan.freeze(&mut writer, TreeLimits::default()).unwrap();
    assert_eq!(frozen.of(Role::Primary).len(), 4);
    assert_eq!(frozen.of(Role::Composition).len(), 1);
    assert!(frozen.of(Role::ClaimedOutput).is_empty());
    assert_eq!(frozen.of(Role::Evidence).len(), 6);
    assert!(!frozen.of(Role::Runtime).is_empty());
    assert_eq!(source.byte_requests(), 0);
    let parent_context = reel::tig_episode::freeze_composition_context(
        &mut source,
        &parent_request,
        &mut writer,
        TreeLimits::default(),
    )
    .unwrap();
    let mut stale_request = serde_json::to_value(&parent_request).unwrap();
    stale_request["assembly_id"] = json!("parent/stale");
    let stale_request = serde_json::from_value(stale_request).unwrap();
    let stale_context = reel::tig_episode::freeze_composition_context(
        &mut source,
        &stale_request,
        &mut writer,
        TreeLimits::default(),
    )
    .unwrap();
    let witness = writer.finish().unwrap();
    drop(writer);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut reader = MetadataReader::open(&metadata, &witness).unwrap();
    let mut owners = SourceJournal::create(
        &f.root().join("owners"),
        key("presentation-fixture"),
        key("epoch"),
    )
    .unwrap();
    fs::create_dir(f.root().join("coordination")).unwrap();
    let coordination = Coordination {
        root: f.root().join("coordination"),
    };
    let identity = Identity {
        source: key("opening-main"),
        owner: key("worker-one"),
    };
    let budget = ProtectionBudget {
        max_objects: 8192,
        max_logical_bytes: 2 * 1024 * 1024 * 1024,
    };
    let before = owners.checkpoint();
    assert!(
        plan.readback(frozen.selection(), &mut reader, TreeLimits::default())
            .unwrap()
            .protect(
                &source,
                &mut owners,
                &coordination,
                identity.clone(),
                0,
                &key("too-small"),
                &ProtectionBudget {
                    max_objects: 1,
                    max_logical_bytes: 1
                }
            )
            .is_err()
    );
    assert_eq!(before, owners.checkpoint());
    let protected = plan
        .readback(frozen.selection(), &mut reader, TreeLimits::default())
        .unwrap()
        .protect(
            &source,
            &mut owners,
            &coordination,
            identity.clone(),
            0,
            &key("start"),
            &budget,
        )
        .unwrap();
    let peer = plan
        .readback(frozen.selection(), &mut reader, TreeLimits::default())
        .unwrap()
        .protect(
            &source,
            &mut owners,
            &coordination,
            Identity {
                source: key("opening-peer"),
                owner: key("worker-two"),
            },
            0,
            &key("peer-start"),
            &budget,
        )
        .unwrap();
    peer.verify_current(&source, &mut owners).unwrap();
    assert_eq!(protected.revision(), 2);
    // Original receipts are historical evidence, not prerequisites for a new comparison.
    fs::remove_file(output.join("receipt.json")).unwrap();
    let verified = protected
        .verify_selected_master(&mut source, &mut owners, f.root(), 1024 * 1024)
        .unwrap();
    assert_eq!(verified.selected().sha256, selected.sha256.as_str());
    assert_eq!(verified.recipe(), plan.recipe().fingerprint());
    verified.verify_current(&source, &mut owners).unwrap();
    assert_eq!(source.active_readers(), 5);
    let verified = BTreeMap::from([("opening".to_owned(), verified)]);
    let no_scenes = BTreeMap::new();
    let stale = reel::tig_episode::CompositionJobPlan::from_current_components(
        &mut source,
        &stale_request,
        &stale_context,
        &programs,
        &no_scenes,
        &verified,
    )
    .err()
    .unwrap();
    assert!(stale.to_string().contains("recompile parent controls"));
    assert!(
        reel::tig_episode::CompositionJobPlan::from_current_scenes(
            &mut source,
            &parent_request,
            &parent_context,
            &programs,
            &no_scenes
        )
        .is_err()
    );
    assert!(
        reel::tig_episode::CompositionJobPlan::from_current_components(
            &mut source,
            &parent_request,
            &parent_context,
            &programs,
            &no_scenes,
            &BTreeMap::new()
        )
        .is_err()
    );
    let parent_plan = reel::tig_episode::CompositionJobPlan::from_current_components(
        &mut source,
        &parent_request,
        &parent_context,
        &programs,
        &no_scenes,
        &verified,
    )
    .unwrap();
    let mut parent_writer = MetadataStore::open(&metadata, &witness).unwrap();
    let parent_frozen = parent_plan
        .freeze(&mut parent_writer, TreeLimits::default())
        .unwrap();
    let parent_witness = parent_writer.finish().unwrap();
    drop(parent_writer);
    assert!(parent_frozen.of(Role::Primary).is_empty());
    assert!(frozen.objects().is_subset(parent_frozen.objects()));
    let empty_children = reel::tig_episode::protect_current_scenes(
        &mut source,
        &parent_request,
        &no_scenes,
        BTreeMap::new(),
        &mut owners,
        &coordination,
        &budget,
    )
    .unwrap();
    let mut parent_reader = MetadataReader::open(&metadata, &parent_witness).unwrap();
    let ownership = reel::tig_episode::CompositionOwnership {
        coordination: &coordination,
        identity: Identity {
            source: key("parent-assembly"),
            owner: key("parent-worker"),
        },
        expected_revision: 0,
        budget: &budget,
    };
    let parent = parent_plan
        .readback(
            parent_frozen.selection(),
            &mut parent_reader,
            TreeLimits::default(),
        )
        .unwrap()
        .protect(
            &source,
            &empty_children,
            &mut owners,
            &ownership,
            &key("parent-start"),
        )
        .unwrap();
    parent
        .verify_current(&source, &empty_children, &mut owners)
        .unwrap();
    owners
        .apply(
            key("external-withdraw"),
            Command::Withdraw {
                source: identity.source.clone(),
                owner: identity.owner.clone(),
                expected_revision: 2,
            },
        )
        .unwrap();
    assert!(
        verified["opening"]
            .verify_current(&source, &mut owners)
            .is_err()
    );
    assert!(
        parent
            .verify_current(&source, &empty_children, &mut owners)
            .is_err()
    );
    assert!(
        parent
            .with_current(
                &source,
                &empty_children,
                &mut owners,
                |_| -> anyhow::Result<()> {
                    panic!("stale presentation must refuse before action")
                }
            )
            .is_err()
    );
    peer.verify_current(&source, &mut owners).unwrap();
    drop(parent);
    drop(parent_plan);
    drop(verified);
    assert_eq!(source.active_readers(), 0);
    assert!(
        protected
            .verify_selected_master(&mut source, &mut owners, f.root(), 1024 * 1024)
            .is_err()
    );
    drop(protected);
    assert!(
        plan.readback(frozen.selection(), &mut reader, TreeLimits::default())
            .unwrap()
            .resume(&source, &mut owners, &coordination, identity, 2, &budget)
            .is_err()
    );
    assert!(
        owners
            .source(&key("opening-peer"))
            .unwrap()
            .unwrap()
            .current
            .is_some()
    );
}
