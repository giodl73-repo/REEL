//! Durable logical headroom for the four progress writes and their settlement.
use super::*;
use std::collections::BTreeSet;
pub const EVENTS: u64 = 5;
pub const COMMAND_BYTES: u64 = 4096;
pub const SETTLEMENT_BYTES: u64 = 1024;
pub const BYTES: u64 = 4 * COMMAND_BYTES + SETTLEMENT_BYTES;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetRef {
    journal: Digest,
    id: Key,
    owner: Key,
    creation: Key,
}
impl BudgetRef {
    pub fn id(&self) -> &Key {
        &self.id
    }
    pub fn journal(&self) -> &Digest {
        &self.journal
    }
    pub fn owner(&self) -> &Key {
        &self.owner
    }
    pub fn wrap(&self, command: Command) -> Command {
        Command::ProgressBudgeted {
            budget: self.clone(),
            command: Box::new(command),
        }
    }
}
#[derive(Clone)]
pub(super) struct Record {
    pub owner: Key,
    pub source: Key,
    pub first: u64,
    creation: Key,
    pub spent: u64,
    pub spent_bytes: u64,
    pub closed: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capacity {
    pub available_events: u64,
    pub available_bytes: u64,
    pub held_events: u64,
    pub held_bytes: u64,
}
pub fn capacity(j: &mut SourceJournal) -> Result<Capacity> {
    j.sync()?;
    let (events, bytes) = held(&j.state)?;
    let cancels = j.state.sources.values().filter(|s| s.cancelled).count() as u64;
    Ok(Capacity {
        available_events: MAX_COMMANDS
            .checked_sub(
                j.state.head.sequence - cancels + events + outcome_budget::held(&j.state)?.0,
            )
            .ok_or(Error::Corrupt)?,
        available_bytes: MAX_COMMAND_BYTES
            .checked_sub(
                j.state.bytes - j.state.cancel_bytes + bytes + outcome_budget::held(&j.state)?.1,
            )
            .ok_or(Error::Corrupt)?,
        held_events: events + outcome_budget::held(&j.state)?.0,
        held_bytes: bytes + outcome_budget::held(&j.state)?.1,
    })
}
pub fn reserve(
    j: &mut SourceJournal,
    id: Key,
    owner: Key,
    source: Key,
    first: u64,
    guards: Vec<Guard>,
) -> Result<BudgetRef> {
    reserve_mode(j, id, owner, source, first, guards, false)
}
#[allow(clippy::too_many_arguments)]
pub(super) fn reserve_mode(
    j: &mut SourceJournal,
    id: Key,
    owner: Key,
    source: Key,
    first: u64,
    guards: Vec<Guard>,
    authority: bool,
) -> Result<BudgetRef> {
    let creation = operation("pbr", j.identity(), &id)?;
    let result = j.apply_once(
        creation.clone(),
        Command::ReserveProgress {
            id: id.clone(),
            owner: owner.clone(),
            source,
            first_revision: first,
            guards,
        },
        |_| Ok(()),
        authority,
    );
    j.mark_uncertain(&result);
    result?;
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
pub fn release(j: &mut SourceJournal, reference: &BudgetRef) -> Result<()> {
    release_mode(j, reference, false)
}
pub(super) fn release_mode(
    j: &mut SourceJournal,
    reference: &BudgetRef,
    authority: bool,
) -> Result<()> {
    j.sync()?;
    if record(&j.state, reference)?.closed {
        return Ok(());
    }
    let result = j.apply_once(
        operation("pbf", j.identity(), &reference.id)?,
        Command::ReleaseProgress {
            budget: reference.clone(),
        },
        |_| Ok(()),
        authority,
    );
    j.mark_uncertain(&result);
    result?;
    Ok(())
}
fn operation(prefix: &str, journal: &Digest, id: &Key) -> Result<Key> {
    Key::parse(&format!(
        "{prefix}-{}",
        Digest::blob(&encode(&(journal, id))?).as_str()
    ))
    .map_err(|_| Error::InvalidReference)
}
pub(super) fn held(state: &State) -> Result<(u64, u64)> {
    state
        .progress_budgets
        .values()
        .filter(|r| !r.closed)
        .try_fold((0u64, 0u64), |(e, b), r| {
            Ok((
                e.checked_add(EVENTS.checked_sub(r.spent).ok_or(Error::Corrupt)?)
                    .ok_or(Error::Corrupt)?,
                b.checked_add(BYTES.checked_sub(r.spent_bytes).ok_or(Error::Corrupt)?)
                    .ok_or(Error::Corrupt)?,
            ))
        })
}
fn record<'a>(state: &'a State, reference: &BudgetRef) -> Result<&'a Record> {
    if reference.journal != state.head.genesis {
        return Err(Error::InvalidReference);
    }
    let r = state
        .progress_budgets
        .get(reference.id.as_str())
        .ok_or(Error::Missing)?;
    if r.owner != reference.owner {
        return Err(Error::Owner);
    }
    if r.creation != reference.creation {
        return Err(Error::InvalidReference);
    }
    Ok(r)
}
/// Existing bounded reservation only; cannot create or renew a budget. Used by
/// provider-neutral recovery when metadata publication failed before Begin.
pub(super) fn reservation(
    j: &mut SourceJournal,
    id: &Key,
) -> Result<Option<(BudgetRef, bool, u64)>> {
    j.sync()?;
    Ok(j.state.progress_budgets.get(id.as_str()).map(|record| {
        (
            BudgetRef {
                journal: j.identity().clone(),
                id: id.clone(),
                owner: record.owner.clone(),
                creation: record.creation.clone(),
            },
            record.closed,
            record.spent,
        )
    }))
}
pub(super) fn scope(command: &Command) -> Option<(&Key, &Key, u64)> {
    match command {
        Command::Prepare {
            source,
            owner,
            expected_revision,
            ..
        }
        | Command::Adopt {
            source,
            owner,
            expected_revision,
            ..
        }
        | Command::AdoptGuarded {
            source,
            owner,
            expected_revision,
            ..
        }
        | Command::Abort {
            source,
            owner,
            expected_revision,
            ..
        }
        | Command::Retire {
            source,
            owner,
            expected_revision,
            ..
        }
        | Command::RetireGuarded {
            source,
            owner,
            expected_revision,
            ..
        }
        | Command::Withdraw {
            source,
            owner,
            expected_revision,
        }
        | Command::Cancel {
            source,
            owner,
            expected_revision,
        } => Some((source, owner, *expected_revision)),
        _ => None,
    }
}
pub(super) fn base(command: &Command) -> Result<&Command> {
    if let Command::ProgressBudgeted { command, .. } = command {
        if !matches!(
            **command,
            Command::Prepare { .. } | Command::Adopt { .. } | Command::AdoptGuarded { .. }
        ) {
            return Err(Error::InvalidReference);
        }
        Ok(command)
    } else {
        Ok(command)
    }
}
/// Extra reservation demand and whether this command spends its own held room.
pub(super) fn admit(state: &State, request: &Request, length: u64) -> Result<(u64, u64, bool)> {
    let command = base(&request.command)?;
    let mut demand = (0, 0, false);
    match &request.command {
        Command::ReserveProgress {
            id,
            owner,
            source,
            first_revision,
            guards,
        } => {
            if state.progress_budgets.contains_key(id.as_str()) {
                return Err(Error::Conflict);
            }
            let last = first_revision.checked_add(3).ok_or(Error::Budget)?;
            if let Some(s) = state.sources.get(source.as_str()) {
                if s.owner != *owner {
                    return Err(Error::Owner);
                }
                if s.cancelled {
                    return Err(Error::Cancelled);
                }
                if s.revision != *first_revision {
                    return Err(Error::Stale);
                }
            } else if *first_revision != 0 {
                return Err(Error::Stale);
            }
            for r in state
                .progress_budgets
                .values()
                .filter(|r| !r.closed && r.source == *source)
            {
                if *first_revision <= r.first + 3 && r.first <= last {
                    return Err(Error::Conflict);
                }
            }
            for g in guards {
                let s = state.sources.get(g.source.as_str()).ok_or(Error::Missing)?;
                if s.owner != g.owner {
                    return Err(Error::Owner);
                }
                if s.cancelled {
                    return Err(Error::Cancelled);
                }
                if s.revision != g.expected_revision {
                    return Err(Error::Stale);
                }
                if s.pending.is_some() {
                    return Err(Error::Pending);
                }
                if s.current.is_none() {
                    return Err(Error::Missing);
                }
            }
            demand = (EVENTS, BYTES, false);
        }
        Command::ProgressBudgeted { budget, .. } => {
            let r = record(state, budget)?;
            if r.closed {
                return Err(Error::Conflict);
            }
            let (source, owner, revision) = scope(command).ok_or(Error::InvalidReference)?;
            if *source != r.source || *owner != r.owner {
                return Err(Error::Owner);
            }
            let valid = match r.spent {
                0 | 2 => matches!(command, Command::Prepare { .. }),
                1 => matches!(command, Command::AdoptGuarded { .. }),
                3 => matches!(command, Command::Adopt { .. }),
                _ => false,
            };
            if !valid || revision != r.first + r.spent {
                return Err(Error::InvalidReference);
            }
            if length > COMMAND_BYTES || r.spent_bytes + length + SETTLEMENT_BYTES > BYTES {
                return Err(Error::Budget);
            }
            demand.2 = true;
        }
        Command::ReleaseProgress { budget } => {
            let r = record(state, budget)?;
            if r.closed {
                return Err(Error::Conflict);
            }
            if r.spent == 2 || r.spent == 3 {
                return Err(Error::Pending);
            }
            if length > SETTLEMENT_BYTES || r.spent_bytes + length > BYTES {
                return Err(Error::Budget);
            }
            demand.2 = true;
        }
        _ => {
            if let Some((source, _, expected)) = scope(command)
                && !matches!(command, Command::Cancel { .. })
                && state.progress_budgets.values().any(|r| {
                    !r.closed
                        && r.source == *source
                        && expected >= r.first
                        && expected <= r.first + 3
                })
            {
                return Err(Error::Budget);
            }
        }
    }
    let mut missing = missing_sources(state);
    if let Command::ReserveOutcomes { plan, .. } = &request.command {
        missing.extend(
            plan.sources()
                .into_iter()
                .filter(|s| !state.sources.contains_key(s)),
        );
    }
    let mut slots = state.sources.len() + missing.len();
    if let Command::ReserveProgress { source, .. } = &request.command {
        if !state.sources.contains_key(source.as_str()) && !missing.contains(source.as_str()) {
            slots += 1;
        }
    } else if let Some((source, owner, _)) = scope(command)
        && !state.sources.contains_key(source.as_str())
    {
        if !missing.contains(source.as_str()) {
            slots += 1;
        } else if state
            .progress_budgets
            .values()
            .any(|r| !r.closed && r.source == *source && r.owner != *owner)
        {
            return Err(Error::Owner);
        }
    }
    if slots > MAX_SOURCES {
        return Err(Error::Budget);
    }
    Ok(demand)
}
pub(super) fn transition(state: &mut State, request: &Request) -> Result<bool> {
    match &request.command {
        Command::ReserveProgress {
            id,
            owner,
            source,
            first_revision,
            ..
        } => {
            state.progress_budgets.insert(
                id.as_str().into(),
                Record {
                    owner: owner.clone(),
                    source: source.clone(),
                    first: *first_revision,
                    creation: request.operation.clone(),
                    spent: 0,
                    spent_bytes: 0,
                    closed: false,
                },
            );
        }
        Command::ReleaseProgress { budget } => {
            state
                .progress_budgets
                .get_mut(budget.id.as_str())
                .ok_or(Error::Missing)?
                .closed = true;
        }
        Command::ProgressBudgeted { budget, .. } => {
            let r = state
                .progress_budgets
                .get_mut(budget.id.as_str())
                .ok_or(Error::Missing)?;
            r.spent += 1;
            r.spent_bytes += encode(request)?.len() as u64;
            return Ok(false);
        }
        _ => return Ok(false),
    }
    state.outcomes.insert(request.operation.as_str().into(), 0);
    Ok(true)
}

#[cfg(test)]
#[path = "progress_budget_tests.rs"]
mod tests;

// Shared by admission and inspection; a source reserved by both pools counts once.
pub(super) fn missing_sources(state: &State) -> BTreeSet<String> {
    let mut missing = state
        .progress_budgets
        .values()
        .filter(|r| !r.closed && !state.sources.contains_key(r.source.as_str()))
        .map(|r| r.source.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    missing.extend(outcome_budget::missing_sources(state));
    missing
}
