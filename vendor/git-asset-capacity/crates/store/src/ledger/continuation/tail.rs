//! Private bounded SQL snapshot and pure replay. No writer/activation authority.
use super::*;
use git_asset_core::tree::TreeMetrics;
use history::index::BudgetPort;

const META: &str = "CREATE TABLE meta(id INTEGER PRIMARY KEY CHECK(id=1),genesis TEXT NOT NULL,sequence INTEGER NOT NULL,hash TEXT NOT NULL,bytes INTEGER NOT NULL)";
const EVENTS: &str = "CREATE TABLE events(sequence INTEGER PRIMARY KEY,operation TEXT NOT NULL UNIQUE,body BLOB NOT NULL,previous TEXT NOT NULL,hash TEXT NOT NULL)";
const HEADER: &str = "CREATE TABLE git_asset_tail(id INTEGER PRIMARY KEY CHECK(id=1),body BLOB NOT NULL CHECK(length(body)<=65536),hash TEXT NOT NULL)";

fn definitions() -> BTreeMap<String, (String, String)> {
    let mut expected = BTreeMap::new();
    for (name, sql) in [
        ("meta", META),
        ("events", EVENTS),
        ("git_asset_tail", HEADER),
    ] {
        expected.insert(name.into(), ("table".into(), sql.into()));
    }
    for action in ["INSERT", "UPDATE", "DELETE"] {
        let name = format!("git_asset_tail_{action}");
        expected.insert(name.clone(), ("trigger".into(),format!("CREATE TRIGGER {name} BEFORE {action} ON git_asset_tail BEGIN SELECT RAISE(ABORT,'git-asset:immutable-tail'); END")));
    }
    expected
}
fn schema(conn: &Connection) -> Result<()> {
    schema_mode(conn, false)
}
fn schema_mode(conn: &Connection, sealed: bool) -> Result<()> {
    let mut expected = definitions();
    if sealed {
        expected.extend(activation::definitions());
    }
    let mut statement = conn.prepare("SELECT length(CAST(name AS BLOB)),length(CAST(type AS BLOB)),length(CAST(sql AS BLOB)),name,type,sql FROM sqlite_master WHERE name NOT GLOB 'sqlite_*' LIMIT 17")?;
    let mut rows = statement.query([])?;
    let mut found = BTreeMap::new();
    while let Some(r) = rows.next()? {
        if number(r, 0)? > 128 || number(r, 1)? > 16 || number(r, 2)? > 4096 {
            return Err(Error::Invalid);
        }
        let name: String = r.get(3)?;
        let kind: String = r.get(4)?;
        let sql: String = r.get(5)?;
        if found.insert(name, (kind, sql)).is_some() {
            return Err(Error::Invalid);
        }
    }
    if found != expected {
        return Err(Error::Invalid);
    }
    Ok(())
}
/// Called only after the common inspector validates the actual fixed seal row.
pub(in crate::ledger) fn inspect_sealed_source(
    conn: &Connection,
    intent: &SealIntent,
) -> super::super::Result<()> {
    let check = || -> Result<()> {
        schema_mode(conn, true)?;
        let identity = header(conn)?;
        let previous = intent.predecessor.as_ref().ok_or(Error::Invalid)?;
        if identity.generation != previous.generation
            || identity.authority != intent.authority
            || head(conn, &identity)?.0 != intent.source
        {
            return Err(Error::Invalid);
        }
        Ok(())
    };
    check().map_err(|_| LedgerError::Corrupt)
}
fn header(conn: &Connection) -> Result<TailIdentity> {
    let mut statement =
        conn.prepare("SELECT id,length(CAST(body AS BLOB)),length(CAST(hash AS BLOB)),body,hash FROM git_asset_tail LIMIT 2")?;
    let mut rows = statement.query([])?;
    let row = rows.next()?.ok_or(Error::Invalid)?;
    if number(row, 0)? != 1 || number(row, 1)? > 65536 || number(row, 2)? != 64 {
        return Err(Error::Invalid);
    }
    let raw: Vec<u8> = row.get(3)?;
    let hash: String = row.get(4)?;
    let identity: TailIdentity = record(&raw)?;
    identity.validate()?;
    if hash != Digest::blob(&raw).as_str() || rows.next()?.is_some() {
        return Err(Error::Invalid);
    }
    Ok(identity)
}
fn head(conn: &Connection, identity: &TailIdentity) -> Result<(Checkpoint, u64)> {
    let mut statement = conn.prepare(
        "SELECT id,length(CAST(genesis AS BLOB)),sequence,length(CAST(hash AS BLOB)),bytes,genesis,hash FROM meta LIMIT 2",
    )?;
    let mut rows = statement.query([])?;
    let r = rows.next()?.ok_or(Error::Invalid)?;
    if number(r, 0)? != 1 || number(r, 1)? != 64 || number(r, 3)? != 64 {
        return Err(Error::Invalid);
    }
    let sequence = number(r, 2)?;
    let bytes = number(r, 4)?;
    let genesis: String = r.get(5)?;
    let hash: String = r.get(6)?;
    let cp = Checkpoint {
        genesis: Digest::parse(&genesis).map_err(|_| Error::Invalid)?,
        sequence,
        hash: Digest::parse(&hash).map_err(|_| Error::Invalid)?,
    };
    if cp.genesis != identity.authority
        || cp.sequence < identity.base.sequence
        || cp.sequence > u32::MAX as u64
        || cp.sequence - identity.base.sequence > MAX_COMMANDS
        || bytes > MAX_JOURNAL_BYTES
        || rows.next()?.is_some()
    {
        return Err(Error::Invalid);
    }
    Ok((cp, bytes))
}
struct Row {
    sequence: u64,
    operation: Key,
    body: Vec<u8>,
    previous: Digest,
    hash: Digest,
}
struct Snapshot {
    identity: TailIdentity,
    head: Checkpoint,
    bytes: u64,
    rows: Vec<Row>,
}
fn collect(conn: &mut Connection, expected: &TailIdentity) -> Result<Snapshot> {
    collect_from(conn, expected, &expected.base, 0)
}
fn collect_from(
    conn: &mut Connection,
    expected: &TailIdentity,
    since: &Checkpoint,
    prefix_bytes: u64,
) -> Result<Snapshot> {
    expected.validate()?;
    let tx = conn.transaction()?;
    schema(&tx)?;
    let identity = header(&tx)?;
    if &identity != expected {
        return Err(Error::Invalid);
    }
    let (head, bytes) = head(&tx, &identity)?;
    if since.genesis != identity.authority
        || since.sequence < identity.base.sequence
        || since.sequence > head.sequence
        || prefix_bytes > bytes
    {
        return Err(Error::Journal(LedgerError::Rollback));
    }
    let count: u64 = tx.query_row("SELECT count(*) FROM events", [], |r| number(r, 0))?;
    if count != head.sequence - identity.base.sequence {
        return Err(Error::Invalid);
    }
    if since.sequence == identity.base.sequence {
        if since != &identity.base || prefix_bytes != 0 {
            return Err(Error::Invalid);
        }
    } else {
        let hash: String = tx.query_row(
            "SELECT hash FROM events WHERE sequence=?1 AND length(CAST(hash AS BLOB))=64",
            [sql_number(since.sequence)?],
            |r| r.get(0),
        )?;
        if hash != since.hash.as_str() {
            return Err(Error::Journal(LedgerError::Rollback));
        }
    }
    let mut copied = Vec::new();
    let mut total = 0u64;
    {
        let mut statement = tx.prepare("SELECT sequence,length(CAST(operation AS BLOB)),length(CAST(body AS BLOB)),length(CAST(previous AS BLOB)),length(CAST(hash AS BLOB)),operation,body,previous,hash FROM events WHERE sequence>?1 ORDER BY sequence LIMIT 16385")?;
        let mut rows = statement.query([sql_number(since.sequence)?])?;
        while let Some(r) = rows.next()? {
            let size = number(r, 2)?;
            total = total.checked_add(size).ok_or(Error::Budget)?;
            if copied.len() as u64 >= MAX_COMMANDS
                || number(r, 1)? > 128
                || size > 65536
                || number(r, 3)? != 64
                || number(r, 4)? != 64
                || total > MAX_JOURNAL_BYTES
            {
                return Err(Error::Budget);
            }
            let operation: String = r.get(5)?;
            let previous: String = r.get(7)?;
            let hash: String = r.get(8)?;
            copied.push(Row {
                sequence: number(r, 0)?,
                operation: Key::parse(&operation).map_err(|_| Error::Invalid)?,
                body: r.get(6)?,
                previous: Digest::parse(&previous).map_err(|_| Error::Invalid)?,
                hash: Digest::parse(&hash).map_err(|_| Error::Invalid)?,
            });
        }
    }
    if total != bytes - prefix_bytes || copied.len() as u64 != head.sequence - since.sequence {
        return Err(Error::Invalid);
    }
    tx.commit()?;
    Ok(Snapshot {
        identity,
        head,
        bytes,
        rows: copied,
    })
}

