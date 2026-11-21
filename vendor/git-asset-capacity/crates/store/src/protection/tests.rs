use super::*;
use crate::source_claims::{Command, SourceJournal};
use git_asset_core::{LogicalPath, tree::MemoryNodes};
use std::{fs, path::Path};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn digest(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn root(nodes: &mut MemoryNodes, objects: &[(&str, Object)]) -> Selection {
    let entries = objects
        .iter()
        .map(|(name, object)| (LogicalPath::parse(name).unwrap(), object.clone()))
        .collect();
    let (tree, _) = Tree::build(&entries, nodes, TreeLimits::default()).unwrap();
    Selection {
        root: tree.root().clone(),
        context: digest("context"),
    }
}
fn selected(j: &mut SourceJournal, name: &str, rev: u64, selection: &Selection) {
    let prepared = j
        .apply(
            key(&format!("prepare-{name}-{rev}")),
            Command::Prepare {
                source: key(name),
                owner: key("owner"),
                expected_revision: rev,
                selection: selection.clone(),
            },
        )
        .unwrap();
    j.apply(
        key(&format!("adopt-{name}-{rev}")),
        Command::Adopt {
            source: key(name),
            owner: key("owner"),
            expected_revision: rev + 1,
            prepared: prepared.reference,
        },
    )
    .unwrap();
}
struct Fixture {
    temp: tempfile::TempDir,
    nodes: MemoryNodes,
    location: Location,
    profile: Profile,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("journal");
        let mut j = SourceJournal::create(&path, key("project"), key("epoch")).unwrap();
        let minimum = j.checkpoint();
        let mut nodes = MemoryNodes::default();
        let common = Object::from_bytes(b"common");
        let old = root(
            &mut nodes,
            &[
                ("shared", common.clone()),
                ("changed", Object::from_bytes(b"old")),
            ],
        );
        let current = root(
            &mut nodes,
            &[
                ("shared", common.clone()),
                ("changed", Object::from_bytes(b"current")),
            ],
        );
        let next = root(
            &mut nodes,
            &[
                ("shared", common),
                ("changed", Object::from_bytes(b"pending")),
            ],
        );
        selected(&mut j, "main", 0, &old);
        selected(&mut j, "main", 2, &current);
        j.apply(
            key("pending-main"),
            Command::Prepare {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 4,
                selection: next.clone(),
            },
        )
        .unwrap();
        selected(&mut j, "draft", 0, &next);
        selected(&mut j, "job", 0, &old);
        selected(&mut j, "retention", 0, &old);
        let places = Places {
            current: vec![key("working"), key("backup")],
            pending: vec![key("working"), key("backup")],
            predecessor: vec![key("history"), key("backup")],
        };
        let sources = [
            ("main", Kind::Branch),
            ("draft", Kind::Draft),
            ("job", Kind::Job),
            ("retention", Kind::Retention),
        ]
        .into_iter()
        .map(|(name, kind)| Source {
            key: key(name),
            owner: key("owner"),
            kind,
            format: Format::FlatBindings,
            places: places.clone(),
        })
        .collect();
        let profile = Profile {
            authority: digest("authority"),
            generation: 1,
            kinds: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
                .into_iter()
                .collect(),
            unmanaged_boundary: Some(digest("admitted-boundary")),
            journals: vec![Journal {
                identity: j.identity().clone(),
                sources,
            }],
            limits: Limits {
                millis: 60_000,
                roots: 32,
                bindings: 1000,
                associations: 2000,
                reads: 10000,
                read_bytes: 4 * 1024 * 1024,
            },
        };
        Self {
            temp,
            nodes,
            location: Location {
                root: path,
                minimum,
            },
            profile,
        }
    }
    fn capture(&self) -> Snapshot {
        capture(
            &self.profile,
            std::slice::from_ref(&self.location),
            &self.nodes,
        )
        .unwrap()
    }
    fn journal(&self) -> SourceJournal {
        SourceJournal::open(&self.location.root, &self.location.minimum).unwrap()
    }
}
#[test]
fn every_registered_source_and_root_role_contributes_shared_demand() {
    let f = Fixture::new();
    let before = fs::read(f.location.root.join("claims.sqlite")).unwrap();
    let snapshot = f.capture();
    assert!(snapshot.summary().observed_scope_resolved);
    assert!(!snapshot.summary().complete_claim_coverage);
    assert!(!snapshot.summary().removal_authority);
    assert_eq!(snapshot.metrics.roots, 6);
    assert_eq!(snapshot.metrics.bindings, 12);
    assert_eq!(snapshot.origins.len(), 6);
    let common = Object::from_bytes(b"common");
    let backup = snapshot
        .demands
        .get(&Placement {
            object: common.clone(),
            place: "backup".into(),
        })
        .unwrap();
    assert_eq!(backup.len(), 6);
    assert!(snapshot.demands.contains_key(&Placement {
        object: Object::from_bytes(b"old"),
        place: "history".into()
    }));
    assert!(snapshot.demands.contains_key(&Placement {
        object: Object::from_bytes(b"old"),
        place: "working".into()
    }));
    assert!(snapshot.demands.contains_key(&Placement {
        object: Object::from_bytes(b"current"),
        place: "working".into()
    }));
    assert!(snapshot.demands.contains_key(&Placement {
        object: Object::from_bytes(b"pending"),
        place: "working".into()
    }));
    let roles = snapshot
        .origins
        .values()
        .map(|o| o.role.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(roles.len(), 3);
    let view = snapshot.view(1).unwrap();
    assert!(!view.complete_claim_coverage);
    let surplus = Placement {
        object: Object::from_bytes(b"unclaimed"),
        place: "working".into(),
    };
    let plan = view.plan(&[surplus.clone()].into_iter().collect()).unwrap();
    assert!(!plan.ensure.is_empty());
    assert!(plan.release_candidates.is_empty());
    assert_eq!(
        plan.held[&surplus],
        git_asset_core::HoldReason::IncompleteClaimCoverage
    );
    assert_eq!(
        fs::read(f.location.root.join("claims.sqlite")).unwrap(),
        before
    );
    assert_eq!(fs::read_dir(&f.location.root).unwrap().count(), 2);
    assert!(
        identity::canonical_json(&serde_json::to_value(snapshot.summary()).unwrap())
            .unwrap()
            .len()
            < 65536
    );
}
#[test]
fn profile_permutations_are_stable_and_policy_and_head_changes_are_bound() {
    let mut f = Fixture::new();
    let initial = f.capture();
    let profile_id = f.profile.id().unwrap();
    f.profile.journals[0].sources.reverse();
    for source in &mut f.profile.journals[0].sources {
        source.places.current.reverse();
    }
    assert_eq!(f.profile.id().unwrap(), profile_id);
    assert_eq!(f.capture().id(), initial.id());
    f.profile.generation += 1;
    assert_ne!(f.capture().id(), initial.id());
    f.profile.generation -= 1;
    let mut j = f.journal();
    j.apply(
        key("withdraw-job"),
        Command::Withdraw {
            source: key("job"),
            owner: key("owner"),
            expected_revision: 2,
        },
    )
    .unwrap();
    assert_ne!(f.capture().id(), initial.id());
}
#[test]
fn unexpected_missing_and_foreign_sources_never_resolve_coverage() {
    let mut f = Fixture::new();
    let selection = f
        .journal()
        .source(&key("job"))
        .unwrap()
        .unwrap()
        .current
        .unwrap()
        .selection;
    selected(&mut f.journal(), "unlisted", 0, &selection);
    f.profile.journals[0].sources[0].owner = key("foreign");
    let mut missing = f.profile.journals[0].sources[0].clone();
    missing.key = key("absent");
    f.profile.journals[0].sources.push(missing);
    let snapshot = f.capture();
    assert!(!snapshot.summary().observed_scope_resolved);
    assert!(
        snapshot
            .gaps
            .iter()
            .any(|g| matches!(g,Gap::UnexpectedSource{source,..} if source=="unlisted"))
    );
    assert!(
        snapshot
            .gaps
            .iter()
            .any(|g| matches!(g, Gap::ForeignOwner { .. }))
    );
    assert!(
        snapshot
            .gaps
            .iter()
            .any(|g| matches!(g, Gap::MissingSource { .. }))
    );
    assert!(
        !snapshot
            .origins
            .values()
            .any(|o| o.source.as_str() == "main")
    );
}
#[test]
fn undeclared_kinds_unmanaged_clients_and_opaque_roots_are_explicit_gaps() {
    let mut f = Fixture::new();
    f.profile.kinds.remove(&Kind::Job);
    f.profile.unmanaged_boundary = None;
    f.profile.journals[0].sources[0].format = Format::Opaque;
    let snapshot = f.capture();
    assert!(!snapshot.summary().observed_scope_resolved);
    assert!(snapshot.gaps.iter().any(|g| matches!(
        g,
        Gap::MissingKind {
            source_kind: Kind::Job
        }
    )));
    assert!(
        snapshot
            .gaps
            .iter()
            .any(|g| matches!(g, Gap::UnmanagedBoundaryUnknown))
    );
    assert!(
        snapshot
            .gaps
            .iter()
            .any(|g| matches!(g, Gap::UnsupportedClosure { .. }))
    );
}
struct Missing<'a> {
    inner: &'a MemoryNodes,
    object: Digest,
    corrupt: bool,
}
impl NodeStore for Missing<'_> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        if o.sha256 == self.object {
            if self.corrupt {
                Ok(Some(b"damaged".to_vec()))
            } else {
                Ok(None)
            }
        } else {
            self.inner.get(o)
        }
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("read-only")
    }
}
#[test]
fn incomplete_root_has_no_partial_demands_and_other_roots_remain_positive() {
    let f = Fixture::new();
    let state = f.journal().source(&key("main")).unwrap().unwrap();
    let bad = state.current.unwrap().selection.root;
    for corrupt in [false, true] {
        let nodes = Missing {
            inner: &f.nodes,
            object: bad.sha256.clone(),
            corrupt,
        };
        let snapshot = capture(&f.profile, std::slice::from_ref(&f.location), &nodes).unwrap();
        assert!(
            snapshot
                .gaps
                .iter()
                .any(|g| matches!(g,Gap::UnresolvedRoot{root} if root==&bad))
        );
        assert!(!snapshot.origins.values().any(|o| o.selection.root == bad));
        assert!(
            !snapshot
                .demands
                .keys()
                .any(|p| p.object == Object::from_bytes(b"current"))
        );
        assert!(
            snapshot
                .demands
                .keys()
                .any(|p| p.object == Object::from_bytes(b"old"))
        );
    }
}
#[test]
fn aggregate_budgets_apply_across_successful_and_failed_roots() {
    for field in 0..5 {
        let mut f = Fixture::new();
        match field {
            0 => f.profile.limits.roots = 1,
            1 => f.profile.limits.bindings = 3,
            2 => f.profile.limits.associations = 2,
            3 => f.profile.limits.reads = 2,
            _ => f.profile.limits.read_bytes = 1,
        }
        assert!(matches!(
            capture(&f.profile, std::slice::from_ref(&f.location), &f.nodes),
            Err(Error::Budget)
        ));
    }
}
#[test]
fn conflicting_object_sizes_across_roots_refuse() {
    let mut f = Fixture::new();
    let old = f
        .journal()
        .source(&key("job"))
        .unwrap()
        .unwrap()
        .current
        .unwrap()
        .selection;
    let mut object = Object::from_bytes(b"old");
    object.bytes += 1;
    let bad = root(&mut f.nodes, &[("bad", object)]);
    selected(&mut f.journal(), "job", 2, &bad);
    assert!(matches!(
        capture(&f.profile, std::slice::from_ref(&f.location), &f.nodes),
        Err(Error::ConflictingSize)
    ));
    assert_ne!(old, bad);
}
struct Mutating<'a> {
    inner: &'a MemoryNodes,
    path: &'a Path,
    witness: &'a Checkpoint,
    selection: Selection,
    fired: Cell<bool>,
}
impl NodeStore for Mutating<'_> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        if !self.fired.replace(true) {
            let mut j = SourceJournal::open(self.path, self.witness).unwrap();
            selected(&mut j, "late", 0, &self.selection);
        }
        self.inner.get(object)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("read-only")
    }
}
#[test]
fn journal_change_during_tree_io_invalidates_observed_scope_without_a_long_transaction() {
    let f = Fixture::new();
    let selection = f
        .journal()
        .source(&key("job"))
        .unwrap()
        .unwrap()
        .current
        .unwrap()
        .selection;
    let nodes = Mutating {
        inner: &f.nodes,
        path: &f.location.root,
        witness: &f.location.minimum,
        selection,
        fired: Cell::new(false),
    };
    let snapshot = capture(&f.profile, std::slice::from_ref(&f.location), &nodes).unwrap();
    assert!(nodes.fired.get());
    assert!(!snapshot.summary().observed_scope_resolved);
    assert!(
        snapshot
            .gaps
            .iter()
            .any(|g| matches!(g, Gap::ChangedJournal { .. }))
    );
    assert!(!snapshot.view(1).unwrap().complete_claim_coverage);
}
#[test]
fn journal_observe_all_is_readonly_bounded_and_detects_unlisted_sources() {
    let f = Fixture::new();
    let before = fs::read(f.location.root.join("claims.sqlite")).unwrap();
    let observed = SourceJournal::observe_all(&f.location.root, &f.location.minimum, 4).unwrap();
    assert_eq!(observed.sources.len(), 4);
    assert!(matches!(
        SourceJournal::observe_all(&f.location.root, &f.location.minimum, 3),
        Err(source_claims::Error::Budget)
    ));
    let missing = f.temp.path().join("not-created");
    assert!(SourceJournal::observe_all(&missing, &f.location.minimum, 0).is_err());
    assert!(!missing.exists());
    assert_eq!(
        fs::read(f.location.root.join("claims.sqlite")).unwrap(),
        before
    );
}
#[test]
fn missing_replaced_and_rolled_back_journals_remain_unknown() {
    let f = Fixture::new();
    let snapshot = capture(&f.profile, &[], &f.nodes).unwrap();
    assert!(matches!(snapshot.gaps[0], Gap::MissingLocation { .. }));
    let missing = Location {
        root: f.temp.path().join("missing"),
        minimum: f.location.minimum.clone(),
    };
    let snapshot = capture(&f.profile, &[missing], &f.nodes).unwrap();
    assert!(matches!(snapshot.gaps[0], Gap::UnavailableJournal { .. }));
    let other = Fixture::new();
    let replaced = Location {
        root: other.location.root.clone(),
        minimum: f.location.minimum.clone(),
    };
    let snapshot = capture(&f.profile, &[replaced], &f.nodes).unwrap();
    assert!(matches!(snapshot.gaps[0], Gap::UnavailableJournal { .. }));
    let rollback = f.temp.path().join("rollback");
    let mut old = SourceJournal::create(&rollback, key("project"), key("epoch")).unwrap();
    let original = fs::read(rollback.join("claims.sqlite")).unwrap();
    let selection = f
        .journal()
        .source(&key("main"))
        .unwrap()
        .unwrap()
        .current
        .unwrap()
        .selection;
    selected(&mut old, "main", 0, &selection);
    let witness = old.checkpoint();
    let mut profile = f.profile.clone();
    profile.journals[0].identity = old.identity().clone();
    profile.journals[0]
        .sources
        .retain(|source| source.key.as_str() == "main");
    drop(old);
    fs::write(rollback.join("claims.sqlite"), &original).unwrap();
    let location = Location {
        root: rollback.clone(),
        minimum: witness,
    };
    let snapshot = capture(&profile, &[location], &f.nodes).unwrap();
    assert!(matches!(snapshot.gaps[0], Gap::UnavailableJournal { .. }));
    assert_eq!(fs::read(rollback.join("claims.sqlite")).unwrap(), original);
}
#[test]
fn duplicate_enrollment_and_unbounded_profiles_refuse_before_io() {
    let mut f = Fixture::new();
    let source = f.profile.journals[0].sources[0].clone();
    f.profile.journals[0].sources.push(source);
    assert!(matches!(
        capture(&f.profile, &[], &f.nodes),
        Err(Error::InvalidProfile)
    ));
    let mut f = Fixture::new();
    let journal = f.profile.journals[0].clone();
    f.profile.journals.push(journal);
    assert!(f.profile.id().is_err());
    let mut f = Fixture::new();
    f.profile.limits.bindings = 500001;
    assert!(f.profile.id().is_err());
}
#[test]
fn gap_samples_are_bounded_and_still_report_total_unknown_scope() {
    let mut f = Fixture::new();
    let base = f.profile.journals[0].sources[0].clone();
    for n in 0..80 {
        let mut missing = base.clone();
        missing.key = key(&format!("missing-{n}"));
        f.profile.journals[0].sources.push(missing);
    }
    let snapshot = f.capture();
    let summary = snapshot.summary();
    assert_eq!(summary.gap_count, 80);
    assert_eq!(summary.gap_samples.len(), 32);
    assert!(summary.gap_samples_truncated);
    assert!(!summary.observed_scope_resolved);
}

