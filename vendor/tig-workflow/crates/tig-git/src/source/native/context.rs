//! Frozen declared source/descriptor context through existing metadata ports.
//! Candidate roots require provider finish/readback and owner adoption separately.
use super::{
    AssetSnapshot, Error, Limits, Result,
    archive::{put, read, tree_error},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use tig_assets::{
    graph::{self, ProductionRequirements, Reference},
    native::{Instance, MAX_RECORD_BYTES},
};
use tig_core::{
    Digest, LogicalPath, Object, identity,
    tree::{NodeStore, Tree, TreeLimits},
};
use tig_store::source_claims::Selection;

const SCHEMA: &str = "tig.native-production-context.experimental-1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: String,
    checkpoint: Digest,
    admission: Digest,
    binding: Digest,
    archive: Object,
    policy: Object,
    descriptor: Object,
    roots: usize,
    primary: usize,
    controls: usize,
    interpretation: usize,
}

/// Exact declared source context, not execution, availability or custody proof.
#[derive(Debug)]
pub struct ProductionContext {
    selection: Selection,
    archive: Object,
    policy: Object,
    descriptor: Vec<u8>,
    roots: BTreeSet<Reference>,
    requirements: ProductionRequirements,
}
impl ProductionContext {
    pub fn selection(&self) -> &Selection {
        &self.selection
    }
    pub fn archive(&self) -> &Object {
        &self.archive
    }
    pub fn policy(&self) -> &Object {
        &self.policy
    }
    /// Opaque private bytes; external references are not inferred or executed.
    pub fn descriptor(&self) -> &[u8] {
        &self.descriptor
    }
    pub fn roots(&self) -> &BTreeSet<Reference> {
        &self.roots
    }
    pub fn requirements(&self) -> &ProductionRequirements {
        &self.requirements
    }
}

fn path(value: &str) -> Result<LogicalPath> {
    LogicalPath::parse(value).map_err(|_| Error::Invalid)
}
fn entries(
    header: &Header,
    identity: &Object,
    roots: &BTreeSet<Reference>,
    requirements: &ProductionRequirements,
) -> Result<BTreeMap<LogicalPath, Object>> {
    let mut entries = BTreeMap::from([
        (path("context/identity")?, identity.clone()),
        (path("source/archive")?, header.archive.clone()),
        (path("source/policy")?, header.policy.clone()),
        (path("operation/descriptor")?, header.descriptor.clone()),
    ]);
    for reference in roots {
        entries.insert(
            path(&format!("roots/{}", reference.object.sha256.as_str()))?,
            reference.object.clone(),
        );
    }
    for (namespace, objects) in [
        ("primary", requirements.primary()),
        ("controls", requirements.controls()),
        ("interpretation", requirements.metadata()),
    ] {
        for object in objects {
            entries.insert(
                path(&format!("{namespace}/{}", object.sha256.as_str()))?,
                object.clone(),
            );
        }
    }
    Ok(entries)
}
fn count(header: &Header) -> Result<usize> {
    [
        header.roots,
        header.primary,
        header.controls,
        header.interpretation,
    ]
    .into_iter()
    .try_fold(4usize, |sum, n| sum.checked_add(n).ok_or(Error::Budget))
}

struct Loaded {
    bindings: BTreeMap<LogicalPath, Object>,
    header: Header,
    identity: Object,
}
fn load(selection: &Selection, store: &mut impl NodeStore, limits: TreeLimits) -> Result<Loaded> {
    let bindings = Tree::from_root(selection.root.clone())
        .entries(store, limits)
        .map_err(tree_error)?
        .0;
    let identity = bindings
        .get(&path("context/identity")?)
        .ok_or(Error::Incomplete)?
        .clone();
    if identity.sha256 != selection.context {
        return Err(Error::Integrity);
    }
    let bytes = read(store, &identity, MAX_RECORD_BYTES)?;
    let value = identity::parse_json(&bytes).map_err(|_| Error::Integrity)?;
    let header: Header = serde_json::from_value(value).map_err(|_| Error::Integrity)?;
    if header.schema != SCHEMA {
        return Err(Error::Invalid);
    }
    if header.roots == 0 || count(&header)? > limits.max_entries {
        return Err(Error::Budget);
    }
    if count(&header)? != bindings.len()
        || header.policy.sha256 != header.admission
        || bindings.get(&path("source/archive")?) != Some(&header.archive)
        || bindings.get(&path("source/policy")?) != Some(&header.policy)
        || bindings.get(&path("operation/descriptor")?) != Some(&header.descriptor)
    {
        return Err(Error::Integrity);
    }
    Ok(Loaded {
        bindings,
        header,
        identity,
    })
}

