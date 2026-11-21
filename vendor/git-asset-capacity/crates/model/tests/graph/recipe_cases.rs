use super::*;
use git_asset_model::graph::recipe::{self, Consumption, Limits as RecipeLimits, Operation, Plan};

fn fixture() -> Fixture {
    let mut f = Fixture::new();
    for ((_, _, definition, _), profile) in f.definitions.iter_mut().zip(&mut f.profiles) {
        let mut value: Value = serde_json::from_slice(definition).unwrap();
        value["authoring"]["properties"] = json!(["deps", "text", "note", "width"]);
        value["authoring"]["defaults"] = json!({"width":80});
        *definition = bytes(&value);
        profile.definition = Object::from_bytes(definition);
    }
    f
}
fn operation() -> Operation {
    Operation {
        name: "report".into(),
        semantics: Digest::blob(b"operation semantics"),
        producer: Object::from_bytes(b"producer build"),
        parameters: Object::from_bytes(b"{}"),
    }
}
fn policies(f: &Fixture, fields: &[&str]) -> Vec<Consumption> {
    f.profiles
        .iter()
        .map(|p| Consumption {
            definition_id: p.definition_id.clone(),
            definition: p.definition.clone(),
            fields: if p.class == NodeKind::Primary {
                fields.iter().map(|s| s.to_string()).collect()
            } else {
                BTreeSet::new()
            },
        })
        .collect()
}
fn plan(f: &mut Fixture, graph: &Graph, fields: &[&str]) -> Plan {
    recipe::plan(
        graph,
        f,
        &f.profiles.clone(),
        &policies(f, fields),
        &operation(),
        RecipeLimits {
            nodes: 2000,
            edges: 2000,
            bytes: 16 * 1024 * 1024,
        },
    )
    .unwrap()
}
fn run(f: &mut Fixture, roots: &[Reference], fields: &[&str]) -> (Graph, Plan) {
    let graph = f.run(&f.query(roots));
    let plan = plan(f, &graph, fields);
    (graph, plan)
}

#[test]
fn annotation_status_and_unconsumed_edits_change_provenance_without_recipe_churn() {
    let mut f = fixture();
    let leaf = f.node("rows/1", NodeKind::Primary, &[], Some(b"rows"));
    let leaf = f.rewrite(&leaf, |v| v["authored"]["text"] = json!("Hola"));
    let (_, before) = run(&mut f, std::slice::from_ref(&leaf), &["text"]);
    let changed = f.rewrite(&leaf, |v| {
        v["metadata"] = json!({"annotation":"private synthetic note"});
        v["status"] = json!({"review":"draft"});
        v["authored"]["note"] = json!("not consumed");
    });
    let (_, after) = run(&mut f, &[changed], &["text"]);
    assert_eq!(before.fingerprint(), after.fingerprint());
    assert_ne!(before.binding(), after.binding());
    assert!(!after.complete_semantic_coverage());
    let debug = format!("{after:?}");
    assert!(!debug.contains("private synthetic note") && !debug.contains("Hola"));
    let changed = f.rewrite(&leaf, |v| v["authored"]["text"] = json!("Hello"));
    let (_, after) = run(&mut f, &[changed], &["text"]);
    assert_ne!(before.fingerprint(), after.fingerprint());
}

#[test]
fn delivery_history_updates_do_not_dirty_a_selected_payload_recipe_but_new_cue_bytes_do() {
    let mut f = fixture();
    let leaf = f.node(
        "cue/shared",
        NodeKind::Primary,
        &[],
        Some(b"recorded cue A"),
    );
    let (_, before) = run(&mut f, std::slice::from_ref(&leaf), &[]);
    let delivered = f.rewrite(&leaf, |v| {
        v["metadata"]["delivery"] = json!({"schema":"tig.delivery-state.experimental-1",
            "policy_slot":"delivery-policy","history_slot":"delivery-history"});
        v["streams"] = json!([
            {"slot":"delivery-policy","role":"policy/delivery","group":"evidence","object":Object::from_bytes(b"admitted publishing policy")},
            {"slot":"delivery-history","role":"history/delivery","group":"evidence","object":Object::from_bytes(b"retained delivery history A")}
        ]);
    });
    let (_, with_receipt) = run(&mut f, std::slice::from_ref(&delivered), &[]);
    assert_eq!(before.fingerprint(), with_receipt.fingerprint());
    assert_ne!(before.binding(), with_receipt.binding());
    let republished = f.rewrite(&delivered, |v| {
        v["streams"][1]["object"] = json!(Object::from_bytes(b"new retained delivery history B"));
    });
    let (_, after_receipt) = run(&mut f, std::slice::from_ref(&republished), &[]);
    assert_eq!(with_receipt.fingerprint(), after_receipt.fingerprint());
    assert_ne!(with_receipt.binding(), after_receipt.binding());
    let new_cue = f.rewrite(&republished, |v| {
        v["selected"] = json!(Object::from_bytes(b"fixed recording B"));
    });
    let (_, after_cue) = run(&mut f, &[new_cue], &[]);
    assert_ne!(after_receipt.fingerprint(), after_cue.fingerprint());
}

