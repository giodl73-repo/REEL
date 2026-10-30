//! Sealed native composition results; publication is not selection or custody.
use super::{CompositionJobPlan, ParentControls, ProtectedComposition, ProtectedScenes};
use crate::{
    episode_conform::{self, SceneControls},
    scene_delivery::{FileRef, InputFiles},
    tig_inputs::Source,
    tig_scene_production::{DeliveryBudget, OutputBinding, Publication, PublishedCandidate},
};
use anyhow::{Result, bail};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use tig_assets::graph::recipe::{
    Plan,
    produced::{Claim, ContextPin},
};
use tig_core::{
    Digest, LogicalPath, Object,
    tree::{NodeStore, TreeLimits},
};
use tig_git::source::native::job;
use tig_store::source_claims::Selection;
use tig_store::{PublicationBackend, source_claims::SourceJournal};

/// Explicit guarded byte providers. The parent manifest is admitted by the
/// composition recipe; source/output providers cannot replace its control pins.
pub struct CompositionFiles<'a> {
    pub parent: &'a ParentControls,
    pub scenes: &'a BTreeMap<String, SceneControls<'a>>,
    pub inputs: &'a BTreeMap<String, &'a dyn InputFiles>,
    pub outputs: &'a BTreeMap<String, &'a dyn InputFiles>,
}
/// Exact metadata/source/output providers for the independently verified
/// presentation components. The managed plan separately owns their currency.
pub struct PresentationComponents<'a> {
    pub files: &'a BTreeMap<String, episode_conform::PresentationFiles<'a>>,
    pub controls: &'a BTreeMap<String, &'a dyn InputFiles>,
}

/// Only constructed after native full-frame/PCM/timestamp verification under
/// current parent/child ownership. Borrowing the plan retains the parent fence.
/// No deserialization/constructor, creative approval or eviction authority.
pub struct RenderedComposition<'a> {
    owner: &'a crate::tig_scene_production::jobs::ScopedSource,
    recipe: &'a Plan,
    context: ContextPin,
    receipt: episode_conform::Receipt,
    receipt_object: Object,
    outputs: BTreeMap<String, FileRef>,
    output: PathBuf,
}

/// Complete byte acknowledgements bound to one native composition plan.
/// Not independent custody, a new selection, or authority to retire children.
pub struct PublishedComposition<'a> {
    recipe: &'a Plan,
    published: PublishedCandidate,
}
impl PublishedComposition<'_> {
    pub fn published(&self) -> &PublishedCandidate {
        &self.published
    }
}

