//! Declared presentation closure and replayable interpretation; no execution authority.
use super::*;
use crate::tig_inputs::{ArchiveProfile, Payload};
use tig_assets::graph::{self, Binding, Collection, EdgeKind, NodeKind, Payloads, Profile, Rule};
use tig_core::tree::{NodeStore, TreeLimits};
use tig_git::source::native::context::ProductionContext;
use tig_store::source_claims::Selection;

fn limits() -> graph::Limits {
    graph::Limits {
        nodes: 160,
        edges: 180,
        depth: 4,
        bytes: 16 * 1024 * 1024,
    }
}
fn rule(pointer: &str, collection: Collection, optional: bool) -> Rule {
    Rule {
        pointer: pointer.into(),
        kind: EdgeKind::Production,
        collection,
        optional,
        binding: Binding::SnapshotSelected,
    }
}
fn profiles(source: &Source, request: &Request) -> Result<Vec<Profile>> {
    request.validate()?;
    let controls = [
        (
            &request.source_definition,
            vec![
                rule("/document/template", Collection::One, false),
                rule("/document/pictures", Collection::Array, false),
                rule("/document/editable_layer", Collection::One, false),
                rule("/document/audio", Collection::One, true),
                rule("/document/fonts", Collection::Array, true),
                rule("/document/picture_timing", Collection::One, true),
            ],
        ),
        (&request.template_definition, vec![]),
        (
            &request.timing_definition,
            vec![rule("/document/clock", Collection::One, false)],
        ),
        (
            &request.clock_definition,
            vec![
                rule("/document/source", Collection::One, false),
                rule("/document/evidence", Collection::One, false),
            ],
        ),
        (&request.evidence_definition, vec![]),
    ];
    let mut result = Vec::new();
    for (id, rules) in controls {
        result.push(Profile {
            definition_id: id.clone(),
            definition: source.admission(id)?.object.clone(),
            class: NodeKind::Container,
            covered: BTreeSet::from([EdgeKind::Production]),
            rules,
            payloads: Payloads::None,
        });
    }
    for id in &request.primary_definitions {
        let admission = source.admission(id)?;
        result.push(Profile {
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
    Ok(result)
}
fn descriptor(request: &Request, compiled: &CompiledPresentation) -> Result<Vec<u8>> {
    let bytes = identity::canonical_json(&json!({
        "schema":"reel.tig-presentation-context.experimental-1", "request":request,
        "manifest":compiled.manifest(),
        "compiler":Object::from_bytes(include_bytes!("../tig_presentation.rs")),
        "context_adapter":Object::from_bytes(include_bytes!("context.rs")),
        "producer_kernel":Object::from_bytes(include_bytes!("../presentation_still_sequence.rs")),
        "control_reader":Object::from_bytes(include_bytes!("../scene_delivery.rs")),
        "input_adapter":Object::from_bytes(include_bytes!("../tig_inputs.rs")),
        "tig_pin":Object::from_bytes(include_bytes!("../../vendor/tig/source-pin.json")),
    }))?;
    if bytes.len() > 65_536 {
        bail!("presentation interpretation exceeds budget");
    }
    Ok(bytes)
}
/// Full declared closure; literal documents are metadata, media are primaries.
/// Successful metadata traversal proves neither availability nor render currency.
pub fn requirements(
    source: &mut Source,
    request: &Request,
) -> Result<graph::ProductionRequirements> {
    compile(source, request)?;
    source.production_requirements(
        &request.presentation_id,
        &profiles(source, request)?,
        limits(),
    )
}
/// Freeze after native metadata validation, with zero primary byte reads.
/// Caller owns durable storage, readback and source/job resource admission.
pub fn freeze_context(
    source: &mut Source,
    request: &Request,
    store: &mut impl NodeStore,
    tree: TreeLimits,
) -> Result<ProductionContext> {
    let compiled = compile(source, request)?;
    let descriptor = descriptor(request, &compiled)?;
    source.check_native_context(&descriptor)?;
    source.freeze_context(
        &request.presentation_id,
        &profiles(source, request)?,
        &descriptor,
        store,
        tree,
        limits(),
    )
}
/// Recompile the same source without its Git checkout or control-body files.
/// A different request, compiler, selected input or original archive is refused.
pub fn restore_context(
    profile: ArchiveProfile,
    request: &Request,
    selection: &Selection,
    store: &mut impl NodeStore,
    tree: TreeLimits,
) -> Result<(Source, ProductionContext)> {
    let archive = profile.archive.clone();
    let native = profile.limits;
    let mut source = Source::from_archive(profile, store, tree)?;
    let compiled = compile(&source, request)?;
    let descriptor = descriptor(request, &compiled)?;
    let context = source.replay_context(
        selection,
        &profiles(&source, request)?,
        store,
        native,
        tree,
        limits(),
    )?;
    if context.archive() != &archive
        || context.roots() != &BTreeSet::from([compiled.root().clone()])
        || context.descriptor() != descriptor
    {
        bail!("presentation context differs from source or native interpretation");
    }
    source.bind_native_context(context.descriptor());
    Ok((source, context))
}
/// Consumed render identity. An annotation-only change can preserve this recipe;
/// exact source custody/checkpoints remain separate from consumed meaning.
/// The producer pin is explicit caller admission, not verified execution.
pub fn recipe(
    source: &mut Source,
    request: &Request,
    context: &ProductionContext,
    producer: &Object,
) -> Result<graph::recipe::Plan> {
    let compiled = compile(source, request)?;
    let descriptor = descriptor(request, &compiled)?;
    source.check_native_context(&descriptor)?;
    if context.descriptor() != descriptor
        || context.requirements().checkpoint() != source.checkpoint()
        || context.roots() != &BTreeSet::from([compiled.root().clone()])
    {
        bail!("presentation recipe differs from frozen source or interpretation");
    }
    let profiles = profiles(source, request)?;
    let consumption = profiles
        .iter()
        .map(|profile| graph::recipe::Consumption {
            definition_id: profile.definition_id.clone(),
            definition: profile.definition.clone(),
            fields: if profile.class == NodeKind::Container {
                BTreeSet::from(["document".into()])
            } else {
                BTreeSet::new()
            },
        })
        .collect::<Vec<_>>();
    let plan = source.production_recipe(&request.presentation_id, &profiles, &consumption, &graph::recipe::Operation {
        name:"reel.native-presentation.declared-inputs.experimental-1".into(),
        semantics:Object::from_bytes(b"logical-primary-ids;literal-native-controls;measured-source-clock;exact-compiled-manifest;independent-producer-admission").sha256,
        producer:producer.clone(), parameters:Object::from_bytes(&descriptor),
    }, limits())?;
    if plan.admission() != context.requirements().admission()
        || plan.binding() != context.requirements().binding_digest()
    {
        bail!("presentation recipe differs from admitted dependency closure");
    }
    Ok(plan)
}
