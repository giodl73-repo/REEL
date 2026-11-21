//! Existing-root bulk inspection with a separately admitted execution interface.
use super::{Result, document, raw, report};
use git_asset_core::Digest;
use git_asset_store::{
    final_retirement::{bulk, claims},
    ledger::{CapacityLedger, Checkpoint, JournalRef},
    local::LocalStore,
    metadata::{Checkpoint as MetadataCheckpoint, MetadataReader},
    orchestration::Place,
    source_claims::{Checkpoint as SourceCheckpoint, admission::Namespace},
};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

const SCHEMA: &str = "tig.bulk-inspection-profile.design-1";
#[path = "bulk/execute.rs"]
mod execute;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Nodes {
    pub(super) root: PathBuf,
    pub(super) minimum: MetadataCheckpoint,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Source {
    pub(super) root: PathBuf,
    pub(super) minimum: SourceCheckpoint,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Store {
    pub(super) root: PathBuf,
    pub(super) ledger_root: PathBuf,
    pub(super) minimum: Checkpoint,
    pub(super) job: JournalRef,
    pub(super) place: Place,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    enrollment: bulk::Enrollment,
    roster: claims::Roster,
    admitted_context: Digest,
    metadata: Nodes,
    namespaces: Vec<Source>,
    stores: Vec<Store>,
    witness: Store,
}
fn code(e: bulk::Error) -> &'static str {
    match e {
        bulk::Error::Invalid => "invalid_plan",
        bulk::Error::Authority => "wrong_authority",
        bulk::Error::Stale => "stale_basis",
        bulk::Error::Budget => "budget",
        bulk::Error::Recovery => "needs_recovery",
        bulk::Error::Source(_) | bulk::Error::Admission(_) | bulk::Error::Claims(_) => {
            "source_held"
        }
        bulk::Error::Provider(_) => "provider_held",
        bulk::Error::Ledger(_) | bulk::Error::Publication(_) => "witness_held",
        _ => "operation_failed",
    }
}
impl Store {
    pub(super) fn open(&self) -> Result<(LocalStore, CapacityLedger)> {
        let store = LocalStore::open(&self.root).map_err(|_| "provider_unavailable")?;
        if Place::local(&store, self.place.generation).map_err(|_| "wrong_store")? != self.place {
            return Err("wrong_store");
        }
        let ledger = CapacityLedger::open(&self.ledger_root, &self.minimum)
            .map_err(|_| "ledger_unavailable")?;
        Ok((store, ledger))
    }
}
impl Profile {
    fn load(path: &Path) -> Result<Self> {
        let p: Self = document(path)?;
        p.validate()
    }
    fn validate(self) -> Result<Self> {
        let p = self;
        p.enrollment.id().map_err(code)?;
        p.roster.id().map_err(|_| "invalid_roster")?;
        if p.schema != SCHEMA
            || p.admitted_context != p.enrollment.context
            || p.roster.scope().map_err(|_| "invalid_roster")? != p.enrollment.scope
            || p.witness.place != p.enrollment.witness
            || p.witness.minimum.identity() != &p.enrollment.capacity
            || p.stores.len() > 4
            || p.namespaces.len() != p.roster.members.len()
            || !p.metadata.root.is_absolute()
        {
            return Err("invalid_profile");
        }
        let mut ids = BTreeSet::new();
        for s in p.stores.iter().chain(std::iter::once(&p.witness)) {
            if !s.root.is_absolute()
                || !s.ledger_root.is_absolute()
                || s.place.generation == 0
                || !ids.insert(s.place.instance.clone())
            {
                return Err("invalid_profile");
            }
        }
        let mut seen = BTreeSet::new();
        for n in &p.namespaces {
            if !n.root.is_absolute()
                || !seen.insert(n.minimum.identity().clone())
                || !p
                    .roster
                    .members
                    .iter()
                    .any(|m| &m.namespace == n.minimum.identity() && m.minimum == n.minimum)
            {
                return Err("invalid_profile");
            }
        }
        Ok(p)
    }
    fn copies(&self, copies: &[bulk::Copy]) -> Result<()> {
        for c in copies {
            let s = self
                .stores
                .iter()
                .find(|s| s.place.instance == c.store)
                .ok_or("unknown_store")?;
            if s.minimum.identity() != &c.capacity {
                return Err("wrong_capacity");
            }
        }
        Ok(())
    }
}
fn bounded(value: serde_json::Value) -> Result<Vec<u8>> {
    let bytes = report(value)?;
    if bytes.len() > 65_536 {
        return Err("response_budget");
    }
    Ok(bytes)
}
pub(super) fn run(args: &[String]) -> Result<Vec<u8>> {
    if args.first().is_some_and(|s| s == "execute") {
        return execute::run(&args[1..]);
    }
    if args.len() == 1 && args[0] == "describe" {
        return bounded(json!({"schema":"tig.bulk-inspection-interface.design-1",
            "profile_schema":SCHEMA,"operations":["plan PROFILE REQUEST","peak PROFILE PLAN","status PROFILE PLAN"],
            "max_document_bytes":65536,"max_plan_bytes":49152,"max_roots":bulk::MAX_ROOTS,
            "max_paths":bulk::MAX_PATHS,"max_objects":bulk::MAX_OBJECTS,
            "inspect_only":true,"final_deletion":false,"removal_authority":false,"initializes_stores":false,
            "additional_interfaces":{"execution":"git-asset-storage bulk execute describe"}}));
    }
    if args.len() != 3 || !matches!(args[0].as_str(), "plan" | "peak" | "status") {
        return Err("usage");
    }
    let input = raw(Path::new(&args[2]))?;
    // Portable decoding precedes profile/provider IO.
    let plan = if args[0] == "plan" {
        None
    } else {
        Some(bulk::Plan::from_json(&input).map_err(code)?)
    };
    let request: Option<bulk::Request> = if plan.is_none() {
        Some(
            serde_json::from_value(
                git_asset_core::identity::parse_json(&input).map_err(|_| "invalid_document")?,
            )
            .map_err(|_| "invalid_document")?,
        )
    } else {
        None
    };
    let profile = Profile::load(Path::new(&args[1]))?;
    if let Some(request) = &request {
        profile.copies(&request.copies)?;
    }
    if let Some(plan) = &plan {
        profile.copies(&plan.request().copies)?;
    }
    let mut namespaces = Vec::new();
    for source in &profile.namespaces {
        match Namespace::open(&source.root, &source.minimum) {
            Ok(ns) => namespaces.push(ns),
            Err(_) if args[0] != "plan" => {}
            Err(_) => return Err("source_unavailable"),
        }
    }
    let mut stores = Vec::new();
    for (index, store) in profile.stores.iter().enumerate() {
        match store.open() {
            Ok((store, ledger)) => stores.push((index, store, ledger)),
            Err("wrong_store") => return Err("wrong_store"),
            Err(_) => {}
        }
    }
    if let Some(request) = request {
        let nodes = MetadataReader::open(&profile.metadata.root, &profile.metadata.minimum)
            .map_err(|_| "metadata_unavailable")?;
        let stores = stores.iter().map(|(_, s, _)| s).collect::<Vec<_>>();
        return bulk::plan(
            &profile.enrollment,
            request,
            &profile.roster,
            &mut namespaces,
            &nodes,
            &stores,
        )
        .map_err(code)?
        .to_json()
        .map_err(code);
    }
    let plan = plan.ok_or("invalid_plan")?;
    let (witness, mut witness_ledger) =
        profile.witness.open().map_err(|_| "witness_unavailable")?;
    let action_jobs = plan
        .actions()
        .map(|(id, _)| {
            profile
                .stores
                .iter()
                .find(|s| &s.place.instance == id)
                .map(|s| s.job.clone())
                .ok_or("unknown_store")
        })
        .collect::<Result<Vec<_>>>()?;
    let mut places = stores
        .iter_mut()
        .map(|(index, store, ledger)| bulk::Local {
            store,
            ledger,
            job: &profile.stores[*index].job,
        })
        .collect::<Vec<_>>();
    let mut witness_runtime = bulk::Local {
        store: &witness,
        ledger: &mut witness_ledger,
        job: &profile.witness.job,
    };
    let mut inspect = bulk::Inspection {
        enrollment: &profile.enrollment,
        roster: &profile.roster,
        namespaces: &mut namespaces,
        metadata: profile.metadata.minimum.identity().clone(),
        places: &mut places,
        action_jobs: &action_jobs,
        witness: &mut witness_runtime,
        admitted_context: &profile.admitted_context,
    };
    if args[0] == "peak" {
        return bounded(
            json!({"schema":"tig.bulk-inspection-peak.design-1","plan":plan.id().map_err(code)?,
            "required_witness_metadata_peak":inspect.required_metadata_peak(&plan).map_err(code)?,"removal_authority":false}),
        );
    }
    bounded(
        serde_json::to_value(inspect.status(&plan).map_err(code)?)
            .map_err(|_| "response_budget")?,
    )
}
