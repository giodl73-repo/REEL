use super::*;
#[path = "retirement_cases/age_cases.rs"]
mod age_cases;
#[path = "retirement_cases/bulk_cases.rs"]
mod bulk_cases;
#[path = "retirement_cases/paged_cases.rs"]
mod paged_cases;
#[path = "retirement_cases/scale_cases.rs"]
mod scale_cases;
use git_asset::source::native::retirement::{self, Categories, Filter, Limits as BindingLimits};
use git_asset_core::{
    Digest, identity,
    tree::{MemoryNodes, Tree, TreeLimits},
};
use git_asset_model::graph::Reference;
use git_asset_store::{
    Key,
    metadata::{Limits as MetadataLimits, MetadataReader, MetadataStore},
    source_claims::{
        self, Command as SourceCommand, Kind, Places, Selection,
        admission::{Namespace, Policy, Select},
    },
};
use std::{collections::BTreeMap, io::Read};

fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn profiles(f: &Fixture) -> Vec<Profile> {
    let mut profiles = f.profiles.clone();
    for p in &mut profiles {
        p.covered.insert(EdgeKind::Composition);
    }
    profiles
}
fn fixture() -> (Fixture, AssetSnapshot) {
    let f = Fixture::new();
    for (name, bytes) in [
        ("a", b"shared".as_slice()),
        ("b", b"shared"),
        ("c", b"unique"),
    ] {
        f.record(name, &format!("asset/{name}"), true, json!([]), Some(bytes));
    }
    let snapshot = capture(&reader(f.root()), commit(f.root()));
    (f, snapshot)
}
fn roots(s: &AssetSnapshot) -> BTreeSet<Reference> {
    s.selected_records()
        .map(|r| r.reference().clone())
        .collect()
}
fn category(s: &AssetSnapshot) -> Categories {
    BTreeMap::from([(
        "drawing".into(),
        BTreeSet::from([s.resolve_id("asset/a").unwrap().definition().clone()]),
    )])
}
fn filter(s: &AssetSnapshot, names: &[&str]) -> Filter {
    Filter {
        assets: names
            .iter()
            .map(|id| s.resolve_id(id).unwrap().reference().clone())
            .collect(),
        categories: BTreeSet::new(),
    }
}

#[test]
fn native_storage_binding_aliases_are_exact_and_retained_subsets_remain_inspectable() {
    let (f, mut snapshot) = fixture();
    let names = roots(&snapshot);
    let mut nodes = MemoryNodes::default();
    let selected = retirement::build(
        &mut snapshot,
        names,
        profiles(&f),
        &mut nodes,
        BindingLimits::default(),
    )
    .unwrap();
    let inventory = retirement::inspect(
        &selected,
        &selected.context,
        &mut nodes,
        BindingLimits::default(),
    )
    .unwrap();
    let block = retirement::select(
        &inventory,
        &category(&snapshot),
        &filter(&snapshot, &["asset/a"]),
    )
    .unwrap();
    assert_eq!(block.assets.len(), 1);
    assert_eq!(block.paths.len(), 4);
    assert_eq!(block.objects.len(), 4);
    assert!(!block.authorizes_removal && !block.age_filter_supported);
    let (mut entries, _) = Tree::from_root(selected.root.clone())
        .entries(&mut nodes, TreeLimits::default())
        .unwrap();
    assert_eq!(
        entries
            .values()
            .filter(|o| **o == Object::from_bytes(b"shared"))
            .count(),
        2
    );
    for p in &block.paths {
        entries.remove(p);
    }
    let survivor = Selection {
        root: Tree::build(&entries, &mut nodes, TreeLimits::default())
            .unwrap()
            .0
            .root()
            .clone(),
        context: d("owned-subset-context"),
    };
    let inventory = retirement::inspect(
        &survivor,
        &selected.context,
        &mut nodes,
        BindingLimits::default(),
    )
    .unwrap();
    let a = retirement::select(
        &inventory,
        &Categories::new(),
        &filter(&snapshot, &["asset/a"]),
    )
    .unwrap();
    assert_eq!(a.assets[0].remaining_bindings, 0);
    assert!(a.paths.is_empty());
    let b = retirement::select(
        &inventory,
        &Categories::new(),
        &filter(&snapshot, &["asset/b"]),
    )
    .unwrap();
    assert_eq!(b.paths.len(), 4);
    assert!(b.objects.contains(&Object::from_bytes(b"shared")));
    assert!(
        retirement::inspect(
            &survivor,
            &d("unadmitted-header"),
            &mut nodes,
            BindingLimits::default()
        )
        .is_err()
    );
    entries.insert(
        LogicalPath::parse("foreign").unwrap(),
        Object::from_bytes(b"foreign"),
    );
    let forged = Selection {
        root: Tree::build(&entries, &mut nodes, TreeLimits::default())
            .unwrap()
            .0
            .root()
            .clone(),
        context: survivor.context,
    };
    assert!(
        retirement::inspect(
            &forged,
            &selected.context,
            &mut nodes,
            BindingLimits::default()
        )
        .is_err()
    );
}

