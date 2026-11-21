use super::{
    admission,
    profile::Profile,
    witnesses::{self, Minimums, RetentionReport},
    work::{self, Work},
};
use std::collections::{BTreeMap, BTreeSet};
use tig_core::{Digest, Object, identity::parse_json};
use tig_store::{
    AckLevel, Key,
    ledger::CapacityLedger,
    local::LocalStore,
    provider::custody::create,
    retention::{self, RetentionGrant, RetentionPlan, RetentionRequest},
};
type Pair = (Digest, Object);
type Claims = BTreeMap<Pair, RetentionGrant>;
struct Candidate {
    index: usize,
    input: admission::profile::Input,
    issuer: Option<super::issuer_profiles::Accepted>,
    keys: super::issuer_profiles::Keys,
}
pub struct Action {
    pub index: usize,
    pub input: admission::profile::Input,
    pub plan: RetentionPlan,
    pub request: create::Request,
    pub prior: Option<RetentionGrant>,
    pub issuer: Option<super::issuer_profiles::Accepted>,
    pub budget: Key,
    pub finish: Key,
}
pub fn key(prefix: &str, base: &Key) -> Result<Key, String> {
    Key::parse(&format!("{prefix}{}", base.as_str()))
        .map_err(|_| "retention operation identity exceeds bound".into())
}
fn previous(bytes: Option<&[u8]>) -> Result<(Claims, Minimums), String> {
    let Some(bytes) = bytes else {
        return Ok((BTreeMap::new(), BTreeMap::new()));
    };
    let minimums = witnesses::parse(bytes)?;
    let report: RetentionReport =
        serde_json::from_value(parse_json(bytes).map_err(|_| "invalid prior claim result")?)
            .map_err(|_| "prior claim result required")?;
    if !matches!(
        report.schema.as_str(),
        "tig.retained-placement-result.experimental-1"
            | "tig.retained-placement-result.experimental-2"
    ) {
        return Err("prior claim result required".into());
    }
    let mut claims = BTreeMap::new();
    for entry in report.completed {
        let bytes = serde_json::to_vec(&entry["grant"]).map_err(|_| "invalid native grant")?;
        let grant =
            RetentionGrant::from_json(&bytes).map_err(|_| "invalid native grant identity")?;
        let pair = (
            grant.store().clone(),
            grant.request().publication.object.clone(),
        );
        if claims.insert(pair, grant).is_some() {
            return Err("duplicate prior claim pair; partition evidence explicitly".into());
        }
    }
    Ok((claims, minimums))
}
fn select(work: &Work, profile: &Profile) -> Result<Vec<Candidate>, String> {
    let wanted: BTreeSet<_> = profile.places.iter().cloned().collect();
    let mut seen = BTreeSet::new();
    let mut selected = Vec::new();
    for (index, job) in work.jobs.iter().enumerate() {
        let places: Vec<_> = job
            .places
            .iter()
            .filter(|p| wanted.contains(*p))
            .cloned()
            .collect();
        if places.is_empty() {
            continue;
        }
        seen.extend(places.iter().cloned());
        let input = work::input(job)?;
        let (issuer, keys) = if let Some(issuance) = &profile.issuance {
            let (issuer, keys) = issuance.select(
                &places,
                &input.destination.place,
                &work.digest,
                &work.owner,
                &input.request.object,
            )?;
            (Some(issuer), keys)
        } else {
            (
                None,
                super::issuer_profiles::Keys {
                    claim: key("claim-", &input.job)?,
                    budget: key("claim-budget-", &input.job)?,
                    finish: key("claim-finished-", &input.job)?,
                },
            )
        };
        selected.push(Candidate {
            index,
            input,
            issuer,
            keys,
        });
    }
    if !work.jobs.is_empty() && seen != wanted {
        return Err("selected place is absent from saved work".into());
    }
    Ok(selected)
}
struct Checks<'a> {
    profile: &'a Profile,
    extra: Vec<Minimums>,
    checked: BTreeSet<Digest>,
    minimums: Minimums,
}
fn preflight(
    candidate: Candidate,
    prior: Option<RetentionGrant>,
    checks: &mut Checks<'_>,
) -> Result<Action, String> {
    let input = &candidate.input;
    let store = LocalStore::open(&input.destination.root)
        .map_err(|_| "cannot open selected retention destination")?;
    if store.identity() != &input.destination.place {
        return Err("selected retention instance changed".into());
    }
    let mut ledger = CapacityLedger::open(&input.capacity.root, &input.capacity.minimum_checkpoint)
        .map_err(|_| "cannot open selected witnessed authority")?;
    if ledger.identity() != &input.capacity.ledger {
        return Err("selected retention authority changed".into());
    }
    if checks.checked.insert(input.capacity.ledger.clone()) {
        for extra in &checks.extra {
            if let Some(minimum) = extra.get(&input.capacity.ledger) {
                ledger = CapacityLedger::open(&input.capacity.root, minimum)
                    .map_err(|_| "supplied retention witness is not satisfied")?;
                if ledger.identity() != &input.capacity.ledger {
                    return Err("supplied retention authority changed".into());
                }
            }
        }
    }
    let request = RetentionRequest {
        operation: candidate.keys.claim,
        owner: input.request.owner.clone(),
        publication: input.request.clone(),
        minimum_ack: AckLevel::ProcessRestart,
    };
    let plan =
        retention::plan(&store, &request).map_err(|_| "selected retention geometry unsupported")?;
    let wanted = create::native::request(&store, &plan)
        .map_err(|_| "selected publication incomplete or stale")?;
    if let Some(grant) = &prior {
        retention::observe_claim(&mut ledger, &store, grant)
            .map_err(|_| "prior selected claim is not active; reconcile exact evidence")?;
    } else {
        if let Some(issuer) = &candidate.issuer {
            issuer.preflight(&store)?;
        }
        if plan.required_peak() > checks.profile.maximum_peak_per_action {
            return Err("new retention exceeds explicit peak ceiling".into());
        }
    }
    let current = ledger.checkpoint();
    if checks
        .minimums
        .get(ledger.identity())
        .is_none_or(|old| old.sequence() < current.sequence())
    {
        checks.minimums.insert(ledger.identity().clone(), current);
    }
    Ok(Action {
        index: candidate.index,
        input: candidate.input,
        plan,
        request: wanted,
        prior,
        issuer: candidate.issuer,
        budget: candidate.keys.budget,
        finish: candidate.keys.finish,
    })
}
pub fn prepare(
    profile: &Profile,
    work: &Work,
    old: Option<&[u8]>,
    supplied: Option<&[u8]>,
) -> Result<(Vec<Action>, Minimums), String> {
    work::validate(work)?;
    let (mut claims, previous) = previous(old)?;
    let supplied = supplied
        .map(witnesses::parse)
        .transpose()?
        .unwrap_or_default();
    witnesses::check_scope(work, &previous)?;
    witnesses::check_scope(work, &supplied)?;
    let candidates = select(work, profile)?;
    let pairs: BTreeSet<_> = candidates
        .iter()
        .map(|c| {
            (
                c.input.destination.place.clone(),
                c.input.request.object.clone(),
            )
        })
        .collect();
    if claims.iter().any(|(pair, grant)| {
        !pairs.contains(pair)
            || grant.request().owner != work.owner
            || grant.request().minimum_ack < AckLevel::ProcessRestart
    }) {
        return Err(
            "prior claim outside selected scope or acknowledgement; partition/reconcile evidence explicitly".into(),
        );
    }
    let mut checks = Checks {
        profile,
        extra: vec![previous, supplied],
        checked: BTreeSet::new(),
        minimums: BTreeMap::new(),
    };
    let mut actions = Vec::new();
    for candidate in candidates {
        let prior = claims.remove(&(
            candidate.input.destination.place.clone(),
            candidate.input.request.object.clone(),
        ));
        actions.push(preflight(candidate, prior, &mut checks)?);
    }
    Ok((actions, checks.minimums))
}
