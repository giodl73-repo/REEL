use git_asset_core::{Digest, Object};
use git_asset_model::graph::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[path = "graph/payload_consumption_cases.rs"]
mod payload_consumption_cases;
#[path = "graph/produced_cases.rs"]
mod produced_cases;
#[path = "graph/recipe_cases.rs"]
mod recipe_cases;

fn bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}
fn pin(r: &Reference) -> Value {
    json!({"id":r.id,"sha256":r.object.sha256,"bytes":r.object.bytes})
}

type RecordBytes = (Vec<u8>, Vec<u8>, Vec<u8>);

#[derive(Clone)]
struct Fixture {
    checkpoint: Digest,
    records: BTreeMap<Reference, RecordBytes>,
    definitions: Vec<(NodeKind, String, Vec<u8>, Vec<u8>)>,
    profiles: Vec<Profile>,
    calls: BTreeMap<Reference, usize>,
    selections: BTreeMap<String, Reference>,
    selection_calls: BTreeMap<String, usize>,
}

impl Fixture {
    fn new() -> Self {
        let mut this = Self {
            checkpoint: Digest::blob(b"snapshot"),
            records: BTreeMap::new(),
            definitions: vec![],
            profiles: vec![],
            calls: BTreeMap::new(),
            selections: BTreeMap::new(),
            selection_calls: BTreeMap::new(),
        };
        for (class, name) in [
            (NodeKind::Primary, "primary"),
            (NodeKind::Output, "output"),
            (NodeKind::Container, "container"),
        ] {
            let id = format!("types/{name}");
            let schema = bytes(&json!({"type":"object"}));
            let def = bytes(&json!({"schema":"tig.definition.design-1","id":id,
                "authored_schema":Object::from_bytes(&schema),
                "authoring":{"properties":["deps"],"required":["deps"],"defaults":{}},
                "stream_rules":{},"semantic_rules":{}}));
            this.profiles.push(Profile {
                definition_id: id.clone(),
                definition: Object::from_bytes(&def),
                class,
                payloads: if class == NodeKind::Primary {
                    Payloads::SelectedObject
                } else {
                    Payloads::None
                },
                covered: BTreeSet::from([EdgeKind::Production]),
                rules: vec![Rule {
                    pointer: "/deps".into(),
                    kind: EdgeKind::Production,
                    collection: Collection::Array,
                    optional: false,
                    binding: Binding::Either,
                }],
            });
            this.definitions.push((class, id, def, schema));
        }
        this
    }

    fn node(
        &mut self,
        id: &str,
        class: NodeKind,
        deps: &[Reference],
        payload: Option<&[u8]>,
    ) -> Reference {
        let (_, def_id, def, schema) = self.definitions.iter().find(|x| x.0 == class).unwrap();
        let mut def_pin = serde_json::to_value(Object::from_bytes(def)).unwrap();
        def_pin["id"] = json!(def_id);
        let record = bytes(
            &json!({"schema":"tig.instance.design-1","id":id,"definition":def_pin,
            "authored":{"deps":deps.iter().map(pin).collect::<Vec<_>>()},"streams":[],
            "selected":payload.map(Object::from_bytes),"status":{},"metadata":{}}),
        );
        let reference = Reference {
            id: id.into(),
            object: Object::from_bytes(&record),
        };
        self.records
            .insert(reference.clone(), (record, def.clone(), schema.clone()));
        self.selections.insert(id.into(), reference.clone());
        reference
    }

    fn rewrite(&mut self, reference: &Reference, edit: impl FnOnce(&mut Value)) -> Reference {
        let (raw, def, schema) = self.records[reference].clone();
        let mut v: Value = serde_json::from_slice(&raw).unwrap();
        edit(&mut v);
        let raw = bytes(&v);
        let new = Reference {
            id: reference.id.clone(),
            object: Object::from_bytes(&raw),
        };
        self.records.insert(new.clone(), (raw, def, schema));
        self.selections.insert(new.id.clone(), new.clone());
        new
    }

    fn query(&self, roots: &[Reference]) -> Query {
        Query {
            checkpoint: self.checkpoint.clone(),
            roots: roots.iter().cloned().collect(),
            kinds: BTreeSet::from([EdgeKind::Production]),
            limits: Limits {
                nodes: 100,
                edges: 100,
                depth: 20,
                bytes: 1_000_000,
            },
        }
    }

    fn run(&mut self, query: &Query) -> Graph {
        let profiles = self.profiles.clone();
        resolve(self, query, &profiles).unwrap()
    }
}

impl Source for Fixture {
    fn select(&mut self, id: &str, _: usize) -> SelectionLookup {
        *self.selection_calls.entry(id.into()).or_default() += 1;
        SelectionLookup {
            checkpoint: self.checkpoint.clone(),
            selection: self
                .selections
                .get(id)
                .cloned()
                .map(Selection::Found)
                .unwrap_or(Selection::Missing),
        }
    }
    fn lookup(&mut self, reference: &Reference, _: usize) -> Lookup {
        *self.calls.entry(reference.clone()).or_default() += 1;
        Lookup {
            checkpoint: self.checkpoint.clone(),
            data: match self.records.get(reference) {
                Some((i, d, s)) => Data::Record {
                    instance: i.clone(),
                    definition: d.clone(),
                    schema: s.clone(),
                },
                None => Data::Missing,
            },
        }
    }
}

fn has_gap(g: &Graph, reason: GapReason) -> bool {
    g.gaps().iter().any(|g| g.reason == reason)
}

