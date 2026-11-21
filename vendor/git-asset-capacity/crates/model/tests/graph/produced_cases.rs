use super::*;
#[path = "assessment_cases.rs"]
mod assessment_cases;
use git_asset_model::{
    graph::recipe::{
        self, Consumption, Operation, Plan,
        produced::{self, BoundSubject, Claim, ContextPin, Correspondence as C, Selector, Subject},
    },
    native::Instance,
};

fn setup() -> (Fixture, Reference, Plan, BoundSubject) {
    let mut f = Fixture::new();
    let input = f.node("inputs/rows", NodeKind::Primary, &[], Some(b"rows"));
    let plan = plan(&mut f, &input, operation());
    let output = f.node(
        "sensitive/output",
        NodeKind::Output,
        &[],
        Some(b"produced document"),
    );
    let subject = subject(&f, &output, Selector::Selected {}).unwrap();
    (f, input, plan, subject)
}
fn operation() -> Operation {
    Operation {
        name: "report".into(),
        semantics: Digest::blob(b"semantics"),
        producer: Object::from_bytes(b"producer"),
        parameters: Object::from_bytes(b"{}"),
    }
}
fn plan(f: &mut Fixture, root: &Reference, operation: Operation) -> Plan {
    let graph = f.run(&f.query(std::slice::from_ref(root)));
    let profiles = f.profiles.clone();
    let fields = profiles
        .iter()
        .map(|p| Consumption {
            definition_id: p.definition_id.clone(),
            definition: p.definition.clone(),
            fields: BTreeSet::new(),
        })
        .collect::<Vec<_>>();
    recipe::plan(
        &graph,
        f,
        &profiles,
        &fields,
        &operation,
        recipe::Limits {
            nodes: 100,
            edges: 100,
            bytes: 1_000_000,
        },
    )
    .unwrap()
}
fn subject(f: &Fixture, reference: &Reference, selector: Selector) -> Option<BoundSubject> {
    let (record, definition, schema) = &f.records[reference];
    let bound = Instance::parse(record)
        .unwrap()
        .bind(definition, schema)
        .unwrap();
    let admission = Reference {
        id: bound.definition().value().id.clone(),
        object: bound.definition().object().clone(),
    };
    BoundSubject::bind(&bound, &admission, selector).unwrap()
}
fn context() -> ContextPin {
    ContextPin {
        root: Object::from_bytes(b"opaque context not verified by pure comparison"),
        identity: Digest::blob(b"header"),
    }
}
fn claim(plan: &Plan, subject: &BoundSubject) -> Claim {
    Claim::create(plan, context(), vec![subject.subject().clone()]).unwrap()
}
fn changed(
    claim: &Claim,
    edit: impl FnOnce(&mut Value),
) -> Result<Claim, git_asset_core::identity::Error> {
    let mut value: Value = serde_json::from_slice(claim.bytes()).unwrap();
    edit(&mut value);
    let bytes = bytes(&value);
    Claim::parse(&Object::from_bytes(&bytes), &bytes)
}

#[test]
fn bounded_claim_roundtrip_matches_original_plan_and_exact_current_output() {
    let (_, _, plan, subject) = setup();
    let claim = claim(&plan, &subject);
    let parsed = Claim::parse(claim.object(), claim.bytes()).unwrap();
    assert_eq!(parsed.object(), claim.object());
    assert_eq!(parsed.bytes(), claim.bytes());
    assert_eq!(parsed.operation(), plan.operation());
    assert_eq!(parsed.recipe(), plan.fingerprint());
    assert_eq!(parsed.origin().checkpoint(), plan.checkpoint());
    assert_eq!(parsed.origin().binding(), plan.binding());
    assert_eq!(parsed.origin().admission(), plan.admission());
    assert_eq!(parsed.origin().consumption(), plan.consumption());
    assert_eq!(parsed.context(), &context());
    assert_eq!(parsed.outputs(), &[subject.subject().clone()]);
    let checked = parsed.check_plan(&plan).unwrap();
    assert_eq!(
        produced::compare(&plan, Some(&subject), Some(&checked)).unwrap(),
        C::ClaimedRecipeMatch
    );
    let debug = format!("{claim:?}");
    assert!(!debug.contains("sensitive/output") && !debug.contains("opaque context"));
}

