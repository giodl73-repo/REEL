//! Portable, scoped authoring for scenes and reusable presentation templates.
//! Creative selection remains with the owner. No template geometry or clock
//! seconds belong in a scene invocation.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CATALOG_SCHEMA: &str = "reel.scene-template-catalog.v1";
pub const EPISODE_SCHEMA: &str = "reel.episode-authoring.v1";
pub const SCENE_SCHEMA: &str = "reel.scene-authoring.v1";
pub const BINDINGS_SCHEMA: &str = "reel.scene-asset-bindings.v1";
pub const POLICY_SCHEMA: &str = "reel.scene-policy.v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenePolicy {
    pub schema: String,
    pub policy_id: String,
    pub target_composition_seconds_min: f64,
    pub target_composition_seconds_max: f64,
    pub hard_unchanged_composition_seconds_max: f64,
    pub semantic_cuts_required: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RenderedSpan {
    pub composition_id: String,
    pub start_sample: u64,
    pub end_sample: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CompositionRun {
    pub composition_id: String,
    pub start_sample: u64,
    pub end_sample: u64,
    pub target_hint: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssetRef {
    pub logical_id: String,
    pub sha256: String,
    pub bytes: u64,
    pub cache_uri: String,
    pub selection_state: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub template_id: String,
    pub kind: String,
    pub definition_sha256: String,
    pub required_content_keys: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateCatalog {
    pub schema: String,
    pub templates: Vec<Template>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationUse {
    pub role: String,
    pub template_id: String,
    pub content: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_binding: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreBinding {
    pub role: String,
    pub theme_id: String,
    pub source_poem_id: String,
    pub arrangement_id: String,
    pub asset_binding: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Episode {
    pub schema: String,
    pub episode_id: String,
    pub season_id: String,
    pub master_template_id: String,
    pub scene_policy_id: String,
    pub presentation: Vec<PresentationUse>,
    pub score_palette: Vec<ScoreBinding>,
    pub scene_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cue {
    pub cue_id: String,
    pub source_id: String,
    pub exact_text_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub take_binding: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phrase_alignment_binding: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub event_id: String,
    pub cue_id: String,
    pub semantic_trigger_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture_binding: Option<String>,
    pub score: ScoreUse,
    #[serde(default)]
    pub sonic_bindings: Vec<String>,
    #[serde(default)]
    pub vfx_bindings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "disposition", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ScoreUse {
    Role { role: String },
    Silence,
    Held,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Language {
    pub cues: Vec<Cue>,
    pub events: Vec<Event>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub schema: String,
    pub scene_id: String,
    pub episode_id: String,
    pub authoring_state: String,
    #[serde(default)]
    pub legacy_evidence: Vec<LegacyEvidence>,
    pub source_scope_ids: Vec<String>,
    pub source_evidence_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_policy_override_id: Option<String>,
    pub languages: BTreeMap<String, Language>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<PresentationUse>,
    #[serde(default)]
    pub continuity_tags: Vec<String>,
    #[serde(default)]
    pub holds: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyEvidence {
    pub path: String,
    pub sha256: String,
    pub status: String,
    #[serde(default)]
    pub event_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedBindings {
    pub schema: String,
    pub scope_id: String,
    pub assets: BTreeMap<String, AssetRef>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResolvedScene {
    pub scene_id: String,
    pub selected_inputs: BTreeMap<String, AssetRef>,
    pub template_definitions: BTreeMap<String, String>,
    pub score_roles: BTreeMap<String, String>,
    pub scene_policy_id: String,
    pub language_fingerprints: BTreeMap<String, String>,
    pub fingerprint_sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResolvedEpisodePresentation {
    pub episode_id: String,
    pub selected_inputs: BTreeMap<String, AssetRef>,
    pub template_definitions: BTreeMap<String, String>,
    pub fingerprint_sha256: String,
}

pub fn validate_policy(policy: &ScenePolicy) -> Result<()> {
    if policy.schema != POLICY_SCHEMA
        || policy.policy_id.is_empty()
        || !policy.target_composition_seconds_min.is_finite()
        || !policy.target_composition_seconds_max.is_finite()
        || !policy.hard_unchanged_composition_seconds_max.is_finite()
        || policy.target_composition_seconds_min <= 0.0
        || policy.target_composition_seconds_min > policy.target_composition_seconds_max
        || policy.target_composition_seconds_max > policy.hard_unchanged_composition_seconds_max
    {
        bail!("invalid scene policy");
    }
    Ok(())
}

/// Audit the visible result. Adjacent semantic events that reuse the same
/// composition form one continuous run, even if each event is individually short.
pub fn audit_rendered_compositions(
    policy: &ScenePolicy,
    sample_rate: u32,
    spans: &[RenderedSpan],
) -> Result<Vec<CompositionRun>> {
    validate_policy(policy)?;
    if sample_rate == 0 || spans.is_empty() {
        bail!("rendered picture clock is missing");
    }
    let mut runs: Vec<CompositionRun> = Vec::new();
    for (index, span) in spans.iter().enumerate() {
        if span.composition_id.is_empty()
            || span.end_sample <= span.start_sample
            || (index > 0 && spans[index - 1].end_sample != span.start_sample)
        {
            bail!("rendered picture spans must form a gapless ordered clock");
        }
        if let Some(last) = runs.last_mut() {
            if last.composition_id == span.composition_id {
                last.end_sample = span.end_sample;
                continue;
            }
        }
        runs.push(CompositionRun {
            composition_id: span.composition_id.clone(),
            start_sample: span.start_sample,
            end_sample: span.end_sample,
            target_hint: None,
        });
    }
    for run in &mut runs {
        let seconds = (run.end_sample - run.start_sample) as f64 / sample_rate as f64;
        if seconds > policy.hard_unchanged_composition_seconds_max + 1e-9 {
            bail!(
                "composition {} remains unchanged for {seconds:.3}s; policy maximum is {:.3}s",
                run.composition_id,
                policy.hard_unchanged_composition_seconds_max
            );
        }
        if seconds < policy.target_composition_seconds_min
            || seconds > policy.target_composition_seconds_max
        {
            run.target_hint = Some(format!("{seconds:.3}s outside preferred composition range"));
        }
    }
    Ok(runs)
}

fn sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn digest<T: Serialize>(value: &T) -> Result<String> {
    Ok(Sha256::digest(serde_json::to_vec(value)?)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Resolve opening, chapter, credits, and other episode presentation without
/// making their bindings dependencies of every narrative scene.
pub fn resolve_episode_presentation(
    catalog: &TemplateCatalog,
    episode: &Episode,
    season: &ScopedBindings,
    episode_bindings: &ScopedBindings,
) -> Result<ResolvedEpisodePresentation> {
    if catalog.schema != CATALOG_SCHEMA
        || episode.schema != EPISODE_SCHEMA
        || season.schema != BINDINGS_SCHEMA
        || episode_bindings.schema != BINDINGS_SCHEMA
    {
        bail!("unsupported authoring schema");
    }
    let master = catalog
        .templates
        .iter()
        .find(|t| t.template_id == episode.master_template_id && t.kind == "episode-master")
        .ok_or_else(|| anyhow::anyhow!("episode master template missing"))?;
    if !sha(&master.definition_sha256) {
        bail!("invalid master template hash");
    }
    let mut template_definitions =
        BTreeMap::from([(master.template_id.clone(), master.definition_sha256.clone())]);
    let mut selected_inputs = BTreeMap::new();
    let scopes = [episode_bindings, season];
    let mut roles = BTreeSet::new();
    for use_ in &episode.presentation {
        if !roles.insert(use_.role.as_str()) {
            bail!("duplicate episode presentation role");
        }
        let selected = use_template(use_, catalog)?;
        if !sha(&selected.definition_sha256) {
            bail!("invalid template hash");
        }
        template_definitions.insert(
            selected.template_id.clone(),
            selected.definition_sha256.clone(),
        );
        if let Some(key) = &use_.asset_binding {
            selected_inputs.insert(key.clone(), binding(key, &scopes)?.clone());
        }
    }
    let fingerprint_sha256 = digest(&(
        &episode.episode_id,
        &episode.presentation,
        &selected_inputs,
        &template_definitions,
    ))?;
    Ok(ResolvedEpisodePresentation {
        episode_id: episode.episode_id.clone(),
        selected_inputs,
        template_definitions,
        fingerprint_sha256,
    })
}

fn binding<'a>(key: &str, scopes: &'a [&ScopedBindings]) -> Result<&'a AssetRef> {
    let matches = scopes
        .iter()
        .filter_map(|scope| scope.assets.get(key))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        bail!(
            "binding {key} must resolve exactly once; found {}",
            matches.len()
        );
    }
    let asset = matches[0];
    if asset.logical_id.is_empty()
        || asset.bytes == 0
        || !sha(&asset.sha256)
        || asset.cache_uri != format!("cache://sha256/{}", asset.sha256)
    {
        bail!("binding {key} lacks exact cache identity");
    }
    if ![
        "selected-private-production",
        "principal-approved",
        "release-cleared",
    ]
    .contains(&asset.selection_state.as_str())
    {
        bail!("binding {key} is not selected for production");
    }
    Ok(asset)
}

fn use_template<'a>(use_: &PresentationUse, catalog: &'a TemplateCatalog) -> Result<&'a Template> {
    let template = catalog
        .templates
        .iter()
        .find(|item| item.template_id == use_.template_id)
        .ok_or_else(|| anyhow::anyhow!("unknown template {}", use_.template_id))?;
    if template.kind != use_.role {
        bail!(
            "template {} has kind {}, not {}",
            template.template_id,
            template.kind,
            use_.role
        );
    }
    let content = use_
        .content
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("template content must be an object"))?;
    for key in &template.required_content_keys {
        if !content.contains_key(key) {
            bail!("template {} missing content {key}", template.template_id);
        }
    }
    if content.keys().any(|key| {
        ["layout", "font", "geometry", "pixels", "duration_seconds"].contains(&key.as_str())
    }) {
        bail!("scene content contains template-owned presentation settings");
    }
    Ok(template)
}

/// Resolve one scene without changing any source file or choosing an asset.
/// Episode and season bindings may be replaced independently of scene data.
pub fn resolve_scene(
    catalog: &TemplateCatalog,
    episode: &Episode,
    scene: &Scene,
    policy: &ScenePolicy,
    season: &ScopedBindings,
    episode_bindings: &ScopedBindings,
    scene_bindings: &ScopedBindings,
) -> Result<ResolvedScene> {
    if catalog.schema != CATALOG_SCHEMA
        || episode.schema != EPISODE_SCHEMA
        || scene.schema != SCENE_SCHEMA
        || [season, episode_bindings, scene_bindings]
            .iter()
            .any(|s| s.schema != BINDINGS_SCHEMA)
    {
        bail!("unsupported authoring schema");
    }
    if scene.episode_id != episode.episode_id || !episode.scene_ids.contains(&scene.scene_id) {
        bail!("scene is outside episode scope");
    }
    if scene.authoring_state != "ready-for-private-build" {
        bail!(
            "scene {} authoring state is {}; complete the source and selected binding migration first",
            scene.scene_id,
            scene.authoring_state
        );
    }
    validate_policy(policy)?;
    let effective_policy_id = scene
        .scene_policy_override_id
        .as_deref()
        .unwrap_or(&episode.scene_policy_id);
    if effective_policy_id != policy.policy_id {
        bail!("effective scene policy does not match");
    }
    if scene.source_scope_ids.is_empty() || !sha(&scene.source_evidence_sha256) {
        bail!("scene lacks source evidence");
    }
    let scopes = [scene_bindings, episode_bindings, season];
    let mut selected_inputs = BTreeMap::new();
    let mut template_definitions = BTreeMap::new();
    let mut score_roles = BTreeMap::new();
    for use_ in scene.presentation.iter() {
        let selected = use_template(use_, catalog)?;
        if !sha(&selected.definition_sha256) {
            bail!("invalid template hash");
        }
        template_definitions.insert(
            selected.template_id.clone(),
            selected.definition_sha256.clone(),
        );
        if let Some(key) = &use_.asset_binding {
            selected_inputs.insert(key.clone(), binding(key, &scopes)?.clone());
        }
    }
    let mut roles = BTreeSet::new();
    for score in &episode.score_palette {
        if !roles.insert(score.role.as_str())
            || score.theme_id.is_empty()
            || score.source_poem_id.is_empty()
            || score.arrangement_id.is_empty()
        {
            bail!("invalid episode score palette");
        }
        score_roles.insert(score.role.clone(), score.asset_binding.clone());
    }
    if scene.languages.is_empty() {
        bail!("scene has no language lanes");
    }
    let mut language_fingerprints = BTreeMap::new();
    for (language_id, language) in &scene.languages {
        let mut language_inputs = BTreeMap::new();
        if language.cues.is_empty() || language.events.is_empty() {
            bail!("empty {language_id} lane");
        }
        let mut cues = BTreeSet::new();
        for cue in &language.cues {
            if !cues.insert(cue.cue_id.as_str())
                || cue.source_id.is_empty()
                || !sha(&cue.exact_text_sha256)
            {
                bail!("invalid {language_id} cue");
            }
            for key in [&cue.take_binding, &cue.phrase_alignment_binding]
                .into_iter()
                .flatten()
            {
                let asset = binding(key, &scopes)?.clone();
                selected_inputs.insert(key.clone(), asset.clone());
                language_inputs.insert(key.clone(), asset);
            }
        }
        let mut events = BTreeSet::new();
        for event in &language.events {
            if !events.insert(event.event_id.as_str())
                || !cues.contains(event.cue_id.as_str())
                || event.semantic_trigger_id.is_empty()
            {
                bail!("invalid {language_id} event");
            }
            if let ScoreUse::Role { role } = &event.score {
                let key = score_roles
                    .get(role)
                    .ok_or_else(|| anyhow::anyhow!("unknown score role {role}"))?;
                let asset = binding(key, &scopes)?.clone();
                selected_inputs.insert(key.clone(), asset.clone());
                language_inputs.insert(key.clone(), asset);
            }
            for key in event
                .picture_binding
                .iter()
                .chain(&event.sonic_bindings)
                .chain(&event.vfx_bindings)
            {
                let asset = binding(key, &scopes)?.clone();
                selected_inputs.insert(key.clone(), asset.clone());
                language_inputs.insert(key.clone(), asset);
            }
        }
        language_fingerprints.insert(
            language_id.clone(),
            digest(&(
                &scene.scene_id,
                &scene.source_scope_ids,
                &scene.source_evidence_sha256,
                &scene.presentation,
                &scene.continuity_tags,
                &language,
                &language_inputs,
                &template_definitions,
                &policy,
            ))?,
        );
    }
    let fingerprint_sha256 = digest(&(&scene.scene_id, &language_fingerprints))?;
    Ok(ResolvedScene {
        scene_id: scene.scene_id.clone(),
        selected_inputs,
        template_definitions,
        score_roles,
        scene_policy_id: policy.policy_id.clone(),
        language_fingerprints,
        fingerprint_sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> ScenePolicy {
        ScenePolicy {
            schema: POLICY_SCHEMA.into(),
            policy_id: "tv-cuts".into(),
            target_composition_seconds_min: 5.0,
            target_composition_seconds_max: 10.0,
            hard_unchanged_composition_seconds_max: 10.0,
            semantic_cuts_required: true,
        }
    }

    #[test]
    fn adjacent_same_picture_events_are_one_run() {
        let spans = [
            RenderedSpan {
                composition_id: "a".into(),
                start_sample: 0,
                end_sample: 6_000,
            },
            RenderedSpan {
                composition_id: "a".into(),
                start_sample: 6_000,
                end_sample: 12_000,
            },
        ];
        assert!(audit_rendered_compositions(&policy(), 1_000, &spans).is_err());
    }

    #[test]
    fn another_producer_can_choose_another_cadence() {
        let mut other = policy();
        other.policy_id = "slow-documentary".into();
        other.target_composition_seconds_max = 15.0;
        other.hard_unchanged_composition_seconds_max = 20.0;
        let spans = [
            RenderedSpan {
                composition_id: "a".into(),
                start_sample: 0,
                end_sample: 6_000,
            },
            RenderedSpan {
                composition_id: "a".into(),
                start_sample: 6_000,
                end_sample: 12_000,
            },
        ];
        assert_eq!(
            audit_rendered_compositions(&other, 1_000, &spans)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn montage_override_is_explicit() {
        let episode = Episode {
            schema: EPISODE_SCHEMA.into(),
            episode_id: "e1".into(),
            season_id: "s1".into(),
            master_template_id: "master".into(),
            scene_policy_id: "default".into(),
            presentation: vec![],
            score_palette: vec![],
            scene_ids: vec!["montage".into()],
        };
        let scene = Scene {
            schema: SCENE_SCHEMA.into(),
            scene_id: "montage".into(),
            episode_id: "e1".into(),
            authoring_state: "ready-for-private-build".into(),
            legacy_evidence: vec![],
            source_scope_ids: vec!["b1".into()],
            source_evidence_sha256: "a".repeat(64),
            scene_policy_override_id: Some("montage".into()),
            languages: BTreeMap::new(),
            presentation: None,
            continuity_tags: vec![],
            holds: vec![],
        };
        assert_eq!(
            scene
                .scene_policy_override_id
                .as_deref()
                .unwrap_or(&episode.scene_policy_id),
            "montage"
        );
    }
}
