//! Native scene compilation from one admitted Tig metadata view.
//! Small local controls are derived; primary media remain in existing caches.
use crate::{
    scene_delivery::FileRef,
    scene_delivery_compile::{self, CompileManifest, DeliveryProfile},
    tig_inputs::{ArchiveProfile, Payload, Source},
};
use anyhow::{Context, Result, anyhow, bail};
use reel_assembly::{
    Asset, Disposition, Graph, ImmutableRef, Lane, Node, Revision, SelectedPointer, SemanticEvent,
    Slot,
    scene_authoring::{self, AssetRef, Episode, NativeAlignment, Scene, ScopedBindings},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::Path,
};
use tig_assets::graph::{self, Binding, Collection, EdgeKind, NodeKind, Payloads, Profile, Rule};
use tig_core::{
    Object,
    tree::{NodeStore, TreeLimits},
};
use tig_git::source::native::context::ProductionContext;
use tig_store::source_claims::Selection;

pub const SCHEMA: &str = "reel.tig-native-scene.experimental-1";
const METADATA_MAX: u64 = 16 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub scene_id: String,
    pub scene_definition: String,
    pub primary_definitions: Vec<String>,
    pub language: String,
    pub controls: Controls,
    pub delivery_id: String,
    pub delivery_title: String,
}
/// Exact named control streams on the admitted parent record. These are not
/// executable recipes or filesystem paths. Scene content is its selected object.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Controls {
    pub catalog: String,
    pub episode: String,
    pub policy: String,
    pub profile: String,
    pub template_receipt: Option<String>,
}
impl Request {
    fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA
            || self.primary_definitions.is_empty()
            || self.primary_definitions.len() > 256
            || self.primary_definitions.contains(&self.scene_definition)
            || self
                .primary_definitions
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != self.primary_definitions.len()
        {
            bail!("invalid native scene protocol admission");
        }
        Ok(())
    }
    pub fn read(path: &Path) -> Result<Self> {
        let mut bytes = Vec::new();
        fs::File::open(path)?.take(65_537).read_to_end(&mut bytes)?;
        if bytes.len() > 65_536 {
            bail!("native compile request exceeds budget");
        }
        let value = tig_core::identity::parse_json(&bytes)?;
        Ok(serde_json::from_value(value)?)
    }
}

