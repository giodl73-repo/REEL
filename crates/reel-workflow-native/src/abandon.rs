//! Explicit cancellation and source-only retained handoff. No result creation,
//! scratch retirement, payload deletion, provider calls or implicit expiry.
use crate::{
    Tool, admission, closing, error, graph, handoff, hold_tool, io_error, retention, staging,
};
use reel_workflow::{ErrorKind, Result, handoff::State};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use tig_store::Key;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    pub schema: String,
    pub journal: PathBuf,
    pub profile: handoff::Profile,
    pub original: reel_workflow::handoff::Ticket,
    pub target: retention::Target,
    pub policy: retention::capacity::Policy,
    pub reason: String,
    pub runtime: Tool,
    pub operation: Key,
}
fn identity(ticket: &Ticket) -> Result<Key> {
    let digest = reel_workflow::canonical_digest(&serde_json::json!({
        "schema":"reel.workflow-abandon-operation.v1",
        "admission":ticket.profile.binding.admission.operation_identity()?,
        "original":ticket.original,"target":ticket.target,"policy":ticket.policy,
        "reason":ticket.reason,"closer_sha256":ticket.runtime.sha256,"closer_bytes":ticket.runtime.bytes
    }))?;
    Key::parse(&format!("workflow-abandon-{digest}"))
        .map_err(|_| error(ErrorKind::Invalid, "abandon-operation-identity"))
}

/// Preview the original Waiting task. Already-cancelled jobs must reconcile the
/// original exported ticket, rather than prepare a different close operation.
pub fn prepare(
    journal: &Path,
    profile: handoff::Profile,
    target: retention::Target,
    policy: retention::capacity::Policy,
    reason: &str,
) -> Result<(Ticket, closing::Outcome)> {
    let original = handoff::observe(journal, &profile)?;
    if original.state != State::Waiting {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "abandon-original-waiting-job-required",
        ));
    }
    handoff::verify_observation(journal, &profile, &original)?;
    let mut ticket = Ticket {
        schema: "reel.workflow-abandon-ticket.v1".into(),
        journal: journal.into(),
        profile,
        original: original.ticket,
        target,
        policy,
        reason: reason.into(),
        runtime: closing::current_runtime()?,
        operation: Key::parse("pending")
            .map_err(|_| error(ErrorKind::Invalid, "abandon-operation-identity"))?,
    };
    ticket.operation = identity(&ticket)?;
    let preview = ticket.invoke(closing::Action::Preview)?;
    Ok((ticket, preview))
}
impl Ticket {
    fn invoke(&self, action: closing::Action) -> Result<closing::Outcome> {
        reel_workflow::canonical_digest(self)?;
        self.policy.validate()?;
        if self.schema != "reel.workflow-abandon-ticket.v1"
            || self.operation != identity(self)?
            || self.reason.is_empty()
            || self.reason.len() > 128
            || !self
                .reason
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || self.target != self.profile.binding.target
        {
            return Err(error(ErrorKind::Conflict, "abandon-original-ticket"));
        }
        let _runtime = hold_tool(&self.runtime)?;
        if self.runtime.path.canonicalize().map_err(io_error)?
            != std::env::current_exe()
                .map_err(io_error)?
                .canonicalize()
                .map_err(io_error)?
        {
            return Err(error(
                ErrorKind::Conflict,
                "abandon-original-closer-required",
            ));
        }
        let _journal = handoff::lock_abandoned(
            &self.journal,
            &self.profile,
            &self.original,
            &self.operation,
            action != closing::Action::Preview,
        )?;
        admission::Prepared::open(
            &self.original.packet,
            &self.profile.source,
            &self.profile.admission,
            &self.original.attempt_id,
        )?
        .close_abandoned(
            &self.profile.binding.admission,
            &self.target,
            &self.operation,
            action,
            &self.policy,
        )
    }
    fn output(&self, path: &Path) -> Result<(reel_assets::jobs::WorkspaceDirectory, PathBuf)> {
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(error(ErrorKind::Invalid, "abandon-private-output-path"));
        }
        let parent = reel_assets::jobs::open_workspace_directory(
            path.parent()
                .ok_or_else(|| error(ErrorKind::Invalid, "abandon-output-parent"))?,
        )
        .map_err(|_| error(ErrorKind::Denied, "abandon-output-parent"))?;
        staging::check_parent(parent.path(), &self.profile.source, &self.profile.admission)?;
        for root in [
            &self.journal,
            &self.profile.acknowledgements,
            &self.profile.source.snapshot.repository,
        ] {
            if parent
                .path()
                .starts_with(root.canonicalize().map_err(io_error)?)
            {
                return Err(error(ErrorKind::Denied, "abandon-output-overlaps-control"));
            }
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| staging::portable(n))
            .ok_or_else(|| error(ErrorKind::Invalid, "abandon-output-name"))?;
        let path = parent.path().join(name);
        Ok((parent, path))
    }
    pub fn export(&self, path: &Path) -> Result<String> {
        let (_parent, path) = self.output(path)?;
        let raw = serde_json::to_vec(self)
            .map_err(|_| error(ErrorKind::Invalid, "abandon-ticket-json"))?;
        write(&path, &raw)?;
        Ok(reel_workflow::digest(&raw))
    }
    pub fn execute_to(&self, path: &Path, recover: bool) -> Result<closing::Outcome> {
        self.outcome_to(
            path,
            if recover {
                closing::Action::Recover
            } else {
                closing::Action::Apply
            },
        )
    }
    /// Qualification boundary: preserve a real handoff staged before adoption.
    pub fn stage_to(&self, path: &Path) -> Result<closing::Outcome> {
        self.outcome_to(path, closing::Action::Stage)
    }
    fn outcome_to(&self, path: &Path, action: closing::Action) -> Result<closing::Outcome> {
        let (_parent, path) = self.output(path)?;
        let mut file = exclusive(&path)?;
        let outcome = self.invoke(action)?;
        let raw = serde_json::to_vec(&outcome)
            .map_err(|_| error(ErrorKind::Invalid, "abandon-outcome-json"))?;
        file.write_all(&raw)
            .and_then(|_| file.sync_all())
            .map_err(|_| {
                error(
                    ErrorKind::NeedsRecovery,
                    "abandon-receipt-preserve-original",
                )
            })?;
        drop(file);
        readback(&path, &raw)?;
        Ok(outcome)
    }
}
fn exclusive(path: &Path) -> Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                error(ErrorKind::Conflict, "abandon-existing-output")
            } else {
                io_error(e)
            }
        })
}
fn readback(path: &Path, raw: &[u8]) -> Result<()> {
    let (written, _held) = staging::read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    if written != raw {
        return Err(error(ErrorKind::Integrity, "abandon-control-readback"));
    }
    Ok(())
}
fn write(path: &Path, raw: &[u8]) -> Result<()> {
    if raw.len() > reel_workflow::MAX_CONTROL_BYTES {
        return Err(error(ErrorKind::Budget, "abandon-control-bytes"));
    }
    let mut file = exclusive(path)?;
    file.write_all(raw)
        .and_then(|_| file.sync_all())
        .map_err(|_| {
            error(
                ErrorKind::NeedsRecovery,
                "abandon-control-preserve-original",
            )
        })?;
    drop(file);
    readback(path, raw)
}
pub fn read_pinned(path: &Path, sha256: &str) -> Result<(Ticket, File)> {
    let (raw, file) = graph::PinnedFile {
        path: path.into(),
        sha256: sha256.into(),
    }
    .read()?;
    Ok((reel_workflow::decode(&raw)?, file))
}
