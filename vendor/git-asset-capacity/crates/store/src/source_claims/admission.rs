//! Mandatory guarded source mutation and live subject-scoped coordination.
//! A scoped absence fact is not independent custody or removal authority.
use super::{
    Checkpoint, Command, Kind, Places, PreviewDisposition, Receipt, Source, SourceJournal, Stage,
};
use crate::{
    Key, local,
    protection::{BudgetNodes, Limits},
};
use git_asset_core::{
    Digest, LogicalPath, Object,
    tree::{NodeStore, Tree, TreeError, TreeLimits},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    path::Path,
    time::Instant,
};
pub(crate) mod bulk;
pub(crate) mod roster;

pub const PARTITIONS: usize = 256;
/// Ephemeral admission predicate, not a new journal record or retention grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCondition {
    pub source: Key,
    pub revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Journal(super::Error),
    InvalidPolicy,
    Budget,
    Busy,
    WrongAuthority,
    IncompleteClosure,
    ConflictingSize,
    Io,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<super::Error> for Error {
    fn from(e: super::Error) -> Self {
        Self::Journal(e)
    }
}
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
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Active branch/draft/job demands suppress inactive history proximity.
    /// Explicit retention-class demands are always additive.
    pub prefer_active: bool,
    pub classes: BTreeMap<Kind, Places>,
}
impl Policy {
    fn normalized(&self) -> Result<Self> {
        if self.classes.len() != 4
            || [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
                .iter()
                .any(|k| !self.classes.contains_key(k))
        {
            return Err(Error::InvalidPolicy);
        }
        for p in self.classes.values() {
            for places in [&p.current, &p.pending, &p.predecessor] {
                if places.is_empty() || places.len() > 16 {
                    return Err(Error::InvalidPolicy);
                }
            }
        }
        let mut policy = self.clone();
        for p in policy.classes.values_mut() {
            for places in [&mut p.current, &mut p.pending, &mut p.predecessor] {
                places.sort_by(|a, b| a.as_str().cmp(b.as_str()));
                if places.windows(2).any(|p| p[0] == p[1]) {
                    return Err(Error::InvalidPolicy);
                }
            }
        }
        Ok(policy)
    }
    pub(super) fn valid(&self) -> bool {
        self.normalized().is_ok_and(|p| p == *self)
    }
}

pub(super) fn create_anchors(root: &Path) -> super::Result<()> {
    fs::create_dir(root.join("admission-fences"))?;
    for partition in 0..PARTITIONS {
        OpenOptions::new().write(true).create_new(true).open(
            root.join("admission-fences")
                .join(format!("{partition:02x}")),
        )?;
    }
    Ok(())
}
struct Locks(Vec<File>);
impl Drop for Locks {
    fn drop(&mut self) {
        for file in &self.0 {
            let _ = file.unlock();
        }
    }
}
fn partition(object: &Digest) -> Result<usize> {
    usize::from_str_radix(&object.as_str()[..2], 16).map_err(|_| Error::WrongAuthority)
}
fn locks(root: &Path, parts: &BTreeSet<usize>, exclusive: bool) -> Result<Locks> {
    let directory = root.join("admission-fences");
    local::directory(&directory)?;
    let mut held = Locks(Vec::new());
    for part in parts {
        if *part >= PARTITIONS {
            return Err(Error::WrongAuthority);
        }
        let path = directory.join(format!("{part:02x}"));
        local::regular(&path)?;
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        if file.metadata()?.len() != 0 {
            return Err(Error::WrongAuthority);
        }
        let result = if exclusive {
            file.try_lock()
        } else {
            file.try_lock_shared()
        };
        result.map_err(|e| match e {
            fs::TryLockError::WouldBlock => Error::Busy,
            fs::TryLockError::Error(_) => Error::Io,
        })?;
        held.0.push(file);
    }
    Ok(held)
}
/// Only this module constructs a permit, after resolving exact before/after roots.
pub(super) struct Permit {
    authority: Digest,
    request: Digest,
    conditions: Vec<SourceCondition>,
    _locks: Locks,
}
impl Permit {
    pub(super) fn conditions(&self) -> &[SourceCondition] {
        &self.conditions
    }
    pub(super) fn matches(
        &self,
        authority: &Digest,
        request: &super::Request,
    ) -> super::Result<bool> {
        Ok(authority == &self.authority && Digest::blob(&super::encode(request)?) == self.request)
    }
}
pub(super) struct AtomicPermit {
    authority: Digest,
    requests: Digest,
    _locks: Locks,
}
impl AtomicPermit {
    pub(super) fn matches(
        &self,
        authority: &Digest,
        requests: &[super::Request],
    ) -> super::Result<bool> {
        Ok(authority == &self.authority
            && Digest::blob(&super::encode(&requests)?) == self.requests)
    }
}
/// An atomic composition of the existing Prepare and Adopt commands.
/// This changes desired protection only, never byte availability or custody.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Select {
    pub prepare: Key,
    pub adopt: Key,
    pub source: Key,
    pub owner: Key,
    pub expected_revision: u64,
    pub selection: super::Selection,
}
impl Select {
    pub(super) fn requests(&self, authority: &Digest) -> super::Result<[super::Request; 2]> {
        Ok([
            super::Request {
                operation: self.prepare.clone(),
                command: Command::Prepare {
                    source: self.source.clone(),
                    owner: self.owner.clone(),
                    expected_revision: self.expected_revision,
                    selection: self.selection.clone(),
                },
            },
            super::Request {
                operation: self.adopt.clone(),
                command: Command::Adopt {
                    source: self.source.clone(),
                    owner: self.owner.clone(),
                    expected_revision: self
                        .expected_revision
                        .checked_add(1)
                        .ok_or(super::Error::Budget)?,
                    prepared: super::Reference {
                        journal: authority.clone(),
                        operation: self.prepare.clone(),
                    },
                },
            },
        ])
    }
}
pub struct SubjectFence {
    authority: Digest,
    object: Object,
    _locks: Locks,
}
/// Live shared use only, not a durable claim or removal/custody grant.
/// Compatible with guarded claim admission; blocks exclusive subject fences.
pub struct SubjectUse {
    authority: Digest,
    object: Object,
    _locks: Locks,
}
impl SubjectUse {
    pub fn authority(&self) -> &Digest {
        &self.authority
    }
    pub fn object(&self) -> &Object {
        &self.object
    }
}
impl SubjectFence {
    pub fn authority(&self) -> &Digest {
        &self.authority
    }
    pub fn object(&self) -> &Object {
        &self.object
    }
}
/// One predicate under a live namespace fence. Not Clone/Serialize and not a
/// deletion grant; provider scope, custody and retirement authority are separate.
pub struct ScopedAbsence<'a> {
    fence: &'a SubjectFence,
    place: Key,
    checkpoint: Checkpoint,
}
impl ScopedAbsence<'_> {
    pub fn object(&self) -> &Object {
        &self.fence.object
    }
    pub fn place(&self) -> &Key {
        &self.place
    }
    pub fn authority(&self) -> &Digest {
        &self.fence.authority
    }
    pub fn checkpoint(&self) -> &Checkpoint {
        &self.checkpoint
    }
    pub fn authorizes_removal(&self) -> bool {
        false
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Demand {
    bytes: u64,
    active: BTreeSet<String>,
    history: BTreeSet<String>,
    retained: BTreeSet<String>,
}
impl Demand {
    fn extend(&mut self, other: &Self) -> Result<()> {
        if self.bytes != other.bytes {
            return Err(Error::ConflictingSize);
        }
        self.active.extend(other.active.iter().cloned());
        self.history.extend(other.history.iter().cloned());
        self.retained.extend(other.retained.iter().cloned());
        Ok(())
    }
    fn required(&self, policy: &Policy, place: &str) -> bool {
        self.retained.contains(place)
            || self.active.contains(place)
            || ((!policy.prefer_active || self.active.is_empty()) && self.history.contains(place))
    }
}
pub(crate) struct Work<'a, S> {
    nodes: BudgetNodes<'a, S>,
    limits: &'a Limits,
    roots: usize,
    bindings: usize,
    associations: usize,
}
impl<'a, S: NodeStore> Work<'a, S> {
    pub(crate) fn new(nodes: &'a S, limits: &'a Limits, start: Instant) -> Result<Self> {
        limits.validate().map_err(|_| Error::Budget)?;
        Ok(Self {
            nodes: BudgetNodes::new(nodes, limits, start),
            limits,
            roots: 0,
            bindings: 0,
            associations: 0,
        })
    }
    pub(crate) fn timed(&self) -> Result<()> {
        self.nodes.timed().map_err(|_| Error::Budget)
    }
    fn source(&mut self, source: &Source, policy: &Policy) -> Result<BTreeMap<Digest, Demand>> {
        self.source_excluding(source, policy, &BTreeMap::new())
    }
    fn source_excluding(
        &mut self,
        source: &Source,
        policy: &Policy,
        removed: &BTreeMap<u64, BTreeSet<LogicalPath>>,
    ) -> Result<BTreeMap<Digest, Demand>> {
        let kind = source.kind.as_ref().ok_or(Error::WrongAuthority)?;
        let places = policy.classes.get(kind).ok_or(Error::InvalidPolicy)?;
        let mut roots = Vec::new();
        if let Some(current) = &source.current {
            roots.push((&current.selection, &places.current, false, None));
        }
        if let Some(pending) = &source.pending {
            roots.push((&pending.selection, &places.pending, false, None));
        }
        for old in &source.predecessors {
            roots.push((
                &old.selection,
                &places.predecessor,
                true,
                Some(old.revision),
            ));
        }
        let mut result = BTreeMap::<Digest, Demand>::new();
        for (selection, places, history, revision) in roots {
            self.timed()?;
            self.roots += 1;
            if self.roots > self.limits.roots {
                return Err(Error::Budget);
            }
            let limits = TreeLimits {
                max_entries: self.limits.bindings - self.bindings,
                max_reads: self.limits.reads,
                max_read_bytes: self.limits.read_bytes,
                ..TreeLimits::default()
            };
            let (entries, _) = Tree::from_root(selection.root.clone())
                .entries(&mut self.nodes, limits)
                .map_err(|e| {
                    if e == TreeError::Budget {
                        Error::Budget
                    } else {
                        Error::IncompleteClosure
                    }
                })?;
            self.bindings += entries.len();
            self.associations = self
                .associations
                .checked_add(
                    entries
                        .len()
                        .checked_mul(places.len())
                        .ok_or(Error::Budget)?,
                )
                .ok_or(Error::Budget)?;
            if self.associations > self.limits.associations {
                return Err(Error::Budget);
            }
            for (path, object) in &entries {
                if revision
                    .is_some_and(|r| removed.get(&r).is_some_and(|paths| paths.contains(path)))
                {
                    continue;
                }
                let demand = result
                    .entry(object.sha256.clone())
                    .or_insert_with(|| Demand {
                        bytes: object.bytes,
                        ..Demand::default()
                    });
                if demand.bytes != object.bytes {
                    return Err(Error::ConflictingSize);
                }
                let bucket = if *kind == Kind::Retention {
                    &mut demand.retained
                } else if history {
                    &mut demand.history
                } else {
                    &mut demand.active
                };
                bucket.extend(places.iter().map(|p| p.as_str().to_owned()));
            }
        }
        self.timed()?;
        Ok(result)
    }
}

