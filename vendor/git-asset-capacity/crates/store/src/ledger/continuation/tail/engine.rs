//! Private integration of continued state into the existing CapacityLedger.
//! No public enrollment/activation, physical admission or recovery API.
use super::*;
type LR<T> = super::super::super::Result<T>;

fn journal(error: Error) -> LedgerError {
    match error {
        Error::Journal(e) => e,
        Error::Budget => LedgerError::Budget,
        _ => LedgerError::Corrupt,
    }
}
fn point_limits() -> TreeLimits {
    TreeLimits {
        max_reads: 1024,
        max_read_bytes: 4 * 1024 * 1024,
        max_writes: 0,
        max_write_bytes: 0,
        ..history::index::build_limits()
    }
}

pub(in crate::ledger) struct Mode {
    control: ActiveControl,
    index: Index,
    reader: MetadataReader,
    query: TreeLimits,
    spent: TreeMetrics,
}
impl Mode {
    pub(in crate::ledger::continuation) fn control(&self) -> &ActiveControl {
        &self.control
    }
    pub(in crate::ledger) fn check_seal(&self, root: &Path, intent: &SealIntent) -> LR<()> {
        self.guard(root)?;
        if intent.predecessor.as_ref() != Some(&self.control.identity().map_err(journal)?) {
            return Err(LedgerError::Conflict);
        }
        Ok(())
    }
    pub(in crate::ledger) fn verify_unsealed_source(&self, conn: &Connection) -> LR<()> {
        schema(conn).map_err(journal)?;
        if header(conn).map_err(journal)? != self.control.preparation.tail {
            return Err(LedgerError::Corrupt);
        }
        Ok(())
    }
    pub(in crate::ledger) fn guard(&self, root: &Path) -> LR<()> {
        guard(root, &self.control).map(|_| ())
    }
    fn reset_query(&mut self) {
        self.query = point_limits();
        self.spent = TreeMetrics::default();
    }
    pub(in crate::ledger) fn batch_query(&mut self) {
        self.query = history::index::build_limits();
        self.spent = TreeMetrics::default();
    }
    fn remaining(&self) -> TreeLimits {
        TreeLimits {
            max_reads: self.query.max_reads - self.spent.reads,
            max_read_bytes: self.query.max_read_bytes - self.spent.read_bytes,
            ..self.query
        }
    }
    pub(in crate::ledger) fn lookup(&mut self, key: &Key) -> LR<Option<Output>> {
        let limits = self.remaining();
        let mut port = BudgetPort::new(&mut self.reader, limits).map_err(journal)?;
        let result = lookup(&self.index, key, &mut port).map_err(journal);
        let used = port.used();
        self.spent.reads += used.reads;
        self.spent.read_bytes += used.read_bytes;
        result
    }
    pub(in crate::ledger) fn retired(
        &mut self,
        archive: &git_asset_core::capacity::history::RetiredArchive,
        ticket: &AllocationTicket,
    ) -> LR<Option<u64>> {
        let limits = self.remaining();
        let mut port = BudgetPort::new(&mut self.reader, limits).map_err(journal)?;
        let result = archive
            .outcome(&mut port, ticket, state::Limits::default())
            .map(|(outcome, _)| Some(outcome.charge))
            .map_err(|error| match error {
                state::Error::Budget => LedgerError::Budget,
                _ => LedgerError::Corrupt,
            });
        let used = port.used();
        self.spent.reads += used.reads;
        self.spent.read_bytes += used.read_bytes;
        result
    }
}
#[derive(PartialEq, Eq)]
struct Source {
    path: PathBuf,
    tail: Option<TailIdentity>,
}
fn guard(root: &Path, control: &ActiveControl) -> LR<Source> {
    local::directory(root)?;
    local::directory(&root.join("segments"))?;
    local::directory(&root.join("controls"))?;
    local::regular(&root.join(control.preparation.tail.relative_name().map_err(journal)?))?;
    let current: ActiveControl = local::read_record(&root.join("domain.json"))?;
    current.validate().map_err(journal)?;
    if &current != control {
        return Err(LedgerError::NeedsRecovery);
    }
    let archived: ActiveControl =
        local::read_record(&root.join(control.relative_name().map_err(journal)?))?;
    if archived != current {
        return Err(LedgerError::Corrupt);
    }
    let source = match &control.seal.intent.predecessor {
        None if control.preparation.tail.generation == 1 => Source {
            path: root.join("ledger.sqlite"),
            tail: None,
        },
        Some(previous) => {
            let prior: ActiveControl = local::read_record(
                &root
                    .join("controls")
                    .join(format!("{}.json", previous.digest.as_str())),
            )?;
            prior.validate().map_err(journal)?;
            if prior.identity().map_err(journal)? != *previous
                || prior.preparation.tail.authority != control.preparation.tail.authority
            {
                return Err(LedgerError::Corrupt);
            }
            Source {
                path: root.join(prior.preparation.tail.relative_name().map_err(journal)?),
                tail: Some(prior.preparation.tail),
            }
        }
        None => return Err(LedgerError::Corrupt),
    };
    local::regular(&source.path)?;
    Ok(source)
}

