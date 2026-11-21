//! Native lifecycle port for the existing append-only workflow journal.
//! Small private controls only; native assets remain in the existing store.
use crate::in_phase as phase;
use crate::{
    NativeProfile, Tool,
    admission::{self, Prepared, Receipt},
    error, hold_tool, io_error, response, retention,
};
use reel_workflow::{
    Artifact, ErrorKind, Packet, Result, ReturnedResult,
    handoff::{Journal, Lifecycle, ProtectedJob, State, Ticket},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub schema: String,
    pub admission: Receipt,
    pub target: retention::Target,
}
impl Binding {
    pub fn artifact(&self) -> Result<Artifact> {
        if self.schema != "reel.workflow-native-handoff-binding.v1" {
            return Err(error(ErrorKind::Invalid, "handoff-binding-schema"));
        }
        let raw = admission::encode(self)?;
        Ok(Artifact {
            logical_id: format!(
                "workflow/handoff/{}/{}",
                self.admission.task_key, self.admission.attempt_id
            ),
            sha256: reel_workflow::digest(&raw),
            bytes: raw.len() as u64,
            media_type: "application/json".into(),
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub source: NativeProfile,
    pub admission: admission::Profile,
    pub binding: Binding,
    /// Must pin the actual current executable, not a tool merely named in JSON.
    pub runtime: Tool,
    /// Existing private directory for bounded immutable acknowledgement controls.
    pub acknowledgements: PathBuf,
}
pub(crate) fn runtime_artifact(tool: &Tool) -> Artifact {
    Artifact {
        logical_id: "workflow/tool/native-lifecycle".into(),
        sha256: tool.sha256.clone(),
        bytes: tool.bytes,
        media_type: "application/octet-stream".into(),
    }
}
pub(crate) fn runtime(tool: &Tool) -> Result<File> {
    let held = hold_tool(tool)?;
    if tool.path.canonicalize().map_err(io_error)?
        != std::env::current_exe()
            .map_err(io_error)?
            .canonicalize()
            .map_err(io_error)?
    {
        return Err(error(ErrorKind::Conflict, "handoff-current-executable-pin"));
    }
    Ok(held)
}
fn ack_path(profile: &Profile) -> PathBuf {
    profile.acknowledgements.join(format!(
        "retained-{}-{}.json",
        profile.binding.admission.task_key, profile.binding.admission.attempt_id
    ))
}
fn ack_read(profile: &Profile) -> Result<(retention::Acknowledgement, File)> {
    let (raw, held) = crate::staging::read_held(&ack_path(profile), 65536)?;
    let ack: retention::Acknowledgement = reel_workflow::decode(&raw)?;
    if ack.canonical_bytes()? != raw {
        return Err(error(
            ErrorKind::Integrity,
            "handoff-canonical-acknowledgement",
        ));
    }
    Ok((ack, held))
}

struct Adapter<'a> {
    profile: &'a Profile,
    result: Option<ReturnedResult>,
    acknowledgement: Option<Artifact>,
    returns: Option<&'a response::Returns>,
}
struct Guard<'a, 'p> {
    adapter: &'a mut Adapter<'p>,
    _runtime: File,
    _claims: Option<admission::ClaimsGuard>,
    _recovery: Option<admission::RecoveryGuard>,
    _retained: Option<retention::RetainedGuard>,
    _ack: Option<File>,
}
impl Guard<'_, '_> {
    fn clear(&mut self) {
        self._claims = None;
        self._recovery = None;
        self._retained = None;
    }
    fn result(&self, ticket: &Ticket, result: &ReturnedResult) -> Result<()> {
        reel_workflow::check_result(&ticket.packet, result)?;
        if self.adapter.result.as_ref() != Some(result) || result.attempt_id != ticket.attempt_id {
            return Err(error(ErrorKind::Conflict, "handoff-original-result"));
        }
        Ok(())
    }
    fn finish(&mut self, ticket: &Ticket, result: &ReturnedResult) -> Result<Artifact> {
        self.result(ticket, result)?;
        // Release only cooperative fences, never durable claims. Each effect
        // entrypoint reacquires/rechecks the original generation before effects.
        self.clear();
        let p = self.adapter.profile;
        let r = &p.binding.admission;
        let prepared = Prepared::open(&ticket.packet, &p.source, &p.admission, &r.attempt_id)?;
        if !prepared
            .has_retention_work(r, result, &p.binding.target)
            .map_err(|e| phase(e, "handoff-phase"))?
        {
            let returns = self.adapter.returns.ok_or_else(|| {
                error(ErrorKind::Missing, "handoff-original-return-files-required")
            })?;
            response::ingest(&ticket.packet, &p.source, &p.admission, r, result, returns)
                .map_err(|e| phase(e, "handoff-ingest"))?;
        }
        let fresh = retention::retain(
            &ticket.packet,
            &p.source,
            &p.admission,
            r,
            result,
            &p.binding.target,
        )
        .map_err(|e| phase(e, "handoff-retain"))?;
        let path = ack_path(p);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(&fresh.canonical_bytes()?)
                    .and_then(|_| file.sync_all())
                    .map_err(|_| error(ErrorKind::NeedsRecovery, "handoff-ack-write-preserve"))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io_error(e)),
        }
        let (ack, held) = ack_read(p)?;
        if ack.candidate != fresh.candidate
            || ack.target != fresh.target
            || ack.evidence != fresh.evidence
            || ack.work != fresh.work
        {
            return Err(error(
                ErrorKind::Conflict,
                "handoff-original-acknowledgement",
            ));
        }
        self._retained = Some(retention::verify(
            &ticket.packet,
            &p.source,
            &p.admission,
            r,
            result,
            &ack,
        )?);
        self._ack = Some(held);
        ack.artifact()
    }
}
impl ProtectedJob for Guard<'_, '_> {
    fn register(&mut self, ticket: &Ticket, result: &ReturnedResult) -> Result<Artifact> {
        self.finish(ticket, result)
    }
    fn reconcile(&mut self, ticket: &Ticket, result: &ReturnedResult) -> Result<Artifact> {
        self.finish(ticket, result)
    }
    fn verify_retained(
        &mut self,
        ticket: &Ticket,
        result: &ReturnedResult,
        expected: &Artifact,
    ) -> Result<()> {
        self.result(ticket, result)?;
        let p = self.adapter.profile;
        let (ack, held) = ack_read(p)?;
        if ack.artifact()? != *expected || ack.target != p.binding.target {
            return Err(error(ErrorKind::Conflict, "handoff-pinned-acknowledgement"));
        }
        self.clear();
        self._retained = Some(retention::verify(
            &ticket.packet,
            &p.source,
            &p.admission,
            &p.binding.admission,
            result,
            &ack,
        )?);
        self._ack = Some(held);
        Ok(())
    }
}
impl<'p> Lifecycle for Adapter<'p> {
    type Guard<'a>
        = Guard<'a, 'p>
    where
        Self: 'a;
    fn reopen<'a>(&'a mut self, ticket: &Ticket) -> Result<Self::Guard<'a>> {
        let p = self.profile;
        if ticket.native_binding != p.binding.artifact()?
            || ticket.lifecycle_tool != runtime_artifact(&p.runtime)
            || ticket.attempt_id != p.binding.admission.attempt_id
        {
            return Err(error(ErrorKind::Conflict, "handoff-ticket-native-binding"));
        }
        let runtime = runtime(&p.runtime)?;
        let prepared = Prepared::open(&ticket.packet, &p.source, &p.admission, &ticket.attempt_id)?;
        prepared.validate_retained_target(&p.binding.admission, &p.binding.target)?;
        let mut claims = None;
        let mut recovery = None;
        let mut retained = None;
        let mut held_ack = None;
        if let Some(result) = &self.result {
            reel_workflow::check_result(&ticket.packet, result)?;
            if result.provenance.mode == reel_workflow::Mode::Tool {
                return Err(error(
                    ErrorKind::Unsupported,
                    "handoff-tool-receipt-adapter-required",
                ));
            }
            prepared
                .has_retention_work(&p.binding.admission, result, &p.binding.target)
                .map_err(|e| phase(e, "handoff-reopen-phase"))?;
            if let Some(expected) = &self.acknowledgement {
                let (ack, held) = ack_read(p)?;
                if ack.artifact()? != *expected || ack.target != p.binding.target {
                    return Err(error(ErrorKind::Conflict, "handoff-pinned-acknowledgement"));
                }
                retained = Some(retention::verify(
                    &ticket.packet,
                    &p.source,
                    &p.admission,
                    &p.binding.admission,
                    result,
                    &ack,
                )?);
                held_ack = Some(held);
            } else if let Some(ack) = prepared
                .completed_retention(&p.binding.admission, result, &p.binding.target)
                .map_err(|e| phase(e, "handoff-reopen-retained"))?
            {
                retained = Some(retention::verify(
                    &ticket.packet,
                    &p.source,
                    &p.admission,
                    &p.binding.admission,
                    result,
                    &ack,
                )?);
            } else {
                recovery = Some(
                    prepared
                        .recovery_guard(&p.binding.admission)
                        .map_err(|e| phase(e, "handoff-reopen-recovery"))?,
                );
            }
        } else {
            claims = Some(prepared.reopen(&p.binding.admission)?);
        }
        Ok(Guard {
            adapter: self,
            _runtime: runtime,
            _claims: claims,
            _recovery: recovery,
            _retained: retained,
            _ack: held_ack,
        })
    }
}

