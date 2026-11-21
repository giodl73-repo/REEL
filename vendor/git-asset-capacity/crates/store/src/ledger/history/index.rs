//! Private successor-index component; no active format or public admission path.
use super::*;
use git_asset_core::tree;
use std::collections::BTreeSet;

const ITEM_SCHEMA: &str = "tig.capacity-successor-op.design-1";

fn sequence_path(sequence: u64) -> Result<LogicalPath> {
    LogicalPath::parse(&format!("seq/{sequence:020}")).map_err(|_| Error::Invalid)
}

pub(in crate::ledger) fn build_limits() -> TreeLimits {
    TreeLimits {
        max_entries: u32::MAX as usize,
        max_reads: 2_000_000,
        max_writes: 1_000_000,
        max_read_bytes: 512 * 1024 * 1024,
        max_write_bytes: 512 * 1024 * 1024,
        ..TreeLimits::default()
    }
}

pub(in crate::ledger) struct BudgetPort<'a, S> {
    nodes: &'a mut S,
    limits: TreeLimits,
    used: Rc<Cell<tree::TreeMetrics>>,
}

// This meter is per operation, not a cache or persistent authority. Pure proof
// writes and overlay work invisible to the provider are charged after each phase.
impl<'a, S> BudgetPort<'a, S> {
    pub(in crate::ledger) fn new(nodes: &'a mut S, limits: TreeLimits) -> Result<Self> {
        let max = build_limits();
        if limits.max_entries > max.max_entries
            || limits.max_reads > max.max_reads
            || limits.max_writes > max.max_writes
            || limits.max_read_bytes > max.max_read_bytes
            || limits.max_write_bytes > max.max_write_bytes
            || limits.max_map_depth > max.max_map_depth
        {
            return Err(Error::Budget);
        }
        Ok(Self {
            nodes,
            limits,
            used: Rc::new(Cell::new(tree::TreeMetrics::default())),
        })
    }
    fn fork<'b, T>(&self, nodes: &'b mut T) -> BudgetPort<'b, T> {
        BudgetPort {
            nodes,
            limits: self.limits,
            used: self.used.clone(),
        }
    }
    pub(in crate::ledger) fn used(&self) -> tree::TreeMetrics {
        let used = self.used.take();
        self.used.set(used.clone());
        used
    }
    fn charge(
        &self,
        reads: usize,
        writes: usize,
        read_bytes: usize,
        write_bytes: usize,
    ) -> TreeResult<()> {
        let mut next = self.used();
        next.reads = next.reads.checked_add(reads).ok_or(TreeError::Budget)?;
        next.writes = next.writes.checked_add(writes).ok_or(TreeError::Budget)?;
        next.read_bytes = next
            .read_bytes
            .checked_add(read_bytes)
            .ok_or(TreeError::Budget)?;
        next.write_bytes = next
            .write_bytes
            .checked_add(write_bytes)
            .ok_or(TreeError::Budget)?;
        if next.reads > self.limits.max_reads
            || next.writes > self.limits.max_writes
            || next.read_bytes > self.limits.max_read_bytes
            || next.write_bytes > self.limits.max_write_bytes
        {
            return Err(TreeError::Budget);
        }
        self.used.set(next);
        Ok(())
    }
    pub(in crate::ledger) fn account_nested(
        &self,
        before: &tree::TreeMetrics,
        total: &tree::TreeMetrics,
    ) -> Result<()> {
        let after = self.used();
        let unseen = |old: usize, new: usize, total: usize| -> Result<usize> {
            total
                .checked_sub(new.checked_sub(old).ok_or(Error::Invalid)?)
                .ok_or(Error::Invalid)
        };
        self.charge(
            unseen(before.reads, after.reads, total.reads)?,
            unseen(before.writes, after.writes, total.writes)?,
            unseen(before.read_bytes, after.read_bytes, total.read_bytes)?,
            unseen(before.write_bytes, after.write_bytes, total.write_bytes)?,
        )?;
        Ok(())
    }
    pub(in crate::ledger) fn remaining(&self) -> TreeLimits {
        let used = self.used();
        TreeLimits {
            max_reads: self.limits.max_reads - used.reads,
            max_writes: self.limits.max_writes - used.writes,
            max_read_bytes: self.limits.max_read_bytes - used.read_bytes,
            max_write_bytes: self.limits.max_write_bytes - used.write_bytes,
            ..self.limits
        }
    }
}