#[test]
fn absent_output_is_unbuilt_and_existing_history_without_a_claim_is_unproven() {
    let (mut f, _, plan, subject) = setup();
    assert_eq!(produced::compare(&plan, None, None).unwrap(), C::Unbuilt);
    assert_eq!(
        produced::compare(&plan, Some(&subject), None).unwrap(),
        C::Unproven
    );
    let unbuilt = f.node("outputs/planned", NodeKind::Output, &[], None);
    assert!(self::subject(&f, &unbuilt, Selector::Selected {}).is_none());
    assert!(
        self::subject(
            &f,
            &unbuilt,
            Selector::Stream {
                slot: "missing".into()
            }
        )
        .is_none()
    );
    let claim = claim(&plan, &subject);
    let checked = claim.check_plan(&plan).unwrap();
    assert!(produced::compare(&plan, None, Some(&checked)).is_err());
}

#[test]
fn source_annotation_changes_keep_a_truthful_old_claim_without_inheriting_approval() {
    let (mut f, input, old, subject) = setup();
    let claim = claim(&old, &subject);
    let changed = f.rewrite(&input, |v| {
        v["metadata"] = json!({"annotation":"changed"});
        v["status"] = json!({"project_review":"different"});
    });
    let desired = plan(&mut f, &changed, operation());
    assert_ne!(desired.binding(), old.binding());
    assert_eq!(desired.fingerprint(), old.fingerprint());
    assert!(claim.check_plan(&desired).is_err());
    let checked = claim.check_plan(&old).unwrap();
    assert_eq!(
        produced::compare(&desired, Some(&subject), Some(&checked)).unwrap(),
        C::ClaimedRecipeMatch
    );
    assert_eq!(checked.claim().origin().binding(), old.binding());
}

#[test]
fn changed_input_parameters_producer_and_operation_make_exact_old_output_stale() {
    let (mut f, input, old, subject) = setup();
    let claim = claim(&old, &subject);
    let checked = claim.check_plan(&old).unwrap();
    let changed = f.rewrite(&input, |v| {
        v["selected"] = json!(Object::from_bytes(b"new rows"))
    });
    let desired = plan(&mut f, &changed, operation());
    assert_eq!(
        produced::compare(&desired, Some(&subject), Some(&checked)).unwrap(),
        C::StaleRecipe
    );
    let op = operation();
    for operation in [
        Operation {
            parameters: Object::from_bytes(b"different language"),
            ..op.clone()
        },
        Operation {
            producer: Object::from_bytes(b"new build"),
            ..op.clone()
        },
        Operation {
            semantics: Digest::blob(b"new semantics"),
            ..op.clone()
        },
        Operation {
            name: "other".into(),
            ..op
        },
    ] {
        let desired = plan(&mut f, &input, operation);
        assert_eq!(
            produced::compare(&desired, Some(&subject), Some(&checked)).unwrap(),
            C::StaleRecipe
        );
    }
}