struct Controls {
    _directories: Vec<reel_assets::jobs::WorkspaceDirectory>,
    _writers: Vec<reel_assets::admission::MetadataLock>,
}
fn controls(path: &Path, profile: &Profile) -> Result<Controls> {
    if !cfg!(windows) {
        return Err(error(
            ErrorKind::Unsupported,
            "handoff-windows-qualified-scope",
        ));
    }
    if profile.schema != "reel.workflow-native-handoff.v1"
        || !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        || !profile.acknowledgements.is_absolute()
    {
        return Err(error(
            ErrorKind::Invalid,
            "handoff-explicit-control-profile",
        ));
    }
    let receipt = &profile.binding.admission;
    if receipt.task_key.len() != 64
        || !receipt
            .task_key
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || receipt.attempt_id.len() > 128
        || !crate::staging::portable(&receipt.attempt_id)
        || receipt.attempt_id.contains('/')
    {
        return Err(error(ErrorKind::Invalid, "handoff-control-identity"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| error(ErrorKind::Invalid, "handoff-journal-parent"))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| error(ErrorKind::Invalid, "handoff-journal-name"))?;
    if !crate::staging::portable(name) {
        return Err(error(ErrorKind::Invalid, "handoff-journal-name"));
    }
    let mut directories = vec![
        reel_assets::jobs::open_workspace_directory(parent)
            .map_err(|_| error(ErrorKind::Denied, "handoff-parent-namespace"))?,
        reel_assets::jobs::open_workspace_directory(&profile.acknowledgements)
            .map_err(|_| error(ErrorKind::Denied, "handoff-ack-namespace"))?,
    ];
    let journal = parent.canonicalize().map_err(io_error)?.join(name);
    let ack = profile.acknowledgements.canonicalize().map_err(io_error)?;
    if journal.starts_with(&ack) || ack.starts_with(&journal) {
        return Err(error(ErrorKind::Denied, "handoff-control-root-overlap"));
    }
    for root in [
        &profile.admission.index_root,
        &profile.admission.metadata_root,
        &profile.admission.scope.coordination_root,
        &profile.source.source.pool.repository,
    ] {
        let root = root.canonicalize().unwrap_or_else(|_| root.clone());
        if [&journal, &ack]
            .iter()
            .any(|p| p.starts_with(&root) || root.starts_with(p))
        {
            return Err(error(ErrorKind::Denied, "handoff-managed-root-overlap"));
        }
    }
    let writer = reel_assets::admission::lock(
        &profile
            .admission
            .scope
            .coordination_root
            .join("workflow-handoff-controls")
            .join(reel_workflow::digest(journal.to_string_lossy().as_bytes())),
    )
    .map_err(|_| error(ErrorKind::Busy, "handoff-writer-busy"))?;
    let ack_writer = reel_assets::admission::lock(
        &profile
            .admission
            .scope
            .coordination_root
            .join("workflow-handoff-acks")
            .join(reel_workflow::digest(ack.to_string_lossy().as_bytes())),
    )
    .map_err(|_| error(ErrorKind::Busy, "handoff-ack-writer-busy"))?;
    let mut count = 0usize;
    for entry in std::fs::read_dir(&ack).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        count += 1;
        let name = entry.file_name();
        let name = name.to_str().unwrap_or("");
        if count > 128
            || !name.starts_with("retained-")
            || !name.ends_with(".json")
            || !crate::staging::portable(name)
        {
            return Err(error(ErrorKind::Budget, "handoff-ack-directory-bounds"));
        }
        let (_raw, _held) = crate::staging::read_held(&entry.path(), 65536)?;
    }
    if path.exists() {
        directories.push(
            reel_assets::jobs::open_workspace_directory(path)
                .map_err(|_| error(ErrorKind::Denied, "handoff-journal-namespace"))?,
        );
    }
    if count == 128 && !ack_path(profile).exists() {
        return Err(error(ErrorKind::Budget, "handoff-ack-directory-full"));
    }
    Ok(Controls {
        _directories: directories,
        _writers: vec![writer, ack_writer],
    })
}