#[test]
fn two_cels_share_a_cue_and_only_replaced_primary_bytes_become_candidates() {
    let mut f = Fixture::new();
    let cue = f.node("cues/1", NodeKind::Primary, &[], Some(b"cue-v1"));
    let a = f.node(
        "cels/a",
        NodeKind::Primary,
        std::slice::from_ref(&cue),
        Some(b"a-v1"),
    );
    let b = f.node(
        "cels/b",
        NodeKind::Primary,
        std::slice::from_ref(&cue),
        Some(b"b-v1"),
    );
    let root = f.node(
        "scenes/1",
        NodeKind::Output,
        &[a.clone(), b],
        Some(b"old-render"),
    );
    let old = f.run(&f.query(&[root]));
    assert!(old.complete_declared_coverage());
    assert!(!old.complete_semantic_coverage());
    assert_eq!(old.nodes().len(), 4);
    assert_eq!(old.edges().len(), 4);
    assert_eq!(f.calls[&cue], 1);
    let b2 = f.node(
        "cels/b",
        NodeKind::Primary,
        std::slice::from_ref(&cue),
        Some(b"b-v2"),
    );
    let root2 = f.node("scenes/1", NodeKind::Output, &[a, b2], Some(b"old-render"));
    f.checkpoint = Digest::blob(b"new-snapshot");
    let new = f.run(&f.query(&[root2]));
    let delta = primary_change(&old, &new).unwrap();
    assert_eq!(delta.added, BTreeSet::from([Object::from_bytes(b"b-v2")]));
    assert_eq!(
        delta.shared,
        BTreeSet::from([Object::from_bytes(b"a-v1"), Object::from_bytes(b"cue-v1")])
    );
    assert_eq!(
        delta.unreferenced_by_new_query,
        BTreeSet::from([Object::from_bytes(b"b-v1")])
    );
    // Keeping a stale last render is valid source state; no freshness is asserted.
    assert!(!new.complete_semantic_coverage());
}

#[test]
fn another_current_root_keeps_an_old_primary_selected() {
    let mut f = Fixture::new();
    let a = f.node("picture/a", NodeKind::Primary, &[], Some(b"a"));
    let b = f.node("picture/b", NodeKind::Primary, &[], Some(b"b"));
    let root = f.node(
        "output/1",
        NodeKind::Output,
        std::slice::from_ref(&a),
        Some(b"one"),
    );
    let shared = f.node("output/2", NodeKind::Output, &[a], Some(b"two"));
    let old = f.run(&f.query(&[root, shared.clone()]));
    let root2 = f.node("output/1", NodeKind::Output, &[b], Some(b"one-new"));
    let new = f.run(&f.query(&[root2, shared]));
    assert!(
        primary_change(&old, &new)
            .unwrap()
            .unreferenced_by_new_query
            .is_empty()
    );
}

#[test]
fn nonmedia_invoice_uses_same_container_and_primary_reference_mechanism() {
    let mut f = Fixture::new();
    let data = f.node(
        "crm/accounts/42",
        NodeKind::Primary,
        &[],
        Some(b"account data"),
    );
    let plan = f.node("reports/month", NodeKind::Container, &[data], None);
    let pdf = f.node(
        "invoices/42",
        NodeKind::Output,
        &[plan],
        Some(b"invoice bytes"),
    );
    let graph = f.run(&f.query(&[pdf]));
    assert!(graph.complete_declared_coverage());
    assert_eq!(graph.nodes().len(), 3);
}

#[test]
fn output_as_production_input_is_a_gap_even_if_status_claims_primary() {
    let mut f = Fixture::new();
    let rendered = f.node("renders/old", NodeKind::Output, &[], Some(b"movie"));
    let disguised = f.rewrite(&rendered, |v| {
        v["status"] = json!({"class":"primary","approved":true});
    });
    let root = f.node(
        "renders/new",
        NodeKind::Output,
        &[disguised],
        Some(b"new movie"),
    );
    let graph = f.run(&f.query(&[root]));
    assert!(has_gap(&graph, GapReason::OutputAsPrimary));
    assert!(primary_change(&graph, &graph).is_err());
}

#[test]
fn composition_and_evidence_are_distinct_from_production() {
    for kind in [
        EdgeKind::Composition,
        EdgeKind::Evidence,
        EdgeKind::Authority,
        EdgeKind::Publication,
    ] {
        let mut f = Fixture::new();
        for p in &mut f.profiles {
            p.covered.insert(kind);
            p.rules[0].kind = kind;
        }
        let output = f.node("old/result", NodeKind::Output, &[], Some(b"old"));
        let root = f.node("new/result", NodeKind::Output, &[output], Some(b"new"));
        let mut query = f.query(&[root]);
        query.kinds = BTreeSet::from([kind]);
        let graph = f.run(&query);
        assert!(graph.complete_declared_coverage());
        assert!(primary_change(&graph, &graph).is_err());
    }
}

#[test]
fn retained_candidate_inventory_does_not_establish_production_coverage() {
    let mut f = Fixture::new();
    let root = f.node("inventories/retained", NodeKind::Container, &[], None);
    let p = f
        .profiles
        .iter_mut()
        .find(|p| p.class == NodeKind::Container)
        .unwrap();
    p.covered = BTreeSet::from([EdgeKind::Evidence]);
    p.rules[0].kind = EdgeKind::Evidence;
    let graph = f.run(&f.query(&[root]));
    assert!(has_gap(&graph, GapReason::UncoveredKind));
    assert!(primary_change(&graph, &graph).is_err());
}

#[test]
fn exact_missing_revision_never_uses_another_revision_with_the_same_id() {
    let mut f = Fixture::new();
    let old = f.node("cels/b", NodeKind::Primary, &[], Some(b"old"));
    let _new = f.node("cels/b", NodeKind::Primary, &[], Some(b"new"));
    let root = f.node(
        "scenes/one",
        NodeKind::Output,
        std::slice::from_ref(&old),
        None,
    );
    f.records.remove(&old);
    let graph = f.run(&f.query(&[root]));
    assert!(has_gap(&graph, GapReason::Missing));
    assert_eq!(graph.nodes().len(), 1);
}

#[test]
fn changed_checkpoint_and_wrong_raw_record_are_rejected() {
    let mut f = Fixture::new();
    let root = f.node("records/a", NodeKind::Primary, &[], Some(b"a"));
    let mut query = f.query(std::slice::from_ref(&root));
    query.checkpoint = Digest::blob(b"other snapshot");
    assert!(has_gap(&f.run(&query), GapReason::CheckpointChanged));
    f.records.get_mut(&root).unwrap().0.push(b' ');
    assert!(has_gap(&f.run(&f.query(&[root])), GapReason::InvalidRecord));
}

#[test]
fn unadmitted_definition_and_changed_schema_cannot_weaken_rules() {
    let mut f = Fixture::new();
    let root = f.node("records/a", NodeKind::Primary, &[], Some(b"a"));
    let query = f.query(std::slice::from_ref(&root));
    assert!(has_gap(
        &resolve(&mut f, &query, &[]).unwrap(),
        GapReason::UnadmittedDefinition
    ));
    f.records.get_mut(&root).unwrap().2.push(b' ');
    assert!(has_gap(&f.run(&query), GapReason::InvalidRecord));
}

