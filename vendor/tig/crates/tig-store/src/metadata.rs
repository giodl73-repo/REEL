//! Owned immutable metadata NodeStore. No media ingest, deletion or census.
use crate::local;
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{
    cell::Cell,
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
use tig_core::{
    Digest, Object, identity,
    tree::{NodeStore, TreeError, TreeResult},
};

pub const MAX_NODE_BYTES: usize = 65_536;
pub const BATCH_NODES: usize = 1024;
pub const BATCH_BYTES: usize = 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub nodes: u64,
    pub bytes: u64,
    pub database_bytes: u64,
    pub min_free_bytes: u64,
}
impl Limits {
    fn validate(&self) -> Result<()> {
        if self.nodes == 0
            || self.nodes > 2_000_000
            || self.bytes == 0
            || self.bytes > 2 * 1024 * 1024 * 1024
            || self.database_bytes < 16_384
            || self.database_bytes > 4 * 1024 * 1024 * 1024
            || !self.database_bytes.is_multiple_of(4096)
            || self.min_free_bytes < 65_536
            || self.min_free_bytes > 32 * 1024 * 1024 * 1024
        {
            return Err(Error::Budget);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Io,
    Sql,
    Busy,
    Corrupt,
    Budget,
    Rollback,
    Foreign,
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
        if let rusqlite::Error::SqliteFailure(c, _) = &e {
            if matches!(
                c.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ) {
                return Self::Busy;
            }
            if c.code == rusqlite::ErrorCode::DiskFull {
                return Self::Budget;
            }
        }
        Self::Sql
    }
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    schema: String,
    instance: Digest,
    limits: Limits,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    identity: Digest,
    generation: u64,
    nodes: u64,
    bytes: u64,
}
impl Checkpoint {
    pub fn nodes(&self) -> u64 {
        self.nodes
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Metrics {
    pub node_gets: u64,
    pub sql_node_reads: u64,
    pub node_bytes_read: u64,
    pub puts: u64,
    pub committed_flushes: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    BeforeRows,
    BeforeCommit,
    AfterCommit,
}
pub struct MetadataStore {
    conn: Connection,
    root: PathBuf,
    config: Config,
    checkpoint: Checkpoint,
    pending: BTreeMap<Digest, Vec<u8>>,
    pending_bytes: usize,
    metrics: Cell<Metrics>,
    poisoned: bool,
}
impl MetadataStore {
    /// Explicit metadata scope and bounds, not a shared capacity reservation.
    pub fn create(root: &Path, limits: Limits) -> Result<Self> {
        limits.validate()?;
        local::check_root_spelling(root)?;
        let mut nonce = [0; 32];
        getrandom::fill(&mut nonce).map_err(|_| Error::Io)?;
        let config = Config {
            schema: "tig.metadata.design-1".into(),
            instance: Digest::blob(&nonce),
            limits,
        };
        let bytes = encode(&config)?;
        let identity = Digest::blob(&bytes);
        fs::create_dir(root)?;
        let mut marker = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("metadata.json"))?;
        marker.write_all(&bytes)?;
        marker.sync_all()?;
        drop(marker);
        drop(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join("nodes.sqlite"))?,
        );
        let conn = connect(&root.join("nodes.sqlite"), limits)?;
        // Keep the OID lookup index separate from node bodies. Bulk admission
        // probes can stay in the bounded cache while immutable bodies append.
        conn.execute_batch("BEGIN IMMEDIATE; CREATE TABLE counters(id INTEGER PRIMARY KEY CHECK(id=1),identity TEXT NOT NULL,generation INTEGER NOT NULL,nodes INTEGER NOT NULL,bytes INTEGER NOT NULL); CREATE TABLE nodes(oid TEXT PRIMARY KEY NOT NULL,size INTEGER NOT NULL,body BLOB NOT NULL);")?;
        conn.execute(
            "INSERT INTO counters VALUES(1,?1,0,0,0)",
            [identity.as_str()],
        )?;
        conn.execute_batch("COMMIT")?;
        drop(conn);
        Self::open(
            root,
            &Checkpoint {
                identity,
                generation: 0,
                nodes: 0,
                bytes: 0,
            },
        )
    }
    pub fn open(root: &Path, minimum: &Checkpoint) -> Result<Self> {
        local::check_root_spelling(root)?;
        local::directory(root)?;
        local::regular(&root.join("nodes.sqlite"))?;
        let config: Config = local::read_record(&root.join("metadata.json"))?;
        config.limits.validate()?;
        if config.schema != "tig.metadata.design-1" {
            return Err(Error::Corrupt);
        }
        let identity = Digest::blob(&encode(&config)?);
        if identity != minimum.identity {
            return Err(Error::Foreign);
        }
        let conn = connect(&root.join("nodes.sqlite"), config.limits)?;
        let checkpoint = counters(&conn, &identity, config.limits)?;
        if checkpoint.generation < minimum.generation
            || checkpoint.nodes < minimum.nodes
            || checkpoint.bytes < minimum.bytes
        {
            return Err(Error::Rollback);
        }
        Ok(Self {
            conn,
            root: fs::canonicalize(root)?,
            config,
            checkpoint,
            pending: BTreeMap::new(),
            pending_bytes: 0,
            metrics: Cell::new(Metrics::default()),
            poisoned: false,
        })
    }
    pub fn checkpoint(&self) -> Checkpoint {
        self.checkpoint.clone()
    }
    pub fn metrics(&self) -> Metrics {
        self.metrics.get()
    }
    pub fn reset_metrics(&self) {
        self.metrics.set(Metrics::default());
    }
    pub fn pending_nodes(&self) -> usize {
        self.pending.len()
    }
    pub fn finish(&mut self) -> Result<Checkpoint> {
        self.finish_hook(|_| Ok(()))
    }
    pub fn finish_hook(&mut self, hook: impl FnMut(Stage) -> Result<()>) -> Result<Checkpoint> {
        self.flush(hook)?;
        Ok(self.checkpoint())
    }
    fn flush(&mut self, mut hook: impl FnMut(Stage) -> Result<()>) -> Result<()> {
        if self.poisoned {
            return Err(Error::NeedsRecovery);
        }
        if self.pending.is_empty() {
            let next = counters(&self.conn, &self.checkpoint.identity, self.config.limits)?;
            if next.generation < self.checkpoint.generation
                || next.nodes < self.checkpoint.nodes
                || next.bytes < self.checkpoint.bytes
            {
                self.poisoned = true;
                return Err(Error::Rollback);
            }
            self.checkpoint = next;
            return Ok(());
        }
        let result = (|| {
            let headroom = self
                .config
                .limits
                .database_bytes
                .checked_mul(3)
                .and_then(|n| n.checked_add(self.config.limits.min_free_bytes))
                .ok_or(Error::Budget)?;
            if fs2::available_space(&self.root)? < headroom {
                return Err(Error::Budget);
            }
            let tx = self
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut next = counters(&tx, &self.checkpoint.identity, self.config.limits)?;
            if next.generation < self.checkpoint.generation
                || next.nodes < self.checkpoint.nodes
                || next.bytes < self.checkpoint.bytes
            {
                return Err(Error::Rollback);
            }
            let mut new = Vec::new();
            for (id, bytes) in &self.pending {
                let object = Object {
                    sha256: id.clone(),
                    bytes: bytes.len() as u64,
                };
                if let Some(old) = read_node(&tx, &object, &self.metrics)? {
                    if old != *bytes {
                        return Err(Error::Corrupt);
                    }
                } else {
                    new.push((id, bytes));
                    next.nodes = next.nodes.checked_add(1).ok_or(Error::Budget)?;
                    next.bytes = next
                        .bytes
                        .checked_add(bytes.len() as u64)
                        .ok_or(Error::Budget)?;
                }
            }
            if next.nodes > self.config.limits.nodes || next.bytes > self.config.limits.bytes {
                return Err(Error::Budget);
            }
            hook(Stage::BeforeRows).map_err(|_| Error::Injected)?;
            {
                let mut insert = tx.prepare_cached("INSERT INTO nodes VALUES(?1,?2,?3)")?;
                for (id, bytes) in new {
                    insert.execute(params![id.as_str(), bytes.len() as i64, bytes])?;
                }
            }
            next.generation = next.generation.checked_add(1).ok_or(Error::Budget)?;
            tx.execute(
                "UPDATE counters SET generation=?1,nodes=?2,bytes=?3 WHERE id=1",
                params![
                    number(next.generation)?,
                    number(next.nodes)?,
                    number(next.bytes)?
                ],
            )?;
            hook(Stage::BeforeCommit).map_err(|_| Error::Injected)?;
            tx.commit()?;
            hook(Stage::AfterCommit).map_err(|_| Error::Injected)?;
            Ok(next)
        })();
        match result {
            Ok(next) => {
                self.checkpoint = next;
                self.pending.clear();
                self.pending_bytes = 0;
                let mut m = self.metrics.get();
                m.committed_flushes = m.committed_flushes.saturating_add(1);
                self.metrics.set(m);
                Ok(())
            }
            Err(e) => {
                if e != Error::Busy {
                    self.poisoned = true;
                }
                Err(e)
            }
        }
    }
    fn insert(&mut self, bytes: &[u8]) -> Result<Object> {
        if self.poisoned {
            return Err(Error::NeedsRecovery);
        }
        if bytes.len() > MAX_NODE_BYTES {
            return Err(Error::Budget);
        }
        let object = Object::from_bytes(bytes);
        let mut m = self.metrics.get();
        m.puts = m.puts.saturating_add(1);
        self.metrics.set(m);
        if let Some(old) = self.pending.get(&object.sha256) {
            if old != bytes {
                return Err(Error::Corrupt);
            }
            return Ok(object);
        }
        if let Some(old) = read_node(&self.conn, &object, &self.metrics)? {
            if old != bytes {
                return Err(Error::Corrupt);
            }
            return Ok(object);
        }
        if self.pending.len() >= BATCH_NODES || self.pending_bytes + bytes.len() > BATCH_BYTES {
            self.flush(|_| Ok(()))?;
        }
        self.pending_bytes += bytes.len();
        self.pending.insert(object.sha256.clone(), bytes.to_vec());
        Ok(object)
    }
}
impl NodeStore for MetadataStore {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        if self.poisoned {
            return Err(TreeError::Store("metadata requires reopen".into()));
        }
        if object.bytes > MAX_NODE_BYTES as u64 {
            return Err(TreeError::Budget);
        }
        let bytes = if let Some(bytes) = self.pending.get(&object.sha256) {
            if Object::from_bytes(bytes) != *object {
                return Err(TreeError::Integrity);
            }
            Some(bytes.clone())
        } else {
            read_node(&self.conn, object, &self.metrics).map_err(tree_error)?
        };
        let mut m = self.metrics.get();
        m.node_gets = m.node_gets.saturating_add(1);
        if let Some(bytes) = &bytes {
            m.node_bytes_read = m.node_bytes_read.saturating_add(bytes.len() as u64);
        }
        self.metrics.set(m);
        Ok(bytes)
    }
    fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
        self.insert(bytes).map_err(tree_error)
    }
}
fn tree_error(error: Error) -> TreeError {
    match error {
        Error::Corrupt => TreeError::Integrity,
        Error::Budget => TreeError::Budget,
        Error::Busy => TreeError::Store("metadata busy".into()),
        _ => TreeError::Store("metadata unavailable".into()),
    }
}
/// Read-only existing metadata. Recovery/writes require an explicit writer.
pub struct MetadataReader {
    conn: Connection,
    checkpoint: Checkpoint,
    metrics: Cell<Metrics>,
}
impl MetadataReader {
    pub fn open(root: &Path, minimum: &Checkpoint) -> Result<Self> {
        local::check_root_spelling(root)?;
        local::directory(root)?;
        local::regular(&root.join("nodes.sqlite"))?;
        let config: Config = local::read_record(&root.join("metadata.json"))?;
        config.limits.validate()?;
        if config.schema != "tig.metadata.design-1" {
            return Err(Error::Corrupt);
        }
        let identity = Digest::blob(&encode(&config)?);
        if identity != minimum.identity {
            return Err(Error::Foreign);
        }
        if fs::metadata(root.join("nodes.sqlite"))?.len() > config.limits.database_bytes {
            return Err(Error::Budget);
        }
        let conn = Connection::open_with_flags(
            root.join("nodes.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(Duration::ZERO)?;
        conn.pragma_update(None, "query_only", true)?;
        conn.pragma_update(None, "cache_size", -8192)?;
        let checkpoint = counters(&conn, &identity, config.limits)?;
        if checkpoint.generation < minimum.generation
            || checkpoint.nodes < minimum.nodes
            || checkpoint.bytes < minimum.bytes
        {
            return Err(Error::Rollback);
        }
        Ok(Self {
            conn,
            checkpoint,
            metrics: Cell::new(Metrics::default()),
        })
    }
    pub fn checkpoint(&self) -> &Checkpoint {
        &self.checkpoint
    }
    pub fn metrics(&self) -> Metrics {
        self.metrics.get()
    }
}
impl NodeStore for MetadataReader {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        if object.bytes > MAX_NODE_BYTES as u64 {
            return Err(TreeError::Budget);
        }
        let bytes = read_node(&self.conn, object, &self.metrics).map_err(tree_error)?;
        let mut m = self.metrics.get();
        m.node_gets = m.node_gets.saturating_add(1);
        if let Some(b) = &bytes {
            m.node_bytes_read = m.node_bytes_read.saturating_add(b.len() as u64);
        }
        self.metrics.set(m);
        Ok(bytes)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Err(TreeError::Store("read-only metadata".into()))
    }
}
fn read_node(
    conn: &Connection,
    object: &Object,
    metrics: &Cell<Metrics>,
) -> Result<Option<Vec<u8>>> {
    let mut m = metrics.get();
    m.sql_node_reads = m.sql_node_reads.saturating_add(1);
    metrics.set(m);
    let mut stmt = conn.prepare_cached("SELECT size,length(body),body FROM nodes WHERE oid=?1")?;
    let bytes = stmt
        .query_row([object.sha256.as_str()], |r| {
            let size = unsigned(r, 0)?;
            let len = unsigned(r, 1)?;
            if size != object.bytes || len != size || len > MAX_NODE_BYTES as u64 {
                return Err(rusqlite::Error::InvalidQuery);
            }
            r.get::<_, Vec<u8>>(2)
        })
        .optional()
        .map_err(|e| {
            if matches!(e, rusqlite::Error::InvalidQuery) {
                Error::Corrupt
            } else {
                e.into()
            }
        })?;
    if bytes
        .as_ref()
        .is_some_and(|b| Object::from_bytes(b) != *object)
    {
        return Err(Error::Corrupt);
    }
    Ok(bytes)
}
fn counters(conn: &Connection, identity: &Digest, limits: Limits) -> Result<Checkpoint> {
    let (id, generation, nodes, bytes) = conn.query_row(
        "SELECT identity,generation,nodes,bytes FROM counters WHERE id=1",
        [],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                unsigned(r, 1)?,
                unsigned(r, 2)?,
                unsigned(r, 3)?,
            ))
        },
    )?;
    if id != identity.as_str() || nodes > limits.nodes || bytes > limits.bytes {
        return Err(Error::Corrupt);
    }
    Ok(Checkpoint {
        identity: identity.clone(),
        generation,
        nodes,
        bytes,
    })
}
fn connect(path: &Path, limits: Limits) -> Result<Connection> {
    let c = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    c.busy_timeout(Duration::ZERO)?;
    c.pragma_update(None, "journal_mode", "DELETE")?;
    c.pragma_update(None, "synchronous", "FULL")?;
    c.pragma_update(None, "cache_size", -65536)?;
    c.pragma_update(
        None,
        "max_page_count",
        number(limits.database_bytes / 4096)?,
    )?;
    let pages: u64 = c.pragma_query_value(None, "max_page_count", |r| unsigned(r, 0))?;
    let page_size: u64 = c.pragma_query_value(None, "page_size", |r| unsigned(r, 0))?;
    let mode: String = c.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
    let sync: i64 = c.pragma_query_value(None, "synchronous", |r| r.get(0))?;
    if pages * page_size > limits.database_bytes
        || page_size != 4096
        || mode != "delete"
        || sync != 2
    {
        return Err(Error::Corrupt);
    }
    Ok(c)
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    identity::canonical_json(&serde_json::to_value(value).map_err(|_| Error::Corrupt)?)
        .map_err(|_| Error::Corrupt)
}
fn number(n: u64) -> Result<i64> {
    i64::try_from(n).map_err(|_| Error::Budget)
}
fn unsigned(row: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(i)?).map_err(|_| rusqlite::Error::InvalidQuery)
}
