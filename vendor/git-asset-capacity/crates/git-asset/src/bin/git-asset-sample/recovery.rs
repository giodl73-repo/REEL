//! Internal fresh-fixture child, not a general existing-cache resume interface.
use super::*;
use std::io::Read;
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Frame {
    pub root: PathBuf,
    pub plan: Value,
    pub enrollment: final_bulk::Enrollment,
    pub roster: final_claims::Roster,
    pub providers: Vec<placement::Provider>,
    pub namespace: source_claims::Checkpoint,
    pub metadata: git_asset_store::metadata::Checkpoint,
    pub ledgers: Vec<git_asset_store::ledger::Checkpoint>,
    pub witness: git_asset_store::ledger::Checkpoint,
    pub witness_job: git_asset_store::ledger::JournalRef,
}
pub(super) fn child(expected: &str) -> Result<Value> {
    let mut raw = Vec::new();
    std::io::stdin().take(65537).read_to_end(&mut raw)?;
    if raw.len() > 65536 || Digest::blob(&raw).as_str() != expected {
        return Err("invalid fixture child frame".into());
    }
    let value = identity::parse_json(&raw)?;
    if identity::canonical_json(&value)? != raw {
        return Err("noncanonical fixture frame".into());
    }
    let f: Frame = serde_json::from_value(value)?;
    let root = fs::canonicalize(&f.root)?;
    if root != f.root
        || root.ancestors().any(|p| p.join(".git").exists())
        || fs::read(root.join("final-child-frame.json"))? != raw
    {
        return Err("fixture frame ownership differs".into());
    }
    let child = |name: &str| -> Result<PathBuf> {
        let p = fs::canonicalize(root.join(name))?;
        if !p.starts_with(&root) {
            return Err("fixture child escaped root".into());
        }
        Ok(p)
    };
    for name in [
        "namespace",
        "metadata",
        "working",
        "history",
        "backup",
        "ledger-working",
        "ledger-history",
        "ledger-backup",
        "final-witness",
        "final-witness-ledger",
    ] {
        child(name)?;
    }
    if f.providers.len() != 3
        || f.ledgers.len() != 3
        || f.roster.members.len() != 1
        || f.namespace.identity() != &f.roster.members[0].namespace
    {
        return Err("fixture child scope differs".into());
    }
    let plan = final_bulk::Plan::from_json(&identity::canonical_json(&f.plan)?)?;
    if plan.enrollment().id()? != f.enrollment.id()?
        || f.enrollment.clock != Digest::blob(b"public sample simulated clock")
    {
        return Err("fixture child enrollment differs".into());
    }
    let mut namespaces = vec![Namespace::open(&child("namespace")?, &f.namespace)?];
    let mut metadata = MetadataStore::open(&child("metadata")?, &f.metadata)?;
    let mut stores = Vec::new();
    let mut ledgers = Vec::new();
    for (index, role) in ["working", "history", "backup"].into_iter().enumerate() {
        let store = LocalStore::open(&child(role)?)?;
        let pin = &f.providers[index];
        let ledger = CapacityLedger::open(&child(&format!("ledger-{role}"))?, &f.ledgers[index])?;
        if Place::local(&store, 1)? != pin.place
            || ledger.identity() != &pin.capacity
            || pin.assignments.len() != 1
            || pin.assignments[0].place != key(role)
            || pin.assignments[0].namespace != *namespaces[0].identity()
            || store.final_policy().is_none()
        {
            return Err("fixture provider identity differs".into());
        }
        stores.push(store);
        ledgers.push(ledger);
    }
    let witness = LocalStore::open(&child("final-witness")?)?;
    let mut witness_ledger = CapacityLedger::open(&child("final-witness-ledger")?, &f.witness)?;
    if Place::local(&witness, 1)? != f.enrollment.witness
        || witness_ledger.identity() != &f.enrollment.capacity
    {
        return Err("fixture witness identity differs".into());
    }
    let jobs = plan
        .actions()
        .map(|(store, _)| {
            f.providers
                .iter()
                .find(|p| &p.place.instance == store)
                .map(|p| p.job.clone())
                .ok_or("unknown child action")
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    struct Clock(Digest);
    impl final_provider::Clock for Clock {
        fn now(&self, p: &Digest) -> final_provider::Result<u64> {
            if p != &self.0 {
                return Err(final_provider::Error::Clock);
            }
            Ok(100)
        }
    }
    let clock = Clock(f.enrollment.clock.clone());
    let mut places = stores
        .iter()
        .zip(&mut ledgers)
        .zip(&f.providers)
        .map(|((store, ledger), p)| placement::Local {
            store,
            ledger,
            job: &p.job,
        })
        .collect::<Vec<_>>();
    final_bulk::Execution {
        enrollment: &f.enrollment,
        roster: &f.roster,
        namespaces: &mut namespaces,
        metadata: &mut metadata,
        places: &mut places,
        action_jobs: &jobs,
        witness: placement::Local {
            store: &witness,
            ledger: &mut witness_ledger,
            job: &f.witness_job,
        },
        clock: &clock,
        admitted_context: &f.enrollment.context,
    }
    .apply_hook(&plan, &plan.authorize()?, |stage| {
        if stage == final_bulk::Stage::Intent {
            std::process::exit(94);
        }
        Ok(())
    })?;
    Err("fixture child did not reach intent exit".into())
}
