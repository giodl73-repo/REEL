//! Complete-issue source correspondence. No renderer, custody or completion.
use crate::{publication::InputReader, *};
use reel_publication::{
    Content, Publication,
    comic_issue_source::{Assembly, AssemblyReceipt, Plan as IssuePlan},
    comic_source::{Object, Pin, Receipt},
};

#[derive(Debug)]
pub struct BoundIssue {
    task: Packet,
    publication_id: String,
    request: Object,
    plan: Object,
    parts: BTreeMap<String, Object>,
    pages: Vec<String>,
    output_bytes: u64,
    composition: Option<CompositionOrigins>,
}
/// Expected logical origins, checked against frozen native upstream evidence by
/// the managed adapter. Byte correspondence alone does not establish origin.
#[derive(Debug)]
pub struct CompositionOrigins {
    pub parents: BTreeSet<String>,
    pub inputs: BTreeMap<String, crate::upstream::Reference>,
}
impl CompositionOrigins {
    fn insert(&mut self, id: String, reference: crate::upstream::Reference) -> Result<()> {
        if self.inputs.insert(id, reference).is_some() {
            return fail(ErrorKind::Conflict, "issue-composition-origin-alias");
        }
        Ok(())
    }
}
impl BoundIssue {
    pub fn composition_origins(&self) -> Option<&CompositionOrigins> {
        self.composition.as_ref()
    }
    pub fn output_bytes(&self) -> u64 {
        self.output_bytes
    }
}
fn object(a: &Artifact) -> Object {
    Object {
        sha256: a.sha256.clone(),
        bytes: a.bytes,
    }
}
fn verified(reader: &mut impl InputReader, a: &Artifact, max: u64) -> Result<Vec<u8>> {
    if a.bytes > max {
        return fail(ErrorKind::Budget, "issue-input-bytes");
    }
    let raw = reader.read(a, max)?;
    if raw.len() as u64 != a.bytes || digest(&raw) != a.sha256 {
        return fail(ErrorKind::Integrity, "issue-input-content");
    }
    Ok(raw)
}
fn leaf<'a>(packet: &'a Packet, path: &str, expected: &Object) -> Result<&'a Artifact> {
    if !publication::portable_source(path) || path == "request.json" {
        return fail(ErrorKind::Invalid, "issue-source-path");
    }
    let tool = packet.task.tool.as_ref().ok_or_else(|| Error {
        kind: ErrorKind::Missing,
        code: "issue-tool".into(),
    })?;
    let id = tool.source_bindings.get(path).ok_or_else(|| Error {
        kind: ErrorKind::Missing,
        code: "issue-source-binding".into(),
    })?;
    let asset = packet
        .task
        .inputs
        .iter()
        .find(|a| &a.logical_id == id)
        .ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "issue-source-input".into(),
        })?;
    if object(asset) != *expected {
        return fail(ErrorKind::Conflict, "issue-source-pin");
    }
    Ok(asset)
}
fn pin_object(pin: &Pin) -> Object {
    Object {
        sha256: pin.sha256.clone(),
        bytes: pin.bytes,
    }
}

