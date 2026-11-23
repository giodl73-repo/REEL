use super::*;
use crate::{
    existing::ByteSource,
    local_capacity::LocalCapacityProbe,
    source_claims::{Command, Kind, Places, Selection},
    store_protection::Binding,
};
use git_asset_core::{
    LogicalPath,
    tree::{MemoryNodes, Tree, TreeLimits},
};
use std::{fs, io::Cursor as Reader};

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn limits() -> Limits {
    Limits {
        millis: 60000,
        roots: 256,
        bindings: 10000,
        associations: 100000,
        reads: 100000,
        read_bytes: 32 * 1024 * 1024,
    }
}
struct Seed {
    bytes: BTreeMap<Digest, Vec<u8>>,
    opens: usize,
    offline: bool,
}
impl ByteSource for Seed {
    type Reader = Reader<Vec<u8>>;
    fn open_verified(&mut self, object: &Object) -> crate::existing::Result<Self::Reader> {
        self.opens += 1;
        if self.offline {
            return Err(crate::existing::Error::Missing);
        }
        let bytes = self.bytes.get(&object.sha256).unwrap();
        assert_eq!(&Object::from_bytes(bytes), object);
        Ok(Reader::new(bytes.clone()))
    }
}
struct Fixture {
    temp: tempfile::TempDir,
    namespaces: Vec<Namespace>,
    nodes: MemoryNodes,
    stores: Vec<LocalStore>,
    ledgers: Vec<CapacityLedger>,
    jobs: Vec<JournalRef>,
    config: Config,
    seed: Seed,
    old: Object,
    new: Object,
    shared: Object,
}

