//! Explicit logical component composition from one captured Tig view.
//! Planning does not qualify producer currency, decode media or execute a render.
use crate::{
    episode_conform::{Manifest, SegmentKind},
    tig_inputs::{Payload, Source},
};
use anyhow::{Context, Result, bail};
use reel_assembly::scene_authoring::Episode;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::Path,
};
use tig_assets::{graph::Reference, resolve::Resolved};
use tig_core::{Digest, Object, identity};

pub const SCHEMA: &str = "reel.tig-episode-plan-request.experimental-1";
mod context;
pub use context::{composition_recipe, freeze_composition_context, restore_composition_context};
#[cfg(feature = "tig-publication")]
mod correspondence;
#[cfg(feature = "tig-publication")]
pub use correspondence::{
    CurrentScene, check_current_scene, check_current_scene_with_retained_producer,
    require_current_scenes,
};
#[cfg(feature = "tig-publication")]
mod protection;
#[cfg(feature = "tig-publication")]
pub use protection::{ProtectedScenes, ProtectionBudget, SceneAdmission, protect_current_scenes};
#[cfg(feature = "tig-publication")]
mod jobs;
#[cfg(feature = "tig-publication")]
pub use jobs::{
    CompositionJobPlan, CompositionOwnership, CompositionReadback, ProtectedComposition,
};
#[cfg(feature = "tig-publication")]
mod controls;
#[cfg(feature = "tig-publication")]
mod current_controls;
#[cfg(feature = "tig-publication")]
mod scene_controls;
#[cfg(feature = "tig-publication")]
pub use controls::{ControlTree, ParentControls};
#[cfg(feature = "tig-publication")]
pub use scene_controls::CurrentSceneControls;
#[cfg(feature = "tig-publication")]
mod production;
#[cfg(feature = "tig-publication")]
pub use production::{
    CompositionFiles, PresentationComponents, PublishedComposition, RenderedComposition,
};
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub assembly_id: String,
    pub assembly_definition: String,
    pub episode_definition: String,
    pub scene_definitions: Vec<String>,
    pub component_definitions: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Assembly {
    episode: Id,
    language: String,
    conform_slot: String,
    components: BTreeMap<String, Id>,
}
#[derive(Serialize)]
pub struct Component {
    pub position: usize,
    pub segment_id: String,
    pub kind: String,
    pub record: Reference,
    pub selected: Object,
    pub baseline: Object,
    pub changed_from_baseline: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scene: Option<Reference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_scene_selected: Option<Object>,
}
#[derive(Serialize)]
pub struct Plan {
    pub schema: &'static str,
    pub source_checkpoint: Digest,
    pub assembly: Reference,
    pub episode: Reference,
    pub episode_selected: Object,
    pub conform: Object,
    pub language: String,
    pub components: Vec<Component>,
    pub unique_output_objects: usize,
    pub unique_output_bytes: u64,
    pub changed_components: usize,
    pub executable: bool,
    pub production_currency_verified: bool,
    pub payloads_verified: bool,
    pub authorizes_eviction: bool,
}
fn token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
fn field<T: serde::de::DeserializeOwned>(record: &Resolved, key: &str) -> Result<T> {
    Ok(serde_json::from_value(
        record
            .authored()
            .get(key)
            .context("required native field missing")?
            .clone(),
    )?)
}
impl Request {
    pub fn read(path: &Path) -> Result<Self> {
        let mut raw = Vec::new();
        fs::File::open(path)?.take(65_537).read_to_end(&mut raw)?;
        if raw.len() > 65_536 {
            bail!("episode request exceeds budget");
        }
        Ok(serde_json::from_value(identity::parse_json(&raw)?)?)
    }
    fn validate(&self) -> Result<()> {
        let mut definitions = BTreeSet::new();
        for id in [&self.assembly_definition, &self.episode_definition]
            .into_iter()
            .chain(self.scene_definitions.iter())
            .chain(self.component_definitions.iter())
        {
            if !token(id) || !definitions.insert(id) {
                bail!("episode definition roles must be distinct");
            }
        }
        if self.schema != SCHEMA
            || !token(&self.assembly_id)
            || self.scene_definitions.is_empty()
            || self.scene_definitions.len() > 32
            || self.component_definitions.is_empty()
            || self.component_definitions.len() > 32
        {
            bail!("invalid bounded episode planning request");
        }
        Ok(())
    }
}
fn plan_inner(source: &mut Source, request: &Request) -> Result<Plan> {
    request.validate()?;
    for id in [&request.assembly_definition, &request.episode_definition]
        .into_iter()
        .chain(request.scene_definitions.iter())
        .chain(request.component_definitions.iter())
    {
        if !matches!(source.admission(id)?.payload, Payload::Selected {}) {
            bail!("episode role requires an explicit selected-object admission");
        }
    }
    let assembly = source.record(&request.assembly_id)?;
    if assembly.definition().id != request.assembly_definition {
        bail!("assembly type differs");
    }
    let authored: Assembly =
        serde_json::from_value(serde_json::Value::Object(assembly.authored().clone()))?;
    if !token(&authored.episode.id)
        || authored.language.is_empty()
        || authored.language.len() > 32
        || !authored
            .language
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || !token(&authored.conform_slot)
        || authored.conform_slot.len() > 128
        || authored.components.is_empty()
        || authored.components.len() > 128
    {
        bail!("invalid assembly source or component count");
    }
    let episode = source.record(&authored.episode.id)?;
    if episode.definition().id != request.episode_definition {
        bail!("episode type differs");
    }
    let declared_episode: String = field(&episode, "episode_id")?;
    let scenes: Vec<Id> = field(&episode, "scene_ids")?;
    let scene_ids = scenes
        .iter()
        .map(|s| s.id.as_str())
        .collect::<BTreeSet<_>>();
    if scenes.len() > 128 || scene_ids.len() != scenes.len() || scene_ids.iter().any(|s| !token(s))
    {
        bail!("invalid episode scene identities");
    }
    let episode_selected = episode.selected()?.clone();
    let conform_pin = episode.stream(&authored.conform_slot)?.clone();
    let native: Episode = serde_json::from_value(identity::parse_json(
        &source.read_metadata(&episode_selected, 4 * 1024 * 1024)?,
    )?)?;
    let conform: Manifest = serde_json::from_value(identity::parse_json(
        &source.read_metadata(&conform_pin, 4 * 1024 * 1024)?,
    )?)?;
    if conform.schema != crate::episode_conform::SCHEMA
        || native.schema != reel_assembly::scene_authoring::EPISODE_SCHEMA
        || !token(&declared_episode)
        || conform.episode_id != declared_episode
        || native.episode_id != declared_episode
        || conform.language != authored.language
        || conform.segments.len() != authored.components.len()
        || native.scene_ids.len() != scenes.len()
        || conform.output_sample_rate == 0
    {
        bail!("episode conform scope or component count differs");
    }
    let mut mapped = BTreeSet::new();
    let mut outputs = BTreeMap::new();
    let mut sizes = BTreeMap::new();
    let mut components = Vec::new();
    let mut scene_index = 0;
    for (position, segment) in conform.segments.iter().enumerate() {
        if !token(&segment.id) || !mapped.insert(segment.id.clone()) {
            bail!("duplicate or invalid native segment");
        }
        let mapping = authored
            .components
            .get(&segment.id)
            .context("unmapped native component")?;
        if !token(&mapping.id) {
            bail!("invalid mapped component ID");
        }
        let record = source.record(&mapping.id)?;
        if !request
            .component_definitions
            .contains(&record.definition().id)
        {
            bail!("component output type differs");
        }
        let kind = match segment.kind {
            SegmentKind::Scene => "scene",
            SegmentKind::EpisodePresentation => "episode-presentation",
        };
        if field::<String>(&record, "language")? != authored.language
            || field::<String>(&record, "source_segment_id")? != segment.id
            || field::<String>(&record, "component_kind")? != kind
        {
            bail!("component scope differs");
        }
        let selected = record.selected()?.clone();
        let baseline: Object = serde_json::from_value(
            serde_json::json!({"sha256":segment.master.sha256,"bytes":segment.master.bytes}),
        )?;
        for object in [&selected, &baseline] {
            if sizes
                .insert(object.sha256.clone(), object.bytes)
                .is_some_and(|old| old != object.bytes)
            {
                bail!("component digest size differs");
            }
        }
        outputs.insert(selected.sha256.clone(), selected.bytes);
        let (source_scene, source_scene_selected) = match segment.kind {
            SegmentKind::Scene => {
                if native.scene_ids.get(scene_index) != Some(&segment.id) {
                    bail!("native scene order differs");
                }
                let expected = scenes.get(scene_index).context("extra native scene")?;
                let desired: Id = field(&record, "scene")?;
                if desired.id != expected.id {
                    bail!("component desired scene differs");
                }
                let scene = source.record(&expected.id)?;
                if !request.scene_definitions.contains(&scene.definition().id)
                    || field::<String>(&scene, "scene_id")? != segment.id
                    || field::<String>(&scene, "episode_id")? != declared_episode
                {
                    bail!("native scene type or scope differs");
                }
                scene_index += 1;
                (
                    Some(scene.reference().clone()),
                    Some(scene.selected()?.clone()),
                )
            }
            SegmentKind::EpisodePresentation => {
                if native
                    .presentation
                    .iter()
                    .filter(|p| p.role == segment.id)
                    .count()
                    != 1
                {
                    bail!("native presentation role is missing or ambiguous");
                }
                let desired: Id = field(&record, "episode")?;
                if desired.id != authored.episode.id {
                    bail!("presentation desired episode differs");
                }
                (None, None)
            }
        };
        components.push(Component {
            position,
            segment_id: segment.id.clone(),
            kind: kind.into(),
            record: record.reference().clone(),
            changed_from_baseline: selected != baseline,
            selected,
            baseline,
            source_scene,
            source_scene_selected,
        });
    }
    if scene_index != scenes.len() {
        bail!("episode scene coverage differs");
    }
    let changed_components = components
        .iter()
        .filter(|c| c.changed_from_baseline)
        .count();
    let unique_output_bytes = outputs
        .values()
        .try_fold(0u64, |sum, size| sum.checked_add(*size))
        .context("component byte sum overflow")?;
    Ok(Plan {
        schema: "reel.tig-episode-composition-plan.experimental-1",
        source_checkpoint: source.checkpoint().clone(),
        assembly: assembly.reference().clone(),
        episode: episode.reference().clone(),
        episode_selected,
        conform: conform_pin,
        language: authored.language,
        components,
        unique_output_objects: outputs.len(),
        unique_output_bytes,
        changed_components,
        executable: false,
        production_currency_verified: false,
        payloads_verified: false,
        authorizes_eviction: false,
    })
}
/// Exact logical composition only; no media lookup, current-producer assertion or rendering.
pub fn plan(source: &mut Source, request: &Request) -> Result<Plan> {
    plan_inner(source,request).map_err(|_|anyhow::anyhow!("episode composition planning refused; verify exact definitions, episode/language/order, component mappings and metadata bounds"))
}
pub fn write_plan(
    source: &mut Source,
    request: &Request,
    output: &Path,
) -> Result<serde_json::Value> {
    if !output.is_absolute()
        || output.symlink_metadata().is_ok()
        || !output.parent().is_some_and(|p| p.is_dir())
    {
        bail!("episode plan requires a new absolute file with an existing parent");
    }
    let plan = plan(source, request)?;
    let raw = serde_json::to_vec(&plan)?;
    if raw.len() > 4 * 1024 * 1024 {
        bail!("episode plan exceeds output budget");
    }
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output)?
        .write_all(&raw)?;
    Ok(
        serde_json::json!({"schema":"reel.tig-episode-planning-result.experimental-1","components":plan.components.len(),
        "unique_output_objects":plan.unique_output_objects,"unique_output_bytes":plan.unique_output_bytes,
        "changed_components":plan.changed_components,"executable":false,"payloads_verified":false,"authorizes_eviction":false}),
    )
}