/// Binds complete declared part packets, including editable sources. Native
/// adapters must additionally verify origins/custody and deterministically replay.
pub fn bind(packet: &Packet, reader: &mut impl InputReader) -> Result<BoundIssue> {
    check_packet(packet)?;
    let tool = packet.task.tool.as_ref().ok_or_else(|| Error {
        kind: ErrorKind::Missing,
        code: "issue-tool".into(),
    })?;
    if tool.capability != ToolCapability::ComicIssueAssembly || tool.part.is_some() {
        return fail(ErrorKind::Unsupported, "issue-assembly-capability-required");
    }
    let publication: Publication = decode(&verified(
        reader,
        &tool.publication,
        MAX_CONTROL_BYTES as u64,
    )?)?;
    let request: Assembly = decode(&verified(reader, &tool.request, MAX_CONTROL_BYTES as u64)?)?;
    request.check_version()?;
    if !(1..=128).contains(&request.parts.len())
        || !(1..=128 * 1024 * 1024).contains(&request.limits.input_bytes)
        || !(1..=128 * 1024 * 1024).contains(&request.limits.output_bytes)
    {
        return fail(ErrorKind::Budget, "issue-assembly-contract");
    }
    let plan_asset = leaf(packet, &request.plan.path, &pin_object(&request.plan))?;
    let plan: IssuePlan = decode(&verified(reader, plan_asset, MAX_CONTROL_BYTES as u64)?)?;
    plan.check()?;
    if plan.publication != publication
        || plan.edition_id != tool.edition_id
        || request.parts.len() != plan.parts.len()
        || plan
            .parts
            .iter()
            .any(|p| !request.parts.contains_key(&p.id))
    {
        return fail(ErrorKind::Conflict, "issue-master-part-binding");
    }
    if request
        .bundles
        .keys()
        .any(|id| !request.parts.contains_key(id))
    {
        return fail(ErrorKind::Conflict, "issue-unused-bundle");
    }
    let edition = publication
        .editions
        .iter()
        .find(|e| e.id == tool.edition_id)
        .ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "issue-edition".into(),
        })?;
    let Content::Comic { pages, .. } = &edition.content else {
        return fail(ErrorKind::Unsupported, "issue-comic-required");
    };
    if packet.task.language.as_ref() != Some(&edition.language)
        || packet.task.outputs != edition.outputs
        || packet.task.outputs.len() != 2
        || [
            ("reading.pdf", "application/pdf"),
            ("transcript.json", "application/json"),
        ]
        .iter()
        .any(|(name, media)| {
            packet.task.outputs.get(*name).is_none_or(|c| {
                !c.required || c.media_type != *media || c.max_bytes > request.limits.output_bytes
            })
        })
    {
        return fail(ErrorKind::Conflict, "issue-edition-output-binding");
    }
    let mut expected = BTreeMap::from([(request.plan.path.clone(), pin_object(&request.plan))]);
    let mut composition = if let Some(spec) = &request.composition {
        let asset = leaf(packet, &spec.intent.path, &pin_object(&spec.intent))?;
        let intent: reel_publication::composition::Intent =
            decode(&verified(reader, asset, MAX_CONTROL_BYTES as u64)?)?;
        reel_publication::comic_issue_source::check_composition(&plan, spec, &intent)?;
        let edition = &intent.editions[&plan.edition_id];
        if intent.project != packet.project || edition.final_task != packet.task.id {
            return fail(ErrorKind::Conflict, "issue-composition-task-binding");
        }
        if expected
            .insert(spec.intent.path.clone(), pin_object(&spec.intent))
            .is_some()
        {
            return fail(ErrorKind::Conflict, "issue-source-path-alias");
        }
        Some(CompositionOrigins {
            parents: edition
                .components
                .iter()
                .chain(&edition.controls)
                .cloned()
                .collect(),
            inputs: BTreeMap::new(),
        })
    } else {
        None
    };
    let mut parts = BTreeMap::new();
    for part in &plan.parts {
        let pin = &request.parts[&part.id];
        let prefix = pin
            .path
            .strip_suffix("receipt.json")
            .filter(|p| p.is_empty() || p.ends_with('/'))
            .ok_or_else(|| Error {
                kind: ErrorKind::Invalid,
                code: "issue-part-receipt-path".into(),
            })?;
        let receipt_asset = leaf(packet, &pin.path, &pin_object(pin))?;
        if let Some(origins) = &mut composition {
            let spec = request.composition.as_ref().expect("checked composition");
            origins.insert(
                receipt_asset.logical_id.clone(),
                crate::upstream::Reference {
                    task_id: spec.producers[&part.id].clone(),
                    artifact: crate::upstream::Selector::WorkerReceipt,
                },
            )?;
        }
        let receipt: Receipt = decode(&verified(reader, receipt_asset, MAX_CONTROL_BYTES as u64)?)?;
        if receipt.schema != "reel.comic-build-receipt.v1"
            || receipt.part_id.as_ref().is_some_and(|id| id != &part.id)
            || (request.composition.is_some() && receipt.part_id.as_ref() != Some(&part.id))
            || receipt.publication_id != publication.id
            || receipt.edition_id != edition.id
            || receipt.language != edition.language
            || receipt.page_order != part.pages
            || receipt.source_files.get("request.json") != Some(&receipt.request)
            || receipt.human_approval
            || receipt.retained_owner_acknowledged
            || receipt.print_ready
        {
            return fail(ErrorKind::Conflict, "issue-part-receipt-binding");
        }
        if expected.insert(pin.path.clone(), pin_object(pin)).is_some() {
            return fail(ErrorKind::Conflict, "issue-source-path-alias");
        }
        parts.insert(part.id.clone(), pin_object(pin));
        if let Some(bundle) = request.bundles.get(&part.id) {
            let object = pin_object(bundle);
            let asset = leaf(packet, &bundle.path, &object)?;
            if let Some(origins) = &mut composition {
                let spec = request.composition.as_ref().expect("checked composition");
                origins.insert(
                    asset.logical_id.clone(),
                    crate::upstream::Reference {
                        task_id: spec.producers[&part.id].clone(),
                        artifact: crate::upstream::Selector::Output {
                            name: "editable.packet".into(),
                        },
                    },
                )?;
            }
            if bundle.path != format!("{prefix}editable.packet")
                || receipt.outputs.get("editable.packet") != Some(asset)
                || asset.media_type != "application/vnd.reel.editable-packet.v1"
            {
                return fail(ErrorKind::Conflict, "issue-part-bundle-binding");
            }
            if expected.insert(bundle.path.clone(), object).is_some() {
                return fail(ErrorKind::Conflict, "issue-source-path-alias");
            }
            continue;
        }
        for (path, source) in &receipt.source_files {
            let path = format!("{prefix}sources/{path}");
            leaf(packet, &path, source)?;
            if expected.insert(path, source.clone()).is_some() {
                return fail(ErrorKind::Conflict, "issue-source-path-alias");
            }
        }
        for (name, asset) in &receipt.outputs {
            reel_publication::check_artifact(asset)?;
            if let Some(origins) = &mut composition {
                let spec = request.composition.as_ref().expect("checked composition");
                origins.insert(
                    asset.logical_id.clone(),
                    crate::upstream::Reference {
                        task_id: spec.producers[&part.id].clone(),
                        artifact: crate::upstream::Selector::Output { name: name.clone() },
                    },
                )?;
            }
            let path = format!("{prefix}{name}");
            if leaf(packet, &path, &object(asset))? != asset {
                return fail(ErrorKind::Conflict, "issue-part-output-logical-binding");
            }
            if expected.insert(path, object(asset)).is_some() {
                return fail(ErrorKind::Conflict, "issue-source-path-alias");
            }
        }
    }
    let mut ids = BTreeSet::from([
        tool.publication.logical_id.clone(),
        tool.request.logical_id.clone(),
    ]);
    let mut folded = BTreeSet::new();
    if expected.len() != tool.source_bindings.len() {
        return fail(ErrorKind::Conflict, "issue-unused-source-binding");
    }
    for (path, obj) in &expected {
        if !folded.insert(path.to_ascii_lowercase()) {
            return fail(ErrorKind::Conflict, "issue-source-case-collision");
        }
        let a = leaf(packet, path, obj)?;
        ids.insert(a.logical_id.clone());
    }
    if ids.len() != packet.task.inputs.len()
        || packet
            .task
            .inputs
            .iter()
            .any(|a| !ids.contains(&a.logical_id))
    {
        return fail(ErrorKind::Conflict, "issue-unused-input");
    }
    let total = packet
        .task
        .inputs
        .iter()
        .try_fold(0u64, |n, a| n.checked_add(a.bytes))
        .ok_or_else(|| Error {
            kind: ErrorKind::Budget,
            code: "issue-input-overflow".into(),
        })?;
    if total > 64 * 1024 * 1024 {
        return fail(ErrorKind::Budget, "issue-input-closure-bytes");
    }
    for a in &packet.task.inputs {
        verified(reader, a, 64 * 1024 * 1024)?;
    }
    Ok(BoundIssue {
        task: packet.clone(),
        publication_id: publication.id.clone(),
        request: object(&tool.request),
        plan: pin_object(&request.plan),
        parts,
        pages: pages.iter().map(|p| p.id.clone()).collect(),
        output_bytes: request.limits.output_bytes,
        composition,
    })
}

