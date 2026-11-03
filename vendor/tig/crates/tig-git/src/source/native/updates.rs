//! Exact captured source to pure update preparation; no writes or owner adoption.
pub mod publication;
use super::AssetSnapshot;
use tig_assets::{
    edits::{self, Change, Inputs, Prepared},
    revisions::{Limits, Reader},
    schema::Dialect,
};
use tig_core::Object;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Missing,
    Stale,
    Integrity,
    Update(edits::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

impl AssetSnapshot {
    /// Prepare one selected asset against its expected captured raw record.
    /// Retained old records never substitute for this snapshot's ID selection.
    /// Caller must separately persist/protect metadata and base-fence Git adoption.
    pub fn prepare_history(
        &self,
        id: &str,
        expected: &Object,
        dialect: Dialect,
        limits: Limits,
        change: Change<'_>,
        reader: &mut impl Reader,
    ) -> Result<Prepared, Error> {
        let reference = self.selected.get(id).ok_or(Error::Missing)?;
        if &reference.object != expected {
            return Err(Error::Stale);
        }
        let record = self.records.get(reference).ok_or(Error::Integrity)?;
        let instance = self.blobs.get(expected).ok_or(Error::Integrity)?;
        let definition = self.blobs.get(&record.definition).ok_or(Error::Integrity)?;
        let schema = self.blobs.get(&record.schema).ok_or(Error::Integrity)?;
        edits::prepare(
            Inputs {
                expected,
                instance,
                definition,
                authored_schema: schema,
                dialect,
                limits,
            },
            change,
            reader,
        )
        .map_err(Error::Update)
    }
}
