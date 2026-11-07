//! Immutable desired assignments through existing metadata ports, not protection.
use super::Demand;
use crate::source::native::{
    Error, Result,
    archive::{put, read, tree_error},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use tig_core::{
    Claim, Digest, LogicalPath, Object, Placement, View, identity,
    tree::{NodeStore, Tree, TreeLimits},
};
use tig_store::source_claims::Selection;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: String,
    source_checkpoint: Digest,
    record: Object,
    policy: Digest,
    claim: String,
    working_places: BTreeSet<String>,
    retained_places: BTreeSet<String>,
    working: usize,
    retained: usize,
}
fn path(value: &str) -> Result<LogicalPath> {
    LogicalPath::parse(value).map_err(|_| Error::Invalid)
}
fn canonical(header: &Header) -> Result<Vec<u8>> {
    identity::canonical_json(&serde_json::to_value(header).map_err(|_| Error::Metadata)?)
        .map_err(|_| Error::Metadata)
}
impl Demand {
    /// Candidate root only. Provider finish and independent readback remain required.
    pub fn archive(&self, store: &mut impl NodeStore, limits: TreeLimits) -> Result<Selection> {
        if self
            .retained
            .len()
            .checked_add(1)
            .is_none_or(|n| n > limits.max_entries)
        {
            return Err(Error::Budget);
        }
        let header = Header {
            schema: "tig.native-placement.experimental-1".into(),
            source_checkpoint: self.source_checkpoint.clone(),
            record: self.record.clone(),
            policy: self.policy.clone(),
            claim: self.claim.id.clone(),
            working_places: self.working_places.clone(),
            retained_places: self.retained_places.clone(),
            working: self.working.len(),
            retained: self.retained.len(),
        };
        let raw = canonical(&header)?;
        if raw.len() > 65_536 {
            return Err(Error::Budget);
        }
        let mut tree_limits = limits;
        tree_limits.max_writes = limits.max_writes.checked_sub(1).ok_or(Error::Budget)?;
        tree_limits.max_write_bytes = limits
            .max_write_bytes
            .checked_sub(raw.len())
            .ok_or(Error::Budget)?;
        let identity = put(store, &raw)?;
        let mut entries = BTreeMap::from([(path("identity")?, identity.clone())]);
        for object in &self.retained {
            let role = if self.working.contains(object) {
                "working"
            } else {
                "retained"
            };
            entries.insert(
                path(&format!("{role}/{}", object.sha256.as_str()))?,
                object.clone(),
            );
        }
        let tree = Tree::build(&entries, store, tree_limits)
            .map_err(tree_error)?
            .0;
        Ok(Selection {
            root: tree.root().clone(),
            context: identity.sha256,
        })
    }

    /// Replays exact historical desired assignments, not current policy/custody.
    pub fn from_archive(
        selection: &Selection,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<Self> {
        let (mut entries, metrics) = Tree::from_root(selection.root.clone())
            .entries(store, limits)
            .map_err(tree_error)?;
        let pin = entries
            .remove(&path("identity")?)
            .ok_or(Error::Incomplete)?;
        if pin.sha256 != selection.context {
            return Err(Error::Integrity);
        }
        if metrics.reads >= limits.max_reads {
            return Err(Error::Budget);
        }
        let remaining = limits
            .max_read_bytes
            .checked_sub(metrics.read_bytes)
            .ok_or(Error::Budget)?;
        let raw = read(store, &pin, remaining)?;
        // Strict canonical parser catches duplicate fields before typed deserialization.
        let value = identity::parse_json(&raw).map_err(|_| Error::Metadata)?;
        let header: Header = serde_json::from_value(value).map_err(|_| Error::Metadata)?;
        if canonical(&header)? != raw
            || header.schema != "tig.native-placement.experimental-1"
            || header.retained > 65_536
            || header.retained != entries.len()
            || header.working > header.retained
            || header.working_places.is_empty()
            || header.retained_places.is_empty()
            || header.working_places.len() > 16
            || header.retained_places.len() > 16
        {
            return Err(Error::Integrity);
        }
        let mut working = BTreeSet::new();
        let mut retained = BTreeSet::new();
        for (entry, object) in entries {
            let (role, hash) = entry.as_str().split_once('/').ok_or(Error::Integrity)?;
            if hash != object.sha256.as_str()
                || !matches!(role, "working" | "retained")
                || !retained.insert(object.clone())
            {
                return Err(Error::Integrity);
            }
            if role == "working" {
                working.insert(object);
            }
        }
        if working.len() != header.working || !working.contains(&header.record) {
            return Err(Error::Integrity);
        }
        let count = retained.len() * header.retained_places.len()
            + working.len()
                * header
                    .working_places
                    .difference(&header.retained_places)
                    .count();
        if count > 65_536 {
            return Err(Error::Budget);
        }
        let placements = retained
            .iter()
            .flat_map(|object| {
                header.retained_places.iter().map(move |place| Placement {
                    object: object.clone(),
                    place: place.clone(),
                })
            })
            .chain(working.iter().flat_map(|object| {
                header.working_places.iter().map(move |place| Placement {
                    object: object.clone(),
                    place: place.clone(),
                })
            }))
            .collect();
        let claim = Claim {
            id: header.claim,
            placements,
        };
        View::new(0, false, vec![claim.clone()]).map_err(|_| Error::Integrity)?;
        Ok(Self {
            source_checkpoint: header.source_checkpoint,
            record: header.record,
            policy: header.policy,
            claim,
            working,
            retained,
            working_places: header.working_places,
            retained_places: header.retained_places,
        })
    }
}