#[test]
fn publication_identity_is_readonly_and_refuses_foreign_provider_binding() {
    let f = Fixture::new();
    let before = f
        .ledgers
        .iter()
        .map(CapacityLedger::checkpoint)
        .collect::<Vec<_>>();
    let request = publication(&f.config, &f.config.providers[0], &f.stores[0], &f.old).unwrap();
    assert_eq!(request.object, f.old);
    assert!(!f.present(0, &f.old)); // identity is not custody
    let mut foreign = f.config.providers[0].clone();
    foreign.capacity = Digest::blob(b"foreign capacity");
    assert!(matches!(
        publication(&f.config, &foreign, &f.stores[0], &f.old),
        Err(Error::WrongEnrollment)
    ));
    assert!(matches!(
        publication(&f.config, &f.config.providers[0], &f.stores[1], &f.old),
        Err(Error::WrongEnrollment)
    ));
    assert_eq!(
        f.ledgers
            .iter()
            .map(CapacityLedger::checkpoint)
            .collect::<Vec<_>>(),
        before
    );
    assert_eq!(f.seed.opens, 0);
}
impl Fixture {
    fn new() -> Self {
        Self::configured(false, false)
    }
    fn configured(roster: bool, additive: bool) -> Self {
        Self::configured_group(roster, additive, roster)
    }
    fn configured_group(roster: bool, additive: bool, multiple: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let p = Places {
            current: vec![key("working"), key("backup")],
            pending: vec![key("working"), key("backup")],
            predecessor: vec![key("history"), key("backup")],
        };
        let policy = crate::source_claims::admission::Policy {
            prefer_active: !additive,
            classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
                .into_iter()
                .map(|k| (k, p.clone()))
                .collect(),
        };
        let mut ns = Namespace::create(
            &temp.path().join("namespace"),
            key("project"),
            key("epoch"),
            policy.clone(),
        )
        .unwrap();
        let nodes = MemoryNodes::default();
        for (source, class) in [("main", Kind::Branch), ("job", Kind::Job)] {
            ns.apply(
                key(&format!("enroll-{source}")),
                Command::Enroll {
                    source: key(source),
                    owner: key("owner"),
                    class,
                },
                &nodes,
                &limits(),
            )
            .unwrap();
        }
        let mut namespaces = vec![ns];
        if multiple {
            let mut peer = Namespace::create(
                &temp.path().join("job-namespace"),
                key("job-project"),
                key("epoch"),
                policy,
            )
            .unwrap();
            peer.apply(
                key("enroll-job"),
                Command::Enroll {
                    source: key("job"),
                    owner: key("owner"),
                    class: Kind::Job,
                },
                &nodes,
                &limits(),
            )
            .unwrap();
            namespaces.push(peer);
        }
        let authorities = namespaces
            .iter()
            .map(|ns| Authority {
                namespace: ns.identity().clone(),
                policy: digest(ns.policy()).unwrap(),
            })
            .collect();
        let mut stores = Vec::new();
        let mut ledgers = Vec::new();
        let mut jobs = Vec::new();
        let mut providers = Vec::new();
        for (name, role) in [
            ("working", Role::Cache),
            ("history", Role::Cache),
            ("backup", Role::Retained),
        ] {
            let root = temp.path().join(name);
            let store = if role == Role::Cache {
                let constructor = if roster {
                    Binding::new_roster
                } else {
                    Binding::new
                };
                let binding = constructor(
                    1,
                    Digest::blob(b"trusted boundary"),
                    namespaces
                        .iter()
                        .map(|ns| Member {
                            namespace: ns.identity().clone(),
                            places: vec![key(name)],
                        })
                        .collect(),
                )
                .unwrap();
                LocalStore::create_managed(&root, key(name), binding).unwrap()
            } else {
                LocalStore::create(&root, key(name)).unwrap()
            };
            let mut ledger = CapacityLedger::create(
                &temp.path().join(format!("ledger-{name}")),
                key(name),
                key("epoch"),
                0,
            )
            .unwrap();
            LocalCapacityProbe::bind(&store, &mut ledger)
                .unwrap()
                .observe(&store, &mut ledger, key("probe"), key("sample"))
                .unwrap();
            let job = ledger
                .reserve(key("controller"), key("owner"), 256 * 1024 * 1024)
                .unwrap()
                .reference()
                .clone();
            providers.push(Provider {
                place: Place::local(&store, 1).unwrap(),
                role,
                failure_domain: key(name),
                capacity: ledger.identity().clone(),
                job: job.clone(),
                assignments: namespaces
                    .iter()
                    .map(|ns| Mapping {
                        namespace: ns.identity().clone(),
                        place: key(name),
                    })
                    .collect(),
            });
            stores.push(store);
            ledgers.push(ledger);
            jobs.push(job);
        }
        let config = Config {
            schema: if roster {
                "tig.placement-enrollment.design-2"
            } else {
                "tig.placement-enrollment.design-1"
            }
            .into(),
            owner: key("owner"),
            authority: Digest::blob(b"owner"),
            seed: Digest::blob(b"verified source"),
            providers,
            namespaces: authorities,
            minimum_domains: 1,
            budget: Budget {
                objects: 32,
                max_object_bytes: 8 * 1024 * 1024,
                max_stream_bytes: 32 * 1024 * 1024,
                max_peak_bytes: 256 * 1024 * 1024,
                limits: limits(),
            },
        };
        let seed = Seed {
            bytes: [b"old".to_vec(), b"new".to_vec(), b"shared".to_vec()]
                .into_iter()
                .map(|b| (Object::from_bytes(&b).sha256, b))
                .collect(),
            opens: 0,
            offline: false,
        };
        Self {
            temp,
            namespaces,
            nodes,
            stores,
            ledgers,
            jobs,
            config,
            seed,
            old: Object::from_bytes(b"old"),
            new: Object::from_bytes(b"new"),
            shared: Object::from_bytes(b"shared"),
        }
    }
    fn select(&mut self, source: &str, objects: &[Object]) {
        self.select_at(0, source, objects)
    }
    fn select_at(&mut self, index: usize, source: &str, objects: &[Object]) {
        let entries = objects
            .iter()
            .enumerate()
            .map(|(i, o)| (LogicalPath::parse(&format!("item-{i}")).unwrap(), o.clone()))
            .collect();
        let (tree, _) = Tree::build(&entries, &mut self.nodes, TreeLimits::default()).unwrap();
        let n = &mut self.namespaces[index];
        let revision = n.source(&key(source)).unwrap().unwrap().revision;
        n.select(
            &crate::source_claims::admission::Select {
                prepare: key(&format!("prepare-{source}-{revision}")),
                adopt: key(&format!("adopt-{source}-{revision}")),
                source: key(source),
                owner: key("owner"),
                expected_revision: revision,
                selection: Selection {
                    root: tree.root().clone(),
                    context: Digest::blob(b"source"),
                },
            },
            &self.nodes,
            &limits(),
        )
        .unwrap();
    }
    fn run<T>(&mut self, f: impl FnOnce(&mut Execution<'_, '_, Seed, MemoryNodes>) -> T) -> T {
        let mut places = self
            .stores
            .iter()
            .zip(self.ledgers.iter_mut())
            .zip(self.jobs.iter())
            .map(|((store, ledger), job)| Local { store, ledger, job })
            .collect::<Vec<_>>();
        let seed_identity = self.config.seed.clone();
        f(&mut Execution {
            config: &self.config,
            namespaces: &mut self.namespaces,
            nodes: &self.nodes,
            places: &mut places,
            seed: &mut self.seed,
            seed_identity: &seed_identity,
            allow_copy: true,
            allow_eviction: true,
            allow_reacquisition: true,
            allow_cancellation: true,
        })
    }
    fn apply(&mut self) -> Report {
        self.run(|e| {
            let p = e.plan(None).unwrap();
            e.apply(&p).unwrap()
        })
    }
    fn present(&self, index: usize, object: &Object) -> bool {
        self.stores[index].read_object(object).is_ok()
    }
}

