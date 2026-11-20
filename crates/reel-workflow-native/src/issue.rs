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
    bound: Option<reel_workflow::issue::BoundIssue>,
    pub(crate) outputs: reel_workflow::ArtifactSet,
    pub(crate) files: BTreeMap<String, Vec<u8>>,
    resolution: Option<crate::resolution::Proposal>,
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
    if packet
        .task
        .tool
        .as_ref()
        .is_some_and(|t| t.capability == reel_workflow::ToolCapability::RepairResolution)
    {
        let mut native = native;
        let generated = crate::resolution_job::generate(packet, &mut native)?;
        return Ok(Generated {
            _inputs: native,
            bound: None,
            outputs: generated.outputs,
            files: generated.files,
            resolution: Some(generated.checked),
        });
    }
    let origins = if packet
        .task
        .tool
        .as_ref()
        .is_some_and(|t| t.capability == reel_workflow::ToolCapability::ComicReviewControl)
    {
        Some(native.review_origins()?.clone())
    } else {
        None
    };
    let mut inputs = Inputs {
        native,
        bytes: BTreeMap::new(),
    };
    if let Some(origins) = origins {
        let parents = origins
            .parents
            .iter()
            .map(|(id, p)| {
                (
                    id.clone(),
                    reel_workflow::upstream::Parent {
                        packet: &p.packet,
                        result: &p.result,
                    },
                )
            })
            .collect();
        let generated = reel_workflow::review_control::generate(
            packet,
            &origins.bindings,
            &parents,
            &mut inputs,
        )?;
        return Ok(Generated {
            _inputs: inputs.native,
            bound: None,
            outputs: generated.outputs,
            files: generated.files,
            resolution: None,
        });
    }
    if packet
        .task
        .tool
        .as_ref()
        .is_some_and(|t| t.capability == reel_workflow::ToolCapability::FilmSceneControl)
    {
        let g = reel_workflow::film::generate(packet, &mut inputs)?;
        return Ok(Generated {
            _inputs: inputs.native,
            bound: None,
            outputs: g.outputs,
            files: g.files,
            resolution: None,
        });
    }
    if packet
        .task
        .tool
        .as_ref()
        .is_some_and(|t| t.capability == reel_workflow::ToolCapability::ComicIssueControl)
    {
        let generated = reel_workflow::issue_control::generate(packet, &mut inputs)?;
        return Ok(Generated {
            _inputs: inputs.native,
            bound: None,
            outputs: generated.outputs,
            files: generated.files,
            resolution: None,
        });
    }
    let bound = reel_workflow::issue::bind(packet, &mut inputs)?;
    if let Some(expected) = bound.composition_origins() {
        let origins = inputs.native.review_origins()?;
        let actual = origins.bound(packet)?;
        check_composition_origins(expected, &actual)?;
    }
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
        bound: Some(bound),
        outputs: receipt.outputs,
        files,
        resolution: None,
    })
}
fn check_composition_origins(
    expected: &reel_workflow::issue::CompositionOrigins,
    actual: &BTreeMap<String, reel_workflow::upstream::BoundInput>,
) -> Result<()> {
    let parents = actual
        .values()
        .map(|a| a.reference.task_id.clone())
        .collect::<BTreeSet<_>>();
    if parents != expected.parents
        || expected.inputs.iter().any(|(id, reference)| {
            actual
                .get(id)
                .is_none_or(|bound| &bound.reference != reference)
        })
    {
        return Err(error(
            ErrorKind::Conflict,
            "issue-composition-original-producers",
        ));
    }
    Ok(())
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
    if let Some(bound) = &generated.bound {
        reel_workflow::issue::check_receipt(bound, result, raw)
    } else {
        // Both the receipt and every output must equal deterministic replay.
        // check_result independently enforces the pinned tool provenance.
        if result.artifacts != generated.outputs {
            return Err(error(
                ErrorKind::Conflict,
                "issue-control-result-correspondence",
            ));
        }
        Ok(())
    }
}
pub(crate) fn check_returned(
    packet: &Packet,
    native: NativeInputs,
    result: &ReturnedResult,
    root: &std::path::Path,
    paths: &BTreeMap<String, String>,
) -> Result<(Vec<File>, Vec<reel_assets::jobs::WorkspaceDirectory>)> {
    reel_workflow::check_result(packet, result)?;
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
impl Proof {
    pub(crate) fn resolution(&self) -> Option<&crate::resolution::Proposal> {
        self._generated.resolution.as_ref()
    }
}
pub(crate) fn native_proof(
    packet: &Packet,
    profile: &NativeProfile,
    result: &ReturnedResult,
    capture: bool,
    permit: InputPermit,
) -> Result<Proof> {
    reel_workflow::check_result(packet, result)?;
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

#[cfg(test)]
mod composition_tests {
    use super::*;
    use reel_workflow::upstream::{BoundInput, Reference, Selector};

    #[test]
    fn composition_requires_original_receipt_and_bundle_producers_and_exact_parent_set() {
        let receipt = Reference {
            task_id: "draw-opening".into(),
            artifact: Selector::WorkerReceipt,
        };
        let bundle = Reference {
            task_id: "draw-opening".into(),
            artifact: Selector::Output {
                name: "editable.packet".into(),
            },
        };
        let expected = reel_workflow::issue::CompositionOrigins {
            parents: BTreeSet::from(["draw-opening".into(), "assembly-control".into()]),
            inputs: BTreeMap::from([
                ("receipt".into(), receipt.clone()),
                ("bundle".into(), bundle.clone()),
            ]),
        };
        let input = |reference| BoundInput {
            reference,
            artifact: Artifact {
                logical_id: "synthetic".into(),
                sha256: "a".repeat(64),
                bytes: 1,
                media_type: "application/json".into(),
            },
            result_sha256: "b".repeat(64),
        };
        // Frozen::bound separately verifies artifacts against original parent
        // packets/results. This unit check covers the editorial origin matcher.
        let actual = BTreeMap::from([
            ("receipt".into(), input(receipt)),
            ("bundle".into(), input(bundle)),
            (
                "request".into(),
                input(Reference {
                    task_id: "assembly-control".into(),
                    artifact: Selector::Output {
                        name: "assembly.json".into(),
                    },
                }),
            ),
        ]);
        check_composition_origins(&expected, &actual).unwrap();
        let mut wrong = actual.clone();
        wrong.get_mut("receipt").unwrap().reference.task_id = "different-drawing".into();
        assert!(check_composition_origins(&expected, &wrong).is_err());
        let mut wrong = actual.clone();
        wrong.get_mut("receipt").unwrap().reference.artifact = Selector::Output {
            name: "receipt.json".into(),
        };
        assert!(check_composition_origins(&expected, &wrong).is_err());
        let mut wrong = actual.clone();
        wrong.get_mut("bundle").unwrap().reference.artifact = Selector::Output {
            name: "reading.pdf".into(),
        };
        assert!(check_composition_origins(&expected, &wrong).is_err());
        let mut wrong = actual.clone();
        wrong.remove("bundle");
        assert!(check_composition_origins(&expected, &wrong).is_err());
        let mut wrong = actual;
        wrong.remove("request");
        assert!(check_composition_origins(&expected, &wrong).is_err());
    }
}