#[test]
fn missing_null_wrong_array_and_malformed_reference_are_not_empty_dependencies() {
    for bad in [
        Value::Null,
        json!({}),
        json!([{"id":null}]),
        json!([{"id":"x","sha256":"bad","bytes":1}]),
    ] {
        let mut f = Fixture::new();
        let node = f.node("records/a", NodeKind::Container, &[], None);
        let changed = f.rewrite(&node, |v| {
            v["authored"]["deps"] = bad;
        });
        assert!(has_gap(
            &f.run(&f.query(&[changed])),
            GapReason::InvalidReference
        ));
    }
}

#[test]
fn escaped_pointer_and_single_reference_are_resolved_exactly() {
    let mut f = Fixture::new();
    let leaf = f.node("records/leaf", NodeKind::Primary, &[], Some(b"data"));
    let node = f.node("records/root", NodeKind::Output, &[], None);
    let changed = f.rewrite(&node, |v| {
        v["authored"]["deps"] = json!({"x/y~z":pin(&leaf)});
    });
    let p = f
        .profiles
        .iter_mut()
        .find(|p| p.class == NodeKind::Output)
        .unwrap();
    p.rules[0].pointer = "/deps/x~1y~0z".into();
    p.rules[0].collection = Collection::One;
    assert!(f.run(&f.query(&[changed])).complete_declared_coverage());
}

#[test]
fn bad_or_duplicate_profile_paths_and_invalid_limits_refuse_before_source_use() {
    for path in ["deps", "/deps/~2", "/deps/~"] {
        let mut f = Fixture::new();
        let node = f.node("records/a", NodeKind::Primary, &[], None);
        f.profiles[0].rules[0].pointer = path.into();
        // Profile syntax is rejected even for records of another class.
        let profiles = f.profiles.clone();
        let query = f.query(&[node]);
        assert!(resolve(&mut f, &query, &profiles).is_err());
        assert!(f.calls.is_empty());
    }
    let mut f = Fixture::new();
    let root = f.node("records/a", NodeKind::Primary, &[], None);
    let query = f.query(std::slice::from_ref(&root));
    let mut profiles = f.profiles.clone();
    let repeated = profiles[0].rules[0].clone();
    profiles[0].rules.push(repeated);
    assert!(resolve(&mut f, &query, &profiles).is_err());
    let mut profiles = f.profiles.clone();
    profiles.push(profiles[0].clone());
    assert!(resolve(&mut f, &query, &profiles).is_err());
    let mut query = f.query(&[root]);
    query.limits.nodes = 0;
    let profiles = f.profiles.clone();
    assert!(resolve(&mut f, &query, &profiles).is_err());
    assert!(f.calls.is_empty());
}

#[test]
fn node_edge_and_depth_limits_keep_partial_results_incomplete() {
    let mut f = Fixture::new();
    let a = f.node("inputs/a", NodeKind::Primary, &[], Some(b"a"));
    let b = f.node("inputs/b", NodeKind::Primary, &[], Some(b"b"));
    let container = f.node("plans/a", NodeKind::Container, &[a, b], None);
    let root = f.node("outputs/a", NodeKind::Output, &[container], None);
    for reason in [GapReason::Nodes, GapReason::Edges, GapReason::Depth] {
        let mut query = f.query(std::slice::from_ref(&root));
        match reason {
            GapReason::Nodes => query.limits.nodes = 2,
            GapReason::Edges => query.limits.edges = 1,
            GapReason::Depth => query.limits.depth = 1,
            _ => unreachable!(),
        }
        let graph = f.run(&query);
        assert!(has_gap(&graph, reason));
        assert!(primary_change(&graph, &graph).is_err());
    }
}

#[test]
fn byte_budget_accounts_even_rejected_provider_data_without_underflow() {
    let mut f = Fixture::new();
    let root = f.node("records/a", NodeKind::Primary, &[], None);
    let mut query = f.query(std::slice::from_ref(&root));
    query.limits.bytes = 1;
    let graph = f.run(&query);
    assert!(has_gap(&graph, GapReason::Bytes));
    assert!(graph.bytes_read() > 1);
    query.limits.bytes = 1_000_000;
    f.records.get_mut(&root).unwrap().0 = vec![b' '; 65_537];
    assert!(has_gap(&f.run(&query), GapReason::Bytes));
}

#[test]
fn selected_payload_size_conflicts_across_records_block_comparison() {
    let mut f = Fixture::new();
    let a = f.node("inputs/a", NodeKind::Primary, &[], Some(b"same"));
    let b = f.node("inputs/b", NodeKind::Primary, &[], Some(b"same"));
    let wrong = f.rewrite(&b, |v| {
        v["selected"]["bytes"] = json!(999);
    });
    let root = f.node("outputs/a", NodeKind::Output, &[a, wrong], None);
    let graph = f.run(&f.query(&[root]));
    assert!(has_gap(&graph, GapReason::ConflictingSizes));
    assert!(primary_change(&graph, &graph).is_err());
}

#[test]
fn planned_primary_without_selected_bytes_is_not_a_complete_payload_inventory() {
    let mut f = Fixture::new();
    let root = f.node("inputs/planned", NodeKind::Primary, &[], None);
    let graph = f.run(&f.query(&[root]));
    assert!(graph.complete_declared_coverage());
    assert!(primary_change(&graph, &graph).is_err());
}

#[test]
fn denied_unavailable_and_unsupported_preserve_distinct_gaps() {
    struct Failure {
        checkpoint: Digest,
        kind: usize,
    }
    impl Source for Failure {
        fn select(&mut self, _: &str, _: usize) -> SelectionLookup {
            SelectionLookup {
                checkpoint: self.checkpoint.clone(),
                selection: Selection::Unsupported,
            }
        }
        fn lookup(&mut self, _: &Reference, _: usize) -> Lookup {
            Lookup {
                checkpoint: self.checkpoint.clone(),
                data: match self.kind {
                    0 => Data::Denied,
                    1 => Data::Unavailable,
                    _ => Data::Unsupported,
                },
            }
        }
    }
    let mut f = Fixture::new();
    let root = f.node("records/a", NodeKind::Primary, &[], None);
    let query = f.query(&[root]);
    for (kind, reason) in [
        GapReason::Denied,
        GapReason::Unavailable,
        GapReason::Unsupported,
    ]
    .into_iter()
    .enumerate()
    {
        let graph = resolve(
            &mut Failure {
                checkpoint: f.checkpoint.clone(),
                kind,
            },
            &query,
            &f.profiles,
        )
        .unwrap();
        assert!(has_gap(&graph, reason));
    }
}

