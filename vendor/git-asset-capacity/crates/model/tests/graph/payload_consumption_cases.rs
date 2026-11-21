use super::*;
use git_asset_model::graph::recipe::{self, Consumption, Operation, PayloadConsumption, Plan};

fn operation() -> Operation {
    Operation {
        name: "projection".into(),
        semantics: Digest::blob(b"explicit semantic contract"),
        producer: Object::from_bytes(b"producer"),
        parameters: Object::from_bytes(b"English meaning"),
    }
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
fn policies(f: &Fixture) -> Vec<PayloadConsumption> {
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
fn limits() -> recipe::Limits {
    recipe::Limits {
        nodes: 2000,
        edges: 2000,
        bytes: 16 * 1024 * 1024,
    }
}
fn plan(
    f: &mut Fixture,
    g: &Graph,
    p: &[PayloadConsumption],
    op: &Operation,
) -> Result<Plan, git_asset_core::identity::Error> {
    recipe::plan_with_payloads(g, f, &f.profiles.clone(), &fields(f), p, op, limits())
}
fn container() -> (Fixture, Reference, Reference) {
    let mut f = Fixture::new();
    f.profiles[2].payloads = Payloads::SelectedObject;
    let input = f.node("input/one", NodeKind::Primary, &[], Some(b"input"));
    let root = f.node(
        "control",
        NodeKind::Container,
        std::slice::from_ref(&input),
        Some(b"bilingual control v1"),
    );
    (f, root, input)
}

#[test]
fn captured_control_changes_provenance_and_retention_without_projected_recipe_churn() {
    let (mut f, root, _) = container();
    let p = policies(&f);
    let g = f.run(&f.query(std::slice::from_ref(&root)));
    let original = plan(&mut f, &g, &p, &operation()).unwrap();
    let legacy = recipe::plan(
        &g,
        &mut f.clone(),
        &f.profiles,
        &fields(&f),
        &operation(),
        limits(),
    )
    .unwrap();
    assert_ne!(legacy.fingerprint(), original.fingerprint());
    let changed = f.rewrite(&root, |v| {
        v["selected"] = json!(Object::from_bytes(b"only Spanish changed"))
    });
    let next = f.run(&f.query(&[changed]));
    let after = plan(&mut f, &next, &p, &operation()).unwrap();
    assert_eq!(original.fingerprint(), after.fingerprint());
    assert_ne!(original.binding(), after.binding());
    assert_eq!(original.binding(), g.binding_digest().unwrap());
    assert_eq!(original.admission(), g.admission());
    assert_eq!(
        next.production_requirements().unwrap().controls(),
        &BTreeSet::from([Object::from_bytes(b"only Spanish changed")])
    );
    let mut op = operation();
    op.parameters = Object::from_bytes(b"English meaning changed");
    assert_ne!(
        after.fingerprint(),
        plan(&mut f, &next, &p, &op).unwrap().fingerprint()
    );
    assert!(!after.complete_semantic_coverage());
}

#[test]
fn consumed_primary_and_recursive_wiring_still_change_projected_recipe() {
    let (mut f, root, input) = container();
    let p = policies(&f);
    let g = f.run(&f.query(std::slice::from_ref(&root)));
    let before = plan(&mut f, &g, &p, &operation()).unwrap();
    let changed = f.rewrite(&input, |v| {
        v["selected"] = json!(Object::from_bytes(b"new input"))
    });
    let root = f.rewrite(&root, |v| v["authored"]["deps"] = json!([pin(&changed)]));
    let next = f.run(&f.query(&[root]));
    assert_ne!(
        before.fingerprint(),
        plan(&mut f, &next, &p, &operation()).unwrap().fingerprint()
    );
}

#[test]
fn payload_policy_is_complete_unique_admitted_bounded_and_preflighted_before_lookup() {
    let (mut f, root, _) = container();
    let graph = f.run(&f.query(&[root]));
    let p = policies(&f);
    let calls = f.calls.clone();
    let mut cases = Vec::new();
    cases.push(p[..2].to_vec());
    let mut bad = p.clone();
    bad[1] = bad[0].clone();
    cases.push(bad);
    let mut bad = p.clone();
    bad[0].definition_id = "unadmitted".into();
    cases.push(bad);
    let mut bad = p.clone();
    bad[0].definition = Object::from_bytes(b"foreign");
    cases.push(bad);
    let mut bad = p.clone();
    bad[1].selected = true;
    cases.push(bad);
    let mut bad = p.clone();
    bad[0].slots.insert("uncaptured".into());
    cases.push(bad);
    let mut bad = p.clone();
    bad[0].slots.insert("x".repeat(129));
    cases.push(bad);
    for bad in cases {
        assert!(plan(&mut f, &graph, &bad, &operation()).is_err());
        assert_eq!(f.calls, calls);
    }
    let debug = format!("{:?}", p[0]);
    assert!(!debug.contains("types/primary"));
}

#[test]
fn stream_projection_preserves_order_role_group_and_excludes_unconsumed_stream_changes() {
    let mut f = Fixture::new();
    f.profiles[0].payloads = Payloads::Named {
        selected: false,
        slots: BTreeSet::from(["en".into(), "es".into()]),
    };
    let root = f.node("cue", NodeKind::Primary, &[], None);
    let root = f.rewrite(&root, |v| {
        v["streams"] = json!([
            {"slot":"en","role":"audio","group":"language","object":Object::from_bytes(b"en")},
            {"slot":"es","role":"audio","group":"language","object":Object::from_bytes(b"es")}
        ])
    });
    let mut p = policies(&f);
    p[0].selected = false;
    p[0].slots.insert("en".into());
    let g = f.run(&f.query(std::slice::from_ref(&root)));
    let before = plan(&mut f, &g, &p, &operation()).unwrap();
    let changed = f.rewrite(&root, |v| {
        v["streams"][1]["object"] = json!(Object::from_bytes(b"es2"))
    });
    let g = f.run(&f.query(&[changed]));
    assert_eq!(
        before.fingerprint(),
        plan(&mut f, &g, &p, &operation()).unwrap().fingerprint()
    );
    assert_eq!(g.production_requirements().unwrap().primary().len(), 2);
    for field in ["role", "group"] {
        let changed = f.rewrite(&root, |v| v["streams"][0][field] = "different".into());
        let g = f.run(&f.query(&[changed]));
        assert_ne!(
            before.fingerprint(),
            plan(&mut f, &g, &p, &operation()).unwrap().fingerprint()
        );
    }
    p[0].slots.insert("es".into());
    let g = f.run(&f.query(std::slice::from_ref(&root)));
    let before = plan(&mut f, &g, &p, &operation()).unwrap();
    let changed = f.rewrite(&root, |v| v["streams"].as_array_mut().unwrap().reverse());
    let g = f.run(&f.query(&[changed]));
    assert_ne!(
        before.fingerprint(),
        plan(&mut f, &g, &p, &operation()).unwrap().fingerprint()
    );
}

#[test]
fn identical_bytes_do_not_authorize_uncaptured_stream_roles_or_missing_slots() {
    let mut f = Fixture::new();
    f.profiles[0].payloads = Payloads::Streams {
        roles: BTreeSet::from(["audio".into()]),
        groups: BTreeSet::from(["language".into()]),
        min: 1,
    };
    let root = f.node("cue", NodeKind::Primary, &[], None);
    let root=f.rewrite(&root,|v|v["streams"]=json!([
        {"slot":"en","role":"audio","group":"language","object":Object::from_bytes(b"shared")},
        {"slot":"private-note","role":"note","group":"language","object":Object::from_bytes(b"shared")}
    ]));
    let g = f.run(&f.query(&[root]));
    let mut p = policies(&f);
    p[0].selected = false;
    for slot in ["private-note", "missing"] {
        p[0].slots = BTreeSet::from([slot.into()]);
        assert!(plan(&mut f, &g, &p, &operation()).is_err());
    }
    p[0].slots = BTreeSet::from(["en".into()]);
    assert!(plan(&mut f, &g, &p, &operation()).is_ok());
}

#[test]
fn an_empty_consumed_payload_policy_cannot_turn_an_output_into_a_primary_input() {
    let mut f = Fixture::new();
    let root = f.node("compiled/master", NodeKind::Output, &[], Some(b"master"));
    let g = f.run(&f.query(&[root]));
    let calls = f.calls.clone();
    let p = policies(&f);
    assert!(plan(&mut f, &g, &p, &operation()).is_err());
    assert_eq!(calls, f.calls);
}
