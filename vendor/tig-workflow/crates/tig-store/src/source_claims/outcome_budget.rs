//! Exact, bounded logical outcome capacity in the existing journal.
use super::*;
use std::{collections::BTreeSet, sync::Arc};
pub const STEP_BYTES: u64 = 4096;
pub const SETTLEMENT_BYTES: u64 = 1024;
pub const MAX_STEPS: usize = 8;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub operation: Key,
    pub command: Box<Command>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub operation: Key,
    pub source: Key,
    pub owner: Key,
    pub revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub steps: Vec<Step>,
    pub admissions: Vec<Admission>,
    pub endings: Vec<Vec<Key>>,
}
impl Plan {
    pub(super) fn sources(&self) -> BTreeSet<String> {
        self.steps
            .iter()
            .filter_map(|s| {
                progress_budget::scope(&s.command).map(|(s, _, _)| s.as_str().to_owned())
            })
            .chain(self.admissions.iter().map(|a| a.source.as_str().to_owned()))
            .collect()
    }
    fn validate(&self, owner: &Key) -> Result<()> {
        if self.steps.is_empty()
            || self.steps.len() > MAX_STEPS
            || self.admissions.len() > 1
            || self.endings.is_empty()
            || self.endings.len() > 2
            || self
                .steps
                .windows(2)
                .any(|w| w[0].operation.as_str() >= w[1].operation.as_str())
        {
            return Err(Error::InvalidReference);
        }
        let keys = self
            .steps
            .iter()
            .map(|s| s.operation.as_str())
            .collect::<BTreeSet<_>>();
        for s in &self.steps {
            if !matches!(
                *s.command,
                Command::Prepare { .. }
                    | Command::Adopt { .. }
                    | Command::Abort { .. }
                    | Command::Retire { .. }
            ) || progress_budget::scope(&s.command).is_none_or(|(_, o, _)| o != owner)
                || encode(&Request {
                    operation: s.operation.clone(),
                    command: (*s.command).clone(),
                })?
                .len()
                    > 2048
            {
                return Err(Error::InvalidReference);
            }
        }
        for a in &self.admissions {
            if a.owner != *owner || keys.contains(a.operation.as_str()) {
                return Err(Error::Owner);
            }
        }
        for end in &self.endings {
            if end.is_empty()
                || end.windows(2).any(|w| w[0].as_str() >= w[1].as_str())
                || end.iter().any(|k| !keys.contains(k.as_str()))
            {
                return Err(Error::InvalidReference);
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetRef {
    journal: Digest,
    id: Key,
    owner: Key,
    creation: Key,
}
#[derive(Clone)]
pub(super) struct Record {
    owner: Key,
    creation: Key,
    plan: Arc<Plan>,
    spent: BTreeSet<String>,
    bytes: u64,
    closed: bool,
}
fn operation(prefix: &str, journal: &Digest, id: &Key) -> Result<Key> {
    Key::parse(&format!(
        "{prefix}-{}",
        Digest::blob(&encode(&(journal, id))?).as_str()
    ))
    .map_err(|_| Error::InvalidReference)
}
fn issue(
    j: &mut SourceJournal,
    operation: Key,
    command: Command,
    privileged: bool,
) -> Result<Receipt> {
    let result = j.apply_once(operation, command, |_| Ok(()), privileged);
    j.mark_uncertain(&result);
    result
}
pub fn reserve(j: &mut SourceJournal, id: Key, owner: Key, plan: Plan) -> Result<BudgetRef> {
    reserve_mode(j, id, owner, plan, false)
}
pub(crate) fn reserve_mode(
    j: &mut SourceJournal,
    id: Key,
    owner: Key,
    plan: Plan,
    privileged: bool,
) -> Result<BudgetRef> {
    let creation = operation("obr", j.identity(), &id)?;
    issue(
        j,
        creation.clone(),
        Command::ReserveOutcomes {
            id: id.clone(),
            owner: owner.clone(),
            plan,
        },
        privileged,
    )?;
    let reference = BudgetRef {
        journal: j.identity().clone(),
        id,
        owner,
        creation,
    };
    if record(&j.state, &reference)?.closed {
        return Err(Error::Conflict);
    }
    Ok(reference)
}
pub fn lookup(j: &mut SourceJournal, id: &Key, owner: &Key) -> Result<Option<BudgetRef>> {
    j.sync()?;
    let Some(r) = j.state.outcome_budgets.get(id.as_str()) else {
        return Ok(None);
    };
    if r.owner != *owner {
        return Err(Error::Owner);
    }
    Ok(Some(BudgetRef {
        journal: j.identity().clone(),
        id: id.clone(),
        owner: owner.clone(),
        creation: r.creation.clone(),
    }))
}
pub(crate) fn active(j: &mut SourceJournal, r: &BudgetRef) -> Result<bool> {
    j.sync()?;
    Ok(!record(&j.state, r)?.closed)
}
pub fn release(j: &mut SourceJournal, r: &BudgetRef) -> Result<()> {
    release_mode(j, r, false)
}
pub(crate) fn release_mode(j: &mut SourceJournal, r: &BudgetRef, privileged: bool) -> Result<()> {
    j.sync()?;
    if record(&j.state, r)?.closed {
        return Ok(());
    }
    issue(
        j,
        operation("obf", j.identity(), &r.id)?,
        Command::ReleaseOutcomes { budget: r.clone() },
        privileged,
    )?;
    Ok(())
}
fn record<'a>(state: &'a State, r: &BudgetRef) -> Result<&'a Record> {
    if r.journal != state.head.genesis {
        return Err(Error::InvalidReference);
    }
    let found = state
        .outcome_budgets
        .get(r.id.as_str())
        .ok_or(Error::Missing)?;
    if found.owner != r.owner || found.creation != r.creation {
        return Err(Error::Owner);
    }
    Ok(found)
}
pub(super) fn held(state: &State) -> Result<(u64, u64)> {
    Ok((state.outcome_events, state.outcome_bytes))
}
pub(super) fn missing_sources(state: &State) -> BTreeSet<String> {
    state
        .outcome_sources
        .keys()
        .filter(|s| !state.sources.contains_key(*s))
        .cloned()
        .collect()
}
pub(super) fn admit(state: &State, request: &Request, length: u64) -> Result<(u64, u64, bool)> {
    if matches!(
        request.command,
        Command::ReserveOutcomes { .. } | Command::ReleaseOutcomes { .. }
    ) && state
        .outcome_operations
        .contains_key(request.operation.as_str())
    {
        return Err(Error::Conflict);
    }
    match &request.command {
        Command::ReserveOutcomes { id, owner, plan } => {
            plan.validate(owner)?;
            if state.outcome_budgets.contains_key(id.as_str()) {
                return Err(Error::Conflict);
            }
            let close = operation("obf", &state.head.genesis, id)?;
            for key in plan
                .steps
                .iter()
                .map(|s| &s.operation)
                .chain(plan.admissions.iter().map(|a| &a.operation))
            {
                if state.outcomes.contains_key(key.as_str())
                    || state.outcome_operations.contains_key(key.as_str())
                    || *key == request.operation
                    || *key == close
                {
                    return Err(Error::Conflict);
                }
            }
            let sources = plan.sources();
            if sources
                .iter()
                .any(|s| state.outcome_sources.contains_key(s))
                || state
                    .progress_budgets
                    .values()
                    .any(|r| !r.closed && sources.contains(r.source.as_str()))
            {
                return Err(Error::Conflict);
            }
            for s in &sources {
                if let Some(found)=state.sources.get(s) {if found.owner != *owner {return Err(Error::Owner)}
                if found.cancelled{return Err(Error::Cancelled)}}
                else if !plan.steps.iter().any(|s2|matches!(&*s2.command,Command::Prepare{source,expected_revision:0,..} if source.as_str()==s)) {return Err(Error::Missing)}
            }
            Ok((
                (plan.steps.len() + plan.admissions.len()) as u64 + 1,
                (plan.steps.len() + plan.admissions.len()) as u64 * STEP_BYTES + SETTLEMENT_BYTES,
                false,
            ))
        }
        Command::ReleaseOutcomes { budget } => {
            let r = record(state, budget)?;
            if r.closed {
                return Err(Error::Conflict);
            }
            let empty = r.spent.is_empty()
                && r.plan
                    .admissions
                    .iter()
                    .all(|a| !state.outcomes.contains_key(a.operation.as_str()));
            let terminal = r.plan.endings.iter().any(|e| {
                e.iter()
                    .map(|k| k.as_str().to_owned())
                    .collect::<BTreeSet<_>>()
                    == r.spent
                        .iter()
                        .filter(|key| {
                            r.plan
                                .steps
                                .iter()
                                .any(|s| s.operation.as_str() == key.as_str())
                        })
                        .cloned()
                        .collect::<BTreeSet<_>>()
            });
            if !empty && !terminal {
                return Err(Error::Pending);
            }
            if length > SETTLEMENT_BYTES {
                return Err(Error::Budget);
            }
            Ok((0, 0, true))
        }
        _ => {
            if let Some(id) = state.outcome_operations.get(request.operation.as_str()) {
                let r = state.outcome_budgets.get(id).ok_or(Error::Corrupt)?;
                if r.closed {
                    return Err(Error::Conflict);
                }
                if let Some(step) = r
                    .plan
                    .steps
                    .iter()
                    .find(|s| s.operation == request.operation)
                {
                    if r.spent.contains(request.operation.as_str())
                        || *step.command != request.command
                    {
                        return Err(Error::Conflict);
                    }
                    if length > STEP_BYTES {
                        return Err(Error::Budget);
                    }
                    return Ok((0, 0, true));
                }
                let a = r
                    .plan
                    .admissions
                    .iter()
                    .find(|a| a.operation == request.operation)
                    .ok_or(Error::Corrupt)?;
                return match &request.command {
                    Command::Prepare {
                        source,
                        owner,
                        expected_revision,
                        ..
                    } if *source == a.source
                        && *owner == a.owner
                        && *expected_revision == a.revision =>
                    {
                        if length > STEP_BYTES {
                            Err(Error::Budget)
                        } else {
                            Ok((0, 0, true))
                        }
                    }
                    _ => Err(Error::Conflict),
                };
            }
            let source = match &request.command {
                Command::ReserveProgress { source, .. } => Some(source),
                Command::Cancel { .. } => None,
                _ => progress_budget::scope(&request.command).map(|(s, _, _)| s),
            };
            if source.is_some_and(|s| state.outcome_sources.contains_key(s.as_str())) {
                return Err(Error::Budget);
            }
            Ok((0, 0, false))
        }
    }
}

pub(super) fn transition(state: &mut State, request: &Request) -> Result<bool> {
    match &request.command {
        Command::ReserveOutcomes { id, owner, plan } => {
            let name = id.as_str().to_owned();
            for operation in plan
                .steps
                .iter()
                .map(|s| &s.operation)
                .chain(plan.admissions.iter().map(|a| &a.operation))
            {
                state
                    .outcome_operations
                    .insert(operation.as_str().into(), name.clone());
            }
            for source in plan.sources() {
                state.outcome_sources.insert(source, name.clone());
            }
            state.outcome_events = state
                .outcome_events
                .checked_add((plan.steps.len() + plan.admissions.len()) as u64 + 1)
                .ok_or(Error::Budget)?;
            state.outcome_bytes = state
                .outcome_bytes
                .checked_add(
                    (plan.steps.len() + plan.admissions.len()) as u64 * STEP_BYTES
                        + SETTLEMENT_BYTES,
                )
                .ok_or(Error::Budget)?;
            state.outcome_budgets.insert(
                name,
                Record {
                    owner: owner.clone(),
                    creation: request.operation.clone(),
                    plan: Arc::new(plan.clone()),
                    spent: BTreeSet::new(),
                    bytes: 0,
                    closed: false,
                },
            );
        }
        Command::ReleaseOutcomes { budget } => {
            let r = state
                .outcome_budgets
                .get_mut(budget.id.as_str())
                .ok_or(Error::Missing)?;
            state.outcome_events = state
                .outcome_events
                .checked_sub(
                    (r.plan.steps.len() + r.plan.admissions.len()) as u64 + 1
                        - r.spent.len() as u64,
                )
                .ok_or(Error::Corrupt)?;
            state.outcome_bytes = state
                .outcome_bytes
                .checked_sub(
                    (r.plan.steps.len() + r.plan.admissions.len()) as u64 * STEP_BYTES
                        + SETTLEMENT_BYTES
                        - r.bytes,
                )
                .ok_or(Error::Corrupt)?;
            for source in r.plan.sources() {
                state.outcome_sources.remove(&source);
            }
            r.closed = true;
        }
        _ => {
            if let Some(id) = state.outcome_operations.get(request.operation.as_str()) {
                let r = state.outcome_budgets.get_mut(id).ok_or(Error::Corrupt)?;
                if !r.closed {
                    let bytes = encode(request)?.len() as u64;
                    r.spent.insert(request.operation.as_str().into());
                    r.bytes += bytes;
                    state.outcome_events =
                        state.outcome_events.checked_sub(1).ok_or(Error::Corrupt)?;
                    state.outcome_bytes = state
                        .outcome_bytes
                        .checked_sub(bytes)
                        .ok_or(Error::Corrupt)?;
                }
            }
            return Ok(false);
        }
    }
    state.outcomes.insert(request.operation.as_str().into(), 0);
    Ok(true)
}

#[cfg(test)]
#[path = "outcome_budget_tests.rs"]
pub(crate) mod tests;