#[test]
fn declared_defaults_normalize_only_consumed_values_without_rewriting_instances() {
    let mut f = fixture();
    let root = f.node("rows/1", NodeKind::Primary, &[], Some(b"rows"));
    let (_, before) = run(&mut f, std::slice::from_ref(&root), &["width"]);
    let explicit = f.rewrite(&root, |v| v["authored"]["width"] = json!(80));
    let (_, after) = run(&mut f, &[explicit], &["width"]);
    assert_eq!(before.fingerprint(), after.fingerprint());
    let null = f.rewrite(&root, |v| v["authored"]["width"] = Value::Null);
    let (_, after) = run(&mut f, &[null], &["width"]);
    assert_ne!(before.fingerprint(), after.fingerprint());
    let changed = f.rewrite(&root, |v| v["authored"]["width"] = json!(90));
    let (_, after) = run(&mut f, &[changed], &["width"]);
    assert_ne!(before.fingerprint(), after.fingerprint());
    let data = &f.records[&root].0;
    assert!(
        serde_json::from_slice::<Value>(data).unwrap()["authored"]
            .get("width")
            .is_none()
    );
}

#[test]
fn consumed_default_changes_recipe_but_other_definition_edits_do_not() {
    let mut f = fixture();
    let root = f.node("rows/1", NodeKind::Primary, &[], Some(b"rows"));
    let (_, before) = run(&mut f, std::slice::from_ref(&root), &["width"]);
    let mut change = |width: u64, note: &str| {
        let (_, id, definition, schema) = &f.definitions[0];
        let mut definition: Value = serde_json::from_slice(definition).unwrap();
        definition["authoring"]["defaults"]["width"] = json!(width);
        definition["semantic_rules"] = json!({"status_vocabulary":note});
        let definition = bytes(&definition);
        let mut record: Value = serde_json::from_slice(&f.records[&root].0).unwrap();
        record["definition"] = json!({"id":id,"sha256":Object::from_bytes(&definition).sha256,"bytes":definition.len()});
        let raw = bytes(&record);
        let reference = Reference {
            id: root.id.clone(),
            object: Object::from_bytes(&raw),
        };
        f.records
            .insert(reference.clone(), (raw, definition.clone(), schema.clone()));
        f.profiles[0].definition = Object::from_bytes(&definition);
        reference
    };
    let edited = change(80, "new vocabulary");
    let mut g = f.clone();
    let (_, after) = run(&mut g, &[edited], &["width"]);
    assert_eq!(before.fingerprint(), after.fingerprint());
    assert_ne!(before.admission(), after.admission());
    assert_ne!(before.consumption(), after.consumption());
    let (_, id, def, schema) = &f.definitions[0];
    let mut definition: Value = serde_json::from_slice(def).unwrap();
    definition["authoring"]["defaults"]["width"] = json!(90);
    let definition = bytes(&definition);
    let mut record: Value = serde_json::from_slice(&f.records[&root].0).unwrap();
    record["definition"] =
        json!({"id":id,"sha256":Object::from_bytes(&definition).sha256,"bytes":definition.len()});
    let raw = bytes(&record);
    let edited = Reference {
        id: root.id,
        object: Object::from_bytes(&raw),
    };
    f.records
        .insert(edited.clone(), (raw, definition.clone(), schema.clone()));
    f.profiles[0].definition = Object::from_bytes(&definition);
    let (_, after) = run(&mut f, &[edited], &["width"]);
    assert_ne!(before.fingerprint(), after.fingerprint());
}

