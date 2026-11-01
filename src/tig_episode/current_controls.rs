//! Deterministic current presentation controls; historical receipts stay intact.
use super::*;
use crate::tig_scene_production::control_snapshot::frame;
use crate::{
    scene_delivery::{FileRef, InputFiles},
    tig_presentation::jobs::VerifiedPresentation,
};
use std::path::PathBuf;

pub(super) struct CurrentControls {
    pub baseline: Object,
    pub reference: FileRef,
    pub raw: Vec<u8>,
    pub metadata: BTreeMap<Object, Vec<u8>>,
    pub storage: BTreeMap<Object, Vec<u8>>,
    pub root: Object,
    pub scene_production: BTreeMap<String, FileRef>,
}
impl CurrentControls {
    pub fn compile(
        source: &mut Source,
        plan: &Plan,
        proofs: &BTreeMap<String, &VerifiedPresentation<'_>>,
        scenes: &BTreeMap<String, CurrentSceneControls>,
        references: &BTreeMap<
            String,
            &crate::tig_scene_production::jobs::reference::VerifiedScene<'_>,
        >,
    ) -> Result<Self> {
        let raw = source.read_metadata(&plan.conform, 4 * 1024 * 1024)?;
        let mut value = identity::parse_json(&raw)?;
        let mut metadata = BTreeMap::new();
        let mut total = raw.len();
        let mut scene_production = BTreeMap::new();
        // Top-level controls are real resource obligations, not an opaque receipt.
        for key in [
            "catalog",
            "master_template_definition",
            "episode",
            "season_bindings",
            "episode_bindings",
            "source_master_template",
        ] {
            if let Some(pin) = value.get(key).filter(|v| !v.is_null()) {
                let file: FileRef = serde_json::from_value(pin.clone())?;
                crate::scene_delivery::control_name(&file.path)?;
                let object = Object {
                    sha256: Digest::parse(&file.sha256)?,
                    bytes: file.bytes,
                };
                if key == "episode" && object != plan.episode_selected {
                    bail!("parent episode control differs from authoritative source");
                }
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    metadata.entry(object.clone())
                {
                    let body = source.read_metadata(&object, 4 * 1024 * 1024)?;
                    total = total
                        .checked_add(body.len())
                        .context("current parent metadata overflow")?;
                    if total > 16 * 1024 * 1024 {
                        bail!("current parent metadata exceeds budget");
                    }
                    entry.insert(body);
                }
            }
        }
        let segments = value["segments"]
            .as_array_mut()
            .context("parent segments missing")?;
        if segments.len() != plan.components.len() {
            bail!("current parent segment coverage differs");
        }
        for (segment, component) in segments.iter_mut().zip(&plan.components) {
            if segment["id"].as_str() != Some(component.segment_id.as_str()) {
                bail!("current parent segment order differs");
            }
            if component.kind == "scene" {
                if let Some(proof) = references.get(&component.segment_id) {
                    let raw = proof.declaration()?;
                    let object = Object::from_bytes(&raw);
                    metadata.insert(object.clone(), raw);
                    let name = Digest::blob(component.segment_id.as_bytes());
                    segment["master"] = serde_json::json!({"path":format!("current/{}/master.mkv",name.as_str()),"sha256":component.selected.sha256,"bytes":component.selected.bytes});
                    segment["source_receipt"] = serde_json::json!({"path":format!("current/{}/reference.json",name.as_str()),"sha256":object.sha256,"bytes":object.bytes});
                    let fields = segment
                        .as_object_mut()
                        .context("invalid reference scene segment")?;
                    for key in [
                        "adoption_manifest",
                        "delivery_job",
                        "delivery_receipt",
                        "master_proof",
                        "master_proof_validator",
                        "semantic_delivery",
                        "imported_source_manifest",
                        "imported_source_root",
                    ] {
                        fields.remove(key);
                    }
                    continue;
                }
                if let Some(controls) = scenes.get(&component.segment_id) {
                    metadata.extend(controls.metadata.clone());
                    scene_production
                        .insert(component.segment_id.clone(), controls.production.clone());
                    let name = Digest::blob(component.segment_id.as_bytes());
                    segment["master"] = serde_json::json!({"path":format!("current/{}/master.mkv",name.as_str()),"sha256":component.selected.sha256,"bytes":component.selected.bytes});
                    segment["source_receipt"] = serde_json::to_value(&controls.receipt)?;
                    segment["delivery_receipt"] = serde_json::to_value(&controls.receipt)?;
                    segment["delivery_job"] = serde_json::to_value(&controls.job)?;
                    segment["semantic_delivery"] = serde_json::to_value(&controls.semantic)?;
                    let fields = segment
                        .as_object_mut()
                        .context("invalid native scene segment")?;
                    for key in [
                        "imported_source_manifest",
                        "imported_source_root",
                        "adoption_manifest",
                        "master_proof",
                        "master_proof_validator",
                    ] {
                        fields.remove(key);
                    }
                    continue;
                }
                if component.changed_from_baseline {
                    bail!("changed scene requires current native scene controls");
                }
                continue;
            }
            let proof = proofs
                .get(&component.segment_id)
                .context("current presentation proof missing")?;
            if proof.output() != &component.record
                || proof.checkpoint() != &plan.source_checkpoint
                || proof.selected().sha256 != component.selected.sha256.as_str()
                || proof.selected().bytes != component.selected.bytes
            {
                bail!("current parent presentation proof differs");
            }
            let receipt = proof.declaration()?;
            let object = Object::from_bytes(&receipt);
            metadata.insert(object.clone(), receipt);
            let name = Digest::blob(component.segment_id.as_bytes());
            let name = name.as_str();
            segment["master"] = serde_json::json!({"path":format!("current/{name}/master.mkv"),"sha256":component.selected.sha256,"bytes":component.selected.bytes});
            segment["source_receipt"] = serde_json::json!({"path":format!("current/{name}/reference.json"),"sha256":object.sha256,"bytes":object.bytes});
            // A fresh primary-reference proof is not the historical adoption chain.
            let fields = segment.as_object_mut().context("invalid native segment")?;
            for key in [
                "adoption_manifest",
                "delivery_job",
                "delivery_receipt",
                "master_proof",
                "master_proof_validator",
                "semantic_delivery",
                "imported_source_manifest",
                "imported_source_root",
            ] {
                fields.remove(key);
            }
        }
        let raw = identity::canonical_json(&value)?;
        if raw.len() > 4 * 1024 * 1024 {
            bail!("compiled parent controls exceed budget");
        }
        let object = Object::from_bytes(&raw);
        metadata.insert(object.clone(), raw.clone());
        if metadata
            .values()
            .try_fold(0usize, |sum, body| sum.checked_add(body.len()))
            .is_none_or(|bytes| bytes > 16 * 1024 * 1024)
        {
            bail!("compiled parent metadata exceeds total budget");
        }
        let (root, storage) = frame(&metadata)?;
        Ok(Self {
            baseline: plan.conform.clone(),
            reference: FileRef {
                path: "manifest.json".into(),
                sha256: object.sha256.as_str().into(),
                bytes: object.bytes,
            },
            raw,
            metadata,
            storage,
            root,
            scene_production,
        })
    }
}

#[cfg(test)]
#[path = "current_controls_tests.rs"]
mod tests;

pub(super) struct CurrentParent<'a> {
    pub baseline: &'a ParentControls,
    pub compiled: &'a CurrentControls,
}
impl InputFiles for CurrentParent<'_> {
    fn verified_path(&self, reference: &FileRef) -> Result<PathBuf> {
        self.baseline.verified_path(reference)
    }
    fn control_bytes(&self, reference: &FileRef) -> Result<Vec<u8>> {
        crate::scene_delivery::control_name(&reference.path)?;
        let object = Object {
            sha256: Digest::parse(&reference.sha256)?,
            bytes: reference.bytes,
        };
        if reference.path == self.compiled.reference.path {
            if reference != &self.compiled.reference {
                bail!("derived parent manifest pin differs");
            }
            return Ok(self.compiled.raw.clone());
        }
        if let Some(raw) = self.compiled.metadata.get(&object) {
            return Ok(raw.clone());
        }
        self.baseline.control_bytes(reference)
    }
}
