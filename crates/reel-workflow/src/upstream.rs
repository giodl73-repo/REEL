//! Portable logical bindings for retained dependency inputs. These checks grant
//! no storage custody, byte access, admission or task completion.
use crate::*;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub task_id: String,
    pub artifact: Selector,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Selector {
    Output { name: String },
    WorkerReceipt,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bindings {
    pub schema: String,
    /// Child input logical identity -> exact parent task and output name.
    pub inputs: BTreeMap<String, Reference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streaming: Option<Streaming>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Streaming {
    pub schema: String,
    pub input_bytes: u64,
    pub input_object_bytes: u64,
}
#[derive(Clone, Debug)]
pub struct BoundInput {
    pub reference: Reference,
    pub artifact: Artifact,
    pub result_sha256: String,
}
pub struct Parent<'a> {
    pub packet: &'a Packet,
    pub result: &'a ReturnedResult,
}
impl Bindings {
    pub fn allowance(&self) -> Result<(u64, u64)> {
        match (self.schema.as_str(), &self.streaming) {
            ("reel.workflow-upstream-inputs.v1" | "reel.workflow-upstream-inputs.v2", None) => {
                Ok((64 * 1024 * 1024, 64 * 1024 * 1024))
            }
            ("reel.workflow-upstream-inputs.v3", Some(s)) => {
                if s.schema != "reel.workflow-upstream-streaming.v1" {
                    return fail(ErrorKind::Invalid, "upstream-streaming-version");
                }
                if s.input_object_bytes == 0
                    || s.input_object_bytes > s.input_bytes
                    || s.input_object_bytes > 16 * 1024 * 1024 * 1024
                    || s.input_bytes > 64 * 1024 * 1024 * 1024
                {
                    return fail(ErrorKind::Budget, "upstream-streaming-bounds");
                }
                Ok((s.input_bytes, s.input_object_bytes))
            }
            _ => fail(ErrorKind::Invalid, "upstream-version-envelope"),
        }
    }
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        let raw = serde_json::to_vec(self).map_err(|_| Error {
            kind: ErrorKind::Invalid,
            code: "upstream-bindings-json".into(),
        })?;
        if raw.len() > MAX_CONTROL_BYTES {
            return fail(ErrorKind::Budget, "upstream-bindings-bytes");
        }
        Ok(raw)
    }
}
pub fn bind(
    packet: &Packet,
    bindings: &Bindings,
    parents: &BTreeMap<String, Parent<'_>>,
) -> Result<BTreeMap<String, BoundInput>> {
    check_packet(packet)?;
    let pin = packet.task.upstream_inputs.as_ref().ok_or_else(|| Error {
        kind: ErrorKind::Missing,
        code: "upstream-binding-pin".into(),
    })?;
    let raw = bindings.canonical_bytes()?;
    if raw.len() as u64 != pin.bytes || digest(&raw) != pin.sha256 {
        return fail(ErrorKind::Integrity, "upstream-binding-content");
    }
    let (input_bytes, object_bytes) = bindings.allowance()?;
    if bindings.inputs.is_empty() || bindings.inputs.len() > MAX_INPUTS {
        return fail(ErrorKind::Invalid, "upstream-binding-contract");
    }
    let used: BTreeSet<_> = bindings.inputs.values().map(|r| &r.task_id).collect();
    if used != parents.keys().collect() {
        return fail(ErrorKind::Conflict, "upstream-parent-set");
    }
    let mut evidence_bytes = 0usize;
    for (id, parent) in parents {
        check_result(parent.packet, parent.result)?;
        let evidence = serde_json::to_vec(&(parent.packet, parent.result)).map_err(|_| Error {
            kind: ErrorKind::Invalid,
            code: "upstream-parent-json".into(),
        })?;
        evidence_bytes = evidence_bytes
            .checked_add(evidence.len())
            .ok_or_else(|| Error {
                kind: ErrorKind::Budget,
                code: "upstream-parent-evidence-bytes".into(),
            })?;
        if evidence_bytes > 32 * MAX_CONTROL_BYTES {
            return fail(ErrorKind::Budget, "upstream-parent-evidence-bytes");
        }
        let expected = packet.dependencies.get(id).ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "upstream-declared-parent".into(),
        })?;
        if parent.packet.task.id != *id
            || parent.packet.project != packet.project
            || (bindings.schema == "reel.workflow-upstream-inputs.v1"
                && parent.packet.source_commit != packet.source_commit)
            || canonical_digest(parent.result)? != expected.result_sha256
            || parent.result.artifacts != expected.artifacts
        {
            return fail(ErrorKind::Stale, "upstream-original-parent-result");
        }
    }
    let mut bound = BTreeMap::new();
    let mut total = 0u64;
    for (id, reference) in &bindings.inputs {
        let dependency = packet
            .dependencies
            .get(&reference.task_id)
            .ok_or_else(|| Error {
                kind: ErrorKind::Missing,
                code: "upstream-declared-parent".into(),
            })?;
        let parent = &parents[&reference.task_id];
        let artifact = match &reference.artifact {
            Selector::Output { name } => parent.result.artifacts.get(name),
            Selector::WorkerReceipt => parent.result.worker_receipt.as_ref(),
        }
        .ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "upstream-declared-output".into(),
        })?;
        if artifact.logical_id != *id || !packet.task.inputs.contains(artifact) {
            return fail(ErrorKind::Conflict, "upstream-logical-input-binding");
        }
        total = total.checked_add(artifact.bytes).ok_or_else(|| Error {
            kind: ErrorKind::Budget,
            code: "upstream-input-bytes".into(),
        })?;
        if total > input_bytes || artifact.bytes > object_bytes {
            return fail(ErrorKind::Budget, "upstream-input-bytes");
        }
        bound.insert(
            id.clone(),
            BoundInput {
                reference: reference.clone(),
                artifact: artifact.clone(),
                result_sha256: dependency.result_sha256.clone(),
            },
        );
    }
    Ok(bound)
}