#[test]
fn core_view_limit_refuses_without_truncating_a_larger_demand_snapshot() {
    let mut f = Fixture::new();
    let bindings = (0..33_000)
        .map(|n| {
            (
                LogicalPath::parse(&format!("item-{n:05}")).unwrap(),
                Object::from_bytes(format!("object-{n}").as_bytes()),
            )
        })
        .collect();
    let (tree, _) = Tree::build(&bindings, &mut f.nodes, TreeLimits::default()).unwrap();
    let selection = Selection {
        root: tree.root().clone(),
        context: digest("large"),
    };
    selected(&mut f.journal(), "job", 2, &selection);
    f.profile.limits.millis = 180_000;
    f.profile.limits.bindings = 100_000;
    f.profile.limits.associations = 200_000;
    f.profile.limits.reads = 500_000;
    f.profile.limits.read_bytes = 128 * 1024 * 1024;
    let snapshot = f.capture();
    assert!(snapshot.summary().observed_scope_resolved);
    assert!(snapshot.demands.len() > 65_536);
    assert!(matches!(snapshot.view(1), Err(Error::Budget)));
}

#[test]
fn explicitly_retired_opaque_source_remains_registered_without_a_payload_gap() {
    let mut f = Fixture::new();
    f.profile.journals[0].sources[0].format = Format::Opaque;
    assert!(!f.capture().summary().observed_scope_resolved);
    let mut j = f.journal();
    let source = j.source(&key("main")).unwrap().unwrap();
    j.apply(
        key("abort-main"),
        Command::Abort {
            source: key("main"),
            owner: key("owner"),
            expected_revision: source.revision,
            prepared: source.pending.unwrap().reference,
        },
    )
    .unwrap();
    let source = j.source(&key("main")).unwrap().unwrap();
    j.apply(
        key("withdraw-main"),
        Command::Withdraw {
            source: key("main"),
            owner: key("owner"),
            expected_revision: source.revision,
        },
    )
    .unwrap();
    let old = j.source(&key("main")).unwrap().unwrap().predecessors;
    for version in old {
        let revision = j.source(&key("main")).unwrap().unwrap().revision;
        j.apply(
            key(&format!("retire-main-{}", version.revision)),
            Command::Retire {
                source: key("main"),
                owner: key("owner"),
                expected_revision: revision,
                predecessor_revision: version.revision,
            },
        )
        .unwrap();
    }
    drop(j);
    let snapshot = f.capture();
    assert!(snapshot.summary().observed_scope_resolved);
    assert!(
        !snapshot
            .origins
            .values()
            .any(|origin| origin.source.as_str() == "main")
    );
    assert!(
        snapshot
            .origins
            .values()
            .any(|origin| origin.kind == Kind::Job)
    );
    assert!(
        SourceJournal::observe_all(&f.location.root, &f.location.minimum, 4)
            .unwrap()
            .sources
            .contains_key("main")
    );
}