#[test]
fn changing_admitted_classification_requires_an_explicit_policy_comparison() {
    let mut f = Fixture::new();
    let root = f.node("inputs/a", NodeKind::Primary, &[], Some(b"data"));
    let query = f.query(&[root]);
    let old = f.run(&query);
    f.profiles[0].class = NodeKind::Container;
    let new = f.run(&query);
    assert!(old.complete_declared_coverage() && new.complete_declared_coverage());
    assert_ne!(old.admission(), new.admission());
    assert!(primary_change(&old, &new).is_err());
}

#[test]
fn admission_order_does_not_change_policy_identity() {
    let mut f = Fixture::new();
    let root = f.node("inputs/a", NodeKind::Primary, &[], Some(b"data"));
    let query = f.query(&[root]);
    let old = f.run(&query);
    f.profiles.reverse();
    let new = f.run(&query);
    assert_eq!(old.admission(), new.admission());
    assert_eq!(old.admission_document(), new.admission_document());
    assert_eq!(&Digest::blob(old.admission_document()), old.admission());
    assert_eq!(old.roots(), &query.roots);
    assert!(primary_change(&old, &new).unwrap().added.is_empty());
}

#[test]
fn malformed_target_extensions_and_oversized_instance_pins_are_refused() {
    let mut f = Fixture::new();
    let leaf = f.node("inputs/a", NodeKind::Primary, &[], Some(b"a"));
    let root = f.node("outputs/a", NodeKind::Output, &[leaf], None);
    for (key, value) in [
        ("bytes", json!(65_537)),
        ("id", json!("\n")),
        ("media_type", Value::Null),
    ] {
        let changed = f.rewrite(&root, |v| {
            v["authored"]["deps"][0][key] = value;
        });
        assert!(has_gap(
            &f.run(&f.query(&[changed])),
            GapReason::InvalidReference
        ));
    }
}

#[test]
fn explicit_stream_selector_counts_image_and_prompt_but_not_history_or_manifest() {
    let mut f = Fixture::new();
    let leaf = f.node("cels/a", NodeKind::Primary, &[], Some(b"result manifest"));
    let leaf=f.rewrite(&leaf,|v| {v["streams"]=json!([
        {"slot":"image","role":"image","group":"working","object":Object::from_bytes(b"picture")},
        {"slot":"prompt","role":"prompt","group":"working","object":Object::from_bytes(b"prompt")},
        {"slot":"old-image","role":"image","group":"history","object":Object::from_bytes(b"old picture")},
        {"slot":"review","role":"review","group":"working","object":Object::from_bytes(b"review")}
    ]);});
    f.profiles[0].payloads = Payloads::Streams {
        roles: BTreeSet::from(["image".into(), "prompt".into()]),
        groups: BTreeSet::from(["working".into()]),
        min: 2,
    };
    let graph = f.run(&f.query(&[leaf]));
    assert!(graph.complete_declared_coverage());
    let change = primary_change(&graph, &graph).unwrap();
    assert_eq!(
        change.shared,
        BTreeSet::from([
            Object::from_bytes(b"picture"),
            Object::from_bytes(b"prompt")
        ])
    );
    assert!(
        !change
            .shared
            .contains(&Object::from_bytes(b"result manifest"))
    );
}

#[test]
fn unresolved_payload_meaning_or_missing_required_stream_blocks_payload_comparison() {
    let mut f = Fixture::new();
    let root = f.node("cels/a", NodeKind::Primary, &[], Some(b"opaque manifest"));
    for selector in [
        Payloads::Unsupported,
        Payloads::Streams {
            roles: BTreeSet::from(["image".into()]),
            groups: BTreeSet::from(["working".into()]),
            min: 1,
        },
    ] {
        f.profiles[0].payloads = selector;
        let graph = f.run(&f.query(std::slice::from_ref(&root)));
        assert!(graph.complete_declared_coverage());
        assert!(primary_change(&graph, &graph).is_err());
    }
    f.profiles[0].payloads = Payloads::None;
    let graph = f.run(&f.query(&[root]));
    assert!(primary_change(&graph, &graph).unwrap().shared.is_empty());
}

#[test]
fn thousand_primary_inputs_use_bounded_hierarchical_records_and_shared_revisions() {
    let mut f = Fixture::new();
    let mut chunks = Vec::new();
    for group in 0..10 {
        let mut leaves = Vec::new();
        for item in 0..100 {
            let name = format!("inputs/{group}-{item}");
            leaves.push(f.node(&name, NodeKind::Primary, &[], Some(name.as_bytes())));
        }
        chunks.push(f.node(
            &format!("groups/{group}"),
            NodeKind::Container,
            &leaves,
            None,
        ));
    }
    let root = f.node("outputs/collection", NodeKind::Output, &chunks, None);
    let mut query = f.query(&[root]);
    query.limits = Limits {
        nodes: 1100,
        edges: 1100,
        depth: 4,
        bytes: 4_000_000,
    };
    let graph = f.run(&query);
    assert!(graph.complete_declared_coverage());
    assert_eq!(graph.nodes().len(), 1011);
    assert_eq!(graph.edges().len(), 1010);
    assert!(graph.bytes_read() <= query.limits.bytes);
    assert!(f.calls.values().all(|count| *count == 1));
    assert_eq!(primary_change(&graph, &graph).unwrap().shared.len(), 1000);
}

