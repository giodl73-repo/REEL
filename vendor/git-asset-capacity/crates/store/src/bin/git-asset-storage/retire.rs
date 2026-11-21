//! Explicit independently admitted local retirement. No bootstrap or discovery.
use super::{Result, document, raw, report};
use git_asset_core::{Digest, tree::MemoryNodes};
use git_asset_store::{
    AckLevel, Key, Request,
    ledger::{CapacityLedger, Checkpoint, JournalRef},
    local::LocalStore,
    metadata::{Checkpoint as MetadataCheckpoint, MetadataReader},
    protection::Limits,
    replica::ReplicaEnrollment,
    retention::{self, RetentionGrant},
    retirement::{self, Plan, ReacquisitionPlan},
    source_claims::{Checkpoint as SourceCheckpoint, admission::Namespace},
    store_protection::Enrollment,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
const SCHEMA: &str = "tig.storage-retirement-profile.design-1";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    owner: Key,
    context: Digest,
    minimum_domains: usize,
    max_object_bytes: u64,
    max_peak_bytes: u64,
    #[serde(default)]
    allow_eviction: bool,
    #[serde(default)]
    allow_reacquisition: bool,
    #[serde(default)]
    allow_cancellation: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Nodes {
    root: PathBuf,
    minimum: MetadataCheckpoint,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    root: PathBuf,
    minimum: SourceCheckpoint,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Assignment {
    namespace: Digest,
    place: Key,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Keeper {
    root: PathBuf,
    ledger_root: PathBuf,
    minimum: Checkpoint,
    enrollment: ReplicaEnrollment,
    grant: RetentionGrant,
    places: Vec<Assignment>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    source_root: PathBuf,
    enrollment: Enrollment,
    ledger_root: PathBuf,
    minimum: Checkpoint,
    job: JournalRef,
    source: ReplicaEnrollment,
    metadata: Nodes,
    namespaces: Vec<Source>,
    backups: Vec<Keeper>,
    limits: Limits,
    policy: Policy,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanInput {
    schema: String,
    operation: Key,
    publication: Request,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReacquireInput {
    schema: String,
    publication: Request,
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    git_asset_core::identity::canonical_json(
        &serde_json::to_value(value).map_err(|_| "invalid_document")?,
    )
    .map_err(|_| "response_budget")
}
fn code(e: retirement::Error) -> &'static str {
    use retirement::Error;
    match e {
        Error::InvalidPlan => "invalid_plan",
        Error::WrongAuthorization => "wrong_authorization",
        Error::MissingCustody => "missing_custody",
        Error::PendingPartial => "private_partial_requires_review",
        Error::Store(git_asset_store::Error::AuthorizedAbsence) => "authorized_absence",
        Error::Store(git_asset_store::Error::ReacquisitionRequired) => "reacquisition_required",
        Error::Store(git_asset_store::Error::NeedsRecovery) => "needs_recovery",
        Error::Store(git_asset_store::Error::Busy) => "busy",
        Error::Ledger(git_asset_store::ledger::LedgerError::Conflict) => "operation_conflict",
        Error::Ledger(git_asset_store::ledger::LedgerError::Budget) => "journal_budget",
        Error::Protection(git_asset_store::store_protection::Error::Required) => {
            "required_by_claim"
        }
        Error::Protection(git_asset_store::store_protection::Error::Busy) => "busy",
        Error::Protection(git_asset_store::store_protection::Error::Retained) => "retained",
        Error::Protection(git_asset_store::store_protection::Error::IncompleteRoster) => {
            "incomplete_scope"
        }
        _ => "operation_failed",
    }
}
impl Profile {
    fn load(path: &Path) -> Result<Self> {
        let p: Self = document(path)?;
        if p.schema != SCHEMA
            || p.namespaces.is_empty()
            || p.namespaces.len() > 16
            || p.backups.is_empty()
            || p.backups.len() > 16
            || p.policy.minimum_domains == 0
            || p.policy.minimum_domains > p.backups.len()
            || p.policy.max_object_bytes == 0
            || p.policy.max_object_bytes > git_asset_store::local::MAX_OBJECT_BYTES
            || p.policy.max_peak_bytes == 0
            || p.policy.max_peak_bytes > 1024 * 1024 * 1024
            || !p.source_root.is_absolute()
            || !p.ledger_root.is_absolute()
            || !p.metadata.root.is_absolute()
            || p.namespaces.iter().any(|n| !n.root.is_absolute())
            || p.backups.iter().any(|b| {
                !b.root.is_absolute()
                    || !b.ledger_root.is_absolute()
                    || b.places.is_empty()
                    || b.places.len() > 256
            })
        {
            return Err("invalid_profile");
        }
        p.limits.validate().map_err(|_| "invalid_profile")?;
        let mut stores = BTreeSet::new();
        for b in &p.backups {
            if b.enrollment.store != *b.grant.store()
                || b.enrollment.store == p.source.store
                || !stores.insert(b.enrollment.store.clone())
            {
                return Err("invalid_profile");
            }
        }
        Ok(p)
    }
    fn open(&self) -> Result<(LocalStore, CapacityLedger)> {
        let source = LocalStore::open(&self.source_root).map_err(|_| "source_unavailable")?;
        self.enrollment.check(&source).map_err(|_| "wrong_store")?;
        if !source.supports_retirement() || source.identity() != &self.source.store {
            return Err("wrong_store");
        }
        let ledger = CapacityLedger::open(&self.ledger_root, &self.minimum)
            .map_err(|_| "ledger_unavailable")?;
        Ok((source, ledger))
    }
    fn publication(&self, r: &Request) -> Result<()> {
        if r.owner != self.policy.owner
            || r.required != AckLevel::ProcessRestart
            || r.object.bytes > self.policy.max_object_bytes
        {
            return Err("policy_denied");
        }
        Ok(())
    }
    fn request(&self, input: PlanInput, ledger: &CapacityLedger) -> Result<retirement::Request> {
        if input.schema != "tig.retirement-request.design-1" {
            return Err("invalid_document");
        }
        self.publication(&input.publication)?;
        Ok(retirement::Request {
            operation: input.operation,
            owner: self.policy.owner.clone(),
            publication: input.publication,
            context: self.policy.context.clone(),
            source: self.source.clone(),
            keepers: self
                .backups
                .iter()
                .map(|b| retirement::Keeper {
                    grant: b.grant.clone(),
                    enrollment: b.enrollment.clone(),
                })
                .collect(),
            required_domains: self.policy.minimum_domains,
            capacity: ledger.identity().clone(),
        })
    }
    fn admit(&self, p: &Plan, ledger: &CapacityLedger, current: bool) -> Result<()> {
        let r = p.request();
        self.publication(&r.publication)?;
        if p.enrollment() != &self.enrollment
            || r.capacity != *ledger.identity()
            || r.owner != self.policy.owner
        {
            return Err("wrong_store");
        }
        if !current {
            return Ok(());
        }
        if r.context != self.policy.context
            || r.source != self.source
            || r.required_domains < self.policy.minimum_domains
            || r.keepers.len() != self.backups.len()
            || p.required_peak() > self.policy.max_peak_bytes
        {
            return Err("policy_denied");
        }
        for keeper in &r.keepers {
            let b = self
                .backups
                .iter()
                .find(|b| b.enrollment.store == keeper.enrollment.store)
                .ok_or("policy_denied")?;
            if b.enrollment != keeper.enrollment || encode(&b.grant)? != encode(&keeper.grant)? {
                return Err("policy_denied");
            }
        }
        Ok(())
    }
    fn namespaces(&self, store: &LocalStore) -> Result<Vec<Namespace>> {
        let mut namespaces = Vec::new();
        for n in &self.namespaces {
            namespaces
                .push(Namespace::open(&n.root, &n.minimum).map_err(|_| "namespace_unavailable")?);
        }
        let members = store.protection().ok_or("wrong_store")?.members();
        if namespaces.len() != members.len() {
            return Err("incomplete_scope");
        }
        let mut expected = BTreeSet::new();
        let mut assigned = BTreeSet::new();
        for n in &namespaces {
            for p in n.policy().classes.values() {
                for place in p.current.iter().chain(&p.pending).chain(&p.predecessor) {
                    expected.insert((n.identity().clone(), place.as_str().to_owned()));
                }
            }
        }
        let unique = namespaces
            .iter()
            .map(|n| n.identity().clone())
            .collect::<BTreeSet<_>>();
        if unique.len() != namespaces.len()
            || members.iter().any(|m| !unique.contains(&m.namespace))
        {
            return Err("incomplete_scope");
        }
        for m in members {
            for place in &m.places {
                assigned.insert((m.namespace.clone(), place.as_str().to_owned()));
            }
        }
        for b in &self.backups {
            for a in &b.places {
                assigned.insert((a.namespace.clone(), a.place.as_str().to_owned()));
            }
        }
        if expected != assigned {
            return Err("unmapped_place");
        }
        Ok(namespaces)
    }
    fn backups(&self) -> Result<Vec<(LocalStore, CapacityLedger)>> {
        self.backups
            .iter()
            .map(|b| {
                let store = LocalStore::open(&b.root).map_err(|_| "keeper_unavailable")?;
                if store.identity() != &b.enrollment.store {
                    return Err("wrong_keeper");
                }
                let ledger = CapacityLedger::open(&b.ledger_root, &b.minimum)
                    .map_err(|_| "keeper_unavailable")?;
                Ok((store, ledger))
            })
            .collect()
    }
}
pub(super) fn run(args: &[String]) -> Result<Vec<u8>> {
    let Some(action) = args.first().map(String::as_str) else {
        return Err("usage");
    };
    if action == "describe" && args.len() == 1 {
        return report(
            json!({"schema":"tig.retirement-interface.design-1","profile_schema":SCHEMA,
        "operations":["plan PROFILE REQUEST","status PROFILE PLAN","apply PROFILE PLAN","resume PROFILE PLAN","cancel PROFILE PLAN","reacquire-plan PROFILE RETIRED_PLAN REQUEST","reacquire PROFILE PLAN","reacquire-status PROFILE PLAN"],
        "max_document_bytes":65536,"max_namespaces":16,"max_keepers":16,"default_apply":false,"final_deletion":false,"initializes_stores":false}),
        );
    }
    if !matches!(
        action,
        "plan"
            | "status"
            | "apply"
            | "resume"
            | "cancel"
            | "reacquire-plan"
            | "reacquire"
            | "reacquire-status"
    ) || args.len() != if action == "reacquire-plan" { 4 } else { 3 }
    {
        return Err("usage");
    }
    let profile = Profile::load(Path::new(&args[1]))?;
    // Parse portable documents before provider IO. No untrusted field is a root.
    let plan = if matches!(
        action,
        "status" | "apply" | "resume" | "cancel" | "reacquire-plan"
    ) {
        Some(Plan::from_json(&raw(Path::new(&args[2]))?).map_err(code)?)
    } else {
        None
    };
    let reacquire = if matches!(action, "reacquire" | "reacquire-status") {
        Some(ReacquisitionPlan::from_json(&raw(Path::new(&args[2]))?).map_err(code)?)
    } else {
        None
    };
    let request = if action == "plan" {
        Some(document::<PlanInput>(Path::new(&args[2]))?)
    } else {
        None
    };
    let new_request = if action == "reacquire-plan" {
        Some(document::<ReacquireInput>(Path::new(&args[3]))?)
    } else {
        None
    };
    let (source, mut ledger) = profile.open()?;
    if let Some(input) = request {
        profile.namespaces(&source)?;
        let p = retirement::plan(&source, profile.request(input, &ledger)?).map_err(code)?;
        profile.admit(&p, &ledger, true)?;
        return p.to_json().map_err(code);
    }
    if let Some(p) = reacquire {
        profile.publication(p.publication())?;
        if p.enrollment() != &profile.enrollment || p.capacity() != ledger.identity() {
            return Err("wrong_store");
        }
        if action == "reacquire-status" {
            return encode(
                &retirement::status::inspect_reacquisition(&source, &mut ledger, &profile.job, &p)
                    .map_err(code)?,
            );
        }
        if !profile.policy.allow_reacquisition
            || p.context() != &profile.policy.context
            || p.required_peak().map_err(code)? > profile.policy.max_peak_bytes
        {
            return Err("policy_denied");
        }
        let b = &profile.backups[0];
        if b.grant.request().publication.object != p.publication().object {
            return Err("policy_denied");
        }
        let keeper = LocalStore::open(&b.root).map_err(|_| "keeper_unavailable")?;
        if keeper.identity() != b.grant.store() {
            return Err("wrong_keeper");
        }
        let mut capacity =
            CapacityLedger::open(&b.ledger_root, &b.minimum).map_err(|_| "keeper_unavailable")?;
        let _retained =
            retention::verify(&mut capacity, &keeper, &b.grant).map_err(|_| "missing_custody")?;
        let mut bytes = keeper
            .read_object(&p.publication().object)
            .map_err(|_| "keeper_unavailable")?;
        let receipt = retirement::Reacquisition {
            store: &source,
            ledger: &mut ledger,
            job: &profile.job,
        }
        .apply(&p, &p.authorize().map_err(code)?, &mut bytes)
        .map_err(code)?;
        return report(
            json!({"schema":"tig.reacquisition-result.design-1","plan":p.id().map_err(code)?,"publication":receipt.publication().request(),"phase":"published","charged_file_data_bytes":receipt.charged_footprint(),"removal_authority":false}),
        );
    }
    let p = plan.ok_or("invalid_plan")?;
    profile.admit(
        &p,
        &ledger,
        !matches!(action, "status" | "cancel" | "reacquire-plan"),
    )?;
    if action == "status" {
        return encode(
            &retirement::status::inspect(&source, &mut ledger, &profile.job, &p).map_err(code)?,
        );
    }
    if let Some(input) = new_request {
        if input.schema != "tig.reacquisition-request.design-1" {
            return Err("invalid_document");
        }
        profile.publication(&input.publication)?;
        let p = retirement::plan_reacquisition(
            &source,
            &p,
            input.publication,
            profile.policy.context.clone(),
        )
        .map_err(code)?;
        if p.required_peak().map_err(code)? > profile.policy.max_peak_bytes {
            return Err("policy_denied");
        }
        return p.to_json().map_err(code);
    }
    if action == "cancel" {
        if !profile.policy.allow_cancellation {
            return Err("policy_denied");
        }
        let mut namespaces = [];
        let mut backups = [];
        let receipt = retirement::Execution {
            store: &source,
            ledger: &mut ledger,
            job: &profile.job,
            namespaces: &mut namespaces,
            nodes: &MemoryNodes::default(),
            limits: &profile.limits,
            backups: &mut backups,
            admitted_context: &profile.policy.context,
            source_enrollment: &profile.source,
        }
        .cancel(&p, &p.authorize().map_err(code)?)
        .map_err(code)?;
        return encode(&receipt);
    }
    if !profile.policy.allow_eviction {
        return Err("policy_denied");
    }
    let mut namespaces = profile.namespaces(&source)?;
    let nodes = MetadataReader::open(&profile.metadata.root, &profile.metadata.minimum)
        .map_err(|_| "metadata_unavailable")?;
    let mut stores = profile.backups()?;
    let mut backups = stores
        .iter_mut()
        .zip(&profile.backups)
        .map(|((store, ledger), b)| retirement::Backup {
            store,
            ledger,
            enrollment: &b.enrollment,
        })
        .collect::<Vec<_>>();
    let receipt = retirement::Execution {
        store: &source,
        ledger: &mut ledger,
        job: &profile.job,
        namespaces: &mut namespaces,
        nodes: &nodes,
        limits: &profile.limits,
        backups: &mut backups,
        admitted_context: &profile.policy.context,
        source_enrollment: &profile.source,
    }
    .apply(&p, &p.authorize().map_err(code)?)
    .map_err(code)?;
    encode(&receipt)
}