#[test]
fn observed_demand_basis_drives_existing_copy_and_changed_claims_conflict() {
    use crate::{
        AckLevel, Request,
        ledger::CapacityLedger,
        local::LocalStore,
        local_capacity::LocalCapacityProbe,
        orchestration::{Budget, Copy, Hold, LocalExecution, Place, Plan, PlanRequest},
        publish,
    };
    let f = Fixture::new();
    let snapshot = f.capture();
    let basis = snapshot.copy_basis(digest("source-context")).unwrap();
    assert_eq!(&basis.claims, snapshot.id());
    let mut source = LocalStore::create(&f.temp.path().join("source"), key("source")).unwrap();
    let destination =
        LocalStore::create(&f.temp.path().join("destination"), key("destination")).unwrap();
    let mut ledger = CapacityLedger::create(
        &f.temp.path().join("capacity"),
        key("capacity"),
        key("epoch"),
        0,
    )
    .unwrap();
    let request = Request {
        operation: key("original"),
        owner: key("owner"),
        object: Object::from_bytes(b"current"),
        required: AckLevel::ProcessRestart,
    };
    assert!(snapshot.demands.keys().any(|p| p.object == request.object));
    publish(&mut source, &request, &mut std::io::Cursor::new(b"current")).unwrap();
    let plan = Plan::new(PlanRequest {
        source: Place::local(&source, 1).unwrap(),
        destination: Place::local(&destination, 1).unwrap(),
        capacity: ledger.identity().clone(),
        owner: key("owner"),
        basis: basis.clone(),
        budget: Budget {
            max_objects: 1,
            max_payload_bytes: 1024,
            max_peak_bytes: 32 * 1024 * 1024,
        },
        copies: vec![Copy {
            source: request,
            operation: key("copy-current"),
        }],
    })
    .unwrap();
    assert!(plan.holds().contains(&Hold::IncompleteProtectionScope));
    LocalCapacityProbe::bind(&destination, &mut ledger)
        .unwrap()
        .observe(&destination, &mut ledger, key("probe"), key("sample"))
        .unwrap();
    let job = ledger
        .reserve(
            key("job"),
            key("owner"),
            plan.local_peak(&destination).unwrap(),
        )
        .unwrap()
        .reference()
        .clone();
    let mut execution = LocalExecution {
        source: &source,
        destination: &destination,
        ledger: &mut ledger,
        admitted_source: &plan.request().source,
        admitted_destination: &plan.request().destination,
        admitted_basis: &basis,
    };
    drop(execution.apply(&plan, 0, &job).unwrap());
    assert!(source.read_object(&Object::from_bytes(b"current")).is_ok());
    let mut j = f.journal();
    j.apply(
        key("withdraw-job"),
        Command::Withdraw {
            source: key("job"),
            owner: key("owner"),
            expected_revision: 2,
        },
    )
    .unwrap();
    let newer = f.capture().copy_basis(digest("source-context")).unwrap();
    assert_ne!(newer.claims, basis.claims);
    let mut changed = plan.request().clone();
    changed.basis = newer.clone();
    let changed = Plan::new(changed).unwrap();
    execution.admitted_basis = &newer;
    assert!(execution.apply(&changed, 0, &job).is_err());
}