#[test]
fn exact_categories_and_aggregate_native_graph_budgets_cannot_be_inferred_or_multiplied() {
    let (f, mut snapshot) = fixture();
    let mut nodes = MemoryNodes::default();
    let all = roots(&snapshot);
    let limits = BindingLimits {
        nodes: 2,
        ..BindingLimits::default()
    };
    assert!(
        retirement::build(&mut snapshot, all.clone(), profiles(&f), &mut nodes, limits).is_err()
    );
    assert_eq!(nodes.objects().count(), 0);
    let limits = BindingLimits {
        writes: 0,
        ..BindingLimits::default()
    };
    assert!(
        retirement::build(&mut snapshot, all.clone(), profiles(&f), &mut nodes, limits).is_err()
    );
    assert_eq!(nodes.objects().count(), 0);
    let mut unsupported = profiles(&f);
    unsupported[0].payloads = Payloads::Unsupported;
    assert!(
        retirement::build(
            &mut snapshot,
            all.clone(),
            unsupported,
            &mut nodes,
            BindingLimits::default()
        )
        .is_err()
    );
    assert_eq!(nodes.objects().count(), 0);
    let selected = retirement::build(
        &mut snapshot,
        all,
        profiles(&f),
        &mut nodes,
        BindingLimits::default(),
    )
    .unwrap();
    let inventory = retirement::inspect(
        &selected,
        &selected.context,
        &mut nodes,
        BindingLimits::default(),
    )
    .unwrap();
    let mut category_filter = Filter {
        assets: BTreeSet::new(),
        categories: BTreeSet::from(["drawing".into()]),
    };
    assert_eq!(
        retirement::select(&inventory, &category(&snapshot), &category_filter)
            .unwrap()
            .assets
            .len(),
        3
    );
    category_filter.assets = filter(&snapshot, &["asset/c"]).assets;
    assert_eq!(
        retirement::select(&inventory, &category(&snapshot), &category_filter)
            .unwrap()
            .assets
            .len(),
        1
    );
    let wrong = BTreeMap::from([(
        "drawing".into(),
        BTreeSet::from([Reference {
            id: "types/primary".into(),
            object: Object::from_bytes(b"different-definition"),
        }]),
    )]);
    assert!(
        retirement::select(&inventory, &wrong, &category_filter)
            .unwrap()
            .assets
            .is_empty()
    );
    category_filter.categories.insert("unadmitted".into());
    assert!(retirement::select(&inventory, &category(&snapshot), &category_filter).is_err());
    let wrong_revision = Filter {
        assets: BTreeSet::from([Reference {
            id: "asset/a".into(),
            object: Object::from_bytes(b"other-record-revision"),
        }]),
        categories: BTreeSet::new(),
    };
    assert!(matches!(
        retirement::select(&inventory, &Categories::new(), &wrong_revision),
        Err(retirement::Error::Incomplete)
    ));
    assert!(
        retirement::select(
            &inventory,
            &Categories::new(),
            &Filter {
                assets: BTreeSet::new(),
                categories: BTreeSet::new()
            }
        )
        .is_err()
    );
}

