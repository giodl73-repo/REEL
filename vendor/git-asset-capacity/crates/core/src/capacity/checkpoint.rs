//! Content-addressed bounded working state, not journal activation or authority.
use super::*;
use crate::{
    LogicalPath, Object, identity,
    tree::{NodeStore, Tree, TreeError, TreeLimits, TreeResult},
};
use serde::{Deserialize, Serialize};
use std::{cell::Cell, collections::BTreeSet};
pub const MAX_RECORD_BYTES: usize = 65_536;
pub const MAX_LEAVES: usize = 1 + MAX_RESERVATIONS + MAX_EVENTS + MAX_EVENTS;
pub const MAX_NODES: usize = 65_536;
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Budget,
    Integrity,
    Missing(Object),
    Store(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<TreeError> for Error {
    fn from(e: TreeError) -> Self {
        match e {
            TreeError::Budget => Self::Budget,
            TreeError::Integrity => Self::Integrity,
            TreeError::MissingNode(o) => Self::Missing(o),
            TreeError::Store(s) => Self::Store(s),
            _ => Self::Invalid,
        }
    }
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_leaves: usize,
    pub max_nodes: usize,
    pub max_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_leaves: MAX_LEAVES,
            max_nodes: MAX_NODES,
            max_bytes: MAX_BYTES,
        }
    }
}
impl Limits {
    pub(super) fn validate(self) -> Result<()> {
        if self.max_leaves > MAX_LEAVES || self.max_nodes > MAX_NODES || self.max_bytes > MAX_BYTES
        {
            Err(Error::Budget)
        } else {
            Ok(())
        }
    }
    pub(super) fn tree(self) -> TreeLimits {
        TreeLimits {
            max_entries: self.max_leaves,
            max_reads: self.max_nodes,
            max_writes: self.max_nodes,
            max_read_bytes: self.max_bytes,
            max_write_bytes: self.max_bytes,
            ..TreeLimits::default()
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Metrics {
    pub calls: usize,
    pub bytes: usize,
}
pub(super) struct Budget {
    limits: Limits,
    calls: Cell<usize>,
    bytes: Cell<usize>,
}
impl Budget {
    pub(super) fn new(limits: Limits) -> Self {
        Self {
            limits,
            calls: Cell::new(0),
            bytes: Cell::new(0),
        }
    }
    fn charge(&self, bytes: usize) -> TreeResult<()> {
        if bytes > MAX_RECORD_BYTES {
            return Err(TreeError::Budget);
        }
        let n = self.calls.get().checked_add(1).ok_or(TreeError::Budget)?;
        let b = self
            .bytes
            .get()
            .checked_add(bytes)
            .ok_or(TreeError::Budget)?;
        if n > self.limits.max_nodes || b > self.limits.max_bytes {
            return Err(TreeError::Budget);
        }
        self.calls.set(n);
        self.bytes.set(b);
        Ok(())
    }
    pub(super) fn metrics(&self) -> Metrics {
        Metrics {
            calls: self.calls.get(),
            bytes: self.bytes.get(),
        }
    }
}
pub(super) struct Meter<'a, S> {
    pub(super) store: &'a mut S,
    pub(super) budget: Budget,
}
impl<S: NodeStore> NodeStore for Meter<'_, S> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        let size = usize::try_from(o.bytes).map_err(|_| TreeError::Budget)?;
        self.budget.charge(size)?;
        let v = self.store.get(o)?;
        if v.as_ref().is_some_and(|b| Object::from_bytes(b) != *o) {
            return Err(TreeError::Integrity);
        }
        Ok(v)
    }
    fn put(&mut self, b: &[u8]) -> TreeResult<Object> {
        self.budget.charge(b.len())?;
        let o = self.store.put(b)?;
        if o != Object::from_bytes(b) {
            return Err(TreeError::Integrity);
        }
        Ok(o)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    root: Object,
}
impl Checkpoint {
    pub fn from_root(root: Object) -> Self {
        Self { root }
    }
    pub fn root(&self) -> &Object {
        &self.root
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema: String,
    entry: Entry,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Entry {
    Header {
        domain: String,
        epoch: String,
        floor: u64,
        next_generation: u64,
        allocation_sequence: u64,
        observation_nonce: u64,
        observed_through: u64,
        pending_observation: Option<u64>,
        sampled_free: Option<u64>,
        reservation_count: usize,
        event_count: usize,
        debit_count: usize,
    },
    Reservation {
        id: String,
        owner: String,
        generation: u64,
        peak: u64,
        remaining: u64,
        closed: bool,
    },
    Allocation {
        generation: u64,
        event: String,
        limit: u64,
        completed: Option<Completed>,
    },
    Debit {
        sequence: u64,
        bytes: u64,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Completed {
    charge: u64,
    sequence: u64,
}
impl Entry {
    fn path(&self) -> Result<LogicalPath> {
        let p = match self {
            Self::Header { .. } => "header".to_owned(),
            Self::Reservation { generation, .. } => format!("r/{generation}"),
            Self::Allocation {
                generation, event, ..
            } => format!(
                "a/{generation}/{}",
                crate::Digest::blob(event.as_bytes()).as_str()
            ),
            Self::Debit { sequence, .. } => format!("d/{sequence}"),
        };
        LogicalPath::parse(&p).map_err(|_| Error::Invalid)
    }
}
fn bytes(entry: Entry) -> Result<Vec<u8>> {
    identity::canonical_json(
        &serde_json::to_value(Document {
            schema: "tig.capacity-state.design-1".into(),
            entry,
        })
        .map_err(|_| Error::Invalid)?,
    )
    .map_err(|_| Error::Invalid)
}
fn put(
    entry: Entry,
    store: &mut impl NodeStore,
    entries: &mut BTreeMap<LogicalPath, Object>,
    limit: usize,
) -> Result<()> {
    if entries.len() >= limit {
        return Err(Error::Budget);
    }
    let path = entry.path()?;
    let object = store.put(&bytes(entry)?)?;
    if entries.insert(path, object).is_some() {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn get(path: &LogicalPath, object: &Object, store: &impl NodeStore) -> Result<Entry> {
    let raw = store
        .get(object)?
        .ok_or_else(|| Error::Missing(object.clone()))?;
    let v = identity::parse_json(&raw).map_err(|_| Error::Invalid)?;
    let d: Document = serde_json::from_value(v).map_err(|_| Error::Invalid)?;
    if d.schema != "tig.capacity-state.design-1" || d.entry.path()? != *path {
        return Err(Error::Invalid);
    }
    if bytes_ref(&d)? != raw {
        return Err(Error::Invalid);
    }
    Ok(d.entry)
}
fn bytes_ref(d: &Document) -> Result<Vec<u8>> {
    identity::canonical_json(&serde_json::to_value(d).map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)
}
impl CapacityDomain {
    /// Exports immutable metadata only. Caller supplies an admitted metadata port.
    /// A returned root is not a current journal head, fresh probe or write authority.
    pub fn export_checkpoint(
        &self,
        store: &mut impl NodeStore,
        limits: Limits,
    ) -> Result<(Checkpoint, Metrics)> {
        limits.validate()?;
        validate(self)?;
        let mut metered = Meter {
            store,
            budget: Budget::new(limits),
        };
        let mut entries = BTreeMap::new();
        put(
            Entry::Header {
                domain: self.domain.clone(),
                epoch: self.epoch.clone(),
                floor: self.floor,
                next_generation: self.next_generation,
                allocation_sequence: self.allocation_sequence,
                observation_nonce: self.observation_nonce,
                observed_through: self.observed_through,
                pending_observation: self.pending_observation,
                sampled_free: self.sampled_free,
                reservation_count: self.records.len(),
                event_count: self.event_count,
                debit_count: self.debits.len(),
            },
            &mut metered,
            &mut entries,
            limits.max_leaves,
        )?;
        for r in self.records.values() {
            put(
                Entry::Reservation {
                    id: r.token.id.clone(),
                    owner: r.token.owner.clone(),
                    generation: r.token.generation,
                    peak: r.peak,
                    remaining: r.remaining,
                    closed: r.closed,
                },
                &mut metered,
                &mut entries,
                limits.max_leaves,
            )?;
            for (name, e) in &r.events {
                put(
                    Entry::Allocation {
                        generation: r.token.generation,
                        event: name.clone(),
                        limit: e.limit,
                        completed: e
                            .completed
                            .map(|(charge, sequence)| Completed { charge, sequence }),
                    },
                    &mut metered,
                    &mut entries,
                    limits.max_leaves,
                )?;
            }
        }
        for (sequence, charge) in &self.debits {
            put(
                Entry::Debit {
                    sequence: *sequence,
                    bytes: *charge,
                },
                &mut metered,
                &mut entries,
                limits.max_leaves,
            )?;
        }
        let (tree, _) = Tree::build(&entries, &mut metered, limits.tree())?;
        Ok((
            Checkpoint {
                root: tree.root().clone(),
            },
            metered.budget.metrics(),
        ))
    }
}
/// Decoded historical working state. The writable model is only exposed after
/// invalidating sampled-free and pending observation admission.
pub struct Decoded {
    root: Object,
    domain: CapacityDomain,
    metrics: Metrics,
}
impl Decoded {
    pub fn root(&self) -> &Object {
        &self.root
    }
    pub fn metrics(&self) -> Metrics {
        self.metrics
    }
    pub fn recorded_accounting(&self) -> super::Result<CapacitySummary> {
        self.domain.summary()
    }
    /// Compare with independently admitted original context, not fields read
    /// back from this checkpoint. This proves consistency, not current authority.
    pub fn matches_context(&self, domain: &str, epoch: &str, floor: u64) -> bool {
        self.domain.domain == domain && self.domain.epoch == epoch && self.domain.floor == floor
    }
    /// Bind restored accounting to its expected context and invalidate old
    /// observation admission before returning the model. No IO or new reservation.
    pub fn into_domain_for_context(
        self,
        domain: &str,
        epoch: &str,
        floor: u64,
    ) -> Result<CapacityDomain> {
        if !self.matches_context(domain, epoch, floor) {
            return Err(Error::Invalid);
        }
        Ok(self.into_domain())
    }
    pub fn into_domain(mut self) -> CapacityDomain {
        self.domain.invalidate_observation();
        self.domain
    }
}
impl Checkpoint {
    pub fn read(&self, store: &mut impl NodeStore, limits: Limits) -> Result<Decoded> {
        limits.validate()?;
        let mut metered = Meter {
            store,
            budget: Budget::new(limits),
        };
        let (mut entries, _) =
            Tree::from_root(self.root.clone()).entries(&mut metered, limits.tree())?;
        let header = LogicalPath::parse("header").map_err(|_| Error::Invalid)?;
        let object = entries.remove(&header).ok_or(Error::Invalid)?;
        let Entry::Header {
            domain,
            epoch,
            floor,
            next_generation,
            allocation_sequence,
            observation_nonce,
            observed_through,
            pending_observation,
            sampled_free,
            reservation_count,
            event_count,
            debit_count,
        } = get(&header, &object, &metered)?
        else {
            return Err(Error::Invalid);
        };
        if reservation_count > MAX_RESERVATIONS
            || event_count > MAX_EVENTS
            || debit_count > MAX_EVENTS
            || reservation_count
                .checked_add(event_count)
                .and_then(|n| n.checked_add(debit_count))
                != Some(entries.len())
        {
            return Err(Error::Invalid);
        }
        let mut state = CapacityDomain {
            domain,
            epoch,
            floor,
            next_generation,
            allocation_sequence,
            observation_nonce,
            observed_through,
            pending_observation,
            sampled_free,
            records: BTreeMap::new(),
            debits: BTreeMap::new(),
            event_count,
        };
        let mut generations = BTreeMap::new();
        let mut allocations = Vec::new();
        for (path, object) in entries {
            match get(&path, &object, &metered)? {
                Entry::Reservation {
                    id,
                    owner,
                    generation,
                    peak,
                    remaining,
                    closed,
                } => {
                    if generations.insert(generation, id.clone()).is_some() {
                        return Err(Error::Invalid);
                    }
                    let token = ReservationToken {
                        domain: state.domain.clone(),
                        epoch: state.epoch.clone(),
                        id: id.clone(),
                        owner,
                        generation,
                    };
                    if state
                        .records
                        .insert(
                            id,
                            Reservation {
                                token,
                                peak,
                                remaining,
                                closed,
                                events: BTreeMap::new(),
                            },
                        )
                        .is_some()
                    {
                        return Err(Error::Invalid);
                    }
                }
                Entry::Allocation {
                    generation,
                    event,
                    limit,
                    completed,
                } => {
                    if allocations.len() >= MAX_EVENTS {
                        return Err(Error::Budget);
                    }
                    allocations.push((generation, event, limit, completed));
                }
                Entry::Debit { sequence, bytes } => {
                    if state.debits.insert(sequence, bytes).is_some() {
                        return Err(Error::Invalid);
                    }
                }
                Entry::Header { .. } => return Err(Error::Invalid),
            }
        }
        if state.records.len() != reservation_count
            || allocations.len() != event_count
            || state.debits.len() != debit_count
        {
            return Err(Error::Invalid);
        }
        for (generation, event, limit, completed) in allocations {
            let id = generations.get(&generation).ok_or(Error::Invalid)?;
            let r = state.records.get_mut(id).ok_or(Error::Invalid)?;
            if r.events
                .insert(
                    event,
                    Event {
                        limit,
                        completed: completed.map(|c| (c.charge, c.sequence)),
                    },
                )
                .is_some()
            {
                return Err(Error::Invalid);
            }
        }
        validate(&state)?;
        Ok(Decoded {
            root: self.root.clone(),
            domain: state,
            metrics: metered.budget.metrics(),
        })
    }
}
fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).ok_or(Error::Invalid)
}
fn validate(d: &CapacityDomain) -> Result<()> {
    if !crate::token(&d.domain)
        || !crate::token(&d.epoch)
        || d.records.len() > MAX_RESERVATIONS
        || d.event_count > MAX_EVENTS
        || d.debits.len() > MAX_EVENTS
        || d.observed_through > d.allocation_sequence
        || d.pending_observation
            .is_some_and(|n| n == 0 || n != d.observation_nonce)
    {
        return Err(Error::Invalid);
    }
    let mut generations = BTreeSet::new();
    let mut completed = BTreeMap::new();
    let mut events = 0usize;
    let mut remaining = 0u64;
    for (id, r) in &d.records {
        if !crate::token(id)
            || !crate::token(&r.token.owner)
            || r.token.id != *id
            || r.token.domain != d.domain
            || r.token.epoch != d.epoch
            || r.token.generation == 0
            || r.token.generation > d.next_generation
            || !generations.insert(r.token.generation)
            || r.peak == 0
            || r.remaining > r.peak
        {
            return Err(Error::Invalid);
        }
        events = events.checked_add(r.events.len()).ok_or(Error::Budget)?;
        if events > MAX_EVENTS {
            return Err(Error::Budget);
        }
        remaining = add(remaining, r.remaining)?;
        let (mut charged, mut in_flight) = (0u64, 0u64);
        for (id, e) in &r.events {
            if !crate::token(id) || e.limit == 0 {
                return Err(Error::Invalid);
            }
            if let Some((charge, sequence)) = e.completed {
                if charge > e.limit
                    || sequence == 0
                    || sequence > d.allocation_sequence
                    || completed.insert(sequence, charge).is_some()
                {
                    return Err(Error::Invalid);
                }
                charged = add(charged, charge)?;
            } else {
                in_flight = add(in_flight, e.limit)?;
            }
        }
        if charged > r.peak
            || in_flight > r.remaining
            || r.closed && (r.remaining != 0 || in_flight != 0)
            || !r.closed && r.peak.checked_sub(charged) != Some(r.remaining)
        {
            return Err(Error::Invalid);
        }
    }
    if events != d.event_count {
        return Err(Error::Invalid);
    }
    let mut debits = 0u64;
    for (sequence, charge) in &d.debits {
        if *sequence <= d.observed_through
            || *sequence > d.allocation_sequence
            || *charge == 0
            || completed.get(sequence).is_some_and(|c| c != charge)
        {
            return Err(Error::Invalid);
        }
        debits = add(debits, *charge)?;
    }
    for (sequence, charge) in completed {
        if sequence > d.observed_through && charge > 0 && d.debits.get(&sequence) != Some(&charge) {
            return Err(Error::Invalid);
        }
    }
    add(remaining, debits)?;
    Ok(())
}

#[cfg(test)]
mod tests;
