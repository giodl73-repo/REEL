//! Exact source freezing and explicit admission of one expanded task.
use crate::{NativeProfile, SnapshotInputs, admission, error, graph, io_error, staging, upstream};
use reel_workflow::{
    Artifact, ErrorKind, Plan, Result,
    expansion::{Expansion, Template},
};
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

pub fn read_pinned(path: &Path, sha256: &str) -> Result<(Frozen, std::fs::File)> {
    let (raw, held) = graph::PinnedFile {
        path: path.into(),
        sha256: sha256.into(),
    }
    .read()?;
    Ok((reel_workflow::decode(&raw)?, held))
}

fn exclusive(path: &Path) -> Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                error(ErrorKind::Conflict, "frozen-expansion-existing-output")
            } else {
                io_error(e)
            }
        })
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Frozen {
    pub schema: String,
    pub expansion: Expansion,
    pub bindings: graph::Bindings,
    pub source: NativeProfile,
    pub admission: admission::Profile,
    pub attempt_id: String,
    pub parents: Option<upstream::Parents>,
    pub preview: admission::Preview,
}
impl Frozen {
    pub fn check(&self, base: &Plan) -> Result<()> {
        reel_workflow::canonical_digest(self)?;
        let expansion = &self.expansion;
        if self.schema != "reel.workflow-frozen-expansion.v1"
            || expansion.schema != "reel.workflow-expansion.v1"
            || expansion.base_plan_sha256 != reel_workflow::canonical_digest(base)?
            || expansion.plan.schema != base.schema
            || expansion.plan.project != base.project
            || expansion.plan.source_commit != base.source_commit
            || expansion.plan.tasks.len() != base.tasks.len() + 1
            || expansion.plan.tasks[..base.tasks.len()] != base.tasks
            || expansion
                .plan
                .tasks
                .last()
                .is_none_or(|t| t != &expansion.packet.task)
            || self.bindings.schema != "reel.workflow-run-bindings.v1"
            || self
                .bindings
                .tasks
                .keys()
                .any(|id| !base.tasks.iter().any(|t| &t.id == id))
            || reel_workflow::make_packet(
                &expansion.plan,
                &expansion.packet.task.id,
                expansion.packet.dependencies.clone(),
            )? != expansion.packet
        {
            return Err(error(ErrorKind::Stale, "frozen-expansion-original-plan"));
        }
        let _snapshot = SnapshotInputs::open(&expansion.packet, &self.source.snapshot)?;
        match (
            &expansion.upstream,
            &self.source.snapshot.upstream,
            &self.parents,
        ) {
            (Some(bindings), Some(frozen), Some(parents)) => {
                if reel_workflow::canonical_digest(bindings)?
                    != reel_workflow::canonical_digest(&frozen.bindings)?
                    || reel_workflow::canonical_digest(frozen)?
                        != reel_workflow::canonical_digest(&upstream::Frozen::capture(
                            bindings,
                            &parents.borrowed()?,
                        ))?
                {
                    return Err(error(
                        ErrorKind::Conflict,
                        "frozen-expansion-origin-controls",
                    ));
                }
            }
            (None, None, None) => (),
            _ => {
                return Err(error(
                    ErrorKind::Conflict,
                    "frozen-expansion-origin-controls",
                ));
            }
        }
        Ok(())
    }
    /// Preview again under the same task/attempt/source/profile. Metadata is not
    /// live parent custody: composite preparation independently verifies it.
    pub fn accept(&self, base: &Plan) -> Result<admission::Receipt> {
        self.check(base)?;
        match (&self.expansion.upstream, &self.parents) {
            (Some(_), Some(parents)) => {
                let prepared = upstream::Admission::prepare(
                    &self.expansion.packet,
                    &self.source,
                    &self.admission,
                    &self.attempt_id,
                    parents.borrowed()?,
                )?;
                if prepared.preview()? != self.preview {
                    return Err(error(ErrorKind::Stale, "frozen-expansion-preview"));
                }
                prepared.accept()
            }
            (None, None) => {
                let prepared = admission::Prepared::open(
                    &self.expansion.packet,
                    &self.source,
                    &self.admission,
                    &self.attempt_id,
                )?;
                if prepared.preview()? != self.preview {
                    return Err(error(ErrorKind::Stale, "frozen-expansion-preview"));
                }
                prepared.accept()
            }
            _ => Err(error(ErrorKind::Conflict, "frozen-expansion-parent-set")),
        }
    }
    pub fn export(&self, output: &Path) -> Result<()> {
        let (_parent, output) = self.output_path(output)?;
        let raw = serde_json::to_vec(self)
            .map_err(|_| error(ErrorKind::Invalid, "frozen-expansion-output-json"))?;
        if raw.len() > reel_workflow::MAX_CONTROL_BYTES {
            return Err(error(ErrorKind::Budget, "frozen-expansion-output-bytes"));
        }
        let mut file = exclusive(&output)?;
        file.write_all(&raw)
            .and_then(|_| file.sync_all())
            .map_err(|_| error(ErrorKind::NeedsRecovery, "frozen-expansion-write-preserve"))?;
        drop(file);
        let (written, _guard) =
            staging::read_held(&output, reel_workflow::MAX_CONTROL_BYTES as u64)?;
        if written != raw {
            return Err(error(
                ErrorKind::Integrity,
                "frozen-expansion-output-readback",
            ));
        }
        Ok(())
    }
    pub fn accept_to(&self, base: &Plan, output: &Path) -> Result<admission::Receipt> {
        self.receipt_to(base, output, false)
    }
    pub fn recover_to(&self, base: &Plan, output: &Path) -> Result<admission::Receipt> {
        self.receipt_to(base, output, true)
    }
    fn receipt_to(&self, base: &Plan, output: &Path, recover: bool) -> Result<admission::Receipt> {
        self.check(base)?;
        let (_parent, output) = self.output_path(output)?;
        let mut file = exclusive(&output)?;
        let receipt = if recover {
            admission::Prepared::open(
                &self.expansion.packet,
                &self.source,
                &self.admission,
                &self.attempt_id,
            )?
            .recover()?
        } else {
            self.accept(base)?
        };
        let _claims = admission::Prepared::open(
            &self.expansion.packet,
            &self.source,
            &self.admission,
            &self.attempt_id,
        )?
        .reopen(&receipt)?;
        let raw = receipt.canonical_bytes()?;
        file.write_all(&raw)
            .and_then(|_| file.sync_all())
            .map_err(|_| {
                error(
                    ErrorKind::NeedsRecovery,
                    "frozen-expansion-receipt-preserve-original",
                )
            })?;
        drop(file);
        let (written, _held) =
            staging::read_held(&output, reel_workflow::MAX_CONTROL_BYTES as u64)?;
        if written != raw {
            return Err(error(
                ErrorKind::Integrity,
                "frozen-expansion-receipt-readback",
            ));
        }
        Ok(receipt)
    }
    fn output_path(
        &self,
        output: &Path,
    ) -> Result<(reel_assets::jobs::WorkspaceDirectory, std::path::PathBuf)> {
        if !output.is_absolute()
            || output
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir))
        {
            return Err(error(ErrorKind::Invalid, "frozen-expansion-output-path"));
        }
        let parent = reel_assets::jobs::open_workspace_directory(
            output
                .parent()
                .ok_or_else(|| error(ErrorKind::Invalid, "frozen-expansion-output-parent"))?,
        )
        .map_err(|_| error(ErrorKind::Denied, "frozen-expansion-output-namespace"))?;
        staging::check_parent(parent.path(), &self.source, &self.admission)?;
        if parent.path().starts_with(
            self.source
                .snapshot
                .repository
                .canonicalize()
                .map_err(io_error)?,
        ) {
            return Err(error(
                ErrorKind::Denied,
                "frozen-expansion-output-inside-source",
            ));
        }
        if let Some(parents) = &self.parents {
            for p in parents.parents.values() {
                staging::check_parent(parent.path(), &p.profile.source, &p.profile.admission)?;
                for root in [
                    &p.profile.source.snapshot.repository,
                    &p.profile.acknowledgements,
                ] {
                    if parent
                        .path()
                        .starts_with(root.canonicalize().map_err(io_error)?)
                    {
                        return Err(error(
                            ErrorKind::Denied,
                            "frozen-expansion-output-overlaps-parent",
                        ));
                    }
                }
            }
        }
        for binding in self.bindings.tasks.values() {
            if parent
                .path()
                .starts_with(binding.journal.canonicalize().map_err(io_error)?)
            {
                return Err(error(
                    ErrorKind::Denied,
                    "frozen-expansion-output-inside-journal",
                ));
            }
        }
        let name = output
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| staging::portable(s))
            .ok_or_else(|| error(ErrorKind::Invalid, "frozen-expansion-output-name"))?;
        let output = parent.path().join(name);
        Ok((parent, output))
    }
}