#[test]
fn replacement_job_end_demotion_and_revert_reuse_original_placements() {
    let mut f = Fixture::new();
    f.select("main", &[f.old.clone(), f.shared.clone()]);
    let first = f.apply();
    assert_eq!(first.storage_converged, Some(true), "{first:?}");
    f.select("job", &[f.old.clone()]);
    f.select("main", &[f.new.clone(), f.shared.clone()]);
    let active = f.apply();
    assert_eq!(active.storage_converged, Some(true), "{active:?}");
    assert!(f.present(0, &f.old));
    assert!(!f.present(1, &f.old));
    let ns = &mut f.namespaces[0];
    let rev = ns.source(&key("job")).unwrap().unwrap().revision;
    ns.apply(
        key("job-end"),
        Command::Withdraw {
            source: key("job"),
            owner: key("owner"),
            expected_revision: rev,
        },
        &f.nodes,
        &limits(),
    )
    .unwrap();
    let demoted = f.apply();
    assert_eq!(demoted.storage_converged, Some(true), "{demoted:?}");
    assert!(!f.present(0, &f.old));
    assert!(f.present(1, &f.old));
    assert!(f.present(2, &f.old));
    assert!(!f.present(1, &f.shared));
    let before = f
        .ledgers
        .iter()
        .map(CapacityLedger::checkpoint)
        .collect::<Vec<_>>();
    f.seed.offline = true;
    let no_op = f.apply();
    assert_eq!(no_op.storage_converged, Some(true), "{no_op:?}");
    assert!(no_op.effects.is_empty());
    assert_eq!(no_op.stream_bytes, 0);
    assert_eq!(
        before,
        f.ledgers
            .iter()
            .map(CapacityLedger::checkpoint)
            .collect::<Vec<_>>()
    );
    f.select("main", &[f.old.clone(), f.shared.clone()]);
    let restored = f.apply();
    assert_eq!(restored.storage_converged, Some(true), "{restored:?}");
    assert!(f.present(0, &f.old));
    assert!(!f.present(1, &f.old));
    assert!(
        restored
            .effects
            .iter()
            .any(|e| e.kind == EffectKind::Reacquired)
    );
}

#[test]
fn altered_plan_cannot_claim_convergence_or_apply() {
    let mut f = Fixture::new();
    f.select("main", &[f.old.clone()]);
    let mut p = f.run(|e| e.plan(None).unwrap());
    p.rows[0].required.fill(false);
    assert_eq!(f.run(|e| e.status(&p)).unwrap_err(), Error::Invalid);
    assert_eq!(f.run(|e| e.apply(&p)).unwrap_err(), Error::Stale);
    assert_eq!(f.seed.opens, 0);
}

#[test]
fn roster_precedence_job_start_and_end_reconcile_both_plan_and_guarded_eviction() {
    let mut f = Fixture::configured(true, false);
    f.select("main", &[f.old.clone(), f.shared.clone()]);
    assert_eq!(f.apply().storage_converged, Some(true));
    f.select("main", &[f.new.clone(), f.shared.clone()]);
    assert_eq!(f.apply().storage_converged, Some(true));
    assert!(!f.present(0, &f.old));
    assert!(f.present(1, &f.old));
    f.select_at(1, "job", &[f.old.clone()]);
    let p = f.run(|e| e.plan(None).unwrap());
    assert_eq!(
        p.rows.iter().find(|r| r.object == f.old).unwrap().required,
        vec![true, false, true]
    );
    let active = f.apply();
    assert_eq!(active.storage_converged, Some(true), "{active:?}");
    assert!(f.present(0, &f.old));
    assert!(!f.present(1, &f.old));
    assert!(f.present(2, &f.old));
    assert!(!f.present(1, &f.shared));
    let n = &mut f.namespaces[1];
    let revision = n.source(&key("job")).unwrap().unwrap().revision;
    n.apply(
        key("finish-job"),
        Command::Withdraw {
            source: key("job"),
            owner: key("owner"),
            expected_revision: revision,
        },
        &f.nodes,
        &limits(),
    )
    .unwrap();
    let inactive = f.apply();
    assert_eq!(inactive.storage_converged, Some(true), "{inactive:?}");
    assert!(!f.present(0, &f.old));
    assert!(f.present(1, &f.old));
    assert!(f.present(2, &f.old));
}

#[test]
fn roster_precedence_keeps_explicitly_additive_history_and_retention_class_demands() {
    for additive in [false, true] {
        let mut f = Fixture::configured(true, additive);
        f.select("main", &[f.old.clone()]);
        f.select("main", &[f.new.clone()]);
        f.select_at(1, "job", &[f.old.clone()]);
        if !additive {
            f.namespaces[0]
                .apply(
                    key("enroll-hard"),
                    Command::Enroll {
                        source: key("hard"),
                        owner: key("owner"),
                        class: Kind::Retention,
                    },
                    &f.nodes,
                    &limits(),
                )
                .unwrap();
            f.select("hard", &[f.old.clone()]);
            f.select("hard", &[f.new.clone()]);
        }
        let p = f.run(|e| e.plan(None).unwrap());
        assert_eq!(
            p.rows.iter().find(|r| r.object == f.old).unwrap().required,
            vec![true, true, true]
        );
        let report = f.apply();
        assert_eq!(report.storage_converged, Some(true), "{report:?}");
        assert!((0..3).all(|i| f.present(i, &f.old)));
    }
}

#[test]
fn roster_semantics_require_matching_fresh_store_enrollment() {
    let mut old = Fixture::new();
    old.config.schema = "tig.placement-enrollment.design-2".into();
    assert_eq!(
        old.run(|e| e.plan(None)).unwrap_err(),
        Error::WrongEnrollment
    );
    let mut new = Fixture::configured(true, false);
    let new_id = new.config.id().unwrap();
    new.config.schema = "tig.placement-enrollment.design-1".into();
    assert_ne!(new.config.id().unwrap(), new_id);
    assert_eq!(
        new.run(|e| e.plan(None)).unwrap_err(),
        Error::WrongEnrollment
    );
}

