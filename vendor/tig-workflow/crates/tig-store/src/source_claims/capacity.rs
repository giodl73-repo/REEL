//! Logical journal headroom at one verified checkpoint, never disk capacity.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityPool {
    pub limit: u64,
    pub used: u64,
    pub held: u64,
    pub available: u64,
}
impl CapacityPool {
    fn new(limit: u64, used: u64, held: u64) -> Result<Self> {
        let available = limit
            .checked_sub(used.checked_add(held).ok_or(Error::Corrupt)?)
            .ok_or(Error::Corrupt)?;
        Ok(Self {
            limit,
            used,
            held,
            available,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalCapacity {
    pub schema: String,
    pub checkpoint: Checkpoint,
    pub sources: CapacityPool,
    pub commands: CapacityPool,
    pub command_body_bytes: CapacityPool,
    pub cancellations: CapacityPool,
    pub cancellation_body_bytes: CapacityPool,
    pub pending_sources: u64,
    pub complete_claim_coverage: bool,
}
impl SourceJournal {
    /// Observes existing metadata only. Availability can change immediately;
    /// this neither reserves room nor authorizes work or payload removal.
    pub fn capacity(&mut self) -> Result<JournalCapacity> {
        self.sync()?;
        let state = &self.state;
        let (pe, pb) = progress_budget::held(state)?;
        let (oe, ob) = outcome_budget::held(state)?;
        let cancels = state.sources.values().filter(|s| s.cancelled).count() as u64;
        Ok(JournalCapacity {
            schema: "tig.source-journal-capacity.experimental-1".into(),
            checkpoint: state.head.clone(),
            sources: CapacityPool::new(
                MAX_SOURCES as u64,
                state.sources.len() as u64,
                progress_budget::missing_sources(state).len() as u64,
            )?,
            commands: CapacityPool::new(
                MAX_COMMANDS,
                state
                    .head
                    .sequence
                    .checked_sub(cancels)
                    .ok_or(Error::Corrupt)?,
                pe.checked_add(oe).ok_or(Error::Corrupt)?,
            )?,
            command_body_bytes: CapacityPool::new(
                MAX_COMMAND_BYTES,
                state
                    .bytes
                    .checked_sub(state.cancel_bytes)
                    .ok_or(Error::Corrupt)?,
                pb.checked_add(ob).ok_or(Error::Corrupt)?,
            )?,
            cancellations: CapacityPool::new(MAX_SOURCES as u64, cancels, 0)?,
            cancellation_body_bytes: CapacityPool::new(MAX_CANCEL_BYTES, state.cancel_bytes, 0)?,
            pending_sources: state
                .sources
                .values()
                .filter(|s| s.pending.is_some())
                .count() as u64,
            complete_claim_coverage: false,
        })
    }
}

#[cfg(test)]
mod tests;
