//! Fresh approval for copy-only work after a sealed original paged operation.
use super::*;
use git_asset_store::{
    final_retirement::paged::control::followup as fresh, ledger::JournalRef, placement::Local,
};
const PROFILE: &str = "tig.held-copy-followup-profile.design-1";
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Permissions {
    stage: bool,
    apply: bool,
    cancel: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Admission {
    schema: String,
    operator: Profile,
    original_witness_job: JournalRef,
    original_jobs: Vec<control::StoreJob>,
    final_loss: Option<Digest>,
    #[serde(default)]
    permissions: Permissions,
    #[serde(default)]
    run_limits: fresh::RunLimits,
}
impl Admission {
    fn load(path: &Path) -> Result<Self> {
        let mut p: Self = document(path)?;
        p.operator = p.operator.validate()?;
        if p.schema != PROFILE
            || p.original_jobs.len() > 4
            || p.original_jobs.windows(2).any(|j| j[0].store >= j[1].store)
            || p.run_limits.actions == 0
            || p.run_limits.actions > 32
        {
            return Err("invalid_profile");
        }
        Ok(p)
    }
    fn origin(&self, old: control::Decision) -> fresh::Origin {
        fresh::Origin {
            decision: old,
            witness_job: self.original_witness_job.clone(),
            jobs: self.original_jobs.clone(),
        }
    }
    fn check_origin(&self, origin: &fresh::Origin) -> Result<()> {
        if origin.jobs != self.original_jobs
            || origin.witness_job != self.original_witness_job
            || origin.decision.enrollment_id() != &self.operator.enrollment.id().map_err(code)?
        {
            return Err("wrong_origin");
        }
        Ok(())
    }
    fn approve(&self, d: &fresh::Decision, cancel: bool) -> Result<()> {
        if !(if cancel {
            self.permissions.cancel
        } else {
            self.permissions.apply
        }) {
            return Err("execution_disabled");
        }
        if self.final_loss.as_ref() != Some(&d.id().map_err(code)?) {
            return Err("unapproved_decision");
        }
        self.check_origin(&d.request().origin)?;
        if d.enrollment_id() != &self.operator.enrollment.id().map_err(code)? {
            return Err("wrong_authority");
        }
        if !cancel {
            self.operator.clock_config()?;
            if self.operator.enrollment.age.is_some() && self.operator.age_registry.is_none() {
                return Err("registry_required");
            }
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    operation: Key,
    original: control::Decision,
    copies: Object,
    window: git_asset_store::final_retirement::provider::Window,
}
fn plan(p: &Admission, r: Request) -> Result<Vec<u8>> {
    if !p.permissions.stage {
        return Err("staging_disabled");
    }
    if r.schema != "tig.held-copy-followup-request.design-1" {
        return Err("invalid_document");
    }
    let o = &p.operator;
    let origin = p.origin(r.original);
    p.check_origin(&origin)?;
    let clock = o.clock()?;
    let (store, mut ledger) = o.witness.open().map_err(|_| "witness_unavailable")?;
    let mut witness = Local {
        store: &store,
        ledger: &mut ledger,
        job: &p.original_witness_job,
    };
    let mut metadata = MetadataStore::open(&o.metadata.root, &o.metadata.minimum)
        .map_err(|_| "metadata_unavailable")?;
    let session = paged::Session::new(&o.limits).map_err(selection_code)?;
    let copies = control::read_copies(
        &r.copies,
        &mut session.nodes(&mut metadata),
        session.limits(),
    )
    .map_err(code)?;
    o.copies(&copies)?;
    let mut namespaces = o
        .namespaces
        .iter()
        .map(|s| Namespace::open(&s.root, &s.minimum).map_err(|_| "source_unavailable"))
        .collect::<Result<Vec<_>>>()?;
    let mut stores = Vec::new();
    for s in &o.stores {
        if let Ok(store) = git_asset_store::local::LocalStore::open(&s.root) {
            if git_asset_store::orchestration::Place::local(&store, s.place.generation)
                .map_err(|_| "wrong_store")?
                != s.place
            {
                return Err("wrong_store");
            }
            stores.push(store);
        }
    }
    let stores = stores.iter().collect::<Vec<_>>();
    let registry = o
        .age_registry
        .as_ref()
        .map(|r| Namespace::open(&r.root, &r.minimum).map_err(|_| "registry_unavailable"))
        .transpose()?;
    let mut registry = registry;
    let age = registry
        .as_mut()
        .zip(o.age_registry.as_ref())
        .map(|(ns, r)| control::aged::Context {
            registry: ns,
            minimum: &r.minimum,
            limits: &r.limits,
        });
    let decision = fresh::Planning {
        enrollment: &o.enrollment,
        roster: &o.roster,
        namespaces: &mut namespaces,
        nodes: &mut session.nodes(&mut metadata),
        stores: &stores,
        origin_witness: &mut witness,
        clock: &clock,
        age,
        limits: session.limits(),
    }
    .plan(fresh::Request {
        operation: r.operation,
        origin,
        copies: r.copies,
        window: r.window,
    })
    .map_err(code)?;
    let minimum = metadata.finish().map_err(|_| "metadata_finish")?;
    drop(metadata);
    let mut reader =
        MetadataReader::open(&o.metadata.root, &minimum).map_err(|_| "metadata_unavailable")?;
    fresh::inspect(
        &o.enrollment,
        &decision,
        &mut session.nodes(&mut reader),
        session.limits(),
    )
    .map_err(code)?;
    bounded(
        json!({"schema":"tig.held-copy-followup-plan-response.design-1","decision":decision,"decision_id":decision.id().map_err(code)?,"minimum":minimum,"authorizes_removal":false}),
    )
}
fn scope(
    p: &Admission,
    d: &fresh::Decision,
    kind: &str,
    cursor: Option<Cursor>,
) -> Result<Vec<u8>> {
    p.check_origin(&d.request().origin)?;
    let o = &p.operator;
    let id = d.id().map_err(code)?;
    let start = if let Some(c) = cursor {
        if c.schema != "tig.paged-scope-cursor.design-1" || c.decision != id || c.kind != kind {
            return Err("invalid_cursor");
        }
        c.start
    } else {
        0
    };
    let session = paged::Session::new(&o.limits).map_err(selection_code)?;
    let mut reader = MetadataReader::open(&o.metadata.root, &o.metadata.minimum)
        .map_err(|_| "metadata_unavailable")?;
    let view = fresh::inspect(
        &o.enrollment,
        d,
        &mut session.nodes(&mut reader),
        session.limits(),
    )
    .map_err(code)?;
    if kind == "inspect" {
        return bounded(
            json!({"schema":"tig.held-copy-followup-inspection.design-1","decision_id":id,"decision":d,
        "subjects":view.subjects().len(),"actions":view.actions().len(),"historical_only":true,"authorizes_removal":false}),
        );
    }
    let rows = if kind == "subjects" {
        view.subjects()
            .map(serde_json::to_value)
            .collect::<std::result::Result<Vec<_>, _>>()
    } else {
        view.actions()
            .map(|(store, action)| Ok(json!({"store":store,"plan":action})))
            .collect()
    }
    .map_err(|_| "response_budget")?;
    scope_page(id, kind, rows, start)
}
fn cancelled(mut report: fresh::Report, error: &str) -> Result<Vec<u8>> {
    report.cleanup_unknown = true;
    let mut value = serde_json::to_value(report).map_err(|_| "response_budget")?;
    value["cleanup_error"] = json!(error);
    bounded(value)
}
fn execute(p: &Admission, d: &fresh::Decision, op: &str) -> Result<Vec<u8>> {
    let mutation = matches!(op, "apply" | "resume" | "cancel");
    let cancel = op == "cancel";
    if mutation {
        p.approve(d, cancel)?;
    } else {
        p.check_origin(&d.request().origin)?;
    }
    let o = &p.operator;
    let mut jobs = o
        .stores
        .iter()
        .map(|s| control::StoreJob {
            store: s.place.instance.clone(),
            job: s.job.clone(),
        })
        .collect::<Vec<_>>();
    jobs.sort_by(|a, b| a.store.cmp(&b.store));
    let auth = if mutation {
        Some(d.authorize_final_loss().map_err(code)?)
    } else {
        None
    };
    let (store, mut ledger) = o.witness.open().map_err(|_| "witness_unavailable")?;
    let mut witness = Local {
        store: &store,
        ledger: &mut ledger,
        job: &o.witness.job,
    };
    let stopped = if cancel {
        let result = fresh::Cancellation {
            inspection: fresh::Inspection {
                enrollment: &o.enrollment,
                roster: &o.roster,
                metadata: None,
                places: &mut [],
                jobs: &jobs,
                witness: &mut witness,
                admitted_context: &o.admitted_context,
                limits: &o.limits,
            },
        }
        .cancel(d, auth.as_ref().ok_or("unapproved_decision")?);
        match result {
            Ok(report) => Some(report),
            Err(error) => {
                let failure = code(error);
                let observed = fresh::Inspection {
                    enrollment: &o.enrollment,
                    roster: &o.roster,
                    metadata: None,
                    places: &mut [],
                    jobs: &jobs,
                    witness: &mut witness,
                    admitted_context: &o.admitted_context,
                    limits: &o.limits,
                }
                .status(d);
                if let Ok(report) = observed
                    && matches!(
                        report.phase,
                        control::Phase::Cancelled | control::Phase::Completed
                    )
                {
                    return cancelled(report, failure);
                }
                return Err(failure);
            }
        }
    } else {
        None
    };
    let mut opened = Vec::new();
    for (i, s) in o.stores.iter().enumerate() {
        match s.open() {
            Ok((store, ledger)) => opened.push((i, store, ledger)),
            Err("wrong_store") if !cancel => return Err("wrong_store"),
            Err(_) => (),
        }
    }
    let mut places = opened
        .iter_mut()
        .map(|(i, store, ledger)| Local {
            store,
            ledger,
            job: &o.stores[*i].job,
        })
        .collect::<Vec<_>>();
    let metadata = MetadataReader::open(&o.metadata.root, &o.metadata.minimum).ok();
    if matches!(op, "status" | "peak" | "cancel") {
        let mut inspection = fresh::Inspection {
            enrollment: &o.enrollment,
            roster: &o.roster,
            metadata,
            places: &mut places,
            jobs: &jobs,
            witness: &mut witness,
            admitted_context: &o.admitted_context,
            limits: &o.limits,
        };
        if op == "peak" {
            return bounded(
                json!({"schema":"tig.held-copy-followup-peak.design-1","decision":d.id().map_err(code)?,"witness_peak_bytes":inspection.required_witness_peak(d).map_err(code)?,"authorizes_removal":false}),
            );
        }
        let result = if cancel {
            fresh::Cancellation { inspection }
                .cancel(d, auth.as_ref().ok_or("unapproved_decision")?)
        } else {
            inspection.status(d)
        };
        return match result {
            Ok(report) => bounded(serde_json::to_value(report).map_err(|_| "response_budget")?),
            Err(error) => {
                if let Some(stopped) = stopped {
                    cancelled(stopped, code(error))
                } else {
                    Err(code(error))
                }
            }
        };
    }
    let mut metadata = metadata.ok_or("metadata_unavailable")?;
    let clock = o.clock()?;
    let mut namespaces = o
        .namespaces
        .iter()
        .map(|s| Namespace::open(&s.root, &s.minimum).map_err(|_| "source_unavailable"))
        .collect::<Result<Vec<_>>>()?;
    let mut runtime = fresh::Execution {
        enrollment: &o.enrollment,
        roster: &o.roster,
        namespaces: &mut namespaces,
        metadata: &mut metadata,
        places: &mut places,
        jobs: &jobs,
        witness,
        clock: &clock,
        admitted_context: &o.admitted_context,
        limits: &o.limits,
        run_limits: p.run_limits.clone(),
    };
    let auth = auth.as_ref().ok_or("unapproved_decision")?;
    let result = if o.enrollment.age.is_some() {
        let r = o.age_registry.as_ref().ok_or("registry_required")?;
        let mut ns = Namespace::open(&r.root, &r.minimum).map_err(|_| "registry_unavailable")?;
        runtime.apply_aged(
            d,
            auth,
            control::aged::Context {
                registry: &mut ns,
                minimum: &r.minimum,
                limits: &r.limits,
            },
        )
    } else {
        runtime.apply(d, auth)
    };
    bounded(serde_json::to_value(result.map_err(code)?).map_err(|_| "response_budget")?)
}
pub(super) fn run(args: &[String]) -> Result<Vec<u8>> {
    if args.len() == 1 && args[0] == "describe" {
        return bounded(
            json!({"schema":"tig.held-copy-followup-interface.design-1","profile_schema":PROFILE,
        "operations":["plan PROFILE REQUEST","inspect PROFILE DECISION","subjects PROFILE DECISION [CURSOR]","actions PROFILE DECISION [CURSOR]","peak PROFILE DECISION","status PROFILE DECISION","apply PROFILE DECISION","resume PROFILE DECISION","cancel PROFILE DECISION"],
        "permissions_default_enabled":false,"fresh_final_loss_approval":true,"source_mutation":false,"metadata_writer_on_apply":false,"cancel_requires_clock":false,"max_document_bytes":65536,"initializes_stores":false}),
        );
    }
    if !(3..=4).contains(&args.len())
        || !matches!(
            args[0].as_str(),
            "plan"
                | "inspect"
                | "subjects"
                | "actions"
                | "peak"
                | "status"
                | "apply"
                | "resume"
                | "cancel"
        )
        || (args.len() == 4 && !matches!(args[0].as_str(), "subjects" | "actions"))
    {
        return Err("usage");
    }
    let input = raw(Path::new(&args[2]))?;
    let d = if args[0] == "plan" {
        None
    } else {
        Some(fresh::Decision::from_json(&input).map_err(code)?)
    };
    let cursor = if args.len() == 4 {
        Some(document(Path::new(&args[3]))?)
    } else {
        None
    };
    let p = Admission::load(Path::new(&args[1]))?;
    match args[0].as_str() {
        "plan" => plan(
            &p,
            serde_json::from_value(
                git_asset_core::identity::parse_json(&input).map_err(|_| "invalid_document")?,
            )
            .map_err(|_| "invalid_document")?,
        ),
        "inspect" | "subjects" | "actions" => {
            scope(&p, d.as_ref().ok_or("invalid_decision")?, &args[0], cursor)
        }
        _ => execute(&p, d.as_ref().ok_or("invalid_decision")?, &args[0]),
    }
}
