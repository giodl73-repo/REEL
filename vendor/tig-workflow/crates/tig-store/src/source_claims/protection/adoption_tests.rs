use super::*;
#[path = "adoption_tests/accepted_head.rs"]
mod accepted_head;
#[path = "adoption_tests/policy_registry.rs"]
mod policy_registry;
use crate::source_claims::{self as claims, Version, coordinated};
use work::adoption::{self, Stage as Cut};
struct Case {
    f: Fixture,
    source: SourceJournal,
}
impl Case {
    fn new() -> Self {
        let mut c = Self::uninitialized();
        adoption::enrollment::initialize(&mut c.f.index, &mut c.source, &mut c.f.nodes).unwrap();
        c
    }
    fn uninitialized() -> Self {
        let mut f = Fixture::bound(3);
        let initial = f.index.actor(&mut f.nodes, &key("main")).unwrap().selection;
        let source = SourceJournal::create_coordinated(
            &f.temp.path().join("source"),
            key("project"),
            key("epoch"),
            coordinated::Binding {
                index: f.index.identity().clone(),
                source: key("main"),
                actor: key("main"),
                owner: key("source-owner"),
                initial,
            },
        )
        .unwrap();
        Self { f, source }
    }
    fn batch(&mut self, id: &str, value: usize) -> batch::Batch {
        let history = self.f.request(
            "history",
            id,
            [(p(&format!("history/{value}")), Some(object(0)))].into(),
            true,
        );
        let main = self.f.request(
            "main",
            id,
            [(p("assets/a-00000"), Some(object(value)))].into(),
            true,
        );
        batch::Batch {
            operation: key(id),
            updates: vec![history, main],
        }
    }
    fn stage(&mut self, b: &batch::Batch) -> work::WorkRef {
        adoption::stage(
            &mut self.f.index,
            &mut self.source,
            b,
            None,
            &mut self.f.nodes,
        )
        .unwrap()
    }
    fn run(&mut self, r: &work::WorkRef, hook: impl FnMut(Cut) -> Result<()>) -> Result<Receipt> {
        adoption::execute_validated(
            &mut self.f.index,
            &mut self.source,
            r,
            &mut self.f.nodes,
            |_| Ok(()),
            hook,
        )
    }
    fn reopen(&mut self) {
        let checkpoint = self.source.checkpoint();
        self.source = SourceJournal::open(&self.f.temp.path().join("source"), &checkpoint).unwrap();
        self.f.reopen();
    }
    fn revisions(&mut self) -> (u64, u64) {
        (
            self.f
                .index
                .actor(&mut self.f.nodes, &key("history"))
                .unwrap()
                .revision,
            self.f
                .index
                .actor(&mut self.f.nodes, &key("main"))
                .unwrap()
                .revision,
        )
    }
}
#[test]
fn union_precedes_logical_adoption_and_selection_follows_it() {
    let mut c = Case::new();
    let b = c.batch("merge", 10);
    let r = c.stage(&b);
    assert!(matches!(
        work::execute(&mut c.f.index, &r, &mut c.f.nodes),
        Err(Error::Pending)
    ));
    assert!(matches!(
        c.f.index.update_batch(&b, &mut c.f.nodes),
        Err(Error::Pending)
    ));
    for (cut, logical_new) in [
        (Cut::ProtectionPrepared, false),
        (Cut::SourcePrepared, false),
        (Cut::SourceAdopted, true),
    ] {
        assert!(matches!(
            c.run(&r, |stage| if stage == cut {
                Err(Error::Injected)
            } else {
                Ok(())
            }),
            Err(Error::Injected)
        ));
        assert_eq!(c.revisions(), (1, 1));
        let source = c.source.source(&key("main")).unwrap().unwrap();
        assert_eq!(
            source.current.as_ref().unwrap().selection == b.updates[1].selection,
            logical_new
        );
        assert_eq!(
            c.f.index
                .inspect(&mut c.f.nodes, &object(0), &d("F"))
                .unwrap()
                .protected_by,
            vec![key("main")]
        );
        assert_eq!(
            c.f.index
                .inspect(&mut c.f.nodes, &object(10), &d("F"))
                .unwrap()
                .protected_by,
            vec![key("main")]
        );
        assert!(matches!(
            c.f.index.resume(&b.operation, &mut c.f.nodes),
            Err(Error::Pending)
        ));
        c.reopen();
    }
    c.run(&r, |_| Ok(())).unwrap();
    assert_eq!(c.revisions(), (2, 2));
    assert!(
        c.f.index
            .inspect(&mut c.f.nodes, &object(0), &d("F"))
            .unwrap()
            .protected_by
            .is_empty()
    );
    assert!(work::complete(&mut c.f.index, &r).unwrap());
}
#[test]
fn controlled_heads_refuse_raw_writes_while_other_sources_and_history_remain_usable() {
    let mut c = Case::new();
    let initial = c.source.coordinator().unwrap().initial.clone();
    let commands = [
        Command::Prepare {
            source: key("main"),
            owner: key("source-owner"),
            expected_revision: 2,
            selection: initial.clone(),
        },
        Command::Withdraw {
            source: key("main"),
            owner: key("source-owner"),
            expected_revision: 2,
        },
        Command::Abort {
            source: key("main"),
            owner: key("source-owner"),
            expected_revision: 2,
            prepared: Reference {
                journal: c.source.identity().clone(),
                operation: key("fake"),
            },
        },
        Command::Adopt {
            source: key("main"),
            owner: key("source-owner"),
            expected_revision: 2,
            prepared: Reference {
                journal: c.source.identity().clone(),
                operation: key("fake"),
            },
        },
    ];
    for (i, command) in commands.into_iter().enumerate() {
        assert!(matches!(
            c.source.apply(key(&format!("raw-{i}")), command),
            Err(claims::Error::Restricted)
        ));
    }
    let b = c.batch("scoped", 10);
    let r = c.stage(&b);
    let mut peer =
        SourceJournal::open(&c.f.temp.path().join("source"), &c.source.checkpoint()).unwrap();
    c.run(&r, |cut| {
        if cut == Cut::ProtectionPrepared {
            assert!(matches!(
                peer.apply(
                    key("cancel-during-lock"),
                    Command::Cancel {
                        source: key("main"),
                        owner: key("source-owner"),
                        expected_revision: 2,
                    }
                ),
                Err(claims::Error::Busy)
            ));
            peer.apply(
                key("unrelated"),
                Command::Prepare {
                    source: key("other"),
                    owner: key("other-owner"),
                    expected_revision: 0,
                    selection: initial.clone(),
                },
            )
            .unwrap();
        }
        Ok(())
    })
    .unwrap();
    c.source
        .apply(
            key("trim-initial"),
            Command::Retire {
                source: key("main"),
                owner: key("source-owner"),
                expected_revision: 4,
                predecessor_revision: 2,
            },
        )
        .unwrap();
    assert_eq!(
        c.source
            .source(&key("main"))
            .unwrap()
            .unwrap()
            .current
            .unwrap()
            .selection,
        b.updates[1].selection
    );
}
#[test]
fn cancel_and_failed_new_validation_never_select_the_protection_tuple() {
    for admitted in [false, true] {
        let mut c = Case::new();
        let b = c.batch("cancel", 10);
        let r = c.stage(&b);
        let mut calls = 0;
        assert!(matches!(
            adoption::execute_validated(
                &mut c.f.index,
                &mut c.source,
                &r,
                &mut c.f.nodes,
                |_| {
                    calls += 1;
                    if calls == 2 {
                        Err(Error::Stale)
                    } else {
                        Ok(())
                    }
                },
                |_| Ok(())
            ),
            Err(Error::Stale)
        ));
        assert_eq!(calls, 2);
        assert_eq!(c.revisions(), (1, 1));
        assert!(c.run(&r, |_| Ok(())).is_err());
        let mut c = Case::new();
        let b = c.batch("cancel-after-validation-case", 10);
        let r = c.stage(&b);
        if admitted {
            assert!(matches!(
                c.run(&r, |s| if s == Cut::ProtectionPrepared {
                    Err(Error::Injected)
                } else {
                    Ok(())
                }),
                Err(Error::Injected)
            ));
        }
        c.source
            .apply(
                key("terminal-stop"),
                Command::Cancel {
                    source: key("main"),
                    owner: key("source-owner"),
                    expected_revision: 2,
                },
            )
            .unwrap();
        assert!(c.run(&r, |_| Ok(())).is_err());
        assert_eq!(c.revisions(), (1, 1));
        assert!(!work::complete(&mut c.f.index, &r).unwrap());
    }
}
#[test]
fn changed_recipe_foreign_journal_and_historical_replay_cannot_rebind_or_rewind() {
    let mut c = Case::new();
    let b = c.batch("first", 10);
    let r = c.stage(&b);
    c.run(&r, |_| Ok(())).unwrap();
    let next = c.batch("second", 20);
    let nr = c.stage(&next);
    c.run(&nr, |_| Ok(())).unwrap();
    assert_eq!(c.revisions(), (3, 3));
    assert_eq!(c.stage(&b), r);
    assert!(c.run(&r, |_| Ok(())).unwrap().replayed);
    assert_eq!(c.revisions(), (3, 3));
    assert!(
        adoption::initialize(&mut c.f.index, &mut c.source, &mut c.f.nodes)
            .unwrap()
            .replayed
    );
    assert_eq!(
        c.source
            .source(&key("main"))
            .unwrap()
            .unwrap()
            .current
            .unwrap()
            .selection,
        next.updates[1].selection
    );
    let mut changed = b.clone();
    changed.updates[0].selection.context = d("changed");
    assert!(matches!(
        adoption::stage(
            &mut c.f.index,
            &mut c.source,
            &changed,
            None,
            &mut c.f.nodes
        ),
        Err(Error::Conflict)
    ));
    let mut foreign = SourceJournal::create_coordinated(
        &c.f.temp.path().join("foreign"),
        key("project"),
        key("epoch"),
        c.source.coordinator().unwrap().clone(),
    )
    .unwrap();
    assert!(matches!(
        adoption::execute(&mut c.f.index, &mut foreign, &r, &mut c.f.nodes),
        Err(Error::Invalid)
    ));
}
#[test]
fn source_commit_uncertainty_reopens_and_reconciles_exact_original_commands() {
    for after_adopt in [false, true] {
        let mut c = Case::new();
        let b = c.batch("uncertain", 10);
        let r = c.stage(&b);
        assert!(matches!(
            c.run(&r, |s| if s == Cut::ProtectionPrepared {
                Err(Error::Injected)
            } else {
                Ok(())
            }),
            Err(Error::Injected)
        ));
        let record = work::read(&mut c.f.index, &r, &c.f.nodes).unwrap();
        let link = record.adoption.as_ref().unwrap();
        let (p, pc, a, ac) = adoption::commands(&c.f.index, link, &b.operation, false).unwrap();
        let minimum = c.source.checkpoint();
        if after_adopt {
            c.source
                .apply_once(p.clone(), pc.clone(), |_| Ok(()), true)
                .unwrap();
        }
        let (id, command) = if after_adopt { (a, ac) } else { (p, pc) };
        let result = c.source.apply_once(
            id,
            command,
            |s| {
                if s == claims::Stage::AfterCommit {
                    Err(claims::Error::Injected)
                } else {
                    Ok(())
                }
            },
            true,
        );
        c.source.mark_uncertain(&result);
        assert!(result.is_err());
        c.source = SourceJournal::open(&c.f.temp.path().join("source"), &minimum).unwrap();
        adoption::resume_admitted(&mut c.f.index, &mut c.source, &r, &mut c.f.nodes).unwrap();
        assert_eq!(c.revisions(), (2, 2));
    }
}
#[derive(Serialize, Deserialize)]
struct Start {
    index: Checkpoint,
    source: Checkpoint,
    metadata: metadata::Checkpoint,
    cut: String,
}
#[test]
fn pending_source_retirement_and_profile_tampering_refuse_without_downgrade() {
    let mut c = Case::new();
    let b = c.batch("pending", 10);
    let r = c.stage(&b);
    assert!(matches!(
        c.run(&r, |s| if s == Cut::SourcePrepared {
            Err(Error::Injected)
        } else {
            Ok(())
        }),
        Err(Error::Injected)
    ));
    assert!(matches!(
        c.source.apply(
            key("retire-while-pending"),
            Command::Retire {
                source: key("main"),
                owner: key("source-owner"),
                expected_revision: 3,
                predecessor_revision: 2
            }
        ),
        Err(claims::Error::Pending)
    ));
    c.run(&r, |_| Ok(())).unwrap();
    let legacy_root = c.f.temp.path().join("legacy");
    let _legacy = SourceJournal::create(&legacy_root, key("legacy"), key("epoch")).unwrap();
    let legacy: serde_json::Value =
        serde_json::from_slice(&std::fs::read(legacy_root.join("claims.json")).unwrap()).unwrap();
    assert!(!legacy.as_object().unwrap().contains_key("coordinator"));
    let minimum = c.source.checkpoint();
    let path = c.f.temp.path().join("source/claims.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["schema"], "tig.source-claims.design-3");
    value["coordinator"]["actor"] = "history".into();
    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(matches!(
        SourceJournal::open(&c.f.temp.path().join("source"), &minimum),
        Err(claims::Error::InvalidReference)
    ));
}
#[test]
fn reservation_blocks_retirement_across_crash_before_logical_prepare() {
    let mut c = Case::new();
    let first = c.batch("first-reservation", 10);
    let first_ref = c.stage(&first);
    c.run(&first_ref, |_| Ok(())).unwrap();
    let next = c.batch("second-reservation", 20);
    let nr = c.stage(&next);
    assert!(matches!(
        c.run(&nr, |s| if s == Cut::ProtectionPrepared {
            Err(Error::Injected)
        } else {
            Ok(())
        }),
        Err(Error::Injected)
    ));
    c.reopen();
    let source = c.source.source(&key("main")).unwrap().unwrap();
    assert!(source.pending.is_none());
    assert_eq!(source.revision, 4);
    assert!(matches!(
        c.source.apply(
            key("retire-between-phases"),
            Command::Retire {
                source: key("main"),
                owner: key("source-owner"),
                expected_revision: 4,
                predecessor_revision: 2
            }
        ),
        Err(claims::Error::Pending)
    ));
    c.run(&nr, |_| Ok(())).unwrap();
    assert_eq!(c.revisions(), (3, 3));
    c.source
        .apply(
            key("retire-after-close"),
            Command::Retire {
                source: key("main"),
                owner: key("source-owner"),
                expected_revision: 6,
                predecessor_revision: 2,
            },
        )
        .unwrap();
}
#[test]
fn stale_unadmitted_work_abandons_its_reservation_and_allows_a_fresh_operation() {
    let mut c = Case::new();
    let b = c.batch("obsolete", 10);
    let r = c.stage(&b);
    assert!(matches!(
        c.run(&r, |s| if s == Cut::Reserved {
            Err(Error::Injected)
        } else {
            Ok(())
        }),
        Err(Error::Injected)
    ));
    assert!(matches!(
        adoption::execute_validated(
            &mut c.f.index,
            &mut c.source,
            &r,
            &mut c.f.nodes,
            |_| Err(Error::Stale),
            |_| Ok(())
        ),
        Err(Error::Stale)
    ));
    assert!(c.run(&r, |_| Ok(())).is_err());
    assert_eq!(c.revisions(), (1, 1));
    let next = c.batch("fresh-operation", 20);
    let nr = c.stage(&next);
    c.run(&nr, |_| Ok(())).unwrap();
    assert_eq!(c.revisions(), (2, 2));
    assert!(!work::complete(&mut c.f.index, &r).unwrap());
}
#[test]
fn adoption_child() {
    let Some(root) = std::env::var_os("TIG_PUBLIC_ADOPTION_CHILD") else {
        return;
    };
    let root = PathBuf::from(root);
    let start: Start =
        serde_json::from_slice(&std::fs::read(root.join("adoption-start.json")).unwrap()).unwrap();
    let mut index = Index::open(&root.join("index"), &start.index).unwrap();
    let mut source = SourceJournal::open(&root.join("source"), &start.source).unwrap();
    let mut nodes = metadata::MetadataStore::open(&root.join("nodes"), &start.metadata).unwrap();
    if start.cut.starts_with("Enrollment") {
        adoption::enrollment::initialize_hook(&mut index, &mut source, &mut nodes, |stage| {
            if format!("Enrollment{stage:?}") == start.cut {
                std::process::exit(73);
            }
            Ok(())
        })
        .unwrap();
        panic!("must exit at enrollment boundary");
    }
    let reference = work::discover(&mut index, 1).unwrap().pop().unwrap();
    if start.cut == "Staged" {
        std::process::exit(73);
    }
    adoption::execute_validated(
        &mut index,
        &mut source,
        &reference,
        &mut nodes,
        |_| Ok(()),
        |s| {
            if format!("{s:?}") == start.cut {
                std::process::exit(73);
            }
            Ok(())
        },
    )
    .unwrap();
    panic!("must exit at coordinator boundary");
}
#[test]
fn twelve_actual_exits_recover_source_and_projection_from_only_trusted_minima() {
    for cut in [
        "Staged",
        "HeadroomReserved",
        "Reserved",
        "MetadataReady",
        "ProtectionPrepared",
        "SourcePrepared",
        "SourceAdopted",
        "ProtectionSelected",
        "ReservationReleased",
        "SourceRoomReleased",
        "IndexRoomReleased",
        "Complete",
    ] {
        let mut c = Case::new();
        let b = c.batch("crash", 10);
        let r = c.stage(&b);
        let start = Start {
            index: c.f.index.checkpoint(),
            source: c.source.checkpoint(),
            metadata: c.f.nodes.checkpoint(),
            cut: cut.into(),
        };
        std::fs::write(
            c.f.temp.path().join("adoption-start.json"),
            encode(&start).unwrap(),
        )
        .unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "source_claims::protection::tests::adoption_tests::adoption_child",
                "--nocapture",
            ])
            .env("TIG_PUBLIC_ADOPTION_CHILD", c.f.temp.path())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73));
        c.f.index = Index::open(&c.f.temp.path().join("index"), &start.index).unwrap();
        c.source = SourceJournal::open(&c.f.temp.path().join("source"), &start.source).unwrap();
        c.f.nodes =
            metadata::MetadataStore::open(&c.f.temp.path().join("nodes"), &start.metadata).unwrap();
        let found = work::discover(&mut c.f.index, 1).unwrap().pop().unwrap();
        assert_eq!(r, found);
        if cut == "Staged"
            || cut == "HeadroomReserved"
            || cut == "Reserved"
            || cut == "MetadataReady"
        {
            assert!(matches!(
                adoption::resume_admitted(&mut c.f.index, &mut c.source, &found, &mut c.f.nodes),
                Err(Error::Pending)
            ));
            c.run(&found, |_| Ok(())).unwrap();
        } else {
            adoption::resume_admitted(&mut c.f.index, &mut c.source, &found, &mut c.f.nodes)
                .unwrap();
        }
        assert_eq!(c.revisions(), (2, 2));
        assert_eq!(
            c.source.source(&key("main")).unwrap().unwrap().current,
            Some(Version {
                revision: 4,
                selection: b.updates[1].selection.clone()
            })
        );
        assert!(work::complete(&mut c.f.index, &found).unwrap());
    }
}