#[test]
fn legacy_namespace_scope_preserves_additive_assignments_across_namespaces() {
    let mut f = Fixture::configured_group(false, false, true);
    f.select("main", &[f.old.clone()]);
    f.select("main", &[f.new.clone()]);
    f.select_at(1, "job", &[f.old.clone()]);
    let p = f.run(|e| e.plan(None).unwrap());
    assert_eq!(
        p.rows.iter().find(|r| r.object == f.old).unwrap().required,
        vec![true, true, true]
    );
    let report = f.apply();
    assert_eq!(report.storage_converged, Some(true), "{report:?}");
    assert!((0..3).all(|i| f.present(i, &f.old)));
}

#[test]
fn roster_projection_keeps_one_budget_and_refuses_missing_closure_or_conflicting_sizes() {
    let mut f = Fixture::configured(true, false);
    f.select("main", &[f.old.clone()]);
    f.select_at(1, "job", &[f.old.clone()]);
    let before = f
        .ledgers
        .iter()
        .map(CapacityLedger::checkpoint)
        .collect::<Vec<_>>();
    f.config.budget.limits.roots = 1;
    assert_eq!(f.run(|e| e.plan(None)).unwrap_err(), Error::Budget);
    f.config.budget.limits = limits();
    let nodes = std::mem::take(&mut f.nodes);
    assert_eq!(f.run(|e| e.plan(None)).unwrap_err(), Error::Recovery);
    f.nodes = nodes;
    let mut wrong = f.old.clone();
    wrong.bytes += 1;
    let mut g = Fixture::configured(true, false);
    g.select("main", &[g.old.clone()]);
    g.select_at(1, "job", &[wrong]);
    assert_eq!(g.run(|e| e.plan(None)).unwrap_err(), Error::Recovery);
    assert_eq!(g.seed.opens, 0);
    assert_eq!(f.seed.opens, 0);
    assert_eq!(
        before,
        f.ledgers
            .iter()
            .map(CapacityLedger::checkpoint)
            .collect::<Vec<_>>()
    );
}

#[test]
fn roster_assessment_does_not_clear_unknown_closure_and_releases_all_fences_on_failure() {
    let mut f = Fixture::configured(true, false);
    f.select("main", &[f.old.clone()]);
    f.select("main", &[f.new.clone()]);
    assert_eq!(f.apply().storage_converged, Some(true));
    f.select_at(1, "job", &[f.old.clone()]);
    let enrollment = crate::store_protection::Enrollment::bind(&f.stores[1]).unwrap();
    assert!(matches!(
        crate::store_protection::assess(
            &f.stores[1],
            &enrollment,
            &f.old,
            &mut f.namespaces,
            &MemoryNodes::default(),
            &limits()
        ),
        Err(crate::store_protection::Error::Admission(
            crate::source_claims::admission::Error::IncompleteClosure
        ))
    ));
    for ns in &f.namespaces {
        drop(ns.fence(&f.old).unwrap());
    }
    assert!(f.present(1, &f.old));
}

#[test]
fn roster_assessment_fences_loss_of_remote_active_use_and_rejects_missing_members() {
    let mut f = Fixture::configured(true, false);
    f.select("main", &[f.old.clone()]);
    f.select("main", &[f.new.clone()]);
    assert_eq!(f.apply().storage_converged, Some(true));
    f.select_at(1, "job", &[f.old.clone()]);
    let mut peer = Namespace::open(
        &f.temp.path().join("job-namespace"),
        &f.namespaces[1].checkpoint(),
    )
    .unwrap();
    let revision = peer.source(&key("job")).unwrap().unwrap().revision;
    let command = Command::Withdraw {
        source: key("job"),
        owner: key("owner"),
        expected_revision: revision,
    };
    let enrollment = crate::store_protection::Enrollment::bind(&f.stores[1]).unwrap();
    {
        let assessment = crate::store_protection::assess(
            &f.stores[1],
            &enrollment,
            &f.old,
            &mut f.namespaces,
            &f.nodes,
            &limits(),
        )
        .unwrap();
        assert!(matches!(
            peer.apply(key("finish-peer"), command.clone(), &f.nodes, &limits()),
            Err(crate::source_claims::admission::Error::Busy)
        ));
        assessment.check_current().unwrap();
    }
    peer.apply(key("finish-peer"), command, &f.nodes, &limits())
        .unwrap();
    assert!(matches!(
        crate::store_protection::assess(
            &f.stores[1],
            &enrollment,
            &f.old,
            &mut f.namespaces,
            &f.nodes,
            &limits()
        ),
        Err(crate::store_protection::Error::Required)
    ));
    let member = f.namespaces.pop().unwrap();
    assert!(matches!(
        crate::store_protection::assess(
            &f.stores[1],
            &enrollment,
            &f.old,
            &mut f.namespaces,
            &f.nodes,
            &limits()
        ),
        Err(crate::store_protection::Error::IncompleteRoster)
    ));
    f.namespaces.push(member);
}

