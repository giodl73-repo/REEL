use super::*;
use git_asset::source::native::assessment::{self as native, ClaimLocation, Issue};

struct Rig {
    f: Fixture,
    fields: Vec<Consumption>,
    operation: Operation,
}
impl Rig {
    fn new() -> (Self, AssetSnapshot, Plan, MemoryNodes) {
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
        let fields = policies(&f);
        let operation = operation();
        let original = recipe(&mut source, &f.profiles, &fields, &operation);
        let mut nodes = MemoryNodes::default();
        let context = source
            .freeze_production_context(
                &query(&source, "inputs/rows"),
                &f.profiles,
                b"{}",
                &mut nodes,
                TreeLimits::default(),
            )
            .unwrap();
        let claim = Claim::create(
            &original,
            pin(&context),
            vec![subject(&source, &f.profiles).subject().clone()],
        )
        .unwrap();
        nodes.put(claim.bytes()).unwrap();
        let rig = Self {
            f,
            fields,
            operation,
        };
        let (source, desired) = rig.revise("report", |v| {
            v["metadata"]["production"] = json!({"claim":claim.object()})
        });
        (rig, source, desired, nodes)
    }
    fn revise(&self, record: &str, edit: impl FnOnce(&mut Value)) -> (AssetSnapshot, Plan) {
        let path = self.f.root().join(format!("records/{record}.asset"));
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        edit(&mut value);
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        let mut source = capture(&reader(self.f.root()), commit(self.f.root()));
        let plan = recipe(&mut source, &self.f.profiles, &self.fields, &self.operation);
        (source, plan)
    }
    fn assess(
        &self,
        source: &(impl AssetResolver + ?Sized),
        desired: &Plan,
        nodes: &mut impl NodeStore,
    ) -> native::OutputAssessment {
        let original = admission(&self.f.profiles, &self.fields, &self.operation);
        let definition = graph::Reference {
            id: self.f.profiles[1].definition_id.clone(),
            object: self.f.profiles[1].definition.clone(),
        };
        native::assess(
            &native::Request {
                key: &Key {
                    asset_id: "outputs/report".into(),
                    selector: Selector::Selected {},
                },
                desired,
                definition: &definition,
                claim_location: &ClaimLocation::new(vec!["production".into(), "claim".into()])
                    .unwrap(),
                original: &original,
            },
            source,
            nodes,
        )
    }
}

struct NoAccess;
impl NodeStore for NoAccess {
    fn get(&self, _: &Object) -> TreeResult<Option<Vec<u8>>> {
        panic!("unexpected metadata IO")
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("unexpected metadata write")
    }
}
struct Readonly<'a>(&'a MemoryNodes);
impl NodeStore for Readonly<'_> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.0.get(object)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("assessment must not write")
    }
}

#[test]
fn native_assessment_follows_exact_current_claim_and_detects_consumed_changes_without_writes() {
    let (rig, source, desired, nodes) = Rig::new();
    let result = rig.assess(&source, &desired, &mut Readonly(&nodes));
    assert_eq!(result.state, State::ClaimedRecipeMatch);
    assert_eq!(result.issue, None);
    assert_eq!(
        result.record.as_ref().unwrap(),
        source.resolve_id("outputs/report").unwrap().reference()
    );
    assert!(result.claim.is_some());
    let resolved = source.resolve_id("outputs/report").unwrap();
    assert_eq!(
        resolved.metadata()["production"]["claim"],
        json!(result.claim.unwrap())
    );
    assert!(!format!("{resolved:?}").contains("DO NOT PRINT PRIVATE METADATA"));
    let (annotation, desired) =
        rig.revise("rows", |v| v["metadata"]["note"] = json!("private note"));
    assert_eq!(
        rig.assess(&annotation, &desired, &mut Readonly(&nodes))
            .state,
        State::ClaimedRecipeMatch
    );
    let (changed, desired) = rig.revise("rows", |v| v["authored"]["text"] = json!("fixed rows"));
    let result = rig.assess(&changed, &desired, &mut Readonly(&nodes));
    assert_eq!(result.state, State::StaleRecipe);
    assert!(result.state.catch_up_candidate());
    let debug = format!("{result:?}");
    assert!(
        !debug.contains("outputs/report")
            && !debug.contains("private")
            && !debug.contains("sha256")
    );
}

