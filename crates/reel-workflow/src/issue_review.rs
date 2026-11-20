//! Metadata correspondence over an already captured issue. Native callers must
//! separately prove custody and deterministic replay of the entire issue.
use super::*;
use reel_publication::comic_source::{PageSource, Request as PartRequest};

fn captured<'a>(
    files: &'a BTreeMap<String, Vec<u8>>,
    path: &str,
    pin: &Object,
    maximum: u64,
) -> Result<&'a [u8]> {
    if !crate::publication::portable_source(path) || pin.bytes > maximum {
        return fail(ErrorKind::Budget, "issue-review-source-bound");
    }
    let raw = files.get(path).ok_or(Error {
        kind: ErrorKind::Missing,
        code: "issue-review-source".into(),
    })?;
    if raw.len() as u64 != pin.bytes || digest(raw) != pin.sha256 {
        return fail(ErrorKind::Integrity, "issue-review-source-pin");
    }
    Ok(raw)
}

/// Derive whole-issue page/panel/balloon targets from exact captured source and
/// part receipt pins. This verifies metadata/bytes, not native custody or viewing.
pub fn review_copy(
    bound: &BoundIssue,
    packet: &Packet,
    result: &ReturnedResult,
    receipt: &[u8],
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<crate::review::Export> {
    if files.len() > 262_144
        || files
            .values()
            .try_fold(0u64, |n, b| n.checked_add(b.len() as u64))
            .is_none_or(|n| n > bound.output_bytes)
    {
        return fail(ErrorKind::Budget, "issue-review-capture-budget");
    }
    if &bound.task != packet {
        return fail(ErrorKind::Stale, "issue-review-original-task");
    }
    check_receipt(bound, result, receipt)?;
    let plan = &bound.review_plan;
    let edition = plan
        .publication
        .editions
        .iter()
        .find(|e| e.id == plan.edition_id)
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "issue-review-edition".into(),
        })?;
    let Content::Comic { pages, .. } = &edition.content else {
        return fail(ErrorKind::Unsupported, "issue-review-comic-required");
    };
    let mut units = BTreeMap::new();
    let mut reading = BTreeMap::new();
    for part in &plan.parts {
        let prefix = format!("parts/{}/", part.id);
        let raw = captured(
            files,
            &format!("{prefix}receipt.json"),
            &bound.parts[&part.id],
            MAX_CONTROL_BYTES as u64,
        )?;
        let part_receipt: Receipt = decode(raw)?;
        if part_receipt.schema != "reel.comic-build-receipt.v1"
            || part_receipt
                .part_id
                .as_ref()
                .is_some_and(|id| id != &part.id)
            || part_receipt.publication_id != plan.publication.id
            || part_receipt.edition_id != edition.id
            || part_receipt.language != edition.language
            || part_receipt.page_order != part.pages
            || part_receipt.human_approval
            || part_receipt.retained_owner_acknowledged
            || part_receipt.print_ready
        {
            return fail(ErrorKind::Conflict, "issue-review-part-correspondence");
        }
        let request_raw = captured(
            files,
            &format!("{prefix}sources/request.json"),
            &part_receipt.request,
            MAX_CONTROL_BYTES as u64,
        )?;
        let request: PartRequest = decode(request_raw)?;
        if request.schema != "reel.comic-build.v1"
            || request.publication_id != plan.publication.id
            || request.edition_id != edition.id
            || request.language != edition.language
            || request.part_id != part_receipt.part_id
            || request.pages.len() != part.pages.len()
            || part_receipt.source_files.get("request.json") != Some(&part_receipt.request)
        {
            return fail(ErrorKind::Conflict, "issue-review-part-request");
        }
        for (page_id, pin) in part.pages.iter().zip(&request.pages) {
            let selected = pages.iter().find(|p| &p.id == page_id).ok_or(Error {
                kind: ErrorKind::Missing,
                code: "issue-review-selected-page".into(),
            })?;
            let object = pin_object(pin);
            if object != super::object(&selected.source)
                || part_receipt.source_files.get(&pin.path) != Some(&object)
            {
                return fail(ErrorKind::Conflict, "issue-review-page-selection");
            }
            let source: PageSource = decode(captured(
                files,
                &format!("{prefix}sources/{}", pin.path),
                &object,
                256 * 1024,
            )?)?;
            if source.schema != "reel.comic-page.v1"
                || source.id != *page_id
                || !source
                    .panels
                    .iter()
                    .map(|p| &p.id)
                    .eq(selected.panel_order.iter())
                || !source
                    .utterances
                    .iter()
                    .map(|u| &u.id)
                    .eq(selected.utterance_order.iter())
            {
                return fail(ErrorKind::Conflict, "issue-review-page-order");
            }
            let mut add = |panel_id: Option<String>, balloon_id: Option<String>| -> Result<()> {
                let unit = crate::review::Unit::Comic {
                    page_id: page_id.clone(),
                    panel_id,
                    balloon_id,
                };
                if units
                    .insert(crate::review::unit_key(&unit)?, unit)
                    .is_some()
                {
                    return fail(ErrorKind::Conflict, "issue-review-unit-alias");
                }
                Ok(())
            };
            add(None, None)?;
            for panel in &source.panels {
                add(Some(panel.id.clone()), None)?;
            }
            for utterance in &source.utterances {
                if !source.panels.iter().any(|p| p.id == utterance.panel_id) {
                    return fail(ErrorKind::Missing, "issue-review-balloon-panel");
                }
                add(Some(utterance.panel_id.clone()), Some(utterance.id.clone()))?;
            }
            let name = format!("{page_id}-reading.png");
            let artifact = part_receipt.outputs.get(&name).ok_or(Error {
                kind: ErrorKind::Missing,
                code: "issue-review-reading".into(),
            })?;
            reel_publication::check_artifact(artifact)?;
            let expected_id = if let Some(part_id) = &part_receipt.part_id {
                format!(
                    "{}/{}/parts/{part_id}/{name}",
                    plan.publication.id, edition.id
                )
            } else {
                format!("{}/{}/{name}", plan.publication.id, edition.id)
            };
            if artifact.logical_id != expected_id || artifact.media_type != "image/png" {
                return fail(ErrorKind::Conflict, "issue-review-reading-identity");
            }
            captured(
                files,
                &format!("{prefix}{name}"),
                &super::object(artifact),
                128 * 1024 * 1024,
            )?;
            if reading.insert(page_id.clone(), artifact.clone()).is_some() {
                return fail(ErrorKind::Conflict, "issue-review-reading-alias");
            }
        }
    }
    let manifest=serde_json::to_vec(&serde_json::json!({"schema":"reel.workflow-comic-review.v1","task_key":packet.task_key,
        "edition_id":edition.id,"language":edition.language,"page_order":bound.pages,"reading":reading}))
        .map_err(|_|Error {kind:ErrorKind::Invalid,code:"issue-review-manifest-json".into()})?;
    if manifest.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "issue-review-manifest-bytes");
    }
    let copy = crate::review::Copy {
        schema: "reel.workflow-review-copy.v1".into(),
        project: packet.project.clone(),
        source_commit: packet.source_commit.clone(),
        language: edition.language.clone(),
        publication: packet.task.tool.as_ref().unwrap().publication.clone(),
        review: Artifact {
            logical_id: format!("workflow/review/{}/reading", packet.task_key),
            sha256: digest(&manifest),
            bytes: manifest.len() as u64,
            media_type: "application/json".into(),
        },
        format: crate::review::Format::Comic {
            page_order: bound.pages.clone(),
        },
        units,
    };
    crate::review::check_copy(&copy)?;
    Ok(crate::review::Export {
        copy,
        manifest: Some(manifest),
    })
}
