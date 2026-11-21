//! One admitted source identity per explicitly bound index epoch.
use super::*;
use crate::source_claims::Source;
const SOURCE: &str = "source-enrollment";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Prepared,
    SourcePrepared,
    SourceAdopted,
    Selected,
}
/// Both checkpoints must be retained by the controller's trusted restore state.
pub struct Initialization {
    pub source: Receipt,
    pub index: Receipt,
}
fn expected(journal: &SourceJournal, binding: &coordinated::Binding) -> Selection {
    Selection {
        root: binding.initial.root.clone(),
        context: journal.identity().clone(),
    }
}
fn row(index: &mut Index) -> Result<Option<Source>> {
    let source = index.journal.source(&key(SOURCE))?;
    if source.as_ref().is_some_and(|s| {
        s.owner != index.enrollment.owner || s.cancelled || !s.predecessors.is_empty()
    }) {
        return Err(Error::Invalid);
    }
    Ok(source)
}
/// No payload IO or enumeration; an incomplete enrollment is not a negative proof.
pub fn selected(index: &mut Index) -> Result<Option<Digest>> {
    if !index.requires_bound_source() {
        return Err(Error::Invalid);
    }
    let Some(source) = row(index)? else {
        return Ok(None);
    };
    if source.pending.is_some() {
        return Err(Error::Pending);
    }
    if source.revision != 2 {
        return Err(Error::Invalid);
    }
    Ok(Some(
        source.current.ok_or(Error::Unknown)?.selection.context,
    ))
}
pub(super) fn verify(
    index: &mut Index,
    journal: &SourceJournal,
    binding: &coordinated::Binding,
) -> Result<()> {
    if !index.requires_bound_source() {
        return Ok(());
    }
    let Some(source) = row(index)? else {
        return Err(Error::Unknown);
    };
    let wanted = expected(journal, binding);
    if let Some(pending) = source.pending {
        return Err(if pending.selection == wanted {
            Error::Pending
        } else {
            Error::Conflict
        });
    }
    if source.revision != 2 || source.current.is_none_or(|c| c.selection != wanted) {
        return Err(Error::Conflict);
    }
    Ok(())
}
pub fn initialize(
    index: &mut Index,
    journal: &mut SourceJournal,
    nodes: &mut impl NodeStore,
) -> Result<Initialization> {
    initialize_hook(index, journal, nodes, |_| Ok(()))
}
/// Reserves the identity before initializing its head; selects enrollment only
/// after the exact logical adoption. Historical retries replay both receipts.
pub fn initialize_hook(
    index: &mut Index,
    journal: &mut SourceJournal,
    nodes: &mut impl NodeStore,
    mut hook: impl FnMut(Stage) -> Result<()>,
) -> Result<Initialization> {
    if !index.requires_bound_source() {
        return Err(Error::Invalid);
    }
    let binding = candidate_binding(index, journal)?;
    let _source_fence = coordinated::lock(journal)?;
    let _enrollment_fence = index.lock_name(SOURCE)?;
    let wanted = expected(journal, &binding);
    match row(index)? {
        None => {
            let state = index.actor(nodes, &binding.actor)?;
            if state.revision != 1
                || !state.ready
                || state.selection != binding.initial
                || journal.source(&binding.source)?.is_some()
            {
                return Err(Error::Stale);
            }
        }
        Some(source) => match (source.pending, source.current, source.revision) {
            (Some(pending), None, 1) if pending.selection == wanted => {}
            (None, Some(current), 2) if current.selection == wanted => {}
            _ => return Err(Error::Conflict),
        },
    }
    let prepare = op(index.identity(), &key(SOURCE), "prepare-enrollment")?;
    let adopt = op(index.identity(), &key(SOURCE), "select-enrollment")?;
    let prepared = index.command(
        prepare,
        Command::Prepare {
            source: key(SOURCE),
            owner: index.enrollment.owner.clone(),
            expected_revision: 0,
            selection: wanted,
        },
    )?;
    hook(Stage::Prepared)?;
    let source = initialize_head(index, journal, nodes, &binding, &mut hook)?;
    let receipt = index.command(
        adopt,
        Command::Adopt {
            source: key(SOURCE),
            owner: index.enrollment.owner.clone(),
            expected_revision: 1,
            prepared: prepared.reference,
        },
    )?;
    hook(Stage::Selected)?;
    Ok(Initialization {
        source,
        index: receipt,
    })
}
