//! External response registration and unselected native byte publication.
//! Retained-owner acknowledgement and task completion are separate operations.
use crate::{
    NativeProfile,
    admission::{self, Prepared, Receipt},
    error, hold_tool, io_error,
};
use reel_workflow::{ErrorKind, Mode, Packet, Result, ReturnedResult, check_result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::PathBuf,
};
use tig_core::{Digest, LogicalPath, Object};
use tig_store::{Key, source_claims::protection::work::WorkRef};

const MAX_BYTES: u64 = 128 * 1024 * 1024;

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
    // Worker receipts need their own production-specific validation before they
    // can enter this generic provider adapter. Never infer tool approval here.
    if result.provenance.mode == Mode::Tool {
        return Err(error(
            ErrorKind::Unsupported,
            "response-tool-receipt-adapter-required",
        ));
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
        || returns.files.len() > 32
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
    for (name, path) in &returns.files {
        if !paths.insert(path.to_ascii_lowercase()) {
            return Err(error(ErrorKind::Conflict, "response-file-alias"));
        }
        let artifact = &result.artifacts[name];
        total = total
            .checked_add(artifact.bytes)
            .filter(|n| *n <= MAX_BYTES)
            .ok_or_else(|| error(ErrorKind::Budget, "response-byte-limit"))?;
        let path = root.join(path);
        directories.push(
            reel_assets::jobs::open_workspace_directory(path.parent().unwrap())
                .map_err(|_| error(ErrorKind::Denied, "response-file-ancestors"))?,
        );
        let (raw, held) = crate::staging::read_held(&path, artifact.bytes.min(MAX_BYTES))?;
        reel_workflow::check_output(packet, result, name, &raw)?;
        files.push(held);
    }
    Ok(Verified {
        _directories: directories,
        _files: files,
        root,
    })
}

pub(crate) fn identity(
    profile: &admission::Profile,
    receipt: &Receipt,
    result: &ReturnedResult,
) -> Result<(Key, Vec<u8>, BTreeSet<Object>)> {
    // Content and activation deliberately excluded from the logical key. A new
    // return body, provider claim or slot generation conflicts, never overwrites.
    let key = operation_key(profile, &result.task_key, &result.attempt_id)?;
    let raw = admission::encode(&serde_json::json!({
        "schema":"reel.workflow-response-registration.v1", "index":profile.index_identity,
        "admission":receipt.operation_identity()?, "store":profile.scope.store, "result":result
    }))?;
    let objects = result
        .artifacts
        .values()
        .chain(result.worker_receipt.iter())
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
pub(crate) fn operation_key(
    profile: &admission::Profile,
    task_key: &str,
    attempt_id: &str,
) -> Result<Key> {
    Key::parse(&format!(
        "workflow-response-{}",
        Digest::blob(&admission::encode(&(
            "reel.workflow-response-key.v1",
            &profile.index_identity,
            task_key,
            attempt_id
        ))?)
        .as_str()
    ))
    .map_err(|_| error(ErrorKind::Invalid, "response-operation-key"))
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
#[cfg(all(windows, feature = "film"))]
pub(crate) fn stage_verified(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &Receipt,
    result: &ReturnedResult,
) -> Result<Candidate> {
    check_result(packet, result)?;
    if result.attempt_id != receipt.attempt_id {
        return Err(error(ErrorKind::Conflict, "response-original-attempt"));
    }
    let (_guard, candidate) = protected(packet, source, profile, receipt, result, false)?;
    Ok(candidate)
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
    ingest_verified(
        packet,
        source,
        profile,
        receipt,
        result,
        VerifiedFiles {
            returns,
            root: &held.root,
            worker_receipt: None,
        },
    )
}
/// Internal producer adapters must retain verified file/namespace guards across
/// this call. Public external ingestion continues to refuse tool results.
pub(crate) struct VerifiedFiles<'a> {
    pub returns: &'a Returns,
    pub root: &'a std::path::Path,
    pub worker_receipt: Option<&'a str>,
}
pub(crate) fn ingest_verified(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &Receipt,
    result: &ReturnedResult,
    files: VerifiedFiles<'_>,
) -> Result<Candidate> {
    check_result(packet, result)?;
    if result.attempt_id != receipt.attempt_id {
        return Err(error(ErrorKind::Conflict, "response-original-attempt"));
    }
    if files.returns.files.keys().ne(result.artifacts.keys()) {
        return Err(error(ErrorKind::Conflict, "response-verified-file-set"));
    }
    let mut declared: Vec<_> = files
        .returns
        .files
        .iter()
        .map(|(name, path)| (&result.artifacts[name], path.as_str()))
        .collect();
    match (&result.worker_receipt, files.worker_receipt) {
        (Some(artifact), Some(path)) => declared.push((artifact, path)),
        (None, None) => (),
        _ => return Err(error(ErrorKind::Conflict, "response-worker-file-binding")),
    }
    if declared.len() > 32
        || declared
            .iter()
            .any(|(_, path)| !crate::staging::portable(path))
        || declared
            .iter()
            .try_fold(0u64, |n, (a, _)| n.checked_add(a.bytes))
            .is_none_or(|n| n > MAX_BYTES)
    {
        return Err(error(ErrorKind::Budget, "response-verified-file-bounds"));
    }
    let _annex = hold_tool(&source.annex)?;
    let (_guard, candidate) = protected(packet, source, profile, receipt, result, true)?;
    let mut publications = Vec::new();
    let mut published_objects = BTreeSet::new();
    for (artifact, path) in declared {
        let object = Object {
            sha256: Digest::parse(&artifact.sha256)
                .map_err(|_| error(ErrorKind::Invalid, "response-output-digest"))?,
            bytes: artifact.bytes,
        };
        if !published_objects.insert(object.clone()) {
            continue;
        }
        let published = tig_git::annex::workflow::publication::publish(
            &tig_git::annex::workflow::publication::Profile {
                schema: "tig.annex-file-publication.experimental-1".into(),
                pool: source.source.pool.clone(),
                store: profile.scope.store.clone(),
                source_root: files.root.into(),
                source: LogicalPath::parse(path)
                    .map_err(|_| error(ErrorKind::Invalid, "response-output-path"))?,
                source_owned: true,
                expected: Some(object),
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
