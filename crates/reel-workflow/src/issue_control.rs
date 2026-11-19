//! Derived assembly controls are candidate outputs, never injected Git records.
use crate::{publication::InputReader, *};
use reel_publication::{
    Content, Publication,
    comic_issue_source::{Assembly, Plan},
    comic_source::{Limits, Pin, Receipt},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PartPaths {
    pub receipt: String,
    pub bundle: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub plan_path: String,
    pub parts: BTreeMap<String, PartPaths>,
    pub limits: Limits,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControlReceipt {
    pub schema: String,
    pub task_key: String,
    pub request: Artifact,
    pub inputs: BTreeMap<String, Artifact>,
    pub outputs: ArtifactSet,
}
pub struct Generated {
    pub outputs: ArtifactSet,
    pub files: BTreeMap<String, Vec<u8>>,
}
fn verified(reader: &mut impl InputReader, a: &Artifact, max: u64) -> Result<Vec<u8>> {
    artifact(a)?;
    if a.bytes > max {
        return fail(ErrorKind::Budget, "issue-control-input-bytes");
    }
    let raw = reader.read(a, max)?;
    if raw.len() as u64 != a.bytes || digest(&raw) != a.sha256 {
        return fail(ErrorKind::Integrity, "issue-control-input-content");
    }
    Ok(raw)
}
fn pin(path: String, a: &Artifact) -> Pin {
    Pin {
        path,
        sha256: a.sha256.clone(),
        bytes: a.bytes,
    }
}
/// Metadata plus exact declared bytes. Native callers independently establish
/// original parent custody and job admission; this function grants neither.
pub fn generate(packet: &Packet, reader: &mut impl InputReader) -> Result<Generated> {
    check_packet(packet)?;
    let tool = packet.task.tool.as_ref().ok_or_else(|| Error {
        kind: ErrorKind::Missing,
        code: "issue-control-tool".into(),
    })?;
    if tool.capability != ToolCapability::ComicIssueControl || tool.part.is_some() {
        return fail(ErrorKind::Unsupported, "issue-control-capability");
    }
    let publication: Publication = decode(&verified(
        reader,
        &tool.publication,
        MAX_CONTROL_BYTES as u64,
    )?)?;
    let request: Request = decode(&verified(reader, &tool.request, MAX_CONTROL_BYTES as u64)?)?;
    if request.schema != "reel.workflow-issue-control.v1"
        || !(1..=128).contains(&request.parts.len())
        || !(1..=128 * 1024 * 1024).contains(&request.limits.input_bytes)
        || !(1..=128 * 1024 * 1024).contains(&request.limits.output_bytes)
    {
        return fail(ErrorKind::Invalid, "issue-control-request");
    }
    let mut used = BTreeMap::new();
    let mut total = tool
        .publication
        .bytes
        .checked_add(tool.request.bytes)
        .ok_or_else(|| Error {
            kind: ErrorKind::Budget,
            code: "issue-control-byte-overflow".into(),
        })?;
    let mut source = |path: &str, max: u64| -> Result<(Artifact, Vec<u8>)> {
        if !publication::portable_source(path) || path == "request.json" || used.contains_key(path)
        {
            return fail(ErrorKind::Conflict, "issue-control-source-path");
        }
        let id = tool.source_bindings.get(path).ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "issue-control-source-binding".into(),
        })?;
        let a = packet
            .task
            .inputs
            .iter()
            .find(|a| &a.logical_id == id)
            .ok_or_else(|| Error {
                kind: ErrorKind::Missing,
                code: "issue-control-source-input".into(),
            })?
            .clone();
        total = total.checked_add(a.bytes).ok_or_else(|| Error {
            kind: ErrorKind::Budget,
            code: "issue-control-byte-overflow".into(),
        })?;
        if total > 64 * 1024 * 1024 {
            return fail(ErrorKind::Budget, "issue-control-input-closure");
        }
        let raw = verified(reader, &a, max)?;
        used.insert(path.to_owned(), a.clone());
        Ok((a, raw))
    };
    let (plan_asset, plan_raw) = source(&request.plan_path, MAX_CONTROL_BYTES as u64)?;
    let plan: Plan = decode(&plan_raw)?;
    plan.check()?;
    let edition = publication
        .editions
        .iter()
        .find(|e| e.id == tool.edition_id)
        .ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "issue-control-edition".into(),
        })?;
    if plan.publication != publication
        || plan.edition_id != tool.edition_id
        || packet.task.language.as_ref() != Some(&edition.language)
        || !matches!(edition.content, Content::Comic { .. })
        || request.parts.keys().collect::<BTreeSet<_>>()
            != plan.parts.iter().map(|p| &p.id).collect()
    {
        return fail(ErrorKind::Conflict, "issue-control-master-parts");
    }
    let mut parts = BTreeMap::new();
    let mut bundles = BTreeMap::new();
    for part in &plan.parts {
        let paths = &request.parts[&part.id];
        let prefix = paths
            .receipt
            .strip_suffix("/receipt.json")
            .ok_or_else(|| Error {
                kind: ErrorKind::Invalid,
                code: "issue-control-receipt-path".into(),
            })?;
        if paths.bundle != format!("{prefix}/editable.packet") {
            return fail(ErrorKind::Conflict, "issue-control-bundle-path");
        }
        let (receipt_asset, raw) = source(&paths.receipt, MAX_CONTROL_BYTES as u64)?;
        let receipt: Receipt = decode(&raw)?;
        let (bundle, _) = source(&paths.bundle, 64 * 1024 * 1024)?;
        if receipt.part_id.as_deref() != Some(&part.id)
            || receipt.schema != "reel.comic-build-receipt.v1"
            || receipt.publication_id != publication.id
            || receipt.edition_id != edition.id
            || receipt.language != edition.language
            || receipt.page_order != part.pages
            || receipt.outputs.get("editable.packet") != Some(&bundle)
            || bundle.logical_id
                != format!(
                    "{}/{}/parts/{}/editable.packet",
                    publication.id, edition.id, part.id
                )
            || bundle.media_type != "application/vnd.reel.editable-packet.v1"
            || receipt_asset.media_type != "application/json"
            || receipt.human_approval
            || receipt.retained_owner_acknowledged
            || receipt.print_ready
        {
            return fail(ErrorKind::Conflict, "issue-control-part-correspondence");
        }
        parts.insert(part.id.clone(), pin(paths.receipt.clone(), &receipt_asset));
        bundles.insert(part.id.clone(), pin(paths.bundle.clone(), &bundle));
    }
    if used.len() != tool.source_bindings.len() {
        return fail(ErrorKind::Conflict, "issue-control-unused-binding");
    }
    let expected: BTreeSet<_> = used
        .values()
        .map(|a| &a.logical_id)
        .chain([&tool.publication.logical_id, &tool.request.logical_id])
        .collect();
    if expected.len() != packet.task.inputs.len()
        || packet
            .task
            .inputs
            .iter()
            .any(|a| !expected.contains(&a.logical_id))
    {
        return fail(ErrorKind::Conflict, "issue-control-unused-input");
    }
    let assembly = Assembly {
        schema: "reel.comic-issue-assembly.v1".into(),
        plan: pin(request.plan_path, &plan_asset),
        parts,
        bundles,
        limits: request.limits,
    };
    let raw = serde_json::to_vec(&assembly).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "issue-control-output-json".into(),
    })?;
    if raw.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "issue-control-output-bytes");
    }
    if packet.task.outputs.len() != 1
        || packet.task.outputs.get("assembly.json").is_none_or(|c| {
            !c.required
                || c.media_type != "application/json"
                || c.max_bytes > MAX_CONTROL_BYTES as u64
                || c.max_bytes < raw.len() as u64
        })
    {
        return fail(ErrorKind::Conflict, "issue-control-output-contract");
    }
    let outputs = BTreeMap::from([(
        "assembly.json".into(),
        Artifact {
            logical_id: format!("workflow/{}/assembly.json", packet.task_key),
            sha256: digest(&raw),
            bytes: raw.len() as u64,
            media_type: "application/json".into(),
        },
    )]);
    let receipt = ControlReceipt {
        schema: "reel.workflow-issue-control-receipt.v1".into(),
        task_key: packet.task_key.clone(),
        request: tool.request.clone(),
        inputs: used,
        outputs: outputs.clone(),
    };
    let receipt_raw = serde_json::to_vec(&receipt).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "issue-control-receipt-json".into(),
    })?;
    if receipt_raw.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "issue-control-receipt-bytes");
    }
    Ok(Generated {
        outputs,
        files: BTreeMap::from([
            ("assembly.json".into(), raw),
            ("receipt.json".into(), receipt_raw),
        ]),
    })
}