/// Derive the entire declared native scene input set, without byte/cache IO.
/// Whole-scene coverage is distinct from minimal per-language consumption,
/// production eligibility, opaque semantic completeness and retained custody.
pub fn requirements(
    source: &mut Source,
    request: &Request,
) -> Result<graph::ProductionRequirements> {
    let profiles = production_profiles(source, request)?;
    source.production_requirements(&request.scene_id, &profiles, query_limits())
}
fn query_limits() -> graph::Limits {
    graph::Limits {
        nodes: 4097,
        edges: 4096,
        depth: 2,
        bytes: 16 * 1024 * 1024,
    }
}
fn production_profiles(source: &Source, request: &Request) -> Result<Vec<Profile>> {
    request.validate()?;
    let parent = source.record(&request.scene_id)?;
    if parent.definition().id != request.scene_definition {
        bail!("scene definition differs from admitted protocol");
    }
    let references = parent
        .authored()
        .get("binding_ids")
        .and_then(Value::as_object)
        .context("scene binding_ids missing")?;
    if references.is_empty()
        || references.len() > 4096
        || references
            .keys()
            .any(|key| key.is_empty() || key.len() > 256 || key.chars().any(char::is_control))
    {
        bail!("native binding count/key exceeds budget");
    }
    let slots = [
        &request.controls.catalog,
        &request.controls.episode,
        &request.controls.policy,
        &request.controls.profile,
    ]
    .into_iter()
    .chain(request.controls.template_receipt.iter())
    .cloned()
    .collect();
    let mut profiles = vec![Profile {
        definition_id: request.scene_definition.clone(),
        definition: source.admission(&request.scene_definition)?.object.clone(),
        class: NodeKind::Container,
        covered: BTreeSet::from([EdgeKind::Production]),
        rules: vec![Rule {
            pointer: "/binding_ids".into(),
            kind: EdgeKind::Production,
            collection: Collection::ObjectValues,
            optional: false,
            binding: Binding::SnapshotSelected,
        }],
        payloads: Payloads::Named {
            selected: true,
            slots,
        },
    }];
    for id in &request.primary_definitions {
        let admission = source.admission(id)?;
        profiles.push(Profile {
            definition_id: id.clone(),
            definition: admission.object.clone(),
            class: NodeKind::Primary,
            covered: BTreeSet::from([EdgeKind::Production]),
            rules: vec![],
            payloads: match &admission.payload {
                Payload::Selected => Payloads::SelectedObject,
                Payload::Stream { slot } => Payloads::Named {
                    selected: false,
                    slots: BTreeSet::from([slot.clone()]),
                },
            },
        });
    }
    Ok(profiles)
}
fn descriptor(request: &Request) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&json!({
        "schema":"reel.tig-native-scene-context.experimental-1",
        "request":request,
        "scene_adapter_sha256":Object::from_bytes(include_bytes!("tig_scene.rs")).sha256,
        "input_adapter_sha256":Object::from_bytes(include_bytes!("tig_inputs.rs")).sha256,
        "tig_source_pin_sha256":Object::from_bytes(include_bytes!("../vendor/tig/source-pin.json")).sha256
    }))?)
}
/// Freeze declared metadata and exact request as a candidate root, with no byte IO.
/// Caller owns finish/reopen/readback and owner-journal adoption before job handoff.
pub fn freeze_context(
    source: &mut Source,
    request: &Request,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<ProductionContext> {
    source.check_native_context(&descriptor(request)?)?;
    let profiles = production_profiles(source, request)?;
    source.freeze_context(
        &request.scene_id,
        &profiles,
        &descriptor(request)?,
        store,
        limits,
        query_limits(),
    )
}
/// Replay under externally admitted source/policy/request, without Git or byte IO.
/// Exact adapter identity is required; this neither adopts nor executes a job.
pub fn restore_context(
    profile: ArchiveProfile,
    request: &Request,
    selection: &Selection,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<(Source, ProductionContext)> {
    let archive = profile.archive.clone();
    let native_limits = profile.limits;
    let mut source = Source::from_archive(profile, store, limits)?;
    let profiles = production_profiles(&source, request)?;
    let root = source.record(&request.scene_id)?.reference().clone();
    let context = source.replay_context(
        selection,
        &profiles,
        store,
        native_limits,
        limits,
        query_limits(),
    )?;
    if context.archive() != &archive
        || context.roots() != &BTreeSet::from([root])
        || context.descriptor() != descriptor(request)?
    {
        bail!("frozen native context differs from admitted request/source/tool");
    }
    source.bind_native_context(context.descriptor());
    Ok((source, context))
}
fn put(root: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(name))?
        .write_all(bytes)?;
    Ok(())
}
fn json_file(root: &Path, name: &str, value: &impl serde::Serialize) -> Result<()> {
    put(root, name, &serde_json::to_vec(value)?)
}
fn metadata(source: &mut Source, object: &Object, total: &mut u64) -> Result<Vec<u8>> {
    *total = total
        .checked_add(object.bytes)
        .context("metadata size overflow")?;
    if *total > METADATA_MAX {
        bail!("native scene metadata exceeds aggregate budget");
    }
    source.read_metadata(object, crate::tig_inputs::JOB_BYTES_MAX)
}
fn binding<'a>(scope: &'a ScopedBindings, key: Option<&str>) -> Result<&'a AssetRef> {
    scope
        .assets
        .get(key.context("missing native binding key")?)
        .context("native binding missing")
}