#[test]
fn storage_closure_includes_explicit_output_bytes_without_admitting_them_as_production_inputs() {
    let f = Fixture::new();
    f.record("output", "result/1", false, json!([]), Some(b"old render"));
    let mut snapshot = capture(&reader(f.root()), commit(f.root()));
    let mut p = profiles(&f);
    p[1].payloads = Payloads::SelectedObject;
    let root = snapshot.resolve_id("result/1").unwrap().reference().clone();
    let checkpoint = snapshot.checkpoint().clone();
    let graph = graph::resolve(
        &mut snapshot,
        &Query {
            checkpoint,
            roots: BTreeSet::from([root.clone()]),
            kinds: BTreeSet::from([EdgeKind::Production, EdgeKind::Composition]),
            limits: graph::Limits {
                nodes: 10,
                edges: 10,
                depth: 10,
                bytes: 100_000,
            },
        },
        &p,
    )
    .unwrap();
    assert!(
        graph
            .storage_objects()
            .unwrap()
            .contains(&Object::from_bytes(b"old render"))
    );
    assert!(graph.production_requirements().is_err());
    let production = graph::resolve(
        &mut snapshot,
        &Query {
            checkpoint: graph.checkpoint().clone(),
            roots: BTreeSet::from([root.clone()]),
            kinds: BTreeSet::from([EdgeKind::Production]),
            limits: graph::Limits {
                nodes: 10,
                edges: 10,
                depth: 10,
                bytes: 100_000,
            },
        },
        &p,
    )
    .unwrap();
    assert!(production.complete_declared_coverage());
    assert!(production.production_requirements().is_err());
    let mut nodes = MemoryNodes::default();
    let selected = retirement::build(
        &mut snapshot,
        BTreeSet::from([root]),
        p,
        &mut nodes,
        BindingLimits::default(),
    )
    .unwrap();
    assert_eq!(
        retirement::inspect(
            &selected,
            &selected.context,
            &mut nodes,
            BindingLimits::default()
        )
        .unwrap()
        .assets(),
        1
    );
}

