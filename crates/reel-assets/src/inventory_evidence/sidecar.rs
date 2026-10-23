//! Explicit derived indexes over admitted inventory metadata. No default store,
//! path discovery, cache mutation, source admission or custody authorization.
use super::*;
use serde_json::{Value, json};

const HEADER: &str = "reel.inventory-sidecar.header.v1";
const NODE: &str = "reel.inventory-sidecar.node.v1";
const MAX_NODE_BYTES: u64 = 4 * 1024 * 1024;
pub mod file_store;
pub mod validation;

/// Caller-supplied immutable storage. `publish` must atomically publish without
/// overwriting an existing node, and reject a corrupt existing winner. Its
/// durability/retention guarantees belong to the backend, not this interface.
/// `open` must return exactly the named blob or an error; never scan/fallback.
pub trait Store {
    fn publish(&mut self, bytes: &[u8]) -> Result<Blob>;
    fn open(&mut self, blob: &Blob) -> Result<Box<dyn Read + '_>>;
}

#[derive(Clone, Copy)]
pub struct BuildLimits {
    pub node_bytes: u64,
    pub leaf_rows: usize,
    pub total_bytes: u64,
    pub nodes: usize,
}
impl Default for BuildLimits {
    fn default() -> Self {
        Self {
            node_bytes: 256 * 1024,
            leaf_rows: 128,
            total_bytes: 1024 * 1024 * 1024,
            nodes: 100_000,
        }
    }
}
#[derive(Clone, Copy)]
pub struct ReadLimits {
    pub node_bytes: u64,
    pub total_bytes: u64,
    pub nodes: usize,
    pub rows: usize,
}
impl Default for ReadLimits {
    fn default() -> Self {
        Self {
            node_bytes: 256 * 1024,
            total_bytes: 8 * 1024 * 1024,
            nodes: 66,
            rows: 4096,
        }
    }
}
/// Private evidence; intentionally not serializable as a shareable report.
pub struct IndexedLookup {
    /// None means not listed at capture, never physical absence or zero refs.
    pub observation: Option<ObservedObject>,
    pub capture: Option<Capture>,
    pub profile_sha256: String,
    pub registry_generation: u64,
    pub scope: Option<String>,
    pub named_scope: Option<String>,
    pub producer_dependency_coverage: Option<bool>,
    pub tiers: Vec<TierObservation>,
    pub nodes_read: usize,
    pub bytes_read: u64,
    pub rows_inspected: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    sha256: String,
    bytes: u64,
}
impl From<&Blob> for Descriptor {
    fn from(b: &Blob) -> Self {
        Self {
            sha256: b.sha256.clone(),
            bytes: b.bytes,
        }
    }
}
impl Descriptor {
    fn blob(&self) -> Blob {
        Blob {
            sha256: self.sha256.clone(),
            bytes: self.bytes,
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: String,
    source: Value,
    profile: String,
    generation: u64,
    capture: Option<(u64, u64)>,
    scope: Option<String>,
    named_scope: Option<String>,
    dependency_coverage: Option<bool>,
    tiers: Vec<TierWire>,
    objects: usize,
    copies: usize,
    tree: Descriptor,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TierWire {
    tier: usize,
    root: String,
    volume_id: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RowWire {
    record: usize,
    observation: Value,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    schema: String,
    prefix: String,
    body: Body,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Body {
    Leaf {
        rows: Vec<RowWire>,
    },
    Branch {
        children: BTreeMap<String, Descriptor>,
    },
}
fn source_key(s: &Source) -> Value {
    json!({"encoded":Descriptor::from(&s.encoded),"decoded":Descriptor::from(&s.decoded),
        "encoding":match s.encoding {Encoding::Identity=>"identity",Encoding::Gzip=>"gzip"},
        "source_schema":s.schema,"project":s.project_id,"authority":s.authority_namespace,
        "adapter":ADAPTER_ID,"adapter_version":s.adapter_version})
}
fn observation(row: &ObservedObject) -> Value {
    let copies: Vec<_> = row
        .copies
        .iter()
        .map(|c| {
            json!({"tier":c.tier,"bytes":c.bytes,
        "physical_id":c.physical_id,"physical_identity_measured":c.physical_identity_measured,
        "allocated_bytes":c.allocated_bytes,"hard_links":c.hard_links,
        "known_lfs_hard_links":c.known_lfs_hard_links,"other_hard_links":c.other_hard_links,
        "descriptor_size_matches":c.descriptor_size_matches})
        })
        .collect();
    json!({"sha256":row.sha256,"bytes":row.bytes,"registered":row.registered,"copies":copies})
}
// Private v1 codec: sorted JSON object keys, no whitespace, exact serde_json
// encoding. Not a general canonical graph interchange format.
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&serde_json::to_value(value)?)?)
}
fn decode<T: serde::de::DeserializeOwned + Serialize>(bytes: &[u8]) -> Result<T> {
    let value: Value = serde_json::from_slice(bytes)?;
    if serde_json::to_vec(&value)? != bytes {
        return Err("Noncanonical/duplicate-key sidecar node".into());
    }
    let decoded: T = serde_json::from_value(value)?;
    if encode(&decoded)? != bytes {
        return Err("Incomplete/noncanonical sidecar wire record".into());
    }
    Ok(decoded)
}
struct Meter {
    limits: ReadLimits,
    nodes: usize,
    bytes: u64,
}
impl Meter {
    fn read(&mut self, store: &mut impl Store, blob: &Blob) -> Result<Vec<u8>> {
        let next = self
            .bytes
            .checked_add(blob.bytes)
            .ok_or("Sidecar byte overflow")?;
        if !valid_oid(&blob.sha256)
            || blob.bytes > self.limits.node_bytes
            || next > self.limits.total_bytes
            || self.nodes >= self.limits.nodes
        {
            return Err("Sidecar read budget/descriptor rejected before open".into());
        }
        let bytes = read_limited(store.open(blob)?, blob.bytes)?;
        check_blob(&bytes, blob)?;
        self.nodes += 1;
        self.bytes = next;
        Ok(bytes)
    }
}
fn check_node_limit(bytes: u64) -> Result<()> {
    if bytes == 0 || bytes > MAX_NODE_BYTES {
        return Err("Invalid sidecar node budget".into());
    }
    Ok(())
}
struct Publisher<'a, S> {
    store: &'a mut S,
    limits: BuildLimits,
    nodes: usize,
    bytes: u64,
}
impl<S: Store> Publisher<'_, S> {
    fn put(&mut self, bytes: &[u8]) -> Result<Descriptor> {
        let total = self
            .bytes
            .checked_add(bytes.len() as u64)
            .ok_or("Sidecar byte overflow")?;
        if bytes.len() as u64 > self.limits.node_bytes
            || total > self.limits.total_bytes
            || self.nodes >= self.limits.nodes
        {
            return Err("Sidecar publication budget exceeded".into());
        }
        let expected = Blob {
            sha256: hash(bytes),
            bytes: bytes.len() as u64,
        };
        let actual = self.store.publish(bytes)?;
        if actual != expected {
            return Err("Store returned wrong published identity".into());
        }
        // Publication success alone is insufficient, even for a preexisting node.
        check_blob(
            &read_limited(self.store.open(&expected)?, expected.bytes)?,
            &expected,
        )?;
        self.nodes += 1;
        self.bytes = total;
        Ok(Descriptor::from(&expected))
    }
    fn tree(&mut self, rows: &[&ObservedObject], prefix: &str) -> Result<Descriptor> {
        if rows.len() <= self.limits.leaf_rows {
            let mut leaf = Vec::new();
            let mut size = 0u64;
            for row in rows {
                // Check raw variable-length fields before cloning them into the
                // private wire representation. JSON escaping is checked below.
                let strings = row
                    .copies
                    .iter()
                    .filter_map(|c| c.physical_id.as_ref())
                    .try_fold(64u64, |n, s| n.checked_add(s.len() as u64))
                    .ok_or("Sidecar record size overflow")?;
                if strings > self.limits.node_bytes {
                    return Err("Oversized sidecar record".into());
                }
                let wire = RowWire {
                    record: row.source_record,
                    observation: observation(row),
                };
                size = size
                    .checked_add(encode(&wire)?.len() as u64)
                    .ok_or("Sidecar leaf size overflow")?;
                if size > self.limits.node_bytes {
                    break;
                }
                leaf.push(wire);
            }
            let complete = leaf.len() == rows.len();
            let node = Node {
                schema: NODE.into(),
                prefix: prefix.into(),
                body: Body::Leaf { rows: leaf },
            };
            let bytes = encode(&node)?;
            if complete && bytes.len() as u64 <= self.limits.node_bytes {
                return self.put(&bytes);
            }
        }
        if rows.len() <= 1 || prefix.len() >= 64 {
            return Err("Single sidecar record cannot fit node budget".into());
        }
        let mut groups: BTreeMap<String, Vec<&ObservedObject>> = BTreeMap::new();
        for row in rows {
            groups
                .entry(row.sha256[..prefix.len() + 1].into())
                .or_default()
                .push(row);
        }
        let mut children = BTreeMap::new();
        for (key, group) in groups {
            children.insert(key.clone(), self.tree(&group, &key)?);
        }
        self.put(&encode(&Node {
            schema: NODE.into(),
            prefix: prefix.into(),
            body: Body::Branch { children },
        })?)
    }
}
/// Publish only from a validated preparation. Immutable children are published
/// first, header last. Failure may leave unrooted nodes; caller owns their staging
/// protection and retirement. No source/catalog/root is changed here.
pub fn publish(
    store: &mut impl Store,
    index: &PreparedInventory,
    limits: BuildLimits,
) -> Result<Blob> {
    check_node_limit(limits.node_bytes)?;
    if limits.leaf_rows == 0 || limits.leaf_rows > 4096 {
        return Err("Invalid leaf row budget".into());
    }
    let strings = index
        .tiers
        .values()
        .flat_map(|t| [Some(&t.root), t.volume_id.as_ref()])
        .flatten()
        .chain(index.scope.iter())
        .chain(index.named_scope.iter())
        .try_fold(0u64, |n, s| n.checked_add(s.len() as u64))
        .ok_or("Sidecar metadata size overflow")?;
    if strings > limits.node_bytes {
        return Err("Oversized sidecar header metadata".into());
    }
    let mut publisher = Publisher {
        store,
        limits,
        nodes: 0,
        bytes: 0,
    };
    let tree = publisher.tree(&index.rows.values().collect::<Vec<_>>(), "")?;
    let header = Header {
        schema: HEADER.into(),
        source: source_key(&index.source),
        profile: index.profile.clone(),
        generation: index.generation,
        capture: index.capture.map(|c| (c.started, c.completed)),
        scope: index.scope.clone(),
        named_scope: index.named_scope.clone(),
        dependency_coverage: index.dependency_coverage,
        tiers: index
            .tiers
            .values()
            .map(|t| TierWire {
                tier: t.tier,
                root: t.root.clone(),
                volume_id: t.volume_id.clone(),
            })
            .collect(),
        objects: index.rows.len(),
        copies: index.copy_count,
        tree,
    };
    Ok(publisher.put(&encode(&header)?)?.blob())
}

