//! Scoped source ownership for independently checked presentation masters.
//! No scheduler, creative approval, complete environment or eviction authority.
use super::*;
use crate::{
    media_runtime::BoundPrograms,
    tig_episode::ProtectionBudget,
    tig_scene_production::jobs::{Coordination, Identity, ScopedSource},
};
use tig_assets::graph::recipe::Plan;
use tig_core::{
    LogicalPath,
    tree::{NodeStore, TreeLimits},
};
use tig_git::source::native::{context::ProductionContext, job};
use tig_store::{
    Key,
    source_claims::{Selection, SourceJournal},
};

pub struct JobPlan<'a> {
    context: &'a ProductionContext,
    programs: &'a BoundPrograms,
    recipe: Plan,
    compiled: CompiledPresentation,
    runtime: BTreeMap<LogicalPath, Object>,
    evidence: BTreeMap<LogicalPath, Object>,
    composition: BTreeMap<LogicalPath, Object>,
    target: Reference,
    master: FileRef,
    production_definitions: BTreeSet<String>,
}
pub struct Readback<'p, 'a> {
    plan: &'p JobPlan<'a>,
    resources: job::JobContext,
}
pub struct ProtectedPresentation<'p, 'a> {
    readback: Readback<'p, 'a>,
    source: ScopedSource,
}

