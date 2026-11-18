//! Exact selection and positive transfer observations. Plans grant no authority.
use super::{
    identity::Key,
    process::{Command, Output, Remote, Runner, Uuid},
};
use crate::source::Extraction;
use sha2::{Digest as _, Sha256};
use std::{
    collections::BTreeSet,
    fs::OpenOptions,
    io::{self, Read},
    path::Component,
};
use tig_core::{Claim, Digest, Object, Placement, View};
use tig_store::source_claims::protection::{Index, effects::HeldObject};
pub mod adoption;
pub mod enrollment;
pub mod merge;
pub mod policy;
pub mod publication;
pub mod reader;

pub struct Selection {
    pub digest: Digest,
    pub objects: BTreeSet<Object>,
}
impl Selection {
    pub fn from_annex(extraction: &Extraction) -> io::Result<Self> {
        if !extraction.is_annex() {
            return Err(invalid());
        }
        Ok(Self {
            digest: extraction.selection_digest().map_err(|_| invalid())?,
            objects: extraction
                .bindings()
                .iter()
                .map(|b| b.object.clone())
                .collect(),
        })
    }
}
fn invalid() -> io::Error {
    io::Error::other("annex workflow refused")
}

/// Release placement is a pure projection using Tig's existing planner. Caller
/// observations are not custody, coverage or permission to remove anything.
pub fn plan(
    current: &Selection,
    deprecated: &[Selection],
    additional: Vec<Claim>,
    observed: &BTreeSet<Placement>,
) -> io::Result<tig_core::Plan> {
    if deprecated.len() > 128 || additional.len() > 128 {
        return Err(invalid());
    }
    let mut claims = additional;
    let golden = current
        .objects
        .iter()
        .flat_map(|o| {
            ["F", "M"].map(|p| Placement {
                object: o.clone(),
                place: p.into(),
            })
        })
        .collect();
    claims.push(Claim {
        id: "annex-current".into(),
        placements: golden,
    });
    let old = deprecated
        .iter()
        .flat_map(|s| &s.objects)
        .filter(|o| !current.objects.contains(o))
        .flat_map(|o| {
            ["D", "M"].map(|p| Placement {
                object: o.clone(),
                place: p.into(),
            })
        })
        .collect();
    claims.push(Claim {
        id: "annex-deprecated".into(),
        placements: old,
    });
    // This adapter has not established complete admitted consumer coverage.
    View::new(1, false, claims)
        .and_then(|v| v.plan(observed))
        .map_err(|_| invalid())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferOutcome {
    /// Exact destination bytes verified; not retained or independent custody.
    VerifiedPresent,
    /// No per-key success, skipped bytes, or unavailable verification.
    Pending,
}
fn info(runner: &Runner, remote: Option<&Remote>) -> io::Result<serde_json::Value> {
    let output = runner.run(Command::Info(remote))?;
    if !output.success {
        return Err(invalid());
    }
    serde_json::from_slice(&output.stdout).map_err(|_| invalid())
}
fn uuid(info: &serde_json::Value) -> io::Result<Uuid> {
    Uuid::parse(
        info.get("uuid")
            .and_then(|v| v.as_str())
            .ok_or_else(invalid)?,
    )
}
fn managed_guards(runners: &[&Runner], key: &Key) -> io::Result<Vec<HeldObject>> {
    let mut controls = Vec::new();
    for runner in runners {
        if let Some(binding) = runner.control() {
            if uuid(&info(runner, None)?)?.store() != binding.store {
                return Err(invalid());
            }
            controls.push(binding);
        }
    }
    controls.sort_by(|a, b| a.index.cmp(&b.index));
    let mut held = Vec::new();
    let mut previous: Option<(Digest, std::path::PathBuf)> = None;
    for binding in controls {
        let root = std::fs::canonicalize(&binding.root)?;
        let mut index = Index::open(&binding.root, &binding.minimum).map_err(|_| invalid())?;
        if index.identity() != &binding.index
            || !index.supports_effect_fences()
            || !index.enrollment_profile().stores.contains(&binding.store)
        {
            return Err(invalid());
        }
        if let Some((identity, old_root)) = &previous
            && identity == &binding.index
        {
            if old_root != &root {
                return Err(invalid());
            }
            continue;
        }
        held.push(
            index
                .hold_object(&key.object(), &binding.store)
                .map_err(|_| invalid())?,
        );
        previous = Some((binding.index.clone(), root));
    }
    Ok(held)
}

/// Discover one legacy key without scanning the pool. An untrusted location log
/// is needed because annex's get --from silently skips unrecorded remote keys.
/// This observation never promotes the pool to retained-copy authority.
pub fn get_legacy(
    runner: &Runner,
    key: &Key,
    from: &Remote,
    budget: u64,
) -> io::Result<TransferOutcome> {
    if key.object().bytes > budget {
        return Err(invalid());
    }
    let _admission = managed_guards(&[runner], key)?;
    if verify_present(runner, key, budget).unwrap_or(false) {
        return Ok(TransferOutcome::VerifiedPresent);
    }
    let remote = info(runner, Some(from))?;
    if remote.get("trust").and_then(|v| v.as_str()) != Some("untrusted") {
        return Err(invalid());
    }
    let remote_uuid = uuid(&remote)?;
    if !runner
        .run(Command::CheckPresent { key, remote: from })?
        .success
    {
        return Ok(TransferOutcome::Pending);
    }
    if !runner
        .run(Command::RegisterPresent {
            key,
            uuid: &remote_uuid,
        })?
        .success
    {
        return Ok(TransferOutcome::Pending);
    }
    let output = runner.run(Command::Get { key, from })?;
    if successful_key(&output, key, "get") && verify_present(runner, key, budget).unwrap_or(false) {
        Ok(TransferOutcome::VerifiedPresent)
    } else {
        Ok(TransferOutcome::Pending)
    }
}

/// Positive copy only. The caller admits scratch before remote-to-remote staging.
/// A successful old destination can be reused even when annex skips transfer.
pub fn copy(
    caller: &Runner,
    destination: &Runner,
    key: &Key,
    from: Option<&Remote>,
    to: &Remote,
    scratch_bytes: u64,
    verification_bytes: u64,
) -> io::Result<TransferOutcome> {
    if from.is_some() && scratch_bytes < key.object().bytes {
        return Err(invalid());
    }
    if verification_bytes < key.object().bytes {
        return Err(invalid());
    }
    let _admission = managed_guards(&[caller, destination], key)?;
    let target = info(caller, Some(to))?;
    if target.get("trust").and_then(|v| v.as_str()) == Some("untrusted")
        || uuid(&target)? != uuid(&info(destination, None)?)?
    {
        return Err(invalid());
    }
    let output = caller.run(Command::Copy { key, from, to })?;
    // Both the actual command's per-key outcome and destination evidence matter.
    if !successful_key(&output, key, "copy") {
        return Ok(TransferOutcome::Pending);
    }
    match verify_present(destination, key, verification_bytes) {
        Ok(true) => Ok(TransferOutcome::VerifiedPresent),
        _ => Ok(TransferOutcome::Pending),
    }
}
pub fn successful_key(output: &Output, key: &Key, command: &str) -> bool {
    if !output.success {
        return false;
    }
    output
        .stdout
        .split(|b| *b == b'\n')
        .filter(|b| !b.is_empty())
        .any(|line| {
            serde_json::from_slice::<serde_json::Value>(line).is_ok_and(|v| {
                v.get("key").and_then(|v| v.as_str()) == Some(key.as_str())
                    && v.get("command").and_then(|v| v.as_str()) == Some(command)
                    && v.get("success").and_then(|v| v.as_bool()) == Some(true)
            })
        })
}

/// Ask annex for its provider-owned physical location; do not reproduce annex's
/// internal hashed directory scheme. Hold the path and stream-check exact bytes.
pub fn verify_present(runner: &Runner, key: &Key, byte_budget: u64) -> io::Result<bool> {
    Ok(hold_copy(runner, key, byte_budget)?.is_some())
}
pub(crate) struct VerifiedCopy {
    location: String,
    _parents: Vec<std::fs::File>,
    _file: std::fs::File,
}
impl VerifiedCopy {
    pub(super) fn same_file(&self, source: &same_file::Handle) -> io::Result<bool> {
        Ok(*source == same_file::Handle::from_file(self._file.try_clone()?)?)
    }
}
pub(crate) fn hold_copy(
    runner: &Runner,
    key: &Key,
    byte_budget: u64,
) -> io::Result<Option<VerifiedCopy>> {
    hold_copy_with_guards(runner, key, byte_budget, super::legacy::hold_directories)
}
fn hold_copy_with_guards(
    runner: &Runner,
    key: &Key,
    byte_budget: u64,
    guard_directories: fn(&std::path::Path) -> io::Result<Vec<std::fs::File>>,
) -> io::Result<Option<VerifiedCopy>> {
    if key.object().bytes > byte_budget {
        return Err(invalid());
    }
    let output = runner.run(Command::ContentLocationBatch(key))?;
    if !output.success {
        return Err(invalid());
    }
    if matches!(output.stdout.as_slice(), b"\n" | b"\r\n") {
        return Ok(None);
    }
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| invalid())?
        .trim_end_matches(['\r', '\n']);
    let relative = std::path::Path::new(text);
    if text.is_empty()
        || text.contains(['\r', '\n'])
        || relative.is_absolute()
        || relative
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(invalid());
    }
    let path = runner.repository().join(relative);
    let parents = guard_directories(path.parent().ok_or_else(invalid)?)?;
    let metadata = std::fs::symlink_metadata(&path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x00200000);
    }
    let mut file = options.open(&path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if file.metadata()?.file_attributes() & 0x400 != 0 {
            return Err(invalid());
        }
    }
    if file.metadata()?.len() != key.object().bytes {
        return Err(invalid());
    }
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > std::time::Duration::from_secs(30) {
            return Err(invalid());
        }
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > key.object().bytes {
            return Err(invalid());
        }
        hash.update(&buffer[..n]);
    }
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if count != key.object().bytes || digest != key.object().sha256.as_str() {
        return Err(invalid());
    }
    Ok(Some(VerifiedCopy {
        location: text.to_owned(),
        _parents: parents,
        _file: file,
    }))
}

pub mod control;
pub mod retirement;
