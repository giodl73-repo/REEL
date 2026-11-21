use super::{
    native_operation::fresh_operation,
    prepare::{self, Action},
    profile::Profile,
    witnesses::{self, Minimums, RetentionReport},
    work::Work,
};
use serde_json::{Value, json};
use tig_core::identity::canonical_json;
use tig_store::{
    ledger::{CapacityLedger, JournalRef, LedgerError},
    local::LocalStore,
    local_capacity::LocalCapacityProbe,
    provider::custody::create,
    retention,
};
fn reserve(
    ledger: &mut CapacityLedger,
    store: &LocalStore,
    action: &Action,
) -> Result<JournalRef, String> {
    let operation = action.budget.clone();
    let owner = action.input.request.owner.clone();
    let peak = action.plan.required_peak();
    let receipt = match ledger.reserve(operation.clone(), owner.clone(), peak) {
        Ok(receipt) => receipt,
        Err(LedgerError::NeedsProbe) => {
            let probe = LocalCapacityProbe::bind(store, ledger)
                .map_err(|_| "retention capacity enrollment unavailable")?;
            probe
                .observe(
                    store,
                    ledger,
                    fresh_operation("retain-probe")?,
                    fresh_operation("retain-sample")?,
                )
                .map_err(|_| "retention capacity observation failed; effects may exist")?;
            ledger
                .reserve(operation, owner, peak)
                .map_err(|_| "retention capacity admission refused; effects may exist")?
        }
        Err(_) => return Err("retention capacity admission refused; effects may exist".into()),
    };
    Ok(receipt.reference().clone())
}
fn one(action: &Action, minimums: &mut Minimums) -> Result<Value, String> {
    let input = &action.input;
    let store = LocalStore::open(&input.destination.root)
        .map_err(|_| "selected destination unavailable")?;
    if store.identity() != &input.destination.place {
        return Err("selected destination changed".into());
    }
    let minimum = minimums
        .get(&input.capacity.ledger)
        .ok_or("missing selected authority witness")?;
    let mut ledger = CapacityLedger::open(&input.capacity.root, minimum)
        .map_err(|_| "selected authority unavailable or restored")?;
    if ledger.identity() != &input.capacity.ledger {
        return Err("selected authority changed".into());
    }
    let reused = action.prior.is_some();
    let grant = if let Some(grant) = &action.prior {
        grant.clone()
    } else {
        if let Some(issuer) = &action.issuer {
            issuer.preflight(&store)?;
        }
        let job = reserve(&mut ledger, &store, action)?;
        minimums.insert(input.capacity.ledger.clone(), ledger.checkpoint());
        let issued = {
            let mut issuer = create::native::Native {
                store: &store,
                ledger: &mut ledger,
                job: &job,
                plan: &action.plan,
            };
            let result = if let Some(profile) = &action.issuer {
                profile.issue(&mut issuer, &action.request)
            } else {
                create::issue_exact(&mut issuer, &action.request, true)
            };
            result.map_err(|e| format!("native retained claim failed ({e}); effects may exist"))?
        };
        ledger
            .finish(action.finish.clone(), &job)
            .map_err(|_| "retention completion pending; claim may exist")?;
        issued.evidence.grant
    };
    let observation = retention::observe_claim(&mut ledger, &store, &grant)
        .map_err(|_| "retained claim observation failed; reconcile exact evidence")?;
    minimums.insert(input.capacity.ledger.clone(), ledger.checkpoint());
    // The immutable work index resolves aliases; selected aliases occur once in
    // the report header, rather than growing once per object in every receipt.
    let mut output = json!({"index":action.index,"grant":grant,"observation":observation,
        "prior_claim_reused":reused,"payload_hashed_in_command":!reused});
    if let Some(issuer) = &action.issuer {
        output["execution_profile"] = if reused {
            Value::Null
        } else {
            json!(issuer.expected)
        };
        output["issuance_profile_checked"] = json!(!reused);
    }
    Ok(output)
}
pub fn apply(
    profile: Profile,
    work: Work,
    previous: Option<&[u8]>,
    supplied: Option<&[u8]>,
) -> Result<(Vec<u8>, u8), String> {
    let (actions, mut minimums) = prepare::prepare(&profile, &work, previous, supplied)?;
    let mut completed = Vec::new();
    let mut pending = None;
    for action in &actions {
        match one(action, &mut minimums) {
            Ok(result) => completed.push(result),
            Err(reason) => {
                witnesses::observe(&work.jobs[action.index], &mut minimums);
                pending =
                    Some(json!({"index":action.index,"reason":reason,"effects_may_exist":true}));
                break;
            }
        }
    }
    let complete = pending.is_none();
    let mut places = profile.places;
    places.sort();
    let profiled = profile.issuance.is_some();
    let report = RetentionReport {
        schema: if profiled {
            "tig.retained-placement-result.experimental-2".into()
        } else {
            "tig.retained-placement-result.experimental-1".into()
        },
        requested_issuance_profiles: profile
            .issuance
            .as_ref()
            .map(|p| p.profiles.keys().cloned().collect()),
        work_digest: work.digest,
        selected_places: places,
        claim_metadata_complete: complete,
        completed,
        pending,
        last_confirmed_minimum_checkpoints: minimums,
        witness_scope: "selected-retention-authorities".into(),
        enforcement: "cooperative-process-restart".into(),
        held_custody: false,
        independent_failure_domains: "unknown".into(),
        removal_authorized: false,
    };
    let output = canonical_json(
        &serde_json::to_value(report)
            .map_err(|_| "cannot encode retention result; effects may exist")?,
    )
    .map_err(|_| "retention result exceeds metadata budget; effects may exist")?;
    Ok((output, if complete { 0 } else { 3 }))
}
