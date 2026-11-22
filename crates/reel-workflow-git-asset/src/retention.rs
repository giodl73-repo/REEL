//! Exact original retained control sets; existing Namespace and native bytes.
//! Seeds/decoded data are not custody, owner authentication or executable jobs.
use crate::{claims::admission_error, error, native_error};
use git_asset::annex::managed;
use git_asset_core::{
    Digest, LogicalPath, Object,
    tree::{NodeStore, Tree, TreeLimits},
};
use git_asset_store::{
    Key,
    metadata::MetadataReader,
    protection::Limits,
    source_claims::{
        Checkpoint, Kind, Selection, Version,
        admission::{
            Namespace, Select,
            source_use::{Use, UseError},
        },
    },
};
use reel_workflow::{ErrorKind, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Read, time::Instant};
const SCHEMA: &str = "reel.workflow-public-retention-seed.v1";
const MAX_SEED: usize = 16 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Wire {
    schema: String,
    minimum: Checkpoint,
    source: Key,
    owner: Key,
    revision: u64,
    prepare: Key,
    adopt: Key,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seed {
    wire: Wire,
}
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub objects: usize,
    pub object_bytes: u64,
    pub total_bytes: u64,
    pub millis: u64,
}
impl Budget {
    fn check(&self) -> Result<()> {
        if self.objects == 0
            || self.objects > 64
            || self.object_bytes == 0
            || self.object_bytes > 1024 * 1024
            || self.total_bytes == 0
            || self.total_bytes > 64 * 1024 * 1024
            || self.millis == 0
            || self.millis > 300000
        {
            return Err(error(ErrorKind::Budget, "public-control-budget"));
        }
        Ok(())
    }
    fn timed(&self, start: Instant) -> Result<()> {
        if start.elapsed().as_millis() >= u128::from(self.millis) {
            Err(error(ErrorKind::Budget, "public-control-time"))
        } else {
            Ok(())
        }
    }
}
fn source_error(e: UseError) -> reel_workflow::Error {
    match e {
        UseError::Unsupported => error(ErrorKind::Unsupported, "public-control-format"),
        UseError::Admission(e) => admission_error(e),
        UseError::Owner => error(ErrorKind::Denied, "public-control-owner"),
        UseError::Stale | UseError::Inactive => {
            error(ErrorKind::Stale, "public-control-generation")
        }
    }
}
impl Seed {
    pub(crate) fn same_source(&self, other: &Self) -> bool {
        self.wire.minimum.identity() == other.wire.minimum.identity()
            && self.wire.source == other.wire.source
    }
    /// No enrollment, selection or byte IO. Caller owns initial source enrollment.
    pub fn prepare(
        ns: &mut Namespace,
        source: Key,
        owner: Key,
        prepare: Key,
        adopt: Key,
    ) -> Result<Self> {
        if !ns.supports_source_use() {
            return Err(error(ErrorKind::Unsupported, "public-control-format"));
        }
        let state = ns
            .source(&source)
            .map_err(admission_error)?
            .ok_or_else(|| error(ErrorKind::Missing, "public-control-source"))?;
        if state.kind != Some(Kind::Retention) || state.owner != owner {
            return Err(error(ErrorKind::Denied, "public-control-owner"));
        }
        if prepare == adopt
            || state.current.is_some()
            || state.pending.is_some()
            || !state.predecessors.is_empty()
        {
            return Err(error(ErrorKind::Stale, "public-control-empty-source"));
        }
        state
            .revision
            .checked_add(2)
            .ok_or_else(|| error(ErrorKind::Budget, "public-control-revision"))?;
        Ok(Self {
            wire: Wire {
                schema: SCHEMA.into(),
                minimum: ns.checkpoint(),
                source,
                owner,
                revision: state.revision,
                prepare,
                adopt,
            },
        })
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        let raw = serde_json::to_vec(&self.wire)
            .map_err(|_| error(ErrorKind::Invalid, "public-control-seed"))?;
        if raw.len() > MAX_SEED {
            return Err(error(ErrorKind::Budget, "public-control-seed"));
        }
        Ok(raw)
    }
    pub fn decode(raw: &[u8], expected: &Object) -> Result<Self> {
        if raw.len() > MAX_SEED {
            return Err(error(ErrorKind::Budget, "public-control-seed"));
        }
        if &Object::from_bytes(raw) != expected {
            return Err(error(ErrorKind::Integrity, "public-control-seed"));
        }
        let wire: Wire = serde_json::from_slice(raw)
            .map_err(|_| error(ErrorKind::Invalid, "public-control-seed"))?;
        if wire.schema != SCHEMA
            || wire.prepare == wire.adopt
            || wire.revision == 0
            || wire.revision.checked_add(2).is_none()
        {
            return Err(error(ErrorKind::Invalid, "public-control-seed"));
        }
        Ok(Self { wire })
    }
    /// Explicit metadata-tree construction only. No native byte publication.
    pub fn selection(
        &self,
        entries: &BTreeMap<LogicalPath, Object>,
        context: Digest,
        nodes: &mut impl NodeStore,
    ) -> Result<Select> {
        if entries.is_empty() || entries.len() > 256 {
            return Err(error(ErrorKind::Budget, "public-control-count"));
        }
        let (tree, _) = Tree::build(
            entries,
            nodes,
            TreeLimits {
                max_entries: 256,
                ..TreeLimits::default()
            },
        )
        .map_err(|_| error(ErrorKind::Invalid, "public-control-tree"))?;
        Ok(self.request(Selection {
            root: tree.root().clone(),
            context,
        }))
    }
    fn request(&self, selection: Selection) -> Select {
        Select {
            prepare: self.wire.prepare.clone(),
            adopt: self.wire.adopt.clone(),
            source: self.wire.source.clone(),
            owner: self.wire.owner.clone(),
            expected_revision: self.wire.revision,
            selection,
        }
    }
    fn namespace(&self, profile: &managed::Profile) -> Result<Namespace> {
        let member = profile
            .binding
            .members()
            .iter()
            .find(|m| &m.namespace == self.wire.minimum.identity())
            .ok_or_else(|| error(ErrorKind::Denied, "public-control-roster"))?;
        let location = profile
            .members
            .iter()
            .find(|m| m.minimum.identity() == &member.namespace)
            .ok_or_else(|| error(ErrorKind::Denied, "public-control-roster"))?;
        let mut ns = Namespace::open(&location.root, &location.minimum).map_err(admission_error)?;
        ns.observe(&self.wire.minimum).map_err(admission_error)?;
        if !ns.supports_source_use() {
            return Err(error(ErrorKind::Unsupported, "public-control-format"));
        }
        let state = ns
            .source(&self.wire.source)
            .map_err(admission_error)?
            .ok_or_else(|| error(ErrorKind::Missing, "public-control-source"))?;
        if state.kind != Some(Kind::Retention) || state.owner != self.wire.owner {
            return Err(error(ErrorKind::Denied, "public-control-owner"));
        }
        if !ns
            .policy()
            .classes
            .get(&Kind::Retention)
            .is_some_and(|p| p.current.iter().any(|p| member.places.contains(p)))
        {
            return Err(error(ErrorKind::Denied, "public-control-pool-not-required"));
        }
        Ok(ns)
    }
    /// Admit the original selection only while all its verified bytes are held.
    /// Retry uses identical IDs/selection; an old receipt never revives a source.
    pub fn admit(
        &self,
        profile: &managed::Profile,
        request: &Select,
        nodes: &mut MetadataReader,
        limits: &Limits,
        budget: Budget,
    ) -> Result<Held> {
        self.check_request(request)?;
        budget.check()?;
        let start = Instant::now();
        let mut ns = self.namespace(profile)?;
        let mut held = read_set(profile, &request.selection, nodes, limits, budget, start)?;
        budget.timed(start)?;
        // Tree verification and the subsequent admission share the caller's limits.
        let mut remaining = limits.clone();
        remaining.roots = remaining.roots.saturating_sub(1);
        remaining.bindings = remaining.bindings.saturating_sub(held.entries.len());
        remaining.reads = remaining.reads.saturating_sub(held.reads_used);
        remaining.read_bytes = remaining.read_bytes.saturating_sub(held.read_bytes_used);
        remaining.millis = remaining
            .millis
            .saturating_sub(u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX));
        ns.select(request, nodes, &remaining)
            .map_err(admission_error)?;
        held.generation = Some(self.use_original(&mut ns, request)?);
        if budget.timed(start).is_err() {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "public-control-time-after-admission",
            ));
        }
        Ok(held)
    }
    fn check_request(&self, request: &Select) -> Result<()> {
        if self.request(request.selection.clone()) != *request {
            Err(error(ErrorKind::Conflict, "public-control-request"))
        } else {
            Ok(())
        }
    }
    fn use_original(&self, ns: &mut Namespace, request: &Select) -> Result<Use> {
        let revision = self.wire.revision + 2;
        let version = Version {
            revision,
            selection: request.selection.clone(),
        };
        let held = ns
            .source_use(&self.wire.source, &self.wire.owner, revision, &version)
            .map_err(source_error)?;
        if ns
            .selection_status(request)
            .map_err(admission_error)?
            .is_none()
        {
            return Err(error(ErrorKind::Stale, "public-control-original-receipt"));
        }
        Ok(held)
    }
    /// Exact independently pinned selection, no source mutation or payload transfer.
    pub fn hold(
        &self,
        profile: &managed::Profile,
        request: &Select,
        nodes: &mut MetadataReader,
        limits: &Limits,
        budget: Budget,
    ) -> Result<Held> {
        self.check_request(request)?;
        budget.check()?;
        let start = Instant::now();
        let mut ns = self.namespace(profile)?;
        let generation = self.use_original(&mut ns, request)?;
        let mut held = read_set(profile, &request.selection, nodes, limits, budget, start)?;
        held.generation = Some(generation);
        budget.timed(start)?;
        Ok(held)
    }
    /// Discover only this seed's ORIGINAL fixed adopted revision and context.
    /// A newer source, pending transition or historical receipt is never latest.
    pub fn original(
        &self,
        profile: &managed::Profile,
        context: &Digest,
        nodes: &mut MetadataReader,
        limits: &Limits,
        budget: Budget,
    ) -> Result<Held> {
        budget.check()?;
        let start = Instant::now();
        let mut ns = self.namespace(profile)?;
        let state = ns
            .source(&self.wire.source)
            .map_err(admission_error)?
            .ok_or_else(|| error(ErrorKind::Missing, "public-control-source"))?;
        let revision = self.wire.revision + 2;
        let version = state
            .current
            .filter(|v| v.revision == revision && &v.selection.context == context)
            .ok_or_else(|| error(ErrorKind::Stale, "public-control-original-version"))?;
        let request = self.request(version.selection);
        let generation = self.use_original(&mut ns, &request)?;
        let mut held = read_set(profile, &request.selection, nodes, limits, budget, start)?;
        held.generation = Some(generation);
        budget.timed(start)?;
        Ok(held)
    }
}
/// Actual guards plus hash/length-verified bytes. Not cloneable/serializable.
pub struct Held {
    entries: BTreeMap<LogicalPath, Object>,
    bytes: BTreeMap<Digest, Vec<u8>>,
    _reads: Vec<managed::Read>,
    generation: Option<Use>,
    selection: Selection,
    reads_used: usize,
    read_bytes_used: usize,
}
impl Held {
    pub fn entries(&self) -> &BTreeMap<LogicalPath, Object> {
        &self.entries
    }
    pub fn selection(&self) -> &Selection {
        &self.selection
    }
    pub fn bytes(&self, path: &LogicalPath) -> Result<&[u8]> {
        let object = self
            .entries
            .get(path)
            .ok_or_else(|| error(ErrorKind::Missing, "public-control-entry"))?;
        self.bytes
            .get(&object.sha256)
            .map(Vec::as_slice)
            .ok_or_else(|| error(ErrorKind::NeedsRecovery, "public-control-bytes"))
    }
}
fn read_set(
    profile: &managed::Profile,
    selection: &Selection,
    nodes: &mut MetadataReader,
    limits: &Limits,
    budget: Budget,
    start: Instant,
) -> Result<Held> {
    limits
        .validate()
        .map_err(|_| error(ErrorKind::Budget, "public-control-metadata-budget"))?;
    budget.timed(start)?;
    let (entries, metrics) = Tree::from_root(selection.root.clone())
        .entries(
            nodes,
            TreeLimits {
                max_entries: budget.objects.min(limits.bindings),
                max_reads: limits.reads,
                max_read_bytes: limits.read_bytes,
                ..TreeLimits::default()
            },
        )
        .map_err(|e| {
            error(
                match e {
                    git_asset_core::tree::TreeError::Budget => ErrorKind::Budget,
                    git_asset_core::tree::TreeError::MissingNode(_) => ErrorKind::Missing,
                    git_asset_core::tree::TreeError::Store(_) => ErrorKind::NeedsRecovery,
                    git_asset_core::tree::TreeError::StaleRoot => ErrorKind::Stale,
                    git_asset_core::tree::TreeError::PathConflict => ErrorKind::Conflict,
                    _ => ErrorKind::Integrity,
                },
                "public-control-metadata",
            )
        })?;
    if entries.is_empty() {
        return Err(error(ErrorKind::Invalid, "public-control-empty"));
    }
    let mut total = 0u64;
    let mut sizes = BTreeMap::new();
    for object in entries.values() {
        if sizes
            .insert(object.sha256.clone(), object.bytes)
            .is_some_and(|old| old != object.bytes)
        {
            return Err(error(ErrorKind::Integrity, "public-control-size-conflict"));
        }
        if object.bytes > budget.object_bytes {
            return Err(error(ErrorKind::Budget, "public-control-object-budget"));
        }
        total = total
            .checked_add(object.bytes)
            .ok_or_else(|| error(ErrorKind::Budget, "public-control-total-budget"))?;
    }
    if total > budget.total_bytes {
        return Err(error(ErrorKind::Budget, "public-control-total-budget"));
    }
    let mut reader = managed::Reader::new(profile.clone()).map_err(native_error)?;
    let mut reads = Vec::new();
    let mut bytes = BTreeMap::new();
    for object in entries.values() {
        budget.timed(start)?;
        if bytes.contains_key(&object.sha256) {
            continue;
        }
        let mut held = reader.open(object).map_err(native_error)?;
        let mut raw = Vec::with_capacity(
            usize::try_from(object.bytes)
                .map_err(|_| error(ErrorKind::Budget, "public-control-length"))?,
        );
        held.by_ref()
            .take(object.bytes + 1)
            .read_to_end(&mut raw)
            .map_err(|_| error(ErrorKind::NeedsRecovery, "public-control-read"))?;
        if Object::from_bytes(&raw) != *object {
            return Err(error(ErrorKind::Integrity, "public-control-bytes"));
        }
        bytes.insert(object.sha256.clone(), raw);
        reads.push(held);
    }
    budget.timed(start)?;
    Ok(Held {
        entries,
        bytes,
        _reads: reads,
        generation: None,
        selection: selection.clone(),
        reads_used: metrics.reads,
        read_bytes_used: metrics.read_bytes,
    })
}