impl ProtectedComposition<'_, '_> {
    /// Run through the existing bounded job scheduler in production. This low
    /// level primitive neither creates capacity reservations nor limits scratch.
    /// Explicit control providers remove any source-checkout fallback.
    pub fn assemble(
        &self,
        source: &Source,
        children: &ProtectedScenes<'_, '_>,
        journal: &mut SourceJournal,
        output: &Path,
        files: CompositionFiles<'_>,
    ) -> Result<RenderedComposition<'_>> {
        self.assemble_with_presentations(
            source,
            children,
            journal,
            output,
            files,
            PresentationComponents {
                files: &BTreeMap::new(),
                controls: &BTreeMap::new(),
            },
        )
    }
    /// Recheck current scene and presentation owners before and after native
    /// assembly, retaining every borrowed child guard through the operation.
    /// Explicit provider refusals never fall back to a historical checkout.
    pub fn assemble_with_presentations(
        &self,
        source: &Source,
        children: &ProtectedScenes<'_, '_>,
        journal: &mut SourceJournal,
        output: &Path,
        files: CompositionFiles<'_>,
        presentations: PresentationComponents<'_>,
    ) -> Result<RenderedComposition<'_>> {
        let plan = self.plan();
        if plan.current_controls.is_some() {
            if !presentations.files.is_empty() || !presentations.controls.is_empty() {
                bail!(
                    "current native presentations use their sealed providers, not caller replacements"
                );
            }
        } else if !plan.presentations.keys().eq(presentations.files.keys())
            || !plan.presentations.keys().eq(presentations.controls.keys())
        {
            bail!("composition presentation providers differ from verified component coverage");
        }
        let reference = files.parent.manifest_ref();
        let baseline = plan
            .current_controls
            .as_ref()
            .map_or(&plan.conform, |c| &c.baseline);
        if reference.sha256 != baseline.sha256.as_str() || reference.bytes != baseline.bytes {
            bail!("composition controls differ from admitted conform");
        }
        self.with_current(source, children, journal, |_| {
            let local = PathBuf::from(".");
            let programs = plan.programs.with_inputs(&local);
            let receipt = if let Some(compiled) = &plan.current_controls {
                let controls = super::current_controls::CurrentParent {
                    baseline: files.parent,
                    compiled,
                };
                episode_conform::build_with_current_presentations(
                    &compiled.reference,
                    output,
                    episode_conform::Files {
                        scene_inputs: files.inputs,
                        scene_outputs: files.outputs,
                        presentations: presentations.files,
                        programs: &programs,
                    },
                    &controls,
                    files.scenes,
                    &plan.presentations,
                )?
            } else {
                episode_conform::build_with_component_controls(
                    reference,
                    output,
                    episode_conform::Files {
                        scene_inputs: files.inputs,
                        scene_outputs: files.outputs,
                        presentations: presentations.files,
                        programs: &programs,
                    },
                    episode_conform::ComponentControls {
                        parent: files.parent,
                        scenes: files.scenes,
                        presentations: presentations.controls,
                    },
                )?
            };
            if receipt.manifest_sha256 != plan.conform.sha256.as_str()
                || !receipt.decoded_master_matches_ordered_segments
                || !receipt.timestamps_verified
            {
                bail!("composition native verification is incomplete");
            }
            let bytes = read_receipt(output)?;
            if serde_json::to_value(tig_core::identity::parse_json(&bytes)?)?
                != serde_json::to_value(&receipt)?
            {
                bail!("composition native receipt changed after verification");
            }
            let mut outputs = BTreeMap::new();
            if receipt
                .encoded_delivery
                .as_ref()
                .is_none_or(|e| e.lossless_master_retained)
            {
                outputs.insert(
                    "master.mkv".into(),
                    FileRef {
                        path: "master.mkv".into(),
                        sha256: receipt.master_sha256.clone(),
                        bytes: receipt.master_bytes,
                    },
                );
            }
            if let Some(encoded) = &receipt.encoded_delivery {
                if encoded.filename != "episode.mp4" || !encoded.full_decode_verified {
                    bail!("composition encoded delivery is incomplete");
                }
                outputs.insert(
                    encoded.filename.clone(),
                    FileRef {
                        path: encoded.filename.clone().into(),
                        sha256: encoded.sha256.clone(),
                        bytes: encoded.bytes,
                    },
                );
            }
            let candidate = RenderedComposition {
                owner: self.source_guard(),
                recipe: &plan.recipe,
                context: ContextPin {
                    root: plan.context.selection().root.clone(),
                    identity: plan.context.selection().context.clone(),
                },
                receipt,
                receipt_object: Object::from_bytes(&bytes),
                outputs,
                output: output.canonicalize()?,
            };
            candidate.checked_outputs()?;
            Ok(candidate)
        })
    }

    /// Ownership remains current across publication, including errors. Each
    /// acknowledged prefix survives for exact retry; no sources are withdrawn.
    #[allow(clippy::too_many_arguments)]
    pub fn publish_delivery(
        &self,
        source: &Source,
        children: &ProtectedScenes<'_, '_>,
        journal: &mut SourceJournal,
        candidate: &RenderedComposition<'_>,
        bindings: &[OutputBinding<'_>],
        publication: &Publication,
        backend: &mut impl PublicationBackend,
        budget: &DeliveryBudget,
    ) -> Result<PublishedComposition<'_>> {
        if !std::ptr::eq(candidate.recipe, &self.plan().recipe)
            || !std::ptr::eq(candidate.owner, self.source_guard())
        {
            bail!("composition candidate belongs to a different protected job");
        }
        self.with_current(source, children, journal, |_| {
            let claim = candidate.claim(bindings)?;
            let receipt = read_receipt(&candidate.output)?;
            let mut outputs = Vec::new();
            let mut named = BTreeMap::new();
            for (name, reference) in &candidate.outputs {
                let object = Object {
                    sha256: Digest::parse(&reference.sha256)?,
                    bytes: reference.bytes,
                };
                let path = crate::scene_delivery::checked_file(&candidate.output, reference)?;
                outputs.push((publication.request("output", name, object.clone())?, path));
                named.insert(name.clone(), object);
            }
            let published = crate::tig_scene_production::publication::publish_checked(
                claim,
                &receipt,
                &candidate.receipt_object,
                outputs,
                Some(named),
                publication,
                backend,
                Some(budget),
            )?;
            Ok(PublishedComposition {
                recipe: &self.plan().recipe,
                published,
            })
        })
    }
}

