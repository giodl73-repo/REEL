use super::*;
use crate::{
    AckLevel, Request,
    ledger::CapacityLedger,
    local::{
        LocalStore,
        removal::{self, tests::Finite},
    },
    retention::{self, release},
    retirement_intent::progress::{local_work, release_work},
};
use std::{fs, io::Cursor};
use tig_core::{
    LogicalPath,
    retirement::{
        Activity, Binding, Catalogue, ClockPoint, InactiveInterval, Policy, Preview,
        review::{
            Copy, CopyState, Inventory, Kind, Obligation, Protection, Release, Review,
            decision::{Approval, Consent, Decision, Target},
        },
    },
};

fn retirement(
    f: &mut Fixture,
    objects: &BTreeSet<Object>,
    stores: &BTreeMap<String, LocalStore>,
    grants: &BTreeMap<(Object, String), retention::RetentionGrant>,
) -> ObservedIntent {
    let selected = objects
        .iter()
        .enumerate()
        .map(|(i, o)| {
            (
                LogicalPath::parse(&format!("retired/item-{i}.tig")).unwrap(),
                o.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let preview = Preview::create(
        &Catalogue {
            snapshot: d("approved-native-block"),
            bindings: selected
                .iter()
                .map(|(p, o)| {
                    (
                        p.clone(),
                        Binding {
                            object: o.clone(),
                            activity: Activity::Inactive(InactiveInterval {
                                clock: d("clock"),
                                identity: d("interval"),
                                started: 1,
                                through: 100,
                            }),
                        },
                    )
                })
                .collect(),
        },
        selected.keys().cloned().collect(),
        Policy {
            identity: d("final-D-M"),
            minimum_inactive_ticks: 10,
            places: ["D".into(), "M".into()].into(),
        },
        ClockPoint {
            era: d("clock"),
            tick: 100,
        },
    )
    .unwrap();
    let selected_grants = grants
        .iter()
        .filter(|((o, _), _)| objects.contains(o))
        .map(|(_, g)| g)
        .collect::<Vec<_>>();
    let releases = selected_grants
        .iter()
        .map(|g| Release {
            claim: g.claim_id().clone(),
            object: g.request().publication.object.clone(),
        })
        .collect::<BTreeSet<_>>();
    let review = Review::create(
        preview,
        d("subject"),
        stores
            .iter()
            .map(|(p, s)| (p.clone(), s.identity().clone()))
            .collect(),
        Protection {
            checkpoint: d("finite-native-protection"),
            complete: true,
            obligations: releases
                .iter()
                .map(|r| Obligation {
                    claim: r.claim.clone(),
                    object: r.object.clone(),
                    owner: d("subject"),
                    kind: Kind::Ordinary,
                })
                .collect(),
        },
        Inventory {
            checkpoint: d("native-inventory"),
            complete: true,
            stores: stores.values().map(|s| s.identity().clone()).collect(),
            copies: selected_grants
                .iter()
                .map(|g| Copy {
                    object: g.request().publication.object.clone(),
                    store: g.store().clone(),
                    state: CopyState::Verified {
                        evidence: g.claim_id().clone(),
                    },
                })
                .collect(),
        },
        releases.clone(),
    )
    .unwrap();
    let decision = Decision::authorize(
        &review,
        Approval {
            namespace: f.j.identity().clone(),
            operation: d("native-block-retire"),
            owner: d("subject"),
            evidence: d("explicit-native-final-loss"),
        },
        Consent {
            release_owned_claims: releases,
            remove_copies: objects
                .iter()
                .flat_map(|o| {
                    stores.values().map(move |s| Target {
                        object: o.clone(),
                        store: s.identity().clone(),
                    })
                })
                .collect(),
            accept_intentional_loss: objects.clone(),
        },
    )
    .unwrap();
    let enrollment = Enrollment {
        owner: key("controller"),
        subject: d("subject"),
    };
    let candidate =
        crate::retirement_intent::stage(&mut f.j, &mut f.n, &enrollment, &review, &decision)
            .unwrap();
    let ready = crate::retirement_intent::finish(&mut f.j, &mut f.n, candidate).unwrap();
    let prepared =
        crate::retirement_intent::prepare(&mut f.j, &f.n, ready, key("native-prepare")).unwrap();
    crate::retirement_intent::adopt(&mut f.j, &f.n, &prepared, key("native-adopt")).unwrap();
    crate::retirement_intent::observe(&mut f.j, &f.n, &enrollment, &d("native-block-retire"))
        .unwrap()
}
fn remove(
    f: &mut Fixture,
    ctx: &ObservedIntent,
    ledger: &mut CapacityLedger,
    store: &LocalStore,
    object: &Object,
) -> removal::Result<()> {
    if inspect(&mut f.j, &f.n, ctx, object)
        .unwrap()
        .removed
        .contains_key(store.identity())
    {
        return Ok(());
    }
    let w = local_work::current(&mut f.j, &f.n, ctx, object)
        .unwrap()
        .unwrap();
    let r = removal::apply(
        removal::Execution {
            store,
            ledger,
            job: w.job(),
            journal: &mut f.j,
            nodes: &f.n,
            intent: ctx,
            protection: &mut Finite::default(),
        },
        w.plan(),
    )?;
    finish(
        &mut f.j,
        &mut f.n,
        ctx,
        object,
        w.plan().scope().attempt,
        Action::Remove {
            store: store.identity().clone(),
        },
        Outcome::Removed {
            evidence: r.evidence,
            allocation: Allocation {
                before: Some(r.file_data_before),
                reclaimed: None,
            },
        },
    )
    .unwrap();
    Ok(())
}
#[test]
fn two_asset_block_runs_native_d_m_final_retirement_offline_resume_and_preserves_unapproved_bytes()
{
    let mut f = Fixture::new_block();
    let objects = members(&f);
    let catalogue: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../examples/teaching/catalogue.json"
    ))
    .unwrap();
    let mut payloads = catalogue["assets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            let bytes = a["content"].as_str().unwrap().as_bytes().to_vec();
            (Object::from_bytes(&bytes), bytes)
        })
        .collect::<BTreeMap<_, _>>();
    for bytes in [
        b"public synthetic retired report 1".as_slice(),
        b"public synthetic invoice".as_slice(),
    ] {
        payloads.insert(Object::from_bytes(bytes), bytes.to_vec());
    }
    let mut stores = ["D", "M"]
        .into_iter()
        .map(|p| {
            (
                p.to_string(),
                LocalStore::create_removable(&f.temp.path().join(p), key(p)).unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut ledger = CapacityLedger::create(
        &f.temp.path().join("ledger"),
        key("fixture-volume"),
        key("epoch"),
        0,
    )
    .unwrap();
    crate::local_capacity::LocalCapacityProbe::bind(&stores["D"], &mut ledger)
        .unwrap()
        .observe(&stores["D"], &mut ledger, key("probe"), key("sample"))
        .unwrap();
    let job = ledger
        .reserve(
            key("native-block-job"),
            key("controller"),
            128 * 1024 * 1024,
        )
        .unwrap()
        .reference()
        .clone();
    let mut grants = BTreeMap::new();
    for object in f.old.values().collect::<BTreeSet<_>>() {
        for (p, store) in &stores {
            let req = Request {
                operation: key(&format!("put-{p}-{}", object.sha256.as_str())),
                owner: key("controller"),
                object: object.clone(),
                required: AckLevel::ProcessRestart,
            };
            let plan = crate::accounted::plan(store, &req).unwrap();
            crate::accounted::publish_accounted(
                &mut ledger,
                store,
                &job,
                &plan,
                &mut Cursor::new(&payloads[object]),
            )
            .unwrap();
            let plan = retention::plan(
                store,
                &retention::RetentionRequest {
                    operation: key(&format!("hold-{p}-{}", object.sha256.as_str())),
                    owner: key("controller"),
                    publication: req,
                    minimum_ack: AckLevel::ProcessRestart,
                },
            )
            .unwrap();
            let g = retention::retain(&mut ledger, store, &job, &plan)
                .unwrap()
                .grant;
            grants.insert((object.clone(), p.clone()), g);
        }
    }
    let b = begin(&mut f);
    let (anchor, attempt) = anchor(&b);
    apply(&mut f.j, &f.n, &f.ctx, anchor, attempt).unwrap();
    report(&mut f.j, &mut f.n, &f.ctx, anchor, attempt).unwrap();
    let retained = f.retained();
    assert!(retained.values().all(|o| !objects.contains(o)));
    let ctx = retirement(&mut f, &objects, &stores, &grants);
    for object in &objects {
        for (p, store) in &stores {
            let g = &grants[&(object.clone(), p.clone())];
            let a = release_work::start(&mut f.j, &mut f.n, &ctx, object, g.claim_id(), |_| {
                Ok(release_work::ReleaseWork::new(
                    release::plan(
                        store,
                        g,
                        ctx.decision(),
                        &Enrollment {
                            owner: key("controller"),
                            subject: d("subject"),
                        },
                    )
                    .unwrap(),
                    job.clone(),
                ))
            })
            .unwrap();
            let w = release_work::current(&mut f.j, &f.n, &ctx, object)
                .unwrap()
                .unwrap();
            let r = release::apply(
                &mut ledger,
                store,
                w.job(),
                w.plan(),
                &mut f.j,
                &f.n,
                &ctx,
                a.attempt,
            )
            .unwrap();
            finish(
                &mut f.j,
                &mut f.n,
                &ctx,
                object,
                a.attempt,
                Action::Release {
                    claim: g.claim_id().clone(),
                },
                Outcome::Released {
                    evidence: r.evidence,
                },
            )
            .unwrap();
        }
        local_work::start(
            &mut f.j,
            &mut f.n,
            &ctx,
            object,
            stores["D"].identity(),
            |attempt| {
                Ok(local_work::LocalWork::new(
                    removal::plan(&stores["D"], &ctx, object, attempt).unwrap(),
                    job.clone(),
                ))
            },
        )
        .unwrap();
        remove(&mut f, &ctx, &mut ledger, &stores["D"], object).unwrap();
    }
    let first = objects.first().unwrap();
    local_work::start(
        &mut f.j,
        &mut f.n,
        &ctx,
        first,
        stores["M"].identity(),
        |attempt| {
            Ok(local_work::LocalWork::new(
                removal::plan(&stores["M"], &ctx, first, attempt).unwrap(),
                job.clone(),
            ))
        },
    )
    .unwrap();
    fs::rename(f.temp.path().join("M"), f.temp.path().join("M.offline")).unwrap();
    assert!(remove(&mut f, &ctx, &mut ledger, &stores["M"], first).is_err());
    assert!(
        inspect(&mut f.j, &f.n, &ctx, first)
            .unwrap()
            .in_flight
            .is_some()
    );
    fs::rename(f.temp.path().join("M.offline"), f.temp.path().join("M")).unwrap();
    f.reopen();
    ledger = CapacityLedger::open(&f.temp.path().join("ledger"), &ledger.checkpoint()).unwrap();
    for p in ["D", "M"] {
        stores.insert(p.into(), LocalStore::open(&f.temp.path().join(p)).unwrap());
    }
    let ctx = crate::retirement_intent::observe(
        &mut f.j,
        &f.n,
        &Enrollment {
            owner: key("controller"),
            subject: d("subject"),
        },
        &d("native-block-retire"),
    )
    .unwrap();
    for object in &objects {
        remove(&mut f, &ctx, &mut ledger, &stores["D"], object).unwrap(); // completed D is skipped
        if local_work::current(&mut f.j, &f.n, &ctx, object)
            .unwrap()
            .is_none()
        {
            local_work::start(
                &mut f.j,
                &mut f.n,
                &ctx,
                object,
                stores["M"].identity(),
                |attempt| {
                    Ok(local_work::LocalWork::new(
                        removal::plan(&stores["M"], &ctx, object, attempt).unwrap(),
                        job.clone(),
                    ))
                },
            )
            .unwrap();
        }
        remove(&mut f, &ctx, &mut ledger, &stores["M"], object).unwrap();
        for store in stores.values() {
            assert!(store.read_object(object).is_err());
        }
    }
    for object in retained.values() {
        for (p, store) in &stores {
            assert!(
                retention::verify(&mut ledger, store, &grants[&(object.clone(), p.clone())])
                    .is_ok()
            );
        }
    }
    println!(
        "BLOCK NATIVE PASS: two approved assets -> one history projection -> native D/M grant/copy retirement; offline M resumes; shared cel and history-only invoice retain verified D/M custody. Complete authority is a finite fixture; net reclaim unknown."
    );
}
