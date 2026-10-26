//! Bounded restart-persistent command journal. One trusted local DB per capacity
//! domain; no physical probe, allocation IO, daemon or deletion in this module.
use crate::{Key, local};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tig_core::{
    Digest,
    capacity::{
        self, AllocationTicket, CapacityDomain, CapacitySummary, ObservationTicket,
        ReservationToken,
    },
    identity,
};

pub const MAX_COMMANDS: u64 = 16_384;
pub const MAX_JOURNAL_BYTES: u64 = 16 * 1024 * 1024;
const FRESH_FOR: Duration = Duration::from_secs(5);
#[derive(Debug)]
pub enum LedgerError {
    Store(crate::Error),
    Sql(String),
    Capacity(capacity::CapacityError),
    Busy,
    Corrupt,
    Rollback,
    Conflict,
    InvalidReference,
    NeedsProbe,
    NeedsRecovery,
    Budget,
    Injected,
    Cancelled,
}
impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for LedgerError {}
impl From<crate::Error> for LedgerError {
    fn from(e: crate::Error) -> Self {
        Self::Store(e)
    }
}
impl From<std::io::Error> for LedgerError {
    fn from(e: std::io::Error) -> Self {
        Self::Store(e.into())
    }
}
impl From<capacity::CapacityError> for LedgerError {
    fn from(e: capacity::CapacityError) -> Self {
        Self::Capacity(e)
    }
}
impl From<rusqlite::Error> for LedgerError {
    fn from(e: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(code, _) = &e
            && matches!(
                code.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
        {
            return Self::Busy;
        }
        Self::Sql(e.to_string())
    }
}
pub type Result<T> = std::result::Result<T, LedgerError>;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    schema: String,
    domain: Key,
    epoch: Key,
    instance: Digest,
    floor: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    genesis: Digest,
    sequence: u64,
    hash: Digest,
}
impl Checkpoint {
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalRef {
    genesis: Digest,
    operation: Key,
}
#[derive(Clone, Debug)]
pub enum Response {
    Reserved,
    AllocationStarted,
    AllocationCompleted(u64),
    ProbeStarted,
    Observed(CapacitySummary),
    Finished,
    Invalidated,
    FootprintCharged(u64),
    CleanupStarted,
    CleanupCompleted(u64),
    RetentionStarted,
    RetentionSettled(u64),
    HandoffStarted,
    HandoffSettled(u64),
}
#[derive(Clone, Debug)]
pub struct Receipt {
    reference: JournalRef,
    checkpoint: Checkpoint,
    response: Response,
    replayed: bool,
}
impl Receipt {
    /// True when this invocation returned an existing committed result.
    /// Historical replies do not grant fresh observation/admission authority.
    pub fn was_replayed(&self) -> bool {
        self.replayed
    }
    pub fn reference(&self) -> &JournalRef {
        &self.reference
    }
    pub fn checkpoint(&self) -> &Checkpoint {
        &self.checkpoint
    }
    pub fn response(&self) -> &Response {
        &self.response
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    BeginHandoff {
        reservation: Key,
        owner: Key,
        binding: Digest,
        limit: u64,
    },
    SettleHandoff {
        allocation: Key,
        binding: Digest,
        footprint: u64,
    },
    BeginRetention {
        reservation: Key,
        owner: Key,
        binding: Digest,
        limit: u64,
    },
    SettleRetention {
        allocation: Key,
        binding: Digest,
        footprint: u64,
    },
    BeginCleanup {
        allocation: Key,
        binding: Digest,
        reviewed: Digest,
    },
    CompleteCleanup {
        allocation: Key,
        binding: Digest,
        reviewed: Digest,
        footprint: u64,
    },
    BeginPublication {
        reservation: Key,
        owner: Key,
        binding: Digest,
        limit: u64,
    },
    SettleFootprint {
        allocation: Key,
        binding: Digest,
        footprint: u64,
    },
    Reserve {
        owner: Key,
        peak: u64,
    },
    BeginAllocation {
        reservation: Key,
        event: Key,
        limit: u64,
    },
    CompleteAllocation {
        allocation: Key,
        actual: u64,
    },
    BeginProbe,
    Observe {
        probe: Key,
        free: u64,
    },
    Finish {
        reservation: Key,
    },
    Invalidate,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    operation: Key,
    command: Command,
}
#[derive(Clone)]
enum Output {
    Handoff {
        ticket: AllocationTicket,
        binding: Digest,
    },
    HandoffSettled(u64),
    Retention {
        ticket: AllocationTicket,
        binding: Digest,
    },
    RetentionSettled(u64),
    Cleanup {
        reviewed: Digest,
    },
    Cleaned(u64),
    Publication {
        ticket: AllocationTicket,
        binding: Digest,
    },
    Footprint(u64),
    Reservation(ReservationToken),
    Allocation(AllocationTicket),
    Sequence(u64),
    Probe(ObservationTicket),
    Summary(CapacitySummary),
    Finished,
    Invalidated,
}
impl Output {
    fn response(&self) -> Response {
        match self {
            Self::Handoff { .. } => Response::HandoffStarted,
            Self::HandoffSettled(bytes) => Response::HandoffSettled(*bytes),
            Self::Retention { .. } => Response::RetentionStarted,
            Self::RetentionSettled(bytes) => Response::RetentionSettled(*bytes),
            Self::Cleanup { .. } => Response::CleanupStarted,
            Self::Cleaned(bytes) => Response::CleanupCompleted(*bytes),
            Self::Publication { .. } => Response::AllocationStarted,
            Self::Footprint(bytes) => Response::FootprintCharged(*bytes),
            Self::Reservation(_) => Response::Reserved,
            Self::Allocation(_) => Response::AllocationStarted,
            Self::Sequence(n) => Response::AllocationCompleted(*n),
            Self::Probe(_) => Response::ProbeStarted,
            Self::Summary(s) => Response::Observed(s.clone()),
            Self::Finished => Response::Finished,
            Self::Invalidated => Response::Invalidated,
        }
    }
}
#[derive(Serialize)]
struct HashEnvelope<'a> {
    schema: &'static str,
    sequence: u64,
    previous: &'a Digest,
    request: &'a Request,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Validated,
    Written,
    Committed,
}

pub struct CapacityLedger {
    root: PathBuf,
    config: Config,
    conn: Connection,
    domain: CapacityDomain,
    checkpoint: Checkpoint,
    bytes: u64,
    outputs: BTreeMap<String, Output>,
    probes: BTreeMap<String, Instant>,
    fresh: Option<Instant>,
    poisoned: bool,
    last_replay: u64,
}
impl CapacityLedger {
    pub fn create(root: &Path, domain: Key, epoch: Key, floor: u64) -> Result<Self> {
        local::check_root_spelling(root)?;
        let mut nonce = [0u8; 32];
        getrandom::fill(&mut nonce).map_err(|_| LedgerError::Corrupt)?;
        let config = Config {
            schema: "tig.capacity-ledger.design-1".into(),
            domain,
            epoch,
            instance: Digest::blob(&nonce),
            floor,
        };
        let body = encode(&config)?;
        let genesis = Digest::blob(&body);
        fs::create_dir(root)?;
        let mut marker = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("domain.json"))?;
        marker.write_all(&body)?;
        marker.sync_all()?;
        drop(marker);
        drop(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join("ledger.sqlite"))?,
        );
        let conn = connect(&root.join("ledger.sqlite"))?;
        conn.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE meta(id INTEGER PRIMARY KEY CHECK(id=1),genesis TEXT NOT NULL,sequence INTEGER NOT NULL,hash TEXT NOT NULL,bytes INTEGER NOT NULL);
            CREATE TABLE events(sequence INTEGER PRIMARY KEY,operation TEXT NOT NULL UNIQUE,body BLOB NOT NULL,previous TEXT NOT NULL,hash TEXT NOT NULL);")?;
        conn.execute("INSERT INTO meta VALUES(1,?1,0,?1,0)", [genesis.as_str()])?;
        conn.execute_batch("COMMIT")?;
        drop(conn);
        Self::open(
            root,
            &Checkpoint {
                genesis: genesis.clone(),
                sequence: 0,
                hash: genesis,
            },
        )
    }
    /// witness is a trusted minimum, not proof that no newer unseen checkpoint
    /// ever existed. Restoring backups requires external witness reconciliation.
    pub fn open(root: &Path, witness: &Checkpoint) -> Result<Self> {
        local::check_root_spelling(root)?;
        local::directory(root)?;
        let config: Config = local::read_record(&root.join("domain.json"))?;
        if config.schema != "tig.capacity-ledger.design-1" {
            return Err(LedgerError::Corrupt);
        }
        let genesis = Digest::blob(&encode(&config)?);
        if genesis != witness.genesis {
            return Err(LedgerError::InvalidReference);
        }
        local::regular(&root.join("ledger.sqlite"))?;
        let conn = connect(&root.join("ledger.sqlite"))?;
        let domain = CapacityDomain::new(
            config.domain.as_str(),
            config.instance.as_str(),
            config.floor,
        )?;
        let mut ledger = Self {
            root: fs::canonicalize(root)?,
            config,
            conn,
            domain,
            checkpoint: Checkpoint {
                genesis: genesis.clone(),
                sequence: 0,
                hash: genesis,
            },
            bytes: 0,
            outputs: BTreeMap::new(),
            probes: BTreeMap::new(),
            fresh: None,
            poisoned: false,
            last_replay: 0,
        };
        ledger.sync()?;
        if ledger.checkpoint.sequence < witness.sequence {
            return Err(LedgerError::Rollback);
        }
        let hash = if witness.sequence == 0 {
            ledger.checkpoint.genesis.as_str().to_owned()
        } else {
            ledger.conn.query_row(
                "SELECT hash FROM events WHERE sequence=?1",
                [sql_number(witness.sequence)?],
                |r| r.get::<_, String>(0),
            )?
        };
        if hash != witness.hash.as_str() {
            return Err(LedgerError::Rollback);
        }
        Ok(ledger)
    }
    /// Events replayed by the last sync; not a count of SQL or physical disk reads.
    pub fn last_replay_count(&self) -> u64 {
        self.last_replay
    }
    pub fn checkpoint(&self) -> Checkpoint {
        self.checkpoint.clone()
    }
    pub fn identity(&self) -> &Digest {
        &self.checkpoint.genesis
    }
    pub(crate) fn check_history(&mut self) -> Result<()> {
        self.sync()
    }
    pub(crate) fn begin_fresh_probe(&mut self, op: Key) -> Result<Receipt> {
        // A failed refresh must not leave this handle's old sample spendable.
        self.fresh = None;
        let receipt = self.begin_probe(op)?;
        if receipt.was_replayed() {
            return Err(LedgerError::NeedsProbe);
        }
        Ok(receipt)
    }
    pub(crate) fn begin_publication(
        &mut self,
        op: Key,
        reservation: &JournalRef,
        owner: Key,
        binding: Digest,
        limit: u64,
    ) -> Result<Receipt> {
        let reservation = self.reference(reservation)?;
        self.apply(
            op,
            Command::BeginPublication {
                reservation,
                owner,
                binding,
                limit,
            },
        )
    }
    pub(crate) fn begin_handoff(
        &mut self,
        op: Key,
        reservation: &JournalRef,
        owner: Key,
        binding: Digest,
        limit: u64,
    ) -> Result<Receipt> {
        let reservation = self.reference(reservation)?;
        self.apply(
            op,
            Command::BeginHandoff {
                reservation,
                owner,
                binding,
                limit,
            },
        )
    }
    pub(crate) fn handoff_status(
        &mut self,
        allocation: &JournalRef,
        binding: &Digest,
    ) -> Result<Option<u64>> {
        let key = self.reference(allocation)?;
        self.sync()?;
        let Some(Output::Handoff {
            ticket,
            binding: expected,
        }) = self.outputs.get(key.as_str())
        else {
            return Err(LedgerError::InvalidReference);
        };
        if binding != expected {
            return Err(LedgerError::Conflict);
        }
        Ok(self.domain.allocation_status(ticket)?)
    }
    pub(crate) fn settle_handoff(
        &mut self,
        op: Key,
        allocation: &JournalRef,
        binding: Digest,
        footprint: u64,
    ) -> Result<Receipt> {
        let allocation = self.reference(allocation)?;
        self.apply(
            op,
            Command::SettleHandoff {
                allocation,
                binding,
                footprint,
            },
        )
    }
    pub(crate) fn begin_retention(
        &mut self,
        op: Key,
        reservation: &JournalRef,
        owner: Key,
        binding: Digest,
        limit: u64,
    ) -> Result<Receipt> {
        let reservation = self.reference(reservation)?;
        self.apply(
            op,
            Command::BeginRetention {
                reservation,
                owner,
                binding,
                limit,
            },
        )
    }
    pub(crate) fn retention_status(
        &mut self,
        allocation: &JournalRef,
        binding: &Digest,
    ) -> Result<Option<u64>> {
        let key = self.reference(allocation)?;
        self.sync()?;
        let Some(Output::Retention {
            ticket,
            binding: expected,
        }) = self.outputs.get(key.as_str())
        else {
            return Err(LedgerError::InvalidReference);
        };
        if binding != expected {
            return Err(LedgerError::Conflict);
        }
        Ok(self.domain.allocation_status(ticket)?)
    }
    pub(crate) fn settle_retention(
        &mut self,
        op: Key,
        allocation: &JournalRef,
        binding: Digest,
        footprint: u64,
    ) -> Result<Receipt> {
        let allocation = self.reference(allocation)?;
        self.apply(
            op,
            Command::SettleRetention {
                allocation,
                binding,
                footprint,
            },
        )
    }
    pub(crate) fn publication_status(
        &mut self,
        allocation: &JournalRef,
        binding: &Digest,
    ) -> Result<Option<u64>> {
        let key = self.reference(allocation)?;
        self.sync()?;
        let Some(Output::Publication {
            ticket,
            binding: expected,
        }) = self.outputs.get(key.as_str())
        else {
            return Err(LedgerError::InvalidReference);
        };
        if binding != expected {
            return Err(LedgerError::Conflict);
        }
        if self.outputs.contains_key(cleanup_key(&key)?.as_str()) {
            return Err(LedgerError::Cancelled);
        }
        Ok(self.domain.allocation_status(ticket)?)
    }
    pub(crate) fn cleanup_status(
        &mut self,
        allocation: &JournalRef,
        binding: &Digest,
    ) -> Result<Option<(Digest, Option<u64>)>> {
        let key = self.reference(allocation)?;
        self.sync()?;
        let Some(Output::Publication {
            ticket,
            binding: expected,
        }) = self.outputs.get(key.as_str())
        else {
            return Err(LedgerError::InvalidReference);
        };
        if binding != expected {
            return Err(LedgerError::Conflict);
        }
        match self.outputs.get(cleanup_key(&key)?.as_str()) {
            Some(Output::Cleanup { reviewed }) => Ok(Some((
                reviewed.clone(),
                self.domain.allocation_status(ticket)?,
            ))),
            None => {
                if self.domain.allocation_status(ticket)?.is_some() {
                    return Err(LedgerError::Conflict);
                }
                Ok(None)
            }
            _ => Err(LedgerError::Corrupt),
        }
    }
    pub(crate) fn publication_reference(&mut self, op: Key) -> Result<JournalRef> {
        self.sync()?;
        if !matches!(
            self.outputs.get(op.as_str()),
            Some(Output::Publication { .. })
        ) {
            return Err(LedgerError::InvalidReference);
        }
        Ok(JournalRef {
            genesis: self.checkpoint.genesis.clone(),
            operation: op,
        })
    }
    pub(crate) fn begin_cleanup(
        &mut self,
        allocation: &JournalRef,
        binding: Digest,
        reviewed: Digest,
    ) -> Result<Receipt> {
        let allocation = self.reference(allocation)?;
        self.apply(
            cleanup_key(&allocation)?,
            Command::BeginCleanup {
                allocation,
                binding,
                reviewed,
            },
        )
    }
    pub(crate) fn complete_cleanup(
        &mut self,
        allocation: &JournalRef,
        binding: Digest,
        reviewed: Digest,
        footprint: u64,
    ) -> Result<Receipt> {
        let allocation = self.reference(allocation)?;
        let op = Key::parse(&format!(
            "cleanup-done-{}",
            Digest::blob(allocation.as_str().as_bytes()).as_str()
        ))?;
        self.apply(
            op,
            Command::CompleteCleanup {
                allocation,
                binding,
                reviewed,
                footprint,
            },
        )
    }
    pub(crate) fn settle_footprint(
        &mut self,
        op: Key,
        allocation: &JournalRef,
        binding: Digest,
        footprint: u64,
    ) -> Result<Receipt> {
        let allocation = self.reference(allocation)?;
        self.apply(
            op,
            Command::SettleFootprint {
                allocation,
                binding,
                footprint,
            },
        )
    }
    pub fn reserve(&mut self, op: Key, owner: Key, peak: u64) -> Result<Receipt> {
        self.apply(op, Command::Reserve { owner, peak })
    }
    pub fn begin_allocation(
        &mut self,
        op: Key,
        reservation: &JournalRef,
        event: Key,
        limit: u64,
    ) -> Result<Receipt> {
        let reservation = self.reference(reservation)?;
        self.apply(
            op,
            Command::BeginAllocation {
                reservation,
                event,
                limit,
            },
        )
    }
    pub fn complete_allocation(
        &mut self,
        op: Key,
        allocation: &JournalRef,
        actual: u64,
    ) -> Result<Receipt> {
        let allocation = self.reference(allocation)?;
        self.apply(op, Command::CompleteAllocation { allocation, actual })
    }
    pub fn begin_probe(&mut self, op: Key) -> Result<Receipt> {
        self.apply(op, Command::BeginProbe)
    }
    pub fn observe(&mut self, op: Key, probe: &JournalRef, free: u64) -> Result<Receipt> {
        let probe = self.reference(probe)?;
        self.apply(op, Command::Observe { probe, free })
    }
    pub fn finish(&mut self, op: Key, reservation: &JournalRef) -> Result<Receipt> {
        let reservation = self.reference(reservation)?;
        self.apply(op, Command::Finish { reservation })
    }
    pub fn invalidate(&mut self, op: Key) -> Result<Receipt> {
        self.apply(op, Command::Invalidate)
    }
    /// Accounted state only; historical free bytes are not fresh admission permission.
    pub fn accounting(&mut self) -> Result<CapacitySummary> {
        self.sync()?;
        Ok(self.domain.summary()?)
    }
    fn reference(&self, reference: &JournalRef) -> Result<Key> {
        if reference.genesis != self.checkpoint.genesis {
            return Err(LedgerError::InvalidReference);
        }
        Ok(reference.operation.clone())
    }
    fn guard(&self) -> Result<()> {
        if self.poisoned {
            return Err(LedgerError::NeedsRecovery);
        }
        local::directory(&self.root)?;
        local::regular(&self.root.join("ledger.sqlite"))?;
        let current: Config = local::read_record(&self.root.join("domain.json"))?;
        if current != self.config {
            return Err(LedgerError::Corrupt);
        }
        Ok(())
    }
    fn sync(&mut self) -> Result<()> {
        self.guard()?;
        let tx = self.conn.transaction()?;
        let before = self.checkpoint.sequence;
        let result = catch_up(
            &tx,
            &mut self.domain,
            &mut self.outputs,
            &mut self.checkpoint,
            &mut self.bytes,
            &mut self.fresh,
        );
        self.last_replay = self.checkpoint.sequence.saturating_sub(before);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn apply(&mut self, operation: Key, command: Command) -> Result<Receipt> {
        self.apply_hook(operation, command, |_| Ok(()))
    }
    fn apply_hook(
        &mut self,
        operation: Key,
        command: Command,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        let result = self.apply_once(operation, command, hook);
        if matches!(
            &result,
            Err(LedgerError::Sql(_)
                | LedgerError::Corrupt
                | LedgerError::Rollback
                | LedgerError::Injected)
        ) {
            self.poisoned = true;
        }
        result
    }
    fn apply_once(
        &mut self,
        operation: Key,
        command: Command,
        mut hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        self.guard()?;
        let request = Request { operation, command };
        let body = encode(&request)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let before = self.checkpoint.sequence;
        if let Err(e) = catch_up(
            &tx,
            &mut self.domain,
            &mut self.outputs,
            &mut self.checkpoint,
            &mut self.bytes,
            &mut self.fresh,
        ) {
            self.poisoned = true;
            return Err(e);
        }
        self.last_replay = self.checkpoint.sequence.saturating_sub(before);
        let old = tx
            .query_row(
                "SELECT sequence,previous,hash,length(body),body FROM events WHERE operation=?1",
                [request.operation.as_str()],
                |r| {
                    if number(r, 3)? > 65_536 {
                        return Err(rusqlite::Error::InvalidQuery);
                    }
                    Ok((
                        number(r, 0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Vec<u8>>(4)?,
                    ))
                },
            )
            .optional()?;
        if let Some((sequence, previous, hash, old_body)) = old {
            if old_body != body {
                return Err(LedgerError::Conflict);
            }
            let previous = Digest::parse(&previous).map_err(|_| LedgerError::Corrupt)?;
            let hash = Digest::parse(&hash).map_err(|_| LedgerError::Corrupt)?;
            if event_hash(sequence, &previous, &request)? != hash {
                self.poisoned = true;
                return Err(LedgerError::Corrupt);
            }
            let output = self
                .outputs
                .get(request.operation.as_str())
                .ok_or(LedgerError::Corrupt)?;
            return Ok(receipt(
                &self.checkpoint.genesis,
                request.operation,
                sequence,
                hash,
                output,
                true,
            ));
        }
        match &request.command {
            Command::Reserve { .. } if self.fresh.is_none_or(|t| t.elapsed() > FRESH_FOR) => {
                return Err(LedgerError::NeedsProbe);
            }
            Command::Observe { probe, .. }
                if self
                    .probes
                    .get(probe.as_str())
                    .is_none_or(|t| t.elapsed() > FRESH_FOR) =>
            {
                return Err(LedgerError::NeedsProbe);
            }
            _ => (),
        }
        if self.checkpoint.sequence >= MAX_COMMANDS {
            return Err(LedgerError::Budget);
        }
        let bytes = self
            .bytes
            .checked_add(body.len() as u64)
            .ok_or(LedgerError::Budget)?;
        if bytes > MAX_JOURNAL_BYTES {
            return Err(LedgerError::Budget);
        }
        let sequence = self.checkpoint.sequence + 1;
        let previous = self.checkpoint.hash.clone();
        let hash = event_hash(sequence, &previous, &request)?;
        let output = self.domain.try_transition(|staged| {
            let output = execute(staged, &self.outputs, &request)?;
            hook(Stage::Validated)?;
            tx.execute(
                "INSERT INTO events VALUES(?1,?2,?3,?4,?5)",
                params![
                    sql_number(sequence)?,
                    request.operation.as_str(),
                    body,
                    previous.as_str(),
                    hash.as_str()
                ],
            )?;
            tx.execute(
                "UPDATE meta SET sequence=?1,hash=?2,bytes=?3 WHERE id=1",
                params![sql_number(sequence)?, hash.as_str(), sql_number(bytes)?],
            )?;
            hook(Stage::Written)?;
            tx.commit()?;
            hook(Stage::Committed)?;
            Ok(output)
        });
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                if matches!(
                    error,
                    LedgerError::Sql(_) | LedgerError::Corrupt | LedgerError::Injected
                ) {
                    self.poisoned = true;
                }
                return Err(error);
            }
        };
        match &request.command {
            Command::BeginProbe => {
                self.probes
                    .insert(request.operation.as_str().into(), Instant::now());
            }
            Command::Observe { probe, .. } => {
                self.fresh = self.probes.remove(probe.as_str());
            }
            Command::Reserve { .. } | Command::Invalidate => self.fresh = None,
            _ => (),
        }
        self.checkpoint.sequence = sequence;
        self.checkpoint.hash = hash.clone();
        self.bytes = bytes;
        let result = receipt(
            &self.checkpoint.genesis,
            request.operation.clone(),
            sequence,
            hash,
            &output,
            false,
        );
        self.outputs
            .insert(request.operation.as_str().into(), output);
        Ok(result)
    }
}

