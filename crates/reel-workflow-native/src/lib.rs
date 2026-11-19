//! Qualified SDK inputs, durable claims, private staging and response ingress.
//! Guarded reads alone do not establish durable job or retained-result ownership.
use reel_workflow::{Artifact, Error, ErrorKind, Packet, Result, publication::InputReader};
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
pub mod advancement;
pub mod closing;
mod comic;
pub mod dispatch;
pub mod expansion;
#[cfg(feature = "film")]
pub mod film_inputs;
#[cfg(all(windows, feature = "film"))]
pub mod film_process;
pub mod graph;
pub mod handoff;
pub mod image;
mod issue;
#[cfg(all(windows, feature = "film"))]
pub mod movie;
mod output_layout;
mod producer;
mod qualified;
pub mod resolution;
pub mod resolution_job;
pub mod response;
pub mod retention;
mod review;
pub mod runner;
pub mod staging;
pub mod upstream;
pub use staging::comic_export;
pub use staging::provider;
pub use staging::worker;
/// Read-only deterministic input validation; no custody or execution authority.
pub fn check_publication_inputs(packet: &Packet, inputs: &mut impl InputReader) -> Result<()> {
    producer::output_budget(packet, inputs).map(|_| ())
}
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
    pub upstream: Option<upstream::Frozen>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<SnapshotBudget>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SnapshotBudget {
    pub schema: String,
    pub input_bytes: u64,
    pub input_object_bytes: u64,
    pub projection_bytes: u64,
}
impl SnapshotBudget {
    pub fn validate(&self) -> Result<()> {
        if self.schema != "reel.workflow-snapshot-budget.v1" {
            return Err(error(ErrorKind::Invalid, "native-snapshot-budget-version"));
        }
        if self.input_object_bytes == 0
            || self.input_object_bytes > self.input_bytes
            || self.input_bytes > 64 * 1024 * 1024 * 1024
            || self.projection_bytes < self.input_bytes
            || self.projection_bytes > 128 * 1024 * 1024 * 1024
        {
            return Err(error(ErrorKind::Budget, "native-snapshot-budget"));
        }
        Ok(())
    }
}
impl SnapshotProfile {
    /// Portable authored declaration; machine repository/tool locators are not
    /// authored identity. Pin these exact canonical bytes in task.native_inputs.
    pub fn declaration_bytes(&self) -> Result<Vec<u8>> {
        let mut value = serde_json::json!({"schema":"reel.workflow-native-bindings.v1",
            "commit":self.commit,"records":self.records,"definitions":self.definitions,
            "bindings":self.bindings,"input_streams":self.input_streams});
        if let Some(upstream) = &self.upstream {
            value["schema"] = "reel.workflow-native-composite-bindings.v1".into();
            value["upstream"] = serde_json::to_value(upstream)
                .map_err(|_| error(ErrorKind::Invalid, "composite-declaration-json"))?;
        }
        if let Some(budget) = &self.budget {
            budget.validate()?;
            value["schema"] = if self.upstream.is_some() {
                "reel.workflow-native-composite-bindings.v2"
            } else {
                "reel.workflow-native-bindings.v2"
            }
            .into();
            value["budget"] = serde_json::to_value(budget)
                .map_err(|_| error(ErrorKind::Invalid, "native-snapshot-budget-json"))?;
        }
        let bytes = tig_core::identity::canonical_json(&value)
            .map_err(|_| error(ErrorKind::Invalid, "native-binding-canonical-json"))?;
        if bytes.len() > reel_workflow::MAX_CONTROL_BYTES {
            return Err(error(ErrorKind::Budget, "native-binding-bytes"));
        }
        Ok(bytes)
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeProfile {
    pub snapshot: SnapshotProfile,
    pub annex: Tool,
    /// Qualified SDK scope: exactly one explicitly declared native store.
    pub source: reader::Profile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub movie_verifier: Option<MovieVerifier>,
}
/// Explicit read-only verification installation and existing scratch admission.
/// These machine locators never retarget a producer's frozen worker ticket.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MovieVerifier {
    pub runtime: Tool,
    pub auxiliary_programs: BTreeMap<String, Tool>,
    pub staging: staging::Profile,
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
    projection_bytes: u64,
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
        let origins = match (&packet.task.upstream_inputs, &profile.upstream) {
            (None, None) => BTreeMap::new(),
            (Some(_), Some(frozen)) => frozen.bound(packet)?,
            _ => return Err(error(ErrorKind::Conflict, "composite-origin-declaration")),
        };
        let schema = match (profile.upstream.is_some(), profile.budget.is_some()) {
            (false, false) => "reel.workflow-native-snapshot.v1",
            (true, false) => "reel.workflow-native-composite-snapshot.v1",
            (false, true) => "reel.workflow-native-snapshot.v2",
            (true, true) => "reel.workflow-native-composite-snapshot.v2",
        };
        if profile.schema != schema || profile.commit != packet.source_commit {
            return Err(error(ErrorKind::Stale, "native-task-commit"));
        }
        if let Some(budget) = &profile.budget {
            budget.validate()?;
        }
        let projection_bytes = profile
            .budget
            .as_ref()
            .map(|b| b.projection_bytes)
            .unwrap_or(1024 * 1024 * 1024);
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
            .filter(|a| !origins.contains_key(&a.logical_id))
            .map(|a| a.logical_id.clone())
            .collect();
        if (ids.is_empty() && profile.upstream.is_none())
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
        let input_limit = profile
            .budget
            .as_ref()
            .map(|b| b.input_bytes)
            .unwrap_or(64 * 1024 * 1024);
        if total > input_limit
            || profile.budget.as_ref().is_some_and(|b| {
                packet
                    .task
                    .inputs
                    .iter()
                    .any(|a| a.bytes > b.input_object_bytes)
            })
        {
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
        let guard = hold_tool(&profile.git)?;
        let repository = Repository::new(
            profile.git.path.clone(),
            profile.repository.clone(),
            source::Limits::default(),
        )
        .map_err(git_error)?;
        let snapshot = repository.commit(commit).map_err(git_error)?;
        if ids.is_empty() {
            if !profile.records.is_empty() || !profile.definitions.is_empty() {
                return Err(error(ErrorKind::Conflict, "composite-empty-git-scope"));
            }
            let mut metadata = tig_core::tree::MemoryNodes::default();
            let raw = serde_json::to_vec(&serde_json::json!({"schema":"reel.workflow-empty-git-inputs.v1","commit":profile.commit})).map_err(|_|error(ErrorKind::Invalid,"composite-empty-git-json"))?;
            use tig_core::tree::NodeStore;
            let git_archive = metadata
                .put(&raw)
                .map_err(|_| error(ErrorKind::Integrity, "composite-empty-git-archive"))?;
            let (archive, objects, declared_objects) = profile
                .upstream
                .as_ref()
                .ok_or_else(|| error(ErrorKind::Missing, "composite-origins"))?
                .archive(
                    packet,
                    &git_archive,
                    &mut metadata,
                    BTreeMap::new(),
                    BTreeSet::new(),
                )?;
            return Ok(Self {
                projection_bytes,
                packet: packet.clone(),
                objects,
                checkpoint: archive.sha256.as_str().into(),
                git: profile.git.clone(),
                _git_guard: guard,
                archive,
                metadata,
                declared_objects,
            });
        }
        let catalogue = Catalogue {
            scope: scope(paths.into_iter().collect())?,
            bindings: profile.bindings.clone(),
        };
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
            if origins.contains_key(&asset.logical_id) {
                continue;
            }
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
        let declared_objects = bound
            .payloads
            .bindings()
            .iter()
            .map(|b| b.object.clone())
            .collect();
        let (archive, objects, declared_objects) = if let Some(frozen) = &profile.upstream {
            frozen.archive(packet, &archive, &mut metadata, objects, declared_objects)?
        } else {
            (archive, objects, declared_objects)
        };
        let checkpoint = if profile.upstream.is_some() {
            archive.sha256.as_str().into()
        } else {
            bound.assets.checkpoint().as_str().into()
        };
        if profile.budget.as_ref().is_some_and(|b| {
            declared_objects
                .iter()
                .any(|o| o.bytes > b.input_object_bytes)
        }) {
            return Err(error(ErrorKind::Budget, "native-declared-object-budget"));
        }
        Ok(Self {
            projection_bytes,
            packet: packet.clone(),
            objects,
            checkpoint,
            git: profile.git.clone(),
            _git_guard: guard,
            archive,
            metadata,
            declared_objects,
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
/// exact record-to-leaf identity and byte access, not durable actor protection.
pub struct NativeInputs {
    origins: Option<upstream::Frozen>,
    #[cfg(all(windows, feature = "film"))]
    repository: PathBuf,
    snapshot: SnapshotInputs,
    source: reader::Profile,
    held: Vec<reader::VerifiedRead>,
    _annex_guard: File,
    captured: BTreeMap<Object, Vec<u8>>,
    reads: usize,
    _claims: Option<Box<admission::ClaimsGuard>>,
    _permit: Option<admission::InputPermit>,
}
impl NativeInputs {
    pub(crate) fn review_origins(&self) -> Result<&upstream::Frozen> {
        if self._claims.is_none() && self._permit.is_none() {
            return Err(error(
                ErrorKind::Denied,
                "review-control-live-authority-required",
            ));
        }
        self.origins
            .as_ref()
            .ok_or_else(|| error(ErrorKind::Missing, "review-control-frozen-producer"))
    }
    #[cfg(all(windows, feature = "film"))]
    pub(crate) fn movie_authority(&self) -> Result<(&admission::Profile, &str)> {
        if let Some(claims) = &self._claims {
            return Ok((claims.profile(), claims.attempt()));
        }
        if let Some(permit) = &self._permit {
            return Ok((permit.profile(), permit.attempt()));
        }
        Err(error(
            ErrorKind::Denied,
            "movie-live-input-authority-required",
        ))
    }
    pub fn open(packet: &Packet, profile: &NativeProfile) -> Result<Self> {
        if packet.task.upstream_inputs.is_some() {
            return Err(error(
                ErrorKind::Denied,
                "composite-inputs-require-child-claims",
            ));
        }
        Self::open_inner(packet, profile, None)
    }
    pub fn from_claims(
        packet: &Packet,
        profile: &NativeProfile,
        claims: admission::ClaimsGuard,
    ) -> Result<Self> {
        claims.check_source(packet, profile)?;
        let mut inputs = Self::open_inner(packet, profile, Some(Box::new(claims)))?;
        crate::review::prepare(packet, &mut inputs)?;
        Ok(inputs)
    }
    pub fn open_admitted(
        packet: &Packet,
        source: &NativeProfile,
        profile: &admission::Profile,
        receipt: &admission::Receipt,
    ) -> Result<Self> {
        let claims = admission::Prepared::open(packet, source, profile, &receipt.attempt_id)?
            .reopen(receipt)?;
        Self::from_claims(packet, source, claims)
    }
    pub(crate) fn from_permit(
        packet: &Packet,
        profile: &NativeProfile,
        permit: admission::InputPermit,
    ) -> Result<Self> {
        permit.check_source(packet, profile)?;
        let mut inputs = Self::open_inner(packet, profile, None)?;
        inputs._permit = Some(permit);
        crate::review::prepare(packet, &mut inputs)?;
        Ok(inputs)
    }
    fn open_inner(
        packet: &Packet,
        profile: &NativeProfile,
        claims: Option<Box<admission::ClaimsGuard>>,
    ) -> Result<Self> {
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
            origins: profile.snapshot.upstream.clone(),
            #[cfg(all(windows, feature = "film"))]
            repository: profile.snapshot.repository.clone(),
            snapshot,
            source: profile.source.clone(),
            held: Vec::new(),
            _annex_guard: guard,
            captured: BTreeMap::new(),
            reads: 0,
            _claims: claims,
            _permit: None,
        })
    }
    pub fn snapshot(&self) -> &SnapshotInputs {
        &self.snapshot
    }
    pub(crate) fn captured(&self, object: &Object) -> Option<&[u8]> {
        self.captured.get(object).map(Vec::as_slice)
    }
    /// Stream one exact declared input while retaining its native gate through
    /// this provider's lifetime. Large inputs are never added to captured buffers.
    /// Callers must reuse their verified local copy for duplicate object aliases.
    pub(crate) fn with_stream<T>(
        &mut self,
        asset: &Artifact,
        consume: impl FnOnce(&mut dyn Read) -> Result<T>,
    ) -> Result<T> {
        if !self.snapshot.packet.task.inputs.contains(asset) {
            return Err(error(ErrorKind::Conflict, "native-undeclared-read"));
        }
        if self.reads >= 256 {
            return Err(error(ErrorKind::Budget, "native-read-count"));
        }
        self.reads += 1;
        let object = &self.snapshot.objects[&asset.logical_id];
        if let Some(raw) = self.captured.get(object) {
            return consume(&mut std::io::Cursor::new(raw));
        }
        let mut held = reader::open(&self.source, object).map_err(io_error)?;
        let result = consume(&mut held);
        self.held.push(held);
        result
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
        // Reject unbounded repeat opens even if the consumer misbehaves.
        if self.reads >= 256 {
            return Err(error(ErrorKind::Budget, "native-read-count"));
        }
        self.reads += 1;
        let object = &self.snapshot.objects[&asset.logical_id];
        // The exact declared logical record/leaf binding was checked above and
        // at snapshot admission. Reuse only this operation's already held gate.
        if let Some(bytes) = self.captured.get(object) {
            return Ok(bytes.clone());
        }
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
        self.captured.insert(object.clone(), bytes.clone());
        Ok(bytes)
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
/// Read an ordinary caller-selected control with a mutation guard and a fixed
/// 1 MiB bound. This supplies no admission, custody, execution or approval.
pub fn read_candidate_control(path: &Path) -> Result<(Vec<u8>, File)> {
    staging::read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)
}