/// Private fixture admission over one known, actually sealed immediate source.
/// Full controller/recovery and physical metadata admission remain required.
pub(in crate::ledger::continuation) fn open(
    root: &Path,
    control: &ActiveControl,
    minimum: &Minimum,
    mut reader: MetadataReader,
) -> LR<CapacityLedger> {
    local::check_root_spelling(root)?;
    control.validate().map_err(journal)?;
    let descriptor = guard(root, control)?;
    let source = connect(&descriptor.path)?;
    activation::verify_seal(
        &source,
        &control.seal.intent.to_json().map_err(journal)?,
        &control.seal.hash,
    )?;
    if let Some(expected) = &descriptor.tail
        && header(&source).map_err(journal)? != *expected
    {
        return Err(LedgerError::Corrupt);
    }
    drop(source);
    let base = read_base(
        control,
        &control.seal.intent.authority,
        minimum,
        &mut reader,
        ReadLimits::default(),
    )
    .map_err(journal)?;
    let config = base.data.config.clone();
    let index = Index::from_roots(
        config.clone(),
        base.data.source.clone(),
        base.data.operations.clone(),
        base.data.sequences.clone(),
    )
    .map_err(journal)?;
    let mut conn = connect(&root.join(control.preparation.tail.relative_name().map_err(journal)?))?;
    let replayed = read_tail_policy(
        base,
        control,
        &mut conn,
        &mut reader,
        history::index::build_limits(),
        Observation::RecordedForEngine,
    )
    .map_err(journal)?;
    let last_replay = replayed.checkpoint.sequence - control.preparation.tail.base.sequence;
    let mode = Mode {
        control: control.clone(),
        index,
        reader,
        query: point_limits(),
        spent: TreeMetrics::default(),
    };
    let ledger = CapacityLedger {
        root: fs::canonicalize(root)?,
        config,
        conn,
        domain: replayed.model,
        checkpoint: replayed.checkpoint,
        bytes: replayed.bytes,
        outputs: replayed.outputs,
        probes: BTreeMap::new(),
        fresh: None,
        poisoned: false,
        last_replay,
        clock: replayed.clock,
        continued: Some(mode),
    };
    ledger.guard()?;
    if guard(root, control)? != descriptor {
        return Err(LedgerError::NeedsRecovery);
    }
    Ok(ledger)
}