pub fn check_receipt(bound: &BoundIssue, result: &ReturnedResult, raw: &[u8]) -> Result<()> {
    check_result(&bound.task, result)?;
    let receipt: AssemblyReceipt = decode(raw)?;
    let pin = result.worker_receipt.as_ref().ok_or_else(|| Error {
        kind: ErrorKind::Missing,
        code: "issue-worker-receipt".into(),
    })?;
    let tool = bound.task.task.tool.as_ref().ok_or_else(|| Error {
        kind: ErrorKind::Missing,
        code: "issue-tool".into(),
    })?;
    if pin.bytes != raw.len() as u64 || pin.sha256 != digest(raw) {
        return fail(ErrorKind::Integrity, "issue-worker-receipt-pin");
    }
    if result.provenance.mode != Mode::Tool
        || result.provenance.executable.as_ref() != Some(&tool.executable)
        || result.provenance.provider.is_some()
        || result.provenance.model.is_some()
        || receipt.schema != "reel.comic-issue-receipt.v1"
        || receipt.request != bound.request
        || receipt.plan != bound.plan
        || receipt.parts != bound.parts
        || receipt.page_order != bound.pages
        || receipt.outputs != result.artifacts
        || result.artifacts.iter().any(|(name, a)| {
            a.logical_id != format!("{}/{}/{}", bound.publication_id, tool.edition_id, name)
        })
    {
        return fail(ErrorKind::Conflict, "issue-receipt-correspondence");
    }
    if receipt.human_approval || receipt.retained_owner_acknowledged || receipt.print_ready {
        return fail(ErrorKind::Denied, "issue-worker-claims-authority");
    }
    Ok(())
}
