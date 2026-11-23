//! Retire obsolete control-head references using the original journal command.
//! Run records, metadata bytes, protection and annex payloads are never removed.
use super::*;
use std::cell::Cell;

struct ReadScope<'a> {
    nodes: &'a MetadataStore,
    used: Cell<(usize, u64)>,
}
impl NodeStore for ReadScope<'_> {
    fn get(&self, object: &Object) -> tig_core::tree::TreeResult<Option<Vec<u8>>> {
        let (reads, bytes) = self.used.get();
        let reads = reads
            .checked_add(1)
            .ok_or(tig_core::tree::TreeError::Budget)?;
        let bytes = bytes
            .checked_add(object.bytes)
            .ok_or(tig_core::tree::TreeError::Budget)?;
        self.used.set((reads, bytes));
        if reads > 4096 || bytes > 4 * 1024 * 1024 {
            return Err(tig_core::tree::TreeError::Budget);
        }
        self.nodes.get(object)
    }
    fn put(&mut self, _: &[u8]) -> tig_core::tree::TreeResult<Object> {
        Err(tig_core::tree::TreeError::InvalidNode)
    }
}
fn deferred(nodes: &mut impl NodeStore, root: &Object) -> io::Result<()> {
    Tree::from_root(root.clone())
        .entries(nodes, tree_limits())
        .map_err(|_| invalid())?;
    Ok(())
}

#[derive(Serialize)]
pub struct Report {
    pub handle: Handle,
    pub retired_versions: usize,
    pub retained_versions: usize,
}

/// Explicit metadata-only maintenance. An unsettled head or held progress refuses
/// rather than adopting, canceling or reconciling it on the caller's behalf.
pub fn retire_heads(handle: &Handle) -> io::Result<Report> {
    let _lock = root_lock(&handle.root)?;
    let mut journal = SourceJournal::open(&handle.root, &handle.minimum).map_err(|_| invalid())?;
    let mut nodes = MetadataStore::open(&handle.metadata.root, &handle.metadata.minimum)
        .map_err(|_| invalid())?;
    let enrolled = enrollment(handle, &mut journal, &nodes)?;
    let head: Head =
        selected(&mut journal, &nodes, &k("head")?, &enrolled.owner)?.ok_or_else(invalid)?;
    let mut handle = handle.clone();
    handle.index_minimum = stronger(&handle.index_minimum, &head.index_minimum)?;
    handle.policy_minimum = stronger(&handle.policy_minimum, &head.policy_minimum)?;
    let retired_versions = trim(&mut journal, &mut nodes, &enrolled.owner)?;
    handle.minimum = journal.checkpoint();
    handle.metadata.minimum = nodes.checkpoint();
    Ok(Report {
        handle,
        retired_versions,
        retained_versions: 0,
    })
}

pub(super) fn trim(
    journal: &mut SourceJournal,
    nodes: &mut MetadataStore,
    owner: &Key,
) -> io::Result<usize> {
    let mut scope = ReadScope {
        nodes,
        used: Cell::new((0, 0)),
    };
    let nodes = &mut scope;
    let source = k("head")?;
    let state = journal
        .source(&source)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if state.owner != *owner || state.pending.is_some() || state.cancelled {
        return Err(invalid());
    }
    let head: Head = version(nodes, state.current.as_ref().ok_or_else(invalid)?)?;
    let capacity = progress_budget::capacity(journal).map_err(|_| invalid())?;
    if head.active.is_some()
        || head.settled
        || capacity.held_events != 0
        || capacity.held_bytes != 0
    {
        return Err(invalid());
    }
    deferred(nodes, &head.deferred)?;
    let configuration = journal
        .source(&k("configuration")?)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if configuration.owner != *owner
        || configuration.revision != 2
        || configuration.pending.is_some()
        || configuration.cancelled
    {
        return Err(invalid());
    }
    // Validate the entire bounded selection before withdrawing any reference.
    // Every former active head must have an independently selected terminal run.
    for previous in &state.predecessors {
        let old: Head = version(nodes, previous)?;
        if let Some(active) = old.active {
            let saved: Saved = selected(journal, nodes, &run_key(&active.operation)?, owner)?
                .ok_or_else(invalid)?;
            let recipe: Recipe = read(nodes, &saved.recipe)?;
            let done = saved.terminal.ok_or_else(invalid)?;
            if saved.recipe != active.recipe
                || recipe.after != active.commit
                || !matches!(
                    done.status,
                    Status::Complete | Status::Deferred | Status::Superseded
                )
            {
                return Err(invalid());
            }
            let outcomes: Vec<retirement::Report> = read(nodes, &done.outcomes)?;
            if outcomes
                .iter()
                .any(|outcome| matches!(outcome.status, retirement::Status::Uncertain))
            {
                return Err(invalid());
            }
            deferred(nodes, &done.deferred)?;
        } else if old.settled {
            return Err(invalid());
        }
    }
    let count = state.predecessors.len();
    let mut revision = state.revision;
    for previous in state.predecessors {
        journal
            .apply(
                key(
                    "retire-head",
                    &(journal.identity(), revision, previous.revision),
                )?,
                Command::RetireGuarded {
                    source: source.clone(),
                    owner: owner.clone(),
                    expected_revision: revision,
                    predecessor_revision: previous.revision,
                    replacement: None,
                    guards: vec![source_claims::Guard {
                        source: k("configuration")?,
                        owner: owner.clone(),
                        expected_revision: configuration.revision,
                    }],
                },
            )
            .map_err(|_| invalid())?;
        revision = revision.checked_add(1).ok_or_else(invalid)?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests;