pub struct Namespace {
    journal: SourceJournal,
}
/// Exact desired assignment within this namespace checkpoint. This is not a
/// live protection guard and says nothing about other namespace authorities.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Assignment {
    pub object: Object,
    pub places: Vec<Key>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Requirements {
    pub namespace: Digest,
    pub checkpoint: Checkpoint,
    pub entries: Vec<Assignment>,
}
impl Namespace {
    pub fn requirements(
        &mut self,
        nodes: &impl NodeStore,
        limits: &Limits,
    ) -> Result<Requirements> {
        let start = Instant::now();
        let mut work = Work::new(nodes, limits, start)?;
        self.requirements_work(&mut work)
    }
    pub(crate) fn requirements_work<S: NodeStore>(
        &mut self,
        work: &mut Work<'_, S>,
    ) -> Result<Requirements> {
        self.journal.check_guarded_marker()?;
        self.journal.sync()?;
        let checkpoint = self.journal.checkpoint();
        let policy = self.policy();
        let mut all = BTreeMap::<Digest, Demand>::new();
        for source in self.journal.state.sources.values() {
            for (id, demand) in work.source(source, policy)? {
                match all.entry(id) {
                    std::collections::btree_map::Entry::Vacant(e) => {
                        e.insert(demand);
                    }
                    std::collections::btree_map::Entry::Occupied(mut e) => {
                        e.get_mut().extend(&demand)?
                    }
                }
            }
        }
        let keys = policy
            .classes
            .values()
            .flat_map(|p| p.current.iter().chain(&p.pending).chain(&p.predecessor))
            .map(|k| k.as_str())
            .collect::<BTreeSet<_>>();
        let mut entries = Vec::new();
        for (id, demand) in all {
            let places = keys
                .iter()
                .filter(|p| demand.required(policy, p))
                .map(|p| Key::parse(p).map_err(Error::from))
                .collect::<Result<Vec<_>>>()?;
            entries.push(Assignment {
                object: Object {
                    sha256: id,
                    bytes: demand.bytes,
                },
                places,
            });
        }
        work.timed()?;
        self.journal.check_guarded_marker()?;
        Ok(Requirements {
            namespace: self.identity().clone(),
            checkpoint,
            entries,
        })
    }
    pub(crate) fn check_marker(&self) -> Result<()> {
        Ok(self.journal.check_guarded_marker()?)
    }
    pub fn create(root: &Path, project: Key, epoch: Key, policy: Policy) -> Result<Self> {
        let policy = policy.normalized()?;
        Ok(Self {
            journal: SourceJournal::create_mode(root, project, epoch, Some(policy))?,
        })
    }
    pub fn open(root: &Path, minimum: &Checkpoint) -> Result<Self> {
        let journal = SourceJournal::open(root, minimum)?;
        if journal.config.admission.is_none() {
            return Err(Error::WrongAuthority);
        }
        journal.check_guarded_marker()?;
        Ok(Self { journal })
    }
    pub fn checkpoint(&self) -> Checkpoint {
        self.journal.checkpoint()
    }
    /// Observe the guarded journal and require this historical minimum's ancestry.
    /// This is no lease, independent latest-authority witness or complete roster.
    pub fn observe(&mut self, minimum: &Checkpoint) -> Result<Checkpoint> {
        self.refresh_minimum(minimum)?;
        Ok(self.checkpoint())
    }
    pub(crate) fn refresh_minimum(&mut self, minimum: &Checkpoint) -> Result<()> {
        self.journal.check_guarded_marker()?;
        self.journal.sync()?;
        self.verify_minimum(minimum)
    }
    pub(crate) fn verify_minimum(&self, minimum: &Checkpoint) -> Result<()> {
        self.journal.check_guarded_marker()?;
        Ok(self.journal.verify_minimum(minimum)?)
    }
    pub fn identity(&self) -> &Digest {
        self.journal.identity()
    }
    pub fn source(&mut self, key: &Key) -> Result<Option<Source>> {
        self.journal.check_guarded_marker()?;
        Ok(self.journal.source(key)?)
    }
    /// Owned pure planner snapshot, including existing slot/event/byte usage.
    /// Mutating the returned value cannot change this journal or grant a permit.
    pub(super) fn planning_state(&mut self) -> Result<super::State> {
        self.journal.check_guarded_marker()?;
        self.journal.sync()?;
        Ok(self.journal.state.clone())
    }
    /// Fixed policy changes require explicit creation/adoption of another epoch.
    pub fn policy(&self) -> &Policy {
        self.journal
            .config
            .admission
            .as_ref()
            .expect("guarded namespace")
    }
    pub fn apply(
        &mut self,
        operation: Key,
        command: Command,
        nodes: &impl NodeStore,
        limits: &Limits,
    ) -> Result<Receipt> {
        self.apply_hook(operation, command, nodes, limits, |_| Ok(()))
    }
    pub fn apply_if_sources(
        &mut self,
        operation: Key,
        command: Command,
        conditions: &[SourceCondition],
        nodes: &impl NodeStore,
        limits: &Limits,
    ) -> Result<Receipt> {
        self.apply_if_hook(operation, command, conditions, nodes, limits, |_| Ok(()))
    }
    /// Historical receipt only; conditions are never re-granted by inspection.
    pub fn command_status(&mut self, operation: Key, command: Command) -> Result<Option<Receipt>> {
        self.journal.check_guarded_marker()?;
        match self
            .journal
            .apply_once(operation, command, true, |_| Ok(()))
        {
            Ok(receipt) => Ok(Some(receipt)),
            Err(super::Error::Missing) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    /// Exact historical recovery; no commands are appended and no tree IO occurs.
    pub fn selection_status(&mut self, selection: &Select) -> Result<Option<Receipt>> {
        self.journal.check_guarded_marker()?;
        let requests = selection.requests(self.identity())?;
        match self.journal.apply_requests(&requests, true, |_| Ok(())) {
            Ok(mut receipts) => Ok(receipts.pop()),
            Err(super::Error::Missing) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    pub fn select(
        &mut self,
        selection: &Select,
        nodes: &impl NodeStore,
        limits: &Limits,
    ) -> Result<Receipt> {
        self.select_hook(selection, nodes, limits, |_| Ok(()))
    }
    fn select_hook(
        &mut self,
        selection: &Select,
        nodes: &impl NodeStore,
        limits: &Limits,
        hook: impl FnMut(Stage) -> super::Result<()>,
    ) -> Result<Receipt> {
        let requests = selection.requests(self.identity())?.to_vec();
        self.select_requests(selection, requests, nodes, limits, hook)
    }
    /// Atomically Prepare/Adopt/Withdraw a historical selection. No observer can
    /// see artificial active use suppressing the original history placement.
    pub fn select_predecessor(
        &mut self,
        selection: &Select,
        withdraw: Key,
        nodes: &impl NodeStore,
        limits: &Limits,
    ) -> Result<Receipt> {
        self.select_predecessor_hook(selection, withdraw, nodes, limits, |_| Ok(()))
    }
    fn select_predecessor_hook(
        &mut self,
        selection: &Select,
        withdraw: Key,
        nodes: &impl NodeStore,
        limits: &Limits,
        hook: impl FnMut(Stage) -> super::Result<()>,
    ) -> Result<Receipt> {
        let mut requests = selection.requests(self.identity())?.to_vec();
        requests.push(super::Request {
            operation: withdraw,
            command: Command::Withdraw {
                source: selection.source.clone(),
                owner: selection.owner.clone(),
                expected_revision: selection
                    .expected_revision
                    .checked_add(2)
                    .ok_or(super::Error::Budget)?,
            },
        });
        self.select_requests(selection, requests, nodes, limits, hook)
    }
    fn select_requests(
        &mut self,
        selection: &Select,
        requests: Vec<super::Request>,
        nodes: &impl NodeStore,
        limits: &Limits,
        hook: impl FnMut(Stage) -> super::Result<()>,
    ) -> Result<Receipt> {
        let start = Instant::now();
        limits.validate().map_err(|_| Error::Budget)?;
        self.journal.check_guarded_marker()?;
        self.journal.sync()?;
        match self.journal.apply_requests(&requests, true, |_| Ok(())) {
            Ok(mut receipts) => {
                return receipts
                    .pop()
                    .ok_or(super::Error::Corrupt)
                    .map_err(Into::into);
            }
            Err(super::Error::Missing) => {}
            Err(error) => return Err(error.into()),
        }
        let mut after = self.journal.state.clone();
        let before = after.sources.get(selection.source.as_str()).cloned();
        for request in &requests {
            after.transition(request)?;
        }
        let after = after
            .sources
            .get(selection.source.as_str())
            .ok_or(super::Error::Corrupt)?;
        let mut work = Work::new(nodes, limits, start)?;
        let before = match before {
            Some(source) => work.source(&source, self.policy())?,
            None => BTreeMap::new(),
        };
        let after = work.source(after, self.policy())?;
        let mut parts = BTreeSet::new();
        for id in before.keys().chain(after.keys()) {
            if before.get(id) != after.get(id) {
                parts.insert(partition(id)?);
            }
        }
        work.timed()?;
        let held = locks(&self.journal.root, &parts, false)?;
        work.timed()?;
        self.journal.check_guarded_marker()?;
        let permit = AtomicPermit {
            authority: self.identity().clone(),
            requests: Digest::blob(&super::encode(&requests)?),
            _locks: held,
        };
        self.journal
            .apply_atomic_admitted(&requests, &permit, hook)?
            .pop()
            .ok_or(super::Error::Corrupt)
            .map_err(Into::into)
    }
    fn apply_hook(
        &mut self,
        operation: Key,
        command: Command,
        nodes: &impl NodeStore,
        limits: &Limits,
        hook: impl FnMut(Stage) -> super::Result<()>,
    ) -> Result<Receipt> {
        self.apply_if_hook(operation, command, &[], nodes, limits, hook)
    }
    fn apply_if_hook(
        &mut self,
        operation: Key,
        command: Command,
        conditions: &[SourceCondition],
        nodes: &impl NodeStore,
        limits: &Limits,
        hook: impl FnMut(Stage) -> super::Result<()>,
    ) -> Result<Receipt> {
        if conditions.len() > 8
            || conditions.iter().any(|c| c.revision == 0)
            || conditions
                .windows(2)
                .any(|pair| pair[0].source.as_str() >= pair[1].source.as_str())
        {
            return Err(super::Error::Conflict.into());
        }
        let start = Instant::now();
        limits.validate().map_err(|_| Error::Budget)?;
        self.journal.check_guarded_marker()?;
        self.journal.sync()?;
        let preview = SourceJournal::preview_mode(
            &self.journal.root,
            &self.journal.checkpoint(),
            operation.clone(),
            command.clone(),
            true,
        )?;
        let (before, after) = match preview.disposition {
            PreviewDisposition::Recorded { receipt } => return Ok(receipt),
            PreviewDisposition::Proposed { before, after } => (before, after),
        };
        if conditions.iter().any(|c| {
            self.journal
                .state
                .sources
                .get(c.source.as_str())
                .is_none_or(|s| s.revision != c.revision)
        }) {
            return Err(super::Error::Stale.into());
        }
        let mut work = Work::new(nodes, limits, start)?;
        let policy = self.policy();
        let before = match &before {
            Some(source) => work.source(source, policy)?,
            None => BTreeMap::new(),
        };
        let after = work.source(&after, policy)?;
        let mut parts = BTreeSet::new();
        for id in before.keys().chain(after.keys()) {
            if before.get(id) != after.get(id) {
                parts.insert(partition(id)?);
            }
        }
        work.timed()?;
        let held = locks(&self.journal.root, &parts, false)?;
        work.timed()?;
        self.journal.check_guarded_marker()?;
        let request = super::Request {
            operation: operation.clone(),
            command: command.clone(),
        };
        let permit = Permit {
            authority: self.identity().clone(),
            request: Digest::blob(&super::encode(&request)?),
            conditions: conditions.to_vec(),
            _locks: held,
        };
        Ok(self
            .journal
            .apply_admitted(operation, command, &permit, hook)?)
    }
    /// Hold an existing namespace partition shared through read/publication/adoption.
    /// Process exit releases only this lock, never durable claims or payload bytes.
    pub fn hold(&self, object: &Object) -> Result<SubjectUse> {
        self.journal.check_guarded_marker()?;
        let held = locks(
            &self.journal.root,
            &[partition(&object.sha256)?].into_iter().collect(),
            false,
        )?;
        self.journal.check_guarded_marker()?;
        Ok(SubjectUse {
            authority: self.identity().clone(),
            object: object.clone(),
            _locks: held,
        })
    }
    /// Mutual exclusion only. This neither initializes a byte store nor deletes.
    pub fn fence(&self, object: &Object) -> Result<SubjectFence> {
        self.journal.check_guarded_marker()?;
        let held = locks(
            &self.journal.root,
            &[partition(&object.sha256)?].into_iter().collect(),
            true,
        )?;
        self.journal.check_guarded_marker()?;
        Ok(SubjectFence {
            authority: self.identity().clone(),
            object: object.clone(),
            _locks: held,
        })
    }
    /// No global head recheck: mandatory mutation admission keeps this subject's
    /// predicate stable under the held exclusive fence while other heads advance.
    pub fn absence<'a>(
        &mut self,
        fence: &'a SubjectFence,
        place: &Key,
        nodes: &impl NodeStore,
        limits: &Limits,
    ) -> Result<Option<ScopedAbsence<'a>>> {
        let mut work = Work::new(nodes, limits, Instant::now())?;
        let Some(checkpoint) = self.absent_places(fence, std::slice::from_ref(place), &mut work)?
        else {
            return Ok(None);
        };
        Ok(Some(ScopedAbsence {
            fence,
            place: place.clone(),
            checkpoint,
        }))
    }
    /// Used by store-wide assessment to share one aggregate budget and avoid
    /// traversing identical namespace roots separately for each place alias.
    pub(crate) fn absent_places(
        &mut self,
        fence: &SubjectFence,
        places: &[Key],
        work: &mut Work<'_, impl NodeStore>,
    ) -> Result<Option<Checkpoint>> {
        self.journal.check_guarded_marker()?;
        if fence.authority != *self.identity() {
            return Err(Error::WrongAuthority);
        }
        if places.is_empty()
            || places.len() > 16
            || !places.iter().all(|place| {
                self.policy().classes.values().any(|p| {
                    p.current
                        .iter()
                        .chain(&p.pending)
                        .chain(&p.predecessor)
                        .any(|k| k == place)
                })
            })
        {
            return Err(Error::InvalidPolicy);
        }
        self.journal.sync()?;
        let state = self.journal.state.clone();
        let policy = self.policy();
        let mut subject = Demand {
            bytes: fence.object.bytes,
            ..Demand::default()
        };
        for source in state.sources.values() {
            let demands = work.source(source, policy)?;
            if let Some(demand) = demands.get(&fence.object.sha256) {
                subject.extend(demand)?;
            }
        }
        self.journal.check_guarded_marker()?;
        work.timed()?;
        if places
            .iter()
            .any(|place| subject.required(policy, place.as_str()))
        {
            return Ok(None);
        }
        Ok(Some(state.head))
    }

    /// Content membership, independent of assigned place or active precedence.
    /// Used only under this exact object's live fence; never removal authority.
    pub(crate) fn absent_object(
        &mut self,
        fence: &SubjectFence,
        work: &mut Work<'_, impl NodeStore>,
    ) -> Result<Option<Checkpoint>> {
        self.journal.check_guarded_marker()?;
        if fence.authority != *self.identity() {
            return Err(Error::WrongAuthority);
        }
        self.journal.sync()?;
        let state = self.journal.state.clone();
        for source in state.sources.values() {
            let demands = work.source(source, self.policy())?;
            if let Some(demand) = demands.get(&fence.object.sha256) {
                if demand.bytes != fence.object.bytes {
                    return Err(Error::ConflictingSize);
                }
                return Ok(None);
            }
        }
        self.journal.check_guarded_marker()?;
        work.timed()?;
        Ok(Some(state.head))
    }
}
#[cfg(test)]
mod tests;