pub fn prepare(
    base: &Plan,
    bindings: &graph::Bindings,
    template: &Template,
    mut source: NativeProfile,
    admission: admission::Profile,
    attempt: &str,
) -> Result<Frozen> {
    if source.snapshot.upstream.is_some()
        || source.snapshot.schema != "reel.workflow-native-snapshot.v1"
        || source.snapshot.commit != base.source_commit
    {
        return Err(error(
            ErrorKind::Stale,
            "expansion-explicit-unfrozen-source",
        ));
    }
    let (mut expansion, parents) = graph::expansion_parents(base, bindings, template)?;
    if let (Some(bindings), Some(parents)) = (&expansion.upstream, &parents) {
        source.snapshot.schema = "reel.workflow-native-composite-snapshot.v1".into();
        source.snapshot.upstream = Some(upstream::Frozen::capture(bindings, &parents.borrowed()?));
    }
    let raw = source.snapshot.declaration_bytes()?;
    let id = expansion.packet.task.id.clone();
    expansion
        .plan
        .tasks
        .last_mut()
        .ok_or_else(|| error(ErrorKind::Missing, "expansion-new-task"))?
        .native_inputs = Some(Artifact {
        logical_id: format!("workflow/expansion/{id}/native-inputs"),
        sha256: reel_workflow::digest(&raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    });
    expansion.packet =
        reel_workflow::make_packet(&expansion.plan, &id, expansion.packet.dependencies.clone())?;
    let preview = match &parents {
        Some(parents) => upstream::Admission::prepare(
            &expansion.packet,
            &source,
            &admission,
            attempt,
            parents.borrowed()?,
        )?
        .preview()?,
        None => {
            admission::Prepared::open(&expansion.packet, &source, &admission, attempt)?.preview()?
        }
    };
    let binding_raw = serde_json::to_vec(bindings)
        .map_err(|_| error(ErrorKind::Invalid, "expansion-run-bindings-json"))?;
    let frozen = Frozen {
        schema: "reel.workflow-frozen-expansion.v1".into(),
        expansion,
        bindings: reel_workflow::decode(&binding_raw)?,
        source,
        admission,
        attempt_id: attempt.into(),
        parents,
        preview,
    };
    frozen.check(base)?;
    Ok(frozen)
}
