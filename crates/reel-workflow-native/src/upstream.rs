//! Retained dependency origins and child-owned input admission. No new store,
//! cache, provider, publication, selection or parent-retention release protocol.
use crate::{NativeProfile, admission, error, io_error, retention};
use reel_workflow::{ErrorKind, Packet, Result, ReturnedResult, upstream as contract};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use tig_core::{
    Digest, Object,
    tree::{MemoryNodes, NodeStore},
};
use tig_store::{
    Key,
    source_claims::{Selection, protection::work::WorkRef},
};

pub struct Parent<'a> {
    pub packet: &'a Packet,
    pub result: &'a ReturnedResult,
    pub source: &'a NativeProfile,
    pub admission: &'a admission::Profile,
    pub receipt: &'a admission::Receipt,
    pub acknowledgement: &'a retention::Acknowledgement,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParentControl {
    pub packet: Packet,
    pub result: ReturnedResult,
    pub profile: crate::handoff::Profile,
    pub acknowledgement: retention::Acknowledgement,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Parents {
    pub schema: String,
    pub parents: BTreeMap<String, ParentControl>,
}
impl Parents {
    pub fn borrowed(&self) -> Result<BTreeMap<String, Parent<'_>>> {
        if self.schema != "reel.workflow-native-parents.v1"
            || self.parents.is_empty()
            || self.parents.len() > 128
        {
            return Err(error(ErrorKind::Invalid, "upstream-parent-controls"));
        }
        self.parents
            .iter()
            .map(|(id, p)| {
                p.profile.binding.artifact()?;
                if p.profile.schema != "reel.workflow-native-handoff.v1"
                    || p.profile.binding.target != p.acknowledgement.target
                {
                    return Err(error(ErrorKind::Conflict, "upstream-parent-target"));
                }
                Ok((
                    id.clone(),
                    Parent {
                        packet: &p.packet,
                        result: &p.result,
                        source: &p.profile.source,
                        admission: &p.profile.admission,
                        receipt: &p.profile.binding.admission,
                        acknowledgement: &p.acknowledgement,
                    },
                ))
            })
            .collect()
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub packet: Packet,
    pub result: ReturnedResult,
    pub origin: Origin,
}
/// Stable native operation witnesses; machine paths are not authored task
/// identity. This data alone is never evidence of live parent custody.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub index: Digest,
    pub store: Digest,
    pub actor: Key,
    pub owner: Key,
    pub operation: Key,
    pub policy: Digest,
    pub input: Selection,
    pub source_archive: Object,
    pub activation: Object,
    pub admission_work: WorkRef,
    pub acknowledgement: retention::Acknowledgement,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Frozen {
    pub schema: String,
    pub bindings: contract::Bindings,
    pub parents: BTreeMap<String, Evidence>,
}
impl Frozen {
    pub fn capture(bindings: &contract::Bindings, parents: &BTreeMap<String, Parent<'_>>) -> Self {
        Self {
            schema: "reel.workflow-frozen-origins.v1".into(),
            bindings: bindings.clone(),
            parents: parents
                .iter()
                .map(|(id, p)| {
                    (
                        id.clone(),
                        Evidence {
                            packet: p.packet.clone(),
                            result: p.result.clone(),
                            origin: Origin {
                                index: p.admission.index_identity.clone(),
                                store: p.source.source.store.clone(),
                                actor: p.receipt.scope.actor.clone(),
                                owner: p.receipt.scope.owner.clone(),
                                operation: p.receipt.scope.operation.clone(),
                                policy: p.receipt.scope.policy.clone(),
                                input: p.receipt.input.clone(),
                                source_archive: p.receipt.source_archive.clone(),
                                activation: p.receipt.activation.clone(),
                                admission_work: p.receipt.work.clone(),
                                acknowledgement: p.acknowledgement.clone(),
                            },
                        },
                    )
                })
                .collect(),
        }
    }
    pub(crate) fn declaration_pins(&self) -> Result<serde_json::Value> {
        let mut parents = BTreeMap::new();
        for (id, evidence) in &self.parents {
            parents.insert(
                id,
                Object::from_bytes(&admission::encode(&(&evidence.packet, &evidence.result))?),
            );
        }
        Ok(
            serde_json::json!({"schema":"reel.workflow-frozen-origin-pins.v1","bindings":Object::from_bytes(&self.bindings.canonical_bytes()?),"parents":parents}),
        )
    }
    pub(crate) fn bound(&self, child: &Packet) -> Result<BTreeMap<String, contract::BoundInput>> {
        if self.schema != "reel.workflow-frozen-origins.v1" {
            return Err(error(ErrorKind::Invalid, "upstream-frozen-schema"));
        }
        contract::bind(
            child,
            &self.bindings,
            &self
                .parents
                .iter()
                .map(|(id, p)| {
                    (
                        id.clone(),
                        contract::Parent {
                            packet: &p.packet,
                            result: &p.result,
                        },
                    )
                })
                .collect(),
        )
    }
    pub(crate) fn archive(
        &self,
        child: &Packet,
        git: &Object,
        nodes: &mut MemoryNodes,
        mut objects: BTreeMap<String, Object>,
        mut declared: BTreeSet<Object>,
    ) -> Result<(Object, BTreeMap<String, Object>, BTreeSet<Object>)> {
        let bound = self.bound(child)?;
        let mut parents = BTreeMap::new();
        for (id, evidence) in &self.parents {
            parents.insert(
                id,
                nodes
                    .put(&admission::encode(evidence)?)
                    .map_err(|_| error(ErrorKind::Integrity, "upstream-evidence-archive"))?,
            );
        }
        let bindings = nodes
            .put(&self.bindings.canonical_bytes()?)
            .map_err(|_| error(ErrorKind::Integrity, "upstream-binding-archive"))?;
        for (id, input) in bound {
            let object = Object {
                sha256: Digest::parse(&input.artifact.sha256)
                    .map_err(|_| error(ErrorKind::Invalid, "upstream-object-digest"))?,
                bytes: input.artifact.bytes,
            };
            if objects.insert(id, object.clone()).is_some() {
                return Err(error(ErrorKind::Conflict, "upstream-origin-collision"));
            }
            declared.insert(object);
        }
        let raw = admission::encode(
            &serde_json::json!({"schema":"reel.workflow-composite-source-archive.v1","git":git,"bindings":bindings,"parents":parents,"inputs":objects}),
        )?;
        let archive = nodes
            .put(&raw)
            .map_err(|_| error(ErrorKind::Integrity, "upstream-source-archive"))?;
        Ok((archive, objects, declared))
    }
}
pub struct Admission {
    prepared: admission::Prepared,
    _pins: Vec<retention::ReadPin>,
    _fences: admission::RetainedFences,
    _roots: Vec<reel_assets::jobs::WorkspaceDirectory>,
}
fn store_pin(source: &tig_git::annex::workflow::reader::Profile) -> Result<String> {
    reel_workflow::canonical_digest(&(&source.pool, &source.store))
}
impl Admission {
    pub fn prepare(
        child: &Packet,
        source: &NativeProfile,
        profile: &admission::Profile,
        attempt: &str,
        parents: BTreeMap<String, Parent<'_>>,
    ) -> Result<Self> {
        let mut root_paths = BTreeSet::from([
            profile.index_root.clone(),
            profile.metadata_root.clone(),
            profile.scope.coordination_root.clone(),
        ]);
        for parent in parents.values() {
            root_paths.extend([
                parent.admission.index_root.clone(),
                parent.admission.metadata_root.clone(),
                parent.admission.scope.coordination_root.clone(),
            ]);
        }
        let roots = root_paths
            .iter()
            .map(|path| {
                reel_assets::jobs::open_workspace_directory(path)
                    .map_err(|_| error(ErrorKind::Denied, "upstream-controller-namespace"))
            })
            .collect::<Result<Vec<_>>>()?;
        let frozen = source
            .snapshot
            .upstream
            .as_ref()
            .ok_or_else(|| error(ErrorKind::Missing, "upstream-frozen-origins"))?;
        let expected = Frozen::capture(&frozen.bindings, &parents);
        if reel_workflow::canonical_digest(frozen)? != reel_workflow::canonical_digest(&expected)? {
            return Err(error(ErrorKind::Conflict, "upstream-live-origin-mismatch"));
        }
        for parent in parents.values() {
            if store_pin(&parent.source.source)? != store_pin(&source.source)?
                || parent.admission.index_identity != profile.index_identity
                || parent
                    .admission
                    .index_root
                    .canonicalize()
                    .map_err(io_error)?
                    != profile.index_root.canonicalize().map_err(io_error)?
                || parent
                    .admission
                    .metadata_root
                    .canonicalize()
                    .map_err(io_error)?
                    != profile.metadata_root.canonicalize().map_err(io_error)?
                || parent
                    .admission
                    .scope
                    .coordination_root
                    .canonicalize()
                    .map_err(io_error)?
                    != profile
                        .scope
                        .coordination_root
                        .canonicalize()
                        .map_err(io_error)?
                || parent.acknowledgement.target.actor == profile.scope.actor
            {
                return Err(error(ErrorKind::Conflict, "upstream-child-store-actor"));
            }
        }
        let prepared = admission::Prepared::open(child, source, profile, attempt)?;
        prepared.preview()?;
        let mut fences = admission::RetainedFences::default();
        let pins = parents
            .values()
            .map(|p| retention::pin_for_read(p, &mut fences))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            prepared,
            _pins: pins,
            _fences: fences,
            _roots: roots,
        })
    }
    pub fn preview(&self) -> Result<admission::Preview> {
        self.prepared.preview()
    }
    /// Parents remain fenced until the child's positive claims settle. Only
    /// child claims, never these borrowed parent witnesses, authorize production.
    pub fn accept(self) -> Result<admission::Receipt> {
        self.prepared.accept_verified_upstream()
    }
}
