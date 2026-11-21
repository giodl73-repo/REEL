//! Exact loss disclosure for review, never authorization or deletion authority.
use super::{Catalogue, ClockPoint, Policy, Preview};
use crate::{Digest, LogicalPath, Object, identity};
use serde::{Deserialize, Serialize};
pub mod decision;
pub mod eviction;
pub mod routine_policy;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_OBLIGATIONS: usize = 1024;
pub const MAX_STORES: usize = 16;
pub const MAX_CELLS: usize = 4096;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Kind {
    Ordinary,
    Job,
    Hard,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Obligation {
    pub claim: Digest,
    pub object: Object,
    pub owner: Digest,
    pub kind: Kind,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub claim: Digest,
    pub object: Object,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protection {
    pub checkpoint: Digest,
    pub complete: bool,
    pub obligations: Vec<Obligation>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum CopyState {
    Unknown,
    Absent { evidence: Digest },
    Verified { evidence: Digest },
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Copy {
    pub object: Object,
    pub store: Digest,
    pub state: CopyState,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub checkpoint: Digest,
    pub complete: bool,
    pub stores: BTreeSet<Digest>,
    pub copies: Vec<Copy>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Loss {
    Unknown,
    NoObservedSurvivor,
    ObservedSurvivors(BTreeSet<Digest>),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Hold {
    IncompleteProtection,
    Protected(BTreeSet<Digest>),
    UnknownCopies,
    NoVerifiedTargets,
    IncompleteDesired,
    DesiredAtTarget(BTreeSet<Digest>),
    UnverifiedDesired(BTreeSet<Digest>),
    NoObservedSurvivor,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObjectReview {
    object: Object,
    paths: BTreeSet<LogicalPath>,
    targets: BTreeSet<Digest>,
    loss: Loss,
    holds: Vec<Hold>,
}
impl ObjectReview {
    pub fn object(&self) -> &Object {
        &self.object
    }
    pub fn paths(&self) -> &BTreeSet<LogicalPath> {
        &self.paths
    }
    pub fn targets(&self) -> &BTreeSet<Digest> {
        &self.targets
    }
    pub fn loss(&self) -> &Loss {
        &self.loss
    }
    pub fn holds(&self) -> &[Hold] {
        &self.holds
    }
    /// Review eligibility only; no permission to release claims or remove bytes.
    pub fn eligible(&self) -> bool {
        self.holds.is_empty()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Age(super::Error),
    Budget,
    InvalidScope,
    ForbiddenRelease,
    Encoding,
    Blocked,
    Changed,
}
type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
enum Basis {
    Age { preview: Preview },
    Eviction { eviction: eviction::Selection },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Review {
    schema: &'static str,
    scope: &'static str,
    #[serde(flatten)]
    basis: Basis,
    owner: Digest,
    places: BTreeMap<String, Digest>,
    protection: Protection,
    inventory: Inventory,
    requested_releases: BTreeSet<Release>,
    objects: Vec<ObjectReview>,
    eligible_logical_object_bytes: u64,
    physical_reclaim: &'static str,
}
impl Review {
    /// Recomputes internal claims, never authenticates their source or authority.
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Record {
            schema: String,
            scope: String,
            #[serde(default)]
            preview: Option<serde_json::Value>,
            #[serde(default)]
            eviction: Option<serde_json::Value>,
            owner: Digest,
            places: BTreeMap<String, Digest>,
            protection: Protection,
            inventory: Inventory,
            requested_releases: BTreeSet<Release>,
            objects: serde_json::Value,
            eligible_logical_object_bytes: u64,
            physical_reclaim: String,
        }
        let value = identity::parse_json(bytes).map_err(|_| Error::Encoding)?;
        let age = match value["schema"].as_str() {
            Some("tig.retirement-review.design-1") if value.get("eviction").is_none() => true,
            Some("tig.retained-eviction-review.design-1") if value.get("preview").is_none() => {
                false
            }
            _ => return Err(Error::Encoding),
        };
        let record: Record = serde_json::from_value(value).map_err(|_| Error::Encoding)?;
        let (schema, scope) = if age {
            (
                "tig.retirement-review.design-1",
                "review-only-not-final-loss-authorization",
            )
        } else {
            (
                "tig.retained-eviction-review.design-1",
                "review-only-retained-eviction",
            )
        };
        if record.schema != schema
            || record.scope != scope
            || record.physical_reclaim != "unknown-provider-measurement-required"
        {
            return Err(Error::Encoding);
        }
        let basis = if age {
            Basis::Age {
                preview: Preview::from_json(
                    &identity::canonical_json(&record.preview.ok_or(Error::Encoding)?)
                        .map_err(|_| Error::Encoding)?,
                )
                .map_err(Error::Age)?,
            }
        } else {
            Basis::Eviction {
                eviction: eviction::Selection::from_json(
                    &identity::canonical_json(&record.eviction.ok_or(Error::Encoding)?)
                        .map_err(|_| Error::Encoding)?,
                )?,
            }
        };
        let checked = Self::derive(
            basis,
            record.owner,
            record.places,
            record.protection,
            record.inventory,
            record.requested_releases,
        )?;
        if checked.eligible_logical_object_bytes != record.eligible_logical_object_bytes
            || serde_json::to_value(&checked.objects).map_err(|_| Error::Encoding)?
                != record.objects
            || checked.to_json()? != bytes
        {
            return Err(Error::Encoding);
        }
        Ok(checked)
    }
    pub fn create(
        preview: Preview,
        owner: Digest,
        places: BTreeMap<String, Digest>,
        protection: Protection,
        inventory: Inventory,
        requested_releases: BTreeSet<Release>,
    ) -> Result<Self> {
        Self::derive(
            Basis::Age { preview },
            owner,
            places,
            protection,
            inventory,
            requested_releases,
        )
    }
    fn derive(
        basis: Basis,
        owner: Digest,
        places: BTreeMap<String, Digest>,
        mut protection: Protection,
        mut inventory: Inventory,
        requested_releases: BTreeSet<Release>,
    ) -> Result<Self> {
        if places.len() > 16
            || places.keys().any(|p| p.len() > 128)
            || protection.obligations.len() > MAX_OBLIGATIONS
            || requested_releases.len() > MAX_OBLIGATIONS
            || inventory.stores.len() > MAX_STORES
            || inventory.copies.len() > MAX_CELLS
        {
            return Err(Error::Budget);
        }
        let mut objects: BTreeMap<Object, BTreeSet<LogicalPath>> = BTreeMap::new();
        let aliases = match &basis {
            Basis::Age { preview } => {
                for (path, binding) in preview.qualified() {
                    objects
                        .entry(binding.object.clone())
                        .or_default()
                        .insert(path.clone());
                }
                &preview.policy.places
            }
            Basis::Eviction { eviction } => {
                for (path, object) in &eviction.bindings {
                    objects
                        .entry(object.clone())
                        .or_default()
                        .insert(path.clone());
                }
                &eviction.targets
            }
        };
        let target_stores: BTreeSet<_> = places.values().cloned().collect();
        if &places.keys().cloned().collect::<BTreeSet<_>>() != aliases
            || !target_stores.is_subset(&inventory.stores)
        {
            return Err(Error::InvalidScope);
        }
        protection.obligations.sort();
        inventory.copies.sort();
        let mut claims = BTreeMap::new();
        for obligation in &protection.obligations {
            if !objects.contains_key(&obligation.object)
                || claims
                    .insert(
                        Release {
                            claim: obligation.claim.clone(),
                            object: obligation.object.clone(),
                        },
                        obligation,
                    )
                    .is_some()
            {
                return Err(Error::InvalidScope);
            }
        }
        for release in &requested_releases {
            let claim = claims.get(release).ok_or(Error::InvalidScope)?;
            if claim.owner != owner || claim.kind != Kind::Ordinary {
                return Err(Error::ForbiddenRelease);
            }
        }
        let mut copies = BTreeMap::new();
        for copy in &inventory.copies {
            if !objects.contains_key(&copy.object)
                || !inventory.stores.contains(&copy.store)
                || copies
                    .insert((&copy.object, &copy.store), &copy.state)
                    .is_some()
            {
                return Err(Error::InvalidScope);
            }
        }
        let mut reviews = Vec::new();
        let mut logical_bytes = 0u64;
        for (object, paths) in objects {
            let mut holds = Vec::new();
            if !protection.complete {
                holds.push(Hold::IncompleteProtection);
            }
            if let Basis::Eviction { eviction } = &basis {
                if !eviction.desired.complete {
                    holds.push(Hold::IncompleteDesired);
                }
                let desired: BTreeSet<_> = eviction
                    .desired
                    .placements
                    .iter()
                    .filter(|p| p.object == object && target_stores.contains(&p.store))
                    .map(|p| p.store.clone())
                    .collect();
                if !desired.is_empty() {
                    holds.push(Hold::DesiredAtTarget(desired));
                }
                let unverified: BTreeSet<_> = eviction
                    .desired
                    .placements
                    .iter()
                    .filter(|p| {
                        p.object == object
                            && !matches!(
                                copies.get(&(&object, &p.store)).copied(),
                                Some(CopyState::Verified { .. })
                            )
                    })
                    .map(|p| p.store.clone())
                    .collect();
                if !unverified.is_empty() {
                    holds.push(Hold::UnverifiedDesired(unverified));
                }
            }
            let protected: BTreeSet<_> = protection
                .obligations
                .iter()
                .filter(|c| {
                    c.object == object
                        && !requested_releases.contains(&Release {
                            claim: c.claim.clone(),
                            object: object.clone(),
                        })
                })
                .map(|c| c.claim.clone())
                .collect();
            if !protected.is_empty() {
                holds.push(Hold::Protected(protected));
            }
            let mut unknown = !inventory.complete;
            let mut targets = BTreeSet::new();
            let mut survivors = BTreeSet::new();
            for store in &inventory.stores {
                match copies
                    .get(&(&object, store))
                    .copied()
                    .unwrap_or(&CopyState::Unknown)
                {
                    CopyState::Unknown => unknown = true,
                    CopyState::Absent { .. } => (),
                    CopyState::Verified { .. } if target_stores.contains(store) => {
                        targets.insert(store.clone());
                    }
                    CopyState::Verified { .. } => {
                        survivors.insert(store.clone());
                    }
                }
            }
            if unknown {
                holds.push(Hold::UnknownCopies);
            }
            if targets.is_empty() {
                holds.push(Hold::NoVerifiedTargets);
            }
            let loss = if !survivors.is_empty() {
                Loss::ObservedSurvivors(survivors)
            } else if unknown {
                Loss::Unknown
            } else {
                Loss::NoObservedSurvivor
            };
            if matches!(&basis, Basis::Eviction { .. })
                && !matches!(&loss, Loss::ObservedSurvivors(_))
            {
                holds.push(Hold::NoObservedSurvivor);
            }
            if holds.is_empty() {
                logical_bytes = logical_bytes
                    .checked_add(object.bytes)
                    .ok_or(Error::Budget)?;
            }
            reviews.push(ObjectReview {
                object,
                paths,
                targets,
                loss,
                holds,
            });
        }
        let result = Self {
            schema: if matches!(&basis, Basis::Age { .. }) {
                "tig.retirement-review.design-1"
            } else {
                "tig.retained-eviction-review.design-1"
            },
            scope: if matches!(&basis, Basis::Age { .. }) {
                "review-only-not-final-loss-authorization"
            } else {
                "review-only-retained-eviction"
            },
            basis,
            owner,
            places,
            protection,
            inventory,
            requested_releases,
            objects: reviews,
            eligible_logical_object_bytes: logical_bytes,
            physical_reclaim: "unknown-provider-measurement-required",
        };
        result.to_json()?;
        Ok(result)
    }
    pub fn objects(&self) -> &[ObjectReview] {
        &self.objects
    }
    pub fn owner(&self) -> &Digest {
        &self.owner
    }
    pub fn policy_identity(&self) -> &Digest {
        match &self.basis {
            Basis::Age { preview } => &preview.policy.identity,
            Basis::Eviction { eviction } => &eviction.policy,
        }
    }
    pub fn eligible_logical_object_bytes(&self) -> u64 {
        self.eligible_logical_object_bytes
    }
    pub fn is_routine_eviction(&self) -> bool {
        matches!(&self.basis, Basis::Eviction { .. })
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        identity::canonical_json(&serde_json::to_value(self).map_err(|_| Error::Encoding)?)
            .map_err(|_| Error::Encoding)
    }
    pub fn digest(&self) -> Result<Digest> {
        Ok(Digest::blob(&self.to_json()?))
    }
    pub fn revalidate(
        &self,
        catalogue: &Catalogue,
        policy: &Policy,
        at: ClockPoint,
        protection: Protection,
        inventory: Inventory,
    ) -> Result<()> {
        let Basis::Age { preview } = &self.basis else {
            return Err(Error::InvalidScope);
        };
        preview
            .revalidate(catalogue, policy, at)
            .map_err(Error::Age)?;
        let fresh = Self::create(
            preview.clone(),
            self.owner.clone(),
            self.places.clone(),
            protection,
            inventory,
            self.requested_releases.clone(),
        )?;
        self.validate_replacement(&fresh)
    }
    fn validate_replacement(&self, fresh: &Self) -> Result<()> {
        for old in self.objects.iter().filter(|o| o.eligible()) {
            let new = fresh
                .objects
                .iter()
                .find(|o| o.object == old.object)
                .ok_or(Error::Changed)?;
            if !new.eligible() {
                return Err(Error::Blocked);
            }
            if new.targets != old.targets || new.loss != old.loss {
                return Err(Error::Changed);
            }
        }
        Ok(())
    }
}