fn checked_row(row: &Row, current: &Checkpoint) -> Result<Request> {
    let request: Request = decode(&row.body)?;
    if row.sequence != current.sequence.checked_add(1).ok_or(Error::Budget)?
        || row.operation != request.operation
        || row.previous != current.hash
        || event_hash(row.sequence, &row.previous, &request)? != row.hash
    {
        return Err(Error::Invalid);
    }
    Ok(request)
}

fn dependencies(command: &Command) -> super::super::Result<Vec<(Key, bool)>> {
    Ok(match command {
        Command::BeginAllocation { reservation, .. }
        | Command::BeginPublication { reservation, .. }
        | Command::BeginRetention { reservation, .. }
        | Command::BeginHandoff { reservation, .. }
        | Command::Finish { reservation }
        | Command::RetireClosed { reservation } => vec![(reservation.clone(), true)],
        Command::CompleteAllocation { allocation, .. }
        | Command::SettleRetention { allocation, .. }
        | Command::SettleHandoff { allocation, .. }
        | Command::BeginCleanup { allocation, .. } => vec![(allocation.clone(), true)],
        Command::SettleFootprint { allocation, .. } => vec![
            (allocation.clone(), true),
            (cleanup_key(allocation)?, false),
        ],
        Command::CompleteCleanup { allocation, .. } => {
            vec![(allocation.clone(), true), (cleanup_key(allocation)?, true)]
        }
        Command::Observe { probe, .. } => vec![(probe.clone(), true)],
        Command::ObserveClock { .. }
        | Command::Reserve { .. }
        | Command::BeginProbe
        | Command::Invalidate => vec![],
    })
}
fn inputs(
    request: &Request,
    outputs: &BTreeMap<String, Output>,
    index: &Index,
    nodes: &mut impl NodeStore,
) -> Result<BTreeMap<String, Output>> {
    let mut selected = BTreeMap::new();
    for (key, required) in dependencies(&request.command)? {
        let output = match outputs.get(key.as_str()) {
            Some(output) => Some(output.clone()),
            None => index.execution_input(&key, nodes, ReadLimits::default())?,
        };
        if let Some(output) = output {
            selected.insert(key.as_str().into(), output);
        } else if required {
            return Err(Error::Journal(LedgerError::InvalidReference));
        }
    }
    Ok(selected)
}
struct Replayed {
    model: CapacityDomain,
    outputs: BTreeMap<String, Output>,
    checkpoint: Checkpoint,
    bytes: u64,
    clock: ClockState,
    metrics: TreeMetrics,
}
fn replay(
    base: Base,
    snapshot: &Snapshot,
    nodes: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<Replayed> {
    replay_inner(
        base,
        snapshot,
        nodes,
        limits,
        Names::Check,
        Observation::Invalidate,
    )
}
enum Names {
    Check,
    DeferToIndex,
}
enum Observation {
    Invalidate,
    RecordedForEngine,
}
fn replay_inner(
    base: Base,
    snapshot: &Snapshot,
    nodes: &mut impl NodeStore,
    limits: TreeLimits,
    names: Names,
    observation: Observation,
) -> Result<Replayed> {
    if snapshot.identity.base != base.data.source
        || snapshot.identity.generation != base.data.generation
        || snapshot.identity.authority != base.data.source.genesis
    {
        return Err(Error::Invalid);
    }
    let index = Index::from_roots(
        base.data.config.clone(),
        base.data.source.clone(),
        base.data.operations,
        base.data.sequences,
    )?;
    let mut port = BudgetPort::new(nodes, limits)?;
    let mut model = base.model;
    let mut clock = ClockState {
        enabled: base.data.config.clock.is_some(),
        high: base.data.clock_high,
    };
    let mut outputs = BTreeMap::new();
    let mut current = base.data.source;
    let required = match base.tail {
        TailRequirement::BaseVerified => None,
        TailRequirement::AtLeast(cp) => Some(cp),
    };
    let mut minimum_proven = required.is_none();
    for row in &snapshot.rows {
        let request = checked_row(row, &current)?;
        if outputs.contains_key(request.operation.as_str()) {
            return Err(Error::Invalid);
        }
        if matches!(names, Names::Check)
            && index
                .inspect(&request.operation, &mut port, ReadLimits::default())?
                .is_some()
        {
            return Err(Error::Journal(LedgerError::Conflict));
        }
        let selected = inputs(&request, &outputs, &index, &mut port)?;
        let output = execute(&mut model, &selected, &request, &clock)?;
        clock.advance(&output);
        current.sequence = row.sequence;
        current.hash = row.hash.clone();
        if required.as_ref() == Some(&current) {
            minimum_proven = true;
        }
        outputs.insert(request.operation.as_str().into(), output);
    }
    if current != snapshot.head {
        return Err(Error::Invalid);
    }
    if !minimum_proven {
        return Err(Error::Journal(LedgerError::Rollback));
    }
    if matches!(observation, Observation::Invalidate) {
        model.invalidate_observation();
    }
    Ok(Replayed {
        model,
        outputs,
        checkpoint: current,
        bytes: snapshot.bytes,
        clock,
        metrics: port.used(),
    })
}

// An owned private intermediate, deliberately without state/ticket accessors.
// Only finish can release its model, after conditional insertion proves names.
struct Staged {
    index: Index,
    identity: TailIdentity,
    end: Checkpoint,
    bytes: u64,
    count: usize,
    replayed: Replayed,
}
fn stage_for_index(
    base: Base,
    snapshot: &Snapshot,
    nodes: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<Staged> {
    let index = Index::from_roots(
        base.data.config.clone(),
        base.data.source.clone(),
        base.data.operations.clone(),
        base.data.sequences.clone(),
    )?;
    let replayed = replay_inner(
        base,
        snapshot,
        nodes,
        limits,
        Names::DeferToIndex,
        Observation::Invalidate,
    )?;
    Ok(Staged {
        index,
        identity: snapshot.identity.clone(),
        end: snapshot.head.clone(),
        bytes: snapshot.bytes,
        count: snapshot.rows.len(),
        replayed,
    })
}
impl Staged {
    fn metrics(&self) -> &TreeMetrics {
        &self.replayed.metrics
    }
    fn finish<S: NodeStore>(
        mut self,
        snapshot: &Snapshot,
        nodes: &mut BudgetPort<'_, S>,
    ) -> Result<(Replayed, Index)> {
        if self.identity != snapshot.identity
            || self.end != snapshot.head
            || self.bytes != snapshot.bytes
            || self.count != snapshot.rows.len()
        {
            return Err(Error::Invalid);
        }
        let mut delta = Vec::new();
        for row in &snapshot.rows {
            let request: Request = decode(&row.body)?;
            if request.operation != row.operation {
                return Err(Error::Invalid);
            }
            let output = self
                .replayed
                .outputs
                .remove(request.operation.as_str())
                .ok_or(Error::Invalid)?;
            delta.push(history::Item {
                schema: "tig.capacity-successor-op.design-1".into(),
                sequence: row.sequence,
                previous: row.previous.clone(),
                hash: row.hash.clone(),
                request,
                output: history::ArchivedOutput::capture(&output, nodes)?,
            });
        }
        if !self.replayed.outputs.is_empty() {
            return Err(Error::Invalid);
        }
        let remaining = nodes.remaining();
        let before = nodes.used();
        eprintln!("joined before index: used={before:?}, remaining={remaining:?}");
        let (next, work) = self.index.extend(&delta, &self.end, nodes, remaining)?;
        nodes.account_nested(&before, &work)?;
        Ok((self.replayed, next))
    }
}

/// Read-only adapter: the control/archive/seal/admission gates are still absent.
fn read_tail(
    base: Base,
    control: &ActiveControl,
    conn: &mut Connection,
    nodes: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<Replayed> {
    read_tail_policy(base, control, conn, nodes, limits, Observation::Invalidate)
}
fn read_tail_policy(
    base: Base,
    control: &ActiveControl,
    conn: &mut Connection,
    nodes: &mut impl NodeStore,
    limits: TreeLimits,
    observation: Observation,
) -> Result<Replayed> {
    control.validate()?;
    if base.data.source != control.seal.intent.source
        || base.data.generation != control.preparation.tail.generation
        || Object::from_bytes(&base.data.to_json()?) != control.preparation.data
        || base.data.predecessor != control.seal.intent.predecessor
    {
        return Err(Error::Invalid);
    }
    let snapshot = collect(conn, &control.preparation.tail)?;
    let expected = (snapshot.head.clone(), snapshot.bytes);
    let replayed = match observation {
        Observation::Invalidate => replay(base, &snapshot, nodes, limits)?,
        Observation::RecordedForEngine => replay_inner(
            base,
            &snapshot,
            nodes,
            limits,
            Names::Check,
            Observation::RecordedForEngine,
        )?,
    };
    // No node-provider IO during either transaction. A changed peer is explicit
    // conflict; a failed read consumes the staged model instead of adopting it.
    let tx = conn.transaction()?;
    schema(&tx)?;
    if header(&tx)? != control.preparation.tail || head(&tx, &control.preparation.tail)? != expected
    {
        return Err(Error::Journal(LedgerError::Conflict));
    }
    tx.commit()?;
    Ok(replayed)
}

#[cfg(test)]
mod tests;

pub(in crate::ledger) mod engine;
pub(super) mod preparation;
