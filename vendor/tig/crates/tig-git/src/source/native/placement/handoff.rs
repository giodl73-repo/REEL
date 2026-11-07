//! Exact working-owner withdrawal after admitted permanent protection, not eviction.
use super::{Demand, journal::Reads};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use tig_core::{
    Claim, Digest, Placement, View, identity,
    tree::{NodeStore, TreeLimits},
};
use tig_store::{
    AckLevel, Key,
    source_claims::{Command, Receipt, Selection, SourceJournal},
};
pub mod local;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Metadata,
    Budget,
    Scope,
    Stale,
    Protection,
    Journal(tig_store::source_claims::Error),
    Injected,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;
impl From<tig_store::source_claims::Error> for Error {
    fn from(e: tig_store::source_claims::Error) -> Self {
        Self::Journal(e)
    }
}
impl From<crate::source::Error> for Error {
    fn from(e: crate::source::Error) -> Self {
        match e {
            crate::source::Error::Budget => Self::Budget,
            _ => Self::Metadata,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stamp {
    pub source: Key,
    pub owner: Key,
    pub revision: u64,
    pub selection: Selection,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub journal: Digest,
    pub operation: Key,
    pub previous: Stamp,
    pub successor: Stamp,
    pub required: AckLevel,
}
impl Request {
    fn identity(&self) -> Result<String> {
        if self.previous.source == self.successor.source
            || self.previous.revision.checked_add(3).is_none()
        {
            return Err(Error::Scope);
        }
        let raw = identity::canonical_json(&serde_json::json!({
            "schema":"tig.working-handoff-intent.experimental-1","request":self
        }))
        .map_err(|_| Error::Metadata)?;
        Ok(format!("handoff/{}", Digest::blob(&raw).as_str()))
    }
    fn operation(&self, phase: &str) -> Result<Key> {
        let raw = identity::canonical_json(&serde_json::json!({
            "schema":"tig.working-handoff-operation.experimental-1","operation":self.operation,"phase":phase
        })).map_err(|_| Error::Metadata)?;
        Key::parse(&format!("handoff-{}", Digest::blob(&raw).as_str())).map_err(|_| Error::Metadata)
    }
    fn prepare(&self, intent: &Selection) -> Command {
        Command::Prepare {
            source: self.previous.source.clone(),
            owner: self.previous.owner.clone(),
            expected_revision: self.previous.revision,
            selection: intent.clone(),
        }
    }
    fn withdraw(&self) -> Command {
        Command::Withdraw {
            source: self.previous.source.clone(),
            owner: self.previous.owner.clone(),
            expected_revision: self.previous.revision + 2,
        }
    }
}
/// An explicitly admitted compiled provider. Guards must cover settled durable
/// retention at this exact place/object and survive until dropped. No discovery.
pub trait Protection {
    type Guard;
    fn verify(&mut self, placement: &Placement, required: AckLevel) -> Result<Self::Guard>;
    fn revalidate(&mut self, guard: &mut Self::Guard) -> Result<()>;
}
pub struct Completion<G> {
    receipt: Receipt,
    guards: Vec<G>,
    fresh: bool,
}
impl<G> Completion<G> {
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
    pub fn fresh_protection_acknowledged(&self) -> bool {
        self.fresh
    }
    pub fn held_guards(&self) -> usize {
        self.guards.len()
    }
    pub fn authorizes_eviction(&self) -> bool {
        false
    }
}
#[derive(Clone, Copy)]
pub enum Stage {
    Prepared,
    Protected,
    Aborted,
    Withdrawn,
}
#[derive(Clone, Copy)]
pub struct Limits {
    pub tree: TreeLimits,
    pub guards: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            tree: TreeLimits::default(),
            guards: 4096,
        }
    }
}

/// Writes only a candidate intent. Caller must finish/reopen the metadata provider.
pub fn prepare(
    request: &Request,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<Selection> {
    let id = request.identity()?;
    let mut demand =
        Demand::from_archive(&request.successor.selection, store, limits).map_err(Error::from)?;
    demand.claim.id = id;
    demand.archive(store, limits).map_err(Error::from)
}
fn current(journal: &mut SourceJournal, stamp: &Stamp) -> Result<()> {
    let source = journal.source(&stamp.source)?.ok_or(Error::Stale)?;
    if source.owner != stamp.owner
        || source.revision != stamp.revision
        || source.pending.is_some()
        || source
            .current
            .as_ref()
            .is_none_or(|v| v.selection != stamp.selection)
    {
        return Err(Error::Stale);
    }
    Ok(())
}
/// Host must retain this exact request/intent in its existing operation record
/// and supply independently finished metadata readback before calling.
pub fn execute<B: Protection>(
    request: &Request,
    intent: &Selection,
    journal: &mut SourceJournal,
    readback: &impl NodeStore,
    limits: Limits,
    provider: &mut B,
) -> Result<Completion<B::Guard>> {
    execute_with(request, intent, journal, readback, limits, provider, |_| {
        Ok(())
    })
}
/// Typed host progress/fault seam, never a manifest-loaded executable hook.
pub fn execute_with<B: Protection>(
    request: &Request,
    intent: &Selection,
    journal: &mut SourceJournal,
    readback: &impl NodeStore,
    limits: Limits,
    provider: &mut B,
    mut observe: impl FnMut(Stage) -> Result<()>,
) -> Result<Completion<B::Guard>> {
    let id = request.identity()?;
    if &request.journal != journal.identity() {
        return Err(Error::Scope);
    }
    if limits.guards == 0 || limits.guards > 65_536 {
        return Err(Error::Budget);
    }
    let mut reader = Reads::new(readback, limits.tree);
    let old = Demand::from_archive(&request.previous.selection, &mut reader, limits.tree)
        .map_err(Error::from)?;
    let new = Demand::from_archive(&request.successor.selection, &mut reader, limits.tree)
        .map_err(Error::from)?;
    let bound = Demand::from_archive(intent, &mut reader, limits.tree).map_err(Error::from)?;
    if bound.claim.id != id
        || bound.source_checkpoint != new.source_checkpoint
        || bound.record != new.record
        || bound.policy != new.policy
        || bound.working != new.working
        || bound.retained != new.retained
        || bound.working_places != new.working_places
        || bound.retained_places != new.retained_places
        || bound.claim.placements != new.claim.placements
    {
        return Err(Error::Scope);
    }
    let source = journal
        .source(&request.previous.source)?
        .ok_or(Error::Stale)?;
    if source.owner != request.previous.owner {
        return Err(Error::Scope);
    }
    if source.revision > request.previous.revision + 2 {
        // These old expected revisions cannot mutate newer source state. Exact
        // recorded commands may recover acknowledgments; changed reuse refuses.
        let prepared = journal.apply(request.operation("prepare")?, request.prepare(intent))?;
        if !prepared.replayed {
            return Err(Error::Stale);
        }
        let receipt = journal.apply(request.operation("withdraw")?, request.withdraw())?;
        if !receipt.replayed {
            return Err(Error::Stale);
        }
        return Ok(Completion {
            receipt,
            guards: Vec::new(),
            fresh: false,
        });
    }
    if source
        .current
        .as_ref()
        .is_none_or(|v| v.selection != request.previous.selection)
    {
        return Err(Error::Stale);
    }
    current(journal, &request.successor)?;
    let mut placements = BTreeSet::new();
    for placement in new.claim.placements.iter().chain(
        old.claim
            .placements
            .iter()
            .filter(|p| old.retained_places.contains(&p.place)),
    ) {
        placements.insert(placement.clone());
        if placements.len() > limits.guards {
            return Err(Error::Budget);
        }
    }
    View::new(
        0,
        false,
        vec![Claim {
            id: id.clone(),
            placements: placements.clone(),
        }],
    )
    .map_err(|_| Error::Scope)?;
    let prepared = journal.apply(request.operation("prepare")?, request.prepare(intent))?;
    observe(Stage::Prepared)?;
    let mut guards = Vec::new();
    for placement in &placements {
        guards.push(provider.verify(placement, request.required)?);
    }
    for guard in &mut guards {
        provider.revalidate(guard)?;
    }
    current(journal, &request.successor)?;
    observe(Stage::Protected)?;
    current(journal, &request.successor)?;
    journal.apply(
        request.operation("abort")?,
        Command::Abort {
            source: request.previous.source.clone(),
            owner: request.previous.owner.clone(),
            expected_revision: request.previous.revision + 1,
            prepared: prepared.reference,
        },
    )?;
    observe(Stage::Aborted)?;
    current(journal, &request.successor)?;
    let receipt = journal.apply(request.operation("withdraw")?, request.withdraw())?;
    observe(Stage::Withdrawn)?;
    Ok(Completion {
        receipt,
        guards,
        fresh: true,
    })
}
