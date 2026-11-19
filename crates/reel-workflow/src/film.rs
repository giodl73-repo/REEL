//! Exact selected-take semantic film controls. No render, selection or approval.
use crate::{publication::InputReader, *};
use reel_assembly::{self as assembly, scene_authoring as scene};

pub const OUTPUTS: [&str; 5] = [
    "events.json",
    "graph.json",
    "pointer.json",
    "alignments.json",
    "review-map.json",
];
pub const DIRECTION_OUTPUT: &str = "direction.json";
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CueInputs {
    pub text_path: String,
    pub take_path: String,
    pub words_path: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub graph_path: String,
    pub pointer_path: String,
    pub episode_path: String,
    pub scope_paths: [String; 3],
    pub cues: BTreeMap<String, CueInputs>,
    pub next_lock_logical_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction_path: Option<String>,
}
pub struct Generated {
    pub outputs: ArtifactSet,
    pub files: BTreeMap<String, Vec<u8>>,
}
fn json(value: &impl Serialize) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| Error {
        kind: ErrorKind::Invalid,
        code: "film-control-json".into(),
    })
}
fn checked(reader: &mut impl InputReader, a: &Artifact, max: u64) -> Result<Vec<u8>> {
    if a.bytes > max {
        return fail(ErrorKind::Budget, "film-control-input-bytes");
    }
    let raw = reader.read(a, max)?;
    if raw.len() as u64 != a.bytes || digest(&raw) != a.sha256 {
        return fail(ErrorKind::Integrity, "film-control-input-content");
    }
    Ok(raw)
}
fn compilation<T, E>(r: std::result::Result<T, E>) -> Result<T> {
    r.map_err(|_| Error {
        kind: ErrorKind::Integrity,
        code: "film-control-semantic-compile".into(),
    })
}
fn selected(a: &Artifact, b: &scene::AssetRef) -> Result<()> {
    if a.logical_id != b.logical_id || a.sha256 != b.sha256 || a.bytes != b.bytes {
        return fail(ErrorKind::Conflict, "film-control-selected-binding");
    }
    Ok(())
}
pub fn generate(packet: &Packet, reader: &mut impl InputReader) -> Result<Generated> {
    check_packet(packet)?;
    let tool = packet.task.tool.as_ref().ok_or_else(|| Error {
        kind: ErrorKind::Missing,
        code: "film-control-tool".into(),
    })?;
    if tool.capability != ToolCapability::FilmSceneControl || tool.part.is_some() {
        return fail(ErrorKind::Unsupported, "film-control-capability");
    }
    let authored: scene::Scene = decode(&checked(
        reader,
        &tool.publication,
        MAX_CONTROL_BYTES as u64,
    )?)?;
    let request: Request = decode(&checked(reader, &tool.request, MAX_CONTROL_BYTES as u64)?)?;
    if !matches!(
        (request.schema.as_str(), request.direction_path.as_ref()),
        ("reel.workflow-film-scene-control.v1", None)
            | ("reel.workflow-film-scene-control.v2", Some(_))
    ) || !(1..=32).contains(&request.cues.len())
    {
        return fail(ErrorKind::Invalid, "film-control-request");
    }
    let lang = packet.task.language.as_deref().ok_or_else(|| Error {
        kind: ErrorKind::Missing,
        code: "film-control-language".into(),
    })?;
    let materialized = compilation(scene::materialize_scene(&authored))?;
    let lane = materialized.languages.get(lang).ok_or_else(|| Error {
        kind: ErrorKind::Missing,
        code: "film-control-scene-language".into(),
    })?;
    if lane.cues.len() != request.cues.len() {
        return fail(ErrorKind::Conflict, "film-control-cue-set");
    }
    let mut total = tool.publication.bytes + tool.request.bytes;
    let mut values = BTreeMap::new();
    let mut used = BTreeSet::new();
    for (path, id) in &tool.source_bindings {
        if path == "request.json"
            || id == &tool.request.logical_id
            || id == &tool.publication.logical_id
        {
            return fail(ErrorKind::Conflict, "film-control-binding-alias");
        }
        let a = packet
            .task
            .inputs
            .iter()
            .find(|a| &a.logical_id == id)
            .ok_or_else(|| Error {
                kind: ErrorKind::Missing,
                code: "film-control-input".into(),
            })?;
        total = total.checked_add(a.bytes).ok_or_else(|| Error {
            kind: ErrorKind::Budget,
            code: "film-control-input-total".into(),
        })?;
        if total > 128 * 1024 * 1024 {
            return fail(ErrorKind::Budget, "film-control-input-total");
        }
        let bytes = checked(reader, a, 128 * 1024 * 1024)?;
        values.insert(path.as_str(), (a, bytes));
    }
    let mut source = |path: &str, media: &str| -> Result<(&Artifact, &[u8])> {
        let (a, raw) = values.get(path).ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "film-control-source-path".into(),
        })?;
        if a.media_type != media || (media == "application/json" && raw.len() > MAX_CONTROL_BYTES) {
            return fail(ErrorKind::Invalid, "film-control-source-type");
        }
        used.insert(path.to_owned());
        Ok((*a, raw))
    };
    let graph: assembly::Graph = decode(source(&request.graph_path, "application/json")?.1)?;
    if compilation(assembly::graph_lock(&graph, &graph.lock.logical_id))?.sha256
        != graph.lock.sha256
    {
        return fail(ErrorKind::Integrity, "film-control-graph-lock");
    }
    let pointer: assembly::SelectedPointer =
        decode(source(&request.pointer_path, "application/json")?.1)?;
    let episode: scene::Episode = decode(source(&request.episode_path, "application/json")?.1)?;
    let scopes: Vec<scene::ScopedBindings> = request
        .scope_paths
        .iter()
        .map(|p| source(p, "application/json").and_then(|(_, raw)| decode(raw)))
        .collect::<Result<_>>()?;
    let scope_refs: Vec<_> = scopes.iter().collect();
    let mut alignments = BTreeMap::new();
    for cue in &lane.cues {
        let input = request.cues.get(&cue.cue_id).ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "film-control-cue-input".into(),
        })?;
        let (text_asset, text) = source(&input.text_path, "text/plain")?;
        if text_asset.sha256 != cue.exact_text_sha256 {
            return fail(ErrorKind::Conflict, "film-control-source-text");
        }
        let text = std::str::from_utf8(text).map_err(|_| Error {
            kind: ErrorKind::Invalid,
            code: "film-control-source-utf8".into(),
        })?;
        let (take_asset, take) = source(&input.take_path, "audio/wav")?;
        let binding = cue.take_binding.as_deref().ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "film-control-selected-take".into(),
        })?;
        selected(
            take_asset,
            compilation(scene::resolve_selected_binding(binding, &scope_refs))?,
        )?;
        let clock = compilation(assembly::audio_clock::mono_pcm_wave_clock(take))?;
        let words: scene::WordTimingEvidence =
            decode(source(&input.words_path, "application/json")?.1)?;
        if words.selected_take_sha256 != take_asset.sha256
            || (words.sample_rate, words.cue_end_sample) != clock
            || words.language != lang
            || words.cue_id != cue.cue_id
        {
            return fail(ErrorKind::Conflict, "film-control-take-clock");
        }
        compilation(scene::verify_word_text(&words, text))?;
        let triggers = compilation(scene::authored_trigger_text(&authored, &words, text))?;
        alignments.insert(
            cue.cue_id.clone(),
            compilation(scene::resolve_text_triggers(&words, &triggers))?,
        );
    }
    let direction: Option<crate::direction::Settings> = request
        .direction_path
        .as_ref()
        .map(|p| source(p, "application/json").and_then(|(_, raw)| decode(raw)))
        .transpose()?;
    if let Some(settings) = &direction {
        settings.check()?;
        if settings.policy.policy_id
            != materialized
                .scene_policy_override_id
                .as_deref()
                .unwrap_or(&episode.scene_policy_id)
        {
            return fail(ErrorKind::Conflict, "film-control-direction-policy");
        }
        for reference in &settings.references {
            let (path, (actual, _)) = values
                .iter()
                .find(|(_, (a, _))| a.logical_id == reference.logical_id)
                .ok_or(Error {
                    kind: ErrorKind::Missing,
                    code: "film-control-direction-reference".into(),
                })?;
            if *actual != reference {
                return fail(ErrorKind::Integrity, "film-control-direction-reference-pin");
            }
            used.insert((*path).into());
        }
    }
    // Resolve selected picture bytes as declared inputs, rather than trusting
    // matching metadata inside a scope document alone.
    let mut pictures = BTreeMap::new();
    for event in &lane.events {
        let binding = event.picture_binding.as_deref().ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "film-control-selected-picture".into(),
        })?;
        let asset = compilation(scene::resolve_selected_binding(binding, &scope_refs))?;
        let (path, (declared, _)) = values
            .iter()
            .find(|(_, (a, _))| a.logical_id == asset.logical_id)
            .ok_or_else(|| Error {
                kind: ErrorKind::Missing,
                code: "film-control-picture-bytes".into(),
            })?;
        selected(declared, asset)?;
        used.insert((*path).into());
        pictures.insert(event.event_id.clone(), (*declared).clone());
    }
    if used.len() != values.len()
        || packet.task.inputs.len()
            != values
                .values()
                .map(|(a, _)| &a.logical_id)
                .chain([&tool.request.logical_id, &tool.publication.logical_id])
                .collect::<BTreeSet<_>>()
                .len()
    {
        return fail(ErrorKind::Conflict, "film-control-unused-input");
    }
    let events = compilation(scene::compile_selected_event_request(
        &graph,
        &pointer,
        &episode,
        &authored,
        lang,
        &scope_refs,
        &alignments,
        &request.next_lock_logical_id,
    ))?;
    let next = compilation(assembly::append_semantic_events(&graph, &events))?;
    let next_pointer = compilation(assembly::advance_pointer(&pointer.logical_id, &next))?;
    let spans = compilation(scene::compile_native_event_spans(lang, lane, &alignments))?;
    let review:Vec<_>=spans.iter().map(|s|serde_json::json!({"event_id":s.event_id,"cue_id":s.cue_id,"cel":pictures[&s.event_id],"start_sample":s.start_sample,"end_sample":s.end_sample,"sample_rate":s.sample_rate})).collect();
    let review = serde_json::json!({"schema":"reel.workflow-film-review-map.v1","language":lang,"scene_id":authored.scene_id,"events":review,"rendered":false,"approved":false});
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::from([
        ("events.json".into(), json(&events)?),
        ("graph.json".into(), json(&next)?),
        ("pointer.json".into(), json(&next_pointer)?),
        ("alignments.json".into(), json(&alignments)?),
        ("review-map.json".into(), json(&review)?),
    ]);
    if let Some(settings) = &direction {
        files.insert(
            DIRECTION_OUTPUT.into(),
            json(&crate::direction::preview(
                settings, lang, &spans, &pictures,
            )?)?,
        );
    }
    if packet.task.outputs.len() != OUTPUTS.len() + usize::from(direction.is_some()) {
        return fail(ErrorKind::Conflict, "film-control-output-set");
    }
    let mut outputs = BTreeMap::new();
    for (name, raw) in &files {
        let c = packet.task.outputs.get(name).ok_or_else(|| Error {
            kind: ErrorKind::Missing,
            code: "film-control-output".into(),
        })?;
        if !c.required
            || c.media_type != "application/json"
            || c.max_bytes > MAX_CONTROL_BYTES as u64
            || raw.len() as u64 > c.max_bytes
        {
            return fail(ErrorKind::Budget, "film-control-output-contract");
        }
        outputs.insert(
            name.clone(),
            Artifact {
                logical_id: format!("workflow/{}/{name}", packet.task_key),
                sha256: digest(raw),
                bytes: raw.len() as u64,
                media_type: "application/json".into(),
            },
        );
    }
    let receipt = json(
        &serde_json::json!({"schema":"reel.workflow-film-control-receipt.v1","task_key":packet.task_key,"request":tool.request,"scene":tool.publication,"inputs":packet.task.inputs,"outputs":outputs,"selected":false,"approved":false,"rendered":false}),
    )?;
    if receipt.len() > MAX_CONTROL_BYTES {
        return fail(ErrorKind::Budget, "film-control-receipt");
    }
    files.insert("receipt.json".into(), receipt);
    Ok(Generated { outputs, files })
}
