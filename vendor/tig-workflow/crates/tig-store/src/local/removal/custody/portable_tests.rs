use super::*;
use crate::{acquire::SourceError, provider::custody as pc};
use std::{cell::Cell, rc::Rc};
fn profile(c: &Case) -> pc::Profile {
    let legacy = c.p.custody.as_ref().unwrap();
    pc::Profile {
        source_domain: legacy.source_domain.clone(),
        retained: legacy
            .retained
            .iter()
            .map(|s| pc::Survivor {
                commitment: pc::native::commitment(&s.grant),
                failure_domain: s.failure_domain.clone(),
            })
            .collect(),
        required_domains: legacy.required_domains,
        minimum_ack: legacy.minimum_ack,
        allow_cooperative: true,
    }
}
fn convert(c: &mut Case) {
    let p = profile(c);
    c.p = portable::plan(
        &c.f.store,
        &c.f.ctx,
        &c.p.scope.object,
        c.p.scope.attempt,
        &p,
    )
    .unwrap();
}
fn apply_native(
    c: &mut Case,
) -> std::result::Result<RemovalReceipt, crate::provider::removal::Error> {
    use crate::provider::removal as port;
    let mut providers = c
        .backups
        .iter_mut()
        .map(|b| pc::native::Native {
            store: &b.store,
            ledger: &mut b.ledger,
            grant: &b.grant,
        })
        .collect::<Vec<_>>();
    let mut endpoints = providers
        .iter_mut()
        .map(|p| p as &mut dyn pc::Retainer)
        .collect::<Vec<_>>();
    let mut protection = Finite::default();
    let mut executor = port::native::retained(
        Case::execute(&mut c.f, &mut protection),
        &c.p,
        &mut endpoints,
    )?;
    Ok(port::remove_exact(&mut executor, &port::native::request(&c.p)?, true)?.evidence)
}
// A separate, nonfilesystem protocol fixture mixed with a real native survivor.
struct Remote {
    claim: pc::Commitment,
    bytes: Vec<u8>,
    live: Rc<Cell<usize>>,
    offline: Rc<Cell<bool>>,
}
struct RemoteHold {
    claim: pc::Commitment,
    live: Rc<Cell<usize>>,
    offline: Rc<Cell<bool>>,
    valid: bool,
}
impl Drop for RemoteHold {
    fn drop(&mut self) {
        self.live.set(self.live.get() - 1);
    }
}
impl pc::Held for RemoteHold {
    fn commitment(&self) -> &pc::Commitment {
        &self.claim
    }
    fn revalidate(&mut self) -> std::result::Result<(), SourceError> {
        self.valid = false;
        if self.offline.get() {
            return Err(SourceError::Unavailable);
        }
        self.valid = true;
        Ok(())
    }
    fn fresh(&self) -> std::result::Result<(), SourceError> {
        if self.valid {
            Ok(())
        } else {
            Err(SourceError::Stale)
        }
    }
}
impl pc::Retainer for Remote {
    fn describe(&self) -> pc::Capabilities {
        pc::Capabilities {
            place: self.claim.place.clone(),
            max_object_bytes: 1024,
            max_ack: AckLevel::ProcessRestart,
            enforcement: Some(pc::Enforcement::ProviderHeld),
        }
    }
    fn acquire(
        &mut self,
        wanted: &pc::Commitment,
    ) -> std::result::Result<Box<dyn pc::Held + '_>, SourceError> {
        if self.offline.get() {
            return Err(SourceError::Unavailable);
        }
        if self.claim != *wanted {
            return Err(SourceError::Stale);
        }
        if Object::from_bytes(&self.bytes) != wanted.object {
            return Err(SourceError::Integrity);
        }
        self.live.set(self.live.get() + 1);
        Ok(Box::new(RemoteHold {
            claim: self.claim.clone(),
            live: self.live.clone(),
            offline: self.offline.clone(),
            valid: true,
        }))
    }
}
#[test]
fn mixed_custody_holds_through_actual_unlink_and_preserves_legacy_encoding() {
    let mut c = Case::new();
    let legacy = serde_json::to_value(&c.p).unwrap();
    assert!(legacy.get("provider_custody").is_none());
    convert(&mut c);
    let object = c.p.scope.object.clone();
    let (a, b) = c.backups.split_at_mut(1);
    let mut native = pc::native::Native {
        store: &a[0].store,
        ledger: &mut a[0].ledger,
        grant: &a[0].grant,
    };
    let live = Rc::new(Cell::new(0));
    let offline = Rc::new(Cell::new(false));
    let mut remote = Remote {
        claim: pc::native::commitment(&b[0].grant),
        bytes: seed("old"),
        live: live.clone(),
        offline,
    };
    let r = portable::run(
        Case::execute(&mut c.f, &mut Finite::default()),
        &c.p,
        &mut [&mut native, &mut remote],
        |stage| {
            if matches!(stage, Stage::Marked | Stage::Unlinked | Stage::Completed) {
                assert_eq!(live.get(), 1);
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(live.get(), 0);
    c.report(&r);
    assert!(c.f.store.read_object(&object).is_err());
    assert!(removal_entry_refuses(&mut c));
}
fn removal_entry_refuses(c: &mut Case) -> bool {
    super::super::super::apply(Case::execute(&mut c.f, &mut Finite::default()), &c.p).is_err()
}
#[test]
fn custody_lost_after_begin_keeps_source_and_exact_profile_recovers() {
    let mut c = Case::new();
    convert(&mut c);
    let (a, b) = c.backups.split_at_mut(1);
    let mut native = pc::native::Native {
        store: &a[0].store,
        ledger: &mut a[0].ledger,
        grant: &a[0].grant,
    };
    let offline = Rc::new(Cell::new(false));
    let live = Rc::new(Cell::new(0));
    let mut remote = Remote {
        claim: pc::native::commitment(&b[0].grant),
        bytes: seed("old"),
        live: live.clone(),
        offline: offline.clone(),
    };
    assert!(
        portable::run(
            Case::execute(&mut c.f, &mut Finite::default()),
            &c.p,
            &mut [&mut native, &mut remote],
            |stage| {
                if stage == Stage::Marked {
                    offline.set(true);
                }
                Ok(())
            }
        )
        .is_err()
    );
    assert_eq!(live.get(), 0);
    assert!(verify_payload(&c.f.store, &c.p.scope.object).is_ok());
    let mut changed = c.p.clone();
    changed.provider_custody.as_mut().unwrap().allow_cooperative = false;
    changed.scope.preservation = Some(changed.provider_custody.as_ref().unwrap().digest().unwrap());
    let saved = c.p.clone();
    c.p = changed;
    assert!(apply_native(&mut c).is_err());
    c.p = saved;
    c.reopen();
    let r = apply_native(&mut c).unwrap();
    c.report(&r);
    assert!(c.f.store.read_object(&c.p.scope.object).is_err());
}
#[test]
fn historical_portable_eviction_replays_without_custody_and_preserves_fresh_ingest() {
    let mut c = Case::new();
    convert(&mut c);
    let r = apply_native(&mut c).unwrap();
    c.report(&r);
    let fresh = Request {
        operation: key("fresh"),
        owner: key("owner"),
        object: c.p.scope.object.clone(),
        required: AckLevel::ProcessRestart,
    };
    crate::publish(&mut c.f.store, &fresh, &mut Cursor::new(seed("old"))).unwrap();
    c.reopen();
    let replay = {
        use crate::provider::removal as port;
        let mut protection = Finite::default();
        let mut endpoints = [];
        let mut executor = port::native::retained(
            Case::execute(&mut c.f, &mut protection),
            &c.p,
            &mut endpoints,
        )
        .unwrap();
        port::remove_exact(&mut executor, &port::native::request(&c.p).unwrap(), true)
            .unwrap()
            .evidence
    };
    assert_eq!(r.evidence, replay.evidence);
    assert!(c.f.store.read_object(&fresh.object).is_ok());
}
#[test]
fn portable_profile_child_exits_recover_marked_unlinked_and_completed() {
    for cut in ["marked", "unlinked", "completed"] {
        let mut c = Case::new();
        convert(&mut c);
        let controls = Controls {
            ledger: c.f.ledger.checkpoint(),
            journal: c.f.journal.checkpoint(),
            nodes: c.f.nodes.checkpoint(),
            job: c.f.job.clone(),
            plan: c.p.clone(),
            backups: c
                .backups
                .iter()
                .map(|b| BackupControl {
                    root: b.temp.path().to_owned(),
                    ledger: b.ledger.checkpoint(),
                })
                .collect(),
            provider_grants: c.backups.iter().map(|b| b.grant.clone()).collect(),
        };
        fs::write(
            c.f.temp.path().join("eviction-controls.json"),
            serde_json::to_vec(&controls).unwrap(),
        )
        .unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "local::removal::custody::tests::custody_child",
                "--nocapture",
            ])
            .env("TIG_RETAINED_EVICTION_TEST_ROOT", c.f.temp.path())
            .env("TIG_RETAINED_EVICTION_TEST_CUT", cut)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(76));
        c.reopen();
        let r = if cut == "completed" {
            portable::apply(
                Case::execute(&mut c.f, &mut Finite::default()),
                &c.p,
                &mut [],
            )
            .unwrap()
        } else {
            apply_native(&mut c).unwrap()
        };
        c.report(&r);
        assert!(c.f.store.read_object(&c.p.scope.object).is_err());
    }
}
