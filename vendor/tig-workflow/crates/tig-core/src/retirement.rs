//! Bounded age-selection previews. No removal authority, provider IO or expiry.
use crate::{Digest, LogicalPath, Object, identity};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub mod history;
pub mod review;

pub const MAX_BINDINGS: usize = 4096;
pub const MAX_SELECTED: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClockPoint {
    pub era: Digest,
    pub tick: u64,
}
/// Caller-admitted evidence of a continuous interval, not two timestamp samples.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InactiveInterval {
    pub clock: Digest,
    pub identity: Digest,
    pub started: u64,
    pub through: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Activity {
    Unknown,
    Active,
    Inactive(InactiveInterval),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub object: Object,
    pub activity: Activity,
}
#[derive(Clone, Debug)]
pub struct Catalogue {
    pub snapshot: Digest,
    pub bindings: BTreeMap<LogicalPath, Binding>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub identity: Digest,
    pub minimum_inactive_ticks: u64,
    pub places: BTreeSet<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Exclusion {
    UnknownActivity,
    Active,
    ClockMismatch,
    InvalidInterval,
    IncompleteInterval,
    TooYoung,
    UnselectedBinding,
    SharedIneligibleBinding,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedBinding {
    pub object: Object,
    pub interval: InactiveInterval,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Budget,
    InvalidPolicy,
    MissingPath,
    ConflictingSize,
    Overflow,
    Encoding,
    SourceChanged,
    PolicyChanged,
    ClockChanged,
    EligibilityChanged,
}
type Result<T> = std::result::Result<T, Error>;

/// Private fields keep the serialized, hashed artifact immutable after checking.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Preview {
    schema: &'static str,
    scope: &'static str,
    snapshot: Digest,
    policy: Policy,
    at: ClockPoint,
    resolved: BTreeMap<LogicalPath, Object>,
    qualified: BTreeMap<LogicalPath, QualifiedBinding>,
    excluded: BTreeMap<LogicalPath, Exclusion>,
    logical_object_bytes: u64,
}
fn exclusion(activity: &Activity, at: &ClockPoint, minimum: u64) -> Option<Exclusion> {
    match activity {
        Activity::Unknown => Some(Exclusion::UnknownActivity),
        Activity::Active => Some(Exclusion::Active),
        Activity::Inactive(i) if i.clock != at.era => Some(Exclusion::ClockMismatch),
        Activity::Inactive(i) if i.started > i.through || i.through > at.tick => {
            Some(Exclusion::InvalidInterval)
        }
        Activity::Inactive(i) if i.through != at.tick => Some(Exclusion::IncompleteInterval),
        Activity::Inactive(i) if at.tick - i.started < minimum => Some(Exclusion::TooYoung),
        Activity::Inactive(_) => None,
    }
}
impl Preview {
    /// Restores internal assertions only; source completeness/authenticity is external.
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Record {
            schema: String,
            scope: String,
            snapshot: Digest,
            policy: Policy,
            at: ClockPoint,
            resolved: BTreeMap<LogicalPath, Object>,
            qualified: BTreeMap<LogicalPath, QualifiedBinding>,
            excluded: BTreeMap<LogicalPath, Exclusion>,
            logical_object_bytes: u64,
        }
        let value = identity::parse_json(bytes).map_err(|_| Error::Encoding)?;
        let record: Record = serde_json::from_value(value).map_err(|_| Error::Encoding)?;
        if record.schema != "tig.retirement-preview.design-1"
            || record.scope != "age-only-not-removal-authority"
        {
            return Err(Error::Encoding);
        }
        let catalogue = Catalogue {
            snapshot: record.snapshot,
            bindings: record
                .resolved
                .iter()
                .map(|(p, o)| {
                    (
                        p.clone(),
                        Binding {
                            object: o.clone(),
                            activity: record.qualified.get(p).map_or(Activity::Unknown, |b| {
                                Activity::Inactive(b.interval.clone())
                            }),
                        },
                    )
                })
                .collect(),
        };
        let mut checked = Self::create(
            &catalogue,
            record.resolved.keys().cloned().collect(),
            record.policy,
            record.at,
        )?;
        if checked.qualified != record.qualified
            || checked.excluded.keys().ne(record.excluded.keys())
            || checked.logical_object_bytes != record.logical_object_bytes
        {
            return Err(Error::Encoding);
        }
        checked.excluded = record.excluded;
        if checked.to_json()? != bytes {
            return Err(Error::Encoding);
        }
        Ok(checked)
    }
    pub fn create(
        catalogue: &Catalogue,
        selected: BTreeSet<LogicalPath>,
        policy: Policy,
        at: ClockPoint,
    ) -> Result<Self> {
        if catalogue.bindings.len() > MAX_BINDINGS
            || selected.is_empty()
            || selected.len() > MAX_SELECTED
            || policy.places.len() > 16
        {
            return Err(Error::Budget);
        }
        if policy.minimum_inactive_ticks == 0
            || policy.places.is_empty()
            || policy.places.iter().any(|p| !crate::token(p))
        {
            return Err(Error::InvalidPolicy);
        }
        crate::check_sizes(catalogue.bindings.values().map(|b| &b.object))
            .map_err(|_| Error::ConflictingSize)?;
        let mut qualified = BTreeMap::new();
        let mut excluded = BTreeMap::new();
        for path in &selected {
            let binding = catalogue.bindings.get(path).ok_or(Error::MissingPath)?;
            if let Some(reason) = exclusion(&binding.activity, &at, policy.minimum_inactive_ticks) {
                excluded.insert(path.clone(), reason);
            } else if let Activity::Inactive(interval) = &binding.activity {
                qualified.insert(
                    path.clone(),
                    QualifiedBinding {
                        object: binding.object.clone(),
                        interval: interval.clone(),
                    },
                );
            }
        }
        // An object is indivisible here: no selected eligible alias may mask
        // another known alias that is unselected or ineligible.
        let unselected: BTreeSet<_> = catalogue
            .bindings
            .iter()
            .filter(|(p, _)| !selected.contains(*p))
            .map(|(_, b)| &b.object)
            .collect();
        let ineligible: BTreeSet<_> = excluded
            .keys()
            .map(|p| &catalogue.bindings[p].object)
            .collect();
        let held: Vec<_> = qualified
            .iter()
            .filter_map(|(p, b)| {
                if unselected.contains(&b.object) {
                    Some((p.clone(), Exclusion::UnselectedBinding))
                } else if ineligible.contains(&b.object) {
                    Some((p.clone(), Exclusion::SharedIneligibleBinding))
                } else {
                    None
                }
            })
            .collect();
        for (path, reason) in held {
            qualified.remove(&path);
            excluded.insert(path, reason);
        }
        let objects: BTreeSet<_> = qualified.values().map(|b| &b.object).collect();
        let mut logical_object_bytes = 0u64;
        for object in objects {
            logical_object_bytes = logical_object_bytes
                .checked_add(object.bytes)
                .ok_or(Error::Overflow)?;
        }
        let result = Self {
            schema: "tig.retirement-preview.design-1",
            scope: "age-only-not-removal-authority",
            snapshot: catalogue.snapshot.clone(),
            policy,
            at,
            resolved: selected
                .iter()
                .map(|p| (p.clone(), catalogue.bindings[p].object.clone()))
                .collect(),
            qualified,
            excluded,
            logical_object_bytes,
        };
        result.to_json()?; // Refuse over-budget or noncanonical numeric inputs now.
        Ok(result)
    }
    pub fn qualified(&self) -> &BTreeMap<LogicalPath, QualifiedBinding> {
        &self.qualified
    }
    pub fn excluded(&self) -> &BTreeMap<LogicalPath, Exclusion> {
        &self.excluded
    }
    pub fn logical_object_bytes(&self) -> u64 {
        self.logical_object_bytes
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        identity::canonical_json(&serde_json::to_value(self).map_err(|_| Error::Encoding)?)
            .map_err(|_| Error::Encoding)
    }
    pub fn digest(&self) -> Result<Digest> {
        Ok(Digest::blob(&self.to_json()?))
    }
    /// Checks the ORIGINAL qualified block only. Newly eligible excluded paths
    /// never expand it. Success is still not deletion authority.
    pub fn revalidate(&self, catalogue: &Catalogue, policy: &Policy, at: ClockPoint) -> Result<()> {
        if catalogue.snapshot != self.snapshot {
            return Err(Error::SourceChanged);
        }
        if policy != &self.policy {
            return Err(Error::PolicyChanged);
        }
        if at.era != self.at.era || at.tick < self.at.tick {
            return Err(Error::ClockChanged);
        }
        let fresh = Self::create(
            catalogue,
            self.resolved.keys().cloned().collect(),
            policy.clone(),
            at,
        )?;
        if fresh.resolved != self.resolved {
            return Err(Error::EligibilityChanged);
        }
        for (path, old) in &self.qualified {
            let current = fresh.qualified.get(path).ok_or(Error::EligibilityChanged)?;
            if current.object != old.object
                || current.interval.identity != old.interval.identity
                || current.interval.clock != old.interval.clock
                || current.interval.started != old.interval.started
            {
                return Err(Error::EligibilityChanged);
            }
        }
        Ok(())
    }
}