#[test]
fn snapshot_selected_cel_changes_without_rewriting_parent_and_old_snapshot_restores() {
    let mut f = Fixture::new();
    let old = f.node("cels/b", NodeKind::Primary, &[], Some(b"v1"));
    let parent = f.node("scenes/1", NodeKind::Output, &[], Some(b"last render"));
    let parent = f.rewrite(&parent, |v| {
        v["authored"]["deps"] = json!([{"id":"cels/b"}]);
    });
    let query = f.query(std::slice::from_ref(&parent));
    let mut historical = f.clone();
    let before = f.run(&query);
    let new = f.node("cels/b", NodeKind::Primary, &[], Some(b"v2"));
    f.checkpoint = Digest::blob(b"day two");
    let after = f.run(&f.query(std::slice::from_ref(&parent)));
    assert_eq!(before.roots(), after.roots()); // Parent .tig bytes are unchanged.
    assert_ne!(before.binding_digest(), after.binding_digest());
    assert!(after.edges()[0].snapshot_selected);
    assert_eq!(before.edges()[0].target, old);
    assert_eq!(after.edges()[0].target, new);
    let delta = primary_change(&before, &after).unwrap();
    assert_eq!(delta.added, BTreeSet::from([Object::from_bytes(b"v2")]));
    assert_eq!(
        delta.unreferenced_by_new_query,
        BTreeSet::from([Object::from_bytes(b"v1")])
    );
    let restored = historical.run(&query);
    assert_eq!(restored.edges(), before.edges());
    assert!(has_gap(&f.run(&query), GapReason::CheckpointChanged));
}

#[test]
fn frozen_exact_reference_ignores_newer_snapshot_selection() {
    let mut f = Fixture::new();
    let old = f.node("cels/b", NodeKind::Primary, &[], Some(b"v1"));
    let parent = f.node(
        "scenes/1",
        NodeKind::Output,
        std::slice::from_ref(&old),
        None,
    );
    f.node("cels/b", NodeKind::Primary, &[], Some(b"v2"));
    f.checkpoint = Digest::blob(b"day two");
    let graph = f.run(&f.query(&[parent]));
    assert!(graph.complete_declared_coverage());
    assert_eq!(graph.edges()[0].target, old);
    assert!(f.selection_calls.is_empty());
}

#[test]
fn shared_snapshot_selection_is_resolved_once_per_id_per_query() {
    let mut f = Fixture::new();
    let cue = f.node("cues/1", NodeKind::Primary, &[], Some(b"cue"));
    let mut cels = Vec::new();
    for id in ["cels/a", "cels/b"] {
        let cel = f.node(id, NodeKind::Primary, &[], Some(id.as_bytes()));
        cels.push(f.rewrite(&cel, |v| {
            v["authored"]["deps"] = json!([{"id":"cues/1"}]);
        }));
    }
    let parent = f.node("scenes/1", NodeKind::Output, &cels, None);
    let graph = f.run(&f.query(&[parent]));
    assert!(graph.complete_declared_coverage());
    assert_eq!(f.selection_calls["cues/1"], 1);
    assert_eq!(f.calls[&cue], 1);
    assert_eq!(graph.edges().len(), 4);
}

#[test]
fn missing_snapshot_selection_never_falls_back_to_an_available_old_pin() {
    let mut f = Fixture::new();
    f.node("cels/b", NodeKind::Primary, &[], Some(b"old"));
    let parent = f.node("scenes/1", NodeKind::Output, &[], None);
    let parent = f.rewrite(&parent, |v| {
        v["authored"]["deps"] = json!([{"id":"cels/b"}]);
    });
    f.selections.remove("cels/b");
    assert!(has_gap(&f.run(&f.query(&[parent])), GapReason::Missing));
}

#[test]
fn admitted_binding_mode_rejects_wrong_exact_or_selected_reference_form() {
    for binding in [Binding::ExactOnly, Binding::SnapshotSelected] {
        let mut f = Fixture::new();
        let leaf = f.node("cels/b", NodeKind::Primary, &[], Some(b"data"));
        let parent = f.node("scenes/1", NodeKind::Output, &[leaf], None);
        let parent = if matches!(binding, Binding::ExactOnly) {
            f.rewrite(&parent, |v| {
                v["authored"]["deps"] = json!([{"id":"cels/b"}]);
            })
        } else {
            parent
        };
        f.profiles
            .iter_mut()
            .find(|p| p.class == NodeKind::Output)
            .unwrap()
            .rules[0]
            .binding = binding;
        assert!(has_gap(
            &f.run(&f.query(&[parent])),
            GapReason::InvalidReference
        ));
    }
}

#[test]
fn unqueried_evidence_does_not_resolve_logical_selections() {
    let mut f = Fixture::new();
    let parent = f.node("scenes/1", NodeKind::Output, &[], None);
    let parent = f.rewrite(&parent, |v| {
        v["authored"]["deps"] = json!([{"id":"not-present"}]);
    });
    let p = f
        .profiles
        .iter_mut()
        .find(|p| p.class == NodeKind::Output)
        .unwrap();
    p.rules[0].kind = EdgeKind::Evidence;
    p.covered.insert(EdgeKind::Evidence);
    let graph = f.run(&f.query(&[parent]));
    assert!(graph.complete_declared_coverage());
    assert!(f.selection_calls.is_empty());
}

#[test]
fn selection_metadata_consumes_budget_before_loading_target_bytes() {
    let mut f = Fixture::new();
    let leaf = f.node("cels/b", NodeKind::Primary, &[], Some(b"data"));
    let parent = f.node("scenes/1", NodeKind::Output, &[], None);
    let parent = f.rewrite(&parent, |v| {
        v["authored"]["deps"] = json!([{"id":"cels/b"}]);
    });
    let data = &f.records[&parent];
    let mut query = f.query(std::slice::from_ref(&parent));
    query.limits.bytes = data.0.len() + data.1.len() + data.2.len() + 1;
    let graph = f.run(&query);
    assert!(has_gap(&graph, GapReason::Bytes));
    assert!(!f.calls.contains_key(&leaf));
}