struct Slow<'a>(&'a MemoryNodes);
impl NodeStore for Slow<'_> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        std::thread::sleep(Duration::from_millis(5));
        self.0.get(object)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("read-only")
    }
}
#[test]
fn elapsed_budget_refuses_late_capture_and_invalid_windows_before_io() {
    let mut f = Fixture::new();
    f.profile.limits.millis = 0;
    assert!(matches!(
        capture(&f.profile, &[], &f.nodes),
        Err(Error::Budget)
    ));
    f.profile.limits.millis = 600_001;
    assert!(f.profile.id().is_err());
    f.profile.limits.millis = 1;
    assert!(matches!(
        capture(
            &f.profile,
            std::slice::from_ref(&f.location),
            &Slow(&f.nodes)
        ),
        Err(Error::Budget)
    ));
}

#[test]
fn repeated_identical_roots_keep_exact_selection_revisions_in_provenance() {
    let f = Fixture::new();
    let selection = f
        .journal()
        .source(&key("job"))
        .unwrap()
        .unwrap()
        .current
        .unwrap()
        .selection;
    selected(&mut f.journal(), "job", 2, &selection);
    selected(&mut f.journal(), "job", 4, &selection);
    let snapshot = f.capture();
    let revisions = snapshot
        .origins
        .values()
        .filter(|origin| origin.source.as_str() == "job")
        .map(|origin| match origin.revision {
            Revision::Selected { revision } => revision,
            _ => panic!("selected job"),
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(revisions, [2, 4, 6].into_iter().collect());
    assert!(
        snapshot
            .origins
            .values()
            .any(|origin| matches!(origin.revision, Revision::Prepared { .. }))
    );
    assert!(snapshot.summary().observed_scope_resolved);
}
