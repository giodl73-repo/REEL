//! Explicit immutable original-result export and bounded historical point lookup.
//! No activation, pruning, live probes or provider authority.
use super::*;
use git_asset_core::{
    LogicalPath, Object,
    capacity::{checkpoint as state, history as core_history},
    tree::{NodeStore, Tree, TreeError, TreeLimits, TreeResult},
};
use std::{cell::Cell, rc::Rc};
pub const MAX_CALLS: usize = 262144;
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
#[derive(Debug)]
pub enum Error {
    Journal(LedgerError),
    State(state::Error),
    Invalid,
    Budget,
    Integrity,
    Missing(Object),
    Store(String),
}
impl From<LedgerError> for Error {
    fn from(e: LedgerError) -> Self {
        Self::Journal(e)
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Journal(e.into())
    }
}
impl From<state::Error> for Error {
    fn from(e: state::Error) -> Self {
        Self::State(e)
    }
}
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
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_calls: usize,
    pub max_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_calls: MAX_CALLS,
            max_bytes: MAX_BYTES,
        }
    }
}
impl Limits {
    fn validate(self) -> Result<()> {
        if self.max_calls > MAX_CALLS || self.max_bytes > MAX_BYTES {
            Err(Error::Budget)
        } else {
            Ok(())
        }
    }
    fn tree(self) -> TreeLimits {
        TreeLimits {
            max_entries: MAX_COMMANDS as usize,
            max_reads: self.max_calls,
            max_writes: self.max_calls,
            max_read_bytes: self.max_bytes,
            max_write_bytes: self.max_bytes,
            ..TreeLimits::default()
        }
    }
    fn point(self) -> Self {
        Self {
            max_calls: self.max_calls.min(256),
            max_bytes: self.max_bytes.min(1024 * 1024),
        }
    }
}
pub(in crate::ledger) struct Meter<'a, S> {
    nodes: &'a mut S,
    limits: Limits,
    calls: Rc<Cell<usize>>,
    bytes: Rc<Cell<usize>>,
}
impl<'a, S> Meter<'a, S> {
    pub(in crate::ledger) fn new(nodes: &'a mut S, limits: Limits) -> Result<Self> {
        limits.validate()?;
        Ok(Self {
            nodes,
            limits,
            calls: Rc::new(Cell::new(0)),
            bytes: Rc::new(Cell::new(0)),
        })
    }
    fn fork<'b, T>(&self, nodes: &'b mut T) -> Meter<'b, T> {
        Meter {
            nodes,
            limits: self.limits,
            calls: self.calls.clone(),
            bytes: self.bytes.clone(),
        }
    }
    fn charge(&self, bytes: usize) -> TreeResult<()> {
        let n = self.calls.get().checked_add(1).ok_or(TreeError::Budget)?;
        let b = self
            .bytes
            .get()
            .checked_add(bytes)
            .ok_or(TreeError::Budget)?;
        if bytes > 65536 || n > self.limits.max_calls || b > self.limits.max_bytes {
            return Err(TreeError::Budget);
        }
        self.calls.set(n);
        self.bytes.set(b);
        Ok(())
    }
    pub(in crate::ledger) fn metrics(&self) -> state::Metrics {
        state::Metrics {
            calls: self.calls.get(),
            bytes: self.bytes.get(),
        }
    }
}
impl<S: NodeStore> NodeStore for Meter<'_, S> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.charge(usize::try_from(o.bytes).map_err(|_| TreeError::Budget)?)?;
        let b = self.nodes.get(o)?;
        if b.as_ref().is_some_and(|b| Object::from_bytes(b) != *o) {
            return Err(TreeError::Integrity);
        }
        Ok(b)
    }
    fn put(&mut self, b: &[u8]) -> TreeResult<Object> {
        self.charge(b.len())?;
        let o = self.nodes.put(b)?;
        if o != Object::from_bytes(b) {
            return Err(TreeError::Integrity);
        }
        Ok(o)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    schema: String,
    config: Config,
    journal: Checkpoint,
    working_state: state::Checkpoint,
    operations: Object,
    operation_count: u64,
    encoded_request_bytes: u64,
    clock_high: Option<u64>,
}
impl Snapshot {
    pub fn journal(&self) -> &Checkpoint {
        &self.journal
    }
    pub fn working_state(&self) -> &state::Checkpoint {
        &self.working_state
    }
    pub fn operations(&self) -> &Object {
        &self.operations
    }
    /// Historical accepted lower bound; never current-time authority.
    pub fn clock_high_water(&self) -> Option<u64> {
        self.clock_high
    }
    fn validate(&self) -> Result<()> {
        let clock = match (self.config.schema.as_str(), &self.config.clock) {
            ("tig.capacity-ledger.design-1", None) => false,
            ("tig.capacity-ledger.design-2", Some(p)) if p.id().is_ok() => true,
            _ => return Err(Error::Invalid),
        };
        if self.schema != "tig.capacity-history.design-1"
            || Digest::blob(&encode(&self.config)?) != self.journal.genesis
            || self.operation_count != self.journal.sequence
            || self.operation_count > MAX_COMMANDS
            || self.encoded_request_bytes > MAX_JOURNAL_BYTES
            || self.clock_high == Some(0)
            || !clock && self.clock_high.is_some()
            || self.operations.bytes == 0
            || self.operations.bytes > 65536
            || self.working_state.root().bytes == 0
            || self.working_state.root().bytes > 65536
        {
            return Err(Error::Invalid);
        }
        if self.operation_count == 0
            && (self.journal.hash != self.journal.genesis
                || self.encoded_request_bytes != 0
                || self.clock_high.is_some())
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.validate()?;
        Ok(encode(self)?)
    }
    /// Framing checks only. Admit the exact descriptor and use validate_all for full segment validation.
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        let s: Self = decode(raw)?;
        s.validate()?;
        Ok(s)
    }
    /// Complete validation of this bounded original segment. The caller's port
    /// is read-only; deterministic proof writes use private memory and the same
    /// shared budget. This is explicit validation, not a lazy lookup or activation.
    pub fn validate_all(
        &self,
        nodes: &mut impl NodeStore,
        limits: Limits,
    ) -> Result<state::Metrics> {
        self.validate_all_with_proof(
            nodes,
            &mut git_asset_core::tree::MemoryNodes::default(),
            limits,
        )
    }
    fn validate_all_with_proof(
        &self,
        nodes: &mut impl NodeStore,
        proof: &mut impl NodeStore,
        limits: Limits,
    ) -> Result<state::Metrics> {
        self.validate()?;
        let mut meter = Meter::new(nodes, limits)?;
        self.working_state
            .read(&mut meter, state::Limits::default())?;
        let (entries, _) =
            Tree::from_root(self.operations.clone()).entries(&mut meter, limits.tree())?;
        if entries.len() as u64 != self.operation_count {
            return Err(Error::Invalid);
        }
        let mut items = Vec::new();
        for (p, o) in entries {
            let raw = meter.get(&o)?.ok_or_else(|| Error::Missing(o.clone()))?;
            let item: Item = decode(&raw)?;
            if item.schema != "tig.capacity-history-op.design-1"
                || operation_path(&item.request.operation)? != p
            {
                return Err(Error::Invalid);
            }
            item.output
                .validate(&item.request, &self.config, item.sequence)?;
            items.push(item);
        }
        items.sort_by_key(|i| i.sequence);
        let mut domain = CapacityDomain::new(
            self.config.domain.as_str(),
            self.config.instance.as_str(),
            self.config.floor,
        )
        .map_err(LedgerError::from)?;
        let mut clock = ClockState {
            enabled: self.config.clock.is_some(),
            high: None,
        };
        let mut outputs = BTreeMap::new();
        let mut bytes = 0u64;
        let mut previous = self.journal.genesis.clone();
        for (i, item) in items.into_iter().enumerate() {
            if item.sequence != i as u64 + 1
                || item.previous != previous
                || event_hash(item.sequence, &previous, &item.request)? != item.hash
                || outputs.contains_key(item.request.operation.as_str())
            {
                return Err(Error::Invalid);
            }
            bytes = bytes
                .checked_add(encode(&item.request)?.len() as u64)
                .ok_or(Error::Budget)?;
            if bytes > MAX_JOURNAL_BYTES {
                return Err(Error::Budget);
            }
            let output = execute(&mut domain, &outputs, &item.request, &clock)?;
            let expected = ArchivedOutput::capture(&output, &mut meter.fork(proof))?;
            if encode(&expected)? != encode(&item.output)? {
                return Err(Error::Invalid);
            }
            if let ArchivedOutput::Retired { archive } = &item.output {
                archive.read(&mut meter, state::Limits::default())?;
            }
            clock.advance(&output);
            outputs.insert(item.request.operation.as_str().to_owned(), output);
            previous = item.hash;
        }
        let (expected, _) =
            domain.export_checkpoint(&mut meter.fork(proof), state::Limits::default())?;
        if previous != self.journal.hash
            || bytes != self.encoded_request_bytes
            || clock.high != self.clock_high
            || expected != self.working_state
        {
            return Err(Error::Invalid);
        }
        Ok(meter.metrics())
    }
    /// Selected original receipt/dependencies under an exact admitted descriptor.
    /// This is not full closure/provenance validation or new admission.
    pub fn inspect(
        &self,
        nodes: &mut impl NodeStore,
        operation: &Key,
        limits: Limits,
    ) -> Result<Inspection> {
        self.validate()?;
        limits.validate()?;
        let limits = limits.point();
        let mut meter = Meter::new(nodes, limits)?;
        let item = self.checked(&mut meter, operation, limits, 0)?;
        let response = item.output.response();
        let request = encode(&item.request)?;
        Ok(Inspection {
            receipt: Receipt {
                reference: JournalRef {
                    genesis: self.journal.genesis.clone(),
                    operation: operation.clone(),
                },
                checkpoint: Checkpoint {
                    genesis: self.journal.genesis.clone(),
                    sequence: item.sequence,
                    hash: item.hash,
                },
                response,
                replayed: true,
            },
            request,
            output: item.output,
            metrics: meter.metrics(),
        })
    }
    fn item(&self, nodes: &mut impl NodeStore, operation: &Key, limits: Limits) -> Result<Item> {
        let p = operation_path(operation)?;
        let (o, _) = Tree::from_root(self.operations.clone()).get(&p, nodes, limits.tree())?;
        let o = o.ok_or(Error::Invalid)?;
        let raw = nodes.get(&o)?.ok_or_else(|| Error::Missing(o.clone()))?;
        let item: Item = decode(&raw)?;
        if item.schema != "tig.capacity-history-op.design-1"
            || item.request.operation != *operation
            || item.sequence == 0
            || item.sequence > self.operation_count
            || item.sequence == 1 && item.previous != self.journal.genesis
            || item.sequence == self.operation_count && item.hash != self.journal.hash
            || event_hash(item.sequence, &item.previous, &item.request)? != item.hash
        {
            return Err(Error::Invalid);
        }
        item.output
            .validate(&item.request, &self.config, item.sequence)?;
        Ok(item)
    }
    /// Reads one retired allocation outcome under one shared512-call/4MiB budget.
    pub fn retired_outcome(
        &self,
        nodes: &mut impl NodeStore,
        operation: &Key,
        ticket: &AllocationTicket,
        mut limits: Limits,
    ) -> Result<(core_history::Outcome, state::Metrics)> {
        self.validate()?;
        limits.validate()?;
        limits.max_calls = limits.max_calls.min(512);
        limits.max_bytes = limits.max_bytes.min(4 * 1024 * 1024);
        let mut meter = Meter::new(nodes, limits)?;
        let item = self.checked(&mut meter, operation, limits, 0)?;
        let ArchivedOutput::Retired { archive } = item.output else {
            return Err(Error::Invalid);
        };
        let (outcome, _) = archive.outcome(&mut meter, ticket, state::Limits::default())?;
        Ok((outcome, meter.metrics()))
    }
}
// Shared typed checks; callers retain their own format, range and IO budgets.
trait DependencyView {
    fn load_item(
        &self,
        nodes: &mut impl NodeStore,
        operation: &Key,
        limits: Limits,
    ) -> Result<Item>;
    fn contains_operation(
        &self,
        nodes: &mut impl NodeStore,
        operation: &Key,
        limits: Limits,
    ) -> Result<bool>;
    fn dependency(
        &self,
        nodes: &mut impl NodeStore,
        operation: &Key,
        sequence: u64,
        limits: Limits,
        depth: usize,
    ) -> Result<Item> {
        let item = self.checked(nodes, operation, limits, depth + 1)?;
        if item.sequence >= sequence {
            return Err(Error::Invalid);
        }
        Ok(item)
    }
    fn checked(
        &self,
        nodes: &mut impl NodeStore,
        operation: &Key,
        limits: Limits,
        depth: usize,
    ) -> Result<Item> {
        if depth > 4 {
            return Err(Error::Budget);
        }
        let item = self.load_item(nodes, operation, limits)?;
        match &item.request.command {
            Command::BeginAllocation { reservation, .. }
            | Command::BeginPublication { reservation, .. }
            | Command::BeginRetention { reservation, .. }
            | Command::BeginHandoff { reservation, .. } => {
                let dep = self.dependency(nodes, reservation, item.sequence, limits, depth)?;
                let ArchivedOutput::Reservation { token } = dep.output else {
                    return Err(Error::Invalid);
                };
                let ticket = match &item.output {
                    ArchivedOutput::Allocation { ticket }
                    | ArchivedOutput::Publication { ticket, .. }
                    | ArchivedOutput::Retention { ticket, .. }
                    | ArchivedOutput::Handoff { ticket, .. } => ticket,
                    _ => return Err(Error::Invalid),
                };
                if ticket.reservation() != &token {
                    return Err(Error::Invalid);
                }
            }
            Command::CompleteAllocation { allocation, actual } => {
                let dep = self.dependency(nodes, allocation, item.sequence, limits, depth)?;
                let ArchivedOutput::Allocation { ticket } = dep.output else {
                    return Err(Error::Invalid);
                };
                if *actual > ticket.limit() {
                    return Err(Error::Invalid);
                }
            }
            Command::SettleFootprint {
                allocation,
                binding,
                ..
            }
            | Command::SettleRetention {
                allocation,
                binding,
                ..
            }
            | Command::SettleHandoff {
                allocation,
                binding,
                ..
            }
            | Command::BeginCleanup {
                allocation,
                binding,
                ..
            }
            | Command::CompleteCleanup {
                allocation,
                binding,
                ..
            } => {
                let dep = self.dependency(nodes, allocation, item.sequence, limits, depth)?;
                let (ticket, expected) = match (&item.request.command, dep.output) {
                    (
                        Command::SettleRetention { .. },
                        ArchivedOutput::Retention { ticket, binding },
                    )
                    | (
                        Command::SettleHandoff { .. },
                        ArchivedOutput::Handoff { ticket, binding },
                    ) => (ticket, binding),
                    (
                        Command::SettleFootprint { .. }
                        | Command::BeginCleanup { .. }
                        | Command::CompleteCleanup { .. },
                        ArchivedOutput::Publication { ticket, binding },
                    ) => (ticket, binding),
                    _ => return Err(Error::Invalid),
                };
                if binding != &expected {
                    return Err(Error::Invalid);
                }
                match &item.request.command {
                    Command::SettleFootprint { footprint, .. }
                    | Command::SettleRetention { footprint, .. }
                    | Command::SettleHandoff { footprint, .. }
                    | Command::CompleteCleanup { footprint, .. }
                        if *footprint > ticket.limit() =>
                    {
                        return Err(Error::Invalid);
                    }
                    _ => (),
                }
                if let Command::CompleteCleanup { reviewed, .. } = &item.request.command {
                    let cleanup = self.dependency(
                        nodes,
                        &cleanup_key(allocation)?,
                        item.sequence,
                        limits,
                        depth,
                    )?;
                    let ArchivedOutput::Cleanup { reviewed: expected } = cleanup.output else {
                        return Err(Error::Invalid);
                    };
                    if reviewed != &expected {
                        return Err(Error::Invalid);
                    }
                }
                if matches!(item.request.command, Command::SettleFootprint { .. })
                    && self.contains_operation(nodes, &cleanup_key(allocation)?, limits)?
                {
                    return Err(Error::Invalid);
                }
            }
            Command::Observe { probe, .. } => {
                let dep = self.dependency(nodes, probe, item.sequence, limits, depth)?;
                if !matches!(dep.output, ArchivedOutput::Probe { .. }) {
                    return Err(Error::Invalid);
                }
            }
            Command::Finish { reservation } | Command::RetireClosed { reservation } => {
                let dep = self.dependency(nodes, reservation, item.sequence, limits, depth)?;
                let ArchivedOutput::Reservation { token } = dep.output else {
                    return Err(Error::Invalid);
                };
                if let ArchivedOutput::Retired { archive } = &item.output
                    && archive.reservation(nodes, state::Limits::default())?.0 != token
                {
                    return Err(Error::Invalid);
                }
            }
            Command::ObserveClock { .. }
            | Command::Reserve { .. }
            | Command::BeginProbe
            | Command::Invalidate => (),
        }
        Ok(item)
    }
}
impl DependencyView for Snapshot {
    fn load_item(
        &self,
        nodes: &mut impl NodeStore,
        operation: &Key,
        limits: Limits,
    ) -> Result<Item> {
        self.item(nodes, operation, limits)
    }
    fn contains_operation(
        &self,
        nodes: &mut impl NodeStore,
        operation: &Key,
        limits: Limits,
    ) -> Result<bool> {
        Ok(Tree::from_root(self.operations.clone())
            .get(&operation_path(operation)?, nodes, limits.tree())?
            .0
            .is_some())
    }
}
pub struct Inspection {
    receipt: Receipt,
    request: Vec<u8>,
    output: ArchivedOutput,
    metrics: state::Metrics,
}
impl Inspection {
    #[cfg(test)]
    pub(in crate::ledger) fn into_output(self) -> Result<Output> {
        self.output.matching_output()
    }
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
    pub fn original_request(&self) -> &[u8] {
        &self.request
    }
    pub fn dependency_json(&self) -> Result<Vec<u8>> {
        Ok(encode(&self.output)?)
    }
    pub fn metrics(&self) -> state::Metrics {
        self.metrics
    }
    pub fn check_request(&self, raw: &[u8]) -> Result<()> {
        let _: Request = decode(raw)?;
        if raw != self.request {
            return Err(Error::Journal(LedgerError::Conflict));
        }
        Ok(())
    }
}
pub struct Export {
    snapshot: Snapshot,
    metrics: state::Metrics,
}
impl Export {
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    pub fn metrics(&self) -> state::Metrics {
        self.metrics
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ledger) struct Item {
    pub(in crate::ledger) schema: String,
    pub(in crate::ledger) sequence: u64,
    pub(in crate::ledger) previous: Digest,
    pub(in crate::ledger) hash: Digest,
    pub(in crate::ledger) request: Request,
    pub(in crate::ledger) output: ArchivedOutput,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::ledger) enum ArchivedOutput {
    Clock {
        millis: u64,
    },
    Retired {
        archive: core_history::RetiredArchive,
    },
    Handoff {
        ticket: core_history::AllocationRecord,
        binding: Digest,
    },
    HandoffSettled {
        bytes: u64,
    },
    Retention {
        ticket: core_history::AllocationRecord,
        binding: Digest,
    },
    RetentionSettled {
        bytes: u64,
    },
    Cleanup {
        reviewed: Digest,
    },
    Cleaned {
        bytes: u64,
    },
    Publication {
        ticket: core_history::AllocationRecord,
        binding: Digest,
    },
    Footprint {
        bytes: u64,
    },
    Reservation {
        token: core_history::ReservationRecord,
    },
    Allocation {
        ticket: core_history::AllocationRecord,
    },
    Sequence {
        sequence: u64,
    },
    Probe {
        evidence: core_history::ProbeEvidence,
    },
    Summary {
        sampled_free: u64,
        reserved_remaining: u64,
        allocated_after_watermark: u64,
        available: u64,
    },
    Finished,
    Invalidated,
}
impl ArchivedOutput {
    pub(in crate::ledger) fn capture(output: &Output, nodes: &mut impl NodeStore) -> Result<Self> {
        Ok(match output {
            #[cfg(test)]
            Output::Historical(_) => return Err(Error::Invalid),
            Output::Clock(n) => Self::Clock { millis: *n },
            Output::Retired(r) => Self::Retired {
                archive: r.export_archive(nodes, state::Limits::default())?.0,
            },
            Output::Handoff { ticket, binding } => Self::Handoff {
                ticket: core_history::AllocationRecord::capture(ticket),
                binding: binding.clone(),
            },
            Output::HandoffSettled(n) => Self::HandoffSettled { bytes: *n },
            Output::Retention { ticket, binding } => Self::Retention {
                ticket: core_history::AllocationRecord::capture(ticket),
                binding: binding.clone(),
            },
            Output::RetentionSettled(n) => Self::RetentionSettled { bytes: *n },
            Output::Cleanup { reviewed } => Self::Cleanup {
                reviewed: reviewed.clone(),
            },
            Output::Cleaned(n) => Self::Cleaned { bytes: *n },
            Output::Publication { ticket, binding } => Self::Publication {
                ticket: core_history::AllocationRecord::capture(ticket),
                binding: binding.clone(),
            },
            Output::Footprint(n) => Self::Footprint { bytes: *n },
            Output::Reservation(t) => Self::Reservation {
                token: core_history::ReservationRecord::capture(t),
            },
            Output::Allocation(t) => Self::Allocation {
                ticket: core_history::AllocationRecord::capture(t),
            },
            Output::Sequence(n) => Self::Sequence { sequence: *n },
            Output::Probe(t) => Self::Probe {
                evidence: core_history::ProbeEvidence::capture(t),
            },
            Output::Summary(s) => Self::Summary {
                sampled_free: s.sampled_free,
                reserved_remaining: s.reserved_remaining,
                allocated_after_watermark: s.allocated_after_watermark,
                available: s.available,
            },
            Output::Finished => Self::Finished,
            Output::Invalidated => Self::Invalidated,
        })
    }
    pub(in crate::ledger) fn response(&self) -> Response {
        match self {
            Self::Clock { millis } => Response::TimeObserved(*millis),
            Self::Retired { .. } => Response::ReservationRetired,
            Self::Handoff { .. } => Response::HandoffStarted,
            Self::HandoffSettled { bytes } => Response::HandoffSettled(*bytes),
            Self::Retention { .. } => Response::RetentionStarted,
            Self::RetentionSettled { bytes } => Response::RetentionSettled(*bytes),
            Self::Cleanup { .. } => Response::CleanupStarted,
            Self::Cleaned { bytes } => Response::CleanupCompleted(*bytes),
            Self::Publication { .. } | Self::Allocation { .. } => Response::AllocationStarted,
            Self::Footprint { bytes } => Response::FootprintCharged(*bytes),
            Self::Reservation { .. } => Response::Reserved,
            Self::Sequence { sequence } => Response::AllocationCompleted(*sequence),
            Self::Probe { .. } => Response::ProbeStarted,
            Self::Summary {
                sampled_free,
                reserved_remaining,
                allocated_after_watermark,
                available,
            } => Response::Observed(CapacitySummary {
                sampled_free: *sampled_free,
                reserved_remaining: *reserved_remaining,
                allocated_after_watermark: *allocated_after_watermark,
                available: *available,
            }),
            Self::Finished => Response::Finished,
            Self::Invalidated => Response::Invalidated,
        }
    }
    #[cfg(test)]
    fn matching_output(self) -> Result<Output> {
        Ok(match self {
            Self::Reservation { token } => Output::Reservation(token.token()?),
            Self::Allocation { ticket } => Output::Allocation(ticket.ticket()?),
            Self::Publication { ticket, binding } => Output::Publication {
                ticket: ticket.ticket()?,
                binding,
            },
            Self::Retention { ticket, binding } => Output::Retention {
                ticket: ticket.ticket()?,
                binding,
            },
            Self::Handoff { ticket, binding } => Output::Handoff {
                ticket: ticket.ticket()?,
                binding,
            },
            Self::Cleanup { reviewed } => Output::Cleanup { reviewed },
            Self::Clock { millis } => Output::Clock(millis),
            Self::HandoffSettled { bytes } => Output::HandoffSettled(bytes),
            Self::RetentionSettled { bytes } => Output::RetentionSettled(bytes),
            Self::Cleaned { bytes } => Output::Cleaned(bytes),
            Self::Footprint { bytes } => Output::Footprint(bytes),
            Self::Sequence { sequence } => Output::Sequence(sequence),
            Self::Summary {
                sampled_free,
                reserved_remaining,
                allocated_after_watermark,
                available,
            } => Output::Summary(CapacitySummary {
                sampled_free,
                reserved_remaining,
                allocated_after_watermark,
                available,
            }),
            Self::Finished => Output::Finished,
            Self::Invalidated => Output::Invalidated,
            old @ (Self::Probe { .. } | Self::Retired { .. }) => Output::Historical(old),
        })
    }
    fn validate(&self, request: &Request, config: &Config, sequence: u64) -> Result<()> {
        let context = |t: &core_history::ReservationRecord| -> Result<ReservationToken> {
            if !t.matches_context(config.domain.as_str(), config.instance.as_str()) {
                return Err(Error::Invalid);
            }
            Ok(t.token()?)
        };
        let allocation = |t: &core_history::AllocationRecord,
                          id: &Key,
                          event: &str,
                          limit: u64,
                          owner: Option<&Key>|
         -> Result<()> {
            let token = context(t.reservation())?;
            t.ticket()?;
            if token.id() != id.as_str()
                || t.event() != event
                || t.limit() != limit
                || owner.is_some_and(|o| o.as_str() != token.owner())
            {
                return Err(Error::Invalid);
            }
            Ok(())
        };
        match (self, &request.command) {
            (Self::Clock { millis }, Command::ObserveClock { millis: n, .. })
                if millis == n && *millis > 0 && config.clock.is_some() => {}
            (Self::Retired { archive }, Command::RetireClosed { reservation })
                if request.operation == retirement_key(reservation)? =>
            {
                archive.validate()?
            }
            (Self::Reservation { token }, Command::Reserve { owner, peak }) => {
                let t = context(token)?;
                if t.id() != request.operation.as_str() || t.owner() != owner.as_str() || *peak == 0
                {
                    return Err(Error::Invalid);
                }
            }
            (
                Self::Allocation { ticket },
                Command::BeginAllocation {
                    reservation,
                    event,
                    limit,
                },
            ) => allocation(ticket, reservation, event.as_str(), *limit, None)?,
            (
                Self::Publication { ticket, binding: b },
                Command::BeginPublication {
                    reservation,
                    owner,
                    binding,
                    limit,
                },
            ) if b == binding => allocation(
                ticket,
                reservation,
                request.operation.as_str(),
                *limit,
                Some(owner),
            )?,
            (
                Self::Retention { ticket, binding: b },
                Command::BeginRetention {
                    reservation,
                    owner,
                    binding,
                    limit,
                },
            ) if b == binding => allocation(
                ticket,
                reservation,
                request.operation.as_str(),
                *limit,
                Some(owner),
            )?,
            (
                Self::Handoff { ticket, binding: b },
                Command::BeginHandoff {
                    reservation,
                    owner,
                    binding,
                    limit,
                },
            ) if b == binding => allocation(
                ticket,
                reservation,
                request.operation.as_str(),
                *limit,
                Some(owner),
            )?,
            (Self::Footprint { bytes }, Command::SettleFootprint { footprint, .. })
            | (Self::HandoffSettled { bytes }, Command::SettleHandoff { footprint, .. })
            | (Self::RetentionSettled { bytes }, Command::SettleRetention { footprint, .. })
            | (Self::Cleaned { bytes }, Command::CompleteCleanup { footprint, .. })
                if bytes == footprint => {}
            (
                Self::Cleanup { reviewed },
                Command::BeginCleanup {
                    allocation,
                    reviewed: r,
                    ..
                },
            ) if reviewed == r && request.operation == cleanup_key(allocation)? => (),
            (Self::Sequence { sequence: n }, Command::CompleteAllocation { .. })
                if *n > 0 && *n <= sequence => {}
            (Self::Probe { evidence }, Command::BeginProbe) => {
                evidence.validate()?;
                if !evidence.matches_context(config.domain.as_str(), config.instance.as_str()) {
                    return Err(Error::Invalid);
                }
            }
            (
                Self::Summary {
                    sampled_free,
                    reserved_remaining,
                    allocated_after_watermark,
                    available,
                },
                Command::Observe { free, .. },
            ) if sampled_free == free => {
                let held = reserved_remaining
                    .checked_add(*allocated_after_watermark)
                    .ok_or(Error::Invalid)?;
                if *available
                    != sampled_free
                        .saturating_sub(config.floor)
                        .saturating_sub(held)
                {
                    return Err(Error::Invalid);
                }
            }
            (Self::Finished, Command::Finish { .. }) | (Self::Invalidated, Command::Invalidate) => {
            }
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }
}
fn operation_path(operation: &Key) -> Result<LogicalPath> {
    LogicalPath::parse(&format!(
        "op/{}",
        Digest::blob(operation.as_str().as_bytes()).as_str()
    ))
    .map_err(|_| Error::Invalid)
}
struct Row {
    sequence: u64,
    operation: String,
    body: Vec<u8>,
    previous: Digest,
    hash: Digest,
}
impl CapacityLedger {
    /// Explicit read-only source export. Caller admits staging, finishes and reads back metadata.
    /// All SQL row cursors are dropped before any NodeStore call; no activation occurs.
    pub fn export_history(&mut self, nodes: &mut impl NodeStore, limits: Limits) -> Result<Export> {
        #[cfg(test)]
        if self.continued.is_some() {
            return Err(Error::Invalid);
        }
        limits.validate()?;
        self.sync()?;
        let before = self.checkpoint.clone();
        let mut meter = Meter::new(nodes, limits)?;
        let rows = {
            let mut stmt=self.conn.prepare("SELECT sequence,operation,length(body),body,previous,hash FROM events WHERE sequence<=?1 ORDER BY sequence")?;
            let mut cursor = stmt.query([sql_number(before.sequence)?])?;
            let mut collected = Vec::new();
            let mut total = 0u64;
            while let Some(row) = cursor.next()? {
                let length = number(row, 2)?;
                total = total.checked_add(length).ok_or(Error::Budget)?;
                if length > 65536
                    || total > MAX_JOURNAL_BYTES
                    || collected.len() >= MAX_COMMANDS as usize
                {
                    return Err(Error::Budget);
                }
                // Charge bounded encoded source-body staging before copying it.
                meter.charge(length as usize)?;
                collected.push(Row {
                    sequence: number(row, 0)?,
                    operation: row.get(1)?,
                    body: row.get(3)?,
                    previous: Digest::parse(&row.get::<_, String>(4)?)
                        .map_err(|_| Error::Invalid)?,
                    hash: Digest::parse(&row.get::<_, String>(5)?).map_err(|_| Error::Invalid)?,
                });
            }
            collected
        };
        if rows.len() as u64 != before.sequence || self.outputs.len() != rows.len() {
            return Err(Error::Invalid);
        }
        let (working_state, _) = self
            .domain
            .export_checkpoint(&mut meter, state::Limits::default())?;
        let mut entries = BTreeMap::new();
        let mut previous = before.genesis.clone();
        let mut bytes = 0u64;
        for (index, row) in rows.into_iter().enumerate() {
            let request: Request = decode(&row.body)?;
            if row.sequence != index as u64 + 1
                || row.operation != request.operation.as_str()
                || row.previous != previous
                || event_hash(row.sequence, &previous, &request)? != row.hash
            {
                return Err(Error::Invalid);
            }
            bytes = bytes
                .checked_add(row.body.len() as u64)
                .ok_or(Error::Budget)?;
            let output = ArchivedOutput::capture(
                self.outputs
                    .get(request.operation.as_str())
                    .ok_or(Error::Invalid)?,
                &mut meter,
            )?;
            output.validate(&request, &self.config, row.sequence)?;
            let p = operation_path(&request.operation)?;
            let item = Item {
                schema: "tig.capacity-history-op.design-1".into(),
                sequence: row.sequence,
                previous: row.previous,
                hash: row.hash.clone(),
                request,
                output,
            };
            if entries.insert(p, meter.put(&encode(&item)?)?).is_some() {
                return Err(Error::Invalid);
            }
            previous = row.hash;
        }
        if previous != before.hash || bytes != self.bytes {
            return Err(Error::Invalid);
        }
        let (tree, _) = Tree::build(&entries, &mut meter, limits.tree())?;
        let snapshot = Snapshot {
            schema: "tig.capacity-history.design-1".into(),
            config: self.config.clone(),
            journal: before.clone(),
            working_state,
            operations: tree.root().clone(),
            operation_count: before.sequence,
            encoded_request_bytes: bytes,
            clock_high: self.clock.high,
        };
        snapshot.validate()?;
        let metrics = meter.metrics();
        self.sync()?;
        if self.checkpoint != before {
            return Err(Error::Journal(LedgerError::Conflict));
        }
        Ok(Export { snapshot, metrics })
    }
}
#[cfg(test)]
pub(super) mod tests;

#[cfg(test)]
pub(super) mod index;