/// Build a current, derived assembly projection. Version history stays in Tig
/// records/Git; no retained selected graph or binding snapshot overrides them.
fn snapshot_graph(
    scene: &Scene,
    language: &str,
    bindings: &ScopedBindings,
    alignments: &BTreeMap<String, NativeAlignment>,
) -> Result<(Graph, SelectedPointer)> {
    let current = scene_authoring::materialize_scene(scene)?;
    let lane = current
        .languages
        .get(language)
        .context("scene language missing")?;
    let spans = scene_authoring::compile_native_event_spans(language, lane, alignments)?;
    let mut slots = BTreeMap::<String, Slot>::new();
    let mut add = |id: &str, beat: &str, lane: Lane, asset: &AssetRef| -> Result<()> {
        if let Some(old) = slots.get(id) {
            if old.lane != lane
                || old.revisions[0].asset.sha256 != asset.sha256
                || old.revisions[0].asset.logical_id != asset.logical_id
            {
                bail!("native slot has conflicting bindings");
            }
            return Ok(());
        }
        slots.insert(
            id.into(),
            Slot {
                slot_id: id.into(),
                beat_id: beat.into(),
                lane,
                disposition: Disposition::Selected,
                selected_revision_id: Some(asset.sha256.clone()),
                revisions: vec![Revision {
                    revision_id: asset.sha256.clone(),
                    supersedes: None,
                    asset: Asset {
                        logical_id: asset.logical_id.clone(),
                        sha256: asset.sha256.clone(),
                        cache_uri: asset.cache_uri.clone(),
                    },
                }],
            },
        );
        Ok(())
    };
    for cue in &lane.cues {
        let take = binding(bindings, cue.take_binding.as_deref())?;
        scene_authoring::validate_native_alignment(
            &alignments[&cue.cue_id],
            language,
            &cue.cue_id,
            &take.sha256,
        )?;
        add(&cue.narration_slot_id, &cue.cue_id, Lane::Narration, take)?;
    }
    let mut events = Vec::new();
    for (event, span) in lane.events.iter().zip(spans) {
        let cue = lane
            .cues
            .iter()
            .find(|cue| cue.cue_id == event.cue_id)
            .context("event cue missing")?;
        let take = binding(bindings, cue.take_binding.as_deref())?;
        let picture = binding(bindings, event.picture_binding.as_deref())?;
        add(&event.picture_slot_id, &cue.cue_id, Lane::Picture, picture)?;
        events.push(SemanticEvent {
            event_id: event.event_id.clone(),
            scene_id: scene.scene_id.clone(),
            language: language.into(),
            narration: ImmutableRef {
                logical_id: take.logical_id.clone(),
                sha256: take.sha256.clone(),
            },
            picture: ImmutableRef {
                logical_id: picture.logical_id.clone(),
                sha256: picture.sha256.clone(),
            },
            phrase_start_seconds: span.start_sample as f64 / f64::from(span.sample_rate),
            phrase_end_seconds: span.end_sample as f64 / f64::from(span.sample_rate),
        });
    }
    let lock_id = "tig-current-scene";
    let mut graph = Graph {
        schema: reel_assembly::GRAPH_SCHEMA.into(),
        lock: ImmutableRef {
            logical_id: lock_id.into(),
            sha256: "0".repeat(64),
        },
        nodes: vec![Node {
            id: scene.scene_id.clone(),
            inputs: vec![],
            slots: slots.keys().cloned().collect(),
            events: events.iter().map(|e| e.event_id.clone()).collect(),
        }],
        slots: slots.into_values().collect(),
        events,
        presentation_targets: vec![],
    };
    graph.lock = reel_assembly::graph_lock(&graph, lock_id)?;
    let pointer = SelectedPointer {
        schema: reel_assembly::POINTER_SCHEMA.into(),
        logical_id: "tig-current-scene-selection".into(),
        selected_lock: graph.lock.clone(),
    };
    reel_assembly::selected_closure(&pointer, &graph, &scene.scene_id)?;
    Ok((graph, pointer))
}

