//! Immutable small task controls over the original native Plan/Publisher.
//! No enrollment, publication, composite selection or replacement recovery IDs.
use crate::{
    binding::{Binding, object},
    claims::admission_error,
    error, publication_error,
};
use git_asset::annex::publication::{self, Plan};
use git_asset_core::{
    Object,
    tree::{NodeStore, TreeLimits},
};
use git_asset_store::{
    existing::{self, ByteSource},
    source_claims::Kind,
};
use reel_workflow::{Artifact, ErrorKind, Result};
use std::{
    io::{self, Cursor, Write},
    sync::Arc,
};
const MAX_BYTES: usize = 1024 * 1024;
const MAX_PLAN: usize = 64 * 1024;
struct PlanBytes {
    bytes: Vec<u8>,
    full: bool,
}
impl Write for PlanBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_PLAN.saturating_sub(self.bytes.len()) {
            self.full = true;
            return Err(io::Error::other("bounded plan encoding"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
/// Owned original bytes. Clones share immutable bytes, never a mutable path.
#[derive(Clone)]
pub struct Control {
    artifact: Artifact,
    bytes: Arc<[u8]>,
}
impl Control {
    pub fn new(artifact: Artifact, bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BYTES || artifact.bytes > MAX_BYTES as u64 {
            return Err(error(ErrorKind::Budget, "public-ingress-bytes"));
        }
        if object(&artifact)? != Object::from_bytes(bytes) {
            return Err(error(ErrorKind::Integrity, "public-ingress-bytes"));
        }
        Ok(Self {
            artifact,
            bytes: Arc::from(bytes),
        })
    }
    pub fn artifact(&self) -> &Artifact {
        &self.artifact
    }
    /// Explicit metadata freeze only. The ingress source must already be an
    /// empty Retention source at the actual configured roster root. Preparation
    /// is not admission; Publisher revalidates at effect time.
    pub fn plan(
        &self,
        binding: &Binding,
        profile: &publication::Profile,
        request: publication::Request,
        admission: publication::Admission,
        nodes: &mut impl NodeStore,
    ) -> Result<Plan> {
        self.check_candidate(&request.candidate)?;
        let mut namespace = binding.control_seed().namespace(&profile.pool)?;
        let claim = binding.claim();
        if &admission.namespace != namespace.identity()
            || request.owner != claim.request().owner
            || (claim.minimum().identity() == namespace.identity()
                && request.source == claim.request().source)
            || [
                binding.control_seed(),
                binding.result_seed(),
                binding.retained_seed(),
            ]
            .iter()
            .any(|seed| seed.is_source(namespace.identity(), &request.source))
        {
            return Err(error(ErrorKind::Denied, "public-ingress-reserved-source"));
        }
        let state = namespace
            .source(&request.source)
            .map_err(admission_error)?
            .ok_or_else(|| error(ErrorKind::Missing, "public-ingress-source"))?;
        if state.owner != request.owner || state.kind != Some(Kind::Retention) {
            return Err(error(ErrorKind::Denied, "public-ingress-source"));
        }
        if state.revision != request.expected_revision
            || state.current.is_some()
            || state.pending.is_some()
            || !state.predecessors.is_empty()
        {
            return Err(error(ErrorKind::Stale, "public-ingress-source"));
        }
        let frozen = publication::freeze(&request, nodes, TreeLimits::default())
            .map_err(publication_error)?;
        let plan = Plan::new(profile, request, frozen, admission).map_err(publication_error)?;
        self.encode_plan(&plan)?;
        Ok(plan)
    }
    fn check_candidate(&self, candidate: &publication::Candidate) -> Result<()> {
        if candidate.logical_id != self.artifact.logical_id
            || candidate.object != object(&self.artifact)?
        {
            return Err(error(ErrorKind::Conflict, "public-ingress-candidate"));
        }
        Ok(())
    }
    /// The SDK's original wire, not a second plan schema. Persist it and its
    /// independent expected Object BEFORE Publisher effects; never regenerate
    /// operation IDs on an uncertain retry. Encoded data proves no custody.
    pub fn encode_plan(&self, plan: &Plan) -> Result<Vec<u8>> {
        Self::encode_for(&self.artifact, plan)
    }
    fn encode_for(artifact: &Artifact, plan: &Plan) -> Result<Vec<u8>> {
        if artifact.bytes > MAX_BYTES as u64 {
            return Err(error(ErrorKind::Budget, "public-ingress-bytes"));
        }
        if plan.candidate().logical_id != artifact.logical_id
            || plan.candidate().object != object(artifact)?
        {
            return Err(error(ErrorKind::Conflict, "public-ingress-candidate"));
        }
        if !plan.has_guarded_effect() {
            return Err(error(ErrorKind::Unsupported, "public-ingress-protocol"));
        }
        let mut encoded = PlanBytes {
            bytes: Vec::new(),
            full: false,
        };
        if serde_json::to_writer(&mut encoded, plan).is_err() {
            return Err(error(
                if encoded.full {
                    ErrorKind::Budget
                } else {
                    ErrorKind::Invalid
                },
                "public-ingress-plan",
            ));
        }
        Ok(encoded.bytes)
    }
    /// Original separately pinned SDK plan only; no metadata writes or IO.
    /// Actual source/staging may be absent for SDK resume/reopen afterwards.
    pub fn restore_plan(artifact: &Artifact, raw: &[u8], expected: &Object) -> Result<Plan> {
        if raw.len() > MAX_PLAN {
            return Err(error(ErrorKind::Budget, "public-ingress-plan"));
        }
        if Object::from_bytes(raw) != *expected {
            return Err(error(ErrorKind::Integrity, "public-ingress-plan"));
        }
        let plan: Plan = serde_json::from_slice(raw)
            .map_err(|_| error(ErrorKind::Invalid, "public-ingress-plan"))?;
        if Self::encode_for(artifact, &plan)? != raw {
            return Err(error(ErrorKind::Invalid, "public-ingress-plan-encoding"));
        }
        Ok(plan)
    }
}
impl ByteSource for Control {
    type Reader = Cursor<Arc<[u8]>>;
    fn open_verified(&mut self, object: &Object) -> existing::Result<Self::Reader> {
        if *object != Object::from_bytes(&self.bytes) {
            return Err(existing::Error::Missing);
        }
        Ok(Cursor::new(self.bytes.clone()))
    }
}
