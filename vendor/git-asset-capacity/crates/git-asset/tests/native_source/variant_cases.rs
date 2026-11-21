use super::*;
use git_asset_core::{
    Digest,
    tree::{NodeStore, TreeLimits},
};
use git_asset_model::{
    graph::recipe::{self, Consumption, Operation},
    variants::{self, Admission, Index, Parameters, Selection, Space},
};
use git_asset_store::metadata::{Limits as StoreLimits, MetadataReader, MetadataStore};

pub(super) fn setup() -> (Fixture, Vec<u8>, Vec<u8>, Admission) {
    let f = Fixture::new();
    let space=serde_json::to_vec(&json!({"schema":"tig.variant-space.experimental-1","axes":{"language":["en","es"],"style":["painted","realistic"]}})).unwrap();
    let admission = Admission {
        definition: graph::Reference {
            id: f.profiles[0].definition_id.clone(),
            object: f.profiles[0].definition.clone(),
        },
        space: Object::from_bytes(&space),
        semantics: Digest::blob(b"variant/v1"),
        uses: BTreeSet::from(["language".into()]),
    };
    let index=serde_json::to_vec(&json!({"schema":"tig.variant-index.experimental-1","asset":"inputs/rows","definition":admission.definition,"space":admission.space,"semantics":admission.semantics,"uses":["language"],"entries":[{"parameters":{"language":"en"},"slot":"en"},{"parameters":{"language":"es"},"slot":"es"}]})).unwrap();
    f.record("cue", "inputs/rows", true, json!([]), Some(&index));
    f.record(
        "scene",
        "scene/one",
        false,
        json!([{"id":"inputs/rows"}]),
        None,
    );
    let path = f.root().join("records/cue.asset");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["streams"] = json!([{"slot":"en","role":"audio","group":"language","object":Object::from_bytes(b"en1")},{"slot":"es","role":"audio","group":"language","object":Object::from_bytes(b"es1")}]);
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    (f, space, index, admission)
}
pub(super) fn select(
    source: &impl AssetResolver,
    space: &[u8],
    index: &[u8],
    admission: &Admission,
) -> Selection {
    let context = Parameters::from([
        ("language".into(), "en".into()),
        ("style".into(), "painted".into()),
    ]);
    variants::resolve(
        &source.resolve_id("inputs/rows").unwrap(),
        admission,
        &Space::parse(&Object::from_bytes(space), space).unwrap(),
        &Index::parse(&Object::from_bytes(index), index).unwrap(),
        &context,
    )
    .unwrap()
}
pub(super) fn plan(source: &mut AssetSnapshot, f: &Fixture, selection: &Selection) -> recipe::Plan {
    let mut profiles = f.profiles.clone();
    profiles[0].payloads = Payloads::Named {
        selected: false,
        slots: BTreeSet::from([selection.slot().into()]),
    };
    let q = query(source, "inputs/rows");
    let graph = graph::resolve(source, &q, &profiles).unwrap();
    assert!(graph.complete_declared_coverage());
    assert_eq!(
        graph.production_requirements().unwrap().primary(),
        &BTreeSet::from([selection.object().clone()])
    );
    let consumption = profiles
        .iter()
        .map(|p| Consumption {
            definition_id: p.definition_id.clone(),
            definition: p.definition.clone(),
            fields: BTreeSet::new(),
        })
        .collect::<Vec<_>>();
    let operation = Operation {
        name: "synthetic-consumer".into(),
        semantics: Digest::blob(b"consumer"),
        producer: Object::from_bytes(b"producer"),
        parameters: Object::from_bytes(
            &serde_json::to_vec(
                &json!({"variant":selection.subject(),"context":selection.parameters()}),
            )
            .unwrap(),
        ),
    };
    recipe::plan(
        &graph,
        source,
        &profiles,
        &consumption,
        &operation,
        recipe::Limits {
            nodes: 100,
            edges: 100,
            bytes: 1_000_000,
        },
    )
    .unwrap()
}

