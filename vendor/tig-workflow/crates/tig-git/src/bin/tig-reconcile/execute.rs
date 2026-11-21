use super::{
    acquisition, admission, witnesses,
    work::{self, Work},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use tig_core::{Digest, identity::canonical_json};
use tig_store::{
    ledger::{CapacityLedger, Checkpoint},
    local::LocalStore,
};
fn preflight(
    work: &Work,
    supplied: &witnesses::Minimums,
) -> Result<BTreeMap<Digest, Checkpoint>, String> {
    let mut witnesses: BTreeMap<Digest, Checkpoint> = BTreeMap::new();
    let mut checked = BTreeSet::new();
    for job in &work.jobs {
        let input = work::input(job)?;
        let store = LocalStore::open(&input.destination.root)
            .map_err(|_| "cannot open an existing work destination")?;
        if store.identity() != &input.destination.place {
            return Err("work destination identity changed".into());
        }
        let plan = tig_store::accounted::plan(&store, &input.request)
            .map_err(|_| "work request unsupported by destination")?;
        if plan.required_peak() > input.maximum_peak {
            return Err("work request exceeds per-action admission ceiling".into());
        }
        let ledger = CapacityLedger::open(&input.capacity.root, &input.capacity.minimum_checkpoint)
            .map_err(|_| "cannot open a witnessed work authority")?;
        if ledger.identity() != &input.capacity.ledger {
            return Err("work authority identity changed".into());
        }
        if let Some(provider) = work::provider(work, job)? {
            provider.preflight(&tig_store::provider::publication::native::capabilities(
                &store,
            ))?;
        }
        let mut current = ledger.checkpoint();
        if checked.insert(input.capacity.ledger.clone())
            && let Some(minimum) = supplied.get(&input.capacity.ledger)
        {
            drop(ledger);
            let newer = CapacityLedger::open(&input.capacity.root, minimum)
                .map_err(|_| "newer trusted work witness is not satisfied")?;
            if newer.identity() != &input.capacity.ledger {
                return Err("trusted witness authority identity changed".into());
            }
            current = newer.checkpoint();
        }
        if witnesses
            .get(&input.capacity.ledger)
            .is_none_or(|old| old.sequence() < current.sequence())
        {
            witnesses.insert(input.capacity.ledger, current);
        }
    }
    Ok(witnesses)
}
fn one(
    mut input: Value,
    provider: Option<crate::provider_profile::Envelope>,
    witnesses: &mut BTreeMap<Digest, Checkpoint>,
) -> Result<Value, String> {
    let parsed = admission::profile::parse(
        &serde_json::to_vec(&input).map_err(|_| "invalid admitted work")?,
    )?;
    let identity = parsed.capacity.ledger;
    let witness = witnesses
        .get(&identity)
        .ok_or("missing preflight witness")?;
    input["capacity"]["minimum_checkpoint"] = json!(witness);
    if let Some(provider) = provider {
        input["schema"] = json!("tig.publication-admission-profile.experimental-2");
        input["provider"] =
            serde_json::to_value(provider).map_err(|_| "cannot encode exact provider context")?;
    }
    let admitted = admission::execution::execute(admission::profile::parse(
        &serde_json::to_vec(&input).map_err(|_| "invalid admitted work")?,
    )?)?;
    let acquisition_profile = acquisition::profile::parse(&admitted)?;
    witnesses.insert(
        identity.clone(),
        acquisition_profile.admission.minimum_checkpoint.clone(),
    );
    let completed = acquisition::execution::execute(acquisition_profile)?;
    let output: Value = serde_json::from_slice(&completed)
        .map_err(|_| "cannot decode completed publication; effects may exist")?;
    let checkpoint: Checkpoint = serde_json::from_value(output["ledger_checkpoint"].clone())
        .map_err(|_| "cannot decode completed witness; effects may exist")?;
    witnesses.insert(identity, checkpoint);
    Ok(output)
}
pub fn apply(work: Work, supplied: witnesses::Minimums) -> Result<(Vec<u8>, u8), String> {
    work::validate(&work)?;
    witnesses::check_scope(&work, &supplied)?;
    let mut witnesses = preflight(&work, &supplied)?;
    let mut results = Vec::new();
    let mut failure = None;
    for (index, job) in work.jobs.iter().enumerate() {
        match one(
            job.admission.clone(),
            work::provider(&work, job)?,
            &mut witnesses,
        ) {
            Ok(result) => {
                // Resolve aliases once through the immutable work, rather than
                // repeating potentially long lists in every object receipt.
                results.push(json!({"index":index,"publication":result}))
            }
            Err(reason) => {
                witnesses::observe(job, &mut witnesses);
                failure = Some(json!({"index":index,"reason":reason,"effects_may_exist":true}));
                break;
            }
        }
    }
    let complete = failure.is_none();
    let mut output = json!({"schema":"tig.positive-placement-result.experimental-2","work_digest":work.digest,
        "positive_phase_complete":complete,"completed":results,"pending":failure,
        "last_confirmed_minimum_checkpoints":witnesses,"custody":"unknown","removal_authorized":false});
    if let Some(profiles) = &work.profiles {
        output["schema"] = json!("tig.positive-placement-result.experimental-3");
        output["provider_profiles"] = json!(profiles.keys().collect::<Vec<_>>());
    }
    let bytes =
        canonical_json(&output).map_err(|_| "cannot encode work result; effects may exist")?;
    Ok((bytes, if complete { 0 } else { 3 }))
}