#[test]
fn routed_actor_refuses_other_operation_ids_and_unlinked_work_before_effect() {
    let mut c = Case::new();
    let b = c.batch("direct-bypass", 10);
    let checkpoint = c.f.index.checkpoint();
    assert!(matches!(
        c.f.index.update(&b.updates[1], &mut c.f.nodes),
        Err(Error::Pending)
    ));
    assert!(matches!(
        c.f.index.update_batch(&b, &mut c.f.nodes),
        Err(Error::Pending)
    ));
    assert!(matches!(
        c.f.index.prepare_batch_hook(&b, &mut c.f.nodes, |_| Ok(())),
        Err(Error::Pending)
    ));
    assert!(matches!(
        work::stage(&mut c.f.index, &b.updates[1], None, &mut c.f.nodes),
        Err(Error::Pending)
    ));
    assert!(matches!(
        work::stage_batch(&mut c.f.index, &b, None, &mut c.f.nodes),
        Err(Error::Pending)
    ));
    assert_eq!(c.f.index.checkpoint(), checkpoint);
    assert!(
        work::lookup(&mut c.f.index, &b.operation)
            .unwrap()
            .is_none()
    );
    c.reopen();
    let mut changed = b.updates[1].clone();
    changed.operation = key("another-bypass");
    changed.ready = false;
    assert!(matches!(
        c.f.index.update(&changed, &mut c.f.nodes),
        Err(Error::Pending)
    ));
    assert_eq!(c.revisions(), (1, 1));
    let r = c.stage(&b);
    c.run(&r, |_| Ok(())).unwrap();
    changed.operation = key("after-controller");
    changed.expected_revision = 2;
    assert!(matches!(
        c.f.index.update(&changed, &mut c.f.nodes),
        Err(Error::Pending)
    ));
    assert_eq!(c.revisions(), (2, 2));
}

