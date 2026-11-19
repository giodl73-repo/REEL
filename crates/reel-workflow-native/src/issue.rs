//! Exact declared issue sources and complete deterministic packet replay.
use crate::{NativeInputs, NativeProfile, admission::InputPermit, error, io_error};
use reel_workflow::{
    Artifact, ErrorKind, Packet, Result, ReturnedResult, publication::InputReader,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
};
use tig_git::annex::workflow::reader;

struct Inputs {
    native: NativeInputs,
    bytes: BTreeMap<String, Vec<u8>>,
}
impl InputReader for Inputs {
    fn read(&mut self, asset: &Artifact, max: u64) -> Result<Vec<u8>> {
        if let Some(raw) = self.bytes.get(&asset.logical_id) {
            return Ok(raw.clone());
        }
        let raw = self.native.read(asset, max)?;
        self.bytes.insert(asset.logical_id.clone(), raw.clone());
        Ok(raw)
    }
}
pub(crate) struct Generated {
    _inputs: NativeInputs,
    bound: reel_workflow::issue::BoundIssue,
    pub(crate) receipt: reel_publication::comic_issue_source::AssemblyReceipt,
    pub(crate) files: BTreeMap<String, Vec<u8>>,
}
fn source(packet: &Packet, inputs: &Inputs, path: &str, max: u64) -> anyhow::Result<Vec<u8>> {
    let tool = packet
        .task
        .tool
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("issue tool required"))?;
    let id = if path == "request.json" {
        &tool.request.logical_id
    } else {
        tool.source_bindings
            .get(path)
            .ok_or_else(|| anyhow::anyhow!("declared issue source required"))?
    };
    let raw = inputs
        .bytes
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("verified issue source required"))?;
    anyhow::ensure!(raw.len() as u64 <= max, "issue source bound");
    Ok(raw.clone())
}
pub(crate) fn generate(packet: &Packet, native: NativeInputs) -> Result<Generated> {
    let mut inputs = Inputs {
        native,
        bytes: BTreeMap::new(),
    };
    let bound = reel_workflow::issue::bind(packet, &mut inputs)?;
    let raw = source(
        packet,
        &inputs,
        "request.json",
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )
    .map_err(|_| error(ErrorKind::Integrity, "issue-request-closure"))?;
    let (receipt, files) = reel_publication::comic_issue::capture_with(&raw, |path, max| {
        source(packet, &inputs, path, max)
    })
    .map_err(|_| error(ErrorKind::Integrity, "issue-worker-production"))?;
    Ok(Generated {
        _inputs: inputs.native,
        bound,
        receipt,
        files,
    })
}
fn receipt_pin(result: &ReturnedResult) -> Result<&Artifact> {
    result
        .worker_receipt
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Missing, "issue-worker-receipt-required"))
}
fn receipt(generated: &Generated, result: &ReturnedResult, raw: &[u8]) -> Result<()> {
    let pin = receipt_pin(result)?;
    if pin.bytes != raw.len() as u64
        || pin.sha256 != reel_workflow::digest(raw)
        || generated
            .files
            .get("receipt.json")
            .is_none_or(|expected| expected != raw)
    {
        return Err(error(ErrorKind::Integrity, "issue-worker-receipt-replay"));
    }
    reel_workflow::issue::check_receipt(&generated.bound, result, raw)
}
pub(crate) fn check_returned(
    packet: &Packet,
    native: NativeInputs,
    result: &ReturnedResult,
    root: &std::path::Path,
    paths: &BTreeMap<String, String>,
) -> Result<(Vec<File>, Vec<reel_assets::jobs::WorkspaceDirectory>)> {
    let generated = generate(packet, native)?;
    let (raw, held) =
        crate::staging::read_held(&root.join("receipt.json"), receipt_pin(result)?.bytes)?;
    receipt(&generated, result, &raw)?;
    let mut files = vec![held];
    let mut directories = Vec::new();
    let mut expected_files = BTreeSet::new();
    for (name, expected) in &generated.files {
        let path = if result.artifacts.contains_key(name) {
            paths
                .get(name)
                .ok_or_else(|| error(ErrorKind::Missing, "issue-return-output-path"))?
        } else {
            name
        };
        if !expected_files.insert(path.clone()) {
            return Err(error(ErrorKind::Conflict, "issue-return-path-alias"));
        }
        let target = root.join(path);
        let parent = target
            .parent()
            .ok_or_else(|| error(ErrorKind::Invalid, "issue-return-parent"))?;
        directories.push(
            reel_assets::jobs::open_workspace_directory(parent)
                .map_err(|_| error(ErrorKind::Denied, "issue-return-directory"))?,
        );
        let (raw, held) = crate::staging::read_held(&target, expected.len() as u64)?;
        if raw != *expected {
            return Err(error(ErrorKind::Integrity, "issue-return-replay"));
        }
        files.push(held);
    }
    let mut actual = BTreeSet::new();
    crate::comic::inventory(root, root, 0, &mut actual, &mut directories, &mut 0)?;
    if actual != expected_files {
        return Err(error(ErrorKind::Conflict, "issue-return-file-set"));
    }
    Ok((files, directories))
}
pub(crate) struct Proof {
    _generated: Generated,
    _outputs: Vec<reader::VerifiedRead>,
    pub(crate) files: Option<BTreeMap<String, Vec<u8>>>,
}
pub(crate) fn native_proof(
    packet: &Packet,
    profile: &NativeProfile,
    result: &ReturnedResult,
    capture: bool,
    permit: InputPermit,
) -> Result<Proof> {
    let native = NativeInputs::from_permit(packet, profile, permit)?;
    let generated = generate(packet, native)?;
    let mut held = Vec::new();
    for (name, asset) in std::iter::once(("receipt.json", receipt_pin(result)?))
        .chain(result.artifacts.iter().map(|(name, a)| (name.as_str(), a)))
    {
        let object = tig_core::Object {
            sha256: tig_core::Digest::parse(&asset.sha256)
                .map_err(|_| error(ErrorKind::Invalid, "issue-native-object"))?,
            bytes: asset.bytes,
        };
        let mut guard = reader::open(&profile.source, &object).map_err(io_error)?;
        let mut raw = Vec::new();
        Read::by_ref(&mut guard)
            .take(asset.bytes + 1)
            .read_to_end(&mut raw)
            .map_err(io_error)?;
        if raw.len() as u64 != asset.bytes
            || reel_workflow::digest(&raw) != asset.sha256
            || generated
                .files
                .get(name)
                .is_none_or(|expected| expected != &raw)
        {
            return Err(error(ErrorKind::Integrity, "issue-native-replay"));
        }
        if name == "receipt.json" {
            receipt(&generated, result, &raw)?;
        }
        held.push(guard);
    }
    let files = capture.then(|| generated.files.clone());
    Ok(Proof {
        _generated: generated,
        _outputs: held,
        files,
    })
}