fn lookup(index: &Index, key: &Key, nodes: &mut impl NodeStore) -> Result<Option<Output>> {
    index
        .inspect(key, nodes, ReadLimits::default())?
        .map(|found| found.into_output())
        .transpose()
}
fn selected(
    request: &Request,
    cached: &BTreeMap<String, Output>,
    delta: &BTreeMap<String, Output>,
    index: &Index,
    nodes: &mut impl NodeStore,
) -> Result<BTreeMap<String, Output>> {
    let mut inputs = BTreeMap::new();
    for (key, required) in dependencies(&request.command)? {
        let output = match delta.get(key.as_str()).or_else(|| cached.get(key.as_str())) {
            Some(output) => Some(output.clone()),
            None => lookup(index, &key, nodes)?,
        };
        if let Some(output) = output {
            inputs.insert(key.as_str().into(), output);
        } else if required {
            return Err(Error::Journal(LedgerError::InvalidReference));
        }
    }
    Ok(inputs)
}
fn compare(
    conn: &Connection,
    identity: &TailIdentity,
    checkpoint: &Checkpoint,
    bytes: u64,
) -> LR<()> {
    schema(conn).map_err(journal)?;
    if header(conn).map_err(journal)? != *identity
        || head(conn, identity).map_err(journal)? != (checkpoint.clone(), bytes)
    {
        return Err(LedgerError::Conflict);
    }
    Ok(())
}
fn uncertain(error: &LedgerError) -> bool {
    matches!(
        error,
        LedgerError::Sql(_) | LedgerError::Corrupt | LedgerError::Rollback | LedgerError::Injected
    )
}
pub(in crate::ledger) fn sync(ledger: &mut CapacityLedger) -> LR<()> {
    sync_hook(ledger, || Ok(()))
}
fn sync_hook(ledger: &mut CapacityLedger, hook: impl FnMut() -> LR<()>) -> LR<()> {
    let result = sync_once(ledger, hook);
    if result.as_ref().is_err_and(uncertain) {
        ledger.poisoned = true;
    }
    result
}
fn sync_once(ledger: &mut CapacityLedger, mut hook: impl FnMut() -> LR<()>) -> LR<()> {
    let CapacityLedger {
        root,
        conn,
        domain,
        checkpoint,
        bytes,
        outputs,
        fresh,
        last_replay,
        clock,
        continued,
        ..
    } = ledger;
    let mode = continued.as_mut().ok_or(LedgerError::Corrupt)?;
    let snapshot =
        collect_from(conn, &mode.control.preparation.tail, checkpoint, *bytes).map_err(journal)?;
    *last_replay = 0;
    if snapshot.rows.is_empty() {
        mode.reset_query();
        return Ok(());
    }
    let before = checkpoint.sequence;
    let staged = domain.try_transition(|staged| -> LR<_> {
        let mut current = checkpoint.clone();
        let mut next_clock = ClockState {
            enabled: clock.enabled,
            high: clock.high,
        };
        let mut delta = BTreeMap::new();
        let mut invalidate = false;
        {
            let mut port = BudgetPort::new(&mut mode.reader, history::index::build_limits())
                .map_err(journal)?;
            for row in &snapshot.rows {
                let request = checked_row(row, &current).map_err(journal)?;
                if outputs.contains_key(request.operation.as_str())
                    || delta.contains_key(request.operation.as_str())
                    || mode
                        .index
                        .inspect(&request.operation, &mut port, ReadLimits::default())
                        .map_err(journal)?
                        .is_some()
                {
                    return Err(LedgerError::Corrupt);
                }
                let inputs =
                    selected(&request, outputs, &delta, &mode.index, &mut port).map_err(journal)?;
                let output = execute(staged, &inputs, &request, &next_clock)?;
                next_clock.advance(&output);
                invalidate |= matches!(
                    request.command,
                    Command::Invalidate | Command::RetireClosed { .. }
                );
                current.sequence = row.sequence;
                current.hash = row.hash.clone();
                delta.insert(request.operation.as_str().into(), output);
            }
        }
        if current != snapshot.head {
            return Err(LedgerError::Corrupt);
        }
        hook()?;
        mode.guard(root)?;
        let tx = conn.transaction()?;
        compare(
            &tx,
            &mode.control.preparation.tail,
            &snapshot.head,
            snapshot.bytes,
        )?;
        tx.commit()?;
        Ok((delta, next_clock, current, invalidate))
    })?;
    let (mut delta, next_clock, current, invalidate) = staged;
    outputs.append(&mut delta);
    *clock = next_clock;
    *checkpoint = current;
    *bytes = snapshot.bytes;
    if invalidate {
        *fresh = None;
    }
    *last_replay = checkpoint.sequence - before;
    mode.reset_query();
    Ok(())
}

