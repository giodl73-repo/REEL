mod final_absence;
use super::*;
use crate::{existing::ByteSource, orchestration::Place, placement as p};

struct Seed;
impl ByteSource for Seed {
    type Reader = Cursor<Vec<u8>>;
    fn open_verified(&mut self, object: &Object) -> crate::existing::Result<Self::Reader> {
        assert_eq!(*object, Object::from_bytes(b"asset"));
        Ok(Cursor::new(b"asset".to_vec()))
    }
}

#[test]
fn ordinary_reconciliation_holds_final_absence_and_accepts_only_explicit_fresh_birth() {
    let mut f = Fixture::new();
    let old = f.plan();
    f.apply(&old);
    let ns = f.namespaces[0].identity().clone();
    let cache = LocalStore::create_managed(
        &f.root.path().join("cache"),
        key("cache"),
        Binding::new(
            1,
            d("cache-boundary"),
            vec![Member {
                namespace: ns.clone(),
                places: vec![key("D"), key("F")],
            }],
        )
        .unwrap(),
    )
    .unwrap();
    let mut ledger = CapacityLedger::create(
        &f.root.path().join("cache-ledger"),
        key("cache-capacity"),
        key("epoch"),
        0,
    )
    .unwrap();
    LocalCapacityProbe::bind(&cache, &mut ledger)
        .unwrap()
        .observe(&cache, &mut ledger, key("probe"), key("sample"))
        .unwrap();
    let job = ledger
        .reserve(key("job"), key("owner"), 256 * 1024 * 1024)
        .unwrap()
        .reference()
        .clone();
    let config = p::Config {
        schema: "tig.placement-enrollment.design-1".into(),
        owner: key("owner"),
        authority: d("placement-authority"),
        seed: d("seed"),
        minimum_domains: 1,
        namespaces: vec![p::Authority {
            namespace: ns.clone(),
            policy: digest(f.namespaces[0].policy()).unwrap(),
        }],
        providers: vec![
            p::Provider {
                place: Place::local(&f.store, 1).unwrap(),
                role: p::Role::Retained,
                failure_domain: key("backup-domain"),
                capacity: f.ledger.identity().clone(),
                job: f.job.clone(),
                assignments: vec![p::Mapping {
                    namespace: ns.clone(),
                    place: key("M"),
                }],
            },
            p::Provider {
                place: Place::local(&cache, 1).unwrap(),
                role: p::Role::Cache,
                failure_domain: key("cache-domain"),
                capacity: ledger.identity().clone(),
                job: job.clone(),
                assignments: ["D", "F"]
                    .into_iter()
                    .map(|place| p::Mapping {
                        namespace: ns.clone(),
                        place: key(place),
                    })
                    .collect(),
            },
        ],
        budget: p::Budget {
            objects: 16,
            max_object_bytes: 1024,
            max_stream_bytes: 1024,
            max_peak_bytes: 128 * 1024 * 1024,
            limits: limits(),
        },
    };
    let mut seed = Seed;
    let mut run = |f: &mut Fixture| {
        let mut places = [
            p::Local {
                store: &f.store,
                ledger: &mut f.ledger,
                job: &f.job,
            },
            p::Local {
                store: &cache,
                ledger: &mut ledger,
                job: &job,
            },
        ];
        let mut e = p::Execution {
            config: &config,
            namespaces: &mut f.namespaces[..1],
            nodes: &f.nodes,
            places: &mut places,
            seed: &mut seed,
            seed_identity: &config.seed,
            allow_copy: true,
            allow_eviction: true,
            allow_reacquisition: true,
            allow_cancellation: true,
        };
        let page = e.plan(None).unwrap();
        (page.rows.len(), e.apply(&page).unwrap())
    };
    assert_eq!(run(&mut f).0, 0, "no old inventory-only retained floor");
    f.selected(0);
    let (rows, report) = run(&mut f);
    assert_eq!(rows, 1);
    assert!(
        report
            .holds
            .iter()
            .any(|e| matches!(e, p::Error::FinalRetirement))
    );
    assert!(!f.store.object_present(&f.publication.object).unwrap());
    let rp = reacquire::plan(
        &f.store,
        &old,
        reacquire::Request {
            publication: Publication {
                operation: key("explicit-fresh"),
                ..f.publication.clone()
            },
            capacity: f.ledger.identity().clone(),
            context: f.context.clone(),
            window: Window {
                not_before: 50,
                expires: 500,
            },
        },
    )
    .unwrap();
    reacquire::Execution {
        store: &f.store,
        ledger: &mut f.ledger,
        job: &f.job,
        clock: &f.clock,
        admitted_context: &f.context,
    }
    .apply(&rp, &rp.authorize().unwrap(), &mut Cursor::new(b"asset"))
    .unwrap();
    let (_, report) = run(&mut f);
    assert!(report.holds.is_empty(), "{:?}", report.holds);
    assert!(report.page_converged);
    assert_eq!(f.bytes(), b"asset");
    f.apply(&old);
    assert_eq!(f.bytes(), b"asset");
}