/// Read identity-bound private descriptor bytes so a caller can choose its
/// externally admitted decoder/policy. This is NOT complete context replay,
/// semantic validation, authority or permission to execute the descriptor.
pub fn read_descriptor(
    selection: &Selection,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<Vec<u8>> {
    let loaded = load(selection, store, limits)?;
    read(store, &loaded.header.descriptor, MAX_RECORD_BYTES)
}

impl AssetSnapshot {
    /// Resolve against this snapshot, then persist a candidate context root.
    /// No payload bytes are read; finish/readback and owner adoption are external.
    pub fn freeze_production_context(
        &mut self,
        query: &graph::Query,
        profiles: &[graph::Profile],
        descriptor: &[u8],
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<ProductionContext> {
        if query.checkpoint != self.checkpoint || descriptor.len() > MAX_RECORD_BYTES {
            return Err(if descriptor.len() > MAX_RECORD_BYTES {
                Error::Budget
            } else {
                Error::Invalid
            });
        }
        let graph = graph::resolve(self, query, profiles).map_err(|_| Error::Invalid)?;
        let requirements = graph
            .production_requirements()
            .map_err(|_| Error::Incomplete)?;
        let required_entries = [
            graph.roots().len(),
            requirements.primary().len(),
            requirements.controls().len(),
            requirements.metadata().len(),
        ]
        .into_iter()
        .try_fold(4usize, |sum, n| sum.checked_add(n).ok_or(Error::Budget))?;
        if required_entries > limits.max_entries {
            return Err(Error::Budget);
        }
        let archive = self.archive_metadata(store, limits)?;
        let policy = put(store, graph.admission_document())?;
        if &policy.sha256 != graph.admission() {
            return Err(Error::Integrity);
        }
        let descriptor_object = put(store, descriptor)?;
        let header = Header {
            schema: SCHEMA.into(),
            checkpoint: self.checkpoint.clone(),
            admission: graph.admission().clone(),
            binding: requirements.binding_digest().clone(),
            archive: archive.clone(),
            policy: policy.clone(),
            descriptor: descriptor_object,
            roots: graph.roots().len(),
            primary: requirements.primary().len(),
            controls: requirements.controls().len(),
            interpretation: requirements.metadata().len(),
        };
        let bytes =
            identity::canonical_json(&serde_json::to_value(&header).map_err(|_| Error::Invalid)?)
                .map_err(|_| Error::Invalid)?;
        let header_object = put(store, &bytes)?;
        let bindings = entries(&header, &header_object, graph.roots(), &requirements)?;
        let tree = Tree::build(&bindings, store, limits).map_err(tree_error)?.0;
        Ok(ProductionContext {
            selection: Selection {
                root: tree.root().clone(),
                context: header_object.sha256,
            },
            archive,
            policy,
            descriptor: descriptor.to_vec(),
            roots: graph.roots().clone(),
            requirements,
        })
    }

    /// Replay only under externally admitted selection/profiles; stored policy
    /// bytes do not grant authority. Returns the exact source and declared context.
    pub fn from_production_context(
        selection: &Selection,
        profiles: &[graph::Profile],
        store: &mut impl NodeStore,
        native_limits: Limits,
        tree_limits: TreeLimits,
        query_limits: graph::Limits,
    ) -> Result<(Self, ProductionContext)> {
        let Loaded {
            bindings,
            header,
            identity,
        } = load(selection, store, tree_limits)?;
        if header.roots > query_limits.nodes {
            return Err(Error::Budget);
        }
        let policy_bytes = read(store, &header.policy, MAX_RECORD_BYTES)?;
        let descriptor = read(store, &header.descriptor, MAX_RECORD_BYTES)?;
        let mut source =
            Self::from_metadata_archive(store, &header.archive, native_limits, tree_limits)?;
        if source.checkpoint != header.checkpoint {
            return Err(Error::Integrity);
        }
        if header.roots > source.records.len() || header.interpretation > source.blobs.len() {
            return Err(Error::Integrity);
        }
        let mut roots = BTreeSet::new();
        for (key, object) in &bindings {
            if key.components().next() != Some("roots") {
                continue;
            }
            // Root documents must already belong to the verified archive. This
            // avoids a second, unaccounted stream of arbitrary provider reads.
            let bytes = source.blobs.get(object).ok_or(Error::Incomplete)?;
            let record = Instance::parse(bytes).map_err(|_| Error::Integrity)?;
            let reference = Reference {
                id: record.value().id.clone(),
                object: object.clone(),
            };
            if *key != path(&format!("roots/{}", object.sha256.as_str()))?
                || !roots.insert(reference)
            {
                return Err(Error::Integrity);
            }
        }
        if roots.len() != header.roots {
            return Err(Error::Integrity);
        }
        let query = graph::Query {
            checkpoint: header.checkpoint.clone(),
            roots: roots.clone(),
            kinds: BTreeSet::from([graph::EdgeKind::Production]),
            limits: query_limits,
        };
        let graph = graph::resolve(&mut source, &query, profiles).map_err(|_| Error::Invalid)?;
        let requirements = graph
            .production_requirements()
            .map_err(|_| Error::Incomplete)?;
        if graph.admission_document() != policy_bytes
            || graph.admission() != &header.admission
            || requirements.binding_digest() != &header.binding
            || requirements.primary().len() != header.primary
            || requirements.controls().len() != header.controls
            || requirements.metadata().len() != header.interpretation
            || entries(&header, &identity, &roots, &requirements)? != bindings
        {
            return Err(Error::Integrity);
        }
        let context = ProductionContext {
            selection: selection.clone(),
            archive: header.archive,
            policy: header.policy,
            descriptor,
            roots,
            requirements,
        };
        Ok((source, context))
    }
}
