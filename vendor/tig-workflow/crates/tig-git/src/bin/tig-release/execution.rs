use super::profile::{self, Mode, Parsed};
use serde_json::{Value, json};
use tig_core::identity::canonical_json;
use tig_store::{
    ledger::{CapacityLedger, JournalRef},
    local::LocalStore,
    metadata::MetadataStore,
    provider::{custody, custody::release as port, profile as provider_profile},
    retention::RetentionGrant,
    retirement_intent::{
        self as intent,
        progress::{self, Action, ClaimProtocol, Outcome},
    },
    source_claims::SourceJournal,
};
enum Work {
    Retirement(Box<progress::release_work::ReleaseWork>),
    Handoff(Box<progress::handoff_work::HandoffWork>),
}
impl Work {
    fn previous(&self) -> &RetentionGrant {
        match self {
            Self::Retirement(w) => w.plan().grant(),
            Self::Handoff(w) => &w.plan().request().previous,
        }
    }
    fn job(&self) -> &JournalRef {
        match self {
            Self::Retirement(w) => w.job(),
            Self::Handoff(w) => w.job(),
        }
    }
}
fn open(binding: &profile::Native) -> Result<(LocalStore, CapacityLedger), String> {
    let store = LocalStore::open(&binding.root).map_err(|_| "native claim place unavailable")?;
    let ledger = CapacityLedger::open(&binding.capacity.root, &binding.capacity.minimum_checkpoint)
        .map_err(|_| "native claim capacity unavailable or below supplied minimum")?;
    if store.identity() != &binding.place || ledger.identity() != &binding.capacity.ledger {
        return Err("foreign native claim binding".into());
    }
    Ok((store, ledger))
}
fn guarded_release<E: port::Executor>(
    mut executor: E,
    request: &port::Request,
    guard: Option<&provider_profile::Guard<'_, provider_profile::Binding>>,
    cooperative: bool,
    checked: &mut bool,
) -> Result<port::Released<E::Evidence>, port::Error> {
    if let Some(guard) = guard {
        let result = guard.release(&mut executor, request, cooperative);
        if result.is_ok() {
            *checked = true;
        }
        result
    } else {
        port::release_exact(&mut executor, request, cooperative)
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
    let mut journal = SourceJournal::open(&p.journal.root, &p.journal.minimum_checkpoint)
        .map_err(|_| "cannot open witnessed claim intent journal")?;
    let mut nodes = MetadataStore::open(&p.metadata.root, &p.metadata.minimum_checkpoint)
        .map_err(|_| "cannot open witnessed claim metadata")?;
    if journal.identity() != &p.journal.identity {
        return Err("foreign claim intent journal".into());
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
    .map_err(|_| "exact claim intent unavailable")?;
    let ctx = if let Some(authority) = authority {
        ctx.attach_policy_authority(&nodes, authority)
            .map_err(|_| "saved policy authority binding refused")?
    } else {
        ctx
    };
    if ctx
        .decision()
        .digest()
        .map_err(|_| "invalid claim decision")?
        != p.decision
    {
        return Err("claim decision mismatch".into());
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
    let role = match &p.mode {
        Mode::Retirement => intent::execution_profile::Role::RetirementRelease,
        Mode::Handoff { .. } => intent::execution_profile::Role::HandoffRelease,
    };
    let source_profile = saved_profiles
        .map(|context| context.binding(role, &p.claim))
        .transpose()
        .map_err(|_| "missing saved exact claim protocol profile")?;
    if source_profile.is_some_and(|(_, record)| record.binding.place != p.source.place) {
        return Err("saved release profile has foreign source place".into());
    }
    let successor_profile = match (&p.mode, saved_profiles) {
        (Mode::Handoff { successor }, Some(context)) => Some(
            context
                .binding(intent::execution_profile::Role::Custody, &successor.place)
                .map_err(|_| "missing saved successor custody profile")?,
        ),
        _ => None,
    };
    let protocol = p.mode.protocol();
    let recovered = progress::resume_released_outcome(
        &mut journal,
        &mut nodes,
        &ctx,
        &p.object,
        p.attempt,
        &p.claim,
        protocol,
    )
    .map_err(|_| "prepared claim outcome cannot be reconciled")?;
    let (work, outcome) = match protocol {
        ClaimProtocol::Retirement => {
            let (w, o) = progress::release_work::at_attempt(
                &mut journal,
                &nodes,
                &ctx,
                &p.object,
                p.attempt,
            )
            .map_err(|_| "saved retirement work unavailable")?
            .ok_or("no exact typed retirement work")?;
            (Work::Retirement(Box::new(w)), o)
        }
        ClaimProtocol::Handoff => {
            let (w, o) = progress::handoff_work::at_attempt(
                &mut journal,
                &nodes,
                &ctx,
                &p.object,
                p.attempt,
            )
            .map_err(|_| "saved handoff work unavailable")?
            .ok_or("no exact typed handoff work")?;
            (Work::Handoff(Box::new(w)), o)
        }
    };
    if work.previous().claim_id() != &p.claim
        || work.previous().store() != &p.source.place
        || work.previous().request().publication.object != p.object
        || work.job().ledger_identity() != &p.source.capacity.ledger
    {
        return Err("exact claim/source/capacity scope mismatch".into());
    }
    match (&work, &p.mode) {
        (Work::Handoff(w), Mode::Handoff { successor })
            if w.plan().request().successor.store() == &successor.place => {}
        (Work::Retirement(_), Mode::Retirement) => (),
        _ => return Err("claim protocol/successor mismatch".into()),
    }
    let mut evidence = None;
    let mut native_receipt = None;
    let mut pending = None;
    let mut source_min = None;
    let mut successor_min = None;
    let mut release_profile_checked = false;
    let mut successor_profile_checked = false;
    let reported = match outcome {
        Some(Outcome::Released { evidence: e }) => {
            evidence = Some(e);
            true
        }
        Some(_) => {
            return Err(
                "attempt already has another terminal outcome; new admission required".into(),
            );
        }
        None => false,
    };
    if !reported {
        let (source, mut ledger) = open(&p.source)?;
        let release_guard = source_profile
            .map(|(id, record)| record.admit(id, &record.binding))
            .transpose()
            .map_err(|_| "invalid saved release profile")?;
        let result: Result<(tig_core::Digest, Value), String> = match (&work, &p.mode) {
            (Work::Retirement(w), Mode::Retirement) => {
                let request = port::native::retirement_request(w.plan(), p.attempt)
                    .map_err(|e| format!("native retirement request ({e:?})"))?;
                let result = port::native::retirement(port::native::Retirement {
                    ledger: &mut ledger,
                    store: &source,
                    job: w.job(),
                    plan: w.plan(),
                    journal: &mut journal,
                    nodes: &nodes,
                    intent: &ctx,
                    attempt: p.attempt,
                })
                .and_then(|executor| {
                    guarded_release(
                        executor,
                        &request,
                        release_guard.as_ref(),
                        p.allow_cooperative,
                        &mut release_profile_checked,
                    )
                });
                result.map(|r|{let r=r.evidence;(r.evidence,json!({"settlement_checkpoint":r.settlement.checkpoint(),"charged_footprint":r.charged_footprint}))})
                    .map_err(|e|format!("native retirement pending ({e:?}); effects may exist"))
            }
            (Work::Handoff(_), Mode::Handoff { successor }) => {
                let (successor, mut peer) = open(successor)?;
                let peer_guard = successor_profile
                    .map(|(id, record)| record.admit(id, &record.binding))
                    .transpose()
                    .map_err(|_| "invalid saved successor profile")?;
                let peer_caps = custody::native::capabilities(&successor);
                if let Some(guard) = &peer_guard {
                    guard
                        .check_custody(&peer_caps)
                        .map_err(|e| format!("successor custody profile refused ({e:?})"))?;
                }
                let result = port::native::admitted_handoff(
                    progress::handoff_work::Execution {
                        source: &source,
                        source_ledger: &mut ledger,
                        successor: &successor,
                        successor_ledger: &mut peer,
                        journal: &mut journal,
                        nodes: &nodes,
                        intent: &ctx,
                    },
                    &p.object,
                    p.attempt,
                )
                .and_then(|(request, executor)| {
                    guarded_release(
                        executor,
                        &request,
                        release_guard.as_ref(),
                        p.allow_cooperative,
                        &mut release_profile_checked,
                    )
                });
                if let Some(guard) = &peer_guard {
                    guard.check_custody(&custody::native::capabilities(&successor)).map_err(|e| format!("successor custody profile changed after native execution ({e:?}); effects may exist"))?;
                    successor_profile_checked = result.is_ok();
                }
                successor_min = Some(peer.checkpoint());
                result.map(|r|{let r=r.evidence;(r.marker,json!({"settlement_checkpoint":r.settlement.checkpoint(),"charged_footprint":r.charged_footprint}))})
                    .map_err(|e|format!("native handoff pending ({e:?}); effects may exist"))
            }
            _ => return Err("claim protocol changed".into()),
        };
        source_min = Some(ledger.checkpoint());
        match result {
            Ok((e, receipt)) => {
                evidence = Some(e.clone());
                native_receipt = Some(receipt);
                if let Err(e) = progress::finish(
                    &mut journal,
                    &mut nodes,
                    &ctx,
                    &p.object,
                    p.attempt,
                    Action::Release {
                        claim: p.claim.clone(),
                    },
                    Outcome::Released { evidence: e },
                ) {
                    pending = Some(format!(
                        "native effect confirmed; claim progress pending ({e:?})"
                    ));
                }
            }
            Err(e) => pending = Some(e),
        }
    }
    let complete = pending.is_none();
    let mut out = json!({"schema":"tig.claim-execution-result.experimental-1","input":parsed.digest,"operation":p.operation,
        "decision":p.decision,"object":p.object,"attempt":p.attempt,"claim":p.claim,"source":p.source.place,
        "claim_work_complete":complete,"reported_completion_reused":reported,"progress_recovered_from_prepared":recovered,
        "reported_evidence":evidence,"native_effect_verified_in_command":native_receipt.is_some(),"native_receipt":native_receipt,"pending":pending,
        "witness_scope":if reported {"intent-authorities-only"} else {"opened-native-capacity-authorities"},
        "last_confirmed_minimum_checkpoints":{"journal":journal.checkpoint(),"metadata":nodes.checkpoint(),"source_capacity":source_min,"successor_capacity":successor_min},
        "current_claim_state":"unknown","current_custody":"unknown","payload_deleted":false,"removal_authorized":false,"net_reclaimed_bytes":null,
        "enforcement":"cooperative-process-restart"});
    if p.schema == "tig.claim-execution-profile.experimental-2" {
        out["schema"] = json!("tig.claim-execution-result.experimental-2");
        out["intent_execution_profile"] = json!(p.intent_execution_profile);
        out["release_profile_requested"] = json!(source_profile.map(|(id, _)| id));
        out["release_profile_checked"] = json!(release_profile_checked);
        out["successor_custody_profile_requested"] = json!(successor_profile.map(|(id, _)| id));
        out["successor_custody_profile_checked"] = json!(successor_profile_checked);
    }
    Ok((
        canonical_json(&out).map_err(|_| "cannot encode claim result; effects may exist")?,
        if complete { 0 } else { 3 },
    ))
}
