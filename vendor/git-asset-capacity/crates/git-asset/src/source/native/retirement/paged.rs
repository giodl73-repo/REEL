//! Indexed native selection candidates. No source or payload mutation authority.
use super::*;
use git_asset_store::{
    Key,
    final_retirement::{paged as selection, provider::Clock},
    source_claims::{Checkpoint, Kind, admission::Namespace, age},
};

const INDEX: &str = "tig.native-retirement-occurrences.design-1";
const CANDIDATE: &str = "tig.native-paged-selection.design-1";
const MAX_ROOTS: usize = 4096;
/// One call-owned budget across staging, finish and independent reader views.
/// Cloning a session retains the same counters/deadline.
#[derive(Clone)]
pub struct Session {
    limits: Limits,
    meter: Rc<Meter>,
}
impl Session {
    pub fn new(limits: Limits) -> Result<Self> {
        limits.validate()?;
        Ok(Self {
            limits,
            meter: Meter::new(),
        })
    }
    fn work<'a, S>(&self, nodes: &'a mut S) -> Result<Work<'a, S>> {
        Work::shared(nodes, self.limits, self.meter.clone())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Occurrence {
    pub namespace: Digest,
    pub source: Key,
    pub predecessor_revision: u64,
    pub header: Digest,
}
impl Occurrence {
    fn request(&self) -> age::Request {
        age::Request {
            namespace: self.namespace.clone(),
            source: self.source.clone(),
            predecessor_revision: self.predecessor_revision,
        }
    }
    fn id(&self) -> Result<Digest> {
        self.request().id().map_err(|_| Error::Invalid)
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    schema: String,
    root: Object,
    count: usize,
}
fn doc(value: &impl Serialize, work: &mut Work<'_, impl NodeStore>) -> Result<Object> {
    work.put(&encode(value)?).map_err(tree_error)
}
fn read<T: serde::de::DeserializeOwned + Serialize>(
    o: &Object,
    work: &Work<'_, impl NodeStore>,
) -> Result<T> {
    if o.bytes > 65_536 {
        return Err(Error::Budget);
    }
    let raw = work.get(o).map_err(tree_error)?.ok_or(Error::Metadata)?;
    let value = serde_json::from_value(identity::parse_json(&raw).map_err(|_| Error::Integrity)?)
        .map_err(|_| Error::Integrity)?;
    if encode(&value)? != raw {
        return Err(Error::Integrity);
    }
    Ok(value)
}
fn entries(
    root: &Object,
    work: &mut Work<'_, impl NodeStore>,
) -> Result<BTreeMap<LogicalPath, Object>> {
    let limits = work.limits.tree();
    Tree::from_root(root.clone())
        .entries(work, limits)
        .map(|r| r.0)
        .map_err(tree_error)
}
fn index(
    root: &Object,
    work: &mut Work<'_, impl NodeStore>,
) -> Result<(Index, BTreeMap<LogicalPath, Object>)> {
    let h: Index = read(root, work)?;
    if h.schema != INDEX || h.count == 0 || h.count > MAX_ROOTS {
        return Err(Error::Invalid);
    }
    let map = entries(&h.root, work)?;
    if map.len() != h.count {
        return Err(Error::Integrity);
    }
    for (key, object) in &map {
        let r: Occurrence = read(object, work)?;
        if key.as_str() != r.id()?.as_str() {
            return Err(Error::Integrity);
        }
    }
    Ok((h, map))
}
/// Append a bounded chunk without mutating the prior index or any source claim.
/// Conflicting header reuse for one occurrence refuses. Finish/readback is separate.
pub fn occurrences(
    base: Option<&Object>,
    rows: Vec<Occurrence>,
    nodes: &mut impl NodeStore,
    session: &Session,
) -> Result<Object> {
    if rows.is_empty() || rows.len() > MAX_ROOTS {
        return Err(Error::Invalid);
    }
    let limits = session.limits;
    let mut work = session.work(nodes)?;
    let (old, mut map) = if let Some(base) = base {
        let (h, map) = index(base, &mut work)?;
        (Some(h.root), map)
    } else {
        (None, BTreeMap::new())
    };
    let mut changes = BTreeMap::new();
    for row in rows {
        let key = path(row.id()?.as_str())?;
        if let Some(o) = map.get(&key) {
            if read::<Occurrence>(o, &work)? != row {
                return Err(Error::Integrity);
            }
            continue;
        }
        if map.len() >= MAX_ROOTS {
            return Err(Error::Budget);
        }
        let o = doc(&row, &mut work)?;
        changes.insert(key.clone(), Some(o.clone()));
        map.insert(key, o);
    }
    let root = if let Some(old) = old {
        Tree::from_root(old.clone())
            .edit(&old, &changes, &mut work, limits.tree())
            .map_err(tree_error)?
            .0
    } else {
        Tree::build(&map, &mut work, limits.tree())
            .map_err(tree_error)?
            .0
    };
    doc(
        &Index {
            schema: INDEX.into(),
            root: root.root().clone(),
            count: map.len(),
        },
        &mut work,
    )
}
pub fn inspect_occurrences(
    root: &Object,
    nodes: &mut impl NodeStore,
    session: &Session,
) -> Result<usize> {
    let (header, _) = index(root, &mut session.work(nodes)?)?;
    Ok(header.count)
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgePolicy {
    pub enrollment: age::Enrollment,
    pub required_age_millis: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub metadata: Digest,
    pub owner: Key,
    pub categories: Categories,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age: Option<AgePolicy>,
}
impl Enrollment {
    pub fn id(&self) -> Result<Digest> {
        if let Some(a) = &self.age {
            a.enrollment.id().map_err(|_| Error::Invalid)?;
            if a.enrollment.metadata != self.metadata
                || a.enrollment.owner != self.owner
                || a.required_age_millis == 0
                || a.required_age_millis > 9_007_199_254_740_991
            {
                return Err(Error::Invalid);
            }
        }
        Ok(Digest::blob(&encode(
            &serde_json::json!({"schema":"tig.native-paged-enrollment.design-1","value":self}),
        )?))
    }
}
pub struct AgeContext<'a> {
    pub registry: &'a mut Namespace,
    pub minimum: &'a Checkpoint,
    pub clock: &'a dyn Clock,
    pub limits: &'a age::Limits,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgeObservation {
    evidence: Option<Object>,
    observed: Option<age::Stamp>,
    current: Option<age::Stamp>,
    old_enough: bool,
    hold: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hold {
    NoBindings,
    AgeUnknown,
    AgeRecent,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub occurrence: Occurrence,
    pub original: Selection,
    pub source_checkpoint: Checkpoint,
    pub assets: Object,
    pub asset_count: usize,
    pub bindings: Object,
    pub binding_count: usize,
    pub hold: Option<Hold>,
    age: Option<AgeObservation>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    schema: String,
    enrollment: Digest,
    input: Object,
    filter: Object,
    paging: selection::Paging,
    rows: Object,
    request: Option<Object>,
    roots: usize,
    selected_roots: usize,
    matched_assets: usize,
    bindings: usize,
}
impl Candidate {
    pub fn id(&self) -> Result<Digest> {
        if self.schema != CANDIDATE
            || self.roots == 0
            || self.roots > MAX_ROOTS
            || self.selected_roots > self.roots
            || self.bindings > 65536
            || self.matched_assets > 65536
            || self.request.is_some() != (self.selected_roots != 0)
        {
            return Err(Error::Invalid);
        }
        Ok(Digest::blob(&encode(self)?))
    }
    pub fn request(&self) -> Option<&Object> {
        self.request.as_ref()
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.id()?;
        encode(self)
    }
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        if raw.len() > 65_536 {
            return Err(Error::Budget);
        }
        let value: Self =
            serde_json::from_value(identity::parse_json(raw).map_err(|_| Error::Invalid)?)
                .map_err(|_| Error::Invalid)?;
        if value.to_json()? != raw {
            return Err(Error::Integrity);
        }
        Ok(value)
    }
}
fn chosen(
    inv: &Inventory,
    e: &Enrollment,
    filter: &Filter,
    found: &mut BTreeSet<Reference>,
) -> Result<Preview> {
    let assets = filter
        .assets
        .iter()
        .filter(|r| inv.assets.iter().any(|a| &a.reference == *r))
        .cloned()
        .collect::<BTreeSet<_>>();
    found.extend(assets.clone());
    if !filter.assets.is_empty() && assets.is_empty() {
        return Ok(Preview {
            schema: "tig.native-retirement-selection.design-1",
            header: inv.header.clone(),
            source_checkpoint: inv.checkpoint.clone(),
            assets: Vec::new(),
            paths: Vec::new(),
            objects: BTreeSet::new(),
            age_filter_supported: false,
            complete_semantic_coverage: false,
            payload_verified: false,
            authorizes_removal: false,
        });
    }
    super::select_inner(
        inv,
        &e.categories,
        &Filter {
            assets,
            categories: filter.categories.clone(),
        },
    )
}
fn asset_index(assets: &[MatchedAsset], work: &mut Work<'_, impl NodeStore>) -> Result<Object> {
    let mut map = BTreeMap::new();
    for a in assets {
        map.insert(path(a.asset.object.sha256.as_str())?, doc(a, work)?);
    }
    let limits = work.limits.tree();
    Tree::build(&map, work, limits)
        .map(|t| t.0.root().clone())
        .map_err(tree_error)
}
fn consistent_sizes(
    inv: &Inventory,
    known: &mut BTreeMap<Digest, u64>,
    limit: usize,
) -> Result<()> {
    for object in inv.actual.values() {
        if known
            .insert(object.sha256.clone(), object.bytes)
            .is_some_and(|old| old != object.bytes)
        {
            return Err(Error::Integrity);
        }
        if known.len() > limit {
            return Err(Error::Budget);
        }
    }
    Ok(())
}
/// Stage the whole native derivation and exact existing paged request. Both
/// source and provider execution permissions remain external and independent.
#[allow(clippy::too_many_arguments)]
pub fn select(
    e: &Enrollment,
    input: &Object,
    filter: &Filter,
    paging: selection::Paging,
    namespaces: &mut [Namespace],
    nodes: &mut impl NodeStore,
    mut age_context: Option<&mut AgeContext<'_>>,
    session: &Session,
) -> Result<Candidate> {
    e.id()?;
    paging.validate().map_err(|_| Error::Invalid)?;
    if namespaces.is_empty()
        || namespaces.len() > 16
        || e.age.is_some() != age_context.is_some()
        || (filter.assets.is_empty() && filter.categories.is_empty())
        || filter.assets.len() > 1024
        || filter
            .categories
            .iter()
            .any(|c| !e.categories.contains_key(c))
    {
        return Err(Error::Invalid);
    }
    let mut basis = BTreeMap::new();
    for n in &mut *namespaces {
        let minimum = n.checkpoint();
        n.observe(&minimum).map_err(|_| Error::Incomplete)?;
        if basis.insert(n.identity().clone(), n.checkpoint()).is_some()
            || e.age
                .as_ref()
                .is_some_and(|a| &a.enrollment.registry == n.identity())
        {
            return Err(Error::Invalid);
        }
    }
    let limits = session.limits;
    let mut work = session.work(nodes)?;
    let (_, index) = index(input, &mut work)?;
    let filter_object = doc(filter, &mut work)?;
    let mut rows = BTreeMap::new();
    let mut selections = Vec::new();
    let mut found = BTreeSet::new();
    let mut sizes = BTreeMap::new();
    let (mut matched_assets, mut bindings, mut all_bindings) = (0usize, 0usize, 0usize);
    for (key, object) in &index {
        work.timed()?;
        let occurrence: Occurrence = read(object, &work)?;
        let ns = namespaces
            .iter_mut()
            .find(|n| n.identity() == &occurrence.namespace)
            .ok_or(Error::Incomplete)?;
        let source = ns
            .source(&occurrence.source)
            .map_err(|_| Error::Incomplete)?
            .ok_or(Error::Incomplete)?;
        if source.owner != e.owner
            || !matches!(source.kind, Some(Kind::Branch | Kind::Draft))
            || source.pending.is_some()
        {
            return Err(Error::Integrity);
        }
        let original = source
            .predecessors
            .iter()
            .find(|v| v.revision == occurrence.predecessor_revision)
            .ok_or(Error::Incomplete)?
            .selection
            .clone();
        let inv = super::inspect_work(&original, &occurrence.header, &mut work)?;
        consistent_sizes(&inv, &mut sizes, limits.nodes)?;
        let preview = chosen(&inv, e, filter, &mut found)?;
        matched_assets = matched_assets
            .checked_add(preview.assets.len())
            .ok_or(Error::Budget)?;
        all_bindings = all_bindings
            .checked_add(preview.paths.len())
            .ok_or(Error::Budget)?;
        if matched_assets > limits.bindings || all_bindings > limits.bindings {
            return Err(Error::Budget);
        }
        let map = preview
            .paths
            .iter()
            .map(|p| {
                Ok((
                    p.clone(),
                    inv.actual.get(p).cloned().ok_or(Error::Integrity)?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let chosen_root = Tree::build(&map, &mut work, limits.tree())
            .map_err(tree_error)?
            .0
            .root()
            .clone();
        let age = if let (Some(policy), Some(ctx)) = (&e.age, age_context.as_deref_mut()) {
            let assessment = age::Inspection {
                enrollment: &policy.enrollment,
                registry: ctx.registry,
                minimum: ctx.minimum,
                metadata: e.metadata.clone(),
                nodes: &mut work,
                limits: ctx.limits,
            }
            .assess(
                &occurrence.request(),
                ns,
                basis.get(&occurrence.namespace).ok_or(Error::Integrity)?,
                ctx.clock,
                policy.required_age_millis,
            )
            .map_err(|e| match e {
                age::Error::Budget => Error::Budget,
                age::Error::Authority => Error::Integrity,
                _ => Error::Incomplete,
            })?;
            if assessment
                .status
                .target
                .as_ref()
                .is_some_and(|t| t.selection != original)
            {
                return Err(Error::Integrity);
            }
            Some(AgeObservation {
                evidence: assessment.status.evidence,
                observed: assessment.status.observed,
                current: assessment.current,
                old_enough: assessment.old_enough,
                hold: assessment.hold.map(str::to_owned),
            })
        } else {
            None
        };
        let hold = if map.is_empty() {
            Some(Hold::NoBindings)
        } else if age.as_ref().is_some_and(|a| a.evidence.is_none()) {
            Some(Hold::AgeUnknown)
        } else if age.as_ref().is_some_and(|a| !a.old_enough) {
            Some(Hold::AgeRecent)
        } else {
            None
        };
        if hold.is_none() {
            bindings += map.len();
            selections.push(selection::Selection {
                namespace: occurrence.namespace.clone(),
                source: occurrence.source.clone(),
                predecessor_revision: occurrence.predecessor_revision,
                bindings: chosen_root.clone(),
            });
        }
        let row = Row {
            occurrence,
            original,
            source_checkpoint: ns.checkpoint(),
            assets: asset_index(&preview.assets, &mut work)?,
            asset_count: preview.assets.len(),
            bindings: chosen_root,
            binding_count: map.len(),
            hold,
            age,
        };
        rows.insert(key.clone(), doc(&row, &mut work)?);
    }
    if found != filter.assets {
        return Err(Error::Incomplete);
    }
    let selected_roots = selections.len();
    let request = if selections.is_empty() {
        None
    } else {
        Some(
            selection::request(
                selections,
                paging.clone(),
                &mut work,
                &selection::Limits::default(),
            )
            .map_err(selection_error)?,
        )
    };
    let rows = Tree::build(&rows, &mut work, limits.tree())
        .map_err(tree_error)?
        .0
        .root()
        .clone();
    for ns in namespaces {
        ns.observe(basis.get(ns.identity()).ok_or(Error::Integrity)?)
            .map_err(|_| Error::Incomplete)?;
        if basis.get(ns.identity()) != Some(&ns.checkpoint()) {
            return Err(Error::Integrity);
        }
    }
    work.timed()?;
    let c = Candidate {
        schema: CANDIDATE.into(),
        enrollment: e.id()?,
        input: input.clone(),
        filter: filter_object,
        paging,
        rows,
        request,
        roots: index.len(),
        selected_roots,
        matched_assets,
        bindings,
    };
    c.id()?;
    Ok(c)
}

#[derive(Serialize)]
pub struct Summary {
    pub schema: &'static str,
    pub candidate: Digest,
    pub request: Option<Object>,
    pub rows: Object,
    pub roots: usize,
    pub selected_roots: usize,
    pub matched_assets: usize,
    pub bindings: usize,
    pub held_roots: usize,
    pub historical_only: bool,
    pub authorizes_claim_release: bool,
    pub authorizes_removal: bool,
}
pub struct Inspection {
    rows: Vec<Row>,
    summary: Summary,
}
impl Inspection {
    pub fn rows(&self) -> impl ExactSizeIterator<Item = &Row> {
        self.rows.iter()
    }
    pub fn summary(&self) -> &Summary {
        &self.summary
    }
}
fn age_hold(e: &Enrollment, row: &Row) -> Result<Option<Hold>> {
    if row.age.is_some() != e.age.is_some() {
        return Err(Error::Integrity);
    }
    if let (Some(policy), Some(a)) = (&e.age, &row.age) {
        match (&a.evidence, a.observed, a.current) {
            (None, None, None) if !a.old_enough && a.hold.as_deref() == Some("unknown_age") => (),
            (Some(_), Some(observed), Some(now)) => {
                if observed.earliest == 0
                    || observed.latest < observed.earliest
                    || now.earliest < observed.earliest
                    || now.latest < now.earliest
                    || now.latest > 9_007_199_254_740_991
                {
                    return Err(Error::Integrity);
                }
                let deadline = observed
                    .latest
                    .checked_add(policy.required_age_millis)
                    .filter(|n| *n <= 9_007_199_254_740_991)
                    .ok_or(Error::Integrity)?;
                if a.old_enough != (now.earliest >= deadline)
                    || a.hold.as_deref()
                        != if a.old_enough {
                            None
                        } else {
                            Some("not_old_enough")
                        }
                {
                    return Err(Error::Integrity);
                }
            }
            _ => return Err(Error::Integrity),
        }
    }
    Ok(if row.binding_count == 0 {
        Some(Hold::NoBindings)
    } else if row.age.as_ref().is_some_and(|a| a.evidence.is_none()) {
        Some(Hold::AgeUnknown)
    } else if row.age.as_ref().is_some_and(|a| !a.old_enough) {
        Some(Hold::AgeRecent)
    } else {
        None
    })
}
fn selection_key(namespace: &Digest, source: &Key, revision: u64) -> (Digest, String, u64) {
    (namespace.clone(), source.as_str().to_owned(), revision)
}
fn selection_error(error: selection::Error) -> Error {
    match error {
        selection::Error::Budget | selection::Error::Tree(TreeError::Budget) => Error::Budget,
        selection::Error::Invalid => Error::Integrity,
        _ => Error::Metadata,
    }
}
/// Verify native derivation and exact indexed-request linkage from immutable
/// metadata. Frozen age observations are historical claims, not fresh eligibility.
pub fn inspect(
    e: &Enrollment,
    c: &Candidate,
    nodes: &mut impl NodeStore,
    session: &Session,
) -> Result<Inspection> {
    if c.enrollment != e.id()? {
        return Err(Error::Integrity);
    }
    c.id()?;
    c.paging.validate().map_err(|_| Error::Invalid)?;
    let mut work = session.work(nodes)?;
    let (_, input) = index(&c.input, &mut work)?;
    let filter: Filter = read(&c.filter, &work)?;
    let stored = entries(&c.rows, &mut work)?;
    if stored.len() != c.roots || input.len() != c.roots {
        return Err(Error::Integrity);
    }
    let (mut matched_assets, mut bindings, mut all_bindings) = (0usize, 0usize, 0usize);
    let mut found = BTreeSet::new();
    let mut sizes = BTreeMap::new();
    let mut rows = Vec::new();
    let mut expected = BTreeMap::new();
    for (key, object) in stored {
        let row: Row = read(&object, &work)?;
        let original: Occurrence = read(input.get(&key).ok_or(Error::Integrity)?, &work)?;
        if row.occurrence != original
            || row.source_checkpoint.identity() != &original.namespace
            || key.as_str() != original.id()?.as_str()
            || row.hold != age_hold(e, &row)?
        {
            return Err(Error::Integrity);
        }
        let inv = super::inspect_work(&row.original, &original.header, &mut work)?;
        consistent_sizes(&inv, &mut sizes, session.limits.nodes)?;
        let preview = chosen(&inv, e, &filter, &mut found)?;
        let stored_assets = entries(&row.assets, &mut work)?;
        if stored_assets.len() != row.asset_count || preview.assets.len() != row.asset_count {
            return Err(Error::Integrity);
        }
        for asset in &preview.assets {
            let object = stored_assets
                .get(&path(asset.asset.object.sha256.as_str())?)
                .ok_or(Error::Integrity)?;
            if read::<MatchedAsset>(object, &work)? != *asset {
                return Err(Error::Integrity);
            }
        }
        let map = entries(&row.bindings, &mut work)?;
        if map.len() != row.binding_count
            || preview.paths.len() != row.binding_count
            || preview
                .paths
                .iter()
                .any(|p| map.get(p) != inv.actual.get(p))
        {
            return Err(Error::Integrity);
        }
        matched_assets = matched_assets
            .checked_add(row.asset_count)
            .ok_or(Error::Budget)?;
        all_bindings = all_bindings.checked_add(map.len()).ok_or(Error::Budget)?;
        if matched_assets > session.limits.bindings || all_bindings > session.limits.bindings {
            return Err(Error::Budget);
        }
        if row.hold.is_none() {
            bindings += map.len();
            expected.insert(
                selection_key(
                    &original.namespace,
                    &original.source,
                    original.predecessor_revision,
                ),
                row.bindings.clone(),
            );
        }
        rows.push(row);
    }
    if found != filter.assets
        || matched_assets != c.matched_assets
        || bindings != c.bindings
        || expected.len() != c.selected_roots
    {
        return Err(Error::Integrity);
    }
    if let Some(request) = &c.request {
        let (paging, selected) =
            selection::read_request(request, &mut work, &selection::Limits::default())
                .map_err(selection_error)?;
        let actual = selected
            .iter()
            .map(|s| {
                (
                    selection_key(&s.namespace, &s.source, s.predecessor_revision),
                    s.bindings.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        if paging != c.paging || actual != expected || actual.len() != selected.len() {
            return Err(Error::Integrity);
        }
    }
    work.timed()?;
    Ok(Inspection {
        summary: Summary {
            schema: "tig.native-paged-preview.design-1",
            candidate: c.id()?,
            request: c.request.clone(),
            rows: c.rows.clone(),
            roots: c.roots,
            selected_roots: c.selected_roots,
            matched_assets,
            bindings,
            held_roots: c.roots - c.selected_roots,
            historical_only: true,
            authorizes_claim_release: false,
            authorizes_removal: false,
        },
        rows,
    })
}
/// Require the same source observations before admitting this derivation into
/// a later plan. This does not replace that plan's complete protection roster.
pub fn validate_current(
    e: &Enrollment,
    c: &Candidate,
    namespaces: &mut [Namespace],
    nodes: &mut impl NodeStore,
    session: &Session,
) -> Result<Inspection> {
    let inspected = inspect(e, c, nodes, session)?;
    for row in &inspected.rows {
        let n = namespaces
            .iter_mut()
            .find(|n| n.identity() == &row.occurrence.namespace)
            .ok_or(Error::Incomplete)?;
        n.observe(&row.source_checkpoint)
            .map_err(|_| Error::Incomplete)?;
        let source = n
            .source(&row.occurrence.source)
            .map_err(|_| Error::Incomplete)?
            .ok_or(Error::Incomplete)?;
        if n.checkpoint() != row.source_checkpoint
            || source.owner != e.owner
            || source.pending.is_some()
            || !matches!(source.kind, Some(Kind::Branch | Kind::Draft))
            || source
                .predecessors
                .iter()
                .find(|v| v.revision == row.occurrence.predecessor_revision)
                .is_none_or(|v| v.selection != row.original)
        {
            return Err(Error::Integrity);
        }
    }
    session.work(nodes)?.timed()?;
    Ok(inspected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_asset_core::tree::MemoryNodes;
    #[test]
    fn independent_native_paged_enrollment_occurrence_index_row_and_candidate_frames() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/vectors/native-paged-retirement.design-1.json"
        )))
        .unwrap();
        for case in vectors["cases"].as_array().unwrap() {
            let raw = case["canonical"].as_str().unwrap().as_bytes();
            assert_eq!(Digest::blob(raw).as_str(), case["sha256"]);
            let value = identity::parse_json(raw).unwrap();
            let (encoded, id) = match case["kind"].as_str().unwrap() {
                "enrollment" => {
                    let v: Enrollment = serde_json::from_value(value).unwrap();
                    (encode(&v).unwrap(), v.id().unwrap())
                }
                "occurrence" => {
                    let v: Occurrence = serde_json::from_value(value).unwrap();
                    (encode(&v).unwrap(), v.id().unwrap())
                }
                "index" => {
                    let v: Index = serde_json::from_value(value).unwrap();
                    let raw = encode(&v).unwrap();
                    let id = Digest::blob(&raw);
                    (raw, id)
                }
                "row" => {
                    let v: Row = serde_json::from_value(value).unwrap();
                    let raw = encode(&v).unwrap();
                    let id = Digest::blob(&raw);
                    (raw, id)
                }
                "candidate" => {
                    let v = Candidate::from_json(raw).unwrap();
                    let mut changed = v.clone();
                    changed.paging.max_bindings += 1;
                    assert_ne!(changed.id().unwrap(), v.id().unwrap());
                    changed = v.clone();
                    changed.filter = Object::from_bytes(b"different-exact-filter");
                    assert_ne!(changed.id().unwrap(), v.id().unwrap());
                    let mut malformed = String::from_utf8(raw.to_vec()).unwrap();
                    malformed.insert_str(1, "\"unexpected\":true,");
                    assert!(Candidate::from_json(malformed.as_bytes()).is_err());
                    (v.to_json().unwrap(), v.id().unwrap())
                }
                _ => panic!("unknown frame"),
            };
            assert_eq!(encoded, raw);
            assert_eq!(id.as_str(), case["id"]);
        }
    }
    struct Counted<'a> {
        nodes: &'a mut MemoryNodes,
        reads: Cell<usize>,
        writes: usize,
    }
    impl NodeStore for Counted<'_> {
        fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
            self.reads.set(self.reads.get() + 1);
            self.nodes.get(o)
        }
        fn put(&mut self, b: &[u8]) -> TreeResult<Object> {
            self.writes += 1;
            self.nodes.put(b)
        }
    }
    #[test]
    fn changing_views_or_cloning_sessions_does_not_reset_the_readback_budget() {
        let mut nodes = MemoryNodes::default();
        let session = Session::new(Limits {
            reads: 1,
            ..Default::default()
        })
        .unwrap();
        let input = occurrences(
            None,
            vec![Occurrence {
                namespace: Digest::blob(b"namespace"),
                source: Key::parse("main").unwrap(),
                predecessor_revision: 3,
                header: Digest::blob(b"native-header"),
            }],
            &mut nodes,
            &session,
        )
        .unwrap();
        let mut reader = Counted {
            nodes: &mut nodes,
            reads: Cell::new(0),
            writes: 0,
        };
        assert!(matches!(
            inspect_occurrences(&input, &mut reader, &session),
            Err(Error::Budget)
        ));
        assert_eq!(reader.reads.get(), 1);
        assert!(matches!(
            inspect_occurrences(&input, &mut reader, &session.clone()),
            Err(Error::Budget)
        ));
        assert_eq!(reader.reads.get(), 1);
        assert_eq!(reader.writes, 0);
        assert_eq!(
            inspect_occurrences(
                &input,
                &mut reader,
                &Session::new(Limits::default()).unwrap()
            )
            .unwrap(),
            1
        );
    }
}
