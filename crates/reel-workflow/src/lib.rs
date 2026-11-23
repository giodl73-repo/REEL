//! Pure workflow contracts. No provider, storage admission, retention, selection,
//! task completion, Git mutation or approval follows from these checks.
pub use reel_publication::{Artifact, ArtifactSet, OutputContract, OutputContracts};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
pub mod film_inspect;
pub mod film_rebuild;
pub mod film_repair;
pub mod film_review;
pub mod handoff;
pub mod input;
pub mod manual;
pub mod speech_handoff;

pub const PLAN_SCHEMA: &str = "reel.workflow-plan.v2";
pub const SCOPED_PLAN_SCHEMA: &str = "reel.workflow-plan.v3";
pub const PACKET_SCHEMA: &str = "reel.workflow-packet.v2";
pub const RESULT_SCHEMA: &str = "reel.workflow-result.v2";
pub const MAX_CONTROL_BYTES: usize = 1024 * 1024;
const MAX_TASKS: usize = 128;
const MAX_INPUTS: usize = 128;
const MAX_PROMPT_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorKind {
    Invalid,
    Integrity,
    Missing,
    Conflict,
    Stale,
    Denied,
    Busy,
    Unsupported,
    NeedsRecovery,
    Budget,
}

/// Bounded diagnostics contain no submitted prompt, path, provider reply or secret.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Error {
    pub kind: ErrorKind,
    pub code: String,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.code)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

impl From<reel_publication::Error> for Error {
    fn from(value: reel_publication::Error) -> Self {
        let kind = match value.kind {
            reel_publication::ErrorKind::Invalid => ErrorKind::Invalid,
            reel_publication::ErrorKind::Missing => ErrorKind::Missing,
            reel_publication::ErrorKind::Conflict => ErrorKind::Conflict,
            reel_publication::ErrorKind::Budget => ErrorKind::Budget,
        };
        Self {
            kind,
            code: value.code.into(),
        }
    }
}