#[test]
fn exact_asset_slot_definition_and_payload_subjects_cannot_be_substituted() {
    let (mut f, _, plan, current) = setup();
    let claim = claim(&plan, &current);
    let checked = claim.check_plan(&plan).unwrap();
    let other = f.node(
        "another/output",
        NodeKind::Output,
        &[],
        Some(b"produced document"),
    );
    let other = subject(&f, &other, Selector::Selected {}).unwrap();
    assert!(produced::compare(&plan, Some(&other), Some(&checked)).is_err());
    let reference = f.selections["sensitive/output"].clone();
    let other = f.rewrite(&reference, |v| {
        v["selected"] = json!(Object::from_bytes(b"new output"))
    });
    let other = subject(&f, &other, Selector::Selected {}).unwrap();
    assert!(produced::compare(&plan, Some(&other), Some(&checked)).is_err());
    let other=f.rewrite(&reference,|v|v["streams"]=json!([{"slot":"version-1","role":"history","group":"past","object":current.subject().object()}]));
    let other = subject(
        &f,
        &other,
        Selector::Stream {
            slot: "version-1".into(),
        },
    )
    .unwrap();
    assert!(produced::compare(&plan, Some(&other), Some(&checked)).is_err());
    let (record, definition, schema) = f.records[&reference].clone();
    let mut definition: Value = serde_json::from_slice(&definition).unwrap();
    definition["semantic_rules"] = json!({"annotation":"changed definition"});
    let definition = bytes(&definition);
    let pin = Object::from_bytes(&definition);
    let mut record: Value = serde_json::from_slice(&record).unwrap();
    record["definition"]["sha256"] = json!(pin.sha256);
    record["definition"]["bytes"] = json!(pin.bytes);
    let record = bytes(&record);
    let other = Reference {
        id: reference.id,
        object: Object::from_bytes(&record),
    };
    f.records
        .insert(other.clone(), (record, definition, schema));
    let other = subject(&f, &other, Selector::Selected {}).unwrap();
    assert!(produced::compare(&plan, Some(&other), Some(&checked)).is_err());
}

#[test]
fn multiple_candidates_can_share_a_recipe_but_not_their_claim_or_output_subject() {
    let (mut f, _, plan, first) = setup();
    let claim1 = claim(&plan, &first);
    let reference = f.selections["sensitive/output"].clone();
    let reference = f.rewrite(&reference, |v| {
        v["selected"] = json!(Object::from_bytes(b"other candidate"))
    });
    let second = subject(&f, &reference, Selector::Selected {}).unwrap();
    let claim2 = claim(&plan, &second);
    assert_eq!(claim1.recipe(), claim2.recipe());
    assert_ne!(claim1.object(), claim2.object());
    let checked = claim2.check_plan(&plan).unwrap();
    assert_eq!(
        produced::compare(&plan, Some(&second), Some(&checked)).unwrap(),
        C::ClaimedRecipeMatch
    );
    assert!(produced::compare(&plan, Some(&first), Some(&checked)).is_err());
}

#[test]
fn every_original_provenance_and_operation_field_is_checked_not_only_recipe_hash() {
    let (_, _, plan, subject) = setup();
    let original = claim(&plan, &subject);
    for field in ["checkpoint", "binding", "admission", "consumption"] {
        let claim = changed(&original, |v| {
            v["origin"][field] = json!(Digest::blob(format!("other-{field}").as_bytes()))
        })
        .unwrap();
        assert!(claim.check_plan(&plan).is_err());
    }
    for field in ["producer", "parameters"] {
        let claim = changed(&original, |v| {
            v["operation"][field] = json!(Object::from_bytes(format!("other-{field}").as_bytes()))
        })
        .unwrap();
        assert!(claim.check_plan(&plan).is_err());
    }
    for field in ["semantics", "name"] {
        let claim = changed(&original, |v| {
            v["operation"][field] = json!(Digest::blob(format!("other-{field}").as_bytes()))
        })
        .unwrap();
        assert!(claim.check_plan(&plan).is_err());
    }
    let claim = changed(&original, |v| {
        v["recipe"] = json!(Digest::blob(b"other recipe"))
    })
    .unwrap();
    assert!(claim.check_plan(&plan).is_err());
}

