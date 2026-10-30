//! Assembly-owned source context; child inputs and execution gates stay separate.
use super::*;
use crate::tig_inputs::ArchiveProfile;
use tig_assets::graph::{self, Binding, Collection, EdgeKind, NodeKind, Payloads, Profile, Rule};
use tig_core::tree::{NodeStore, TreeLimits};
use tig_git::source::native::context::ProductionContext;
use tig_store::source_claims::Selection;

fn query_limits() -> graph::Limits {
    graph::Limits {
        nodes: 130,
        edges: 129,
        depth: 2,
        bytes: 16 * 1024 * 1024,
    }
}
fn rule(pointer: &str, collection: Collection) -> Rule {
    Rule {
        pointer: pointer.into(),
        kind: EdgeKind::Production,
        collection,
        optional: false,
        binding: Binding::SnapshotSelected,
    }
}
fn profiles(source: &Source, request: &Request) -> Result<Vec<Profile>> {
    request.validate()?;
    let assembly = source.record(&request.assembly_id)?;
    let authored: Assembly =
        serde_json::from_value(serde_json::Value::Object(assembly.authored().clone()))?;
    let mut profiles = Vec::new();
    for id in [&request.assembly_definition, &request.episode_definition]
        .into_iter()
        .chain(&request.scene_definitions)
    {
        let (rules, payloads) = if id == &request.assembly_definition {
            (vec![rule("/episode", Collection::One)], Payloads::None)
        } else if id == &request.episode_definition {
            (
                vec![rule("/scene_ids", Collection::Array)],
                Payloads::Named {
                    selected: true,
                    slots: BTreeSet::from([authored.conform_slot.clone()]),
                },
            )
        } else {
            (vec![], Payloads::SelectedObject)
        };
        profiles.push(Profile {
            definition_id: id.clone(),
            definition: source.admission(id)?.object.clone(),
            class: NodeKind::Container,
            covered: BTreeSet::from([EdgeKind::Production]),
            rules,
            payloads,
        });
    }
    Ok(profiles)
}
fn descriptor(source: &Source, request: &Request, plan: &Plan) -> Result<Vec<u8>> {
    // Record revisions/checkpoints/status are retained interpretation, not consumed
    // render meaning. Selected component bytes must still enter the recipe even
    // though Output nodes cannot enter the Production graph.
    let components = plan
        .components
        .iter()
        .map(|component| {
            let output = source.record(&component.record.id)?;
            Ok(
                serde_json::json!({"position":component.position,"segment":component.segment_id,
                "kind":component.kind,"id":component.record.id,"selected":component.selected,
                "definition":output.definition()}),
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let bytes = identity::canonical_json(&serde_json::json!({
        "schema":"reel.tig-episode-composition-context.experimental-1",
        "request":request,"language":plan.language,"components":components,
        "adapter":Object::from_bytes(include_bytes!("context.rs")),
        "planner":Object::from_bytes(include_bytes!("../tig_episode.rs")),
        "current_control_compiler":Object::from_bytes(include_bytes!("current_controls.rs")),
        "input_adapter":Object::from_bytes(include_bytes!("../tig_inputs.rs")),
        "tig_source_pin":Object::from_bytes(include_bytes!("../../vendor/tig/source-pin.json")),
    }))?;
    if bytes.len() > 65_536 {
        bail!("episode composition descriptor exceeds budget");
    }
    Ok(bytes)
}
fn consumption(profiles: &[Profile], request: &Request) -> Vec<graph::recipe::Consumption> {
    profiles
        .iter()
        .map(|profile| graph::recipe::Consumption {
            definition_id: profile.definition_id.clone(),
            definition: profile.definition.clone(),
            fields: if profile.definition_id == request.assembly_definition {
                ["episode", "language", "conform_slot", "components"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            } else {
                [
                    "episode_id",
                    if profile.definition_id == request.episode_definition {
                        "scene_ids"
                    } else {
                        "scene_id"
                    },
                ]
                .into_iter()
                .map(str::to_owned)
                .collect()
            },
        })
        .collect()
}
/// Candidate assembly source metadata only. No child-primary closure, component
/// byte availability, current-producer proof, owned job or execution is implied.
pub fn freeze_composition_context(
    source: &mut Source,
    request: &Request,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<ProductionContext> {
    let plan = plan(source, request)?;
    let descriptor = descriptor(source, request, &plan)?;
    source.check_native_context(&descriptor)?;
    source.freeze_context(
        &request.assembly_id,
        &profiles(source, request)?,
        &descriptor,
        store,
        limits,
        query_limits(),
    )
}
/// Exact original source/request/adapter admission; uses no Git or media bodies.
/// Reads the two bounded episode/conform controls from the enrolled byte provider.
pub fn restore_composition_context(
    profile: ArchiveProfile,
    request: &Request,
    selection: &Selection,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<(Source, ProductionContext)> {
    let archive = profile.archive.clone();
    let native_limits = profile.limits;
    let mut source = Source::from_archive(profile, store, limits)?;
    let plan = plan(&mut source, request)?;
    let descriptor = descriptor(&source, request, &plan)?;
    let profiles = profiles(&source, request)?;
    let context = source.replay_context(
        selection,
        &profiles,
        store,
        native_limits,
        limits,
        query_limits(),
    )?;
    if context.archive() != &archive
        || context.roots() != &BTreeSet::from([plan.assembly])
        || context.descriptor() != descriptor
    {
        bail!("episode composition context differs from admitted source or request");
    }
    source.bind_native_context(context.descriptor());
    Ok((source, context))
}
/// Current consumed composition identity. A match does not prove component
/// freshness: native scene/presentation correspondence must be gated separately.
pub fn composition_recipe(
    source: &mut Source,
    request: &Request,
    context: &ProductionContext,
    producer: &Object,
) -> Result<graph::recipe::Plan> {
    let plan = plan(source, request)?;
    let descriptor = descriptor(source, request, &plan)?;
    source.check_native_context(&descriptor)?;
    if context.descriptor() != descriptor
        || context.requirements().checkpoint() != source.checkpoint()
        || context.roots() != &BTreeSet::from([plan.assembly])
    {
        bail!("episode recipe differs from frozen source or composition");
    }
    let profiles = profiles(source, request)?;
    let recipe = source.production_recipe(
        &request.assembly_id,
        &profiles,
        &consumption(&profiles, request),
        &graph::recipe::Operation {
            name: "reel.native-episode.composition.experimental-1".into(),
            semantics: Object::from_bytes(b"ordered-selected-components;explicit-native-source-controls;external-child-currency-and-byte-protection").sha256,
            producer: producer.clone(),
            parameters: Object::from_bytes(&descriptor),
        },
        query_limits(),
    )?;
    if recipe.admission() != context.requirements().admission()
        || recipe.binding() != context.requirements().binding_digest()
    {
        bail!("episode recipe interpretation differs from frozen context");
    }
    Ok(recipe)
}
