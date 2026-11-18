//! Exact native comic source binding plus deterministic byte replay. A supplied
//! worker executable descriptor is provenance, not an authenticated execution.
use crate::{NativeInputs, NativeProfile, error, io_error};
use reel_workflow::{
    Artifact, ErrorKind, Packet, Result, ReturnedResult,
    publication::{self, InputReader},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::Path,
};
use tig_git::annex::workflow::reader;

struct Inputs {
    native: NativeInputs,
    bytes: BTreeMap<String, Vec<u8>>,
}
impl InputReader for Inputs {
    fn read(&mut self, artifact: &Artifact, max: u64) -> Result<Vec<u8>> {
        if let Some(raw) = self.bytes.get(&artifact.logical_id) {
            return Ok(raw.clone());
        }
        let raw = self.native.read(artifact, max)?;
        self.bytes.insert(artifact.logical_id.clone(), raw.clone());
        Ok(raw)
    }
}
pub(crate) struct Proof {
    _inputs: NativeInputs,
    _outputs: Vec<reader::VerifiedRead>,
    pub(crate) files: Option<BTreeMap<String, Vec<u8>>>,
}
fn pin(result: &ReturnedResult) -> Result<&Artifact> {
    result
        .worker_receipt
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Missing, "comic-worker-receipt-required"))
}
fn checked_raw(artifact: &Artifact, raw: &[u8]) -> Result<()> {
    if raw.len() as u64 != artifact.bytes || reel_workflow::digest(raw) != artifact.sha256 {
        return Err(error(ErrorKind::Integrity, "comic-worker-payload-pin"));
    }
    Ok(())
}
fn source(packet: &Packet, inputs: &Inputs, path: &str) -> anyhow::Result<Vec<u8>> {
    let tool = packet
        .task
        .tool
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("comic tool required"))?;
    let id = if path == "request.json" {
        &tool.request.logical_id
    } else {
        tool.source_bindings
            .get(path)
            .ok_or_else(|| anyhow::anyhow!("declared source required"))?
    };
    inputs
        .bytes
        .get(id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("verified source required"))
}
fn bind(packet: &Packet, profile: &NativeProfile) -> Result<(Inputs, publication::BoundComic)> {
    let mut inputs = Inputs {
        native: NativeInputs::open(packet, profile)?,
        bytes: BTreeMap::new(),
    };
    let bound = publication::bind_comic(packet, &mut inputs)?;
    Ok((inputs, bound))
}
pub(crate) struct Generated {
    _inputs: NativeInputs,
    pub(crate) receipt: reel_publication::comic_source::Receipt,
    pub(crate) files: BTreeMap<String, Vec<u8>>,
}
pub(crate) fn generate(packet: &Packet, profile: &NativeProfile) -> Result<Generated> {
    let (inputs, _bound) = bind(packet, profile)?;
    let request = source(packet, &inputs, "request.json")
        .map_err(|_| error(ErrorKind::Integrity, "comic-request-closure"))?;
    let (receipt, files) =
        reel_publication::comic::capture_with(&request, |pin| source(packet, &inputs, &pin.path))
            .map_err(|_| error(ErrorKind::Integrity, "comic-worker-production"))?;
    Ok(Generated {
        _inputs: inputs.native,
        receipt,
        files,
    })
}
fn inventory(
    root: &Path,
    at: &Path,
    depth: usize,
    names: &mut BTreeSet<String>,
    directories: &mut Vec<reel_assets::jobs::WorkspaceDirectory>,
    count: &mut usize,
) -> Result<()> {
    if depth > 16 {
        return Err(error(ErrorKind::Budget, "comic-return-depth"));
    }
    for entry in std::fs::read_dir(at).map_err(io_error)? {
        let path = entry.map_err(io_error)?.path();
        *count += 1;
        if *count > 1024 {
            return Err(error(ErrorKind::Budget, "comic-return-entries"));
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(io_error)?;
        #[cfg(windows)]
        let linked = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let linked = metadata.file_type().is_symlink();
        if linked {
            return Err(error(ErrorKind::Denied, "comic-return-reparse"));
        }
        if metadata.is_dir() {
            directories.push(
                reel_assets::jobs::open_workspace_directory(&path)
                    .map_err(|_| error(ErrorKind::Denied, "comic-return-directory"))?,
            );
            inventory(root, &path, depth + 1, names, directories, count)?;
        } else if metadata.is_file() {
            names.insert(
                path.strip_prefix(root)
                    .ok()
                    .and_then(|p| p.to_str())
                    .ok_or_else(|| error(ErrorKind::Invalid, "comic-return-name"))?
                    .replace('\\', "/"),
            );
        } else {
            return Err(error(ErrorKind::Denied, "comic-return-ordinary-file"));
        }
    }
    Ok(())
}
/// Reads actual copied source files as well as all returned render outputs.
/// Native read gates drop before the caller admits new native protection.
pub(crate) fn check_returned(
    packet: &Packet,
    profile: &NativeProfile,
    result: &ReturnedResult,
    root: &std::path::Path,
    paths: &BTreeMap<String, String>,
) -> Result<(Vec<File>, Vec<reel_assets::jobs::WorkspaceDirectory>)> {
    let (inputs, bound) = bind(packet, profile)?;
    let (receipt, held) =
        crate::staging::read_held(&root.join("receipt.json"), pin(result)?.bytes)?;
    checked_raw(pin(result)?, &receipt)?;
    publication::check_comic_receipt(&bound, result, &receipt)?;
    let mut files = vec![held];
    let mut directories = Vec::new();
    let mut expected_files = BTreeSet::new();
    let request = source(packet, &inputs, "request.json")
        .map_err(|_| error(ErrorKind::Integrity, "comic-request-closure"))?;
    reel_publication::comic::check_with(
        &request,
        |p| source(packet, &inputs, &p.path),
        |name, max| {
            let path = if name == "receipt.json" {
                "receipt.json"
            } else if name.starts_with("sources/") {
                name
            } else {
                paths
                    .get(name)
                    .map(String::as_str)
                    .ok_or_else(|| anyhow::anyhow!("declared output required"))?
            };
            let target = root.join(path);
            expected_files.insert(path.to_owned());
            let directory = reel_assets::jobs::open_workspace_directory(
                target
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("output parent required"))?,
            )
            .map_err(|_| anyhow::anyhow!("ordinary output namespace required"))?;
            let (raw, held) = crate::staging::read_held(&target, max)
                .map_err(|_| anyhow::anyhow!("bounded output required"))?;
            files.push(held);
            directories.push(directory);
            Ok(raw)
        },
    )
    .map_err(|_| error(ErrorKind::Integrity, "comic-deterministic-replay"))?;
    let mut actual_files = BTreeSet::new();
    inventory(root, root, 0, &mut actual_files, &mut directories, &mut 0)?;
    if actual_files != expected_files {
        return Err(error(ErrorKind::Conflict, "comic-return-file-set"));
    }
    Ok((files, directories))
}

