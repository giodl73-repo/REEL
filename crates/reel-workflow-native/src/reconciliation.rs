//! Separately pinned verification-only reconciliation of an ORIGINAL layered
//! ImportPending result. The producer runtime/profile and all job identities
//! remain unchanged; this is not a general runtime or provider substitution.
use crate::{Tool, closing, error, graph, handoff, hold_tool, io_error, response, staging};
use reel_workflow::{
    ErrorKind, Result, ReturnedResult, ToolCapability,
    handoff::{Journal, State},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    schema: String,
    journal: PathBuf,
    profile: handoff::Profile,
    original: reel_workflow::handoff::Ticket,
    result: ReturnedResult,
    returns: graph::PinnedFile,
    verification_runtime: Tool,
}
fn scope(ticket: &reel_workflow::handoff::Ticket, result: &ReturnedResult) -> Result<()> {
    reel_workflow::check_packet(&ticket.packet)?;
    reel_workflow::check_result(&ticket.packet, result)?;
    if ticket.packet.task.tool.as_ref().map(|t| t.capability)
        != Some(ToolCapability::LayeredComicCompose)
        || result.provenance.mode != reel_workflow::Mode::Tool
        || result.worker_receipt.is_none()
        || result.attempt_id != ticket.attempt_id
    {
        return Err(error(
            ErrorKind::Unsupported,
            "reconcile-layered-original-only",
        ));
    }
    Ok(())
}
pub fn prepare(
    journal: &Path,
    profile: handoff::Profile,
    returns: graph::PinnedFile,
) -> Result<Ticket> {
    let observed = handoff::observe(journal, &profile)?;
    if observed.state != State::ImportPending {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "reconcile-original-import-pending-required",
        ));
    }
    let result = observed
        .result
        .ok_or_else(|| error(ErrorKind::Missing, "reconcile-original-result"))?;
    scope(&observed.ticket, &result)?;
    let (raw, _held) = returns.read()?;
    let _: response::Returns = reel_workflow::decode(&raw)?;
    let ticket = Ticket {
        schema: "reel.workflow-original-layered-reconciliation.v1".into(),
        journal: journal.into(),
        profile,
        original: observed.ticket,
        result,
        returns,
        verification_runtime: closing::current_runtime()?,
    };
    reel_workflow::canonical_digest(&ticket)?;
    Ok(ticket)
}
impl Ticket {
    fn output(&self, path: &Path) -> Result<(reel_assets::jobs::WorkspaceDirectory, PathBuf)> {
        if !path.is_absolute()
            || path
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir))
        {
            return Err(error(ErrorKind::Invalid, "reconcile-private-output-path"));
        }
        let parent = reel_assets::jobs::open_workspace_directory(
            path.parent()
                .ok_or_else(|| error(ErrorKind::Invalid, "reconcile-output-parent"))?,
        )
        .map_err(|_| error(ErrorKind::Denied, "reconcile-output-parent"))?;
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
                return Err(error(
                    ErrorKind::Denied,
                    "reconcile-output-overlaps-control",
                ));
            }
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| staging::portable(n))
            .ok_or_else(|| error(ErrorKind::Invalid, "reconcile-output-name"))?;
        let path = parent.path().join(name);
        Ok((parent, path))
    }
    pub fn export(&self, path: &Path) -> Result<String> {
        let (_parent, path) = self.output(path)?;
        let raw = crate::advancement::raw(self)?;
        let mut file = exclusive(&path)?;
        file.write_all(&raw)
            .and_then(|_| file.sync_all())
            .map_err(|_| error(ErrorKind::NeedsRecovery, "reconcile-ticket-preserve"))?;
        drop(file);
        readback(&path, &raw)?;
        Ok(reel_workflow::digest(&raw))
    }
    pub fn execute_to(&self, path: &Path) -> Result<State> {
        scope(&self.original, &self.result)?;
        if self.schema != "reel.workflow-original-layered-reconciliation.v1" {
            return Err(error(ErrorKind::Invalid, "reconcile-ticket-schema"));
        }
        let _verifier = hold_tool(&self.verification_runtime)?;
        if self
            .verification_runtime
            .path
            .canonicalize()
            .map_err(io_error)?
            != std::env::current_exe()
                .map_err(io_error)?
                .canonicalize()
                .map_err(io_error)?
        {
            return Err(error(
                ErrorKind::Conflict,
                "reconcile-original-verifier-required",
            ));
        }
        let (raw, _returns_guard) = self.returns.read()?;
        let returns: response::Returns = reel_workflow::decode(&raw)?;
        let (_parent, path) = self.output(path)?;
        let mut file = exclusive(&path)?;
        let witness = crate::advancement::raw(self)?;
        let state = handoff::reconcile_layered_original(
            &self.journal,
            &self.profile,
            &self.original,
            &self.result,
            &returns,
            &witness,
        )?;
        let journal = Journal::open(&self.journal)?;
        let raw = crate::advancement::raw(&serde_json::json!({
            "schema":"reel.workflow-original-reconciliation-outcome.v1","ticket_sha256":reel_workflow::digest(&witness),
            "task_key":self.original.packet.task_key,"attempt_id":self.original.attempt_id,
            "admission_identity":self.profile.binding.admission.operation_identity()?,
            "original_work":self.profile.binding.admission.work,"original_activation":self.profile.binding.admission.activation,
            "original_producer_runtime":self.profile.runtime,"verification_runtime":self.verification_runtime,
            "acknowledgement":journal.acknowledgement(),"state":state,
            "worker_invoked":false,"provider_invoked":false,"original_result_unchanged":journal.result()==Some(&self.result),
            "profile_adopted":false,"scratch_retired":false,"selected":false
        }))?;
        file.write_all(&raw)
            .and_then(|_| file.sync_all())
            .map_err(|_| {
                error(
                    ErrorKind::NeedsRecovery,
                    "reconcile-outcome-preserve-original",
                )
            })?;
        drop(file);
        readback(&path, &raw)?;
        Ok(state)
    }
}
pub fn read_pinned(path: &Path, sha256: &str) -> Result<(Ticket, File)> {
    let (raw, file) = graph::PinnedFile {
        path: path.into(),
        sha256: sha256.into(),
    }
    .read()?;
    Ok((reel_workflow::decode(&raw)?, file))
}
fn exclusive(path: &Path) -> Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                error(ErrorKind::Conflict, "reconcile-existing-output")
            } else {
                io_error(e)
            }
        })
}
fn readback(path: &Path, raw: &[u8]) -> Result<()> {
    let (actual, _held) = staging::read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    if actual != raw {
        return Err(error(ErrorKind::Integrity, "reconcile-control-readback"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reel_workflow::*;
    use std::collections::BTreeMap;
    fn artifact(id: &str, raw: &[u8], media: &str) -> Artifact {
        Artifact {
            logical_id: id.into(),
            sha256: digest(raw),
            bytes: raw.len() as u64,
            media_type: media.into(),
        }
    }
    #[test]
    fn verification_only_scope_refuses_generic_intelligence_and_nonworker_results() {
        let plan = Plan {
            schema: PLAN_SCHEMA.into(),
            project: "scope-test".into(),
            source_commit: "a".repeat(40),
            task_snapshots: BTreeMap::new(),
            tasks: vec![Task {
                id: "task".into(),
                native_inputs: None,
                upstream_inputs: None,
                operation: Operation::TextProposal,
                tool: None,
                language: Some("en".into()),
                prompt: "Return a candidate".into(),
                inputs: vec![],
                depends_on: vec![],
                outputs: BTreeMap::from([(
                    "out.txt".into(),
                    OutputContract {
                        media_type: "text/plain".into(),
                        max_bytes: 8,
                        required: true,
                    },
                )]),
            }],
        };
        let packet = make_packet(&plan, "task", BTreeMap::new()).unwrap();
        let result = ReturnedResult {
            schema: RESULT_SCHEMA.into(),
            task_id: "task".into(),
            task_key: packet.task_key.clone(),
            attempt_id: "attempt-1".into(),
            artifacts: BTreeMap::from([(
                "out.txt".into(),
                artifact("output/out", b"x", "text/plain"),
            )]),
            provenance: Provenance {
                mode: Mode::Handoff,
                provider: None,
                model: None,
                executable: None,
            },
            worker_receipt: None,
        };
        let ticket = reel_workflow::handoff::Ticket {
            packet,
            attempt_id: "attempt-1".into(),
            native_binding: artifact("control/native", b"{}", "application/json"),
            lifecycle_tool: artifact("tool/old", b"old", "application/octet-stream"),
        };
        assert_eq!(
            scope(&ticket, &result).unwrap_err().code,
            "reconcile-layered-original-only"
        );
    }
}
