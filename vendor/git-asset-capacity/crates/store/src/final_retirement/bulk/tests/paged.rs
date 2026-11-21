use super::*;
use crate::final_retirement::paged::{self as selection, control};
mod aged;
mod followup;
mod operator;

fn compile(
    f: &mut Fixture,
    extra: usize,
) -> (control::Enrollment, control::Decision, selection::Limits) {
    compile_with_window(
        f,
        extra,
        provider::Window {
            not_before: 50,
            expires: 500,
        },
    )
}
fn compile_with_window(
    f: &mut Fixture,
    extra: usize,
    window: provider::Window,
) -> (control::Enrollment, control::Decision, selection::Limits) {
    let shared = root(
        &mut f.metadata,
        &[
            ("a", f.publications["a"].object.clone()),
            ("keep", f.publications["keep"].object.clone()),
        ],
    );
    let mut requested = f.request().selections;
    for i in 0..extra {
        select(
            &mut f.namespaces[0],
            &f.metadata,
            "main",
            "owner",
            shared.clone(),
            &format!("more-{i}"),
        );
        let revision = f.namespaces[0]
            .source(&key("main"))
            .unwrap()
            .unwrap()
            .revision;
        f.namespaces[0]
            .apply(
                key(&format!("end-more-{i}")),
                Command::Withdraw {
                    source: key("main"),
                    owner: key("owner"),
                    expected_revision: revision,
                },
                &f.metadata,
                &limits(),
            )
            .unwrap();
        requested.push(Selection {
            namespace: f.namespaces[0].identity().clone(),
            request: subset::Request {
                source: key("main"),
                predecessor_revision: revision,
                paths: vec![path("a")],
            },
        });
    }
    let limits = selection::Limits::default();
    let e = control::Enrollment {
        age: None,
        selection: selection::Enrollment {
            authority: f.enrollment.sources[0].authority.clone(),
            epoch: f.enrollment.epoch.clone(),
            owner: key("owner"),
            metadata: f.metadata.checkpoint().identity().clone(),
            scope: f.roster.scope().unwrap(),
            classes: [Kind::Branch, Kind::Draft].into_iter().collect(),
        },
        sources: f.enrollment.sources.clone(),
        context: d("paged-context"),
        clock: f.enrollment.clock.clone(),
        witness: f.enrollment.witness.clone(),
        capacity: f.enrollment.capacity.clone(),
        max_window: f.enrollment.max_window,
        max_metadata_bytes: 512 * 1024 * 1024,
    };
    let mut selections = Vec::new();
    for s in requested {
        let source = f.namespaces[0].source(&s.request.source).unwrap().unwrap();
        let original = &source
            .predecessors
            .iter()
            .find(|p| p.revision == s.request.predecessor_revision)
            .unwrap()
            .selection;
        let bindings = s
            .request
            .paths
            .iter()
            .map(|p| {
                let o = Tree::from_root(original.root.clone())
                    .get(p, &mut f.metadata, TreeLimits::default())
                    .unwrap()
                    .0
                    .unwrap();
                (p.clone(), o)
            })
            .collect();
        let root = Tree::build(&bindings, &mut f.metadata, TreeLimits::default())
            .unwrap()
            .0
            .root()
            .clone();
        selections.push(selection::Selection {
            namespace: s.namespace,
            source: s.request.source,
            predecessor_revision: s.request.predecessor_revision,
            bindings: root,
        });
    }
    let input = selection::request(
        selections,
        selection::Paging {
            max_bindings: 1,
            ..Default::default()
        },
        &mut f.metadata,
        &limits,
    )
    .unwrap();
    f.metadata.finish().unwrap();
    let manifest = selection::build(
        &e.selection,
        &input,
        &f.roster,
        &mut f.namespaces,
        &mut f.metadata,
        &limits,
    )
    .unwrap();
    let copies = control::copies(f.request().copies, &mut f.metadata, &limits).unwrap();
    f.metadata.finish().unwrap();
    let decision = control::plan(
        &e,
        control::Request {
            operation: key("paged-block"),
            manifest,
            copies,
            window,
        },
        &f.roster,
        &mut f.namespaces,
        &mut f.metadata,
        &[&f.store],
        &limits,
    )
    .unwrap();
    f.metadata.finish().unwrap();
    (e, decision, limits)
}
fn run<T>(
    f: &mut Fixture,
    e: &control::Enrollment,
    limits: &selection::Limits,
    action: impl FnOnce(&mut control::Execution<'_, '_>) -> T,
) -> T {
    let jobs = [control::StoreJob {
        store: f.store.identity().clone(),
        job: f.job.clone(),
    }];
    let mut places = [Local {
        store: &f.store,
        ledger: &mut f.ledger,
        job: &f.job,
    }];
    action(&mut control::Execution {
        enrollment: e,
        roster: &f.roster,
        namespaces: &mut f.namespaces,
        metadata: &mut f.metadata,
        places: &mut places,
        jobs: &jobs,
        witness: Local {
            store: &f.witness,
            ledger: &mut f.witness_ledger,
            job: &f.witness_job,
        },
        clock: &f.clock,
        admitted_context: &e.context,
        limits,
        run_limits: control::RunLimits::default(),
    })
}
#[test]
fn more_than_sixteen_shared_roots_finish_across_calls_and_only_original_eligible_copies_are_deleted()
 {
    let mut f = Fixture::new();
    let (e, p, l) = compile(&mut f, 15);
    let explained =
        control::inspect(&e, &p, &mut f.metadata.finished_reader().unwrap(), &l).unwrap();
    assert_eq!(explained.decision(), &p.id().unwrap());
    assert_eq!(explained.actions().len(), 3);
    assert!(
        explained
            .subjects()
            .any(|s| s.object == f.publications["shared"].object
                && s.hold == Some(control::Hold::SurvivingClaims))
    );
    assert!(!explained.authorizes_claim_release());
    assert!(!explained.authorizes_removal());
    let before = (f.ledger.checkpoint(), f.witness_ledger.checkpoint());
    let status = run(&mut f, &e, &l, |x| x.status(&p)).unwrap();
    assert_eq!(status.total_pages, 20);
    assert_eq!(status.phase, control::Phase::NotStarted);
    assert_eq!(
        before,
        (f.ledger.checkpoint(), f.witness_ledger.checkpoint())
    );
    let first = run(&mut f, &e, &l, |x| {
        x.apply(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
        )
    })
    .unwrap();
    assert_eq!(first.phase, control::Phase::Intent);
    assert_eq!(first.completed_pages, Some(16));
    for name in ["a", "b", "c", "shared", "keep"] {
        assert!(
            f.store
                .object_present(&f.publications[name].object)
                .unwrap()
        );
    }
    let done = run(&mut f, &e, &l, |x| {
        x.apply(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
        )
    })
    .unwrap();
    assert_eq!(done.phase, control::Phase::Completed, "{:?}", done.holds);
    assert_eq!(done.completed_pages, Some(20));
    assert_eq!(done.historical_deleted_actions, Some(3));
    assert_eq!(done.excluded_objects, Some(1));
    for name in ["a", "b", "c"] {
        assert!(
            !f.store
                .object_present(&f.publications[name].object)
                .unwrap()
        );
    }
    for name in ["shared", "keep"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
    f.clock.0.set(700);
    let before = (f.ledger.checkpoint(), f.witness_ledger.checkpoint());
    assert_eq!(
        run(&mut f, &e, &l, |x| x.apply(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap()
        ))
        .unwrap()
        .phase,
        control::Phase::Completed
    );
    assert_eq!(
        before,
        (f.ledger.checkpoint(), f.witness_ledger.checkpoint())
    );
}
#[test]
fn page_intent_and_source_boundaries_resume_without_repeating_final_loss() {
    for stop in [
        control::Stage::Intent,
        control::Stage::PageAdmitted(0),
        control::Stage::PageIntent(0),
        control::Stage::Source(0, subset::Stage::Enrolled),
        control::Stage::Source(0, subset::Stage::SurvivorProtected),
        control::Stage::Source(0, subset::Stage::Retired),
        control::Stage::PageCompleted(0),
        control::Stage::Removed(0),
    ] {
        let mut f = Fixture::new();
        let (e, p, l) = compile(&mut f, 0);
        assert!(
            run(&mut f, &e, &l, |x| x.apply_hook(
                &p,
                &p.authorize_claim_release().unwrap(),
                &p.authorize_final_loss().unwrap(),
                |stage| {
                    if stage == stop {
                        Err(control::Error::Injected)
                    } else {
                        Ok(())
                    }
                }
            ))
            .is_err(),
            "{stop:?}"
        );
        let done = run(&mut f, &e, &l, |x| {
            x.apply(
                &p,
                &p.authorize_claim_release().unwrap(),
                &p.authorize_final_loss().unwrap(),
            )
        })
        .unwrap();
        assert_eq!(
            done.phase,
            control::Phase::Completed,
            "{stop:?}: {:?}",
            done.holds
        );
        for name in ["shared", "keep"] {
            assert!(f.store.read_object(&f.publications[name].object).is_ok());
        }
    }
}
#[test]
fn cancelled_page_never_resumes_source_mutation_even_after_clock_expiry() {
    let mut f = Fixture::new();
    let (e, p, l) = compile(&mut f, 0);
    assert!(
        run(&mut f, &e, &l, |x| x.apply_hook(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
            |s| {
                if s == control::Stage::Source(0, subset::Stage::Retired) {
                    Err(control::Error::Injected)
                } else {
                    Ok(())
                }
            }
        ))
        .is_err()
    );
    f.clock.0.set(700);
    let stopped = run(&mut f, &e, &l, |x| {
        x.cancel(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
        )
    })
    .unwrap();
    assert_eq!(stopped.phase, control::Phase::Cancelled);
    assert_eq!(stopped.cancelled_pages, Some(1));
    assert_eq!(stopped.historical_retired_pages, Some(1));
    let before = f
        .namespaces
        .iter()
        .map(Namespace::checkpoint)
        .collect::<Vec<_>>();
    let again = run(&mut f, &e, &l, |x| {
        x.apply(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
        )
    })
    .unwrap();
    assert_eq!(again.phase, control::Phase::Cancelled);
    assert_eq!(
        before,
        f.namespaces
            .iter()
            .map(Namespace::checkpoint)
            .collect::<Vec<_>>()
    );
    for name in ["a", "b", "c", "shared", "keep"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
}

#[test]
fn offline_metadata_cancellation_records_stop_and_reports_unknown_cleanup_then_resolves() {
    let mut f = Fixture::new();
    let (e, p, l) = compile(&mut f, 0);
    assert!(
        run(&mut f, &e, &l, |x| x.apply_hook(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
            |s| {
                if s == control::Stage::Source(0, subset::Stage::Retired) {
                    Err(control::Error::Injected)
                } else {
                    Ok(())
                }
            }
        ))
        .is_err()
    );
    let minimum = f.metadata.checkpoint();
    drop(f.metadata);
    std::fs::rename(
        f.temp.path().join("metadata"),
        f.temp.path().join("offline-metadata"),
    )
    .unwrap();
    let jobs = [control::StoreJob {
        store: f.store.identity().clone(),
        job: f.job.clone(),
    }];
    let mut places = [Local {
        store: &f.store,
        ledger: &mut f.ledger,
        job: &f.job,
    }];
    let mut witness = Local {
        store: &f.witness,
        ledger: &mut f.witness_ledger,
        job: &f.witness_job,
    };
    let stopped = control::Cancellation {
        inspection: control::Inspection {
            enrollment: &e,
            roster: &f.roster,
            namespaces: &mut f.namespaces,
            metadata: None,
            places: &mut places,
            jobs: &jobs,
            witness: &mut witness,
            admitted_context: &e.context,
            limits: &l,
        },
    }
    .cancel(
        &p,
        &p.authorize_claim_release().unwrap(),
        &p.authorize_final_loss().unwrap(),
    )
    .unwrap();
    assert_eq!(stopped.phase, control::Phase::Cancelled);
    assert!(stopped.cleanup_unknown);
    assert_eq!(stopped.completed_pages, None);
    assert_eq!(stopped.historical_deleted_actions, None);
    assert_eq!(stopped.pending_cancellations, None);
    std::fs::rename(
        f.temp.path().join("offline-metadata"),
        f.temp.path().join("metadata"),
    )
    .unwrap();
    f.metadata = MetadataStore::open(&f.temp.path().join("metadata"), &minimum).unwrap();
    let done = run(&mut f, &e, &l, |x| {
        x.cancel(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
        )
    })
    .unwrap();
    assert_eq!(done.phase, control::Phase::Cancelled);
    assert!(!done.cleanup_unknown);
    assert_eq!(done.historical_retired_pages, Some(1));
    assert_eq!(done.pending_cancellations, Some(0));
    for name in ["a", "b", "c", "shared", "keep"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
}

#[test]
fn changed_source_and_late_transient_use_invalidate_future_work() {
    let mut f = Fixture::new();
    let (e, p, l) = compile(&mut f, 0);
    transient_use(&mut f);
    let before = f.namespaces[0].checkpoint();
    assert!(
        run(&mut f, &e, &l, |x| x.apply(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap()
        ))
        .is_err()
    );
    assert_eq!(before, f.namespaces[0].checkpoint());
    let mut f = Fixture::new();
    let selected = root(
        &mut f.metadata,
        &[("input", f.publications["a"].object.clone())],
    );
    let (e, p, l) = compile(&mut f, 0);
    let mut peer =
        Namespace::open(&f.temp.path().join("ns-1"), &f.namespaces[1].checkpoint()).unwrap();
    let reader = f.metadata.finished_reader().unwrap();
    let done = run(&mut f, &e, &l, |x| {
        x.apply_hook(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
            |s| {
                if s == control::Stage::PageCompleted(4) {
                    peer.apply(
                        key("late-enroll"),
                        Command::Enroll {
                            source: key("late"),
                            owner: key("other"),
                            class: Kind::Job,
                        },
                        &reader,
                        &limits(),
                    )
                    .unwrap();
                    let prepared = peer
                        .apply(
                            key("late-prepare"),
                            Command::Prepare {
                                source: key("late"),
                                owner: key("other"),
                                expected_revision: 1,
                                selection: selected.clone(),
                            },
                            &reader,
                            &limits(),
                        )
                        .unwrap();
                    peer.apply(
                        key("late-abort"),
                        Command::Abort {
                            source: key("late"),
                            owner: key("other"),
                            expected_revision: 2,
                            prepared: prepared.reference,
                        },
                        &reader,
                        &limits(),
                    )
                    .unwrap();
                }
                Ok(())
            },
        )
    })
    .unwrap();
    assert_eq!(done.phase, control::Phase::Intent);
    assert_eq!(done.completed_pages, Some(5));
    assert!(!done.holds.is_empty());
    for name in ["a", "b", "c"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
}

#[test]
fn full_witness_peak_is_checked_before_any_claim_settlement() {
    let mut f = Fixture::new();
    let (mut e, p, l) = compile(&mut f, 0);
    let peak = run(&mut f, &e, &l, |x| x.required_witness_peak(&p)).unwrap();
    e.max_metadata_bytes = peak - 1;
    let p = control::plan(
        &e,
        p.request().clone(),
        &f.roster,
        &mut f.namespaces,
        &mut f.metadata,
        &[&f.store],
        &l,
    )
    .unwrap();
    f.metadata.finish().unwrap();
    let before = f
        .namespaces
        .iter()
        .map(Namespace::checkpoint)
        .collect::<Vec<_>>();
    assert!(matches!(
        run(&mut f, &e, &l, |x| x.apply(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap()
        )),
        Err(control::Error::Budget)
    ));
    assert_eq!(
        before,
        f.namespaces
            .iter()
            .map(Namespace::checkpoint)
            .collect::<Vec<_>>()
    );
}

#[derive(Serialize, Deserialize)]
struct PagedSaved {
    age: Option<source_claims::Checkpoint>,
    enrollment: control::Enrollment,
    decision: control::Decision,
    roster: claims::Roster,
    namespaces: Vec<source_claims::Checkpoint>,
    metadata: crate::metadata::Checkpoint,
    ledger: crate::ledger::Checkpoint,
    witness: crate::ledger::Checkpoint,
    job: JournalRef,
    witness_job: JournalRef,
}
#[test]
fn paged_child() {
    let Some(dir) = std::env::var_os("GIT_ASSET_PAGED_CHILD") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let s: PagedSaved =
        serde_json::from_slice(&std::fs::read(dir.join("paged-saved.json")).unwrap()).unwrap();
    let mut namespaces = s
        .namespaces
        .iter()
        .enumerate()
        .map(|(i, c)| Namespace::open(&dir.join(format!("ns-{i}")), c).unwrap())
        .collect::<Vec<_>>();
    let mut metadata = MetadataStore::open(&dir.join("metadata"), &s.metadata).unwrap();
    let store = LocalStore::open(&dir.join("M")).unwrap();
    let mut ledger = CapacityLedger::open(&dir.join("ledger"), &s.ledger).unwrap();
    let witness = LocalStore::open(&dir.join("witness")).unwrap();
    let mut witness_ledger = CapacityLedger::open(&dir.join("witness-ledger"), &s.witness).unwrap();
    let jobs = [control::StoreJob {
        store: store.identity().clone(),
        job: s.job.clone(),
    }];
    let mut places = [Local {
        store: &store,
        ledger: &mut ledger,
        job: &s.job,
    }];
    let clock = Clock(Cell::new(100), s.enrollment.clock.clone());
    let limits = selection::Limits::default();
    let stop = std::env::var("GIT_ASSET_PAGED_STOP").unwrap();
    let mut execution = control::Execution {
        enrollment: &s.enrollment,
        roster: &s.roster,
        namespaces: &mut namespaces,
        metadata: &mut metadata,
        places: &mut places,
        jobs: &jobs,
        witness: Local {
            store: &witness,
            ledger: &mut witness_ledger,
            job: &s.witness_job,
        },
        clock: &clock,
        admitted_context: &s.enrollment.context,
        limits: &limits,
        run_limits: control::RunLimits::default(),
    };
    let hook = |stage| {
        if format!("{stage:?}") == stop {
            std::process::exit(76);
        }
        Ok(())
    };
    if let Some(minimum) = &s.age {
        let mut registry = Namespace::open(&dir.join("ages"), minimum).unwrap();
        execution
            .apply_aged(
                &s.decision,
                &s.decision.authorize_claim_release().unwrap(),
                &s.decision.authorize_final_loss().unwrap(),
                control::aged::Context {
                    registry: &mut registry,
                    minimum,
                    limits: &source_claims::age::Limits::default(),
                },
                hook,
            )
            .unwrap();
    } else {
        execution
            .apply_hook(
                &s.decision,
                &s.decision.authorize_claim_release().unwrap(),
                &s.decision.authorize_final_loss().unwrap(),
                hook,
            )
            .unwrap();
    }
    panic!("requested paged exit was not reached");
}
#[test]
fn actual_child_exits_resume_global_page_claim_and_final_copy_boundaries() {
    for stop in [
        control::Stage::Intent,
        control::Stage::PageIntent(0),
        control::Stage::Source(0, subset::Stage::SurvivorProtected),
        control::Stage::Source(0, subset::Stage::Retired),
        control::Stage::PageCompleted(0),
        control::Stage::Removed(0),
        control::Stage::Completed,
        control::Stage::Provider(0, provider::Stage::Intent),
        control::Stage::Provider(0, provider::Stage::Released),
        control::Stage::Provider(0, provider::Stage::GrantsRemoved),
        control::Stage::Provider(0, provider::Stage::BeforeUnlink),
        control::Stage::Provider(0, provider::Stage::Unlinked),
        control::Stage::Provider(0, provider::Stage::Terminal),
        control::Stage::Provider(0, provider::Stage::Settled),
    ] {
        let mut f = Fixture::new();
        let (e, p, l) = compile(&mut f, 0);
        let saved = PagedSaved {
            age: None,
            enrollment: e.clone(),
            decision: p.clone(),
            roster: f.roster.clone(),
            namespaces: f.namespaces.iter().map(Namespace::checkpoint).collect(),
            metadata: f.metadata.checkpoint(),
            ledger: f.ledger.checkpoint(),
            witness: f.witness_ledger.checkpoint(),
            job: f.job.clone(),
            witness_job: f.witness_job.clone(),
        };
        std::fs::write(
            f.temp.path().join("paged-saved.json"),
            serde_json::to_vec(&saved).unwrap(),
        )
        .unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "final_retirement::bulk::tests::paged::paged_child",
                "--nocapture",
            ])
            .env("GIT_ASSET_PAGED_CHILD", f.temp.path())
            .env("GIT_ASSET_PAGED_STOP", format!("{stop:?}"))
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(76),
            "{stop:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        f.namespaces = saved
            .namespaces
            .iter()
            .enumerate()
            .map(|(i, c)| Namespace::open(&f.temp.path().join(format!("ns-{i}")), c).unwrap())
            .collect();
        f.metadata = MetadataStore::open(&f.temp.path().join("metadata"), &saved.metadata).unwrap();
        f.ledger = CapacityLedger::open(&f.temp.path().join("ledger"), &saved.ledger).unwrap();
        f.witness_ledger =
            CapacityLedger::open(&f.temp.path().join("witness-ledger"), &saved.witness).unwrap();
        let done = run(&mut f, &e, &l, |x| {
            x.apply(
                &p,
                &p.authorize_claim_release().unwrap(),
                &p.authorize_final_loss().unwrap(),
            )
        })
        .unwrap();
        assert_eq!(
            done.phase,
            control::Phase::Completed,
            "{stop:?}: {:?}",
            done.holds
        );
        assert_eq!(done.historical_deleted_actions, Some(3));
        for name in ["shared", "keep"] {
            assert!(f.store.read_object(&f.publications[name].object).is_ok());
        }
    }
}

#[test]
fn claim_and_final_approvals_cannot_mix_decisions_or_rebind_an_acknowledged_operation() {
    let mut f = Fixture::new();
    let (e, p, l) = compile(&mut f, 0);
    let mut request = p.request().clone();
    request.window.expires = 450;
    let other = control::plan(
        &e,
        request,
        &f.roster,
        &mut f.namespaces,
        &mut f.metadata,
        &[&f.store],
        &l,
    )
    .unwrap();
    f.metadata.finish().unwrap();
    let before = (f.ledger.checkpoint(), f.witness_ledger.checkpoint());
    assert!(matches!(
        run(&mut f, &e, &l, |x| x.apply(
            &p,
            &p.authorize_claim_release().unwrap(),
            &other.authorize_final_loss().unwrap()
        )),
        Err(control::Error::Authority)
    ));
    assert_eq!(
        before,
        (f.ledger.checkpoint(), f.witness_ledger.checkpoint())
    );
    assert!(
        run(&mut f, &e, &l, |x| x.apply_hook(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
            |s| {
                if s == control::Stage::Intent {
                    Err(control::Error::Injected)
                } else {
                    Ok(())
                }
            }
        ))
        .is_err()
    );
    let before = f
        .namespaces
        .iter()
        .map(Namespace::checkpoint)
        .collect::<Vec<_>>();
    assert!(
        run(&mut f, &e, &l, |x| x.apply(
            &other,
            &other.authorize_claim_release().unwrap(),
            &other.authorize_final_loss().unwrap()
        ))
        .is_err()
    );
    assert_eq!(
        before,
        f.namespaces
            .iter()
            .map(Namespace::checkpoint)
            .collect::<Vec<_>>()
    );
    for name in ["a", "b", "c", "shared", "keep"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
}
#[test]
fn bounded_action_calls_skip_completed_prefix_and_do_not_expand_original_exclusions() {
    let mut f = Fixture::new();
    let (e, p, l) = compile(&mut f, 0);
    for count in 1..=3 {
        let r = run(&mut f, &e, &l, |x| {
            x.run_limits.actions = 1;
            x.apply(
                &p,
                &p.authorize_claim_release().unwrap(),
                &p.authorize_final_loss().unwrap(),
            )
        })
        .unwrap();
        assert_eq!(r.historical_deleted_actions, Some(count));
        assert_eq!(r.effects.len(), 1);
        assert_eq!(
            r.phase,
            if count == 3 {
                control::Phase::Completed
            } else {
                control::Phase::Intent
            }
        );
    }
    f.namespaces[1]
        .apply(
            key("end-job"),
            Command::Withdraw {
                source: key("job"),
                owner: key("other"),
                expected_revision: 3,
            },
            &f.metadata,
            &limits(),
        )
        .unwrap();
    let again = run(&mut f, &e, &l, |x| {
        x.apply(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
        )
    })
    .unwrap();
    assert_eq!(again.phase, control::Phase::Completed);
    assert!(again.effects.is_empty());
    assert!(
        f.store
            .read_object(&f.publications["shared"].object)
            .is_ok()
    );
}

#[test]
fn wrong_concrete_metadata_identity_refuses_before_witness_admission() {
    let mut f = Fixture::new();
    let (e, p, l) = compile(&mut f, 0);
    let foreign = MetadataStore::create(
        &f.temp.path().join("foreign-metadata"),
        MetadataLimits {
            nodes: 1000,
            bytes: 4 * 1024 * 1024,
            database_bytes: 32 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let original = std::mem::replace(&mut f.metadata, foreign);
    let before = (f.ledger.checkpoint(), f.witness_ledger.checkpoint());
    assert!(matches!(
        run(&mut f, &e, &l, |x| x.apply(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap()
        )),
        Err(control::Error::Authority)
    ));
    assert!(matches!(
        run(&mut f, &e, &l, |x| x.status(&p)),
        Err(control::Error::Authority)
    ));
    assert_eq!(
        before,
        (f.ledger.checkpoint(), f.witness_ledger.checkpoint())
    );
    f.metadata = original;
}

#[test]
fn completed_page_witness_refuses_a_known_restored_source_prefix() {
    let mut f = Fixture::new();
    let (e, p, l) = compile(&mut f, 0);
    let minimum = f.namespaces[0].checkpoint();
    let source = f.temp.path().join("ns-0/claims.sqlite");
    let backup = f.temp.path().join("before-page.sqlite");
    std::fs::copy(&source, &backup).unwrap();
    assert!(
        run(&mut f, &e, &l, |x| x.apply_hook(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
            |s| {
                if s == control::Stage::PageCompleted(0) {
                    Err(control::Error::Injected)
                } else {
                    Ok(())
                }
            }
        ))
        .is_err()
    );
    drop(f.namespaces.remove(0));
    std::fs::copy(&backup, &source).unwrap();
    f.namespaces.insert(
        0,
        Namespace::open(&f.temp.path().join("ns-0"), &minimum).unwrap(),
    );
    assert!(matches!(
        run(&mut f, &e, &l, |x| x.apply(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap()
        )),
        Err(control::Error::Recovery)
    ));
    for name in ["a", "b", "c"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
}
#[test]
fn expired_window_cannot_resume_remaining_source_pages() {
    let mut f = Fixture::new();
    let (e, p, l) = compile(&mut f, 0);
    let first = run(&mut f, &e, &l, |x| {
        x.run_limits.pages = 1;
        x.apply(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
        )
    })
    .unwrap();
    assert_eq!(first.completed_pages, Some(1));
    let before = f
        .namespaces
        .iter()
        .map(Namespace::checkpoint)
        .collect::<Vec<_>>();
    f.clock.0.set(700);
    assert!(
        run(&mut f, &e, &l, |x| x.apply(
            &p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap()
        ))
        .is_err()
    );
    assert_eq!(
        before,
        f.namespaces
            .iter()
            .map(Namespace::checkpoint)
            .collect::<Vec<_>>()
    );
    for name in ["a", "b", "c"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
}