#[test]
fn routed_actor_preserves_unrelated_updates_and_guards_pending_selection_after_reopen() {
    let mut c = Case::new();
    let jobs = c.f.request(
        "jobs",
        "job-before",
        [(p("jobs/a"), Some(object(30)))].into(),
        true,
    );
    c.f.index.update(&jobs, &mut c.f.nodes).unwrap();
    let b = c.batch("routed-first", 10);
    let r = c.stage(&b);
    assert!(matches!(
        c.run(&r, |s| if s == Cut::SourceAdopted {
            Err(Error::Injected)
        } else {
            Ok(())
        }),
        Err(Error::Injected)
    ));
    c.reopen();
    assert!(matches!(
        c.f.index.resume(&b.operation, &mut c.f.nodes),
        Err(Error::Pending)
    ));
    adoption::resume_admitted(&mut c.f.index, &mut c.source, &r, &mut c.f.nodes).unwrap();
    let jobs =
        c.f.request("jobs", "job-after", [(p("jobs/a"), None)].into(), true);
    c.f.index.update(&jobs, &mut c.f.nodes).unwrap();
    let history = c.f.request(
        "history",
        "history-release",
        [(p("history/10"), None)].into(),
        true,
    );
    c.f.index.update(&history, &mut c.f.nodes).unwrap();
    c.reopen();
    let b = c.batch("routed-second", 20);
    let r = c.stage(&b);
    c.run(&r, |_| Ok(())).unwrap();
    assert_eq!(c.revisions(), (4, 3));
}

