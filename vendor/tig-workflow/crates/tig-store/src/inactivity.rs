//! Typed history descriptors in a dedicated source journal. Observations, not leases.
use crate::{
    Key,
    source_claims::{self, Selection, Source, SourceJournal},
};
use serde::{Deserialize, Serialize};
use tig_core::{
    Digest, Object, identity,
    retirement::history::{Checkpoint, History, archive},
    tree::{NodeStore, TreeError},
};

pub mod publication;

const SCHEMA: &str = "tig.inactivity-head.design-1";
const MAX_BYTES: u64 = 65_536;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Journal(source_claims::Error),
    Metadata(TreeError),
    Archive(archive::Error),
    Provider(crate::metadata::Error),
    Lineage,
    Budget,
    Invalid,
    Integrity,
    Missing,
    Owner,
    Stale,
    Pending,
    Cancelled,
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    journal: Digest,
    source: Key,
    owner: Key,
    history: Object,
    checkpoint: Checkpoint,
}
#[derive(Clone, Debug)]
pub struct Candidate {
    journal: Digest,
    source: Key,
    owner: Key,
    selection: Selection,
    checkpoint: Checkpoint,
}
impl Candidate {
    pub fn selection(&self) -> Selection {
        self.selection.clone()
    }
    pub fn checkpoint(&self) -> Checkpoint {
        self.checkpoint.clone()
    }
}
#[derive(Clone, Debug)]
pub struct ObservedHead {
    journal: Digest,
    source: Key,
    state: Source,
    history: History,
}
impl ObservedHead {
    pub fn history(&self) -> &History {
        &self.history
    }
    pub fn revision(&self) -> u64 {
        self.state.revision
    }
    /// Read-only change detection. Does not hold a lease or prevent later changes.
    pub fn revalidate(&self, journal: &mut SourceJournal) -> Result<()> {
        if journal.identity() != &self.journal {
            return Err(Error::Stale);
        }
        let current = journal.source(&self.source).map_err(Error::Journal)?;
        if current.as_ref() != Some(&self.state) {
            return Err(Error::Stale);
        }
        Ok(())
    }
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    identity::canonical_json(&serde_json::to_value(value).map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)
}
fn context(record: &Record) -> Result<Digest> {
    Ok(Digest::blob(&encode(&(
        "tig.inactivity-head-context.design-1",
        &record.journal,
        &record.source,
        &record.owner,
        &record.checkpoint,
    ))?))
}
/// Stage only; provider finish/readback must precede journal preparation/adoption.
pub fn stage(
    history: &History,
    journal: &SourceJournal,
    source: &Key,
    owner: &Key,
    nodes: &mut impl NodeStore,
) -> Result<Candidate> {
    let history_root = archive::stage(history, nodes).map_err(Error::Archive)?;
    let record = Record {
        schema: SCHEMA.into(),
        journal: journal.identity().clone(),
        source: source.clone(),
        owner: owner.clone(),
        history: history_root,
        checkpoint: history.checkpoint(),
    };
    let bytes = encode(&record)?;
    let root = Object::from_bytes(&bytes);
    let acknowledged = nodes.put(&bytes).map_err(Error::Metadata)?;
    if acknowledged != root {
        return Err(Error::Integrity);
    }
    if nodes.get(&root).map_err(Error::Metadata)?.as_deref() != Some(bytes.as_slice()) {
        return Err(Error::Integrity);
    }
    Ok(Candidate {
        journal: record.journal.clone(),
        source: record.source.clone(),
        owner: record.owner.clone(),
        selection: Selection {
            root,
            context: context(&record)?,
        },
        checkpoint: record.checkpoint,
    })
}
pub fn read_current(
    journal: &mut SourceJournal,
    nodes: &impl NodeStore,
    source: &Key,
    owner: &Key,
    expected_revision: u64,
) -> Result<ObservedHead> {
    let state = journal
        .source(source)
        .map_err(Error::Journal)?
        .ok_or(Error::Missing)?;
    if &state.owner != owner {
        return Err(Error::Owner);
    }
    if state.cancelled {
        return Err(Error::Cancelled);
    }
    if state.revision != expected_revision {
        return Err(Error::Stale);
    }
    if state.pending.is_some() {
        return Err(Error::Pending);
    }
    let selection = &state.current.as_ref().ok_or(Error::Missing)?.selection;
    let history = load(journal.identity(), nodes, source, owner, selection)?;
    let observed = ObservedHead {
        journal: journal.identity().clone(),
        source: source.clone(),
        state,
        history,
    };
    observed.revalidate(journal)?;
    Ok(observed)
}

fn load(
    journal: &Digest,
    nodes: &impl NodeStore,
    source: &Key,
    owner: &Key,
    selection: &Selection,
) -> Result<History> {
    if selection.root.bytes == 0 || selection.root.bytes > MAX_BYTES {
        return Err(Error::Budget);
    }
    let bytes = nodes
        .get(&selection.root)
        .map_err(Error::Metadata)?
        .ok_or(Error::Missing)?;
    if bytes.len() != selection.root.bytes as usize || Object::from_bytes(&bytes) != selection.root
    {
        return Err(Error::Integrity);
    }
    let value = identity::parse_json(&bytes).map_err(|_| Error::Invalid)?;
    if identity::canonical_json(&value).map_err(|_| Error::Invalid)? != bytes {
        return Err(Error::Invalid);
    }
    let record: Record = serde_json::from_value(value).map_err(|_| Error::Invalid)?;
    if record.schema != SCHEMA
        || &record.journal != journal
        || &record.source != source
        || &record.owner != owner
        || context(&record)? != selection.context
    {
        return Err(Error::Invalid);
    }
    archive::restore(&record.history, nodes, &record.checkpoint).map_err(Error::Archive)
}