#[test]
fn roster_peer_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_ROSTER_PEER_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let checkpoint =
        serde_json::from_slice(&fs::read(root.join("peer-minimum.json")).unwrap()).unwrap();
    let mut peer = Namespace::open(&root.join("job-namespace"), &checkpoint).unwrap();
    let captured: Vec<Vec<u8>> =
        serde_json::from_slice(&fs::read(root.join("peer-nodes.json")).unwrap()).unwrap();
    let mut nodes = MemoryNodes::default();
    for bytes in captured {
        nodes.put(&bytes).unwrap();
    }
    let revision = peer.source(&key("job")).unwrap().unwrap().revision;
    let result = peer.apply(
        key("child-finish"),
        Command::Withdraw {
            source: key("job"),
            owner: key("owner"),
            expected_revision: revision,
        },
        &nodes,
        &limits(),
    );
    if matches!(result, Err(crate::source_claims::admission::Error::Busy)) {
        std::process::exit(77);
    }
    if result.is_ok() {
        std::process::exit(78);
    }
    panic!("unexpected child result {result:?}");
}

#[test]
fn actual_process_cannot_withdraw_remote_active_use_until_roster_assessment_releases() {
    let mut f = Fixture::configured(true, false);
    f.select("main", &[f.old.clone()]);
    f.select("main", &[f.new.clone()]);
    assert_eq!(f.apply().storage_converged, Some(true));
    f.select_at(1, "job", &[f.old.clone()]);
    fs::write(
        f.temp.path().join("peer-minimum.json"),
        serde_json::to_vec(&f.namespaces[1].checkpoint()).unwrap(),
    )
    .unwrap();
    let captured = f
        .nodes
        .objects()
        .map(|(_, bytes)| bytes.to_vec())
        .collect::<Vec<_>>();
    fs::write(
        f.temp.path().join("peer-nodes.json"),
        serde_json::to_vec(&captured).unwrap(),
    )
    .unwrap();
    let child = || {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "placement::tests::roster_peer_child",
                "--nocapture",
            ])
            .env("GIT_ASSET_ROSTER_PEER_ROOT", f.temp.path())
            .output()
            .unwrap()
    };
    let enrollment = crate::store_protection::Enrollment::bind(&f.stores[1]).unwrap();
    {
        let assessment = crate::store_protection::assess(
            &f.stores[1],
            &enrollment,
            &f.old,
            &mut f.namespaces,
            &f.nodes,
            &limits(),
        )
        .unwrap();
        let output = child();
        assert_eq!(output.status.code(), Some(77), "{output:?}");
        assessment.check_current().unwrap();
    }
    let output = child();
    assert_eq!(output.status.code(), Some(78), "{output:?}");
    assert!(matches!(
        crate::store_protection::assess(
            &f.stores[1],
            &enrollment,
            &f.old,
            &mut f.namespaces,
            &f.nodes,
            &limits()
        ),
        Err(crate::store_protection::Error::Required)
    ));
}

#[test]
fn backup_offline_keeps_ingest_and_catchup_needs_no_new_selection() {
    let mut f = Fixture::new();
    f.select("main", &[f.old.clone()]);
    let root = f.temp.path().join("backup");
    let offline = f.temp.path().join("offline-backup");
    fs::rename(&root, &offline).unwrap();
    let pending = f.apply();
    assert_eq!(pending.storage_converged, Some(false));
    assert!(f.present(0, &f.old));
    assert!(
        pending
            .observations
            .iter()
            .any(|o| o.provider == f.config.providers[0].place.instance
                && o.state == Observation::Retained)
    );
    fs::rename(offline, root).unwrap();
    f.seed.offline = true;
    let complete = f.apply();
    assert_eq!(complete.storage_converged, Some(true), "{complete:?}");
    assert!(f.present(2, &f.old));
}

#[test]
fn budgets_and_provider_order_do_not_mint_new_copy_identity() {
    let mut f = Fixture::new();
    let id = f.config.id().unwrap();
    f.config.budget.objects = 1;
    f.config.budget.max_stream_bytes = 1024;
    assert_eq!(id, f.config.id().unwrap());
    f.config.providers.reverse();
    assert_eq!(id, f.config.id().unwrap());
}

#[test]
fn pagination_reports_page_progress_without_claiming_full_convergence() {
    let mut f = Fixture::new();
    f.config.budget.objects = 1;
    f.select("main", &[f.old.clone(), f.new.clone(), f.shared.clone()]);
    let first = f.run(|e| e.plan(None).unwrap());
    let r = f.run(|e| e.apply(&first).unwrap());
    assert!(r.page_converged, "{r:?}");
    assert_eq!(r.storage_converged, None);
    let second = f.run(|e| e.plan(r.next.as_ref()).unwrap());
    let r = f.run(|e| e.apply(&second).unwrap());
    assert!(r.page_converged);
    assert_eq!(r.storage_converged, None);
    let third = f.run(|e| e.plan(r.next.as_ref()).unwrap());
    let r = f.run(|e| e.apply(&third).unwrap());
    assert!(r.page_converged);
    assert!(r.next.is_none());
    assert_eq!(r.storage_converged, None);
    f.select("main", &[f.old.clone()]);
    assert_eq!(f.run(|e| e.apply(&second)).unwrap_err(), Error::Stale);
    assert_eq!(
        f.run(|e| e.plan(first.next.as_ref())).unwrap_err(),
        Error::Stale
    );
}

