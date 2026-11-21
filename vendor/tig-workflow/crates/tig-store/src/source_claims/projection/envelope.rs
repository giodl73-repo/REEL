//! Flat source/index envelope preserving the original coordinated equality rule.
//! Native context binding is inspectable; only exact admitted restore validates
//! declared resources. No mutation of source/index heads or payload bytes here.
use super::{Error, Limits, Resources, Result, Selection};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use tig_core::{
    Digest, LogicalPath, Object, identity,
    tree::{NodeStore, Tree, TreeError},
};
const SCHEMA: &str = "tig.coordinated-resource-envelope.experimental-1";
const HEADER: &str = "envelope/identity";
#[derive(Clone)]
pub struct Binding {
    pub native: Selection,
    pub policy: Digest,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: String,
    native: Selection,
    policy: Digest,
    objects: usize,
    logical_bytes: u64,
}
pub struct Envelope {
    selection: Selection,
    binding: Binding,
    objects: BTreeSet<Object>,
}
impl Envelope {
    pub fn selection(&self) -> &Selection {
        &self.selection
    }
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    pub fn resources(&self) -> Resources {
        let mut objects = self.objects.clone();
        objects.insert(self.selection.root.clone());
        Resources::admitted(&self.selection, objects)
    }
    pub fn complete_claim_coverage(&self) -> bool {
        false
    }
}
impl std::fmt::Debug for Envelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Envelope")
            .field("objects", &self.objects.len())
            .finish_non_exhaustive()
    }
}
fn path(s: &str) -> Result<LogicalPath> {
    LogicalPath::parse(s).map_err(|_| Error::InvalidReference)
}
fn tree_error(error: TreeError) -> Error {
    match error {
        TreeError::Budget => Error::Budget,
        TreeError::MissingNode(_) => Error::Missing,
        TreeError::Store(_) => Error::Io,
        _ => Error::Corrupt,
    }
}
fn native(selection: &Selection) -> Result<()> {
    if !(1..=65536).contains(&selection.root.bytes) {
        return Err(Error::InvalidReference);
    }
    Ok(())
}
type Prepared = (Header, Vec<u8>, BTreeMap<LogicalPath, Object>);
fn prepare(resources: &Resources, policy: &Digest, limits: Limits) -> Result<Prepared> {
    limits.validate()?;
    native(&resources.selection)?;
    let mut objects = BTreeMap::<Digest, Object>::new();
    let mut bytes = 0u64;
    for object in resources.objects.iter().chain([&resources.selection.root]) {
        if let Some(old) = objects.get(&object.sha256) {
            if old != object {
                return Err(Error::Conflict);
            }
            continue;
        }
        bytes = bytes.checked_add(object.bytes).ok_or(Error::Budget)?;
        if object.bytes > 9_007_199_254_740_991
            || bytes > limits.logical_bytes
            || objects.len() >= limits.objects
        {
            return Err(Error::Budget);
        }
        objects.insert(object.sha256.clone(), object.clone());
    }
    let header = Header {
        schema: SCHEMA.into(),
        native: resources.selection.clone(),
        policy: policy.clone(),
        objects: objects.len(),
        logical_bytes: bytes,
    };
    let raw = identity::canonical_json(
        &serde_json::to_value(&header).map_err(|_| Error::InvalidReference)?,
    )
    .map_err(|_| Error::InvalidReference)?;
    let identity = Object::from_bytes(&raw);
    if raw.len() > 65536
        || objects.len() + 1 > limits.objects
        || bytes
            .checked_add(identity.bytes)
            .is_none_or(|n| n > limits.logical_bytes)
    {
        return Err(Error::Budget);
    }
    let mut entries = objects
        .values()
        .map(|o| Ok((path(&format!("objects/{}", o.sha256.as_str()))?, o.clone())))
        .collect::<Result<BTreeMap<_, _>>>()?;
    entries.insert(path(HEADER)?, identity);
    if entries.len() > limits.tree.max_entries {
        return Err(Error::Budget);
    }
    Ok((header, raw, entries))
}
fn result(
    selection: Selection,
    header: Header,
    entries: BTreeMap<LogicalPath, Object>,
) -> Envelope {
    Envelope {
        selection,
        binding: Binding {
            native: header.native,
            policy: header.policy,
        },
        objects: entries.into_values().collect(),
    }
}
/// Immutable candidate only; finish/reopen/readback and coordinated admission
/// remain separate. An IO failure may leave unreferenced candidate metadata.
pub fn create(
    resources: &Resources,
    policy: &Digest,
    nodes: &mut impl NodeStore,
    limits: Limits,
) -> Result<Envelope> {
    let (header, raw, entries) = prepare(resources, policy, limits)?;
    let identity = Object::from_bytes(&raw);
    if nodes.put(&raw).map_err(tree_error)? != identity {
        return Err(Error::Corrupt);
    }
    let (tree, _) = Tree::build(&entries, nodes, limits.tree).map_err(tree_error)?;
    Ok(result(
        Selection {
            root: tree.root().clone(),
            context: identity.sha256,
        },
        header,
        entries,
    ))
}
fn read_header(
    selection: &Selection,
    entries: &BTreeMap<LogicalPath, Object>,
    nodes: &impl NodeStore,
) -> Result<Header> {
    let object = entries.get(&path(HEADER)?).ok_or(Error::Missing)?;
    if object.sha256 != selection.context || !(1..=65536).contains(&object.bytes) {
        return Err(Error::Corrupt);
    }
    let bytes = nodes
        .get(object)
        .map_err(tree_error)?
        .ok_or(Error::Missing)?;
    if bytes.len() > 65536 || Object::from_bytes(&bytes) != *object {
        return Err(Error::Corrupt);
    }
    let value = identity::parse_json(&bytes).map_err(|_| Error::Corrupt)?;
    if identity::canonical_json(&value).map_err(|_| Error::Corrupt)? != bytes {
        return Err(Error::Corrupt);
    }
    let header: Header = serde_json::from_value(value).map_err(|_| Error::Corrupt)?;
    if header.schema != SCHEMA {
        return Err(Error::InvalidReference);
    }
    native(&header.native)?;
    Ok(header)
}
/// Inspect only the native selection/policy needed for independent replay.
/// A structurally valid binding is NOT resource-closure or producer admission.
pub fn read_binding(
    selection: &Selection,
    nodes: &mut impl NodeStore,
    limits: Limits,
) -> Result<Binding> {
    limits.validate()?;
    let (entries, _) = Tree::from_root(selection.root.clone())
        .entries(nodes, limits.tree)
        .map_err(tree_error)?;
    let header = read_header(selection, &entries, nodes)?;
    if entries.len() > limits.objects
        || entries.len() != header.objects.checked_add(1).ok_or(Error::Budget)?
    {
        return Err(Error::Corrupt);
    }
    let mut bytes = 0u64;
    let mut found = false;
    for (key, object) in &entries {
        if key == &path(HEADER)? {
            continue;
        }
        if *key != path(&format!("objects/{}", object.sha256.as_str()))? {
            return Err(Error::Corrupt);
        }
        bytes = bytes.checked_add(object.bytes).ok_or(Error::Budget)?;
        found |= object == &header.native.root;
    }
    let header_bytes = entries[&path(HEADER)?].bytes;
    if !found || bytes != header.logical_bytes {
        return Err(Error::Corrupt);
    }
    if bytes
        .checked_add(header_bytes)
        .is_none_or(|n| n > limits.logical_bytes)
    {
        return Err(Error::Budget);
    }
    Ok(Binding {
        native: header.native,
        policy: header.policy,
    })
}
/// Verify an independently replayed/admitted exact native resource set and
/// policy against every envelope entry. No writes, selection or fallback.
pub fn restore(
    selection: &Selection,
    resources: &Resources,
    policy: &Digest,
    nodes: &mut impl NodeStore,
    limits: Limits,
) -> Result<Envelope> {
    let (expected, raw, wanted) = prepare(resources, policy, limits)?;
    let (entries, _) = Tree::from_root(selection.root.clone())
        .entries(nodes, limits.tree)
        .map_err(tree_error)?;
    let header = read_header(selection, &entries, nodes)?;
    if selection.context != Object::from_bytes(&raw).sha256
        || entries != wanted
        || header.native != expected.native
        || header.policy != expected.policy
    {
        return Err(Error::Conflict);
    }
    Ok(result(selection.clone(), expected, entries))
}
