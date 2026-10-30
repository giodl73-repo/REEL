//! Metadata-only declared input closure. This cannot grant render eligibility.
use super::*;
use crate::{tig_presentation, tig_scene_production::jobs::reference::ReferencePlan};
use tig_assets::graph::recipe::Plan;

pub(super) struct Root {
    pub resources: job::JobContext,
    pub recipe: Plan,
    pub outputs: std::collections::BTreeSet<String>,
    runtime: BTreeMap<LogicalPath, Object>,
    composition: BTreeMap<LogicalPath, Object>,
    evidence: BTreeMap<LogicalPath, Object>,
}
impl Root {
    fn admission<'a>(&'a self, context: &'a ProductionContext) -> job::Admission<'a> {
        job::Admission {
            context,
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
}
fn enroll(
    evidence: &mut BTreeMap<LogicalPath, Object>,
    id: &str,
    resources: &job::JobContext,
) -> Result<()> {
    for object in resources.objects() {
        evidence.insert(
            LogicalPath::parse(&format!(
                "children/{}/{}",
                Digest::blob(id.as_bytes()).as_str(),
                object.sha256.as_str()
            ))?,
            object.clone(),
        );
        if evidence.len() > 60_000 {
            bail!("episode declared resource entry budget exceeded");
        }
    }
    Ok(())
}
pub(super) fn freeze(
    inputs: &Inputs,
    source: &mut Source,
    context: &ProductionContext,
    programs: &BoundPrograms,
    store: &mut impl NodeStore,
    preparation: &Path,
) -> Result<Root> {
    let plan = tig_episode::plan(source, &inputs.request)?;
    let scenes = inputs
        .scenes
        .iter()
        .map(|s| (s.id.as_str(), s))
        .collect::<BTreeMap<_, _>>();
    let presentations = inputs
        .presentations
        .iter()
        .map(|s| (s.id.as_str(), s))
        .collect::<BTreeMap<_, _>>();
    if scenes.len() != inputs.scenes.len()
        || presentations.len() != inputs.presentations.len()
        || scenes.len() + presentations.len() != plan.components.len()
        || scenes.keys().any(|id| presentations.contains_key(id))
    {
        bail!("episode worker requires unique complete component coverage");
    }
    let mut composition = BTreeMap::new();
    let mut evidence = BTreeMap::new();
    for component in &plan.components {
        if component.kind == "scene" {
            let spec = scenes
                .get(component.segment_id.as_str())
                .context("scene admission missing")?;
            if spec.output_id != component.record.id
                || spec.request.language != plan.language
                || component
                    .source_scene
                    .as_ref()
                    .is_none_or(|scene| scene.id != spec.request.scene_id)
            {
                bail!("episode scene admission differs from source scope");
            }
        } else {
            let spec = presentations
                .get(component.segment_id.as_str())
                .context("presentation admission missing")?;
            if spec.output_id != component.record.id {
                bail!("episode presentation admission differs from source");
            }
        }
        composition.insert(
            LogicalPath::parse(Digest::blob(component.segment_id.as_bytes()).as_str())?,
            component.selected.clone(),
        );
    }
    let manifest: crate::episode_conform::Manifest =
        serde_json::from_slice(&source.read_metadata(&plan.conform, 4 * 1024 * 1024)?)?;
    let mut outputs = std::collections::BTreeSet::from(["receipt.json".to_owned()]);
    if manifest
        .compact_delivery
        .as_ref()
        .is_none_or(|c| c.retain_lossless_master)
    {
        outputs.insert("master.mkv".into());
    }
    if manifest.compact_delivery.is_some() {
        outputs.insert("episode.mp4".into());
    }
    for (index, spec) in inputs.scenes.iter().enumerate() {
        let archive: ArchiveProfile = serde_json::from_value(inputs.source.clone())?;
        let mut child = Source::from_archive(archive, store, TreeLimits::default())?;
        let context = if spec.compiled {
            tig_scene::freeze_render_context(
                &mut child,
                &spec.request,
                &preparation.join(format!("context-{index}")),
                store,
                TreeLimits::default(),
            )?
        } else {
            tig_scene::freeze_context(&mut child, &spec.request, store, TreeLimits::default())?
        };
        let native = if spec.compiled {
            jobs::JobPlan::from_render_context(
                &mut child,
                &spec.request,
                &context,
                programs,
                &preparation.join(format!("recipe-{index}")),
            )?
        } else {
            jobs::JobPlan::from_context(&mut child, &spec.request, &context, programs)?
        };
        let reference = ReferencePlan::compile(
            &mut child,
            &native,
            &spec.output_id,
            &spec.output_definition,
            &preparation.join(format!("scene-{index}")),
        )?;
        enroll(
            &mut evidence,
            &spec.id,
            &reference.freeze(store, TreeLimits::default())?,
        )?;
    }
    for spec in &inputs.presentations {
        let mut child = Source::from_archive(
            serde_json::from_value(inputs.source.clone())?,
            store,
            TreeLimits::default(),
        )?;
        let context = tig_presentation::freeze_context(
            &mut child,
            &spec.request,
            store,
            TreeLimits::default(),
        )?;
        let reference = tig_presentation::jobs::JobPlan::from_context(
            &mut child,
            &spec.request,
            &context,
            programs,
            &spec.output_id,
            &spec.output_definition,
        )?;
        enroll(
            &mut evidence,
            &spec.id,
            &reference.freeze(store, TreeLimits::default())?,
        )?;
    }
    let producer = Object::from_bytes(programs.manifest_bytes());
    let recipe = tig_episode::composition_recipe(source, &inputs.request, context, &producer)?;
    if store.put(programs.manifest_bytes())? != producer {
        bail!("episode runtime manifest changed");
    }
    let runtime = runtime(programs)?;
    let resources = job::freeze_with_dependencies(
        &job::Admission {
            context,
            plan: &recipe,
            runtime: &runtime,
            output: None,
        },
        &job::Dependencies {
            composition: &composition,
            evidence: &evidence,
        },
        None,
        store,
        TreeLimits::default(),
    )?;
    Ok(Root {
        outputs,
        resources,
        recipe,
        runtime,
        composition,
        evidence,
    })
}
pub(super) fn readback(
    _inputs: &Inputs,
    source: &Source,
    context: &ProductionContext,
    programs: &BoundPrograms,
    root: &Root,
    store: &mut impl NodeStore,
) -> Result<()> {
    if root.recipe.checkpoint() != source.checkpoint()
        || store.get(&root.recipe.operation().producer)?.as_deref()
            != Some(programs.manifest_bytes())
    {
        bail!("episode planned runtime or source changed");
    }
    let restored = job::restore_with_dependencies(
        root.resources.selection(),
        &root.admission(context),
        &root.dependencies(),
        None,
        store,
        TreeLimits::default(),
    )?;
    if restored.objects() != root.resources.objects() {
        bail!("episode planned resources differ after readback");
    }
    Ok(())
}