#[test]
fn missing_acknowledged_backup_is_held_without_repair_or_source_eviction() {
    let mut f = Fixture::new();
    f.select("main", &[f.old.clone()]);
    assert_eq!(f.apply().storage_converged, Some(true));
    fs::remove_file(
        f.temp
            .path()
            .join("backup/objects")
            .join(f.old.sha256.as_str()),
    )
    .unwrap();
    f.select("main", &[f.new.clone()]);
    let opens = f.seed.opens;
    let result = f.apply();
    assert_eq!(result.storage_converged, Some(false));
    assert!(f.present(0, &f.old));
    assert!(!f.present(2, &f.old));
    // The new version can make safe progress; the lost acknowledged old backup
    // is never silently recreated from the remaining good source.
    assert!(
        result
            .observations
            .iter()
            .any(|o| o.object == f.old && matches!(o.state, Observation::Held(_)))
    );
    assert!(f.seed.opens <= opens + 1);
}

#[test]
fn reader_blocks_eviction_and_retry_finishes_without_repeating_copies() {
    let mut f = Fixture::new();
    f.select("main", &[f.old.clone()]);
    f.apply();
    let reader = f.stores[0].read_object(&f.old).unwrap();
    f.select("main", &[f.new.clone()]);
    let blocked = f.apply();
    assert_eq!(blocked.storage_converged, Some(false));
    assert!(f.present(0, &f.old));
    assert!(f.present(1, &f.old));
    assert!(f.present(0, &f.new));
    assert!(f.present(2, &f.new));
    drop(reader);
    f.seed.offline = true;
    let done = f.apply();
    assert_eq!(done.storage_converged, Some(true), "{done:?}");
    assert!(!f.present(0, &f.old));
    assert_eq!(done.stream_bytes, 0);
}

#[test]
fn late_job_claim_fences_old_plan_eviction() {
    let mut f = Fixture::new();
    f.select("main", &[f.old.clone()]);
    f.apply();
    f.select("main", &[f.new.clone()]);
    let root = f.temp.path().join("namespace");
    let minimum = f.namespaces[0].checkpoint();
    let mut nodes = MemoryNodes::default();
    for object in [&f.old, &f.new] {
        let entries = [(LogicalPath::parse("item-0").unwrap(), object.clone())]
            .into_iter()
            .collect();
        Tree::build(&entries, &mut nodes, TreeLimits::default()).unwrap();
    }
    let entries = [(LogicalPath::parse("job-input").unwrap(), f.old.clone())]
        .into_iter()
        .collect();
    let (tree, _) = Tree::build(&entries, &mut nodes, TreeLimits::default()).unwrap();
    Tree::build(&entries, &mut f.nodes, TreeLimits::default()).unwrap();
    let old = f.old.clone();
    let mut injected = false;
    let result = f.run(|e| {
        let p = e.plan(None).unwrap();
        e.apply_hook(&p, |object, index| {
            if object == &old && index == 1 && !injected {
                let mut peer = Namespace::open(&root, &minimum).unwrap();
                let revision = peer.source(&key("job")).unwrap().unwrap().revision;
                peer.select(
                    &crate::source_claims::admission::Select {
                        prepare: key("late-prepare"),
                        adopt: key("late-adopt"),
                        source: key("job"),
                        owner: key("owner"),
                        expected_revision: revision,
                        selection: Selection {
                            root: tree.root().clone(),
                            context: Digest::blob(b"late job"),
                        },
                    },
                    &nodes,
                    &limits(),
                )
                .unwrap();
                injected = true;
            }
            Ok(())
        })
        .unwrap()
    });
    assert!(injected);
    assert!(!result.basis_current);
    assert_eq!(result.storage_converged, Some(false));
    assert!(f.present(0, &f.old));
    assert!(result.holds.contains(&Error::Protected), "{result:?}");
}

#[test]
fn placement_process_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_PLACEMENT_FIXTURE_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("placement-fixture.json")).unwrap()).unwrap();
    let config: Config = serde_json::from_value(value["config"].clone()).unwrap();
    let minimum = serde_json::from_value(value["namespace"].clone()).unwrap();
    let mut namespaces = vec![Namespace::open(&root.join("namespace"), &minimum).unwrap()];
    let mut stores = Vec::new();
    let mut ledgers = Vec::new();
    let mut jobs = Vec::new();
    for (index, name) in ["working", "history", "backup"].iter().enumerate() {
        stores.push(LocalStore::open(&root.join(name)).unwrap());
        let witness = serde_json::from_value(value["ledgers"][index].clone()).unwrap();
        ledgers.push(CapacityLedger::open(&root.join(format!("ledger-{name}")), &witness).unwrap());
        jobs.push(config.providers[index].job.clone());
    }
    let mut nodes = MemoryNodes::default();
    let entries = [(
        LogicalPath::parse("item-0").unwrap(),
        Object::from_bytes(b"old"),
    )]
    .into_iter()
    .collect();
    Tree::build(&entries, &mut nodes, TreeLimits::default()).unwrap();
    let mut seed = Seed {
        bytes: [(Object::from_bytes(b"old").sha256, b"old".to_vec())]
            .into_iter()
            .collect(),
        opens: 0,
        offline: false,
    };
    let mut places = stores
        .iter()
        .zip(ledgers.iter_mut())
        .zip(jobs.iter())
        .map(|((store, ledger), job)| Local { store, ledger, job })
        .collect::<Vec<_>>();
    let mut execution = Execution {
        config: &config,
        namespaces: &mut namespaces,
        nodes: &nodes,
        places: &mut places,
        seed: &mut seed,
        seed_identity: &config.seed,
        allow_copy: true,
        allow_eviction: true,
        allow_reacquisition: true,
        allow_cancellation: true,
    };
    let plan = execution.plan(None).unwrap();
    execution
        .apply_hook(&plan, |_, index| {
            if index == 2 {
                std::process::exit(63);
            }
            Ok(())
        })
        .unwrap();
    panic!("fixture did not reach exit boundary");
}

