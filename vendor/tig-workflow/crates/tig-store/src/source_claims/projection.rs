//! Bounded retained-resource projection candidates through the existing Tree.
//! Resolution is an admitted adapter responsibility, never an empty fallback.
pub mod envelope;
use super::{Checkpoint, Error, Obligation, ObligationSet, Result, Selection, SourceJournal};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use tig_core::{
    Digest, LogicalPath, Object, identity,
    tree::{NodeStore, Tree, TreeLimits},
};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub obligations: usize,
    pub objects: usize,
    pub logical_bytes: u64,
    pub tree: TreeLimits,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            obligations: 1024,
            objects: 16384,
            logical_bytes: 64 * 1024 * 1024 * 1024,
            tree: TreeLimits::default(),
        }
    }
}
impl Limits {
    fn validate(&self) -> Result<()> {
        if !(1..=super::MAX_SOURCES * (super::MAX_PREDECESSORS + 2)).contains(&self.obligations)
            || !(1..=500_000).contains(&self.objects)
            || self.logical_bytes == 0
            || self.logical_bytes > 9_007_199_254_740_991
        {
            return Err(Error::Budget);
        }
        Ok(())
    }
}
/// Explicit adapter admission bound to one exact retained selection. This is
/// neither automatic closure discovery nor proof of external owner authority.
#[derive(Clone)]
pub struct Resources {
    selection: Selection,
    objects: BTreeSet<Object>,
}
impl Resources {
    pub fn admitted(selection: &Selection, objects: BTreeSet<Object>) -> Self {
        Self {
            selection: selection.clone(),
            objects,
        }
    }
    pub fn objects(&self) -> &BTreeSet<Object> {
        &self.objects
    }
}
pub struct Projection {
    checkpoint: Checkpoint,
    selection: Selection,
    objects: BTreeSet<Object>,
}
impl Projection {
    pub fn checkpoint(&self) -> &Checkpoint {
        &self.checkpoint
    }
    pub fn selection(&self) -> &Selection {
        &self.selection
    }
    pub fn objects(&self) -> &BTreeSet<Object> {
        &self.objects
    }
    pub fn complete_claim_coverage(&self) -> bool {
        false
    }
    /// Point revalidation only. Callers still need cooperating writer admission
    /// and original scoped gates; a successful check is not a held lease.
    pub fn validate_current(&self, root: &Path) -> Result<()> {
        let observed = SourceJournal::open_mode(root, &self.checkpoint, true)?.checkpoint();
        if observed != self.checkpoint {
            return Err(Error::Stale);
        }
        Ok(())
    }
}
impl std::fmt::Debug for Projection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Projection")
            .field("objects", &self.objects.len())
            .finish_non_exhaustive()
    }
}
/// Each adapter must resolve every obligation's admitted declared resources,
/// including opaque dependencies where applicable. Missing/unsupported resolution
/// must return an error, never an empty set. No payload reads are required here.
pub fn build(
    snapshot: &ObligationSet,
    policy: &Digest,
    resolve: &mut impl FnMut(&Obligation) -> Result<Resources>,
    nodes: &mut impl NodeStore,
    limits: Limits,
) -> Result<Projection> {
    limits.validate()?;
    if snapshot.len() > limits.obligations {
        return Err(Error::Budget);
    }
    let mut objects = BTreeMap::<Digest, Object>::new();
    let mut bytes = 0u64;
    for obligation in snapshot.iter() {
        let resolved = resolve(obligation)?;
        if resolved.selection != obligation.selection {
            return Err(Error::Conflict);
        }
        let mut resources = resolved.objects;
        // The retained context itself is never lost when its adapter has no
        // external payloads. Repeated selections are intentionally re-admitted
        // under each source/owner/role rather than assuming equal meaning.
        resources.insert(obligation.selection.root.clone());
        if resources.len() > limits.objects {
            return Err(Error::Budget);
        }
        for object in resources {
            if let Some(old) = objects.get(&object.sha256) {
                if old != &object {
                    return Err(Error::Conflict);
                }
                continue;
            }
            bytes = bytes.checked_add(object.bytes).ok_or(Error::Budget)?;
            if object.bytes > 9_007_199_254_740_991
                || bytes > limits.logical_bytes
                || objects.len() == limits.objects
            {
                return Err(Error::Budget);
            }
            objects.insert(object.sha256.clone(), object);
        }
    }
    // All resolution/conflict/aggregate budget checks precede our first write.
    let bindings = objects
        .values()
        .map(|o| {
            Ok((
                LogicalPath::parse(&format!("objects/{}", o.sha256.as_str()))
                    .map_err(|_| Error::InvalidReference)?,
                o.clone(),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let (tree, _) = Tree::build(&bindings, nodes, limits.tree).map_err(|e| match e {
        tig_core::tree::TreeError::Budget => Error::Budget,
        tig_core::tree::TreeError::Store(_) => Error::Io,
        _ => Error::Corrupt,
    })?;
    let context=identity::canonical_json(&serde_json::json!({
        "schema":"tig.retained-resource-projection.experimental-1",
        "checkpoint":snapshot.checkpoint(),"policy":policy,"obligations":snapshot.len(),"root":tree.root()
    })).map_err(|_|Error::InvalidReference)?;
    Ok(Projection {
        checkpoint: snapshot.checkpoint().clone(),
        selection: Selection {
            root: tree.root().clone(),
            context: Digest::blob(&context),
        },
        objects: objects.into_values().collect(),
    })
}
/// Read, resolve and write an immutable candidate, then reject journal drift.
/// On failure, unreferenced candidate metadata may remain; no source/index state
/// is selected, updated or removed. Finish/readback and admission are separate.
pub fn build_current(
    root: &Path,
    minimum: &Checkpoint,
    policy: &Digest,
    resolve: &mut impl FnMut(&Obligation) -> Result<Resources>,
    nodes: &mut impl NodeStore,
    limits: Limits,
) -> Result<Projection> {
    limits.validate()?;
    let snapshot = SourceJournal::observe_obligations(root, minimum, limits.obligations)?;
    let result = build(&snapshot, policy, resolve, nodes, limits)?;
    result.validate_current(root)?;
    Ok(result)
}
