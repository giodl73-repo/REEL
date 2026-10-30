//! Owned independent scene verification, without an asserted historical producer.
use super::*;
use crate::{
    cue_relative::Contract,
    scene_delivery::{self, FileRef, InputFiles, Job},
    scene_primary_reference::{self, Budget, PrimaryReference},
    semantic_delivery::SemanticDelivery,
    tig_inputs::Inputs,
    tig_scene_production::control_snapshot::{body, capture},
};
use anyhow::Context;
use tig_assets::graph::Reference;

/// A current native recipe plus exact selected output and compiled controls.
/// Freeze/reopen this plan before taking source ownership. Preparation contains
/// small controls only; no rendered media or legacy receipt is required.
pub struct ReferencePlan<'a, 'n> {
    native: &'a JobPlan<'n>,
    output: Reference,
    scene: Reference,
    master: FileRef,
    job: FileRef,
    production: FileRef,
    metadata: BTreeMap<Object, Vec<u8>>,
    storage: BTreeMap<Object, Vec<u8>>,
    composition: BTreeMap<LogicalPath, Object>,
    evidence: BTreeMap<LogicalPath, Object>,
}
pub struct ReferenceReadback<'p, 'a, 'n> {
    plan: &'p ReferencePlan<'a, 'n>,
    resources: job::JobContext,
}
pub struct ProtectedReference<'p, 'a, 'n> {
    readback: ReferenceReadback<'p, 'a, 'n>,
    owner: ScopedSource,
}
/// Non-deserializable proof retaining source, runtime and primary/output guards.
/// Dropping it releases byte handles, never durable resource obligations.
pub struct VerifiedScene<'a> {
    owner: &'a ScopedSource,
    _programs: &'a BoundPrograms,
    _inputs: Inputs,
    outputs: Inputs,
    checkpoint: Digest,
    scene: Reference,
    output: Reference,
    master: FileRef,
    recipe: Digest,
    comparison: PrimaryReference,
    resources: &'a job::JobContext,
    production_definitions: std::collections::BTreeSet<String>,
}
impl VerifiedScene<'_> {
    pub(crate) fn checkpoint(&self) -> &Digest {
        &self.checkpoint
    }
    pub(crate) fn selected_path(&self) -> Result<PathBuf> {
        self.outputs.verified_path(&self.master)
    }
    pub(crate) fn source_identity(&self) -> &Key {
        self.owner.source_identity()
    }
    pub(crate) fn production_definitions(&self) -> &std::collections::BTreeSet<String> {
        &self.production_definitions
    }
    pub fn selected(&self) -> &FileRef {
        &self.master
    }
    pub fn scene(&self) -> &Reference {
        &self.scene
    }
    pub fn output(&self) -> &Reference {
        &self.output
    }
    pub fn recipe(&self) -> &Digest {
        &self.recipe
    }
    pub fn resources(&self) -> &job::JobContext {
        self.resources
    }
    pub fn declaration(&self) -> Result<Vec<u8>> {
        let (frames, samples) = self.comparison.counts();
        Ok(identity::canonical_json(&serde_json::json!({
            "schema":"reel.tig-scene-reference.experimental-1",
            "checkpoint":self.checkpoint,"scene":self.scene,"output":self.output,
            "recipe":self.recipe,"resources":self.resources.selection(),
            "comparison":self.comparison.declaration(),
            "frames":frames,"samples":samples,"timestamps_verified":true,
            "original_producer_assertion":false,"approval":false,
            "authorizes_eviction":false,
        }))?)
    }
    pub fn verify_current(&self, source: &Source, journal: &mut SourceJournal) -> Result<()> {
        if source.checkpoint() != &self.checkpoint
            || source.record(&self.scene.id)?.reference() != &self.scene
            || source.record(&self.output.id)?.reference() != &self.output
        {
            bail!("scene reference source changed");
        }
        self.owner.check(journal)?;
        self.outputs.verified_path(&self.master)?;
        Ok(())
    }
}
impl<'a, 'n> ReferencePlan<'a, 'n> {
    pub fn compile(
        source: &mut Source,
        native: &'a JobPlan<'n>,
        output_id: &str,
        output_definition: &str,
        preparation: &Path,
    ) -> Result<Self> {
        let request = native.native_request();
        let scene = source.record(&request.scene_id)?;
        let output = source.record(output_id)?;
        let segment = scene.authored()["scene_id"]
            .as_str()
            .context("scene ID missing")?;
        let authored = output.authored();
        if native.recipe().checkpoint() != source.checkpoint()
            || !native.recipe().nodes().contains_key(scene.reference())
            || output.definition().id != output_definition
            || output_definition == request.scene_definition
            || request
                .primary_definitions
                .iter()
                .any(|id| id == output_definition)
            || request
                .control_definitions
                .iter()
                .any(|id| id == output_definition)
            || !matches!(
                source.admission(output_definition)?.payload,
                crate::tig_inputs::Payload::Selected {}
            )
            || authored["scene"] != serde_json::json!({"id":request.scene_id})
            || authored["language"] != request.language
            || authored["source_segment_id"] != segment
            || authored["component_kind"] != "scene"
        {
            bail!("scene reference output differs from current native scope");
        }
        let selected = output.selected()?.clone();
        let master = FileRef {
            path: output_id.into(),
            sha256: selected.sha256.as_str().into(),
            bytes: selected.bytes,
        };
        let target = output.reference().clone();
        let definition = output.definition().object.clone();
        let scene = scene.reference().clone();
        let segment = segment.to_owned();
        native.compile_parent_controls(source, preparation)?;
        let prefix = format!("reference/{}/", Digest::blob(segment.as_bytes()).as_str());
        let mut metadata = BTreeMap::new();
        let mut total = 0;
        let job = capture(
            preparation,
            Path::new("delivery/job.json"),
            &prefix,
            &mut metadata,
            &mut total,
        )?;
        let parsed: Job = serde_json::from_slice(body(&metadata, &job)?)?;
        let contract = scene_delivery::scoped_control(&job, &parsed.contract)?;
        let captured = capture(
            preparation,
            contract.path.strip_prefix(&prefix)?,
            &prefix,
            &mut metadata,
            &mut total,
        )?;
        if captured != contract {
            bail!("scene reference contract differs");
        }
        let parsed_contract: Contract = serde_json::from_slice(body(&metadata, &contract)?)?;
        let production_path = contract
            .path
            .parent()
            .context("scene contract parent missing")?
            .join(scene_delivery::control_name(
                &parsed_contract.production_manifest,
            )?);
        let production = capture(
            preparation,
            production_path.strip_prefix(&prefix)?,
            &prefix,
            &mut metadata,
            &mut total,
        )?;
        if production.sha256 != parsed.production_manifest_sha256 {
            bail!("scene reference production differs");
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
            || scope.language.as_deref() != Some(request.language.as_str())
            || scene_delivery::scoped_control(&semantic, &scope.scene_delivery_job)? != job
        {
            bail!("scene reference semantic scope differs");
        }
        let mut evidence = BTreeMap::from([(
            LogicalPath::parse("selected-record")?,
            target.object.clone(),
        )]);
        for object in metadata.keys() {
            evidence.insert(
                LogicalPath::parse(&format!("controls/{}", object.sha256.as_str()))?,
                object.clone(),
            );
        }
        let (root, storage) = crate::tig_scene_production::control_snapshot::frame(&metadata)?;
        evidence.insert(LogicalPath::parse("compiled-controls")?, root);
        evidence.insert(LogicalPath::parse("output-definition")?, definition);
        for object in storage.keys() {
            evidence.insert(
                LogicalPath::parse(&format!("control-nodes/{}", object.sha256.as_str()))?,
                object.clone(),
            );
        }
        Ok(Self {
            storage,
            native,
            output: target,
            scene,
            master,
            job,
            production,
            metadata,
            composition: BTreeMap::from([(LogicalPath::parse("selected-master")?, selected)]),
            evidence,
        })
    }
    pub fn selected(&self) -> &FileRef {
        &self.master
    }
    fn dependencies(&self) -> job::Dependencies<'_> {
        job::Dependencies {
            composition: &self.composition,
            evidence: &self.evidence,
        }
    }
    fn controls(&self, store: &mut impl NodeStore, write: bool) -> Result<()> {
        if write {
            self.native.put_manifest(store)?;
        } else {
            self.native.verify_manifest(store)?;
        }
        // Controls can exceed one metadata node: freeze their complete bounded
        // Merkle framing rather than pretending a large blob is one node.
        for (object, raw) in &self.storage {
            if write {
                if store.put(raw)? != *object {
                    bail!("scene reference control publication differs");
                }
            } else if store.get(object)?.as_deref() != Some(raw) {
                bail!("scene reference control unavailable or changed");
            }
        }
        Ok(())
    }
    pub fn freeze(
        &self,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<job::JobContext> {
        self.controls(store, true)?;
        Ok(job::freeze_with_dependencies(
            &self.native.admission(None),
            &self.dependencies(),
            None,
            store,
            limits,
        )?)
    }
    pub fn readback<'p>(
        &'p self,
        selection: &Selection,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<ReferenceReadback<'p, 'a, 'n>> {
        self.controls(store, false)?;
        let resources = job::restore_with_dependencies(
            selection,
            &self.native.admission(None),
            &self.dependencies(),
            None,
            store,
            limits,
        )?;
        Ok(ReferenceReadback {
            plan: self,
            resources,
        })
    }
}
impl InputFiles for ReferencePlan<'_, '_> {
    fn verified_path(&self, _: &FileRef) -> Result<PathBuf> {
        bail!("scene reference controls are not media paths")
    }
    fn control_bytes(&self, file: &FileRef) -> Result<Vec<u8>> {
        Ok(body(&self.metadata, file)?.to_vec())
    }
}
impl<'p, 'a, 'n> ReferenceReadback<'p, 'a, 'n> {
    #[allow(clippy::too_many_arguments)]
    pub fn protect(
        self,
        source: &Source,
        journal: &mut SourceJournal,
        coordination: &Coordination,
        identity: Identity,
        revision: u64,
        operation: &Key,
        budget: &crate::tig_episode::ProtectionBudget,
    ) -> Result<ProtectedReference<'p, 'a, 'n>> {
        if source.checkpoint() != self.plan.native.recipe().checkpoint()
            || budget.max_objects == 0
            || budget.max_objects > 1_000_000
            || budget.max_logical_bytes == 0
            || budget.max_logical_bytes > 9_007_199_254_740_991
            || self.resources.objects().len() > budget.max_objects
            || self.resources.logical_bytes() > budget.max_logical_bytes
        {
            bail!("scene reference ownership source or budget differs");
        }
        let owner = ScopedSource::protect(
            journal,
            coordination,
            identity,
            revision,
            operation,
            self.resources.selection(),
        )?;
        Ok(ProtectedReference {
            readback: self,
            owner,
        })
    }
}
impl ProtectedReference<'_, '_, '_> {
    pub fn resources(&self) -> &job::JobContext {
        &self.readback.resources
    }
    pub fn verify_current(&self, source: &Source, journal: &mut SourceJournal) -> Result<()> {
        if source.checkpoint() != self.readback.plan.native.recipe().checkpoint() {
            bail!("scene reference ownership source changed");
        }
        self.owner.check(journal)
    }
    /// Caller admits staging capacity and runs this in the bounded worker.
    /// No source checkout or preparation files are consulted during comparison.
    pub fn verify_selected_master<'s>(
        &'s self,
        source: &mut Source,
        journal: &mut SourceJournal,
        stage_parent: &Path,
        budget: &Budget,
    ) -> Result<VerifiedScene<'s>> {
        self.verify_current(source, journal)?;
        let plan = self.readback.plan;
        let job: Job = serde_json::from_slice(body(&plan.metadata, &plan.job)?)?;
        let inputs = source.prepare(&job)?;
        let outputs = source.prepare_files(std::slice::from_ref(&plan.master))?;
        let programs = plan.native.programs.with_inputs(&inputs);
        let result = scene_primary_reference::compare_master_to_primaries(
            &plan.job,
            &plan.production,
            plan,
            &programs,
            &outputs,
            &plan.master,
            stage_parent,
            budget,
        );
        self.verify_current(source, journal)?;
        Ok(VerifiedScene {
            owner: &self.owner,
            _programs: plan.native.programs,
            _inputs: inputs,
            outputs,
            checkpoint: source.checkpoint().clone(),
            scene: plan.scene.clone(),
            output: plan.output.clone(),
            master: plan.master.clone(),
            recipe: plan.native.recipe().fingerprint().clone(),
            comparison: result?,
            resources: &self.readback.resources,
            production_definitions: plan
                .native
                .request
                .primary_definitions
                .iter()
                .chain(&plan.native.request.control_definitions)
                .chain(std::iter::once(&plan.native.request.scene_definition))
                .cloned()
                .collect(),
        })
    }
}
