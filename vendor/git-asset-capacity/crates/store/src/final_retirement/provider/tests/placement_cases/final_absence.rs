//! Three actual providers: final-deleted history must not force an unwanted copy.
use super::*;
use crate::ledger::Checkpoint;
use std::collections::BTreeMap;
#[derive(Default)]
struct DeniedSeed {
    attempts: u64,
}
impl ByteSource for DeniedSeed {
    type Reader = Cursor<Vec<u8>>;
    fn open_verified(&mut self, _: &Object) -> crate::existing::Result<Self::Reader> {
        self.attempts += 1;
        Err(crate::existing::Error::Denied)
    }
}
fn capacity(
    store: &LocalStore,
    root: &std::path::Path,
    name: &str,
) -> (CapacityLedger, JournalRef) {
    let mut ledger = CapacityLedger::create(&root.join(name), key(name), key("epoch"), 0).unwrap();
    LocalCapacityProbe::bind(store, &mut ledger)
        .unwrap()
        .observe(store, &mut ledger, key("probe"), key("sample"))
        .unwrap();
    let job = ledger
        .reserve(key("job"), key("owner"), 256 * 1024 * 1024)
        .unwrap()
        .reference()
        .clone();
    (ledger, job)
}
struct Three {
    f: Fixture,
    history: LocalStore,
    history_ledger: CapacityLedger,
    history_job: JournalRef,
    history_delete: Plan,
    main_delete: Plan,
    cache: LocalStore,
    cache_ledger: CapacityLedger,
    config: p::Config,
    seed: DeniedSeed,
}
impl Three {
    fn new(stop: Option<Stage>) -> Self {
        Self::build(stop, None)
    }
    fn build(stop: Option<Stage>, child: Option<Stage>) -> Self {
        let mut f = Fixture::new();
        let ns = f.namespaces[0].identity().clone();
        let binding = |place: &str| {
            Binding::new(
                1,
                d(place),
                vec![Member {
                    namespace: ns.clone(),
                    places: vec![key(place)],
                }],
            )
            .unwrap()
        };
        let mut history = LocalStore::create_final(
            &f.root.path().join("history"),
            key("D"),
            binding("D"),
            f.store.final_policy().unwrap().clone(),
        )
        .unwrap();
        let (mut history_ledger, history_job) =
            capacity(&history, f.root.path(), "history-capacity");
        let publication = Publication {
            operation: key("history-ingest"),
            ..f.publication.clone()
        };
        let pp = accounted::plan(&history, &publication).unwrap();
        accounted::publish_accounted(
            &mut history_ledger,
            &history,
            &history_job,
            &pp,
            &mut Cursor::new(b"asset"),
        )
        .unwrap();
        let rp = retention::plan(
            &history,
            &retention::RetentionRequest {
                operation: key("history-pin"),
                owner: key("owner"),
                publication: publication.clone(),
                minimum_ack: AckLevel::ProcessRestart,
            },
        )
        .unwrap();
        retention::retain(&mut history_ledger, &history, &history_job, &rp).unwrap();
        let history_delete = plan(
            &history,
            Request {
                operation: key("history-delete"),
                owner: key("owner"),
                publication,
                capacity: history_ledger.identity().clone(),
                context: f.context.clone(),
                window: Window {
                    not_before: 50,
                    expires: 500,
                },
            },
        )
        .unwrap();
        if let Some(boundary) = child {
            let saved = Saved {
                plan: history_delete.clone(),
                ledger: history_ledger.checkpoint(),
                job: history_job.clone(),
                roster: f.roster.clone(),
                namespaces: f.namespaces.iter().map(Namespace::checkpoint).collect(),
            };
            let bytes = serde_json::to_vec(&saved).unwrap();
            assert!(bytes.len() <= 65536);
            fs::write(f.root.path().join("history-saved.json"), bytes).unwrap();
            let output=Process::new(std::env::current_exe().unwrap()).args(["--exact","final_retirement::provider::tests::placement_cases::final_absence::history_exit_child","--nocapture"])
                .env("GIT_ASSET_PLACEMENT_ABSENCE_CHILD",f.root.path()).env("GIT_ASSET_PLACEMENT_ABSENCE_STAGE",format!("{boundary:?}")).output().unwrap();
            assert_eq!(
                output.status.code(),
                Some(77),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            drop(history);
            drop(history_ledger);
            history = LocalStore::open(&f.root.path().join("history")).unwrap();
            history_ledger =
                CapacityLedger::open(&f.root.path().join("history-capacity"), &saved.ledger)
                    .unwrap();
        } else {
            let outcome = Execution {
                store: &history,
                ledger: &mut history_ledger,
                job: &history_job,
                roster: &f.roster,
                namespaces: &mut f.namespaces,
                nodes: &f.nodes,
                clock: &f.clock,
                admitted_context: &f.context,
            }
            .apply_hook(
                &history_delete,
                &history_delete.authorize().unwrap(),
                |stage| {
                    if Some(stage) == stop {
                        Err(Error::Injected)
                    } else {
                        Ok(())
                    }
                },
            );
            if stop.is_some() {
                assert!(matches!(outcome, Err(Error::Injected)));
            } else {
                outcome.unwrap();
            }
        }
        let main_delete = f.plan();
        f.apply(&main_delete);
        let cache =
            LocalStore::create_managed(&f.root.path().join("working"), key("F"), binding("F"))
                .unwrap();
        let (cache_ledger, cache_job) = capacity(&cache, f.root.path(), "cache-capacity");
        let provider = |store: &LocalStore,
                        ledger: &CapacityLedger,
                        job: &JournalRef,
                        role: p::Role,
                        place: &str| p::Provider {
            place: Place::local(store, 1).unwrap(),
            role,
            failure_domain: key(place),
            capacity: ledger.identity().clone(),
            job: job.clone(),
            assignments: vec![p::Mapping {
                namespace: ns.clone(),
                place: key(place),
            }],
        };
        let config = p::Config {
            schema: "tig.placement-enrollment.design-1".into(),
            owner: key("owner"),
            authority: d("placement"),
            seed: d("denied-seed"),
            minimum_domains: 1,
            namespaces: vec![p::Authority {
                namespace: ns.clone(),
                policy: digest(f.namespaces[0].policy()).unwrap(),
            }],
            providers: vec![
                provider(&f.store, &f.ledger, &f.job, p::Role::Retained, "M"),
                provider(&cache, &cache_ledger, &cache_job, p::Role::Cache, "F"),
                provider(&history, &history_ledger, &history_job, p::Role::Cache, "D"),
            ],
            budget: p::Budget {
                objects: 16,
                max_object_bytes: 1024,
                max_stream_bytes: 1024,
                max_peak_bytes: 128 * 1024 * 1024,
                limits: limits(),
            },
        };
        Self {
            f,
            history,
            history_ledger,
            history_job,
            history_delete,
            main_delete,
            cache,
            cache_ledger,
            config,
            seed: DeniedSeed::default(),
        }
    }
    fn with<T>(
        &mut self,
        action: impl FnOnce(&mut p::Execution<'_, '_, DeniedSeed, MemoryNodes>) -> p::Result<T>,
    ) -> p::Result<T> {
        let mut places = [
            p::Local {
                store: &self.f.store,
                ledger: &mut self.f.ledger,
                job: &self.config.providers[0].job,
            },
            p::Local {
                store: &self.cache,
                ledger: &mut self.cache_ledger,
                job: &self.config.providers[1].job,
            },
            p::Local {
                store: &self.history,
                ledger: &mut self.history_ledger,
                job: &self.config.providers[2].job,
            },
        ];
        let mut executor = p::Execution {
            config: &self.config,
            namespaces: &mut self.f.namespaces[..1],
            nodes: &self.f.nodes,
            places: &mut places,
            seed: &mut self.seed,
            seed_identity: &self.config.seed,
            allow_copy: true,
            allow_eviction: true,
            allow_reacquisition: true,
            allow_cancellation: true,
        };
        action(&mut executor)
    }
    fn run(&mut self) -> (p::Plan, p::Report) {
        self.with(|e| {
            let plan = e.plan(None)?;
            let report = e.apply(&plan)?;
            Ok((plan, report))
        })
        .unwrap()
    }
    fn checkpoints(&self) -> Vec<Checkpoint> {
        vec![
            self.f.ledger.checkpoint(),
            self.cache_ledger.checkpoint(),
            self.history_ledger.checkpoint(),
        ]
    }
    fn records(&self) -> BTreeMap<String, Object> {
        self.history
            .lifecycle_files(&self.f.publication.object)
            .unwrap()
            .into_iter()
            .map(|path| {
                (
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    Object::from_bytes(&fs::read(path).unwrap()),
                )
            })
            .collect()
    }
    fn fresh_backup(&mut self) -> RetentionGrant {
        let request = Publication {
            operation: key("explicit-new-M"),
            ..self.f.publication.clone()
        };
        let rp = reacquire::plan(
            &self.f.store,
            &self.main_delete,
            reacquire::Request {
                publication: request.clone(),
                capacity: self.f.ledger.identity().clone(),
                context: self.f.context.clone(),
                window: Window {
                    not_before: 50,
                    expires: 500,
                },
            },
        )
        .unwrap();
        reacquire::Execution {
            store: &self.f.store,
            ledger: &mut self.f.ledger,
            job: &self.f.job,
            clock: &self.f.clock,
            admitted_context: &self.f.context,
        }
        .apply(&rp, &rp.authorize().unwrap(), &mut Cursor::new(b"asset"))
        .unwrap();
        let pin = retention::plan(
            &self.f.store,
            &retention::RetentionRequest {
                operation: request.operation.clone(),
                owner: key("owner"),
                publication: request,
                minimum_ack: AckLevel::ProcessRestart,
            },
        )
        .unwrap();
        retention::retain(&mut self.f.ledger, &self.f.store, &self.f.job, &pin)
            .unwrap()
            .grant
    }
    fn cache_bytes(&self) -> Vec<u8> {
        let mut reader = self
            .cache
            .read_object(&Object::from_bytes(b"asset"))
            .unwrap();
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        bytes
    }
    fn replay_history(&mut self) -> Receipt {
        Execution {
            store: &self.history,
            ledger: &mut self.history_ledger,
            job: &self.history_job,
            roster: &self.f.roster,
            namespaces: &mut self.f.namespaces,
            nodes: &self.f.nodes,
            clock: &self.f.clock,
            admitted_context: &self.f.context,
        }
        .apply(
            &self.history_delete,
            &self.history_delete.authorize().unwrap(),
        )
        .unwrap()
    }
    fn history_observation<'a>(&self, r: &'a p::Report) -> &'a p::Observation {
        &r.observations
            .iter()
            .find(|o| {
                o.provider == self.history.identity().clone()
                    && o.object == self.f.publication.object
            })
            .unwrap()
            .state
    }
}
#[test]
fn restored_working_and_new_backup_converge_without_resurrecting_deleted_history() {
    let mut t = Three::new(None);
    t.f.selected(0);
    let (_, missing) = t.run();
    assert!(!missing.page_converged);
    assert!(missing.holds.contains(&p::Error::FinalRetirement));
    assert!(!t.f.store.object_present(&t.f.publication.object).unwrap());
    assert!(!t.cache.object_present(&t.f.publication.object).unwrap());
    let history = t.records();
    let history_checkpoint = t.history_ledger.checkpoint();
    let grant = t.fresh_backup();
    let attempts = t.seed.attempts;
    let (plan, restored) = t.run();
    assert!(restored.page_converged, "{:?}", restored.holds);
    assert_eq!(restored.storage_converged, Some(true));
    assert_eq!(restored.stream_bytes, 5);
    assert_eq!(t.seed.attempts, attempts);
    assert_eq!(t.f.bytes(), b"asset");
    assert_eq!(t.cache_bytes(), b"asset");
    assert!(
        !t.history
            .object_present(&Object::from_bytes(b"asset"))
            .unwrap()
    );
    assert_eq!(t.records(), history);
    assert_eq!(t.history_ledger.checkpoint(), history_checkpoint);
    assert_eq!(
        t.history_observation(&restored),
        &p::Observation::AuthorizedAbsence
    );
    retention::verify(&mut t.f.ledger, &t.f.store, &grant).unwrap();
    let before = t.checkpoints();
    let sources =
        t.f.namespaces
            .iter()
            .map(Namespace::checkpoint)
            .collect::<Vec<_>>();
    let status = t.with(|e| e.status(&plan)).unwrap();
    assert!(status.page_converged);
    assert!(status.effects.is_empty());
    assert_eq!(status.stream_bytes, 0);
    assert_eq!(t.checkpoints(), before);
    assert_eq!(
        t.f.namespaces
            .iter()
            .map(Namespace::checkpoint)
            .collect::<Vec<_>>(),
        sources
    );
    let (_, again) = t.run();
    assert!(again.page_converged);
    assert_eq!(again.stream_bytes, 0);
    assert!(again.effects.is_empty());
    assert_eq!(t.checkpoints(), before);
    t.f.clock.value.set(700);
    let old = t.replay_history();
    assert!(old.historical);
    assert_eq!(old.logical_bytes_removed, 5);
    let old = t.f.apply(&t.main_delete.clone());
    assert!(old.historical);
    assert_eq!(old.logical_bytes_removed, 5);
    assert_eq!(t.f.bytes(), b"asset");
    assert_eq!(t.cache_bytes(), b"asset");
    assert_eq!(t.records(), history);
    assert_eq!(t.checkpoints(), before);
    retention::verify(&mut t.f.ledger, &t.f.store, &grant).unwrap();
    assert_eq!(
        t.f.namespaces
            .iter()
            .map(Namespace::checkpoint)
            .collect::<Vec<_>>(),
        sources
    );
}
#[test]
fn pending_unlink_and_unsettled_terminal_never_count_as_completed_absence() {
    for stage in [
        Stage::Intent,
        Stage::GrantsRemoved,
        Stage::Unlinked,
        Stage::Terminal,
    ] {
        let mut t = Three::new(Some(stage));
        t.f.selected(0);
        t.fresh_backup();
        let (plan, _) = t.run();
        let before = t.checkpoints();
        let status = t.with(|e| e.status(&plan)).unwrap();
        assert!(!status.page_converged, "{stage:?}");
        assert_ne!(
            t.history_observation(&status),
            &p::Observation::AuthorizedAbsence
        );
        assert_eq!(t.checkpoints(), before);
    }
}
#[test]
fn unexpectedly_present_corrupt_and_missing_proof_preserve_recovery_holds() {
    for fault in ["present", "corrupt", "missing"] {
        let mut t = Three::new(None);
        t.f.selected(0);
        t.fresh_backup();
        let terminal = t
            .history
            .lifecycle_files(&t.f.publication.object)
            .unwrap()
            .into_iter()
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .ends_with(".final-terminal.json")
            })
            .unwrap();
        let payload = crate::existing::Layout::Flat.object_path(
            &t.f.root.path().join("history"),
            &t.f.publication.object.sha256,
        );
        match fault {
            "present" => fs::write(&payload, b"asset").unwrap(),
            "corrupt" => fs::write(&terminal, b"{}").unwrap(),
            "missing" => fs::remove_file(&terminal).unwrap(),
            _ => unreachable!(),
        }
        let (plan, report) = t.run();
        assert!(!report.page_converged, "{fault}");
        let before = t.checkpoints();
        let status = t.with(|e| e.status(&plan)).unwrap();
        assert!(!status.page_converged);
        assert_ne!(
            t.history_observation(&status),
            &p::Observation::AuthorizedAbsence
        );
        assert_eq!(t.checkpoints(), before);
        if fault == "present" {
            assert_eq!(fs::read(payload).unwrap(), b"asset");
        }
    }
}

