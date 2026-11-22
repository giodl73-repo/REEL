//! Returned candidates over existing native publication. No creative selection,
//! task completion, scratch retirement or replacement operation allocation.
use crate::{error, publication_error};
use git_asset::annex::publication::{self, Plan, Publisher, Ready, Resources};
use git_asset_core::{Digest, Object, identity};
use git_asset_store::existing::{self, ByteSource};
use reel_workflow::{Artifact, Error, ErrorKind, Result, ReturnedResult, handoff::Ticket};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{self, Cursor, Read, Seek, SeekFrom},
    sync::Arc,
};
const MAX_PACKET: usize = 1024 * 1024;
pub const RETURN_PACKET: &str = "return-packet";
pub const WORKER_RECEIPT: &str = "worker-receipt";
/// Exact original returned bytes plus validated task/attempt and output contracts.
/// This describes the declared set; it grants no source/job or byte custody.
pub struct Candidates {
    raw: Arc<[u8]>,
    result: ReturnedResult,
    expected: BTreeMap<String, Artifact>,
    provenance: Vec<u8>,
}
impl Candidates {
    pub fn bind(ticket: &Ticket, raw: &[u8], packet_logical_id: &str) -> Result<Self> {
        if raw.len() > MAX_PACKET {
            return Err(error(ErrorKind::Budget, "public-git-asset-return-packet"));
        }
        let result: ReturnedResult = serde_json::from_slice(raw)
            .map_err(|_| error(ErrorKind::Invalid, "public-git-asset-return-json"))?;
        reel_workflow::check_result(&ticket.packet, &result)?;
        reel_publication::check_artifact(&ticket.native_binding).map_err(Error::from)?;
        reel_publication::check_artifact(&ticket.lifecycle_tool).map_err(Error::from)?;
        if result.attempt_id != ticket.attempt_id {
            return Err(error(
                ErrorKind::Conflict,
                "public-git-asset-return-attempt",
            ));
        }
        if result.artifacts.len() > 128 {
            return Err(error(ErrorKind::Budget, "public-git-asset-return-count"));
        }
        let object = Object::from_bytes(raw);
        let packet = Artifact {
            logical_id: packet_logical_id.into(),
            sha256: object.sha256.as_str().into(),
            bytes: object.bytes,
            media_type: "application/json".into(),
        };
        reel_publication::check_artifact(&packet).map_err(Error::from)?;
        if result
            .artifacts
            .values()
            .chain(result.worker_receipt.iter())
            .any(|a| a.logical_id == packet.logical_id)
        {
            return Err(error(
                ErrorKind::Conflict,
                "public-git-asset-return-identity",
            ));
        }
        let mut expected = result
            .artifacts
            .iter()
            .map(|(role, a)| (format!("output/{role}"), a.clone()))
            .collect::<BTreeMap<_, _>>();
        if let Some(receipt) = &result.worker_receipt {
            expected.insert(WORKER_RECEIPT.into(), receipt.clone());
        }
        expected.insert(RETURN_PACKET.into(), packet);
        let ticket_bytes = serde_json::to_vec(ticket)
            .map_err(|_| error(ErrorKind::Invalid, "public-git-asset-return-ticket"))?;
        if ticket_bytes.len() > MAX_PACKET {
            return Err(error(ErrorKind::Budget, "public-git-asset-return-ticket"));
        }
        let provenance=identity::canonical_json(&serde_json::json!({
            "schema":"reel.workflow-public-return-provenance.v1","ticket":Object::from_bytes(&ticket_bytes),
            "original_return":object,"provenance":result.provenance
        })).map_err(|_|error(ErrorKind::Invalid,"public-git-asset-return-provenance"))?;
        if provenance.len() > 4096 {
            return Err(error(
                ErrorKind::Budget,
                "public-git-asset-return-provenance",
            ));
        }
        Ok(Self {
            raw: Arc::from(raw),
            result,
            expected,
            provenance,
        })
    }
    pub fn result(&self) -> &ReturnedResult {
        &self.result
    }
    pub fn original_packet(&self) -> &[u8] {
        &self.raw
    }
    pub fn provenance(&self) -> &[u8] {
        &self.provenance
    }
    pub fn expected(&self) -> &BTreeMap<String, Artifact> {
        &self.expected
    }
    /// Only the immutable original returned JSON body; never another output.
    pub fn packet_source(&self) -> PacketSource {
        PacketSource {
            raw: self.raw.clone(),
            object: Object::from_bytes(&self.raw),
        }
    }
    /// Pure correspondence, before any publication or provider IO.
    pub fn check_plan(&self, name: &str, plan: &Plan) -> Result<()> {
        let expected = self
            .expected
            .get(name)
            .ok_or_else(|| error(ErrorKind::Missing, "public-git-asset-return-role"))?;
        let c = plan.candidate();
        if c.logical_id != expected.logical_id
            || c.object.sha256.as_str() != expected.sha256
            || c.object.bytes != expected.bytes
            || c.provenance != Object::from_bytes(&self.provenance)
        {
            return Err(error(ErrorKind::Conflict, "public-git-asset-return-plan"));
        }
        if !plan.has_guarded_effect() {
            return Err(error(
                ErrorKind::Unsupported,
                "public-git-asset-return-legacy-plan",
            ));
        }
        Ok(())
    }
    /// Seal ALL original plans before effects. Persist these control bytes and
    /// their external hash; recovery restores the same cohort, never new IDs.
    pub fn prepare(self, plans: BTreeMap<String, Plan>) -> Result<Prepared> {
        if plans.len() != self.expected.len() {
            return Err(error(ErrorKind::Missing, "public-git-asset-return-plans"));
        }
        for (name, plan) in &plans {
            self.check_plan(name, plan)?;
        }
        let wire = Control {
            schema: "reel.workflow-public-return-plans.v1".into(),
            provenance: Object::from_bytes(&self.provenance),
            packet: self.expected[RETURN_PACKET].clone(),
            plans,
        };
        let encoded = serde_json::to_vec(&wire)
            .map_err(|_| error(ErrorKind::Invalid, "public-git-asset-return-control"))?;
        if encoded.len() > MAX_PACKET {
            return Err(error(ErrorKind::Budget, "public-git-asset-return-control"));
        }
        let identity = Object::from_bytes(&encoded).sha256;
        Ok(Prepared {
            candidates: self,
            plans: wire.plans,
            encoded,
            identity,
        })
    }
    pub fn restore(self, bytes: &[u8], expected: &Object) -> Result<Prepared> {
        if bytes.len() > MAX_PACKET {
            return Err(error(ErrorKind::Budget, "public-git-asset-return-control"));
        }
        if &Object::from_bytes(bytes) != expected {
            return Err(error(
                ErrorKind::Integrity,
                "public-git-asset-return-control",
            ));
        }
        let wire: Control = serde_json::from_slice(bytes)
            .map_err(|_| error(ErrorKind::Invalid, "public-git-asset-return-control"))?;
        if wire.schema != "reel.workflow-public-return-plans.v1"
            || wire.provenance != Object::from_bytes(&self.provenance)
            || wire.packet != self.expected[RETURN_PACKET]
        {
            return Err(error(
                ErrorKind::Conflict,
                "public-git-asset-return-control",
            ));
        }
        let prepared = self.prepare(wire.plans)?;
        if prepared.encoded != bytes {
            return Err(error(
                ErrorKind::Invalid,
                "public-git-asset-return-control-encoding",
            ));
        }
        Ok(prepared)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Control {
    schema: String,
    provenance: Object,
    packet: Artifact,
    plans: BTreeMap<String, Plan>,
}
/// Original immutable plan cohort. No replacement operation IDs on apply/retry.
pub struct Prepared {
    candidates: Candidates,
    plans: BTreeMap<String, Plan>,
    encoded: Vec<u8>,
    identity: Digest,
}
impl Prepared {
    pub fn control(&self) -> &[u8] {
        &self.encoded
    }
    pub fn plans(&self) -> &BTreeMap<String, Plan> {
        &self.plans
    }
    pub fn candidates(&self) -> &Candidates {
        &self.candidates
    }
    fn plan(&self, name: &str) -> Result<&Plan> {
        self.plans
            .get(name)
            .ok_or_else(|| error(ErrorKind::Missing, "public-git-asset-return-role"))
    }
    fn retain(&self, name: &str, plan: &Plan, ready: Ready) -> Result<Retained> {
        if ready.receipt().plan != plan.id().map_err(publication_error)?
            || &ready.receipt().candidate != plan.candidate()
        {
            return Err(error(
                ErrorKind::Integrity,
                "public-git-asset-return-receipt",
            ));
        }
        Ok(Retained {
            identity: self.identity.clone(),
            name: name.into(),
            artifact: self.candidates.expected[name].clone(),
            ready,
        })
    }
    pub fn apply<S: ByteSource>(
        &self,
        name: &str,
        publisher: &mut Publisher,
        resources: &mut Resources<'_>,
        source: &mut S,
    ) -> Result<Retained> {
        let plan = self.plan(name)?;
        let ready = publisher
            .apply(plan, resources, source)
            .map_err(publication_error)?;
        self.retain(name, plan, ready)
    }
    /// Original-operation recovery only. SDK uncertainty never becomes reupload.
    pub fn resume(
        &self,
        name: &str,
        publisher: &mut Publisher,
        resources: &mut Resources<'_>,
    ) -> Result<Retained> {
        let plan = self.plan(name)?;
        let ready = publisher
            .resume(plan, resources)
            .map_err(publication_error)?;
        self.retain(name, plan, ready)
    }
    /// Fresh retained bytes; original source/staging may be unavailable.
    pub fn reopen(
        &self,
        name: &str,
        publisher: &mut Publisher,
        resources: &mut Resources<'_>,
    ) -> Result<Retained> {
        let plan = self.plan(name)?;
        let ready = publisher
            .reopen(plan, resources)
            .map_err(publication_error)?;
        self.retain(name, plan, ready)
    }
    /// Expose the retained return-packet acknowledgement only while guards for
    /// EVERY declared candidate (including worker receipt) remain owned here.
    pub fn gather(&self, retained: Vec<Retained>) -> Result<RetainedBatch> {
        if retained.len() != self.candidates.expected.len() {
            return Err(error(
                ErrorKind::Missing,
                "public-git-asset-return-incomplete",
            ));
        }
        let mut held = BTreeMap::new();
        for value in retained {
            if value.identity != self.identity
                || self.candidates.expected.get(&value.name) != Some(&value.artifact)
                || held.contains_key(&value.name)
            {
                return Err(error(ErrorKind::Conflict, "public-git-asset-return-cohort"));
            }
            held.insert(value.name.clone(), value);
        }
        Ok(RetainedBatch { held })
    }
}

pub struct PacketSource {
    raw: Arc<[u8]>,
    object: Object,
}
impl ByteSource for PacketSource {
    type Reader = Cursor<Arc<[u8]>>;
    fn open_verified(&mut self, object: &Object) -> existing::Result<Self::Reader> {
        if object != &self.object {
            return Err(existing::Error::Missing);
        }
        Ok(Cursor::new(self.raw.clone()))
    }
}
/// Nonserializable actual native byte/protection guard, never a decoded receipt.
pub struct Retained {
    identity: Digest,
    name: String,
    artifact: Artifact,
    ready: Ready,
}
impl Retained {
    pub fn artifact(&self) -> &Artifact {
        &self.artifact
    }
    pub fn receipt(&self) -> &publication::Receipt {
        self.ready.receipt()
    }
    pub fn recovery(&self) -> &publication::Recovery {
        self.ready.recovery()
    }
}
impl Read for Retained {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.ready.read(b)
    }
}
impl Seek for Retained {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.ready.seek(p)
    }
}
pub struct RetainedBatch {
    held: BTreeMap<String, Retained>,
}
impl RetainedBatch {
    pub fn acknowledgement(&self) -> &Artifact {
        self.held[RETURN_PACKET].artifact()
    }
    pub fn candidates(&self) -> impl Iterator<Item = (&str, &Retained)> {
        self.held.iter().map(|(n, v)| (n.as_str(), v))
    }
    pub fn candidate_mut(&mut self, name: &str) -> Option<&mut Retained> {
        self.held.get_mut(name)
    }
}