fn fail<T>(kind: ErrorKind, code: &str) -> Result<T> {
    Err(Error {
        kind,
        code: code.into(),
    })
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    TextProposal,
    ImageCandidate,
    SpeechCandidate,
    BrowserSourceCandidate,
    ExternalSourceIntake,
    ReviewFinding,
    ReviewReport,
    DeterministicTool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ToolCapability {
    RepairResolution,
    FilmRender,
    FilmSceneControl,
    ComicRender,
    LayeredComicCompose,
    ComicIssueAssembly,
    ComicReadingAssembly,
    ComicReadingMerge,
    ComicIssueControl,
    ComicReviewControl,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolContract {
    pub schema: String,
    pub capability: ToolCapability,
    pub executable: Artifact,
    /// Portable identities of auxiliary installed programs; machine paths live
    /// in the original worker invocation, never in this task contract.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub auxiliary_executables: BTreeMap<String, Artifact>,
    pub publication: Artifact,
    pub edition_id: String,
    pub request: Artifact,
    /// Portable producer-relative path -> exact declared logical asset identity.
    pub source_bindings: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<ComicPart>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComicPart {
    pub schema: String,
    pub plan: Artifact,
    pub part_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: String,
    /// Optional exact canonical native record-to-annex input declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_inputs: Option<Artifact>,
    /// Exact portable dependency-to-input mapping; not a custody acknowledgement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_inputs: Option<Artifact>,
    pub operation: Operation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<ToolContract>,
    pub language: Option<String>,
    pub prompt: String,
    pub inputs: Vec<Artifact>,
    pub depends_on: Vec<String>,
    pub outputs: OutputContracts,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: String,
    pub project: String,
    pub source_commit: String,
    /// V3 only: a complete authored task -> exact snapshot map. The plan's
    /// source_commit identifies its overall revision, not an implicit fallback.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub task_snapshots: BTreeMap<String, String>,
    pub tasks: Vec<Task>,
}
impl Plan {
    pub fn source_commit_for(&self, task_id: &str) -> Result<&str> {
        if !self.tasks.iter().any(|t| t.id == task_id) {
            return fail(ErrorKind::Missing, "plan-snapshot-task");
        }
        match self.schema.as_str() {
            PLAN_SCHEMA if self.task_snapshots.is_empty() => Ok(&self.source_commit),
            SCOPED_PLAN_SCHEMA => self
                .task_snapshots
                .get(task_id)
                .map(String::as_str)
                .filter(|c| hex(c, &[40, 64]) && !c.bytes().all(|b| b == b'0'))
                .ok_or_else(|| Error {
                    kind: ErrorKind::Invalid,
                    code: "plan-task-snapshot".into(),
                }),
            _ => fail(ErrorKind::Invalid, "plan-snapshot-version"),
        }
    }
}

/// Planning reference only. A hash or caller-supplied descriptor is not a
/// retained-owner acknowledgement. Execution must independently admit resources.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DependencyResult {
    pub result_sha256: String,
    pub artifacts: ArtifactSet,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Packet {
    pub schema: String,
    pub project: String,
    pub source_commit: String,
    pub task: Task,
    pub dependencies: BTreeMap<String, DependencyResult>,
    pub task_key: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Direct,
    Handoff,
    Tool,
}

/// Provider/model fields record supplied provenance, not authenticated identity.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub mode: Mode,
    pub provider: Option<String>,
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<Artifact>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReturnedResult {
    pub schema: String,
    pub task_id: String,
    pub task_key: String,
    pub attempt_id: String,
    pub artifacts: ArtifactSet,
    pub provenance: Provenance,
    /// Optional exact deterministic-worker receipt. Older v2 results retain
    /// their byte representation when absent; native tool ingestion requires it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_receipt: Option<Artifact>,
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        && value != "."
        && value != ".."
}
fn hex(value: &str, lengths: &[usize]) -> bool {
    lengths.contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn artifact(value: &Artifact) -> Result<()> {
    reel_publication::check_artifact(value).map_err(Into::into)
}
fn task(value: &Task) -> Result<()> {
    if !token(&value.id)
        || value.language.as_ref().is_some_and(|s| !token(s))
        || value.prompt.len() > MAX_PROMPT_BYTES
        || value.inputs.len() > MAX_INPUTS
        || value.depends_on.len() > MAX_TASKS
    {
        return fail(ErrorKind::Invalid, "task-contract");
    }
    reel_publication::check_contracts(&value.outputs)?;
    match (&value.operation, &value.tool) {
        (Operation::DeterministicTool, Some(tool)) => {
            if !value.prompt.is_empty()
                || tool.schema != "reel.workflow-tool.v1"
                || !token(&tool.edition_id)
                || tool.source_bindings.len() > MAX_INPUTS
                || value.language.is_none()
                || tool
                    .source_bindings
                    .iter()
                    .any(|(path, id)| !portable_source(path) || id.is_empty())
            {
                return fail(ErrorKind::Invalid, "deterministic-tool-contract");
            }
            artifact(&tool.executable)?;
            if tool.auxiliary_executables.len() > 16 {
                return fail(ErrorKind::Budget, "tool-auxiliary-program-count");
            }
            for (role, program) in &tool.auxiliary_executables {
                if !token(role)
                    || role.bytes().any(|b| b.is_ascii_uppercase())
                    || program.bytes > 256 * 1024 * 1024
                {
                    return fail(ErrorKind::Invalid, "tool-auxiliary-program-contract");
                }
                artifact(program)?;
            }
            artifact(&tool.publication)?;
            artifact(&tool.request)?;
            if let Some(part) = &tool.part {
                artifact(&part.plan)?;
                if !matches!(
                    tool.capability,
                    ToolCapability::ComicRender | ToolCapability::ComicReadingAssembly
                ) || part.schema != "reel.workflow-comic-part.v1"
                    || !token(&part.part_id)
                    || part.plan.media_type != "application/json"
                    || part.plan.bytes > MAX_CONTROL_BYTES as u64
                    || !value.inputs.contains(&part.plan)
                    || [
                        &tool.publication.logical_id,
                        &tool.request.logical_id,
                        &tool.executable.logical_id,
                    ]
                    .contains(&&part.plan.logical_id)
                    || tool
                        .source_bindings
                        .values()
                        .any(|id| id == &part.plan.logical_id)
                {
                    return fail(ErrorKind::Invalid, "comic-part-input-binding");
                }
            }
            if tool.publication.logical_id == tool.request.logical_id
                || tool.publication.media_type != "application/json"
                || tool.request.media_type != "application/json"
                || !value.inputs.contains(&tool.publication)
                || !value.inputs.contains(&tool.request)
                || tool
                    .source_bindings
                    .values()
                    .any(|id| !value.inputs.iter().any(|a| &a.logical_id == id))
            {
                return fail(ErrorKind::Invalid, "tool-input-binding");
            }
        }
        (Operation::DeterministicTool, None) => {
            return fail(ErrorKind::Missing, "deterministic-tool");
        }
        (_, Some(_)) => return fail(ErrorKind::Invalid, "intelligence-task-has-tool"),
        (_, None) if value.prompt.trim().is_empty() => {
            return fail(ErrorKind::Invalid, "task-prompt");
        }
        _ => (),
    }
    if let Some(binding) = &value.native_inputs {
        artifact(binding)?;
        if binding.media_type != "application/json" || binding.bytes > MAX_CONTROL_BYTES as u64 {
            return fail(ErrorKind::Invalid, "native-input-declaration");
        }
    }
    if let Some(binding) = &value.upstream_inputs {
        artifact(binding)?;
        if binding.media_type != "application/json"
            || binding.bytes > MAX_CONTROL_BYTES as u64
            || (value.depends_on.is_empty() && value.operation != Operation::ExternalSourceIntake)
        {
            return fail(ErrorKind::Invalid, "upstream-input-declaration");
        }
    }
    if value.operation == Operation::ExternalSourceIntake
        && (!value.inputs.is_empty()
            || !value.depends_on.is_empty()
            || value.upstream_inputs.is_none())
    {
        return fail(ErrorKind::Invalid, "source-intake-root-contract");
    }
    let mut inputs = BTreeSet::new();
    for input in &value.inputs {
        artifact(input)?;
        if !inputs.insert(&input.logical_id) {
            return fail(ErrorKind::Conflict, "duplicate-input-identity");
        }
    }
    if value.operation == Operation::ReviewReport {
        let request = value
            .inputs
            .iter()
            .find(|a| a.logical_id == REVIEW_REQUEST_ID)
            .ok_or(Error {
                kind: ErrorKind::Missing,
                code: "review-task-request".into(),
            })?;
        if request.media_type != "application/json" || request.bytes > MAX_CONTROL_BYTES as u64 {
            return fail(ErrorKind::Invalid, "review-task-request-pin");
        }
    }
    let dependencies: BTreeSet<_> = value.depends_on.iter().collect();
    if dependencies.len() != value.depends_on.len()
        || dependencies.iter().any(|id| !token(id) || **id == value.id)
    {
        return fail(ErrorKind::Invalid, "task-dependencies");
    }
    Ok(())
}

/// Only bounded metadata is decoded. Unknown fields refuse; no assets are read.
pub fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    if bytes.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "control-bytes");
    }
    serde_json::from_slice(bytes).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "control-json".into(),
    })
}
pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn canonical_digest(value: &impl Serialize) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "control-encoding".into(),
    })?;
    if bytes.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "control-bytes");
    }
    Ok(digest(&bytes))
}