#[test]
fn child_exit_after_backup_copy_resumes_without_seed_access() {
    let mut f = Fixture::new();
    f.select("main", &[f.old.clone()]);
    let profile = serde_json::json!({"config":f.config,"namespace":f.namespaces[0].checkpoint(),"ledgers":f.ledgers.iter().map(CapacityLedger::checkpoint).collect::<Vec<_>>()});
    fs::write(
        f.temp.path().join("placement-fixture.json"),
        serde_json::to_vec(&profile).unwrap(),
    )
    .unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "placement::tests::placement_process_child",
            "--nocapture",
        ])
        .env("GIT_ASSET_PLACEMENT_FIXTURE_ROOT", f.temp.path())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(63),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(f.present(2, &f.old));
    assert!(!f.present(0, &f.old));
    f.seed.offline = true;
    let result = f.apply();
    assert_eq!(result.storage_converged, Some(true), "{result:?}");
    assert_eq!(f.seed.opens, 0);
    assert!(f.present(0, &f.old));
}

#[test]
fn byte_budget_holds_before_next_stream_and_larger_retry_reuses_backup() {
    let mut f = Fixture::new();
    f.select("main", &[f.old.clone()]);
    f.config.budget.max_stream_bytes = f.old.bytes;
    let partial = f.apply();
    assert_eq!(partial.stream_bytes, f.old.bytes);
    assert_eq!(partial.storage_converged, Some(false));
    assert!(f.present(2, &f.old));
    assert!(!f.present(0, &f.old));
    assert!(partial.holds.contains(&Error::Budget));
    let backup_checkpoint = f.ledgers[2].checkpoint();
    f.config.budget.max_stream_bytes = 1024;
    f.seed.offline = true;
    let done = f.apply();
    assert_eq!(done.storage_converged, Some(true), "{done:?}");
    assert_eq!(backup_checkpoint, f.ledgers[2].checkpoint());
}

#[test]
fn same_declared_domain_never_releases_cache_ingest_or_evicts_it() {
    let mut f = Fixture::new();
    f.config.providers[0].failure_domain = key("backup");
    f.select("main", &[f.old.clone()]);
    let result = f.apply();
    assert_eq!(result.storage_converged, Some(false));
    assert!(result.holds.contains(&Error::Protected));
    assert!(f.present(0, &f.old));
    assert!(
        result
            .observations
            .iter()
            .any(|o| o.provider == f.config.providers[0].place.instance
                && o.state == Observation::Retained)
    );
}

#[test]
fn unexplained_managed_placement_is_not_adopted_or_evicted() {
    let mut f = Fixture::new();
    f.select("main", &[f.old.clone()]);
    let foreign = crate::Request {
        operation: key("foreign"),
        owner: key("owner"),
        object: f.old.clone(),
        required: crate::AckLevel::ProcessRestart,
    };
    let plan = crate::accounted::plan(&f.stores[0], &foreign).unwrap();
    crate::accounted::publish_accounted(
        &mut f.ledgers[0],
        &f.stores[0],
        &f.jobs[0],
        &plan,
        &mut Reader::new(b"old"),
    )
    .unwrap();
    let result = f.apply();
    assert_eq!(result.storage_converged, Some(false));
    assert!(result.holds.contains(&Error::Recovery));
    assert!(f.present(0, &f.old));
    assert!(
        !result
            .effects
            .iter()
            .any(|e| e.provider == f.config.providers[0].place.instance)
    );
}

