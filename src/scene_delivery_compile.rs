//! Compile a selected scene authoring graph into the generic REEL delivery
//! files. All cut positions come from verified native phrase alignments.

use crate::scene_authoring_inputs::read_verified_alignments;
use anyhow::{Context, Result, bail};
use reel_assembly::{
    Graph, SelectedPointer,
    scene_authoring::{
        AssetRef, Episode, Scene, ScenePolicy, ScopedBindings, ScoreUse, TemplateCatalog,
        compile_native_event_spans, materialize_scene, resolve_scene,
    },
    selected_closure,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompileManifest {
    pub schema: String,
    pub catalog: String,
    pub episode: String,
    pub scene: String,
    pub policy: String,
    pub season_bindings: String,
    pub episode_bindings: String,
    pub scene_bindings: String,
    pub graph: String,
    pub pointer: String,
    pub alignment_paths: String,
    pub profile: String,
    pub language: String,
    pub delivery_id: String,
    pub delivery_title: String,
    pub output_dir: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryProfile {
    pub schema: String,
    pub width: u32,
    pub height: u32,
    pub sample_rate: u32,
    pub frame_rate_numerator: u64,
    pub frame_rate_denominator: u64,
    pub max_composition_samples: u64,
    pub score_gain_db: f64,
    pub score_fade_in_samples: u64,
    pub score_fade_out_samples: u64,
    #[serde(default = "default_sonic_gain_db")]
    pub sonic_gain_db: f64,
    #[serde(default)]
    pub sonic_gain_db_by_binding: BTreeMap<String, f64>,
}

fn default_sonic_gain_db() -> f64 {
    -18.0
}

fn sonic_gain_db(profile: &DeliveryProfile, binding: &str) -> f64 {
    profile
        .sonic_gain_db_by_binding
        .get(binding)
        .copied()
        .unwrap_or(profile.sonic_gain_db)
}

#[derive(Clone)]
struct CueClock {
    id: String,
    start: u64,
    end: u64,
}

struct EventSpan {
    id: String,
    semantic_id: String,
    cue_id: String,
    start: u64,
    end: u64,
    picture: AssetRef,
    score_role: Option<String>,
    sonic_bindings: Vec<String>,
    vfx_bindings: Vec<String>,
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&fs::read(path).with_context(|| path.display().to_string())?)
        .with_context(|| path.display().to_string())
}

fn checked(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("scene delivery compile paths must be project-root relative");
    }
    let file = root.join(path).canonicalize()?;
    if !file.starts_with(root) || !file.is_file() {
        bail!("scene delivery compile input escapes project root");
    }
    Ok(file)
}

fn output_dir(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("scene delivery output must be a project-root relative directory");
    }
    let dir = root.join(path);
    fs::create_dir_all(&dir)?;
    let dir = dir.canonicalize()?;
    if !dir.starts_with(root) {
        bail!("scene delivery output escapes project root");
    }
    Ok(dir)
}

fn asset<'a>(key: &str, scopes: &[&'a ScopedBindings]) -> Result<&'a AssetRef> {
    let found = scopes
        .iter()
        .filter_map(|scope| scope.assets.get(key))
        .collect::<Vec<_>>();
    if found.len() != 1 {
        bail!("binding {key} must resolve exactly once");
    }
    let item = found[0];
    if item.sha256.len() != 64
        || item.cache_uri != format!("cache://sha256/{}", item.sha256)
        || item.bytes == 0
        || ![
            "selected-private-production",
            "principal-approved",
            "release-cleared",
        ]
        .contains(&item.selection_state.as_str())
    {
        bail!("binding {key} is not a selected cache object");
    }
    Ok(item)
}

fn media_ref(item: &AssetRef) -> Value {
    json!({
        "path":format!("objects/sha256/{}/{}", &item.sha256[..2], item.sha256),
        "sha256":item.sha256,
        "bytes":item.bytes
    })
}

