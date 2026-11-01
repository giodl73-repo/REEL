//! Existing Tig graph/recipe/context ports over the exact lowered input scope.
use super::*;
use crate::tig_inputs::ArchiveProfile;
use tig_assets::graph::{self, Binding, Collection, EdgeKind, NodeKind, Payloads, Profile, Rule};
use tig_core::tree::{NodeStore, TreeLimits};
use tig_git::source::native::context::ProductionContext;
use tig_store::source_claims::Selection;

fn limits() -> graph::Limits {
    graph::Limits {
        nodes: 4164,
        edges: 4163,
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
    for file in prepared.consumed_bindings.values() {
        roots.insert(file.path.to_str().context("input id is not UTF-8")?.into());
    }
    let mut profiles = vec![Profile {
        definition_id: request.root_definition.clone(),
        definition: source.admission(&request.root_definition)?.object.clone(),
        class: NodeKind::Container,
        covered: BTreeSet::from([EdgeKind::Production]),
        rules: vec![Rule {
            pointer: "/control_ids".into(),
            kind: EdgeKind::Production,
            collection: Collection::ObjectValues,
            optional: false,
            binding: Binding::SnapshotSelected,
        }],
        payloads: Payloads::None,
    }];
    for id in request
        .primary_definitions
        .iter()
        .chain(&request.control_definitions)
    {
        let admitted = source.admission(id)?;
        let is_control = request.control_definitions.contains(id);
        profiles.push(Profile {
            definition_id: id.clone(),
            definition: admitted.object.clone(),
            class: if is_control {
                NodeKind::Container
            } else {
                NodeKind::Primary
            },
            covered: BTreeSet::from([EdgeKind::Production]),
            rules: vec![],
            payloads: if is_control && request.authored_control_fields.contains_key(id) {
                Payloads::None
            } else {
                crate::tig_scene::controls::payloads(&admitted.payload)
            },
        });
    }
    // Authorization is checked by lower and retained as managed-job input evidence.
    // It must not become consumed rendering meaning or invalidate unchanged media.
    let mut render_request = serde_json::to_value(request)?;
    render_request
        .as_object_mut()
        .unwrap()
        .remove("private_review");
    let descriptor = tig_core::identity::canonical_json(&json!({
        "schema":"reel.tig-delivery-context.experimental-1", "request":render_request,
        "adapter":Object::from_bytes(include_bytes!("../tig_delivery_template.rs")),
        "context_adapter":Object::from_bytes(include_bytes!("context.rs")),
        "compiled":{
            "job":Object::from_bytes(&serde_json::to_vec(&prepared.job)?),
            "contract":Object::from_bytes(&prepared.contract_bytes),
            "production":Object::from_bytes(&prepared.production_bytes)
        }
    }))?;
    source.check_native_context(&descriptor)?;
    Ok(Scope {
        prepared,
        roots,
        profiles,
        descriptor,
    })
}
/// Exact scoped obligations, not global demand, byte availability or custody.
/// Explicit selected controls/receipts may be read; primary media are not read.
pub fn requirements(
    source: &mut Source,
    request: &Request,
) -> Result<graph::ProductionRequirements> {
    let s = scope(source, request)?;
    source.production_requirements_for_ids(&s.roots, &s.profiles, limits())
}
/// Freeze a candidate through existing metadata ports. Finish/readback, source
/// enrollment, runtime and resource admission remain the caller's responsibility.
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
/// Recompute scope/controls under admitted request and source; never select a
/// newer commit or treat the old recipe as proof that execution already occurred.
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
        bail!("delivery context differs from lowered controls, request or adapter");
    }
    let expected_roots = s
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
        || context.roots() != &expected_roots
        || context.requirements() != &expected
    {
        bail!("delivery context differs from admitted source scope");
    }
    source.bind_native_context(&s.descriptor);
    Ok((source, context))
}
/// Metadata recipe only. `producer` is external admission, not verified running
/// code/environment. A match grants no render reuse, approval or removal authority.
pub fn recipe(
    source: &mut Source,
    request: &Request,
    context: &ProductionContext,
    producer: &Object,
) -> Result<graph::recipe::Plan> {
    if context.requirements().checkpoint() != source.checkpoint() {
        bail!("delivery recipe source differs from context");
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
        bail!("delivery recipe differs from frozen controls or scope");
    }
    let consumption = s
        .profiles
        .iter()
        .map(|p| graph::recipe::Consumption {
            definition_id: p.definition_id.clone(),
            definition: p.definition.clone(),
            fields: if p.definition_id == request.root_definition {
                BTreeSet::from(["control_ids".into()])
            } else if let Some(field) = request.authored_control_fields.get(&p.definition_id) {
                BTreeSet::from([field.clone()])
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
                _ => bail!("unsupported delivery payload projection"),
            };
            Ok(graph::recipe::PayloadConsumption {
                definition_id: p.definition_id.clone(),
                definition: p.definition.clone(),
                selected,
                slots,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    source.projected_recipe_for_ids(&s.roots,&s.profiles,&consumption,&payloads,&graph::recipe::Operation {
        name:"reel.tig-delivery.render.experimental-1".into(),
        semantics:Object::from_bytes(b"reel.tig-delivery.render.v1:exact-lowered-controls;scoped-primary-roots;source-provenance-distinct;explicit-producer").sha256,
        producer:producer.clone(),parameters:Object::from_bytes(&s.descriptor),
    },limits())
}
