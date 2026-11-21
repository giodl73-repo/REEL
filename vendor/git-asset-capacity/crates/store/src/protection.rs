//! Exact observed demand from admitted journal roots. Never removal authority.
pub(crate) mod budget;
use crate::{
    Key,
    source_claims::{self, Checkpoint, Selection, SourceJournal},
};
use git_asset_core::{
    Claim, Digest, Object, Placement, View, identity,
    tree::{NodeStore, Tree, TreeError, TreeLimits, TreeResult},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidProfile,
    Budget,
    ConflictingSize,
    View,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
pub use crate::source_claims::{Kind, Places};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    FlatBindings,
    Opaque,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub key: Key,
    pub owner: Key,
    pub kind: Kind,
    pub format: Format,
    pub places: Places,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    pub identity: Digest,
    pub sources: Vec<Source>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub millis: u64,
    pub roots: usize,
    pub bindings: usize,
    pub associations: usize,
    pub reads: usize,
    pub read_bytes: usize,
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        if self.millis == 0
            || self.millis > 600_000
            || self.roots == 0
            || self.roots > 4096
            || self.bindings == 0
            || self.bindings > 500_000
            || self.associations == 0
            || self.associations > 500_000
            || self.reads == 0
            || self.reads > 2_000_000
            || self.read_bytes == 0
            || self.read_bytes > 512 * 1024 * 1024
        {
            return Err(Error::Budget);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub authority: Digest,
    pub generation: u64,
    pub kinds: BTreeSet<Kind>,
    /// Explicit boundary admission only, not automatic unmanaged-client discovery.
    pub unmanaged_boundary: Option<Digest>,
    pub journals: Vec<Journal>,
    pub limits: Limits,
}
/// Paths/minimum witnesses are private caller bindings, not portable identities.
pub struct Location {
    pub root: PathBuf,
    pub minimum: Checkpoint,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Current,
    Pending,
    Predecessor,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Revision {
    Selected { revision: u64 },
    Prepared { reference: source_claims::Reference },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Origin {
    pub revision: Revision,
    pub journal: Digest,
    pub source: Key,
    pub owner: Key,
    pub kind: Kind,
    pub role: Role,
    pub selection: Selection,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Gap {
    MissingKind { source_kind: Kind },
    UnmanagedBoundaryUnknown,
    MissingLocation { journal: Digest },
    UnavailableJournal { journal: Digest },
    UnexpectedSource { journal: Digest, source: String },
    MissingSource { journal: Digest, source: Key },
    ForeignOwner { journal: Digest, source: Key },
    ConflictingKind { journal: Digest, source: Key },
    UnsupportedClosure { journal: Digest, source: Key },
    UnresolvedRoot { root: Object },
    ChangedJournal { journal: Digest },
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Metrics {
    pub roots: usize,
    pub bindings: usize,
    pub associations: usize,
    pub reads: usize,
    pub read_bytes: usize,
}
#[derive(Serialize)]
pub struct Summary<'a> {
    pub schema: &'static str,
    pub id: &'a Digest,
    pub profile: &'a Digest,
    pub observed_scope_resolved: bool,
    pub gap_count: usize,
    pub gap_samples: &'a [Gap],
    pub gap_samples_truncated: bool,
    pub metrics: &'a Metrics,
    pub complete_claim_coverage: bool,
    pub removal_authority: bool,
}
pub struct Snapshot {
    id: Digest,
    profile: Digest,
    authority: Digest,
    checkpoints: BTreeMap<Digest, Checkpoint>,
    origins: BTreeMap<Digest, Origin>,
    demands: BTreeMap<Placement, BTreeSet<Digest>>,
    gaps: Vec<Gap>,
    gap_count: usize,
    metrics: Metrics,
}
impl Snapshot {
    pub fn id(&self) -> &Digest {
        &self.id
    }
    pub fn checkpoints(&self) -> &BTreeMap<Digest, Checkpoint> {
        &self.checkpoints
    }
    pub fn origins(&self) -> &BTreeMap<Digest, Origin> {
        &self.origins
    }
    pub fn demands(&self) -> &BTreeMap<Placement, BTreeSet<Digest>> {
        &self.demands
    }
    pub fn summary(&self) -> Summary<'_> {
        Summary {
            schema: "tig.protection-summary.design-1",
            id: &self.id,
            profile: &self.profile,
            observed_scope_resolved: self.gap_count == 0,
            gap_count: self.gap_count,
            gap_samples: &self.gaps,
            gap_samples_truncated: self.gap_count > self.gaps.len(),
            metrics: &self.metrics,
            complete_claim_coverage: false,
            removal_authority: false,
        }
    }
    fn gap(&mut self, gap: Gap) {
        self.gap_count += 1;
        if self.gaps.len() < 32 {
            self.gaps.push(gap);
        }
    }
    /// Bind actual observed claims into the existing copy plan. The adapter
    /// supplies its exact source-context digest; neither observation nor this
    /// identity turns source admission into an enforceable removal fence.
    pub fn copy_basis(&self, source: Digest) -> Result<crate::orchestration::Basis> {
        let mut gaps =
            vec![Key::parse("unfenced-source-admission").map_err(|_| Error::InvalidProfile)?];
        if self.gap_count != 0 {
            gaps.push(Key::parse("unresolved-root-scope").map_err(|_| Error::InvalidProfile)?);
        }
        Ok(crate::orchestration::Basis {
            source,
            policy: self.profile.clone(),
            claims: self.id.clone(),
            authority: self.authority.clone(),
            protection: crate::orchestration::ProtectionScope::Unknown { gaps },
        })
    }
    /// Positive assigned demands only. A resolved observation is not a fence.
    pub fn view(&self, generation: u64) -> Result<View> {
        if self.origins.len() > 1024 || self.metrics.associations > 65_536 {
            return Err(Error::Budget);
        }
        let mut claims: BTreeMap<_, _> = self
            .origins
            .keys()
            .map(|id| (id.clone(), BTreeSet::new()))
            .collect();
        for (placement, origins) in &self.demands {
            for origin in origins {
                claims
                    .get_mut(origin)
                    .ok_or(Error::View)?
                    .insert(placement.clone());
            }
        }
        View::new(
            generation,
            false,
            claims
                .into_iter()
                .map(|(id, placements)| Claim {
                    id: format!("protection-{}", id.as_str()),
                    placements,
                })
                .collect(),
        )
        .map_err(|error| match error {
            git_asset_core::Error::Budget => Error::Budget,
            _ => Error::View,
        })
    }
}
struct Hash(Sha256);
impl Hash {
    fn new(domain: &[u8]) -> Self {
        let mut h = Sha256::new();
        h.update(domain);
        Self(h)
    }
    fn push(&mut self, value: &impl Serialize) -> Result<()> {
        let raw = identity::canonical_json(
            &serde_json::to_value(value).map_err(|_| Error::InvalidProfile)?,
        )
        .map_err(|_| Error::InvalidProfile)?;
        self.0.update((raw.len() as u64).to_be_bytes());
        self.0.update(raw);
        Ok(())
    }
    fn finish(self) -> Digest {
        Digest::from_sha256(self.0.finalize().into())
    }
}
impl Profile {
    fn normalized(&self) -> Result<Self> {
        let l = &self.limits;
        if self.generation == 0
            || self.journals.is_empty()
            || self.journals.len() > 16
            || l.millis == 0
            || l.millis > 600_000
            || l.roots == 0
            || l.roots > 4096
            || l.bindings == 0
            || l.bindings > 500_000
            || l.associations == 0
            || l.associations > 500_000
            || l.reads == 0
            || l.reads > 2_000_000
            || l.read_bytes == 0
            || l.read_bytes > 512 * 1024 * 1024
        {
            return Err(Error::Budget);
        }
        // Bound input amplification before cloning/sorting caller-owned vectors.
        let mut count = 0usize;
        for journal in &self.journals {
            count = count
                .checked_add(journal.sources.len())
                .ok_or(Error::Budget)?;
            if count > 1024 {
                return Err(Error::Budget);
            }
            for source in &journal.sources {
                for places in [
                    &source.places.current,
                    &source.places.pending,
                    &source.places.predecessor,
                ] {
                    if places.is_empty() || places.len() > 16 {
                        return Err(Error::InvalidProfile);
                    }
                }
            }
        }
        let mut p = self.clone();
        p.journals.sort_by(|a, b| a.identity.cmp(&b.identity));
        let mut journals = BTreeSet::new();
        for j in &mut p.journals {
            if !journals.insert(j.identity.clone()) {
                return Err(Error::InvalidProfile);
            }
            j.sources.sort_by(|a, b| a.key.as_str().cmp(b.key.as_str()));
            let mut sources = BTreeSet::new();
            for source in &mut j.sources {
                if !sources.insert(source.key.as_str().to_owned()) {
                    return Err(Error::InvalidProfile);
                }
                for places in [
                    &mut source.places.current,
                    &mut source.places.pending,
                    &mut source.places.predecessor,
                ] {
                    places.sort_by(|a, b| a.as_str().cmp(b.as_str()));
                    if places.windows(2).any(|p| p[0] == p[1]) {
                        return Err(Error::InvalidProfile);
                    }
                }
            }
        }
        Ok(p)
    }
    pub fn id(&self) -> Result<Digest> {
        let p = self.normalized()?;
        let mut h = Hash::new(b"tig.protection-profile.design-1\0");
        h.push(&(
            &p.authority,
            p.generation,
            &p.kinds,
            &p.unmanaged_boundary,
            &p.limits,
        ))?;
        for journal in &p.journals {
            h.push(&journal.identity)?;
            h.push(&journal.sources.len())?;
            for source in &journal.sources {
                h.push(source)?;
            }
        }
        Ok(h.finish())
    }
}
pub(crate) struct BudgetNodes<'a, S> {
    inner: &'a S,
    limits: &'a Limits,
    used: Cell<(usize, usize)>,
    start: Instant,
}
impl<'a, S> BudgetNodes<'a, S> {
    pub(crate) fn new(inner: &'a S, limits: &'a Limits, start: Instant) -> Self {
        Self {
            inner,
            limits,
            start,
            used: Cell::new((0, 0)),
        }
    }
    pub(crate) fn timed(&self) -> TreeResult<()> {
        if self.start.elapsed() > Duration::from_millis(self.limits.millis) {
            Err(TreeError::Budget)
        } else {
            Ok(())
        }
    }
}
impl<S: NodeStore> NodeStore for BudgetNodes<'_, S> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.timed()?;
        let (reads, bytes) = self.used.get();
        let next_reads = reads.checked_add(1).ok_or(TreeError::Budget)?;
        let declared = usize::try_from(object.bytes).map_err(|_| TreeError::Budget)?;
        let next_bytes = bytes.checked_add(declared).ok_or(TreeError::Budget)?;
        if next_reads > self.limits.reads || next_bytes > self.limits.read_bytes {
            return Err(TreeError::Budget);
        }
        self.used.set((next_reads, next_bytes));
        let result = self.inner.get(object)?;
        self.timed()?;
        if let Some(raw) = &result {
            let actual = bytes
                .checked_add(raw.len().max(declared))
                .ok_or(TreeError::Budget)?;
            self.used.set((next_reads, actual));
            if actual > self.limits.read_bytes {
                return Err(TreeError::Budget);
            }
        }
        Ok(result)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Err(TreeError::Store("read-only projection".into()))
    }
}

