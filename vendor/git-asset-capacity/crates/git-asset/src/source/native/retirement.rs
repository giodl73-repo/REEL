//! Native storage closures mapped to exact flat bindings. No removal authority.
use super::{AssetSnapshot, Limits as NativeLimits, archive};
use git_asset_core::{
    Digest, LogicalPath, Object, identity,
    tree::{NodeStore, Tree, TreeError, TreeLimits, TreeResult},
};
use git_asset_model::{
    graph::{self, EdgeKind, Profile, Reference},
    native::Instance,
    resolve::AssetResolver,
};
use git_asset_store::source_claims::{Selection, subset};
use serde::{Deserialize, Serialize};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    ops::Deref,
    rc::Rc,
    time::{Duration, Instant},
};
pub mod paged;

const HEADER: &str = "native-retirement";
const SCHEMA: &str = "tig.native-retirement-bindings.design-1";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Budget,
    Incomplete,
    Integrity,
    Metadata,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
fn tree_error(e: TreeError) -> Error {
    if matches!(e, TreeError::Budget) {
        Error::Budget
    } else {
        Error::Metadata
    }
}
fn archive_error(e: super::Error) -> Error {
    if e == super::Error::Budget {
        Error::Budget
    } else {
        Error::Metadata
    }
}
fn path(s: &str) -> Result<LogicalPath> {
    LogicalPath::parse(s).map_err(|_| Error::Invalid)
}
fn encode(v: &impl Serialize) -> Result<Vec<u8>> {
    let raw = identity::canonical_json(&serde_json::to_value(v).map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)?;
    if raw.len() > 65_536 {
        return Err(Error::Budget);
    }
    Ok(raw)
}
#[derive(Clone, Copy, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub assets: usize,
    pub bindings: usize,
    pub reads: usize,
    pub writes: usize,
    pub bytes: usize,
    pub nodes: usize,
    pub edges: usize,
    pub millis: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            assets: 1024,
            bindings: 65_536,
            reads: 100_000,
            writes: 100_000,
            bytes: 128 * 1024 * 1024,
            nodes: 65_536,
            edges: 262_144,
            millis: 60_000,
        }
    }
}
impl Limits {
    pub fn validate(self) -> Result<()> {
        if !(1..=1024).contains(&self.assets)
            || !(1..=65_536).contains(&self.bindings)
            || !(1..=1_000_000).contains(&self.reads)
            || self.writes > 1_000_000
            || !(1..=268_435_456).contains(&self.bytes)
            || !(1..=500_000).contains(&self.nodes)
            || !(1..=2_000_000).contains(&self.edges)
            || !(1..=300_000).contains(&self.millis)
        {
            return Err(Error::Budget);
        }
        Ok(())
    }
    fn tree(self) -> TreeLimits {
        TreeLimits {
            max_entries: self.bindings,
            max_reads: self.reads,
            max_writes: self.writes,
            max_read_bytes: self.bytes,
            max_write_bytes: self.bytes,
            max_map_depth: 64,
        }
    }
}
struct Work<'a, S> {
    store: &'a mut S,
    limits: Limits,
    meter: Rc<Meter>,
}
struct Meter {
    start: Instant,
    reads: Cell<usize>,
    writes: Cell<usize>,
    bytes: Cell<usize>,
}
impl Meter {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            start: Instant::now(),
            reads: Cell::new(0),
            writes: Cell::new(0),
            bytes: Cell::new(0),
        })
    }
}
impl<S> Deref for Work<'_, S> {
    type Target = Meter;
    fn deref(&self) -> &Meter {
        &self.meter
    }
}
impl<'a, S> Work<'a, S> {
    fn new(store: &'a mut S, limits: Limits) -> Result<Self> {
        Self::shared(store, limits, Meter::new())
    }
    fn shared(store: &'a mut S, limits: Limits, meter: Rc<Meter>) -> Result<Self> {
        limits.validate()?;
        Ok(Self {
            store,
            limits,
            meter,
        })
    }
    fn timed(&self) -> Result<()> {
        if Instant::now()
            .checked_duration_since(self.start)
            .is_none_or(|age| age > Duration::from_millis(self.limits.millis))
        {
            Err(Error::Budget)
        } else {
            Ok(())
        }
    }
    fn charge(&self, bytes: usize) -> TreeResult<()> {
        self.timed().map_err(|_| TreeError::Budget)?;
        let n = self
            .bytes
            .get()
            .checked_add(bytes)
            .ok_or(TreeError::Budget)?;
        if n > self.limits.bytes {
            return Err(TreeError::Budget);
        }
        self.bytes.set(n);
        Ok(())
    }
}
impl<S: NodeStore> NodeStore for Work<'_, S> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        let n = self.reads.get().checked_add(1).ok_or(TreeError::Budget)?;
        if n > self.limits.reads {
            return Err(TreeError::Budget);
        }
        self.reads.set(n);
        self.charge(usize::try_from(object.bytes).map_err(|_| TreeError::Budget)?)?;
        let raw = self.store.get(object)?;
        if raw
            .as_ref()
            .is_some_and(|raw| Object::from_bytes(raw) != *object)
        {
            return Err(TreeError::InvalidNode);
        }
        Ok(raw)
    }
    fn put(&mut self, raw: &[u8]) -> TreeResult<Object> {
        let writes = self.writes.get().checked_add(1).ok_or(TreeError::Budget)?;
        if writes > self.limits.writes {
            return Err(TreeError::Budget);
        }
        self.writes.set(writes);
        self.charge(raw.len())?;
        let actual = self.store.put(raw)?;
        if actual != Object::from_bytes(raw) {
            return Err(TreeError::InvalidNode);
        }
        Ok(actual)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: String,
    checkpoint: Digest,
    snapshot: Object,
    profiles: Object,
    roots: Object,
    assets: usize,
}
#[derive(Clone)]
struct Asset {
    reference: Reference,
    definition: Reference,
    paths: BTreeMap<LogicalPath, Object>,
}
pub struct Inventory {
    header: Digest,
    checkpoint: Digest,
    assets: Vec<Asset>,
    actual: BTreeMap<LogicalPath, Object>,
}
impl Inventory {
    pub fn header(&self) -> &Digest {
        &self.header
    }
    pub fn checkpoint(&self) -> &Digest {
        &self.checkpoint
    }
    pub fn assets(&self) -> usize {
        self.assets.len()
    }
}
fn binding(root: &Reference, object: &Object) -> Result<LogicalPath> {
    path(&format!(
        "assets/{}/{}",
        root.object.sha256.as_str(),
        object.sha256.as_str()
    ))
}
fn compile(
    snapshot: &mut AssetSnapshot,
    roots: &BTreeSet<Reference>,
    profiles: &[Profile],
    work: &Work<'_, impl NodeStore>,
) -> Result<Vec<Asset>> {
    let limits = work.limits;
    if roots.is_empty() || roots.len() > limits.assets {
        return Err(Error::Budget);
    }
    let (mut nodes, mut edges, mut bytes) = (0usize, 0usize, 0usize);
    let mut bindings = 1usize; // The independently retained header binding.
    let mut sizes = BTreeMap::new();
    let mut result = Vec::new();
    for root in roots {
        root.validate().map_err(|_| Error::Invalid)?;
        let resolved = snapshot
            .resolve_id(&root.id)
            .map_err(|_| Error::Incomplete)?;
        if resolved.reference() != root {
            return Err(Error::Integrity);
        }
        work.timed()?;
        if nodes >= limits.nodes || work.bytes.get() >= limits.bytes {
            return Err(Error::Budget);
        }
        let query = graph::Query {
            checkpoint: snapshot.checkpoint().clone(),
            roots: BTreeSet::from([root.clone()]),
            kinds: BTreeSet::from([EdgeKind::Production, EdgeKind::Composition]),
            limits: graph::Limits {
                nodes: limits.nodes - nodes,
                edges: limits.edges.saturating_sub(edges).max(1),
                depth: 128,
                bytes: limits.bytes - work.bytes.get(),
            },
        };
        let graph = graph::resolve(snapshot, &query, profiles).map_err(|_| Error::Invalid)?;
        nodes = nodes
            .checked_add(graph.nodes().len())
            .ok_or(Error::Budget)?;
        edges = edges
            .checked_add(graph.edges().len())
            .ok_or(Error::Budget)?;
        bytes = bytes.checked_add(graph.bytes_read()).ok_or(Error::Budget)?;
        work.charge(graph.bytes_read()).map_err(tree_error)?;
        if nodes > limits.nodes || edges > limits.edges || bytes > limits.bytes {
            return Err(Error::Budget);
        }
        let objects = graph.storage_objects().map_err(|_| Error::Incomplete)?;
        let mut paths = BTreeMap::new();
        for object in objects {
            if sizes
                .insert(object.sha256.clone(), object.bytes)
                .is_some_and(|n| n != object.bytes)
            {
                return Err(Error::Integrity);
            }
            paths.insert(binding(root, &object)?, object);
        }
        bindings = bindings.checked_add(paths.len()).ok_or(Error::Budget)?;
        if bindings > limits.bindings {
            return Err(Error::Budget);
        }
        result.push(Asset {
            reference: root.clone(),
            definition: resolved.definition().clone(),
            paths,
        });
    }
    Ok(result)
}
/// Candidate metadata only: finish/readback and source adoption remain separate.
pub fn build(
    snapshot: &mut AssetSnapshot,
    roots: BTreeSet<Reference>,
    profiles: Vec<Profile>,
    store: &mut impl NodeStore,
    limits: Limits,
) -> Result<Selection> {
    let mut work = Work::new(store, limits)?;
    build_in(snapshot, roots, profiles, &mut work)
}
/// Includes acquisition of the admitted source archive in the same construction
/// budget, rather than resetting the budget after source restoration.
pub fn build_from_archive(
    archive: &Object,
    roots: BTreeSet<Reference>,
    profiles: Vec<Profile>,
    store: &mut impl NodeStore,
    limits: Limits,
) -> Result<Selection> {
    let mut work = Work::new(store, limits)?;
    let mut snapshot = AssetSnapshot::from_metadata_archive(
        &mut work,
        archive,
        NativeLimits {
            documents: 65_536,
            bytes: limits.bytes.min(128 * 1024 * 1024),
            millis: limits.millis,
        },
        limits.tree(),
    )
    .map_err(|e| {
        if e == super::Error::Budget {
            Error::Budget
        } else {
            Error::Incomplete
        }
    })?;
    build_in(&mut snapshot, roots, profiles, &mut work)
}
fn build_in(
    snapshot: &mut AssetSnapshot,
    roots: BTreeSet<Reference>,
    mut profiles: Vec<Profile>,
    work: &mut Work<'_, impl NodeStore>,
) -> Result<Selection> {
    let limits = work.limits;
    if profiles.len() > 128 {
        return Err(Error::Budget);
    }
    profiles
        .sort_by(|a, b| (&a.definition_id, &a.definition).cmp(&(&b.definition_id, &b.definition)));
    let raw = encode(&profiles)?;
    let assets = compile(snapshot, &roots, &profiles, work)?;
    let archive = snapshot
        .archive_metadata(work, limits.tree())
        .map_err(|e| {
            if e == super::Error::Budget {
                Error::Budget
            } else {
                Error::Metadata
            }
        })?;
    let profiles = work.put(&raw).map_err(tree_error)?;
    let index = roots
        .iter()
        .map(|r| Ok((path(r.object.sha256.as_str())?, r.object.clone())))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let roots = Tree::build(&index, work, limits.tree())
        .map_err(tree_error)?
        .0
        .root()
        .clone();
    let header = Header {
        schema: SCHEMA.into(),
        checkpoint: snapshot.checkpoint().clone(),
        snapshot: archive,
        profiles,
        roots,
        assets: assets.len(),
    };
    let header = work.put(&encode(&header)?).map_err(tree_error)?;
    let mut entries = BTreeMap::from([(path(HEADER)?, header.clone())]);
    for asset in assets {
        entries.extend(asset.paths);
    }
    let root = Tree::build(&entries, work, limits.tree())
        .map_err(tree_error)?
        .0
        .root()
        .clone();
    work.timed()?;
    Ok(Selection {
        root,
        context: header.sha256,
    })
}
/// Reads a privately admitted header and a current exact flat root. Retained
/// subsets may omit original paths, but no foreign or changed binding is accepted.
pub fn inspect(
    selection: &Selection,
    admitted_header: &Digest,
    store: &mut impl NodeStore,
    limits: Limits,
) -> Result<Inventory> {
    let mut work = Work::new(store, limits)?;
    inspect_work(selection, admitted_header, &mut work)
}
fn inspect_work(
    selection: &Selection,
    admitted_header: &Digest,
    work: &mut Work<'_, impl NodeStore>,
) -> Result<Inventory> {
    let limits = work.limits;
    let actual = Tree::from_root(selection.root.clone())
        .entries(&mut *work, limits.tree())
        .map_err(tree_error)?
        .0;
    let header = actual.get(&path(HEADER)?).ok_or(Error::Incomplete)?;
    if &header.sha256 != admitted_header {
        return Err(Error::Integrity);
    }
    let raw = archive::read(&*work, header, 65_536).map_err(archive_error)?;
    let decoded: Header =
        serde_json::from_value(identity::parse_json(&raw).map_err(|_| Error::Integrity)?)
            .map_err(|_| Error::Integrity)?;
    if encode(&decoded)? != raw {
        return Err(Error::Integrity);
    }
    if decoded.schema != SCHEMA || decoded.assets == 0 || decoded.assets > limits.assets {
        return Err(Error::Invalid);
    }
    let mut snapshot = AssetSnapshot::from_metadata_archive(
        &mut *work,
        &decoded.snapshot,
        NativeLimits {
            documents: 65_536,
            bytes: limits.bytes.min(128 * 1024 * 1024),
            millis: limits.millis,
        },
        limits.tree(),
    )
    .map_err(|e| {
        if e == super::Error::Budget {
            Error::Budget
        } else {
            Error::Incomplete
        }
    })?;
    if snapshot.checkpoint() != &decoded.checkpoint {
        return Err(Error::Integrity);
    }
    let raw = archive::read(&*work, &decoded.profiles, 65_536).map_err(archive_error)?;
    let profiles: Vec<Profile> =
        serde_json::from_value(identity::parse_json(&raw).map_err(|_| Error::Integrity)?)
            .map_err(|_| Error::Integrity)?;
    if profiles.len() > 128 || encode(&profiles)? != raw {
        return Err(Error::Integrity);
    }
    let index = Tree::from_root(decoded.roots)
        .entries(&mut *work, limits.tree())
        .map_err(tree_error)?
        .0;
    if index.len() != decoded.assets {
        return Err(Error::Integrity);
    }
    let mut roots = BTreeSet::new();
    for (key, object) in index {
        if key != path(object.sha256.as_str())? {
            return Err(Error::Integrity);
        }
        let raw = archive::read(&*work, &object, 65_536).map_err(|e| {
            if e == super::Error::Budget {
                Error::Budget
            } else {
                Error::Incomplete
            }
        })?;
        let record = Instance::parse(&raw).map_err(|_| Error::Integrity)?;
        roots.insert(Reference {
            id: record.value().id.clone(),
            object,
        });
    }
    let assets = compile(&mut snapshot, &roots, &profiles, &*work)?;
    let mut allowed = BTreeMap::from([(path(HEADER)?, header.clone())]);
    for asset in &assets {
        allowed.extend(asset.paths.clone());
    }
    if actual.iter().any(|(p, o)| allowed.get(p) != Some(o)) {
        return Err(Error::Integrity);
    }
    work.timed()?;
    Ok(Inventory {
        header: header.sha256.clone(),
        checkpoint: decoded.checkpoint,
        assets,
        actual,
    })
}

