//! Assembly-owned metadata resources, distinct from child/byte/output approval.
use super::*;
use crate::{
    media_runtime::BoundPrograms,
    tig_presentation::jobs::VerifiedPresentation,
    tig_scene_production::jobs::{Coordination, Identity, ScopedSource},
};
use tig_assets::graph::recipe;
use tig_core::{
    LogicalPath,
    tree::{NodeStore, TreeLimits},
};
use tig_git::source::native::{context::ProductionContext, job};
use tig_store::{
    Key,
    source_claims::{Selection, SourceJournal},
};

/// Every selected component and original scene claim is derived from the single
/// admitted source view. Presentation proofs retain their byte/runtime/source
/// guards through this plan; they cannot be replaced by a status label.
pub struct CompositionJobPlan<'a> {
    pub(super) context: &'a ProductionContext,
    pub(super) programs: &'a BoundPrograms,
    pub(super) recipe: recipe::Plan,
    runtime: BTreeMap<LogicalPath, Object>,
    composition: BTreeMap<LogicalPath, Object>,
    evidence: BTreeMap<LogicalPath, Object>,
    children: BTreeMap<String, Digest>,
    child_declaration: Vec<u8>,
    pub(super) presentations: BTreeMap<String, &'a VerifiedPresentation<'a>>,
    presentation_documents: BTreeMap<LogicalPath, Vec<u8>>,
    pub(super) conform: Object,
    pub(super) current_controls: Option<super::current_controls::CurrentControls>,
}
pub struct CompositionReadback<'p, 'a> {
    plan: &'p CompositionJobPlan<'a>,
    resources: job::JobContext,
}
pub struct CompositionOwnership<'a> {
    pub coordination: &'a Coordination,
    pub identity: Identity,
    pub expected_revision: u64,
    pub budget: &'a ProtectionBudget,
}
/// Durable parent source obligation plus a per-source fence and borrowed bound
/// runtime. This does not own component-media byte handles, attest an executed
/// episode, authorize publication or eviction.
pub struct ProtectedComposition<'p, 'a> {
    readback: CompositionReadback<'p, 'a>,
    source: ScopedSource,
    logical_bytes: u64,
}
impl<'a> CompositionJobPlan<'a> {
    pub fn from_current_scenes(
        source: &mut Source,
        request: &Request,
        context: &'a ProductionContext,
        programs: &'a BoundPrograms,
        proofs: &BTreeMap<String, CurrentScene>,
    ) -> Result<Self> {
        Self::from_components(source, request, context, programs, proofs, None)
    }
    /// Admit exact scene correspondence plus freshly verified presentations.
    /// Keep their owners and metadata providers alive; this borrows all byte,
    /// source and runtime guards and retains their declared resource union.
    pub fn from_current_components(
        source: &mut Source,
        request: &Request,
        context: &'a ProductionContext,
        programs: &'a BoundPrograms,
        scenes: &BTreeMap<String, CurrentScene>,
        presentations: &'a BTreeMap<String, VerifiedPresentation<'a>>,
    ) -> Result<Self> {
        Self::from_components(
            source,
            request,
            context,
            programs,
            scenes,
            Some(presentations),
        )
    }
    fn from_components(
        source: &mut Source,
        request: &Request,
        context: &'a ProductionContext,
        programs: &'a BoundPrograms,
        proofs: &BTreeMap<String, CurrentScene>,
        presentations: Option<&'a BTreeMap<String, VerifiedPresentation<'a>>>,
    ) -> Result<Self> {
        let plan = require_current_scenes(source, request, proofs)?;
        if plan.components.iter().any(|component| {
            component.changed_from_baseline
                && (component.kind == "scene" || presentations.is_none())
        }) {
            bail!(
                "current component outputs differ from pinned conform; recompile parent controls before execution"
            );
        }
        let expected = plan
            .components
            .iter()
            .filter(|c| c.kind == "episode-presentation")
            .collect::<Vec<_>>();
        if expected.len() != presentations.map_or(0, BTreeMap::len)
            || expected.iter().any(|component| {
                presentations
                    .and_then(|proofs| proofs.get(&component.segment_id))
                    .is_none_or(|proof| {
                        proof.checkpoint() != &plan.source_checkpoint
                            || proof.output() != &component.record
                            || proof.selected().sha256 != component.selected.sha256.as_str()
                            || proof.selected().bytes != component.selected.bytes
                            || request
                                .component_definitions
                                .iter()
                                .any(|id| proof.production_definitions().contains(id))
                    })
            })
        {
            bail!("composition presentation eligibility requires exact current guarded coverage");
        }
        let producer = Object::from_bytes(programs.manifest_bytes());
        let recipe = composition_recipe(source, request, context, &producer)?;
        let mut composition = BTreeMap::new();
        let mut evidence = BTreeMap::new();
        let mut children = BTreeMap::new();
        let mut presentation_documents = BTreeMap::new();
        let presentations = presentations
            .into_iter()
            .flat_map(|proofs| proofs.iter())
            .map(|(id, proof)| (id.clone(), proof))
            .collect::<BTreeMap<_, _>>();
        let current_controls = if presentations.is_empty() {
            None
        } else {
            Some(super::current_controls::CurrentControls::compile(
                source,
                &plan,
                &presentations,
            )?)
        };
        if let Some(controls) = &current_controls {
            for object in controls.storage.keys() {
                evidence.insert(
                    LogicalPath::parse(&format!("current-controls/{}", object.sha256.as_str()))?,
                    object.clone(),
                );
            }
            evidence.insert(
                LogicalPath::parse("current-controls-root")?,
                controls.root.clone(),
            );
        }
        for component in &plan.components {
            // Native segment labels are not paths and may contain slashes or
            // reserved names. Their hashes have no platform-specific aliases.
            let name = LogicalPath::parse(Digest::blob(component.segment_id.as_bytes()).as_str())?;
            composition.insert(name.clone(), component.selected.clone());
            if component.kind == "scene" {
                let proof = &proofs[&component.segment_id];
                evidence.insert(name, proof.claim().clone());
                children.insert(
                    component.segment_id.clone(),
                    proof.protection_recipe().clone(),
                );
            } else {
                let proof = presentations[&component.segment_id];
                let declaration = proof.declaration()?;
                if declaration.len() > 65_536 {
                    bail!("presentation evidence exceeds budget");
                }
                evidence.insert(name.clone(), Object::from_bytes(&declaration));
                presentation_documents.insert(name.clone(), declaration);
                // Flat exact obligations make the complete child set visible to
                // retention without treating an opaque receipt as a closure.
                for object in proof
                    .resources()
                    .objects()
                    .iter()
                    .chain(std::iter::once(&proof.resources().selection().root))
                {
                    evidence.insert(
                        LogicalPath::parse(&format!(
                            "presentation-resources/{}/{}",
                            name.as_str(),
                            object.sha256.as_str()
                        ))?,
                        object.clone(),
                    );
                    if evidence.len() > 65_536 {
                        bail!("composition evidence exceeds budget");
                    }
                }
            }
        }
        let runtime = programs
            .components()
            .map(|(name, pin)| {
                Ok((
                    LogicalPath::parse(Digest::blob(name.as_bytes()).as_str())?,
                    Object {
                        sha256: Digest::parse(&pin.sha256)?,
                        bytes: pin.bytes,
                    },
                ))
            })
            .collect::<Result<_>>()?;
        let mut declared = serde_json::json!({
            "schema":"reel.tig-composition-child-jobs.experimental-1", "recipes":children,
        });
        if !presentations.is_empty() {
            declared["schema"] = "reel.tig-composition-child-jobs.experimental-2".into();
            declared["presentations"] = serde_json::to_value(
                presentations
                    .iter()
                    .map(|(id, proof)| (id, proof.resources().selection()))
                    .collect::<BTreeMap<_, _>>(),
            )?;
        }
        let child_declaration = identity::canonical_json(&declared)?;
        if child_declaration.len() > 65_536 {
            bail!("composition child declaration exceeds budget");
        }
        evidence.insert(
            LogicalPath::parse("child-recipes")?,
            Object::from_bytes(&child_declaration),
        );
        Ok(Self {
            context,
            programs,
            recipe,
            runtime,
            composition,
            evidence,
            children,
            child_declaration,
            presentations,
            presentation_documents,
            conform: current_controls
                .as_ref()
                .map_or_else(|| plan.conform.clone(), |c| Object::from_bytes(&c.raw)),
            current_controls,
        })
    }
    pub fn recipe(&self) -> &recipe::Plan {
        &self.recipe
    }
    pub fn compiled_conform(&self) -> Option<&crate::scene_delivery::FileRef> {
        self.current_controls
            .as_ref()
            .map(|controls| &controls.reference)
    }
    pub(super) fn admission(&self) -> job::Admission<'_> {
        job::Admission {
            context: self.context,
            plan: &self.recipe,
            runtime: &self.runtime,
            output: None,
        }
    }
    pub(super) fn dependencies(&self) -> job::Dependencies<'_> {
        job::Dependencies {
            composition: &self.composition,
            evidence: &self.evidence,
        }
    }
    /// Candidate metadata only; finish and reopen the provider before readback.
    pub fn freeze(
        &self,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<job::JobContext> {
        self.put_interpretation(store)?;
        Ok(job::freeze_with_dependencies(
            &self.admission(),
            &self.dependencies(),
            None,
            store,
            limits,
        )?)
    }
    pub(super) fn put_interpretation(&self, store: &mut impl NodeStore) -> Result<()> {
        if let Some(controls) = &self.current_controls {
            for (object, raw) in &controls.storage {
                if store.put(raw)? != *object {
                    bail!("current parent control publication differs");
                }
            }
        }
        for raw in self.presentation_documents.values() {
            if store.put(raw)? != Object::from_bytes(raw) {
                bail!("presentation proof publication differs");
            }
        }
        if store.put(&self.child_declaration)? != Object::from_bytes(&self.child_declaration) {
            bail!("composition child declaration publication differs");
        }
        if store.put(self.programs.manifest_bytes())? != self.recipe.operation().producer {
            bail!("composition runtime manifest publication differs");
        }
        Ok(())
    }
    pub fn readback<'p>(
        &'p self,
        selection: &Selection,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<CompositionReadback<'p, 'a>> {
        self.verify_interpretation(store)?;
        let resources = job::restore_with_dependencies(
            selection,
            &self.admission(),
            &self.dependencies(),
            None,
            store,
            limits,
        )?;
        Ok(CompositionReadback {
            plan: self,
            resources,
        })
    }
    pub(super) fn verify_interpretation(&self, store: &mut impl NodeStore) -> Result<()> {
        if let Some(controls) = &self.current_controls {
            for (object, raw) in &controls.storage {
                if store.get(object)?.as_deref() != Some(raw.as_slice()) {
                    bail!("current parent control missing or substituted");
                }
            }
        }
        for raw in self.presentation_documents.values() {
            if store.get(&Object::from_bytes(raw))?.as_deref() != Some(raw.as_slice()) {
                bail!("presentation proof missing or substituted");
            }
        }
        if store.get(&self.recipe.operation().producer)?.as_deref()
            != Some(self.programs.manifest_bytes())
        {
            bail!("composition runtime manifest missing or substituted");
        }
        if store
            .get(&Object::from_bytes(&self.child_declaration))?
            .as_deref()
            != Some(self.child_declaration.as_slice())
        {
            bail!("composition child declaration missing or substituted");
        }
        Ok(())
    }
}
impl<'p, 'a> CompositionReadback<'p, 'a> {
    pub fn resources(&self) -> &job::JobContext {
        &self.resources
    }
    fn preflight(
        &self,
        source: &Source,
        children: &ProtectedScenes<'_, '_>,
        journal: &mut SourceJournal,
        identity: &Identity,
        budget: &ProtectionBudget,
    ) -> Result<u64> {
        if source.checkpoint() != self.plan.recipe.checkpoint()
            || !children.matches(source.checkpoint(), &self.plan.children)
            || children.contains_source(&identity.source)
            || self
                .plan
                .presentations
                .values()
                .any(|proof| proof.source_identity() == &identity.source)
            || budget.max_objects == 0
            || budget.max_objects > 1_000_000
            || budget.max_logical_bytes == 0
            || budget.max_logical_bytes > 9_007_199_254_740_991
        {
            bail!("composition source, child ownership or resource budget differs");
        }
        children.verify_current(source, journal)?;
        for proof in self.plan.presentations.values() {
            proof.verify_current(source, journal)?;
        }
        let mut sizes = BTreeMap::new();
        let mut bytes = 0u64;
        for object in children.objects().iter().chain(self.resources.objects()) {
            match sizes.insert(object.sha256.clone(), object.bytes) {
                Some(old) if old != object.bytes => bail!("composition resource lengths conflict"),
                Some(_) => {}
                None => {
                    bytes = bytes
                        .checked_add(object.bytes)
                        .context("composition byte overflow")?
                }
            }
            if sizes.len() > budget.max_objects || bytes > budget.max_logical_bytes {
                bail!("composition and child resource union exceeds budget");
            }
        }
        Ok(bytes)
    }
    /// All source/child/budget checks precede adoption. An adoption failure never
    /// withdraws child obligations; retry/recovery remains explicit and scoped.
    pub fn protect(
        self,
        source: &Source,
        children: &ProtectedScenes<'_, '_>,
        journal: &mut SourceJournal,
        ownership: &CompositionOwnership<'_>,
        operation: &Key,
    ) -> Result<ProtectedComposition<'p, 'a>> {
        let logical_bytes = self.preflight(
            source,
            children,
            journal,
            &ownership.identity,
            ownership.budget,
        )?;
        let guard = ScopedSource::protect(
            journal,
            ownership.coordination,
            ownership.identity.clone(),
            ownership.expected_revision,
            operation,
            self.resources.selection(),
        )?;
        let protected = ProtectedComposition {
            readback: self,
            source: guard,
            logical_bytes,
        };
        protected.verify_current(source, children, journal)?;
        Ok(protected)
    }
    /// Resume only an exact current parent obligation, with the exact current
    /// independently protected children. No new journal transition.
    pub fn resume(
        self,
        source: &Source,
        children: &ProtectedScenes<'_, '_>,
        journal: &mut SourceJournal,
        ownership: &CompositionOwnership<'_>,
    ) -> Result<ProtectedComposition<'p, 'a>> {
        let logical_bytes = self.preflight(
            source,
            children,
            journal,
            &ownership.identity,
            ownership.budget,
        )?;
        let guard = ScopedSource::resume(
            journal,
            ownership.coordination,
            ownership.identity.clone(),
            ownership.expected_revision,
            self.resources.selection(),
        )?;
        let protected = ProtectedComposition {
            readback: self,
            source: guard,
            logical_bytes,
        };
        protected.verify_current(source, children, journal)?;
        Ok(protected)
    }
}
impl ProtectedComposition<'_, '_> {
    pub(super) fn source_guard(&self) -> &ScopedSource {
        &self.source
    }
    pub(super) fn plan(&self) -> &CompositionJobPlan<'_> {
        self.readback.plan
    }
    pub fn resources(&self) -> &job::JobContext {
        &self.readback.resources
    }
    pub fn revision(&self) -> u64 {
        self.source.revision()
    }
    /// Unique declared bytes across this parent and its exact child guards.
    /// This is not allocated disk usage or proof of physical byte availability.
    pub fn logical_bytes(&self) -> u64 {
        self.logical_bytes
    }
    pub fn verify_current(
        &self,
        source: &Source,
        children: &ProtectedScenes<'_, '_>,
        journal: &mut SourceJournal,
    ) -> Result<()> {
        if source.checkpoint() != self.readback.plan.recipe.checkpoint()
            || !children.matches(source.checkpoint(), &self.readback.plan.children)
        {
            bail!("composition or child source snapshot differs");
        }
        self.source.check(journal)?;
        for proof in self.readback.plan.presentations.values() {
            proof.verify_current(source, journal)?;
        }
        children.verify_current(source, journal)
    }
    /// Caller supplies actual byte guards and the bounded native action. Check
    /// both ownership layers before and after even a failing action. No returned
    /// value escapes a stale postcondition, and Drop never retires sources.
    pub fn with_current<T>(
        &self,
        source: &Source,
        children: &ProtectedScenes<'_, '_>,
        journal: &mut SourceJournal,
        action: impl FnOnce(&mut SourceJournal) -> Result<T>,
    ) -> Result<T> {
        self.verify_current(source, children, journal)?;
        let result = action(journal);
        self.verify_current(source, children, journal)?;
        result
    }
}
