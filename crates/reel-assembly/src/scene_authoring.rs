//! Portable, scoped authoring for scenes and reusable presentation templates.
//! Creative selection remains with the owner. No template geometry or clock
//! seconds belong in a scene invocation.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    EVENT_BINDING_REQUEST_SCHEMA, EventBindingRequest, Graph, ImmutableRef, SelectedPointer,
    SemanticEvent, SemanticEventBinding, append_semantic_events, validate_selected_graph,
};
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
    pub authoring_state: String,
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
    pub narration_slot_id: String,
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
    pub picture_slot_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes_event_id: Option<String>,
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

pub const NATIVE_ALIGNMENT_SCHEMA: &str = "reel.scene-native-alignment.v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeAlignment {
    pub schema: String,
    pub language: String,
    pub cue_id: String,
    pub selected_take_sha256: String,
    pub sample_rate: u32,
    pub cue_end_sample: u64,
    pub semantic_markers: BTreeMap<String, u64>,
}

pub const WORD_TIMING_SCHEMA: &str = "reel.scene-word-timing-evidence.v1";
pub const TRIGGER_TEXT_SCHEMA: &str = "reel.scene-trigger-text.v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WordTimingEvidence {
    pub schema: String,
    pub language: String,
    pub cue_id: String,
    pub selected_take_sha256: String,
    pub sample_rate: u32,
    pub cue_end_sample: u64,
    pub words: Vec<TimedWord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimedWord {
    pub word: String,
    pub start_sample: u64,
    pub end_sample: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerTextSpec {
    pub schema: String,
    pub language: String,
    pub cue_id: String,
    pub selected_take_sha256: String,
    pub spoken_text: String,
    pub spoken_text_sha256: String,
    pub markers: Vec<TextMarker>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TextMarker {
    pub id: String,
    /// None names cue start; other markers name exact spoken phrase entrances.
    #[serde(default)]
    pub phrase: Option<String>,
    /// Explicit one-based occurrence when a phrase repeats.
    #[serde(default)]
    pub occurrence: Option<usize>,
}

fn tokens(value: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut token = String::new();
    for c in value.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            token.push(c);
        } else if !token.is_empty() {
            result.push(std::mem::take(&mut token));
        }
    }
    if !token.is_empty() {
        result.push(token);
    }
    result
}

fn occurrences(haystack: &[String], needle: &[String]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    haystack
        .windows(needle.len())
        .enumerate()
        .filter_map(|(index, window)| (window == needle).then_some(index))
        .collect()
}