/// Streaming protection for media/results, separate from small in-memory controls.
#[derive(Clone, Copy, Debug)]
pub struct ProtectionBudget {
    pub objects: usize,
    pub object_bytes: u64,
    pub total_bytes: u64,
    pub millis: u64,
}
impl ProtectionBudget {
    fn check(self) -> Result<()> {
        if self.objects == 0
            || self.objects > 256
            || self.object_bytes == 0
            || self.object_bytes > 64 * 1024 * 1024 * 1024
            || self.total_bytes == 0
            || self.total_bytes > 1024 * 1024 * 1024 * 1024
            || self.millis == 0
            || self.millis > 300000
        {
            Err(error(ErrorKind::Budget, "public-retained-budget"))
        } else {
            Ok(())
        }
    }
    fn timed(self, start: Instant) -> Result<()> {
        if start.elapsed().as_millis() >= u128::from(self.millis) {
            Err(error(ErrorKind::Budget, "public-retained-time"))
        } else {
            Ok(())
        }
    }
}
/// Actual verified native readers, without materializing media into memory.
pub struct ProtectedSet {
    _reads: Vec<managed::Read>,
    _generation: Option<Use>,
    reads_used: usize,
    read_bytes_used: usize,
    entries: usize,
}
impl Seed {
    pub fn admit_protection(
        &self,
        profile: &managed::Profile,
        request: &Select,
        nodes: &mut MetadataReader,
        limits: &Limits,
        budget: ProtectionBudget,
    ) -> Result<ProtectedSet> {
        self.check_request(request)?;
        budget.check()?;
        let start = Instant::now();
        let mut ns = self.namespace(profile)?;
        let mut set = protect_set(profile, &request.selection, nodes, limits, budget, start)?;
        budget.timed(start)?;
        let mut remaining = limits.clone();
        remaining.roots = remaining.roots.saturating_sub(1);
        remaining.bindings = remaining.bindings.saturating_sub(set.entries);
        remaining.reads = remaining.reads.saturating_sub(set.reads_used);
        remaining.read_bytes = remaining.read_bytes.saturating_sub(set.read_bytes_used);
        remaining.millis = remaining
            .millis
            .saturating_sub(u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX));
        ns.select(request, nodes, &remaining)
            .map_err(admission_error)?;
        set._generation = Some(self.use_original(&mut ns, request)?);
        if budget.timed(start).is_err() {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "public-retained-time-after-admission",
            ));
        }
        Ok(set)
    }
    pub fn hold_protection(
        &self,
        profile: &managed::Profile,
        request: &Select,
        nodes: &mut MetadataReader,
        limits: &Limits,
        budget: ProtectionBudget,
    ) -> Result<ProtectedSet> {
        self.check_request(request)?;
        budget.check()?;
        let start = Instant::now();
        let mut ns = self.namespace(profile)?;
        let generation = self.use_original(&mut ns, request)?;
        let mut set = protect_set(profile, &request.selection, nodes, limits, budget, start)?;
        set._generation = Some(generation);
        budget.timed(start)?;
        Ok(set)
    }
}
fn protect_set(
    profile: &managed::Profile,
    selection: &Selection,
    nodes: &mut MetadataReader,
    limits: &Limits,
    budget: ProtectionBudget,
    start: Instant,
) -> Result<ProtectedSet> {
    limits
        .validate()
        .map_err(|_| error(ErrorKind::Budget, "public-retained-metadata-budget"))?;
    budget.timed(start)?;
    let (entries, metrics) = Tree::from_root(selection.root.clone())
        .entries(
            nodes,
            TreeLimits {
                max_entries: budget.objects.min(limits.bindings),
                max_reads: limits.reads,
                max_read_bytes: limits.read_bytes,
                ..TreeLimits::default()
            },
        )
        .map_err(|e| {
            error(
                match e {
                    git_asset_core::tree::TreeError::Budget => ErrorKind::Budget,
                    git_asset_core::tree::TreeError::MissingNode(_) => ErrorKind::Missing,
                    git_asset_core::tree::TreeError::Store(_) => ErrorKind::NeedsRecovery,
                    git_asset_core::tree::TreeError::StaleRoot => ErrorKind::Stale,
                    git_asset_core::tree::TreeError::PathConflict => ErrorKind::Conflict,
                    _ => ErrorKind::Integrity,
                },
                "public-retained-metadata",
            )
        })?;
    if entries.is_empty() {
        return Err(error(ErrorKind::Invalid, "public-retained-empty"));
    }
    let mut total = 0u64;
    let mut objects = BTreeMap::new();
    for object in entries.values() {
        if object.bytes > budget.object_bytes {
            return Err(error(ErrorKind::Budget, "public-retained-object"));
        }
        if objects
            .insert(object.sha256.clone(), object.clone())
            .is_some_and(|prior| prior != *object)
        {
            return Err(error(ErrorKind::Integrity, "public-retained-object"));
        }
        total = total
            .checked_add(object.bytes)
            .ok_or_else(|| error(ErrorKind::Budget, "public-retained-total"))?;
    }
    if total > budget.total_bytes {
        return Err(error(ErrorKind::Budget, "public-retained-total"));
    }
    let mut reader = managed::Reader::new(profile.clone()).map_err(native_error)?;
    let mut reads = Vec::new();
    for object in objects.values() {
        budget.timed(start)?;
        reads.push(reader.open(object).map_err(native_error)?);
    }
    budget.timed(start)?;
    Ok(ProtectedSet {
        _reads: reads,
        _generation: None,
        reads_used: metrics.reads,
        read_bytes_used: metrics.read_bytes,
        entries: entries.len(),
    })
}