/// Returns a stable topological order, refusing cycles and missing dependencies.
pub fn check_plan(plan: &Plan) -> Result<Vec<String>> {
    canonical_digest(plan)?;
    if !matches!(plan.schema.as_str(), PLAN_SCHEMA | SCOPED_PLAN_SCHEMA)
        || !token(&plan.project)
        || !hex(&plan.source_commit, &[40, 64])
        || (plan.schema == SCOPED_PLAN_SCHEMA && plan.source_commit.bytes().all(|b| b == b'0'))
        || plan.tasks.is_empty()
        || plan.tasks.len() > MAX_TASKS
    {
        return fail(ErrorKind::Invalid, "plan-contract");
    }
    if (plan.schema == PLAN_SCHEMA && !plan.task_snapshots.is_empty())
        || (plan.schema == SCOPED_PLAN_SCHEMA
            && (plan.task_snapshots.keys().collect::<BTreeSet<_>>()
                != plan.tasks.iter().map(|t| &t.id).collect()
                || plan
                    .tasks
                    .iter()
                    .any(|t| plan.source_commit_for(&t.id).is_err())))
    {
        return fail(ErrorKind::Invalid, "plan-task-snapshot-set");
    }
    let mut by_id = BTreeMap::new();
    for value in &plan.tasks {
        task(value)?;
        if by_id.insert(value.id.clone(), value).is_some() {
            return fail(ErrorKind::Conflict, "duplicate-task-identity");
        }
    }
    if by_id
        .values()
        .any(|t| t.depends_on.iter().any(|id| !by_id.contains_key(id)))
    {
        return fail(ErrorKind::Missing, "dependency-task");
    }
    let mut resolved = BTreeSet::new();
    let mut ordered = Vec::new();
    while !by_id.is_empty() {
        let next = by_id
            .iter()
            .find(|(_, t)| t.depends_on.iter().all(|id| resolved.contains(id)))
            .map(|(id, _)| id.clone());
        let Some(id) = next else {
            return fail(ErrorKind::Invalid, "dependency-cycle");
        };
        by_id.remove(&id);
        resolved.insert(id.clone());
        ordered.push(id);
    }
    Ok(ordered)
}