#[test]
fn selected_payload_changes_but_unselected_history_does_not() {
    let mut f = fixture();
    let root = f.node("cels/a", NodeKind::Primary, &[], Some(b"a"));
    let (_, before) = run(&mut f, std::slice::from_ref(&root), &[]);
    let history=f.rewrite(&root,|v|v["streams"]=json!([{"slot":"old","role":"history","group":"past","object":Object::from_bytes(b"old")} ]));
    let (_, after) = run(&mut f, &[history], &[]);
    assert_eq!(before.fingerprint(), after.fingerprint());
    let changed = f.rewrite(&root, |v| v["selected"] = json!(Object::from_bytes(b"new")));
    let (_, after) = run(&mut f, &[changed], &[]);
    assert_ne!(before.fingerprint(), after.fingerprint());
}

#[test]
fn swapping_named_stream_slots_changes_recipe_even_when_object_set_is_identical() {
    let mut f = fixture();
    f.profiles[0].payloads = Payloads::Named {
        selected: false,
        slots: BTreeSet::from(["left".into(), "right".into()]),
    };
    let root = f.node("recording/stereo", NodeKind::Primary, &[], None);
    let root = f.rewrite(&root, |v| {
        v["streams"] = json!([
            {"slot":"left","role":"audio","group":"channels","object":Object::from_bytes(b"left")},
            {"slot":"right","role":"audio","group":"channels","object":Object::from_bytes(b"right")}
        ])
    });
    let (graph, before) = run(&mut f, std::slice::from_ref(&root), &[]);
    let changed = f.rewrite(&root, |v| {
        let left = v["streams"][0]["object"].clone();
        v["streams"][0]["object"] = v["streams"][1]["object"].clone();
        v["streams"][1]["object"] = left;
    });
    let (next, after) = run(&mut f, &[changed], &[]);
    assert_eq!(
        graph.production_requirements().unwrap().primary(),
        next.production_requirements().unwrap().primary()
    );
    assert_ne!(before.fingerprint(), after.fingerprint());
    let changed = f.rewrite(&root, |v| {
        v["streams"][0]["provenance"] = json!({"note":"unconsumed"})
    });
    let (_, after) = run(&mut f, &[changed], &[]);
    assert_eq!(before.fingerprint(), after.fingerprint());
    let changed = f.rewrite(&root, |v| v["streams"].as_array_mut().unwrap().reverse());
    let (_, after) = run(&mut f, &[changed], &[]);
    assert_ne!(before.fingerprint(), after.fingerprint());
}

#[test]
fn operation_producer_and_semantics_are_recipe_identity_but_checkpoint_is_provenance() {
    let mut f = fixture();
    let root = f.node("rows/1", NodeKind::Primary, &[], Some(b"rows"));
    let (graph, before) = run(&mut f, std::slice::from_ref(&root), &[]);
    let op = operation();
    for changed in [
        Operation {
            name: "other".into(),
            ..op.clone()
        },
        Operation {
            semantics: Digest::blob(b"changed"),
            ..op.clone()
        },
        Operation {
            producer: Object::from_bytes(b"other producer"),
            ..op.clone()
        },
    ] {
        let after = recipe::plan(
            &graph,
            &mut f.clone(),
            &f.profiles,
            &policies(&f, &[]),
            &changed,
            RecipeLimits {
                nodes: 2000,
                edges: 2000,
                bytes: 16 * 1024 * 1024,
            },
        )
        .unwrap();
        assert_ne!(before.fingerprint(), after.fingerprint());
    }
    f.checkpoint = Digest::blob(b"unrelated commit");
    let (_, after) = run(&mut f, &[root], &[]);
    assert_ne!(before.checkpoint(), after.checkpoint());
    assert_eq!(before.binding(), after.binding());
    assert_eq!(before.fingerprint(), after.fingerprint());
}

#[test]
fn recursive_frozen_revision_wiring_cannot_collapse_into_a_flat_projected_graph() {
    let mut f = fixture();
    let red = f.node("cels/red", NodeKind::Primary, &[], Some(b"red"));
    let blue = f.node("cels/blue", NodeKind::Primary, &[], Some(b"blue"));
    let a = f.node("scenes/same-id", NodeKind::Container, &[red], None);
    let b = f.node("scenes/same-id", NodeKind::Container, &[blue], None);
    let x = f.node(
        "episodes/1",
        NodeKind::Container,
        std::slice::from_ref(&a),
        None,
    );
    let y = f.node(
        "episodes/2",
        NodeKind::Container,
        std::slice::from_ref(&b),
        None,
    );
    let (_, before) = run(&mut f, &[x.clone(), y.clone()], &[]);
    let x = f.rewrite(&x, |v| v["authored"]["deps"] = json!([pin(&b)]));
    let y = f.rewrite(&y, |v| v["authored"]["deps"] = json!([pin(&a)]));
    let (_, after) = run(&mut f, &[x, y], &[]);
    assert_ne!(before.fingerprint(), after.fingerprint());
}