/// Exact-subject lookup through an explicitly admitted root AND expected source.
/// Validates the visited path, not the entire tree or current physical custody.
pub fn lookup(
    store: &mut impl Store,
    root: &Blob,
    expected: &Source,
    subject: &str,
    limits: ReadLimits,
) -> Result<IndexedLookup> {
    check_node_limit(limits.node_bytes)?;
    if !valid_oid(subject) {
        return Err("Invalid exact inventory subject".into());
    }
    if expected.schema != SOURCE_SCHEMA
        || expected.adapter_version != ADAPTER_VERSION
        || !valid_oid(&expected.encoded.sha256)
        || !valid_oid(&expected.decoded.sha256)
        || expected.project_id.trim().is_empty()
        || expected.authority_namespace.trim().is_empty()
        || expected.project_id.len() > 256
        || expected.authority_namespace.len() > 256
    {
        return Err("Invalid expected inventory source".into());
    }
    let mut meter = Meter {
        limits,
        nodes: 0,
        bytes: 0,
    };
    let header: Header = decode(&meter.read(store, root)?)?;
    if header.schema != HEADER
        || header.source != source_key(expected)
        || !valid_oid(&header.profile)
        || header.objects > 1_000_000
        || header.copies > 4_000_000
        || header.tiers.len() > MAX_TIERS
        || header.capture.is_some_and(|(s, e)| s > e)
    {
        return Err("Sidecar source/schema/metadata mismatch".into());
    }
    let mut tiers = BTreeMap::new();
    for t in header.tiers {
        if t.root.trim().is_empty() || tiers.contains_key(&t.tier) {
            return Err("Invalid sidecar tiers".into());
        }
        tiers.insert(
            t.tier,
            TierObservation {
                tier: t.tier,
                root: t.root,
                volume_id: t.volume_id,
            },
        );
    }
    let mut prefix = String::new();
    let mut descriptor = header.tree.blob();
    let mut rows_inspected = 0;
    let found = loop {
        let node: Node = decode(&meter.read(store, &descriptor)?)?;
        if node.schema != NODE || node.prefix != prefix {
            return Err("Invalid sidecar node schema/routing".into());
        }
        match node.body {
            Body::Branch { children } => {
                if prefix.len() >= 64 || children.is_empty() || children.len() > 16 {
                    return Err("Invalid radix branch".into());
                }
                for (key, child) in &children {
                    if key.len() != prefix.len() + 1
                        || !key.starts_with(&prefix)
                        || !key
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                        || !valid_oid(&child.sha256)
                        || child.bytes > limits.node_bytes
                    {
                        return Err("Invalid sidecar child routing/descriptor".into());
                    }
                }
                let key = &subject[..prefix.len() + 1];
                let Some(child) = children.get(key) else {
                    break None;
                };
                descriptor = child.blob();
                prefix = key.into();
            }
            Body::Leaf { rows } => {
                rows_inspected = rows.len();
                if rows.len() > limits.rows
                    || rows.len() > 4096
                    || rows.len() > header.objects
                    || (rows.is_empty() && (header.objects != 0 || !prefix.is_empty()))
                {
                    return Err("Sidecar row budget exceeded".into());
                }
                let mut last = String::new();
                let mut ordinals = BTreeSet::new();
                let mut found = None;
                let mut copies = 0usize;
                for wire in rows {
                    let mut row: ObservedObject = serde_json::from_value(wire.observation.clone())?;
                    if wire.observation != observation(&row)
                        || row.sha256 <= last
                        || !row.sha256.starts_with(&prefix)
                        || wire.record >= header.objects
                        || !ordinals.insert(wire.record)
                    {
                        return Err("Invalid sidecar row identity/routing/witness".into());
                    }
                    validate_observation(&row, &tiers)?;
                    copies = copies
                        .checked_add(row.copies.len())
                        .ok_or("Sidecar copy count overflow")?;
                    if copies > header.copies {
                        return Err("Sidecar leaf exceeds captured copy count".into());
                    }
                    last = row.sha256.clone();
                    row.source_record = wire.record;
                    if row.sha256 == subject {
                        found = Some(row);
                    }
                }
                break found;
            }
        }
    };
    Ok(IndexedLookup {
        observation: found,
        capture: header
            .capture
            .map(|(started, completed)| Capture { started, completed }),
        profile_sha256: header.profile,
        registry_generation: header.generation,
        scope: header.scope,
        named_scope: header.named_scope,
        producer_dependency_coverage: header.dependency_coverage,
        tiers: tiers.into_values().collect(),
        nodes_read: meter.nodes,
        bytes_read: meter.bytes,
        rows_inspected,
    })
}
