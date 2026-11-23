//! Independently admitted exact-plan mutation; never invoked by inspection.
use super::*;
use git_asset_store::{
    clock::{Enrollment, LocalClock},
    metadata::MetadataStore,
};

const SCHEMA: &str = "tig.bulk-execution-profile.design-1";
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Permissions {
    #[serde(default)]
    apply: bool,
    #[serde(default)]
    cancel: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Clock {
    root: PathBuf,
    minimum: Checkpoint,
    enrollment: Enrollment,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutionProfile {
    schema: String,
    inspection: Profile,
    admitted_plan: Digest,
    #[serde(default)]
    permissions: Permissions,
    clock: Option<Clock>,
}
impl ExecutionProfile {
    fn load(path: &Path, plan: &bulk::Plan, cancel: bool) -> Result<Self> {
        let mut p: Self = document(path)?;
        p.inspection = p.inspection.validate()?;
        if p.schema != SCHEMA {
            return Err("invalid_profile");
        }
        if !(if cancel {
            p.permissions.cancel
        } else {
            p.permissions.apply
        }) {
            return Err("execution_disabled");
        }
        if p.admitted_plan != plan.id().map_err(code)? {
            return Err("unapproved_plan");
        }
        if p.inspection.enrollment.id().map_err(code)? != plan.enrollment().id().map_err(code)? {
            return Err("wrong_authority");
        }
        p.inspection.copies(&plan.request().copies)?;
        if let Some(clock) = &p.clock {
            if !clock.root.is_absolute()
                || clock.minimum.identity() != &clock.enrollment.journal
                || clock.enrollment.id().map_err(|_| "invalid_clock")?
                    != p.inspection.enrollment.clock
            {
                return Err("invalid_clock");
            }
        } else if !cancel {
            return Err("clock_required");
        }
        Ok(p)
    }
}
pub(super) fn run(args: &[String]) -> Result<Vec<u8>> {
    if args.len() == 1 && args[0] == "describe" {
        return bounded(json!({"schema":"tig.bulk-execution-interface.design-1",
            "profile_schema":SCHEMA,
            "operations":["apply PROFILE PLAN","resume PROFILE PLAN","cancel PROFILE PLAN"],
            "max_document_bytes":65536,"max_plan_bytes":49152,
            "exact_plan_admission":true,"permissions_default_enabled":false,
            "clock":"explicit-enrolled-local-clock","cancel_requires_clock":false,
            "cancel_requires_metadata_writer":false,"initializes_stores":false}));
    }
    if args.len() != 3 || !matches!(args[0].as_str(), "apply" | "resume" | "cancel") {
        return Err("usage");
    }
    let plan = bulk::Plan::from_json(&raw(Path::new(&args[2]))?).map_err(code)?;
    let cancel = args[0] == "cancel";
    // No provider/metadata/clock IO before exact private consent and permission.
    let profile = ExecutionProfile::load(Path::new(&args[1]), &plan, cancel)?;
    let clock = if cancel {
        None
    } else {
        let c = profile.clock.as_ref().ok_or("clock_required")?;
        let ledger = CapacityLedger::open(&c.root, &c.minimum).map_err(|_| "clock_unavailable")?;
        Some(LocalClock::new(ledger, c.enrollment.clone()).map_err(|_| "invalid_clock")?)
    };
    let p = &profile.inspection;
    let mut namespaces = Vec::new();
    for source in &p.namespaces {
        match Namespace::open(&source.root, &source.minimum) {
            Ok(ns) => namespaces.push(ns),
            Err(_) if cancel => (),
            Err(_) => return Err("source_unavailable"),
        }
    }
    let mut stores = Vec::new();
    for (index, store) in p.stores.iter().enumerate() {
        match store.open() {
            Ok((store, ledger)) => stores.push((index, store, ledger)),
            Err("wrong_store") => return Err("wrong_store"),
            Err(_) => (),
        }
    }
    let (witness, mut witness_ledger) = p.witness.open().map_err(|_| "witness_unavailable")?;
    let jobs = plan
        .actions()
        .map(|(id, _)| {
            p.stores
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
            job: &p.stores[*index].job,
        })
        .collect::<Vec<_>>();
    let mut witness_runtime = bulk::Local {
        store: &witness,
        ledger: &mut witness_ledger,
        job: &p.witness.job,
    };
    let auth = plan.authorize().map_err(code)?;
    let result = if cancel {
        bulk::Cancellation {
            inspection: bulk::Inspection {
                enrollment: &p.enrollment,
                roster: &p.roster,
                namespaces: &mut namespaces,
                metadata: p.metadata.minimum.identity().clone(),
                places: &mut places,
                action_jobs: &jobs,
                witness: &mut witness_runtime,
                admitted_context: &p.admitted_context,
            },
        }
        .cancel(&plan, &auth)
    } else {
        let mut metadata = MetadataStore::open(&p.metadata.root, &p.metadata.minimum)
            .map_err(|_| "metadata_unavailable")?;
        bulk::Execution {
            enrollment: &p.enrollment,
            roster: &p.roster,
            namespaces: &mut namespaces,
            metadata: &mut metadata,
            places: &mut places,
            action_jobs: &jobs,
            witness: witness_runtime,
            clock: clock.as_ref().ok_or("clock_required")?,
            admitted_context: &p.admitted_context,
        }
        .apply(&plan, &auth)
    };
    bounded(serde_json::to_value(result.map_err(code)?).map_err(|_| "response_budget")?)
}
