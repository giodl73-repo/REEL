//! Checks that a captured cue matches the pins in its private speech receipt.
use crate::{Error, ErrorKind, Operation, Packet, Result, ReturnedResult};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
struct Pin {
    sha256: String,
    bytes: u64,
}

impl Pin {
    fn matches(&self, artifact: &crate::Artifact) -> bool {
        self.sha256 == artifact.sha256 && self.bytes == artifact.bytes
    }
}

#[derive(Deserialize)]
struct Receipt {
    schema: String,
    request: Pin,
    authorization: Pin,
    wave: Pin,
    alignment: Pin,
    timing_qualified: bool,
    production_selection: bool,
    human_approval: bool,
    public_release: bool,
}

pub fn check(packet: &Packet, result: &ReturnedResult, path: &Path) -> Result<bool> {
    crate::check_result(packet, result)?;
    let error = |code: &str| Error {
        kind: ErrorKind::Conflict,
        code: code.into(),
    };
    if packet.task.operation != Operation::SpeechCandidate || packet.task.language.is_none() {
        return Err(error("speech-handoff-task"));
    }
    let output = |name: &str, media: &str| {
        result
            .artifacts
            .get(name)
            .filter(|a| a.media_type == media)
            .ok_or_else(|| error("speech-handoff-output"))
    };
    let receipt = output("receipt", "application/json")?;
    let raw = crate::manual::read_bytes(path)?;
    if raw.len() as u64 != receipt.bytes || crate::digest(&raw) != receipt.sha256 {
        return Err(error("speech-handoff-receipt-pin"));
    }
    let receipt: Receipt = crate::decode(&raw)?;
    if receipt.schema != "reel.speech-candidate-receipt.v1"
        || receipt.production_selection
        || receipt.human_approval
        || receipt.public_release
        || !receipt.wave.matches(output("recording", "audio/wav")?)
        || !receipt
            .alignment
            .matches(output("alignment", "application/json")?)
        || ![receipt.request, receipt.authorization].iter().all(|pin| {
            packet
                .task
                .inputs
                .iter()
                .any(|a| a.media_type == "application/json" && pin.matches(a))
        })
    {
        return Err(error("speech-handoff-receipt-binding"));
    }
    Ok(receipt.timing_qualified)
}