pub fn create(path: &Path, packet: &Packet, profile: &Profile) -> Result<State> {
    let _controls = controls(path, profile)?;
    let ticket = Ticket {
        packet: packet.clone(),
        attempt_id: profile.binding.admission.attempt_id.clone(),
        native_binding: profile.binding.artifact()?,
        lifecycle_tool: runtime_artifact(&profile.runtime),
    };
    let mut adapter = Adapter {
        profile,
        result: None,
        acknowledgement: None,
        returns: None,
    };
    Ok(Journal::create(path, ticket, &mut adapter)?.state())
}
pub fn import(
    path: &Path,
    profile: &Profile,
    result: ReturnedResult,
    returns: &response::Returns,
) -> Result<State> {
    let _controls = controls(path, profile)?;
    let mut journal = Journal::open(path)?;
    let mut adapter = Adapter {
        profile,
        result: Some(result.clone()),
        acknowledgement: None,
        returns: Some(returns),
    };
    // Validate immutable profile/runtime binding before recording import intent.
    {
        let _guard = adapter.reopen(journal.ticket())?;
    }
    journal.import(result, &mut adapter)?;
    Ok(journal.state())
}
pub fn resume(
    path: &Path,
    profile: &Profile,
    returns: Option<&response::Returns>,
) -> Result<State> {
    let _controls = controls(path, profile)?;
    let mut journal = Journal::open(path)?;
    let mut adapter = Adapter {
        profile,
        result: journal.result().cloned(),
        acknowledgement: journal.acknowledgement().cloned(),
        returns,
    };
    journal.resume(&mut adapter)?;
    Ok(journal.state())
}
