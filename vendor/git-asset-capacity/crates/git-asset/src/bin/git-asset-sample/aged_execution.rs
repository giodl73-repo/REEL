//! Explicit fixture adapter -> retained age -> existing paged execution.
use super::*;
use git_asset_model::graph::Reference;
use git_asset_store::{
    final_retirement::paged::{self as selection, control},
    source_claims::{age, subset},
};
use std::cell::Cell;
struct Clock {
    policy: Digest,
    time: Cell<u64>,
}
impl final_provider::Clock for Clock {
    fn now(&self, p: &Digest) -> final_provider::Result<u64> {
        if p != &self.policy {
            return Err(final_provider::Error::Clock);
        }
        Ok(self.time.get())
    }
}

pub(super) fn run(d: &mut Demo, first: &str, reverted: &str) -> Result<Object> {
    let before = d.copy_snapshot()?;
    let roster = d.final_roster.clone().ok_or("missing final roster")?;
    let mut snapshots = Vec::new();
    for commit in [first, reverted] {
        let view = d.repository.commit(GitId::parse(commit)?)?;
        snapshots.push(d.repository.native(
            &view,
            Scope::new([LogicalPath::parse("records")?])?,
            Scope::new([LogicalPath::parse("definitions")?])?,
            native::Limits::default(),
        )?);
    }
    let category = category_preview::run(&d.root, &mut snapshots[0], &mut d.metadata)?;
    let category_doc: Value =
        serde_json::from_slice(&fs::read(d.root.join("native-category.json"))?)?;
    let assets: BTreeSet<Reference> = category_doc["preview"]["assets"]
        .as_array()
        .ok_or("category assets missing")?
        .iter()
        .map(|v| serde_json::from_value(v["asset"].clone()))
        .collect::<std::result::Result<_, _>>()?;
    let main = d.namespaces[0]
        .source(&key("main"))?
        .ok_or("main missing")?;
    let mut requested = Vec::new();
    let mut occurrences = Vec::new();
    for snapshot in &snapshots {
        let all = Demo::flat_entries(snapshot, None)?;
        let root = Tree::build(&all, &mut d.metadata, TreeLimits::default())?
            .0
            .root()
            .clone();
        let original = main
            .predecessors
            .iter()
            .find(|v| v.selection.context == *snapshot.checkpoint() && v.selection.root == root)
            .ok_or("category source mapping differs")?;
        let selected = Demo::flat_entries(snapshot, Some(&assets))?;
        if selected.is_empty() || !selected.values().any(|o| o == &d.objects["original"]) {
            return Err("category block differs".into());
        }
        let bindings = Tree::build(&selected, &mut d.metadata, TreeLimits::default())?
            .0
            .root()
            .clone();
        requested.push(selection::Selection {
            namespace: d.namespaces[0].identity().clone(),
            source: key("main"),
            predecessor_revision: original.revision,
            bindings,
        });
        occurrences.push(age::Request {
            namespace: d.namespaces[0].identity().clone(),
            source: key("main"),
            predecessor_revision: original.revision,
        });
    }
    d.metadata.finish()?;
    let job = d.namespaces[0].source(&key("job"))?.ok_or("job missing")?;
    if job.current.is_some() || job.pending.is_some() {
        return Err("job still active".into());
    }
    for old in job.predecessors {
        let revision = d.namespaces[0]
            .source(&key("job"))?
            .ok_or("job missing")?
            .revision;
        d.namespaces[0].apply(
            key(&format!("aged-retire-job-{}", old.revision)),
            Command::Retire {
                source: key("job"),
                owner: key("sample"),
                expected_revision: revision,
                predecessor_revision: old.revision,
            },
            &d.metadata.finished_reader()?,
            &limits(),
        )?;
    }
    if d.copy_snapshot()? != before {
        return Err("history release changed bytes".into());
    }
    let witness = LocalStore::create(&d.root.join("aged-witness"), key("aged-witness"))?;
    let mut witness_ledger = CapacityLedger::create(
        &d.root.join("aged-witness-ledger"),
        key("aged-witness-capacity"),
        key("sample-epoch"),
        0,
    )?;
    LocalCapacityProbe::bind(&witness, &mut witness_ledger)?.observe(
        &witness,
        &mut witness_ledger,
        key("probe"),
        key("sample"),
    )?;
    let witness_job = witness_ledger
        .reserve(key("aged-controller"), key("sample"), 512 * 1024 * 1024)?
        .reference()
        .clone();
    let source = subset::Enrollment {
        authority: Digest::blob(b"public sample subset authority"),
        namespace: d.namespaces[0].identity().clone(),
        policy: hash(d.namespaces[0].policy())?,
        owner: key("sample"),
        classes: vec![Kind::Branch, Kind::Draft],
        limits: limits(),
        writes: 10000,
        write_bytes: 8 * 1024 * 1024,
    };
    let mut enrollment = control::Enrollment {
        age: None,
        selection: selection::Enrollment {
            authority: source.authority.clone(),
            epoch: Digest::blob(b"public sample final epoch"),
            owner: key("sample"),
            metadata: d.metadata.checkpoint().identity().clone(),
            scope: roster.scope()?,
            classes: [Kind::Branch, Kind::Draft].into_iter().collect(),
        },
        sources: vec![source],
        context: Digest::blob(b"public sample aged context"),
        clock: Digest::blob(b"public sample simulated clock"),
        witness: Place::local(&witness, 1)?,
        capacity: witness_ledger.identity().clone(),
        max_window: 60000,
        max_metadata_bytes: 512 * 1024 * 1024,
    };
    let mut registry = Namespace::create(
        &d.root.join("aged-registry"),
        key("sample-aged-registry"),
        key("sample-age-epoch"),
        policy(),
    )?;
    registry.apply(
        key("enroll-aged-observations"),
        Command::Enroll {
            source: key("observations"),
            owner: key("sample"),
            class: Kind::Retention,
        },
        &d.metadata,
        &limits(),
    )?;
    let age_enrollment = age::Enrollment {
        authority: enrollment.selection.authority.clone(),
        epoch: enrollment.selection.epoch.clone(),
        metadata: enrollment.selection.metadata.clone(),
        registry: registry.identity().clone(),
        registry_policy: hash(registry.policy())?,
        source: key("observations"),
        owner: key("sample"),
        clock: enrollment.clock.clone(),
        observers: vec![age::Observer {
            namespace: d.namespaces[0].identity().clone(),
            policy: hash(d.namespaces[0].policy())?,
            owners: vec![key("sample")],
            classes: [Kind::Branch, Kind::Draft].into_iter().collect(),
        }],
    };
    enrollment.age = Some(control::aged::Policy {
        enrollment: age_enrollment.clone(),
        required_age_millis: 500,
        metadata_boundary: Digest::blob(b"explicit sample metadata-only registry"),
    });
    let age_limits = age::Limits::default();
    let minimum = registry.checkpoint();
    age::Capture {
        enrollment: &age_enrollment,
        registry: &mut registry,
        minimum: &minimum,
        metadata: &mut d.metadata,
        limits: &age_limits,
    }
    .initialize()?;
    let clock = Clock {
        policy: enrollment.clock.clone(),
        time: Cell::new(100),
    };
    for request in &occurrences {
        let minimum = registry.checkpoint();
        let observed = d.namespaces[0].checkpoint();
        age::Capture {
            enrollment: &age_enrollment,
            registry: &mut registry,
            minimum: &minimum,
            metadata: &mut d.metadata,
            limits: &age_limits,
        }
        .capture(request, &mut d.namespaces[0], &observed, &clock)?;
    }
    let bounds = selection::Limits::default();
    let input = selection::request(
        requested,
        selection::Paging {
            max_bindings: 1,
            ..Default::default()
        },
        &mut d.metadata,
        &bounds,
    )?;
    d.metadata.finish()?;
    let manifest = selection::build(
        &enrollment.selection,
        &input,
        &roster,
        &mut d.namespaces,
        &mut d.metadata,
        &bounds,
    )?;
    let mut copies = Vec::new();
    for ((store, ledger), p) in d.stores.iter().zip(&d.ledgers).zip(&d.config.providers) {
        for name in ["original", "shared"] {
            if before[p.assignments[0].place.as_str()].get(name).is_some() {
                copies.push(final_bulk::Copy {
                    store: store.identity().clone(),
                    capacity: ledger.identity().clone(),
                    publication: placement::publication(&d.config, p, store, &d.objects[name])?,
                });
            }
        }
    }
    let copies = control::copies(copies, &mut d.metadata, &bounds)?;
    d.metadata.finish()?;
    let request = control::Request {
        operation: key("public-category-aged-loss"),
        manifest,
        copies,
        window: final_provider::Window {
            not_before: 50,
            expires: 5000,
        },
    };
    let minimum = registry.checkpoint();
    clock.time.set(1000);
    let decision = control::aged::plan(
        &enrollment,
        request,
        &roster,
        &mut d.namespaces,
        &mut d.metadata,
        &d.stores.iter().collect::<Vec<_>>(),
        &bounds,
        &mut control::aged::Context {
            registry: &mut registry,
            minimum: &minimum,
            limits: &age_limits,
        },
        &clock,
    )?;
    d.metadata.finish()?;
    let inventory = control::inspect(
        &enrollment,
        &decision,
        &mut d.metadata.finished_reader()?,
        &bounds,
    )?;
    if inventory.actions().len() != 2
        || inventory
            .subjects()
            .filter(|s| s.hold.is_none())
            .map(|s| &s.object)
            .collect::<Vec<_>>()
            != vec![&d.objects["original"]]
    {
        return Err("aged category eligibility differs".into());
    }
    let mut jobs = d
        .config
        .providers
        .iter()
        .map(|p| control::StoreJob {
            store: p.place.instance.clone(),
            job: p.job.clone(),
        })
        .collect::<Vec<_>>();
    jobs.sort_by(|a, b| a.store.cmp(&b.store));
    let claim_consent = decision.authorize_claim_release()?;
    let final_consent = decision.authorize_final_loss()?;
    let source_before = d.namespaces[0].checkpoint();
    let report = {
        let mut places = d
            .stores
            .iter()
            .zip(&mut d.ledgers)
            .zip(&d.config.providers)
            .map(|((store, ledger), p)| placement::Local {
                store,
                ledger,
                job: &p.job,
            })
            .collect::<Vec<_>>();
        let mut execution = control::Execution {
            enrollment: &enrollment,
            roster: &roster,
            namespaces: &mut d.namespaces,
            metadata: &mut d.metadata,
            places: &mut places,
            jobs: &jobs,
            witness: placement::Local {
                store: &witness,
                ledger: &mut witness_ledger,
                job: &witness_job,
            },
            clock: &clock,
            admitted_context: &enrollment.context,
            limits: &bounds,
            run_limits: control::RunLimits::default(),
        };
        clock.time.set(500);
        let early = execution.apply_aged(
            &decision,
            &claim_consent,
            &final_consent,
            control::aged::Context {
                registry: &mut registry,
                minimum: &minimum,
                limits: &age_limits,
            },
            |_| Ok(()),
        );
        if early.is_ok()
            || execution.namespaces[0].checkpoint() != source_before
            || Demo::snapshot_at(&d.root, &d.objects)? != before
        {
            return Err("age deadline did not refuse before loss".into());
        }
        clock.time.set(1000);
        execution.apply_aged(
            &decision,
            &claim_consent,
            &final_consent,
            control::aged::Context {
                registry: &mut registry,
                minimum: &minimum,
                limits: &age_limits,
            },
            |_| Ok(()),
        )?
    };
    let after = d.copy_snapshot()?;
    let removed = report
        .effects
        .iter()
        .filter(|e| {
            !e.receipt.historical && e.receipt.outcome == Some(final_provider::Outcome::Deleted)
        })
        .map(|e| e.receipt.logical_bytes_removed)
        .sum::<u64>();
    if report.phase != control::Phase::Completed
        || !report.holds.is_empty()
        || report.age_inheritance_unknown
        || removed != 58
        || ["working", "history", "backup"]
            .iter()
            .any(|r| after[*r].get("original").is_some())
    {
        return Err("aged category loss incomplete".into());
    }
    for role in ["working", "backup"] {
        for name in ["shared", "fixed"] {
            if after[role][name] != before[role][name] {
                return Err("aged loss harmed current/shared bytes".into());
            }
        }
    }
    paged_playback::record(
        d,
        &enrollment,
        &decision,
        &report,
        &before,
        &after,
        clock.time.get(),
    )?;
    save(
        &d.root.join("aged-category-loss.json"),
        &json!({"schema":"tig.sample-aged-category-loss.design-1","category":category,"enrollment":enrollment,"decision":serde_json::from_slice::<Value>(&decision.to_json()?)?,"report":report,
        "occurrences":occurrences,"early_deadline_refused":true,"simulated_clock":true,"capture_time":100,"required_age_millis":500,"execution_time":1000,
        "copies_before":before,"copies_after":after,"synthetic_claim_consent":true,"synthetic_final_consent":true,
        "scope_limit":"fixture-owned category-to-flat adapter with recorded paged final playback; hardware/provider/authority/lifetime acceptance remain open"}),
    )
}
