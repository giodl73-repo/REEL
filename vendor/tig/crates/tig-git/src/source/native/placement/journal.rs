//! Read-only lifecycle projection; never a provider claim-release operation.
use super::Demand;
use crate::source::native::{Error, Result};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
};
use tig_core::{
    Claim, Digest, Object, Placement, View, identity,
    tree::{NodeStore, TreeError, TreeLimits, TreeResult},
};
use tig_store::source_claims::{Checkpoint, ObligationSet, Role};

#[derive(Clone, Copy)]
pub struct Limits {
    pub roots: usize,
    pub tree: TreeLimits,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            roots: 4096,
            tree: TreeLimits::default(),
        }
    }
}
/// Positive obligations from one exact journal observation, not complete coverage.
pub struct Projection {
    checkpoint: Checkpoint,
    view: View,
    roots: usize,
}
impl Projection {
    pub fn checkpoint(&self) -> &Checkpoint {
        &self.checkpoint
    }
    pub fn view(&self) -> &View {
        &self.view
    }
    pub fn roots(&self) -> usize {
        self.roots
    }
}
pub(super) struct Reads<'a, S> {
    store: &'a S,
    remaining: Cell<usize>,
    bytes: Cell<usize>,
}
impl<'a, S> Reads<'a, S> {
    pub(super) fn new(store: &'a S, limits: TreeLimits) -> Self {
        Self {
            store,
            remaining: Cell::new(limits.max_reads),
            bytes: Cell::new(limits.max_read_bytes),
        }
    }
}
impl<S: NodeStore> NodeStore for Reads<'_, S> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        let remaining = self
            .remaining
            .get()
            .checked_sub(1)
            .ok_or(TreeError::Budget)?;
        self.remaining.set(remaining);
        if object.bytes > self.bytes.get() as u64 {
            return Err(TreeError::Budget);
        }
        let raw = self.store.get(object)?;
        if let Some(raw) = &raw {
            let remaining = self
                .bytes
                .get()
                .checked_sub(raw.len())
                .ok_or(TreeError::Budget)?;
            self.bytes.set(remaining);
        }
        Ok(raw)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Err(TreeError::Store("read-only placement projection".into()))
    }
}
pub fn project(
    roots: &ObligationSet,
    store: &impl NodeStore,
    limits: Limits,
) -> Result<Projection> {
    if limits.roots == 0 || limits.roots > 65_536 || roots.len() > limits.roots {
        return Err(Error::Budget);
    }
    let mut reader = Reads::new(store, limits.tree);
    let mut claims = BTreeMap::<String, BTreeSet<Placement>>::new();
    let mut assignments = 0usize;
    for obligation in roots.iter() {
        let demand = Demand::from_archive(&obligation.selection, &mut reader, limits.tree)?;
        let bytes = identity::canonical_json(&serde_json::json!({
            "schema":"tig.journal-placement-claim.experimental-1",
            "journal":roots.checkpoint().identity(),"source":obligation.source,
            "owner":obligation.owner }))
        .map_err(|_| Error::Metadata)?;
        let id = Digest::blob(&bytes).as_str().to_owned();
        if !claims.contains_key(&id) && claims.len() >= 1024 {
            return Err(Error::Budget);
        }
        let placements = claims.entry(id).or_default();
        for placement in &demand.claim.placements {
            if matches!(obligation.role, Role::Predecessor)
                && !demand.retained_places.contains(&placement.place)
            {
                continue;
            }
            if placements.insert(placement.clone()) {
                assignments += 1;
                if assignments > 65_536 {
                    return Err(Error::Budget);
                }
            }
        }
    }
    let view = View::new(
        roots.checkpoint().sequence(),
        false,
        claims
            .into_iter()
            .map(|(id, placements)| Claim { id, placements })
            .collect(),
    )
    .map_err(|error| match error {
        tig_core::Error::Budget => Error::Budget,
        _ => Error::Integrity,
    })?;
    Ok(Projection {
        checkpoint: roots.checkpoint().clone(),
        view,
        roots: roots.len(),
    })
}
