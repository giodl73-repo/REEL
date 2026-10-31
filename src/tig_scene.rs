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
    io::Read,
    path::Path,
};
use tig_assets::graph::{self, Binding, Collection, EdgeKind, NodeKind, Payloads, Profile, Rule};
use tig_core::{
    Object,
    tree::{NodeStore, TreeLimits},
};
use tig_git::source::native::{context::ProductionContext, produced};
use tig_store::source_claims::Selection;

pub const SCHEMA: &str = "reel.tig-native-scene.experimental-1";
const METADATA_MAX: u64 = 16 * 1024 * 1024;
mod bindings;
mod controls;
mod render_context;
mod scoped;
pub use render_context::{
    freeze_render_context, render_recipe, restore_render_claim, restore_render_context,
};
pub use scoped::{compile_language_context, freeze_language_context, restore_language_context};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub scene_id: String,
    pub scene_definition: String,
    pub primary_definitions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub control_definitions: Vec<String>,
    /// Explicit control-definition ID -> top-level authored JSON object field.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub authored_control_fields: BTreeMap<String, String>,
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
    pub(crate) fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA
            || self.primary_definitions.is_empty()
            || self.primary_definitions.len() > 256
            || self.primary_definitions.contains(&self.scene_definition)
            || self.control_definitions.len() > 128
            || self.control_definitions.contains(&self.scene_definition)
            || self.authored_control_fields.iter().any(|(id, field)| {
                !self.control_definitions.contains(id)
                    || field.is_empty()
                    || field.len() > 128
                    || field.chars().any(char::is_control)
            })
            || self
                .control_definitions
                .iter()
                .any(|id| self.primary_definitions.contains(id))
            || self
                .control_definitions
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != self.control_definitions.len()
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
/// Metadata-only whole-scene recipe, using the same declared Production graph.
/// `producer` is an externally admitted package/runtime pin; this API does not
/// establish that it names the running executable, decoder or external tools.
/// A match never skips compilation eligibility or verifies completed output.
pub fn recipe(
    source: &mut Source,
    request: &Request,
    producer: &Object,
) -> Result<graph::recipe::Plan> {
    let parameters = descriptor(request)?;
    source.check_native_context(&parameters)?;
    let profiles = production_profiles(source, request)?;
    source.production_recipe(
        &request.scene_id,
        &profiles,
        &consumption(&profiles, request),
        &operation(producer, &parameters),
        query_limits(),
    )
}
fn consumption(profiles: &[Profile], request: &Request) -> Vec<graph::recipe::Consumption> {
    profiles
        .iter()
        .map(|profile| graph::recipe::Consumption {
            definition_id: profile.definition_id.clone(),
            definition: profile.definition.clone(),
            fields: if profile.definition_id == request.scene_definition {
                let mut fields = BTreeSet::from(["scene_id".into(), "binding_ids".into()]);
                if profile
                    .rules
                    .iter()
                    .any(|r| r.pointer.starts_with("/language_binding_ids/"))
                {
                    fields.insert("language_binding_ids".into());
                }
                if profile.rules.iter().any(|r| r.pointer == "/control_ids") {
                    fields.insert("control_ids".into());
                }
                fields
            } else if let Some(field) = request.authored_control_fields.get(&profile.definition_id)
            {
                BTreeSet::from([field.clone()])
            } else {
                BTreeSet::new()
            },
        })
        .collect()
}
fn operation(producer: &Object, parameters: &[u8]) -> graph::recipe::Operation {
    graph::recipe::Operation {
        name: "reel.native-scene.declared-inputs.experimental-1".into(),
        semantics: Object::from_bytes(
            b"reel.native-scene.recipe.experimental-1:scene_id,binding_ids,optional-language_binding_ids,optional-control_ids;whole-scene-variant-payloads;explicit-producer;exact-context-parameters",
        )
        .sha256,
        producer: producer.clone(),
        parameters: Object::from_bytes(parameters),
    }
}
fn query_limits() -> graph::Limits {
    graph::Limits {
        nodes: 4225,
        edges: 4224,
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
    let mut catalog = bindings::Catalog::read(parent.authored(), &request.language)?;
    let controls = controls::Catalog::read(source, &parent, request)?;
    if !controls.ids.is_empty() {
        catalog.rules.push(controls::rule());
    }
    let slots = [
        &request.controls.catalog,
        &request.controls.episode,
        &request.controls.policy,
        &request.controls.profile,
    ]
    .into_iter()
    .chain(request.controls.template_receipt.iter())
    .filter(|slot| !controls.ids.contains_key(slot.as_str()))
    .cloned()
    .collect();
    let mut profiles = vec![Profile {
        definition_id: request.scene_definition.clone(),
        definition: source.admission(&request.scene_definition)?.object.clone(),
        class: NodeKind::Container,
        covered: BTreeSet::from([EdgeKind::Production]),
        rules: catalog.rules,
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
                Payload::Selected {} => Payloads::SelectedObject,
                Payload::Stream { slot } => Payloads::Named {
                    selected: false,
                    slots: BTreeSet::from([slot.clone()]),
                },
            },
        });
    }
    for id in &request.control_definitions {
        let admission = source.admission(id)?;
        profiles.push(Profile {
            definition_id: id.clone(),
            definition: admission.object.clone(),
            class: NodeKind::Container,
            covered: BTreeSet::from([EdgeKind::Production]),
            rules: vec![],
            payloads: if request.authored_control_fields.contains_key(id) {
                Payloads::None
            } else {
                controls::payloads(&admission.payload)
            },
        });
    }
    Ok(profiles)
}
fn descriptor(request: &Request) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(&json!({
        "schema":"reel.tig-native-scene-context.experimental-1",
        "request":request,
        "scene_adapter_sha256":Object::from_bytes(include_bytes!("tig_scene.rs")).sha256,
        "binding_adapter_sha256":Object::from_bytes(include_bytes!("tig_scene/bindings.rs")).sha256,
        "control_adapter_sha256":Object::from_bytes(include_bytes!("tig_scene/controls.rs")).sha256,
        "input_adapter_sha256":Object::from_bytes(include_bytes!("tig_inputs.rs")).sha256,
        "tig_source_pin_sha256":Object::from_bytes(include_bytes!("../vendor/tig/source-pin.json")).sha256
    }))?;
    if bytes.len() > 65_536 {
        bail!("native scene context descriptor exceeds budget");
    }
    Ok(bytes)
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