#[test]
fn native_assessment_legacy_and_planned_outputs_need_no_claim_store_reads() {
    let (rig, _, _, _) = Rig::new();
    let (legacy, desired) = rig.revise("report", |v| {
        v["metadata"].as_object_mut().unwrap().remove("production");
    });
    let result = rig.assess(&legacy, &desired, &mut NoAccess);
    assert_eq!(result.state, State::Unproven);
    assert_eq!(result.issue, None);
    let (planned, desired) = rig.revise("report", |v| v["selected"] = Value::Null);
    assert_eq!(
        rig.assess(&planned, &desired, &mut NoAccess).state,
        State::Unbuilt
    );
}

#[test]
fn native_assessment_partial_null_unsafe_and_unknown_field_pins_block_before_store_io() {
    let (rig, _, _, _) = Rig::new();
    for bad in [
        Value::Null,
        json!({}),
        json!({"claim":null}),
        json!({"claim": {"sha256":Digest::blob(b"x"),"bytes":65537}}),
        json!({"claim": {"sha256":Digest::blob(b"x"),"bytes":1,"extra":true}}),
    ] {
        let (source, desired) = rig.revise("report", |v| v["metadata"]["production"] = bad);
        let result = rig.assess(&source, &desired, &mut NoAccess);
        assert_eq!(result.state, State::Blocked);
        assert_eq!(result.issue, Some(Issue::ClaimPin));
    }
}

#[test]
fn native_assessment_missing_corrupt_and_failed_claim_reads_never_become_unproven() {
    let (rig, source, desired, nodes) = Rig::new();
    struct Failed;
    impl NodeStore for Failed {
        fn get(&self, _: &Object) -> TreeResult<Option<Vec<u8>>> {
            Err(TreeError::Store("PRIVATE PROVIDER DETAILS".into()))
        }
        fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
            panic!("write")
        }
    }
    struct Corrupt;
    impl NodeStore for Corrupt {
        fn get(&self, _: &Object) -> TreeResult<Option<Vec<u8>>> {
            Ok(Some(b"bad bytes".to_vec()))
        }
        fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
            panic!("write")
        }
    }
    assert_eq!(
        rig.assess(&source, &desired, &mut MemoryNodes::default())
            .issue,
        Some(Issue::ClaimUnavailable)
    );
    let failed = rig.assess(&source, &desired, &mut Failed);
    assert_eq!(failed.issue, Some(Issue::ClaimUnavailable));
    assert!(!format!("{failed:?}").contains("PRIVATE PROVIDER"));
    assert_eq!(
        rig.assess(&source, &desired, &mut Corrupt).issue,
        Some(Issue::ClaimIntegrity)
    );
    let pin =
        source.resolve_id("outputs/report").unwrap().metadata()["production"]["claim"].clone();
    let raw = nodes
        .get(&serde_json::from_value(pin).unwrap())
        .unwrap()
        .unwrap();
    let mut only_claim = MemoryNodes::default();
    only_claim.put(&raw).unwrap();
    assert_eq!(
        rig.assess(&source, &desired, &mut only_claim).issue,
        Some(Issue::ClaimReplay)
    );
}

#[test]
fn native_assessment_foreign_output_and_wrong_checkpoint_block_instead_of_selecting_claim_subject()
{
    let (rig, old, _, nodes) = Rig::new();
    let (changed, desired) = rig.revise("report", |v| {
        v["selected"] = json!(Object::from_bytes(b"different current master"))
    });
    assert_eq!(
        rig.assess(&changed, &desired, &mut Readonly(&nodes)).issue,
        Some(Issue::ClaimSubject)
    );
    assert_eq!(
        rig.assess(&old, &desired, &mut NoAccess).issue,
        Some(Issue::CurrentCheckpoint)
    );
    let (empty, desired) = rig.revise("report", |v| v["selected"] = Value::Null);
    assert_eq!(
        rig.assess(&empty, &desired, &mut Readonly(&nodes)).issue,
        Some(Issue::ClaimSubject)
    );
}