fn presentation_binding<'a>(content: &'a Value, field: &str, language: &str) -> Result<&'a str> {
    content
        .get(field)
        .and_then(Value::as_object)
        .and_then(|bindings| bindings.get(language))
        .and_then(Value::as_str)
        .filter(|binding| !binding.is_empty())
        .with_context(|| format!("presentation {field} missing for {language}"))
}

fn anchor(sample: u64, cues: &[CueClock]) -> Result<Value> {
    let last = cues.last().context("scene has no cues")?;
    if sample == last.end {
        return Ok(json!({"kind":"cue-end","cue_id":last.id}));
    }
    let cue = cues
        .iter()
        .find(|cue| sample >= cue.start && sample < cue.end)
        .context("semantic event lies outside cue clock")?;
    Ok(json!({
        "kind":"cue-start",
        "cue_id":cue.id,
        "offset_samples":sample-cue.start
    }))
}

fn write_new(dir: &Path, name: &str, value: &Value) -> Result<(String, u64)> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let path = dir.join(name);
    use std::io::Write;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .with_context(|| path.display().to_string())?
        .write_all(&bytes)?;
    Ok((hash(&bytes), bytes.len() as u64))
}

fn score_role(score: &ScoreUse) -> Result<Option<String>> {
    match score {
        ScoreUse::Role { role } if !role.is_empty() => Ok(Some(role.clone())),
        ScoreUse::Silence => Ok(None),
        ScoreUse::Role { .. } | ScoreUse::Held => {
            bail!("held or empty score role cannot compile for delivery")
        }
    }
}