pub type Categories = BTreeMap<String, BTreeSet<Reference>>;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub assets: BTreeSet<Reference>,
    pub categories: BTreeSet<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchedAsset {
    pub asset: Reference,
    pub definition: Reference,
    pub original_bindings: usize,
    pub remaining_bindings: usize,
}
#[derive(Serialize)]
pub struct Preview {
    pub schema: &'static str,
    pub header: Digest,
    pub source_checkpoint: Digest,
    pub assets: Vec<MatchedAsset>,
    pub paths: Vec<LogicalPath>,
    pub objects: BTreeSet<Object>,
    pub age_filter_supported: bool,
    pub complete_semantic_coverage: bool,
    pub payload_verified: bool,
    pub authorizes_removal: bool,
}
impl Preview {
    pub fn request(
        &self,
        source: git_asset_store::Key,
        predecessor: u64,
    ) -> Result<subset::Request> {
        if self.paths.is_empty() {
            return Err(Error::Incomplete);
        }
        Ok(subset::Request {
            source,
            predecessor_revision: predecessor,
            paths: self.paths.clone(),
        })
    }
}
pub fn select(inventory: &Inventory, categories: &Categories, filter: &Filter) -> Result<Preview> {
    let result = select_inner(inventory, categories, filter)?;
    if result.paths.len() > 1024 {
        return Err(Error::Budget);
    }
    encode(&result)?;
    Ok(result)
}
fn select_inner(
    inventory: &Inventory,
    categories: &Categories,
    filter: &Filter,
) -> Result<Preview> {
    if (filter.assets.is_empty() && filter.categories.is_empty())
        || filter.assets.len() > 1024
        || categories.len() > 128
    {
        return Err(Error::Invalid);
    }
    for (label, definitions) in categories {
        if label.is_empty()
            || label.len() > 128
            || label.chars().any(char::is_control)
            || definitions.is_empty()
            || definitions.len() > 128
        {
            return Err(Error::Invalid);
        }
        for definition in definitions {
            definition.validate().map_err(|_| Error::Invalid)?;
        }
    }
    if filter
        .categories
        .iter()
        .any(|c| !categories.contains_key(c))
        || filter
            .assets
            .iter()
            .any(|r| !inventory.assets.iter().any(|a| &a.reference == r))
    {
        return Err(Error::Incomplete);
    }
    let mut assets = Vec::new();
    let mut paths = BTreeSet::new();
    let mut objects = BTreeSet::new();
    for asset in &inventory.assets {
        if (!filter.assets.is_empty() && !filter.assets.contains(&asset.reference))
            || (!filter.categories.is_empty()
                && !filter
                    .categories
                    .iter()
                    .any(|c| categories[c].contains(&asset.definition)))
        {
            continue;
        }
        let mut remaining = 0;
        for (path, object) in &asset.paths {
            if inventory.actual.contains_key(path) {
                remaining += 1;
                paths.insert(path.clone());
                objects.insert(object.clone());
            }
        }
        assets.push(MatchedAsset {
            asset: asset.reference.clone(),
            definition: asset.definition.clone(),
            original_bindings: asset.paths.len(),
            remaining_bindings: remaining,
        });
    }
    let result = Preview {
        schema: "tig.native-retirement-selection.design-1",
        header: inventory.header.clone(),
        source_checkpoint: inventory.checkpoint.clone(),
        assets,
        paths: paths.into_iter().collect(),
        objects,
        age_filter_supported: false,
        complete_semantic_coverage: false,
        payload_verified: false,
        authorizes_removal: false,
    };
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_native_binding_header_profiles_and_alias_paths_pin_framing() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/vectors/native-retirement.design-1.json"
        )))
        .unwrap();
        for case in vectors["cases"].as_array().unwrap() {
            if case["kind"] == "binding_path" {
                let root = serde_json::from_value(case["root"].clone()).unwrap();
                let object = serde_json::from_value(case["object"].clone()).unwrap();
                assert_eq!(
                    binding(&root, &object).unwrap().as_str(),
                    case["path"].as_str().unwrap()
                );
            } else {
                let raw = case["canonical"].as_str().unwrap().as_bytes();
                assert_eq!(Digest::blob(raw).as_str(), case["sha256"].as_str().unwrap());
                let bytes = if case["kind"] == "header" {
                    encode(&serde_json::from_slice::<Header>(raw).unwrap()).unwrap()
                } else {
                    encode(&serde_json::from_slice::<Vec<Profile>>(raw).unwrap()).unwrap()
                };
                assert_eq!(bytes, raw);
            }
        }
    }
}