/// Replay an unsigned claim's original native context/recipe, without payload IO.
/// External source/request/producer admission is required; no current/latest
/// fallback, producer execution, output-byte verification or custody is inferred.
pub fn restore_claim<'a>(
    profile: ArchiveProfile,
    request: &Request,
    producer: &Object,
    claim: &'a graph::recipe::produced::Claim,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<(Source, produced::ReplayedClaim<'a>)> {
    request.validate()?;
    let parameters = descriptor(request)?;
    let operation = operation(producer, &parameters);
    if claim.operation() != &operation {
        bail!("produced claim operation differs from native admission");
    }
    let archive = profile.archive.clone();
    let native = profile.limits;
    let mut source = Source::from_archive(profile, store, limits)?;
    let profiles = production_profiles(&source, request)?;
    let consumption = consumption(&profiles, request);
    let root = source.record(&request.scene_id)?.reference().clone();
    let query = query_limits();
    let admission = produced::Admission {
        profiles: &profiles,
        consumption: &consumption,
        operation: &operation,
        native,
        tree: limits,
        query,
        recipe: graph::recipe::Limits {
            nodes: query.nodes,
            edges: query.edges,
            bytes: query.bytes,
        },
    };
    let replay = source.replay_produced(claim, &admission, store)?;
    if replay.context().archive() != &archive
        || replay.context().roots() != &BTreeSet::from([root])
        || replay.context().descriptor() != parameters
    {
        bail!("produced claim context differs from admitted native source/request");
    }
    source.bind_native_context(replay.context().descriptor());
    Ok((source, replay))
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
    let asset = scope
        .assets
        .get(key.context("missing native binding key")?)
        .context("native binding missing")?;
    // Extra compiler inputs (for example sonic-placement evidence) are not all
    // visited by resolve_scene. Enforce eligibility at their consumption too.
    if ![
        "selected-private-production",
        "principal-approved",
        "release-cleared",
    ]
    .contains(&asset.selection_state.as_str())
    {
        bail!("native consumed binding is not selected for production");
    }
    Ok(asset)
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

struct NativeState {
    parent: tig_assets::resolve::Resolved,
    required: Option<graph::ProductionRequirements>,
    unbuilt_catalog_records: Vec<graph::Reference>,
    total: u64,
    scene_bytes: Vec<u8>,
    scene: Scene,
    bindings: ScopedBindings,
    references: serde_json::Map<String, Value>,
    controls: BTreeMap<&'static str, Vec<u8>>,
    pins: BTreeMap<&'static str, Object>,
    authored_controls: BTreeMap<&'static str, Value>,
    empty_episode: ScopedBindings,
    empty_season: ScopedBindings,
    resolved_scene: scene_authoring::ResolvedScene,
}
struct LanguageScope {
    roots: BTreeSet<String>,
    profiles: Vec<Profile>,
    required: graph::ProductionRequirements,
    native_authoring: String,
    profile: Value,
    evidence: Vec<FileRef>,
}
fn read_source(source: &mut Source, request: &Request) -> Result<NativeState> {
    request.validate()?;
    let parent = source.record(&request.scene_id)?;
    if parent.definition().id != request.scene_definition {
        bail!("scene definition differs from admitted protocol");
    }
    let catalog = bindings::Catalog::read(parent.authored(), &request.language)?;
    let control_catalog = controls::Catalog::read(source, &parent, request)?;
    // Admit every declared leaf's type/identity, including unused alternatives.
    // Native resolution checks production eligibility where each input is used;
    // an unused candidate in the authoring catalog is not a selected input.
    let mut catalog_assets = BTreeMap::new();
    let mut unbuilt_catalog_records = Vec::new();
    for id in &catalog.ids {
        let leaf = source.record(id)?;
        if !request.primary_definitions.contains(&leaf.definition().id) {
            bail!("binding is outside admitted primary definitions");
        }
        let asset = source.catalog_asset(id)?;
        if asset.is_none() {
            unbuilt_catalog_records.push(leaf.reference().clone());
        }
        catalog_assets.insert(id.clone(), asset);
    }
    let assets = catalog
        .effective
        .iter()
        .filter_map(|(key, value)| {
            catalog_assets[value["id"].as_str().unwrap()]
                .as_ref()
                .map(|asset| (key.clone(), asset.clone()))
        })
        .collect();
    // Unknown whole-catalog payload demand is never represented by an empty set.
    // The language projection below still derives and verifies its exact inputs.
    let required = if unbuilt_catalog_records.is_empty() {
        Some(requirements(source, request)?)
    } else {
        None
    };
    let mut total = 0;
    let scene_bytes = metadata(source, parent.selected()?, &mut total)?;
    let scene: Scene = serde_json::from_slice(&scene_bytes)?;
    if parent.authored().get("scene_id").and_then(Value::as_str) != Some(scene.scene_id.as_str()) {
        bail!("selected scene identity differs from parent");
    }
    if catalog
        .languages
        .iter()
        .any(|tag| !scene.languages.contains_key(tag))
    {
        bail!("binding variant language is absent from scene");
    }
    let bindings = ScopedBindings {
        schema: scene_authoring::BINDINGS_SCHEMA.into(),
        scope_id: scene.scene_id.clone(),
        assets,
    };
    let mut controls = BTreeMap::new();
    let mut pins = BTreeMap::new();
    let mut authored_controls = BTreeMap::new();
    for (name, slot) in [
        ("catalog.json", &request.controls.catalog),
        ("episode.json", &request.controls.episode),
        ("policy.json", &request.controls.policy),
        ("profile.json", &request.controls.profile),
    ] {
        let control = control_catalog.materialize(source, &parent, slot, &mut total)?;
        controls.insert(name, control.bytes);
        if let Some(origin) = control.authored {
            authored_controls.insert(name, origin);
        } else {
            pins.insert(name, control.pin);
        }
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
    let resolved_scene = scene_authoring::resolve_scene_for_language(
        &serde_json::from_slice(&controls["catalog.json"])?,
        &episode,
        &scene,
        &serde_json::from_slice(&controls["policy.json"])?,
        scene_authoring::SceneBindingScopes {
            season: &empty_season,
            episode: &empty_episode,
            scene: &bindings,
        },
        &request.language,
    )?;
    Ok(NativeState {
        parent,
        required,
        unbuilt_catalog_records,
        total,
        scene_bytes,
        scene,
        bindings,
        references: catalog.effective,
        controls,
        pins,
        authored_controls,
        empty_episode,
        empty_season,
        resolved_scene,
    })
}
fn derive_scope(
    source: &mut Source,
    request: &Request,
    state: &NativeState,
) -> Result<LanguageScope> {
    let NativeState {
        scene,
        bindings,
        references,
        controls,
        resolved_scene,
        ..
    } = state;
    let mut profile: Value = serde_json::from_slice(&controls["profile.json"])?;
    profile
        .as_object_mut()
        .context("profile is not an object")?
        .insert("asset_paths".into(), json!("asset-id"));
    let typed_profile: DeliveryProfile = serde_json::from_value(profile.clone())?;
    if typed_profile.schema != "reel.scene-delivery-profile.v1" {
        bail!("unsupported native delivery profile schema");
    }
    scene_delivery_compile::validate_score_source_starts(
        &typed_profile,
        &serde_json::from_slice(&controls["episode.json"])?,
        &[bindings, &state.empty_episode, &state.empty_season],
    )?;
    let mut language_keys: BTreeSet<_> = resolved_scene
        .inputs_for_language(&request.language)
        .context("resolved scene language missing")?
        .keys()
        .cloned()
        .collect();
    let current = scene_authoring::materialize_scene(scene)?;
    let mut evidence = Vec::new();
    for event in &current.languages[&request.language].events {
        for key in &event.sonic_bindings {
            if let Some(offset) = typed_profile.sonic_placement_offsets_by_binding.get(key) {
                language_keys.insert(offset.evidence_binding.clone());
                let asset = binding(bindings, Some(&offset.evidence_binding))?;
                evidence.push(FileRef {
                    path: asset.logical_id.clone().into(),
                    sha256: asset.sha256.clone(),
                    bytes: asset.bytes,
                });
            }
        }
    }
    // Native semantic membership resolves through the one authored map into
    // exact current roots, without one graph rule per binding.
    let mut language_roots = BTreeSet::from([request.scene_id.clone()]);
    let control_catalog = controls::Catalog::read(source, &state.parent, request)?;
    for slot in [
        &request.controls.catalog,
        &request.controls.episode,
        &request.controls.policy,
        &request.controls.profile,
    ] {
        if let Some(id) = control_catalog.ids.get(slot) {
            language_roots.insert(id.clone());
        }
    }
    for key in &language_keys {
        let id = references
            .get(key)
            .and_then(|r| r.get("id"))
            .and_then(Value::as_str)
            .context("native language input missing from authored binding map")?;
        language_roots.insert(id.to_owned());
    }
    let mut language_profiles = production_profiles(source, request)?;
    language_profiles[0].rules.clear();
    let language_required = source.production_requirements_for_ids(
        &language_roots,
        &language_profiles,
        query_limits(),
    )?;
    Ok(LanguageScope {
        roots: language_roots,
        profiles: language_profiles,
        required: language_required,
        native_authoring: resolved_scene.language_fingerprints[&request.language].clone(),
        profile,
        evidence,
    })
}
/// Materialize bounded metadata and compile into a NEW private local directory.
/// The full native scene is resolved, including its declared language matrix.
/// No primary media copies, cache writes, selection or publication are performed.
pub fn compile(source: &mut Source, request: &Request, output: &Path) -> Result<Value> {
    compile_with_descriptor(source, request, output, &descriptor(request)?)
}
fn compile_with_descriptor(
    source: &mut Source,
    request: &Request,
    output: &Path,
    descriptor: &[u8],
) -> Result<Value> {
    compile_with_scope(source, request, output, descriptor).map(|(receipt, _)| receipt)
}
fn compile_with_scope(
    source: &mut Source,
    request: &Request,
    output: &Path,
    descriptor: &[u8],
) -> Result<(Value, LanguageScope)> {
    request.validate()?;
    source.check_native_context(descriptor)?;
    let state = read_source(source, request)?;
    let scope = derive_scope(source, request, &state)?;
    let NativeState {
        parent,
        required,
        unbuilt_catalog_records,
        mut total,
        scene_bytes,
        scene,
        bindings,
        controls,
        pins,
        authored_controls,
        empty_episode,
        empty_season,
        ..
    } = state;
    let LanguageScope {
        required: language_required,
        native_authoring,
        profile,
        evidence,
        ..
    } = &scope;
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
    // Presentation receipts are independently versioned primary inputs. Resolve
    // the native language binding, not a second retained parent-stream selection.
    let template_object = if let Some(presentation) = &scene.presentation {
        let key = presentation
            .content
            .get("template_receipt_bindings")
            .and_then(|v| v.get(&request.language))
            .and_then(Value::as_str)
            .context("native presentation receipt binding missing for language")?;
        let asset = binding(&bindings, Some(key))?;
        let object = Object {
            sha256: tig_core::Digest::parse(&asset.sha256)?,
            bytes: asset.bytes,
        };
        if let Some(slot) = &request.controls.template_receipt {
            if parent.stream(slot)? != &object {
                bail!("explicit template receipt differs from current native binding");
            }
        }
        Some(object)
    } else {
        request
            .controls
            .template_receipt
            .as_ref()
            .map(|slot| parent.stream(slot).cloned())
            .transpose()?
    };
    let template = template_object
        .as_ref()
        .map(|object| metadata(source, object, &mut total))
        .transpose()?;
    let template_path = template.as_ref().map(|_| "template-receipt.json");
    let language_scope = json!({
        "source_checkpoint":language_required.checkpoint(),
        "admission_sha256":language_required.admission(),
        "binding_sha256":language_required.binding_digest(),
        "native_semantic_sha256":native_authoring,
        "primary_objects":language_required.primary(),
        "control_objects":language_required.controls(),
        "interpretation_objects":language_required.metadata(),
        "complete_semantic_coverage":false,"authorizes_eviction":false,
    });
    // All ordinary metadata is admitted before creating the output directory.
    let mut budget = scene_delivery_compile::MetadataBudget::new(
        scene_delivery_compile::PREPARATION_MAX_BYTES,
        crate::tig_inputs::JOB_BYTES_MAX,
    );
    fs::create_dir(output)?;
    for (name, bytes) in &controls {
        if *name != "profile.json" {
            budget.write_bytes(output, name, bytes)?;
        }
    }
    budget.write_bytes(output, "scene.json", &scene_bytes)?;
    budget.compact(output, "profile.json", profile)?;
    budget.compact(output, "scene-bindings.json", &bindings)?;
    budget.compact(output, "episode-bindings.json", &empty_episode)?;
    budget.compact(output, "season-bindings.json", &empty_season)?;
    budget.compact(output, "graph.json", &graph)?;
    budget.compact(output, "pointer.json", &pointer)?;
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
        budget.write_bytes(output, &file, bytes)?;
        paths.insert(cue.clone(), file);
    }
    budget.compact(output, "alignment-paths.json", &paths)?;
    if let Some(bytes) = template {
        budget.write_bytes(output, "template-receipt.json", &bytes)?;
    }
    let manifest = json!({"schema":"reel.scene-delivery-compile.v1","catalog":"catalog.json","episode":"episode.json","scene":"scene.json","policy":"policy.json", "season_bindings":"season-bindings.json","episode_bindings":"episode-bindings.json","scene_bindings":"scene-bindings.json","graph":"graph.json","pointer":"pointer.json","alignment_paths":"alignment-paths.json","profile":"profile.json","language":request.language,"delivery_id":request.delivery_id,"delivery_title":request.delivery_title,"output_dir":"delivery","template_receipt":template_path});
    let compile: CompileManifest = serde_json::from_value(manifest.clone())?;
    budget.compact(output, "compile.json", &manifest)?;
    let declared_inputs = required.as_ref().map(|required| {
        json!({"source_checkpoint":required.checkpoint(),"admission_sha256":required.admission(),"binding_sha256":required.binding_digest(),"primary_objects":required.primary(),"control_objects":required.controls(),"interpretation_objects":required.metadata(),"complete_semantic_coverage":false,"authorizes_eviction":false})
    });
    budget.compact(
        output,
        "native-source.json",
        &json!({"schema":"reel.tig-native-source.experimental-1","adapter_source_sha256":Object::from_bytes(include_bytes!("tig_scene.rs")).sha256,"tig_source_pin_sha256":Object::from_bytes(include_bytes!("../vendor/tig/source-pin.json")).sha256,"source_checkpoint":source.checkpoint(),"scene_record":parent.reference(),"control_objects":pins,"authored_controls":authored_controls,"primary_definition_ids":request.primary_definitions,"binding_count":bindings.assets.len(),"declared_inputs_available":declared_inputs.is_some(),"declared_inputs":declared_inputs,"unbuilt_catalog_records":unbuilt_catalog_records,"language_inputs":language_scope,"new_production_approval":false,"complete_semantic_coverage":false}),
    )?;
    if evidence.is_empty() {
        scene_delivery_compile::compile_to_dir_with_budget(output, &compile, output, &mut budget)
    } else {
        let inputs = source.prepare_files(evidence)?;
        scene_delivery_compile::compile_to_dir_with_budget(output, &compile, &inputs, &mut budget)
    }
    .map_err(|error| anyhow!("native scene compilation refused: {error}"))
    .map(|receipt| (receipt, scope))
}