#[test]
fn pure_plan_check_does_not_pretend_to_verify_an_opaque_context_or_execute_a_producer() {
    let (_, _, plan, subject) = setup();
    let original = claim(&plan, &subject);
    let claim = changed(&original, |v| {
        v["context"]["root"] = json!(Object::from_bytes(b"missing metadata"))
    })
    .unwrap();
    assert!(claim.check_plan(&plan).is_ok());
    // Native context replay must supply the separate metadata proof. This unsigned
    // claim can describe arbitrary output bytes and grants no execution authority.
    let checked = claim.check_plan(&plan).unwrap();
    assert_eq!(
        produced::compare(&plan, Some(&subject), Some(&checked)).unwrap(),
        C::ClaimedRecipeMatch
    );
}

#[test]
fn strict_claim_parser_refuses_byte_pin_schema_duplicate_unknown_and_missing_fields() {
    let (_, _, plan, subject) = setup();
    let claim = claim(&plan, &subject);
    assert!(Claim::parse(&Object::from_bytes(b"wrong"), claim.bytes()).is_err());
    let mut pin = claim.object().clone();
    pin.bytes += 1;
    assert!(Claim::parse(&pin, claim.bytes()).is_err());
    assert!(changed(&claim, |v| v["schema"] = json!("unknown")).is_err());
    assert!(changed(&claim, |v| v["unknown"] = json!(true)).is_err());
    assert!(changed(&claim, |v| v["operation"]["unknown"] = json!(true)).is_err());
    assert!(
        changed(&claim, |v| {
            v["operation"].as_object_mut().unwrap().remove("parameters");
        })
        .is_err()
    );
    assert!(
        changed(&claim, |v| v["outputs"][0]["selector"]["unexpected"] =
            json!(true))
        .is_err()
    );
    let text = String::from_utf8(claim.bytes().to_vec()).unwrap().replacen(
        "\"schema\":",
        "\"schema\":\"duplicate\",\"schema\":",
        1,
    );
    assert!(Claim::parse(&Object::from_bytes(text.as_bytes()), text.as_bytes()).is_err());
    let oversized = vec![b' '; 65_537];
    assert!(Claim::parse(&Object::from_bytes(&oversized), &oversized).is_err());
}

#[test]
fn duplicate_outputs_invalid_selectors_and_unsafe_conflicting_sizes_refuse() {
    let (_, _, plan, subject) = setup();
    let claim = claim(&plan, &subject);
    assert!(Claim::create(&plan, context(), vec![]).is_err());
    assert!(
        Claim::create(
            &plan,
            context(),
            vec![subject.subject().clone(), subject.subject().clone()]
        )
        .is_err()
    );
    assert!(
        changed(&claim, |v| v["outputs"][0]["object"]["bytes"] =
            json!(9_007_199_254_740_992u64))
        .is_err()
    );
    assert!(
        changed(&claim, |v| v["outputs"][0]["selector"] =
            json!({"kind":"stream","slot":""}))
        .is_err()
    );
    assert!(changed(&claim, |v| v["outputs"][0]["asset_id"] = json!("\n")).is_err());
    assert!(
        changed(&claim, |v| {
            let mut duplicate = v["outputs"][0].clone();
            duplicate["object"] = json!(Object::from_bytes(b"other output"));
            v["outputs"].as_array_mut().unwrap().push(duplicate);
        })
        .is_err()
    );
    assert!(
        changed(&claim, |v| {
            v["operation"]["parameters"] = v["operation"]["producer"].clone();
            v["operation"]["parameters"]["bytes"] = json!(999);
        })
        .is_err()
    );
}

