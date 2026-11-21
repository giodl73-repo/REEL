//! Declared resource adapters for replay-admitted typed contexts. No payload IO.
use super::{
    Error, Result,
    archive::{read, tree_error},
    context::ProductionContext,
    job::JobContext,
};
use std::collections::BTreeSet;
use tig_core::{
    LogicalPath, Object,
    tree::{NodeStore, Tree, TreeLimits},
};
use tig_store::source_claims::{Selection, projection::Resources};

fn entries(
    selection: &Selection,
    identity: &str,
    nodes: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<BTreeSet<Object>> {
    let (entries, _) = Tree::from_root(selection.root.clone())
        .entries(nodes, limits)
        .map_err(tree_error)?;
    let header = entries
        .get(&LogicalPath::parse(identity).map_err(|_| Error::Invalid)?)
        .ok_or(Error::Incomplete)?;
    if header.sha256 != selection.context {
        return Err(Error::Integrity);
    }
    read(nodes, header, 65_536)?;
    Ok(entries
        .into_values()
        .chain([selection.root.clone()])
        .collect())
}
/// Context must already be admitted/replayed by its consumer. This covers the
/// context's declared requirements and interpretation objects, not undeclared
/// project semantics hidden in opaque documents or recursive metadata GC closure.
pub fn source(
    context: &ProductionContext,
    nodes: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<Resources> {
    let mut objects = entries(context.selection(), "context/identity", nodes, limits)?;
    objects.extend(context.requirements().all_objects());
    Ok(Resources::admitted(context.selection(), objects))
}
/// Includes every declared job role: primary, control, interpretation, runtime,
/// composition, claimed output and evidence. Retention is never projected down
/// to only the payloads consumed by a particular recipe.
pub fn job(
    context: &JobContext,
    nodes: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<Resources> {
    let mut objects = entries(context.selection(), "job/identity", nodes, limits)?;
    objects.extend(context.objects().iter().cloned());
    Ok(Resources::admitted(context.selection(), objects))
}