pub(crate) fn check_native(
    packet: &Packet,
    profile: &NativeProfile,
    result: &ReturnedResult,
) -> Result<Option<Proof>> {
    native_proof(packet, profile, result, false)
}
pub(crate) fn capture_native(
    packet: &Packet,
    profile: &NativeProfile,
    result: &ReturnedResult,
) -> Result<Option<Proof>> {
    native_proof(packet, profile, result, true)
}
fn native_proof(
    packet: &Packet,
    profile: &NativeProfile,
    result: &ReturnedResult,
    capture: bool,
) -> Result<Option<Proof>> {
    if result.provenance.mode != reel_workflow::Mode::Tool {
        return Ok(None);
    }
    let (inputs, bound) = bind(packet, profile)?;
    let mut held = Vec::new();
    let mut captured = BTreeMap::new();
    let mut native_read = |asset: &Artifact| -> Result<Vec<u8>> {
        let object = tig_core::Object {
            sha256: tig_core::Digest::parse(&asset.sha256)
                .map_err(|_| error(ErrorKind::Invalid, "comic-native-object"))?,
            bytes: asset.bytes,
        };
        if let Some(raw) = inputs.native.captured(&object) {
            return Ok(raw.to_vec());
        }
        if let Some(raw) = captured.get(&object) {
            return Ok(Vec::clone(raw));
        }
        let mut guard = reader::open(&profile.source, &object).map_err(io_error)?;
        let mut raw = Vec::new();
        Read::by_ref(&mut guard)
            .take(asset.bytes + 1)
            .read_to_end(&mut raw)
            .map_err(io_error)?;
        checked_raw(asset, &raw)?;
        captured.insert(object, raw.clone());
        held.push(guard);
        Ok(raw)
    };
    let receipt = native_read(pin(result)?)?;
    publication::check_comic_receipt(&bound, result, &receipt)?;
    let request = source(packet, &inputs, "request.json")
        .map_err(|_| error(ErrorKind::Integrity, "comic-request-closure"))?;
    let mut files = capture.then(BTreeMap::new);
    reel_publication::comic::check_with(
        &request,
        |p| source(packet, &inputs, &p.path),
        |name, _| {
            let raw = if name == "receipt.json" {
                receipt.clone()
            } else if let Some(path) = name.strip_prefix("sources/") {
                source(packet, &inputs, path)?
            } else {
                native_read(
                    result
                        .artifacts
                        .get(name)
                        .ok_or_else(|| anyhow::anyhow!("declared native output required"))?,
                )
                .map_err(|_| anyhow::anyhow!("native output refused"))?
            };
            if let Some(files) = &mut files {
                files.insert(name.to_owned(), raw.clone());
            }
            Ok(raw)
        },
    )
    .map_err(|_| error(ErrorKind::Integrity, "comic-native-deterministic-replay"))?;
    Ok(Some(Proof {
        _inputs: inputs.native,
        _outputs: held,
        files,
    }))
}
