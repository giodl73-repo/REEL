//! External response registration and unselected native byte publication.
//! Retained-owner acknowledgement and task completion are separate operations.
use crate::{
    NativeProfile,
    admission::{self, Prepared, Receipt},
    error, hold_tool, io_error,
};
use reel_workflow::{Artifact, ErrorKind, Mode, Packet, Result, ReturnedResult, check_result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::PathBuf,
};
use tig_core::{Digest, LogicalPath, Object};
use tig_store::{Key, source_claims::protection::work::WorkRef};

const MAX_BYTES: u64 = 128 * 1024 * 1024;
pub(crate) fn artifacts(result: &ReturnedResult) -> impl Iterator<Item = &Artifact> {
    result
        .artifacts
        .values()
        .chain(result.worker_receipt.iter())
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Returns {
    pub schema: String,
    /// Explicit caller-owned existing return directory; never extracted or run.
    pub root: PathBuf,
    /// Returned artifact name to portable path beneath root.
    pub files: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub schema: String,
    pub task_key: String,
    pub attempt_id: String,
    pub registration: Object,
    pub work: WorkRef,
    pub store: Digest,
}

struct Verified {
    _directories: Vec<reel_assets::jobs::WorkspaceDirectory>,
    _files: Vec<File>,
    root: PathBuf,
}

fn verify(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &Receipt,
    result: &ReturnedResult,
    returns: &Returns,
) -> Result<Verified> {
    check_result(packet, result)?;
    if result.attempt_id != receipt.attempt_id {
        return Err(error(ErrorKind::Conflict, "response-original-attempt"));
    }
    if result.provenance.mode == Mode::Tool && result.worker_receipt.is_none() {
        return Err(error(ErrorKind::Missing, "comic-worker-receipt-required"));
    }
    if !cfg!(windows) {
        return Err(error(
            ErrorKind::Unsupported,
            "response-windows-qualified-scope",
        ));
    }
    if returns.schema != "reel.workflow-returns.v1"
        || !returns.root.is_absolute()
        || returns
            .root
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
        || returns.files.keys().ne(result.artifacts.keys())
        || returns.files.len() > reel_publication::MAX_OUTPUTS
        || returns.files.values().any(|p| !crate::staging::portable(p))
    {
        return Err(error(ErrorKind::Invalid, "response-explicit-file-set"));
    }
    let mut directories = vec![
        reel_assets::jobs::open_workspace_directory(&returns.root)
            .map_err(|_| error(ErrorKind::Denied, "response-owned-return-root"))?,
    ];
    let root = returns.root.canonicalize().map_err(io_error)?;
    for managed in [
        &profile.index_root,
        &profile.metadata_root,
        &profile.scope.coordination_root,
        &source.source.pool.repository,
    ] {
        let managed = managed.canonicalize().unwrap_or_else(|_| managed.clone());
        if root.starts_with(&managed) || managed.starts_with(&root) {
            return Err(error(ErrorKind::Denied, "response-managed-root-overlap"));
        }
    }
    let mut paths = BTreeSet::new();
    let mut files = Vec::new();
    let mut total = 0u64;
    let review = if packet.task.operation == reel_workflow::Operation::ReviewReport {
        let mut native = crate::NativeInputs::open_admitted(packet, source, profile, receipt)?;
        let bound = reel_workflow::review::bind(packet, &mut native)?;
        let mut total = 0u64;
        for pin in &packet.task.inputs {
            total = total
                .checked_add(pin.bytes)
                .filter(|n| *n <= MAX_BYTES)
                .ok_or_else(|| error(ErrorKind::Budget, "review-native-input-bytes"))?;
            reel_workflow::publication::InputReader::read(&mut native, pin, MAX_BYTES)?;
        }
        Some((native, bound))
    } else {
        None
    };
    let mut review_output = None;
    for (name, path) in &returns.files {
        if !paths.insert(path.to_ascii_lowercase()) {
            return Err(error(ErrorKind::Conflict, "response-file-alias"));
        }
        let artifact = &result.artifacts[name];
        if packet.task.operation == reel_workflow::Operation::ImageCandidate
            && artifact.media_type == "image/png"
            && artifact.bytes > reel_workflow_http::raster::MAX_IMAGE
        {
            return Err(error(ErrorKind::Budget, "image-return-png-bytes"));
        }
        total = total
            .checked_add(artifact.bytes)
            .filter(|n| *n <= MAX_BYTES)
            .ok_or_else(|| error(ErrorKind::Budget, "response-byte-limit"))?;
        let path = root.join(path);
        directories.push(
            reel_assets::jobs::open_workspace_directory(path.parent().unwrap())
                .map_err(|_| error(ErrorKind::Denied, "response-file-ancestors"))?,
        );
        let image = packet.task.operation == reel_workflow::Operation::ImageCandidate
            && artifact.media_type == "image/png";
        let report = review
            .as_ref()
            .is_some_and(|(_, bound)| bound.report_output() == name);
        let held = if image || report {
            let (raw, held) = crate::staging::read_held(&path, artifact.bytes.min(MAX_BYTES))?;
            reel_workflow::check_output(packet, result, name, &raw)?;
            if image {
                reel_workflow_http::raster::inspect_png(&raw)?;
            }
            if report {
                review_output = Some(raw);
            }
            held
        } else {
            let mut held = crate::staging::open_held(&path, artifact.bytes.min(MAX_BYTES))?;
            reel_workflow::check_output_reader(packet, result, name, &mut held, MAX_BYTES)?;
            held
        };
        files.push(held);
    }
    if let Some((_, bound)) = &review {
        let raw = review_output
            .as_ref()
            .ok_or_else(|| error(ErrorKind::Missing, "review-report-output"))?;
        bound.check(result, raw)?;
    }
    if result.provenance.mode == Mode::Tool {
        if paths.contains("receipt.json") {
            return Err(error(ErrorKind::Conflict, "comic-receipt-path-alias"));
        }
        let worker_receipt = result
            .worker_receipt
            .as_ref()
            .ok_or_else(|| error(ErrorKind::Missing, "comic-worker-receipt-required"))?;
        if total
            .checked_add(worker_receipt.bytes)
            .is_none_or(|n| n > MAX_BYTES)
        {
            return Err(error(ErrorKind::Budget, "response-byte-limit"));
        }
        let native = crate::NativeInputs::open_admitted(packet, source, profile, receipt)?;
        let (extra_files, extra_directories) =
            crate::producer::check_returned(packet, source, native, result, &root, &returns.files)?;
        files.extend(extra_files);
        directories.extend(extra_directories);
    }
    Ok(Verified {
        _directories: directories,
        _files: files,
        root,
    })
}

/// Read-only semantic/byte preflight, before a handoff records import intent.
pub(crate) fn preflight(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &Receipt,
    result: &ReturnedResult,
    returns: &Returns,
) -> Result<()> {
    drop(verify(packet, source, profile, receipt, result, returns)?);
    Ok(())
}

pub(crate) fn identity(
    profile: &admission::Profile,
    receipt: &Receipt,
    result: &ReturnedResult,
) -> Result<(Key, Vec<u8>, BTreeSet<Object>)> {
    // Content and activation deliberately excluded from the logical key. A new
    // return body, provider claim or slot generation conflicts, never overwrites.
    let key = Key::parse(&format!(
        "workflow-response-{}",
        Digest::blob(&admission::encode(&(
            "reel.workflow-response-key.v1",
            &profile.index_identity,
            &result.task_key,
            &result.attempt_id
        ))?)
        .as_str()
    ))
    .map_err(|_| error(ErrorKind::Invalid, "response-operation-key"))?;
    let raw = admission::encode(&serde_json::json!({
        "schema":"reel.workflow-response-registration.v1", "index":profile.index_identity,
        "admission":receipt.operation_identity()?, "store":profile.scope.store, "result":result
    }))?;
    let objects = artifacts(result)
        .map(|a| {
            Ok(Object {
                sha256: Digest::parse(&a.sha256)
                    .map_err(|_| error(ErrorKind::Invalid, "response-output-digest"))?,
                bytes: a.bytes,
            })
        })
        .collect::<Result<_>>()?;
    Ok((key, raw, objects))
}

fn protected(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &Receipt,
    result: &ReturnedResult,
    apply: bool,
) -> Result<(admission::ClaimsGuard, Candidate)> {
    let (key, raw, outputs) = identity(profile, receipt, result)?;
    let registration = Object::from_bytes(&raw);
    let guard = Prepared::open(packet, source, profile, &receipt.attempt_id)?.response_guard(
        receipt,
        &key,
        &registration,
        &outputs,
    )?;
    let work = guard.protect_response(receipt, &key, &raw, &outputs, apply)?;
    Ok((
        guard,
        Candidate {
            schema: "reel.workflow-response-candidate.v1".into(),
            task_key: result.task_key.clone(),
            attempt_id: result.attempt_id.clone(),
            registration,
            work,
            store: profile.scope.store.clone(),
        },
    ))
}

/// Freeze logical identity and original WorkRef without admitting output claims
/// or publishing payloads. A Candidate descriptor alone proves neither.
pub fn stage(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &Receipt,
    result: &ReturnedResult,
    returns: &Returns,
) -> Result<Candidate> {
    let _held = verify(packet, source, profile, receipt, result, returns)?;
    let (_guard, candidate) = protected(packet, source, profile, receipt, result, false)?;
    Ok(candidate)
}

/// Verify every returned file and record original logical identity plus positive
/// job protection. This does not publish bytes or confer retained ownership.
pub fn register(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &Receipt,
    result: &ReturnedResult,
    returns: &Returns,
) -> Result<Candidate> {
    let _held = verify(packet, source, profile, receipt, result, returns)?;
    let (_guard, candidate) = protected(packet, source, profile, receipt, result, true)?;
    Ok(candidate)
}

/// Idempotent exact-byte ingress into the explicit EXISTING native store. On
/// uncertainty retry this exact body/attempt. Preserve original return files.
/// All publisher handles remain held until every object reopens successfully.
pub fn ingest(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &Receipt,
    result: &ReturnedResult,
    returns: &Returns,
) -> Result<Candidate> {
    let held = verify(packet, source, profile, receipt, result, returns)?;
    let _annex = hold_tool(&source.annex)?;
    let (_guard, candidate) = protected(packet, source, profile, receipt, result, true)?;
    let mut publications = Vec::new();
    let mut published_objects = BTreeSet::new();
    for (artifact, path) in returns
        .files
        .iter()
        .map(|(name, path)| (&result.artifacts[name], path.as_str()))
        .chain(result.worker_receipt.as_ref().map(|a| (a, "receipt.json")))
    {
        if !published_objects.insert((artifact.sha256.clone(), artifact.bytes)) {
            continue;
        }
        let published = tig_git::annex::workflow::publication::publish(
            &tig_git::annex::workflow::publication::Profile {
                schema: "tig.annex-file-publication.experimental-1".into(),
                pool: source.source.pool.clone(),
                store: profile.scope.store.clone(),
                source_root: held.root.clone(),
                source: LogicalPath::parse(path)
                    .map_err(|_| error(ErrorKind::Invalid, "response-output-path"))?,
                source_owned: true,
                expected: Some(Object {
                    sha256: Digest::parse(&artifact.sha256)
                        .map_err(|_| error(ErrorKind::Invalid, "response-output-digest"))?,
                    bytes: artifact.bytes,
                }),
                include_private_location: false,
                maximum_bytes: MAX_BYTES,
                minimum_free_bytes: 100 * 1024 * 1024,
            },
        )
        .map_err(io_error)?;
        if !published.report().reopened_verified {
            return Err(error(ErrorKind::Integrity, "response-native-reopen"));
        }
        publications.push(published);
    }
    Ok(candidate)
}