pub fn capture(
    profile: &Profile,
    locations: &[Location],
    nodes: &impl NodeStore,
) -> Result<Snapshot> {
    let start = Instant::now();
    let profile = profile.normalized()?;
    let id = profile.id()?;
    if locations.len() > 16 {
        return Err(Error::Budget);
    }
    let mut indexed = BTreeMap::new();
    for location in locations {
        if indexed
            .insert(location.minimum.identity().clone(), location)
            .is_some()
        {
            return Err(Error::InvalidProfile);
        }
    }
    if indexed
        .keys()
        .any(|id| !profile.journals.iter().any(|j| &j.identity == id))
    {
        return Err(Error::InvalidProfile);
    }
    let mut snapshot = Snapshot {
        id: id.clone(),
        profile: id,
        authority: profile.authority.clone(),
        checkpoints: BTreeMap::new(),
        origins: BTreeMap::new(),
        demands: BTreeMap::new(),
        gaps: Vec::new(),
        gap_count: 0,
        metrics: Metrics::default(),
    };
    for kind in [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention] {
        if !profile.kinds.contains(&kind) {
            snapshot.gap(Gap::MissingKind { source_kind: kind });
        }
    }
    if profile.unmanaged_boundary.is_none() {
        snapshot.gap(Gap::UnmanagedBoundaryUnknown);
    }
    let mut bounded = BudgetNodes {
        inner: nodes,
        limits: &profile.limits,
        used: Cell::new((0, 0)),
        start,
    };
    let mut sizes = BTreeMap::new();
    for journal in &profile.journals {
        bounded.timed().map_err(|_| Error::Budget)?;
        let Some(location) = indexed.get(&journal.identity) else {
            snapshot.gap(Gap::MissingLocation {
                journal: journal.identity.clone(),
            });
            continue;
        };
        let observation = match SourceJournal::observe_all(&location.root, &location.minimum, 1024)
        {
            Ok(value) => value,
            Err(source_claims::Error::Budget) => return Err(Error::Budget),
            Err(_) => {
                snapshot.gap(Gap::UnavailableJournal {
                    journal: journal.identity.clone(),
                });
                continue;
            }
        };
        snapshot
            .checkpoints
            .insert(journal.identity.clone(), observation.checkpoint);
        for key in observation.sources.keys() {
            if !journal.sources.iter().any(|s| s.key.as_str() == key) {
                snapshot.gap(Gap::UnexpectedSource {
                    journal: journal.identity.clone(),
                    source: key.clone(),
                });
            }
        }
        for source in &journal.sources {
            let Some(Some(state)) = observation.sources.get(source.key.as_str()) else {
                snapshot.gap(Gap::MissingSource {
                    journal: journal.identity.clone(),
                    source: source.key.clone(),
                });
                continue;
            };
            if state.owner != source.owner {
                snapshot.gap(Gap::ForeignOwner {
                    journal: journal.identity.clone(),
                    source: source.key.clone(),
                });
                continue;
            }
            if state.kind.as_ref().is_some_and(|kind| kind != &source.kind) {
                snapshot.gap(Gap::ConflictingKind {
                    journal: journal.identity.clone(),
                    source: source.key.clone(),
                });
                continue;
            }
            if source.format != Format::FlatBindings
                && (state.current.is_some()
                    || state.pending.is_some()
                    || !state.predecessors.is_empty())
            {
                snapshot.gap(Gap::UnsupportedClosure {
                    journal: journal.identity.clone(),
                    source: source.key.clone(),
                });
                continue;
            }
            let mut roots = Vec::new();
            if let Some(current) = &state.current {
                roots.push((
                    Role::Current,
                    Revision::Selected {
                        revision: current.revision,
                    },
                    &current.selection,
                    &source.places.current,
                ));
            }
            if let Some(pending) = &state.pending {
                roots.push((
                    Role::Pending,
                    Revision::Prepared {
                        reference: pending.reference.clone(),
                    },
                    &pending.selection,
                    &source.places.pending,
                ));
            }
            for old in &state.predecessors {
                roots.push((
                    Role::Predecessor,
                    Revision::Selected {
                        revision: old.revision,
                    },
                    &old.selection,
                    &source.places.predecessor,
                ));
            }
            for (role, revision, selection, places) in roots {
                snapshot.metrics.roots += 1;
                if snapshot.metrics.roots > profile.limits.roots {
                    return Err(Error::Budget);
                }
                let limits = TreeLimits {
                    max_entries: profile.limits.bindings - snapshot.metrics.bindings,
                    max_reads: profile.limits.reads,
                    max_read_bytes: profile.limits.read_bytes,
                    ..TreeLimits::default()
                };
                let entries =
                    match Tree::from_root(selection.root.clone()).entries(&mut bounded, limits) {
                        Ok((entries, _)) => entries,
                        Err(TreeError::Budget) => return Err(Error::Budget),
                        Err(_) => {
                            snapshot.gap(Gap::UnresolvedRoot {
                                root: selection.root.clone(),
                            });
                            continue;
                        }
                    };
                snapshot.metrics.bindings += entries.len();
                let origin = Origin {
                    revision,
                    journal: journal.identity.clone(),
                    source: source.key.clone(),
                    owner: source.owner.clone(),
                    kind: source.kind.clone(),
                    role,
                    selection: selection.clone(),
                };
                let mut hash = Hash::new(b"tig.protection-origin.design-1\0");
                hash.push(&origin)?;
                let origin_id = hash.finish();
                snapshot.origins.insert(origin_id.clone(), origin);
                for object in entries.values() {
                    if sizes
                        .insert(object.sha256.clone(), object.bytes)
                        .is_some_and(|n| n != object.bytes)
                    {
                        return Err(Error::ConflictingSize);
                    }
                    for place in places {
                        let origins = snapshot
                            .demands
                            .entry(Placement {
                                object: object.clone(),
                                place: place.as_str().into(),
                            })
                            .or_default();
                        if origins.insert(origin_id.clone()) {
                            snapshot.metrics.associations += 1;
                            if snapshot.metrics.associations > profile.limits.associations {
                                return Err(Error::Budget);
                            }
                        }
                    }
                }
            }
        }
    }
    (snapshot.metrics.reads, snapshot.metrics.read_bytes) = bounded.used.get();
    // No SQL transaction spans tree IO. Recheck every successfully captured head.
    for (id, checkpoint) in snapshot.checkpoints.clone() {
        bounded.timed().map_err(|_| Error::Budget)?;
        let location = indexed.get(&id).ok_or(Error::InvalidProfile)?;
        match SourceJournal::observe_all(&location.root, &checkpoint, 1024) {
            Ok(current) if current.checkpoint == checkpoint => (),
            _ => snapshot.gap(Gap::ChangedJournal { journal: id }),
        }
    }
    let mut hash = Hash::new(b"tig.protection-snapshot.design-1\0");
    hash.push(&snapshot.profile)?;
    for (id, checkpoint) in &snapshot.checkpoints {
        hash.push(&(id, checkpoint))?;
    }
    for (id, origin) in &snapshot.origins {
        hash.push(&(id, origin))?;
    }
    for (placement, origins) in &snapshot.demands {
        hash.push(&(&placement.object, &placement.place, origins.len()))?;
        for origin in origins {
            hash.push(origin)?;
        }
    }
    hash.push(&(&snapshot.gaps, snapshot.gap_count))?;
    snapshot.id = hash.finish();
    bounded.timed().map_err(|_| Error::Budget)?;
    Ok(snapshot)
}

#[cfg(test)]
mod tests;