#[test]
fn renewed_demand_cancels_pending_retirement_and_keeps_original_copy_binding() {
    use crate::{replica::ReplicaEnrollment, retention, retirement};
    let mut f = Fixture::new();
    f.select("main", &[f.old.clone()]);
    f.apply();
    f.select("main", &[f.new.clone()]);
    let publication = |index: usize| crate::Request {
        operation: key(&format!(
            "place-{}",
            digest(&(
                "initial",
                f.config.id().unwrap(),
                &f.config.providers[index].place,
                &f.old
            ))
            .unwrap()
            .as_str()
        )),
        owner: key("owner"),
        object: f.old.clone(),
        required: crate::AckLevel::ProcessRestart,
    };
    let source_publication = publication(0);
    let backup_publication = publication(2);
    let retained = retention::plan(
        &f.stores[2],
        &retention::RetentionRequest {
            operation: backup_publication.operation.clone(),
            owner: key("owner"),
            publication: backup_publication,
            minimum_ack: crate::AckLevel::ProcessRestart,
        },
    )
    .unwrap();
    let grant = retention::recorded(&mut f.ledgers[2], &f.stores[2], &f.jobs[2], &retained)
        .unwrap()
        .unwrap();
    let source_enrollment = ReplicaEnrollment {
        store: f.config.providers[0].place.instance.clone(),
        failure_domain: key("working"),
    };
    let backup_enrollment = ReplicaEnrollment {
        store: f.config.providers[2].place.instance.clone(),
        failure_domain: key("backup"),
    };
    let context = f.config.id().unwrap();
    let plan = retirement::plan(
        &f.stores[0],
        retirement::Request {
            operation: key("pending-fixture"),
            owner: key("owner"),
            publication: source_publication,
            context: context.clone(),
            source: source_enrollment.clone(),
            keepers: vec![retirement::Keeper {
                grant,
                enrollment: backup_enrollment.clone(),
            }],
            required_domains: 1,
            capacity: f.config.providers[0].capacity.clone(),
        },
    )
    .unwrap();
    let (sources, backups) = f.ledgers.split_at_mut(2);
    let mut backups = [retirement::Backup {
        store: &f.stores[2],
        ledger: &mut backups[0],
        enrollment: &backup_enrollment,
    }];
    assert!(matches!(
        retirement::Execution {
            store: &f.stores[0],
            ledger: &mut sources[0],
            job: &f.jobs[0],
            namespaces: &mut f.namespaces,
            nodes: &f.nodes,
            limits: &limits(),
            backups: &mut backups,
            admitted_context: &context,
            source_enrollment: &source_enrollment
        }
        .fixture_pending_intent(&plan),
        Err(retirement::Error::Injected)
    ));
    assert!(f.stores[0].object_present(&f.old).unwrap());
    f.select("main", &[f.old.clone()]);
    let result = f.apply();
    assert_eq!(result.storage_converged, Some(true), "{result:?}");
    assert!(f.present(0, &f.old));
    assert_eq!(
        f.stores[0].life_head(&f.old).unwrap().unwrap().birth.index,
        1
    );
    assert!(
        result
            .effects
            .iter()
            .any(|e| e.kind == EffectKind::Cancelled)
    );
    let retry = f.apply();
    assert_eq!(retry.storage_converged, Some(true), "{retry:?}");
    assert!(retry.effects.is_empty());
}

#[test]
fn superseded_copy_resumes_after_publication_before_ingest_retention() {
    use crate::{AckLevel, Request, accounted};
    let mut f = Fixture::new();
    f.select("main", &[f.old.clone()]);
    let place = &f.config.providers[0].place;
    let id = f.config.id().unwrap();
    let publication = Request {
        operation: key(&format!(
            "place-{}",
            digest(&("initial", &id, place, &f.old)).unwrap().as_str()
        )),
        owner: key("owner"),
        object: f.old.clone(),
        required: AckLevel::ProcessRestart,
    };
    let context = digest(&("tig.placement-object.design-1", id, place, &f.old)).unwrap();
    let plan = accounted::plan_bound(&f.stores[0], &publication, context).unwrap();
    accounted::publish_accounted(
        &mut f.ledgers[0],
        &f.stores[0],
        &f.jobs[0],
        &plan,
        &mut Reader::new(b"old"),
    )
    .unwrap();
    f.select("main", &[f.new.clone()]);
    let result = f.apply();
    assert_eq!(result.storage_converged, Some(true), "{result:?}");
    assert!(!f.present(0, &f.old));
    assert!(f.present(1, &f.old));
    assert!(f.present(2, &f.old));
}

#[test]
fn provider_reordering_keeps_original_handoff_successor_with_multiple_backups() {
    let mut f = Fixture::new();
    let store =
        LocalStore::create(&f.temp.path().join("second-backup"), key("second-backup")).unwrap();
    let mut ledger = CapacityLedger::create(
        &f.temp.path().join("second-capacity"),
        key("second-backup"),
        key("epoch"),
        0,
    )
    .unwrap();
    LocalCapacityProbe::bind(&store, &mut ledger)
        .unwrap()
        .observe(&store, &mut ledger, key("probe"), key("sample"))
        .unwrap();
    let job = ledger
        .reserve(key("controller"), key("owner"), 256 * 1024 * 1024)
        .unwrap()
        .reference()
        .clone();
    f.config.providers.push(Provider {
        place: Place::local(&store, 1).unwrap(),
        role: Role::Retained,
        failure_domain: key("second-backup"),
        capacity: ledger.identity().clone(),
        job: job.clone(),
        assignments: vec![],
    });
    f.config.minimum_domains = 2;
    f.stores.push(store);
    f.ledgers.push(ledger);
    f.jobs.push(job);
    f.select("main", &[f.old.clone()]);
    let first = f.apply();
    assert_eq!(first.storage_converged, Some(true), "{first:?}");
    f.config.providers.reverse();
    f.stores.reverse();
    f.ledgers.reverse();
    f.jobs.reverse();
    f.seed.offline = true;
    let before = f
        .ledgers
        .iter()
        .map(CapacityLedger::checkpoint)
        .collect::<Vec<_>>();
    let repeated = f.apply();
    assert_eq!(repeated.storage_converged, Some(true), "{repeated:?}");
    assert!(repeated.effects.is_empty());
    assert_eq!(repeated.stream_bytes, 0);
    assert_eq!(
        before,
        f.ledgers
            .iter()
            .map(CapacityLedger::checkpoint)
            .collect::<Vec<_>>()
    );
}