fn packet_key(packet: &Packet) -> Result<String> {
    // Exclude the self-hash; declaration order of inputs/dependencies is explicit.
    canonical_digest(&(
        &packet.schema,
        &packet.project,
        &packet.source_commit,
        &packet.task,
        &packet.dependencies,
    ))
}
pub fn check_packet(packet: &Packet) -> Result<()> {
    canonical_digest(packet)?;
    task(&packet.task)?;
    if packet.schema != PACKET_SCHEMA
        || !token(&packet.project)
        || !hex(&packet.source_commit, &[40, 64])
        || !hex(&packet.task_key, &[64])
    {
        return fail(ErrorKind::Invalid, "packet-contract");
    }
    if packet.dependencies.len() != packet.task.depends_on.len()
        || packet
            .task
            .depends_on
            .iter()
            .any(|id| !packet.dependencies.contains_key(id))
    {
        return fail(ErrorKind::Missing, "dependency-result");
    }
    for dependency in packet.dependencies.values() {
        reel_publication::check_artifact_set(&dependency.artifacts)?;
        if !hex(&dependency.result_sha256, &[64]) {
            return fail(ErrorKind::Invalid, "dependency-result-digest");
        }
    }
    if packet.task_key != packet_key(packet)? {
        return fail(ErrorKind::Integrity, "packet-key");
    }
    Ok(())
}

/// Builds a private planning packet. Input closure/protection and dependency
/// retention must be admitted by the managed lifecycle before real dispatch.
pub fn make_packet(
    plan: &Plan,
    task_id: &str,
    dependencies: BTreeMap<String, DependencyResult>,
) -> Result<Packet> {
    check_plan(plan)?;
    let value = plan
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "requested-task".into(),
        })?;
    let mut packet = Packet {
        schema: PACKET_SCHEMA.into(),
        project: plan.project.clone(),
        source_commit: plan.source_commit_for(task_id)?.to_owned(),
        task: value.clone(),
        dependencies,
        task_key: String::new(),
    };
    packet.task_key = packet_key(&packet)?;
    check_packet(&packet)?;
    Ok(packet)
}

pub fn check_result(packet: &Packet, result: &ReturnedResult) -> Result<()> {
    check_packet(packet)?;
    if packet.task.operation == Operation::ExternalSourceIntake
        && (result.provenance.mode != Mode::Handoff
            || result.provenance.provider.is_some()
            || result.provenance.model.is_some())
    {
        return fail(ErrorKind::Conflict, "source-intake-handoff-provenance");
    }
    canonical_digest(result)?;
    if result.schema != RESULT_SCHEMA
        || !token(&result.attempt_id)
        || !token(&result.task_id)
        || !hex(&result.task_key, &[64])
        || [&result.provenance.provider, &result.provenance.model]
            .iter()
            .any(|v| v.as_ref().is_some_and(|s| !token(s)))
    {
        return fail(ErrorKind::Invalid, "result-contract");
    }
    if result.task_id != packet.task.id || result.task_key != packet.task_key {
        return fail(ErrorKind::Stale, "result-task-binding");
    }
    if let Some(tool) = &packet.task.tool {
        if result.provenance.mode != Mode::Tool
            || result.provenance.executable.as_ref() != Some(&tool.executable)
            || result.provenance.provider.is_some()
            || result.provenance.model.is_some()
        {
            return fail(ErrorKind::Conflict, "tool-result-provenance");
        }
    } else if result.provenance.mode == Mode::Tool || result.provenance.executable.is_some() {
        return fail(ErrorKind::Invalid, "intelligence-result-provenance");
    }
    if let Some(receipt) = &result.worker_receipt {
        artifact(receipt)?;
        if packet.task.tool.is_none()
            || receipt.media_type != "application/json"
            || receipt.bytes > MAX_CONTROL_BYTES as u64
            || result
                .artifacts
                .values()
                .any(|a| a.logical_id == receipt.logical_id)
        {
            return fail(ErrorKind::Invalid, "worker-receipt-contract");
        }
    }
    reel_publication::check_outputs(&packet.task.outputs, &result.artifacts)?;
    Ok(())
}