#[test]
fn shared_cue_replacement_dirties_selected_parent_but_frozen_parent_stays_pinned() {
    let mut f = fixture();
    let cue = f.node("cues/shared", NodeKind::Primary, &[], Some(b"cue1"));
    let a = f.node(
        "cels/a",
        NodeKind::Primary,
        std::slice::from_ref(&cue),
        Some(b"a"),
    );
    let b = f.node(
        "cels/b",
        NodeKind::Primary,
        std::slice::from_ref(&cue),
        Some(b"b"),
    );
    let root = f.node("scenes/1", NodeKind::Container, &[a, b], None);
    let selected = f.rewrite(&root, |v| {
        v["authored"]["deps"] = json!([{"id":"cues/shared"}])
    });
    let (_, before) = run(&mut f, std::slice::from_ref(&selected), &[]);
    let (_, frozen) = run(&mut f, std::slice::from_ref(&root), &[]);
    f.node("cues/shared", NodeKind::Primary, &[], Some(b"cue2"));
    let (_, after) = run(&mut f, &[selected], &[]);
    assert_ne!(before.fingerprint(), after.fingerprint());
    let (_, after) = run(&mut f, &[root], &[]);
    assert_eq!(frozen.fingerprint(), after.fingerprint());
}

#[test]
fn foreign_or_missing_consumption_and_graph_policy_and_fields_refuse() {
    let mut f = fixture();
    let root = f.node("rows/1", NodeKind::Primary, &[], Some(b"rows"));
    let graph = f.run(&f.query(&[root]));
    let profiles = f.profiles.clone();
    let op = operation();
    let limits = RecipeLimits {
        nodes: 2000,
        edges: 2000,
        bytes: 16 * 1024 * 1024,
    };
    assert!(recipe::plan(&graph, &mut f, &profiles, &[], &op, limits).is_err());
    for field in ["text", "unknown"] {
        let policy = policies(&f, &[field]);
        assert!(recipe::plan(&graph, &mut f, &profiles, &policy, &op, limits).is_err());
    }
    let mut policy = policies(&f, &[]);
    policy.push(policy[0].clone());
    assert!(recipe::plan(&graph, &mut f, &profiles, &policy, &op, limits).is_err());
    let mut policy = policies(&f, &[]);
    policy[0].definition = Object::from_bytes(b"foreign");
    f.calls.clear();
    assert!(recipe::plan(&graph, &mut f, &profiles, &policy, &op, limits).is_err());
    assert!(f.calls.is_empty());
    let policy = policies(&f, &[]);
    let mut profiles = profiles;
    profiles[0].payloads = Payloads::None;
    assert!(recipe::plan(&graph, &mut f, &profiles, &policy, &op, limits).is_err());
    assert!(f.calls.is_empty());
}

#[test]
fn gaps_outputs_nonproduction_and_source_substitution_cannot_produce_a_recipe() {
    let mut f = fixture();
    let root = f.node("rows/1", NodeKind::Primary, &[], Some(b"rows"));
    let graph = f.run(&f.query(std::slice::from_ref(&root)));
    let profiles = f.profiles.clone();
    let policy = policies(&f, &[]);
    let op = operation();
    let limits = RecipeLimits {
        nodes: 2000,
        edges: 2000,
        bytes: 16 * 1024 * 1024,
    };
    let mut wrong = f.clone();
    wrong.checkpoint = Digest::blob(b"wrong");
    assert!(recipe::plan(&graph, &mut wrong, &profiles, &policy, &op, limits).is_err());
    let mut wrong = f.clone();
    wrong.records.get_mut(&root).unwrap().0 = bytes(&json!({}));
    assert!(recipe::plan(&graph, &mut wrong, &profiles, &policy, &op, limits).is_err());
    let mut wrong = f.clone();
    wrong.records.remove(&root);
    assert!(recipe::plan(&graph, &mut wrong, &profiles, &policy, &op, limits).is_err());
    let graph = wrong.run(&wrong.query(&[root]));
    assert!(recipe::plan(&graph, &mut wrong, &profiles, &policy, &op, limits).is_err());
    let out = f.node("outputs/1", NodeKind::Output, &[], None);
    let graph = f.run(&f.query(&[out]));
    assert!(recipe::plan(&graph, &mut f, &profiles, &policy, &op, limits).is_err());
    let mut query = f.query(&[]);
    query.kinds = BTreeSet::from([EdgeKind::Evidence]);
    query.roots = graph.roots().clone();
    let graph = f.run(&query);
    assert!(recipe::plan(&graph, &mut f, &profiles, &policy, &op, limits).is_err());
}

