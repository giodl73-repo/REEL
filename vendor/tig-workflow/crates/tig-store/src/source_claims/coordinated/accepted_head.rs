//! A short native metadata-admission lease, not authenticated Git/GitHub current
//! authority. It reuses the original enrolled source-head fence. Do not hold it
//! across payload IO; effect-time policy/source authority remains separate work.
use super::*;
use crate::source_claims::protection::{Index, work::adoption::enrollment};
use tig_core::retirement::review::routine_policy::{Frame, Run};

/// Nonserializable, noncloneable cooperating-writer exclusion. A cloned Frame is
/// an observation only; it does not carry this lease or globally latest truth.
pub struct HeldHead {
    _fence: Guard,
    journal: Digest,
    source: Key,
    owner: Key,
    selection: Selection,
    frame: Frame,
}
impl HeldHead {
    pub fn frame(&self) -> &Frame {
        &self.frame
    }
    pub fn journal(&self) -> &Digest {
        &self.journal
    }
    pub fn source(&self) -> &Key {
        &self.source
    }
    pub fn owner(&self) -> &Key {
        &self.owner
    }
    pub fn selection(&self) -> &Selection {
        &self.selection
    }
    pub fn require_run(&self, run: &Run) -> Result<()> {
        if run.config().namespace != self.journal || run.config().frame != self.frame {
            return Err(Error::Stale);
        }
        Ok(())
    }
}

/// Observe one enrolled selected head while excluding the original coordinated
/// writers. Index coverage, native owner-to-subject mapping, policy approval and
/// restore/latest authority are not inferred from this lease.
pub fn hold_current(index: &mut Index, journal: &mut SourceJournal) -> Result<HeldHead> {
    let fence = lock(journal)?;
    let binding = journal.coordinator().ok_or(Error::Restricted)?.clone();
    if &binding.index != index.identity()
        || enrollment::selected(index).map_err(|_| Error::Restricted)?
            != Some(journal.identity().clone())
    {
        return Err(Error::Restricted);
    }
    if let Some(control) = journal.source(&control(&binding)?)? {
        if control.cancelled {
            return Err(Error::Cancelled);
        }
        if control.pending.is_some() {
            return Err(Error::Pending);
        }
    }
    let source = journal.source(&binding.source)?.ok_or(Error::Missing)?;
    if source.owner != binding.owner {
        return Err(Error::Owner);
    }
    if source.cancelled {
        return Err(Error::Cancelled);
    }
    if source.pending.is_some() {
        return Err(Error::Pending);
    }
    let current = source.current.ok_or(Error::Missing)?;
    if current.revision == 0 {
        return Err(Error::Corrupt);
    }
    let journal_id = journal.identity().clone();
    let frame = Frame {
        source: Digest::blob(&encode(&(
            "tig.native-accepted-source.experimental-1",
            &journal_id,
            &binding.source,
        ))?),
        authority: binding.index,
        // The journal instance identity commits its epoch and source binding.
        epoch: journal_id.clone(),
        // Retiring history advances Source.revision, not the accepted version.
        generation: current.revision,
        selection: Digest::blob(&encode(&(
            "tig.native-accepted-selection.experimental-1",
            &current.selection,
        ))?),
    };
    Ok(HeldHead {
        _fence: fence,
        journal: journal_id,
        source: binding.source,
        owner: binding.owner,
        selection: current.selection,
        frame,
    })
}