#[test]
fn routed_profile_seals_actor_and_source_without_changing_legacy_config() {
    let mut c = Case::new();
    let initial =
        c.f.index
            .actor(&mut c.f.nodes, &key("main"))
            .unwrap()
            .selection;
    let mut wrong = SourceJournal::create_coordinated(
        &c.f.temp.path().join("wrong-source"),
        key("project"),
        key("epoch"),
        coordinated::Binding {
            index: c.f.index.identity().clone(),
            source: key("different-source"),
            actor: key("main"),
            owner: key("source-owner"),
            initial,
        },
    )
    .unwrap();
    assert!(matches!(
        adoption::initialize(&mut c.f.index, &mut wrong, &mut c.f.nodes),
        Err(Error::Invalid)
    ));
    let checkpoint = c.f.index.checkpoint();
    let path = c.f.temp.path().join("index/claims.json");
    let saved = std::fs::read(&path).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&saved).unwrap();
    assert_eq!(value["schema"], "tig.source-claims.design-6");
    value["routing"]["source"] = "different-source".into();
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(matches!(
        Index::open(&c.f.temp.path().join("index"), &checkpoint),
        Err(Error::Journal(claims::Error::InvalidReference))
    ));
    std::fs::write(&path, saved).unwrap();
    c.reopen();
    assert_eq!(c.f.index.routing().unwrap().actor, key("main"));
    let legacy = Fixture::new(1);
    let value: serde_json::Value = serde_json::from_slice(
        &std::fs::read(legacy.temp.path().join("index/claims.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(value["schema"], "tig.source-claims.design-2");
    assert!(value.get("routing").is_none());
}

fn competing_source(c: &Case, name: &str) -> SourceJournal {
    SourceJournal::create_coordinated(
        &c.f.temp.path().join(name),
        key("project"),
        key("epoch"),
        c.source.coordinator().unwrap().clone(),
    )
    .unwrap()
}
#[test]
fn bound_enrollment_refuses_competing_identity_and_blocks_unenrolled_negative_proofs() {
    use adoption::enrollment as e;
    let mut c = Case::uninitialized();
    let mut other = competing_source(&c, "competing-source");
    assert_ne!(c.source.identity(), other.identity());
    assert!(matches!(
        c.f.index.inspect(&mut c.f.nodes, &object(99), &d("F")),
        Err(Error::Unknown)
    ));
    assert!(matches!(
        adoption::initialize(&mut c.f.index, &mut c.source, &mut c.f.nodes),
        Err(Error::Unknown)
    ));
    let before = other.checkpoint();
    let receipts = e::initialize(&mut c.f.index, &mut c.source, &mut c.f.nodes).unwrap();
    assert_eq!(receipts.index.checkpoint, c.f.index.checkpoint());
    assert_eq!(receipts.source.checkpoint, c.source.checkpoint());
    assert_eq!(
        e::selected(&mut c.f.index).unwrap(),
        Some(c.source.identity().clone())
    );
    assert!(matches!(
        e::initialize(&mut c.f.index, &mut other, &mut c.f.nodes),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        adoption::initialize(&mut c.f.index, &mut other, &mut c.f.nodes),
        Err(Error::Conflict)
    ));
    assert_eq!(before, other.checkpoint());
    assert!(
        c.f.index
            .inspect(&mut c.f.nodes, &object(99), &d("F"))
            .unwrap()
            .protected_by
            .is_empty()
    );
    let b = c.batch("bound-first", 10);
    let r = c.stage(&b);
    c.run(&r, |_| Ok(())).unwrap();
    let checkpoint = c.f.index.checkpoint();
    let source_checkpoint = c.source.checkpoint();
    let replay = e::initialize(&mut c.f.index, &mut c.source, &mut c.f.nodes).unwrap();
    assert!(replay.source.replayed && replay.index.replayed);
    assert_eq!(replay.index.reference, receipts.index.reference);
    assert_eq!(replay.source.reference, receipts.source.reference);
    assert_eq!(checkpoint, c.f.index.checkpoint());
    assert_eq!(source_checkpoint, c.source.checkpoint());
}
#[test]
fn enrollment_fences_only_its_scope_and_legacy_routed_profile_keeps_its_encoding() {
    use adoption::enrollment as e;
    let mut c = Case::uninitialized();
    let fence = c.f.index.lock_name("source-enrollment").unwrap();
    let mut peer = Index::open(&c.f.temp.path().join("index"), &c.f.index.checkpoint()).unwrap();
    assert!(matches!(
        e::initialize(&mut peer, &mut c.source, &mut c.f.nodes),
        Err(Error::Store(crate::Error::Busy))
    ));
    let job = c.f.request(
        "jobs",
        "during-enrollment-fence",
        [(p("jobs/running"), Some(object(80)))].into(),
        true,
    );
    c.f.index.update(&job, &mut c.f.nodes).unwrap();
    assert!(e::selected(&mut c.f.index).unwrap().is_none());
    drop(fence);
    e::initialize(&mut peer, &mut c.source, &mut c.f.nodes).unwrap();
    c.reopen();
    assert_eq!(
        e::selected(&mut c.f.index).unwrap(),
        Some(c.source.identity().clone())
    );
    let mut old = Fixture::routed(1);
    assert!(!old.index.requires_bound_source());
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(old.temp.path().join("index/claims.json")).unwrap())
            .unwrap();
    assert_eq!(config["schema"], "tig.source-claims.design-4");
    assert!(config.get("bound_source").is_none());
    assert!(
        old.index
            .inspect(&mut old.nodes, &object(99), &d("F"))
            .unwrap()
            .protected_by
            .is_empty()
    );
}
#[test]
fn acknowledged_enrollment_witness_rejects_rollback_and_missing_enrollment_is_unknown() {
    use adoption::enrollment as e;
    let mut c = Case::uninitialized();
    let old = c.f.index.checkpoint();
    let receipt = e::initialize(&mut c.f.index, &mut c.source, &mut c.f.nodes).unwrap();
    let path = c.f.temp.path().join("index");
    let mut db = rusqlite::Connection::open(path.join("claims.sqlite")).unwrap();
    let hash: String = db
        .query_row(
            "SELECT hash FROM events WHERE sequence=?1",
            [old.sequence() as i64],
            |r| r.get(0),
        )
        .unwrap();
    let bytes: i64 = db
        .query_row(
            "SELECT SUM(length(body)) FROM events WHERE sequence<=?1",
            [old.sequence() as i64],
            |r| r.get(0),
        )
        .unwrap();
    let tx = db.transaction().unwrap();
    tx.execute(
        "DELETE FROM events WHERE sequence>?1",
        [old.sequence() as i64],
    )
    .unwrap();
    tx.execute(
        "UPDATE meta SET sequence=?1,hash=?2,bytes=?3 WHERE id=1",
        rusqlite::params![old.sequence() as i64, hash, bytes],
    )
    .unwrap();
    tx.commit().unwrap();
    assert!(matches!(
        Index::open(&path, &receipt.index.checkpoint),
        Err(Error::Journal(claims::Error::Rollback))
    ));
    let mut older = Index::open(&path, &old).unwrap();
    assert!(matches!(
        older.inspect(&mut c.f.nodes, &object(99), &d("F")),
        Err(Error::Unknown)
    ));
    assert!(matches!(
        adoption::initialize(&mut older, &mut c.source, &mut c.f.nodes),
        Err(Error::Unknown)
    ));
}
#[test]
fn four_actual_enrollment_exits_recover_exact_identity_without_competitor_takeover() {
    use adoption::enrollment as e;
    for cut in ["Prepared", "SourcePrepared", "SourceAdopted", "Selected"] {
        let mut c = Case::uninitialized();
        let start = Start {
            index: c.f.index.checkpoint(),
            source: c.source.checkpoint(),
            metadata: c.f.nodes.checkpoint(),
            cut: format!("Enrollment{cut}"),
        };
        std::fs::write(
            c.f.temp.path().join("adoption-start.json"),
            encode(&start).unwrap(),
        )
        .unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "source_claims::protection::tests::adoption_tests::adoption_child",
                "--nocapture",
            ])
            .env("TIG_PUBLIC_ADOPTION_CHILD", c.f.temp.path())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73));
        c.f.index = Index::open(&c.f.temp.path().join("index"), &start.index).unwrap();
        c.source = SourceJournal::open(&c.f.temp.path().join("source"), &start.source).unwrap();
        c.f.nodes =
            metadata::MetadataStore::open(&c.f.temp.path().join("nodes"), &start.metadata).unwrap();
        let mut other = competing_source(&c, "after-crash-competitor");
        assert!(matches!(
            e::initialize(&mut c.f.index, &mut other, &mut c.f.nodes),
            Err(Error::Conflict)
        ));
        if cut != "Selected" {
            assert!(matches!(
                c.f.index.inspect(&mut c.f.nodes, &object(99), &d("F")),
                Err(Error::Pending)
            ));
            assert!(matches!(
                adoption::initialize(&mut c.f.index, &mut c.source, &mut c.f.nodes),
                Err(Error::Pending)
            ));
        }
        let receipt = e::initialize(&mut c.f.index, &mut c.source, &mut c.f.nodes).unwrap();
        assert_eq!(receipt.index.checkpoint, c.f.index.checkpoint());
        assert_eq!(
            e::selected(&mut c.f.index).unwrap(),
            Some(c.source.identity().clone())
        );
        assert_eq!(c.revisions(), (1, 1));
        let b = c.batch("after-enrollment-crash", 10);
        let r = c.stage(&b);
        c.run(&r, |_| Ok(())).unwrap();
        assert_eq!(c.revisions(), (2, 2));
    }
}

