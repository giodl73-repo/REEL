use super::*;
use serde_json::json;

struct Fixture {
    _temp: tempfile::TempDir,
    cache: PathBuf,
    config: Config,
    scope: Scope,
    catalog: Catalog,
    candidate: Candidate,
    source: Source,
    root: Blob,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let cache = temp.path().canonicalize().unwrap();
        for name in ["staging", "retained", "catalog", "coordination"] {
            fs::create_dir(cache.join(name)).unwrap();
        }
        let config:Config=serde_json::from_value(json!({"path_policy":"configured-local-roots","cache_roots":[cache],"archive_roots":[],
            "coordination_root":cache.join("coordination"),"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}})).unwrap();
        let raw=serde_json::to_vec(&json!({"schema":SOURCE_SCHEMA,"project_id":"synthetic","profile_sha256":hash(b"profile"),
            "registry_generation":1,"tiers":[],"objects":[{"sha256":hash(b"observed media"),"bytes":14,"registered":true,"copies":[]}]})).unwrap();
        let original = Blob {
            sha256: hash(&raw),
            bytes: raw.len() as u64,
        };
        let source = Source {
            encoded: original.clone(),
            decoded: original,
            encoding: Encoding::Identity,
            schema: SOURCE_SCHEMA.into(),
            project_id: "synthetic".into(),
            authority_namespace: "fixture".into(),
            adapter_version: ADAPTER_VERSION,
        };
        let index = prepare(
            raw.as_slice(),
            &source,
            super::super::super::super::Limits::default(),
        )
        .unwrap();
        let mut writer = FileStore::begin(
            &cache,
            &cache.join("staging"),
            "index",
            super::super::Limits::default(),
        )
        .unwrap();
        let root =
            super::super::super::publish(&mut writer, &index, BuildLimits::default()).unwrap();
        writer.seal(&root).unwrap();
        writer
            .handoff(&cache.join("retained"), &root, &source)
            .unwrap();
        let candidate = Candidate::verify(
            &cache,
            &cache.join("retained"),
            &root,
            &source,
            ReadLimits::default(),
        )
        .unwrap();
        let scope = Scope {
            project_id: "synthetic".into(),
            authority_namespace: "fixture".into(),
            catalog_id: hash(b"catalog instance"),
        };
        let context = MutationContext {
            project_root: &cache,
            storage: &config,
            overhead_bytes: 64 * 1024,
        };
        let catalog =
            Catalog::initialize(&context, &cache, &cache.join("catalog"), &scope).unwrap();
        Self {
            _temp: temp,
            cache,
            config,
            scope,
            catalog,
            candidate,
            source,
            root,
        }
    }
    fn context(&self) -> MutationContext<'_> {
        MutationContext {
            project_root: &self.cache,
            storage: &self.config,
            overhead_bytes: 64 * 1024,
        }
    }
    fn create(&self, key: &str) -> Snapshot {
        self.catalog.create_key(&self.context(), key).unwrap()
    }
    fn fence(&self, key: &str, binding: Option<Binding>) -> (State, Fence) {
        let before = self.catalog.read(key).unwrap().state;
        let fence = Fence {
            schema: FENCE.into(),
            catalog: self.catalog.tag().into(),
            key: key.into(),
            revision: before.revision + 1,
            previous: Descriptor::from(&digest(&before).unwrap()),
            binding,
        };
        publish_once(
            &self.catalog.record(key, "fences", fence.revision).unwrap(),
            &fence,
        )
        .unwrap();
        (before, fence)
    }
    fn decision(&self, key: &str, fence: &Fence, commit: bool) -> Decision {
        let decision = Decision {
            schema: DECISION.into(),
            catalog: self.catalog.tag().into(),
            key: key.into(),
            revision: fence.revision,
            fence: Descriptor::from(&digest(fence).unwrap()),
            commit,
        };
        publish_once(
            &self
                .catalog
                .record(key, "decisions", fence.revision)
                .unwrap(),
            &decision,
        )
        .unwrap();
        decision
    }
}

#[test]
fn admits_exact_verified_root_and_reopens_without_history_replay() {
    let f = Fixture::new();
    assert_eq!(f.create("census/current").revision, 0);
    let selected = f
        .catalog
        .admit(&f.context(), "census/current", 0, &f.candidate)
        .unwrap();
    assert_eq!(selected.revision, 1);
    assert!(selected.root() == Some(f.root.clone()));
    assert!(selected.source().unwrap().unwrap().encoded == f.source.encoded);
    let reopened = Catalog::open(&f.cache, &f.cache.join("catalog"), &f.scope).unwrap();
    assert_eq!(reopened.read("census/current").unwrap().revision, 1);
}
#[test]
fn tombstone_recreate_and_same_value_updates_never_reuse_revisions() {
    let f = Fixture::new();
    f.create("key");
    f.catalog
        .admit(&f.context(), "key", 0, &f.candidate)
        .unwrap();
    let dead = f.catalog.tombstone(&f.context(), "key", 1).unwrap();
    assert_eq!(dead.revision, 2);
    assert!(dead.root().is_none());
    assert!(
        f.catalog
            .admit(&f.context(), "key", 0, &f.candidate)
            .is_err()
    );
    assert!(
        f.catalog
            .admit(&f.context(), "key", 1, &f.candidate)
            .is_err()
    );
    assert_eq!(
        f.catalog
            .admit(&f.context(), "key", 2, &f.candidate)
            .unwrap()
            .revision,
        3
    );
    assert_eq!(
        f.catalog
            .admit(&f.context(), "key", 3, &f.candidate)
            .unwrap()
            .revision,
        4
    );
    assert_eq!(
        f.catalog.create_key(&f.context(), "key").unwrap().revision,
        4
    );
}
#[test]
fn independent_keys_can_update_while_another_key_is_owned() {
    let f = Fixture::new();
    f.create("one");
    f.create("two");
    let _owner = owner(&f.catalog.slot("one").unwrap().join("owner.lock"), false).unwrap();
    assert!(
        f.catalog
            .admit(&f.context(), "one", 0, &f.candidate)
            .is_err()
    );
    assert_eq!(
        f.catalog
            .admit(&f.context(), "two", 0, &f.candidate)
            .unwrap()
            .revision,
        1
    );
    let _ledger = admission::lock(f.config.coordination_root.as_ref().unwrap()).unwrap();
}
#[test]
fn uncommitted_fence_recovery_aborts_and_consumes_revision() {
    let f = Fixture::new();
    f.create("key");
    f.fence("key", Some(f.candidate.binding.clone()));
    assert!(f.catalog.read("key").is_err());
    assert!(protection::collect(&f.catalog, protection::Limits::default()).is_err());
    let recovered = f.catalog.recover(&f.context(), "key").unwrap();
    assert_eq!(recovered.revision, 1);
    assert!(recovered.root().is_none());
    assert_eq!(
        f.catalog
            .admit(&f.context(), "key", 1, &f.candidate)
            .unwrap()
            .revision,
        2
    );
}
#[test]
fn committed_decision_replays_before_head_and_never_becomes_abort() {
    let f = Fixture::new();
    f.create("key");
    let (_, fence) = f.fence("key", Some(f.candidate.binding.clone()));
    f.decision("key", &fence, true);
    assert!(f.catalog.read("key").is_err());
    let recovered = f.catalog.recover(&f.context(), "key").unwrap();
    assert_eq!(recovered.revision, 1);
    assert!(recovered.root() == Some(f.root.clone()));
    assert_eq!(f.catalog.recover(&f.context(), "key").unwrap().revision, 1);
}
#[test]
fn committed_tombstone_recovery_preserves_counter_and_old_writer_conflicts() {
    let f = Fixture::new();
    f.create("key");
    f.catalog
        .admit(&f.context(), "key", 0, &f.candidate)
        .unwrap();
    let (_, fence) = f.fence("key", None);
    f.decision("key", &fence, true);
    let recovered = f.catalog.recover(&f.context(), "key").unwrap();
    assert_eq!(recovered.revision, 2);
    assert!(recovered.root().is_none());
    assert!(
        f.catalog
            .admit(&f.context(), "key", 1, &f.candidate)
            .is_err()
    );
}
#[test]
fn missing_or_rolled_back_head_does_not_reset_a_key() {
    let f = Fixture::new();
    f.create("key");
    let path = f.catalog.slot("key").unwrap().join("head.json");
    let old = fs::read(&path).unwrap();
    f.catalog
        .admit(&f.context(), "key", 0, &f.candidate)
        .unwrap();
    fs::write(&path, &old).unwrap();
    assert!(f.catalog.read("key").is_err());
    assert!(f.catalog.recover(&f.context(), "key").unwrap().root() == Some(f.root.clone()));
    fs::remove_file(&path).unwrap();
    assert!(f.catalog.create_key(&f.context(), "key").is_err());
    assert!(protection::collect(&f.catalog, protection::Limits::default()).is_err());
}
#[test]
fn changed_state_or_missing_committed_record_fails_instead_of_returning_empty() {
    for target in ["states", "decisions", "fences"] {
        let f = Fixture::new();
        f.create("key");
        f.catalog
            .admit(&f.context(), "key", 0, &f.candidate)
            .unwrap();
        fs::write(f.catalog.record("key", target, 1).unwrap(), b"corrupt").unwrap();
        assert!(f.catalog.read("key").is_err());
        assert!(protection::collect(&f.catalog, protection::Limits::default()).is_err());
    }
}
#[test]
fn scoped_manifest_and_candidate_source_cannot_be_swapped() {
    let f = Fixture::new();
    f.create("key");
    let mut scope = f.scope.clone();
    scope.authority_namespace = "other".into();
    assert!(Catalog::open(&f.cache, &f.cache.join("catalog"), &scope).is_err());
    let mut candidate = Candidate {
        binding: f.candidate.binding.clone(),
    };
    candidate.binding.source["authority"] = json!("other");
    assert!(f.catalog.admit(&f.context(), "key", 0, &candidate).is_err());
    assert_eq!(f.catalog.read("key").unwrap().revision, 0);
    fs::write(f.cache.join("catalog/catalog.json"), b"corrupt").unwrap();
    assert!(f.catalog.read("key").is_err());
}
#[test]
fn missing_retention_blocks_admission_and_committed_recovery() {
    let f = Fixture::new();
    f.create("key");
    let (_, fence) = f.fence("key", Some(f.candidate.binding.clone()));
    f.decision("key", &fence, true);
    let path = f
        .cache
        .join("retained")
        .join(&f.root.sha256)
        .join("objects")
        .join(&f.root.sha256);
    fs::remove_file(&path).unwrap();
    assert!(f.catalog.recover(&f.context(), "key").is_err());
    f.create("other");
    assert!(
        f.catalog
            .admit(&f.context(), "other", 0, &f.candidate)
            .is_err()
    );
    assert_eq!(f.catalog.read("other").unwrap().revision, 0);
}
#[test]
fn protection_is_positive_index_metadata_not_inventory_member_liveness() {
    let f = Fixture::new();
    f.create("key");
    f.catalog
        .admit(&f.context(), "key", 0, &f.candidate)
        .unwrap();
    let snapshot = protection::collect(&f.catalog, protection::Limits::default()).unwrap();
    assert_eq!(snapshot.protected_nodes(), 2);
    assert_eq!(snapshot.keys(), 1);
    assert_eq!(snapshot.required_sources(), 1);
    assert!(matches!(
        snapshot.classify(&f.root.sha256).unwrap(),
        protection::Membership::ProtectedMetadata
    ));
    assert!(matches!(
        snapshot.classify(&hash(b"observed media")).unwrap(),
        protection::Membership::OutsideCatalogScope
    ));
    assert!(
        snapshot
            .requires_original_source(&f.source.encoded.sha256)
            .unwrap()
    );
    assert!(snapshot.classify("../outside").is_err());
    snapshot.revalidate(&f.catalog).unwrap();
    f.catalog.tombstone(&f.context(), "key", 1).unwrap();
    assert!(snapshot.revalidate(&f.catalog).is_err());
    let empty = protection::collect(&f.catalog, protection::Limits::default()).unwrap();
    assert_eq!(empty.protected_nodes(), 0);
    assert!(matches!(
        empty.classify(&f.root.sha256).unwrap(),
        protection::Membership::OutsideCatalogScope
    ));
}
#[test]
fn new_key_or_missing_member_invalidates_protection_stamp() {
    let f = Fixture::new();
    f.create("one");
    let snapshot = protection::collect(&f.catalog, protection::Limits::default()).unwrap();
    f.create("two");
    assert!(snapshot.revalidate(&f.catalog).is_err());
    let base = f.cache.join("catalog/members").join(hash(b"two"));
    fs::remove_file(base.with_extension("json")).unwrap();
    fs::remove_file(base.with_extension("pending")).unwrap();
    assert!(protection::collect(&f.catalog, protection::Limits::default()).is_err());
}
#[test]
fn projection_and_control_budgets_fail_without_partial_protection() {
    let f = Fixture::new();
    f.create("key");
    f.catalog
        .admit(&f.context(), "key", 0, &f.candidate)
        .unwrap();
    for limits in [
        protection::Limits {
            keys: 0,
            ..protection::Limits::default()
        },
        protection::Limits {
            nodes: 1,
            ..protection::Limits::default()
        },
        protection::Limits {
            total_bytes: 1,
            ..protection::Limits::default()
        },
        protection::Limits {
            metadata_bytes: 1,
            ..protection::Limits::default()
        },
        protection::Limits {
            objects: 0,
            ..protection::Limits::default()
        },
    ] {
        assert!(protection::collect(&f.catalog, limits).is_err());
    }
    let mut config: Config = serde_json::from_value(json!({"path_policy":"configured-local-roots",
        "cache_roots":[f.cache],"archive_roots":[],"capacity":f.config.capacity}))
    .unwrap();
    let context = MutationContext {
        project_root: &f.cache,
        storage: &config,
        overhead_bytes: 64 * 1024,
    };
    assert!(f.catalog.create_key(&context, "no-shared-ledger").is_err());
    config.coordination_root = f.config.coordination_root.clone();
    config.capacity.minimum_free_bytes = u64::MAX;
    let context = MutationContext {
        project_root: &f.cache,
        storage: &config,
        overhead_bytes: 64 * 1024,
    };
    assert!(f.catalog.create_key(&context, "denied").is_err());
    assert!(!f.catalog.slot("denied").unwrap().exists());
}

#[test]
fn complete_pending_fence_is_recovered_without_revision_reuse() {
    let f = Fixture::new();
    f.create("key");
    let before = f.catalog.read("key").unwrap().state;
    let fence = Fence {
        schema: FENCE.into(),
        catalog: f.catalog.tag().into(),
        key: "key".into(),
        revision: 1,
        previous: Descriptor::from(&digest(&before).unwrap()),
        binding: Some(f.candidate.binding.clone()),
    };
    create_control(
        &f.catalog
            .record("key", "fences", 1)
            .unwrap()
            .with_extension("pending"),
        &fence,
    )
    .unwrap();
    assert!(f.catalog.read("key").is_err());
    let snapshot = f.catalog.recover(&f.context(), "key").unwrap();
    assert_eq!(snapshot.revision, 1);
    assert!(snapshot.root().is_none());
    assert!(
        f.catalog
            .admit(&f.context(), "key", 0, &f.candidate)
            .is_err()
    );
}
#[test]
fn complete_pending_decision_replays_instead_of_discarding_commit_intent() {
    let f = Fixture::new();
    f.create("key");
    let (_, fence) = f.fence("key", Some(f.candidate.binding.clone()));
    let decision = Decision {
        schema: DECISION.into(),
        catalog: f.catalog.tag().into(),
        key: "key".into(),
        revision: 1,
        fence: Descriptor::from(&digest(&fence).unwrap()),
        commit: true,
    };
    create_control(
        &f.catalog
            .record("key", "decisions", 1)
            .unwrap()
            .with_extension("pending"),
        &decision,
    )
    .unwrap();
    assert!(f.catalog.read("key").is_err());
    let snapshot = f.catalog.recover(&f.context(), "key").unwrap();
    assert_eq!(snapshot.revision, 1);
    assert!(snapshot.root() == Some(f.root));
}
#[test]
fn partial_pending_controls_require_reconciliation_without_silent_abort_or_repair() {
    for kind in ["fences", "decisions", "states"] {
        let f = Fixture::new();
        f.create("key");
        if kind != "fences" {
            let (_, fence) = f.fence("key", Some(f.candidate.binding.clone()));
            if kind == "states" {
                f.decision("key", &fence, true);
            }
        }
        let path = f
            .catalog
            .record("key", kind, 1)
            .unwrap()
            .with_extension("pending");
        fs::write(&path, b"partial").unwrap();
        assert!(f.catalog.read("key").is_err());
        assert!(f.catalog.recover(&f.context(), "key").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"partial");
        assert!(
            f.cache
                .join("retained")
                .join(&f.root.sha256)
                .join("objects")
                .join(&f.root.sha256)
                .is_file()
        );
    }
}

#[test]
fn losing_manifest_never_allows_reinitialization_over_existing_membership() {
    let f = Fixture::new();
    f.create("key");
    f.catalog
        .admit(&f.context(), "key", 0, &f.candidate)
        .unwrap();
    fs::remove_file(f.cache.join("catalog/catalog.json")).unwrap();
    assert!(
        Catalog::initialize(&f.context(), &f.cache, &f.cache.join("catalog"), &f.scope).is_err()
    );
    assert!(!f.cache.join("catalog/catalog.json").exists());
    assert!(f.catalog.record("key", "states", 1).unwrap().is_file());
}