#[test]
fn native_assessment_claim_locations_are_bounded_literal_keys_and_not_path_aliases() {
    assert!(ClaimLocation::new(vec![]).is_err());
    assert!(ClaimLocation::new(vec!["x".into(); 9]).is_err());
    assert!(ClaimLocation::new(vec!["".into()]).is_err());
    assert!(ClaimLocation::new(vec!["x".repeat(129)]).is_err());
    assert!(ClaimLocation::new(vec!["private\nkey".into()]).is_err());
    assert!(ClaimLocation::new(vec!["key/with~literal".into()]).is_ok());
}

#[test]
fn native_assessment_named_stream_and_literal_metadata_key_replay_the_exact_subject() {
    let (rig, source, _, mut nodes) = Rig::new();
    let pin: Object = serde_json::from_value(
        source.resolve_id("outputs/report").unwrap().metadata()["production"]["claim"].clone(),
    )
    .unwrap();
    let raw = nodes.get(&pin).unwrap().unwrap();
    let claim = Claim::parse(&pin, &raw).unwrap();
    let admitted = admission(&rig.f.profiles, &rig.fields, &rig.operation);
    let (mut original, _) = native_claim::replay(&claim, &admitted, &mut nodes).unwrap();
    let original_plan = recipe(&mut original, &rig.f.profiles, &rig.fields, &rig.operation);
    let resolved = source.resolve_id("outputs/report").unwrap();
    let definition = resolved.definition().clone();
    let selector = Selector::Stream {
        slot: "preview".into(),
    };
    let subject = BoundSubject::resolve(&resolved, &definition, selector.clone())
        .unwrap()
        .unwrap();
    let named = Claim::create(
        &original_plan,
        claim.context().clone(),
        vec![subject.subject().clone()],
    )
    .unwrap();
    nodes.put(named.bytes()).unwrap();
    let (source, desired) = rig.revise("report", |v| {
        v["metadata"]["key/with~literal"] = json!(named.object())
    });
    let result = native::assess(
        &native::Request {
            key: &Key {
                asset_id: "outputs/report".into(),
                selector,
            },
            desired: &desired,
            definition: &definition,
            claim_location: &ClaimLocation::new(vec!["key/with~literal".into()]).unwrap(),
            original: &admitted,
        },
        &source,
        &mut Readonly(&nodes),
    );
    assert_eq!(result.state, State::ClaimedRecipeMatch);
    assert_eq!(result.claim.as_ref(), Some(named.object()));
}

#[test]
fn native_assessment_old_producer_admission_is_separate_from_desired_producer_changes() {
    let (rig, mut source, _, nodes) = Rig::new();
    let changed_operation = Operation {
        producer: Object::from_bytes(b"new producer binary"),
        ..rig.operation.clone()
    };
    let desired = recipe(
        &mut source,
        &rig.f.profiles,
        &rig.fields,
        &changed_operation,
    );
    assert_eq!(
        rig.assess(&source, &desired, &mut Readonly(&nodes)).state,
        State::StaleRecipe
    );
    let unadmitted = admission(&rig.f.profiles, &rig.fields, &changed_operation);
    let resolved = source.resolve_id("outputs/report").unwrap();
    let result = native::assess(
        &native::Request {
            key: &Key {
                asset_id: "outputs/report".into(),
                selector: Selector::Selected {},
            },
            desired: &desired,
            definition: resolved.definition(),
            claim_location: &ClaimLocation::new(vec!["production".into(), "claim".into()]).unwrap(),
            original: &unadmitted,
        },
        &source,
        &mut Readonly(&nodes),
    );
    assert_eq!(result.state, State::Blocked);
    assert_eq!(result.issue, Some(Issue::ClaimReplay));
}

