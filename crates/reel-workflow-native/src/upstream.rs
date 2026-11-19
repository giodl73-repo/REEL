//! Actual retained parent byte reads. This live borrowed custody is NOT durable
//! child input admission; a waiting child must acquire its own declared claims.
use crate::{NativeProfile, admission, error, io_error, retention};
use reel_workflow::{
    Artifact, ErrorKind, Packet, Result, ReturnedResult, publication::InputReader,
    upstream as contract,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
};
use tig_core::tree::{MemoryNodes, NodeStore};
use tig_core::{Digest, Object};
use tig_git::annex::workflow::reader;

pub struct Parent<'a> {
    pub packet: &'a Packet,
    pub result: &'a ReturnedResult,
    pub source: &'a NativeProfile,
    pub admission: &'a admission::Profile,
    pub receipt: &'a admission::Receipt,
    pub acknowledgement: &'a retention::Acknowledgement,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub packet: Packet,
    pub result: ReturnedResult,
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
            return Err(error(ErrorKind::Invalid, "composite-parent-controls"));
        }
        let mut parents = BTreeMap::new();
        for (id, parent) in &self.parents {
            parent.profile.binding.artifact()?;
            if parent.profile.schema != "reel.workflow-native-handoff.v1"
                || parent.profile.binding.target != parent.acknowledgement.target
            {
                return Err(error(ErrorKind::Conflict, "composite-parent-target"));
            }
            parents.insert(
                id.clone(),
                Parent {
                    packet: &parent.packet,
                    result: &parent.result,
                    source: &parent.profile.source,
                    admission: &parent.profile.admission,
                    receipt: &parent.profile.binding.admission,
                    acknowledgement: &parent.acknowledgement,
                },
            );
        }
        Ok(parents)
    }
}
/// Frozen logical origin metadata. It cannot grant first admission by itself.
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
                        },
                    )
                })
                .collect(),
        }
    }
    pub(crate) fn bound(&self, child: &Packet) -> Result<BTreeMap<String, contract::BoundInput>> {
        if self.schema != "reel.workflow-frozen-origins.v1" {
            return Err(error(ErrorKind::Invalid, "composite-origins-schema"));
        }
        let parents = self
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
            .collect();
        contract::bind(child, &self.bindings, &parents)
    }
    pub(crate) fn archive(
        &self,
        packet: &Packet,
        git_archive: &Object,
        nodes: &mut MemoryNodes,
        mut objects: BTreeMap<String, Object>,
        mut declared: BTreeSet<Object>,
    ) -> Result<(Object, BTreeMap<String, Object>, BTreeSet<Object>)> {
        let bound = self.bound(packet)?;
        let mut parents = BTreeMap::new();
        for (id, evidence) in &self.parents {
            let raw = admission::encode(evidence)?;
            let object = nodes
                .put(&raw)
                .map_err(|_| error(ErrorKind::Integrity, "composite-parent-evidence"))?;
            parents.insert(id, object);
        }
        let binding = nodes
            .put(&self.bindings.canonical_bytes()?)
            .map_err(|_| error(ErrorKind::Integrity, "composite-binding-archive"))?;
        for (id, input) in &bound {
            let object = Object {
                sha256: Digest::parse(&input.artifact.sha256)
                    .map_err(|_| error(ErrorKind::Invalid, "composite-object-digest"))?,
                bytes: input.artifact.bytes,
            };
            if objects.insert(id.clone(), object.clone()).is_some() {
                return Err(error(ErrorKind::Conflict, "composite-origin-collision"));
            }
            declared.insert(object);
        }
        let raw = admission::encode(
            &serde_json::json!({"schema":"reel.workflow-composite-source-archive.v1","git":git_archive,"bindings":binding,"parents":parents,"inputs":objects}),
        )?;
        let archive = nodes
            .put(&raw)
            .map_err(|_| error(ErrorKind::Integrity, "composite-source-archive"))?;
        Ok((archive, objects, declared))
    }
}
/// Actual parent custody stays fenced until the child's acceptance settles.
pub struct Admission<'a> {
    prepared: admission::Prepared,
    borrowed: BorrowedInputs<'a>,
}
impl<'a> Admission<'a> {
    pub fn prepare(
        child: &Packet,
        source: &NativeProfile,
        profile: &admission::Profile,
        attempt: &str,
        parents: BTreeMap<String, Parent<'a>>,
    ) -> Result<Self> {
        let frozen = source
            .snapshot
            .upstream
            .as_ref()
            .ok_or_else(|| error(ErrorKind::Missing, "composite-frozen-origins"))?;
        let expected = Frozen::capture(&frozen.bindings, &parents);
        if reel_workflow::canonical_digest(frozen)? != reel_workflow::canonical_digest(&expected)? {
            return Err(error(ErrorKind::Conflict, "composite-live-origin-mismatch"));
        }
        for parent in parents.values() {
            if reel_workflow::canonical_digest(&parent.source.source)?
                != reel_workflow::canonical_digest(&source.source)?
                || parent.admission.index_identity != profile.index_identity
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
                return Err(error(ErrorKind::Conflict, "composite-child-store-actor"));
            }
        }
        let prepared = admission::Prepared::open(child, source, profile, attempt)?;
        prepared.preview()?;
        let borrowed = BorrowedInputs::open(child, &frozen.bindings, parents)?;
        Ok(Self { prepared, borrowed })
    }
    pub fn preview(&self) -> Result<admission::Preview> {
        self.prepared.preview()
    }
    pub fn accept(mut self) -> Result<admission::Receipt> {
        // Source byte gates cannot coexist with new index work. Parent owner
        // fences and actual tool pins remain live until acceptance returns.
        self.borrowed.held.clear();
        self.borrowed.captured.clear();
        self.prepared.accept_verified_upstream()
    }
}
pub struct BorrowedInputs<'a> {
    bound: BTreeMap<String, contract::BoundInput>,
    parents: BTreeMap<String, Parent<'a>>,
    held: Vec<reader::VerifiedRead>,
    captured: BTreeMap<Object, Vec<u8>>,
    reads: usize,
    // Drop object gates before tool pins and finally retained-owner fences.
    _pins: Vec<retention::ReadPin>,
    _fences: admission::RetainedFences,
}
impl<'a> BorrowedInputs<'a> {
    pub fn open(
        child: &Packet,
        bindings: &contract::Bindings,
        parents: BTreeMap<String, Parent<'a>>,
    ) -> Result<Self> {
        let observed = parents
            .iter()
            .map(|(id, p)| {
                (
                    id.clone(),
                    contract::Parent {
                        packet: p.packet,
                        result: p.result,
                    },
                )
            })
            .collect();
        let bound = contract::bind(child, bindings, &observed)?;
        // The currently qualified native protocol declares one exact reader
        // store/pool/controller configuration. Do not imply multi-pool support.
        let mut source_pin = None;
        let mut fences = admission::RetainedFences::default();
        let mut pins = Vec::new();
        for parent in parents.values() {
            let pin = reel_workflow::canonical_digest(&parent.source.source)?;
            if source_pin.as_ref().is_some_and(|expected| expected != &pin) {
                return Err(error(
                    ErrorKind::Unsupported,
                    "upstream-single-reader-profile-required",
                ));
            }
            source_pin = Some(pin);
            pins.push(retention::pin_for_read(parent, &mut fences)?);
        }
        Ok(Self {
            bound,
            parents,
            _fences: fences,
            _pins: pins,
            held: Vec::new(),
            captured: BTreeMap::new(),
            reads: 0,
        })
    }
}
impl InputReader for BorrowedInputs<'_> {
    fn read(&mut self, asset: &Artifact, maximum: u64) -> Result<Vec<u8>> {
        let bound = self
            .bound
            .get(&asset.logical_id)
            .ok_or_else(|| error(ErrorKind::Conflict, "upstream-undeclared-read"))?;
        if &bound.artifact != asset {
            return Err(error(
                ErrorKind::Conflict,
                "upstream-exact-artifact-required",
            ));
        }
        if asset.bytes > maximum || self.reads >= 256 {
            return Err(error(ErrorKind::Budget, "upstream-read-allowance"));
        }
        self.reads += 1;
        let object = Object {
            sha256: Digest::parse(&asset.sha256)
                .map_err(|_| error(ErrorKind::Invalid, "upstream-input-digest"))?,
            bytes: asset.bytes,
        };
        // Logical task/result/selector identity was checked before physical reuse.
        if let Some(raw) = self.captured.get(&object) {
            return Ok(raw.clone());
        }
        let parent = &self.parents[&bound.reference.task_id];
        let mut held = reader::open(&parent.source.source, &object).map_err(io_error)?;
        let mut raw = Vec::new();
        Read::by_ref(&mut held)
            .take(asset.bytes + 1)
            .read_to_end(&mut raw)
            .map_err(io_error)?;
        if raw.len() as u64 != asset.bytes || reel_workflow::digest(&raw) != asset.sha256 {
            return Err(error(ErrorKind::Integrity, "upstream-input-content"));
        }
        self.held.push(held);
        self.captured.insert(object, raw.clone());
        Ok(raw)
    }
}