#[test]
fn selected_and_historical_named_subjects_are_explicit_and_claim_pointer_has_no_self_cycle() {
    let (mut f, _, plan, current) = setup();
    let reference = f.selections["sensitive/output"].clone();
    let reference=f.rewrite(&reference,|v|v["streams"]=json!([{"slot":"version-1","role":"history","group":"past","object":current.subject().object()}]));
    let selected = subject(&f, &reference, Selector::Selected {}).unwrap();
    let historical = subject(
        &f,
        &reference,
        Selector::Stream {
            slot: "version-1".into(),
        },
    )
    .unwrap();
    let claim = Claim::create(
        &plan,
        context(),
        vec![selected.subject().clone(), historical.subject().clone()],
    )
    .unwrap();
    let reference = f.rewrite(&reference, |v| {
        v["streams"][0]["provenance"] = json!({"production_claim":claim.object()})
    });
    assert_eq!(
        subject(&f, &reference, Selector::Selected {}).unwrap(),
        selected
    );
    assert_eq!(
        subject(
            &f,
            &reference,
            Selector::Stream {
                slot: "version-1".into()
            }
        )
        .unwrap(),
        historical
    );
    let checked = claim.check_plan(&plan).unwrap();
    for subject in [&selected, &historical] {
        assert_eq!(
            produced::compare(&plan, Some(subject), Some(&checked)).unwrap(),
            C::ClaimedRecipeMatch
        );
    }
}

#[test]
fn definition_admission_is_exact_and_primary_assets_can_also_be_producer_subjects() {
    let (f, input, _, _) = setup();
    assert!(subject(&f, &input, Selector::Selected {}).is_some());
    let (record, definition, schema) = &f.records[&input];
    let bound = Instance::parse(record)
        .unwrap()
        .bind(definition, schema)
        .unwrap();
    let mut admission = Reference {
        id: bound.definition().value().id.clone(),
        object: bound.definition().object().clone(),
    };
    admission.object = Object::from_bytes(b"wrong definition");
    assert!(BoundSubject::bind(&bound, &admission, Selector::Selected {}).is_err());
    admission.object = bound.definition().object().clone();
    admission.id = "other type".into();
    assert!(BoundSubject::bind(&bound, &admission, Selector::Selected {}).is_err());
}

#[test]
fn claim_output_count_and_canonical_byte_budget_are_bounded() {
    let (mut f, _, plan, _) = setup();
    let mut subjects = Vec::new();
    for n in 0..257 {
        let root = f.node(
            &format!("outputs/{n}"),
            NodeKind::Output,
            &[],
            Some(b"same immutable bytes"),
        );
        subjects.push(
            subject(&f, &root, Selector::Selected {})
                .unwrap()
                .subject()
                .clone(),
        );
    }
    assert!(Claim::create(&plan, context(), subjects.clone()).is_err());
    let subjects = subjects
        .into_iter()
        .take(128)
        .enumerate()
        .map(|(n, s)| {
            let mut value = serde_json::to_value(s).unwrap();
            value["asset_id"] = json!(format!("{}-{n}", "x".repeat(248)));
            serde_json::from_value::<Subject>(value).unwrap()
        })
        .collect();
    assert!(Claim::create(&plan, context(), subjects).is_err());
}

#[test]
fn qualified_resolver_binds_current_subject_without_accepting_a_claim_as_source() {
    use git_asset_model::resolve::Resolved;
    let (f, _, plan, current) = setup();
    let reference = &f.selections["sensitive/output"];
    let (record, definition, schema) = &f.records[reference];
    let bound = Instance::parse(record)
        .unwrap()
        .bind(definition, schema)
        .unwrap();
    let resolved = Resolved::from_bound(f.checkpoint.clone(), &bound);
    let admission = current.subject().definition();
    let resolved_subject = BoundSubject::resolve(&resolved, admission, Selector::Selected {})
        .unwrap()
        .unwrap();
    assert_eq!(resolved_subject, current);
    assert!(
        BoundSubject::resolve(
            &resolved,
            admission,
            Selector::Stream {
                slot: "absent".into()
            }
        )
        .unwrap()
        .is_none()
    );
    let mut wrong = admission.clone();
    wrong.object = Object::from_bytes(b"foreign definition");
    assert!(BoundSubject::resolve(&resolved, &wrong, Selector::Selected {}).is_err());
    let claim = claim(&plan, &current);
    let checked = claim.check_plan(&plan).unwrap();
    assert_eq!(
        produced::compare(&plan, Some(&resolved_subject), Some(&checked)).unwrap(),
        C::ClaimedRecipeMatch
    );
}
