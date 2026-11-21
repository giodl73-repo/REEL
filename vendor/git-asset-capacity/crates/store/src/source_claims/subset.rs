//! Owner-authorized partial historical-root settlement. No payload deletion.
use super::{Command, Kind, Receipt, Selection, Source, Version, admission};
use crate::{Key, metadata::MetadataStore, protection::Limits};
use admission::{Namespace, Select, SourceCondition};
use git_asset_core::{
    Digest, LogicalPath, Object, identity,
    tree::{MemoryNodes, NodeStore, Tree, TreeError, TreeLimits, TreeResult},
};
use serde::{Deserialize, Serialize};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};

const MAX_PLAN: usize = 65_536;
pub const MAX_PATHS: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Authority,
    Stale,
    Missing,
    Budget,
    PendingMetadata,
    Journal(admission::Error),
    Metadata(crate::metadata::Error),
    Tree(TreeError),
    Injected,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<admission::Error> for Error {
    fn from(e: admission::Error) -> Self {
        Self::Journal(e)
    }
}
impl From<super::Error> for Error {
    fn from(e: super::Error) -> Self {
        if e == super::Error::Budget {
            Self::Budget
        } else {
            Self::Journal(e.into())
        }
    }
}
impl From<crate::metadata::Error> for Error {
    fn from(e: crate::metadata::Error) -> Self {
        Self::Metadata(e)
    }
}
impl From<TreeError> for Error {
    fn from(e: TreeError) -> Self {
        Self::Tree(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

fn encode(v: &impl Serialize) -> Result<Vec<u8>> {
    let raw = identity::canonical_json(&serde_json::to_value(v).map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)?;
    if raw.len() > MAX_PLAN {
        return Err(Error::Budget);
    }
    Ok(raw)
}
fn digest(v: &impl Serialize) -> Result<Digest> {
    Ok(Digest::blob(&encode(v)?))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub authority: Digest,
    pub namespace: Digest,
    pub policy: Digest,
    pub owner: Key,
    /// This controller only settles explicitly admitted branch/draft history.
    pub classes: Vec<Kind>,
    pub limits: Limits,
    pub writes: usize,
    pub write_bytes: usize,
}
impl Enrollment {
    pub fn id(&self) -> Result<Digest> {
        self.limits.validate().map_err(|_| Error::Budget)?;
        if self.classes.is_empty()
            || self.classes.len() > 2
            || self
                .classes
                .iter()
                .any(|k| !matches!(k, Kind::Branch | Kind::Draft))
            || self.classes.windows(2).any(|p| p[0] >= p[1])
            || self.writes == 0
            || self.writes > 65_536
            || self.write_bytes == 0
            || self.write_bytes > 32 * 1024 * 1024
        {
            return Err(Error::Invalid);
        }
        digest(&serde_json::json!({"schema":"tig.subset-enrollment.design-1","value":self}))
    }
    fn check(&self, namespace: &Namespace) -> Result<()> {
        self.id()?;
        if namespace.identity() != &self.namespace || digest(namespace.policy())? != self.policy {
            return Err(Error::Authority);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub source: Key,
    pub predecessor_revision: u64,
    /// Exact, sorted, unique flat binding paths. Selection policy is caller-owned.
    pub paths: Vec<LogicalPath>,
}
impl Request {
    fn check(&self) -> Result<()> {
        if self.predecessor_revision == 0
            || self.paths.is_empty()
            || self.paths.len() > MAX_PATHS
            || self.paths.windows(2).any(|p| p[0] >= p[1])
        {
            return Err(Error::Invalid);
        }
        encode(self)?;
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub selected_bindings: usize,
    pub selected_unique_objects: usize,
    pub survivor_bindings: usize,
    pub selected_objects_shared_with_survivors: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    schema: String,
    enrollment: Digest,
    request: Request,
    expected_revision: u64,
    before: Digest,
    class: Kind,
    original: Selection,
    survivor: Selection,
    effect: Digest,
    pub summary: Summary,
}
impl Plan {
    pub fn request(&self) -> &Request {
        &self.request
    }
    pub fn original(&self) -> &Selection {
        &self.original
    }
    pub(crate) fn survivor_selection(&self) -> Option<&Selection> {
        (self.summary.survivor_bindings != 0).then_some(&self.survivor)
    }
    pub(crate) fn class(&self) -> &Kind {
        &self.class
    }
    pub(crate) fn validate_projection(
        &self,
        e: &Enrollment,
        reader: &impl NodeStore,
    ) -> Result<()> {
        self.bound_profile(e)?;
        let (root, summary, _) = projection(e, &self.request, &self.original, reader)?;
        if root != self.survivor.root || summary != self.summary {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    /// Exact commands owned by this controller, never caller-chosen exemptions.
    pub(crate) fn commands(&self, e: &Enrollment) -> Result<Vec<(Key, Command)>> {
        self.bound_profile(e)?;
        let mut commands = Vec::new();
        if self.summary.survivor_bindings != 0 {
            commands.push(self.enroll(e)?);
            for r in self
                .select(e)?
                .requests(&e.namespace)
                .map_err(|_| Error::Invalid)?
            {
                commands.push((r.operation, r.command));
            }
            commands.push(self.withdraw(e)?);
        }
        commands.push(self.retire(e)?);
        Ok(commands)
    }
    /// Resolve selection meaning in the exact original root; no removal grant.
    pub fn selected_bindings(
        &self,
        e: &Enrollment,
        namespace: &mut Namespace,
        reader: &impl NodeStore,
    ) -> Result<BTreeMap<LogicalPath, Object>> {
        self.bound(e, namespace)?;
        let (_, original, class) = original_source(e, namespace, &self.request)?;
        if original != self.original || class != self.class {
            return Err(Error::Stale);
        }
        let mut overlay = Overlay::new(reader, e);
        let limits = overlay.limits();
        let (entries, _) = Tree::from_root(original.root).entries(&mut overlay, limits)?;
        self.request
            .paths
            .iter()
            .map(|path| {
                Ok((
                    path.clone(),
                    entries.get(path).ok_or(Error::Missing)?.clone(),
                ))
            })
            .collect()
    }
    pub fn id(&self) -> Result<Digest> {
        self.request.check()?;
        if self.schema != "tig.subset-plan.design-2"
            || self.expected_revision == 0
            || !matches!(self.class, Kind::Branch | Kind::Draft)
            || self.summary.selected_bindings != self.request.paths.len()
            || self.summary.selected_unique_objects == 0
            || self.summary.selected_unique_objects > self.summary.selected_bindings
            || self.summary.selected_objects_shared_with_survivors
                > self.summary.selected_unique_objects
        {
            return Err(Error::Invalid);
        }
        digest(self)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.id()?;
        encode(self)
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_PLAN {
            return Err(Error::Budget);
        }
        let plan: Self =
            serde_json::from_value(identity::parse_json(bytes).map_err(|_| Error::Invalid)?)
                .map_err(|_| Error::Invalid)?;
        if plan.to_json()? != bytes {
            return Err(Error::Invalid);
        }
        Ok(plan)
    }
    pub fn survivor_source(&self) -> Result<Option<Key>> {
        Ok(if self.summary.survivor_bindings == 0 {
            None
        } else {
            Some(self.key("subset")?)
        })
    }
    fn key(&self, prefix: &str) -> Result<Key> {
        Key::parse(&format!("{prefix}-{}", self.effect.as_str())).map_err(|_| Error::Invalid)
    }
    fn retire(&self, e: &Enrollment) -> Result<(Key, Command)> {
        Ok((
            Key::parse(&format!("subset-retire-{}", self.id()?.as_str()))
                .map_err(|_| Error::Invalid)?,
            Command::Retire {
                source: self.request.source.clone(),
                owner: e.owner.clone(),
                expected_revision: self.expected_revision,
                predecessor_revision: self.request.predecessor_revision,
            },
        ))
    }
    fn enroll(&self, e: &Enrollment) -> Result<(Key, Command)> {
        Ok((
            self.key("subset-enroll")?,
            Command::Enroll {
                source: self.key("subset")?,
                owner: e.owner.clone(),
                class: self.class.clone(),
            },
        ))
    }
    fn select(&self, e: &Enrollment) -> Result<Select> {
        Ok(Select {
            prepare: self.key("subset-prepare")?,
            adopt: self.key("subset-adopt")?,
            source: self.key("subset")?,
            owner: e.owner.clone(),
            expected_revision: 1,
            selection: self.survivor.clone(),
        })
    }
    fn withdraw(&self, e: &Enrollment) -> Result<(Key, Command)> {
        Ok((
            self.key("subset-withdraw")?,
            Command::Withdraw {
                source: self.key("subset")?,
                owner: e.owner.clone(),
                expected_revision: 3,
            },
        ))
    }
    fn bound(&self, e: &Enrollment, namespace: &Namespace) -> Result<()> {
        e.check(namespace)?;
        self.bound_profile(e)
    }
    fn bound_profile(&self, e: &Enrollment) -> Result<()> {
        self.id()?;
        if self.enrollment != e.id()? || !e.classes.contains(&self.class) {
            return Err(Error::Authority);
        }
        if self.effect
            != effect(
                e,
                &self.request,
                &self.class,
                &self.original,
                &self.survivor.root,
            )?
        {
            return Err(Error::Invalid);
        }
        if self.survivor.context != self.effect {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    fn survivor_matches(&self, e: &Enrollment, source: &Source) -> bool {
        source.owner == e.owner
            && source.kind.as_ref() == Some(&self.class)
            && source.revision == 4
            && source.current.is_none()
            && source.pending.is_none()
            && source.predecessors
                == [Version {
                    revision: 3,
                    selection: self.survivor.clone(),
                }]
    }
}
fn effect(
    e: &Enrollment,
    request: &Request,
    class: &Kind,
    original: &Selection,
    root: &Object,
) -> Result<Digest> {
    // Runtime budgets and unrelated current-source changes do not fork survivor custody.
    digest(
        &serde_json::json!({"schema":"tig.subset-effect.design-2","authority":e.authority,
        "namespace":e.namespace,"policy":e.policy,"owner":e.owner,"class":class,
        "request":request,"original":original,"survivor_root":root}),
    )
}

// Aggregate projection budget spans traversal, sparse edits and verification.
// Only newly written immutable nodes are staged, never a second metadata engine.
struct Overlay<'a, S> {
    reader: &'a S,
    nodes: MemoryNodes,
    e: &'a Enrollment,
    start: Instant,
    reads: Cell<usize>,
    read_bytes: Cell<usize>,
    writes: usize,
    write_bytes: usize,
}
impl<'a, S> Overlay<'a, S> {
    fn new(reader: &'a S, e: &'a Enrollment) -> Self {
        Self {
            reader,
            nodes: MemoryNodes::default(),
            e,
            start: Instant::now(),
            reads: Cell::new(0),
            read_bytes: Cell::new(0),
            writes: 0,
            write_bytes: 0,
        }
    }
    fn timed(&self) -> TreeResult<()> {
        if self.start.elapsed().as_millis() > u128::from(self.e.limits.millis) {
            Err(TreeError::Budget)
        } else {
            Ok(())
        }
    }
    fn limits(&self) -> TreeLimits {
        TreeLimits {
            max_entries: self.e.limits.bindings,
            max_reads: self.e.limits.reads,
            max_read_bytes: self.e.limits.read_bytes,
            max_writes: self.e.writes,
            max_write_bytes: self.e.write_bytes,
            max_map_depth: 64,
        }
    }
}
impl<S: NodeStore> NodeStore for Overlay<'_, S> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.timed()?;
        let reads = self.reads.get().checked_add(1).ok_or(TreeError::Budget)?;
        let bytes = self
            .read_bytes
            .get()
            .checked_add(usize::try_from(object.bytes).map_err(|_| TreeError::Budget)?)
            .ok_or(TreeError::Budget)?;
        if reads > self.e.limits.reads || bytes > self.e.limits.read_bytes {
            return Err(TreeError::Budget);
        }
        self.reads.set(reads);
        self.read_bytes.set(bytes);
        let value = match self.nodes.get(object)? {
            Some(raw) => Some(raw),
            None => self.reader.get(object)?,
        };
        if let Some(raw) = &value
            && Object::from_bytes(raw) != *object
        {
            return Err(TreeError::Integrity);
        }
        self.timed()?;
        Ok(value)
    }
    fn put(&mut self, raw: &[u8]) -> TreeResult<Object> {
        self.timed()?;
        self.writes = self.writes.checked_add(1).ok_or(TreeError::Budget)?;
        self.write_bytes = self
            .write_bytes
            .checked_add(raw.len())
            .ok_or(TreeError::Budget)?;
        if self.writes > self.e.writes || self.write_bytes > self.e.write_bytes {
            return Err(TreeError::Budget);
        }
        self.nodes.put(raw)
    }
}

fn projection(
    e: &Enrollment,
    request: &Request,
    original: &Selection,
    reader: &impl NodeStore,
) -> Result<(Object, Summary, MemoryNodes)> {
    let tree = Tree::from_root(original.root.clone());
    let mut overlay = Overlay::new(reader, e);
    let limits = overlay.limits();
    let (mut survivors, _) = tree.entries(&mut overlay, limits)?;
    let mut selected = BTreeMap::new();
    for path in &request.paths {
        overlay.timed()?;
        let object = survivors.remove(path).ok_or(Error::Missing)?;
        selected.insert(object.sha256.clone(), object);
    }
    let survivor_ids: BTreeSet<_> = survivors.values().map(|o| &o.sha256).collect();
    let shared = selected
        .keys()
        .filter(|id| survivor_ids.contains(id))
        .count();
    overlay.timed()?;
    let changes = request.paths.iter().map(|p| (p.clone(), None)).collect();
    let (replacement, _) = tree.edit(tree.root(), &changes, &mut overlay, limits)?;
    let (verified, _) = replacement.entries(&mut overlay, limits)?;
    if verified != survivors {
        return Err(Error::Invalid);
    }
    let summary = Summary {
        selected_bindings: request.paths.len(),
        selected_unique_objects: selected.len(),
        survivor_bindings: survivors.len(),
        selected_objects_shared_with_survivors: shared,
    };
    Ok((replacement.root().clone(), summary, overlay.nodes))
}
fn original_source(
    e: &Enrollment,
    namespace: &mut Namespace,
    request: &Request,
) -> Result<(Source, Selection, Kind)> {
    e.check(namespace)?;
    request.check()?;
    let source = namespace.source(&request.source)?.ok_or(Error::Missing)?;
    let class = source.kind.clone().ok_or(Error::Authority)?;
    if source.owner != e.owner || !e.classes.contains(&class) {
        return Err(Error::Authority);
    }
    if source.pending.is_some() {
        return Err(Error::Stale);
    }
    let original = source
        .predecessors
        .iter()
        .find(|p| p.revision == request.predecessor_revision)
        .ok_or(Error::Missing)?
        .selection
        .clone();
    Ok((source, original, class))
}
pub fn plan(
    e: &Enrollment,
    namespace: &mut Namespace,
    request: Request,
    reader: &impl NodeStore,
) -> Result<Plan> {
    let (source, original, class) = original_source(e, namespace, &request)?;
    let plan = plan_from_source(e, &source, request, original, class, reader)?;
    if namespace.source(&plan.request.source)? != Some(source) {
        return Err(Error::Stale);
    }
    Ok(plan)
}
fn plan_from_source(
    e: &Enrollment,
    source: &Source,
    request: Request,
    original: Selection,
    class: Kind,
    reader: &impl NodeStore,
) -> Result<Plan> {
    let (root, summary, _) = projection(e, &request, &original, reader)?;
    let effect = effect(e, &request, &class, &original, &root)?;
    let plan = Plan {
        schema: "tig.subset-plan.design-2".into(),
        enrollment: e.id()?,
        request,
        expected_revision: source.revision,
        before: digest(&source)?,
        class,
        original,
        survivor: Selection {
            root,
            context: effect.clone(),
        },
        effect,
        summary,
    };
    plan.id()?;
    Ok(plan)
}

/// Plan several exact historical removals, including multiple predecessors of
/// one source, against the existing pure transition model. No journal mutation.
pub fn plan_sequence(
    e: &Enrollment,
    namespace: &mut Namespace,
    requests: Vec<Request>,
    reader: &impl NodeStore,
) -> Result<Vec<Plan>> {
    e.check(namespace)?;
    if requests.is_empty() || requests.len() > 16 {
        return Err(Error::Budget);
    }
    let mut initial = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for request in &requests {
        request.check()?;
        if !seen.insert((
            request.source.as_str().to_owned(),
            request.predecessor_revision,
        )) {
            return Err(Error::Invalid);
        }
        let (source, _, _) = original_source(e, namespace, request)?;
        initial.insert(request.source.as_str().to_owned(), source);
    }
    let mut state = super::State {
        guarded: true,
        sources: initial.clone(),
        outcomes: BTreeMap::new(),
        head: namespace.checkpoint(),
        bytes: 0,
    };
    let mut plans = Vec::new();
    for request in requests {
        let source = state
            .sources
            .get(request.source.as_str())
            .ok_or(Error::Missing)?;
        let original = source
            .predecessors
            .iter()
            .find(|v| v.revision == request.predecessor_revision)
            .ok_or(Error::Missing)?
            .selection
            .clone();
        let class = source.kind.clone().ok_or(Error::Authority)?;
        let plan = plan_from_source(e, source, request, original, class, reader)?;
        let (operation, command) = plan.retire(e)?;
        state
            .transition(&super::Request { operation, command })
            .map_err(|error| {
                if error == super::Error::Budget {
                    Error::Budget
                } else {
                    Error::Stale
                }
            })?;
        plans.push(plan);
    }
    for (key, source) in initial {
        if namespace.source(&Key::parse(&key).map_err(|_| Error::Invalid)?)? != Some(source) {
            return Err(Error::Stale);
        }
    }
    Ok(plans)
}

/// Pure prediction, never a journal acknowledgement. The actual retirement
/// receipt must later match before execution progress may be acknowledged.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectedStep {
    pub plan: Plan,
    pub retirement_checkpoint: super::Checkpoint,
}
/// Compile original-occurrence pages through planned owned survivors. No source
/// command executes; only bounded new immutable metadata is staged in `nodes`.
/// Requests may repeat an original occurrence but may not repeat any binding.
pub fn plan_pages(
    e: &Enrollment,
    namespace: &mut Namespace,
    requests: Vec<Request>,
    nodes: &mut impl NodeStore,
) -> Result<Vec<ProjectedStep>> {
    e.check(namespace)?;
    if requests.is_empty() || requests.len() > 4096 {
        return Err(Error::Budget);
    }
    let mut count = 0usize;
    let mut selected = BTreeSet::new();
    for request in &requests {
        request.check()?;
        count = count
            .checked_add(request.paths.len())
            .ok_or(Error::Budget)?;
        if count > 65536 {
            return Err(Error::Budget);
        }
        for path in &request.paths {
            if !selected.insert((
                request.source.as_str().to_owned(),
                request.predecessor_revision,
                path.clone(),
            )) {
                return Err(Error::Invalid);
            }
        }
    }
    let mut state = namespace.planning_state()?;
    let baseline = state.head.clone();
    let mut positions = BTreeMap::<(String, u64), (Key, u64)>::new();
    let mut overlay = Overlay::new(&*nodes, e);
    let start = Instant::now();
    let mut steps = Vec::new();
    for mut request in requests {
        overlay.timed()?;
        let origin = (
            request.source.as_str().to_owned(),
            request.predecessor_revision,
        );
        if let Some((source, revision)) = positions.get(&origin) {
            request.source = source.clone();
            request.predecessor_revision = *revision;
        }
        let source = state
            .sources
            .get(request.source.as_str())
            .ok_or(Error::Missing)?;
        let class = source.kind.clone().ok_or(Error::Authority)?;
        if source.owner != e.owner || !e.classes.contains(&class) || source.pending.is_some() {
            return Err(Error::Authority);
        }
        let original = source
            .predecessors
            .iter()
            .find(|v| v.revision == request.predecessor_revision)
            .ok_or(Error::Missing)?
            .selection
            .clone();
        let plan = plan_from_source(e, source, request, original, class, &overlay)?;
        let (root, summary, generated) = projection(e, &plan.request, &plan.original, &overlay)?;
        if root != plan.survivor.root || summary != plan.summary {
            return Err(Error::Invalid);
        }
        for (_, body) in generated.objects() {
            overlay.put(body)?;
        }
        for (operation, command) in plan.commands(e)? {
            if state.outcomes.contains_key(operation.as_str()) {
                return Err(Error::Stale);
            }
            let request = super::Request { operation, command };
            let body = super::encode(&request)?;
            let bytes = super::growth_bytes(&state, body.len())?;
            let sequence = state
                .head
                .sequence
                .checked_add(1)
                .filter(|n| *n <= super::MAX_COMMANDS)
                .ok_or(Error::Budget)?;
            state.transition(&request)?;
            state.head = super::Checkpoint {
                genesis: baseline.genesis.clone(),
                sequence,
                hash: super::event_hash(sequence, &state.head.hash, &request)?,
            };
            state.bytes = bytes;
        }
        if let Some(survivor) = plan.survivor_source()? {
            positions.insert(origin, (survivor, 3));
        } else {
            positions.remove(&origin);
        }
        steps.push(ProjectedStep {
            plan,
            retirement_checkpoint: state.head.clone(),
        });
    }
    overlay.timed()?;
    let generated = overlay
        .nodes
        .objects()
        .map(|(_, b)| b.to_vec())
        .collect::<Vec<_>>();
    drop(overlay);
    // Provider IO occurs outside the pure model and never flushes metadata.
    for body in generated {
        if start.elapsed().as_millis() > u128::from(e.limits.millis) {
            return Err(Error::Budget);
        }
        if nodes.put(&body)? != Object::from_bytes(&body) {
            return Err(Error::Invalid);
        }
    }
    if start.elapsed().as_millis() > u128::from(e.limits.millis) {
        return Err(Error::Budget);
    }
    if namespace.planning_state()?.head != baseline {
        return Err(Error::Stale);
    }
    Ok(steps)
}

#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub schema: &'static str,
    pub plan: Digest,
    pub historical_retirement: Option<Receipt>,
    pub historical_survivor_withdrawal: Option<Receipt>,
    pub survivor_protection_current: bool,
    pub original_predecessor_present: bool,
    pub settlement_current: bool,
    pub authorizes_payload_deletion: bool,
}
/// Metadata-offline receipt/state inspection; this grants no coverage or lease.
pub fn inspect(e: &Enrollment, namespace: &mut Namespace, plan: &Plan) -> Result<Status> {
    plan.bound(e, namespace)?;
    let (id, command) = plan.retire(e)?;
    let historical_retirement = namespace.command_status(id, command)?;
    let (historical_survivor_withdrawal, survivor_protection_current) =
        if let Some(key) = plan.survivor_source()? {
            let (id, command) = plan.withdraw(e)?;
            let receipt = namespace.command_status(id, command)?;
            let current = receipt.is_some()
                && namespace
                    .source(&key)?
                    .is_some_and(|s| plan.survivor_matches(e, &s));
            (receipt, current)
        } else {
            (None, true)
        };
    let source = namespace
        .source(&plan.request.source)?
        .ok_or(Error::Missing)?;
    if source.owner != e.owner || source.kind.as_ref() != Some(&plan.class) {
        return Err(Error::Authority);
    }
    let original_predecessor_present = source
        .predecessors
        .iter()
        .any(|v| v.revision == plan.request.predecessor_revision);
    Ok(Status {
        schema: "tig.subset-status.design-1",
        plan: plan.id()?,
        settlement_current: historical_retirement.is_some()
            && !original_predecessor_present
            && survivor_protection_current,
        historical_retirement,
        historical_survivor_withdrawal,
        survivor_protection_current,
        original_predecessor_present,
        authorizes_payload_deletion: false,
    })
}

/// Verified historical unchanged-subset transfer. This is not current protection
/// or removal authority; callers must separately validate the current occurrence.
pub fn historical_survivor(
    e: &Enrollment,
    namespace: &mut Namespace,
    plan: &Plan,
    nodes: &impl NodeStore,
) -> Result<Option<(Key, Version)>> {
    plan.bound(e, namespace)?;
    let (root, summary, _) = projection(e, &plan.request, &plan.original, nodes)?;
    if root != plan.survivor.root || summary != plan.summary {
        return Err(Error::Invalid);
    }
    let (operation, command) = plan.retire(e)?;
    namespace
        .command_status(operation, command)?
        .ok_or(Error::Missing)?;
    let Some(source) = plan.survivor_source()? else {
        return Ok(None);
    };
    let (operation, command) = plan.enroll(e)?;
    namespace
        .command_status(operation, command)?
        .ok_or(Error::Missing)?;
    let select = plan.select(e)?;
    let prepared = namespace
        .command_status(
            select.prepare,
            Command::Prepare {
                source: source.clone(),
                owner: e.owner.clone(),
                expected_revision: 1,
                selection: plan.survivor.clone(),
            },
        )?
        .ok_or(Error::Missing)?;
    namespace
        .command_status(
            select.adopt,
            Command::Adopt {
                source: source.clone(),
                owner: e.owner.clone(),
                expected_revision: 2,
                prepared: prepared.reference,
            },
        )?
        .ok_or(Error::Missing)?;
    let (operation, command) = plan.withdraw(e)?;
    namespace
        .command_status(operation, command)?
        .ok_or(Error::Missing)?;
    Ok(Some((
        source,
        Version {
            revision: 3,
            selection: plan.survivor.clone(),
        },
    )))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    MetadataFinished,
    Enrolled,
    SurvivorProtected,
    BeforeRetire,
    Retired,
}
pub fn apply(
    e: &Enrollment,
    namespace: &mut Namespace,
    metadata: &mut MetadataStore,
    plan: &Plan,
) -> Result<Status> {
    apply_hook(e, namespace, metadata, plan, |_| Ok(()))
}
/// Hooks support controlled fault qualification; interruption can leave extra protection.
pub fn apply_hook(
    e: &Enrollment,
    namespace: &mut Namespace,
    metadata: &mut MetadataStore,
    plan: &Plan,
    hook: impl FnMut(Stage) -> Result<()>,
) -> Result<Status> {
    apply_inner(e, namespace, metadata, plan, None, hook)
}
pub(crate) fn apply_metered(
    e: &Enrollment,
    namespace: &mut Namespace,
    metadata: &mut MetadataStore,
    plan: &Plan,
    budget: &crate::protection::budget::Budget,
    hook: impl FnMut(Stage) -> Result<()>,
) -> Result<Status> {
    apply_inner(e, namespace, metadata, plan, Some(budget), hook)
}
fn apply_inner(
    e: &Enrollment,
    namespace: &mut Namespace,
    metadata: &mut MetadataStore,
    plan: &Plan,
    budget: Option<&crate::protection::budget::Budget>,
    mut hook: impl FnMut(Stage) -> Result<()>,
) -> Result<Status> {
    let status = inspect(e, namespace, plan)?;
    if status.historical_retirement.is_some() {
        return Ok(status);
    }
    if metadata.pending_nodes() != 0 {
        return Err(Error::PendingMetadata);
    }
    let mut reader = metadata.finished_reader()?;
    let reader = crate::protection::budget::Nodes {
        inner: &mut reader,
        budget,
    };
    let rebuilt = self::plan(e, namespace, plan.request.clone(), &reader)?;
    if &rebuilt != plan {
        return Err(Error::Stale);
    }
    let mut conditions = Vec::new();
    if plan.summary.survivor_bindings != 0 {
        let (root, summary, nodes) = projection(e, &plan.request, &plan.original, &reader)?;
        if root != plan.survivor.root || summary != plan.summary {
            return Err(Error::Stale);
        }
        for (_, bytes) in nodes.objects() {
            crate::protection::budget::Nodes {
                inner: &mut *metadata,
                budget,
            }
            .put(bytes)?;
        }
        metadata.finish()?;
        let mut finished = metadata.finished_reader()?;
        let finished = crate::protection::budget::Nodes {
            inner: &mut finished,
            budget,
        };
        // Independent finished-store closure readback, before any source adoption.
        let mut checked = Overlay::new(&finished, e);
        let limits = checked.limits();
        let (entries, _) = Tree::from_root(root).entries(&mut checked, limits)?;
        if entries.len() != summary.survivor_bindings {
            return Err(Error::Invalid);
        }
        hook(Stage::MetadataFinished)?;
        let (id, command) = plan.enroll(e)?;
        // Existing matching state without this controller's exact receipt is held.
        if namespace
            .command_status(id.clone(), command.clone())?
            .is_none()
            && namespace.source(&plan.key("subset")?)?.is_some()
        {
            return Err(Error::Stale);
        }
        namespace.apply(id, command, &finished, &e.limits)?;
        hook(Stage::Enrolled)?;
        namespace.select_predecessor(
            &plan.select(e)?,
            plan.key("subset-withdraw")?,
            &finished,
            &e.limits,
        )?;
        hook(Stage::SurvivorProtected)?;
        let key = plan.key("subset")?;
        let source = namespace.source(&key)?.ok_or(Error::Stale)?;
        if !plan.survivor_matches(e, &source) {
            return Err(Error::Stale);
        }
        conditions.push(SourceCondition {
            source: key,
            revision: source.revision,
        });
    }
    hook(Stage::BeforeRetire)?;
    let (id, command) = plan.retire(e)?;
    let mut finished = metadata.finished_reader()?;
    let finished = crate::protection::budget::Nodes {
        inner: &mut finished,
        budget,
    };
    // Checks the original revision and the survivor witness in one short transaction.
    namespace.apply_if_sources(id, command, &conditions, &finished, &e.limits)?;
    hook(Stage::Retired)?;
    inspect(e, namespace, plan)
}

#[cfg(test)]
mod tests;