#[test]
fn reserved_outcomes_finish_both_journals_under_event_and_byte_pressure() {
    use crate::source_claims::{outcome_budget, progress_budget};
    for bytes in [false, true] {
        let mut c = Case::new();
        let b = c.batch("pressure-merge", 10);
        let r = c.stage(&b);
        assert!(matches!(
            c.run(&r, |s| if s == Cut::ProtectionPrepared {
                Err(Error::Injected)
            } else {
                Ok(())
            }),
            Err(Error::Injected)
        ));
        for journal in [&mut c.source, &mut c.f.index.journal] {
            if bytes {
                outcome_budget::tests::exhaust_bytes(journal)
            } else {
                outcome_budget::tests::exhaust_events(journal)
            }
        }
        let source_room = progress_budget::capacity(&mut c.source).unwrap();
        let index_room = progress_budget::capacity(&mut c.f.index.journal).unwrap();
        assert!(source_room.held_events > 0 && index_room.held_events > 0);
        if bytes {
            assert!(source_room.available_events > 1000 && index_room.available_events > 1000);
            assert!(source_room.available_bytes < 1024 && index_room.available_bytes < 1024)
        } else {
            assert_eq!(source_room.available_events, 0);
            assert_eq!(index_room.available_events, 0)
        }
        c.reopen();
        adoption::resume_admitted(&mut c.f.index, &mut c.source, &r, &mut c.f.nodes).unwrap();
        assert_eq!(c.revisions(), (2, 2));
        assert!(work::complete(&mut c.f.index, &r).unwrap());
        assert_eq!(
            progress_budget::capacity(&mut c.source)
                .unwrap()
                .held_events,
            0
        );
        assert_eq!(
            progress_budget::capacity(&mut c.f.index.journal)
                .unwrap()
                .held_events,
            0
        );
    }
}
#[test]
fn insufficient_index_room_abandons_only_unadmitted_source_work_and_releases_its_room() {
    use crate::source_claims::{outcome_budget, progress_budget};
    let mut c = Case::new();
    let b = c.batch("cannot-admit-merge", 10);
    let r = c.stage(&b);
    outcome_budget::tests::exhaust_events(&mut c.f.index.journal);
    assert!(c.run(&r, |_| Ok(())).is_err());
    assert_eq!(c.revisions(), (1, 1));
    assert_eq!(c.source.source(&key("main")).unwrap().unwrap().revision, 2);
    assert_eq!(
        progress_budget::capacity(&mut c.source)
            .unwrap()
            .held_events,
        0
    );
    assert!(!work::complete(&mut c.f.index, &r).unwrap());
    assert!(c.run(&r, |_| Ok(())).is_err());
}