#[test]
fn selected_reference_checkpoint_and_logical_id_are_verified() {
    struct Fault {
        source: Fixture,
        mode: usize,
    }
    impl Source for Fault {
        fn lookup(&mut self, r: &Reference, b: usize) -> Lookup {
            self.source.lookup(r, b)
        }
        fn select(&mut self, id: &str, b: usize) -> SelectionLookup {
            let mut result = self.source.select(id, b);
            if self.mode == 0 {
                result.checkpoint = Digest::blob(b"wrong view");
            } else if self.mode == 2 {
                result.selection = Selection::Ambiguous;
            } else if let Selection::Found(reference) = &mut result.selection {
                reference.id = "wrong-id".into();
            }
            result
        }
    }
    let mut f = Fixture::new();
    f.node("cels/b", NodeKind::Primary, &[], Some(b"data"));
    let parent = f.node("scenes/1", NodeKind::Output, &[], None);
    let parent = f.rewrite(&parent, |v| {
        v["authored"]["deps"] = json!([{"id":"cels/b"}]);
    });
    let query = f.query(&[parent]);
    let profiles = f.profiles.clone();
    for (mode, reason) in [
        (0, GapReason::CheckpointChanged),
        (1, GapReason::InvalidReference),
        (2, GapReason::AmbiguousSelection),
    ] {
        let graph = resolve(
            &mut Fault {
                source: f.clone(),
                mode,
            },
            &query,
            &profiles,
        )
        .unwrap();
        assert!(has_gap(&graph, reason));
    }
}

#[test]
fn production_cycle_through_snapshot_selected_ids_is_incomplete() {
    let mut f = Fixture::new();
    let a = f.node("inputs/a", NodeKind::Primary, &[], Some(b"a"));
    let a = f.rewrite(&a, |v| {
        v["authored"]["deps"] = json!([{"id":"inputs/b"}]);
    });
    let b = f.node("inputs/b", NodeKind::Primary, &[], Some(b"b"));
    f.rewrite(&b, |v| {
        v["authored"]["deps"] = json!([{"id":"inputs/a"}]);
    });
    let graph = f.run(&f.query(&[a]));
    assert!(has_gap(&graph, GapReason::Cycle));
    assert!(graph.binding_digest().is_none());
    assert!(primary_change(&graph, &graph).is_err());
}

#[test]
fn evidence_relationship_cycles_are_not_build_cycles() {
    let mut f = Fixture::new();
    let a = f.node("crm/a", NodeKind::Primary, &[], Some(b"a"));
    let a = f.rewrite(&a, |v| {
        v["authored"]["deps"] = json!([{"id":"crm/b"}]);
    });
    let b = f.node("crm/b", NodeKind::Primary, &[], Some(b"b"));
    f.rewrite(&b, |v| {
        v["authored"]["deps"] = json!([{"id":"crm/a"}]);
    });
    for p in &mut f.profiles {
        p.covered.insert(EdgeKind::Evidence);
        p.rules[0].kind = EdgeKind::Evidence;
    }
    let mut query = f.query(&[a]);
    query.kinds = BTreeSet::from([EdgeKind::Evidence]);
    let graph = f.run(&query);
    assert!(graph.complete_declared_coverage());
    assert!(graph.binding_digest().is_some());
}

#[test]
fn unrelated_git_snapshot_changes_do_not_change_resolved_binding_identity() {
    let mut f = Fixture::new();
    f.node("inputs/a", NodeKind::Primary, &[], Some(b"a"));
    let root = f.node("outputs/a", NodeKind::Output, &[], Some(b"result"));
    let root = f.rewrite(&root, |v| {
        v["authored"]["deps"] = json!([{"id":"inputs/a"}]);
    });
    let before = f.run(&f.query(std::slice::from_ref(&root)));
    f.node("unrelated/b", NodeKind::Primary, &[], Some(b"unrelated"));
    f.checkpoint = Digest::blob(b"unrelated commit");
    let after = f.run(&f.query(&[root]));
    assert_ne!(before.checkpoint(), after.checkpoint());
    assert_eq!(before.binding_digest(), after.binding_digest());
}

fn map_profile(f: &mut Fixture, class: NodeKind) {
    f.profiles
        .iter_mut()
        .find(|p| p.class == class)
        .unwrap()
        .rules[0]
        .collection = Collection::ObjectValues;
}

#[test]
fn map_keys_have_deterministic_escaped_pointers_and_shared_ids_resolve_once() {
    let mut f = Fixture::new();
    let leaf = f.node("inputs/shared", NodeKind::Primary, &[], Some(b"shared"));
    let root = f.node("container", NodeKind::Container, &[], None);
    let root = f.rewrite(&root, |v| {
        v["authored"]["deps"] = json!({
            "z":{"id":leaf.id}, "a/b~c":{"id":leaf.id}, "":{"id":leaf.id},
            "\u{10000}":{"id":leaf.id}, "\u{e000}":{"id":leaf.id}
        })
    });
    map_profile(&mut f, NodeKind::Container);
    let graph = f.run(&f.query(&[root]));
    assert!(graph.complete_declared_coverage());
    assert_eq!(
        graph
            .edges()
            .iter()
            .map(|e| e.pointer.as_str())
            .collect::<Vec<_>>(),
        [
            "/deps/",
            "/deps/a~1b~0c",
            "/deps/z",
            "/deps/\u{e000}",
            "/deps/\u{10000}"
        ]
    );
    assert!(graph.edges().iter().all(|e| e.target == leaf));
    assert_eq!(f.selection_calls["inputs/shared"], 1);
    assert_eq!(f.calls[&leaf], 1);
}

#[test]
fn map_shape_and_key_failures_cannot_become_empty_dependencies() {
    for bad in [
        json!([]),
        json!(null),
        json!({"a":null}),
        json!({"a":{"nested":{"id":"x"}}}),
        json!({"a":{"id":"x","bytes":1}}),
        json!({"a\n":{"id":"x"}}),
        json!({"x".repeat(257):{"id":"x"}}),
    ] {
        let mut f = Fixture::new();
        let root = f.node("root", NodeKind::Container, &[], None);
        let root = f.rewrite(&root, |v| v["authored"]["deps"] = bad);
        map_profile(&mut f, NodeKind::Container);
        assert!(has_gap(
            &f.run(&f.query(&[root])),
            GapReason::InvalidReference
        ));
        assert!(f.selection_calls.is_empty());
    }
    let mut f = Fixture::new();
    let root = f.node("empty", NodeKind::Container, &[], None);
    let root = f.rewrite(&root, |v| v["authored"]["deps"] = json!({}));
    // Map shape is not implicitly accepted by the old array interpretation.
    assert!(has_gap(
        &f.run(&f.query(std::slice::from_ref(&root))),
        GapReason::InvalidReference
    ));
    map_profile(&mut f, NodeKind::Container);
    let graph = f.run(&f.query(&[root]));
    assert!(graph.complete_declared_coverage() && graph.edges().is_empty());
}

