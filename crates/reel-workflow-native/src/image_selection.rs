//! Retained image candidate -> exact record/leaf/decision proposal -> owned ref.
//! Project authority is an explicit host capability, never a deserialized flag.
use crate::{
    NativeProfile, SnapshotInputs, SnapshotProfile, admission, error, git_error, hold_tool,
    retention, scope, selection,
};
use reel_workflow::{Artifact, ErrorKind, Operation, Packet, Result, ReturnedResult};
use serde::{Deserialize, Serialize};
use tig_assets::{native::Instance, resolve::AssetResolver};
use tig_core::Object;
use tig_git::{
    annex::identity::Key,
    source::{
        GitId, Limits, Repository,
        native::{self, annex::Catalogue},
    },
};

/// Only the producing project's authority implementation can interpret its
/// decision evidence. Exact-byte/subject checks below do not grant that authority.
/// The callback must refuse missing/expired/out-of-scope creative and rights
/// decisions. It receives caller-authored records with stable identity preserved.
pub trait Authority {
    fn authorize(&mut self, context: &DecisionContext<'_>) -> Result<()>;
}
pub struct DecisionContext<'a> {
    pub project: &'a str,
    pub old_subject: &'a Artifact,
    pub candidate: &'a Artifact,
    pub decision: &'a Artifact,
    pub decision_bytes: &'a [u8],
    pub before: &'a Instance,
    pub after: &'a Instance,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub schema: String,
    pub old_subject: Artifact,
    pub output_slot: String,
    /// Must be an explicitly enumerated original native record path, not an alias.
    pub record_path: String,
    pub old_record: Vec<u8>,
    /// Existing regular control path. The proposal replaces its expected bytes
    /// with the exact decision bytes, retaining evidence in the selected commit.
    /// New-path registration is not inferred from the SDK's replacement API.
    pub decision_path: String,
    pub transaction: selection::Request,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub packet: Packet,
    pub source: NativeProfile,
    pub admission: admission::Profile,
    pub receipt: admission::Receipt,
    pub result: ReturnedResult,
    pub acknowledgement: retention::Acknowledgement,
    pub spec: Spec,
}
#[derive(Serialize)]
pub struct Inspection {
    pub old_subject: Artifact,
    pub candidate: Artifact,
    pub record_before: Object,
    pub record_after: Object,
    pub status_cleared: bool,
    pub native_retention_verified: bool,
    pub selection_applied: bool,
}
#[derive(Serialize)]
pub struct Outcome {
    pub selection: selection::Outcome,
    pub native_retention_verified: bool,
    pub proposed_native_binding_verified: bool,
    pub approvals_transferred: bool,
    pub consumers_rebuilt: bool,
}
struct Checked<'a> {
    candidate: &'a Artifact,
    before: tig_assets::native::Parsed<Instance>,
    after: tig_assets::native::Parsed<Instance>,
    decision: &'a [u8],
}
fn object(artifact: &Artifact) -> Result<Object> {
    Ok(Object {
        sha256: tig_core::Digest::parse(&artifact.sha256)
            .map_err(|_| error(ErrorKind::Invalid, "image-selection-digest"))?,
        bytes: artifact.bytes,
    })
}
fn controls<'a>(
    packet: &Packet,
    profile: &SnapshotProfile,
    result: &'a ReturnedResult,
    spec: &'a Spec,
) -> Result<Checked<'a>> {
    reel_workflow::check_result(packet, result)?;
    let tx = &spec.transaction;
    let decision_name = spec
        .decision_path
        .strip_prefix(".reel-workflow/decisions/")
        .and_then(|name| name.strip_suffix(".json"));
    if spec.schema != "reel.workflow-image-selection.v1"
        || packet.task.operation != Operation::ImageCandidate
        || !packet.task.inputs.contains(&spec.old_subject)
        || !spec.old_subject.media_type.starts_with("image/")
        || tx.base != packet.source_commit
        || tx.base != profile.commit
        || tx.repository != profile.repository
        || serde_json::to_vec(&tx.git).ok() != serde_json::to_vec(&profile.git).ok()
        || !profile.records.contains(&spec.record_path)
        || profile.input_streams.get(&spec.old_subject.logical_id) != Some(&None)
        || !decision_name.is_some_and(|name| {
            !name.is_empty()
                && name.len() <= 96
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    {
        return Err(error(
            ErrorKind::Conflict,
            "image-selection-original-subject",
        ));
    }
    let candidate = result
        .artifacts
        .get(&spec.output_slot)
        .ok_or_else(|| error(ErrorKind::Missing, "image-selection-output"))?;
    if !matches!(
        candidate.media_type.as_str(),
        "image/png" | "image/jpeg" | "image/webp"
    ) || object(candidate)? == object(&spec.old_subject)?
    {
        return Err(error(ErrorKind::Invalid, "image-selection-candidate"));
    }
    let binding = profile
        .bindings
        .get(&spec.old_subject.logical_id)
        .ok_or_else(|| error(ErrorKind::Missing, "image-selection-original-binding"))?;
    let changes = &tx.changes;
    let record = changes
        .iter()
        .find(|c| c.path == spec.record_path)
        .ok_or_else(|| error(ErrorKind::Missing, "image-selection-record-change"))?;
    let leaf = changes
        .iter()
        .find(|c| c.path == binding.selected.as_str())
        .ok_or_else(|| error(ErrorKind::Missing, "image-selection-leaf-change"))?;
    let decision = changes
        .iter()
        .find(|c| c.path == spec.decision_path)
        .ok_or_else(|| error(ErrorKind::Missing, "image-selection-decision-change"))?;
    if changes.len() != 3
        || spec.record_path == binding.selected.as_str()
        || spec.decision_path == spec.record_path
        || spec.decision_path == binding.selected.as_str()
        || Object::from_bytes(&decision.bytes) != object(&tx.decision)?
        || leaf.expected
            != Object::from_bytes(&Key::from_object(&object(&spec.old_subject)?).unlocked_pointer())
        || leaf.bytes != Key::from_object(&object(candidate)?).unlocked_pointer()
    {
        return Err(error(
            ErrorKind::Conflict,
            "image-selection-exact-change-set",
        ));
    }
    let before = Instance::parse(&spec.old_record)
        .map_err(|_| error(ErrorKind::Invalid, "image-selection-old-record"))?;
    let after = Instance::parse(&record.bytes)
        .map_err(|_| error(ErrorKind::Invalid, "image-selection-new-record"))?;
    let old_pin = before
        .value()
        .selected
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Missing, "image-selection-old-selected"))?;
    let new_pin = after
        .value()
        .selected
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Missing, "image-selection-new-selected"))?;
    let mut unchanged = after.value().clone();
    unchanged.selected = before.value().selected.clone();
    unchanged.status = before.value().status.clone();
    if record.expected != *before.object()
        || before.value().id != spec.old_subject.logical_id
        || old_pin.object() != object(&spec.old_subject)?
        || new_pin.object() != object(candidate)?
        || new_pin.id != old_pin.id
        || new_pin.media_type.as_deref() != Some(&candidate.media_type)
        || !after.value().status.is_empty()
        || unchanged != *before.value()
    {
        return Err(error(
            ErrorKind::Conflict,
            "image-selection-record-identity-or-approval",
        ));
    }
    Ok(Checked {
        candidate,
        before,
        after,
        decision: &decision.bytes,
    })
}
fn binding_at(
    profile: &SnapshotProfile,
    spec: &Spec,
    commit: GitId,
    record: &Object,
    selected: &Object,
) -> Result<()> {
    let _tool = hold_tool(&profile.git)?;
    let repository = Repository::new(
        profile.git.path.clone(),
        profile.repository.clone(),
        Limits::default(),
    )
    .map_err(git_error)?;
    let snapshot = repository.commit(commit).map_err(git_error)?;
    let paths = profile
        .bindings
        .values()
        .flat_map(|b| {
            std::iter::once(b.selected.as_str().to_owned())
                .chain(b.streams.values().map(|p| p.as_str().to_owned()))
        })
        .collect();
    let bound = repository
        .native_annex(
            &snapshot,
            scope(profile.records.clone())?,
            scope(profile.definitions.clone())?,
            &Catalogue {
                scope: scope(paths)?,
                bindings: profile.bindings.clone(),
            },
            native::Limits {
                documents: 512,
                bytes: 16 * 1024 * 1024,
                millis: 30_000,
            },
        )
        .map_err(git_error)?;
    let resolved = bound
        .assets
        .resolve_id(&spec.old_subject.logical_id)
        .map_err(|_| error(ErrorKind::Missing, "image-selection-resolved-record"))?;
    if &resolved.reference().object != record
        || resolved
            .selected()
            .map_err(|_| error(ErrorKind::Missing, "image-selection-resolved-selected"))?
            != selected
    {
        return Err(error(ErrorKind::Conflict, "image-selection-record-binding"));
    }
    Ok(())
}
/// Metadata-only inspection: no provider, payload read, protection, proposal or
/// selection. This output cannot be deserialized into an apply capability.
pub fn inspect(
    packet: &Packet,
    profile: &SnapshotProfile,
    result: &ReturnedResult,
    spec: &Spec,
) -> Result<Inspection> {
    let checked = controls(packet, profile, result, spec)?;
    let _snapshot = SnapshotInputs::open(packet, profile)?;
    binding_at(
        profile,
        spec,
        GitId::parse(&profile.commit).map_err(git_error)?,
        checked.before.object(),
        &object(&spec.old_subject)?,
    )?;
    Ok(Inspection {
        old_subject: spec.old_subject.clone(),
        candidate: checked.candidate.clone(),
        record_before: checked.before.object().clone(),
        record_after: checked.after.object().clone(),
        status_cleared: true,
        native_retention_verified: false,
        selection_applied: false,
    })
}
/// Rechecks live retained custody and project authority on every apply/recovery.
/// The decision bytes, record and exact bound leaf share the proposal commit.
pub fn run(request: &Request, authority: &mut impl Authority, apply: bool) -> Result<Outcome> {
    inspect(
        &request.packet,
        &request.source.snapshot,
        &request.result,
        &request.spec,
    )?;
    let checked = controls(
        &request.packet,
        &request.source.snapshot,
        &request.result,
        &request.spec,
    )?;
    authority.authorize(&DecisionContext {
        project: &request.packet.project,
        old_subject: &request.spec.old_subject,
        candidate: checked.candidate,
        decision: &request.spec.transaction.decision,
        decision_bytes: checked.decision,
        before: checked.before.value(),
        after: checked.after.value(),
    })?;
    let _retained = retention::verify(
        &request.packet,
        &request.source,
        &request.admission,
        &request.receipt,
        &request.result,
        &request.acknowledgement,
    )?;
    let selection = selection::run_checked(&request.spec.transaction, apply, |commit| {
        binding_at(
            &request.source.snapshot,
            &request.spec,
            commit.clone(),
            checked.after.object(),
            &object(checked.candidate)?,
        )
    })?;
    Ok(Outcome {
        selection,
        native_retention_verified: true,
        proposed_native_binding_verified: apply,
        approvals_transferred: false,
        consumers_rebuilt: false,
    })
}