#[test]
fn no_effect_absent_cache_needs_no_eviction_or_copy_permission() {
    let mut t = Three::new(None);
    let root = Tree::build(
        &BTreeMap::from([(
            LogicalPath::parse("item").unwrap(),
            Object::from_bytes(b"asset"),
        )]),
        &mut t.f.nodes,
        TreeLimits::default(),
    )
    .unwrap()
    .0
    .root()
    .clone();
    t.f.namespaces[0]
        .apply(
            key("keep-enroll"),
            Command::Enroll {
                source: key("keep"),
                owner: key("owner"),
                class: Kind::Retention,
            },
            &t.f.nodes,
            &limits(),
        )
        .unwrap();
    t.f.namespaces[0]
        .select(
            &Select {
                prepare: key("keep-prepare"),
                adopt: key("keep-adopt"),
                source: key("keep"),
                owner: key("owner"),
                expected_revision: 1,
                selection: Selection {
                    root,
                    context: d("keep"),
                },
            },
            &t.f.nodes,
            &limits(),
        )
        .unwrap();
    let grant = t.fresh_backup();
    let before = t.checkpoints();
    let history = t.records();
    let source = t.f.namespaces[0].checkpoint();
    let report = t
        .with(|e| {
            e.allow_copy = false;
            e.allow_eviction = false;
            e.allow_reacquisition = false;
            e.allow_cancellation = false;
            let plan = e.plan(None)?;
            e.apply(&plan)
        })
        .unwrap();
    assert!(report.page_converged, "{:?}", report.holds);
    assert_eq!(report.storage_converged, Some(true));
    assert!(report.effects.is_empty());
    assert_eq!(report.stream_bytes, 0);
    assert_eq!(
        t.history_observation(&report),
        &p::Observation::AuthorizedAbsence
    );
    assert_eq!(t.seed.attempts, 0);
    assert!(
        !t.cache
            .object_present(&Object::from_bytes(b"asset"))
            .unwrap()
    );
    assert!(
        !t.history
            .object_present(&Object::from_bytes(b"asset"))
            .unwrap()
    );
    assert_eq!(t.checkpoints(), before);
    assert_eq!(t.records(), history);
    assert_eq!(t.f.namespaces[0].checkpoint(), source);
    retention::verify(&mut t.f.ledger, &t.f.store, &grant).unwrap();
}
#[test]
fn a_later_history_requirement_keeps_the_final_reacquisition_fence() {
    let mut t = Three::new(None);
    t.f.selected(0);
    t.fresh_backup();
    assert!(t.run().1.page_converged);
    let root = Tree::build(
        &BTreeMap::from([(
            LogicalPath::parse("item").unwrap(),
            Object::from_bytes(b"new current asset"),
        )]),
        &mut t.f.nodes,
        TreeLimits::default(),
    )
    .unwrap()
    .0
    .root()
    .clone();
    let revision = t.f.namespaces[0]
        .source(&key("main"))
        .unwrap()
        .unwrap()
        .revision;
    t.f.namespaces[0]
        .select(
            &Select {
                prepare: key("new-current-prepare"),
                adopt: key("new-current-adopt"),
                source: key("main"),
                owner: key("owner"),
                expected_revision: revision,
                selection: Selection {
                    root,
                    context: d("new-current"),
                },
            },
            &t.f.nodes,
            &limits(),
        )
        .unwrap();
    let history = t.records();
    let before = t.history_ledger.checkpoint();
    let (_, report) = t.run();
    assert!(!report.page_converged);
    assert!(report.holds.contains(&p::Error::FinalRetirement));
    let row = report
        .observations
        .iter()
        .find(|o| {
            o.object == Object::from_bytes(b"asset") && o.provider == t.history.identity().clone()
        })
        .unwrap();
    assert!(row.required);
    assert_eq!(row.state, p::Observation::Held(p::Error::FinalRetirement));
    assert!(
        !t.history
            .object_present(&Object::from_bytes(b"asset"))
            .unwrap()
    );
    assert_eq!(t.history_ledger.checkpoint(), before);
    assert_eq!(t.records(), history);
}
#[test]
fn wrong_original_job_or_capacity_cannot_establish_absence() {
    for fault in ["job", "capacity"] {
        let mut t = Three::new(None);
        t.f.selected(0);
        t.fresh_backup();
        assert!(t.run().1.page_converged);
        match fault {
            "job" => {
                LocalCapacityProbe::bind(&t.history, &mut t.history_ledger)
                    .unwrap()
                    .observe(
                        &t.history,
                        &mut t.history_ledger,
                        key("new-job-probe"),
                        key("new-job-sample"),
                    )
                    .unwrap();
                t.config.providers[2].job = t
                    .history_ledger
                    .reserve(key("foreign-job"), key("owner"), 16 * 1024 * 1024)
                    .unwrap()
                    .reference()
                    .clone();
            }
            "capacity" => t.config.providers[2].capacity = d("foreign-capacity"),
            _ => unreachable!(),
        }
        let before = t.checkpoints();
        let history = t.records();
        let result = t.with(|e| {
            let plan = e.plan(None)?;
            e.status(&plan)
        });
        match result {
            Ok(report) => {
                assert!(!report.page_converged);
                assert_ne!(
                    t.history_observation(&report),
                    &p::Observation::AuthorizedAbsence
                );
            }
            Err(error) => assert_eq!(error, p::Error::WrongEnrollment),
        }
        assert_eq!(t.checkpoints(), before);
        assert_eq!(t.records(), history);
    }
}
#[test]
fn old_completed_history_evidence_cannot_overlay_a_new_explicit_history_birth() {
    let mut t = Three::new(None);
    t.f.selected(0);
    t.fresh_backup();
    assert!(t.run().1.page_converged);
    let request = Publication {
        operation: key("explicit-new-D"),
        ..t.history_delete.request().publication.clone()
    };
    let rp = reacquire::plan(
        &t.history,
        &t.history_delete,
        reacquire::Request {
            publication: request.clone(),
            capacity: t.history_ledger.identity().clone(),
            context: t.f.context.clone(),
            window: Window {
                not_before: 50,
                expires: 500,
            },
        },
    )
    .unwrap();
    reacquire::Execution {
        store: &t.history,
        ledger: &mut t.history_ledger,
        job: &t.history_job,
        clock: &t.f.clock,
        admitted_context: &t.f.context,
    }
    .apply(&rp, &rp.authorize().unwrap(), &mut Cursor::new(b"asset"))
    .unwrap();
    let pin = retention::plan(
        &t.history,
        &retention::RetentionRequest {
            operation: key("new-history-pin"),
            owner: key("owner"),
            publication: request,
            minimum_ack: AckLevel::ProcessRestart,
        },
    )
    .unwrap();
    let grant = retention::retain(&mut t.history_ledger, &t.history, &t.history_job, &pin)
        .unwrap()
        .grant;
    let before = t.checkpoints();
    let history = t.records();
    let report = t
        .with(|e| {
            let plan = e.plan(None)?;
            e.status(&plan)
        })
        .unwrap();
    assert!(!report.page_converged);
    assert_ne!(
        t.history_observation(&report),
        &p::Observation::AuthorizedAbsence
    );
    t.f.clock.value.set(700);
    let old = t.replay_history();
    assert!(old.historical);
    assert_eq!(old.logical_bytes_removed, 5);
    let mut bytes = Vec::new();
    t.history
        .read_object(&Object::from_bytes(b"asset"))
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, b"asset");
    retention::verify(&mut t.history_ledger, &t.history, &grant).unwrap();
    assert_eq!(t.checkpoints(), before);
    assert_eq!(t.records(), history);
}

