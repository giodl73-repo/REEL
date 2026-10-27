//! Opt-in authored-schema validation of an exact captured/archived native view.
use super::{AssetSnapshot, Error, Result};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use tig_assets::{
    contracts::Contract,
    native::{BoundInstance, Instance},
    resolve::{AssetResolver, Error as ResolveError, Resolved},
    schema::{Dialect, Schema},
};
use tig_core::{Digest, Object};

/// A borrowing check prevents adding/changing source records without revalidation.
/// This does not execute stream/native rules or grant approval/custody authority.
pub struct ValidatedSnapshot<'a> {
    source: &'a AssetSnapshot,
    records: usize,
    definitions: usize,
    policy: Object,
    local_contracts: bool,
}
impl ValidatedSnapshot<'_> {
    pub fn checkpoint(&self) -> &Digest {
        self.source.checkpoint()
    }
    pub fn records_checked(&self) -> usize {
        self.records
    }
    pub fn definitions_checked(&self) -> usize {
        self.definitions
    }
    pub fn policy(&self) -> &Object {
        &self.policy
    }
    pub fn complete_semantic_coverage(&self) -> bool {
        false
    }
    pub fn local_contracts_validated(&self) -> bool {
        self.local_contracts
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Authored,
    Local,
}
enum Engine {
    Authored(Schema),
    Local(Contract),
}
impl Engine {
    fn compile(bound: &BoundInstance, schema: &[u8], dialect: Dialect, mode: Mode) -> Result<Self> {
        match mode {
            Mode::Authored => Schema::compile(bound.definition(), schema, dialect)
                .map(Self::Authored)
                .map_err(|_| Error::Invalid),
            Mode::Local => Contract::compile(bound.definition(), schema, dialect)
                .map(Self::Local)
                .map_err(|_| Error::Invalid),
        }
    }
    fn check(&self, bound: &BoundInstance) -> Result<()> {
        match self {
            Self::Authored(e) => e.check(bound).map(|_| ()).map_err(|_| Error::Invalid),
            Self::Local(e) => e.check(bound).map(|_| ()).map_err(|_| Error::Invalid),
        }
    }
    fn policy(&self) -> &Object {
        match self {
            Self::Authored(e) => e.policy(),
            Self::Local(e) => e.policy(),
        }
    }
}
impl AssetResolver for ValidatedSnapshot<'_> {
    fn resolve_id(&self, id: &str) -> std::result::Result<Resolved, ResolveError> {
        self.source.resolve_id(id)
    }
}
impl AssetSnapshot {
    /// All current and retained records, with compilation shared by exact definition.
    pub fn validate_authored_schemas(&self, dialect: Dialect) -> Result<ValidatedSnapshot<'_>> {
        self.validate(dialect, Mode::Authored)
    }
    /// Authored schema plus explicitly versioned stream/local record assertions.
    pub fn validate_local_contracts(&self, dialect: Dialect) -> Result<ValidatedSnapshot<'_>> {
        self.validate(dialect, Mode::Local)
    }
    fn validate(&self, dialect: Dialect, mode: Mode) -> Result<ValidatedSnapshot<'_>> {
        if self.records.is_empty() {
            return Err(Error::Incomplete);
        }
        let mut compiled = BTreeMap::new();
        let started = Instant::now();
        for (reference, record) in &self.records {
            if started.elapsed() > Duration::from_millis(self.limits.millis) {
                return Err(Error::Budget);
            }
            let raw = self.blobs.get(&reference.object).ok_or(Error::Incomplete)?;
            let definition = self
                .blobs
                .get(&record.definition)
                .ok_or(Error::Incomplete)?;
            let schema = self.blobs.get(&record.schema).ok_or(Error::Incomplete)?;
            let bound = Instance::parse(raw)
                .and_then(|i| i.bind(definition, schema))
                .map_err(|_| Error::Integrity)?;
            let engine = if let Some(engine) = compiled.get(&record.definition) {
                engine
            } else {
                let engine = Engine::compile(&bound, schema, dialect, mode)?;
                compiled.entry(record.definition.clone()).or_insert(engine)
            };
            engine.check(&bound)?;
            if started.elapsed() > Duration::from_millis(self.limits.millis) {
                return Err(Error::Budget);
            }
        }
        let policy = compiled
            .values()
            .next()
            .ok_or(Error::Incomplete)?
            .policy()
            .clone();
        Ok(ValidatedSnapshot {
            source: self,
            records: self.records.len(),
            definitions: compiled.len(),
            policy,
            local_contracts: matches!(mode, Mode::Local),
        })
    }
}
