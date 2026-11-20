//! Derived comic review controls from an exact producer dependency. Pure checks
//! cannot grant native custody; the composite admission adapter owns that proof.
use crate::*;
use publication::InputReader;
pub const OUTPUTS: [&str; 3] = [
    "review-copy.json",
    "review-sources.json",
    "review-request.json",
];
pub struct Generated {
    pub outputs: ArtifactSet,
    pub files: BTreeMap<String, Vec<u8>>,
}
pub fn inputs(parent: &Packet, result: &ReturnedResult) -> Result<Vec<Artifact>> {
    check_result(parent, result)?;
    let receipt = result.worker_receipt.as_ref().ok_or(Error {
        kind: ErrorKind::Missing,
        code: "review-control-producer-receipt".into(),
    })?;
    let mut inputs = BTreeMap::new();
    for pin in parent
        .task
        .inputs
        .iter()
        .chain(result.artifacts.values())
        .chain(std::iter::once(receipt))
    {
        if inputs
            .insert(pin.logical_id.clone(), pin.clone())
            .is_some_and(|old| old != *pin)
        {
            return fail(ErrorKind::Conflict, "review-control-input-identity");
        }
    }
    if inputs.len() > MAX_INPUTS {
        return fail(ErrorKind::Budget, "review-control-input-count");
    }
    Ok(inputs.into_values().collect())
}
pub fn source_bindings(
    parent: &Packet,
    result: &ReturnedResult,
) -> Result<BTreeMap<String, String>> {
    inputs(parent, result)?;
    Ok(parent
        .task
        .inputs
        .iter()
        .enumerate()
        .map(|(i, a)| (format!("inputs/{i:03}"), a.logical_id.clone()))
        .chain(
            result
                .artifacts
                .iter()
                .map(|(name, a)| (format!("outputs/{name}"), a.logical_id.clone())),
        )
        .collect())
}
/// Carry the producer's exact upstream selectors into a derived review task,
/// then add its own outputs/receipt. This is a planning declaration: native
/// admission must still verify every named original retained parent.
pub fn bindings(
    parent: &Packet,
    result: &ReturnedResult,
    inherited: Option<&upstream::Bindings>,
) -> Result<upstream::Bindings> {
    inputs(parent, result)?;
    let mut bindings = match (&parent.task.upstream_inputs, inherited) {
        (None, None) => upstream::Bindings {
            schema: "reel.workflow-upstream-inputs.v2".into(),
            streaming: None,
            inputs: BTreeMap::new(),
        },
        (Some(pin), Some(original)) => {
            original.allowance()?;
            let raw = original.canonical_bytes()?;
            if pin.bytes != raw.len() as u64 || pin.sha256 != digest(&raw) {
                return fail(
                    ErrorKind::Integrity,
                    "review-control-inherited-bindings-pin",
                );
            }
            if original.inputs.is_empty()
                || original
                    .inputs
                    .keys()
                    .any(|id| !parent.task.inputs.iter().any(|a| &a.logical_id == id))
                || original
                    .inputs
                    .values()
                    .map(|r| &r.task_id)
                    .collect::<BTreeSet<_>>()
                    != parent.dependencies.keys().collect()
            {
                return fail(ErrorKind::Conflict, "review-control-inherited-bindings-set");
            }
            original.clone()
        }
        _ => {
            return fail(
                ErrorKind::Missing,
                "review-control-original-bindings-required",
            );
        }
    };
    for (id, selector) in result
        .artifacts
        .iter()
        .map(|(name, pin)| {
            (
                &pin.logical_id,
                upstream::Selector::Output { name: name.clone() },
            )
        })
        .chain(
            result
                .worker_receipt
                .iter()
                .map(|pin| (&pin.logical_id, upstream::Selector::WorkerReceipt)),
        )
    {
        let reference = upstream::Reference {
            task_id: parent.task.id.clone(),
            artifact: selector,
        };
        if bindings.inputs.insert(id.clone(), reference).is_some() {
            return fail(ErrorKind::Conflict, "review-control-inherited-origin-alias");
        }
    }
    if bindings.inputs.len() > MAX_INPUTS {
        return fail(ErrorKind::Budget, "review-control-inherited-input-count");
    }
    bindings.canonical_bytes()?;
    Ok(bindings)
}
fn read(reader: &mut impl InputReader, pin: &Artifact, max: u64) -> Result<Vec<u8>> {
    if pin.bytes > max {
        return fail(ErrorKind::Budget, "review-control-input-bytes");
    }
    let raw = reader.read(pin, max)?;
    if raw.len() as u64 != pin.bytes || digest(&raw) != pin.sha256 {
        return fail(ErrorKind::Integrity, "review-control-input-content");
    }
    Ok(raw)
}
pub fn generate(
    packet: &Packet,
    bindings: &upstream::Bindings,
    parents: &BTreeMap<String, upstream::Parent<'_>>,
    reader: &mut impl InputReader,
) -> Result<Generated> {
    generate_with_issue(packet, bindings, parents, reader, |_, _, _| {
        fail(
            ErrorKind::Unsupported,
            "review-control-issue-replay-required",
        )
    })
}
/// Whole-issue controls additionally require the caller's complete bounded
/// producer replay. The callback supplies captured sources, not custody or
/// creative authority; native callers must keep original parent guards live.
pub fn generate_with_issue<R: InputReader>(
    packet: &Packet,
    bindings: &upstream::Bindings,
    parents: &BTreeMap<String, upstream::Parent<'_>>,
    reader: &mut R,
    capture: impl FnOnce(&Packet, &ReturnedResult, &mut R) -> Result<BTreeMap<String, Vec<u8>>>,
) -> Result<Generated> {
    check_packet(packet)?;
    let tool = packet.task.tool.as_ref().ok_or(Error {
        kind: ErrorKind::Missing,
        code: "review-control-tool".into(),
    })?;
    if tool.capability != ToolCapability::ComicReviewControl
        || !tool.auxiliary_executables.is_empty()
        || tool.part.is_some()
    {
        return fail(ErrorKind::Unsupported, "review-control-capability");
    }
    let origins = upstream::bind(packet, bindings, parents)?;
    let origin = origins.get(&tool.request.logical_id).ok_or(Error {
        kind: ErrorKind::Missing,
        code: "review-control-producer-origin".into(),
    })?;
    if origin.reference.artifact != upstream::Selector::WorkerReceipt {
        return fail(ErrorKind::Conflict, "review-control-receipt-selector");
    }
    let parent = &parents[&origin.reference.task_id];
    let producer = parent.packet.task.tool.as_ref().ok_or(Error {
        kind: ErrorKind::Missing,
        code: "review-control-producer-tool".into(),
    })?;
    if !matches!(
        producer.capability,
        ToolCapability::ComicRender | ToolCapability::ComicIssueAssembly
    ) || tool.publication != producer.publication
        || tool.edition_id != producer.edition_id
        || packet.task.language != parent.packet.task.language
        || parent.result.worker_receipt.as_ref() != Some(&tool.request)
        || tool.source_bindings != source_bindings(parent.packet, parent.result)?
    {
        return fail(ErrorKind::Conflict, "review-control-producer-binding");
    }
    let expected = inputs(parent.packet, parent.result)?;
    let actual: BTreeMap<_, _> = packet
        .task
        .inputs
        .iter()
        .map(|a| (&a.logical_id, a))
        .collect();
    if actual.len() != expected.len()
        || expected
            .iter()
            .any(|a| actual.get(&a.logical_id) != Some(&a))
    {
        return fail(ErrorKind::Conflict, "review-control-input-set");
    }
    if packet.task.outputs.len() != OUTPUTS.len()
        || OUTPUTS.iter().any(|name| {
            packet.task.outputs.get(*name).is_none_or(|c| {
                c.media_type != "application/json"
                    || !c.required
                    || c.max_bytes > MAX_CONTROL_BYTES as u64
            })
        })
    {
        return fail(ErrorKind::Invalid, "review-control-output-contract");
    }
    let mut total = 0u64;
    for pin in &expected {
        total = total
            .checked_add(pin.bytes)
            .filter(|n| *n <= 64 * 1024 * 1024)
            .ok_or(Error {
                kind: ErrorKind::Budget,
                code: "review-control-total-input-bytes".into(),
            })?;
        read(reader, pin, 64 * 1024 * 1024)?;
    }
    let receipt = read(reader, &tool.request, MAX_CONTROL_BYTES as u64)?;
    let mut exported = if producer.capability == ToolCapability::ComicIssueAssembly {
        let bound = crate::issue::bind(parent.packet, reader)?;
        crate::issue::check_receipt(&bound, parent.result, &receipt)?;
        let files = capture(parent.packet, parent.result, reader)?;
        crate::issue::review_copy(&bound, parent.packet, parent.result, &receipt, &files)?
    } else {
        let bound = publication::bind_comic(parent.packet, reader)?;
        publication::comic_review_copy(&bound, parent.packet, parent.result, &receipt)?
    };
    let sources = exported.manifest.take().ok_or(Error {
        kind: ErrorKind::Integrity,
        code: "review-control-comic-manifest".into(),
    })?;
    let pin = |id: String, raw: &[u8]| Artifact {
        logical_id: id,
        sha256: digest(raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    };
    let sources_pin = pin(
        format!("workflow/review/{}/sources", packet.task_key),
        &sources,
    );
    exported.copy.review = sources_pin.clone();
    review::check_copy(&exported.copy)?;
    let copy = serde_json::to_vec(&exported.copy).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "review-control-copy-json".into(),
    })?;
    let copy_pin = pin(format!("workflow/review/{}/copy", packet.task_key), &copy);
    let request = serde_json::to_vec(&review::Request {
        schema: "reel.workflow-review-request.v1".into(),
        production_commit: parent.packet.source_commit.clone(),
        copy_input: copy_pin.logical_id.clone(),
        report_output: "report".into(),
    })
    .map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "review-control-request-json".into(),
    })?;
    let request_pin = pin(review::REQUEST_ID.into(), &request);
    let outputs = ArtifactSet::from([
        ("review-copy.json".into(), copy_pin),
        ("review-sources.json".into(), sources_pin),
        ("review-request.json".into(), request_pin),
    ]);
    reel_publication::check_outputs(&packet.task.outputs, &outputs)?;
    let receipt = serde_json::json!({"schema":"reel.workflow-review-control-receipt.v1","task_id":packet.task.id,"task_key":packet.task_key,
        "producer_task_key":parent.packet.task_key,"producer_result_sha256":canonical_digest(parent.result)?,"producer_receipt":tool.request,"outputs":outputs,
        "human_approval":false,"retained_owner_acknowledged":false});
    let receipt = serde_json::to_vec(&receipt).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "review-control-receipt-json".into(),
    })?;
    if receipt.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "review-control-receipt-bytes");
    }
    Ok(Generated {
        outputs,
        files: BTreeMap::from([
            ("review-copy.json".into(), copy),
            ("review-sources.json".into(), sources),
            ("review-request.json".into(), request),
            ("receipt.json".into(), receipt),
        ]),
    })
}
