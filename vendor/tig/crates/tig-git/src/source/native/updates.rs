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
        edits::prepare(
            self.history_inputs(id, expected, dialect, limits)?,
            change,
            reader,
        )
        .map_err(Error::Update)
    }

    /// Reuse this exact captured snapshot for several validated edits of one
    /// asset, returning one expected-original-base publication/proposal input.
    pub fn prepare_history_sequence(
        &self,
        id: &str,
        expected: &Object,
        dialect: Dialect,
        limits: Limits,
        changes: Vec<Change<'_>>,
        reader: &mut impl Reader,
    ) -> Result<Prepared, Error> {
        edits::sequence::prepare(
            self.history_inputs(id, expected, dialect, limits)?,
            changes,
            reader,
        )
        .map_err(Error::Update)
    }

    fn history_inputs<'a>(
        &'a self,
        id: &str,
        expected: &'a Object,
        dialect: Dialect,
        limits: Limits,
    ) -> Result<Inputs<'a>, Error> {
        let reference = self.selected.get(id).ok_or(Error::Missing)?;
        if &reference.object != expected {
            return Err(Error::Stale);
        }
        let record = self.records.get(reference).ok_or(Error::Integrity)?;
        let instance = self.blobs.get(expected).ok_or(Error::Integrity)?;
        let definition = self.blobs.get(&record.definition).ok_or(Error::Integrity)?;
        let schema = self.blobs.get(&record.schema).ok_or(Error::Integrity)?;
        Ok(Inputs {
            expected,
            instance,
            definition,
            authored_schema: schema,
            dialect,
            limits,
        })
    }
}