/// This writes new files only. It never guesses phrase clocks or creative assets.
pub fn compile_to_dir(root: &Path, request: &CompileManifest) -> Result<Value> {
    if request.schema != "reel.scene-delivery-compile.v1" {
        bail!("unsupported scene delivery compile schema");
    }
    let root = root.canonicalize()?;
    let catalog: TemplateCatalog = read(&checked(&root, &request.catalog)?)?;
    let episode: Episode = read(&checked(&root, &request.episode)?)?;
    let source_scene: Scene = read(&checked(&root, &request.scene)?)?;
    let scene = materialize_scene(&source_scene)?;
    let policy: ScenePolicy = read(&checked(&root, &request.policy)?)?;
    let season: ScopedBindings = read(&checked(&root, &request.season_bindings)?)?;
    let episode_bindings: ScopedBindings = read(&checked(&root, &request.episode_bindings)?)?;
    let scene_bindings: ScopedBindings = read(&checked(&root, &request.scene_bindings)?)?;
    let scopes = [&scene_bindings, &episode_bindings, &season];
    resolve_scene(
        &catalog,
        &episode,
        &source_scene,
        &policy,
        &season,
        &episode_bindings,
        &scene_bindings,
    )?;
    let graph: Graph = read(&checked(&root, &request.graph)?)?;
    let pointer: SelectedPointer = read(&checked(&root, &request.pointer)?)?;
    selected_closure(&pointer, &graph, &scene.scene_id)?;
    let alignments = read_verified_alignments(
        &source_scene,
        &request.language,
        &checked(&root, &request.alignment_paths)?,
        &scopes,
    )?;
    let profile: DeliveryProfile = read(&checked(&root, &request.profile)?)?;
    if profile.schema != "reel.scene-delivery-profile.v1"
        || profile.width == 0
        || profile.height == 0
        || profile.sample_rate == 0
        || profile.frame_rate_numerator == 0
        || profile.frame_rate_denominator == 0
        || profile.max_composition_samples == 0
        || !profile.score_gain_db.is_finite()
        || !profile.sonic_gain_db.is_finite()
        || profile.sonic_gain_db.abs() > 60.0
        || profile
            .sonic_gain_db_by_binding
            .iter()
            .any(|(key, gain)| !key.starts_with("sonic.") || !gain.is_finite() || gain.abs() > 60.0)
    {
        bail!("invalid scene delivery profile");
    }
    let lane = scene
        .languages
        .get(&request.language)
        .context("requested scene language missing")?;
    let native = compile_native_event_spans(&request.language, lane, &alignments)?;
    let mut cues = Vec::new();
    let mut offset = 0u64;
    for cue in &lane.cues {
        let alignment = alignments
            .get(&cue.cue_id)
            .context("cue alignment missing")?;
        if alignment.sample_rate != profile.sample_rate {
            bail!("cue sample rate differs from delivery profile");
        }
        let end = offset
            .checked_add(alignment.cue_end_sample)
            .context("cue clock overflow")?;
        cues.push(CueClock {
            id: cue.cue_id.clone(),
            start: offset,
            end,
        });
        offset = end;
    }
    let duration = offset;
    let selected_events = active_scene_events(&graph, &scene.scene_id, &request.language)?;
    if selected_events.len() != lane.events.len() {
        bail!("selected graph does not cover each scene language event");
    }
    let mut events = Vec::new();
    for (event, span) in lane.events.iter().zip(native.iter()) {
        if event.event_id != span.event_id || event.cue_id != span.cue_id {
            bail!("materialized scene and native events differ");
        }
        let clock = cues
            .iter()
            .find(|cue| cue.id == event.cue_id)
            .context("event cue absent")?;
        let start = clock.start + span.start_sample;
        let end = clock.start + span.end_sample;
        let selected = selected_events
            .get(event.event_id.as_str())
            .context("selected event absent")?;
        let binding = event
            .picture_binding
            .as_deref()
            .context("selected event picture binding missing")?;
        let picture = asset(binding, &scopes)?;
        let take_key = lane
            .cues
            .iter()
            .find(|cue| cue.cue_id == event.cue_id)
            .and_then(|cue| cue.take_binding.as_deref())
            .context("selected take binding missing")?;
        let take = asset(take_key, &scopes)?;
        let graph_start = (selected.phrase_start_seconds * f64::from(profile.sample_rate)).round();
        let graph_end = (selected.phrase_end_seconds * f64::from(profile.sample_rate)).round();
        if selected.narration.sha256 != take.sha256
            || selected.picture.sha256 != picture.sha256
            || (graph_start - span.start_sample as f64).abs() > 1.0
            || (graph_end - span.end_sample as f64).abs() > 1.0
        {
            bail!(
                "selected semantic event {} differs from native scene binding: graph {}..{} / {} {} vs native {}..{} / {} {}",
                event.event_id,
                graph_start,
                graph_end,
                selected.narration.sha256,
                selected.picture.sha256,
                span.start_sample,
                span.end_sample,
                take.sha256,
                picture.sha256
            );
        }
        events.push(EventSpan {
            id: event.event_id.clone(),
            semantic_id: scene
                .language_event_bindings
                .get(&request.language)
                .and_then(|bindings| {
                    bindings
                        .iter()
                        .find(|binding| binding.event_id == event.event_id)
                })
                .map(|binding| binding.semantic_id.clone())
                .unwrap_or_else(|| event.event_id.clone()),
            cue_id: event.cue_id.clone(),
            start,
            end,
            picture: picture.clone(),
            score_role: score_role(&event.score)?,
            sonic_bindings: event.sonic_bindings.clone(),
            vfx_bindings: event.vfx_bindings.clone(),
        });
    }
    if events.first().is_none_or(|event| event.start != 0)
        || events.last().is_none_or(|event| event.end != duration)
        || events.windows(2).any(|pair| pair[0].end != pair[1].start)
    {
        bail!("native semantic events do not cover the scene contiguously");
    }
    if request.delivery_id.trim().is_empty() || request.delivery_title.trim().is_empty() {
        bail!("delivery identity and title must be non-empty");
    }
    let id = request.delivery_id.clone();
    let mut shots = Vec::new();
    let mut pictures = Vec::new();
    let mut attachments = Vec::new();
    let mut event_bindings = Vec::new();
    for event in &events {
        let picture_id = format!("p-{}", event.semantic_id);
        let shot_id = format!("shot-{}", event.semantic_id);
        shots.push(json!({"id":shot_id,"scene_id":id}));
        pictures.push(json!({
            "attachment_id":picture_id,
            "source":media_ref(&event.picture),
            "kind":"still",
            "attention":format!("source concept {}; private audition",event.semantic_id)
        }));
        attachments.push(json!({
            "id":picture_id,
            "target":{"kind":"cel","shot_id":shot_id,"cel_id":format!("cel-{}",event.semantic_id)},
            "start":anchor(event.start,&cues)?,
            "end":anchor(event.end,&cues)?
        }));
        event_bindings.push(json!({
            "event_id":event.id,
            "narration_attachment_id":format!("d-{}",event.cue_id),
            "picture_attachment_id":picture_id,
            "audio_attachment_ids":[],
            "external_layer_attachment_ids":[]
        }));
    }
    let mut audio = Vec::new();
    let mut audio_events = Vec::new();
    let mut external_layers = Vec::new();
    let mut narration_cues = Vec::new();
    let mut contract_cues = Vec::new();
    for (cue, clock) in lane.cues.iter().zip(cues.iter()) {
        let take = asset(
            cue.take_binding
                .as_deref()
                .context("cue take binding absent")?,
            &scopes,
        )?;
        let attachment = format!("d-{}", cue.cue_id);
        audio.push(json!({
            "attachment_id":attachment,
            "source":media_ref(take),
            "bus":"D",
            "cue_id":cue.cue_id
        }));
        audio_events.push(json!({
            "id":cue.cue_id,
            "role":"narration",
            "source":format!("objects/sha256/{}/{}", &take.sha256[..2],take.sha256),
            "start_seconds":0
        }));
        let speakers = cue
            .take_segments
            .iter()
            .map(|segment| &segment.speaker_id)
            .collect::<BTreeSet<_>>();
        let speaker_id = if speakers.len() > 1 {
            format!("multi-speaker-{}", request.language)
        } else {
            cue.speaker_id.clone().unwrap_or_else(|| {
                cue.take_segments
                    .first()
                    .map(|segment| segment.speaker_id.clone())
                    .unwrap_or_else(|| format!("narrator-{}", request.language))
            })
        };
        narration_cues.push(json!({"id":cue.cue_id,"speaker_id":speaker_id}));
        contract_cues.push(json!({
            "cue_id":cue.cue_id,
            "duration_samples":clock.end-clock.start
        }));
        attachments.push(json!({
            "id":attachment,
            "target":{"kind":"audio","audio_event_id":cue.cue_id},
            "start":{"kind":"cue-start","cue_id":cue.cue_id},
            "end":{"kind":"cue-end","cue_id":cue.cue_id}
        }));
    }
    let mut used_roles = BTreeSet::new();
    let mut run = 0usize;
    let mut index = 0usize;
    while index < events.len() {
        let Some(role) = events[index].score_role.as_deref() else {
            index += 1;
            continue;
        };
        let start = index;
        while index < events.len() && events[index].score_role.as_deref() == Some(role) {
            index += 1;
        }
        let palette = episode
            .score_palette
            .iter()
            .find(|entry| entry.role == role)
            .context("score role absent from episode palette")?;
        let score = asset(&palette.asset_binding, &scopes)?;
        let safe_role = role
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>();
        run += 1;
        let attachment = format!("m-{safe_role}-{run}");
        let start_sample = events[start].start;
        let end_sample = events[index - 1].end;
        let run_len = end_sample - start_sample;
        if profile.score_fade_in_samples + profile.score_fade_out_samples > run_len {
            bail!("score fades exceed semantic run length");
        }
        audio.push(json!({
            "attachment_id":attachment,
            "source":media_ref(score),
            "bus":"M",
            "source_start_sample":0,
            "gain_db":profile.score_gain_db,
            "fade_in_samples":profile.score_fade_in_samples,
            "fade_out_samples":profile.score_fade_out_samples
        }));
        audio_events.push(json!({
            "id":attachment,
            "role":"music",
            "source":format!("objects/sha256/{}/{}", &score.sha256[..2],score.sha256),
            "start_seconds":0
        }));
        attachments.push(json!({
            "id":attachment,
            "target":{"kind":"audio","audio_event_id":attachment},
            "start":anchor(start_sample,&cues)?,
            "end":anchor(end_sample,&cues)?
        }));
        for binding in &mut event_bindings[start..index] {
            binding["audio_attachment_ids"] = json!([attachment]);
        }
        used_roles.insert(role.to_owned());
    }
    let mut selected_sonic = false;
    let used_sonic = events
        .iter()
        .flat_map(|event| event.sonic_bindings.iter())
        .collect::<BTreeSet<_>>();
    if profile
        .sonic_gain_db_by_binding
        .keys()
        .any(|key| !used_sonic.contains(key))
    {
        bail!("scene delivery profile has an unused Sonic gain binding");
    }
    for (index, event) in events.iter().enumerate() {
        if event.sonic_bindings.iter().collect::<BTreeSet<_>>().len() != event.sonic_bindings.len()
        {
            bail!(
                "duplicate Sonic key on semantic event {}",
                event.semantic_id
            );
        }
        for (position, key) in event.sonic_bindings.iter().enumerate() {
            if index > 0 && events[index - 1].sonic_bindings.contains(key) {
                continue;
            }
            let mut run_end = index + 1;
            while run_end < events.len() && events[run_end].sonic_bindings.contains(key) {
                run_end += 1;
            }
            let sonic = asset(key, &scopes)?;
            let attachment = format!("e-{}-{}", event.semantic_id, position + 1);
            audio.push(json!({
                "attachment_id": attachment,
                "source": media_ref(sonic),
                "bus": "E",
                "source_start_sample": 0,
                "gain_db": sonic_gain_db(&profile, key)
            }));
            audio_events.push(json!({
                "id": attachment,
                "role": "effect",
                "source": format!("objects/sha256/{}/{}", &sonic.sha256[..2], sonic.sha256),
                "start_seconds": 0
            }));
            attachments.push(json!({
                "id": attachment,
                "target": {"kind":"sonic","audio_event_id":attachment},
                "start": anchor(event.start, &cues)?,
                "end": anchor(events[run_end - 1].end, &cues)?
            }));
            for binding in &mut event_bindings[index..run_end] {
                binding["audio_attachment_ids"]
                    .as_array_mut()
                    .context("semantic event audio binding is not an array")?
                    .push(json!(attachment));
            }
            selected_sonic = true;
        }
    }
    for (index, event) in events.iter().enumerate() {
        if event.vfx_bindings.iter().collect::<BTreeSet<_>>().len() != event.vfx_bindings.len() {
            bail!("duplicate VFX key on semantic event {}", event.semantic_id);
        }
        for (position, key) in event.vfx_bindings.iter().enumerate() {
            if index > 0 && events[index - 1].vfx_bindings.contains(key) {
                continue;
            }
            let mut run_end = index + 1;
            while run_end < events.len() && events[run_end].vfx_bindings.contains(key) {
                run_end += 1;
            }
            let overlay = asset(key, &scopes)?;
            let attachment = format!("fx-{}-{}", event.semantic_id, position + 1);
            let shot_id = format!("shot-{}", event.semantic_id);
            attachments.push(json!({
                "id": attachment,
                "target": {
                    "kind": "overlay",
                    "shot_id": shot_id,
                    "overlay_id": attachment
                },
                "start": anchor(event.start, &cues)?,
                "end": anchor(events[run_end - 1].end, &cues)?
            }));
            external_layers.push(json!({
                "attachment_id": attachment,
                "reason": format!("Selected semantic VFX for {}", event.semantic_id),
                "evidence": media_ref(overlay),
                "render_mode": "timed-video-overlay"
            }));
            for binding in &mut event_bindings[index..run_end] {
                binding["external_layer_attachment_ids"]
                    .as_array_mut()
                    .context("semantic event external layer binding is not an array")?
                    .push(json!(attachment));
            }
        }
    }
    if let Some(presentation) = &scene.presentation {
        let content = &presentation.content;
        let source_text_key =
            presentation_binding(content, "source_text_bindings", &request.language)?;
        let layer_key = presentation_binding(content, "ass_layer_bindings", &request.language)?;
        let receipt_key =
            presentation_binding(content, "template_receipt_bindings", &request.language)?;
        // The editable master must have been compiled from selected source text.
        // Its receipt stays a distinct selected input even though only the ASS
        // bytes and optional font become rendered media.
        asset(source_text_key, &scopes)?;
        asset(receipt_key, &scopes)?;
        let layer = asset(layer_key, &scopes)?;
        let font = presentation
            .asset_binding
            .as_deref()
            .map(|key| asset(key, &scopes))
            .transpose()?;
        let attachment = format!("presentation-{}", request.language);
        attachments.push(json!({
            "id": attachment,
            "target": {"kind":"title","title_id":scene.scene_id},
            "start":anchor(0, &cues)?,
            "end":anchor(duration, &cues)?
        }));
        let mut external = json!({
            "attachment_id": attachment,
            "reason": format!("Editable {} master", presentation.role),
            "evidence": media_ref(layer),
            "render_mode": "ass-overlay"
        });
        if let Some(font) = font {
            external["font"] = media_ref(font);
        }
        external_layers.push(external);
        for binding in &mut event_bindings {
            binding["external_layer_attachment_ids"]
                .as_array_mut()
                .context("semantic event external layer binding is not an array")?
                .push(json!(attachment));
        }
    }
    let production = json!({
        "manifest_version":"reel.manifest.v0.2",
        "profile":"animatic",
        "timing_status":"conformed",
        "work":id,
        "title":request.delivery_title,
        "scenes":[{"id":id}],
        "shots":shots,
        "narration_cues":narration_cues,
        "audio_events":audio_events
    });
    let contract = json!({
        "schema":"reel.cue-relative-assembly.v0.1",
        "id":id,
        "sample_rate":profile.sample_rate,
        "frame_rate":{
            "numerator":profile.frame_rate_numerator,
            "denominator":profile.frame_rate_denominator
        },
        "production_manifest":"production.json",
        "cues":contract_cues,
        "attachments":attachments
    });
    let dir = output_dir(&root, &request.output_dir)?;
    for name in [
        "production.json",
        "contract.json",
        "job.json",
        "semantic-delivery.json",
        "build.json",
        "compile-receipt.json",
    ] {
        if dir.join(name).exists() {
            bail!("scene delivery compile output already exists: {name}");
        }
    }
    let (production_sha, _) = write_new(&dir, "production.json", &production)?;
    let (contract_sha, contract_bytes) = write_new(&dir, "contract.json", &contract)?;
    let job = json!({
        "schema":"reel.scene-delivery.v0.1",
        "id":id,
        "contract":{"path":"contract.json","sha256":contract_sha,"bytes":contract_bytes},
        "production_manifest_sha256":production_sha,
        "width":profile.width,
        "height":profile.height,
        "max_composition_samples":profile.max_composition_samples,
        "pictures":pictures,
        "audio":audio,
        "external_layers":external_layers,
        "buses":{
            "D":{"state":"present","reason":"Cache-verified language-local native candidate takes"},
            "M":{"state":if used_roles.is_empty(){"intentional-silence"}else{"present"},
                 "reason":"Episode score palette selected for source-motivated semantic runs"},
            "E":{"state":if selected_sonic{"present"}else{"intentional-silence"},
                 "reason":if selected_sonic{"Selected semantic Sonic assets"}else{"No selected Sonic binding in this scene"}}
        }
    });
    let (job_sha, job_bytes) = write_new(&dir, "job.json", &job)?;
    let semantic = json!({
        "schema":"reel.semantic-delivery.v1",
        "id":id,
        "pointer":pointer,
        "graph":graph,
        "target":scene.scene_id,
        "scene_id":scene.scene_id,
        "language":request.language,
        "scene_delivery_job":{"path":"job.json","sha256":job_sha,"bytes":job_bytes},
        "event_bindings":event_bindings
    });
    let (semantic_sha, _) = write_new(&dir, "semantic-delivery.json", &semantic)?;
    let build = json!({
        "schema":"reel.scene-build.v1",
        "scene_id":scene.scene_id,
        "language":request.language,
        "catalog":request.catalog,
        "episode":request.episode,
        "scene":request.scene,
        "policy":request.policy,
        "season_bindings":request.season_bindings,
        "episode_bindings":request.episode_bindings,
        "scene_bindings":request.scene_bindings,
        "alignment_paths":request.alignment_paths,
        "semantic_delivery":format!("{}/semantic-delivery.json",request.output_dir)
    });
    let (build_sha, _) = write_new(&dir, "build.json", &build)?;
    let mut input_sha256 = BTreeMap::new();
    for (name, path) in [
        ("catalog", &request.catalog),
        ("episode", &request.episode),
        ("scene", &request.scene),
        ("policy", &request.policy),
        ("season_bindings", &request.season_bindings),
        ("episode_bindings", &request.episode_bindings),
        ("scene_bindings", &request.scene_bindings),
        ("graph", &request.graph),
        ("pointer", &request.pointer),
        ("alignment_paths", &request.alignment_paths),
        ("profile", &request.profile),
    ] {
        input_sha256.insert(name, hash(&fs::read(checked(&root, path)?)?));
    }
    let alignment_sha256 = lane
        .cues
        .iter()
        .map(|cue| {
            let key = cue
                .phrase_alignment_binding
                .as_deref()
                .context("cue alignment binding missing")?;
            Ok((cue.cue_id.clone(), asset(key, &scopes)?.sha256.clone()))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let receipt = json!({
        "schema":"reel.scene-delivery-compile-receipt.v1",
        "scene_id":scene.scene_id,
        "language":request.language,
        "source_scene_sha256":input_sha256["scene"],
        "input_sha256":input_sha256,
        "alignment_sha256":alignment_sha256,
        "compiler_source_sha256":hash(include_bytes!("scene_delivery_compile.rs")),
        "compiler_package_version":env!("CARGO_PKG_VERSION"),
        "selected_graph_lock":semantic["pointer"]["selected_lock"],
        "cue_count":cues.len(),
        "semantic_event_count":events.len(),
        "score_roles":used_roles,
        "production_sha256":production_sha,
        "contract_sha256":contract_sha,
        "job_sha256":job_sha,
        "semantic_delivery_sha256":semantic_sha,
        "build_sha256":build_sha,
        "state":"private technical compilation; creative review remains external"
    });
    write_new(&dir, "compile-receipt.json", &receipt)?;
    Ok(receipt)
}

fn active_scene_events<'a>(
    graph: &'a reel_assembly::Graph,
    scene_id: &str,
    language: &str,
) -> Result<BTreeMap<&'a str, &'a reel_assembly::SemanticEvent>> {
    let active_event_ids = graph
        .nodes
        .iter()
        .find(|node| node.id == scene_id)
        .context("scene node missing from selected graph")?
        .events
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    Ok(graph
        .events
        .iter()
        .filter(|event| {
            event.scene_id == scene_id
                && event.language == language
                && active_event_ids.contains(event.event_id.as_str())
        })
        .map(|event| (event.event_id.as_str(), event))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{CueClock, DeliveryProfile, active_scene_events, anchor, presentation_binding, score_role, sonic_gain_db};
    use reel_assembly::scene_authoring::ScoreUse;
    use serde_json::json;

    #[test]
    fn sonic_gain_override_preserves_the_default_for_other_bindings() {
        let profile: DeliveryProfile = serde_json::from_value(json!({
            "schema": "reel.scene-delivery-profile.v1",
            "width": 448,
            "height": 252,
            "sample_rate": 44100,
            "frame_rate_numerator": 24,
            "frame_rate_denominator": 1,
            "max_composition_samples": 441000,
            "score_gain_db": -18,
            "score_fade_in_samples": 0,
            "score_fade_out_samples": 0,
            "sonic_gain_db": -30,
            "sonic_gain_db_by_binding": {"sonic.thunder": -19}
        })).unwrap();
        assert_eq!(sonic_gain_db(&profile, "sonic.thunder"), -19.0);
        assert_eq!(sonic_gain_db(&profile, "sonic.rain"), -30.0);
    }

    #[test]
    fn scene_clock_anchors_are_relative_to_the_selected_cue() {
        let cues = [
            CueClock {
                id: "a".into(),
                start: 0,
                end: 120_000,
            },
            CueClock {
                id: "b".into(),
                start: 120_000,
                end: 300_000,
            },
        ];
        assert_eq!(
            anchor(144_000, &cues).unwrap(),
            json!({"kind":"cue-start","cue_id":"b","offset_samples":24_000})
        );
        assert_eq!(
            anchor(300_000, &cues).unwrap(),
            json!({"kind":"cue-end","cue_id":"b"})
        );
        assert!(anchor(300_001, &cues).is_err());
    }

    #[test]
    fn unresolved_score_does_not_silently_become_silence() {
        assert!(score_role(&ScoreUse::Held).is_err());
        assert_eq!(score_role(&ScoreUse::Silence).unwrap(), None);
    }

    #[test]
    fn presentation_requires_language_local_editable_layer_bindings() {
        let content = json!({
            "ass_layer_bindings": {"es": "poem-layer.es", "en": "poem-layer.en"}
        });
        assert_eq!(
            presentation_binding(&content, "ass_layer_bindings", "en").unwrap(),
            "poem-layer.en"
        );
        assert!(presentation_binding(&content, "ass_layer_bindings", "fr").is_err());
        assert!(presentation_binding(&content, "template_receipt_bindings", "es").is_err());
    }

    #[test]
    fn superseded_picture_event_is_not_counted_as_active() {
        let event = |id: &str, language: &str| {
            json!({
                "event_id": id,
                "scene_id": "scene-1",
                "language": language,
                "narration": {"logical_id": "voice", "sha256": "0".repeat(64)},
                "picture": {"logical_id": "cel", "sha256": "1".repeat(64)},
                "phrase_start_seconds": 0.0,
                "phrase_end_seconds": 1.0
            })
        };
        let graph = serde_json::from_value(json!({
            "schema": "reel.semantic-assembly.v1",
            "lock": {"logical_id": "lock", "sha256": "0".repeat(64)},
            "slots": [],
            "events": [
                event("scene-1.es.old", "es"),
                event("scene-1.es.new", "es"),
                event("scene-1.en.current", "en")
            ],
            "nodes": [{
                "id": "scene-1",
                "inputs": [],
                "slots": [],
                "events": ["scene-1.es.new", "scene-1.en.current"]
            }],
            "presentation_targets": []
        }))
        .unwrap();
        let spanish = active_scene_events(&graph, "scene-1", "es").unwrap();
        assert_eq!(
            spanish.keys().copied().collect::<Vec<_>>(),
            vec!["scene-1.es.new"]
        );
        assert_eq!(
            active_scene_events(&graph, "scene-1", "en").unwrap().len(),
            1
        );
    }
}
