use super::profile::{self, Parsed};
use serde_json::json;
use tig_core::{Digest, identity::canonical_json};
use tig_store::{
    AckLevel,
    acquire::SourceError,
    ledger::CapacityLedger,
    local::{LocalStore, removal},
    metadata::MetadataStore,
    provider::{custody, profile as provider_profile, removal as port},
    retirement_intent::{
        self as intent,
        progress::{self, Action, Outcome},
    },
    source_claims::{SourceJournal, protection::Index},
};
struct Endpoint {
    binding: profile::Survivor,
    maximum: u64,
    opened: Option<(LocalStore, CapacityLedger)>,
    accepted: Option<(Digest, provider_profile::Profile)>,
    profile_checked: bool,
}
impl custody::Retainer for Endpoint {
    fn describe(&self) -> custody::Capabilities {
        if let Some((_, p)) = &self.accepted
            && let provider_profile::Qualification::Qualified { capabilities, .. } = &p.custody
        {
            return capabilities.clone();
        }
        custody::Capabilities {
            place: self.binding.destination.place.clone(),
            max_object_bytes: self.maximum,
            max_ack: AckLevel::ProcessRestart,
            enforcement: Some(custody::Enforcement::Cooperative),
        }
    }
    fn acquire(
        &mut self,
        wanted: &custody::Commitment,
    ) -> Result<Box<dyn custody::Held + '_>, SourceError> {
        if custody::native::commitment(&self.binding.grant) != *wanted
            || self.binding.destination.place != wanted.place
        {
            return Err(SourceError::Stale);
        }
        if let Some((id, p)) = &self.accepted {
            p.admit(id, &p.binding)?.check_custody(&self.describe())?;
        }
        if self.opened.is_none() {
            let store = LocalStore::open(&self.binding.destination.root)
                .map_err(|_| SourceError::Unavailable)?;
            if store.identity() != &wanted.place {
                return Err(SourceError::Stale);
            }
            let ledger = CapacityLedger::open(
                &self.binding.capacity.root,
                &self.binding.capacity.minimum_checkpoint,
            )
            .map_err(|_| SourceError::Unavailable)?;
            if ledger.identity() != &self.binding.capacity.ledger {
                return Err(SourceError::Stale);
            }
            self.opened = Some((store, ledger));
        }
        let (store, ledger) = self.opened.as_mut().ok_or(SourceError::Unknown)?;
        if let Some((id, p)) = &self.accepted {
            let guard = p.admit(id, &p.binding)?;
            let caps = custody::native::capabilities(store);
            guard.check_custody(&caps)?;
            let held = custody::native::hold(store, ledger, &self.binding.grant, wanted)?;
            let held = guard.bind_custody(&caps, wanted, true, held)?;
            self.profile_checked = true;
            Ok(held)
        } else {
            custody::native::hold(store, ledger, &self.binding.grant, wanted)
        }
    }
}
fn guarded_remove<R: port::ExactRemover>(
    mut executor: R,
    request: &port::Request,
    guard: Option<&provider_profile::Guard<'_, provider_profile::Binding>>,
    cooperative: bool,
    checked: &mut bool,
) -> Result<port::Removed<R::Evidence>, port::Error> {
    if let Some(guard) = guard {
        let result = guard.remove(&mut executor, request, cooperative);
        if result.is_ok() {
            *checked = true;
        }
        result
    } else {
        port::remove_exact(&mut executor, request, cooperative)
    }
}
pub fn execute(parsed: Parsed) -> Result<(Vec<u8>, u8), String> {
    execute_authorized(parsed, None)
}
pub fn execute_authorized(
    parsed: Parsed,
    authority: Option<std::sync::Arc<intent::policy::NativeAuthority>>,
) -> Result<(Vec<u8>, u8), String> {
    let p = parsed.profile;
    let index = Index::open(&p.index.root, &p.index.minimum_checkpoint)
        .map_err(|_| "cannot open witnessed protection index")?;
    let mut journal = SourceJournal::open(&p.journal.root, &p.journal.minimum_checkpoint)
        .map_err(|_| "cannot open witnessed intent journal")?;
    let mut nodes = MetadataStore::open(&p.metadata.root, &p.metadata.minimum_checkpoint)
        .map_err(|_| "cannot open witnessed intent metadata")?;
    if index.identity() != &p.index.identity || journal.identity() != &p.journal.identity {
        return Err("foreign removal authority".into());
    }
    let ctx = intent::inspect_retained(
        &mut journal,
        &nodes,
        &intent::Enrollment {
            owner: p.enrollment.owner.clone(),
            subject: p.enrollment.subject.clone(),
        },
        &p.operation,
    )
    .map_err(|_| "exact admitted intent unavailable")?;
    let ctx = if let Some(authority) = authority {
        ctx.attach_policy_authority(&nodes, authority)
            .map_err(|_| "saved policy authority binding refused")?
    } else {
        ctx
    };
    if ctx
        .decision()
        .digest()
        .map_err(|_| "invalid admitted decision")?
        != p.decision
    {
        return Err("admitted removal decision mismatch".into());
    }
    let saved_profiles = match (p.intent_execution_profile.as_ref(), ctx.execution_profile()) {
        (None, None) => None,
        (Some(expected), Some(context))
            if context
                .fingerprint()
                .map_err(|_| "invalid saved execution context")?
                == *expected =>
        {
            Some(context)
        }
        _ => {
            return Err(
                "missing/foreign saved execution context; explicit profiled format required".into(),
            );
        }
    };
    let preservation = p
        .preservation
        .as_ref()
        .map(|c| c.digest())
        .transpose()
        .map_err(|_| "invalid preservation digest")?;
    let store = LocalStore::open(&p.destination.root)
        .map_err(|_| "native removal destination unavailable")?;
    if store.identity() != &p.destination.place {
        return Err("native removal place mismatch".into());
    }
    let mut ledger = CapacityLedger::open(&p.capacity.root, &p.capacity.minimum_checkpoint)
        .map_err(|_| "cannot open witnessed removal capacity")?;
    if ledger.identity() != &p.capacity.ledger {
        return Err("foreign removal capacity".into());
    }
    let progress_recovered_from_prepared = progress::local_work::resume_removed_outcome(
        &mut journal,
        &mut nodes,
        &ctx,
        &p.object,
        p.attempt,
        &p.destination.place,
        preservation.as_ref(),
    )
    .map_err(|_| "prepared removal outcome cannot be reconciled")?;
    let (work, outcome) =
        progress::local_work::at_attempt_outcome(&mut journal, &nodes, &ctx, &p.object, p.attempt)
            .map_err(|_| "exact adopted removal work unavailable")?
            .ok_or("no bound work for exact removal attempt")?;
    if saved_profiles.is_some()
        && outcome
            .as_ref()
            .is_some_and(|o| !matches!(o, Outcome::Removed { .. }))
    {
        return Err("profiled attempt already has another outcome; new admission required".into());
    }
    let reported = matches!(outcome, Some(Outcome::Removed { .. }));
    let plan = work.plan();
    if plan.scope().store != p.destination.place
        || plan.scope().object != p.object
        || plan.scope().preservation != preservation
    {
        return Err("saved removal scope/preservation mismatch".into());
    }
    let mut gate = index
        .protection(&p.metadata.root, &nodes.checkpoint(), &ctx, plan)
        .map_err(|_| "cannot bind current native protection")?;
    let request = port::native::request(plan).map_err(|_| "invalid native removal request")?;
    let profiled = p.schema != "tig.removal-execution-profile.experimental-1";
    let removal_profile = saved_profiles
        .map(|context| {
            context.binding(
                intent::execution_profile::Role::Removal,
                &p.destination.place,
            )
        })
        .transpose()
        .map_err(|_| "missing saved removal place profile")?;
    let removal_guard = if reported {
        None
    } else {
        removal_profile
            .map(|(id, record)| record.admit(id, &record.binding))
            .transpose()
            .map_err(|_| "invalid saved removal profile")?
    };
    let mut removal_profile_checked = false;
    let mut requested: Vec<_> = p
        .custody_profiles
        .as_ref()
        .map(|ps| ps.keys().cloned().collect())
        .unwrap_or_default();
    let mut endpoints: Vec<_> = p
        .survivors
        .into_iter()
        .map(|binding| {
            let accepted = if let Some(context) = saved_profiles {
                let (id, record) = context
                    .binding(
                        intent::execution_profile::Role::Custody,
                        &binding.destination.place,
                    )
                    .map_err(|_| "missing saved survivor profile")?;
                Some((id.clone(), record.clone()))
            } else {
                binding.custody_profile.as_ref().map(|id| {
                    (
                        id.clone(),
                        p.custody_profiles.as_ref().expect("parsed profiles")[id].clone(),
                    )
                })
            };
            Ok(Endpoint {
                binding,
                maximum: p.maximum_object_bytes,
                opened: None,
                accepted,
                profile_checked: false,
            })
        })
        .collect::<Result<_, String>>()?;
    if saved_profiles.is_some() {
        requested = endpoints
            .iter()
            .filter_map(|e| e.accepted.as_ref().map(|(id, _)| id.clone()))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
    }
    let execution = removal::Execution {
        store: &store,
        ledger: &mut ledger,
        job: work.job(),
        journal: &mut journal,
        nodes: &nodes,
        intent: &ctx,
        protection: &mut gate,
    };
    let result = if p.preservation.is_some() {
        let mut refs: Vec<&mut dyn custody::Retainer> = endpoints
            .iter_mut()
            .map(|e| e as &mut dyn custody::Retainer)
            .collect();
        port::native::retained(execution, plan, &mut refs).and_then(|executor| {
            guarded_remove(
                executor,
                &request,
                removal_guard.as_ref(),
                p.allow_cooperative,
                &mut removal_profile_checked,
            )
        })
    } else {
        port::native::final_loss(execution, plan).and_then(|executor| {
            guarded_remove(
                executor,
                &request,
                removal_guard.as_ref(),
                p.allow_cooperative,
                &mut removal_profile_checked,
            )
        })
    };
    let mut confirmed = None;
    let mut progress_origin = None;
    let mut pending = None;
    match result {
        Ok(removed) => {
            let r = removed.evidence;
            confirmed = Some(
                json!({"evidence":r.evidence,"file_data_before":r.file_data_before,
                "charged_metadata":r.charged_metadata,"settlement_checkpoint":r.settlement.checkpoint()}),
            );
            match progress::finish(
                &mut journal,
                &mut nodes,
                &ctx,
                &p.object,
                p.attempt,
                Action::Remove {
                    store: p.destination.place.clone(),
                },
                Outcome::Removed {
                    evidence: r.evidence,
                    allocation: progress::Allocation {
                        before: Some(r.file_data_before),
                        reclaimed: None,
                    },
                },
            ) {
                Ok(origin) => progress_origin = Some(format!("{origin:?}")),
                Err(e) => {
                    pending = Some(format!("native effect confirmed; progress pending ({e:?})"))
                }
            }
        }
        Err(e) => pending = Some(format!("native removal pending ({e:?}); effects may exist")),
    }
    let survivors:Vec<_>=endpoints.iter().filter_map(|e|e.opened.as_ref().map(|(_,l)|
        json!({"place":e.binding.destination.place,"ledger":l.identity(),"minimum_checkpoint":l.checkpoint()}))).collect();
    let complete = pending.is_none();
    let mut out = json!({"schema":"tig.removal-execution-result.experimental-1","input":parsed.digest,
        "operation":p.operation,"decision":p.decision,"object":p.object,"place":p.destination.place,
        "attempt":p.attempt,"removal_work_complete":complete,"native_disposition":confirmed,
        "progress_origin":progress_origin,"progress_recovered_from_prepared":progress_recovered_from_prepared,"pending":pending,"survivors_opened":survivors,
        "last_confirmed_minimum_checkpoints":{"index":index.checkpoint(),"journal":journal.checkpoint(),
            "metadata":nodes.checkpoint(),"capacity":ledger.checkpoint()},
        "enforcement":"cooperative-process-restart","current_absence":"unknown","net_reclaimed_bytes":null,
        "other_removal_authorized":false});
    if profiled {
        let checked: std::collections::BTreeSet<_> = endpoints
            .iter()
            .filter(|e| e.profile_checked)
            .filter_map(|e| e.accepted.as_ref().map(|(id, _)| id.clone()))
            .collect();
        out["schema"] = json!("tig.removal-execution-result.experimental-2");
        out["custody_profiles_requested"] = json!(requested);
        out["custody_profiles_checked"] = json!(checked);
        out["current_custody"] = json!("unknown");
        out["custody_evidence_scope"] = json!(if checked.is_empty() {
            "no-new-survivor-acquisition"
        } else {
            "held-during-native-execution-only"
        });
        if saved_profiles.is_some() {
            out["schema"] = json!("tig.removal-execution-result.experimental-3");
            out["intent_execution_profile"] = json!(p.intent_execution_profile);
            out["removal_profile_requested"] = json!(removal_profile.map(|(id, _)| id));
            out["removal_profile_checked"] = json!(removal_profile_checked);
            out["reported_completion_reused"] = json!(reported);
        }
    }
    Ok((
        canonical_json(&out).map_err(|_| "cannot encode removal result; effects may exist")?,
        if complete { 0 } else { 3 },
    ))
}
