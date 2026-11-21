use super::*;
#[path = "assessment_cases.rs"]
mod assessment_cases;
use git_asset::source::native::{context::ProductionContext, produced as native_claim};
use git_asset_core::{
    Digest,
    tree::{MemoryNodes, NodeStore, TreeError, TreeLimits, TreeResult},
};
use git_asset_model::graph::recipe::{
    self, Consumption, Operation, Plan,
    assessment::{self, Key, Observation, Request, State},
    produced::{self, BoundSubject, Claim, ContextPin, Correspondence as C, Selector},
};
use git_asset_store::metadata::{Limits as StoreLimits, MetadataReader, MetadataStore};

pub(super) fn policies(f: &Fixture) -> Vec<Consumption> {
    f.profiles
        .iter()
        .map(|p| Consumption {
            definition_id: p.definition_id.clone(),
            definition: p.definition.clone(),
            fields: if p.class == NodeKind::Primary {
                BTreeSet::from(["text".into()])
            } else {
                BTreeSet::new()
            },
        })
        .collect()
}
pub(super) fn operation() -> Operation {
    Operation {
        name: "report".into(),
        semantics: Digest::blob(b"report contract"),
        producer: Object::from_bytes(b"synthetic producer"),
        parameters: Object::from_bytes(b"{}"),
    }
}
pub(super) fn recipe(
    source: &mut AssetSnapshot,
    profiles: &[Profile],
    fields: &[Consumption],
    operation: &Operation,
) -> Plan {
    let q = query(source, "inputs/rows");
    let graph = graph::resolve(source, &q, profiles).unwrap();
    recipe::plan(
        &graph,
        source,
        profiles,
        fields,
        operation,
        recipe::Limits {
            nodes: 100,
            edges: 100,
            bytes: 1_000_000,
        },
    )
    .unwrap()
}
pub(super) fn subject(source: &AssetSnapshot, profiles: &[Profile]) -> BoundSubject {
    let profile = &profiles[1];
    let admission = graph::Reference {
        id: profile.definition_id.clone(),
        object: profile.definition.clone(),
    };
    BoundSubject::resolve(
        &source.resolve_id("outputs/report").unwrap(),
        &admission,
        Selector::Selected {},
    )
    .unwrap()
    .unwrap()
}
pub(super) fn pin(context: &ProductionContext) -> ContextPin {
    ContextPin {
        root: context.selection().root.clone(),
        identity: context.selection().context.clone(),
    }
}
fn admission<'a>(
    profiles: &'a [Profile],
    fields: &'a [Consumption],
    operation: &'a Operation,
) -> native_claim::Admission<'a> {
    native_claim::Admission {
        profiles,
        consumption: fields,
        operation,
        native: Limits::default(),
        tree: TreeLimits::default(),
        query: graph::Limits {
            nodes: 100,
            edges: 100,
            depth: 10,
            bytes: 1_000_000,
        },
        recipe: recipe::Limits {
            nodes: 100,
            edges: 100,
            bytes: 1_000_000,
        },
    }
}

