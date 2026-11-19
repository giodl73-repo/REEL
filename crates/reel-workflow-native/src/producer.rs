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
        ToolCapability::ComicReviewControl => Ok(4 * reel_workflow::MAX_CONTROL_BYTES as u64),
        ToolCapability::FilmRender => {
            let bound = reel_workflow::film_render::bind(packet, inputs)?;
            Ok(bound.request().limits.output_bytes)
        }
        ToolCapability::FilmSceneControl => {
            reel_workflow::film::generate(packet, inputs)?;
            Ok(6 * reel_workflow::MAX_CONTROL_BYTES as u64)
        }
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
        ToolCapability::ComicIssueControl => {
            reel_workflow::issue_control::generate(packet, inputs)?;
            Ok(2 * reel_workflow::MAX_CONTROL_BYTES as u64)
        }
    }
}
pub(crate) enum Generated {
    #[cfg(all(windows, feature = "film"))]
    Movie(Box<crate::movie::Generated>),
    Comic(Box<crate::comic::Generated>),
    Issue(Box<crate::issue::Generated>),
}
impl Generated {
    pub(crate) fn paths(&self) -> Result<BTreeMap<String, String>> {
        let paths = match self {
            #[cfg(all(windows, feature = "film"))]
            Self::Movie(g) => g.paths.clone(),
            _ => crate::output_layout::identity(self.outputs()),
        };
        let raw = self
            .files()
            .get("receipt.json")
            .ok_or_else(|| crate::error(ErrorKind::Missing, "worker-produced-receipt"))?;
        let receipt = reel_workflow::Artifact {
            logical_id: "workflow/worker/receipt".into(),
            sha256: reel_workflow::digest(raw),
            bytes: raw.len() as u64,
            media_type: "application/json".into(),
        };
        let files = self
            .files()
            .iter()
            .map(|(p, raw)| (p.clone(), tig_core::Object::from_bytes(raw)))
            .collect();
        crate::output_layout::check(self.outputs(), &receipt, &paths, &files)?;
        Ok(paths)
    }
    pub(crate) fn files(&self) -> &BTreeMap<String, Vec<u8>> {
        match self {
            #[cfg(all(windows, feature = "film"))]
            Self::Movie(g) => &g.files,
            Self::Comic(g) => &g.files,
            Self::Issue(g) => &g.files,
        }
    }
    pub(crate) fn outputs(&self) -> &ArtifactSet {
        match self {
            #[cfg(all(windows, feature = "film"))]
            Self::Movie(g) => &g.outputs,
            Self::Comic(g) => &g.receipt.outputs,
            Self::Issue(g) => &g.outputs,
        }
    }
}
pub(crate) fn reservation_budget(packet: &Packet, inputs: &mut impl InputReader) -> Result<u64> {
    let output = output_budget(packet, inputs)?;
    let scratch = if capability(packet)? == ToolCapability::FilmRender {
        reel_workflow::film_render::bind(packet, inputs)?
            .request()
            .limits
            .workspace_bytes
    } else {
        0
    };
    output
        .checked_mul(2)
        .and_then(|n| n.checked_add(scratch))
        .and_then(|n| n.checked_add(10 * reel_workflow::MAX_CONTROL_BYTES as u64))
        .ok_or_else(|| crate::error(ErrorKind::Budget, "worker-reservation-overflow"))
}
pub(crate) fn generate(
    packet: &Packet,
    inputs: NativeInputs,
    runtime: &crate::Tool,
    programs: &BTreeMap<String, crate::Tool>,
    workspace: &Path,
) -> Result<Generated> {
    match capability(packet)? {
        ToolCapability::FilmRender => {
            #[cfg(all(windows, feature = "film"))]
            {
                match std::fs::create_dir(workspace) {
                    Ok(()) => (),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                    Err(e) => return Err(crate::io_error(e)),
                }
                crate::movie::generate_or_adopt(packet, inputs, programs, runtime, workspace)
                    .map(|g| Generated::Movie(Box::new(g)))
            }
            #[cfg(not(all(windows, feature = "film")))]
            {
                let _ = (runtime, programs, workspace);
                Err(crate::error(
                    ErrorKind::Unsupported,
                    "film-feature-required",
                ))
            }
        }
        ToolCapability::ComicRender => {
            crate::comic::generate(packet, inputs).map(|g| Generated::Comic(Box::new(g)))
        }
        ToolCapability::ComicIssueAssembly
        | ToolCapability::ComicIssueControl
        | ToolCapability::ComicReviewControl
        | ToolCapability::FilmSceneControl => {
            crate::issue::generate(packet, inputs).map(|g| Generated::Issue(Box::new(g)))
        }
    }
}
pub(crate) fn check_returned(
    packet: &Packet,
    source: &NativeProfile,
    inputs: NativeInputs,
    result: &ReturnedResult,
    root: &Path,
    paths: &BTreeMap<String, String>,
) -> Result<(Vec<File>, Vec<reel_assets::jobs::WorkspaceDirectory>)> {
    match capability(packet)? {
        ToolCapability::FilmRender => {
            #[cfg(all(windows, feature = "film"))]
            {
                crate::movie::check_returned_owned(packet, source, inputs, result, root, paths)
            }
            #[cfg(not(all(windows, feature = "film")))]
            {
                let _ = source;
                Err(crate::error(
                    ErrorKind::Unsupported,
                    "film-feature-required",
                ))
            }
        }
        ToolCapability::ComicRender => {
            crate::comic::check_returned(packet, inputs, result, root, paths)
        }
        ToolCapability::ComicIssueAssembly
        | ToolCapability::ComicIssueControl
        | ToolCapability::ComicReviewControl
        | ToolCapability::FilmSceneControl => {
            crate::issue::check_returned(packet, inputs, result, root, paths)
        }
    }
}
pub(crate) enum Proof {
    Review(Box<crate::review::Proof>),
    #[cfg(all(windows, feature = "film"))]
    Movie(Box<crate::movie::Proof>),
    Comic(Box<crate::comic::Proof>),
    Issue(Box<crate::issue::Proof>),
}
impl Proof {
    pub(crate) fn files(&self) -> Option<&BTreeMap<String, Vec<u8>>> {
        match self {
            Self::Review(_) => None,
            #[cfg(all(windows, feature = "film"))]
            Self::Movie(p) => p.files.as_ref(),
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
    if packet.task.operation == reel_workflow::Operation::ReviewReport {
        return crate::review::verify(packet, source, result, permit)
            .map(|proof| Some(Proof::Review(Box::new(proof))));
    }
    if result.provenance.mode != reel_workflow::Mode::Tool {
        return Ok(None);
    }
    match capability(packet)? {
        ToolCapability::FilmRender => {
            #[cfg(all(windows, feature = "film"))]
            {
                crate::movie::native_proof(packet, source, result, capture, permit)
                    .map(|p| Some(Proof::Movie(Box::new(p))))
            }
            #[cfg(not(all(windows, feature = "film")))]
            {
                Err(crate::error(
                    ErrorKind::Unsupported,
                    "film-feature-required",
                ))
            }
        }
        ToolCapability::ComicRender => if capture {
            crate::comic::capture_native(packet, source, result, permit)
        } else {
            crate::comic::check_native(packet, source, result, permit)
        }
        .map(|p| p.map(|p| Proof::Comic(Box::new(p)))),
        ToolCapability::ComicIssueAssembly
        | ToolCapability::ComicIssueControl
        | ToolCapability::ComicReviewControl
        | ToolCapability::FilmSceneControl => {
            crate::issue::native_proof(packet, source, result, capture, permit)
                .map(|p| Some(Proof::Issue(Box::new(p))))
        }
    }
}