fn receipt(
    genesis: &Digest,
    operation: Key,
    sequence: u64,
    hash: Digest,
    output: &Output,
    replayed: bool,
) -> Receipt {
    Receipt {
        reference: JournalRef {
            genesis: genesis.clone(),
            operation,
        },
        checkpoint: Checkpoint {
            genesis: genesis.clone(),
            sequence,
            hash,
        },
        response: output.response(),
        replayed,
    }
}
fn execute(
    domain: &mut CapacityDomain,
    outputs: &BTreeMap<String, Output>,
    request: &Request,
) -> Result<Output> {
    let reservation = |id: &Key| match outputs.get(id.as_str()) {
        Some(Output::Reservation(token)) => Ok(token),
        _ => Err(LedgerError::InvalidReference),
    };
    Ok(match &request.command {
        Command::BeginHandoff {
            reservation: id,
            owner,
            binding,
            limit,
        } => {
            let token = reservation(id)?;
            if token.owner() != owner.as_str() {
                return Err(LedgerError::InvalidReference);
            }
            Output::Handoff {
                ticket: domain.begin_allocation(token, request.operation.as_str(), *limit)?,
                binding: binding.clone(),
            }
        }
        Command::SettleHandoff {
            allocation,
            binding,
            footprint,
        } => {
            let Some(Output::Handoff {
                ticket,
                binding: expected,
            }) = outputs.get(allocation.as_str())
            else {
                return Err(LedgerError::InvalidReference);
            };
            if binding != expected {
                return Err(LedgerError::Conflict);
            }
            domain.complete_allocation(ticket, *footprint)?;
            Output::HandoffSettled(*footprint)
        }
        Command::BeginRetention {
            reservation: id,
            owner,
            binding,
            limit,
        } => {
            let token = reservation(id)?;
            if token.owner() != owner.as_str() {
                return Err(LedgerError::InvalidReference);
            }
            Output::Retention {
                ticket: domain.begin_allocation(token, request.operation.as_str(), *limit)?,
                binding: binding.clone(),
            }
        }
        Command::SettleRetention {
            allocation,
            binding,
            footprint,
        } => {
            let Some(Output::Retention {
                ticket,
                binding: expected,
            }) = outputs.get(allocation.as_str())
            else {
                return Err(LedgerError::InvalidReference);
            };
            if binding != expected {
                return Err(LedgerError::Conflict);
            }
            domain.complete_allocation(ticket, *footprint)?;
            Output::RetentionSettled(*footprint)
        }
        Command::BeginCleanup {
            allocation,
            binding,
            reviewed,
        } => {
            let Some(Output::Publication {
                ticket,
                binding: expected,
            }) = outputs.get(allocation.as_str())
            else {
                return Err(LedgerError::InvalidReference);
            };
            if binding != expected
                || request.operation != cleanup_key(allocation)?
                || domain.allocation_status(ticket)?.is_some()
            {
                return Err(LedgerError::Conflict);
            }
            Output::Cleanup {
                reviewed: reviewed.clone(),
            }
        }
        Command::CompleteCleanup {
            allocation,
            binding,
            reviewed,
            footprint,
        } => {
            let Some(Output::Publication {
                ticket,
                binding: expected,
            }) = outputs.get(allocation.as_str())
            else {
                return Err(LedgerError::InvalidReference);
            };
            let Some(Output::Cleanup { reviewed: approved }) =
                outputs.get(cleanup_key(allocation)?.as_str())
            else {
                return Err(LedgerError::InvalidReference);
            };
            if binding != expected || reviewed != approved {
                return Err(LedgerError::Conflict);
            }
            domain.complete_allocation(ticket, *footprint)?;
            Output::Cleaned(*footprint)
        }
        Command::BeginPublication {
            reservation: id,
            owner,
            binding,
            limit,
        } => {
            let token = reservation(id)?;
            if token.owner() != owner.as_str() {
                return Err(LedgerError::InvalidReference);
            }
            Output::Publication {
                ticket: domain.begin_allocation(token, request.operation.as_str(), *limit)?,
                binding: binding.clone(),
            }
        }
        Command::SettleFootprint {
            allocation,
            binding,
            footprint,
        } => {
            if outputs.contains_key(cleanup_key(allocation)?.as_str()) {
                return Err(LedgerError::Cancelled);
            }
            let Some(Output::Publication {
                ticket,
                binding: expected,
            }) = outputs.get(allocation.as_str())
            else {
                return Err(LedgerError::InvalidReference);
            };
            if binding != expected {
                return Err(LedgerError::Conflict);
            }
            domain.complete_allocation(ticket, *footprint)?;
            Output::Footprint(*footprint)
        }
        Command::Reserve { owner, peak } => Output::Reservation(domain.reserve(
            request.operation.as_str(),
            owner.as_str(),
            *peak,
        )?),
        Command::BeginAllocation {
            reservation: id,
            event,
            limit,
        } => {
            Output::Allocation(domain.begin_allocation(reservation(id)?, event.as_str(), *limit)?)
        }
        Command::CompleteAllocation { allocation, actual } => {
            let Some(Output::Allocation(ticket)) = outputs.get(allocation.as_str()) else {
                return Err(LedgerError::InvalidReference);
            };
            Output::Sequence(domain.complete_allocation(ticket, *actual)?)
        }
        Command::BeginProbe => Output::Probe(domain.begin_observation()?),
        Command::Observe { probe, free } => {
            let Some(Output::Probe(ticket)) = outputs.get(probe.as_str()) else {
                return Err(LedgerError::InvalidReference);
            };
            domain.observe(ticket.clone(), *free)?;
            Output::Summary(domain.summary()?)
        }
        Command::Finish { reservation: id } => {
            domain.finish(reservation(id)?)?;
            Output::Finished
        }
        Command::Invalidate => {
            domain.invalidate_observation();
            Output::Invalidated
        }
    })
}
fn catch_up(
    conn: &Connection,
    domain: &mut CapacityDomain,
    outputs: &mut BTreeMap<String, Output>,
    checkpoint: &mut Checkpoint,
    bytes: &mut u64,
    fresh: &mut Option<Instant>,
) -> Result<()> {
    let (genesis, head, hash, total) = conn.query_row(
        "SELECT genesis,sequence,hash,bytes FROM meta WHERE id=1",
        [],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                number(r, 1)?,
                r.get::<_, String>(2)?,
                number(r, 3)?,
            ))
        },
    )?;
    if genesis != checkpoint.genesis.as_str() || head > MAX_COMMANDS || total > MAX_JOURNAL_BYTES {
        return Err(LedgerError::Corrupt);
    }
    if head < checkpoint.sequence {
        return Err(LedgerError::Rollback);
    }
    let mut stmt=conn.prepare("SELECT sequence,operation,length(body),body,previous,hash FROM events WHERE sequence>?1 ORDER BY sequence")?;
    let mut rows = stmt.query([sql_number(checkpoint.sequence)?])?;
    while let Some(row) = rows.next()? {
        let sequence = number(row, 0)?;
        let length = number(row, 2)?;
        if sequence != checkpoint.sequence + 1 || sequence > head || length > 65_536 {
            return Err(LedgerError::Corrupt);
        }
        let body = row.get::<_, Vec<u8>>(3)?;
        let request: Request = decode(&body)?;
        if row.get::<_, String>(1)? != request.operation.as_str()
            || row.get::<_, String>(4)? != checkpoint.hash.as_str()
        {
            return Err(LedgerError::Corrupt);
        }
        let next = event_hash(sequence, &checkpoint.hash, &request)?;
        if row.get::<_, String>(5)? != next.as_str()
            || outputs.contains_key(request.operation.as_str())
        {
            return Err(LedgerError::Corrupt);
        }
        let output = execute(domain, outputs, &request)?;
        if matches!(request.command, Command::Invalidate) {
            *fresh = None;
        }
        outputs.insert(request.operation.as_str().into(), output);
        *bytes = bytes.checked_add(length).ok_or(LedgerError::Budget)?;
        if *bytes > MAX_JOURNAL_BYTES {
            return Err(LedgerError::Budget);
        }
        checkpoint.sequence = sequence;
        checkpoint.hash = next;
    }
    if checkpoint.sequence != head || checkpoint.hash.as_str() != hash || *bytes != total {
        return Err(LedgerError::Corrupt);
    }
    Ok(())
}
fn cleanup_key(allocation: &Key) -> Result<Key> {
    Ok(Key::parse(&format!(
        "cancel-{}",
        Digest::blob(allocation.as_str().as_bytes()).as_str()
    ))?)
}
fn event_hash(sequence: u64, previous: &Digest, request: &Request) -> Result<Digest> {
    Ok(Digest::blob(&encode(&HashEnvelope {
        schema: "tig.capacity-event.design-1",
        sequence,
        previous,
        request,
    })?))
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    identity::canonical_json(&serde_json::to_value(value).map_err(|_| LedgerError::Corrupt)?)
        .map_err(|_| LedgerError::Corrupt)
}
fn decode<T: DeserializeOwned + Serialize>(bytes: &[u8]) -> Result<T> {
    let value = identity::parse_json(bytes).map_err(|_| LedgerError::Corrupt)?;
    if identity::canonical_json(&value).map_err(|_| LedgerError::Corrupt)? != bytes {
        return Err(LedgerError::Corrupt);
    }
    let result: T = serde_json::from_value(value).map_err(|_| LedgerError::Corrupt)?;
    if encode(&result)? != bytes {
        return Err(LedgerError::Corrupt);
    }
    Ok(result)
}
fn connect(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::ZERO)?;
    conn.pragma_update(None, "journal_mode", "DELETE")?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    let mode: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
    let sync: i64 = conn.pragma_query_value(None, "synchronous", |r| r.get(0))?;
    if mode != "delete" || sync != 2 {
        return Err(LedgerError::Corrupt);
    }
    Ok(conn)
}

fn number(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value = row.get::<_, i64>(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::InvalidQuery)
}
fn sql_number(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| LedgerError::Budget)
}

#[cfg(test)]
#[path = "ledger_tests.rs"]
mod tests;
