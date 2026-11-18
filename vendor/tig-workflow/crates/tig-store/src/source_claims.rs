//! Bounded durable root obligations. This module grants no payload removal authority.
mod capacity;
pub use capacity::{CapacityPool, JournalCapacity};
pub mod coordinated;
pub mod outcome_budget;
pub mod policy_registry;
pub mod progress_budget;
pub mod projection;
pub mod protection;
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
pub const MAX_CANCEL_BODY: u64 = 1024;
pub const MAX_CANCEL_BYTES: u64 = MAX_SOURCES as u64 * MAX_CANCEL_BODY;
pub const MAX_EVENTS: u64 = MAX_COMMANDS + MAX_SOURCES as u64;
pub const MAX_EVENT_BYTES: u64 = MAX_COMMAND_BYTES + MAX_CANCEL_BYTES;
pub const MAX_GUARDS: usize = 16;
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
    Cancelled,
    Missing,
    Budget,
    NeedsRecovery,
    Injected,
    Restricted,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    authority: Option<protection::Enrollment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    coordinator: Option<coordinated::Binding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    routing: Option<protection::Routing>,
    #[serde(default, skip_serializing_if = "unbound")]
    bound_source: bool,
    #[serde(default, skip_serializing_if = "unbound")]
    outcome_room: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    registry: Option<policy_registry::Binding>,
    #[serde(default, skip_serializing_if = "unbound")]
    effect_fences: bool,
}
#[derive(Default)]
struct Profiles {
    authority: Option<protection::Enrollment>,
    coordinator: Option<coordinated::Binding>,
    routing: Option<protection::Routing>,
    bound_source: bool,
    outcome_room: bool,
    registry: Option<policy_registry::Binding>,
    effect_fences: bool,
}
fn unbound(value: &bool) -> bool {
    !*value
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
/// Bounded in-journal selected/pending references, not complete source coverage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlSource {
    pub source: Key,
    pub revision: u64,
    pub cancelled: bool,
    pub current: Option<Version>,
    pub pending: Option<Pending>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub owner: Key,
    pub revision: u64,
    #[serde(default)]
    pub cancelled: bool,
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
#[serde(deny_unknown_fields)]
pub struct Guard {
    pub source: Key,
    pub owner: Key,
    pub expected_revision: u64,
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
    AdoptGuarded {
        source: Key,
        owner: Key,
        expected_revision: u64,
        prepared: Reference,
        guards: Vec<Guard>,
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
    RetireGuarded {
        source: Key,
        owner: Key,
        expected_revision: u64,
        predecessor_revision: u64,
        replacement: Option<Selection>,
        guards: Vec<Guard>,
    },
    ReserveOutcomes {
        id: Key,
        owner: Key,
        plan: outcome_budget::Plan,
    },
    ReleaseOutcomes {
        budget: outcome_budget::BudgetRef,
    },
    ReserveProgress {
        id: Key,
        owner: Key,
        source: Key,
        first_revision: u64,
        guards: Vec<Guard>,
    },
    ProgressBudgeted {
        budget: progress_budget::BudgetRef,
        command: Box<Command>,
    },
    ReleaseProgress {
        budget: progress_budget::BudgetRef,
    },
    Withdraw {
        source: Key,
        owner: Key,
        expected_revision: u64,
    },
    Cancel {
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
    progress_budgets: BTreeMap<String, progress_budget::Record>,
    outcome_budgets: BTreeMap<String, outcome_budget::Record>,
    outcome_operations: BTreeMap<String, String>,
    outcome_sources: BTreeMap<String, String>,
    outcome_events: u64,
    outcome_bytes: u64,
    head: Checkpoint,
    bytes: u64,
    cancel_bytes: u64,
}
impl State {
    fn transition(&mut self, request: &Request) -> Result<u64> {
        validate_guards(&request.command)?;
        if outcome_budget::transition(self, request)? {
            return Ok(0);
        }
        if progress_budget::transition(self, request)? {
            return Ok(0);
        }
        let command = progress_budget::base(&request.command)?.clone();
        self.transition_source(&Request {
            operation: request.operation.clone(),
            command,
        })
    }
    fn transition_source(&mut self, request: &Request) -> Result<u64> {
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
            | Command::AdoptGuarded {
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
            | Command::RetireGuarded {
                source,
                owner,
                expected_revision,
                ..
            }
            | Command::Withdraw {
                source,
                owner,
                expected_revision,
            }
            | Command::Cancel {
                source,
                owner,
                expected_revision,
            } => (source, owner, *expected_revision),
            _ => return Err(Error::InvalidReference),
        };
        let new_source = !self.sources.contains_key(id.as_str());
        if new_source
            && !matches!(
                request.command,
                Command::Prepare { .. } | Command::Cancel { .. }
            )
        {
            return Err(Error::Missing);
        }
        if new_source && self.sources.len() >= MAX_SOURCES {
            return Err(Error::Budget);
        }
        let mut source = self.sources.get(id.as_str()).cloned().unwrap_or(Source {
            owner: owner.clone(),
            revision: 0,
            cancelled: false,
            current: None,
            pending: None,
            predecessors: Vec::new(),
        });
        if &source.owner != owner {
            return Err(Error::Owner);
        }
        if source.cancelled {
            return Err(Error::Cancelled);
        }
        if source.revision != expected {
            return Err(Error::Stale);
        }
        if let Command::AdoptGuarded { guards, .. } | Command::RetireGuarded { guards, .. } =
            &request.command
        {
            for guard in guards {
                let current = self
                    .sources
                    .get(guard.source.as_str())
                    .ok_or(Error::Missing)?;
                if current.owner != guard.owner {
                    return Err(Error::Owner);
                }
                if current.cancelled {
                    return Err(Error::Cancelled);
                }
                if current.revision != guard.expected_revision {
                    return Err(Error::Stale);
                }
                if current.pending.is_some() {
                    return Err(Error::Pending);
                }
                if current.current.is_none() {
                    return Err(Error::Missing);
                }
            }
        }
        let revision = source.revision.checked_add(1).ok_or(Error::Budget)?;
        match &request.command {
            Command::ReserveOutcomes { .. }
            | Command::ReleaseOutcomes { .. }
            | Command::ReserveProgress { .. }
            | Command::ProgressBudgeted { .. }
            | Command::ReleaseProgress { .. } => return Err(Error::InvalidReference),
            Command::Cancel { .. } => source.cancelled = true,
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
            Command::Adopt { prepared, .. }
            | Command::AdoptGuarded { prepared, .. }
            | Command::Abort { prepared, .. } => {
                let pending = source.pending.as_ref().ok_or(Error::Missing)?;
                if &pending.reference != prepared {
                    return Err(Error::InvalidReference);
                }
                if matches!(
                    request.command,
                    Command::Adopt { .. } | Command::AdoptGuarded { .. }
                ) {
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
            Command::RetireGuarded {
                predecessor_revision,
                replacement,
                ..
            } => {
                let index = source
                    .predecessors
                    .iter()
                    .position(|v| v.revision == *predecessor_revision)
                    .ok_or(Error::Missing)?;
                if let Some(selection) = replacement {
                    if selection.root.bytes == 0 || selection.root.bytes > 65_536 {
                        return Err(Error::InvalidReference);
                    }
                    source.predecessors.remove(index);
                    source.predecessors.push(Version {
                        revision,
                        selection: selection.clone(),
                    });
                } else {
                    source.predecessors.remove(index);
                }
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
fn validate_guards(command: &Command) -> Result<()> {
    let command = progress_budget::base(command)?;
    let (source, guards) = match command {
        Command::AdoptGuarded { source, guards, .. }
        | Command::RetireGuarded { source, guards, .. } => (Some(source), guards),
        Command::ReserveProgress { guards, .. } => (None, guards),
        _ => return Ok(()),
    };
    if guards.len() > MAX_GUARDS {
        return Err(Error::Budget);
    }
    if guards.is_empty()
        || source.is_some_and(|s| guards.iter().any(|g| g.source == *s))
        || guards
            .windows(2)
            .any(|w| w[0].source.as_str() >= w[1].source.as_str())
    {
        return Err(Error::InvalidReference);
    }
    Ok(())
}
fn admit(state: &State, request: &Request, length: u64) -> Result<(u64, u64)> {
    let cancelled = state.sources.values().filter(|s| s.cancelled).count() as u64;
    let ordinary_events = state
        .head
        .sequence
        .checked_sub(cancelled)
        .ok_or(Error::Corrupt)?;
    let ordinary_bytes = state
        .bytes
        .checked_sub(state.cancel_bytes)
        .ok_or(Error::Corrupt)?;
    let (pe, pb, pc) = progress_budget::admit(state, request, length)?;
    let (oe, ob, oc) = outcome_budget::admit(state, request, length)?;
    let extra_events = pe.checked_add(oe).ok_or(Error::Budget)?;
    let extra_bytes = pb.checked_add(ob).ok_or(Error::Budget)?;
    let covered = pc || oc;
    let (pe, pb) = progress_budget::held(state)?;
    let (oe, ob) = outcome_budget::held(state)?;
    let held_events = pe.checked_add(oe).ok_or(Error::Budget)?;
    let held_bytes = pb.checked_add(ob).ok_or(Error::Budget)?;
    let cancel_bytes = if matches!(request.command, Command::Cancel { .. }) {
        if length > MAX_CANCEL_BODY {
            return Err(Error::Budget);
        }
        state
            .cancel_bytes
            .checked_add(length)
            .ok_or(Error::Budget)?
    } else {
        if ordinary_events >= MAX_COMMANDS
            || ordinary_bytes.checked_add(length).ok_or(Error::Budget)? > MAX_COMMAND_BYTES
            || (!covered
                && (ordinary_events + 1 + held_events + extra_events > MAX_COMMANDS
                    || ordinary_bytes + length + held_bytes + extra_bytes > MAX_COMMAND_BYTES))
        {
            return Err(Error::Budget);
        }
        state.cancel_bytes
    };
    let bytes = state.bytes.checked_add(length).ok_or(Error::Budget)?;
    if state.head.sequence >= MAX_EVENTS
        || cancel_bytes > MAX_CANCEL_BYTES
        || bytes > MAX_EVENT_BYTES
    {
        return Err(Error::Budget);
    }
    Ok((bytes, cancel_bytes))
}
pub struct SourceJournal {
    conn: Connection,
    state: State,
    poisoned: bool,
    authority: Option<protection::Enrollment>,
    coordinator: Option<coordinated::Binding>,
    coordinator_root: Option<std::path::PathBuf>,
    routing: Option<protection::Routing>,
    bound_source: bool,
    outcome_room: bool,
    registry: Option<policy_registry::Binding>,
    effect_fences: bool,
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
        authority: Option<protection::Enrollment>,
    ) -> Result<Self> {
        Self::create_profiles(
            root,
            project,
            epoch,
            Profiles {
                authority,
                ..Profiles::default()
            },
        )
    }
    pub fn create_coordinated(
        root: &Path,
        project: Key,
        epoch: Key,
        binding: coordinated::Binding,
    ) -> Result<Self> {
        Self::create_profiles(
            root,
            project,
            epoch,
            Profiles {
                coordinator: Some(binding),
                ..Profiles::default()
            },
        )
    }
    fn create_profiles(root: &Path, project: Key, epoch: Key, profiles: Profiles) -> Result<Self> {
        let Profiles {
            authority,
            coordinator,
            routing,
            bound_source,
            outcome_room,
            registry,
            effect_fences,
        } = profiles;
        local::check_root_spelling(root)?;
        let mut nonce = [0; 32];
        getrandom::fill(&mut nonce).map_err(|_| Error::Io)?;
        let config = Config {
            schema: if effect_fences {
                "tig.source-claims.design-9"
            } else if registry.is_some() {
                if registry.as_ref().is_some_and(|b| b.effect_index.is_some()) {
                    "tig.source-claims.design-10"
                } else if registry.as_ref().is_some_and(|b| b.accounted) {
                    "tig.source-claims.design-8"
                } else {
                    "tig.source-claims.design-7"
                }
            } else if outcome_room {
                "tig.source-claims.design-6"
            } else if bound_source {
                "tig.source-claims.design-5"
            } else if routing.is_some() {
                "tig.source-claims.design-4"
            } else if coordinator.is_some() {
                "tig.source-claims.design-3"
            } else if authority.is_some() {
                "tig.source-claims.design-2"
            } else {
                "tig.source-claims.design-1"
            }
            .into(),
            project,
            epoch,
            instance: Digest::blob(&nonce),
            authority,
            coordinator,
            routing,
            bound_source,
            outcome_room,
            registry,
            effect_fences,
        };
        let bytes = encode(&config)?;
        let genesis = Digest::blob(&bytes);
        fs::create_dir(root)?;
        if config.authority.is_some() || config.coordinator.is_some() || config.registry.is_some() {
            fs::create_dir(root.join("fences"))?;
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
    fn open_mode(root: &Path, witness: &Checkpoint, read_only: bool) -> Result<Self> {
        local::check_root_spelling(root)?;
        local::directory(root)?;
        let config: Config = local::read_record(&root.join("claims.json"))?;
        // An old binary rejects this schema/field instead of silently skipping
        // the pending-effect gate. False is omitted to preserve old identities.
        if config.effect_fences {
            if config.schema != "tig.source-claims.design-9"
                || config.registry.is_some()
                || config.coordinator.is_some()
                || config.authority.is_none()
                || !matches!(
                    (&config.routing, config.bound_source, config.outcome_room),
                    (None, false, false) | (Some(_), true, true)
                )
            {
                return Err(Error::Corrupt);
            }
        } else if config.schema == "tig.source-claims.design-9" {
            return Err(Error::Corrupt);
        }
        if let Some(binding) = &config.registry {
            if config.schema
                != if binding.effect_index.is_some() {
                    "tig.source-claims.design-10"
                } else if binding.accounted {
                    "tig.source-claims.design-8"
                } else {
                    "tig.source-claims.design-7"
                }
                || config.authority.is_some()
                || config.coordinator.is_some()
                || config.routing.is_some()
                || config.bound_source
                || config.outcome_room
            {
                return Err(Error::Corrupt);
            }
            binding.validate()?;
            local::directory(&root.join("fences"))?;
        } else {
            match (
                &config.authority,
                &config.coordinator,
                &config.routing,
                config.bound_source,
                config.outcome_room,
                if config.effect_fences {
                    if config.routing.is_some() {
                        "tig.source-claims.design-6"
                    } else {
                        "tig.source-claims.design-2"
                    }
                } else {
                    config.schema.as_str()
                },
            ) {
                (None, None, None, false, false, "tig.source-claims.design-1") => {}
                (Some(profile), None, None, false, false, "tig.source-claims.design-2") => {
                    profile.validate().map_err(|_| Error::Corrupt)?;
                    local::directory(&root.join("fences"))?;
                }
                (None, Some(_), None, false, false, "tig.source-claims.design-3") => {
                    local::directory(&root.join("fences"))?;
                }
                (Some(profile), None, Some(route), false, false, "tig.source-claims.design-4") => {
                    profile.validate().map_err(|_| Error::Corrupt)?;
                    route.validate(profile).map_err(|_| Error::Corrupt)?;
                    local::directory(&root.join("fences"))?;
                }
                (Some(profile), None, Some(route), true, false, "tig.source-claims.design-5") => {
                    profile.validate().map_err(|_| Error::Corrupt)?;
                    route.validate(profile).map_err(|_| Error::Corrupt)?;
                    local::directory(&root.join("fences"))?;
                }
                (Some(profile), None, Some(route), true, true, "tig.source-claims.design-6") => {
                    profile.validate().map_err(|_| Error::Corrupt)?;
                    route.validate(profile).map_err(|_| Error::Corrupt)?;
                    local::directory(&root.join("fences"))?;
                }
                _ => return Err(Error::Corrupt),
            }
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
            conn,
            state: State {
                sources: BTreeMap::new(),
                outcomes: BTreeMap::new(),
                progress_budgets: BTreeMap::new(),
                outcome_budgets: BTreeMap::new(),
                outcome_operations: BTreeMap::new(),
                outcome_sources: BTreeMap::new(),
                outcome_events: 0,
                outcome_bytes: 0,
                head: Checkpoint {
                    genesis: genesis.clone(),
                    sequence: 0,
                    hash: genesis,
                },
                bytes: 0,
                cancel_bytes: 0,
            },
            poisoned: false,
            authority: config.authority,
            registry: config.registry,
            routing: config.routing,
            bound_source: config.bound_source,
            outcome_room: config.outcome_room,
            effect_fences: config.effect_fences,
            coordinator_root: if config.coordinator.is_some() {
                Some(fs::canonicalize(root)?)
            } else {
                None
            },
            coordinator: config.coordinator,
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
    /// Observe a verified minimum snapshot through a read-only connection.
    /// This grants no lease, complete coverage or recovery authority.
    pub fn observe(root: &Path, witness: &Checkpoint, source: &Key) -> Result<Observation> {
        let mut journal = Self::open_mode(root, witness, true)?;
        let source = journal.source(source)?;
        Ok(Observation {
            checkpoint: journal.checkpoint(),
            source,
        })
    }
    /// Advisory transition for an unbound journal, never an admission or receipt.
    /// Bound journals permit exact historical replay only; their gates belong
    /// to the existing coordinated/registry authority, not this preview API.
    pub fn preview(
        root: &Path,
        minimum: &Checkpoint,
        operation: Key,
        command: Command,
    ) -> Result<Preview> {
        let mut journal = Self::open_mode(root, minimum, true)?;
        let request = Request { operation, command };
        validate_guards(&request.command)?;
        let body = encode(&request).map_err(|_| Error::InvalidReference)?;
        let tx = journal
            .conn
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let mut state = journal.state.clone();
        catch_up(&tx, &mut state)?;
        let checkpoint = state.head.clone();
        let disposition = if let Some(receipt) = replay_record(&tx, &state, &request, &body)? {
            PreviewDisposition::Recorded { receipt }
        } else {
            if journal.authority.is_some()
                || journal.coordinator.is_some()
                || journal.registry.is_some()
                || journal.routing.is_some()
                || journal.bound_source
                || journal.outcome_room
                || journal.effect_fences
            {
                return Err(Error::Restricted);
            }
            admit(&state, &request, body.len() as u64)?;
            let source = match &request.command {
                Command::Prepare { source, .. }
                | Command::Adopt { source, .. }
                | Command::AdoptGuarded { source, .. }
                | Command::Abort { source, .. }
                | Command::Retire { source, .. }
                | Command::RetireGuarded { source, .. }
                | Command::Withdraw { source, .. }
                | Command::Cancel { source, .. } => source.as_str().to_owned(),
                // Budget controls do not describe a Source before/after pair.
                _ => return Err(Error::Restricted),
            };
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
    /// Recover an exact committed historical outcome without applying anything.
    /// Unknown operations refuse; returned revisions do not claim current state.
    pub fn acknowledge(
        root: &Path,
        minimum: &Checkpoint,
        operation: Key,
        command: Command,
    ) -> Result<Receipt> {
        Self::open_mode(root, minimum, true)?
            .replay(operation, command)?
            .ok_or(Error::Missing)
    }
    pub fn checkpoint(&self) -> Checkpoint {
        self.state.head.clone()
    }
    /// Stable journal instance identity, not authentication or latest authority.
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
    /// Filter existing verified control records only. No node/payload reads or
    /// filesystem discovery; a truncated result is refused rather than returned.
    pub fn control_sources(
        &mut self,
        owner: &Key,
        prefix: &str,
        max: usize,
    ) -> Result<Vec<ControlSource>> {
        if max > MAX_SOURCES
            || prefix.len() > 128
            || !prefix
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        {
            return Err(Error::Budget);
        }
        self.sync()?;
        let mut found = Vec::new();
        for (id, s) in &self.state.sources {
            if s.owner == *owner
                && id.starts_with(prefix)
                && (s.pending.is_some() || s.current.is_some())
            {
                if found.len() == max {
                    return Err(Error::Budget);
                }
                found.push(ControlSource {
                    source: Key::parse(id).map_err(|_| Error::Corrupt)?,
                    revision: s.revision,
                    cancelled: s.cancelled,
                    current: s.current.clone(),
                    pending: s.pending.clone(),
                });
            }
        }
        Ok(found)
    }
    /// Read-only coherent retained obligations; never a census of other journals.
    pub fn observe_obligations(
        root: &Path,
        minimum: &Checkpoint,
        max: usize,
    ) -> Result<ObligationSet> {
        Self::open_mode(root, minimum, true)?.obligations(max)
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
    /// Read-only exact historical outcome. Never creates or reapplies a command.
    pub fn replay(&mut self, operation: Key, command: Command) -> Result<Option<Receipt>> {
        if self.poisoned {
            return Err(Error::NeedsRecovery);
        }
        let result = (|| {
            let request = Request { operation, command };
            validate_guards(&request.command)?;
            let body = encode(&request).map_err(|_| Error::InvalidReference)?;
            let tx = self
                .conn
                .transaction_with_behavior(TransactionBehavior::Deferred)?;
            let mut state = self.state.clone();
            catch_up(&tx, &mut state)?;
            let receipt = replay_record(&tx, &state, &request, &body)?;
            tx.commit()?;
            self.state = state;
            Ok(receipt)
        })();
        self.mark_uncertain(&result);
        result
    }
    pub fn apply_hook(
        &mut self,
        operation: Key,
        command: Command,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        let result = self.apply_once(operation, command, hook, false);
        self.mark_uncertain(&result);
        result
    }
    fn apply_once(
        &mut self,
        operation: Key,
        command: Command,
        hook: impl FnMut(Stage) -> Result<()>,
        authority_write: bool,
    ) -> Result<Receipt> {
        self.apply_scoped(operation, command, hook, authority_write, false)
    }
    fn apply_scoped(
        &mut self,
        operation: Key,
        command: Command,
        mut hook: impl FnMut(Stage) -> Result<()>,
        authority_write: bool,
        registry_write: bool,
    ) -> Result<Receipt> {
        if self.registry.is_some() != registry_write {
            return Err(Error::Restricted);
        }
        if let Some(binding) = &self.registry {
            policy_registry::command_scope(binding, &command)?;
        }
        if self.authority.is_some() != authority_write
            && !coordinated::internal(self, &command, authority_write)?
        {
            return Err(Error::Restricted);
        }
        let _source_fence = coordinated::gate(self, &command, authority_write)?;
        if self.poisoned {
            return Err(Error::NeedsRecovery);
        }
        let request = Request { operation, command };
        validate_guards(&request.command)?;
        let body = encode(&request).map_err(|_| Error::InvalidReference)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut state = self.state.clone();
        catch_up(&tx, &mut state)?;
        if let Some(receipt) = replay_record(&tx, &state, &request, &body)? {
            tx.commit()?;
            self.state = state;
            return Ok(receipt);
        }
        coordinated::pending_retirement(
            self.coordinator.as_ref(),
            &state,
            &request.command,
            authority_write,
        )?;
        if self.effect_fences {
            protection::effects::admit_capacity(&state, &request.command)?;
        }
        let (bytes, cancel_bytes) = admit(&state, &request, body.len() as u64)?;
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
        state.cancel_bytes = cancel_bytes;
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
fn replay_record(
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
    if genesis != state.head.genesis.as_str() || sequence > MAX_EVENTS || bytes > MAX_EVENT_BYTES {
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
        number(MAX_EVENTS + 1)?
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
        let (next_bytes, cancel_bytes) =
            admit(state, &request, body.len() as u64).map_err(|_| Error::Corrupt)?;
        state.transition(&request).map_err(|_| Error::Corrupt)?;
        state.bytes = next_bytes;
        state.cancel_bytes = cancel_bytes;
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
