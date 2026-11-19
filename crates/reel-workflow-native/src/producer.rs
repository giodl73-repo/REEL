//! Explicit deterministic capability dispatch. Never executes returned code.
use crate::{NativeInputs, NativeProfile, admission::InputPermit};
use reel_workflow::{
    ArtifactSet, ErrorKind, Packet, Result, ReturnedResult, ToolCapability,
    publication::InputReader,
};
use std::{collections::BTreeMap, fs::File, path::Path};

fn capability(packet: &Packet) -> Result<ToolCapability> {
    packet
        .task
        .tool
        .as_ref()
        .map(|t| t.capability)
        .ok_or_else(|| crate::error(ErrorKind::Missing, "producer-tool-required"))
}
pub(crate) fn output_budget(packet: &Packet, inputs: &mut impl InputReader) -> Result<u64> {
    match capability(packet)? {
        ToolCapability::ComicRender => {
            reel_workflow::publication::bind_comic(packet, inputs)?;
            let asset = &packet
                .task
                .tool
                .as_ref()
                .ok_or_else(|| crate::error(ErrorKind::Missing, "producer-tool-required"))?
                .request;
            let raw = inputs.read(asset, asset.bytes)?;
            let request: reel_publication::comic_source::Request = reel_workflow::decode(&raw)?;
            Ok(request.limits.output_bytes)
        }
        ToolCapability::ComicIssueAssembly => {
            Ok(reel_workflow::issue::bind(packet, inputs)?.output_bytes())
        }
    }
}
pub(crate) enum Generated {
    Comic(Box<crate::comic::Generated>),
    Issue(Box<crate::issue::Generated>),
}
impl Generated {
    pub(crate) fn files(&self) -> &BTreeMap<String, Vec<u8>> {
        match self {
            Self::Comic(g) => &g.files,
            Self::Issue(g) => &g.files,
        }
    }
    pub(crate) fn outputs(&self) -> &ArtifactSet {
        match self {
            Self::Comic(g) => &g.receipt.outputs,
            Self::Issue(g) => &g.receipt.outputs,
        }
    }
}
pub(crate) fn generate(packet: &Packet, inputs: NativeInputs) -> Result<Generated> {
    match capability(packet)? {
        ToolCapability::ComicRender => {
            crate::comic::generate(packet, inputs).map(|g| Generated::Comic(Box::new(g)))
        }
        ToolCapability::ComicIssueAssembly => {
            crate::issue::generate(packet, inputs).map(|g| Generated::Issue(Box::new(g)))
        }
    }
}
pub(crate) fn check_returned(
    packet: &Packet,
    inputs: NativeInputs,
    result: &ReturnedResult,
    root: &Path,
    paths: &BTreeMap<String, String>,
) -> Result<(Vec<File>, Vec<reel_assets::jobs::WorkspaceDirectory>)> {
    match capability(packet)? {
        ToolCapability::ComicRender => {
            crate::comic::check_returned(packet, inputs, result, root, paths)
        }
        ToolCapability::ComicIssueAssembly => {
            crate::issue::check_returned(packet, inputs, result, root, paths)
        }
    }
}
pub(crate) enum Proof {
    Comic(Box<crate::comic::Proof>),
    Issue(Box<crate::issue::Proof>),
}
impl Proof {
    pub(crate) fn files(&self) -> Option<&BTreeMap<String, Vec<u8>>> {
        match self {
            Self::Comic(p) => p.files.as_ref(),
            Self::Issue(p) => p.files.as_ref(),
        }
    }
}
pub(crate) fn check_native(
    packet: &Packet,
    source: &NativeProfile,
    result: &ReturnedResult,
    permit: InputPermit,
) -> Result<Option<Proof>> {
    native(packet, source, result, false, permit)
}
pub(crate) fn capture_native(
    packet: &Packet,
    source: &NativeProfile,
    result: &ReturnedResult,
    permit: InputPermit,
) -> Result<Option<Proof>> {
    native(packet, source, result, true, permit)
}
fn native(
    packet: &Packet,
    source: &NativeProfile,
    result: &ReturnedResult,
    capture: bool,
    permit: InputPermit,
) -> Result<Option<Proof>> {
    if result.provenance.mode != reel_workflow::Mode::Tool {
        return Ok(None);
    }
    match capability(packet)? {
        ToolCapability::ComicRender => if capture {
            crate::comic::capture_native(packet, source, result, permit)
        } else {
            crate::comic::check_native(packet, source, result, permit)
        }
        .map(|p| p.map(|p| Proof::Comic(Box::new(p)))),
        ToolCapability::ComicIssueAssembly => {
            crate::issue::native_proof(packet, source, result, capture, permit)
                .map(|p| Some(Proof::Issue(Box::new(p))))
        }
    }
}