#[test]
fn produced_claim_replays_original_context_after_reopen_and_detects_later_stale_outputs() {
    let f = Fixture::new();
    f.record("rows", "inputs/rows", true, json!([]), Some(b"rows-v1"));
    f.record(
        "report",
        "outputs/report",
        false,
        json!([]),
        Some(b"report-v1"),
    );
    let repository = reader(f.root());
    let mut source = capture(&repository, commit(f.root()));
    let profiles = f.profiles.clone();
    let fields = policies(&f);
    let operation = operation();
    let original = recipe(&mut source, &profiles, &fields, &operation);
    let current = subject(&source, &profiles);
    let runtime = tempfile::tempdir().unwrap();
    let path = runtime.path().join("metadata");
    let mut nodes = MetadataStore::create(
        &path,
        StoreLimits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let q = query(&source, "inputs/rows");
    let context = source
        .freeze_production_context(&q, &profiles, b"{}", &mut nodes, TreeLimits::default())
        .unwrap();
    let candidate =
        Claim::create(&original, pin(&context), vec![current.subject().clone()]).unwrap();
    assert_eq!(&nodes.put(candidate.bytes()).unwrap(), candidate.object());
    let witness = nodes.finish().unwrap();
    drop(nodes);
    drop(source);
    let mut nodes = MetadataReader::open(&path, &witness).unwrap();
    let raw = nodes.get(candidate.object()).unwrap().unwrap();
    let claim = Claim::parse(candidate.object(), &raw).unwrap();
    let trusted = admission(&profiles, &fields, &operation);
    let (_, replay) = native_claim::replay(&claim, &trusted, &mut nodes).unwrap();
    assert_eq!(replay.context().selection(), context.selection());
    // Attach only a pointer after metadata finish/readback, without a self hash.
    let output_path = f.root().join("records/report.asset");
    let mut output: Value = serde_json::from_slice(&fs::read(&output_path).unwrap()).unwrap();
    output["metadata"]["production_claim"] = json!(claim.object());
    fs::write(output_path, serde_json::to_vec(&output).unwrap()).unwrap();
    let input_path = f.root().join("records/rows.asset");
    let mut input: Value = serde_json::from_slice(&fs::read(&input_path).unwrap()).unwrap();
    input["metadata"] = json!({"annotation":"changed"});
    fs::write(&input_path, serde_json::to_vec(&input).unwrap()).unwrap();
    let mut annotation = capture(&repository, commit(f.root()));
    let desired = recipe(&mut annotation, &profiles, &fields, &operation);
    let current = subject(&annotation, &profiles);
    assert_eq!(desired.fingerprint(), original.fingerprint());
    assert_ne!(desired.binding(), original.binding());
    assert!(claim.check_plan(&desired).is_err());
    assert_eq!(
        produced::compare(&desired, Some(&current), Some(replay.checked())).unwrap(),
        C::ClaimedRecipeMatch
    );
    input["authored"]["text"] = json!("changed consumed text");
    fs::write(input_path, serde_json::to_vec(&input).unwrap()).unwrap();
    let mut changed = capture(&repository, commit(f.root()));
    let stale = recipe(&mut changed, &profiles, &fields, &operation);
    assert_ne!(stale.fingerprint(), original.fingerprint());
    drop(repository);
    drop(nodes);
    drop(annotation);
    drop(changed);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut nodes = MetadataReader::open(&path, &witness).unwrap();
    let (source, replay) = native_claim::replay(&claim, &trusted, &mut nodes).unwrap();
    assert_eq!(source.checkpoint(), original.checkpoint());
    assert_eq!(
        produced::compare(&stale, Some(&current), Some(replay.checked())).unwrap(),
        C::StaleRecipe
    );
    assert_eq!(
        produced::compare(&desired, Some(&current), Some(replay.checked())).unwrap(),
        C::ClaimedRecipeMatch
    );
    assert_eq!(
        produced::compare(&desired, Some(&current), None).unwrap(),
        C::Unproven
    );
    // The scoped catch-up view works after Git/working metadata go offline.
    // Plans bind the selected current checkpoint; the original claim stays frozen.
    let requests = [
        Request {
            key: Key {
                asset_id: "outputs/report".into(),
                selector: Selector::Selected {},
            },
            desired: Some(&stale),
            current: Observation::Present(&current),
            claim: Observation::Present(replay.checked()),
        },
        Request {
            key: Key {
                asset_id: "outputs/unavailable".into(),
                selector: Selector::Selected {},
            },
            desired: None,
            current: Observation::Absent,
            claim: Observation::Absent,
        },
    ];
    let assessment = assessment::assess(stale.checkpoint(), &requests, 2).unwrap();
    assert_eq!(assessment.entries()[0].state, State::StaleRecipe);
    assert_eq!(assessment.entries()[1].state, State::Blocked);
    assert!(!assessment.complete_semantic_coverage());
}

#[test]
fn produced_native_replay_refuses_unadmitted_producer_context_parameters_policy_and_origin() {
    use std::cell::Cell;
    struct Readonly {
        nodes: MemoryNodes,
        reads: Cell<usize>,
        writes: usize,
    }
    impl NodeStore for Readonly {
        fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
            self.reads.set(self.reads.get() + 1);
            self.nodes.get(object)
        }
        fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
            self.writes += 1;
            Err(TreeError::Store("readonly".into()))
        }
    }
    let f = Fixture::new();
    f.record("rows", "inputs/rows", true, json!([]), Some(b"rows"));
    f.record(
        "report",
        "outputs/report",
        false,
        json!([]),
        Some(b"report"),
    );
    let mut source = capture(&reader(f.root()), commit(f.root()));
    let profiles = f.profiles.clone();
    let fields = policies(&f);
    let operation = operation();
    let original = recipe(&mut source, &profiles, &fields, &operation);
    let output = subject(&source, &profiles);
    let mut nodes = MemoryNodes::default();
    let q = query(&source, "inputs/rows");
    let context = source
        .freeze_production_context(&q, &profiles, b"{}", &mut nodes, TreeLimits::default())
        .unwrap();
    let claim = Claim::create(&original, pin(&context), vec![output.subject().clone()]).unwrap();
    let trusted = admission(&profiles, &fields, &operation);
    let mut store = Readonly {
        nodes,
        reads: Cell::new(0),
        writes: 0,
    };
    assert!(native_claim::replay(&claim, &trusted, &mut store).is_ok());
    assert_eq!(store.writes, 0);
    let other = Operation {
        producer: Object::from_bytes(b"unadmitted producer"),
        ..operation.clone()
    };
    let mut wrong = admission(&profiles, &fields, &other);
    store.reads.set(0);
    assert!(native_claim::replay(&claim, &wrong, &mut store).is_err());
    assert_eq!(store.reads.get(), 0);
    wrong = admission(&profiles, &fields, &operation);
    wrong.tree.max_entries = 1;
    assert!(native_claim::replay(&claim, &wrong, &mut store).is_err());
    let other = Operation {
        parameters: Object::from_bytes(b"other parameters"),
        ..operation.clone()
    };
    let bad_plan = recipe(&mut source, &profiles, &fields, &other);
    let bad_claim =
        Claim::create(&bad_plan, pin(&context), vec![output.subject().clone()]).unwrap();
    let wrong = admission(&profiles, &fields, &other);
    assert!(matches!(
        native_claim::replay(&bad_claim, &wrong, &mut store),
        Err(Error::Integrity)
    ));
    for field in ["root", "identity"] {
        let mut value: Value = serde_json::from_slice(claim.bytes()).unwrap();
        value["context"][field] = if field == "root" {
            json!(Object::from_bytes(b"missing root"))
        } else {
            json!(Digest::blob(b"wrong header"))
        };
        let bytes = serde_json::to_vec(&value).unwrap();
        let forged = Claim::parse(&Object::from_bytes(&bytes), &bytes).unwrap();
        assert!(native_claim::replay(&forged, &trusted, &mut store).is_err());
    }
    let mut value: Value = serde_json::from_slice(claim.bytes()).unwrap();
    value["origin"]["binding"] = json!(Digest::blob(b"wrong binding"));
    let bytes = serde_json::to_vec(&value).unwrap();
    let forged = Claim::parse(&Object::from_bytes(&bytes), &bytes).unwrap();
    assert!(native_claim::replay(&forged, &trusted, &mut store).is_err());
    let mut value: Value = serde_json::from_slice(claim.bytes()).unwrap();
    let mut conflict = source
        .resolve_id("inputs/rows")
        .unwrap()
        .selected()
        .unwrap()
        .clone();
    conflict.bytes += 1;
    value["outputs"][0]["object"] = json!(conflict);
    let bytes = serde_json::to_vec(&value).unwrap();
    let forged = Claim::parse(&Object::from_bytes(&bytes), &bytes).unwrap();
    assert!(matches!(
        native_claim::replay(&forged, &trusted, &mut store),
        Err(Error::Integrity)
    ));
    let mut foreign = profiles.clone();
    foreign[0].payloads = Payloads::None;
    assert!(
        native_claim::replay(
            &claim,
            &admission(&foreign, &fields, &operation),
            &mut store
        )
        .is_err()
    );
    let mut foreign = fields.clone();
    foreign[0].fields = BTreeSet::from(["undeclared".into()]);
    assert!(
        native_claim::replay(
            &claim,
            &admission(&profiles, &foreign, &operation),
            &mut store
        )
        .is_err()
    );
    assert_eq!(store.writes, 0);
}