#[test]
fn map_frozen_reference_keeps_old_bytes_when_current_leaf_changes() {
    let mut f = Fixture::new();
    let old = f.node("leaf", NodeKind::Primary, &[], Some(b"old bytes"));
    let root = f.node("root", NodeKind::Container, &[], None);
    let root = f.rewrite(&root, |v| {
        v["authored"]["deps"] = json!({"current":{"id":"leaf"},"frozen":pin(&old)})
    });
    map_profile(&mut f, NodeKind::Container);
    let before = f.run(&f.query(std::slice::from_ref(&root)));
    let next = f.node("leaf", NodeKind::Primary, &[], Some(b"new bytes"));
    f.checkpoint = Digest::blob(b"next map snapshot");
    let after = f.run(&f.query(std::slice::from_ref(&root)));
    assert!(after.complete_declared_coverage());
    assert_eq!(before.roots(), after.roots());
    assert_eq!(after.edges()[0].target, next);
    assert_eq!(after.edges()[1].target, old);
    let change = primary_change(&before, &after).unwrap();
    assert_eq!(
        change.added,
        BTreeSet::from([Object::from_bytes(b"new bytes")])
    );
    assert!(change.unreferenced_by_new_query.is_empty());
    let mut profile = f.profiles.clone();
    profile
        .iter_mut()
        .find(|p| p.class == NodeKind::Container)
        .unwrap()
        .rules[0]
        .binding = Binding::SnapshotSelected;
    let query = f.query(&[root]);
    assert!(has_gap(
        &resolve(&mut f, &query, &profile).unwrap(),
        GapReason::InvalidReference
    ));
}

#[test]
fn map_output_classification_and_edge_budgets_still_block_comparison() {
    let mut f = Fixture::new();
    let output = f.node("compiled", NodeKind::Output, &[], Some(b"render"));
    let root = f.node("root", NodeKind::Container, &[], None);
    let root = f.rewrite(&root, |v| {
        v["authored"]["deps"] = json!({"pretend-input":{"id":output.id}})
    });
    map_profile(&mut f, NodeKind::Container);
    assert!(has_gap(
        &f.run(&f.query(&[root])),
        GapReason::OutputAsPrimary
    ));
    let leaf = f.node("leaf", NodeKind::Primary, &[], Some(b"data"));
    let root = f.node("bounded", NodeKind::Container, &[], None);
    let root = f.rewrite(&root, |v| {
        v["authored"]["deps"] = json!({"a":{"id":leaf.id},"b":{"id":leaf.id}})
    });
    let mut query = f.query(&[root]);
    query.limits.edges = 1;
    f.selection_calls.clear();
    let graph = f.run(&query);
    assert!(has_gap(&graph, GapReason::Edges));
    assert!(primary_change(&graph, &graph).is_err());
    assert!(f.selection_calls.is_empty());
}

#[test]
fn map_collection_interpretation_changes_admission_identity_even_when_not_traversed() {
    let mut f = Fixture::new();
    let leaf = f.node("leaf", NodeKind::Primary, &[], Some(b"data"));
    let query = f.query(&[leaf]);
    let before = f.run(&query);
    map_profile(&mut f, NodeKind::Container);
    let after = f.run(&query);
    assert!(before.complete_declared_coverage() && after.complete_declared_coverage());
    assert_ne!(before.admission(), after.admission());
    assert!(primary_change(&before, &after).is_err());
}

#[test]
fn thousand_invoice_document_map_entries_keep_one_lookup_per_exact_revision() {
    let mut f = Fixture::new();
    let mut entries = serde_json::Map::new();
    for n in 0..1000u32 {
        let id = format!("document-{n:04}");
        f.node(&id, NodeKind::Primary, &[], Some(&n.to_be_bytes()));
        entries.insert(format!("attachment-{n:04}"), json!({"id":id}));
    }
    let root = f.node("invoice", NodeKind::Container, &[], None);
    let root = f.rewrite(&root, |v| v["authored"]["deps"] = Value::Object(entries));
    map_profile(&mut f, NodeKind::Container);
    let mut query = f.query(&[root]);
    query.limits.nodes = 1001;
    query.limits.edges = 1000;
    query.limits.bytes = 2_000_000;
    let graph = f.run(&query);
    assert!(graph.complete_declared_coverage());
    assert_eq!(graph.nodes().len(), 1001);
    assert_eq!(graph.edges().len(), 1000);
    assert_eq!(primary_change(&graph, &graph).unwrap().shared.len(), 1000);
    assert!(f.calls.values().all(|calls| *calls == 1));
    assert!(f.selection_calls.values().all(|calls| *calls == 1));
    assert!(graph.bytes_read() <= query.limits.bytes);
}

