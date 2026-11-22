//! Public git-asset adapter. No implicit enrollment, transfer or task completion.
//! This input port is not durable job admission or retained publication authority.
#![forbid(unsafe_code)]
use git_asset::{
    annex,
    source::{self, GitId, Repository, Scope, native::AssetSnapshot},
};
use git_asset_core::{Digest, LogicalPath, Object};
use git_asset_model::resolve::AssetResolver;
use reel_workflow::{Artifact, Error, ErrorKind, Result, publication::InputReader};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
};

pub mod candidates;
pub mod claims;
pub mod workspace;

pub const SDK_COMMIT: &str = "0ccb137cf5a78416e780f8a3372017f642a5cb71";
fn error(kind: ErrorKind, code: &str) -> Error {
    Error {
        kind,
        code: code.into(),
    }
}
fn source_error(e: source::Error) -> Error {
    error(
        match e {
            source::Error::Budget => ErrorKind::Budget,
            source::Error::Integrity => ErrorKind::Integrity,
            source::Error::Incomplete => ErrorKind::Missing,
            source::Error::Ambiguous => ErrorKind::Conflict,
            source::Error::Invalid => ErrorKind::Invalid,
            _ => ErrorKind::NeedsRecovery,
        },
        "public-git-asset-source",
    )
}
fn native_error(e: annex::managed::Error) -> Error {
    publication_error(e.into())
}
fn publication_error(e: annex::publication::Error) -> Error {
    use annex::publication::Error as E;
    error(
        match e {
            E::Invalid => ErrorKind::Invalid,
            E::Unsupported => ErrorKind::Unsupported,
            E::Denied => ErrorKind::Denied,
            E::Budget => ErrorKind::Budget,
            E::Busy => ErrorKind::Busy,
            E::Conflict => ErrorKind::Conflict,
            E::Stale => ErrorKind::Stale,
            E::Integrity => ErrorKind::Integrity,
            E::Missing | E::MissingAcknowledged => ErrorKind::Missing,
            E::ProtectionLost | E::NeedsRecovery | E::Uncertain => ErrorKind::NeedsRecovery,
        },
        e.code(),
    )
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub artifact: Artifact,
    pub record: Object,
    pub object: Object,
    pub stream: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub schema: String,
    pub commit: String,
    pub checkpoint: Digest,
    pub inputs: Vec<Input>,
    pub logical_bytes: u64,
}
impl Preview {
    /// Deterministic private control bytes; the hash binds the exact input view.
    pub fn encode(&self) -> Result<Vec<u8>> {
        if self.inputs.is_empty() || self.inputs.len() > 128 {
            return Err(error(ErrorKind::Budget, "public-git-asset-preview-count"));
        }
        if self.schema != "reel.workflow-public-git-asset-inputs.v1" {
            return Err(error(ErrorKind::Invalid, "public-git-asset-preview-wire"));
        }
        GitId::parse(&self.commit).map_err(source_error)?;
        for input in &self.inputs {
            reel_publication::check_artifact(&input.artifact).map_err(Error::from)?;
            if input.stream.as_ref().is_some_and(|s| s.len() > 128) {
                return Err(error(ErrorKind::Budget, "public-git-asset-preview-stream"));
            }
        }
        let bytes = serde_json::to_vec(self)
            .map_err(|_| error(ErrorKind::Invalid, "public-git-asset-preview-wire"))?;
        if bytes.len() > 128 * 1024 {
            return Err(error(ErrorKind::Budget, "public-git-asset-preview-wire"));
        }
        Ok(bytes)
    }
    /// Bounded decoding only; call Snapshot::verify_preview before trusting it.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 128 * 1024 {
            return Err(error(ErrorKind::Budget, "public-git-asset-preview-wire"));
        }
        let preview: Self = serde_json::from_slice(bytes)
            .map_err(|_| error(ErrorKind::Invalid, "public-git-asset-preview-wire"))?;
        if preview.inputs.is_empty() || preview.inputs.len() > 128 {
            return Err(error(ErrorKind::Budget, "public-git-asset-preview-count"));
        }
        Ok(preview)
    }
}
/// One captured immutable project commit. No index/working-tree/latest fallback.
pub struct Snapshot {
    commit: String,
    assets: AssetSnapshot,
}
impl Snapshot {
    pub fn capture(
        repository: &Repository,
        commit: &str,
        records: &[String],
        definitions: &[String],
    ) -> Result<Self> {
        fn scope(paths: &[String]) -> Result<Scope> {
            if paths.is_empty() || paths.len() > 128 {
                return Err(error(ErrorKind::Budget, "public-git-asset-scope"));
            }
            let paths = paths
                .iter()
                .map(|p| {
                    LogicalPath::parse(p)
                        .map_err(|_| error(ErrorKind::Invalid, "public-git-asset-path"))
                })
                .collect::<Result<Vec<_>>>()?;
            Scope::new(paths).map_err(source_error)
        }
        let oid = GitId::parse(commit).map_err(source_error)?;
        let view = repository.commit(oid).map_err(source_error)?;
        let assets = repository
            .native(
                &view,
                scope(records)?,
                scope(definitions)?,
                source::native::Limits {
                    documents: 512,
                    bytes: 8 * 1024 * 1024,
                    millis: 30000,
                },
            )
            .map_err(source_error)?;
        Ok(Self {
            commit: commit.into(),
            assets,
        })
    }
    /// Decoded preview data is never authority: rebind it to captured Git bytes.
    pub fn verify_preview(&self, preview: &Preview, max_bytes: u64) -> Result<()> {
        if preview.inputs.is_empty() || preview.inputs.len() > 128 {
            return Err(error(ErrorKind::Budget, "public-git-asset-preview-count"));
        }
        let declared = preview
            .inputs
            .iter()
            .map(|i| i.artifact.clone())
            .collect::<Vec<_>>();
        let streams = preview
            .inputs
            .iter()
            .map(|i| (i.artifact.logical_id.clone(), i.stream.clone()))
            .collect();
        if &self.preview(&declared, &streams, max_bytes)? != preview {
            return Err(error(ErrorKind::Stale, "public-git-asset-preview-source"));
        }
        Ok(())
    }
    /// Resolve declared exact identities; a missing stream never selects another.
    pub fn preview(
        &self,
        declared: &[Artifact],
        streams: &BTreeMap<String, Option<String>>,
        max_bytes: u64,
    ) -> Result<Preview> {
        if declared.len() > 128 || declared.is_empty() || streams.len() != declared.len() {
            return Err(error(
                ErrorKind::Invalid,
                "public-git-asset-declared-inputs",
            ));
        }
        let mut ids = BTreeMap::new();
        let mut inputs = Vec::new();
        let mut total = 0u64;
        for artifact in declared {
            reel_publication::check_artifact(artifact).map_err(Error::from)?;
            if artifact.logical_id.is_empty()
                || artifact.logical_id.len() > 256
                || ids.insert(&artifact.logical_id, ()).is_some()
            {
                return Err(error(
                    ErrorKind::Conflict,
                    "public-git-asset-logical-identity",
                ));
            }
            let stream = streams
                .get(&artifact.logical_id)
                .ok_or_else(|| error(ErrorKind::Missing, "public-git-asset-input-stream"))?;
            let record = self
                .assets
                .resolve_id(&artifact.logical_id)
                .map_err(|_| error(ErrorKind::Missing, "public-git-asset-record"))?;
            let object = match stream {
                Some(slot) => record.stream(slot),
                None => record.selected(),
            }
            .map_err(|_| error(ErrorKind::Missing, "public-git-asset-payload"))?
            .clone();
            if artifact.sha256 != object.sha256.as_str() || artifact.bytes != object.bytes {
                return Err(error(ErrorKind::Conflict, "public-git-asset-exact-input"));
            }
            total = total
                .checked_add(object.bytes)
                .filter(|n| *n <= max_bytes)
                .ok_or_else(|| error(ErrorKind::Budget, "public-git-asset-input-budget"))?;
            inputs.push(Input {
                artifact: artifact.clone(),
                record: record.reference().object.clone(),
                object,
                stream: stream.clone(),
            });
        }
        inputs.sort_by(|a, b| a.artifact.logical_id.cmp(&b.artifact.logical_id));
        Ok(Preview {
            schema: "reel.workflow-public-git-asset-inputs.v1".into(),
            commit: self.commit.clone(),
            checkpoint: self.assets.checkpoint().clone(),
            inputs,
            logical_bytes: total,
        })
    }
}
/// Process-lifetime shared namespace/byte guards. This does NOT claim durable job
/// custody across process exit; callers must use explicit persistent admission.
pub struct GuardedInputs {
    reader: annex::managed::Reader,
    declared: BTreeMap<String, Input>,
    held: BTreeMap<String, annex::managed::Read>,
    remaining: u64,
    admission: annex::managed::Profile,
    preview: Object,
}
impl GuardedInputs {
    pub fn new(
        profile: annex::managed::Profile,
        snapshot: &Snapshot,
        preview: &Preview,
        read_budget: u64,
    ) -> Result<Self> {
        snapshot.verify_preview(preview, read_budget)?;
        let declared = preview
            .inputs
            .iter()
            .map(|input| (input.artifact.logical_id.clone(), input.clone()))
            .collect();
        let admission = profile.clone();
        let preview = Object::from_bytes(&preview.encode()?);
        let reader = annex::managed::Reader::new(profile).map_err(native_error)?;
        Ok(Self {
            reader,
            declared,
            held: BTreeMap::new(),
            remaining: read_budget,
            admission,
            preview,
        })
    }
}
impl InputReader for GuardedInputs {
    fn read(&mut self, artifact: &Artifact, max_bytes: u64) -> Result<Vec<u8>> {
        let input = self
            .declared
            .get(&artifact.logical_id)
            .ok_or_else(|| error(ErrorKind::Missing, "public-git-asset-undeclared-input"))?;
        if &input.artifact != artifact {
            return Err(error(
                ErrorKind::Conflict,
                "public-git-asset-input-identity",
            ));
        }
        if artifact.bytes > max_bytes
            || artifact.bytes > self.remaining
            || artifact.bytes > 256 * 1024 * 1024
        {
            return Err(error(ErrorKind::Budget, "public-git-asset-read-budget"));
        }
        self.remaining -= artifact.bytes;
        if !self.held.contains_key(&artifact.logical_id) {
            let read = self.reader.open(&input.object).map_err(native_error)?;
            self.held.insert(artifact.logical_id.clone(), read);
        }
        let read = self
            .held
            .get_mut(&artifact.logical_id)
            .ok_or_else(|| error(ErrorKind::NeedsRecovery, "public-git-asset-read-guard"))?;
        read.seek(SeekFrom::Start(0))
            .map_err(|_| error(ErrorKind::NeedsRecovery, "public-git-asset-read"))?;
        let cap = usize::try_from(artifact.bytes)
            .map_err(|_| error(ErrorKind::Budget, "public-git-asset-read-bytes"))?;
        let mut bytes = Vec::with_capacity(cap);
        read.take(artifact.bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| error(ErrorKind::NeedsRecovery, "public-git-asset-read"))?;
        if Object::from_bytes(&bytes) != input.object {
            return Err(error(ErrorKind::Integrity, "public-git-asset-content"));
        }
        Ok(bytes)
    }
}

