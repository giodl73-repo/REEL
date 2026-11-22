//! Immutable task binding: predeclared seeds, no self-referential control hash.
//! Decoded data is not admission, byte custody, owner auth or task completion.
use crate::{Preview, claims::Handle, error, retention::Seed};
use git_asset_core::{
    Digest, LogicalPath, Object,
    tree::{MemoryNodes, NodeStore, Tree, TreeLimits},
};
use git_asset_store::source_claims::admission::Select;
use reel_workflow::{Artifact, ErrorKind, Packet, Result, handoff::Ticket};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
const SCHEMA: &str = "reel.workflow-public-task-binding.v1";
const MAX: usize = 128 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    schema: String,
    binding_id: String,
    attempt: String,
    packet_value: Object,
    controls: BTreeMap<String, Artifact>,
    inputs: Vec<Artifact>,
    claim: Vec<u8>,
    control_seed: Vec<u8>,
    result_seed: Vec<u8>,
    retained_seed: Vec<u8>,
}
pub struct Capture<'a> {
    pub binding_id: &'a str,
    pub packet: &'a Packet,
    pub original_packet: &'a [u8],
    pub attempt: &'a str,
    pub preview: &'a Preview,
    pub claim: &'a Handle,
    pub controls: BTreeMap<String, Artifact>,
    pub control_seed: &'a Seed,
    pub result_seed: &'a Seed,
    pub retained_seed: &'a Seed,
}
pub struct Binding {
    wire: Wire,
    encoded: Vec<u8>,
    claim: Handle,
    control_seed: Seed,
    result_seed: Seed,
    retained_seed: Seed,
}
pub fn object(a: &Artifact) -> Result<Object> {
    reel_publication::check_artifact(a).map_err(reel_workflow::Error::from)?;
    Ok(Object {
        sha256: Digest::parse(&a.sha256)
            .map_err(|_| error(ErrorKind::Invalid, "public-binding-artifact"))?,
        bytes: a.bytes,
    })
}
fn json_object(value: &Packet) -> Result<Object> {
    reel_workflow::check_packet(value)?;
    let raw =
        serde_json::to_vec(value).map_err(|_| error(ErrorKind::Invalid, "public-binding-json"))?;
    if raw.len() > 1024 * 1024 {
        return Err(error(ErrorKind::Budget, "public-binding-json"));
    }
    Ok(Object::from_bytes(&raw))
}
fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
/// Fixed resource paths; input IDs never become physical or metadata paths.
pub fn resources(
    packet: &Packet,
    preview: &Preview,
    controls: &BTreeMap<String, Artifact>,
) -> Result<BTreeMap<LogicalPath, Object>> {
    reel_workflow::check_packet(packet)?;
    preview.encode()?;
    if controls.len() < 4 || controls.len() > 32 {
        return Err(error(ErrorKind::Budget, "public-binding-controls"));
    }
    for required in ["packet", "request", "preview", "tool"] {
        if !controls.contains_key(required) {
            return Err(error(ErrorKind::Missing, "public-binding-control-role"));
        }
    }
    if preview.commit != packet.source_commit {
        return Err(error(ErrorKind::Conflict, "public-binding-commit"));
    }
    let mut expected = BTreeMap::new();
    for a in packet.task.inputs.iter().chain(
        packet
            .dependencies
            .values()
            .flat_map(|d| d.artifacts.values()),
    ) {
        object(a)?;
        if expected
            .insert(a.logical_id.clone(), a.clone())
            .is_some_and(|old| old != *a)
        {
            return Err(error(ErrorKind::Conflict, "public-binding-input-identity"));
        }
    }
    let actual = preview
        .inputs
        .iter()
        .map(|i| (i.artifact.logical_id.clone(), i.artifact.clone()))
        .collect::<BTreeMap<_, _>>();
    if actual.len() != preview.inputs.len() || actual != expected {
        return Err(error(ErrorKind::Conflict, "public-binding-input-closure"));
    }
    for a in packet
        .task
        .native_inputs
        .iter()
        .chain(packet.task.upstream_inputs.iter())
    {
        if !controls.values().any(|c| c == a) {
            return Err(error(ErrorKind::Missing, "public-binding-declaration"));
        }
    }
    if object(&controls["preview"])? != Object::from_bytes(&preview.encode()?) {
        return Err(error(ErrorKind::Conflict, "public-binding-preview"));
    }
    let mut entries = BTreeMap::new();
    let mut identities = actual;
    for (key, a) in controls {
        if !token(key) {
            return Err(error(ErrorKind::Invalid, "public-binding-control-role"));
        }
        object(a)?;
        if identities
            .insert(a.logical_id.clone(), a.clone())
            .is_some_and(|old| old != *a)
        {
            return Err(error(
                ErrorKind::Conflict,
                "public-binding-control-identity",
            ));
        }
        entries.insert(
            LogicalPath::parse(&format!("control/{key}"))
                .map_err(|_| error(ErrorKind::Invalid, "public-binding-path"))?,
            object(a)?,
        );
    }
    for (i, input) in preview.inputs.iter().enumerate() {
        if object(&input.artifact)? != input.object {
            return Err(error(ErrorKind::Integrity, "public-binding-input-object"));
        }
        entries.insert(
            LogicalPath::parse(&format!("input/{i:03}"))
                .map_err(|_| error(ErrorKind::Invalid, "public-binding-path"))?,
            input.object.clone(),
        );
    }
    Ok(entries)
}
impl Binding {
    pub fn capture(c: Capture<'_>) -> Result<Self> {
        if !token(c.attempt) || c.original_packet.len() > 1024 * 1024 {
            return Err(error(ErrorKind::Invalid, "public-binding-attempt-packet"));
        }
        if c.control_seed.same_source(c.result_seed)
            || c.control_seed.same_source(c.retained_seed)
            || c.result_seed.same_source(c.retained_seed)
        {
            return Err(error(ErrorKind::Conflict, "public-binding-distinct-seeds"));
        }
        let entries = resources(c.packet, c.preview, &c.controls)?;
        let parsed: Packet = serde_json::from_slice(c.original_packet)
            .map_err(|_| error(ErrorKind::Invalid, "public-binding-packet"))?;
        if parsed != *c.packet
            || object(&c.controls["packet"])? != Object::from_bytes(c.original_packet)
        {
            return Err(error(ErrorKind::Conflict, "public-binding-original-packet"));
        }
        let (tree, _) = Tree::build(&entries, &mut MemoryNodes::default(), TreeLimits::default())
            .map_err(|_| error(ErrorKind::Invalid, "public-binding-tree"))?;
        if c.claim.request().selection.root != *tree.root()
            || c.claim.request().selection.context
                != Object::from_bytes(&c.preview.encode()?).sha256
        {
            return Err(error(ErrorKind::Conflict, "public-binding-job-closure"));
        }
        let wire = Wire {
            schema: SCHEMA.into(),
            binding_id: c.binding_id.into(),
            attempt: c.attempt.into(),
            packet_value: json_object(c.packet)?,
            controls: c.controls,
            inputs: c
                .preview
                .inputs
                .iter()
                .map(|i| i.artifact.clone())
                .collect(),
            claim: c.claim.encode()?,
            control_seed: c.control_seed.encode()?,
            result_seed: c.result_seed.encode()?,
            retained_seed: c.retained_seed.encode()?,
        };
        let encoded = serde_json::to_vec(&wire)
            .map_err(|_| error(ErrorKind::Invalid, "public-binding-json"))?;
        Self::decode(&encoded, &Object::from_bytes(&encoded))
    }
    /// Check the original external pin; this never admits or reopens resources.
    pub fn decode(raw: &[u8], expected: &Object) -> Result<Self> {
        if raw.len() > MAX {
            return Err(error(ErrorKind::Budget, "public-binding-bytes"));
        }
        if &Object::from_bytes(raw) != expected {
            return Err(error(ErrorKind::Integrity, "public-binding-bytes"));
        }
        let wire: Wire = serde_json::from_slice(raw)
            .map_err(|_| error(ErrorKind::Invalid, "public-binding-wire"))?;
        if wire.schema != SCHEMA
            || !token(&wire.attempt)
            || wire.controls.len() < 4
            || wire.controls.len() > 32
            || wire.inputs.is_empty()
            || wire.inputs.len() > 128
        {
            return Err(error(ErrorKind::Invalid, "public-binding-wire"));
        }
        for required in ["packet", "request", "preview", "tool"] {
            if !wire.controls.contains_key(required) {
                return Err(error(ErrorKind::Missing, "public-binding-control-role"));
            }
        }
        for (role, a) in &wire.controls {
            if !token(role) {
                return Err(error(ErrorKind::Invalid, "public-binding-control-role"));
            }
            object(a)?;
        }
        for a in &wire.inputs {
            object(a)?;
        }
        let claim = Handle::decode(&wire.claim, &Object::from_bytes(&wire.claim))?;
        let control_seed =
            Seed::decode(&wire.control_seed, &Object::from_bytes(&wire.control_seed))?;
        let result_seed = Seed::decode(&wire.result_seed, &Object::from_bytes(&wire.result_seed))?;
        let retained_seed = Seed::decode(
            &wire.retained_seed,
            &Object::from_bytes(&wire.retained_seed),
        )?;
        if control_seed.same_source(&result_seed)
            || control_seed.same_source(&retained_seed)
            || result_seed.same_source(&retained_seed)
        {
            return Err(error(ErrorKind::Conflict, "public-binding-distinct-seeds"));
        }
        object(&Artifact {
            logical_id: wire.binding_id.clone(),
            sha256: expected.sha256.as_str().into(),
            bytes: expected.bytes,
            media_type: "application/json".into(),
        })?;
        object(&Artifact {
            logical_id: format!("{}/return-packet", wire.binding_id),
            sha256: expected.sha256.as_str().into(),
            bytes: expected.bytes,
            media_type: "application/json".into(),
        })?;
        Ok(Self {
            wire,
            encoded: raw.to_vec(),
            claim,
            control_seed,
            result_seed,
            retained_seed,
        })
    }
    pub fn control(&self) -> &[u8] {
        &self.encoded
    }
    pub fn pin(&self) -> Object {
        Object::from_bytes(&self.encoded)
    }
    pub fn claim(&self) -> &Handle {
        &self.claim
    }
    pub fn control_seed(&self) -> &Seed {
        &self.control_seed
    }
    pub fn result_seed(&self) -> &Seed {
        &self.result_seed
    }
    pub fn retained_seed(&self) -> &Seed {
        &self.retained_seed
    }
    pub fn controls(&self) -> &BTreeMap<String, Artifact> {
        &self.wire.controls
    }
    pub fn return_packet_id(&self) -> String {
        format!("{}/return-packet", self.wire.binding_id)
    }
    pub fn artifact(&self) -> Artifact {
        let p = self.pin();
        Artifact {
            logical_id: self.wire.binding_id.clone(),
            sha256: p.sha256.as_str().into(),
            bytes: p.bytes,
            media_type: "application/json".into(),
        }
    }
    pub fn ticket(&self, packet: Packet) -> Result<Ticket> {
        if json_object(&packet)? != self.wire.packet_value {
            return Err(error(ErrorKind::Conflict, "public-binding-packet"));
        }
        Ok(Ticket {
            packet,
            attempt_id: self.wire.attempt.clone(),
            native_binding: self.artifact(),
            lifecycle_tool: self.wire.controls["tool"].clone(),
        })
    }
    /// Semantic correspondence only; retain actual bytes and generation separately.
    pub fn verify(&self, ticket: &Ticket, preview: &Preview, original_packet: &[u8]) -> Result<()> {
        if ticket.native_binding != self.artifact()
            || ticket.attempt_id != self.wire.attempt
            || ticket.lifecycle_tool != self.wire.controls["tool"]
            || json_object(&ticket.packet)? != self.wire.packet_value
        {
            return Err(error(ErrorKind::Conflict, "public-binding-ticket"));
        }
        if original_packet.len() > 1024 * 1024 {
            return Err(error(ErrorKind::Budget, "public-binding-original-packet"));
        }
        let parsed: Packet = serde_json::from_slice(original_packet)
            .map_err(|_| error(ErrorKind::Invalid, "public-binding-packet"))?;
        if parsed != ticket.packet
            || Object::from_bytes(original_packet) != object(&self.wire.controls["packet"])?
        {
            return Err(error(ErrorKind::Conflict, "public-binding-original-packet"));
        }
        if preview
            .inputs
            .iter()
            .map(|i| i.artifact.clone())
            .collect::<Vec<_>>()
            != self.wire.inputs
        {
            return Err(error(ErrorKind::Conflict, "public-binding-preview-inputs"));
        }
        let entries = resources(&ticket.packet, preview, &self.wire.controls)?;
        let (tree, _) = Tree::build(&entries, &mut MemoryNodes::default(), TreeLimits::default())
            .map_err(|_| error(ErrorKind::Invalid, "public-binding-tree"))?;
        if self.claim.request().selection.root != *tree.root()
            || self.claim.request().selection.context
                != Object::from_bytes(&preview.encode()?).sha256
        {
            return Err(error(ErrorKind::Conflict, "public-binding-job-closure"));
        }
        Ok(())
    }
    /// Build control metadata AFTER binding bytes are fixed. The selection is not
    /// embedded back into the binding. All original control bytes remain included.
    pub fn control_selection(&self, nodes: &mut impl NodeStore) -> Result<Select> {
        let mut entries = self
            .wire
            .controls
            .iter()
            .map(|(r, a)| {
                Ok((
                    LogicalPath::parse(&format!("control/{r}"))
                        .map_err(|_| error(ErrorKind::Invalid, "public-binding-path"))?,
                    object(a)?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        entries.insert(
            LogicalPath::parse("binding")
                .map_err(|_| error(ErrorKind::Invalid, "public-binding-path"))?,
            self.pin(),
        );
        self.control_seed
            .selection(&entries, self.pin().sha256, nodes)
    }
}
