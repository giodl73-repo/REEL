//! Explicit read-only ledger observation into a caller-admitted metadata port.
//! No segment activation, journal retirement, current permission or payload IO.
use super::*;
use git_asset_core::{capacity::checkpoint as state, tree::NodeStore};
#[derive(Debug)]
pub enum Error {
    Journal(LedgerError),
    State(state::Error),
    Invalid,
}
impl From<LedgerError> for Error {
    fn from(e: LedgerError) -> Self {
        Self::Journal(e)
    }
}
impl From<state::Error> for Error {
    fn from(e: state::Error) -> Self {
        Self::State(e)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    schema: String,
    journal: Checkpoint,
    state: state::Checkpoint,
}
impl Descriptor {
    pub fn journal(&self) -> &Checkpoint {
        &self.journal
    }
    pub fn state(&self) -> &state::Checkpoint {
        &self.state
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        if self.schema != "tig.capacity-working-checkpoint.design-1"
            || self.state.root().bytes == 0
            || self.state.root().bytes > 65536
        {
            return Err(Error::Invalid);
        }
        identity::canonical_json(&serde_json::to_value(self).map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)
    }
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        let value = identity::parse_json(raw).map_err(|_| Error::Invalid)?;
        let descriptor: Self = serde_json::from_value(value).map_err(|_| Error::Invalid)?;
        if descriptor.to_json()? != raw {
            return Err(Error::Invalid);
        }
        Ok(descriptor)
    }
}
pub struct Export {
    descriptor: Descriptor,
    metrics: state::Metrics,
}
impl Export {
    pub fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }
    pub fn metrics(&self) -> state::Metrics {
        self.metrics
    }
}
impl CapacityLedger {
    /// Caller must admit metadata staging/capacity before invoking its NodeStore.
    /// Caller must finish and read back its metadata port before relying on durability.
    /// No DB transaction spans node IO. A peer advance rejects a current export;
    /// orphaned immutable nodes do not install a new root or change old receipts.
    pub fn export_checkpoint(
        &mut self,
        nodes: &mut impl NodeStore,
        limits: state::Limits,
    ) -> Result<Export> {
        self.sync()?;
        let before = self.checkpoint.clone();
        let (checkpoint, metrics) = self.domain.export_checkpoint(nodes, limits)?;
        self.sync()?;
        if self.checkpoint != before {
            return Err(Error::Journal(LedgerError::Conflict));
        }
        Ok(Export {
            descriptor: Descriptor {
                schema: "tig.capacity-working-checkpoint.design-1".into(),
                journal: before,
                state: checkpoint,
            },
            metrics,
        })
    }
}

#[cfg(test)]
mod tests;