impl<S: NodeStore> NodeStore for BudgetPort<'_, S> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        if o.bytes > 65536 {
            return Err(TreeError::Budget);
        }
        self.charge(1, 0, o.bytes as usize, 0)?;
        let bytes = self.nodes.get(o)?;
        if bytes.as_ref().is_some_and(|b| Object::from_bytes(b) != *o) {
            return Err(TreeError::Integrity);
        }
        Ok(bytes)
    }
    fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
        if bytes.len() > 65536 {
            return Err(TreeError::Budget);
        }
        self.charge(0, 1, 0, bytes.len())?;
        let expected = Object::from_bytes(bytes);
        if self.nodes.put(bytes)? != expected {
            return Err(TreeError::Integrity);
        }
        Ok(expected)
    }
}

#[derive(Clone)]
pub(in crate::ledger) struct Index {
    config: Config,
    head: Checkpoint,
    operations: Object,
    sequences: Object,
}

impl Index {
    /// Framing only: caller must admit the exact original source and roots.
    pub(in crate::ledger) fn from_roots(
        config: Config,
        head: Checkpoint,
        operations: Object,
        sequences: Object,
    ) -> Result<Self> {
        let index = Self {
            config,
            head,
            operations,
            sequences,
        };
        index.validate()?;
        Ok(index)
    }
    pub(in crate::ledger) fn roots(&self) -> (&Object, &Object) {
        (&self.operations, &self.sequences)
    }

