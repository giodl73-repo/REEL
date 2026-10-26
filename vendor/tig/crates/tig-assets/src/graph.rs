//! Exact, bounded declared-reference traversal. No IO or removal authority.
use crate::native::{Instance, MAX_RECORD_BYTES, Pin};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use tig_core::{
    Digest, Object,
    identity::{Error, Result, canonical_json},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Production,
    Composition,
    Evidence,
    Authority,
    Publication,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Primary,
    Output,
    Container,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub id: String,
    pub object: Object,
}

impl Reference {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.id.is_empty() && self.id.len() <= 256 && !self.id.chars().any(char::is_control),
            "invalid reference ID",
        )?;
        require(
            self.object.bytes <= MAX_RECORD_BYTES as u64,
            "reference record byte budget",
        )
    }
}

/// Caller-admitted meaning, never policy obtained from an instance's own status.
#[derive(Clone, Debug, Serialize)]
pub struct Profile {
    pub definition_id: String,
    pub definition: Object,
    pub class: NodeKind,
    pub covered: BTreeSet<EdgeKind>,
    pub rules: Vec<Rule>,
    pub payloads: Payloads,
}

/// Explicit admitted meaning: selected can name a manifest, so it is never
/// automatically treated as all of an asset's primary byte dependencies.
#[derive(Clone, Debug, Serialize)]
pub enum Payloads {
    None,
    SelectedObject,
    Streams {
        roles: BTreeSet<String>,
        groups: BTreeSet<String>,
        min: usize,
    },
    Unsupported,
}

