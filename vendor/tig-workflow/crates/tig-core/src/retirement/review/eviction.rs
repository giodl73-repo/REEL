//! Placement eligibility without a retirement clock. All facts are caller-admitted.
use super::{Error, Inventory, Protection, Release, Result, Review};
use crate::{Digest, LogicalPath, Object, identity};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const SCHEMA: &str = "tig.retained-eviction-selection.design-1";
const SCOPE: &str = "placement-only-not-removal-authority";

/// Exact desired placements for the selected objects, never observed custody.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Desired {
    pub checkpoint: Digest,
    pub complete: bool,
    pub placements: BTreeSet<super::decision::Target>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Selection {
    schema: &'static str,
    scope: &'static str,
    snapshot: Digest,
    pub(super) policy: Digest,
    pub(super) bindings: BTreeMap<LogicalPath, Object>,
    pub(super) targets: BTreeSet<String>,
    pub(super) desired: Desired,
}
impl Selection {
    pub fn new(
        snapshot: Digest,
        policy: Digest,
        bindings: BTreeMap<LogicalPath, Object>,
        targets: BTreeSet<String>,
        desired: Desired,
    ) -> Result<Self> {
        if bindings.is_empty()
            || bindings.len() > crate::retirement::MAX_SELECTED
            || targets.is_empty()
            || targets.len() > super::MAX_STORES
            || desired.placements.len() > super::MAX_CELLS
        {
            return Err(Error::Budget);
        }
        if targets.iter().any(|p| !crate::token(p)) {
            return Err(Error::InvalidScope);
        }
        let mut sizes = BTreeMap::new();
        for object in bindings.values() {
            if sizes
                .insert(object.sha256.clone(), object.bytes)
                .is_some_and(|old| old != object.bytes)
            {
                return Err(Error::InvalidScope);
            }
        }
        let objects: BTreeSet<_> = bindings.values().cloned().collect();
        if desired
            .placements
            .iter()
            .any(|p| !objects.contains(&p.object))
        {
            return Err(Error::InvalidScope);
        }
        let result = Self {
            schema: SCHEMA,
            scope: SCOPE,
            snapshot,
            policy,
            bindings,
            targets,
            desired,
        };
        result.to_json()?;
        Ok(result)
    }
    pub fn snapshot(&self) -> &Digest {
        &self.snapshot
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        identity::canonical_json(&serde_json::to_value(self).map_err(|_| Error::Encoding)?)
            .map_err(|_| Error::Encoding)
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Record {
            schema: String,
            scope: String,
            snapshot: Digest,
            policy: Digest,
            bindings: BTreeMap<LogicalPath, Object>,
            targets: BTreeSet<String>,
            desired: Desired,
        }
        let record: Record =
            serde_json::from_value(identity::parse_json(bytes).map_err(|_| Error::Encoding)?)
                .map_err(|_| Error::Encoding)?;
        if record.schema != SCHEMA || record.scope != SCOPE {
            return Err(Error::Encoding);
        }
        let result = Self::new(
            record.snapshot,
            record.policy,
            record.bindings,
            record.targets,
            record.desired,
        )?;
        if result.to_json()? != bytes {
            return Err(Error::Encoding);
        }
        Ok(result)
    }
    fn same_scope(&self, fresh: &Self) -> bool {
        self.snapshot == fresh.snapshot
            && self.policy == fresh.policy
            && self.bindings == fresh.bindings
            && self.targets == fresh.targets
    }
}

impl Review {
    /// A pure exact-placement review, never deletion permission or live custody.
    pub fn create_eviction(
        selection: Selection,
        owner: Digest,
        places: BTreeMap<String, Digest>,
        protection: Protection,
        inventory: Inventory,
        requested_releases: BTreeSet<Release>,
    ) -> Result<Self> {
        Self::derive(
            super::Basis::Eviction {
                eviction: selection,
            },
            owner,
            places,
            protection,
            inventory,
            requested_releases,
        )
    }
    /// New evidence cannot change the reviewed selection or widen its targets.
    pub fn revalidate_eviction(
        &self,
        selection: Selection,
        protection: Protection,
        inventory: Inventory,
    ) -> Result<()> {
        let super::Basis::Eviction { eviction } = &self.basis else {
            return Err(Error::InvalidScope);
        };
        if !eviction.same_scope(&selection) {
            return Err(Error::Changed);
        }
        let fresh = Self::create_eviction(
            selection,
            self.owner.clone(),
            self.places.clone(),
            protection,
            inventory,
            self.requested_releases.clone(),
        )?;
        self.validate_replacement(&fresh)
    }
}