/// Resolve source-authored phrase entrances against exact selected-take word
/// evidence. The resulting sample clocks remain evidence until listened.
pub fn resolve_text_triggers(
    evidence: &WordTimingEvidence,
    spec: &TriggerTextSpec,
) -> Result<NativeAlignment> {
    if evidence.schema != WORD_TIMING_SCHEMA
        || spec.schema != TRIGGER_TEXT_SCHEMA
        || evidence.language != spec.language
        || evidence.cue_id != spec.cue_id
        || evidence.selected_take_sha256 != spec.selected_take_sha256
        || !sha(&evidence.selected_take_sha256)
        || evidence.sample_rate == 0
        || evidence.cue_end_sample == 0
        || evidence.words.is_empty()
        || spec.markers.is_empty()
    {
        bail!("word evidence and trigger spec identity differ or are empty");
    }
    let text_sha = Sha256::digest(spec.spoken_text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if text_sha != spec.spoken_text_sha256 {
        bail!("spoken text hash mismatch");
    }
    let canonical = tokens(&spec.spoken_text);
    if canonical.is_empty() {
        bail!("spoken text has no words");
    }
    let mut words = Vec::<(String, u64)>::new();
    let mut prior_start = None;
    for item in &evidence.words {
        if item.end_sample <= item.start_sample
            || item.end_sample > evidence.cue_end_sample
            || prior_start.is_some_and(|start| item.start_sample < start)
        {
            bail!("word timing is invalid or out of order");
        }
        let normalized = tokens(&item.word);
        if normalized.is_empty() {
            bail!("word evidence has an empty token");
        }
        for token in normalized {
            words.push((token, item.start_sample));
        }
        prior_start = Some(item.start_sample);
    }
    let measured = words
        .iter()
        .map(|(token, _)| token.clone())
        .collect::<Vec<_>>();
    let mut markers = BTreeMap::new();
    let mut previous = None;
    for (ordinal, marker) in spec.markers.iter().enumerate() {
        if marker.id.trim().is_empty()
            || marker.occurrence == Some(0)
            || markers.contains_key(&marker.id)
        {
            bail!("trigger ID or occurrence invalid");
        }
        let sample = match marker.phrase.as_deref() {
            None if ordinal == 0 && marker.occurrence.is_none() => 0,
            None => bail!("only first marker may name cue start"),
            Some(phrase) if ordinal > 0 => {
                let phrase_tokens = tokens(phrase);
                let in_source = occurrences(&canonical, &phrase_tokens);
                let in_take = occurrences(&measured, &phrase_tokens);
                let occurrence = marker.occurrence.unwrap_or(1);
                if in_source.len() != in_take.len()
                    || occurrence > in_source.len()
                    || (in_source.len() > 1 && marker.occurrence.is_none())
                {
                    bail!(
                        "trigger phrase is missing or ambiguous between source and take: {}",
                        marker.id
                    );
                }
                words[in_take[occurrence - 1]].1
            }
            Some(_) => bail!("first marker must name cue start"),
        };
        if sample >= evidence.cue_end_sample || previous.is_some_and(|start| sample <= start) {
            bail!("trigger samples are not strictly ordered inside selected take");
        }
        markers.insert(marker.id.clone(), sample);
        previous = Some(sample);
    }
    Ok(NativeAlignment {
        schema: NATIVE_ALIGNMENT_SCHEMA.into(),
        language: evidence.language.clone(),
        cue_id: evidence.cue_id.clone(),
        selected_take_sha256: evidence.selected_take_sha256.clone(),
        sample_rate: evidence.sample_rate,
        cue_end_sample: evidence.cue_end_sample,
        semantic_markers: markers,
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct NativeEventSpan {
    pub event_id: String,
    pub cue_id: String,
    pub start_sample: u64,
    pub end_sample: u64,
    pub sample_rate: u32,
}

/// Turns exact semantic markers measured on a selected native take into
/// contiguous event spans. Scene authoring never supplies event seconds.
pub fn compile_native_event_spans(
    language_id: &str,
    language: &Language,
    alignments: &BTreeMap<String, NativeAlignment>,
) -> Result<Vec<NativeEventSpan>> {
    if language.cues.is_empty() || language.events.is_empty() {
        bail!("native lane is empty");
    }
    let mut output = Vec::new();
    for cue in &language.cues {
        let alignment = alignments
            .get(&cue.cue_id)
            .ok_or_else(|| anyhow::anyhow!("missing native alignment for {}", cue.cue_id))?;
        if alignment.schema != NATIVE_ALIGNMENT_SCHEMA
            || alignment.language != language_id
            || alignment.cue_id != cue.cue_id
            || !sha(&alignment.selected_take_sha256)
            || alignment.sample_rate == 0
            || alignment.cue_end_sample == 0
        {
            bail!("invalid native alignment for {}", cue.cue_id);
        }
        let events = language
            .events
            .iter()
            .filter(|event| event.cue_id == cue.cue_id)
            .collect::<Vec<_>>();
        if events.is_empty() {
            bail!("cue {} has no semantic events", cue.cue_id);
        }
        let mut starts = Vec::new();
        for event in &events {
            let start = *alignment
                .semantic_markers
                .get(&event.semantic_trigger_id)
                .ok_or_else(|| anyhow::anyhow!("missing marker {}", event.semantic_trigger_id))?;
            if start >= alignment.cue_end_sample {
                bail!("marker outside native cue");
            }
            starts.push(start);
        }
        if starts[0] != 0 || starts.windows(2).any(|pair| pair[0] >= pair[1]) {
            bail!("semantic markers must cover the cue in source order from sample zero");
        }
        for (index, event) in events.iter().enumerate() {
            output.push(NativeEventSpan {
                event_id: event.event_id.clone(),
                cue_id: cue.cue_id.clone(),
                start_sample: starts[index],
                end_sample: starts
                    .get(index + 1)
                    .copied()
                    .unwrap_or(alignment.cue_end_sample),
                sample_rate: alignment.sample_rate,
            });
        }
    }
    if output.len() != language.events.len() {
        bail!("semantic event references an unknown cue");
    }
    Ok(output)
}

/// Compile measured native marker spans into REEL's append-only selected-graph
/// request. The caller must verify alignment file bytes against its selected
/// binding before deserializing them; this function checks take and graph IDs.
pub fn compile_selected_event_request(
    graph: &Graph,
    pointer: &SelectedPointer,
    scene: &Scene,
    language_id: &str,
    scopes: &[&ScopedBindings],
    alignments: &BTreeMap<String, NativeAlignment>,
    next_lock_logical_id: &str,
) -> Result<EventBindingRequest> {
    validate_selected_graph(pointer, graph)?;
    if scene.authoring_state != "ready-for-private-build" {
        bail!("scene is not ready for private build");
    }
    let language = scene
        .languages
        .get(language_id)
        .ok_or_else(|| anyhow::anyhow!("missing {language_id} scene lane"))?;
    let spans = compile_native_event_spans(language_id, language, alignments)?;
    let cues = language
        .cues
        .iter()
        .map(|cue| (cue.cue_id.as_str(), cue))
        .collect::<BTreeMap<_, _>>();
    let events = language
        .events
        .iter()
        .map(|event| (event.event_id.as_str(), event))
        .collect::<BTreeMap<_, _>>();
    let mut bindings = Vec::new();
    for span in spans {
        let cue = cues[span.cue_id.as_str()];
        let event = events[span.event_id.as_str()];
        let take = binding(
            cue.take_binding
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("selected take missing"))?,
            scopes,
        )?;
        let picture = binding(
            event
                .picture_binding
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("selected picture missing"))?,
            scopes,
        )?;
        if alignments[&cue.cue_id].selected_take_sha256 != take.sha256 {
            bail!(
                "native alignment uses a different selected take for {}",
                cue.cue_id
            );
        }
        bindings.push(SemanticEventBinding {
            event: SemanticEvent {
                event_id: event.event_id.clone(),
                scene_id: scene.scene_id.clone(),
                language: language_id.into(),
                narration: ImmutableRef {
                    logical_id: take.logical_id.clone(),
                    sha256: take.sha256.clone(),
                },
                picture: ImmutableRef {
                    logical_id: picture.logical_id.clone(),
                    sha256: picture.sha256.clone(),
                },
                phrase_start_seconds: span.start_sample as f64 / span.sample_rate as f64,
                phrase_end_seconds: span.end_sample as f64 / span.sample_rate as f64,
            },
            node_id: scene.scene_id.clone(),
            narration_slot_id: cue.narration_slot_id.clone(),
            picture_slot_id: event.picture_slot_id.clone(),
            supersedes_event_id: event.supersedes_event_id.clone(),
        });
    }
    let request = EventBindingRequest {
        schema: EVENT_BINDING_REQUEST_SCHEMA.into(),
        bindings,
        next_lock_logical_id: next_lock_logical_id.into(),
    };
    append_semantic_events(graph, &request)?;
    Ok(request)
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
    pub source_authority_id: String,
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
    pub evidence_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
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
    if episode.authoring_state != "ready-for-private-build" {
        bail!(
            "episode {} authoring state is {}",
            episode.episode_id,
            episode.authoring_state
        );
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
    if episode.authoring_state != "ready-for-private-build" {
        bail!(
            "episode {} authoring state is {}",
            episode.episode_id,
            episode.authoring_state
        );
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
    if scene.source_scope_ids.is_empty() || scene.source_authority_id.is_empty() {
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
        if let Some(use_) = &scene.presentation {
            if let Some(key) = &use_.asset_binding {
                language_inputs.insert(key.clone(), binding(key, &scopes)?.clone());
            }
            for map_name in [
                "source_text_bindings",
                "ass_layer_bindings",
                "template_receipt_bindings",
            ] {
                if let Some(key) = use_
                    .content
                    .get(map_name)
                    .and_then(|items| items.get(language_id))
                    .and_then(|item| item.as_str())
                {
                    let asset = binding(key, &scopes)?.clone();
                    selected_inputs.insert(key.to_string(), asset.clone());
                    language_inputs.insert(key.to_string(), asset);
                }
            }
        }
        if language.cues.is_empty() || language.events.is_empty() {
            bail!("empty {language_id} lane");
        }
        let mut cues = BTreeSet::new();
        for cue in &language.cues {
            if !cues.insert(cue.cue_id.as_str())
                || cue.source_id.is_empty()
                || cue.narration_slot_id.is_empty()
                || !sha(&cue.exact_text_sha256)
            {
                bail!("invalid {language_id} cue");
            }
            if cue.take_binding.is_none() || cue.phrase_alignment_binding.is_none() {
                bail!("ready {language_id} cue lacks selected take or native alignment");
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
                || event.picture_slot_id.is_empty()
            {
                bail!("invalid {language_id} event");
            }
            if event.picture_binding.is_none() {
                bail!("ready {language_id} event lacks selected picture");
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
                &scene.source_authority_id,
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
            authoring_state: "ready-for-private-build".into(),
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
            source_authority_id: "source-1".into(),
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
