//! Classify checked immutable history into existing claims. No IO or authority.
use super::lifecycle::Inspection;
use std::collections::{BTreeMap, BTreeSet};
use tig_assets::revisions::Coverage;
use tig_core::{Claim, Digest, Object, Placement, View};
pub mod archive;
pub mod handoff;
pub mod journal;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unknown,
    Budget,
    Conflict,
    Draft,
    Policy,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

/// Trusted adapter must bound acquisition and resolve the complete frozen closure.
/// Unknown/unsupported is an error; an empty set explicitly admits a leaf witness.
pub trait Witnesses {
    fn expand(&mut self, witness: &Object, maximum: usize) -> Result<BTreeSet<Object>, Error>;
}
pub struct Policy {
    pub generation: Digest,
    pub working: BTreeSet<String>,
    pub retained: BTreeSet<String>,
    pub maximum_objects: usize,
    pub maximum_logical_bytes: u64,
}
/// Private inventory, not a public scorecard, custody receipt or durable claim.
pub struct Demand {
    source_checkpoint: Digest,
    record: Object,
    policy: Digest,
    claim: Claim,
    working: BTreeSet<Object>,
    retained: BTreeSet<Object>,
    working_places: BTreeSet<String>,
    retained_places: BTreeSet<String>,
}
impl Demand {
    pub fn source_checkpoint(&self) -> &Digest {
        &self.source_checkpoint
    }
    pub fn record(&self) -> &Object {
        &self.record
    }
    pub fn policy(&self) -> &Digest {
        &self.policy
    }
    pub fn claim(&self) -> &Claim {
        &self.claim
    }
    pub fn working(&self) -> &BTreeSet<Object> {
        &self.working
    }
    pub fn retained(&self) -> &BTreeSet<Object> {
        &self.retained
    }
    pub fn complete_claim_coverage(&self) -> bool {
        false
    }
    pub fn authorizes_eviction(&self) -> bool {
        false
    }
}
struct Budget<'a> {
    policy: &'a Policy,
    sizes: BTreeMap<Digest, u64>,
    bytes: u64,
    expanded_references: usize,
}
impl Budget<'_> {
    fn add(&mut self, object: &Object) -> Result<(), Error> {
        if let Some(bytes) = self.sizes.get(&object.sha256) {
            return if *bytes == object.bytes {
                Ok(())
            } else {
                Err(Error::Conflict)
            };
        }
        self.bytes = self.bytes.checked_add(object.bytes).ok_or(Error::Budget)?;
        if self.sizes.len() >= self.policy.maximum_objects
            || self.bytes > self.policy.maximum_logical_bytes
        {
            return Err(Error::Budget);
        }
        self.sizes.insert(object.sha256.clone(), object.bytes);
        Ok(())
    }
}
impl Inspection {
    /// Policy/draft admission and witness adapter trust are caller responsibilities.
    /// Result remains scoped desired state, never current protection or custody.
    pub fn placement(
        &self,
        claim_id: &str,
        policy: &Policy,
        authorized_drafts: &BTreeSet<Object>,
        resolver: &mut impl Witnesses,
    ) -> Result<Demand, Error> {
        if policy.maximum_objects == 0
            || policy.maximum_objects > 65_536
            || policy.working.is_empty()
            || policy.retained.is_empty()
            || policy.working.len() > 16
            || policy.retained.len() > 16
        {
            return Err(Error::Policy);
        }
        let history = self.revisions().ok_or(Error::Unknown)?;
        if history.coverage() != Coverage::Complete
            || (self.selected_subject().is_some() && history.selected_revision().is_none())
            || self
                .delivery()
                .is_some_and(|h| h.coverage() == Some(Coverage::Partial))
        {
            return Err(Error::Unknown);
        }
        if authorized_drafts.len() > policy.maximum_objects {
            return Err(Error::Budget);
        }
        let known: BTreeSet<_> = history.entries().map(|e| e.revision.clone()).collect();
        if !authorized_drafts.is_subset(&known) {
            return Err(Error::Draft);
        }
        let summary = self.summary();
        let places = policy
            .working
            .union(&policy.retained)
            .map(|place| Placement {
                object: summary.record.clone(),
                place: place.clone(),
            })
            .collect();
        View::new(
            0,
            false,
            vec![Claim {
                id: claim_id.into(),
                placements: places,
            }],
        )
        .map_err(|_| Error::Policy)?;
        let mut working = BTreeSet::from([
            summary.record.clone(),
            summary.definition.clone(),
            summary.authored_schema.clone(),
        ]);
        let mut retained = working.clone();
        let mut budget = Budget {
            policy,
            sizes: BTreeMap::new(),
            bytes: 0,
            expanded_references: 0,
        };
        for object in &retained {
            budget.add(object)?;
        }
        for object in history.documents() {
            budget.add(object)?;
            retained.insert(object.clone());
        }
        let mut expansions = BTreeMap::new();
        for entry in history.entries() {
            let active = history.selected_revision() == Some(entry.revision)
                || authorized_drafts.contains(entry.revision);
            if active {
                working.insert(entry.revision.clone());
            }
            for object in entry.payloads {
                budget.add(object)?;
                retained.insert(object.clone());
                if active {
                    working.insert(object.clone());
                }
            }
            for witness in entry.witnesses {
                expand(witness, &mut expansions, resolver, &mut budget)?;
                for object in &expansions[witness] {
                    retained.insert(object.clone());
                    if active {
                        working.insert(object.clone());
                    }
                }
            }
        }
        if let Some(delivery) = self.delivery() {
            for object in delivery.metadata().iter().chain(delivery.receipts()) {
                budget.add(object)?;
                retained.insert(object.clone());
            }
            for witness in delivery.opaque() {
                expand(witness, &mut expansions, resolver, &mut budget)?;
                retained.extend(expansions[witness].iter().cloned());
            }
        }
        // Working is a subset of retained. Bound placement expansion before allocation.
        let count = retained.len() * policy.retained.len()
            + working.len() * policy.working.difference(&policy.retained).count();
        if count > 65_536 {
            return Err(Error::Budget);
        }
        let placements = working
            .iter()
            .flat_map(|object| {
                policy.working.iter().map(move |place| Placement {
                    object: object.clone(),
                    place: place.clone(),
                })
            })
            .chain(retained.iter().flat_map(|object| {
                policy.retained.iter().map(move |place| Placement {
                    object: object.clone(),
                    place: place.clone(),
                })
            }))
            .collect();
        let claim = Claim {
            id: claim_id.into(),
            placements,
        };
        // Existing planner owns identity/placement count and size validation.
        View::new(0, false, vec![claim.clone()]).map_err(|e| match e {
            tig_core::Error::Budget => Error::Budget,
            _ => Error::Policy,
        })?;
        Ok(Demand {
            source_checkpoint: summary.source_checkpoint.clone(),
            record: summary.record.clone(),
            policy: policy.generation.clone(),
            claim,
            working,
            retained,
            working_places: policy.working.clone(),
            retained_places: policy.retained.clone(),
        })
    }
}
fn expand(
    witness: &Object,
    cache: &mut BTreeMap<Object, BTreeSet<Object>>,
    resolver: &mut impl Witnesses,
    budget: &mut Budget<'_>,
) -> Result<(), Error> {
    budget.add(witness)?;
    if !cache.contains_key(witness) {
        let maximum = budget
            .policy
            .maximum_objects
            .min(65_536 - budget.expanded_references);
        let mut closure = resolver.expand(witness, maximum)?;
        if closure.len() > maximum {
            return Err(Error::Budget);
        }
        closure.insert(witness.clone());
        budget.expanded_references = budget
            .expanded_references
            .checked_add(closure.len())
            .filter(|count| *count <= 65_536)
            .ok_or(Error::Budget)?;
        for object in &closure {
            budget.add(object)?;
        }
        cache.insert(witness.clone(), closure);
    }
    Ok(())
}