/// Verifies supplied bytes, not their semantic quality, provenance or retention.
pub fn check_output(
    packet: &Packet,
    result: &ReturnedResult,
    name: &str,
    bytes: &[u8],
) -> Result<()> {
    check_result(packet, result)?;
    let artifact = result.artifacts.get(name).ok_or(Error {
        kind: ErrorKind::Missing,
        code: "named-output".into(),
    })?;
    if bytes.len() as u64 != artifact.bytes || digest(bytes) != artifact.sha256 {
        return fail(ErrorKind::Integrity, "output-content");
    }
    Ok(())
}

/// Verify an exact returned output using a fixed-size buffer. `maximum` is an
/// additional caller allowance; it never expands the packet's output contract.
/// This checks bytes only, not semantic quality, execution or retained custody.
pub fn check_output_reader(
    packet: &Packet,
    result: &ReturnedResult,
    name: &str,
    reader: &mut impl std::io::Read,
    maximum: u64,
) -> Result<()> {
    check_result(packet, result)?;
    let artifact = result.artifacts.get(name).ok_or(Error {
        kind: ErrorKind::Missing,
        code: "named-output".into(),
    })?;
    if artifact.bytes > maximum {
        return fail(ErrorKind::Budget, "output-stream-allowance");
    }
    let mut hash = Sha256::new();
    let mut length = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return fail(ErrorKind::NeedsRecovery, "output-stream-io"),
        };
        length = length
            .checked_add(count as u64)
            .filter(|n| *n <= artifact.bytes)
            .ok_or_else(|| Error {
                kind: ErrorKind::Integrity,
                code: "output-content".into(),
            })?;
        hash.update(&buffer[..count]);
    }
    let actual: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if length != artifact.bytes || actual != artifact.sha256 {
        return fail(ErrorKind::Integrity, "output-content");
    }
    Ok(())
}

/// Same logical attempt may replay exactly. A different response under that
/// attempt conflicts; it must not overwrite the first or become a fresh retry.
pub fn check_replay(prior: &ReturnedResult, returned: &ReturnedResult) -> Result<()> {
    if prior.task_key != returned.task_key
        || prior.task_id != returned.task_id
        || prior.attempt_id != returned.attempt_id
        || prior != returned
    {
        return fail(ErrorKind::Conflict, "attempt-result-conflict");
    }
    Ok(())
}

/// Implementations own admission, provider calls and uncertain-operation recovery.
/// This crate does not install an adapter or dispatch automatically.
pub trait DirectAdapter {
    fn execute(&mut self, packet: &Packet, attempt_id: &str) -> Result<ReturnedResult>;
}
pub fn call_direct(
    adapter: &mut impl DirectAdapter,
    packet: &Packet,
    attempt_id: &str,
) -> Result<ReturnedResult> {
    check_packet(packet)?;
    if packet.task.operation == Operation::DeterministicTool {
        return fail(ErrorKind::Unsupported, "llm-adapter-cannot-run-tool");
    }
    if !token(attempt_id) {
        return fail(ErrorKind::Invalid, "attempt-identity");
    }
    let result = adapter.execute(packet, attempt_id)?;
    check_result(packet, &result)?;
    if result.attempt_id != attempt_id || result.provenance.mode != Mode::Direct {
        return fail(ErrorKind::Conflict, "direct-attempt-binding");
    }
    Ok(result)
}

/// Reserved identity for the separately implemented review-request contract.
pub const REVIEW_REQUEST_ID: &str = "control/review-request";
fn portable_source(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 256
        && !path.contains('\\')
        && path.split('/').all(reel_publication::valid_id)
}
pub mod upstream;
