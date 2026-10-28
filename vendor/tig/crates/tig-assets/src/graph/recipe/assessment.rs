//! Pure scoped catch-up assessment. No scheduling, approval or storage authority.
use super::{Plan, produced::*};
use crate::graph::{Result, require};
use std::collections::BTreeSet;
use tig_core::Digest;

/// Caller-selected output/variant key; no implicit variant expansion.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    pub asset_id: String,
    pub selector: Selector,
}

/// A failed lookup must not masquerade as confirmed absence.
pub enum Observation<'a, T> {
    Absent,
    Present(&'a T),
    Unavailable,
}

/// None for desired means unavailable, never an empty or successfully built plan.
pub struct Request<'a> {
    pub key: Key,
    pub desired: Option<&'a Plan>,
    pub current: Observation<'a, BoundSubject>,
    pub claim: Observation<'a, PlanCheckedClaim<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Unbuilt,
    Unproven,
    ClaimedRecipeMatch,
    StaleRecipe,
    Blocked,
}
impl State {
    /// Advice only: not admission to execute, publish, deliver or evict.
    pub fn catch_up_candidate(self) -> bool {
        matches!(self, Self::Unbuilt | Self::StaleRecipe)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: Key,
    pub state: State,
}

/// Private report for exactly the supplied scope, not complete project coverage.
pub struct Assessment {
    checkpoint: Digest,
    entries: Vec<Entry>,
}
impl Assessment {
    pub fn checkpoint(&self) -> &Digest {
        &self.checkpoint
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn complete_semantic_coverage(&self) -> bool {
        false
    }
}
impl std::fmt::Debug for Assessment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Assessment")
            .field("entries", &self.entries.len())
            .field(
                "catch_up_candidates",
                &self
                    .entries
                    .iter()
                    .filter(|e| e.state.catch_up_candidate())
                    .count(),
            )
            .finish_non_exhaustive()
    }
}

/// All plans must be evaluated at one exact checkpoint; failures stay blocked.
/// Checked claims must first be replayed/admitted by the caller against originals.
pub fn assess(checkpoint: &Digest, requests: &[Request<'_>], maximum: usize) -> Result<Assessment> {
    require(
        (1..=4096).contains(&maximum) && !requests.is_empty() && requests.len() <= maximum,
        "assessment entry budget",
    )?;
    let mut keys = BTreeSet::new();
    for request in requests {
        super::token(&request.key.asset_id)?;
        request.key.selector.validate()?;
        require(keys.insert(&request.key), "duplicate assessment key")?;
    }
    let mut entries = Vec::with_capacity(requests.len());
    for request in requests {
        let state = state(checkpoint, request);
        entries.push(Entry {
            key: request.key.clone(),
            state,
        });
    }
    entries.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(Assessment {
        checkpoint: checkpoint.clone(),
        entries,
    })
}

fn state(checkpoint: &Digest, request: &Request<'_>) -> State {
    let Some(desired) = request.desired else {
        return State::Blocked;
    };
    let current = match &request.current {
        Observation::Absent => None,
        Observation::Present(subject) => Some(*subject),
        Observation::Unavailable => return State::Blocked,
    };
    let claim = match &request.claim {
        Observation::Absent => None,
        Observation::Present(claim) => Some(*claim),
        Observation::Unavailable => return State::Blocked,
    };
    if desired.checkpoint() != checkpoint
        || current.is_some_and(|s| {
            s.subject().asset_id() != request.key.asset_id
                || s.subject().selector() != &request.key.selector
        })
    {
        return State::Blocked;
    }
    match compare(desired, current, claim) {
        Ok(Correspondence::Unbuilt) => State::Unbuilt,
        Ok(Correspondence::Unproven) => State::Unproven,
        Ok(Correspondence::ClaimedRecipeMatch) => State::ClaimedRecipeMatch,
        Ok(Correspondence::StaleRecipe) => State::StaleRecipe,
        Err(_) => State::Blocked,
    }
}
