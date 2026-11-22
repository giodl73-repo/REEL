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

pub mod claims;

pub const SDK_COMMIT: &str = "3ddeab5572f9479475134e6b7334a26effcb9277";
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
    let e = annex::publication::Error::from(e);
    error(
        match e {
            annex::publication::Error::Missing | annex::publication::Error::MissingAcknowledged => {
                ErrorKind::Missing
            }
            annex::publication::Error::Integrity => ErrorKind::Integrity,
            annex::publication::Error::Busy => ErrorKind::Busy,
            annex::publication::Error::Budget => ErrorKind::Budget,
            annex::publication::Error::Stale => ErrorKind::Stale,
            annex::publication::Error::Conflict => ErrorKind::Conflict,
            annex::publication::Error::Denied => ErrorKind::Denied,
            annex::publication::Error::Unsupported => ErrorKind::Unsupported,
            _ => ErrorKind::NeedsRecovery,
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
        let reader = annex::managed::Reader::new(profile).map_err(native_error)?;
        Ok(Self {
            reader,
            declared,
            held: BTreeMap::new(),
            remaining: read_budget,
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