#[test]
fn native_variants_real_git_leaf_changes_keep_unrelated_audio_recipe_and_primary_closure_small() {
    let (f, space, index, admission) = setup();
    let parent = fs::read(f.root().join("records/scene.asset")).unwrap();
    let repository = reader(f.root());
    let before_commit = commit(f.root());
    let mut before = capture(&repository, before_commit.clone());
    let selected = select(&before, &space, &index, &admission);
    let original = plan(&mut before, &f, &selected);
    let path = f.root().join("records/cue.asset");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["streams"][1]["object"] = json!(Object::from_bytes(b"es2"));
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let mut changed = capture(&repository, commit(f.root()));
    let same = select(&changed, &space, &index, &admission);
    let after = plan(&mut changed, &f, &same);
    assert_eq!(selected.subject(), same.subject());
    assert_ne!(selected.record(), same.record());
    assert_ne!(selected.checkpoint(), same.checkpoint());
    assert_ne!(original.binding(), after.binding());
    assert_eq!(original.fingerprint(), after.fingerprint());
    assert_eq!(
        fs::read(f.root().join("records/scene.asset")).unwrap(),
        parent
    );
    let old = capture(&repository, before_commit);
    assert_eq!(
        old.resolve_id("inputs/rows").unwrap().stream("es").unwrap(),
        &Object::from_bytes(b"es1")
    );
    value["streams"][0]["object"] = json!(Object::from_bytes(b"en2"));
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    let mut changed = capture(&repository, commit(f.root()));
    let new = select(&changed, &space, &index, &admission);
    let changed_plan = plan(&mut changed, &f, &new);
    assert_ne!(new.subject(), same.subject());
    assert_ne!(changed_plan.fingerprint(), after.fingerprint());
}

#[test]
fn native_variants_restore_exact_index_space_and_current_metadata_without_git() {
    let (f, space, index, admission) = setup();
    let source = capture(&reader(f.root()), commit(f.root()));
    let before = select(&source, &space, &index, &admission);
    let runtime = tempfile::tempdir().unwrap();
    let path = runtime.path().join("metadata");
    let mut store = MetadataStore::create(
        &path,
        StoreLimits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    // Explicit retained interpretation controls, not primary media or global claims.
    let space_pin = store.put(&space).unwrap();
    let index_pin = store.put(&index).unwrap();
    let parsed_space = Space::parse(&space_pin, &space).unwrap();
    let context = parsed_space
        .admit(&Parameters::from([
            ("language".into(), "en".into()),
            ("style".into(), "painted".into()),
        ]))
        .unwrap();
    let context_pin = store.put(context.bytes()).unwrap();
    assert_eq!(
        before.controls(),
        BTreeSet::from([space_pin.clone(), index_pin.clone(), context_pin.clone()])
    );
    let root = source
        .archive_metadata(&mut store, TreeLimits::default())
        .unwrap();
    let witness = store.finish().unwrap();
    drop(store);
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut store = MetadataReader::open(&path, &witness).unwrap();
    let source = AssetSnapshot::from_metadata_archive(
        &mut store,
        &root,
        Limits::default(),
        TreeLimits::default(),
    )
    .unwrap();
    let parsed_space = Space::parse(&space_pin, &store.get(&space_pin).unwrap().unwrap()).unwrap();
    let parsed_index = Index::parse(&index_pin, &store.get(&index_pin).unwrap().unwrap()).unwrap();
    let restored = variants::Context::restore(
        &context_pin,
        &store.get(&context_pin).unwrap().unwrap(),
        &parsed_space,
    )
    .unwrap();
    let after = variants::resolve_in_context(
        &source.resolve_id("inputs/rows").unwrap(),
        &admission,
        &parsed_space,
        &parsed_index,
        &restored,
        &space_pin,
    )
    .unwrap();
    assert_eq!(after.subject(), before.subject());
    assert_eq!(after.record(), before.record());
    assert_eq!(after.controls(), before.controls());
    assert!(!after.complete_semantic_coverage());
}
