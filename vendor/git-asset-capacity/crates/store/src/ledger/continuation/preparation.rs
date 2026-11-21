//! Private read-only preparation. No physical admission, seal or publication.
use super::*;
use crate::metadata::{Error as MetadataError, MetadataStore};
use git_asset_core::tree::TreeMetrics;
use history::index::BudgetPort;

pub(super) struct Prepared {
    pub(super) data: PreparedData,
    pub(super) preparation: Preparation,
    pub(super) metrics: TreeMetrics,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    Materialized,
    Finished,
}

fn metadata(error: MetadataError) -> Error {
    match error {
        MetadataError::Budget => Error::Budget,
        MetadataError::Rollback => Error::Journal(LedgerError::Rollback),
        MetadataError::Foreign | MetadataError::Corrupt => Error::Integrity,
        _ => Error::Store(error.to_string()),
    }
}
fn unsealed(ledger: &CapacityLedger) -> Result<()> {
    let exists: bool = ledger.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name GLOB 'git_asset_activation*' LIMIT 1)",
        [],
        |row| row.get(0),
    )?;
    if exists {
        return Err(Error::Journal(LedgerError::NeedsRecovery));
    }
    Ok(())
}
pub(super) fn legacy(
    ledger: &mut CapacityLedger,
    operation: Key,
    nodes: &mut MetadataStore,
    limits: TreeLimits,
) -> Result<Prepared> {
    legacy_hook(ledger, operation, nodes, limits, |_| Ok(()))
}
fn legacy_hook(
    ledger: &mut CapacityLedger,
    operation: Key,
    nodes: &mut MetadataStore,
    limits: TreeLimits,
    mut hook: impl FnMut(Stage) -> Result<()>,
) -> Result<Prepared> {
    if ledger.continued.is_some() {
        return Err(Error::Invalid);
    }
    ledger.sync()?;
    unsealed(ledger)?;
    let source = ledger.checkpoint();
    let config = ledger.config.clone();
    if source.sequence > limits.max_entries as u64 {
        return Err(Error::Budget);
    }
    let (data, object, spent) = {
        let mut port = BudgetPort::new(nodes, limits)?;
        let sql = TreeMetrics {
            reads: source.sequence as usize + 20,
            read_bytes: ledger.bytes as usize + 264 * source.sequence as usize + 256 * 1024,
            ..TreeMetrics::default()
        };
        port.account_nested(&port.used(), &sql)?;
        let export = ledger.export_history(&mut port, ReadLimits::default())?;
        if export.snapshot().journal() != &source {
            return Err(Error::Journal(LedgerError::Conflict));
        }
        let remaining = port.remaining();
        let before = port.used();
        let (index, work) = Index::from_legacy(export.snapshot(), &mut port, remaining)?;
        port.account_nested(&before, &work)?;
        let (operations, sequences) = index.roots();
        let data = PreparedData {
            schema: "tig.capacity-successor-data.design-1".into(),
            config,
            generation: 1,
            source: source.clone(),
            working_state: export.snapshot().working_state().clone(),
            operations: operations.clone(),
            sequences: sequences.clone(),
            activations: Tree::empty(&mut port)?.root().clone(),
            operation_count: source.sequence,
            clock_high: export.snapshot().clock_high_water(),
            predecessor: None,
        };
        let object = port.put(&data.to_json()?)?;
        (data, object, port.used())
    };
    let ready = finish(operation, nodes, limits, data, object, spent, &mut hook)?;
    ledger.sync()?;
    unsealed(ledger)?;
    if ledger.checkpoint() != source {
        return Err(Error::Journal(LedgerError::Conflict));
    }
    Ok(ready)
}

fn finish(
    operation: Key,
    nodes: &mut MetadataStore,
    limits: TreeLimits,
    data: PreparedData,
    object: Object,
    spent: TreeMetrics,
    hook: &mut impl FnMut(Stage) -> Result<()>,
) -> Result<Prepared> {
    let source = data.source.clone();
    hook(Stage::Materialized)?;
    let minimum = nodes.finish().map_err(metadata)?;
    hook(Stage::Finished)?;
    let mut reader = nodes.finished_reader().map_err(metadata)?;
    let mut port = BudgetPort::new(&mut reader, limits)?;
    // A fresh provider view carries all prior source/build/proof consumption.
    port.account_nested(&port.used(), &spent)?;
    if read_record::<PreparedData>(&mut port, &object)? != data {
        return Err(Error::Integrity);
    }
    let index = Index::from_roots(
        data.config.clone(),
        source.clone(),
        data.operations.clone(),
        data.sequences.clone(),
    )?;
    if index
        .prefix(source.sequence, &mut port, ReadLimits::default())?
        .0
        != source
    {
        return Err(Error::Integrity);
    }
    data.working_state
        .read(&mut port, state::Limits::default())?
        .into_domain_for_context(
            data.config.domain.as_str(),
            data.config.instance.as_str(),
            data.config.floor,
        )?;
    let preparation = Preparation {
        schema: "tig.capacity-successor-preparation.design-1".into(),
        operation: operation.clone(),
        data: object,
        metadata: minimum,
        tail: TailIdentity {
            authority: source.genesis.clone(),
            generation: data.generation,
            operation,
            base: source.clone(),
        },
    };
    let control = preparation.to_json()?;
    port.account_nested(
        &port.used(),
        &TreeMetrics {
            writes: 1,
            write_bytes: control.len(),
            ..TreeMetrics::default()
        },
    )?;
    let metrics = port.used();
    drop(port);
    drop(reader);
    Ok(Prepared {
        data,
        preparation,
        metrics,
    })
}

pub(super) fn continued(
    ledger: &mut CapacityLedger,
    operation: Key,
    minimum: &Minimum,
    nodes: &mut MetadataStore,
    limits: TreeLimits,
) -> Result<Prepared> {
    continued_hook(ledger, operation, minimum, nodes, limits, |_| Ok(()))
}
pub(super) fn continued_hook(
    ledger: &mut CapacityLedger,
    operation: Key,
    minimum: &Minimum,
    nodes: &mut MetadataStore,
    limits: TreeLimits,
    mut hook: impl FnMut(Stage) -> Result<()>,
) -> Result<Prepared> {
    ledger.guard()?;
    let control = ledger
        .continued
        .as_ref()
        .ok_or(Error::Invalid)?
        .control()
        .clone();
    let mut reader = nodes.finished_reader().map_err(metadata)?;
    let (data, object, spent, bytes) = tail::preparation::build(
        &control,
        minimum,
        &mut ledger.conn,
        &mut reader,
        nodes,
        limits,
    )?;
    drop(reader);
    let source = data.source.clone();
    let ready = finish(operation, nodes, limits, data, object, spent, &mut hook)?;
    ledger.guard()?;
    tail::preparation::unchanged(&mut ledger.conn, &control.preparation.tail, &source, bytes)?;
    ledger.guard()?;
    Ok(ready)
}

#[cfg(test)]
mod tests;