#[test]
fn bounded_nonmedia_thousand_row_report_hashes_fanout_without_a_giant_canonical_record() {
    let mut f = fixture();
    let mut deps = Vec::new();
    for n in 0..1000 {
        let id = format!("rows/{n}");
        f.node(
            &id,
            NodeKind::Primary,
            &[],
            Some(format!("row-{n}").as_bytes()),
        );
        deps.push(json!({"id":id}));
    }
    let root = f.node("reports/1", NodeKind::Container, &[], None);
    let root = f.rewrite(&root, |v| v["authored"]["deps"] = json!(deps));
    let mut q = f.query(&[root]);
    q.limits = Limits {
        nodes: 1200,
        edges: 1200,
        depth: 4,
        bytes: 16 * 1024 * 1024,
    };
    let graph = f.run(&q);
    let before = plan(&mut f, &graph, &[]);
    assert_eq!(before.nodes().len(), 1001);
    assert!(before.bytes_read() > 0);
    let policy = policies(&f, &[]);
    let profiles = f.profiles.clone();
    let recipe_limits = RecipeLimits {
        nodes: q.limits.nodes,
        edges: q.limits.edges,
        bytes: q.limits.bytes,
    };
    for limits in [
        RecipeLimits {
            nodes: 100,
            ..recipe_limits
        },
        RecipeLimits {
            edges: 100,
            ..recipe_limits
        },
        RecipeLimits {
            bytes: 1,
            ..recipe_limits
        },
        RecipeLimits {
            bytes: 0,
            ..recipe_limits
        },
    ] {
        assert!(recipe::plan(&graph, &mut f, &profiles, &policy, &operation(), limits).is_err());
    }
    f.node("rows/42", NodeKind::Primary, &[], Some(b"replacement row"));
    let next = f.run(&q);
    let after = plan(&mut f, &next, &[]);
    assert_ne!(before.fingerprint(), after.fingerprint());
    assert_eq!(
        before
            .nodes()
            .values()
            .filter(|v| after.nodes().values().any(|n| n == *v))
            .count(),
        999
    );
}

#[test]
fn different_definition_ids_and_production_edge_order_change_recipe() {
    let mut f = fixture();
    let a = f.node("inputs/a", NodeKind::Primary, &[], Some(b"a"));
    let b = f.node("inputs/b", NodeKind::Primary, &[], Some(b"b"));
    let root = f.node(
        "scene/1",
        NodeKind::Container,
        &[a.clone(), b.clone()],
        None,
    );
    let (_, before) = run(&mut f, std::slice::from_ref(&root), &[]);
    let changed = f.rewrite(&root, |v| v["authored"]["deps"] = json!([pin(&b), pin(&a)]));
    let (_, after) = run(&mut f, &[changed], &[]);
    assert_ne!(before.fingerprint(), after.fingerprint());
    let (_, before) = run(&mut f, std::slice::from_ref(&a), &[]);
    let mut definition: Value = serde_json::from_slice(&f.definitions[0].2).unwrap();
    definition["id"] = json!("types/another-primary");
    let definition = bytes(&definition);
    let object = Object::from_bytes(&definition);
    let mut record: Value = serde_json::from_slice(&f.records[&a].0).unwrap();
    record["definition"] =
        json!({"id":"types/another-primary","sha256":object.sha256,"bytes":object.bytes});
    let raw = bytes(&record);
    let changed = Reference {
        id: a.id,
        object: Object::from_bytes(&raw),
    };
    f.records.insert(
        changed.clone(),
        (raw, definition, f.definitions[0].3.clone()),
    );
    f.profiles[0].definition_id = "types/another-primary".into();
    f.profiles[0].definition = object;
    let (_, after) = run(&mut f, &[changed], &[]);
    assert_ne!(before.fingerprint(), after.fingerprint());
}