/// Holds primary and output byte guards, the source fence and bound runtime
/// through use. Constructed only after fresh primary comparison.
/// A technical match never substitutes for an original producer or approval.
pub struct VerifiedPresentation<'a> {
    owner: &'a ScopedSource,
    _programs: &'a BoundPrograms,
    _inputs: crate::tig_inputs::Inputs,
    outputs: crate::tig_inputs::Inputs,
    checkpoint: Digest,
    source: Reference,
    output: Reference,
    recipe: Digest,
    comparison: presentation_still_sequence::PrimaryComparison,
    frames: u64,
    samples: u64,
    resources: &'a job::JobContext,
    production_definitions: &'a BTreeSet<String>,
}
impl VerifiedPresentation<'_> {
    pub(crate) fn selected_path(&self) -> Result<std::path::PathBuf> {
        self.outputs.verified_path(self.selected())
    }
    pub(crate) fn checkpoint(&self) -> &Digest {
        &self.checkpoint
    }
    pub(crate) fn resources(&self) -> &job::JobContext {
        self.resources
    }
    pub(crate) fn source_identity(&self) -> &Key {
        self.owner.source_identity()
    }
    pub(crate) fn production_definitions(&self) -> &BTreeSet<String> {
        self.production_definitions
    }
    pub(crate) fn declaration(&self) -> Result<Vec<u8>> {
        let (yuv, rgb) = self.comparison.picture_sha256();
        Ok(identity::canonical_json(&json!({
            "schema":"reel.tig-presentation-verification.experimental-1",
            "checkpoint":self.checkpoint,"source":self.source,"output":self.output,
            "job":self.resources.selection(),"recipe":self.recipe,
            "manifest":self.comparison.manifest(),"master":self.comparison.master(),
            "yuv_sha256":yuv,"rgb_sha256":rgb,"pcm_sha256":self.comparison.audio_sha256(),
            "frames":self.frames,"samples":self.samples,"timestamps_verified":true,
            "approval":false,"original_producer_assertion":false,
        }))?)
    }
    pub fn selected(&self) -> &FileRef {
        self.comparison.master()
    }
    pub fn source(&self) -> &Reference {
        &self.source
    }
    pub fn output(&self) -> &Reference {
        &self.output
    }
    pub fn recipe(&self) -> &Digest {
        &self.recipe
    }
    pub fn verify_current(&self, source: &Source, journal: &mut SourceJournal) -> Result<()> {
        if source.checkpoint() != &self.checkpoint {
            bail!("presentation proof belongs to another source");
        }
        self.owner.check(journal)?;
        self.outputs.verified_path(self.selected())?;
        Ok(())
    }
}
fn object(pin: &crate::media_runtime::ByteIdentity) -> Result<Object> {
    Ok(Object {
        sha256: Digest::parse(&pin.sha256)?,
        bytes: pin.bytes,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceScope {
    episode_id: String,
    role: String,
}
fn target(
    source: &Source,
    request: &Request,
    id: &str,
    definition: &str,
) -> Result<(Reference, FileRef)> {
    let output = source.record(id)?;
    let controls = [
        &request.source_definition,
        &request.template_definition,
        &request.timing_definition,
        &request.clock_definition,
        &request.evidence_definition,
    ];
    let spec: Spec = serde_json::from_value(
        source.record(&request.presentation_id)?.authored()["document"].clone(),
    )?;
    let episode = output
        .authored()
        .get("episode_id")
        .and_then(Value::as_str)
        .context("presentation occurrence episode missing")?;
    let occurrence = output
        .authored()
        .get("source_segment_id")
        .and_then(Value::as_str)
        .context("presentation occurrence role missing")?;
    token(episode)?;
    token(occurrence)?;
    // A season opening is a reusable source; its occurrence in an episode can
    // have another ID. Explicitly bind that source scope, never infer a rename.
    let scope = output
        .authored()
        .get("presentation_scope")
        .map(|value| serde_json::from_value::<SourceScope>(value.clone()))
        .transpose()?
        .unwrap_or_else(|| SourceScope {
            episode_id: episode.into(),
            role: occurrence.into(),
        });
    if output.definition().id != definition
        || controls.iter().any(|id| id.as_str() == definition)
        || request.primary_definitions.contains(definition)
        || !matches!(
            source.admission(definition)?.payload,
            crate::tig_inputs::Payload::Selected {}
        )
        || output.authored().get("presentation") != Some(&json!({"id":request.presentation_id}))
        || output.authored().get("language") != Some(&json!(spec.language))
        || scope.episode_id != spec.episode_id
        || scope.role != spec.role
        || output.authored().get("component_kind") != Some(&json!("episode-presentation"))
    {
        bail!("presentation output differs from admitted source, type or scope");
    }
    let selected = output.selected()?;
    Ok((
        output.reference().clone(),
        FileRef {
            path: id.into(),
            sha256: selected.sha256.as_str().into(),
            bytes: selected.bytes,
        },
    ))
}
impl<'a> JobPlan<'a> {
    /// Derive the native recipe and resources from one source and bound runtime.
    /// No source journal transition or primary-byte IO occurs here.
    pub fn from_context(
        source: &mut Source,
        request: &'a Request,
        context: &'a ProductionContext,
        programs: &'a BoundPrograms,
        output_id: &str,
        output_definition: &str,
    ) -> Result<Self> {
        let (target, master) = target(source, request, output_id, output_definition)?;
        let recipe = super::recipe(source, request, context, &object(programs.identity())?)?;
        let compiled = compile(source, request)?;
        let runtime = programs
            .components()
            .map(|(name, pin)| {
                Ok((
                    LogicalPath::parse(Digest::blob(name.as_bytes()).as_str())?,
                    object(pin)?,
                ))
            })
            .collect::<Result<_>>()?;
        let mut evidence: BTreeMap<LogicalPath, Object> = compiled
            .metadata_files()
            .map(|(pin, _)| {
                Ok((
                    LogicalPath::parse(
                        Digest::blob(pin.path.to_string_lossy().as_bytes()).as_str(),
                    )?,
                    Object {
                        sha256: Digest::parse(&pin.sha256)?,
                        bytes: pin.bytes,
                    },
                ))
            })
            .collect::<Result<_>>()?;
        evidence.insert(
            LogicalPath::parse("selected-record")?,
            target.object.clone(),
        );
        let composition = BTreeMap::from([(
            LogicalPath::parse("selected-master")?,
            Object {
                sha256: Digest::parse(&master.sha256)?,
                bytes: master.bytes,
            },
        )]);
        Ok(Self {
            context,
            programs,
            recipe,
            compiled,
            runtime,
            evidence,
            composition,
            target,
            master,
            production_definitions: request
                .primary_definitions
                .iter()
                .chain([
                    &request.source_definition,
                    &request.template_definition,
                    &request.timing_definition,
                    &request.clock_definition,
                    &request.evidence_definition,
                ])
                .cloned()
                .collect(),
        })
    }
    pub fn recipe(&self) -> &Plan {
        &self.recipe
    }
    pub fn compiled(&self) -> &CompiledPresentation {
        &self.compiled
    }
    pub fn selected_master(&self) -> &FileRef {
        &self.master
    }
    fn counts(&self) -> Result<(u64, u64)> {
        let manifest =
            identity::parse_json(&self.compiled.control_bytes(self.compiled.manifest())?)?;
        let template = &manifest["template"];
        let template = FileRef {
            path: serde_json::from_value(template["path"].clone())?,
            sha256: template["sha256"]
                .as_str()
                .context("native template hash missing")?
                .into(),
            bytes: template["bytes"]
                .as_u64()
                .context("native template length missing")?,
        };
        let template = identity::parse_json(&self.compiled.control_bytes(&template)?)?;
        let integer = |key: &str| {
            template[key]
                .as_u64()
                .context("native frame clock field missing")
        };
        let frames = match template.get("total_frames").and_then(Value::as_u64) {
            Some(frames) => frames,
            None => integer("picture_count")?
                .checked_mul(integer("frames_per_picture")?)
                .context("native frame count overflow")?,
        };
        let fps = integer("frame_rate")?;
        let samples = frames
            .checked_mul(integer("sample_rate")?)
            .and_then(|n| n.checked_div(fps))
            .context("native sample clock overflow")?;
        if frames == 0 || samples == 0 {
            bail!("empty checked native frame clock");
        }
        Ok((frames, samples))
    }
    fn admission(&self) -> job::Admission<'_> {
        job::Admission {
            context: self.context,
            plan: &self.recipe,
            runtime: &self.runtime,
            output: None,
        }
    }
    fn dependencies(&self) -> job::Dependencies<'_> {
        job::Dependencies {
            composition: &self.composition,
            evidence: &self.evidence,
        }
    }
    fn interpretation(&self, store: &mut impl NodeStore, write: bool) -> Result<()> {
        let manifest = self.programs.manifest_bytes();
        let expected = &self.recipe.operation().producer;
        if write {
            if store.put(manifest)? != *expected {
                bail!("presentation runtime publication differs");
            }
        } else if store.get(expected)?.as_deref() != Some(manifest) {
            bail!("presentation runtime manifest unavailable or changed");
        }
        for (pin, raw) in self.compiled.metadata_files() {
            let expected = Object {
                sha256: Digest::parse(&pin.sha256)?,
                bytes: pin.bytes,
            };
            if write {
                if store.put(raw)? != expected {
                    bail!("presentation control publication differs");
                }
            } else if store.get(&expected)?.as_deref() != Some(raw) {
                bail!("presentation control unavailable or changed");
            }
        }
        Ok(())
    }
    /// Caller finishes and reopens this provider before readback and adoption.
    pub fn freeze(
        &self,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<job::JobContext> {
        self.interpretation(store, true)?;
        Ok(job::freeze_with_dependencies(
            &self.admission(),
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
    ) -> Result<Readback<'p, 'a>> {
        self.interpretation(store, false)?;
        let resources = job::restore_with_dependencies(
            selection,
            &self.admission(),
            &self.dependencies(),
            None,
            store,
            limits,
        )?;
        Ok(Readback {
            plan: self,
            resources,
        })
    }
}
impl<'p, 'a> Readback<'p, 'a> {
    pub fn resources(&self) -> &job::JobContext {
        &self.resources
    }
    fn preflight(&self, source: &Source, budget: &ProtectionBudget) -> Result<()> {
        if source.checkpoint() != self.plan.recipe.checkpoint()
            || budget.max_objects == 0
            || budget.max_objects > 1_000_000
            || budget.max_logical_bytes == 0
            || budget.max_logical_bytes > 9_007_199_254_740_991
            || self.resources.objects().len() > budget.max_objects
            || self.resources.logical_bytes() > budget.max_logical_bytes
        {
            bail!("presentation ownership source or resource budget differs");
        }
        Ok(())
    }
    /// Uses only the existing per-source fence. Independent sources can proceed.
    /// A later failure never silently withdraws already adopted obligations.
    #[allow(clippy::too_many_arguments)]
    pub fn protect(
        self,
        source: &Source,
        journal: &mut SourceJournal,
        coordination: &Coordination,
        identity: Identity,
        revision: u64,
        operation: &Key,
        budget: &ProtectionBudget,
    ) -> Result<ProtectedPresentation<'p, 'a>> {
        self.preflight(source, budget)?;
        let owner = ScopedSource::protect(
            journal,
            coordination,
            identity,
            revision,
            operation,
            self.resources.selection(),
        )?;
        let protected = ProtectedPresentation {
            readback: self,
            source: owner,
        };
        protected.verify_current(source, journal)?;
        Ok(protected)
    }
    #[allow(clippy::too_many_arguments)]
    pub fn resume(
        self,
        source: &Source,
        journal: &mut SourceJournal,
        coordination: &Coordination,
        identity: Identity,
        revision: u64,
        budget: &ProtectionBudget,
    ) -> Result<ProtectedPresentation<'p, 'a>> {
        self.preflight(source, budget)?;
        let owner = ScopedSource::resume(
            journal,
            coordination,
            identity,
            revision,
            self.resources.selection(),
        )?;
        let protected = ProtectedPresentation {
            readback: self,
            source: owner,
        };
        protected.verify_current(source, journal)?;
        Ok(protected)
    }
}
impl ProtectedPresentation<'_, '_> {
    pub fn resources(&self) -> &job::JobContext {
        &self.readback.resources
    }
    pub fn revision(&self) -> u64 {
        self.source.revision()
    }
    pub fn verify_current(&self, source: &Source, journal: &mut SourceJournal) -> Result<()> {
        if source.checkpoint() != self.readback.plan.recipe.checkpoint() {
            bail!("presentation ownership belongs to another source");
        }
        self.source.check(journal)
    }
    /// Freshly compare the selected master to current native primaries under
    /// current source ownership. Run within the bounded scheduler in production.
    /// The output and all primary/runtime guards remain owned or borrowed by
    /// the returned proof. No old receipt is rewritten or required.
    pub fn verify_selected_master<'p>(
        &'p self,
        source: &mut Source,
        journal: &mut SourceJournal,
        stage_parent: &std::path::Path,
        max_staged_bytes: u64,
    ) -> Result<VerifiedPresentation<'p>> {
        self.verify_current(source, journal)?;
        let plan = self.readback.plan;
        let inputs = plan.compiled.prepare(source)?;
        let outputs = source.prepare_files(std::slice::from_ref(&plan.master))?;
        let programs = plan.programs.with_inputs(&inputs);
        let result = presentation_still_sequence::compare_master_to_primaries(
            plan.compiled.manifest(),
            &plan.compiled,
            &programs,
            &crate::tig_scene_production::jobs::reference::SelectedOutputs(&outputs),
            &plan.master,
            stage_parent,
            max_staged_bytes,
        );
        self.verify_current(source, journal)?;
        let comparison = result?;
        let (frames, samples) = plan.counts()?;
        Ok(VerifiedPresentation {
            owner: &self.source,
            _programs: plan.programs,
            _inputs: inputs,
            outputs,
            checkpoint: source.checkpoint().clone(),
            source: plan.compiled.root().clone(),
            output: plan.target.clone(),
            recipe: plan.recipe.fingerprint().clone(),
            comparison,
            frames,
            samples,
            resources: &self.readback.resources,
            production_definitions: &plan.production_definitions,
        })
    }
}
