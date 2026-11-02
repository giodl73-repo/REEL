//! Existing graph, recipe and metadata-context ports; no browser/media IO.
use super::*;
use crate::tig_inputs::ArchiveProfile;
use serde_json::json;
use tig_assets::graph::{self, Binding, Collection, EdgeKind, NodeKind, Payloads, Profile, Rule};
use tig_core::{
    Object, identity,
    tree::{NodeStore, TreeLimits},
};
use tig_git::source::native::context::ProductionContext;
use tig_store::source_claims::Selection;

fn limits() -> graph::Limits {
    graph::Limits {
        nodes: 4097,
        edges: 4096,
        depth: 2,
        bytes: 16 * 1024 * 1024,
    }
}
struct Scope {
    prepared: Prepared,
    roots: BTreeSet<String>,
    profiles: Vec<Profile>,
    descriptor: Vec<u8>,
}
fn scope(source: &mut Source, request: &Request) -> Result<Scope> {
    let prepared = lower(source, request)?;
    let mut roots = BTreeSet::from([request.root_id.clone()]);
    for input in prepared.inputs.values() {
        roots.insert(
            input
                .path
                .to_str()
                .context("browser primary identity is not UTF-8")?
                .into(),
        );
    }
    let mut profiles = vec![Profile {
        definition_id: request.root_definition.clone(),
        definition: source.admission(&request.root_definition)?.object.clone(),
        class: NodeKind::Container,
        covered: BTreeSet::from([EdgeKind::Production]),
        rules: vec![Rule {
            pointer: "/document/inputs".into(),
            kind: EdgeKind::Production,
            collection: Collection::ObjectValues,
            optional: false,
            binding: Binding::SnapshotSelected,
        }],
        payloads: Payloads::None,
    }];
    for id in &request.primary_definitions {
        let admitted = source.admission(id)?;
        profiles.push(Profile {
            definition_id: id.clone(),
            definition: admitted.object.clone(),
            class: NodeKind::Primary,
            covered: BTreeSet::from([EdgeKind::Production]),
            rules: vec![],
            payloads: crate::tig_scene::controls::payloads(&admitted.payload),
        });
    }
    let mut admitted_request = serde_json::to_value(request)?;
    admitted_request
        .as_object_mut()
        .unwrap()
        .remove("private_review");
    let descriptor = identity::canonical_json(&json!({
        "schema":"reel.tig-browser-context.experimental-1","request":admitted_request,
        "adapter":Object::from_bytes(include_bytes!("../tig_browser.rs")),
        "context_adapter":Object::from_bytes(include_bytes!("context.rs")),
        "manifest":Object::from_bytes(&prepared.manifest_bytes)
    }))?;
    source.check_native_context(&descriptor)?;
    Ok(Scope {
        prepared,
        roots,
        profiles,
        descriptor,
    })
}

/// Scoped declared obligations, never complete global demand or byte custody.
pub fn requirements(
    source: &mut Source,
    request: &Request,
) -> Result<graph::ProductionRequirements> {
    let s = scope(source, request)?;
    source.production_requirements_for_ids(&s.roots, &s.profiles, limits())
}

pub fn freeze_context(
    source: &mut Source,
    request: &Request,
    store: &mut impl NodeStore,
    trees: TreeLimits,
) -> Result<(Prepared, ProductionContext)> {
    let s = scope(source, request)?;
    let context = source.freeze_context_for_ids(
        &s.roots,
        &s.profiles,
        &s.descriptor,
        store,
        trees,
        limits(),
    )?;
    Ok((s.prepared, context))
}

pub fn restore_context(
    profile: ArchiveProfile,
    request: &Request,
    selection: &Selection,
    store: &mut impl NodeStore,
    trees: TreeLimits,
) -> Result<(Source, ProductionContext)> {
    let archive = profile.archive.clone();
    let native_limits = profile.limits;
    let mut source = Source::from_archive(profile, store, trees)?;
    let s = scope(&mut source, request)?;
    if tig_git::source::native::context::read_descriptor(selection, store, trees)? != s.descriptor {
        bail!("browser context differs from admitted controls, request or adapter");
    }
    let roots = s
        .roots
        .iter()
        .map(|id| source.record(id).map(|r| r.reference().clone()))
        .collect::<Result<BTreeSet<_>>>()?;
    let expected = source.production_requirements_for_ids(&s.roots, &s.profiles, limits())?;
    let context = source.replay_context(
        selection,
        &s.profiles,
        store,
        native_limits,
        trees,
        limits(),
    )?;
    if context.archive() != &archive
        || context.roots() != &roots
        || context.requirements() != &expected
    {
        bail!("browser context differs from captured primary scope");
    }
    source.bind_native_context(&s.descriptor);
    Ok((source, context))
}

/// Metadata correspondence recipe only; external producer identity is admitted,
/// not verified running code. Browser/job/output qualification is still required.
pub fn recipe(
    source: &mut Source,
    request: &Request,
    context: &ProductionContext,
    producer: &Object,
) -> Result<graph::recipe::Plan> {
    if context.requirements().checkpoint() != source.checkpoint() {
        bail!("browser recipe source differs from context");
    }
    let s = scope(source, request)?;
    let roots = s
        .roots
        .iter()
        .map(|id| source.record(id).map(|r| r.reference().clone()))
        .collect::<Result<BTreeSet<_>>>()?;
    if context.descriptor() != s.descriptor
        || context.roots() != &roots
        || context.requirements()
            != &source.production_requirements_for_ids(&s.roots, &s.profiles, limits())?
    {
        bail!("browser recipe differs from frozen source scope");
    }
    let consumption = s
        .profiles
        .iter()
        .map(|p| graph::recipe::Consumption {
            definition_id: p.definition_id.clone(),
            definition: p.definition.clone(),
            fields: if p.definition_id == request.root_definition {
                BTreeSet::from(["document".into()])
            } else {
                BTreeSet::new()
            },
        })
        .collect::<Vec<_>>();
    let payloads = s
        .profiles
        .iter()
        .map(|p| {
            let (selected, slots) = match &p.payloads {
                Payloads::None => (false, BTreeSet::new()),
                Payloads::SelectedObject => (true, BTreeSet::new()),
                Payloads::Named { selected, slots } => (*selected, slots.clone()),
                _ => bail!("unsupported browser primary payload projection"),
            };
            Ok(graph::recipe::PayloadConsumption {
                definition_id: p.definition_id.clone(),
                definition: p.definition.clone(),
                selected,
                slots,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    // Resource/authorization bounds affect the frozen request, not pixel meaning.
    source.projected_recipe_for_ids(&s.roots,&s.profiles,&consumption,&payloads,&graph::recipe::Operation {
        name:"reel.tig-browser.render.experimental-1".into(),
        semantics:Object::from_bytes(b"reel.tig-browser.v1:exact-primary-map;preserved-svg-clock;no-working-file-fallback").sha256,
        producer:producer.clone(),parameters:Object::from_bytes(&s.prepared.manifest_bytes),
    },limits())
}