    fn validate(&self) -> Result<()> {
        if self.head.genesis != Digest::blob(&encode(&self.config)?)
            || self.head.sequence > u32::MAX as u64
            || self.head.sequence == 0 && self.head.hash != self.head.genesis
            || [self.operations.bytes, self.sequences.bytes]
                .into_iter()
                .any(|n| n == 0 || n > 65536)
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    fn tree_limits(&self, limits: Limits) -> TreeLimits {
        TreeLimits {
            max_entries: self.head.sequence.max(1) as usize,
            ..limits.tree()
        }
    }
    fn at(
        &self,
        root: &Object,
        path: &LogicalPath,
        nodes: &mut impl NodeStore,
        limits: Limits,
    ) -> Result<Option<Object>> {
        Ok(Tree::from_root(root.clone())
            .get(path, nodes, self.tree_limits(limits))?
            .0)
    }
    fn record(&self, o: &Object, sequence: u64, nodes: &mut impl NodeStore) -> Result<Item> {
        let raw = nodes.get(o)?.ok_or_else(|| Error::Missing(o.clone()))?;
        let item: Item = decode(&raw)?;
        if item.sequence != sequence
            || sequence == 0
            || sequence > self.head.sequence
            || !matches!(
                item.schema.as_str(),
                "tig.capacity-history-op.design-1" | ITEM_SCHEMA
            )
            || item.schema == "tig.capacity-history-op.design-1" && sequence > MAX_COMMANDS
            || sequence == 1 && item.previous != self.head.genesis
            || sequence == self.head.sequence && item.hash != self.head.hash
            || event_hash(sequence, &item.previous, &item.request)? != item.hash
        {
            return Err(Error::Invalid);
        }
        item.output
            .validate(&item.request, &self.config, sequence)?;
        Ok(item)
    }
    fn item_by_sequence(
        &self,
        sequence: u64,
        nodes: &mut impl NodeStore,
        limits: Limits,
    ) -> Result<(Object, Item)> {
        let o = self
            .at(&self.sequences, &sequence_path(sequence)?, nodes, limits)?
            .ok_or(Error::Invalid)?;
        let item = self.record(&o, sequence, nodes)?;
        if self.at(
            &self.operations,
            &operation_path(&item.request.operation)?,
            nodes,
            limits,
        )? != Some(o.clone())
        {
            return Err(Error::Invalid);
        }
        Ok((o, item))
    }
    fn point_limits(limits: Limits) -> Result<Limits> {
        limits.validate()?;
        Ok(Limits {
            max_calls: limits.max_calls.min(1024),
            max_bytes: limits.max_bytes.min(4 * 1024 * 1024),
        })
    }
    pub(in crate::ledger) fn prefix(
        &self,
        sequence: u64,
        nodes: &mut impl NodeStore,
        limits: Limits,
    ) -> Result<(Checkpoint, state::Metrics)> {
        self.validate()?;
        if sequence > self.head.sequence {
            return Err(Error::Journal(LedgerError::Rollback));
        }
        let mut meter = Meter::new(nodes, Self::point_limits(limits)?)?;
        let hash = if sequence == 0 {
            self.head.genesis.clone()
        } else {
            let (_, item) = self.item_by_sequence(sequence, &mut meter, limits)?;
            self.checked(&mut meter, &item.request.operation, limits, 0)?
                .hash
        };
        Ok((
            Checkpoint {
                genesis: self.head.genesis.clone(),
                sequence,
                hash,
            },
            meter.metrics(),
        ))
    }
    pub(in crate::ledger) fn inspect(
        &self,
        operation: &Key,
        nodes: &mut impl NodeStore,
        limits: Limits,
    ) -> Result<Option<Inspection>> {
        self.validate()?;
        let limits = Self::point_limits(limits)?;
        let mut meter = Meter::new(nodes, limits)?;
        if !self.contains_operation(&mut meter, operation, limits)? {
            return Ok(None);
        }
        let item = self.checked(&mut meter, operation, limits, 0)?;
        Ok(Some(Inspection {
            receipt: Receipt {
                reference: JournalRef {
                    genesis: self.head.genesis.clone(),
                    operation: operation.clone(),
                },
                checkpoint: Checkpoint {
                    genesis: self.head.genesis.clone(),
                    sequence: item.sequence,
                    hash: item.hash,
                },
                response: item.output.response(),
                replayed: true,
            },
            request: encode(&item.request)?,
            output: item.output,
            metrics: meter.metrics(),
        }))
    }
    /// Exact typed input for pure continued replay; never rehydrate a probe or
    /// materialize a retired history. Their historical APIs remain separate.
    pub(in crate::ledger) fn execution_input(
        &self,
        operation: &Key,
        nodes: &mut impl NodeStore,
        limits: Limits,
    ) -> Result<Option<Output>> {
        let Some(found) = self.inspect(operation, nodes, limits)? else {
            return Ok(None);
        };
        Ok(Some(match found.output {
            ArchivedOutput::Reservation { token } => Output::Reservation(token.token()?),
            ArchivedOutput::Allocation { ticket } => Output::Allocation(ticket.ticket()?),
            ArchivedOutput::Publication { ticket, binding } => Output::Publication {
                ticket: ticket.ticket()?,
                binding,
            },
            ArchivedOutput::Retention { ticket, binding } => Output::Retention {
                ticket: ticket.ticket()?,
                binding,
            },
            ArchivedOutput::Handoff { ticket, binding } => Output::Handoff {
                ticket: ticket.ticket()?,
                binding,
            },
            ArchivedOutput::Cleanup { reviewed } => Output::Cleanup { reviewed },
            _ => return Err(Error::Journal(LedgerError::InvalidReference)),
        }))
    }
    fn retired_outcome(
        &self,
        operation: &Key,
        ticket: &AllocationTicket,
        nodes: &mut impl NodeStore,
        limits: Limits,
    ) -> Result<(core_history::Outcome, state::Metrics)> {
        self.validate()?;
        let limits = Self::point_limits(limits)?;
        let mut meter = Meter::new(nodes, limits)?;
        let item = self.checked(&mut meter, operation, limits, 0)?;
        let ArchivedOutput::Retired { archive } = item.output else {
            return Err(Error::Invalid);
        };
        let (outcome, _) = archive.outcome(&mut meter, ticket, state::Limits::default())?;
        Ok((outcome, meter.metrics()))
    }
    pub(in crate::ledger) fn from_legacy(
        snapshot: &Snapshot,
        nodes: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<(Self, tree::TreeMetrics)> {
        snapshot.validate()?;
        if snapshot.operation_count > limits.max_entries as u64 {
            return Err(Error::Budget);
        }
        let mut port = BudgetPort::new(nodes, limits)?;
        // Original format validation retains its own old caps. The private
        // proof port shares every global read/write counter before each call.
        let mut memory = git_asset_core::tree::MemoryNodes::default();
        let mut proof = port.fork(&mut memory);
        snapshot.validate_all_with_proof(&mut port, &mut proof, Limits::default())?;
        let (ops, _) = Tree::from_root(snapshot.operations.clone()).entries(&mut port, limits)?;
        if ops.len() as u64 != snapshot.operation_count {
            return Err(Error::Invalid);
        }
        let mut sequences = BTreeMap::new();
        for o in ops.values() {
            let raw = port.get(o)?.ok_or_else(|| Error::Missing(o.clone()))?;
            let item: Item = decode(&raw)?;
            if sequences
                .insert(sequence_path(item.sequence)?, o.clone())
                .is_some()
            {
                return Err(Error::Invalid);
            }
        }
        let remaining = port.remaining();
        let (sequence_tree, _) = Tree::build(&sequences, &mut port, remaining)?;
        let index = Self {
            config: snapshot.config.clone(),
            head: snapshot.journal.clone(),
            operations: snapshot.operations.clone(),
            sequences: sequence_tree.root().clone(),
        };
        index.validate()?;
        Ok((index, port.used()))
    }
    // Rows must come from the controller's exact acknowledged bounded source;
    // this checks framing/chain/dependencies, not independent source authority.
    pub(in crate::ledger) fn extend(
        &self,
        delta: &[Item],
        end: &Checkpoint,
        nodes: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<(Self, tree::TreeMetrics)> {
        self.validate()?;
        if end.sequence > limits.max_entries as u64 {
            return Err(Error::Budget);
        }
        if delta.is_empty()
            || delta.len() > MAX_COMMANDS as usize
            || end.genesis != self.head.genesis
            || end.sequence
                != self
                    .head
                    .sequence
                    .checked_add(delta.len() as u64)
                    .ok_or(Error::Budget)?
            || end.sequence > u32::MAX as u64
        {
            return Err(Error::Invalid);
        }
        let mut port = BudgetPort::new(nodes, limits)?;
        let mut operations = BTreeMap::new();
        let mut sequences = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let mut previous = self.head.hash.clone();
        let mut request_bytes = 0usize;
        for (i, item) in delta.iter().enumerate() {
            request_bytes = request_bytes
                .checked_add(encode(&item.request)?.len())
                .ok_or(Error::Budget)?;
            if item.schema != ITEM_SCHEMA
                || item.sequence != self.head.sequence + i as u64 + 1
                || item.previous != previous
                || event_hash(item.sequence, &previous, &item.request)? != item.hash
                || request_bytes > MAX_JOURNAL_BYTES as usize
            {
                return Err(Error::Invalid);
            }
            item.output
                .validate(&item.request, &self.config, item.sequence)?;
            let path = operation_path(&item.request.operation)?;
            if !seen.insert(path) {
                return Err(Error::Journal(LedgerError::Conflict));
            }
            previous = item.hash.clone();
        }
        if previous != end.hash {
            return Err(Error::Invalid);
        }
        drop(seen);
        let view = DeltaView {
            base: self,
            rows: delta
                .iter()
                .map(|item| (item.request.operation.as_str(), item))
                .collect(),
        };
        for item in delta {
            view.checked(&mut port, &item.request.operation, Limits::default(), 0)?;
        }
        for item in delta {
            let o = port.put(&encode(item)?)?;
            operations.insert(operation_path(&item.request.operation)?, o.clone());
            sequences.insert(sequence_path(item.sequence)?, o);
        }
        let operations = staged_insert(&self.operations, &operations, &mut port)?;
        let sequences = staged_insert(&self.sequences, &sequences, &mut port)?;
        let next = Self {
            config: self.config.clone(),
            head: end.clone(),
            operations,
            sequences,
        };
        next.validate()?;
        Ok((next, port.used()))
    }
}

pub(in crate::ledger) fn staged_insert<S: NodeStore>(
    root: &Object,
    delta: &BTreeMap<LogicalPath, Object>,
    port: &mut BudgetPort<'_, S>,
) -> Result<Object> {
    let before = port.used();
    let remaining = port.remaining();
    eprintln!(
        "index stage before={before:?}, remaining={remaining:?}, delta={}",
        delta.len()
    );
    let (next, metrics) =
        match Tree::from_root(root.clone()).insert_staged(root, delta, port, remaining) {
            Ok(result) => result,
            Err(error) => {
                eprintln!(
                    "index stage refused: {error:?}; provider work={:?}",
                    port.used()
                );
                return Err(match error {
                    TreeError::PathConflict => Error::Journal(LedgerError::Conflict),
                    other => other.into(),
                });
            }
        };
    let after = port.used();
    port.charge(
        metrics
            .tree
            .reads
            .checked_sub(after.reads - before.reads)
            .ok_or(Error::Invalid)?,
        metrics
            .tree
            .writes
            .checked_sub(after.writes - before.writes)
            .ok_or(Error::Invalid)?,
        metrics
            .tree
            .read_bytes
            .checked_sub(after.read_bytes - before.read_bytes)
            .ok_or(Error::Invalid)?,
        metrics
            .tree
            .write_bytes
            .checked_sub(after.write_bytes - before.write_bytes)
            .ok_or(Error::Invalid)?,
    )?;
    Ok(next.root().clone())
}

// Only this admitted bounded delta, never a lifetime output map.
struct DeltaView<'a> {
    base: &'a Index,
    rows: BTreeMap<&'a str, &'a Item>,
}
impl DependencyView for DeltaView<'_> {
    fn load_item(
        &self,
        nodes: &mut impl NodeStore,
        operation: &Key,
        limits: Limits,
    ) -> Result<Item> {
        match self.rows.get(operation.as_str()) {
            Some(item) => Ok((*item).clone()),
            None => self.base.load_item(nodes, operation, limits),
        }
    }
    fn contains_operation(
        &self,
        nodes: &mut impl NodeStore,
        operation: &Key,
        limits: Limits,
    ) -> Result<bool> {
        Ok(self.rows.contains_key(operation.as_str())
            || self.base.contains_operation(nodes, operation, limits)?)
    }
}

impl DependencyView for Index {
    fn load_item(
        &self,
        nodes: &mut impl NodeStore,
        operation: &Key,
        limits: Limits,
    ) -> Result<Item> {
        let o = self
            .at(&self.operations, &operation_path(operation)?, nodes, limits)?
            .ok_or(Error::Invalid)?;
        let raw = nodes.get(&o)?.ok_or_else(|| Error::Missing(o.clone()))?;
        let preliminary: Item = decode(&raw)?;
        let item = self.record(&o, preliminary.sequence, nodes)?;
        if item.request.operation != *operation
            || self.at(
                &self.sequences,
                &sequence_path(item.sequence)?,
                nodes,
                limits,
            )? != Some(o)
        {
            return Err(Error::Invalid);
        }
        if item.sequence > 1 {
            let (_, previous) = self.item_by_sequence(item.sequence - 1, nodes, limits)?;
            if previous.hash != item.previous {
                return Err(Error::Invalid);
            }
        }
        Ok(item)
    }
    fn contains_operation(
        &self,
        nodes: &mut impl NodeStore,
        operation: &Key,
        limits: Limits,
    ) -> Result<bool> {
        Ok(self
            .at(&self.operations, &operation_path(operation)?, nodes, limits)?
            .is_some())
    }
}

#[cfg(test)]
mod tests;
