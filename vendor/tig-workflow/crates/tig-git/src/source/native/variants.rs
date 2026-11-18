//! Retained variant interpretation for an exact declared production context.
//! Metadata candidates only; no media IO, execution or custody authority.
use super::{
    AssetSnapshot, Error, Result,
    archive::{put, read, tree_error},
    context::ProductionContext,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};
use tig_assets::{
    graph::{self, Reference},
    native::{Instance, MAX_RECORD_BYTES},
    resolve::Resolved,
    variants::{self, Context, Index, Space},
};
use tig_core::{
    LogicalPath, Object, identity,
    tree::{NodeStore, Tree, TreeLimits},
};
use tig_store::source_claims::Selection;

/// External policy, never inferred from retained index/policy documents.
pub struct Policy {
    pub asset: variants::Admission,
    pub project_space: Object,
}
pub struct Request {
    pub record: Reference,
    pub index: Object,
    pub context: Object,
}
pub struct Admission<'a> {
    pub source: &'a mut AssetSnapshot,
    pub context: &'a ProductionContext,
    pub profiles: &'a [graph::Profile],
    pub policies: &'a [Policy],
    pub requests: &'a [Request],
    pub query_limits: graph::Limits,
}
/// Sealed checked packet. Private authored values are deliberately not Debug.
pub struct Packet {
    selection: Selection,
    source: Selection,
    objects: BTreeSet<Object>,
    selections: BTreeMap<Reference, variants::Selection>,
}
impl Packet {
    pub fn selection(&self) -> &Selection {
        &self.selection
    }
    pub fn source(&self) -> &Selection {
        &self.source
    }
    /// Declared interpretation references, not physical tree-node allocation.
    pub fn objects(&self) -> &BTreeSet<Object> {
        &self.objects
    }
    pub fn selections(&self) -> &BTreeMap<Reference, variants::Selection> {
        &self.selections
    }
}
struct Prepared {
    entries: BTreeMap<LogicalPath, Object>,
    documents: BTreeMap<Object, Vec<u8>>,
    source: Selection,
    header: Object,
    selections: BTreeMap<Reference, variants::Selection>,
    control_reads: usize,
    control_bytes: usize,
}
fn path(namespace: &str, object: &Object) -> Result<LogicalPath> {
    LogicalPath::parse(&format!("{namespace}/{}", object.sha256.as_str()))
        .map_err(|_| Error::Invalid)
}
fn document(value: &impl Serialize) -> Result<Vec<u8>> {
    let raw = identity::canonical_json(&serde_json::to_value(value).map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)?;
    if raw.len() > MAX_RECORD_BYTES {
        return Err(Error::Budget);
    }
    Ok(raw)
}
fn prepare(
    admission: &mut Admission<'_>,
    store: &impl NodeStore,
    limits: TreeLimits,
) -> Result<Prepared> {
    let start = Instant::now();
    if admission.policies.is_empty()
        || admission.policies.len() > 256
        || admission.requests.is_empty()
        || admission.requests.len() > 4096
    {
        return Err(Error::Budget);
    }
    let source = &mut *admission.source;
    let context = admission.context;
    if source.checkpoint() != context.requirements().checkpoint() {
        return Err(Error::Invalid);
    }
    // Validate the full pin set before any provider read. The same digest
    // cannot acquire another length through an unselected policy or request.
    let mut pins = BTreeMap::new();
    let external = admission
        .policies
        .iter()
        .flat_map(|p| [&p.asset.definition.object, &p.asset.space, &p.project_space])
        .chain(
            admission
                .requests
                .iter()
                .flat_map(|r| [&r.record.object, &r.index, &r.context]),
        )
        .chain(std::iter::once(&context.selection().root));
    for object in external.chain(context.requirements().all_objects().iter()) {
        if object.bytes > 9_007_199_254_740_991
            || pins
                .insert(object.sha256.clone(), object.bytes)
                .is_some_and(|n| n != object.bytes)
        {
            return Err(Error::Integrity);
        }
    }
    if admission
        .requests
        .len()
        .checked_add(admission.policies.len())
        .and_then(|n| n.checked_add(2))
        .is_none_or(|n| n > limits.max_entries)
    {
        return Err(Error::Budget);
    }
    let query = graph::Query {
        checkpoint: source.checkpoint().clone(),
        roots: context.roots().clone(),
        kinds: BTreeSet::from([graph::EdgeKind::Production]),
        limits: admission.query_limits,
    };
    let graph = graph::resolve(source, &query, admission.profiles).map_err(|_| Error::Invalid)?;
    let required = graph
        .production_requirements()
        .map_err(|_| Error::Incomplete)?;
    if required.binding_digest() != context.requirements().binding_digest()
        || required.admission() != context.requirements().admission()
    {
        return Err(Error::Integrity);
    }
    let mut policies = BTreeMap::new();
    let mut documents = BTreeMap::new();
    let mut entries = BTreeMap::from([(
        LogicalPath::parse("source/context").map_err(|_| Error::Invalid)?,
        context.selection().root.clone(),
    )]);
    for policy in admission.policies {
        let bytes = document(&serde_json::json!({
            "schema":"tig.native-variant-policy.experimental-1",
            "definition":policy.asset.definition,"space":policy.asset.space,
            "project_space":policy.project_space,"semantics":policy.asset.semantics,
            "uses":policy.asset.uses,
        }))?;
        let object = Object::from_bytes(&bytes);
        if policies
            .insert(policy.asset.definition.clone(), (policy, object.clone()))
            .is_some()
        {
            return Err(Error::Ambiguous);
        }
        if entries
            .insert(
                path("policy", &policy.asset.definition.object)?,
                object.clone(),
            )
            .is_some()
        {
            return Err(Error::Ambiguous);
        }
        documents.insert(object, bytes);
    }
    let mut wanted = BTreeSet::new();
    for (reference, node) in graph.nodes() {
        let record = source.records.get(reference).ok_or(Error::Incomplete)?;
        if node.class == graph::NodeKind::Primary
            && policies.contains_key(record.resolved.definition())
        {
            wanted.insert(reference.clone());
        }
    }
    let mut selections = BTreeMap::new();
    let mut control_bytes = 0usize;
    let mut controls = BTreeMap::new();
    for request in admission.requests {
        if start.elapsed() > Duration::from_millis(source.limits.millis) {
            return Err(Error::Budget);
        }
        if !wanted.remove(&request.record) {
            return Err(Error::Invalid);
        }
        let record = source
            .records
            .get(&request.record)
            .ok_or(Error::Incomplete)?;
        let (policy, policy_object) = policies
            .get(record.resolved.definition())
            .ok_or(Error::Invalid)?;
        // Rebind retained immutable records to this source checkpoint, never a
        // newer ID selection or the checkpoint of an old retained snapshot.
        let bound = Instance::parse(
            source
                .blobs
                .get(&request.record.object)
                .ok_or(Error::Incomplete)?,
        )
        .map_err(|_| Error::Integrity)?
        .bind(
            source
                .blobs
                .get(&record.definition)
                .ok_or(Error::Incomplete)?,
            source.blobs.get(&record.schema).ok_or(Error::Incomplete)?,
        )
        .map_err(|_| Error::Integrity)?;
        let resolved = Resolved::from_bound(source.checkpoint().clone(), &bound);
        for object in [
            &policy.asset.space,
            &policy.project_space,
            &request.index,
            &request.context,
        ] {
            if !controls.contains_key(object) {
                control_bytes = control_bytes
                    .checked_add(usize::try_from(object.bytes).map_err(|_| Error::Budget)?)
                    .ok_or(Error::Budget)?;
                if control_bytes > source.limits.bytes.min(limits.max_read_bytes)
                    || controls.len() >= limits.max_reads
                {
                    return Err(Error::Budget);
                }
                let raw = read(store, object, MAX_RECORD_BYTES)?;
                controls.insert(object.clone(), raw);
            }
        }
        let space = Space::parse(&policy.asset.space, &controls[&policy.asset.space])
            .map_err(|_| Error::Integrity)?;
        let project = Space::parse(&policy.project_space, &controls[&policy.project_space])
            .map_err(|_| Error::Integrity)?;
        let index = Index::parse(&request.index, &controls[&request.index])
            .map_err(|_| Error::Integrity)?;
        let variant_context =
            Context::restore(&request.context, &controls[&request.context], &project)
                .map_err(|_| Error::Integrity)?;
        let selection = variants::resolve_in_context(
            &resolved,
            &policy.asset,
            &space,
            &index,
            &variant_context,
            &policy.project_space,
        )
        .map_err(|_| Error::Integrity)?;
        if graph.nodes()[&request.record].primary_objects.as_ref()
            != Some(&BTreeSet::from([selection.object().clone()]))
        {
            return Err(Error::Integrity);
        }
        let bytes = document(&serde_json::json!({
            "schema":"tig.native-variant-witness.experimental-1",
            "record":request.record,"policy":policy_object,"index":request.index,
            "context":request.context,"subject":selection.subject(),
            "slot":selection.slot(),"object":selection.object(),
        }))?;
        let object = Object::from_bytes(&bytes);
        entries.insert(path("witness", &request.record.object)?, object.clone());
        documents.insert(object, bytes);
        selections.insert(request.record.clone(), selection);
    }
    if !wanted.is_empty() {
        return Err(Error::Incomplete);
    }
    for object in controls.keys() {
        entries.insert(path("control", object)?, object.clone());
    }
    let bytes = document(&serde_json::json!({
        "schema":"tig.native-variant-packet.experimental-1",
        "source":context.selection(),"selections":selections.len(),
        "policies":policies.len(),"controls":controls.len(),
    }))?;
    let header = Object::from_bytes(&bytes);
    documents.insert(header.clone(), bytes);
    entries.insert(
        LogicalPath::parse("packet/identity").map_err(|_| Error::Invalid)?,
        header.clone(),
    );
    if entries.len() > limits.max_entries {
        return Err(Error::Budget);
    }
    let document_bytes = documents.values().try_fold(0usize, |sum, b| {
        sum.checked_add(b.len()).ok_or(Error::Budget)
    })?;
    if document_bytes > source.limits.bytes {
        return Err(Error::Budget);
    }
    let mut sizes = BTreeMap::new();
    for object in entries.values().chain(required.all_objects().iter()) {
        if object.bytes > 9_007_199_254_740_991
            || sizes
                .insert(object.sha256.clone(), object.bytes)
                .is_some_and(|n| n != object.bytes)
        {
            return Err(Error::Integrity);
        }
    }
    Ok(Prepared {
        entries,
        documents,
        source: context.selection().clone(),
        header,
        selections,
        control_reads: controls.len(),
        control_bytes,
    })
}
fn result(prepared: Prepared, root: Object) -> Packet {
    let mut objects: BTreeSet<_> = prepared.entries.values().cloned().collect();
    objects.insert(root.clone());
    Packet {
        selection: Selection {
            root,
            context: prepared.header.sha256,
        },
        source: prepared.source,
        objects,
        selections: prepared.selections,
    }
}
/// Reads admitted controls, writes only candidate interpretation metadata.
pub fn freeze(
    admission: &mut Admission<'_>,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<Packet> {
    let prepared = prepare(admission, store, limits)?;
    let mut remaining = limits;
    remaining.max_writes = remaining
        .max_writes
        .checked_sub(prepared.documents.len())
        .ok_or(Error::Budget)?;
    for bytes in prepared.documents.values() {
        remaining.max_write_bytes = remaining
            .max_write_bytes
            .checked_sub(bytes.len())
            .ok_or(Error::Budget)?;
    }
    for bytes in prepared.documents.values() {
        put(store, bytes)?;
    }
    let tree = Tree::build(&prepared.entries, store, remaining)
        .map_err(tree_error)?
        .0;
    Ok(result(prepared, tree.root().clone()))
}
/// Recompute under independent admission. Never writes or discovers a policy.
pub fn restore(
    selection: &Selection,
    admission: &mut Admission<'_>,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<Packet> {
    let prepared = prepare(admission, store, limits)?;
    if selection.context != prepared.header.sha256 {
        return Err(Error::Integrity);
    }
    let mut remaining = limits;
    remaining.max_reads = remaining
        .max_reads
        .checked_sub(prepared.control_reads)
        .and_then(|n| n.checked_sub(prepared.documents.len()))
        .ok_or(Error::Budget)?;
    remaining.max_read_bytes = remaining
        .max_read_bytes
        .checked_sub(prepared.control_bytes)
        .ok_or(Error::Budget)?;
    for bytes in prepared.documents.values() {
        remaining.max_read_bytes = remaining
            .max_read_bytes
            .checked_sub(bytes.len())
            .ok_or(Error::Budget)?;
    }
    let bindings = Tree::from_root(selection.root.clone())
        .entries(store, remaining)
        .map_err(tree_error)?
        .0;
    if bindings != prepared.entries {
        return Err(Error::Integrity);
    }
    for (object, bytes) in &prepared.documents {
        if read(store, object, bytes.len())? != *bytes {
            return Err(Error::Integrity);
        }
    }
    Ok(result(prepared, selection.root.clone()))
}
