//! Exact declared job resources as flat guarded-namespace bindings.
//! Metadata membership is not byte availability, custody or removal authority.
use super::{JobContext, path};
use crate::source::native::{
    Error, Result,
    archive::{put, read, tree_error},
};
use git_asset_core::{
    LogicalPath, Object, identity,
    tree::{NodeStore, Tree, TreeLimits},
};
use git_asset_store::source_claims::Selection;
use std::collections::BTreeMap;

fn prepare(
    job: &JobContext,
    limits: TreeLimits,
) -> Result<(Vec<u8>, BTreeMap<LogicalPath, Object>)> {
    let count = job.objects().len().checked_add(1).ok_or(Error::Budget)?;
    if count > limits.max_entries || count > 65_536 {
        return Err(Error::Budget);
    }
    let header = identity::canonical_json(&serde_json::json!({
        "schema":"tig.native-job-protection.experimental-1",
        "job":job.selection(),"objects":job.objects().len()
    }))
    .map_err(|_| Error::Invalid)?;
    let mut entries = BTreeMap::from([(path("job/protection")?, Object::from_bytes(&header))]);
    for object in job.objects() {
        entries.insert(
            path(&format!("objects/{}", object.sha256.as_str()))?,
            object.clone(),
        );
    }
    Ok((header, entries))
}

/// Candidate metadata only. Finish/reopen and verify before Namespace::select.
/// Does not change the original JobContext, source ownership or creative selection.
pub fn freeze(
    job: &JobContext,
    store: &mut impl NodeStore,
    mut limits: TreeLimits,
) -> Result<Selection> {
    let (header, entries) = prepare(job, limits)?;
    limits.max_writes = limits.max_writes.checked_sub(1).ok_or(Error::Budget)?;
    limits.max_write_bytes = limits
        .max_write_bytes
        .checked_sub(header.len())
        .ok_or(Error::Budget)?;
    let header = put(store, &header)?;
    let (tree, _) = Tree::build(&entries, store, limits).map_err(tree_error)?;
    Ok(Selection {
        root: tree.root().clone(),
        context: header.sha256,
    })
}

/// Check exact header and flat bindings against an admitted/restored job.
/// Reads metadata only; refuses substitutions without opening payloads or writing.
pub fn verify(
    selection: &Selection,
    job: &JobContext,
    store: &mut impl NodeStore,
    mut limits: TreeLimits,
) -> Result<()> {
    let (header, expected) = prepare(job, limits)?;
    let object = Object::from_bytes(&header);
    if selection.context != object.sha256 {
        return Err(Error::Integrity);
    }
    limits.max_reads = limits.max_reads.checked_sub(1).ok_or(Error::Budget)?;
    limits.max_read_bytes = limits
        .max_read_bytes
        .checked_sub(header.len())
        .ok_or(Error::Budget)?;
    if read(store, &object, header.len())? != header {
        return Err(Error::Integrity);
    }
    let (entries, _) = Tree::from_root(selection.root.clone())
        .entries(store, limits)
        .map_err(tree_error)?;
    if entries != expected {
        return Err(Error::Integrity);
    }
    Ok(())
}
