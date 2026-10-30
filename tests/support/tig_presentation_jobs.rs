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
        "component_kind"
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
            &json!({"schema":"tig.instance.design-1","id":id,"definition":{"id":"types/presentation-output","sha256":output_definition.sha256,"bytes":output_definition.bytes},"authored":{"document":{},"presentation":{"id":"presentation/source"},"episode_id":"fixture","language":language,"source_segment_id":"opening","component_kind":"episode-presentation"},"selected":selected,"streams":[],"status":{},"metadata":{}}),
        );
    }
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
    assert!(verified.verify_current(&source, &mut owners).is_err());
    peer.verify_current(&source, &mut owners).unwrap();
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