#[test]
fn unavailable_history_provider_never_becomes_known_absence() {
    let mut t = Three::new(None);
    t.f.selected(0);
    t.fresh_backup();
    assert!(t.run().1.page_converged);
    let before = t.checkpoints();
    let records = t.records();
    let owner = t.f.root.path().canonicalize().unwrap();
    let root = owner.join("history");
    let offline = owner.join("history-offline");
    assert!(root.is_absolute() && offline.is_absolute());
    assert_eq!(root.parent(), Some(owner.as_path()));
    assert_eq!(offline.parent(), Some(owner.as_path()));
    assert!(!offline.exists());
    fs::rename(&root, &offline).unwrap();
    let result = t.with(|e| {
        let plan = e.plan(None)?;
        e.status(&plan)
    });
    fs::rename(&offline, &root).unwrap();
    match result {
        Ok(report) => {
            assert!(!report.page_converged);
            assert_eq!(
                t.history_observation(&report),
                &p::Observation::Held(p::Error::Unavailable)
            );
        }
        Err(error) => assert_eq!(error, p::Error::Unavailable),
    }
    assert_eq!(t.checkpoints(), before);
    assert_eq!(t.records(), records);
}
#[test]
fn acknowledged_settlement_minimum_refuses_an_actual_older_ledger_backup() {
    let mut t = Three::new(Some(Stage::Terminal));
    let ledger_root = t.f.root.path().join("history-capacity");
    let backup = t.f.root.path().join("old-history.sqlite");
    let old_minimum = t.history_ledger.checkpoint();
    drop(t.history_ledger);
    fs::copy(ledger_root.join("ledger.sqlite"), &backup).unwrap();
    let mut ledger = CapacityLedger::open(&ledger_root, &old_minimum).unwrap();
    let receipt = Execution {
        store: &t.history,
        ledger: &mut ledger,
        job: &t.history_job,
        roster: &t.f.roster,
        namespaces: &mut t.f.namespaces,
        nodes: &t.f.nodes,
        clock: &t.f.clock,
        admitted_context: &t.f.context,
    }
    .apply(&t.history_delete, &t.history_delete.authorize().unwrap())
    .unwrap();
    assert!(receipt.historical);
    let minimum = ledger.checkpoint();
    assert!(minimum.sequence() > old_minimum.sequence());
    drop(ledger);
    fs::copy(backup, ledger_root.join("ledger.sqlite")).unwrap();
    assert!(matches!(
        CapacityLedger::open(&ledger_root, &minimum),
        Err(crate::ledger::LedgerError::Rollback)
    ));
    assert!(
        !t.history
            .object_present(&Object::from_bytes(b"asset"))
            .unwrap()
    );
}
#[test]
fn history_exit_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_PLACEMENT_ABSENCE_CHILD") else {
        return;
    };
    let root = PathBuf::from(root);
    let bytes = fs::read(root.join("history-saved.json")).unwrap();
    assert!(bytes.len() <= 65536);
    let saved: Saved = serde_json::from_slice(&bytes).unwrap();
    let store = LocalStore::open(&root.join("history")).unwrap();
    let mut ledger = CapacityLedger::open(&root.join("history-capacity"), &saved.ledger).unwrap();
    let mut namespaces = saved
        .namespaces
        .iter()
        .enumerate()
        .map(|(i, c)| Namespace::open(&root.join(format!("ns-{i}")), c).unwrap())
        .collect::<Vec<_>>();
    let clock = TestClock {
        value: Cell::new(100),
        last: Cell::new(0),
    };
    let nodes = MemoryNodes::default();
    let context = d("private-context");
    let stop = std::env::var("GIT_ASSET_PLACEMENT_ABSENCE_STAGE").unwrap();
    Execution {
        store: &store,
        ledger: &mut ledger,
        job: &saved.job,
        roster: &saved.roster,
        namespaces: &mut namespaces,
        nodes: &nodes,
        clock: &clock,
        admitted_context: &context,
    }
    .apply_hook(&saved.plan, &saved.plan.authorize().unwrap(), |stage| {
        if format!("{stage:?}") == stop {
            std::process::exit(77);
        }
        Ok(())
    })
    .unwrap();
    panic!("actual exit boundary not reached");
}
#[test]
fn actual_exits_distinguish_unlink_terminal_and_settled_absence() {
    for stage in [Stage::Unlinked, Stage::Terminal, Stage::Settled] {
        let mut t = Three::build(None, Some(stage));
        t.f.selected(0);
        t.fresh_backup();
        let before = t.history_ledger.checkpoint();
        let records = t.records();
        let (plan, report) = t.run();
        assert_eq!(
            report.page_converged,
            stage == Stage::Settled,
            "{stage:?}: {:?}",
            report.holds
        );
        assert_eq!(t.history_ledger.checkpoint(), before);
        assert_eq!(t.records(), records);
        assert_eq!(t.f.bytes(), b"asset");
        assert_eq!(t.cache_bytes(), b"asset");
        let status = t.with(|e| e.status(&plan)).unwrap();
        assert_eq!(
            t.history_observation(&status) == &p::Observation::AuthorizedAbsence,
            stage == Stage::Settled
        );
    }
}

#[test]
fn cancelled_present_history_is_not_intentional_absence() {
    let mut t = Three::new(Some(Stage::Intent));
    let receipt = Execution {
        store: &t.history,
        ledger: &mut t.history_ledger,
        job: &t.history_job,
        roster: &t.f.roster,
        namespaces: &mut t.f.namespaces,
        nodes: &t.f.nodes,
        clock: &t.f.clock,
        admitted_context: &t.f.context,
    }
    .cancel(&t.history_delete, &t.history_delete.authorize().unwrap())
    .unwrap();
    assert_eq!(receipt.outcome, Some(Outcome::Cancelled));
    t.f.selected(0);
    t.fresh_backup();
    let (plan, _) = t.run();
    let status = t.with(|e| e.status(&plan)).unwrap();
    assert!(!status.page_converged);
    assert_ne!(
        t.history_observation(&status),
        &p::Observation::AuthorizedAbsence
    );
    let path = crate::existing::Layout::Flat.object_path(
        &t.f.root.path().join("history"),
        &Object::from_bytes(b"asset").sha256,
    );
    assert_eq!(fs::read(path).unwrap(), b"asset");
}
