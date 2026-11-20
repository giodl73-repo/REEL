//! Exact reviewed producer metadata. Native callers prove the retained source
//! of this request independently; these descriptors never confer byte custody.
use super::*;

pub type Production = crate::repair::ResultWitness;

pub(super) fn check_copy(witness: &Production, copy: &Copy) -> Result<()> {
    check_result(&witness.packet, &witness.result)?;
    let packet = &witness.packet;
    let tool = packet.task.tool.as_ref().ok_or(Error {
        kind: ErrorKind::Missing,
        code: "review-production-tool".into(),
    })?;
    if packet.project != copy.project
        || packet.source_commit != copy.source_commit
        || packet.task.language.as_ref() != Some(&copy.language)
        || tool.publication != copy.publication
        || witness.result.provenance.mode != Mode::Tool
        || witness.result.provenance.executable.as_ref() != Some(&tool.executable)
        || witness.result.worker_receipt.is_none()
        || !matches!(
            (&copy.format, tool.capability),
            (
                Format::Comic { .. },
                ToolCapability::ComicRender
                    | ToolCapability::ComicIssueAssembly
                    | ToolCapability::LayeredComicCompose
            ) | (Format::Film { .. }, ToolCapability::FilmRender)
        )
    {
        return fail(ErrorKind::Conflict, "review-production-copy");
    }
    Ok(())
}

pub(super) fn check_plan(
    witness: &Production,
    copy: &Copy,
    plan: &Plan,
    task: Option<&str>,
) -> Result<()> {
    check_copy(witness, copy)?;
    let packet = &witness.packet;
    if task.is_some_and(|id| id != packet.task.id)
        || make_packet(plan, &packet.task.id, packet.dependencies.clone())? != *packet
    {
        return fail(ErrorKind::Stale, "review-production-plan");
    }
    Ok(())
}
