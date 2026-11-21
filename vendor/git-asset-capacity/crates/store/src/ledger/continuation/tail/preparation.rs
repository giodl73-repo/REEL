//! Shared continued-data build; no writer, physical admission or publication.
use super::*;
use history::index::staged_insert;

pub(in crate::ledger::continuation) fn build<S: NodeStore>(
    c: &ActiveControl,
    m: &Minimum,
    conn: &mut Connection,
    reader: &mut MetadataReader,
    nodes: &mut S,
    limits: TreeLimits,
) -> Result<(PreparedData, Object, TreeMetrics, u64)> {
    let mut port = BudgetPort::new(nodes, limits)?;
    let snapshot = collect(conn, &c.preparation.tail)?;
    let sql = TreeMetrics {
        reads: snapshot.rows.len() + 20,
        read_bytes: snapshot.bytes as usize + 264 * snapshot.rows.len() + 256 * 1024,
        ..TreeMetrics::default()
    };
    port.account_nested(&port.used(), &sql)?;
    let remaining = port.remaining();
    let base = read_base(
        c,
        &c.seal.intent.authority,
        m,
        reader,
        ReadLimits {
            max_calls: remaining.max_reads.min(history::MAX_CALLS),
            max_bytes: remaining.max_read_bytes.min(history::MAX_BYTES),
        },
    )?;
    let initial = TreeMetrics {
        reads: base.metrics.calls,
        read_bytes: base.metrics.bytes,
        ..TreeMetrics::default()
    };
    port.account_nested(&port.used(), &initial)?;
    let config = base.data.config.clone();
    let activations = base.data.activations.clone();
    let remaining = port.remaining();
    let before = port.used();
    let staged = stage_for_index(base, &snapshot, &mut port, remaining)?;
    port.account_nested(&before, staged.metrics())?;
    let (result, next) = staged.finish(&snapshot, &mut port)?;
    let identity = c.identity()?;
    let ancestor = Ancestor {
        schema: "tig.capacity-successor-ancestor.design-1".into(),
        authority: Digest::blob(&encode(&config)?),
        generation: identity.generation,
        digest: identity.digest.clone(),
    };
    let ancestor = port.put(&bytes(&ancestor)?)?;
    let added = BTreeMap::from([(ancestor_path(identity.generation)?, ancestor)]);
    let activations = staged_insert(&activations, &added, &mut port)?;
    let working_state = result
        .model
        .export_checkpoint(&mut port, state::Limits::default())?
        .0;
    let (operations, sequences) = next.roots();
    let data = PreparedData {
        schema: "tig.capacity-successor-data.design-1".into(),
        config,
        generation: identity.generation.checked_add(1).ok_or(Error::Budget)?,
        source: result.checkpoint.clone(),
        working_state,
        operations: operations.clone(),
        sequences: sequences.clone(),
        activations,
        operation_count: result.checkpoint.sequence,
        clock_high: result.clock.high,
        predecessor: Some(identity),
    };
    let object = port.put(&data.to_json()?)?;
    Ok((data, object, port.used(), snapshot.bytes))
}

pub(in crate::ledger::continuation) fn unchanged(
    conn: &mut Connection,
    identity: &TailIdentity,
    expected: &Checkpoint,
    bytes: u64,
) -> Result<()> {
    let tx = conn.transaction()?;
    schema(&tx)?;
    if header(&tx)? != *identity || head(&tx, identity)? != (expected.clone(), bytes) {
        return Err(Error::Journal(LedgerError::Conflict));
    }
    tx.commit()?;
    Ok(())
}