#[test]
fn native_assessment_invalid_requests_and_foreign_definition_refuse_before_evidence_reads() {
    let (rig, source, desired, _) = Rig::new();
    struct NoResolve;
    impl AssetResolver for NoResolve {
        fn resolve_id(&self, _: &str) -> Result<git_asset_model::resolve::Resolved, ResolveError> {
            panic!("invalid request must not resolve")
        }
    }
    let definition = source
        .resolve_id("outputs/report")
        .unwrap()
        .definition()
        .clone();
    let original = admission(&rig.f.profiles, &rig.fields, &rig.operation);
    let location = ClaimLocation::new(vec!["production".into(), "claim".into()]).unwrap();
    for id in ["", "x\nprivate", &"x".repeat(257)] {
        let result = native::assess(
            &native::Request {
                key: &Key {
                    asset_id: id.into(),
                    selector: Selector::Selected {},
                },
                desired: &desired,
                definition: &definition,
                claim_location: &location,
                original: &original,
            },
            &NoResolve,
            &mut NoAccess,
        );
        assert_eq!(result.issue, Some(Issue::InvalidRequest));
    }
    let wrong = graph::Reference {
        id: "types/foreign".into(),
        object: definition.object,
    };
    let result = native::assess(
        &native::Request {
            key: &Key {
                asset_id: "outputs/report".into(),
                selector: Selector::Selected {},
            },
            desired: &desired,
            definition: &wrong,
            claim_location: &location,
            original: &original,
        },
        &source,
        &mut NoAccess,
    );
    assert_eq!(result.issue, Some(Issue::CurrentDefinition));
    let result = native::assess(
        &native::Request {
            key: &Key {
                asset_id: "outputs/missing".into(),
                selector: Selector::Selected {},
            },
            desired: &desired,
            definition: &wrong,
            claim_location: &location,
            original: &original,
        },
        &source,
        &mut NoAccess,
    );
    assert_eq!(result.issue, Some(Issue::CurrentUnavailable));
}

#[test]
fn native_assessment_reopens_current_and_original_metadata_with_git_and_working_files_offline() {
    let (rig, current, matching, nodes) = Rig::new();
    let (changed, stale) = rig.revise("rows", |v| {
        v["authored"]["text"] = json!("changed after delivery")
    });
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
    for (_, bytes) in nodes.objects() {
        store.put(bytes).unwrap();
    }
    let current_root = current
        .archive_metadata(&mut store, TreeLimits::default())
        .unwrap();
    let changed_root = changed
        .archive_metadata(&mut store, TreeLimits::default())
        .unwrap();
    let witness = store.finish().unwrap();
    drop(store);
    drop(current);
    drop(changed);
    drop(nodes);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            rig.f.root().join(name),
            rig.f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut store = MetadataReader::open(&path, &witness).unwrap();
    let current = AssetSnapshot::from_metadata_archive(
        &mut store,
        &current_root,
        Limits::default(),
        TreeLimits::default(),
    )
    .unwrap();
    let changed = AssetSnapshot::from_metadata_archive(
        &mut store,
        &changed_root,
        Limits::default(),
        TreeLimits::default(),
    )
    .unwrap();
    let resolver: &dyn AssetResolver = &current;
    assert_eq!(
        rig.assess(resolver, &matching, &mut store).state,
        State::ClaimedRecipeMatch
    );
    assert_eq!(
        rig.assess(&changed, &stale, &mut store).state,
        State::StaleRecipe
    );
    #[cfg(feature = "schema")]
    {
        let validated = current
            .validate_authored_schemas(git_asset_model::schema::Dialect::Draft202012)
            .unwrap();
        assert_eq!(
            rig.assess(&validated, &matching, &mut store).state,
            State::ClaimedRecipeMatch
        );
        assert!(!validated.complete_semantic_coverage());
    }
    assert!(!rig.f.root().join(".git").exists());
    assert!(!rig.f.root().join("records").exists());
}
