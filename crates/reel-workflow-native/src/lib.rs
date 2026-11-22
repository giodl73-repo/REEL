//! Qualified SDK inputs, durable claims, private staging and response ingress.
//! Guarded reads alone do not establish durable job or retained-result ownership.
use reel_workflow::{Artifact, Error, ErrorKind, Packet, Result};
mod output_bounds;
mod pins;
pub use pins::*;
/// Bounded metadata/candidate reads. Retaining a file handle does not admit a job.
pub trait InputReader {
    fn read(&mut self, asset: &Artifact, maximum: u64) -> Result<Vec<u8>>;
}
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};
use tig_assets::resolve::AssetResolver;
use tig_core::{LogicalPath, Object};
pub mod admission;
pub mod closing;
#[cfg(all(windows, feature = "film"))]
pub mod film_cli;
#[cfg(all(windows, feature = "film"))]
pub mod film_inputs;
pub mod film_metadata;
#[cfg(all(windows, feature = "film"))]
pub mod film_process;
#[cfg(all(windows, feature = "film"))]
pub mod film_task;
pub mod handoff;
mod qualified;
pub mod response;
pub mod retention;
#[cfg(windows)]
pub mod selection;
pub mod staging;
use tig_git::{
    annex::workflow::reader,
    source::{
        self, GitId, Repository, Scope,
        native::{
            self,
            annex::{Binding, Catalogue},
        },
    },
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotProfile {
    pub schema: String,
    pub git: Tool,
    pub repository: PathBuf,
    pub commit: String,
    pub records: Vec<String>,
    pub definitions: Vec<String>,
    /// One explicit record-to-annex relationship per declared logical input.
    pub bindings: BTreeMap<String, Binding>,
    /// None selects the record's selected payload; Some selects an exact slot.
    pub input_streams: BTreeMap<String, Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<SnapshotBudget>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotBudget {
    pub schema: String,
    pub input_bytes: u64,
    pub input_object_bytes: u64,
}
impl SnapshotProfile {
    /// Portable authored declaration; machine repository/tool locators are not
    /// authored identity. Pin these exact canonical bytes in task.native_inputs.
    pub fn declaration_bytes(&self) -> Result<Vec<u8>> {
        let mut value = serde_json::json!({"schema":"reel.workflow-native-bindings.v1",
            "commit":self.commit,"records":self.records,"definitions":self.definitions,
            "bindings":self.bindings,"input_streams":self.input_streams});
        if let Some(budget) = &self.budget {
            self.input_limits()?;
            value["schema"] = "reel.workflow-native-bindings.v2".into();
            value["budget"] = serde_json::to_value(budget)
                .map_err(|_| error(ErrorKind::Invalid, "native-input-budget-json"))?;
        }
        let bytes = tig_core::identity::canonical_json(&value)
            .map_err(|_| error(ErrorKind::Invalid, "native-binding-canonical-json"))?;
        if bytes.len() > reel_workflow::MAX_CONTROL_BYTES {
            return Err(error(ErrorKind::Budget, "native-binding-bytes"));
        }
        Ok(bytes)
    }
    fn input_limits(&self) -> Result<(u64, u64)> {
        match (&self.schema[..], &self.budget) {
            ("reel.workflow-native-snapshot.v1", None) => Ok((64 * 1024 * 1024, 64 * 1024 * 1024)),
            ("reel.workflow-native-snapshot.v2", Some(b))
                if b.schema == "reel.workflow-native-input-budget.v1"
                    && b.input_object_bytes > 0
                    && b.input_object_bytes <= b.input_bytes
                    && b.input_bytes <= 64 * 1024 * 1024 * 1024 =>
            {
                Ok((b.input_bytes, b.input_object_bytes))
            }
            _ => Err(error(ErrorKind::Invalid, "native-input-budget-schema")),
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeProfile {
    pub snapshot: SnapshotProfile,
    pub annex: Tool,
    /// Qualified SDK scope: exactly one explicitly declared native store.
    pub source: reader::Profile,
}
fn error(kind: ErrorKind, code: &str) -> Error {
    Error {
        kind,
        code: code.into(),
    }
}
pub(crate) fn in_phase(mut error: Error, phase: &str) -> Error {
    error.code = format!("{phase}:{}", error.code);
    error
}
fn git_error(value: source::Error) -> Error {
    let kind = match value {
        source::Error::Invalid => ErrorKind::Invalid,
        source::Error::Budget => ErrorKind::Budget,
        source::Error::Integrity => ErrorKind::Integrity,
        source::Error::Incomplete => ErrorKind::Missing,
        source::Error::Ambiguous => ErrorKind::Conflict,
        source::Error::Io | source::Error::Git | source::Error::Metadata => {
            ErrorKind::NeedsRecovery
        }
    };
    error(kind, "native-snapshot-resolution")
}
fn io_error(value: std::io::Error) -> Error {
    let kind = match value.kind() {
        std::io::ErrorKind::NotFound => ErrorKind::Missing,
        std::io::ErrorKind::PermissionDenied => ErrorKind::Denied,
        std::io::ErrorKind::WouldBlock => ErrorKind::Busy,
        _ => ErrorKind::NeedsRecovery,
    };
    error(kind, "native-input-io")
}
fn scope(values: Vec<String>) -> Result<Scope> {
    if values.is_empty() || values.len() > 256 {
        return Err(error(ErrorKind::Budget, "native-scope-count"));
    }
    Scope::new(
        values
            .iter()
            .map(|p| {
                LogicalPath::parse(p).map_err(|_| error(ErrorKind::Invalid, "native-scope-path"))
            })
            .collect::<Result<Vec<_>>>()?,
    )
    .map_err(git_error)
}
fn hold_tool(tool: &Tool) -> Result<File> {
    if !tool.path.is_absolute() || tool.bytes == 0 || tool.bytes > 256 * 1024 * 1024 {
        return Err(error(ErrorKind::Invalid, "native-tool-pin"));
    }
    for path in tool.path.ancestors() {
        let metadata = std::fs::symlink_metadata(path).map_err(io_error)?;
        #[cfg(windows)]
        let linked = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let linked = metadata.file_type().is_symlink();
        if linked {
            return Err(error(ErrorKind::Denied, "native-tool-reparse"));
        }
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let mut file = options.open(&tool.path).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() || metadata.len() != tool.bytes {
        return Err(error(ErrorKind::Integrity, "native-tool-length"));
    }
    let mut reader = Read::by_ref(&mut file).take(tool.bytes + 1);
    let mut hash = Sha256::new();
    let mut length = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let count = reader.read(&mut buffer).map_err(io_error)?;
        if count == 0 {
            break;
        }
        length += count as u64;
        hash.update(&buffer[..count]);
    }
    let actual: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if length != tool.bytes || actual != tool.sha256 {
        return Err(error(ErrorKind::Integrity, "native-tool-content"));
    }
    Ok(file)
}

/// Constructed only by actual exact-commit, record and annex-leaf resolution.
/// No Deserialize implementation: a supplied receipt cannot create this proof.
pub struct SnapshotInputs {
    packet: Packet,
    objects: BTreeMap<String, Object>,
    checkpoint: String,
    git: Tool,
    _git_guard: File,
    archive: Object,
    metadata: tig_core::tree::MemoryNodes,
    declared_objects: BTreeSet<Object>,
}
impl SnapshotInputs {
    pub fn open(packet: &Packet, profile: &SnapshotProfile) -> Result<Self> {
        reel_workflow::check_packet(packet)?;
        if packet.task.upstream_inputs.is_some() {
            return Err(error(
                ErrorKind::Unsupported,
                "native-composite-input-adapter-not-enabled",
            ));
        }
        let (maximum, object_maximum) = profile.input_limits()?;
        if profile.commit != packet.source_commit {
            return Err(error(ErrorKind::Stale, "native-task-commit"));
        }
        let commit = GitId::parse(&profile.commit).map_err(git_error)?;
        let binding = packet
            .task
            .native_inputs
            .as_ref()
            .ok_or_else(|| error(ErrorKind::Missing, "task-native-binding-pin"))?;
        let declaration = profile.declaration_bytes()?;
        if declaration.len() as u64 != binding.bytes
            || reel_workflow::digest(&declaration) != binding.sha256
        {
            return Err(error(ErrorKind::Conflict, "task-native-binding-content"));
        }
        if !profile.repository.is_absolute() {
            return Err(error(ErrorKind::Invalid, "native-repository-path"));
        }
        let ids: BTreeSet<_> = packet
            .task
            .inputs
            .iter()
            .map(|a| a.logical_id.clone())
            .collect();
        if ids.is_empty()
            || profile.bindings.keys().cloned().collect::<BTreeSet<_>>() != ids
            || profile
                .input_streams
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                != ids
        {
            return Err(error(ErrorKind::Conflict, "native-declared-input-set"));
        }
        let total = packet
            .task
            .inputs
            .iter()
            .try_fold(0u64, |n, a| n.checked_add(a.bytes))
            .ok_or_else(|| error(ErrorKind::Budget, "native-input-total"))?;
        if total > maximum || packet.task.inputs.iter().any(|a| a.bytes > object_maximum) {
            return Err(error(ErrorKind::Budget, "native-input-total"));
        }
        let paths: BTreeSet<_> = profile
            .bindings
            .values()
            .flat_map(|b| {
                std::iter::once(b.selected.as_str().to_owned())
                    .chain(b.streams.values().map(|p| p.as_str().to_owned()))
            })
            .collect();
        let catalogue = Catalogue {
            scope: scope(paths.into_iter().collect())?,
            bindings: profile.bindings.clone(),
        };
        let guard = hold_tool(&profile.git)?;
        let repository = Repository::new(
            profile.git.path.clone(),
            profile.repository.clone(),
            source::Limits::default(),
        )
        .map_err(git_error)?;
        let snapshot = repository.commit(commit).map_err(git_error)?;
        let bound = repository
            .native_annex(
                &snapshot,
                scope(profile.records.clone())?,
                scope(profile.definitions.clone())?,
                &catalogue,
                native::Limits {
                    documents: 512,
                    bytes: 16 * 1024 * 1024,
                    millis: 30_000,
                },
            )
            .map_err(git_error)?;
        let mut objects = BTreeMap::new();
        for asset in &packet.task.inputs {
            let resolved = bound
                .assets
                .resolve_id(&asset.logical_id)
                .map_err(|_| error(ErrorKind::Missing, "native-logical-record"))?;
            let object = match &profile.input_streams[&asset.logical_id] {
                None => resolved.selected(),
                Some(slot) => resolved.stream(slot),
            }
            .map_err(|_| error(ErrorKind::Missing, "native-record-stream"))?;
            if object.sha256.as_str() != asset.sha256 || object.bytes != asset.bytes {
                return Err(error(ErrorKind::Conflict, "native-record-task-object"));
            }
            objects.insert(asset.logical_id.clone(), object.clone());
        }
        let mut metadata = tig_core::tree::MemoryNodes::default();
        let archive = bound
            .assets
            .archive_metadata(&mut metadata, tig_core::tree::TreeLimits::default())
            .map_err(git_error)?;
        Ok(Self {
            packet: packet.clone(),
            objects,
            checkpoint: bound.assets.checkpoint().as_str().into(),
            git: profile.git.clone(),
            _git_guard: guard,
            archive,
            metadata,
            declared_objects: bound
                .payloads
                .bindings()
                .iter()
                .map(|b| b.object.clone())
                .collect(),
        })
    }
    pub fn input_count(&self) -> usize {
        self.objects.len()
    }
    pub fn checkpoint(&self) -> &str {
        &self.checkpoint
    }
}

/// Native provider handles remain held for this object's lifetime. This bridges
/// exact record-to-leaf identity and byte access. `open` grants no durable actor
/// protection; `from_claims` additionally holds exact reopened admission claims.
pub struct NativeInputs {
    snapshot: SnapshotInputs,
    source: reader::Profile,
    held: Vec<reader::VerifiedRead>,
    opens: usize,
    _annex_guard: File,
    _claims: Option<admission::ClaimsGuard>,
}
impl NativeInputs {
    pub fn from_claims(
        packet: &Packet,
        profile: &NativeProfile,
        claims: admission::ClaimsGuard,
    ) -> Result<Self> {
        claims.check_source(packet, profile)?;
        let mut inputs = Self::open(packet, profile)?;
        inputs._claims = Some(claims);
        Ok(inputs)
    }
    /// Exact native streaming read retaining this input context and its pinned
    /// tools. No whole-payload buffer or durable job admission is created.
    pub fn stream<'a>(&'a mut self, asset: &Artifact, maximum: u64) -> Result<NativeStream<'a>> {
        if !self.snapshot.packet.task.inputs.contains(asset) {
            return Err(error(ErrorKind::Conflict, "native-undeclared-read"));
        }
        if asset.bytes > maximum {
            return Err(error(ErrorKind::Budget, "native-read-allowance"));
        }
        if self.opens >= 256 {
            return Err(error(ErrorKind::Budget, "native-read-count"));
        }
        self.opens += 1;
        let guard = reader::open(&self.source, &self.snapshot.objects[&asset.logical_id])
            .map_err(io_error)?;
        Ok(NativeStream {
            _inputs: self,
            guard,
        })
    }
    pub fn open(packet: &Packet, profile: &NativeProfile) -> Result<Self> {
        if !cfg!(windows) {
            return Err(error(
                ErrorKind::Unsupported,
                "native-reader-requires-qualified-windows-scope",
            ));
        }
        let snapshot = SnapshotInputs::open(packet, &profile.snapshot)?;
        if profile.source.pool.git != snapshot.git.path
            || profile.source.pool.executable != profile.annex.path
        {
            return Err(error(ErrorKind::Conflict, "native-provider-tool-binding"));
        }
        let guard = hold_tool(&profile.annex)?;
        Ok(Self {
            _claims: None,
            snapshot,
            source: profile.source.clone(),
            held: Vec::new(),
            opens: 0,
            _annex_guard: guard,
        })
    }
    pub fn snapshot(&self) -> &SnapshotInputs {
        &self.snapshot
    }
}
impl InputReader for NativeInputs {
    fn read(&mut self, asset: &Artifact, maximum: u64) -> Result<Vec<u8>> {
        if !self.snapshot.packet.task.inputs.contains(asset) {
            return Err(error(ErrorKind::Conflict, "native-undeclared-read"));
        }
        if asset.bytes > maximum {
            return Err(error(ErrorKind::Budget, "native-read-allowance"));
        }
        if asset.bytes > 64 * 1024 * 1024 {
            return Err(error(
                ErrorKind::Unsupported,
                "native-buffered-read-use-stream",
            ));
        }
        // Reject unbounded repeat opens even if the consumer misbehaves.
        if self.opens >= 256 {
            return Err(error(ErrorKind::Budget, "native-read-count"));
        }
        self.opens += 1;
        let mut guard = reader::open(&self.source, &self.snapshot.objects[&asset.logical_id])
            .map_err(io_error)?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut guard)
            .take(asset.bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        if bytes.len() as u64 != asset.bytes || reel_workflow::digest(&bytes) != asset.sha256 {
            return Err(error(ErrorKind::Integrity, "native-read-content"));
        }
        self.held.push(guard);
        Ok(bytes)
    }
}
pub struct NativeStream<'a> {
    _inputs: &'a mut NativeInputs,
    guard: reader::VerifiedRead,
}
impl Read for NativeStream<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.guard.read(bytes)
    }
}

pub fn read_control<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let mut raw = Vec::new();
    File::open(path)
        .map_err(io_error)?
        .take(reel_workflow::MAX_CONTROL_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(io_error)?;
    reel_workflow::decode(&raw)
}
