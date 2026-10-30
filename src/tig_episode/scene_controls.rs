//! Current native scene metadata compiled from its matched job, not an old conform.
use super::{CurrentScene, Source};
use crate::tig_scene_production::control_snapshot::{body, capture};
use crate::{
    cue_relative::Contract,
    scene_delivery::{self, FileRef, InputFiles, Job},
    semantic_delivery::SemanticDelivery,
    tig_scene_outputs::Bundle,
    tig_scene_production::jobs::JobPlan,
};
use anyhow::{Context, Result, bail};
use std::{collections::BTreeMap, path::Path};
use tig_assets::graph::Reference;
use tig_core::{Digest, Object};

/// Sealed metadata correspondence. The parent still needs live child ownership,
/// guarded input/output providers and the native full delivery check at assembly.
pub struct CurrentSceneControls {
    pub(super) checkpoint: Digest,
    pub(super) output: Reference,
    pub(super) selected: Object,
    pub(super) claim: Object,
    pub(super) protection_recipe: Digest,
    pub(super) segment: String,
    pub(super) language: String,
    pub(super) job: FileRef,
    pub(super) semantic: FileRef,
    pub(super) production: FileRef,
    pub(super) receipt: FileRef,
    pub(super) metadata: BTreeMap<Object, Vec<u8>>,
}
impl CurrentSceneControls {
    /// Compile only into a new private preparation directory. No render, cache
    /// publication, approval, custody or owner adoption occurs here.
    pub fn compile(
        source: &mut Source,
        proof: &CurrentScene,
        current: &JobPlan<'_>,
        outputs: &Bundle,
        preparation: &Path,
    ) -> Result<Self> {
        if proof.checkpoint() != source.checkpoint()
            || !proof.matches_job(current)
            || source.record(&proof.output().id)?.reference() != proof.output()
            || source.record(&proof.scene().id)?.reference() != proof.scene()
        {
            bail!("scene control compilation differs from current correspondence");
        }
        let master = outputs
            .receipt()
            .outputs
            .get("master.mkv")
            .context("current scene receipt lacks master")?;
        if master.sha256 != proof.selected().sha256.as_str()
            || master.bytes != proof.selected().bytes
        {
            bail!("scene control receipt differs from selected output");
        }
        current.compile_parent_controls(source, preparation)?;
        let segment = source
            .record(&proof.scene().id)?
            .authored()
            .get("scene_id")
            .and_then(serde_json::Value::as_str)
            .context("native scene ID missing")?
            .to_owned();
        let prefix = format!("current/{}/", Digest::blob(segment.as_bytes()).as_str());
        let mut metadata = BTreeMap::new();
        let mut total = 0u64;
        let job_ref = capture(
            preparation,
            Path::new("delivery/job.json"),
            &prefix,
            &mut metadata,
            &mut total,
        )?;
        let job: Job = serde_json::from_slice(body(&metadata, &job_ref)?)?;
        if outputs.receipt().plan.job_sha256 != job_ref.sha256 {
            bail!("retained scene delivery differs from freshly compiled current job");
        }
        let contract_ref = scene_delivery::scoped_control(&job_ref, &job.contract)?;
        let captured = capture(
            preparation,
            contract_ref.path.strip_prefix(&prefix)?,
            &prefix,
            &mut metadata,
            &mut total,
        )?;
        if captured != contract_ref {
            bail!("compiled scene contract differs from current job");
        }
        let contract: Contract = serde_json::from_slice(body(&metadata, &contract_ref)?)?;
        let production_path = contract_ref
            .path
            .parent()
            .context("scene contract parent missing")?
            .join(scene_delivery::control_name(&contract.production_manifest)?);
        let production = capture(
            preparation,
            production_path.strip_prefix(&prefix)?,
            &prefix,
            &mut metadata,
            &mut total,
        )?;
        if production.sha256 != job.production_manifest_sha256 {
            bail!("compiled scene production differs from current job");
        }
        let semantic = capture(
            preparation,
            Path::new("delivery/semantic-delivery.json"),
            &prefix,
            &mut metadata,
            &mut total,
        )?;
        let scope: SemanticDelivery = serde_json::from_slice(body(&metadata, &semantic)?)?;
        if scope.scene_id.as_deref() != Some(segment.as_str())
            || scope.language.as_deref() != Some(current.native_request().language.as_str())
            || scene_delivery::scoped_control(&semantic, &scope.scene_delivery_job)? != job_ref
        {
            bail!("compiled scene semantic scope differs");
        }
        let mut receipt = outputs.receipt_ref().clone();
        let raw = outputs.control_bytes(&receipt)?;
        let object = Object::from_bytes(&raw);
        if object.sha256.as_str() != receipt.sha256 || object.bytes != receipt.bytes {
            bail!("current scene receipt changed");
        }
        if total
            .checked_add(object.bytes)
            .is_none_or(|bytes| bytes > 16 * 1024 * 1024)
        {
            bail!("scene controls and receipt exceed metadata budget");
        }
        receipt.path = format!("{prefix}receipt.json").into();
        metadata.insert(object, raw);
        Ok(Self {
            checkpoint: proof.checkpoint().clone(),
            output: proof.output().clone(),
            selected: proof.selected().clone(),
            claim: proof.claim().clone(),
            protection_recipe: proof.protection_recipe().clone(),
            segment,
            language: current.native_request().language.clone(),
            job: job_ref,
            semantic,
            production,
            receipt,
            metadata,
        })
    }
}
