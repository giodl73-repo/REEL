//! Immutable whole-selection metadata for future paged retirement. No mutation
//! of source claims, payload IO, consent cookie or deletion capability.
use super::claims;
use crate::{
    Key, protection,
    source_claims::{
        self, Kind,
        admission::{self, Namespace},
    },
};
use git_asset_core::{
    Digest, LogicalPath, Object, identity,
    tree::{NodeStore, Tree, TreeError, TreeLimits, TreeResult},
};
use serde::{Deserialize, Serialize};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};
mod compile;
pub mod control;
mod inspect;
pub mod preparation;
mod session;
pub use compile::{build, read_request, request};
pub use inspect::{inspect, page};
pub use session::Session;
#[cfg(test)]
mod tests;
const MAX_DOC: usize = 65536;
const REQUEST_SCHEMA: &str = "tig.paged-selection-request.design-1";
const MANIFEST_SCHEMA: &str = "tig.paged-selection-manifest.design-1";
const PAGE_SCHEMA: &str = "tig.paged-selection-page.design-1";
const MAX_NUMBER: u64 = 9_007_199_254_740_991;
#[derive(Debug)]
pub enum Error {
    Invalid,
    Authority,
    Missing,
    Stale,
    Budget,
    Tree(TreeError),
    Source(admission::Error),
    Roster(claims::Error),
    Subset(source_claims::subset::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<TreeError> for Error {
    fn from(e: TreeError) -> Self {
        if e == TreeError::Budget {
            Self::Budget
        } else {
            Self::Tree(e)
        }
    }
}
impl From<admission::Error> for Error {
    fn from(e: admission::Error) -> Self {
        if e == admission::Error::Budget {
            Self::Budget
        } else {
            Self::Source(e)
        }
    }
}
impl From<claims::Error> for Error {
    fn from(e: claims::Error) -> Self {
        Self::Roster(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;
impl From<source_claims::subset::Error> for Error {
    fn from(e: source_claims::subset::Error) -> Self {
        if e == source_claims::subset::Error::Budget {
            Self::Budget
        } else {
            Self::Subset(e)
        }
    }
}
fn encode(v: &impl Serialize) -> Result<Vec<u8>> {
    let raw = identity::canonical_json(&serde_json::to_value(v).map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)?;
    if raw.len() > MAX_DOC {
        return Err(Error::Budget);
    }
    Ok(raw)
}
fn hash(v: &impl Serialize) -> Result<Digest> {
    Ok(Digest::blob(&encode(v)?))
}
fn path(s: &str) -> Result<LogicalPath> {
    LogicalPath::parse(s).map_err(|_| Error::Invalid)
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub authority: Digest,
    pub epoch: Digest,
    pub owner: Key,
    pub metadata: Digest,
    pub scope: Digest,
    pub classes: BTreeSet<Kind>,
}
impl Enrollment {
    pub fn id(&self) -> Result<Digest> {
        if self.classes.is_empty()
            || self.classes.len() > 2
            || self
                .classes
                .iter()
                .any(|k| !matches!(k, Kind::Branch | Kind::Draft))
        {
            return Err(Error::Invalid);
        }
        hash(&serde_json::json!({"schema":"tig.paged-selection-enrollment.design-1","value":self}))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Paging {
    pub max_bindings: usize,
    pub max_bytes: usize,
}
impl Default for Paging {
    fn default() -> Self {
        Self {
            max_bindings: 256,
            max_bytes: 49152,
        }
    }
}
impl Paging {
    pub fn validate(&self) -> Result<()> {
        if self.max_bindings == 0
            || self.max_bindings > 1024
            || !(8192..=49152).contains(&self.max_bytes)
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
/// Exact input binding Tree. Its leaf objects are expected identities, not
/// payloads read from this metadata provider. Construction confers no authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub namespace: Digest,
    pub source: Key,
    pub predecessor_revision: u64,
    pub bindings: Object,
}
impl Selection {
    fn id(&self) -> Result<Digest> {
        if self.predecessor_revision == 0 || self.predecessor_revision > MAX_NUMBER {
            return Err(Error::Invalid);
        }
        hash(&serde_json::json!({"schema":"tig.paged-selection.design-1","value":self}))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    paging: Paging,
    roots: Object,
    count: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub namespace: Digest,
    pub source: Key,
    pub predecessor_revision: u64,
    pub original: source_claims::Selection,
    pub owner: Key,
    pub class: Kind,
    pub policy: Digest,
}
impl Origin {
    fn id(&self) -> Result<Digest> {
        hash(&serde_json::json!({"schema":"tig.paged-occurrence.design-1","value":self}))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cohort {
    pub origin: Origin,
    pub bindings: Object,
    pub count: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    pub object: Object,
    pub held_by_surviving_claims: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub roots: usize,
    pub bindings: usize,
    pub unique_objects: usize,
    pub held_objects: usize,
    pub unique_bytes: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: String,
    enrollment: Digest,
    request: Object,
    basis: claims::Roster,
    cohorts: Object,
    subjects: Object,
    summary: Summary,
    paging: Paging,
}
/// Validated historical metadata, not an authenticated decision or current lease.
pub struct Inventory {
    header: Header,
    cohorts: BTreeMap<Digest, Cohort>,
    subjects: BTreeMap<Digest, Subject>,
}
impl Inventory {
    pub fn summary(&self) -> &Summary {
        &self.header.summary
    }
    pub fn basis(&self) -> &claims::Roster {
        &self.header.basis
    }
    pub fn subjects(&self) -> impl Iterator<Item = &Subject> {
        self.subjects.values()
    }
    pub fn cohorts(&self) -> impl Iterator<Item = &Cohort> {
        self.cohorts.values()
    }
    pub fn authorizes_claim_release(&self) -> bool {
        false
    }
    pub fn authorizes_removal(&self) -> bool {
        false
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub manifest: Digest,
    pub occurrence: Digest,
    pub after: LogicalPath,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub path: LogicalPath,
    pub object: Object,
}
#[derive(Clone, Debug, Serialize)]
pub struct Page {
    pub schema: &'static str,
    pub manifest: Object,
    pub cohort: Cohort,
    pub bindings: Vec<Binding>,
    pub next: Option<Cursor>,
    pub historical_only: bool,
    pub authorizes_claim_release: bool,
    pub authorizes_removal: bool,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub source: protection::Limits,
    pub selections: usize,
    pub bindings: usize,
    pub objects: usize,
    pub writes: usize,
    pub write_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            source: protection::Limits {
                millis: 60000,
                roots: 4096,
                bindings: 500000,
                associations: 500000,
                reads: 500000,
                read_bytes: 128 * 1024 * 1024,
            },
            selections: 4096,
            bindings: 65536,
            objects: 65536,
            writes: 65536,
            write_bytes: 64 * 1024 * 1024,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        self.source.validate().map_err(|_| Error::Budget)?;
        if self.selections == 0
            || self.selections > 4096
            || self.selections > self.source.roots
            || self.bindings == 0
            || self.bindings > 65536
            || self.bindings > self.source.bindings
            || self.objects == 0
            || self.objects > self.bindings
            || self.writes > 65536
            || self.write_bytes > 64 * 1024 * 1024
        {
            return Err(Error::Budget);
        }
        Ok(())
    }
    fn tree(&self, entries: usize) -> TreeLimits {
        TreeLimits {
            max_entries: entries,
            max_reads: self.source.reads,
            max_writes: self.writes,
            max_read_bytes: self.source.read_bytes,
            max_write_bytes: self.write_bytes,
            max_map_depth: 64,
        }
    }
}
struct Work<'a, S> {
    nodes: &'a mut S,
    limits: &'a Limits,
    start: Instant,
    reads: Cell<usize>,
    bytes: Cell<usize>,
    writes: usize,
    write_bytes: usize,
}
impl<'a, S: NodeStore> Work<'a, S> {
    fn new(nodes: &'a mut S, limits: &'a Limits) -> Result<Self> {
        limits.validate()?;
        Ok(Self {
            nodes,
            limits,
            start: Instant::now(),
            reads: Cell::new(0),
            bytes: Cell::new(0),
            writes: 0,
            write_bytes: 0,
        })
    }
    fn timed(&self) -> TreeResult<()> {
        if self.start.elapsed() > Duration::from_millis(self.limits.source.millis) {
            Err(TreeError::Budget)
        } else {
            Ok(())
        }
    }
    fn read<T: serde::de::DeserializeOwned + Serialize>(&self, o: &Object) -> Result<T> {
        if o.bytes > MAX_DOC as u64 {
            return Err(Error::Budget);
        }
        let raw = self.get(o)?.ok_or(Error::Missing)?;
        let value = serde_json::from_value(identity::parse_json(&raw).map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
        if encode(&value)? != raw {
            return Err(Error::Invalid);
        }
        Ok(value)
    }
    fn doc(&mut self, v: &impl Serialize) -> Result<Object> {
        Ok(self.put(&encode(v)?)?)
    }
    fn entries(&mut self, root: &Object, count: usize) -> Result<BTreeMap<LogicalPath, Object>> {
        let limits = self.limits.tree(count);
        Ok(Tree::from_root(root.clone()).entries(self, limits)?.0)
    }
    fn tree(&mut self, map: &BTreeMap<LogicalPath, Object>, count: usize) -> Result<Object> {
        let limits = self.limits.tree(count);
        Ok(Tree::build(map, self, limits)?.0.root().clone())
    }
}
impl<S: NodeStore> NodeStore for Work<'_, S> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.timed()?;
        let reads = self.reads.get().checked_add(1).ok_or(TreeError::Budget)?;
        let bytes = self
            .bytes
            .get()
            .checked_add(usize::try_from(o.bytes).map_err(|_| TreeError::Budget)?)
            .ok_or(TreeError::Budget)?;
        if reads > self.limits.source.reads || bytes > self.limits.source.read_bytes {
            return Err(TreeError::Budget);
        }
        self.reads.set(reads);
        self.bytes.set(bytes);
        let raw = self.nodes.get(o)?;
        self.timed()?;
        if raw
            .as_ref()
            .is_some_and(|b| b.len() as u64 != o.bytes || Object::from_bytes(b) != *o)
        {
            return Err(TreeError::Integrity);
        }
        Ok(raw)
    }
    fn put(&mut self, b: &[u8]) -> TreeResult<Object> {
        self.timed()?;
        self.writes = self.writes.checked_add(1).ok_or(TreeError::Budget)?;
        self.write_bytes = self
            .write_bytes
            .checked_add(b.len())
            .ok_or(TreeError::Budget)?;
        if self.writes > self.limits.writes || self.write_bytes > self.limits.write_bytes {
            return Err(TreeError::Budget);
        }
        let o = self.nodes.put(b)?;
        self.timed()?;
        if o != Object::from_bytes(b) {
            return Err(TreeError::Integrity);
        }
        Ok(o)
    }
}
