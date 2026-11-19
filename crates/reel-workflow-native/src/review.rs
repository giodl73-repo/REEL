//! Structured intelligence review semantics under the existing native lifecycle.
use crate::{NativeInputs, NativeProfile, admission::InputPermit, error, io_error};
use reel_workflow::{
    ErrorKind, Operation, Packet, Result, ReturnedResult, publication::InputReader,
};
use std::{collections::BTreeMap, io::Read};
use tig_core::Object;
use tig_git::annex::workflow::reader;
pub(crate) fn prepare(packet: &Packet, native: &mut NativeInputs) -> Result<()> {
    if packet.task.operation == Operation::ReviewReport {
        reel_workflow::review::bind(packet, native)?;
    }
    Ok(())
}
pub(crate) struct Proof {
    _inputs: NativeInputs,
    _outputs: Vec<reader::VerifiedRead>,
    pub(crate) checked: reel_workflow::review::CheckedReport,
}
pub(crate) fn verify(
    packet: &Packet,
    source: &NativeProfile,
    result: &ReturnedResult,
    permit: InputPermit,
) -> Result<Proof> {
    reel_workflow::check_result(packet, result)?;
    result
        .artifacts
        .values()
        .try_fold(0u64, |n, a| n.checked_add(a.bytes))
        .filter(|n| *n <= 128 * 1024 * 1024)
        .ok_or_else(|| error(ErrorKind::Budget, "review-native-output-bytes"))?;
    let mut native = NativeInputs::from_permit(packet, source, permit)?;
    let bound = reel_workflow::review::bind(packet, &mut native)?;
    let mut total = 0u64;
    for pin in &packet.task.inputs {
        total = total
            .checked_add(pin.bytes)
            .filter(|n| *n <= 128 * 1024 * 1024)
            .ok_or_else(|| error(ErrorKind::Budget, "review-native-input-bytes"))?;
        native.read(pin, 128 * 1024 * 1024)?;
    }
    let mut held = Vec::new();
    let mut captured = BTreeMap::new();
    for pin in result.artifacts.values() {
        let object = Object {
            sha256: tig_core::Digest::parse(&pin.sha256)
                .map_err(|_| error(ErrorKind::Invalid, "review-native-output-digest"))?,
            bytes: pin.bytes,
        };
        if captured.contains_key(&object) {
            continue;
        }
        let raw = if let Some(raw) = native.captured(&object) {
            raw.to_vec()
        } else {
            let mut guard = reader::open(&source.source, &object).map_err(io_error)?;
            let mut raw = Vec::new();
            Read::by_ref(&mut guard)
                .take(pin.bytes.saturating_add(1))
                .read_to_end(&mut raw)
                .map_err(io_error)?;
            held.push(guard);
            raw
        };
        if raw.len() as u64 != pin.bytes || reel_workflow::digest(&raw) != pin.sha256 {
            return Err(error(ErrorKind::Integrity, "review-native-output-content"));
        }
        captured.insert(object, raw);
    }
    let pin = result
        .artifacts
        .get(bound.report_output())
        .ok_or_else(|| error(ErrorKind::Missing, "review-native-report-output"))?;
    let object = Object {
        sha256: tig_core::Digest::parse(&pin.sha256)
            .map_err(|_| error(ErrorKind::Invalid, "review-native-output-digest"))?,
        bytes: pin.bytes,
    };
    let checked = bound.check(result, &captured[&object])?;
    Ok(Proof {
        _inputs: native,
        _outputs: held,
        checked,
    })
}
