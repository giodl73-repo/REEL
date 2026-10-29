use super::*;
use tig_core::tree::{MemoryNodes, TreeLimits};
use tig_git::source::native::{context::ProductionContext, job};
use tig_store::metadata::{MetadataReader, MetadataStore};

fn fixture() -> Fixture {
    let mut f = Fixture::new();
    f.profile["byte_source"]["limits"]["requests"] = json!(16);
    f.profile["byte_source"]["limits"]["total_bytes"] = json!(1024 * 1024);
    f
}
fn freeze(f: &Fixture) -> (ProductionContext, MemoryNodes) {
    let mut nodes = MemoryNodes::default();
    let context = tig_episode::freeze_composition_context(
        &mut f.source(),
        &f.request(),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    (context, nodes)
}
fn recipe(f: &Fixture, context: &ProductionContext) -> tig_assets::graph::recipe::Plan {
    tig_episode::composition_recipe(
        &mut f.source(),
        &f.request(),
        context,
        &Object::from_bytes(b"admitted runtime"),
    )
    .unwrap()
}

#[test]
fn assembly_context_excludes_compiled_outputs_from_primary_and_binds_selected_components() {
    let mut f = fixture();
    let assembly = fs::read(f.root.path().join("records/assembly.tig")).unwrap();
    let (old, _) = freeze(&f);
    let old_recipe = recipe(&f, &old);
    assert!(old.requirements().primary().is_empty());
    let plan = tig_episode::plan(&mut f.source(), &f.request()).unwrap();
    for component in &plan.components {
        assert!(!old.requirements().controls().contains(&component.selected));
        assert!(
            !old.requirements()
                .all_objects()
                .contains(&component.selected)
        );
    }
    assert!(old.requirements().controls().contains(&plan.conform));
    assert!(
        old.requirements()
            .controls()
            .contains(&plan.episode_selected)
    );
    let runtime = Default::default();
    let mut nodes = MemoryNodes::default();
    let composition = plan
        .components
        .iter()
        .map(|component| {
            (
                tig_core::LogicalPath::parse(&component.segment_id).unwrap(),
                component.selected.clone(),
            )
        })
        .collect();
    let evidence = std::collections::BTreeMap::from([(
        tig_core::LogicalPath::parse("prior-source-claim").unwrap(),
        Object::from_bytes(b"externally admitted input evidence, body absent"),
    )]);
    let admission = job::Admission {
        context: &old,
        plan: &old_recipe,
        runtime: &runtime,
        output: None,
    };
    let dependencies = job::Dependencies {
        composition: &composition,
        evidence: &evidence,
    };
    let resources = job::freeze_with_dependencies(
        &admission,
        &dependencies,
        None,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    assert!(resources.of(job::Role::Primary).is_empty());
    assert!(resources.of(job::Role::ClaimedOutput).is_empty());
    for component in &plan.components {
        assert!(
            resources
                .of(job::Role::Composition)
                .contains(&component.selected)
        );
        assert!(
            !resources
                .of(job::Role::Primary)
                .contains(&component.selected)
        );
        assert!(
            !resources
                .of(job::Role::Runtime)
                .contains(&component.selected)
        );
    }
    let replay = job::restore_with_dependencies(
        resources.selection(),
        &admission,
        &dependencies,
        None,
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(resources.selection(), replay.selection());
    assert!(
        job::restore(
            resources.selection(),
            &admission,
            &mut nodes,
            TreeLimits::default(),
        )
        .is_err(),
        "composition/evidence cannot be silently omitted during replay"
    );
    assert!(!resources.complete_claim_coverage());

    // Neither movie exists. Metadata must still detect a leaf selection change.
    f.edit("master", |v| {
        v["selected"] = json!(Object::from_bytes(b"replacement movie absent"))
    });
    assert_eq!(
        fs::read(f.root.path().join("records/assembly.tig")).unwrap(),
        assembly
    );
    assert!(
        tig_episode::composition_recipe(
            &mut f.source(),
            &f.request(),
            &old,
            &Object::from_bytes(b"admitted runtime"),
        )
        .is_err()
    );
    let (new, _) = freeze(&f);
    assert_ne!(recipe(&f, &new).fingerprint(), old_recipe.fingerprint());
    assert_ne!(new.selection(), old.selection());
}

#[test]
fn unrelated_commits_and_status_preserve_recipe_but_source_and_producer_remain_exact() {
    let mut f = fixture();
    let original = f.profile.clone();
    let (old, _) = freeze(&f);
    let expected = recipe(&f, &old);
    f.edit("master", |v| {
        v["status"]["production_state"] = json!("another-label")
    });
    f.edit("scene", |v| {
        v["metadata"]["note"] = json!("non-consumed note")
    });
    let (new, _) = freeze(&f);
    let actual = recipe(&f, &new);
    assert_eq!(actual.fingerprint(), expected.fingerprint());
    assert_ne!(actual.checkpoint(), expected.checkpoint());
    assert_ne!(new.selection(), old.selection());
    let different_producer = tig_episode::composition_recipe(
        &mut f.source(),
        &f.request(),
        &new,
        &Object::from_bytes(b"another admitted runtime"),
    )
    .unwrap();
    assert_ne!(different_producer.fingerprint(), actual.fingerprint());
    let mut wrong_request = f.request();
    wrong_request.assembly_id = "other".into();
    assert!(
        tig_episode::composition_recipe(
            &mut f.source(),
            &wrong_request,
            &new,
            &Object::from_bytes(b"admitted runtime"),
        )
        .is_err()
    );
    f.profile = original;
    assert_eq!(recipe(&f, &old).fingerprint(), expected.fingerprint());
}

#[test]
fn component_definition_and_native_source_changes_invalidate_without_touching_parent() {
    let mut f = fixture();
    let parent = fs::read(f.root.path().join("records/assembly.tig")).unwrap();
    let (old, _) = freeze(&f);
    let expected = recipe(&f, &old);
    let mut definition: Value =
        serde_json::from_slice(&fs::read(f.root.path().join("definitions/output.json")).unwrap())
            .unwrap();
    definition["authoring"]["properties"]
        .as_array_mut()
        .unwrap()
        .push(json!("credit"));
    let pin = write(f.root.path(), "definitions/output.json", &definition);
    f.profile["admitted"][3]["object"] = json!(pin);
    for name in ["master", "opening"] {
        let path = format!("records/{name}.tig");
        let mut record: Value =
            serde_json::from_slice(&fs::read(f.root.path().join(&path)).unwrap()).unwrap();
        record["definition"]["sha256"] = json!(pin.sha256);
        record["definition"]["bytes"] = json!(pin.bytes);
        write(f.root.path(), &path, &record);
    }
    git(f.root.path(), &["add", "definitions"]);
    f.commit();
    let (updated, _) = freeze(&f);
    let updated_recipe = recipe(&f, &updated);
    assert_ne!(updated_recipe.fingerprint(), expected.fingerprint());
    f.edit("scene", |v| {
        v["selected"] = json!(Object::from_bytes(b"changed native scene document absent"))
    });
    let (changed, _) = freeze(&f);
    assert_ne!(
        recipe(&f, &changed).fingerprint(),
        updated_recipe.fingerprint()
    );
    assert_eq!(
        fs::read(f.root.path().join("records/assembly.tig")).unwrap(),
        parent
    );
}

#[test]
fn insufficient_context_budget_refuses_before_any_metadata_store_access() {
    struct NoIo;
    impl tig_core::tree::NodeStore for NoIo {
        fn get(&self, _: &Object) -> tig_core::tree::TreeResult<Option<Vec<u8>>> {
            panic!("metadata store read before context budget admission");
        }
        fn put(&mut self, _: &[u8]) -> tig_core::tree::TreeResult<Object> {
            panic!("metadata store write before context budget admission");
        }
    }
    let f = fixture();
    let mut source = f.source();
    assert!(
        tig_episode::freeze_composition_context(
            &mut source,
            &f.request(),
            &mut NoIo,
            TreeLimits {
                max_entries: 0,
                ..TreeLimits::default()
            },
        )
        .is_err()
    );
    assert_eq!(
        source.byte_requests(),
        2,
        "bounded controls only, never absent primary/output bodies"
    );
    assert_eq!(source.active_readers(), 0);
}

#[test]
fn durable_context_replays_with_source_checkout_unavailable_and_refuses_substituted_admission() {
    let f = fixture();
    let metadata_parent = tempfile::tempdir().unwrap();
    let metadata = metadata_parent.path().join("metadata");
    let mut writer = MetadataStore::create(
        &metadata,
        tig_store::metadata::Limits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65_536,
        },
    )
    .unwrap();
    let mut source = f.source();
    let context = tig_episode::freeze_composition_context(
        &mut source,
        &f.request(),
        &mut writer,
        TreeLimits::default(),
    )
    .unwrap();
    let expected = recipe(&f, &context);
    let archive = source
        .archive_metadata(&mut writer, TreeLimits::default())
        .unwrap();
    let profile: reel::tig_inputs::ArchiveProfile = serde_json::from_value(json!({
        "schema":reel::tig_inputs::ARCHIVE_PROFILE_SCHEMA,"archive":archive,
        "checkpoint":source.checkpoint(),"admitted":f.profile["admitted"],
        "byte_source":f.profile["byte_source"],
    }))
    .unwrap();
    let checkpoint = writer.finish().unwrap();
    drop(writer);
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root.path().join(name),
            f.root.path().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut reader = MetadataReader::open(&metadata, &checkpoint).unwrap();
    let (mut restored, replayed) = tig_episode::restore_composition_context(
        profile.clone(),
        &f.request(),
        context.selection(),
        &mut reader,
        TreeLimits::default(),
    )
    .unwrap();
    let actual = tig_episode::composition_recipe(
        &mut restored,
        &f.request(),
        &replayed,
        &Object::from_bytes(b"admitted runtime"),
    )
    .unwrap();
    assert_eq!(actual.fingerprint(), expected.fingerprint());
    assert_eq!(replayed.selection(), context.selection());
    assert_eq!(
        restored.byte_requests(),
        4,
        "only two metadata controls per pass"
    );
    assert_eq!(restored.active_readers(), 0);
    let mut substituted = context.selection().clone();
    substituted.context = Object::from_bytes(b"foreign context").sha256;
    assert!(
        tig_episode::restore_composition_context(
            profile.clone(),
            &f.request(),
            &substituted,
            &mut reader,
            TreeLimits::default(),
        )
        .is_err()
    );
    let mut other_profile = profile;
    other_profile.checkpoint = Object::from_bytes(b"another source checkpoint").sha256;
    assert!(
        tig_episode::restore_composition_context(
            other_profile,
            &f.request(),
            context.selection(),
            &mut reader,
            TreeLimits::default(),
        )
        .is_err()
    );
}
