//! Pinned close intent over the original qualified slot lifecycle.
use crate::{
    Tool,
    admission::Prepared,
    error, graph, handoff, hold_tool, io_error,
    retention::{Acknowledgement, Target},
    staging,
};
use reel_workflow::{ErrorKind, Result, handoff::State};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tig_core::Object;
use tig_store::{
    Key, metadata,
    source_claims::{Checkpoint, protection::work::WorkRef},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Preview,
    Stage,
    Apply,
    Recover,
}
#[derive(Debug)]
pub(crate) struct Staged;
impl std::fmt::Display for Staged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original close handoff staged")
    }
}
impl std::error::Error for Staged {}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub schema: String,
    pub operation: Key,
    pub admission_identity: String,
    pub activation: Object,
    pub target: Target,
    pub applied: bool,
    pub slot_idle: bool,
    pub original_clear_complete: bool,
    pub handoff: Option<WorkRef>,
    pub clear: Option<WorkRef>,
    pub index_checkpoint: Checkpoint,
    pub metadata_checkpoint: metadata::Checkpoint,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Original {
    pub ticket: reel_workflow::handoff::Ticket,
    pub result: reel_workflow::ReturnedResult,
    pub acknowledgement: reel_workflow::Artifact,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    pub schema: String,
    pub journal: PathBuf,
    pub profile: handoff::Profile,
    pub original: Original,
    pub acknowledgement: Acknowledgement,
    pub target: Target,
    pub runtime: Tool,
    pub operation: Key,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_owner_plan: Option<crate::retention::capacity::Plan>,
}
fn identity(ticket: &Ticket) -> Result<Key> {
    let mut subject = serde_json::json!({
        "schema":"reel.workflow-close-operation.v1","admission":ticket.profile.binding.admission.operation_identity()?,
        "result":ticket.original.result,"acknowledgement":ticket.acknowledgement.artifact()?,"target":ticket.target,
        "closer_sha256":ticket.runtime.sha256,"closer_bytes":ticket.runtime.bytes,
    });
    if let Some(plan) = &ticket.retained_owner_plan {
        subject["schema"] = serde_json::json!("reel.workflow-close-operation.v2");
        subject["retained_owner_plan"] =
            serde_json::to_value(Object::from_bytes(&plan.canonical_bytes()?))
                .map_err(|_| error(ErrorKind::Invalid, "close-policy-object-json"))?;
    }
    Key::parse(&format!(
        "workflow-close-{}",
        reel_workflow::canonical_digest(&subject)?
    ))
    .map_err(|_| error(ErrorKind::Invalid, "close-operation-identity"))
}
fn current_runtime() -> Result<Tool> {
    let path = std::env::current_exe()
        .map_err(io_error)?
        .canonicalize()
        .map_err(io_error)?;
    let file = File::open(&path).map_err(io_error)?;
    let mut raw = Vec::new();
    file.take(256 * 1024 * 1024 + 1)
        .read_to_end(&mut raw)
        .map_err(io_error)?;
    if raw.len() > 256 * 1024 * 1024 {
        return Err(error(ErrorKind::Budget, "close-runtime-bytes"));
    }
    let runtime = Tool {
        path,
        sha256: reel_workflow::digest(&raw),
        bytes: raw.len() as u64,
    };
    drop(hold_tool(&runtime)?);
    Ok(runtime)
}
pub fn prepare(
    journal: &Path,
    profile: handoff::Profile,
    target: Target,
) -> Result<(Ticket, Outcome)> {
    prepare_inner(journal, profile, target, None)
}
pub fn prepare_with_retained_policy(
    journal: &Path,
    profile: handoff::Profile,
    target: Target,
    plan: crate::retention::capacity::Plan,
    expected_sha256: &str,
) -> Result<(Ticket, Outcome)> {
    crate::retention::capacity::check_pin(&plan, expected_sha256)?;
    if profile.binding.retained_owner_policy.is_some() {
        return Err(error(
            ErrorKind::Conflict,
            "close-owner-policy-already-bound",
        ));
    }
    prepare_inner(journal, profile, target, Some(plan))
}
fn prepare_inner(
    journal: &Path,
    profile: handoff::Profile,
    target: Target,
    retained_owner_plan: Option<crate::retention::capacity::Plan>,
) -> Result<(Ticket, Outcome)> {
    let original = handoff::observe(journal, &profile)?;
    if original.state != State::Retained {
        return Err(error(ErrorKind::Denied, "close-retained-job-required"));
    }
    let ack_path = profile.acknowledgements.join(format!(
        "retained-{}-{}.json",
        original.ticket.packet.task_key, original.ticket.attempt_id
    ));
    let (raw, _held) = staging::read_held(&ack_path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    let acknowledgement: Acknowledgement = reel_workflow::decode(&raw)?;
    if acknowledgement.canonical_bytes()? != raw
        || Some(acknowledgement.artifact()?) != original.acknowledgement
    {
        return Err(error(ErrorKind::Conflict, "close-original-acknowledgement"));
    }
    let runtime = current_runtime()?;
    let mut ticket = Ticket {
        schema: if retained_owner_plan.is_some() {
            "reel.workflow-close-ticket.v2"
        } else {
            "reel.workflow-close-ticket.v1"
        }
        .into(),
        journal: journal.into(),
        profile,
        original: Original {
            ticket: original.ticket,
            result: original
                .result
                .ok_or_else(|| error(ErrorKind::Missing, "close-original-result"))?,
            acknowledgement: original
                .acknowledgement
                .ok_or_else(|| error(ErrorKind::Missing, "close-original-acknowledgement"))?,
        },
        acknowledgement,
        target,
        runtime,
        operation: Key::parse("pending-close-identity")
            .map_err(|_| error(ErrorKind::Invalid, "close-operation-key"))?,
        retained_owner_plan,
    };
    ticket.operation = identity(&ticket)?;
    let preview = ticket.invoke(Action::Preview)?;
    Ok((ticket, preview))
}
impl Ticket {
    fn invoke(&self, action: Action) -> Result<Outcome> {
        reel_workflow::canonical_digest(self)?;
        if !matches!(
            (self.schema.as_str(), &self.retained_owner_plan),
            ("reel.workflow-close-ticket.v1", None) | ("reel.workflow-close-ticket.v2", Some(_))
        ) || self.operation != identity(self)?
            || self.acknowledgement.artifact()? != self.original.acknowledgement
            || self.acknowledgement.target != self.profile.binding.target
        {
            return Err(error(ErrorKind::Conflict, "close-original-ticket"));
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
                "close-original-runtime-required",
            ));
        }
        let observed = handoff::observe(&self.journal, &self.profile)?;
        if observed.state != State::Retained
            || observed.ticket != self.original.ticket
            || observed.result.as_ref() != Some(&self.original.result)
            || observed.acknowledgement.as_ref() != Some(&self.original.acknowledgement)
        {
            return Err(error(ErrorKind::Stale, "close-original-job-changed"));
        }
        if action != Action::Recover {
            handoff::verify_observation(&self.journal, &self.profile, &observed)?;
        }
        let result = &self.original.result;
        let policy = match (
            &self.profile.binding.retained_owner_policy,
            &self.retained_owner_plan,
        ) {
            (Some(_), Some(_)) => {
                return Err(error(
                    ErrorKind::Conflict,
                    "close-owner-policy-already-bound",
                ));
            }
            (Some(policy), None) => Some((policy, None)),
            (None, Some(plan)) => Some((&plan.policy, Some(plan))),
            (None, None) => None,
        };
        let owner_request = policy
            .map(|(policy, recovered)| {
                crate::retention::capacity::request(
                    &self.original.ticket.packet,
                    &self.profile.source,
                    &self.profile.admission,
                    &self.profile.binding.admission,
                    result,
                    &self.profile.binding.target,
                )
                .map(|request| crate::retention::capacity::ClosePolicy {
                    policy,
                    request,
                    recovered,
                })
            })
            .transpose()?;
        Prepared::open(
            &self.original.ticket.packet,
            &self.profile.source,
            &self.profile.admission,
            &self.original.ticket.attempt_id,
        )?
        .close_retained(
            &self.profile.binding.admission,
            result,
            &self.acknowledgement,
            &self.target,
            &self.operation,
            action,
            owner_request.as_ref(),
        )
    }
    fn output(&self, path: &Path) -> Result<(reel_assets::jobs::WorkspaceDirectory, PathBuf)> {
        if !path.is_absolute()
            || path
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir))
        {
            return Err(error(ErrorKind::Invalid, "close-output-path"));
        }
        let parent = reel_assets::jobs::open_workspace_directory(
            path.parent()
                .ok_or_else(|| error(ErrorKind::Invalid, "close-output-parent"))?,
        )
        .map_err(|_| error(ErrorKind::Denied, "close-output-namespace"))?;
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
                return Err(error(ErrorKind::Denied, "close-output-overlaps-control"));
            }
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|s| staging::portable(s))
            .ok_or_else(|| error(ErrorKind::Invalid, "close-output-name"))?;
        let output = parent.path().join(name);
        Ok((parent, output))
    }
    pub fn export(&self, path: &Path) -> Result<String> {
        let (_parent, path) = self.output(path)?;
        let raw =
            serde_json::to_vec(self).map_err(|_| error(ErrorKind::Invalid, "close-ticket-json"))?;
        write(&path, &raw)?;
        Ok(reel_workflow::digest(&raw))
    }
    pub fn execute_to(&self, path: &Path, recover: bool) -> Result<Outcome> {
        self.outcome_to(
            path,
            if recover {
                Action::Recover
            } else {
                Action::Apply
            },
        )
    }
    pub fn stage_to(&self, path: &Path) -> Result<Outcome> {
        self.outcome_to(path, Action::Stage)
    }
    fn outcome_to(&self, path: &Path, action: Action) -> Result<Outcome> {
        let (_parent, path) = self.output(path)?;
        let mut file = exclusive(&path)?;
        let outcome = self.invoke(action)?;
        let raw = serde_json::to_vec(&outcome)
            .map_err(|_| error(ErrorKind::Invalid, "close-outcome-json"))?;
        if raw.len() > reel_workflow::MAX_CONTROL_BYTES {
            return Err(error(ErrorKind::Budget, "close-outcome-bytes"));
        }
        file.write_all(&raw)
            .and_then(|_| file.sync_all())
            .map_err(|_| error(ErrorKind::NeedsRecovery, "close-receipt-preserve-original"))?;
        drop(file);
        readback(&path, &raw)?;
        Ok(outcome)
    }
}
fn exclusive(path: &Path) -> Result<File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                error(ErrorKind::Conflict, "close-existing-output")
            } else {
                io_error(e)
            }
        })
}
fn readback(path: &Path, raw: &[u8]) -> Result<()> {
    let (written, _held) = staging::read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    if written != raw {
        return Err(error(ErrorKind::Integrity, "close-control-readback"));
    }
    Ok(())
}
fn write(path: &Path, raw: &[u8]) -> Result<()> {
    if raw.len() > reel_workflow::MAX_CONTROL_BYTES {
        return Err(error(ErrorKind::Budget, "close-control-bytes"));
    }
    let mut file = exclusive(path)?;
    file.write_all(raw)
        .and_then(|_| file.sync_all())
        .map_err(|_| error(ErrorKind::NeedsRecovery, "close-control-write-preserve"))?;
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
