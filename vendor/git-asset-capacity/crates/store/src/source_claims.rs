//! Bounded durable root obligations. This module grants no payload removal authority.
use crate::{Key, local};
use git_asset_core::{Digest, Object, identity};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

pub mod admission;
pub mod age;
pub mod subset;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Branch,
    Draft,
    Job,
    Retention,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Places {
    pub current: Vec<Key>,
    pub pending: Vec<Key>,
    pub predecessor: Vec<Key>,
}

pub const MAX_SOURCES: usize = 1024;
pub const MAX_PREDECESSORS: usize = 64;
pub const MAX_COMMANDS: u64 = 16_384;
pub const MAX_COMMAND_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_OBSERVED_SOURCES: usize = 64;
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
    AdmissionRequired,
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
        if matches!(&e, rusqlite::Error::SqliteFailure(c, _)
            if c.extended_code == rusqlite::ffi::SQLITE_READONLY_ROLLBACK)
        {
            return Self::NeedsRecovery;
        }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    admission: Option<admission::Policy>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    genesis: Digest,
    sequence: u64,
    hash: Digest,
}
impl Checkpoint {
    /// Stable enrolled journal/epoch identity, not latest-state or custody proof.
    pub fn identity(&self) -> &Digest {
        &self.genesis
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    pub owner: Key,
    pub revision: u64,
    pub current: Option<Version>,
    pub pending: Option<Pending>,
    pub predecessors: Vec<Version>,
}
/// One verified journal snapshot, not a lease, liveness or recovery decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    pub checkpoint: Checkpoint,
    pub source: Option<Source>,
}
/// Explicit source selections from one replay, not complete claim coverage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchObservation {
    pub checkpoint: Checkpoint,
    pub sources: BTreeMap<String, Option<Source>>,
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub reference: Reference,
    pub checkpoint: Checkpoint,
    pub source_revision: u64,
    pub replayed: bool,
}
/// A coherent read-only candidate, never a newly committed receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub checkpoint: Checkpoint,
    pub disposition: PreviewDisposition,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PreviewDisposition {
    Recorded {
        receipt: Receipt,
    },
    Proposed {
        before: Option<Source>,
        after: Box<Source>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Enroll {
        source: Key,
        owner: Key,
        class: Kind,
    },
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
impl Command {
    fn source(&self) -> &Key {
        match self {
            Self::Enroll { source, .. }
            | Self::Prepare { source, .. }
            | Self::Adopt { source, .. }
            | Self::Abort { source, .. }
            | Self::Retire { source, .. }
            | Self::Withdraw { source, .. } => source,
        }
    }
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
    guarded: bool,
    sources: BTreeMap<String, Source>,
    outcomes: BTreeMap<String, u64>,
    head: Checkpoint,
    bytes: u64,
}
impl State {
    fn transition(&mut self, request: &Request) -> Result<u64> {
        if let Command::Enroll {
            source,
            owner,
            class,
        } = &request.command
        {
            if !self.guarded {
                return Err(Error::AdmissionRequired);
            }
            if let Some(existing) = self.sources.get(source.as_str()) {
                return Err(if &existing.owner == owner {
                    Error::Conflict
                } else {
                    Error::Owner
                });
            }
            if self.sources.len() >= MAX_SOURCES {
                return Err(Error::Budget);
            }
            self.sources.insert(
                source.as_str().into(),
                Source {
                    kind: Some(class.clone()),
                    owner: owner.clone(),
                    revision: 1,
                    current: None,
                    pending: None,
                    predecessors: Vec::new(),
                },
            );
            self.outcomes.insert(request.operation.as_str().into(), 1);
            return Ok(1);
        }
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
            Command::Enroll { .. } => return Err(Error::AdmissionRequired),
        };
        let new_source = !self.sources.contains_key(id.as_str());
        if self.guarded && new_source {
            return Err(Error::AdmissionRequired);
        }
        if new_source && !matches!(request.command, Command::Prepare { .. }) {
            return Err(Error::Missing);
        }
        if new_source && self.sources.len() >= MAX_SOURCES {
            return Err(Error::Budget);
        }
        let mut source = self.sources.get(id.as_str()).cloned().unwrap_or(Source {
            kind: None,
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
            Command::Enroll { .. } => return Err(Error::AdmissionRequired),
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
    root: PathBuf,
    config: Config,
    conn: Connection,
    state: State,
    poisoned: bool,
}
impl SourceJournal {
    /// Explicit runtime-metadata creation only; caller owns root and disk budget.
    pub fn create(root: &Path, project: Key, epoch: Key) -> Result<Self> {
        Self::create_mode(root, project, epoch, None)
    }
    fn create_mode(
        root: &Path,
        project: Key,
        epoch: Key,
        admission: Option<admission::Policy>,
    ) -> Result<Self> {
        local::check_root_spelling(root)?;
        let mut nonce = [0; 32];
        getrandom::fill(&mut nonce).map_err(|_| Error::Io)?;
        let config = Config {
            schema: if admission.is_some() {
                "tig.source-claims.design-2"
            } else {
                "tig.source-claims.design-1"
            }
            .into(),
            admission,
            project,
            epoch,
            instance: Digest::blob(&nonce),
        };
        let bytes = encode(&config)?;
        let genesis = Digest::blob(&bytes);
        fs::create_dir(root)?;
        if config.admission.is_some() {
            admission::create_anchors(root)?;
        }
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
        Self::open_mode(root, witness, false)
    }
    /// Read an existing authority without write access, creation or recovery.
    /// Both fields come from one replay. Absence is scoped to this snapshot;
    /// later commands still require current revision and owner authorization.
    pub fn observe(root: &Path, witness: &Checkpoint, source: &Key) -> Result<Observation> {
        let journal = Self::open_mode(root, witness, true)?;
        Ok(Observation {
            checkpoint: journal.state.head.clone(),
            source: journal.state.sources.get(source.as_str()).cloned(),
        })
    }
    /// Read bounded distinct sources at one checkpoint without mutating authority.
    pub fn observe_many(
        root: &Path,
        witness: &Checkpoint,
        sources: &[Key],
    ) -> Result<BatchObservation> {
        if sources.is_empty() || sources.len() > MAX_OBSERVED_SOURCES {
            return Err(Error::Budget);
        }
        let mut selected = BTreeMap::new();
        for source in sources {
            if selected.insert(source.as_str().to_owned(), None).is_some() {
                return Err(Error::InvalidReference);
            }
        }
        let journal = Self::open_mode(root, witness, true)?;
        for (id, state) in &mut selected {
            *state = journal.state.sources.get(id).cloned();
        }
        Ok(BatchObservation {
            checkpoint: journal.state.head.clone(),
            sources: selected,
        })
    }
    /// Observe every registered source in one bounded replay. This detects
    /// unexpected sources omitted from a caller's enrollment; no global scope
    /// or new source-admission fence is implied.
    pub fn observe_all(
        root: &Path,
        witness: &Checkpoint,
        max_sources: usize,
    ) -> Result<BatchObservation> {
        if max_sources == 0 || max_sources > MAX_SOURCES {
            return Err(Error::Budget);
        }
        let journal = Self::open_mode(root, witness, true)?;
        if journal.state.sources.len() > max_sources {
            return Err(Error::Budget);
        }
        Ok(BatchObservation {
            checkpoint: journal.state.head.clone(),
            sources: journal
                .state
                .sources
                .into_iter()
                .map(|(id, source)| (id, Some(source)))
                .collect(),
        })
    }
    fn open_mode(root: &Path, witness: &Checkpoint, read_only: bool) -> Result<Self> {
        local::check_root_spelling(root)?;
        local::directory(root)?;
        let config: Config = local::read_record(&root.join("claims.json"))?;
        match (&config.schema[..], &config.admission) {
            ("tig.source-claims.design-1", None) => (),
            ("tig.source-claims.design-2", Some(policy)) if policy.valid() => (),
            _ => return Err(Error::Corrupt),
        }
        let genesis = Digest::blob(&encode(&config)?);
        if genesis != witness.genesis {
            return Err(Error::InvalidReference);
        }
        local::regular(&root.join("claims.sqlite"))?;
        let conn = if read_only {
            connect_read_only(&root.join("claims.sqlite"))?
        } else {
            connect(&root.join("claims.sqlite"))?
        };
        let mut journal = Self {
            root: root.to_owned(),
            config: config.clone(),
            conn,
            state: State {
                guarded: config.admission.is_some(),
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
        journal.verify_minimum(witness)?;
        Ok(journal)
    }
    /// Shared exact ancestry check for open and stricter caller-admitted minima.
    fn verify_minimum(&self, witness: &Checkpoint) -> Result<()> {
        if self.identity() != witness.identity() {
            return Err(Error::InvalidReference);
        }
        if self.state.head.sequence < witness.sequence {
            return Err(Error::Rollback);
        }
        let actual = if witness.sequence == 0 {
            self.state.head.genesis.as_str().to_owned()
        } else {
            self.conn.query_row(
                "SELECT hash FROM events WHERE sequence=?1",
                [number(witness.sequence)?],
                |r| r.get::<_, String>(0),
            )?
        };
        if actual != witness.hash.as_str() {
            return Err(Error::Rollback);
        }
        Ok(())
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
            Err(Error::Sql
                | Error::Corrupt
                | Error::Rollback
                | Error::Injected
                | Error::NeedsRecovery)
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
    pub fn preview(
        root: &Path,
        minimum: &Checkpoint,
        operation: Key,
        command: Command,
    ) -> Result<Preview> {
        Self::preview_mode(root, minimum, operation, command, false)
    }
    fn preview_mode(
        root: &Path,
        minimum: &Checkpoint,
        operation: Key,
        command: Command,
        admitted: bool,
    ) -> Result<Preview> {
        let mut journal = Self::open_mode(root, minimum, true)?;
        if journal.config.admission.is_some() && !admitted {
            return Err(Error::AdmissionRequired);
        }
        let request = Request { operation, command };
        let body = encode(&request).map_err(|_| Error::InvalidReference)?;
        let tx = journal
            .conn
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let mut state = journal.state.clone();
        catch_up(&tx, &mut state)?;
        let checkpoint = state.head.clone();
        let disposition = if let Some(receipt) = recorded_receipt(&tx, &state, &request, &body)? {
            PreviewDisposition::Recorded { receipt }
        } else {
            growth_bytes(&state, body.len())?;
            let source = request.command.source().as_str().to_owned();
            let before = state.sources.get(&source).cloned();
            state.transition(&request)?;
            let after = state.sources.get(&source).cloned().ok_or(Error::Corrupt)?;
            PreviewDisposition::Proposed {
                before,
                after: Box::new(after),
            }
        };
        tx.commit()?;
        Ok(Preview {
            checkpoint,
            disposition,
        })
    }
    /// Recover only an exact previously committed acknowledgment. Unknown IDs
    /// refuse without executing a transition or opening a writable authority.
    /// The returned revision/checkpoint are historical, not current state.
    pub fn acknowledge(
        root: &Path,
        minimum: &Checkpoint,
        operation: Key,
        command: Command,
    ) -> Result<Receipt> {
        let mut journal = Self::open_mode(root, minimum, true)?;
        journal.apply_once(operation, command, true, |_| Ok(()))
    }
    pub fn apply_hook(
        &mut self,
        operation: Key,
        command: Command,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        if self.config.admission.is_some() {
            return Err(Error::AdmissionRequired);
        }
        let result = self.apply_once(operation, command, false, hook);
        self.mark_uncertain(&result);
        result
    }
    fn apply_admitted(
        &mut self,
        operation: Key,
        command: Command,
        permit: &admission::Permit,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        self.check_guarded_marker()?;
        if !permit.matches(
            self.identity(),
            &Request {
                operation: operation.clone(),
                command: command.clone(),
            },
        )? {
            return Err(Error::AdmissionRequired);
        }
        let result = self
            .apply_requests_if(
                &[Request { operation, command }],
                false,
                permit.conditions(),
                hook,
            )
            .and_then(|mut receipts| receipts.pop().ok_or(Error::Corrupt));
        self.mark_uncertain(&result);
        result
    }
    fn check_guarded_marker(&self) -> Result<()> {
        if self.config.admission.is_none() {
            return Err(Error::AdmissionRequired);
        }
        local::directory(&self.root)?;
        let actual: Config = local::read_record(&self.root.join("claims.json"))?;
        if actual != self.config {
            return Err(Error::InvalidReference);
        }
        Ok(())
    }
    fn apply_once(
        &mut self,
        operation: Key,
        command: Command,
        recorded_only: bool,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        self.apply_requests(&[Request { operation, command }], recorded_only, hook)?
            .pop()
            .ok_or(Error::Corrupt)
    }
    fn apply_atomic_admitted(
        &mut self,
        requests: &[Request],
        permit: &admission::AtomicPermit,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Vec<Receipt>> {
        self.check_guarded_marker()?;
        if !permit.matches(self.identity(), requests)? {
            return Err(Error::AdmissionRequired);
        }
        let result = self.apply_requests(requests, false, hook);
        self.mark_uncertain(&result);
        result
    }
    /// Up to three existing commands; no new event encoding or journal engine.
    fn apply_requests(
        &mut self,
        requests: &[Request],
        recorded_only: bool,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Vec<Receipt>> {
        self.apply_requests_if(requests, recorded_only, &[], hook)
    }
    fn apply_requests_if(
        &mut self,
        requests: &[Request],
        recorded_only: bool,
        conditions: &[admission::SourceCondition],
        mut hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Vec<Receipt>> {
        if self.poisoned {
            return Err(Error::NeedsRecovery);
        }
        if requests.is_empty()
            || requests.len() > 3
            || requests.iter().enumerate().any(|(i, r)| {
                requests[..i]
                    .iter()
                    .any(|prior| prior.operation == r.operation)
            })
        {
            return Err(Error::Conflict);
        }
        let bodies = requests
            .iter()
            .map(|request| encode(request).map_err(|_| Error::InvalidReference))
            .collect::<Result<Vec<_>>>()?;
        let tx = self.conn.transaction_with_behavior(if recorded_only {
            TransactionBehavior::Deferred
        } else {
            TransactionBehavior::Immediate
        })?;
        let mut state = self.state.clone();
        catch_up(&tx, &mut state)?;
        let recorded = requests
            .iter()
            .zip(&bodies)
            .map(|(request, body)| recorded_receipt(&tx, &state, request, body))
            .collect::<Result<Vec<_>>>()?;
        let count = recorded.iter().filter(|r| r.is_some()).count();
        if count != 0 && count != requests.len() {
            return Err(Error::NeedsRecovery);
        }
        if count == requests.len() {
            tx.commit()?;
            self.state = state;
            return Ok(recorded.into_iter().flatten().collect());
        }
        if recorded_only {
            return Err(Error::Missing);
        }
        if conditions.iter().any(|c| {
            state
                .sources
                .get(c.source.as_str())
                .is_none_or(|s| s.revision != c.revision)
        }) {
            return Err(Error::Stale);
        }
        // Admit the complete group before its first append. One remaining event
        // slot is not room for an atomic Prepare/Adopt pair.
        if state
            .head
            .sequence
            .checked_add(requests.len() as u64)
            .ok_or(Error::Budget)?
            > MAX_COMMANDS
        {
            return Err(Error::Budget);
        }
        let body_bytes = bodies.iter().try_fold(0usize, |total, b| {
            total.checked_add(b.len()).ok_or(Error::Budget)
        })?;
        growth_bytes(&state, body_bytes)?;
        let mut receipts = Vec::new();
        for (request, body) in requests.iter().zip(bodies) {
            let bytes = growth_bytes(&state, body.len())?;
            let revision = state.transition(request)?;
            let sequence = state.head.sequence.checked_add(1).ok_or(Error::Budget)?;
            let hash = event_hash(sequence, &state.head.hash, request)?;
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
            state.head.sequence = sequence;
            state.head.hash = hash;
            state.bytes = bytes;
            receipts.push(Receipt {
                reference: Reference {
                    journal: state.head.genesis.clone(),
                    operation: request.operation.clone(),
                },
                checkpoint: state.head.clone(),
                source_revision: revision,
                replayed: false,
            });
        }
        hook(Stage::BeforeCommit).map_err(|_| Error::Injected)?;
        tx.commit()?;
        hook(Stage::AfterCommit).map_err(|_| Error::Injected)?;
        self.state = state;
        Ok(receipts)
    }
}
fn growth_bytes(state: &State, body_len: usize) -> Result<u64> {
    let bytes = state
        .bytes
        .checked_add(body_len as u64)
        .ok_or(Error::Budget)?;
    if state.head.sequence >= MAX_COMMANDS || bytes > MAX_COMMAND_BYTES {
        return Err(Error::Budget);
    }
    Ok(bytes)
}
fn recorded_receipt(
    conn: &Connection,
    state: &State,
    request: &Request,
    body: &[u8],
) -> Result<Option<Receipt>> {
    let old = conn
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
    let Some((sequence, old, previous, hash)) = old else {
        return Ok(None);
    };
    if old != body {
        return Err(Error::Conflict);
    }
    let hash = digest(&hash)?;
    if event_hash(sequence, &digest(&previous)?, request)? != hash {
        return Err(Error::Corrupt);
    }
    let source_revision = *state
        .outcomes
        .get(request.operation.as_str())
        .ok_or(Error::Corrupt)?;
    Ok(Some(Receipt {
        reference: Reference {
            journal: state.head.genesis.clone(),
            operation: request.operation.clone(),
        },
        checkpoint: Checkpoint {
            genesis: state.head.genesis.clone(),
            sequence,
            hash,
        },
        source_revision,
        replayed: true,
    }))
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
fn connect_read_only(path: &Path) -> Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::ZERO)?;
    connection.pragma_update(None, "query_only", true)?;
    let mode: String = connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    if mode != "delete" {
        return Err(Error::Corrupt);
    }
    Ok(connection)
}