#[test]
fn production_requirements_include_primary_controls_and_exact_interpretation_documents() {
    let mut f = Fixture::new();
    let cue = f.node("cue/1", NodeKind::Primary, &[], Some(b"cue"));
    let a = f.node(
        "cel/a",
        NodeKind::Primary,
        std::slice::from_ref(&cue),
        Some(b"a"),
    );
    let b = f.node("cel/b", NodeKind::Primary, &[cue], Some(b"b"));
    let root = f.node(
        "scene/1",
        NodeKind::Container,
        &[a, b],
        Some(b"native scene"),
    );
    let root = f.rewrite(&root, |v| v["streams"] = json!([
        {"slot":"catalog","role":"control","group":"current","object":Object::from_bytes(b"catalog")},
        {"slot":"profile","role":"control","group":"current","object":Object::from_bytes(b"profile")},
        {"slot":"old-render","role":"output","group":"history","object":Object::from_bytes(b"old render")}
    ]));
    f.profiles[2].payloads = Payloads::Named {
        selected: true,
        slots: BTreeSet::from(["catalog".into(), "profile".into()]),
    };
    let graph = f.run(&f.query(&[root]));
    let requirements = graph.production_requirements().unwrap();
    assert_eq!(
        requirements.primary(),
        &BTreeSet::from([
            Object::from_bytes(b"a"),
            Object::from_bytes(b"b"),
            Object::from_bytes(b"cue")
        ])
    );
    assert_eq!(
        requirements.controls(),
        &BTreeSet::from([
            Object::from_bytes(b"native scene"),
            Object::from_bytes(b"catalog"),
            Object::from_bytes(b"profile")
        ])
    );
    let expected = graph
        .nodes()
        .keys()
        .flat_map(|r| {
            let (i, d, s) = &f.records[r];
            [
                Object::from_bytes(i),
                Object::from_bytes(d),
                Object::from_bytes(s),
            ]
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(requirements.metadata(), &expected);
    assert_eq!(expected.len(), 7);
    assert_eq!(requirements.checkpoint(), graph.checkpoint());
    assert_eq!(requirements.admission(), graph.admission());
    assert_eq!(Some(requirements.binding_digest()), graph.binding_digest());
    assert_eq!(requirements.all_objects().len(), 13);
    assert!(
        !requirements
            .all_objects()
            .contains(&Object::from_bytes(b"old render"))
    );
}

#[test]
fn missing_or_unsupported_container_payloads_are_not_complete_production_requirements() {
    let mut f = Fixture::new();
    let leaf = f.node("cel/a", NodeKind::Primary, &[], Some(b"a"));
    let root = f.node("scene/1", NodeKind::Container, &[leaf], None);
    for selector in [
        Payloads::SelectedObject,
        Payloads::Unsupported,
        Payloads::Named {
            selected: false,
            slots: BTreeSet::from(["absent".into()]),
        },
    ] {
        f.profiles[2].payloads = selector;
        let graph = f.run(&f.query(std::slice::from_ref(&root)));
        assert!(graph.complete_declared_coverage());
        assert!(primary_change(&graph, &graph).is_ok());
        assert!(graph.production_requirements().is_err());
    }
}

#[test]
fn output_root_payloads_cannot_become_production_inputs() {
    let mut f = Fixture::new();
    let leaf = f.node("cel/a", NodeKind::Primary, &[], Some(b"a"));
    let root = f.node(
        "delivery/1",
        NodeKind::Output,
        &[leaf],
        Some(b"compiled output"),
    );
    f.profiles[1].payloads = Payloads::SelectedObject;
    let graph = f.run(&f.query(std::slice::from_ref(&root)));
    assert!(graph.complete_declared_coverage());
    assert!(graph.production_requirements().is_err());
    f.profiles[1].payloads = Payloads::None;
    let graph = f.run(&f.query(&[root]));
    let requirements = graph.production_requirements().unwrap();
    assert_eq!(
        requirements.primary(),
        &BTreeSet::from([Object::from_bytes(b"a")])
    );
    assert!(requirements.controls().is_empty());
    assert!(
        !requirements
            .all_objects()
            .contains(&Object::from_bytes(b"compiled output"))
    );
}

#[test]
fn control_replacement_changes_full_requirements_without_creating_primary_changes() {
    let mut f = Fixture::new();
    let leaf = f.node(
        "invoice/rows",
        NodeKind::Primary,
        &[],
        Some(b"invoice rows"),
    );
    let root = f.node(
        "invoice/42",
        NodeKind::Container,
        &[leaf],
        Some(b"invoice spec v1"),
    );
    f.profiles[2].payloads = Payloads::Named {
        selected: true,
        slots: BTreeSet::new(),
    };
    let old = f.run(&f.query(std::slice::from_ref(&root)));
    let new_root = f.rewrite(&root, |v| {
        v["selected"] = json!(Object::from_bytes(b"invoice spec v2"))
    });
    let new = f.run(&f.query(&[new_root]));
    let change = primary_change(&old, &new).unwrap();
    assert!(change.added.is_empty() && change.unreferenced_by_new_query.is_empty());
    assert_eq!(
        change.shared,
        BTreeSet::from([Object::from_bytes(b"invoice rows")])
    );
    let before = old.production_requirements().unwrap();
    let after = new.production_requirements().unwrap();
    assert_ne!(before.controls(), after.controls());
    assert_ne!(before.metadata(), after.metadata());
    assert_eq!(before.primary(), after.primary());
}

#[test]
fn named_selector_validation_and_scoped_query_gaps_refuse_full_requirements() {
    let mut f = Fixture::new();
    let root = f.node("scene/1", NodeKind::Container, &[], Some(b"scene"));
    let q = f.query(std::slice::from_ref(&root));
    for selector in [
        Payloads::Named {
            selected: false,
            slots: BTreeSet::new(),
        },
        Payloads::Named {
            selected: true,
            slots: BTreeSet::from(["bad\nslot".into()]),
        },
        Payloads::Named {
            selected: true,
            slots: BTreeSet::from(["x".repeat(129)]),
        },
        Payloads::Named {
            selected: false,
            slots: (0..257).map(|n| format!("slot-{n}")).collect(),
        },
    ] {
        f.profiles[2].payloads = selector;
        let profiles = f.profiles.clone();
        assert!(resolve(&mut f, &q, &profiles).is_err());
    }
    f.profiles[2].payloads = Payloads::None;
    let mut q = q;
    q.kinds = BTreeSet::from([EdgeKind::Evidence]);
    assert!(f.run(&q).production_requirements().is_err());
    q.kinds = BTreeSet::from([EdgeKind::Production]);
    q.limits.bytes = 1;
    assert!(f.run(&q).production_requirements().is_err());
}

#[test]
fn contradictory_control_and_primary_sizes_invalidate_full_coverage() {
    let mut f = Fixture::new();
    let payload = Object::from_bytes(b"primary");
    let leaf = f.node("primary/1", NodeKind::Primary, &[], Some(b"primary"));
    let root = f.node("container/1", NodeKind::Container, &[leaf], None);
    let wrong = Object {
        bytes: payload.bytes + 1,
        ..payload
    };
    let root = f.rewrite(&root, |v| {
        v["streams"] = json!([
            {"slot":"control","role":"control","group":"current","object":wrong}
        ])
    });
    f.profiles[2].payloads = Payloads::Named {
        selected: false,
        slots: BTreeSet::from(["control".into()]),
    };
    let graph = f.run(&f.query(&[root]));
    assert!(has_gap(&graph, GapReason::ConflictingSizes));
    assert!(graph.production_requirements().is_err());
}
