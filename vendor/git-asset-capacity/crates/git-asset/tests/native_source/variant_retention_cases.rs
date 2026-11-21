use super::*;
use git_asset::source::native::{
    context::ProductionContext,
    job::{self, Role},
    variants::{self, Policy, Request},
};
use git_asset_core::tree::{NodeStore, Tree, TreeLimits, TreeResult};
use git_asset_model::{
    graph::recipe::Plan,
    variants::{Parameters, Space},
};
use git_asset_store::metadata::{Limits as StoreLimits, MetadataReader, MetadataStore};
use std::collections::BTreeMap;

#[derive(Default)]
struct Nodes(BTreeMap<Object, Vec<u8>>);
fn query_limits() -> graph::Limits {
    graph::Limits {
        nodes: 100,
        edges: 100,
        depth: 10,
        bytes: 1_000_000,
    }
}
impl NodeStore for Nodes {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        Ok(self.0.get(object).cloned())
    }
    fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
        let object = Object::from_bytes(bytes);
        self.0.insert(object.clone(), bytes.to_vec());
        Ok(object)
    }
}

struct Case {
    f: Fixture,
    source: AssetSnapshot,
    profiles: Vec<Profile>,
    plan: Plan,
    context: ProductionContext,
    policies: Vec<Policy>,
    requests: Vec<Request>,
    nodes: Nodes,
}
impl Case {
    fn new() -> Self {
        let (f, space, index, admission) = super::variant_cases::setup();
        let mut source = capture(&reader(f.root()), commit(f.root()));
        let selection = super::variant_cases::select(&source, &space, &index, &admission);
        let plan = super::variant_cases::plan(&mut source, &f, &selection);
        let descriptor = serde_json::to_vec(
            &json!({"variant":selection.subject(),"context":selection.parameters()}),
        )
        .unwrap();
        let mut profiles = f.profiles.clone();
        profiles[0].payloads = Payloads::Named {
            selected: false,
            slots: BTreeSet::from(["en".into()]),
        };
        let mut nodes = Nodes::default();
        nodes.put(&space).unwrap();
        nodes.put(&index).unwrap();
        let parsed = Space::parse(&Object::from_bytes(&space), &space).unwrap();
        let ctx = parsed
            .admit(&Parameters::from([
                ("language".into(), "en".into()),
                ("style".into(), "painted".into()),
            ]))
            .unwrap();
        nodes.put(ctx.bytes()).unwrap();
        let context = source
            .freeze_production_context(
                &query(&source, "inputs/rows"),
                &profiles,
                &descriptor,
                &mut nodes,
                TreeLimits::default(),
            )
            .unwrap();
        Self {
            f,
            source,
            profiles,
            plan,
            context,
            policies: vec![Policy {
                project_space: admission.space.clone(),
                asset: admission,
            }],
            requests: vec![Request {
                record: selection.record().clone(),
                index: Object::from_bytes(&index),
                context: ctx.object().clone(),
            }],
            nodes,
        }
    }
    fn freeze(&mut self) -> Result<variants::Packet, Error> {
        variants::freeze(
            &mut variants::Admission {
                source: &mut self.source,
                context: &self.context,
                profiles: &self.profiles,
                policies: &self.policies,
                requests: &self.requests,
                query_limits: query_limits(),
            },
            &mut self.nodes,
            TreeLimits::default(),
        )
    }
}

