//! Acknowledged conservative age observations. No payload or removal authority.
use super::{
    Checkpoint, Command, Kind, Selection, Source,
    admission::{Namespace, Select},
    subset,
};
use crate::{
    Key,
    final_retirement::provider::{Clock, TimeBounds},
    metadata::MetadataStore,
};
use git_asset_core::{
    Digest, LogicalPath, Object, identity,
    tree::{NodeStore, Tree, TreeError, TreeLimits, TreeResult},
};
use serde::{Deserialize, Serialize};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    time::{Duration, Instant},
};

mod capture;
pub use capture::{Capture, Stage};
#[cfg(test)]
mod tests;
const MAX_NUMBER: u64 = 9_007_199_254_740_991;
const MAX_RECORD: usize = 65_536;
#[derive(Debug)]
pub enum Error {
    Invalid,
    Authority,
    Missing,
    Recovery,
    Stale,
    Budget,
    Clock,
    Injected,
    Admission(super::admission::Error),
    Metadata(crate::metadata::Error),
    Subset(subset::Error),
    Tree(TreeError),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<super::admission::Error> for Error {
    fn from(e: super::admission::Error) -> Self {
        Self::Admission(e)
    }
}
impl From<crate::metadata::Error> for Error {
    fn from(e: crate::metadata::Error) -> Self {
        Self::Metadata(e)
    }
}
impl From<subset::Error> for Error {
    fn from(e: subset::Error) -> Self {
        Self::Subset(e)
    }
}
impl From<TreeError> for Error {
    fn from(e: TreeError) -> Self {
        if matches!(e, TreeError::Budget) {
            Self::Budget
        } else {
            Self::Tree(e)
        }
    }
}
pub type Result<T> = std::result::Result<T, Error>;
fn encode(v: &impl Serialize) -> Result<Vec<u8>> {
    let raw = identity::canonical_json(&serde_json::to_value(v).map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)?;
    if raw.len() > MAX_RECORD {
        return Err(Error::Budget);
    }
    Ok(raw)
}
fn hash(v: &impl Serialize) -> Result<Digest> {
    Ok(Digest::blob(&encode(v)?))
}
fn path(s: &str) -> Result<LogicalPath> {
    LogicalPath::parse(s).map_err(|_| Error::Invalid)
}
fn operation(prefix: &str, id: &Digest) -> Result<Key> {
    Key::parse(&format!("age-{prefix}-{}", id.as_str())).map_err(|_| Error::Invalid)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observer {
    pub namespace: Digest,
    pub policy: Digest,
    pub owners: Vec<Key>,
    pub classes: BTreeSet<Kind>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub authority: Digest,
    pub epoch: Digest,
    pub metadata: Digest,
    pub registry: Digest,
    pub registry_policy: Digest,
    pub source: Key,
    pub owner: Key,
    pub clock: Digest,
    pub observers: Vec<Observer>,
}
impl Enrollment {
    pub fn id(&self) -> Result<Digest> {
        if self.observers.is_empty()
            || self.observers.len() > 16
            || self
                .observers
                .windows(2)
                .any(|p| p[0].namespace >= p[1].namespace)
            || self.observers.iter().any(|o| {
                o.namespace == self.registry
                    || o.owners.is_empty()
                    || o.owners.len() > 16
                    || o.owners.windows(2).any(|p| p[0].as_str() >= p[1].as_str())
                    || o.classes.is_empty()
                    || o.classes.len() > 2
                    || o.classes
                        .iter()
                        .any(|k| !matches!(k, Kind::Branch | Kind::Draft))
            })
        {
            return Err(Error::Invalid);
        }
        hash(&serde_json::json!({"schema":"tig.age-enrollment.design-1","value":self}))
    }
    fn registry(&self, n: &mut Namespace, minimum: &Checkpoint) -> Result<Source> {
        self.id()?;
        n.refresh_minimum(minimum)?;
        if n.identity() != &self.registry || hash(n.policy())? != self.registry_policy {
            return Err(Error::Authority);
        }
        let s = n.source(&self.source)?.ok_or(Error::Missing)?;
        if s.owner != self.owner || s.kind != Some(Kind::Retention) || s.pending.is_some() {
            return Err(Error::Authority);
        }
        Ok(s)
    }
    fn observer(&self, n: &mut Namespace, minimum: &Checkpoint) -> Result<&Observer> {
        self.id()?;
        n.refresh_minimum(minimum)?;
        let o = self
            .observers
            .iter()
            .find(|o| &o.namespace == n.identity())
            .ok_or(Error::Authority)?;
        if hash(n.policy())? != o.policy {
            return Err(Error::Authority);
        }
        Ok(o)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub namespace: Digest,
    pub source: Key,
    pub predecessor_revision: u64,
}
impl Request {
    pub fn id(&self) -> Result<Digest> {
        if self.predecessor_revision == 0 || self.predecessor_revision > MAX_NUMBER {
            return Err(Error::Invalid);
        }
        hash(&serde_json::json!({"schema":"tig.age-occurrence.design-1","value":self}))
    }
    fn path(&self) -> Result<LogicalPath> {
        path(&format!("age/{}", self.id()?.as_str()))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub request: Request,
    pub policy: Digest,
    pub owner: Key,
    pub class: Kind,
    pub selection: Selection,
}
fn target(e: &Enrollment, n: &mut Namespace, minimum: &Checkpoint, r: &Request) -> Result<Target> {
    r.id()?;
    let o = e.observer(n, minimum)?;
    if n.identity() != &r.namespace {
        return Err(Error::Authority);
    }
    let s = n.source(&r.source)?.ok_or(Error::Missing)?;
    let class = s.kind.clone().ok_or(Error::Authority)?;
    if !o.owners.contains(&s.owner) || !o.classes.contains(&class) {
        return Err(Error::Authority);
    }
    let v = s
        .predecessors
        .iter()
        .find(|v| v.revision == r.predecessor_revision)
        .ok_or(Error::Missing)?;
    Ok(Target {
        request: r.clone(),
        policy: o.policy.clone(),
        owner: s.owner,
        class,
        selection: v.selection.clone(),
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stamp {
    pub earliest: u64,
    pub latest: u64,
}
impl Stamp {
    fn validate(self) -> Result<Self> {
        if self.earliest == 0 || self.latest < self.earliest || self.latest > MAX_NUMBER {
            Err(Error::Clock)
        } else {
            Ok(self)
        }
    }
    fn sample(clock: &dyn Clock, id: &Digest) -> Result<Self> {
        let TimeBounds { earliest, latest } = clock.bounds(id).map_err(|_| Error::Clock)?;
        Self { earliest, latest }.validate()
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Origin {
    Captured {
        checkpoint: Checkpoint,
        observed: Stamp,
    },
    Inherited {
        checkpoint: Checkpoint,
        parent: Object,
        plan: Object,
        enrollment: Box<subset::Enrollment>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    enrollment: Digest,
    target: Target,
    origin: Origin,
}
impl Record {
    fn slot(&self) -> Result<Digest> {
        match &self.origin {
            Origin::Captured { .. } => self.target.request.id(),
            Origin::Inherited { parent, plan, .. } => {
                inheritance_key(&self.target.request, parent, plan)
            }
        }
    }
    fn path(&self) -> Result<LogicalPath> {
        path(&format!("age/{}", self.slot()?.as_str()))
    }
    fn accepts(&self, key: &LogicalPath) -> Result<bool> {
        // Prior writers stored inherited records at the primary occurrence key.
        Ok(key == &self.target.request.path()? || key == &self.path()?)
    }
}
fn inheritance_key(r: &Request, parent: &Object, plan: &Object) -> Result<Digest> {
    hash(
        &serde_json::json!({"schema":"tig.age-inheritance-key.design-1",
        "request":r.id()?,"parent":parent,"plan":plan}),
    )
}
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub schema: &'static str,
    pub enrollment: Digest,
    pub request: Request,
    pub evidence: Option<Object>,
    pub target: Option<Target>,
    pub observed: Option<Stamp>,
    pub registry_checkpoint: Checkpoint,
    pub historical_only: bool,
    pub authorizes_removal: bool,
}
#[derive(Debug, Serialize)]
pub struct Assessment {
    pub schema: &'static str,
    pub status: Status,
    pub required_age_millis: u64,
    pub current: Option<Stamp>,
    pub age_lower_bound_millis: Option<u64>,
    pub old_enough: bool,
    pub hold: Option<&'static str>,
    pub authorizes_claim_release: bool,
    pub authorizes_removal: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub source: crate::protection::Limits,
    pub entries: usize,
    pub writes: usize,
    pub write_bytes: usize,
    pub lineage: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            source: crate::protection::Limits {
                millis: 60_000,
                roots: 256,
                bindings: 65_536,
                associations: 100_000,
                reads: 100_000,
                read_bytes: 64 * 1024 * 1024,
            },
            entries: 4096,
            writes: 65_536,
            write_bytes: 32 * 1024 * 1024,
            lineage: 32,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        self.source.validate().map_err(|_| Error::Budget)?;
        if self.entries == 0
            || self.entries > 65_535
            || self.entries >= self.source.bindings
            || self.writes > 65_536
            || self.write_bytes > 32 * 1024 * 1024
            || self.lineage == 0
            || self.lineage > 128
        {
            return Err(Error::Budget);
        }
        Ok(())
    }
    fn tree(&self) -> TreeLimits {
        TreeLimits {
            max_entries: self.entries + 1,
            max_reads: self.source.reads,
            max_writes: self.writes,
            max_read_bytes: self.source.read_bytes,
            max_write_bytes: self.write_bytes,
            max_map_depth: 64,
        }
    }
}
struct Meter {
    external: Option<crate::protection::budget::Budget>,
    start: Instant,
    reads: Cell<usize>,
    bytes: Cell<usize>,
    writes: Cell<usize>,
    write_bytes: Cell<usize>,
}
impl Meter {
    fn new() -> Rc<Self> {
        Self::with_budget(None)
    }
    fn with_budget(external: Option<&crate::protection::budget::Budget>) -> Rc<Self> {
        Rc::new(Self {
            external: external.cloned(),
            start: Instant::now(),
            reads: Cell::new(0),
            bytes: Cell::new(0),
            writes: Cell::new(0),
            write_bytes: Cell::new(0),
        })
    }
    fn timed(&self, limits: &Limits) -> TreeResult<()> {
        if let Some(b) = &self.external {
            b.timed()?;
        }
        if Instant::now()
            .checked_duration_since(self.start)
            .is_none_or(|a| a > Duration::from_millis(limits.source.millis))
        {
            Err(TreeError::Budget)
        } else {
            Ok(())
        }
    }
}
struct Work<'a, S> {
    store: &'a mut S,
    limits: &'a Limits,
    meter: Rc<Meter>,
}
impl<'a, S: NodeStore> Work<'a, S> {
    #[cfg(test)]
    fn new(store: &'a mut S, limits: &'a Limits) -> Result<Self> {
        Self::shared(store, limits, Meter::new())
    }
    fn shared(store: &'a mut S, limits: &'a Limits, meter: Rc<Meter>) -> Result<Self> {
        limits.validate()?;
        meter.timed(limits)?;
        Ok(Self {
            store,
            limits,
            meter,
        })
    }
    fn read<T: serde::de::DeserializeOwned + Serialize>(&self, o: &Object) -> Result<T> {
        if o.bytes > MAX_RECORD as u64 {
            return Err(Error::Budget);
        }
        let raw = self.get(o)?.ok_or(Error::Recovery)?;
        let value =
            serde_json::from_value(identity::parse_json(&raw).map_err(|_| Error::Recovery)?)
                .map_err(|_| Error::Recovery)?;
        if encode(&value)? != raw {
            return Err(Error::Recovery);
        }
        Ok(value)
    }
    fn record(&self, o: &Object, e: &Enrollment) -> Result<Record> {
        let r: Record = self.read(o)?;
        r.target.request.id()?;
        if r.schema != "tig.age-observation.design-1" || r.enrollment != e.id()? {
            return Err(Error::Authority);
        }
        Ok(r)
    }
}
impl<S: NodeStore> NodeStore for Work<'_, S> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.meter.timed(self.limits)?;
        let n = self
            .meter
            .reads
            .get()
            .checked_add(1)
            .ok_or(TreeError::Budget)?;
        let b = self
            .meter
            .bytes
            .get()
            .checked_add(usize::try_from(o.bytes).map_err(|_| TreeError::Budget)?)
            .ok_or(TreeError::Budget)?;
        if n > self.limits.source.reads || b > self.limits.source.read_bytes {
            return Err(TreeError::Budget);
        }
        self.meter.reads.set(n);
        self.meter.bytes.set(b);
        if let Some(budget) = &self.meter.external {
            budget.read(o)?;
        }
        let raw = self.store.get(o)?;
        self.meter.timed(self.limits)?;
        if raw.as_ref().is_some_and(|b| Object::from_bytes(b) != *o) {
            return Err(TreeError::InvalidNode);
        }
        Ok(raw)
    }
    fn put(&mut self, b: &[u8]) -> TreeResult<Object> {
        self.meter.timed(self.limits)?;
        let writes = self
            .meter
            .writes
            .get()
            .checked_add(1)
            .ok_or(TreeError::Budget)?;
        let write_bytes = self
            .meter
            .write_bytes
            .get()
            .checked_add(b.len())
            .ok_or(TreeError::Budget)?;
        if writes > self.limits.writes || write_bytes > self.limits.write_bytes {
            return Err(TreeError::Budget);
        }
        self.meter.writes.set(writes);
        self.meter.write_bytes.set(write_bytes);
        if let Some(budget) = &self.meter.external {
            budget.write(b)?;
        }
        let o = self.store.put(b)?;
        self.meter.timed(self.limits)?;
        if o != Object::from_bytes(b) {
            return Err(TreeError::InvalidNode);
        }
        Ok(o)
    }
}
fn identity_object(e: &Enrollment) -> Result<(Object, Vec<u8>)> {
    let raw = encode(
        &serde_json::json!({"schema":"tig.age-registry-identity.design-1","enrollment":e.id()?}),
    )?;
    Ok((Object::from_bytes(&raw), raw))
}
fn entries(
    e: &Enrollment,
    s: &Source,
    w: &mut Work<'_, impl NodeStore>,
) -> Result<BTreeMap<LogicalPath, Object>> {
    let v = s.current.as_ref().ok_or(Error::Missing)?;
    if v.selection.context != e.id()? {
        return Err(Error::Authority);
    }
    let map = Tree::from_root(v.selection.root.clone())
        .entries(w, w.limits.tree())?
        .0;
    if map.len() > w.limits.entries + 1
        || map.get(&path("identity")?) != Some(&identity_object(e)?.0)
    {
        return Err(Error::Authority);
    }
    if map.keys().any(|k| {
        k.as_str() != "identity" && (!k.as_str().starts_with("age/") || k.as_str().len() != 68)
    }) {
        return Err(Error::Recovery);
    }
    Ok(map)
}
fn verify_record(
    e: &Enrollment,
    o: &Object,
    observer: &mut Namespace,
    w: &mut Work<'_, impl NodeStore>,
    depth: usize,
) -> Result<(Record, Stamp)> {
    if depth >= w.limits.lineage {
        return Err(Error::Budget);
    }
    let r = w.record(o, e)?;
    let policy = e
        .observers
        .iter()
        .find(|p| p.namespace == r.target.request.namespace && p.policy == r.target.policy)
        .ok_or(Error::Authority)?;
    if observer.identity() != &policy.namespace
        || hash(observer.policy())? != policy.policy
        || !policy.owners.contains(&r.target.owner)
        || !policy.classes.contains(&r.target.class)
    {
        return Err(Error::Authority);
    }
    let stamp = match &r.origin {
        Origin::Captured {
            checkpoint,
            observed,
        } => {
            observer.verify_minimum(checkpoint)?;
            observed.validate()?
        }
        Origin::Inherited {
            checkpoint,
            parent,
            plan,
            enrollment,
        } => {
            observer.verify_minimum(checkpoint)?;
            let (p, stamp) = verify_record(e, parent, observer, w, depth + 1)?;
            let raw: serde_json::Value = w.read(plan)?;
            let plan = subset::Plan::from_json(&encode(&raw)?)?;
            if plan.request().source != p.target.request.source
                || plan.request().predecessor_revision != p.target.request.predecessor_revision
                || plan.original() != &p.target.selection
                || enrollment.owner != p.target.owner
                || enrollment.namespace != p.target.request.namespace
            {
                return Err(Error::Authority);
            }
            let (source, v) = subset::historical_survivor(enrollment, observer, &plan, w)?
                .ok_or(Error::Recovery)?;
            if source != r.target.request.source
                || v.revision != r.target.request.predecessor_revision
                || v.selection != r.target.selection
                || r.target.owner != p.target.owner
                || r.target.class != p.target.class
            {
                return Err(Error::Authority);
            }
            stamp
        }
    };
    Ok((r, stamp))
}

/// Historical inspection has no metadata writer or clock capability.
pub struct Inspection<'a, 'b, S> {
    pub enrollment: &'a Enrollment,
    pub registry: &'a mut Namespace,
    pub minimum: &'a Checkpoint,
    pub metadata: Digest,
    pub nodes: &'b mut S,
    pub limits: &'a Limits,
}
impl<S: NodeStore> Inspection<'_, '_, S> {
    /// Verify one pinned retained fact, independent of a newer best observation.
    /// This is historical evidence, never current eligibility or release authority.
    pub fn proof(
        &mut self,
        r: &Request,
        evidence: &Object,
        observer: &mut Namespace,
        minimum: &Checkpoint,
    ) -> Result<Status> {
        self.proof_with(r, evidence, observer, minimum, Meter::new())
    }
    pub(crate) fn proof_metered(
        &mut self,
        r: &Request,
        evidence: &Object,
        observer: &mut Namespace,
        minimum: &Checkpoint,
        budget: &crate::protection::budget::Budget,
    ) -> Result<Status> {
        self.proof_with(
            r,
            evidence,
            observer,
            minimum,
            Meter::with_budget(Some(budget)),
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn inherited_metered(
        &mut self,
        parent: &Object,
        observer: &mut Namespace,
        minimum: &Checkpoint,
        enrollment: &subset::Enrollment,
        plan: &subset::Plan,
        budget: &crate::protection::budget::Budget,
    ) -> Result<Status> {
        self.enrollment.observer(observer, minimum)?;
        if self.metadata != self.enrollment.metadata {
            return Err(Error::Authority);
        }
        let meter = Meter::with_budget(Some(budget));
        let source = self.enrollment.registry(self.registry, self.minimum)?;
        let mut work = Work::shared(self.nodes, self.limits, meter.clone())?;
        let (source_key, version) = subset::historical_survivor(enrollment, observer, plan, &work)?
            .ok_or(Error::Missing)?;
        let request = Request {
            namespace: observer.identity().clone(),
            source: source_key,
            predecessor_revision: version.revision,
        };
        let key = path(&format!(
            "age/{}",
            inheritance_key(&request, parent, &Object::from_bytes(&plan.to_json()?))?.as_str()
        ))?;
        let map = entries(self.enrollment, &source, &mut work)?;
        let evidence = map.get(&key).ok_or(Error::Missing)?.clone();
        self.proof_with(&request, &evidence, observer, minimum, meter)
    }
    fn proof_with(
        &mut self,
        r: &Request,
        evidence: &Object,
        observer: &mut Namespace,
        minimum: &Checkpoint,
        meter: Rc<Meter>,
    ) -> Result<Status> {
        r.id()?;
        self.enrollment.observer(observer, minimum)?;
        if observer.identity() != &r.namespace || self.metadata != self.enrollment.metadata {
            return Err(Error::Authority);
        }
        let source = self.enrollment.registry(self.registry, self.minimum)?;
        let mut work = Work::shared(self.nodes, self.limits, meter.clone())?;
        let map = entries(self.enrollment, &source, &mut work)?;
        let (record, stamp) = verify_record(self.enrollment, evidence, observer, &mut work, 0)?;
        if &record.target.request != r {
            return Err(Error::Authority);
        }
        if !map
            .iter()
            .any(|(key, object)| object == evidence && record.accepts(key).unwrap_or(false))
        {
            return Err(Error::Missing);
        }
        meter.timed(self.limits)?;
        Ok(Status {
            schema: "tig.age-status.design-1",
            enrollment: self.enrollment.id()?,
            request: r.clone(),
            evidence: Some(evidence.clone()),
            target: Some(record.target),
            observed: Some(stamp),
            registry_checkpoint: self.registry.checkpoint(),
            historical_only: true,
            authorizes_removal: false,
        })
    }
    pub fn status(
        &mut self,
        r: &Request,
        observer: &mut Namespace,
        minimum: &Checkpoint,
    ) -> Result<Status> {
        self.status_with(r, observer, minimum, Meter::new())
    }
    pub(crate) fn status_metered(
        &mut self,
        r: &Request,
        observer: &mut Namespace,
        minimum: &Checkpoint,
        budget: &crate::protection::budget::Budget,
    ) -> Result<Status> {
        self.status_with(r, observer, minimum, Meter::with_budget(Some(budget)))
    }
    fn status_with(
        &mut self,
        r: &Request,
        observer: &mut Namespace,
        minimum: &Checkpoint,
        meter: Rc<Meter>,
    ) -> Result<Status> {
        meter.timed(self.limits)?;
        self.enrollment.observer(observer, minimum)?;
        if observer.identity() != &r.namespace {
            return Err(Error::Authority);
        }
        if self.metadata != self.enrollment.metadata {
            return Err(Error::Authority);
        }
        let s = self.enrollment.registry(self.registry, self.minimum)?;
        let checkpoint = self.registry.checkpoint();
        let mut work = Work::shared(self.nodes, self.limits, meter.clone())?;
        let map = entries(self.enrollment, &s, &mut work)?;
        r.id()?;
        let mut best: Option<(Object, Record, Stamp)> = None;
        for (key, object) in &map {
            if key.as_str() == "identity" {
                continue;
            }
            let record = work.record(object, self.enrollment)?;
            if !record.accepts(key)? {
                return Err(Error::Recovery);
            }
            if &record.target.request != r {
                continue;
            }
            let (record, stamp) = verify_record(self.enrollment, object, observer, &mut work, 0)?;
            if best
                .as_ref()
                .is_some_and(|(_, prior, _)| prior.target != record.target)
            {
                return Err(Error::Recovery);
            }
            if best.as_ref().is_none_or(|(o, _, s)| {
                (stamp.latest, stamp.earliest, &object.sha256) < (s.latest, s.earliest, &o.sha256)
            }) {
                best = Some((object.clone(), record, stamp));
            }
        }
        let (evidence, target, observed) = if let Some((object, record, stamp)) = best {
            (Some(object), Some(record.target), Some(stamp))
        } else {
            (None, None, None)
        };
        meter.timed(self.limits)?;
        Ok(Status {
            schema: "tig.age-status.design-1",
            enrollment: self.enrollment.id()?,
            request: r.clone(),
            evidence,
            target,
            observed,
            registry_checkpoint: checkpoint,
            historical_only: true,
            authorizes_removal: false,
        })
    }
    /// Explicit assessment can append an observation to the independently
    /// admitted clock. It changes no source, registry or metadata record.
    pub fn assess(
        &mut self,
        r: &Request,
        observer: &mut Namespace,
        minimum: &Checkpoint,
        clock: &dyn Clock,
        required: u64,
    ) -> Result<Assessment> {
        if required > MAX_NUMBER {
            return Err(Error::Budget);
        }
        let meter = Meter::new();
        let status = self.status_with(r, observer, minimum, meter.clone())?;
        let mut result = Assessment {
            schema: "tig.age-assessment.design-1",
            status,
            required_age_millis: required,
            current: None,
            age_lower_bound_millis: None,
            old_enough: false,
            hold: None,
            authorizes_claim_release: false,
            authorizes_removal: false,
        };
        let Some(stamp) = result.status.observed else {
            result.hold = Some("unknown_age");
            return Ok(result);
        };
        let actual = match target(self.enrollment, observer, minimum, r) {
            Ok(t) => t,
            Err(Error::Missing) => {
                result.hold = Some("occurrence_missing");
                return Ok(result);
            }
            Err(e) => return Err(e),
        };
        if result.status.target.as_ref() != Some(&actual) {
            return Err(Error::Recovery);
        }
        meter.timed(self.limits)?;
        let now = Stamp::sample(clock, &self.enrollment.clock)?;
        meter.timed(self.limits)?;
        if now.earliest < stamp.earliest {
            return Err(Error::Clock);
        }
        let deadline = stamp
            .latest
            .checked_add(required)
            .filter(|n| *n <= MAX_NUMBER)
            .ok_or(Error::Budget)?;
        result.current = Some(now);
        // Overlapping intervals establish zero positive elapsed time; the
        // independent checked deadline comparison still decides eligibility.
        result.age_lower_bound_millis = Some(now.earliest.saturating_sub(stamp.latest));
        result.old_enough = now.earliest >= deadline;
        if !result.old_enough {
            result.hold = Some("not_old_enough")
        }
        Ok(result)
    }
}