#[derive(Clone, Debug, Serialize)]
pub struct Rule {
    /// RFC 6901 pointer relative to the authored object, not the whole record.
    pub pointer: String,
    pub kind: EdgeKind,
    pub many: bool,
    pub optional: bool,
    pub binding: Binding,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub enum Binding {
    ExactOnly,
    SnapshotSelected,
    Either,
}

impl Profile {
    fn validate(&self) -> Result<()> {
        Reference {
            id: self.definition_id.clone(),
            object: self.definition.clone(),
        }
        .validate()?;
        require(self.rules.len() <= 128, "profile rule budget")?;
        let mut paths = BTreeSet::new();
        for rule in &self.rules {
            pointer(&rule.pointer)?;
            require(paths.insert(&rule.pointer), "duplicate reference pointer")?;
            require(
                self.covered.contains(&rule.kind),
                "rule kind missing from coverage",
            )?;
        }
        if let Payloads::Streams { roles, groups, min } = &self.payloads {
            require(
                !roles.is_empty()
                    && !groups.is_empty()
                    && roles.len() <= 128
                    && groups.len() <= 128
                    && *min <= 256,
                "invalid payload stream selector",
            )?;
            require(
                roles
                    .iter()
                    .chain(groups)
                    .all(|s| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control)),
                "invalid payload selector token",
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub nodes: usize,
    pub edges: usize,
    pub depth: usize,
    pub bytes: usize,
}

impl Limits {
    fn validate(&self) -> Result<()> {
        require(
            (1..=500_000).contains(&self.nodes)
                && (1..=2_000_000).contains(&self.edges)
                && (1..=128).contains(&self.depth)
                && (1..=268_435_456).contains(&self.bytes),
            "invalid graph limits",
        )
    }
}

#[derive(Clone, Debug)]
pub struct Query {
    pub checkpoint: Digest,
    pub roots: BTreeSet<Reference>,
    pub kinds: BTreeSet<EdgeKind>,
    pub limits: Limits,
}

/// A bounded provider response. The provider is responsible for limiting its own
/// allocation/IO; the engine checks returned lengths, identities and checkpoint.
pub enum Data {
    Record {
        instance: Vec<u8>,
        definition: Vec<u8>,
        schema: Vec<u8>,
    },
    Missing,
    Unavailable,
    Unsupported,
    Denied,
}

pub struct Lookup {
    pub checkpoint: Digest,
    pub data: Data,
}

pub enum Selection {
    Found(Reference),
    Missing,
    Unavailable,
    Unsupported,
    Denied,
    Ambiguous,
}
pub struct SelectionLookup {
    pub checkpoint: Digest,
    pub selection: Selection,
}

pub trait Source {
    fn lookup(&mut self, reference: &Reference, remaining_bytes: usize) -> Lookup;
    /// Resolve only within the enrolled immutable view; never host-global latest.
    fn select(&mut self, id: &str, remaining_bytes: usize) -> SelectionLookup;
}

enum Target {
    Exact(Reference),
    Selected(String),
}
struct RawEdge {
    pointer: String,
    kind: EdgeKind,
    target: Target,
}

#[derive(Clone, Debug, Serialize)]
pub struct Node {
    pub reference: Reference,
    pub class: NodeKind,
    pub selected: Option<Object>,
    pub primary_objects: Option<BTreeSet<Object>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Edge {
    pub from: Reference,
    pub pointer: String,
    pub kind: EdgeKind,
    pub target: Reference,
    pub snapshot_selected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum GapReason {
    Missing,
    Unavailable,
    Unsupported,
    Denied,
    CheckpointChanged,
    InvalidRecord,
    UnadmittedDefinition,
    UncoveredKind,
    InvalidReference,
    Nodes,
    Edges,
    Depth,
    Bytes,
    OutputAsPrimary,
    ConflictingSizes,
    Cycle,
    AmbiguousSelection,
}

#[derive(Clone, Debug, Serialize)]
pub struct Gap {
    pub at: Reference,
    pub reason: GapReason,
}

/// Immutable scoped result. It is not global source/job or retained-custody proof.
#[derive(Debug)]
pub struct Graph {
    checkpoint: Digest,
    admission: Digest,
    roots: BTreeSet<Reference>,
    kinds: BTreeSet<EdgeKind>,
    nodes: BTreeMap<Reference, Node>,
    edges: Vec<Edge>,
    gaps: Vec<Gap>,
    bytes_read: usize,
    edge_attempts: usize,
    sizes: BTreeMap<Digest, u64>,
    binding_digest: Option<Digest>,
}

impl Graph {
    pub fn checkpoint(&self) -> &Digest {
        &self.checkpoint
    }
    pub fn admission(&self) -> &Digest {
        &self.admission
    }
    pub fn roots(&self) -> &BTreeSet<Reference> {
        &self.roots
    }
    pub fn kinds(&self) -> &BTreeSet<EdgeKind> {
        &self.kinds
    }
    /// Full declared binding identity, not a consumed-projection recipe or approval.
    /// The unrelated source checkpoint is deliberately excluded from this hash.
    pub fn binding_digest(&self) -> Option<&Digest> {
        self.binding_digest.as_ref()
    }
    pub fn nodes(&self) -> &BTreeMap<Reference, Node> {
        &self.nodes
    }
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }
    pub fn gaps(&self) -> &[Gap] {
        &self.gaps
    }
    /// Returned metadata bytes, including refused data; not measured physical IO.
    pub fn bytes_read(&self) -> usize {
        self.bytes_read
    }
    pub fn complete_declared_coverage(&self) -> bool {
        self.gaps.is_empty()
    }
    pub fn complete_semantic_coverage(&self) -> bool {
        false
    }

    fn gap(&mut self, at: &Reference, reason: GapReason) {
        self.gaps.push(Gap {
            at: at.clone(),
            reason,
        });
    }

    fn primary_payloads(&self) -> BTreeSet<Object> {
        self.nodes
            .values()
            .filter(|n| n.class == NodeKind::Primary)
            .flat_map(|n| {
                n.primary_objects
                    .iter()
                    .flat_map(|objects| objects.iter().cloned())
            })
            .collect()
    }

    fn check_size(&mut self, reference: &Reference, object: &Object) {
        if self
            .sizes
            .insert(object.sha256.clone(), object.bytes)
            .is_some_and(|n| n != object.bytes)
        {
            self.gap(reference, GapReason::ConflictingSizes);
        }
    }
}

/// Scoped query difference only; neither candidate set authorizes any eviction.
#[derive(Debug, PartialEq, Eq)]
pub struct PrimaryChange {
    pub before: Digest,
    pub after: Digest,
    pub admission: Digest,
    pub added: BTreeSet<Object>,
    pub shared: BTreeSet<Object>,
    pub unreferenced_by_new_query: BTreeSet<Object>,
}

pub fn primary_change(old: &Graph, new: &Graph) -> Result<PrimaryChange> {
    let production = BTreeSet::from([EdgeKind::Production]);
    require(
        old.complete_declared_coverage()
            && new.complete_declared_coverage()
            && old.kinds == production
            && new.kinds == production,
        "complete Production-only queries required",
    )?;
    require(
        old.admission == new.admission,
        "same admitted reference policy required",
    )?;
    require(
        old.nodes
            .values()
            .chain(new.nodes.values())
            .all(|n| n.class != NodeKind::Primary || n.primary_objects.is_some()),
        "primary payload selections missing",
    )?;
    let before = old.primary_payloads();
    let after = new.primary_payloads();
    Ok(PrimaryChange {
        before: old.checkpoint.clone(),
        after: new.checkpoint.clone(),
        admission: old.admission.clone(),
        added: after.difference(&before).cloned().collect(),
        shared: after.intersection(&before).cloned().collect(),
        unreferenced_by_new_query: before.difference(&after).cloned().collect(),
    })
}

pub fn resolve(source: &mut impl Source, query: &Query, profiles: &[Profile]) -> Result<Graph> {
    query.limits.validate()?;
    require(
        !query.roots.is_empty()
            && query.roots.len() <= query.limits.nodes
            && !query.kinds.is_empty()
            && profiles.len() <= 1024,
        "invalid graph query",
    )?;
    let mut admitted = BTreeMap::new();
    for profile in profiles {
        profile.validate()?;
        require(
            admitted.insert(&profile.definition, profile).is_none(),
            "duplicate definition profile",
        )?;
    }
    let admission = Digest::blob(&canonical_json(&serde_json::json!({
        "schema":"tig.native-profile-set.experimental-1",
        "profiles":admitted.values().collect::<Vec<_>>()
    }))?);
    let mut graph = Graph {
        checkpoint: query.checkpoint.clone(),
        admission,
        roots: query.roots.clone(),
        kinds: query.kinds.clone(),
        nodes: BTreeMap::new(),
        edges: vec![],
        gaps: vec![],
        bytes_read: 0,
        edge_attempts: 0,
        sizes: BTreeMap::new(),
        binding_digest: None,
    };
    let mut seen = query.roots.clone();
    for root in &seen {
        root.validate()?;
    }
    let mut pending: VecDeque<_> = seen.iter().cloned().map(|r| (r, 0)).collect();
    let mut selections = BTreeMap::new();
    while let Some((reference, depth)) = pending.pop_front() {
        let edges = visit(source, query, &admitted, &reference, &mut graph);
        for raw in edges {
            if graph.edge_attempts == query.limits.edges {
                graph.gap(&reference, GapReason::Edges);
                break;
            }
            graph.edge_attempts += 1;
            let target =
                match resolve_target(source, query, &raw.target, &mut graph, &mut selections) {
                    Ok(target) => target,
                    Err(reason) => {
                        graph.gap(&reference, reason);
                        continue;
                    }
                };
            graph.check_size(&target, &target.object);
            graph.edges.push(Edge {
                from: reference.clone(),
                pointer: raw.pointer,
                kind: raw.kind,
                target: target.clone(),
                snapshot_selected: matches!(&raw.target, Target::Selected(_)),
            });
            if seen.contains(&target) {
                continue;
            }
            if depth == query.limits.depth {
                graph.gap(&target, GapReason::Depth);
                continue;
            }
            if seen.len() == query.limits.nodes {
                graph.gap(&target, GapReason::Nodes);
                continue;
            }
            seen.insert(target.clone());
            pending.push_back((target, depth + 1));
        }
    }
    let bad: BTreeSet<_> = graph
        .edges
        .iter()
        .filter(|e| {
            e.kind == EdgeKind::Production
                && graph
                    .nodes
                    .get(&e.target)
                    .is_some_and(|n| n.class == NodeKind::Output)
        })
        .map(|e| e.from.clone())
        .collect();
    for reference in bad {
        graph.gap(&reference, GapReason::OutputAsPrimary);
    }
    if let Some(reference) = build_cycle(&graph) {
        graph.gap(&reference, GapReason::Cycle);
    }
    if graph.complete_declared_coverage() {
        graph.binding_digest = Some(binding_digest(&graph)?);
    }
    Ok(graph)
}

/// Iterative Kahn elimination. Evidence/authority/publication relationships may
/// cycle; production/composition build relationships cannot form a build DAG.
fn build_cycle(graph: &Graph) -> Option<Reference> {
    let mut degrees: BTreeMap<_, usize> = graph.nodes.keys().map(|r| (r, 0)).collect();
    let mut outgoing: BTreeMap<&Reference, Vec<&Reference>> = BTreeMap::new();
    for edge in &graph.edges {
        if !matches!(edge.kind, EdgeKind::Production | EdgeKind::Composition)
            || !graph.nodes.contains_key(&edge.from)
            || !graph.nodes.contains_key(&edge.target)
        {
            continue;
        }
        if let Some(degree) = degrees.get_mut(&edge.target) {
            *degree += 1;
        }
        outgoing.entry(&edge.from).or_default().push(&edge.target);
    }
    let mut ready: VecDeque<_> = degrees
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(r, _)| *r)
        .collect();
    while let Some(from) = ready.pop_front() {
        if let Some(targets) = outgoing.get(from) {
            for target in targets {
                if let Some(degree) = degrees.get_mut(target) {
                    *degree -= 1;
                    if *degree == 0 {
                        ready.push_back(target);
                    }
                }
            }
        }
    }
    degrees
        .into_iter()
        .find(|(_, degree)| *degree > 0)
        .map(|(r, _)| r.clone())
}

fn binding_digest(graph: &Graph) -> Result<Digest> {
    fn feed(hash: &mut Sha256, value: &impl Serialize) -> Result<()> {
        let value =
            serde_json::to_value(value).map_err(|_| Error("invalid graph hash value".into()))?;
        let bytes = canonical_json(&value)?;
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
        Ok(())
    }
    let mut hash = Sha256::new();
    hash.update(b"TIG\0native-binding\0experimental-1\0");
    feed(&mut hash, &graph.admission)?;
    feed(&mut hash, &graph.kinds)?;
    hash.update((graph.roots.len() as u64).to_be_bytes());
    for root in &graph.roots {
        feed(&mut hash, root)?;
    }
    hash.update((graph.nodes.len() as u64).to_be_bytes());
    for node in graph.nodes.values() {
        feed(&mut hash, node)?;
    }
    hash.update((graph.edges.len() as u64).to_be_bytes());
    for edge in &graph.edges {
        feed(&mut hash, edge)?;
    }
    Ok(Digest::from_sha256(hash.finalize().into()))
}

fn visit(
    source: &mut impl Source,
    query: &Query,
    profiles: &BTreeMap<&Object, &Profile>,
    reference: &Reference,
    graph: &mut Graph,
) -> Vec<RawEdge> {
    let remaining = query.limits.bytes.saturating_sub(graph.bytes_read);
    if remaining == 0 {
        graph.gap(reference, GapReason::Bytes);
        return vec![];
    }
    let lookup = source.lookup(reference, remaining);
    let (cost, bounded) = response_size(&lookup.data);
    graph.bytes_read = graph.bytes_read.saturating_add(cost);
    if !bounded || cost > remaining {
        graph.gap(reference, GapReason::Bytes);
        return vec![];
    }
    if lookup.checkpoint != query.checkpoint {
        graph.gap(reference, GapReason::CheckpointChanged);
        return vec![];
    }
    match lookup.data {
        Data::Record {
            instance,
            definition,
            schema,
        } => {
            match extract(
                reference,
                &instance,
                &definition,
                &schema,
                &query.kinds,
                profiles,
                query.limits.edges.saturating_sub(graph.edge_attempts),
            ) {
                Ok((node, edges, covered)) => {
                    graph.check_size(reference, &node.reference.object);
                    if let Some(object) = &node.selected {
                        graph.check_size(reference, object);
                    }
                    if let Some(objects) = &node.primary_objects {
                        for object in objects {
                            graph.check_size(reference, object);
                        }
                    }
                    graph.nodes.insert(reference.clone(), node);
                    if !covered {
                        graph.gap(reference, GapReason::UncoveredKind);
                    }
                    edges
                }
                Err(reason) => {
                    graph.gap(reference, reason);
                    vec![]
                }
            }
        }
        Data::Missing => {
            graph.gap(reference, GapReason::Missing);
            vec![]
        }
        Data::Unavailable => {
            graph.gap(reference, GapReason::Unavailable);
            vec![]
        }
        Data::Unsupported => {
            graph.gap(reference, GapReason::Unsupported);
            vec![]
        }
        Data::Denied => {
            graph.gap(reference, GapReason::Denied);
            vec![]
        }
    }
}

fn response_size(data: &Data) -> (usize, bool) {
    if let Data::Record {
        instance,
        definition,
        schema,
    } = data
    {
        let sizes = [instance.len(), definition.len(), schema.len()];
        (
            sizes.iter().fold(0usize, |n, size| n.saturating_add(*size)),
            sizes.iter().all(|size| *size <= MAX_RECORD_BYTES),
        )
    } else {
        (0, true)
    }
}

fn extract(
    reference: &Reference,
    instance: &[u8],
    definition: &[u8],
    schema: &[u8],
    kinds: &BTreeSet<EdgeKind>,
    profiles: &BTreeMap<&Object, &Profile>,
    edge_budget: usize,
) -> std::result::Result<(Node, Vec<RawEdge>, bool), GapReason> {
    let parsed = Instance::parse(instance).map_err(|_| GapReason::InvalidRecord)?;
    if parsed.object() != &reference.object || parsed.value().id != reference.id {
        return Err(GapReason::InvalidRecord);
    }
    let key = parsed.value().definition.object();
    let profile = profiles.get(&key).ok_or(GapReason::UnadmittedDefinition)?;
    let bound = parsed
        .bind(definition, schema)
        .map_err(|_| GapReason::InvalidRecord)?;
    if bound.definition().value().id != profile.definition_id {
        return Err(GapReason::UnadmittedDefinition);
    }
    let authored = Value::Object(parsed.value().authored.clone());
    let mut edges = Vec::new();
    for rule in &profile.rules {
        let Some(value) = authored.pointer(&rule.pointer) else {
            if rule.optional {
                continue;
            } else {
                return Err(GapReason::InvalidReference);
            }
        };
        if rule.many {
            let values = value.as_array().ok_or(GapReason::InvalidReference)?;
            for (i, item) in values.iter().enumerate() {
                let target = target(item, rule.binding)?;
                if kinds.contains(&rule.kind) {
                    if edges.len() == edge_budget {
                        return Err(GapReason::Edges);
                    }
                    edges.push(RawEdge {
                        pointer: format!("{}/{i}", rule.pointer),
                        kind: rule.kind,
                        target,
                    });
                }
            }
        } else {
            let target = target(value, rule.binding)?;
            if kinds.contains(&rule.kind) {
                if edges.len() == edge_budget {
                    return Err(GapReason::Edges);
                }
                edges.push(RawEdge {
                    pointer: rule.pointer.clone(),
                    kind: rule.kind,
                    target,
                });
            }
        }
    }
    Ok((
        Node {
            reference: reference.clone(),
            class: profile.class,
            selected: parsed.value().selected.as_ref().map(Pin::object),
            primary_objects: primary_objects(parsed.value(), profile),
        },
        edges,
        kinds.is_subset(&profile.covered),
    ))
}

fn primary_objects(instance: &Instance, profile: &Profile) -> Option<BTreeSet<Object>> {
    if profile.class != NodeKind::Primary {
        return Some(BTreeSet::new());
    }
    match &profile.payloads {
        Payloads::None => Some(BTreeSet::new()),
        Payloads::Unsupported => None,
        Payloads::SelectedObject => instance
            .selected
            .as_ref()
            .map(|p| BTreeSet::from([p.object()])),
        Payloads::Streams { roles, groups, min } => {
            let matches: Vec<_> = instance
                .streams
                .iter()
                .filter(|s| roles.contains(&s.role) && groups.contains(&s.group))
                .collect();
            (matches.len() >= *min)
                .then(|| matches.into_iter().map(|s| s.object.object()).collect())
        }
    }
}

fn target(value: &Value, binding: Binding) -> std::result::Result<Target, GapReason> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Exact {
        id: String,
        sha256: Digest,
        bytes: u64,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Selected {
        id: String,
    }
    if let Ok(pin) = serde_json::from_value::<Exact>(value.clone()) {
        if matches!(binding, Binding::SnapshotSelected) {
            return Err(GapReason::InvalidReference);
        }
        let reference = Reference {
            id: pin.id,
            object: Object {
                sha256: pin.sha256,
                bytes: pin.bytes,
            },
        };
        reference
            .validate()
            .map_err(|_| GapReason::InvalidReference)?;
        Ok(Target::Exact(reference))
    } else {
        if matches!(binding, Binding::ExactOnly) {
            return Err(GapReason::InvalidReference);
        }
        let selected: Selected =
            serde_json::from_value(value.clone()).map_err(|_| GapReason::InvalidReference)?;
        require(
            !selected.id.is_empty()
                && selected.id.len() <= 256
                && !selected.id.chars().any(char::is_control),
            "invalid selected ID",
        )
        .map_err(|_| GapReason::InvalidReference)?;
        Ok(Target::Selected(selected.id))
    }
}

fn resolve_target(
    source: &mut impl Source,
    query: &Query,
    target: &Target,
    graph: &mut Graph,
    selections: &mut BTreeMap<String, std::result::Result<Reference, GapReason>>,
) -> std::result::Result<Reference, GapReason> {
    let id = match target {
        Target::Selected(id) => id,
        Target::Exact(reference) => return Ok(reference.clone()),
    };
    if let Some(result) = selections.get(id) {
        return result.clone();
    }
    let result = select(source, query, id, graph);
    selections.insert(id.clone(), result.clone());
    result
}

fn select(
    source: &mut impl Source,
    query: &Query,
    id: &str,
    graph: &mut Graph,
) -> std::result::Result<Reference, GapReason> {
    let remaining = query.limits.bytes.saturating_sub(graph.bytes_read);
    if remaining == 0 {
        return Err(GapReason::Bytes);
    }
    let lookup = source.select(id, remaining);
    let result = match lookup.selection {
        Selection::Found(reference) => {
            reference
                .validate()
                .map_err(|_| GapReason::InvalidReference)?;
            if reference.id != id {
                return Err(GapReason::InvalidReference);
            }
            let encoded =
                serde_json::to_vec(&reference).map_err(|_| GapReason::InvalidReference)?;
            graph.bytes_read = graph.bytes_read.saturating_add(encoded.len());
            if encoded.len() > remaining {
                return Err(GapReason::Bytes);
            }
            Ok(reference)
        }
        Selection::Missing => Err(GapReason::Missing),
        Selection::Unavailable => Err(GapReason::Unavailable),
        Selection::Unsupported => Err(GapReason::Unsupported),
        Selection::Denied => Err(GapReason::Denied),
        Selection::Ambiguous => Err(GapReason::AmbiguousSelection),
    };
    if lookup.checkpoint != query.checkpoint {
        Err(GapReason::CheckpointChanged)
    } else {
        result
    }
}

fn pointer(value: &str) -> Result<()> {
    require(
        value.starts_with('/') && value.len() <= 256 && !value.chars().any(char::is_control),
        "invalid reference pointer",
    )?;
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '~' {
            require(
                matches!(chars.next(), Some('0' | '1')),
                "invalid pointer escape",
            )?;
        }
    }
    Ok(())
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error(message.into()))
    }
}
