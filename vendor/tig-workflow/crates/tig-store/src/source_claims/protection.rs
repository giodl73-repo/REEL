//! Native local controller index over acknowledged immutable metadata.
//! Enrollment/source authenticity and the trusted restore minimum are admitted.
use super::{Checkpoint, Command, Key, Receipt, Reference, Selection, SourceJournal};
use crate::{local, metadata, retirement_intent::ObservedIntent};
use fs2::FileExt;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    cell::Cell,
    collections::BTreeMap,
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};
use tig_core::{
    Digest, LogicalPath, Object, identity,
    tree::{NodeStore, Tree, TreeError, TreeLimits},
};

const SCHEMA: &str = "tig.protection-index.design-1";
const ROUTED_HEADER: &str = "tig.protection-index.design-2";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Routing {
    pub source: Key,
    pub actor: Key,
}
impl Routing {
    pub(super) fn validate(&self, enrollment: &Enrollment) -> Result<()> {
        enrollment.actor(&self.actor)?;
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum UpdateMode {
    Select,
    Prepare,
    CoordinatedPrepare,
}
fn is_false(value: &bool) -> bool {
    !*value
}
pub mod batch;
pub mod effects;
pub mod work;
pub const MAX_CHANGES: usize = 128;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    pub id: Key,
    pub owner: Key,
    pub stores: Vec<Digest>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub owner: Key,
    pub subject: Digest,
    pub policies: Vec<Digest>,
    pub stores: Vec<Digest>,
    pub actors: Vec<Actor>,
    pub max_bindings: u64,
}
impl Enrollment {
    pub fn validate(&self) -> Result<()> {
        if self.max_bindings == 0
            || self.max_bindings > 500_000
            || self.actors.is_empty()
            || self.actors.len() > 16
            || !sorted(&self.stores)
            || self.stores.len() > 16
            || !sorted(&self.policies)
            || self.policies.len() > 16
            || self
                .actors
                .windows(2)
                .any(|w| w[0].id.as_str() >= w[1].id.as_str())
        {
            return Err(Error::Invalid);
        }
        for actor in &self.actors {
            if !sorted(&actor.stores) || actor.stores.iter().any(|s| !self.stores.contains(s)) {
                return Err(Error::Invalid);
            }
        }
        Ok(())
    }
    fn actor(&self, id: &Key) -> Result<&Actor> {
        self.actors
            .iter()
            .find(|a| a.id == *id)
            .ok_or(Error::Invalid)
    }
}
fn sorted<T: Ord>(v: &[T]) -> bool {
    !v.is_empty() && v.windows(2).all(|w| w[0] < w[1])
}

#[derive(Debug)]
pub enum Error {
    Journal(super::Error),
    Metadata(metadata::Error),
    Tree(TreeError),
    Store(crate::Error),
    Invalid,
    Conflict,
    Stale,
    Pending,
    Unknown,
    Protected,
    Budget,
    Injected,
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
impl From<metadata::Error> for Error {
    fn from(e: metadata::Error) -> Self {
        Self::Metadata(e)
    }
}
impl From<TreeError> for Error {
    fn from(e: TreeError) -> Self {
        Self::Tree(e)
    }
}
impl From<crate::Error> for Error {
    fn from(e: crate::Error) -> Self {
        Self::Store(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorState {
    pub selection: Selection,
    pub revision: u64,
    pub ready: bool,
    pub bindings: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: String,
    journal: Digest,
    operation: Key,
    request: Digest,
    base_revision: u64,
    objects: Object,
    actors: BTreeMap<String, ActorState>,
    affected: Vec<Object>,
    #[serde(default, skip_serializing_if = "is_false")]
    coordinated: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Counts {
    schema: String,
    object: Object,
    actors: BTreeMap<String, u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub operation: Key,
    pub actor: Key,
    pub owner: Key,
    pub expected_revision: u64,
    pub selection: Selection,
    pub changes: BTreeMap<LogicalPath, Option<Object>>,
    pub ready: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    MetadataReady,
    Prepared,
    Adopted,
}
#[derive(Debug)]
pub struct Coverage {
    pub checkpoint: Checkpoint,
    pub protected_by: Vec<Key>,
    pub metadata_reads: usize,
    pub metadata_bytes: usize,
}
/// One verified control snapshot, not complete consumer coverage or a lease.
pub struct Observation {
    pub index: Digest,
    pub checkpoint: Checkpoint,
    pub enrollment: Enrollment,
    pub actors: BTreeMap<String, ActorState>,
    pub pending: bool,
    pub cancelled: bool,
    pub effect_fences: bool,
}
pub struct Index {
    root: PathBuf,
    journal: SourceJournal,
    enrollment: Enrollment,
}
impl Index {
    /// New explicit provider-neutral effect profile. Existing enrollments are
    /// never upgraded implicitly. A routed profile retains original source and
    /// outcome-admission requirements.
    pub fn create_effects(
        root: &Path,
        epoch: Key,
        enrollment: Enrollment,
        routing: Option<Routing>,
    ) -> Result<Self> {
        enrollment.validate()?;
        if let Some(route) = &routing {
            route.validate(&enrollment)?;
        }
        let routed = routing.is_some();
        let journal = SourceJournal::create_profiles(
            root,
            key("protection-index"),
            epoch,
            super::Profiles {
                authority: Some(enrollment.clone()),
                routing,
                bound_source: routed,
                outcome_room: routed,
                effect_fences: true,
                ..super::Profiles::default()
            },
        )?;
        Ok(Self {
            root: std::fs::canonicalize(root).map_err(crate::Error::from)?,
            journal,
            enrollment,
        })
    }
    /// Immutable admitted enrollment; this is not owner authentication.
    pub fn enrollment_profile(&self) -> &Enrollment {
        &self.enrollment
    }
    pub fn create(root: &Path, epoch: Key, enrollment: Enrollment) -> Result<Self> {
        enrollment.validate()?;
        let journal = SourceJournal::create_mode(
            root,
            key("protection-index"),
            epoch,
            Some(enrollment.clone()),
        )?;
        Ok(Self {
            root: std::fs::canonicalize(root).map_err(crate::Error::from)?,
            journal,
            enrollment,
        })
    }
    /// Fresh explicit profile: this actor may only advance through linked adoption.
    pub fn create_routed(
        root: &Path,
        epoch: Key,
        enrollment: Enrollment,
        routing: Routing,
    ) -> Result<Self> {
        Self::create_routing(root, epoch, enrollment, routing, false, false)
    }
    /// Fresh profile requiring completed enrollment of an exact source identity.
    pub fn create_bound(
        root: &Path,
        epoch: Key,
        enrollment: Enrollment,
        routing: Routing,
    ) -> Result<Self> {
        Self::create_routing(root, epoch, enrollment, routing, true, false)
    }
    pub fn create_recoverable(
        root: &Path,
        epoch: Key,
        enrollment: Enrollment,
        routing: Routing,
    ) -> Result<Self> {
        Self::create_routing(root, epoch, enrollment, routing, true, true)
    }
    fn create_routing(
        root: &Path,
        epoch: Key,
        enrollment: Enrollment,
        routing: Routing,
        bound_source: bool,
        outcome_room: bool,
    ) -> Result<Self> {
        enrollment.validate()?;
        routing.validate(&enrollment)?;
        let journal = SourceJournal::create_profiles(
            root,
            key("protection-index"),
            epoch,
            super::Profiles {
                authority: Some(enrollment.clone()),
                routing: Some(routing),
                bound_source,
                outcome_room,
                ..super::Profiles::default()
            },
        )?;
        Ok(Self {
            root: std::fs::canonicalize(root).map_err(crate::Error::from)?,
            journal,
            enrollment,
        })
    }
    pub fn requires_outcome_room(&self) -> bool {
        self.journal.outcome_room
    }
    pub fn requires_bound_source(&self) -> bool {
        self.journal.bound_source
    }
    pub fn routing(&self) -> Option<&Routing> {
        self.journal.routing.as_ref()
    }
    pub fn open(root: &Path, minimum: &Checkpoint) -> Result<Self> {
        let journal = SourceJournal::open(root, minimum)?;
        let enrollment = journal.authority.clone().ok_or(Error::Invalid)?;
        Ok(Self {
            root: std::fs::canonicalize(root).map_err(crate::Error::from)?,
            journal,
            enrollment,
        })
    }
    /// Read-only journal connection and immutable metadata lookup. Actor states
    /// come from one header at the returned checkpoint, never separate queries.
    /// Pending/cancelled state must not be interpreted as readiness to cut over.
    pub fn observe(
        root: &Path,
        minimum: &Checkpoint,
        nodes: &impl NodeStore,
    ) -> Result<Observation> {
        let journal = SourceJournal::open_mode(root, minimum, true)?;
        let enrollment = journal.authority.clone().ok_or(Error::Invalid)?;
        let mut index = Self {
            root: std::fs::canonicalize(root).map_err(crate::Error::from)?,
            journal,
            enrollment,
        };
        if index.requires_bound_source()
            && work::adoption::enrollment::selected(&mut index)?.is_none()
        {
            return Err(Error::Unknown);
        }
        let checked = index.checkpoint();
        let source = index.source()?.ok_or(Error::Unknown)?;
        if index.checkpoint() != checked {
            return Err(Error::Unknown);
        }
        let selected = source.current.ok_or(Error::Unknown)?;
        let header = index.header(nodes, &selected.selection.root)?;
        if let Some(pending) = &source.pending {
            index.header(nodes, &pending.selection.root)?;
        }
        Ok(Observation {
            index: index.identity().clone(),
            checkpoint: index.checkpoint(),
            enrollment: index.enrollment,
            actors: header.actors,
            pending: source.pending.is_some(),
            cancelled: source.cancelled,
            effect_fences: index.journal.effect_fences,
        })
    }
    pub fn checkpoint(&self) -> Checkpoint {
        self.journal.checkpoint()
    }
    pub fn identity(&self) -> &Digest {
        self.journal.identity()
    }
    /// Initial explicit enrollment may traverse the declared roots once. No
    /// authority lease can exist until this first complete header is admitted.
    pub fn bootstrap(
        &mut self,
        selections: BTreeMap<String, Selection>,
        nodes: &mut metadata::MetadataStore,
    ) -> Result<Receipt> {
        self.bootstrap_inner(selections, nodes, false, |_| Ok(()))
    }
    /// Explicit exact initialization retry. Original command IDs/bodies fence
    /// changed inputs; no existing authority is reset or recreated.
    pub fn bootstrap_recoverable(
        &mut self,
        selections: BTreeMap<String, Selection>,
        nodes: &mut metadata::MetadataStore,
    ) -> Result<Receipt> {
        self.bootstrap_inner(selections, nodes, true, |_| Ok(()))
    }
    fn bootstrap_inner(
        &mut self,
        selections: BTreeMap<String, Selection>,
        nodes: &mut metadata::MetadataStore,
        recover: bool,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        let _initialization = self.lock_name("initialization")?;
        if self.source()?.is_some() && !recover {
            return Err(Error::Conflict);
        }
        if selections.len() != self.enrollment.actors.len() {
            return Err(Error::Unknown);
        }
        let mut work = Work::new(nodes);
        let mut counts = BTreeMap::<Digest, Counts>::new();
        let mut actors = BTreeMap::new();
        let mut total = 0u64;
        for actor in &self.enrollment.actors {
            let selection = selections.get(actor.id.as_str()).ok_or(Error::Unknown)?;
            let (entries, _) =
                Tree::from_root(selection.root.clone()).entries(&mut work, limits())?;
            total = total
                .checked_add(entries.len() as u64)
                .ok_or(Error::Budget)?;
            if total > self.enrollment.max_bindings {
                return Err(Error::Budget);
            }
            for object in entries.values() {
                let entry = counts
                    .entry(object.sha256.clone())
                    .or_insert_with(|| Counts {
                        schema: SCHEMA.into(),
                        object: object.clone(),
                        actors: BTreeMap::new(),
                    });
                if entry.object != *object {
                    return Err(Error::Invalid);
                }
                *entry.actors.entry(actor.id.as_str().into()).or_default() += 1;
            }
            actors.insert(
                actor.id.as_str().into(),
                ActorState {
                    selection: selection.clone(),
                    revision: 1,
                    ready: true,
                    bindings: entries.len() as u64,
                },
            );
        }
        let mut bindings = BTreeMap::new();
        for entry in counts.values() {
            bindings.insert(object_path(&entry.object)?, put(&mut work, entry)?);
        }
        let (objects, _) = Tree::build(&bindings, &mut work, limits())?;
        let header = Header {
            schema: SCHEMA.into(),
            journal: self.identity().clone(),
            operation: key("bootstrap"),
            request: Digest::blob(&encode(&selections)?),
            base_revision: 0,
            objects: objects.root().clone(),
            actors,
            affected: Vec::new(),
            coordinated: false,
        };
        let root = put(&mut work, &header)?;
        nodes.finish()?;
        self.commit(root, &header, hook, true)
    }
    pub fn actor(&mut self, nodes: &mut impl NodeStore, actor: &Key) -> Result<ActorState> {
        self.enrollment.actor(actor)?;
        let source = self.source()?.ok_or(Error::Unknown)?;
        let current = source.current.ok_or(Error::Unknown)?;
        Ok(self
            .header(nodes, &current.selection.root)?
            .actors
            .get(actor.as_str())
            .ok_or(Error::Invalid)?
            .clone())
    }
    pub fn update(
        &mut self,
        request: &Update,
        nodes: &mut metadata::MetadataStore,
    ) -> Result<Receipt> {
        self.update_hook(request, nodes, |_| Ok(()))
    }
    pub fn update_hook(
        &mut self,
        request: &Update,
        nodes: &mut metadata::MetadataStore,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        let actor = self.enrollment.actor(&request.actor)?;
        if actor.owner != request.owner {
            return Err(Error::Invalid);
        }
        if request.changes.len() > MAX_CHANGES {
            return Err(Error::Budget);
        }
        let digest = Digest::blob(&encode(request)?);
        self.update_many(
            &request.operation,
            std::slice::from_ref(request),
            digest,
            nodes,
            hook,
            UpdateMode::Select,
        )
    }
    fn update_many(
        &mut self,
        operation: &Key,
        requests: &[Update],
        digest: Digest,
        nodes: &mut metadata::MetadataStore,
        mut hook: impl FnMut(Stage) -> Result<()>,
        mode: UpdateMode,
    ) -> Result<Receipt> {
        let finish = mode == UpdateMode::Select;
        let routed = self
            .routing()
            .is_some_and(|r| requests.iter().any(|u| u.actor == r.actor));
        if routed && mode != UpdateMode::CoordinatedPrepare {
            return Err(Error::Pending);
        }
        if finish && work::is_linked(self, nodes, operation)? {
            return Err(Error::Pending);
        }
        if let Some(header) = self.prepared(nodes, operation)? {
            if header.request != digest {
                return Err(Error::Conflict);
            }
            return if finish {
                self.resume_header(nodes, &header, &mut hook)
            } else {
                self.prepare_header(Object::from_bytes(&encode(&header)?), &header)
            };
        }
        // Reconcile old control references before creating another pending head.
        // Trimming must never advance the revision of somebody else's Prepare.
        if self.source()?.is_some() {
            self.trim()?;
        }
        let source = self.source()?.ok_or(Error::Unknown)?;
        if source.pending.is_some() {
            return Err(Error::Pending);
        }
        let current = source.current.as_ref().ok_or(Error::Unknown)?;
        let mut work = Work::new(nodes);
        let mut header = self.header(&work, &current.selection.root)?;
        // Only coverage invalidation excludes all negative proofs in this
        // enrollment. Ordinary updates and independent removals share the epoch.
        let _epoch = self.lock_mode("coverage", requests.iter().any(|r| !r.ready))?;
        if requests.iter().any(|r| !r.ready) {
            self.require_clear_effect_coverage()?;
        }
        let mut delta = BTreeMap::<Object, BTreeMap<String, i64>>::new();
        let mut next_actors = BTreeMap::new();
        for request in requests {
            let state = header
                .actors
                .get(request.actor.as_str())
                .ok_or(Error::Invalid)?
                .clone();
            if state.revision != request.expected_revision {
                return Err(Error::Stale);
            }
            let old = Tree::from_root(state.selection.root.clone());
            let mut binding_count = state.bindings;
            for (path, next) in &request.changes {
                let (before, _) = old.get(path, &mut work, limits())?;
                if before == *next {
                    continue;
                }
                if let Some(object) = before {
                    *delta
                        .entry(object)
                        .or_default()
                        .entry(request.actor.as_str().into())
                        .or_default() -= 1;
                    binding_count = binding_count.checked_sub(1).ok_or(Error::Invalid)?;
                }
                if let Some(object) = next {
                    *delta
                        .entry(object.clone())
                        .or_default()
                        .entry(request.actor.as_str().into())
                        .or_default() += 1;
                    binding_count = binding_count.checked_add(1).ok_or(Error::Budget)?;
                }
            }
            let (computed, _) = old.edit(old.root(), &request.changes, &mut work, limits())?;
            if computed.root() != &request.selection.root {
                return Err(Error::Invalid);
            }
            next_actors.insert(
                request.actor.as_str().into(),
                ActorState {
                    selection: request.selection.clone(),
                    revision: state.revision.checked_add(1).ok_or(Error::Budget)?,
                    ready: request.ready,
                    bindings: binding_count,
                },
            );
        }
        let mut affected: Vec<_> = delta.keys().cloned().collect();
        affected.sort();
        if affected.windows(2).any(|w| w[0].sha256 == w[1].sha256) {
            return Err(Error::Invalid);
        }
        let _fences = self.lock_objects(&affected)?;
        self.require_clear_effects(&affected)?;
        if self.source()?.as_ref().map(|s| s.revision) != Some(source.revision) {
            return Err(Error::Stale);
        }
        let index = Tree::from_root(header.objects.clone());
        let mut changes = BTreeMap::new();
        for (object, actor_changes) in delta {
            let path = object_path(&object)?;
            let (record, _) = index.get(&path, &mut work, limits())?;
            let mut counts: Counts = match record {
                Some(record) => load(&work, &record)?,
                None => Counts {
                    schema: SCHEMA.into(),
                    object: object.clone(),
                    actors: BTreeMap::new(),
                },
            };
            self.check_counts(&counts, &header, &object)?;
            for (actor, change) in actor_changes {
                let previous = counts.actors.get(&actor).copied().unwrap_or(0);
                let next = if change >= 0 {
                    previous.checked_add(change as u64)
                } else {
                    previous.checked_sub(change.unsigned_abs())
                }
                .ok_or(Error::Invalid)?;
                if next == 0 {
                    counts.actors.remove(&actor);
                } else {
                    counts.actors.insert(actor, next);
                }
            }
            changes.insert(
                path,
                if counts.actors.is_empty() {
                    None
                } else {
                    Some(put(&mut work, &counts)?)
                },
            );
        }
        let (objects, _) = index.edit(index.root(), &changes, &mut work, limits())?;
        header.actors.extend(next_actors);
        if header
            .actors
            .values()
            .try_fold(0u64, |n, a| n.checked_add(a.bindings))
            .ok_or(Error::Budget)?
            > self.enrollment.max_bindings
        {
            return Err(Error::Budget);
        }
        header.coordinated = mode == UpdateMode::CoordinatedPrepare;
        header.schema = if header.coordinated {
            ROUTED_HEADER
        } else {
            SCHEMA
        }
        .into();
        header.objects = objects.root().clone();
        header.base_revision = source.revision;
        header.operation = operation.clone();
        header.request = digest;
        header.affected = affected;
        let root = put(&mut work, &header)?;
        if self.requires_outcome_room() && mode == UpdateMode::CoordinatedPrepare {
            work::adoption::headroom::reserve_index(self, operation, &source, nodes)?;
        }
        nodes.finish()?;
        self.header(nodes, &root)?;
        hook(Stage::MetadataReady)?;
        self.commit(root, &header, hook, finish)
    }
    pub fn resume(&mut self, operation: &Key, nodes: &mut impl NodeStore) -> Result<Receipt> {
        let header = self.prepared(nodes, operation)?.ok_or(Error::Unknown)?;
        self.resume_header(nodes, &header, &mut |_| Ok(()))
    }
    pub fn inspect(
        &mut self,
        nodes: &mut impl NodeStore,
        object: &Object,
        store: &Digest,
    ) -> Result<Coverage> {
        if self.requires_bound_source() && work::adoption::enrollment::selected(self)?.is_none() {
            return Err(Error::Unknown);
        }
        if !self.enrollment.stores.contains(store) {
            return Err(Error::Invalid);
        }
        let source = self.source()?.ok_or(Error::Unknown)?;
        let mut roots = Vec::new();
        if let Some(current) = source.current {
            roots.push(current.selection.root);
        } else {
            return Err(Error::Unknown);
        }
        if let Some(pending) = source.pending {
            roots.push(pending.selection.root);
        }
        let mut work = Work::new(nodes);
        let mut protected = BTreeMap::new();
        for root in roots {
            let header = self.header(&work, &root)?;
            if header.actors.values().any(|a| !a.ready) {
                return Err(Error::Unknown);
            }
            let (record, _) = Tree::from_root(header.objects.clone()).get(
                &object_path(object)?,
                &mut work,
                limits(),
            )?;
            if let Some(record) = record {
                let counts: Counts = load(&work, &record)?;
                self.check_counts(&counts, &header, object)?;
                for id in counts.actors.keys() {
                    let actor = self
                        .enrollment
                        .actors
                        .iter()
                        .find(|a| a.id.as_str() == id)
                        .ok_or(Error::Invalid)?;
                    if actor.stores.contains(store) {
                        protected.insert(id.clone(), actor.id.clone());
                    }
                }
            }
        }
        Ok(Coverage {
            checkpoint: self.checkpoint(),
            protected_by: protected.into_values().collect(),
            metadata_reads: work.reads.get(),
            metadata_bytes: work.bytes.get(),
        })
    }
    pub fn protection(
        &self,
        metadata_root: &Path,
        minimum: &metadata::Checkpoint,
        intent: &ObservedIntent,
        plan: &local::removal::Plan,
    ) -> Result<NativeProtection> {
        let scope = plan.scope();
        if self.enrollment.owner != *intent.owner()
            || scope.owner != self.enrollment.owner
            || intent.review().owner() != &self.enrollment.subject
            || !self
                .enrollment
                .policies
                .contains(intent.review().policy_identity())
            || !self.enrollment.stores.contains(&scope.store)
            || scope.decision != intent.decision().digest().map_err(|_| Error::Invalid)?
            || scope.operation != intent.decision().approval().operation
        {
            return Err(Error::Invalid);
        }
        Ok(NativeProtection {
            root: self.root.clone(),
            minimum: self.checkpoint(),
            metadata_root: metadata_root.into(),
            metadata_minimum: minimum.clone(),
            scope: scope.clone(),
        })
    }
    fn source(&mut self) -> Result<Option<super::Source>> {
        Ok(self.journal.source(&key("authority"))?)
    }
    fn header(&self, nodes: &impl NodeStore, object: &Object) -> Result<Header> {
        let header: Header = load(nodes, object)?;
        if !matches!(
            (header.schema.as_str(), header.coordinated),
            (SCHEMA, false) | (ROUTED_HEADER, true)
        ) || header.journal != *self.identity()
            || header.actors.len() != self.enrollment.actors.len()
            || header.affected.len() > MAX_CHANGES * 2
            || self
                .enrollment
                .actors
                .iter()
                .any(|a| !header.actors.contains_key(a.id.as_str()))
            || header
                .affected
                .windows(2)
                .any(|w| w[0] >= w[1] || w[0].sha256 == w[1].sha256)
        {
            return Err(Error::Invalid);
        }
        Ok(header)
    }
    fn check_counts(&self, c: &Counts, h: &Header, object: &Object) -> Result<()> {
        if c.schema != SCHEMA
            || c.object != *object
            || c.actors
                .iter()
                .any(|(id, n)| *n == 0 || h.actors.get(id).is_none_or(|a| *n > a.bindings))
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    fn command(&mut self, operation: Key, command: Command) -> Result<Receipt> {
        let result = self
            .journal
            .apply_once(operation, command, |_| Ok(()), true);
        self.journal.mark_uncertain(&result);
        Ok(result?)
    }
    fn commit(
        &mut self,
        root: Object,
        header: &Header,
        mut hook: impl FnMut(Stage) -> Result<()>,
        finish: bool,
    ) -> Result<Receipt> {
        let prepared = self.prepare_header(root, header)?;
        hook(Stage::Prepared)?;
        if !finish {
            return Ok(prepared);
        }
        let receipt = self.command(
            op(self.identity(), &header.operation, "adopt")?,
            Command::Adopt {
                source: key("authority"),
                owner: self.enrollment.owner.clone(),
                expected_revision: header.base_revision + 1,
                prepared: prepared.reference,
            },
        )?;
        hook(Stage::Adopted)?;
        self.trim()?;
        Ok(receipt)
    }
    fn prepare_header(&mut self, root: Object, header: &Header) -> Result<Receipt> {
        self.command(
            op(self.identity(), &header.operation, "prepare")?,
            Command::Prepare {
                source: key("authority"),
                owner: self.enrollment.owner.clone(),
                expected_revision: header.base_revision,
                selection: Selection {
                    context: Digest::blob(&encode(&(SCHEMA, &root))?),
                    root,
                },
            },
        )
    }
    fn prepared(&mut self, nodes: &impl NodeStore, operation: &Key) -> Result<Option<Header>> {
        self.journal.sync()?;
        let id = op(self.identity(), operation, "prepare")?;
        use rusqlite::OptionalExtension;
        let body: Option<Vec<u8>> = self
            .journal
            .conn
            .query_row(
                "SELECT body FROM events WHERE operation=?1",
                [id.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(super::Error::from)?;
        let Some(body) = body else {
            return Ok(None);
        };
        let request: super::Request = super::decode(&body)?;
        let Command::Prepare { selection, .. } = request.command else {
            return Err(Error::Invalid);
        };
        let header = self.header(nodes, &selection.root)?;
        if header.operation != *operation {
            return Err(Error::Conflict);
        }
        Ok(Some(header))
    }
    fn resume_header(
        &mut self,
        nodes: &mut impl NodeStore,
        header: &Header,
        hook: &mut impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        if header.coordinated || work::is_linked(self, nodes, &header.operation)? {
            return Err(Error::Pending);
        }
        self.resume_header_authorized(nodes, header, hook)
    }
    fn resume_header_authorized(
        &mut self,
        nodes: &mut impl NodeStore,
        header: &Header,
        hook: &mut impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        let command = Command::Adopt {
            source: key("authority"),
            owner: self.enrollment.owner.clone(),
            expected_revision: header.base_revision + 1,
            prepared: Reference {
                journal: self.identity().clone(),
                operation: op(self.identity(), &header.operation, "prepare")?,
            },
        };
        if let Some(receipt) = self
            .journal
            .replay(op(self.identity(), &header.operation, "adopt")?, command)?
        {
            self.trim()?;
            return Ok(receipt);
        }
        let _epoch = self.lock_mode("coverage", header.actors.values().any(|a| !a.ready))?;
        if header.actors.values().any(|a| !a.ready) {
            self.require_clear_effect_coverage()?;
        }
        let _fences = self.lock_objects(&header.affected)?;
        self.require_clear_effects(&header.affected)?;
        let source = self.source()?.ok_or(Error::Unknown)?;
        let pending = source.pending.ok_or(Error::Pending)?;
        if source.revision != header.base_revision + 1
            || pending.reference.operation != op(self.identity(), &header.operation, "prepare")?
        {
            return Err(Error::Stale);
        }
        self.header(nodes, &pending.selection.root)?;
        let receipt = self.command(
            op(self.identity(), &header.operation, "adopt")?,
            Command::Adopt {
                source: key("authority"),
                owner: self.enrollment.owner.clone(),
                expected_revision: source.revision,
                prepared: pending.reference,
            },
        )?;
        hook(Stage::Adopted)?;
        self.trim()?;
        Ok(receipt)
    }
    fn trim(&mut self) -> Result<()> {
        loop {
            let source = self.source()?.ok_or(Error::Unknown)?;
            if source.pending.is_some() {
                return Ok(());
            }
            let Some(old) = source.predecessors.first() else {
                return Ok(());
            };
            let id = key(&format!("trim-{}-{}", source.revision, old.revision));
            self.command(
                id,
                Command::Retire {
                    source: key("authority"),
                    owner: self.enrollment.owner.clone(),
                    expected_revision: source.revision,
                    predecessor_revision: old.revision,
                },
            )?;
        }
    }
    fn lock_objects(&self, objects: &[Object]) -> Result<Vec<Fence>> {
        objects
            .iter()
            .map(|o| self.lock_name(o.sha256.as_str()))
            .collect()
    }
    fn lock_name(&self, name: &str) -> Result<Fence> {
        self.lock_mode(name, true)
    }
    fn lock_mode(&self, name: &str, exclusive: bool) -> Result<Fence> {
        local::directory(&self.root.join("fences"))?;
        let path = self.root.join("fences").join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => local::regular(&path)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::Store(e.into())),
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(crate::Error::from)?;
        local::regular(&path)?;
        if file.metadata().map_err(crate::Error::from)?.len() != 0 {
            return Err(Error::Invalid);
        }
        let result = if exclusive {
            FileExt::try_lock_exclusive(&file)
        } else {
            FileExt::try_lock_shared(&file)
        };
        result.map_err(|e| {
            if e.raw_os_error() == fs2::lock_contended_error().raw_os_error() {
                Error::Store(crate::Error::Busy)
            } else {
                Error::Store(e.into())
            }
        })?;
        Ok(Fence(file))
    }
}

pub struct NativeProtection {
    root: PathBuf,
    minimum: Checkpoint,
    metadata_root: PathBuf,
    metadata_minimum: metadata::Checkpoint,
    scope: local::removal::Scope,
}
pub struct NativeLease {
    protection: NativeProtection,
    _fence: Fence,
    _epoch: Fence,
}
impl local::removal::Protection for NativeProtection {
    type Lease = NativeLease;
    fn acquire(&mut self, scope: &local::removal::Scope) -> crate::Result<NativeLease> {
        if scope != &self.scope {
            return Err(crate::Error::OperationConflict);
        }
        let index = Index::open(&self.root, &self.minimum).map_err(port_error)?;
        let epoch = index.lock_mode("coverage", false).map_err(port_error)?;
        let fence = index
            .lock_name(scope.object.sha256.as_str())
            .map_err(port_error)?;
        let mut lease = NativeLease {
            protection: NativeProtection {
                root: self.root.clone(),
                minimum: self.minimum.clone(),
                metadata_root: self.metadata_root.clone(),
                metadata_minimum: self.metadata_minimum.clone(),
                scope: self.scope.clone(),
            },
            _fence: fence,
            _epoch: epoch,
        };
        local::removal::ProtectionLease::revalidate(&mut lease, scope)?;
        Ok(lease)
    }
}
impl local::removal::ProtectionLease for NativeLease {
    fn revalidate(&mut self, scope: &local::removal::Scope) -> crate::Result<Digest> {
        if scope != &self.protection.scope {
            return Err(crate::Error::OperationConflict);
        }
        let mut index =
            Index::open(&self.protection.root, &self.protection.minimum).map_err(port_error)?;
        let mut nodes = metadata::MetadataReader::open(
            &self.protection.metadata_root,
            &self.protection.metadata_minimum,
        )
        .map_err(|_| crate::Error::Retained)?;
        let coverage = index
            .inspect(&mut nodes, &scope.object, &scope.store)
            .map_err(port_error)?;
        if !coverage.protected_by.is_empty() {
            return Err(crate::Error::Retained);
        }
        self.protection.minimum = coverage.checkpoint;
        Ok(Digest::blob(
            &encode(&(SCHEMA, &self.protection.minimum, scope)).map_err(port_error)?,
        ))
    }
}
fn port_error(e: Error) -> crate::Error {
    match e {
        Error::Store(crate::Error::Busy) => crate::Error::Busy,
        _ => crate::Error::Retained,
    }
}
struct Fence(File);
impl Drop for Fence {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}
fn key(s: &str) -> Key {
    Key::parse(s).expect("bounded index key")
}
fn op(journal: &Digest, operation: &Key, phase: &str) -> Result<Key> {
    Ok(key(&format!(
        "idx-{}",
        Digest::blob(&encode(&(journal, operation, phase))?).as_str()
    )))
}
fn object_path(object: &Object) -> Result<LogicalPath> {
    LogicalPath::parse(&format!(
        "objects/{}/{}",
        &object.sha256.as_str()[..2],
        object.sha256.as_str()
    ))
    .map_err(|_| Error::Invalid)
}
fn encode(v: &impl Serialize) -> Result<Vec<u8>> {
    identity::canonical_json(&serde_json::to_value(v).map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)
}
fn load<T: DeserializeOwned + Serialize>(nodes: &impl NodeStore, object: &Object) -> Result<T> {
    if object.bytes > 65536 {
        return Err(Error::Budget);
    }
    let bytes = nodes.get(object)?.ok_or(Error::Unknown)?;
    if Object::from_bytes(&bytes) != *object {
        return Err(Error::Invalid);
    }
    let value = identity::parse_json(&bytes).map_err(|_| Error::Invalid)?;
    let record: T = serde_json::from_value(value).map_err(|_| Error::Invalid)?;
    if encode(&record)? != bytes {
        return Err(Error::Invalid);
    }
    Ok(record)
}
fn put(nodes: &mut impl NodeStore, value: &impl Serialize) -> Result<Object> {
    let bytes = encode(value)?;
    if bytes.len() > 65536 {
        return Err(Error::Budget);
    }
    let object = nodes.put(&bytes)?;
    if Object::from_bytes(&bytes) != object {
        return Err(Error::Invalid);
    }
    Ok(object)
}
fn limits() -> TreeLimits {
    TreeLimits {
        max_entries: 500_000,
        max_reads: 2_000_000,
        max_read_bytes: 1024 * 1024 * 1024,
        max_writes: 2_000_000,
        max_write_bytes: 1024 * 1024 * 1024,
        max_map_depth: 64,
    }
}
struct Work<'a, S> {
    inner: &'a mut S,
    limits: TreeLimits,
    reads: Cell<usize>,
    bytes: Cell<usize>,
    writes: usize,
    write_bytes: usize,
}
impl<'a, S> Work<'a, S> {
    fn new(inner: &'a mut S) -> Self {
        Self::bounded(inner, limits())
    }
    fn bounded(inner: &'a mut S, limits: TreeLimits) -> Self {
        Self {
            inner,
            limits,
            reads: Cell::new(0),
            bytes: Cell::new(0),
            writes: 0,
            write_bytes: 0,
        }
    }
}
impl<S: NodeStore> NodeStore for Work<'_, S> {
    fn get(&self, object: &Object) -> std::result::Result<Option<Vec<u8>>, TreeError> {
        if self.reads.get() >= self.limits.max_reads
            || object.bytes > self.limits.max_read_bytes.saturating_sub(self.bytes.get()) as u64
        {
            return Err(TreeError::Budget);
        }
        self.reads.set(self.reads.get() + 1);
        self.bytes.set(self.bytes.get() + object.bytes as usize);
        self.inner.get(object)
    }
    fn put(&mut self, bytes: &[u8]) -> std::result::Result<Object, TreeError> {
        if self.writes >= self.limits.max_writes
            || bytes.len() > self.limits.max_write_bytes.saturating_sub(self.write_bytes)
        {
            return Err(TreeError::Budget);
        }
        self.writes += 1;
        self.write_bytes += bytes.len();
        self.inner.put(bytes)
    }
}

#[cfg(test)]
#[path = "protection/bootstrap_tests.rs"]
mod bootstrap_tests;
#[cfg(test)]
#[path = "protection_tests.rs"]
mod tests;
