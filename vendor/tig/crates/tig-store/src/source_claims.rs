//! Bounded durable root obligations. This module grants no payload removal authority.
use crate::{Key, local};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    time::Duration,
};
use tig_core::{Digest, Object, identity};

pub const MAX_SOURCES: usize = 1024;
pub const MAX_PREDECESSORS: usize = 64;
pub const MAX_COMMANDS: u64 = 16_384;
pub const MAX_COMMAND_BYTES: u64 = 16 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Io,
    Sql,
    Busy,
    Corrupt,
    Rollback,
    InvalidReference,
    Conflict,
    Owner,
    Stale,
    Pending,
    Missing,
    Budget,
    NeedsRecovery,
    Injected,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}
impl From<crate::Error> for Error {
    fn from(_: crate::Error) -> Self {
        Self::Io
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(c, _) = &e
            && matches!(
                c.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
        {
            Self::Busy
        } else {
            Self::Sql
        }
    }
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    schema: String,
    project: Key,
    epoch: Key,
    instance: Digest,
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
pub struct Selection {
    pub root: Object,
    pub context: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Version {
    pub revision: u64,
    pub selection: Selection,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    journal: Digest,
    operation: Key,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    pub reference: Reference,
    pub selection: Selection,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub owner: Key,
    pub revision: u64,
    pub current: Option<Version>,
    pub pending: Option<Pending>,
    pub predecessors: Vec<Version>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Role {
    Current,
    Pending,
    Predecessor,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Obligation {
    pub source: Key,
    pub owner: Key,
    pub role: Role,
    pub selection: Selection,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObligationSet {
    checkpoint: Checkpoint,
    roots: Vec<Obligation>,
}
impl ObligationSet {
    pub fn checkpoint(&self) -> &Checkpoint {
        &self.checkpoint
    }
    /// Unenrolled sources/jobs and root closure are outside this journal's proof.
    pub fn complete_claim_coverage(&self) -> bool {
        false
    }
}
impl std::ops::Deref for ObligationSet {
    type Target = [Obligation];
    fn deref(&self) -> &Self::Target {
        &self.roots
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub reference: Reference,
    pub checkpoint: Checkpoint,
    pub source_revision: u64,
    pub replayed: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Prepare {
        source: Key,
        owner: Key,
        expected_revision: u64,
        selection: Selection,
    },
    Adopt {
        source: Key,
        owner: Key,
        expected_revision: u64,
        prepared: Reference,
    },
    Abort {
        source: Key,
        owner: Key,
        expected_revision: u64,
        prepared: Reference,
    },
    Retire {
        source: Key,
        owner: Key,
        expected_revision: u64,
        predecessor_revision: u64,
    },
    Withdraw {
        source: Key,
        owner: Key,
        expected_revision: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    operation: Key,
    command: Command,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    BeforeAppend,
    BeforeCommit,
    AfterCommit,
}
#[derive(Clone)]
struct State {
    sources: BTreeMap<String, Source>,
    outcomes: BTreeMap<String, u64>,
    head: Checkpoint,
    bytes: u64,
}
impl State {
    fn transition(&mut self, request: &Request) -> Result<u64> {
        let (id, owner, expected) = match &request.command {
            Command::Prepare {
                source,
                owner,
                expected_revision,
                ..
            }
            | Command::Adopt {
                source,
                owner,
                expected_revision,
                ..
            }
            | Command::Abort {
                source,
                owner,
                expected_revision,
                ..
            }
            | Command::Retire {
                source,
                owner,
                expected_revision,
                ..
            }
            | Command::Withdraw {
                source,
                owner,
                expected_revision,
            } => (source, owner, *expected_revision),
        };
        let new_source = !self.sources.contains_key(id.as_str());
        if new_source && !matches!(request.command, Command::Prepare { .. }) {
            return Err(Error::Missing);
        }
        if new_source && self.sources.len() >= MAX_SOURCES {
            return Err(Error::Budget);
        }
        let mut source = self.sources.get(id.as_str()).cloned().unwrap_or(Source {
            owner: owner.clone(),
            revision: 0,
            current: None,
            pending: None,
            predecessors: Vec::new(),
        });
        if &source.owner != owner {
            return Err(Error::Owner);
        }
        if source.revision != expected {
            return Err(Error::Stale);
        }
        let revision = source.revision.checked_add(1).ok_or(Error::Budget)?;
        match &request.command {
            Command::Prepare { selection, .. } => {
                if selection.root.bytes == 0 || selection.root.bytes > 65_536 {
                    return Err(Error::InvalidReference);
                }
                if source.pending.is_some() {
                    return Err(Error::Pending);
                }
                if source.current.is_some() && source.predecessors.len() >= MAX_PREDECESSORS {
                    return Err(Error::Budget);
                }
                source.pending = Some(Pending {
                    reference: Reference {
                        journal: self.head.genesis.clone(),
                        operation: request.operation.clone(),
                    },
                    selection: selection.clone(),
                });
            }
            Command::Adopt { prepared, .. } | Command::Abort { prepared, .. } => {
                let pending = source.pending.as_ref().ok_or(Error::Missing)?;
                if &pending.reference != prepared {
                    return Err(Error::InvalidReference);
                }
                if matches!(request.command, Command::Adopt { .. }) {
                    if source.current.is_some() && source.predecessors.len() >= MAX_PREDECESSORS {
                        return Err(Error::Budget);
                    }
                    if let Some(old) = source.current.take() {
                        source.predecessors.push(old);
                    }
                    source.current = Some(Version {
                        revision,
                        selection: pending.selection.clone(),
                    });
                }
                source.pending = None;
            }
            Command::Retire {
                predecessor_revision,
                ..
            } => {
                let index = source
                    .predecessors
                    .iter()
                    .position(|v| v.revision == *predecessor_revision)
                    .ok_or(Error::Missing)?;
                source.predecessors.remove(index);
            }
            Command::Withdraw { .. } => {
                if source.pending.is_some() {
                    return Err(Error::Pending);
                }
                if source.current.is_none() {
                    return Err(Error::Missing);
                }
                if source.predecessors.len() >= MAX_PREDECESSORS {
                    return Err(Error::Budget);
                }
                source
                    .predecessors
                    .push(source.current.take().ok_or(Error::Missing)?);
            }
        }
        source.revision = revision;
        self.sources.insert(id.as_str().into(), source);
        self.outcomes
            .insert(request.operation.as_str().into(), revision);
        Ok(revision)
    }
}
pub struct SourceJournal {
    conn: Connection,
    state: State,
    poisoned: bool,
}
impl SourceJournal {
    /// Explicit runtime-metadata creation only; caller owns root and disk budget.
    pub fn create(root: &Path, project: Key, epoch: Key) -> Result<Self> {
        local::check_root_spelling(root)?;
        let mut nonce = [0; 32];
        getrandom::fill(&mut nonce).map_err(|_| Error::Io)?;
        let config = Config {
            schema: "tig.source-claims.design-1".into(),
            project,
            epoch,
            instance: Digest::blob(&nonce),
        };
        let bytes = encode(&config)?;
        let genesis = Digest::blob(&bytes);
        fs::create_dir(root)?;
        let mut marker = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("claims.json"))?;
        marker.write_all(&bytes)?;
        marker.sync_all()?;
        drop(marker);
        drop(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join("claims.sqlite"))?,
        );
        let conn = connect(&root.join("claims.sqlite"))?;
        conn.execute_batch("BEGIN IMMEDIATE; CREATE TABLE meta(id INTEGER PRIMARY KEY CHECK(id=1),genesis TEXT NOT NULL,sequence INTEGER NOT NULL,hash TEXT NOT NULL,bytes INTEGER NOT NULL); CREATE TABLE events(sequence INTEGER PRIMARY KEY,operation TEXT NOT NULL UNIQUE,body BLOB NOT NULL,previous TEXT NOT NULL,hash TEXT NOT NULL);")?;
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
    /// The witness is a trusted minimum; it cannot detect every unseen rollback.
    pub fn open(root: &Path, witness: &Checkpoint) -> Result<Self> {
        local::check_root_spelling(root)?;
        local::directory(root)?;
        let config: Config = local::read_record(&root.join("claims.json"))?;
        if config.schema != "tig.source-claims.design-1" {
            return Err(Error::Corrupt);
        }
        let genesis = Digest::blob(&encode(&config)?);
        if genesis != witness.genesis {
            return Err(Error::InvalidReference);
        }
        local::regular(&root.join("claims.sqlite"))?;
        let conn = connect(&root.join("claims.sqlite"))?;
        let mut journal = Self {
            conn,
            state: State {
                sources: BTreeMap::new(),
                outcomes: BTreeMap::new(),
                head: Checkpoint {
                    genesis: genesis.clone(),
                    sequence: 0,
                    hash: genesis,
                },
                bytes: 0,
            },
            poisoned: false,
        };
        journal.sync()?;
        if journal.state.head.sequence < witness.sequence {
            return Err(Error::Rollback);
        }
        let actual = if witness.sequence == 0 {
            journal.state.head.genesis.as_str().to_owned()
        } else {
            journal.conn.query_row(
                "SELECT hash FROM events WHERE sequence=?1",
                [number(witness.sequence)?],
                |r| r.get::<_, String>(0),
            )?
        };
        if actual != witness.hash.as_str() {
            return Err(Error::Rollback);
        }
        Ok(journal)
    }
    pub fn checkpoint(&self) -> Checkpoint {
        self.state.head.clone()
    }
    /// Stable authority identity across revisions/reopen, not a current-state fence.
    pub fn identity(&self) -> &Digest {
        &self.state.head.genesis
    }
    fn sync(&mut self) -> Result<()> {
        if self.poisoned {
            return Err(Error::NeedsRecovery);
        }
        let result = (|| {
            let tx = self
                .conn
                .transaction_with_behavior(TransactionBehavior::Deferred)?;
            let mut state = self.state.clone();
            catch_up(&tx, &mut state)?;
            tx.commit()?;
            self.state = state;
            Ok(())
        })();
        self.mark_uncertain(&result);
        result
    }
    fn mark_uncertain<T>(&mut self, result: &Result<T>) {
        if matches!(
            result,
            Err(Error::Sql | Error::Corrupt | Error::Rollback | Error::Injected)
        ) {
            self.poisoned = true;
        }
    }
    pub fn source(&mut self, source: &Key) -> Result<Option<Source>> {
        self.sync()?;
        Ok(self.state.sources.get(source.as_str()).cloned())
    }
    /// Positive obligations within this journal, never global absence evidence.
    pub fn obligations(&mut self, max: usize) -> Result<ObligationSet> {
        self.sync()?;
        if max > MAX_SOURCES * (MAX_PREDECESSORS + 2) {
            return Err(Error::Budget);
        }
        let mut roots = Vec::new();
        for (id, source) in &self.state.sources {
            let mut add = |role, selection: &Selection| -> Result<()> {
                if roots.len() >= max {
                    return Err(Error::Budget);
                }
                roots.push(Obligation {
                    source: Key::parse(id).map_err(|_| Error::Corrupt)?,
                    owner: source.owner.clone(),
                    role,
                    selection: selection.clone(),
                });
                Ok(())
            };
            if let Some(v) = &source.current {
                add(Role::Current, &v.selection)?;
            }
            if let Some(v) = &source.pending {
                add(Role::Pending, &v.selection)?;
            }
            for v in &source.predecessors {
                add(Role::Predecessor, &v.selection)?;
            }
        }
        Ok(ObligationSet {
            checkpoint: self.state.head.clone(),
            roots,
        })
    }
    pub fn apply(&mut self, operation: Key, command: Command) -> Result<Receipt> {
        self.apply_hook(operation, command, |_| Ok(()))
    }
    pub fn apply_hook(
        &mut self,
        operation: Key,
        command: Command,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        let result = self.apply_once(operation, command, hook);
        self.mark_uncertain(&result);
        result
    }
    fn apply_once(
        &mut self,
        operation: Key,
        command: Command,
        mut hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        if self.poisoned {
            return Err(Error::NeedsRecovery);
        }
        let request = Request { operation, command };
        let body = encode(&request).map_err(|_| Error::InvalidReference)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut state = self.state.clone();
        catch_up(&tx, &mut state)?;
        let old = tx
            .query_row(
                "SELECT sequence,length(body),body,previous,hash FROM events WHERE operation=?1",
                [request.operation.as_str()],
                |r| {
                    if unsigned(r, 1)? > 65_536 {
                        return Err(rusqlite::Error::InvalidQuery);
                    }
                    Ok((
                        unsigned(r, 0)?,
                        r.get::<_, Vec<u8>>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;
        if let Some((sequence, old, previous, hash)) = old {
            if old != body {
                return Err(Error::Conflict);
            }
            let hash = digest(&hash)?;
            if event_hash(sequence, &digest(&previous)?, &request)? != hash {
                return Err(Error::Corrupt);
            }
            let source_revision = *state
                .outcomes
                .get(request.operation.as_str())
                .ok_or(Error::Corrupt)?;
            let receipt = Receipt {
                reference: Reference {
                    journal: state.head.genesis.clone(),
                    operation: request.operation,
                },
                checkpoint: Checkpoint {
                    genesis: state.head.genesis.clone(),
                    sequence,
                    hash,
                },
                source_revision,
                replayed: true,
            };
            tx.commit()?;
            self.state = state;
            return Ok(receipt);
        }
        if state.head.sequence >= MAX_COMMANDS {
            return Err(Error::Budget);
        }
        let bytes = state
            .bytes
            .checked_add(body.len() as u64)
            .ok_or(Error::Budget)?;
        if bytes > MAX_COMMAND_BYTES {
            return Err(Error::Budget);
        }
        let revision = state.transition(&request)?;
        let sequence = state.head.sequence.checked_add(1).ok_or(Error::Budget)?;
        let hash = event_hash(sequence, &state.head.hash, &request)?;
        hook(Stage::BeforeAppend).map_err(|_| Error::Injected)?;
        tx.execute(
            "INSERT INTO events VALUES(?1,?2,?3,?4,?5)",
            params![
                number(sequence)?,
                request.operation.as_str(),
                body,
                state.head.hash.as_str(),
                hash.as_str()
            ],
        )?;
        tx.execute(
            "UPDATE meta SET sequence=?1,hash=?2,bytes=?3 WHERE id=1",
            params![number(sequence)?, hash.as_str(), number(bytes)?],
        )?;
        hook(Stage::BeforeCommit).map_err(|_| Error::Injected)?;
        tx.commit()?;
        hook(Stage::AfterCommit).map_err(|_| Error::Injected)?;
        state.head.sequence = sequence;
        state.head.hash = hash;
        state.bytes = bytes;
        let receipt = Receipt {
            reference: Reference {
                journal: state.head.genesis.clone(),
                operation: request.operation,
            },
            checkpoint: state.head.clone(),
            source_revision: revision,
            replayed: false,
        };
        self.state = state;
        Ok(receipt)
    }
}
fn catch_up(conn: &Connection, state: &mut State) -> Result<()> {
    let (genesis, sequence, hash, bytes) = conn.query_row(
        "SELECT genesis,sequence,hash,bytes FROM meta WHERE id=1",
        [],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                unsigned(r, 1)?,
                r.get::<_, String>(2)?,
                unsigned(r, 3)?,
            ))
        },
    )?;
    if genesis != state.head.genesis.as_str()
        || sequence > MAX_COMMANDS
        || bytes > MAX_COMMAND_BYTES
    {
        return Err(Error::Corrupt);
    }
    if sequence < state.head.sequence {
        return Err(Error::Rollback);
    }
    if state.head.sequence > 0 {
        let prior: String = conn.query_row(
            "SELECT hash FROM events WHERE sequence=?1",
            [number(state.head.sequence)?],
            |r| r.get(0),
        )?;
        if prior != state.head.hash.as_str() {
            return Err(Error::Rollback);
        }
    }
    let mut stmt=conn.prepare("SELECT sequence,operation,length(body),body,previous,hash FROM events WHERE sequence>?1 OR sequence<=0 ORDER BY sequence LIMIT ?2")?;
    let mut rows = stmt.query(params![
        number(state.head.sequence)?,
        number(MAX_COMMANDS + 1)?
    ])?;
    while let Some(row) = rows.next()? {
        let n = unsigned(row, 0)?;
        let operation: String = row.get(1)?;
        let len = unsigned(row, 2)?;
        if n != state.head.sequence + 1 || n > sequence || len > 65_536 {
            return Err(Error::Corrupt);
        }
        let body: Vec<u8> = row.get(3)?;
        let request: Request = decode(&body)?;
        let previous = digest(&row.get::<_, String>(4)?)?;
        let next = digest(&row.get::<_, String>(5)?)?;
        if operation != request.operation.as_str()
            || previous != state.head.hash
            || event_hash(n, &previous, &request)? != next
            || state.outcomes.contains_key(&operation)
        {
            return Err(Error::Corrupt);
        }
        state.transition(&request).map_err(|_| Error::Corrupt)?;
        state.bytes = state
            .bytes
            .checked_add(body.len() as u64)
            .ok_or(Error::Corrupt)?;
        if state.bytes > MAX_COMMAND_BYTES {
            return Err(Error::Corrupt);
        }
        state.head.sequence = n;
        state.head.hash = next;
    }
    if state.head.sequence != sequence || state.head.hash != digest(&hash)? || state.bytes != bytes
    {
        return Err(Error::Corrupt);
    }
    Ok(())
}
fn event_hash(sequence: u64, previous: &Digest, request: &Request) -> Result<Digest> {
    #[derive(Serialize)]
    struct Event<'a> {
        schema: &'static str,
        sequence: u64,
        previous: &'a Digest,
        request: &'a Request,
    }
    Ok(Digest::blob(&encode(&Event {
        schema: "tig.source-event.design-1",
        sequence,
        previous,
        request,
    })?))
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    identity::canonical_json(&serde_json::to_value(value).map_err(|_| Error::Corrupt)?)
        .map_err(|_| Error::Corrupt)
}
fn decode<T: DeserializeOwned + Serialize>(bytes: &[u8]) -> Result<T> {
    let value = identity::parse_json(bytes).map_err(|_| Error::Corrupt)?;
    let result: T = serde_json::from_value(value).map_err(|_| Error::Corrupt)?;
    if encode(&result)? != bytes {
        return Err(Error::Corrupt);
    }
    Ok(result)
}
fn digest(s: &str) -> Result<Digest> {
    Digest::parse(s).map_err(|_| Error::Corrupt)
}
fn number(n: u64) -> Result<i64> {
    i64::try_from(n).map_err(|_| Error::Budget)
}
fn unsigned(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(column)?).map_err(|_| rusqlite::Error::InvalidQuery)
}
fn connect(path: &Path) -> Result<Connection> {
    let c = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    c.busy_timeout(Duration::ZERO)?;
    c.pragma_update(None, "journal_mode", "DELETE")?;
    c.pragma_update(None, "synchronous", "FULL")?;
    let mode: String = c.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
    let sync: i64 = c.pragma_query_value(None, "synchronous", |r| r.get(0))?;
    if mode != "delete" || sync != 2 {
        return Err(Error::Corrupt);
    }
    Ok(c)
}
