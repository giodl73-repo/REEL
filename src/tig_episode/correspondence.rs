//! Native source correspondence, separate from bytes, execution and selection.
use super::*;
use crate::tig_scene_production::jobs::JobPlan;
use tig_assets::graph::recipe::produced::{self, BoundSubject, Correspondence, Selector};
use tig_git::source::native::produced::ReplayedClaim;

/// Created only after exact native-current-job versus original-claim comparison.
/// This is metadata correspondence, not byte validation, execution or custody.
#[derive(Debug)]
pub struct CurrentScene {
    checkpoint: Digest,
    output: Reference,
    scene: Reference,
    selected: Object,
    claim: Object,
    recipe: Digest,
    protection_recipe: Digest,
    producer: Object,
    binding: Digest,
    admission: Digest,
    production_definitions: BTreeSet<String>,
}
impl CurrentScene {
    pub(super) fn checkpoint(&self) -> &Digest {
        &self.checkpoint
    }
    pub(super) fn output(&self) -> &Reference {
        &self.output
    }
    pub(super) fn scene(&self) -> &Reference {
        &self.scene
    }
    pub(super) fn protection_recipe(&self) -> &Digest {
        &self.protection_recipe
    }
    pub(super) fn matches_job(&self, plan: &JobPlan<'_>) -> bool {
        let recipe = plan.recipe();
        self.checkpoint == *recipe.checkpoint()
            && self.protection_recipe == *recipe.fingerprint()
            && self.binding == *recipe.binding()
            && self.admission == *recipe.admission()
            && plan
                .native_request()
                .is_ok_and(|request| request.scene_id == self.scene.id)
            && recipe.nodes().contains_key(&self.scene)
    }
    pub fn selected(&self) -> &Object {
        &self.selected
    }
    pub fn claim(&self) -> &Object {
        &self.claim
    }
    pub fn recipe(&self) -> &Digest {
        &self.recipe
    }
    /// Producer explicitly admitted for the matched retained output, which may
    /// differ from the current checker/ownership job. No execution attestation.
    pub fn matched_producer(&self) -> &Object {
        &self.producer
    }
}

/// Reuse independently replayed original correspondence and an actual native
/// JobPlan derived from current source. No payload lookup, rendering or status
/// promotion. Caller admits the original producer/request/archive separately.
pub fn check_current_scene(
    source: &Source,
    output_id: &str,
    current: &JobPlan<'_>,
    original: &ReplayedClaim<'_>,
) -> Result<CurrentScene> {
    check_recipe(source, output_id, current, current.recipe(), original)
}

/// Caller independently authorizes this original producer for current reuse.
/// Recomputes current native consumed meaning with that pin, without weakening
/// interpreter/parameter checks or pretending the checker executes old code.
/// Projected mode requires new bounded preparation scratch; declared mode None.
/// Actual native byte checks, generated-input eligibility and approval stay separate.
pub fn check_current_scene_with_retained_producer(
    source: &mut Source,
    output_id: &str,
    current: &JobPlan<'_>,
    original: &ReplayedClaim<'_>,
    producer: &Object,
    preparation: Option<&std::path::Path>,
) -> Result<CurrentScene> {
    if &original.checked().claim().operation().producer != producer {
        bail!("retained producer differs from independent reuse admission");
    }
    let recipe = current.recipe_for_producer(source, producer, preparation)?;
    check_recipe(source, output_id, current, &recipe, original)
}

fn check_recipe(
    source: &Source,
    output_id: &str,
    current: &JobPlan<'_>,
    recipe: &tig_assets::graph::recipe::Plan,
    original: &ReplayedClaim<'_>,
) -> Result<CurrentScene> {
    let request = current.native_request()?;
    let scene = source.record(&request.scene_id)?;
    let output = source.record(output_id)?;
    let native_scene: String = field(&scene, "scene_id")?;
    let desired: Id = field(&output, "scene")?;
    let definition = &output.definition().id;
    if current.recipe().checkpoint() != source.checkpoint()
        || !current.recipe().nodes().contains_key(scene.reference())
        || scene.definition().id != request.scene_definition
        || desired.id != request.scene_id
        || field::<String>(&output, "source_segment_id")? != native_scene
        || field::<String>(&output, "language")? != request.language
        || field::<String>(&output, "component_kind")? != "scene"
        || definition == &request.scene_definition
        || request.primary_definitions.contains(definition)
        || request.control_definitions.contains(definition)
        || !matches!(source.admission(definition)?.payload, Payload::Selected {})
        || !original
            .context()
            .roots()
            .iter()
            .any(|r| r.id == request.scene_id)
    {
        bail!("scene correspondence differs from admitted scope or current source");
    }
    let subject = BoundSubject::resolve(&output, output.definition(), Selector::Selected {})?
        .context("scene component has no selected output")?;
    if produced::compare(recipe, Some(&subject), Some(original.checked()))?
        != Correspondence::ClaimedRecipeMatch
    {
        bail!("selected scene output has a stale current-source recipe");
    }
    Ok(CurrentScene {
        checkpoint: source.checkpoint().clone(),
        output: output.reference().clone(),
        scene: scene.reference().clone(),
        selected: subject.subject().object().clone(),
        claim: original.checked().claim().object().clone(),
        recipe: recipe.fingerprint().clone(),
        protection_recipe: current.recipe().fingerprint().clone(),
        producer: recipe.operation().producer.clone(),
        binding: current.recipe().binding().clone(),
        admission: current.recipe().admission().clone(),
        production_definitions: request
            .primary_definitions
            .iter()
            .chain(request.control_definitions.iter())
            .cloned()
            .collect(),
    })
}

/// Every planned native scene needs exact current correspondence. Presentation
/// currency, output bytes and complete managed-job admission remain separate.
/// Replan in this source view; a public/mutable Plan cannot grant this gate.
pub fn require_current_scenes(
    source: &mut Source,
    request: &Request,
    scenes: &BTreeMap<String, CurrentScene>,
) -> Result<Plan> {
    require_scene_components(source, request, scenes, &BTreeMap::new())
}
pub(super) fn require_scene_components(
    source: &mut Source,
    request: &Request,
    scenes: &BTreeMap<String, CurrentScene>,
    references: &BTreeMap<String, &crate::tig_scene_production::jobs::reference::VerifiedScene<'_>>,
) -> Result<Plan> {
    let plan = plan(source, request)?;
    let expected = plan
        .components
        .iter()
        .filter(|c| c.kind == "scene")
        .collect::<Vec<_>>();
    if expected.len() != scenes.len() + references.len()
        || scenes.keys().any(|id| references.contains_key(id))
        || expected.iter().any(|c| {
            if let Some(proof) = references.get(&c.segment_id) {
                return proof.checkpoint() != &plan.source_checkpoint
                    || proof.output() != &c.record
                    || Some(proof.scene()) != c.source_scene.as_ref()
                    || proof.selected().sha256 != c.selected.sha256.as_str()
                    || proof.selected().bytes != c.selected.bytes
                    || request
                        .component_definitions
                        .iter()
                        .any(|id| proof.production_definitions().contains(id));
            }
            scenes.get(&c.segment_id).is_none_or(|proof| {
                request
                    .component_definitions
                    .iter()
                    .any(|id| proof.production_definitions.contains(id))
                    || proof.checkpoint != plan.source_checkpoint
                    || proof.output != c.record
                    || Some(&proof.scene) != c.source_scene.as_ref()
                    || proof.selected != c.selected
            })
        })
    {
        bail!("episode scene correspondence requires exact current coverage");
    }
    Ok(plan)
}