#[test]
fn same_id_frozen_roots_do_not_invalidate_when_raw_reference_sort_order_changes() {
    let mut f = fixture();
    let a = f.node("cels/a", NodeKind::Primary, &[], Some(b"a"));
    let b = f.node("cels/b", NodeKind::Primary, &[], Some(b"b"));
    let first = f.node("scenes/same", NodeKind::Container, &[a], None);
    let second = f.node("scenes/same", NodeKind::Container, &[b], None);
    let (_, before) = run(&mut f, &[first.clone(), second.clone()], &[]);
    let original_order = first.object < second.object;
    let changed = (0..256)
        .find_map(|n| {
            let changed = f.rewrite(&first, |v| v["metadata"] = json!({"annotation":n}));
            ((changed.object < second.object) != original_order).then_some(changed)
        })
        .expect("synthetic metadata must cross raw digest order");
    let (_, after) = run(&mut f, &[changed, second], &[]);
    assert_ne!(before.binding(), after.binding());
    assert_eq!(before.fingerprint(), after.fingerprint());
}

#[test]
fn identical_bilingual_inputs_have_distinct_recipes_for_language_profile_and_encoding_parameters() {
    let mut f = fixture();
    let root = f.node(
        "inputs/bilingual",
        NodeKind::Primary,
        &[],
        Some(b"shared bilingual source"),
    );
    let graph = f.run(&f.query(&[root]));
    let profiles = f.profiles.clone();
    let policy = policies(&f, &[]);
    let limits = RecipeLimits {
        nodes: 100,
        edges: 100,
        bytes: 1_000_000,
    };
    let mut fingerprints = BTreeSet::new();
    let mut node_sets = Vec::new();
    for language in ["es", "en"] {
        for width in [1280, 1920] {
            for encoding in ["review", "delivery"] {
                let parameters =
                    bytes(&json!({"language":language,"width":width,"encoding":encoding}));
                let operation = Operation {
                    parameters: Object::from_bytes(&parameters),
                    ..operation()
                };
                let plan =
                    recipe::plan(&graph, &mut f, &profiles, &policy, &operation, limits).unwrap();
                assert!(fingerprints.insert(plan.fingerprint().clone()));
                node_sets.push(plan.nodes().clone());
            }
        }
    }
    assert_eq!(fingerprints.len(), 8);
    assert!(node_sets.windows(2).all(|pair| pair[0] == pair[1]));
    // Opaque byte pins intentionally do not guess semantic normalization.
    let a = Operation {
        parameters: Object::from_bytes(b"{}"),
        ..operation()
    };
    let b = Operation {
        parameters: Object::from_bytes(b"{ }"),
        ..operation()
    };
    let a = recipe::plan(&graph, &mut f, &profiles, &policy, &a, limits).unwrap();
    let b = recipe::plan(&graph, &mut f, &profiles, &policy, &b, limits).unwrap();
    assert_ne!(a.fingerprint(), b.fingerprint());
}

#[test]
fn unsafe_or_conflicting_operation_parameter_and_producer_pins_refuse_before_source_io() {
    let mut f = fixture();
    let payload = Object::from_bytes(b"rows");
    let root = f.node("rows/1", NodeKind::Primary, &[], Some(b"rows"));
    let graph = f.run(&f.query(&[root]));
    let profiles = f.profiles.clone();
    let policy = policies(&f, &[]);
    let limits = RecipeLimits {
        nodes: 100,
        edges: 100,
        bytes: 1_000_000,
    };
    let mut wrong = payload;
    wrong.bytes += 1;
    let mut unsafe_pin = Object::from_bytes(b"parameters");
    unsafe_pin.bytes = 9_007_199_254_740_992;
    let op = operation();
    let mut conflict = op.producer.clone();
    conflict.bytes += 1;
    let cases = [
        Operation {
            parameters: wrong.clone(),
            ..op.clone()
        },
        Operation {
            producer: wrong,
            ..op.clone()
        },
        Operation {
            parameters: unsafe_pin.clone(),
            ..op.clone()
        },
        Operation {
            producer: unsafe_pin,
            ..op.clone()
        },
        Operation {
            parameters: conflict,
            ..op
        },
    ];
    f.calls.clear();
    for operation in cases {
        assert!(recipe::plan(&graph, &mut f, &profiles, &policy, &operation, limits).is_err());
        assert!(f.calls.is_empty());
    }
}
