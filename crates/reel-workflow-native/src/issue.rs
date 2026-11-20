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
#[cfg(test)]
#[path = "issue_review_test.rs"]
mod review_control_test;

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
    layered: Option<reel_workflow::layered::Bound>,
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
            layered: None,
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
        let generated = reel_workflow::review_control::generate_with_issue(
            packet,
            &origins.bindings,
            &parents,
            &mut inputs,
            capture_review_issue,
        )?;
        return Ok(Generated {
            _inputs: inputs.native,
            bound: None,
            layered: None,
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
            layered: None,
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
        if let Some(expected) = &generated.composition_origins {
            let origins = inputs.native.review_origins()?;
            check_composition_origins(expected, &origins.bound(packet)?)?;
        }
        return Ok(Generated {
            _inputs: inputs.native,
            bound: None,
            layered: None,
            outputs: generated.outputs,
            files: generated.files,
            resolution: None,
        });
    }
    if packet
        .task
        .tool
        .as_ref()
        .is_some_and(|t| t.capability == reel_workflow::ToolCapability::LayeredComicCompose)
    {
        let generated = reel_workflow::layered::generate(packet, &mut inputs)?;
        return Ok(Generated {
            _inputs: inputs.native,
            bound: None,
            layered: Some(generated.bound),
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
        layered: None,
        outputs: receipt.outputs,
        files,
        resolution: None,
    })
}
/// Reconstruct the complete parent issue from its declared exact source bytes.
/// The child's native admission already froze and verified this parent's full
/// retained producer lineage; this callback adds deterministic byte replay.
fn capture_review_issue(
    parent: &Packet,
    result: &ReturnedResult,
    reader: &mut impl InputReader,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let tool = parent
        .task
        .tool
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Missing, "review-issue-tool"))?;
    let request = reader.read(&tool.request, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    let (replayed, files) = reel_publication::comic_issue::capture_with(&request, |path, max| {
        let id = tool
            .source_bindings
            .get(path)
            .ok_or_else(|| anyhow::anyhow!("declared review issue source required"))?;
        let asset = parent
            .task
            .inputs
            .iter()
            .find(|a| &a.logical_id == id)
            .ok_or_else(|| anyhow::anyhow!("review issue source input required"))?;
        let raw = reader.read(asset, max)?;
        anyhow::ensure!(
            raw.len() as u64 == asset.bytes && reel_workflow::digest(&raw) == asset.sha256,
            "review issue source pin differs"
        );
        Ok(raw)
    })
    .map_err(|_| error(ErrorKind::Integrity, "review-issue-producer-replay"))?;
    let receipt = result
        .worker_receipt
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Missing, "review-issue-worker-receipt"))?;
    let expected: reel_publication::comic_issue_source::AssemblyReceipt =
        reel_workflow::decode(&reader.read(receipt, reel_workflow::MAX_CONTROL_BYTES as u64)?)?;
    if reel_workflow::canonical_digest(&replayed)? != reel_workflow::canonical_digest(&expected)? {
        return Err(error(ErrorKind::Integrity, "review-issue-receipt-replay"));
    }
    Ok(files)
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
    if let Some(bound) = &generated.layered {
        bound.check_receipt(result, raw)
    } else if let Some(bound) = &generated.bound {
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
    pub(crate) fn review_copy(
        &self,
        packet: &Packet,
        result: &ReturnedResult,
    ) -> Result<reel_workflow::review::Export> {
        if let Some(bound) = &self._generated.layered {
            let files = self
                .files
                .as_ref()
                .ok_or_else(|| error(ErrorKind::Unsupported, "layered-review-capture-required"))?;
            let raw = files
                .get("receipt.json")
                .ok_or_else(|| error(ErrorKind::Missing, "layered-review-receipt"))?;
            return bound.review_copy(result, raw);
        }
        let bound = self
            ._generated
            .bound
            .as_ref()
            .ok_or_else(|| error(ErrorKind::Unsupported, "issue-review-assembly-required"))?;
        let files = self
            .files
            .as_ref()
            .ok_or_else(|| error(ErrorKind::Unsupported, "issue-review-capture-required"))?;
        let raw = files
            .get("receipt.json")
            .ok_or_else(|| error(ErrorKind::Missing, "issue-review-receipt"))?;
        reel_workflow::issue::review_copy(bound, packet, result, raw, files)
    }
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
        // A layered page deliberately reuses its selected PNG. Its exact input
        // binding and bytes are already verified, and _generated keeps that
        // native gate alive. A second SDK open of the same object is refused.
        // Output identities still pass the original receipt/alias checks below.
        let (raw, guard) = if let Some(raw) = generated._inputs.captured(&object) {
            (raw.to_vec(), None)
        } else {
            let mut guard = reader::open(&profile.source, &object).map_err(io_error)?;
            let mut raw = Vec::new();
            Read::by_ref(&mut guard)
                .take(asset.bytes + 1)
                .read_to_end(&mut raw)
                .map_err(io_error)?;
            (raw, Some(guard))
        };
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
        if let Some(guard) = guard {
            held.push(guard);
        }
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

    #[cfg(windows)]
    #[test]
    #[ignore = "Read-only proof of the existing original BBJ ImportPending job; no publication, retention, new admission, runtime adoption or scratch changes"]
    fn original_layered_candidate_reuses_held_png_input_gate_during_native_output_proof() {
        let root = std::path::PathBuf::from(
            std::env::var_os("REEL_LAYERED_ORIGINAL_JOB").expect("Set original pending job"),
        );
        let profile: crate::handoff::Profile =
            crate::read_control(&root.join("profile.json")).unwrap();
        let ticket: serde_json::Value =
            crate::read_control(&root.join("execution/worker-ticket.json")).unwrap();
        let packet: Packet = serde_json::from_value(ticket["packet"].clone()).unwrap();
        assert_eq!(
            packet.task_key,
            "11b16e3e0081a5e3d134f520b12d4154dc30fc09775206debe882632bd0aa5b0"
        );
        let journal = reel_workflow::handoff::Journal::open(&root.join("journal")).unwrap();
        assert_eq!(
            journal.state(),
            reel_workflow::handoff::State::ImportPending
        );
        let result = journal.result().unwrap();
        let png = &result.artifacts["page.png"];
        assert!(
            packet
                .task
                .inputs
                .iter()
                .any(|a| a.sha256 == png.sha256 && a.bytes == png.bytes)
        );
        let before = tig_store::source_claims::protection::Index::open(
            &profile.admission.index_root,
            &profile.admission.index_minimum,
        )
        .unwrap()
        .checkpoint();
        let claims = crate::admission::Prepared::open(
            &packet,
            &profile.source,
            &profile.admission,
            &result.attempt_id,
        )
        .unwrap()
        .reopen(&profile.binding.admission)
        .unwrap();
        let proof = native_proof(
            &packet,
            &profile.source,
            result,
            false,
            claims.input_permit(),
        )
        .unwrap();
        assert!(proof._generated.layered.is_some());
        assert_eq!(
            proof._outputs.len(),
            3,
            "The PNG's original input gate remains held instead of being reopened"
        );
        assert!(
            proof
                ._generated
                ._inputs
                .captured(&tig_core::Object {
                    sha256: tig_core::Digest::parse(&png.sha256).unwrap(),
                    bytes: png.bytes
                })
                .is_some()
        );
        drop(proof);
        drop(claims);
        let after = tig_store::source_claims::protection::Index::open(
            &profile.admission.index_root,
            &profile.admission.index_minimum,
        )
        .unwrap()
        .checkpoint();
        assert_eq!(before, after);
        assert_eq!(
            reel_workflow::handoff::Journal::open(&root.join("journal"))
                .unwrap()
                .state(),
            reel_workflow::handoff::State::ImportPending
        );
    }

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
