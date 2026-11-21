use super::*;
use git_asset_store::placement::Local;

pub(super) fn run(p: &Profile, d: &control::Decision, operation: &str) -> Result<Vec<u8>> {
    let mutation = matches!(operation, "apply" | "resume" | "cancel");
    let cancel = operation == "cancel";
    if mutation {
        p.approve(d, cancel)?;
    }
    // Sorted complete bindings do not depend on which providers happen to open.
    // These exact bindings also let cancellation acknowledge stop while offline.
    let mut jobs = p
        .stores
        .iter()
        .map(|s| control::StoreJob {
            store: s.place.instance.clone(),
            job: s.job.clone(),
        })
        .collect::<Vec<_>>();
    jobs.sort_by(|a, b| a.store.cmp(&b.store));
    let claims = if mutation {
        Some(d.authorize_claim_release().map_err(code)?)
    } else {
        None
    };
    let finals = if mutation {
        Some(d.authorize_final_loss().map_err(code)?)
    } else {
        None
    };
    // This must be the first runtime root opened for cancellation. Neither
    // metadata nor a source/provider/clock failure can prevent the stop record.
    let (witness, mut ledger) = p.witness.open().map_err(|_| "witness_unavailable")?;
    let mut witness = Local {
        store: &witness,
        ledger: &mut ledger,
        job: &p.witness.job,
    };
    let stopped = if cancel {
        let result = control::Cancellation {
            inspection: control::Inspection {
                enrollment: &p.enrollment,
                roster: &p.roster,
                namespaces: &mut [],
                metadata: None,
                places: &mut [],
                jobs: &jobs,
                witness: &mut witness,
                admitted_context: &p.admitted_context,
                limits: &p.limits,
            },
        }
        .cancel(
            d,
            claims.as_ref().ok_or("unapproved_decision")?,
            finals.as_ref().ok_or("unapproved_decision")?,
        );
        match result {
            Ok(report) => Some(report),
            Err(error) => {
                let failure = code(error);
                // A witness housekeeping error can occur after stop publication.
                // Reobserve only that same witness before reporting failure.
                let observed = control::Inspection {
                    enrollment: &p.enrollment,
                    roster: &p.roster,
                    namespaces: &mut [],
                    metadata: None,
                    places: &mut [],
                    jobs: &jobs,
                    witness: &mut witness,
                    admitted_context: &p.admitted_context,
                    limits: &p.limits,
                }
                .status(d);
                if let Ok(mut report) = observed
                    && matches!(
                        report.phase,
                        control::Phase::Cancelled | control::Phase::Completed
                    )
                {
                    report.cleanup_unknown = true;
                    let mut value = serde_json::to_value(report).map_err(|_| "response_budget")?;
                    value["cleanup_error"] = json!(failure);
                    return bounded(value);
                }
                return Err(failure);
            }
        }
    } else {
        None
    };
    let cleanup = || -> Result<Vec<Namespace>> {
        p.namespaces
            .iter()
            .map(|s| Namespace::open(&s.root, &s.minimum).map_err(|_| "source_unavailable"))
            .collect()
    };
    let mut namespaces = if mutation && !cancel {
        cleanup()?
    } else {
        p.namespaces
            .iter()
            .filter_map(|s| Namespace::open(&s.root, &s.minimum).ok())
            .collect()
    };
    let mut opened = Vec::new();
    for (index, s) in p.stores.iter().enumerate() {
        match s.open() {
            Ok((store, ledger)) => opened.push((index, store, ledger)),
            Err("wrong_store") if !cancel => return Err("wrong_store"),
            Err(_) => (),
        }
    }
    let mut places = opened
        .iter_mut()
        .map(|(i, store, ledger)| Local {
            store,
            ledger,
            job: &p.stores[*i].job,
        })
        .collect::<Vec<_>>();
    if matches!(operation, "peak" | "status" | "cancel") {
        let metadata = MetadataReader::open(&p.metadata.root, &p.metadata.minimum).ok();
        let mut inspection = control::Inspection {
            enrollment: &p.enrollment,
            roster: &p.roster,
            namespaces: &mut namespaces,
            metadata,
            places: &mut places,
            jobs: &jobs,
            witness: &mut witness,
            admitted_context: &p.admitted_context,
            limits: &p.limits,
        };
        if operation == "peak" {
            let peak = inspection.required_witness_peak(d).map_err(code)?;
            return bounded(
                json!({"schema":"tig.paged-peak-response.design-1","decision":d.id().map_err(code)?,"witness_peak_bytes":peak,"authorizes_removal":false}),
            );
        }
        let result = if cancel {
            control::Cancellation { inspection }.cancel(
                d,
                claims.as_ref().ok_or("unapproved_decision")?,
                finals.as_ref().ok_or("unapproved_decision")?,
            )
        } else if let Some(registry) = &p.age_registry {
            match Namespace::open(&registry.root, &registry.minimum) {
                Ok(mut ns) => control::aged::status(
                    &mut inspection,
                    d,
                    &mut control::aged::Context {
                        registry: &mut ns,
                        minimum: &registry.minimum,
                        limits: &registry.limits,
                    },
                ),
                Err(_) => inspection.status(d),
            }
        } else {
            inspection.status(d)
        };
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                if let Some(mut stopped) = stopped {
                    stopped.cleanup_unknown = true;
                    // The first acknowledged stop remains a successful outcome.
                    // Counts absent from that report must not become zero.
                    let mut value = serde_json::to_value(stopped).map_err(|_| "response_budget")?;
                    value["cleanup_error"] = json!(code(e));
                    return bounded(value);
                }
                return Err(code(e));
            }
        };
        return bounded(serde_json::to_value(result).map_err(|_| "response_budget")?);
    }
    let clock = p.clock()?;
    let mut metadata = MetadataStore::open(&p.metadata.root, &p.metadata.minimum)
        .map_err(|_| "metadata_unavailable")?;
    let mut execution = control::Execution {
        enrollment: &p.enrollment,
        roster: &p.roster,
        namespaces: &mut namespaces,
        metadata: &mut metadata,
        places: &mut places,
        jobs: &jobs,
        witness,
        clock: &clock,
        admitted_context: &p.admitted_context,
        limits: &p.limits,
        run_limits: p.run_limits.clone(),
    };
    let claims = claims.as_ref().ok_or("unapproved_decision")?;
    let finals = finals.as_ref().ok_or("unapproved_decision")?;
    let result = if p.enrollment.age.is_some() {
        let registry = p.age_registry.as_ref().ok_or("registry_required")?;
        let mut ns = Namespace::open(&registry.root, &registry.minimum)
            .map_err(|_| "registry_unavailable")?;
        execution.apply_aged(
            d,
            claims,
            finals,
            control::aged::Context {
                registry: &mut ns,
                minimum: &registry.minimum,
                limits: &registry.limits,
            },
            |_| Ok(()),
        )
    } else {
        execution.apply(d, claims, finals)
    };
    bounded(serde_json::to_value(result.map_err(code)?).map_err(|_| "response_budget")?)
}