#[test]
fn retained_variant_job_reopens_without_git_and_keeps_controls_out_of_primary() {
    let mut c = Case::new();
    let packet = c.freeze().unwrap();
    let runtime = BTreeMap::new();
    let admission = job::Admission {
        context: &c.context,
        plan: &c.plan,
        runtime: &runtime,
        output: None,
    };
    let job = job::freeze_with_variants(&admission, &packet, &mut c.nodes, TreeLimits::default())
        .unwrap();
    assert_eq!(
        job.of(Role::Primary),
        &BTreeSet::from([Object::from_bytes(b"en1")])
    );
    for object in packet.objects() {
        assert!(job.of(Role::Interpretation).contains(object));
    }
    let entries = Tree::from_root(job.selection().root.clone())
        .entries(&mut c.nodes, TreeLimits::default())
        .unwrap()
        .0;
    assert_eq!(entries.len(), 5);
    assert_eq!(
        entries[&LogicalPath::parse("source/variants").unwrap()],
        packet.selection().root
    );
    assert!(
        job::restore(
            job.selection(),
            &admission,
            &mut c.nodes,
            TreeLimits::default()
        )
        .is_err()
    );
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("metadata");
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
    for (object, raw) in c.nodes.0 {
        assert_eq!(store.put(&raw).unwrap(), object);
    }
    let witness = store.finish().unwrap();
    drop(store);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            c.f.root().join(name),
            c.f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut store = MetadataReader::open(&path, &witness).unwrap();
    let (mut source, context) = AssetSnapshot::from_production_context(
        c.context.selection(),
        &c.profiles,
        &mut store,
        Limits::default(),
        TreeLimits::default(),
        query_limits(),
    )
    .unwrap();
    let restored = variants::restore(
        packet.selection(),
        &mut variants::Admission {
            source: &mut source,
            context: &context,
            profiles: &c.profiles,
            policies: &c.policies,
            requests: &c.requests,
            query_limits: query_limits(),
        },
        &mut store,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(restored.objects(), packet.objects());
    assert_eq!(
        restored.selections()[&c.requests[0].record].subject(),
        packet.selections()[&c.requests[0].record].subject()
    );
    let replay = job::restore_with_variants(
        job.selection(),
        &job::Admission {
            context: &context,
            plan: &c.plan,
            runtime: &runtime,
            output: None,
        },
        &restored,
        &mut store,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(job.objects(), replay.objects());
    assert!(!replay.complete_claim_coverage());
    use git_asset_store::{
        Key,
        source_claims::{Command, SourceJournal},
    };
    let key = |s| Key::parse(s).unwrap();
    let journal_path = temp.path().join("sources");
    let mut journal = SourceJournal::create(&journal_path, key("fixture"), key("epoch")).unwrap();
    let pending = journal
        .apply(
            key("prepare"),
            Command::Prepare {
                source: key("job"),
                owner: key("fixture"),
                expected_revision: 0,
                selection: replay.selection().clone(),
            },
        )
        .unwrap();
    journal
        .apply(
            key("adopt"),
            Command::Adopt {
                source: key("job"),
                owner: key("fixture"),
                expected_revision: 1,
                prepared: pending.reference,
            },
        )
        .unwrap();
    let checkpoint = journal.checkpoint();
    drop(journal);
    let mut journal = SourceJournal::open(&journal_path, &checkpoint).unwrap();
    assert!(
        journal
            .obligations(10)
            .unwrap()
            .iter()
            .any(|o| o.selection == *replay.selection())
    );
}

#[test]
fn composition_dependencies_coexist_with_variants_and_empty_maps_preserve_v2_identity() {
    let mut c = Case::new();
    let packet = c.freeze().unwrap();
    let empty = BTreeMap::new();
    let admission = job::Admission {
        context: &c.context,
        plan: &c.plan,
        runtime: &empty,
        output: None,
    };
    let legacy =
        job::freeze_with_variants(&admission, &packet, &mut c.nodes, TreeLimits::default())
            .unwrap();
    let none = job::Dependencies {
        composition: &empty,
        evidence: &empty,
    };
    let identical = job::freeze_with_dependencies(
        &admission,
        &none,
        Some(&packet),
        &mut c.nodes,
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(legacy.selection(), identical.selection());
    let component = Object::from_bytes(b"independently resolved component");
    let composition =
        BTreeMap::from([(LogicalPath::parse("component").unwrap(), component.clone())]);
    let dependencies = job::Dependencies {
        composition: &composition,
        evidence: &empty,
    };
    let candidate = job::freeze_with_dependencies(
        &admission,
        &dependencies,
        Some(&packet),
        &mut c.nodes,
        TreeLimits::default(),
    )
    .unwrap();
    let restored = job::restore_with_dependencies(
        candidate.selection(),
        &admission,
        &dependencies,
        Some(&packet),
        &mut c.nodes,
        TreeLimits::default(),
    )
    .unwrap();
    assert!(restored.of(Role::Composition).contains(&component));
    assert!(!restored.of(Role::Primary).contains(&component));
    assert!(
        packet
            .objects()
            .iter()
            .all(|o| restored.of(Role::Interpretation).contains(o))
    );
    assert!(
        job::restore_with_variants(
            candidate.selection(),
            &admission,
            &packet,
            &mut c.nodes,
            TreeLimits::default()
        )
        .is_err()
    );
    assert!(
        job::restore_with_dependencies(
            candidate.selection(),
            &admission,
            &dependencies,
            None,
            &mut c.nodes,
            TreeLimits::default()
        )
        .is_err()
    );
}

#[test]
fn retained_variants_reject_missing_duplicate_foreign_and_unadmitted_requests() {
    let mut c = Case::new();
    let old = c.requests[0].record.clone();
    c.requests[0].record.id = "foreign".into();
    assert!(c.freeze().is_err());
    c.requests[0].record = old;
    c.requests.push(Request {
        record: c.requests[0].record.clone(),
        index: c.requests[0].index.clone(),
        context: c.requests[0].context.clone(),
    });
    assert!(c.freeze().is_err());
    c.requests.pop();
    let old = c.policies[0].asset.semantics.clone();
    c.policies[0].asset.semantics = git_asset_core::Digest::blob(b"unadmitted");
    assert!(c.freeze().is_err());
    c.policies[0].asset.semantics = old;
    let pin = c.requests[0].context.clone();
    let raw = c.nodes.0.remove(&pin).unwrap();
    assert!(c.freeze().is_err());
    c.nodes.0.insert(pin.clone(), b"bad".to_vec());
    assert!(c.freeze().is_err());
    c.nodes.0.insert(pin, raw);
    assert!(c.freeze().is_ok());
    c.requests.clear();
    assert!(c.freeze().is_err());
}

#[test]
fn retained_variants_require_exact_primary_selection_not_all_languages() {
    let mut c = Case::new();
    c.profiles[0].payloads = Payloads::Named {
        selected: false,
        slots: BTreeSet::from(["en".into(), "es".into()]),
    };
    c.context = c
        .source
        .freeze_production_context(
            &query(&c.source, "inputs/rows"),
            &c.profiles,
            b"{}",
            &mut c.nodes,
            TreeLimits::default(),
        )
        .unwrap();
    assert!(c.freeze().is_err());
}

#[test]
fn full_variant_context_changes_job_provenance_without_changing_projected_recipe() {
    let mut c = Case::new();
    let before = c.freeze().unwrap();
    let space_pin = &c.policies[0].project_space;
    let raw = c.nodes.get(space_pin).unwrap().unwrap();
    let space = Space::parse(space_pin, &raw).unwrap();
    let ctx = space
        .admit(&Parameters::from([
            ("language".into(), "en".into()),
            ("style".into(), "realistic".into()),
        ]))
        .unwrap();
    c.nodes.put(ctx.bytes()).unwrap();
    c.requests[0].context = ctx.object().clone();
    let after = c.freeze().unwrap();
    assert_ne!(before.selection(), after.selection());
    let a = &after.selections()[&c.requests[0].record];
    assert_eq!(
        before.selections()[&c.requests[0].record].subject(),
        a.subject()
    );
    assert_eq!(
        super::variant_cases::plan(&mut c.source, &c.f, a).fingerprint(),
        c.plan.fingerprint()
    );
    let runtime = BTreeMap::new();
    let admission = job::Admission {
        context: &c.context,
        plan: &c.plan,
        runtime: &runtime,
        output: None,
    };
    let first = job::freeze_with_variants(&admission, &before, &mut c.nodes, TreeLimits::default())
        .unwrap();
    let second =
        job::freeze_with_variants(&admission, &after, &mut c.nodes, TreeLimits::default()).unwrap();
    assert_ne!(first.selection(), second.selection());
    assert!(
        job::restore_with_variants(
            first.selection(),
            &admission,
            &after,
            &mut c.nodes,
            TreeLimits::default()
        )
        .is_err()
    );
}

#[test]
fn retained_variant_packet_refuses_foreign_context_and_missing_witness_on_replay() {
    let mut c = Case::new();
    let packet = c.freeze().unwrap();
    let entries = Tree::from_root(packet.selection().root.clone())
        .entries(&mut c.nodes, TreeLimits::default())
        .unwrap()
        .0;
    let object = entries
        .iter()
        .find(|(k, _)| k.components().next() == Some("witness"))
        .unwrap()
        .1;
    let raw = c.nodes.0.remove(object).unwrap();
    assert!(
        variants::restore(
            packet.selection(),
            &mut variants::Admission {
                source: &mut c.source,
                context: &c.context,
                profiles: &c.profiles,
                policies: &c.policies,
                requests: &c.requests,
                query_limits: query_limits()
            },
            &mut c.nodes,
            TreeLimits::default()
        )
        .is_err()
    );
    c.nodes.0.insert(object.clone(), raw);
    let other = c
        .source
        .freeze_production_context(
            &query(&c.source, "inputs/rows"),
            &c.profiles,
            b"other operation",
            &mut c.nodes,
            TreeLimits::default(),
        )
        .unwrap();
    let runtime = BTreeMap::new();
    assert!(
        job::freeze_with_variants(
            &job::Admission {
                context: &other,
                plan: &c.plan,
                runtime: &runtime,
                output: None
            },
            &packet,
            &mut c.nodes,
            TreeLimits::default()
        )
        .is_err()
    );
}

#[test]
fn retained_variants_use_frozen_record_not_newer_selected_id() {
    let mut c = Case::new();
    let old = capture(&reader(c.f.root()), head(c.f.root()));
    let path = c.f.root().join("records/cue.asset");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["streams"][0]["object"] = json!(Object::from_bytes(b"en2"));
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    let mut current = capture(&reader(c.f.root()), commit(c.f.root()));
    current.retain(old).unwrap();
    assert_eq!(
        current
            .resolve_id("inputs/rows")
            .unwrap()
            .stream("en")
            .unwrap(),
        &Object::from_bytes(b"en2")
    );
    let query = graph::Query {
        checkpoint: current.checkpoint().clone(),
        roots: BTreeSet::from([c.requests[0].record.clone()]),
        kinds: BTreeSet::from([EdgeKind::Production]),
        limits: query_limits(),
    };
    c.context = current
        .freeze_production_context(
            &query,
            &c.profiles,
            b"{}",
            &mut c.nodes,
            TreeLimits::default(),
        )
        .unwrap();
    c.source = current;
    let packet = c.freeze().unwrap();
    let selected = &packet.selections()[&c.requests[0].record];
    assert_eq!(selected.object(), &Object::from_bytes(b"en1"));
    assert_eq!(selected.checkpoint(), c.source.checkpoint());
}

#[test]
fn retained_variant_budgets_and_conflicting_sizes_refuse_before_metadata_writes() {
    let mut c = Case::new();
    let count = c.nodes.0.len();
    let old = c.requests[0].context.clone();
    c.requests[0].context = c.policies[0].project_space.clone();
    c.requests[0].context.bytes += 1;
    assert!(c.freeze().is_err());
    assert_eq!(count, c.nodes.0.len());
    c.requests[0].context = old;
    for limits in [
        TreeLimits {
            max_entries: 1,
            ..TreeLimits::default()
        },
        TreeLimits {
            max_read_bytes: 1,
            ..TreeLimits::default()
        },
        TreeLimits {
            max_writes: 0,
            ..TreeLimits::default()
        },
        TreeLimits {
            max_write_bytes: 1,
            ..TreeLimits::default()
        },
    ] {
        assert!(
            variants::freeze(
                &mut variants::Admission {
                    source: &mut c.source,
                    context: &c.context,
                    profiles: &c.profiles,
                    policies: &c.policies,
                    requests: &c.requests,
                    query_limits: query_limits()
                },
                &mut c.nodes,
                limits
            )
            .is_err()
        );
        assert_eq!(count, c.nodes.0.len());
    }
    assert!(c.freeze().is_ok());
}

#[test]
fn variant_coverage_is_per_record_even_when_two_records_share_the_same_audio() {
    let mut c = Case::new();
    let mut index: Value =
        serde_json::from_slice(&c.nodes.get(&c.requests[0].index).unwrap().unwrap()).unwrap();
    index["asset"] = "inputs/two".into();
    let index_pin = c.nodes.put(&serde_json::to_vec(&index).unwrap()).unwrap();
    let mut cue: Value =
        serde_json::from_slice(&fs::read(c.f.root().join("records/cue.asset")).unwrap()).unwrap();
    cue["id"] = "inputs/two".into();
    cue["selected"] = json!(index_pin);
    fs::write(
        c.f.root().join("records/cue2.asset"),
        serde_json::to_vec(&cue).unwrap(),
    )
    .unwrap();
    c.f.record(
        "scene",
        "scene/one",
        false,
        json!([{"id":"inputs/rows"},{"id":"inputs/two"}]),
        None,
    );
    c.source = capture(&reader(c.f.root()), commit(c.f.root()));
    c.context = c
        .source
        .freeze_production_context(
            &query(&c.source, "scene/one"),
            &c.profiles,
            b"{}",
            &mut c.nodes,
            TreeLimits::default(),
        )
        .unwrap();
    assert_eq!(c.context.requirements().primary().len(), 1);
    let count = c.nodes.0.len();
    assert!(c.freeze().is_err());
    assert_eq!(c.nodes.0.len(), count);
    c.requests.push(Request {
        record: c
            .source
            .resolve_id("inputs/two")
            .unwrap()
            .reference()
            .clone(),
        index: index_pin,
        context: c.requests[0].context.clone(),
    });
    let packet = c.freeze().unwrap();
    assert_eq!(packet.selections().len(), 2);
    assert_eq!(
        packet
            .selections()
            .values()
            .map(|s| s.object())
            .collect::<BTreeSet<_>>()
            .len(),
        1
    );
}

#[test]
fn packet_replay_is_read_only_and_rejects_extra_entries_and_changed_policy() {
    let mut c = Case::new();
    let packet = c.freeze().unwrap();
    let mut entries = Tree::from_root(packet.selection().root.clone())
        .entries(&mut c.nodes, TreeLimits::default())
        .unwrap()
        .0;
    entries.insert(
        LogicalPath::parse("unexpected/object").unwrap(),
        Object::from_bytes(b"unexpected"),
    );
    let foreign = Tree::build(&entries, &mut c.nodes, TreeLimits::default())
        .unwrap()
        .0;
    let wrong = git_asset_store::source_claims::Selection {
        root: foreign.root().clone(),
        context: packet.selection().context.clone(),
    };
    let mut restore = |selection: &git_asset_store::source_claims::Selection,
                       policies: &[Policy]| {
        variants::restore(
            selection,
            &mut variants::Admission {
                source: &mut c.source,
                context: &c.context,
                profiles: &c.profiles,
                policies,
                requests: &c.requests,
                query_limits: query_limits(),
            },
            &mut c.nodes,
            TreeLimits {
                max_writes: 0,
                max_write_bytes: 0,
                ..TreeLimits::default()
            },
        )
    };
    assert!(restore(&wrong, &c.policies).is_err());
    assert!(restore(packet.selection(), &c.policies).is_ok());
    c.policies[0].asset.uses.clear();
    assert!(restore(packet.selection(), &c.policies).is_err());
}
