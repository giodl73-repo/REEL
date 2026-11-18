//! Resumable editable comic packet export from verified native retained results.
//! Reuses owned staging/capacity transactions; never releases claims or scratch.
use super::*;
use crate::retention::{self, Acknowledgement, RetainedGuard};
use reel_workflow::ReturnedResult;

/// Private control: pin these exact bytes externally before materializing them.
/// Neither deserializing this ticket nor a local completion marker proves custody.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    schema: String,
    profile: Profile,
    reservation: Reservation,
    admission: Receipt,
    result: ReturnedResult,
    retained: Acknowledgement,
    owner: Owner,
}
impl Ticket {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        json(self)
    }
    /// Owned output locator only. Opening or trusting these bytes requires export.
    pub fn packet_directory(&self) -> PathBuf {
        self.profile.parent.join(&self.owner.name).join("packet")
    }
}
fn parent(profile: &Profile, source: &NativeProfile, admission: &admission::Profile) -> Result<()> {
    check_parent(&profile.parent, source, admission)?;
    let repository = source
        .snapshot
        .repository
        .canonicalize()
        .map_err(io_error)?;
    if profile
        .parent
        .canonicalize()
        .map_err(io_error)?
        .starts_with(repository)
    {
        return Err(error(
            ErrorKind::Denied,
            "comic-export-parent-source-repository",
        ));
    }
    Ok(())
}
fn plan(
    packet: &Packet,
    source: &NativeProfile,
    admission: &Receipt,
    result: &ReturnedResult,
    retained: &Acknowledgement,
    proof: &RetainedGuard,
) -> Result<Plan> {
    let mut controls: BTreeMap<String, Vec<u8>> = BTreeMap::from([
        ("packet.json".into(), json(packet)?),
        ("admission.json".into(), admission.canonical_bytes()?),
        ("result.json".into(), json(result)?),
        ("retained.json".into(), retained.canonical_bytes()?),
        (
            "native-inputs.json".into(),
            source.snapshot.declaration_bytes()?,
        ),
    ]);
    let operation = operation(admission, retained)?;
    for (name, raw) in proof.comic_files()? {
        controls.insert(format!("packet/{name}"), raw.clone());
    }
    let files = controls
        .iter()
        .map(|(name, raw)| {
            (
                name.clone(),
                Entry {
                    object: Object::from_bytes(raw),
                    input: None,
                },
            )
        })
        .collect();
    check_entries(&files)?;
    Ok(Plan {
        manifest: Manifest {
            schema: "reel.workflow-comic-export-manifest.v1".into(),
            operation,
            files,
        },
        controls,
    })
}
fn operation(admission: &Receipt, retained: &Acknowledgement) -> Result<String> {
    // Checkpoint witnesses may advance when the same completed native work is
    // rediscovered. Keep one workspace identity; its exact original controls
    // remain pinned by the ticket/intent and must not be silently replaced.
    Ok(reel_workflow::digest(&json(&serde_json::json!({
        "schema":"reel.workflow-comic-export-operation.v1",
        "admission":admission.operation_identity()?, "retained_work":retained.work
    }))?))
}
/// Verifies native retained ownership/source/output bytes before admitting a
/// conservative capacity reservation and fresh owned workspace. Does not copy
/// payload files yet. Persist and pin the returned ticket before `export`.
#[allow(clippy::too_many_arguments)]
pub fn prepare(
    packet: &Packet,
    source: &NativeProfile,
    admission: &admission::Profile,
    receipt: &Receipt,
    result: &ReturnedResult,
    retained: &Acknowledgement,
    profile: &Profile,
) -> Result<Ticket> {
    parent(profile, source, admission)?;
    configuration(profile, &admission.scope.coordination_root)?;
    let proof = retention::capture_comic(packet, source, admission, receipt, result, retained)?;
    let plan = plan(packet, source, receipt, result, retained, &proof)?;
    let (profile, reservation, owner) =
        prepare_owned(profile, &admission.scope.coordination_root, &plan)?;
    Ok(Ticket {
        schema: "reel.workflow-comic-export-ticket.v1".into(),
        profile,
        reservation,
        admission: receipt.clone(),
        result: result.clone(),
        retained: retained.clone(),
        owner,
    })
}
/// Reopens the exact ticket, preserved capacity reservation and owned namespace,
/// then re-verifies actual native retained proof even after local completion.
/// No scratch fallback is permitted when source/history bytes are unavailable.
pub fn export(
    packet: &Packet,
    source: &NativeProfile,
    admission: &admission::Profile,
    ticket_bytes: &[u8],
    expected_sha256: &str,
) -> Result<Summary> {
    if reel_workflow::digest(ticket_bytes) != expected_sha256 {
        return Err(error(ErrorKind::Integrity, "comic-export-ticket-pin"));
    }
    let ticket: Ticket = reel_workflow::decode(ticket_bytes)?;
    if ticket.schema != "reel.workflow-comic-export-ticket.v1" {
        return Err(error(ErrorKind::Invalid, "comic-export-ticket-schema"));
    }
    parent(&ticket.profile, source, admission)?;
    let config = configuration(&ticket.profile, &admission.scope.coordination_root)?;
    let proof = retention::capture_comic(
        packet,
        source,
        admission,
        &ticket.admission,
        &ticket.result,
        &ticket.retained,
    )?;
    let plan = plan(
        packet,
        source,
        &ticket.admission,
        &ticket.result,
        &ticket.retained,
        &proof,
    )?;
    checked_budget(&ticket.profile, &plan)?;
    let mut view = open_owner(&ticket.profile, &ticket.owner, &plan)?;
    let _reservation = capacity::resume_preserved_on(
        &config,
        &ticket.profile.parent,
        &ticket.reservation.id,
        &ticket.reservation.volume,
        ticket.profile.maximum_bytes,
    )
    .map_err(scoped_error)?;
    // Retained actor fences, snapshot/tool pins and native reads stay live until
    // after immutable publication and the completion file's final verification.
    materialize(
        &mut view,
        &ticket.profile,
        &plan,
        &Buffers(BTreeMap::new()),
        expected_sha256,
        &mut |_| Ok(()),
    )
}
pub fn export_file(
    packet: &Packet,
    source: &NativeProfile,
    admission: &admission::Profile,
    path: &Path,
    expected_sha256: &str,
) -> Result<Summary> {
    let (raw, _held) = read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    export(packet, source, admission, &raw, expected_sha256)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rediscovered_checkpoint_witnesses_do_not_allocate_a_new_export_identity() {
        // Serialization/identity contract only. These descriptors grant no guard
        // and the fixture never opens or mutates any native store or workspace.
        let hash = "1".repeat(64);
        let object = serde_json::json!({"sha256":hash,"bytes":10});
        let work = serde_json::json!({"index":hash,"source":"work-fixture","root":object});
        let checkpoint = serde_json::json!({"genesis":hash,"sequence":1,"hash":hash});
        let metadata = serde_json::json!({"identity":hash,"generation":1,"nodes":1,"bytes":10});
        let mut admission: Receipt = serde_json::from_value(serde_json::json!({
            "schema":"reel.workflow-native-admission-receipt.v1","task_key":hash,"attempt_id":"attempt-1",
            "scope":{"coordination_root":"fixture","actor":"job","owner":"owner","operation":"operation","policy":hash,"store":hash},
            "input":{"root":object,"context":hash},"source_archive":object,"candidate":object,
            "activation":object,"work":work,"index_checkpoint":checkpoint,"metadata_checkpoint":metadata
        })).unwrap();
        let mut retained: Acknowledgement = serde_json::from_value(serde_json::json!({
            "schema":"reel.workflow-retained-acknowledgement.v1",
            "candidate":{"schema":"reel.workflow-response-candidate.v1","task_key":hash,"attempt_id":"attempt-1","registration":object,"work":work,"store":hash},
            "target":{"schema":"reel.workflow-retained-target.v1","actor":"retained","owner":"owner"},
            "evidence":object,"work":work,"index_checkpoint":checkpoint,"metadata_checkpoint":metadata
        })).unwrap();
        let original = operation(&admission, &retained).unwrap();
        let original_control = retained.canonical_bytes().unwrap();
        let next_checkpoint = serde_json::json!({"genesis":hash,"sequence":2,"hash":hash});
        let next_metadata =
            serde_json::json!({"identity":hash,"generation":2,"nodes":2,"bytes":20});
        admission.index_checkpoint = serde_json::from_value(next_checkpoint.clone()).unwrap();
        admission.metadata_checkpoint = serde_json::from_value(next_metadata.clone()).unwrap();
        retained.index_checkpoint = serde_json::from_value(next_checkpoint).unwrap();
        retained.metadata_checkpoint = serde_json::from_value(next_metadata).unwrap();
        assert_eq!(original, operation(&admission, &retained).unwrap());
        assert_ne!(original_control, retained.canonical_bytes().unwrap());
        retained.work = serde_json::from_value(
            serde_json::json!({"index":hash,"source":"work-another","root":object}),
        )
        .unwrap();
        assert_ne!(original, operation(&admission, &retained).unwrap());
    }
}