/// Materialize bounded metadata and compile into a NEW private local directory.
/// The full native scene is resolved, including its declared language matrix.
/// No primary media copies, cache writes, selection or publication are performed.
pub fn compile(source: &mut Source, request: &Request, output: &Path) -> Result<Value> {
    request.validate()?;
    source.check_native_context(&descriptor(request)?)?;
    let parent = source.record(&request.scene_id)?;
    if parent.definition().id != request.scene_definition {
        bail!("scene definition differs from admitted protocol");
    }
    let references = parent
        .authored()
        .get("binding_ids")
        .and_then(Value::as_object)
        .context("scene binding_ids missing")?;
    if references.is_empty() || references.len() > 4096 {
        bail!("native binding count exceeds budget");
    }
    // Resolve every declared leaf and status before reading control/media bytes.
    let mut assets = BTreeMap::new();
    for (key, value) in references {
        let map = value
            .as_object()
            .context("binding reference must be an object")?;
        let id = map
            .get("id")
            .and_then(Value::as_str)
            .context("binding ID missing")?;
        if map.len() != 1 || key.is_empty() || key.len() > 256 || key.chars().any(char::is_control)
        {
            bail!("unsupported native binding reference");
        }
        let leaf = source.record(id)?;
        if !request.primary_definitions.contains(&leaf.definition().id) {
            bail!("binding is outside admitted primary definitions");
        }
        let selected = source.resolve_asset(id)?;
        if ![
            "selected-private-production",
            "principal-approved",
            "release-cleared",
        ]
        .contains(&selected.selection_state.as_str())
        {
            bail!("native binding is not selected for production");
        }
        assets.insert(key.clone(), selected);
    }
    let required = requirements(source, request)?;
    let mut total = 0;
    let scene_bytes = metadata(source, parent.selected()?, &mut total)?;
    let scene: Scene = serde_json::from_slice(&scene_bytes)?;
    if parent.authored().get("scene_id").and_then(Value::as_str) != Some(scene.scene_id.as_str()) {
        bail!("selected scene identity differs from parent");
    }
    let bindings = ScopedBindings {
        schema: scene_authoring::BINDINGS_SCHEMA.into(),
        scope_id: scene.scene_id.clone(),
        assets,
    };
    let mut controls = BTreeMap::new();
    let mut pins = BTreeMap::new();
    for (name, slot) in [
        ("catalog.json", &request.controls.catalog),
        ("episode.json", &request.controls.episode),
        ("policy.json", &request.controls.policy),
        ("profile.json", &request.controls.profile),
    ] {
        let pin = parent.stream(slot)?;
        controls.insert(name, metadata(source, pin, &mut total)?);
        pins.insert(name, pin.clone());
    }
    let episode: Episode = serde_json::from_slice(&controls["episode.json"])?;
    let empty_episode = ScopedBindings {
        schema: scene_authoring::BINDINGS_SCHEMA.into(),
        scope_id: episode.episode_id.clone(),
        assets: BTreeMap::new(),
    };
    let empty_season = ScopedBindings {
        schema: scene_authoring::BINDINGS_SCHEMA.into(),
        scope_id: episode.season_id.clone(),
        assets: BTreeMap::new(),
    };
    scene_authoring::resolve_scene(
        &serde_json::from_slice(&controls["catalog.json"])?,
        &episode,
        &scene,
        &serde_json::from_slice(&controls["policy.json"])?,
        &empty_season,
        &empty_episode,
        &bindings,
    )?;
    let mut alignment_bytes = BTreeMap::new();
    let alignments = crate::scene_authoring_inputs::read_verified_alignments_from_assets(
        &scene,
        &request.language,
        &[&bindings],
        |asset, limit| {
            let object = Object {
                sha256: tig_core::Digest::parse(&asset.sha256)?,
                bytes: asset.bytes,
            };
            if object.bytes > limit as u64 {
                bail!("alignment exceeds budget");
            }
            let bytes = metadata(source, &object, &mut total)?;
            alignment_bytes.insert(asset.logical_id.clone(), bytes.clone());
            Ok(bytes)
        },
    )?;
    let (graph, pointer) = snapshot_graph(&scene, &request.language, &bindings, &alignments)?;
    let mut profile: Value = serde_json::from_slice(&controls["profile.json"])?;
    profile
        .as_object_mut()
        .context("profile is not an object")?
        .insert("asset_paths".into(), json!("asset-id"));
    let typed_profile: DeliveryProfile = serde_json::from_value(profile.clone())?;
    let current = scene_authoring::materialize_scene(&scene)?;
    let mut evidence = Vec::new();
    for event in &current.languages[&request.language].events {
        for key in &event.sonic_bindings {
            if let Some(offset) = typed_profile.sonic_placement_offsets_by_binding.get(key) {
                let asset = binding(&bindings, Some(&offset.evidence_binding))?;
                evidence.push(FileRef {
                    path: asset.logical_id.clone().into(),
                    sha256: asset.sha256.clone(),
                    bytes: asset.bytes,
                });
            }
        }
    }
    let template = request
        .controls
        .template_receipt
        .as_ref()
        .map(|slot| -> Result<Vec<u8>> { metadata(source, parent.stream(slot)?, &mut total) })
        .transpose()?;
    // All ordinary metadata is admitted before creating the output directory.
    fs::create_dir(output)?;
    for (name, bytes) in &controls {
        if *name != "profile.json" {
            put(output, name, bytes)?;
        }
    }
    put(output, "scene.json", &scene_bytes)?;
    json_file(output, "profile.json", &profile)?;
    json_file(output, "scene-bindings.json", &bindings)?;
    json_file(output, "episode-bindings.json", &empty_episode)?;
    json_file(output, "season-bindings.json", &empty_season)?;
    json_file(output, "graph.json", &graph)?;
    json_file(output, "pointer.json", &pointer)?;
    let mut paths = BTreeMap::new();
    for (index, cue) in alignments.keys().enumerate() {
        // Preserve exact selected alignment bytes; reserializing can change pins.
        let cue_source = scene.languages[&request.language]
            .cues
            .iter()
            .find(|c| c.cue_id == *cue)
            .context("alignment cue absent")?;
        let asset = binding(&bindings, cue_source.phrase_alignment_binding.as_deref())?;
        let file = format!("alignment-{index:04}.json");
        let bytes = &alignment_bytes[&asset.logical_id];
        put(output, &file, bytes)?;
        paths.insert(cue.clone(), file);
    }
    json_file(output, "alignment-paths.json", &paths)?;
    if let Some(bytes) = template {
        put(output, "template-receipt.json", &bytes)?;
    }
    let manifest = json!({"schema":"reel.scene-delivery-compile.v1","catalog":"catalog.json","episode":"episode.json","scene":"scene.json","policy":"policy.json", "season_bindings":"season-bindings.json","episode_bindings":"episode-bindings.json","scene_bindings":"scene-bindings.json","graph":"graph.json","pointer":"pointer.json","alignment_paths":"alignment-paths.json","profile":"profile.json","language":request.language,"delivery_id":request.delivery_id,"delivery_title":request.delivery_title,"output_dir":"delivery","template_receipt":request.controls.template_receipt.as_ref().map(|_|"template-receipt.json")});
    let compile: CompileManifest = serde_json::from_value(manifest.clone())?;
    json_file(output, "compile.json", &manifest)?;
    json_file(
        output,
        "native-source.json",
        &json!({"schema":"reel.tig-native-source.experimental-1","adapter_source_sha256":Object::from_bytes(include_bytes!("tig_scene.rs")).sha256,"tig_source_pin_sha256":Object::from_bytes(include_bytes!("../vendor/tig/source-pin.json")).sha256,"source_checkpoint":source.checkpoint(),"scene_record":parent.reference(),"control_objects":pins,"primary_definition_ids":request.primary_definitions,"binding_count":bindings.assets.len(),"declared_inputs":{"source_checkpoint":required.checkpoint(),"admission_sha256":required.admission(),"binding_sha256":required.binding_digest(),"primary_objects":required.primary(),"control_objects":required.controls(),"interpretation_objects":required.metadata(),"complete_semantic_coverage":false,"authorizes_eviction":false},"new_production_approval":false,"complete_semantic_coverage":false}),
    )?;
    if evidence.is_empty() {
        scene_delivery_compile::compile_to_dir(output, &compile)
    } else {
        let inputs = source.prepare_files(&evidence)?;
        scene_delivery_compile::compile_to_dir_with_inputs(output, &compile, &inputs)
    }
    .map_err(|error| anyhow!("native scene compilation refused: {error}"))
}
