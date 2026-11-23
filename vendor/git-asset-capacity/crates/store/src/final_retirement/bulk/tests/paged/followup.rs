use super::*;
mod operator;
mod recovery;
use control::followup as fresh;

fn request(f: &Fixture, p: &control::Decision, operation: &str, copies: Object) -> fresh::Request {
    fresh::Request {
        operation: key(operation),
        copies,
        window: provider::Window {
            not_before: 600,
            expires: 900,
        },
        origin: fresh::Origin {
            decision: p.clone(),
            witness_job: f.witness_job.clone(),
            jobs: vec![control::StoreJob {
                store: f.store.identity().clone(),
                job: f.job.clone(),
            }],
        },
    }
}
fn plan(
    f: &mut Fixture,
    e: &control::Enrollment,
    r: fresh::Request,
    l: &selection::Limits,
) -> control::Result<fresh::Decision> {
    let stores = [&f.store];
    let mut witness = Local {
        store: &f.witness,
        ledger: &mut f.witness_ledger,
        job: &f.witness_job,
    };
    let result = fresh::Planning {
        enrollment: e,
        roster: &f.roster,
        namespaces: &mut f.namespaces,
        nodes: &mut f.metadata,
        stores: &stores,
        origin_witness: &mut witness,
        clock: &f.clock,
        age: None,
        limits: l,
    }
    .plan(r);
    f.metadata.finish().unwrap();
    result
}
fn execute<T>(
    f: &mut Fixture,
    e: &control::Enrollment,
    l: &selection::Limits,
    call: impl FnOnce(&mut fresh::Execution<'_, '_>) -> T,
) -> T {
    let mut reader = f.metadata.finished_reader().unwrap();
    let jobs = [control::StoreJob {
        store: f.store.identity().clone(),
        job: f.job.clone(),
    }];
    let mut places = [Local {
        store: &f.store,
        ledger: &mut f.ledger,
        job: &f.job,
    }];
    call(&mut fresh::Execution {
        enrollment: e,
        roster: &f.roster,
        namespaces: &mut f.namespaces,
        metadata: &mut reader,
        places: &mut places,
        jobs: &jobs,
        witness: Local {
            store: &f.witness,
            ledger: &mut f.witness_ledger,
            job: &f.witness_job,
        },
        clock: &f.clock,
        admitted_context: &e.context,
        limits: l,
        run_limits: fresh::RunLimits::default(),
    })
}
fn complete(
    f: &mut Fixture,
    e: &control::Enrollment,
    p: &control::Decision,
    l: &selection::Limits,
) {
    let done = run(f, e, l, |x| {
        x.apply(
            p,
            &p.authorize_claim_release().unwrap(),
            &p.authorize_final_loss().unwrap(),
        )
    })
    .unwrap();
    assert_eq!(done.phase, control::Phase::Completed);
}
fn source_checkpoints(f: &Fixture) -> Vec<source_claims::Checkpoint> {
    f.namespaces.iter().map(Namespace::checkpoint).collect()
}