pub(in crate::ledger) fn apply(
    ledger: &mut CapacityLedger,
    operation: Key,
    command: Command,
    mut hook: impl FnMut(Stage) -> LR<()>,
) -> LR<Receipt> {
    ledger.guard()?;
    sync(ledger)?;
    let request = Request { operation, command };
    let body = encode(&request)?;
    if body.len() > 65536 {
        return Err(LedgerError::Budget);
    }
    let mode = ledger.continued.as_mut().ok_or(LedgerError::Corrupt)?;
    let own_work = {
        let mut port = BudgetPort::new(&mut mode.reader, point_limits()).map_err(journal)?;
        if let Some(found) = mode
            .index
            .inspect(&request.operation, &mut port, ReadLimits::default())
            .map_err(journal)?
        {
            found.check_request(&body).map_err(journal)?;
            return Ok(found.receipt().clone());
        }
        port.used()
    };
    let old:Option<(u64,Vec<u8>,String,String)>=ledger.conn.query_row("SELECT sequence,body,previous,hash FROM events WHERE operation=?1 AND length(CAST(body AS BLOB))<=65536 AND length(CAST(previous AS BLOB))=64 AND length(CAST(hash AS BLOB))=64",[request.operation.as_str()],|r|Ok((number(r,0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    if let Some((sequence, old, previous, hash)) = old {
        if sequence > ledger.checkpoint.sequence {
            return Err(LedgerError::Conflict);
        }
        if old != body {
            return Err(LedgerError::Conflict);
        }
        let previous = Digest::parse(&previous).map_err(|_| LedgerError::Corrupt)?;
        let hash = Digest::parse(&hash).map_err(|_| LedgerError::Corrupt)?;
        if event_hash(sequence, &previous, &request)? != hash {
            return Err(LedgerError::Corrupt);
        }
        return Ok(receipt(
            &ledger.checkpoint.genesis,
            request.operation.clone(),
            sequence,
            hash,
            ledger
                .outputs
                .get(request.operation.as_str())
                .ok_or(LedgerError::Corrupt)?,
            true,
        ));
    }
    match &request.command {
        Command::Reserve { .. } if ledger.fresh.is_none_or(|t| t.elapsed() > FRESH_FOR) => {
            return Err(LedgerError::NeedsProbe);
        }
        Command::Observe { probe, .. }
            if ledger
                .probes
                .get(probe.as_str())
                .is_none_or(|t| t.elapsed() > FRESH_FOR) =>
        {
            return Err(LedgerError::NeedsProbe);
        }
        _ => (),
    }
    if ledger.checkpoint.sequence - mode.control.preparation.tail.base.sequence >= MAX_COMMANDS
        || ledger.checkpoint.sequence >= u32::MAX as u64
    {
        return Err(LedgerError::Budget);
    }
    let bytes = ledger
        .bytes
        .checked_add(body.len() as u64)
        .filter(|n| *n <= MAX_JOURNAL_BYTES)
        .ok_or(LedgerError::Budget)?;
    let point = point_limits();
    let remaining = TreeLimits {
        max_reads: point.max_reads - own_work.reads,
        max_read_bytes: point.max_read_bytes - own_work.read_bytes,
        ..point
    };
    let inputs = {
        let mut port = BudgetPort::new(&mut mode.reader, remaining).map_err(journal)?;
        selected(
            &request,
            &ledger.outputs,
            &BTreeMap::new(),
            &mode.index,
            &mut port,
        )
        .map_err(journal)?
    };
    mode.guard(&ledger.root)?;
    hook(Stage::Guarded)?;
    let tx = ledger
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    compare(
        &tx,
        &mode.control.preparation.tail,
        &ledger.checkpoint,
        ledger.bytes,
    )?;
    let sequence = ledger.checkpoint.sequence + 1;
    let hash = event_hash(sequence, &ledger.checkpoint.hash, &request)?;
    let output = ledger.domain.try_transition(|staged| {
        let output = execute(staged, &inputs, &request, &ledger.clock)?;
        hook(Stage::Validated)?;
        tx.execute(
            "INSERT INTO events VALUES(?1,?2,?3,?4,?5)",
            params![
                sql_number(sequence)?,
                request.operation.as_str(),
                body,
                ledger.checkpoint.hash.as_str(),
                hash.as_str()
            ],
        )?;
        if tx.execute(
            "UPDATE meta SET sequence=?1,hash=?2,bytes=?3 WHERE id=1",
            params![sql_number(sequence)?, hash.as_str(), sql_number(bytes)?],
        )? != 1
        {
            return Err(LedgerError::Corrupt);
        }
        hook(Stage::Written)?;
        tx.commit()?;
        hook(Stage::Committed)?;
        Ok(output)
    })?;
    match &request.command {
        Command::BeginProbe => {
            ledger
                .probes
                .insert(request.operation.as_str().into(), Instant::now());
        }
        Command::Observe { probe, .. } => ledger.fresh = ledger.probes.remove(probe.as_str()),
        Command::Reserve { .. } | Command::Invalidate | Command::RetireClosed { .. } => {
            ledger.fresh = None
        }
        _ => (),
    }
    ledger.clock.advance(&output);
    ledger.checkpoint.sequence = sequence;
    ledger.checkpoint.hash = hash.clone();
    ledger.bytes = bytes;
    let result = receipt(
        &ledger.checkpoint.genesis,
        request.operation.clone(),
        sequence,
        hash,
        &output,
        false,
    );
    ledger
        .outputs
        .insert(request.operation.as_str().into(), output);
    Ok(result)
}

#[cfg(test)]
mod tests;
