//! Content-addressed directory trees with canonical persistent entry maps.
//! Nodes are raw SHA-256 blobs of strict canonical JSON. No filesystem access.
use crate::{Digest, LogicalPath, Object, identity};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeError {
    Budget,
    InvalidNode,
    Integrity,
    MissingNode(Object),
    PathConflict,
    StaleRoot,
    Store(String),
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for TreeError {}
pub type TreeResult<T> = Result<T, TreeError>;

/// Explicit metadata port. Implementations must not infer custody from this API.
pub trait NodeStore {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>>;
    fn put(&mut self, bytes: &[u8]) -> TreeResult<Object>;
}

#[derive(Default)]
pub struct MemoryNodes {
    nodes: BTreeMap<Digest, Vec<u8>>,
}
impl MemoryNodes {
    pub fn objects(&self) -> impl Iterator<Item = (&Digest, &[u8])> {
        self.nodes
            .iter()
            .map(|(key, bytes)| (key, bytes.as_slice()))
    }
}
impl NodeStore for MemoryNodes {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        Ok(self.nodes.get(&object.sha256).cloned())
    }
    fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
        let object = Object::from_bytes(bytes);
        if let Some(old) = self.nodes.get(&object.sha256) {
            if old != bytes {
                return Err(TreeError::Integrity);
            }
        } else {
            self.nodes.insert(object.sha256.clone(), bytes.to_vec());
        }
        Ok(object)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TreeLimits {
    pub max_entries: usize,
    pub max_reads: usize,
    pub max_writes: usize,
    pub max_read_bytes: usize,
    pub max_write_bytes: usize,
    pub max_map_depth: usize,
}
impl Default for TreeLimits {
    fn default() -> Self {
        Self {
            max_entries: 65_536,
            max_reads: 65_536,
            max_writes: 65_536,
            max_read_bytes: 32 * 1024 * 1024,
            max_write_bytes: 32 * 1024 * 1024,
            max_map_depth: 64,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeMetrics {
    pub reads: usize,
    pub writes: usize,
    pub read_bytes: usize,
    pub write_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Link {
    object: Object,
    entries: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Entry {
    File { object: Object },
    Directory { object: Object },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Directory {
    schema: String,
    root: Option<Link>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MapNode {
    schema: String,
    name: String,
    value: Entry,
    left: Option<Link>,
    right: Option<Link>,
}

struct Context<'a, S> {
    store: &'a mut S,
    limits: TreeLimits,
    metrics: TreeMetrics,
}

#[derive(Default)]
struct DraftDirectory(BTreeMap<String, DraftEntry>);
enum DraftEntry {
    File(Object),
    Directory(DraftDirectory),
}

impl DraftDirectory {
    fn insert(&mut self, parts: &[&str], object: Object) -> TreeResult<()> {
        if parts.len() == 1 {
            if matches!(self.0.get(parts[0]), Some(DraftEntry::Directory(_))) {
                return Err(TreeError::PathConflict);
            }
            self.0.insert(parts[0].into(), DraftEntry::File(object));
            return Ok(());
        }
        let child = self
            .0
            .entry(parts[0].into())
            .or_insert_with(|| DraftEntry::Directory(DraftDirectory::default()));
        match child {
            DraftEntry::File(_) => Err(TreeError::PathConflict),
            DraftEntry::Directory(dir) => dir.insert(&parts[1..], object),
        }
    }
}

struct ReadyEntry {
    name: String,
    value: Entry,
    priority: Digest,
}

fn priority(name: &str) -> (Digest, &str) {
    (Digest::blob(name.as_bytes()), name)
}
fn count(link: &Option<Link>) -> u32 {
    link.as_ref().map_or(0, |r| r.entries)
}

impl<S: NodeStore> Context<'_, S> {
    fn build_map(&mut self, entries: &[ReadyEntry], depth: usize) -> TreeResult<Option<Link>> {
        if entries.is_empty() {
            return Ok(None);
        }
        if depth >= self.limits.max_map_depth {
            return Err(TreeError::Budget);
        }
        let (index, item) = entries
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| (&a.priority, &a.name).cmp(&(&b.priority, &b.name)))
            .ok_or(TreeError::InvalidNode)?;
        let left = self.build_map(&entries[..index], depth + 1)?;
        let right = self.build_map(&entries[index + 1..], depth + 1)?;
        self.save(&MapNode {
            schema: "tig.entry-map.design-1".into(),
            name: item.name.clone(),
            value: item.value.clone(),
            left,
            right,
        })
        .map(Some)
    }

    fn build_directory(&mut self, draft: DraftDirectory) -> TreeResult<Object> {
        let mut entries = Vec::with_capacity(draft.0.len());
        for (name, entry) in draft.0 {
            let value = match entry {
                DraftEntry::File(object) => Entry::File { object },
                DraftEntry::Directory(dir) => Entry::Directory {
                    object: self.build_directory(dir)?,
                },
            };
            entries.push(ReadyEntry {
                priority: priority(&name).0,
                name,
                value,
            });
        }
        let root = self.build_map(&entries, 0)?;
        self.write(&Directory {
            schema: "tig.directory.design-1".into(),
            root,
        })
    }

    fn map_entries(
        &mut self,
        root: Option<Link>,
        depth: usize,
        out: &mut Vec<(String, Entry)>,
    ) -> TreeResult<()> {
        let Some(link) = root else {
            return Ok(());
        };
        if depth >= self.limits.max_map_depth {
            return Err(TreeError::Budget);
        }
        let node = self.node(&link)?;
        for child in [&node.left, &node.right].into_iter().flatten() {
            let c = self.node(child)?;
            if priority(&c.name) >= priority(&node.name) {
                return Err(TreeError::InvalidNode);
            }
        }
        self.map_entries(node.left, depth + 1, out)?;
        if out.last().is_some_and(|(name, _)| name >= &node.name) {
            return Err(TreeError::InvalidNode);
        }
        out.push((node.name, node.value));
        self.map_entries(node.right, depth + 1, out)
    }

    fn all_entries(
        &mut self,
        directory: &Object,
        prefix: &str,
        out: &mut BTreeMap<LogicalPath, Object>,
    ) -> TreeResult<()> {
        if prefix.split('/').count() > 32 {
            return Err(TreeError::Budget);
        }
        let dir = self.directory(directory)?;
        let mut entries = Vec::new();
        self.map_entries(dir.root, 0, &mut entries)?;
        for (name, value) in entries {
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            match value {
                Entry::File { object } => {
                    if out.len() >= self.limits.max_entries {
                        return Err(TreeError::Budget);
                    }
                    let path = LogicalPath::parse(&path).map_err(|_| TreeError::InvalidNode)?;
                    if out.insert(path, object).is_some() {
                        return Err(TreeError::InvalidNode);
                    }
                }
                Entry::Directory { object } => self.all_entries(&object, &path, out)?,
            }
        }
        Ok(())
    }

    fn read<T: DeserializeOwned + Serialize>(&mut self, object: &Object) -> TreeResult<T> {
        if object.bytes > 65_536
            || self.metrics.reads >= self.limits.max_reads
            || object.bytes as usize
                > self
                    .limits
                    .max_read_bytes
                    .saturating_sub(self.metrics.read_bytes)
        {
            return Err(TreeError::Budget);
        }
        self.metrics.reads += 1;
        self.metrics.read_bytes += object.bytes as usize;
        let bytes = self
            .store
            .get(object)?
            .ok_or_else(|| TreeError::MissingNode(object.clone()))?;
        if bytes.len() as u64 != object.bytes || Object::from_bytes(&bytes) != *object {
            return Err(TreeError::Integrity);
        }
        let value = identity::parse_json(&bytes).map_err(|_| TreeError::InvalidNode)?;
        if identity::canonical_json(&value).map_err(|_| TreeError::InvalidNode)? != bytes {
            return Err(TreeError::InvalidNode);
        }
        let parsed: T = serde_json::from_value(value).map_err(|_| TreeError::InvalidNode)?;
        let encoded = serde_json::to_value(&parsed).map_err(|_| TreeError::InvalidNode)?;
        if identity::canonical_json(&encoded).map_err(|_| TreeError::InvalidNode)? != bytes {
            return Err(TreeError::InvalidNode);
        }
        Ok(parsed)
    }

    fn write(&mut self, value: &impl Serialize) -> TreeResult<Object> {
        let value = serde_json::to_value(value).map_err(|_| TreeError::InvalidNode)?;
        let bytes = identity::canonical_json(&value).map_err(|_| TreeError::InvalidNode)?;
        if self.metrics.writes >= self.limits.max_writes
            || bytes.len()
                > self
                    .limits
                    .max_write_bytes
                    .saturating_sub(self.metrics.write_bytes)
        {
            return Err(TreeError::Budget);
        }
        self.metrics.writes += 1;
        self.metrics.write_bytes += bytes.len();
        let expected = Object::from_bytes(&bytes);
        if self.store.put(&bytes)? != expected {
            return Err(TreeError::Integrity);
        }
        Ok(expected)
    }

    fn directory(&mut self, object: &Object) -> TreeResult<Directory> {
        let node: Directory = self.read(object)?;
        if node.schema != "tig.directory.design-1" {
            return Err(TreeError::InvalidNode);
        }
        if count(&node.root) as usize > self.limits.max_entries {
            return Err(TreeError::Budget);
        }
        Ok(node)
    }

    fn node(&mut self, link: &Link) -> TreeResult<MapNode> {
        let node: MapNode = self.read(&link.object)?;
        if node.schema != "tig.entry-map.design-1"
            || node.name.contains('/')
            || LogicalPath::parse(&node.name).is_err()
            || count(&node.left)
                .checked_add(count(&node.right))
                .and_then(|n| n.checked_add(1))
                != Some(link.entries)
        {
            return Err(TreeError::InvalidNode);
        }
        if link.entries as usize > self.limits.max_entries {
            return Err(TreeError::Budget);
        }
        Ok(node)
    }

    fn save(&mut self, node: &MapNode) -> TreeResult<Link> {
        let entries = count(&node.left)
            .checked_add(count(&node.right))
            .and_then(|n| n.checked_add(1))
            .ok_or(TreeError::Budget)?;
        if entries as usize > self.limits.max_entries {
            return Err(TreeError::Budget);
        }
        Ok(Link {
            object: self.write(node)?,
            entries,
        })
    }

    fn find(&mut self, mut root: Option<Link>, name: &str) -> TreeResult<Option<Entry>> {
        let mut low: Option<String> = None;
        let mut high: Option<String> = None;
        let mut parent: Option<(Digest, String)> = None;
        for _ in 0..self.limits.max_map_depth {
            let Some(link) = root else { return Ok(None) };
            let node = self.node(&link)?;
            if low.as_ref().is_some_and(|s| node.name <= *s)
                || high.as_ref().is_some_and(|s| node.name >= *s)
                || parent
                    .as_ref()
                    .is_some_and(|p| priority(&node.name) >= (p.0.clone(), p.1.as_str()))
            {
                return Err(TreeError::InvalidNode);
            }
            parent = Some((priority(&node.name).0, node.name.clone()));
            match name.cmp(&node.name) {
                std::cmp::Ordering::Equal => return Ok(Some(node.value)),
                std::cmp::Ordering::Less => {
                    high = Some(node.name);
                    root = node.left;
                }
                std::cmp::Ordering::Greater => {
                    low = Some(node.name);
                    root = node.right;
                }
            }
        }
        Err(TreeError::Budget)
    }

    fn merge(
        &mut self,
        left: Option<Link>,
        right: Option<Link>,
        depth: usize,
    ) -> TreeResult<Option<Link>> {
        if depth >= self.limits.max_map_depth {
            return Err(TreeError::Budget);
        }
        let (Some(l), Some(r)) = (left.clone(), right.clone()) else {
            return Ok(left.or(right));
        };
        let mut a = self.node(&l)?;
        let mut b = self.node(&r)?;
        if priority(&a.name) > priority(&b.name) {
            a.right = self.merge(a.right, Some(r), depth + 1)?;
            self.save(&a).map(Some)
        } else {
            b.left = self.merge(Some(l), b.left, depth + 1)?;
            self.save(&b).map(Some)
        }
    }

    fn change(
        &mut self,
        root: Option<Link>,
        name: &str,
        value: Option<Entry>,
        depth: usize,
    ) -> TreeResult<Option<Link>> {
        if depth >= self.limits.max_map_depth {
            return Err(TreeError::Budget);
        }
        let Some(link) = root else {
            return value
                .map(|value| {
                    self.save(&MapNode {
                        schema: "tig.entry-map.design-1".into(),
                        name: name.into(),
                        value,
                        left: None,
                        right: None,
                    })
                })
                .transpose();
        };
        let mut node = self.node(&link)?;
        match name.cmp(&node.name) {
            std::cmp::Ordering::Equal => {
                let Some(value) = value else {
                    return self.merge(node.left, node.right, depth + 1);
                };
                if node.value == value {
                    return Ok(Some(link));
                }
                node.value = value;
            }
            std::cmp::Ordering::Less => {
                node.left = self.change(node.left, name, value, depth + 1)?;
                if let Some(left) = node.left.clone() {
                    let mut child = self.node(&left)?;
                    if priority(&child.name) > priority(&node.name) {
                        node.left = child.right;
                        child.right = Some(self.save(&node)?);
                        return self.save(&child).map(Some);
                    }
                }
            }
            std::cmp::Ordering::Greater => {
                node.right = self.change(node.right, name, value, depth + 1)?;
                if let Some(right) = node.right.clone() {
                    let mut child = self.node(&right)?;
                    if priority(&child.name) > priority(&node.name) {
                        node.right = child.left;
                        child.left = Some(self.save(&node)?);
                        return self.save(&child).map(Some);
                    }
                }
            }
        }
        self.save(&node).map(Some)
    }

    fn edit_path(
        &mut self,
        directory: &Object,
        parts: &[&str],
        value: Option<Object>,
    ) -> TreeResult<Object> {
        let mut dir = self.directory(directory)?;
        let old = self.find(dir.root.clone(), parts[0])?;
        let next = if parts.len() == 1 {
            if matches!(old, Some(Entry::Directory { .. })) {
                return Err(TreeError::PathConflict);
            }
            value.map(|object| Entry::File { object })
        } else {
            let child = match old.clone() {
                Some(Entry::File { .. }) => return Err(TreeError::PathConflict),
                Some(Entry::Directory { object }) => object,
                None if value.is_none() => return Ok(directory.clone()),
                None => self.write(&Directory {
                    schema: "tig.directory.design-1".into(),
                    root: None,
                })?,
            };
            let object = self.edit_path(&child, &parts[1..], value)?;
            if self.directory(&object)?.root.is_none() {
                None
            } else {
                Some(Entry::Directory { object })
            }
        };
        if old == next {
            return Ok(directory.clone());
        }
        dir.root = self.change(dir.root, parts[0], next, 0)?;
        self.write(&dir)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    root: Object,
}
impl Tree {
    /// A supplied root is not a complete/integrity-verified closure until read.
    pub fn from_root(root: Object) -> Self {
        Self { root }
    }
    pub fn root(&self) -> &Object {
        &self.root
    }

    /// Bulk construction writes each reachable entry-map/directory node once.
    pub fn build(
        bindings: &BTreeMap<LogicalPath, Object>,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> TreeResult<(Self, TreeMetrics)> {
        if bindings.len() > limits.max_entries {
            return Err(TreeError::Budget);
        }
        if crate::check_sizes(bindings.values()).is_err() {
            return Err(TreeError::Integrity);
        }
        let mut draft = DraftDirectory::default();
        for (path, object) in bindings {
            draft.insert(&path.components().collect::<Vec<_>>(), object.clone())?;
        }
        let mut ctx = Context {
            store,
            limits,
            metrics: TreeMetrics::default(),
        };
        let root = ctx.build_directory(draft)?;
        Ok((Self { root }, ctx.metrics))
    }

    /// Complete bounded traversal, unlike a single-path lookup. Missing or
    /// malformed nodes fail; callers must not turn failure into empty coverage.
    pub fn entries(
        &self,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> TreeResult<(BTreeMap<LogicalPath, Object>, TreeMetrics)> {
        let mut ctx = Context {
            store,
            limits,
            metrics: TreeMetrics::default(),
        };
        let mut entries = BTreeMap::new();
        ctx.all_entries(&self.root, "", &mut entries)?;
        if crate::check_sizes(entries.values()).is_err() {
            return Err(TreeError::Integrity);
        }
        Ok((entries, ctx.metrics))
    }

    pub fn empty(store: &mut impl NodeStore) -> TreeResult<Self> {
        let mut ctx = Context {
            store,
            limits: TreeLimits::default(),
            metrics: TreeMetrics::default(),
        };
        let root = ctx.write(&Directory {
            schema: "tig.directory.design-1".into(),
            root: None,
        })?;
        Ok(Self { root })
    }

    pub fn get(
        &self,
        path: &LogicalPath,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> TreeResult<(Option<Object>, TreeMetrics)> {
        let mut ctx = Context {
            store,
            limits,
            metrics: TreeMetrics::default(),
        };
        let parts: Vec<_> = path.components().collect();
        let mut directory = self.root.clone();
        for (i, name) in parts.iter().enumerate() {
            let dir = ctx.directory(&directory)?;
            match ctx.find(dir.root, name)? {
                None => return Ok((None, ctx.metrics)),
                Some(Entry::File { object }) if i + 1 == parts.len() => {
                    return Ok((Some(object), ctx.metrics));
                }
                Some(Entry::Directory { object }) if i + 1 < parts.len() => directory = object,
                _ => return Err(TreeError::PathConflict),
            }
        }
        Err(TreeError::InvalidNode)
    }

    pub fn edit(
        &self,
        expected_root: &Object,
        changes: &BTreeMap<LogicalPath, Option<Object>>,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> TreeResult<(Self, TreeMetrics)> {
        if expected_root != &self.root {
            return Err(TreeError::StaleRoot);
        }
        if changes.len() > limits.max_entries {
            return Err(TreeError::Budget);
        }
        if crate::check_sizes(changes.values().flatten()).is_err() {
            return Err(TreeError::Integrity);
        }
        let mut ctx = Context {
            store,
            limits,
            metrics: TreeMetrics::default(),
        };
        let mut root = self.root.clone();
        for (path, object) in changes {
            let parts: Vec<_> = path.components().collect();
            root = ctx.edit_path(&root, &parts, object.clone())?;
        }
        Ok((Self { root }, ctx.metrics))
    }
}