impl GuardedInputs {
    /// Bridge all named, already verified live byte guards into the original
    /// durable input claim. Opens only the namespace in the held pool's roster,
    /// never a caller-supplied duplicate namespace path. No byte acquisition,
    /// candidate publication, execution lease or task completion is performed.
    pub fn admit_claim(
        &mut self,
        handle: &claims::Handle,
        nodes: &mut git_asset_store::metadata::MetadataReader,
        limits: &git_asset_store::protection::Limits,
    ) -> Result<claims::Status> {
        let (mut namespace, remaining) = self.claim_namespace(handle, nodes, limits)?;
        handle.admit(&mut namespace, nodes, &remaining)
    }
    /// Consume actual managed input guards and hold the original admitted source
    /// generation at the pool's real roster location. Does not admit or publish.
    pub fn protect_claim(
        self,
        handle: &claims::Handle,
        nodes: &mut git_asset_store::metadata::MetadataReader,
        limits: &git_asset_store::protection::Limits,
    ) -> Result<ProtectedInputs> {
        let started = std::time::Instant::now();
        let (mut namespace, _) = self.claim_namespace(handle, nodes, limits)?;
        let generation = handle.hold(&mut namespace)?;
        if started.elapsed().as_millis() >= u128::from(limits.millis) {
            return Err(error(
                ErrorKind::Budget,
                "public-git-asset-source-use-budget",
            ));
        }
        Ok(ProtectedInputs {
            inputs: self,
            _generation: generation,
        })
    }
    fn claim_namespace(
        &self,
        handle: &claims::Handle,
        nodes: &mut git_asset_store::metadata::MetadataReader,
        limits: &git_asset_store::protection::Limits,
    ) -> Result<(
        git_asset_store::source_claims::admission::Namespace,
        git_asset_store::protection::Limits,
    )> {
        use git_asset_core::tree::{Tree, TreeError, TreeLimits};
        use git_asset_store::source_claims::{Kind, admission::Namespace};
        let start = std::time::Instant::now();
        limits
            .validate()
            .map_err(|_| error(ErrorKind::Budget, "public-git-asset-claim-budget"))?;
        if self.declared.keys().any(|id| !self.held.contains_key(id)) {
            return Err(error(ErrorKind::Denied, "public-git-asset-inputs-not-held"));
        }
        if handle.request().selection.context != self.preview.sha256 {
            return Err(error(ErrorKind::Conflict, "public-git-asset-claim-preview"));
        }
        let member = self
            .admission
            .binding
            .members()
            .iter()
            .find(|m| &m.namespace == handle.minimum().identity())
            .ok_or_else(|| error(ErrorKind::Denied, "public-git-asset-claim-roster"))?;
        let location = self
            .admission
            .members
            .iter()
            .find(|m| m.minimum.identity() == &member.namespace)
            .ok_or_else(|| error(ErrorKind::Denied, "public-git-asset-claim-roster"))?;
        let namespace =
            Namespace::open(&location.root, &location.minimum).map_err(claims::admission_error)?;
        let places = namespace
            .policy()
            .classes
            .get(&Kind::Job)
            .ok_or_else(|| error(ErrorKind::Denied, "public-git-asset-claim-policy"))?;
        if !places.current.iter().any(|p| member.places.contains(p)) {
            return Err(error(
                ErrorKind::Denied,
                "public-git-asset-claim-pool-not-required",
            ));
        }
        let (_, metrics) = Tree::from_root(handle.request().selection.root.clone())
            .entries(
                nodes,
                TreeLimits {
                    max_entries: limits.bindings.min(512),
                    max_reads: limits.reads.min(4096),
                    max_read_bytes: limits.read_bytes.min(4 * 1024 * 1024),
                    ..TreeLimits::default()
                },
            )
            .and_then(|(entries, metrics)| {
                if self
                    .declared
                    .values()
                    .any(|input| !entries.values().any(|o| o == &input.object))
                {
                    return Err(TreeError::PathConflict);
                }
                Ok((entries, metrics))
            })
            .map_err(|e| {
                error(
                    match e {
                        TreeError::Budget => ErrorKind::Budget,
                        TreeError::MissingNode(_) => ErrorKind::Missing,
                        TreeError::Integrity | TreeError::InvalidNode => ErrorKind::Integrity,
                        TreeError::PathConflict => ErrorKind::Conflict,
                        TreeError::StaleRoot => ErrorKind::Stale,
                        TreeError::Store(_) => ErrorKind::NeedsRecovery,
                    },
                    "public-git-asset-claim-closure",
                )
            })?;
        // Coverage verification and namespace admission share one caller budget.
        let mut remaining = limits.clone();
        remaining.reads = remaining.reads.saturating_sub(metrics.reads);
        remaining.read_bytes = remaining.read_bytes.saturating_sub(metrics.read_bytes);
        remaining.millis = remaining
            .millis
            .saturating_sub(u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX));
        if remaining.reads == 0 || remaining.read_bytes == 0 || remaining.millis == 0 {
            return Err(error(ErrorKind::Budget, "public-git-asset-claim-budget"));
        }
        Ok((namespace, remaining))
    }
}
/// Owns actual managed byte/config/namespace guards together with the exact
/// original source generation. No serialization, clone or timer-based release.
/// Complete task/control retention and retained publication remain separate.
pub struct ProtectedInputs {
    inputs: GuardedInputs,
    _generation: git_asset_store::source_claims::admission::source_use::Use,
}
impl InputReader for ProtectedInputs {
    fn read(&mut self, artifact: &Artifact, max_bytes: u64) -> Result<Vec<u8>> {
        self.inputs.read(artifact, max_bytes)
    }
}
