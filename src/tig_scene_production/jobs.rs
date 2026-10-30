//! Explicit native job ownership over existing metadata/journal/provider ports.
//! Resource protection is scoped, not global availability, custody or deletion.
use super::{
    DeliveryBudget, OutputBinding, Publication, PublishedCandidate, RecipeMode, RenderedCandidate,
    context_recipe,
};
use crate::{media_runtime::BoundPrograms, tig_inputs::Source, tig_scene::Request};
use anyhow::{Result, bail};
use reel_assets::admission;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tig_assets::graph::recipe::Plan;
use tig_core::{
    Digest, LogicalPath, Object, identity,
    tree::{NodeStore, TreeLimits},
};
use tig_git::source::native::{context::ProductionContext, job};
use tig_store::{
    Key, PublicationBackend,
    source_claims::{Command, Selection, SourceJournal},
};

pub mod reference;
/// Existing private coordination root shared by all managed users of this journal.
pub struct Coordination {
    pub root: PathBuf,
}
#[derive(Clone, PartialEq, Eq)]
pub struct Identity {
    pub source: Key,
    pub owner: Key,
}
pub struct JobPlan<'a> {
    request: &'a Request,
    context: &'a ProductionContext,
    programs: &'a BoundPrograms,
    recipe: Plan,
    runtime: BTreeMap<LogicalPath, Object>,
    mode: RecipeMode,
}
pub struct Readback<'p, 'a> {
    plan: &'p JobPlan<'a>,
    resources: job::JobContext,
}
#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    journal: Digest,
    identity: Identity,
    revision: u64,
    selection: Selection,
}
pub struct ProtectedJob<'p, 'a> {
    plan: &'p JobPlan<'a>,
    stamp: Stamp,
    output_handoff: Option<(Key, Stamp)>,
    _fence: admission::MetadataLock,
}
pub struct ManagedCandidate {
    stamp: Stamp,
    candidate: RenderedCandidate,
}
pub struct ManagedPublication {
    stamp: Stamp,
    published: PublishedCandidate,
}
pub struct OutputReadback {
    stamp: Stamp,
    resources: job::JobContext,
}
impl ManagedCandidate {
    pub fn candidate(&self) -> &RenderedCandidate {
        &self.candidate
    }
}
impl ManagedPublication {
    pub fn published(&self) -> &PublishedCandidate {
        &self.published
    }
}
impl OutputReadback {
    pub fn resources(&self) -> &job::JobContext {
        &self.resources
    }
}
fn object(pin: &crate::media_runtime::ByteIdentity) -> Result<Object> {
    Ok(Object {
        sha256: Digest::parse(&pin.sha256)?,
        bytes: pin.bytes,
    })
}
impl<'a> JobPlan<'a> {
    pub fn from_context(
        source: &mut Source,
        request: &'a Request,
        context: &'a ProductionContext,
        programs: &'a BoundPrograms,
    ) -> Result<Self> {
        let producer = object(programs.identity())?;
        let recipe = context_recipe(source, request, context, &producer)?;
        Self::with_recipe(request, context, programs, recipe, RecipeMode::Declared)
    }
    /// Explicit compiled-context planning; writes only new preparation scratch.
    /// Caller must still finish/read back and protect the resulting job resources.
    pub fn from_render_context(
        source: &mut Source,
        request: &'a Request,
        context: &'a ProductionContext,
        programs: &'a BoundPrograms,
        preparation: &Path,
    ) -> Result<Self> {
        let producer = object(programs.identity())?;
        let recipe =
            crate::tig_scene::render_recipe(source, request, context, &producer, preparation)?;
        Self::with_recipe(request, context, programs, recipe, RecipeMode::Projected)
    }
    fn with_recipe(
        request: &'a Request,
        context: &'a ProductionContext,
        programs: &'a BoundPrograms,
        recipe: Plan,
        mode: RecipeMode,
    ) -> Result<Self> {
        // Component names are opaque labels, not paths. Hash them so valid
        // dependency labels cannot create reserved-name or prefix collisions.
        let runtime = programs
            .components()
            .map(|(name, pin)| {
                Ok((
                    LogicalPath::parse(Digest::blob(name.as_bytes()).as_str())?,
                    object(pin)?,
                ))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            request,
            context,
            programs,
            recipe,
            runtime,
            mode,
        })
    }
    pub fn recipe(&self) -> &Plan {
        &self.recipe
    }
    pub(crate) fn native_request(&self) -> &Request {
        self.request
    }
    /// Recompile current metadata under this exact frozen native interpretation.
    /// This creates only a new bounded preparation directory, never media.
    pub(crate) fn compile_parent_controls(
        &self,
        source: &mut Source,
        preparation: &Path,
    ) -> Result<()> {
        let recipe = match self.mode {
            RecipeMode::Declared => {
                let recipe = context_recipe(
                    source,
                    self.request,
                    self.context,
                    &self.recipe.operation().producer,
                )?;
                crate::tig_scene::compile(source, self.request, preparation)?;
                recipe
            }
            RecipeMode::Projected => self.recipe_for_producer(
                source,
                &self.recipe.operation().producer,
                Some(preparation),
            )?,
        };
        if recipe.fingerprint() != self.recipe.fingerprint() {
            bail!("parent scene controls differ from current native job");
        }
        Ok(())
    }
    pub(crate) fn recipe_for_producer(
        &self,
        source: &mut Source,
        producer: &Object,
        preparation: Option<&Path>,
    ) -> Result<Plan> {
        let recipe = match self.mode {
            RecipeMode::Declared => {
                if preparation.is_some() {
                    bail!("declared producer replay takes no preparation directory");
                }
                context_recipe(source, self.request, self.context, producer)?
            }
            RecipeMode::Projected => crate::tig_scene::render_recipe(
                source,
                self.request,
                self.context,
                producer,
                preparation.ok_or_else(|| {
                    anyhow::anyhow!("projected producer replay requires new preparation scratch")
                })?,
            )?,
        };
        if recipe.checkpoint() != self.recipe.checkpoint()
            || recipe.binding() != self.recipe.binding()
            || recipe.admission() != self.recipe.admission()
            || recipe.nodes() != self.recipe.nodes()
            || recipe.operation().name != self.recipe.operation().name
            || recipe.operation().semantics != self.recipe.operation().semantics
            || recipe.operation().parameters != self.recipe.operation().parameters
        {
            bail!("producer replay changed current native source interpretation");
        }
        Ok(recipe)
    }
    pub fn runtime(&self) -> &BTreeMap<LogicalPath, Object> {
        &self.runtime
    }
    fn admission<'b>(&'b self, output: Option<job::OutputEvidence<'b>>) -> job::Admission<'b> {
        job::Admission {
            context: self.context,
            plan: &self.recipe,
            runtime: &self.runtime,
            output,
        }
    }
    fn put_manifest(&self, store: &mut impl NodeStore) -> Result<()> {
        if store.put(self.programs.manifest_bytes())? != self.recipe.operation().producer {
            bail!("native runtime manifest publication differs");
        }
        Ok(())
    }
    fn verify_manifest(&self, store: &impl NodeStore) -> Result<()> {
        let bytes = store
            .get(&self.recipe.operation().producer)?
            .ok_or_else(|| anyhow::anyhow!("native runtime manifest missing"))?;
        if bytes != self.programs.manifest_bytes() {
            bail!("native runtime manifest differs");
        }
        Ok(())
    }
    /// Candidate metadata only; caller explicitly finishes/reopens its provider.
    pub fn freeze(
        &self,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<job::JobContext> {
        let resources = job::freeze(&self.admission(None), store, limits)?;
        self.put_manifest(store)?;
        Ok(resources)
    }
    pub fn readback<'p>(
        &'p self,
        selection: &Selection,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<Readback<'p, 'a>> {
        self.verify_manifest(store)?;
        let resources = job::restore(selection, &self.admission(None), store, limits)?;
        Ok(Readback {
            plan: self,
            resources,
        })
    }
}
pub(crate) fn fence(
    coordination: &Coordination,
    journal: &SourceJournal,
    identity: &Identity,
) -> Result<admission::MetadataLock> {
    if !coordination.root.is_absolute() || !coordination.root.is_dir() {
        bail!("existing native job coordination root required");
    }
    let root = coordination
        .root
        .canonicalize()?
        .join("tig-native-jobs")
        .join(journal.identity().as_str())
        .join(Digest::blob(identity.source.as_str().as_bytes()).as_str());
    admission::lock(&root).map_err(|error| anyhow::anyhow!("native job fence unavailable: {error}"))
}
fn phase(identity: &Identity, operation: &Key, label: &str) -> Result<Key> {
    let bytes = identity::canonical_json(&serde_json::json!({
        "schema":"reel.native-job-operation.experimental-1",
        "source":identity.source.as_str(),"operation":operation.as_str(),"phase":label,
    }))?;
    Ok(Key::parse(Digest::blob(&bytes).as_str())?)
}
fn check(journal: &mut SourceJournal, stamp: &Stamp) -> Result<()> {
    if journal.identity() != &stamp.journal {
        bail!("native job belongs to another journal authority");
    }
    let current = journal
        .source(&stamp.identity.source)?
        .ok_or_else(|| anyhow::anyhow!("native job source missing"))?;
    if current.owner != stamp.identity.owner
        || current.revision != stamp.revision
        || current.pending.is_some()
        || current
            .current
            .as_ref()
            .is_none_or(|v| v.selection != stamp.selection)
    {
        bail!("native job ownership, revision or selection changed");
    }
    Ok(())
}
fn adopt(
    journal: &mut SourceJournal,
    identity: &Identity,
    revision: u64,
    operation: &Key,
    selection: &Selection,
) -> Result<Stamp> {
    let prepared = journal.apply(
        phase(identity, operation, "prepare")?,
        Command::Prepare {
            source: identity.source.clone(),
            owner: identity.owner.clone(),
            expected_revision: revision,
            selection: selection.clone(),
        },
    )?;
    let adopted = journal.apply(
        phase(identity, operation, "adopt")?,
        Command::Adopt {
            source: identity.source.clone(),
            owner: identity.owner.clone(),
            expected_revision: prepared.source_revision,
            prepared: prepared.reference,
        },
    )?;
    let stamp = Stamp {
        journal: journal.identity().clone(),
        identity: identity.clone(),
        revision: adopted.source_revision,
        selection: selection.clone(),
    };
    // Historical retries cannot reissue a current capability after later changes.
    check(journal, &stamp)?;
    Ok(stamp)
}
/// Internal source ownership shared by native scene and composition adapters.
/// Drop releases only the per-source fence, never the durable source obligation.
pub(crate) struct ScopedSource {
    stamp: Stamp,
    _fence: admission::MetadataLock,
}
impl ScopedSource {
    pub(crate) fn source_identity(&self) -> &Key {
        &self.stamp.identity.source
    }
    pub(crate) fn protect(
        journal: &mut SourceJournal,
        coordination: &Coordination,
        identity: Identity,
        revision: u64,
        operation: &Key,
        selection: &Selection,
    ) -> Result<Self> {
        let guard = fence(coordination, journal, &identity)?;
        let stamp = adopt(journal, &identity, revision, operation, selection)?;
        Ok(Self {
            stamp,
            _fence: guard,
        })
    }
    pub(crate) fn resume(
        journal: &mut SourceJournal,
        coordination: &Coordination,
        identity: Identity,
        revision: u64,
        selection: &Selection,
    ) -> Result<Self> {
        let guard = fence(coordination, journal, &identity)?;
        let stamp = Stamp {
            journal: journal.identity().clone(),
            identity,
            revision,
            selection: selection.clone(),
        };
        check(journal, &stamp)?;
        Ok(Self {
            stamp,
            _fence: guard,
        })
    }
    pub(crate) fn check(&self, journal: &mut SourceJournal) -> Result<()> {
        check(journal, &self.stamp)
    }
    pub(crate) fn revision(&self) -> u64 {
        self.stamp.revision
    }
}
impl<'p, 'a> Readback<'p, 'a> {
    pub(crate) fn plan(&self) -> &JobPlan<'a> {
        self.plan
    }
    pub fn resources(&self) -> &job::JobContext {
        &self.resources
    }
    pub fn protect(
        self,
        journal: &mut SourceJournal,
        coordination: &Coordination,
        identity: Identity,
        expected_revision: u64,
        operation: &Key,
    ) -> Result<ProtectedJob<'p, 'a>> {
        let guard = fence(coordination, journal, &identity)?;
        let stamp = adopt(
            journal,
            &identity,
            expected_revision,
            operation,
            self.resources.selection(),
        )?;
        Ok(ProtectedJob {
            plan: self.plan,
            stamp,
            output_handoff: None,
            _fence: guard,
        })
    }
    /// Resume an externally admitted exact current planned job; no transition.
    pub fn resume(
        self,
        journal: &mut SourceJournal,
        coordination: &Coordination,
        identity: Identity,
        expected_revision: u64,
    ) -> Result<ProtectedJob<'p, 'a>> {
        let guard = fence(coordination, journal, &identity)?;
        let stamp = Stamp {
            journal: journal.identity().clone(),
            identity,
            revision: expected_revision,
            selection: self.resources.selection().clone(),
        };
        check(journal, &stamp)?;
        Ok(ProtectedJob {
            plan: self.plan,
            stamp,
            output_handoff: None,
            _fence: guard,
        })
    }
}
fn attachments(published: &ManagedPublication) -> Result<BTreeMap<LogicalPath, Object>> {
    let mut attachments = BTreeMap::from([(
        LogicalPath::parse("native-receipt")?,
        published.published.receipt_object().clone(),
    )]);
    if let Some(outputs) = published.published.delivery_outputs() {
        for (name, object) in outputs {
            attachments.insert(
                LogicalPath::parse(&format!("native-outputs/{name}"))?,
                object.clone(),
            );
        }
    }
    Ok(attachments)
}
impl ProtectedJob<'_, '_> {
    pub(crate) fn source_identity(&self) -> &Key {
        &self.stamp.identity.source
    }
    pub(crate) fn verify_inputs(&self, journal: &mut SourceJournal) -> Result<()> {
        self.planned(journal)
    }
    pub fn selection(&self) -> &Selection {
        &self.stamp.selection
    }
    pub fn revision(&self) -> u64 {
        self.stamp.revision
    }
    fn planned(&self, journal: &mut SourceJournal) -> Result<()> {
        if self.output_handoff.is_some() {
            bail!("native job outputs already adopted");
        }
        check(journal, &self.stamp)
    }
    pub fn render(
        &mut self,
        journal: &mut SourceJournal,
        source: &mut Source,
        job_directory: &Path,
        output: &Path,
    ) -> Result<ManagedCandidate> {
        self.planned(journal)?;
        let render = match self.plan.mode {
            RecipeMode::Declared => super::render_with_programs,
            RecipeMode::Projected => super::render_projected_with_programs,
        };
        let candidate = render(
            source,
            self.plan.request,
            self.plan.context,
            self.plan.programs,
            job_directory,
            output,
        )?;
        let recipe = candidate.recipe();
        if recipe.fingerprint() != self.plan.recipe.fingerprint()
            || recipe.checkpoint() != self.plan.recipe.checkpoint()
            || recipe.binding() != self.plan.recipe.binding()
            || recipe.admission() != self.plan.recipe.admission()
            || recipe.operation() != self.plan.recipe.operation()
        {
            bail!("native managed candidate differs from planned recipe");
        }
        self.planned(journal)?;
        Ok(ManagedCandidate {
            stamp: self.stamp.clone(),
            candidate,
        })
    }
    pub fn publish(
        &mut self,
        journal: &mut SourceJournal,
        candidate: &ManagedCandidate,
        bindings: &[OutputBinding<'_>],
        publication: &Publication,
        backend: &mut impl PublicationBackend,
    ) -> Result<ManagedPublication> {
        self.planned(journal)?;
        if candidate.stamp != self.stamp {
            bail!("native render belongs to another protected job");
        }
        let published = candidate
            .candidate
            .publish(bindings, publication, backend)?;
        self.planned(journal)?;
        Ok(ManagedPublication {
            stamp: self.stamp.clone(),
            published,
        })
    }
    /// Publish the complete native delivery under this exact active job stamp.
    /// Freeze/readback/adoption must retain its returned complete attachment set.
    pub fn publish_delivery(
        &mut self,
        journal: &mut SourceJournal,
        candidate: &ManagedCandidate,
        bindings: &[OutputBinding<'_>],
        publication: &Publication,
        backend: &mut impl PublicationBackend,
        budget: &DeliveryBudget,
    ) -> Result<ManagedPublication> {
        self.planned(journal)?;
        if candidate.stamp != self.stamp {
            bail!("native render belongs to another protected job");
        }
        let published =
            candidate
                .candidate
                .publish_delivery(bindings, publication, backend, budget)?;
        self.planned(journal)?;
        Ok(ManagedPublication {
            stamp: self.stamp.clone(),
            published,
        })
    }
    pub fn freeze_outputs(
        &mut self,
        journal: &mut SourceJournal,
        published: &ManagedPublication,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<job::JobContext> {
        self.planned(journal)?;
        if published.stamp != self.stamp {
            bail!("native publication belongs to another protected job");
        }
        let attachments = attachments(published)?;
        let resources = job::freeze(
            &self.plan.admission(Some(job::OutputEvidence {
                claim: published.published.claim(),
                attachments: &attachments,
            })),
            store,
            limits,
        )?;
        self.plan.put_manifest(store)?;
        self.planned(journal)?;
        Ok(resources)
    }
    pub fn readback_outputs(
        &mut self,
        journal: &mut SourceJournal,
        published: &ManagedPublication,
        selection: &Selection,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<OutputReadback> {
        self.planned(journal)?;
        if published.stamp != self.stamp {
            bail!("native publication belongs to another protected job");
        }
        self.plan.verify_manifest(store)?;
        let attachments = attachments(published)?;
        let resources = job::restore(
            selection,
            &self.plan.admission(Some(job::OutputEvidence {
                claim: published.published.claim(),
                attachments: &attachments,
            })),
            store,
            limits,
        )?;
        self.planned(journal)?;
        Ok(OutputReadback {
            stamp: self.stamp.clone(),
            resources,
        })
    }
    pub fn adopt_outputs(
        &mut self,
        journal: &mut SourceJournal,
        readback: &OutputReadback,
        operation: &Key,
    ) -> Result<()> {
        if journal.identity() != &self.stamp.journal {
            bail!("native output handoff belongs to another journal authority");
        }
        if let Some((completed, inputs)) = &self.output_handoff {
            if completed != operation
                || &readback.stamp != inputs
                || readback.resources.selection() != &self.stamp.selection
            {
                bail!("native output retry differs from completed handoff");
            }
            return check(journal, &self.stamp);
        }
        if readback.stamp != self.stamp {
            bail!("native output handoff belongs to another job or phase");
        }
        // The exact journal commands recover an interrupted prepare/adopt. They
        // fence changed requests/revisions; post-check rejects historical replay
        // after a later transition. No long SQL transaction spans native work.
        let inputs = self.stamp.clone();
        self.stamp = adopt(
            journal,
            &self.stamp.identity,
            self.stamp.revision,
            operation,
            readback.resources.selection(),
        )?;
        self.output_handoff = Some((operation.clone(), inputs));
        Ok(())
    }
}
