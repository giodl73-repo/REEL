use super::*;
use tig_core::tree::MemoryNodes;

struct Fixture {
    nodes: MemoryNodes,
    resources: Selection,
    recipe: Digest,
    object: Object,
    raw: Vec<u8>,
}
fn fixture(padding: usize) -> Fixture {
    let raw = serde_json::to_vec(&json!({
        "schema":"reel.episode-conform.v1","episode_id":"synthetic",
        "language":"es","segments":[],"fixture_padding":"x".repeat(padding)
    }))
    .unwrap();
    fixture_raw(raw)
}
fn fixture_raw(raw: Vec<u8>) -> Fixture {
    let object = Object::from_bytes(&raw);
    let (frame, storage) =
        crate::tig_scene_production::control_snapshot::frame(&BTreeMap::from([(
            object.clone(),
            raw.clone(),
        )]))
        .unwrap();
    let mut nodes = MemoryNodes::default();
    for (pin, raw) in storage {
        assert_eq!(nodes.put(&raw).unwrap(), pin);
    }
    let recipe = Digest::blob(b"synthetic recipe admission");
    let header = nodes
        .put(
            &identity::canonical_json(&json!({
                "schema":"tig.native-job-context.experimental-3","recipe":recipe
            }))
            .unwrap(),
        )
        .unwrap();
    let entries = BTreeMap::from([
        (LogicalPath::parse("job/identity").unwrap(), header.clone()),
        (
            LogicalPath::parse("input-evidence/current-controls-root").unwrap(),
            frame,
        ),
    ]);
    let (tree, _) = Tree::build(&entries, &mut nodes, TreeLimits::default()).unwrap();
    Fixture {
        nodes,
        resources: Selection {
            root: tree.root().clone(),
            context: header.sha256,
        },
        recipe,
        object,
        raw,
    }
}

#[test]
fn exact_compiled_bytes_survive_multiple_metadata_chunks() {
    let mut f = fixture(tig_store::metadata::MAX_NODE_BYTES * 2 + 7);
    let report = controls(
        &f.resources,
        &f.recipe,
        &f.object.sha256,
        MAX_CONTROL_BYTES,
        &mut f.nodes,
    )
    .unwrap();
    assert_eq!(
        report["manifest"]["object"],
        serde_json::to_value(&f.object).unwrap()
    );
    assert_eq!(
        report["manifest"]["utf8"].as_str().unwrap().as_bytes(),
        f.raw
    );
    assert_eq!(report["metadata_writes"], 0);
    assert_eq!(report["materialized_media_bytes"], 0);
    assert_eq!(report["programs_started"], 0);
    assert_eq!(report["execution_attested"], false);
    assert_eq!(report["full_context_replay"], false);
    assert_eq!(report["approval_transferred"], false);
}

#[test]
fn stale_context_recipe_and_manifest_pins_refuse() {
    let mut f = fixture(0);
    let wrong = Digest::blob(b"different admission");
    let mut stale = f.resources.clone();
    stale.context = wrong.clone();
    assert!(
        controls(
            &stale,
            &f.recipe,
            &f.object.sha256,
            MAX_CONTROL_BYTES,
            &mut f.nodes
        )
        .is_err()
    );
    assert!(
        controls(
            &f.resources,
            &wrong,
            &f.object.sha256,
            MAX_CONTROL_BYTES,
            &mut f.nodes
        )
        .is_err()
    );
    assert!(
        controls(
            &f.resources,
            &f.recipe,
            &wrong,
            MAX_CONTROL_BYTES,
            &mut f.nodes
        )
        .is_err()
    );
}

#[test]
fn caller_and_hard_control_budgets_refuse_before_projection() {
    let mut f = fixture(100);
    for bound in [0, f.object.bytes - 1, MAX_CONTROL_BYTES + 1] {
        assert!(
            controls(
                &f.resources,
                &f.recipe,
                &f.object.sha256,
                bound,
                &mut f.nodes
            )
            .is_err()
        );
    }
}

#[test]
fn malformed_or_ambiguous_control_tags_refuse() {
    for raw in [
        br#"{"schema":"other"}"#.as_slice(),
        br#"{"schema":"reel.episode-conform.v1","schema":"reel.episode-conform.v1"}"#.as_slice(),
        br#"{"schema":"reel.episode-conform.v1""#.as_slice(),
    ] {
        let mut f = fixture_raw(raw.to_vec());
        assert!(
            controls(
                &f.resources,
                &f.recipe,
                &f.object.sha256,
                MAX_CONTROL_BYTES,
                &mut f.nodes
            )
            .is_err()
        );
    }
}

#[test]
fn missing_control_part_refuses() {
    let f = fixture(tig_store::metadata::MAX_NODE_BYTES + 9);
    let missing = Object::from_bytes(&f.raw[..tig_store::metadata::MAX_NODE_BYTES]);
    let mut incomplete = MemoryNodes::default();
    for (_, raw) in f.nodes.objects() {
        if Object::from_bytes(raw) != missing {
            incomplete.put(raw).unwrap();
        }
    }
    assert!(
        controls(
            &f.resources,
            &f.recipe,
            &f.object.sha256,
            MAX_CONTROL_BYTES,
            &mut incomplete
        )
        .is_err()
    );
}

#[test]
fn named_node_budget_and_write_refusal_are_enforced() {
    let mut nodes = MemoryNodes::default();
    let object = nodes.put(b"synthetic").unwrap();
    let mut reader = BoundedReader {
        inner: &mut nodes,
        reads: Cell::new(MAX_NODE_READS),
        bytes: Cell::new(0),
    };
    assert!(matches!(reader.get(&object), Err(TreeError::Budget)));
    reader.reads.set(0);
    reader.bytes.set(MAX_NODE_BYTES);
    assert!(matches!(reader.get(&object), Err(TreeError::Budget)));
    assert!(reader.put(b"no writes").is_err());
}

#[test]
fn real_store_inspection_keeps_the_checkpoint_unchanged() {
    let f = fixture(200);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("metadata");
    let mut writer = MetadataStore::create(
        &root,
        tig_store::metadata::Limits {
            nodes: 4096,
            bytes: 4 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65_536,
        },
    )
    .unwrap();
    for (_, raw) in f.nodes.objects() {
        writer.put(raw).unwrap();
    }
    let checkpoint = writer.finish().unwrap();
    drop(writer);
    let profile = json!({"schema":SCHEMA,"metadata_root":root,"metadata_minimum":checkpoint,
        "resources":f.resources,"recipe":f.recipe,"manifest_sha256":f.object.sha256,
        "max_control_bytes":MAX_CONTROL_BYTES});
    let path = dir.path().join("private-profile.json");
    fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();
    let report = inspect_episode_controls(&path).unwrap();
    assert_eq!(
        report["manifest"]["utf8"].as_str().unwrap().as_bytes(),
        f.raw
    );
    let reader = MetadataReader::open(&root, &checkpoint).unwrap();
    assert_eq!(reader.checkpoint(), &checkpoint);
    assert_eq!(reader.metrics().puts, 0);
    let mut invalid = profile;
    invalid["unknown_secret"] = "private-marker".into();
    fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
    let error = inspect_episode_controls(&path).unwrap_err().to_string();
    assert_eq!(
        error,
        "episode control inspection refused at profile admission"
    );
    assert!(!error.contains("private-marker"));
}