#[test]
fn cancelled_partial_origin_does_not_release_remaining_claims() {
    let mut f = Fixture::new();
    let (e, original, l) = compile(&mut f, 0);
    assert!(matches!(
        run(&mut f, &e, &l, |x| x.apply_hook(
            &original,
            &original.authorize_claim_release().unwrap(),
            &original.authorize_final_loss().unwrap(),
            |s| if s == control::Stage::Source(0, subset::Stage::Retired) {
                Err(control::Error::Injected)
            } else {
                Ok(())
            }
        )),
        Err(control::Error::Injected)
    ));
    let stopped = run(&mut f, &e, &l, |x| {
        x.cancel(
            &original,
            &original.authorize_claim_release().unwrap(),
            &original.authorize_final_loss().unwrap(),
        )
    })
    .unwrap();
    assert_eq!(stopped.phase, control::Phase::Cancelled);
    assert_eq!(stopped.historical_retired_pages, Some(1));
    f.clock.0.set(700);
    let before = source_checkpoints(&f);
    let r = request(
        &f,
        &original,
        "after-partial-cancel",
        original.request().copies.clone(),
    );
    let next = plan(&mut f, &e, r, &l).unwrap();
    let view = fresh::inspect(&e, &next, &mut f.metadata.finished_reader().unwrap(), &l).unwrap();
    // A retired page is not complete settlement of the original selection.
    // Remaining historical obligations still protect every requested object.
    assert_eq!(view.actions().len(), 0);
    for name in ["a", "b", "c", "shared"] {
        assert!(
            view.subjects()
                .any(|s| s.object == f.publications[name].object
                    && s.hold == Some(control::Hold::SurvivingClaims))
        );
    }
    let done = execute(&mut f, &e, &l, |x| {
        x.apply(&next, &next.authorize_final_loss().unwrap())
    })
    .unwrap();
    assert_eq!(done.phase, control::Phase::Completed);
    assert_eq!(done.historical_deleted_actions, Some(0));
    for name in ["a", "b", "c", "shared", "keep"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
    assert_eq!(source_checkpoints(&f), before);
}

#[test]
fn followup_rejects_scope_size_owner_and_original_witness_substitution() {
    let (mut f, e, original, l, _) = recovered();
    let before = (
        source_checkpoints(&f),
        f.ledger.checkpoint(),
        f.witness_ledger.checkpoint(),
    );
    for case in 0..4 {
        let mut copy = Copy {
            store: f.store.identity().clone(),
            capacity: f.ledger.identity().clone(),
            publication: f.publications[if case == 0 { "keep" } else { "a" }].clone(),
        };
        if case == 1 {
            copy.publication.object.bytes += 1;
        }
        if case == 2 {
            copy.publication.owner = key("another-owner");
        }
        let copies = control::copies(vec![copy], &mut f.metadata, &l).unwrap();
        f.metadata.finish().unwrap();
        let mut r = request(&f, &original, &format!("invalid-followup-{case}"), copies);
        if case == 3 {
            r.origin.witness_job = f.job.clone();
        }
        assert!(plan(&mut f, &e, r, &l).is_err(), "case {case}");
        assert_eq!(
            (
                source_checkpoints(&f),
                f.ledger.checkpoint(),
                f.witness_ledger.checkpoint()
            ),
            before
        );
    }
    for name in ["a", "b", "c", "shared", "keep"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
}

#[test]
fn unavailable_candidate_proof_refuses_apply_but_cannot_block_witness_stop() {
    let (mut f, e, _, l, next) = recovered();
    fresh::inspect(&e, &next, &mut f.metadata.finished_reader().unwrap(), &l).unwrap();
    let header = serde_json::to_value(&next).unwrap();
    let subjects: Object = serde_json::from_value(header["subjects"].clone()).unwrap();
    // Damage an acknowledged node in the synthetic provider, preserving its
    // length/counters so actual hash verification must detect the loss.
    let db = rusqlite::Connection::open(f.temp.path().join("metadata/nodes.sqlite")).unwrap();
    assert_eq!(
        db.execute(
            "UPDATE nodes SET body=zeroblob(size) WHERE oid=?1",
            [subjects.sha256.as_str()]
        )
        .unwrap(),
        1
    );
    drop(db);
    let auth = next.authorize_final_loss().unwrap();
    let before = source_checkpoints(&f);
    assert!(execute(&mut f, &e, &l, |x| x.apply(&next, &auth)).is_err());
    let mut witness = Local {
        store: &f.witness,
        ledger: &mut f.witness_ledger,
        job: &f.witness_job,
    };
    let jobs = [control::StoreJob {
        store: f.store.identity().clone(),
        job: f.job.clone(),
    }];
    let mut stop = fresh::Cancellation {
        inspection: fresh::Inspection {
            enrollment: &e,
            roster: &f.roster,
            metadata: Some(f.metadata.finished_reader().unwrap()),
            places: &mut [],
            jobs: &jobs,
            witness: &mut witness,
            admitted_context: &e.context,
            limits: &l,
        },
    };
    let report = stop.cancel(&next, &auth).unwrap();
    assert_eq!(report.phase, control::Phase::Cancelled);
    assert!(!report.metadata_available);
    assert!(report.cleanup_unknown);
    assert_eq!(report.historical_deleted_actions, None);
    assert_eq!(
        stop.inspection.status(&next).unwrap().phase,
        control::Phase::Cancelled
    );
    assert_eq!(source_checkpoints(&f), before);
    for name in ["a", "b", "c", "shared", "keep"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
}

#[test]
fn ended_foreign_job_allows_new_copy_decision_without_reconstructing_retired_history() {
    let mut f = Fixture::new();
    let (e, original, l) = compile(&mut f, 0);
    complete(&mut f, &e, &original, &l);
    let copy = Copy {
        store: f.store.identity().clone(),
        capacity: f.ledger.identity().clone(),
        publication: f.publications["shared"].clone(),
    };
    let copies = control::copies(vec![copy], &mut f.metadata, &l).unwrap();
    f.metadata.finish().unwrap();
    f.clock.0.set(700);
    let r = request(&f, &original, "fresh-held", copies.clone());
    let held = plan(&mut f, &e, r, &l).unwrap();
    let view = fresh::inspect(&e, &held, &mut f.metadata.finished_reader().unwrap(), &l).unwrap();
    assert_eq!(view.actions().len(), 0);
    assert!(
        view.subjects()
            .any(|s| s.object == f.publications["shared"].object
                && s.hold == Some(control::Hold::SurvivingClaims))
    );
    for (op, command) in [
        (
            "end-job",
            Command::Withdraw {
                source: key("job"),
                owner: key("other"),
                expected_revision: 3,
            },
        ),
        (
            "retire-job-history",
            Command::Retire {
                source: key("job"),
                owner: key("other"),
                expected_revision: 4,
                predecessor_revision: 3,
            },
        ),
    ] {
        f.namespaces[1]
            .apply(key(op), command, &f.metadata, &limits())
            .unwrap();
    }
    assert!(
        f.namespaces[0]
            .source(&key("main"))
            .unwrap()
            .unwrap()
            .predecessors
            .is_empty()
    );
    let old = run(&mut f, &e, &l, |x| {
        x.apply(
            &original,
            &original.authorize_claim_release().unwrap(),
            &original.authorize_final_loss().unwrap(),
        )
    })
    .unwrap();
    assert_eq!(old.historical_deleted_actions, Some(3));
    assert!(
        f.store
            .read_object(&f.publications["shared"].object)
            .is_ok()
    );
    let before = source_checkpoints(&f);
    let r = request(&f, &original, "fresh-after-job", copies);
    let next = plan(&mut f, &e, r, &l).unwrap();
    assert_ne!(next.id().unwrap(), held.id().unwrap());
    assert_eq!(
        fresh::inspect(&e, &next, &mut f.metadata.finished_reader().unwrap(), &l)
            .unwrap()
            .actions()
            .len(),
        1
    );
    assert_eq!(source_checkpoints(&f), before);
    assert!(matches!(
        execute(&mut f, &e, &l, |x| x
            .apply(&next, &held.authorize_final_loss().unwrap())),
        Err(control::Error::Authority)
    ));
    let report = execute(&mut f, &e, &l, |x| {
        x.apply(&next, &next.authorize_final_loss().unwrap())
    })
    .unwrap();
    assert_eq!(
        report.phase,
        control::Phase::Completed,
        "{:?}",
        report.holds
    );
    assert_eq!(report.historical_deleted_actions, Some(1));
    assert_eq!(source_checkpoints(&f), before);
    assert!(
        !f.store
            .object_present(&f.publications["shared"].object)
            .unwrap()
    );
    assert!(f.store.read_object(&f.publications["keep"].object).is_ok());
}

#[test]
fn originally_unavailable_provider_recovery_needs_new_approval_and_keeps_old_exclusions() {
    let mut f = Fixture::new();
    let (e, first, l) = compile(&mut f, 0);
    let mut r = first.request().clone();
    r.operation = key("original-offline");
    let original = control::plan(
        &e,
        r,
        &f.roster,
        &mut f.namespaces,
        &mut f.metadata,
        &[],
        &l,
    )
    .unwrap();
    f.metadata.finish().unwrap();
    let settled = run(&mut f, &e, &l, |x| {
        x.places = &mut [];
        x.apply(
            &original,
            &original.authorize_claim_release().unwrap(),
            &original.authorize_final_loss().unwrap(),
        )
    })
    .unwrap();
    assert_eq!(settled.phase, control::Phase::Completed);
    assert_eq!(
        control::inspect(
            &e,
            &original,
            &mut f.metadata.finished_reader().unwrap(),
            &l
        )
        .unwrap()
        .actions()
        .len(),
        0
    );
    f.clock.0.set(700);
    let r = request(
        &f,
        &original,
        "recovered-provider",
        first.request().copies.clone(),
    );
    let before = source_checkpoints(&f);
    let next = plan(&mut f, &e, r, &l).unwrap();
    assert_eq!(
        fresh::inspect(&e, &next, &mut f.metadata.finished_reader().unwrap(), &l)
            .unwrap()
            .actions()
            .len(),
        3
    );
    let first = execute(&mut f, &e, &l, |x| {
        x.run_limits.actions = 1;
        x.apply(&next, &next.authorize_final_loss().unwrap())
    })
    .unwrap();
    assert_eq!(first.phase, control::Phase::Intent);
    assert_eq!(first.historical_deleted_actions, Some(1));
    let view = fresh::inspect(&e, &next, &mut f.metadata.finished_reader().unwrap(), &l).unwrap();
    let mut admitted = 0;
    for (_, action) in view.actions() {
        if provider::admitted(&mut f.ledger, &f.job, action).unwrap() {
            admitted += 1;
        }
    }
    assert_eq!(admitted, 1);
    let done = execute(&mut f, &e, &l, |x| {
        x.apply(&next, &next.authorize_final_loss().unwrap())
    })
    .unwrap();
    assert_eq!(done.phase, control::Phase::Completed, "{:?}", done.holds);
    assert_eq!(done.historical_deleted_actions, Some(3));
    assert_eq!(source_checkpoints(&f), before);
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
    let old = run(&mut f, &e, &l, |x| {
        x.apply(
            &original,
            &original.authorize_claim_release().unwrap(),
            &original.authorize_final_loss().unwrap(),
        )
    })
    .unwrap();
    assert_eq!(old.historical_deleted_actions, Some(0));
}

#[test]
fn unsealed_original_cannot_supply_followup_provenance() {
    let mut f = Fixture::new();
    let (e, original, l) = compile(&mut f, 0);
    f.clock.0.set(700);
    let r = request(
        &f,
        &original,
        "not-sealed",
        original.request().copies.clone(),
    );
    let before = (source_checkpoints(&f), f.witness_ledger.checkpoint());
    assert!(matches!(
        plan(&mut f, &e, r, &l),
        Err(control::Error::Recovery)
    ));
    assert_eq!(
        (source_checkpoints(&f), f.witness_ledger.checkpoint()),
        before
    );
    for name in ["a", "b", "c", "shared", "keep"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
}

fn recovered() -> (
    Fixture,
    control::Enrollment,
    control::Decision,
    selection::Limits,
    fresh::Decision,
) {
    let mut f = Fixture::new();
    let (e, first, l) = compile(&mut f, 0);
    let mut r = first.request().clone();
    r.operation = key("offline-origin");
    let original = control::plan(
        &e,
        r,
        &f.roster,
        &mut f.namespaces,
        &mut f.metadata,
        &[],
        &l,
    )
    .unwrap();
    f.metadata.finish().unwrap();
    let done = run(&mut f, &e, &l, |x| {
        x.places = &mut [];
        x.apply(
            &original,
            &original.authorize_claim_release().unwrap(),
            &original.authorize_final_loss().unwrap(),
        )
    })
    .unwrap();
    assert_eq!(done.phase, control::Phase::Completed);
    f.clock.0.set(700);
    let r = request(
        &f,
        &original,
        "new-copy-followup",
        first.request().copies.clone(),
    );
    let next = plan(&mut f, &e, r, &l).unwrap();
    (f, e, original, l, next)
}

#[test]
fn transient_use_at_provider_boundary_invalidates_fresh_consent_even_after_abort() {
    let (mut f, e, _, l, next) = recovered();
    let selected = root(
        &mut f.metadata,
        &[("input", f.publications["a"].object.clone())],
    );
    let reader = f.metadata.finished_reader().unwrap();
    let mut peer =
        Namespace::open(&f.temp.path().join("ns-1"), &f.namespaces[1].checkpoint()).unwrap();
    let mut changed = false;
    let report = execute(&mut f, &e, &l, |x| {
        x.apply_hook(
            &next,
            &next.authorize_final_loss().unwrap(),
            None,
            |stage| {
                if !changed && matches!(stage, control::Stage::Provider(_, provider::Stage::Bound))
                {
                    changed = true;
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
                    let receipt = peer
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
                            prepared: receipt.reference,
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
    assert!(changed);
    assert_eq!(report.phase, control::Phase::Intent);
    assert_eq!(report.historical_deleted_actions, Some(0));
    assert!(report.holds.iter().any(|h| h.kind == "provider_held"));
    for name in ["a", "b", "c"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
    assert!(matches!(
        execute(&mut f, &e, &l, |x| x
            .apply(&next, &next.authorize_final_loss().unwrap())),
        Err(control::Error::Stale)
    ));
}

#[test]
fn metadata_and_clock_free_stop_preserves_partial_loss_and_cannot_restart() {
    let (mut f, e, _, l, next) = recovered();
    let before = source_checkpoints(&f);
    let auth = next.authorize_final_loss().unwrap();
    assert!(matches!(
        execute(&mut f, &e, &l, |x| x.apply_hook(
            &next,
            &auth,
            None,
            |stage| {
                if stage == control::Stage::Removed(0) {
                    Err(control::Error::Injected)
                } else {
                    Ok(())
                }
            }
        )),
        Err(control::Error::Injected)
    ));
    let jobs = [control::StoreJob {
        store: f.store.identity().clone(),
        job: f.job.clone(),
    }];
    let mut witness = Local {
        store: &f.witness,
        ledger: &mut f.witness_ledger,
        job: &f.witness_job,
    };
    let stopped = fresh::Cancellation {
        inspection: fresh::Inspection {
            enrollment: &e,
            roster: &f.roster,
            metadata: None,
            places: &mut [],
            jobs: &jobs,
            witness: &mut witness,
            admitted_context: &e.context,
            limits: &l,
        },
    }
    .cancel(&next, &auth)
    .unwrap();
    assert_eq!(stopped.phase, control::Phase::Cancelled);
    assert_eq!(stopped.historical_deleted_actions, None);
    assert!(stopped.cleanup_unknown);
    f.clock.0.set(0);
    let done = execute(&mut f, &e, &l, |x| x.apply(&next, &auth)).unwrap();
    assert_eq!(done.phase, control::Phase::Cancelled);
    assert_eq!(done.historical_deleted_actions, Some(1));
    assert_eq!(done.pending_cancellations, Some(0));
    assert!(!done.cleanup_unknown);
    assert_eq!(source_checkpoints(&f), before);
    assert_eq!(
        ["a", "b", "c"]
            .iter()
            .filter(|name| !f
                .store
                .object_present(&f.publications[**name].object)
                .unwrap())
            .count(),
        1
    );
    for name in ["shared", "keep"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
}

#[test]
fn aged_original_proof_still_qualifies_after_source_history_is_gone() {
    let mut f = Fixture::new();
    let (mut e, manual, l) = compile(&mut f, 0);
    let mut ages = super::aged::AgeFixture::new(&mut f, &mut e);
    f.clock.0.set(10);
    ages.capture(&mut f, &e, &manual);
    f.clock.0.set(100);
    let original = ages.plan(&mut f, &e, &manual, &l).unwrap();
    let done = run(&mut f, &e, &l, |x| {
        x.apply_aged(
            &original,
            &original.authorize_claim_release().unwrap(),
            &original.authorize_final_loss().unwrap(),
            ages.context(),
            |_| Ok(()),
        )
    })
    .unwrap();
    assert_eq!(done.phase, control::Phase::Completed);
    for (op, command) in [
        (
            "end-job",
            Command::Withdraw {
                source: key("job"),
                owner: key("other"),
                expected_revision: 3,
            },
        ),
        (
            "retire-job-history",
            Command::Retire {
                source: key("job"),
                owner: key("other"),
                expected_revision: 4,
                predecessor_revision: 3,
            },
        ),
    ] {
        f.namespaces[1]
            .apply(key(op), command, &f.metadata, &limits())
            .unwrap();
    }
    f.clock.0.set(700);
    let copies = control::copies(
        vec![Copy {
            store: f.store.identity().clone(),
            capacity: f.ledger.identity().clone(),
            publication: f.publications["shared"].clone(),
        }],
        &mut f.metadata,
        &l,
    )
    .unwrap();
    let r = request(&f, &original, "aged-followup", copies);
    let stores = [&f.store];
    let mut witness = Local {
        store: &f.witness,
        ledger: &mut f.witness_ledger,
        job: &f.witness_job,
    };
    let next = fresh::Planning {
        enrollment: &e,
        roster: &f.roster,
        namespaces: &mut f.namespaces,
        nodes: &mut f.metadata,
        stores: &stores,
        origin_witness: &mut witness,
        clock: &f.clock,
        age: Some(ages.context()),
        limits: &l,
    }
    .plan(r)
    .unwrap();
    f.metadata.finish().unwrap();
    let before = source_checkpoints(&f);
    assert!(matches!(
        execute(&mut f, &e, &l, |x| x
            .apply(&next, &next.authorize_final_loss().unwrap())),
        Err(control::Error::Authority)
    ));
    let report = execute(&mut f, &e, &l, |x| {
        x.apply_aged(&next, &next.authorize_final_loss().unwrap(), ages.context())
    })
    .unwrap();
    assert_eq!(
        report.phase,
        control::Phase::Completed,
        "{:?}",
        report.holds
    );
    assert_eq!(report.historical_deleted_actions, Some(1));
    assert_eq!(source_checkpoints(&f), before);
    assert!(
        !f.store
            .object_present(&f.publications["shared"].object)
            .unwrap()
    );
}
