//! Experimental pure identity, tree and claim model. No filesystem/provider IO.
#![forbid(unsafe_code)]
pub mod capacity;
pub mod identity;
mod path;
pub mod tree;
pub mod worktree;

pub use identity::Digest;
pub use path::LogicalPath;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Object {
    pub sha256: Digest,
    pub bytes: u64,
}

impl Object {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            sha256: Digest::blob(bytes),
            bytes: bytes.len() as u64,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Placement {
    pub object: Object,
    pub place: String,
}

/// Already assigned placements. Constraint solving and claim authority are external.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claim {
    pub id: String,
    pub placements: BTreeSet<Placement>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub generation: u64,
    pub complete_claim_coverage: bool,
    claims: BTreeMap<String, Claim>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Budget,
    InvalidIdentity,
    DuplicateClaim,
    AmbiguousTransition,
    UnknownRetirement,
    StaleGeneration,
    ConflictingSize,
    Overflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HoldReason {
    IncompleteClaimCoverage,
    NoRetainedObligation,
    RequiredCopyMissing,
}

/// Candidates are NOT removal authorization. Providers must revalidate protection,
/// authority, integrity, independent custody, freshness and object-scoped fencing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub generation: u64,
    pub ensure: BTreeSet<Placement>,
    pub retained: BTreeSet<Placement>,
    pub release_candidates: BTreeSet<Placement>,
    pub held: BTreeMap<Placement, HoldReason>,
    pub required_bytes_by_place: BTreeMap<String, u64>,
}

fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
}

fn check_sizes<'a>(objects: impl Iterator<Item = &'a Object>) -> Result<(), Error> {
    let mut sizes = BTreeMap::new();
    for o in objects {
        if sizes
            .insert(o.sha256.clone(), o.bytes)
            .is_some_and(|bytes| bytes != o.bytes)
        {
            return Err(Error::ConflictingSize);
        }
    }
    Ok(())
}

impl View {
    pub fn new(generation: u64, complete: bool, claims: Vec<Claim>) -> Result<Self, Error> {
        if claims.len() > 1024 {
            return Err(Error::Budget);
        }
        let mut indexed = BTreeMap::new();
        let mut count = 0usize;
        for claim in claims {
            if !token(&claim.id) || claim.placements.iter().any(|p| !token(&p.place)) {
                return Err(Error::InvalidIdentity);
            }
            count = count
                .checked_add(claim.placements.len())
                .ok_or(Error::Budget)?;
            if count > 65_536 {
                return Err(Error::Budget);
            }
            if indexed.insert(claim.id.clone(), claim).is_some() {
                return Err(Error::DuplicateClaim);
            }
        }
        check_sizes(
            indexed
                .values()
                .flat_map(|c| &c.placements)
                .map(|p| &p.object),
        )?;
        Ok(Self {
            generation,
            complete_claim_coverage: complete,
            claims: indexed,
        })
    }

    /// Publish a complete desired-state handoff in one pure projection. The caller
    /// must durably commit/fence it before retiring old protection in a real backend.
    /// Generation is a fixture snapshot fence, not a distributed/global cache lock.
    pub fn transition(
        &self,
        expected_generation: u64,
        put: Vec<Claim>,
        retire: Vec<String>,
    ) -> Result<Self, Error> {
        if expected_generation != self.generation {
            return Err(Error::StaleGeneration);
        }
        if put.len() > 1024 || retire.len() > 1024 {
            return Err(Error::Budget);
        }
        let mut touched = BTreeSet::new();
        for id in put.iter().map(|c| &c.id).chain(&retire) {
            if !touched.insert(id) {
                return Err(Error::AmbiguousTransition);
            }
        }
        let mut next = self.claims.clone();
        for id in retire {
            if next.remove(&id).is_none() {
                return Err(Error::UnknownRetirement);
            }
        }
        for claim in put {
            next.insert(claim.id.clone(), claim);
        }
        Self::new(
            self.generation.checked_add(1).ok_or(Error::Overflow)?,
            self.complete_claim_coverage,
            next.into_values().collect(),
        )
    }

    /// `verified` is a caller-supplied synthetic observation set, NOT proof from
    /// this crate. Nonpresent entries are unknown/missing; no provider is queried.
    pub fn plan(&self, verified: &BTreeSet<Placement>) -> Result<Plan, Error> {
        if verified.len() > 65_536 {
            return Err(Error::Budget);
        }
        let required: BTreeSet<_> = self
            .claims
            .values()
            .flat_map(|c| c.placements.iter().cloned())
            .collect();
        if verified.iter().any(|p| !token(&p.place)) {
            return Err(Error::InvalidIdentity);
        }
        check_sizes(required.iter().chain(verified).map(|p| &p.object))?;
        let mut bytes = BTreeMap::<String, u64>::new();
        let mut obligations = BTreeMap::<Object, Vec<&Placement>>::new();
        for p in &required {
            let total = bytes.entry(p.place.clone()).or_default();
            *total = total.checked_add(p.object.bytes).ok_or(Error::Overflow)?;
            obligations.entry(p.object.clone()).or_default().push(p);
        }
        let mut plan = Plan {
            generation: self.generation,
            ensure: required.difference(verified).cloned().collect(),
            retained: required.intersection(verified).cloned().collect(),
            release_candidates: BTreeSet::new(),
            held: BTreeMap::new(),
            required_bytes_by_place: bytes,
        };
        for surplus in verified.difference(&required) {
            let hold = if !self.complete_claim_coverage {
                Some(HoldReason::IncompleteClaimCoverage)
            } else {
                match obligations.get(&surplus.object) {
                    None => Some(HoldReason::NoRetainedObligation),
                    Some(targets) if targets.iter().any(|p| !verified.contains(*p)) => {
                        Some(HoldReason::RequiredCopyMissing)
                    }
                    Some(_) => None,
                }
            };
            if let Some(reason) = hold {
                plan.held.insert(surplus.clone(), reason);
            } else {
                plan.release_candidates.insert(surplus.clone());
            }
        }
        Ok(plan)
    }
}
