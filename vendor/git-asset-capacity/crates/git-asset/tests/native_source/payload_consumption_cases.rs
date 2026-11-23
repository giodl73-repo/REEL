use super::produced_cases::{operation, pin, subject};
use super::*;
use git_asset::source::native::{
    job::{self, Role},
    produced as replay,
};
use git_asset_core::tree::{NodeStore, TreeLimits};
use git_asset_model::graph::recipe::{
    self, Consumption, PayloadConsumption, Plan,
    produced::{self, Claim},
};
use git_asset_store::metadata::{Limits as StoreLimits, MetadataReader, MetadataStore};
use std::collections::BTreeMap;

fn setup() -> Fixture {
    let mut f = Fixture::new();
    f.record("rows", "inputs/rows", true, json!([]), Some(b"rows-v1"));
    f.record(
        "report",
        "outputs/report",
        false,
        json!([]),
        Some(b"report-v1"),
    );
    let mut definition: Value =
        serde_json::from_slice(&fs::read(f.root().join("definitions/output.json")).unwrap())
            .unwrap();
    definition["id"] = "types/container".into();
    let object = write(f.root(), "definitions/container.json", &definition);
    let old = f.output.clone();
    f.output = json!({"id":"types/container","sha256":object.sha256,"bytes":object.bytes});
    f.record(
        "control",
        "source/control",
        false,
        json!([{"id":"inputs/rows"}]),
        Some(b"bilingual-v1"),
    );
    f.output = old;
    let mut profile = f.profiles[1].clone();
    profile.definition_id = "types/container".into();
    profile.definition = object;
    profile.class = NodeKind::Container;
    profile.payloads = Payloads::SelectedObject;
    f.profiles.push(profile);
    f
}
fn fields(f: &Fixture) -> Vec<Consumption> {
    f.profiles
        .iter()
        .map(|p| Consumption {
            definition_id: p.definition_id.clone(),
            definition: p.definition.clone(),
            fields: BTreeSet::new(),
        })
        .collect()
}
fn payloads(f: &Fixture) -> Vec<PayloadConsumption> {
    f.profiles
        .iter()
        .map(|p| PayloadConsumption {
            definition_id: p.definition_id.clone(),
            definition: p.definition.clone(),
            selected: p.class == NodeKind::Primary,
            slots: BTreeSet::new(),
        })
        .collect()
}
fn projected(source: &mut AssetSnapshot, f: &Fixture) -> Plan {
    let graph = graph::resolve(source, &query(source, "source/control"), &f.profiles).unwrap();
    recipe::plan_with_payloads(
        &graph,
        source,
        &f.profiles,
        &fields(f),
        &payloads(f),
        &operation(),
        recipe::Limits {
            nodes: 100,
            edges: 100,
            bytes: 1_000_000,
        },
    )
    .unwrap()
}

#[test]
fn projected_claim_replay_and_job_context_preserve_full_captured_controls_after_git_disappears() {
    let f = setup();
    let repository = reader(f.root());
    let mut original = capture(&repository, commit(f.root()));
    let plan = projected(&mut original, &f);
    let disk = tempfile::tempdir().unwrap();
    let path = disk.path().join("metadata");
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
    let context = original
        .freeze_production_context(
            &query(&original, "source/control"),
            &f.profiles,
            b"{}",
            &mut store,
            TreeLimits::default(),
        )
        .unwrap();
    let target = subject(&original, &f.profiles);
    let claim = Claim::create(&plan, pin(&context), vec![target.subject().clone()]).unwrap();
    let legacy_query = query(&original, "source/control");
    let graph = graph::resolve(&mut original, &legacy_query, &f.profiles).unwrap();
    let legacy_plan = recipe::plan(
        &graph,
        &mut original,
        &f.profiles,
        &fields(&f),
        &operation(),
        recipe::Limits {
            nodes: 100,
            edges: 100,
            bytes: 1_000_000,
        },
    )
    .unwrap();
    let legacy_claim =
        Claim::create(&legacy_plan, pin(&context), vec![target.subject().clone()]).unwrap();
    let runtime = BTreeMap::new();
    let resources = job::freeze(
        &job::Admission {
            context: &context,
            plan: &plan,
            runtime: &runtime,
            output: None,
        },
        &mut store,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(
        resources.of(Role::Primary),
        &BTreeSet::from([Object::from_bytes(b"rows-v1")])
    );
    assert_eq!(
        resources.of(Role::Control),
        &BTreeSet::from([Object::from_bytes(b"bilingual-v1")])
    );
    let control = f.root().join("records/control.asset");
    let mut value: Value = serde_json::from_slice(&fs::read(&control).unwrap()).unwrap();
    value["selected"] = json!(Object::from_bytes(b"only Spanish changed"));
    fs::write(&control, serde_json::to_vec(&value).unwrap()).unwrap();
    let mut later = capture(&repository, commit(f.root()));
    let desired = projected(&mut later, &f);
    assert_eq!(desired.fingerprint(), plan.fingerprint());
    assert_ne!(desired.binding(), plan.binding());
    store.put(claim.bytes()).unwrap();
    let witness = store.finish().unwrap();
    drop(store);
    drop(original);
    drop(repository);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut store = MetadataReader::open(&path, &witness).unwrap();
    let fields = fields(&f);
    let payloads = payloads(&f);
    let operation = operation();
    let admission = replay::Admission {
        profiles: &f.profiles,
        consumption: &fields,
        operation: &operation,
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
    };
    let (_, restored) =
        replay::replay_with_payloads(&claim, &admission, &payloads, &mut store).unwrap();
    assert_eq!(restored.context().selection(), context.selection());
    assert_eq!(
        produced::compare(&desired, Some(&target), Some(restored.checked())).unwrap(),
        produced::Correspondence::ClaimedRecipeMatch
    );
    let readback = job::restore(
        resources.selection(),
        &job::Admission {
            context: restored.context(),
            plan: &plan,
            runtime: &runtime,
            output: None,
        },
        &mut store,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(resources.objects(), readback.objects());
    assert!(replay::replay(&claim, &admission, &mut store).is_err());
    assert!(
        replay::replay_with_payloads(&legacy_claim, &admission, &payloads, &mut store).is_err()
    );
    assert!(replay::replay(&legacy_claim, &admission, &mut store).is_ok());
    let mut wrong = payloads.clone();
    wrong[2].selected = true;
    assert!(replay::replay_with_payloads(&claim, &admission, &wrong, &mut store).is_err());
}