fn metadata(root: &Path) -> MetadataStore {
    MetadataStore::create(
        root,
        MetadataLimits {
            nodes: 100_000,
            bytes: 64 * 1024 * 1024,
            database_bytes: 128 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap()
}
fn namespace(root: &Path, metadata: &MetadataStore, selection: &Selection) -> Namespace {
    let places = Places {
        current: vec![key("F"), key("M")],
        pending: vec![key("F"), key("M")],
        predecessor: vec![key("M")],
    };
    let policy = Policy {
        prefer_active: true,
        classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
            .into_iter()
            .map(|k| (k, places.clone()))
            .collect(),
    };
    let mut ns = Namespace::create(root, key("project"), key("epoch"), policy).unwrap();
    let limits = protection_limits();
    ns.apply(
        key("enroll"),
        SourceCommand::Enroll {
            source: key("main"),
            owner: key("owner"),
            class: Kind::Branch,
        },
        metadata,
        &limits,
    )
    .unwrap();
    ns.select(
        &Select {
            prepare: key("prepare"),
            adopt: key("adopt"),
            source: key("main"),
            owner: key("owner"),
            expected_revision: 1,
            selection: selection.clone(),
        },
        metadata,
        &limits,
    )
    .unwrap();
    ns.apply(
        key("withdraw"),
        SourceCommand::Withdraw {
            source: key("main"),
            owner: key("owner"),
            expected_revision: 3,
        },
        metadata,
        &limits,
    )
    .unwrap();
    ns
}
fn protection_limits() -> git_asset_store::protection::Limits {
    git_asset_store::protection::Limits {
        roots: 256,
        bindings: 100_000,
        associations: 100_000,
        reads: 100_000,
        read_bytes: 64 * 1024 * 1024,
        millis: 60_000,
    }
}

#[test]
fn real_cli_finishes_and_reads_back_native_bindings_then_selects_an_exact_historical_block_without_writes()
 {
    let (f, snapshot) = fixture();
    let dir = tempfile::tempdir().unwrap();
    let mut store = metadata(&dir.path().join("metadata"));
    let archive = snapshot
        .archive_metadata(&mut store, TreeLimits::default())
        .unwrap();
    let minimum = store.finish().unwrap();
    fs::rename(f.root().join(".git"), f.root().join("git-offline")).unwrap();
    let bind_profile = json!({"schema":"tig.native-retirement-bind-profile.design-1","metadata":{"root":dir.path().join("metadata"),"minimum":minimum},
        "snapshot":archive,"roots":roots(&snapshot),"profiles":profiles(&f),"allow_bind":false});
    let profile_path = dir.path().join("profile.json");
    let request_path = dir.path().join("request.json");
    let call = |op: &str, profile: &Value, request: Option<&Value>| {
        fs::write(&profile_path, serde_json::to_vec(profile).unwrap()).unwrap();
        let mut c = Command::new(env!("CARGO_BIN_EXE_git-asset-retirement"));
        c.arg(op).arg(&profile_path);
        if let Some(request) = request {
            fs::write(&request_path, serde_json::to_vec(request).unwrap()).unwrap();
            c.arg(&request_path);
        }
        c.output().unwrap()
    };
    let refused = call("bind", &bind_profile, None);
    assert!(!refused.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&refused.stdout).unwrap()["code"],
        "binding_disabled"
    );
    assert_eq!(
        MetadataStore::open(&dir.path().join("metadata"), &minimum)
            .unwrap()
            .checkpoint(),
        minimum
    );
    let mut allowed = bind_profile.clone();
    allowed["allow_bind"] = json!(true);
    let result = call("bind", &allowed, None);
    assert!(result.status.success(), "{result:?}");
    assert!(result.stderr.is_empty());
    let result: Value = serde_json::from_slice(&result.stdout).unwrap();
    let selection: Selection = serde_json::from_value(result["selection"].clone()).unwrap();
    let minimum = serde_json::from_value(result["metadata_checkpoint"].clone()).unwrap();
    drop(store);
    let store = MetadataStore::open(&dir.path().join("metadata"), &minimum).unwrap();
    let ns = namespace(&dir.path().join("namespace"), &store, &selection);
    let before = ns.checkpoint();
    let select_profile = json!({"schema":"tig.native-retirement-select-profile.design-1","metadata":{"root":dir.path().join("metadata"),"minimum":minimum},
        "namespace":{"root":dir.path().join("namespace"),"minimum":before},"source":"main","owner":"owner","predecessor_revision":3,
        "header":selection.context,"categories":category(&snapshot)});
    let request = json!({"assets":[],"categories":["drawing"]});
    let output = call("select", &select_profile, Some(&request));
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let preview: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(preview["preview"]["assets"].as_array().unwrap().len(), 3);
    let selected: git_asset_store::final_retirement::bulk::Selection =
        serde_json::from_value(preview["selection"].clone()).unwrap();
    assert_eq!(selected.request.predecessor_revision, 3);
    assert_eq!(selected.request.paths.len(), 12);
    assert_eq!(
        Namespace::open(&dir.path().join("namespace"), &before)
            .unwrap()
            .checkpoint(),
        before
    );
    assert_eq!(
        MetadataReader::open(&dir.path().join("metadata"), &minimum)
            .unwrap()
            .checkpoint()
            .clone(),
        minimum
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("PRIVATE AUTHORING TEXT") && !text.contains("DO NOT PRINT"));
    for (field, value, code) in [
        ("owner", json!("other"), "wrong_authority"),
        ("header", json!(d("wrong")), "wrong_binding"),
    ] {
        let mut wrong = select_profile.clone();
        wrong[field] = value;
        let output = call("select", &wrong, Some(&request));
        assert!(!output.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap()["code"],
            code
        );
    }
    let mut bad = request.clone();
    bad["min_deprecated_millis"] = json!(0);
    let output = call("select", &select_profile, Some(&bad));
    assert!(!output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["code"],
        "invalid_document"
    );
}