impl CompositionJobPlan<'_> {
    fn output_attachments(
        &self,
        published: &PublishedComposition<'_>,
    ) -> Result<BTreeMap<LogicalPath, Object>> {
        if !std::ptr::eq(published.recipe, &self.recipe) {
            bail!("composition publication belongs to a different plan");
        }
        let mut attachments = BTreeMap::from([(
            LogicalPath::parse("native-receipt")?,
            published.published.receipt_object().clone(),
        )]);
        let outputs = published
            .published
            .delivery_outputs()
            .ok_or_else(|| anyhow::anyhow!("composition delivery is incomplete"))?;
        for (name, object) in outputs {
            attachments.insert(
                LogicalPath::parse(&format!("native-outputs/{name}"))?,
                object.clone(),
            );
        }
        Ok(attachments)
    }
    /// Candidate retained resource metadata. Keep all child source obligations;
    /// this parent still does not enroll their full primary/control closures.
    pub fn freeze_outputs(
        &self,
        published: &PublishedComposition<'_>,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<job::JobContext> {
        let attachments = self.output_attachments(published)?;
        self.put_interpretation(store)?;
        let mut admission = self.admission();
        admission.output = Some(job::OutputEvidence {
            claim: published.published.claim(),
            attachments: &attachments,
        });
        Ok(job::freeze_with_dependencies(
            &admission,
            &self.dependencies(),
            None,
            store,
            limits,
        )?)
    }
    /// Reopen and verify the exact complete output resource selection, retaining
    /// composition/evidence/runtime roles. This is not live owner adoption.
    pub fn readback_outputs(
        &self,
        published: &PublishedComposition<'_>,
        selection: &Selection,
        store: &mut impl NodeStore,
        limits: TreeLimits,
    ) -> Result<job::JobContext> {
        let attachments = self.output_attachments(published)?;
        self.verify_interpretation(store)?;
        let mut admission = self.admission();
        admission.output = Some(job::OutputEvidence {
            claim: published.published.claim(),
            attachments: &attachments,
        });
        Ok(job::restore_with_dependencies(
            selection,
            &admission,
            &self.dependencies(),
            None,
            store,
            limits,
        )?)
    }
}
impl RenderedComposition<'_> {
    pub fn receipt(&self) -> &episode_conform::Receipt {
        &self.receipt
    }
    pub fn receipt_object(&self) -> &Object {
        &self.receipt_object
    }
    pub fn recipe(&self) -> &Plan {
        self.recipe
    }
    fn checked_outputs(&self) -> Result<()> {
        if Object::from_bytes(&read_receipt(&self.output)?) != self.receipt_object {
            bail!("composition native receipt changed");
        }
        for reference in self.outputs.values() {
            crate::scene_delivery::checked_file(&self.output, reference)?;
        }
        Ok(())
    }
    /// Unsigned correspondence for exact caller-admitted output subjects.
    /// Rechecks all retained output bytes; approval and custody stay separate.
    pub fn claim(&self, bindings: &[OutputBinding<'_>]) -> Result<Claim> {
        if bindings.is_empty() || bindings.len() > 256 {
            bail!("composition output mapping exceeds budget");
        }
        self.checked_outputs()?;
        let mut subjects = Vec::new();
        for binding in bindings {
            let reference = self
                .outputs
                .get(binding.output_name)
                .ok_or_else(|| anyhow::anyhow!("unknown composition output"))?;
            let object = Object {
                sha256: Digest::parse(&reference.sha256)?,
                bytes: reference.bytes,
            };
            if binding.subject.subject().object() != &object {
                bail!("composition output differs from admitted asset subject");
            }
            subjects.push(binding.subject.subject().clone());
        }
        Ok(Claim::create(self.recipe, self.context.clone(), subjects)?)
    }
}
fn read_receipt(output: &Path) -> Result<Vec<u8>> {
    const MAX: u64 = 4 * 1024 * 1024;
    let mut bytes = Vec::new();
    fs::File::open(output.join("receipt.json"))?
        .take(MAX + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX {
        bail!("composition receipt exceeds metadata budget");
    }
    Ok(bytes)
}
