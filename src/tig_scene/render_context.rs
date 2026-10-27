//! Compile before freezing semantic parameters; no recipe or execution adoption.
use super::*;

const RENDER_SCHEMA: &str = "reel.tig-native-render-context.experimental-1";
const OPERATION: &str = "reel.native-language.render.experimental-1";
const SEMANTICS: &[u8] = b"reel.native-language.render.experimental-1:derived-language-roots;leaf-payloads;compiled-contracts-and-native-authoring-in-frozen-parameters;full-retained-controls;explicit-producer";
const INPUTS: [(&str, &str); 3] = [
    ("production.json", "production_sha256"),
    ("contract.json", "contract_sha256"),
    ("job.json", "job_sha256"),
];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Parameters {
    schema: String,
    native: Value,
    compiler: Object,
    adapter: Object,
    authoring: tig_core::Digest,
    compiled_inputs: BTreeMap<String, Object>,
}

fn parameters(request: &Request, authoring: tig_core::Digest) -> Result<Parameters> {
    request.validate()?;
    Ok(Parameters {
        schema: RENDER_SCHEMA.into(),
        native: serde_json::from_slice(&scoped::scoped_descriptor(request)?)?,
        compiler: Object::from_bytes(include_bytes!("../scene_delivery_compile.rs")),
        adapter: Object::from_bytes(include_bytes!("render_context.rs")),
        authoring,
        compiled_inputs: BTreeMap::new(),
    })
}

fn check_parameters(raw: &[u8], request: &Request) -> Result<()> {
    if raw.len() > 65_536 {
        bail!("render context descriptor exceeds budget");
    }
    let stored: Parameters = serde_json::from_value(tig_core::identity::parse_json(raw)?)?;
    let expected = parameters(request, stored.authoring.clone())?;
    if stored.schema != expected.schema
        || stored.native != expected.native
        || stored.compiler != expected.compiler
        || stored.adapter != expected.adapter
        || stored.compiled_inputs.len() != INPUTS.len()
        || INPUTS.iter().any(|(name, _)| {
            stored.compiled_inputs.get(*name).is_none_or(|object| {
                object.bytes == 0 || object.bytes > crate::tig_inputs::JOB_BYTES_MAX as u64
            })
        })
    {
        bail!("render context differs from admitted request, mode or compiler");
    }
    Ok(())
}

fn operation(producer: &Object, descriptor: &[u8]) -> graph::recipe::Operation {
    graph::recipe::Operation {
        name: OPERATION.into(),
        semantics: Object::from_bytes(SEMANTICS).sha256,
        producer: producer.clone(),
        parameters: Object::from_bytes(descriptor),
    }
}
fn consumption(profiles: &[Profile]) -> Vec<graph::recipe::Consumption> {
    profiles
        .iter()
        .map(|profile| graph::recipe::Consumption {
            definition_id: profile.definition_id.clone(),
            definition: profile.definition.clone(),
            fields: BTreeSet::new(),
        })
        .collect()
}
fn payloads(
    profiles: &[Profile],
    request: &Request,
) -> Result<Vec<graph::recipe::PayloadConsumption>> {
    profiles
        .iter()
        .map(|profile| {
            let (selected, slots) = if profile.definition_id == request.scene_definition {
                (false, BTreeSet::new())
            } else {
                match &profile.payloads {
                    Payloads::SelectedObject => (true, BTreeSet::new()),
                    Payloads::Named { selected, slots } => (*selected, slots.clone()),
                    _ => bail!("unsupported native primary payload policy"),
                }
            };
            Ok(graph::recipe::PayloadConsumption {
                definition_id: profile.definition_id.clone(),
                definition: profile.definition.clone(),
                selected,
                slots,
            })
        })
        .collect()
}

/// Recompile into NEW private scratch and derive a projected metadata recipe.
/// Retains all scoped controls/requirements; never proves producer execution,
/// complete environment/semantic coverage, eligibility of generated inputs or
/// output custody. The caller independently admits `producer`.
pub fn render_recipe(
    source: &mut Source,
    request: &Request,
    context: &ProductionContext,
    producer: &Object,
    output: &Path,
) -> Result<graph::recipe::Plan> {
    check_parameters(context.descriptor(), request)?;
    source.check_native_context(context.descriptor())?;
    if source.checkpoint() != context.requirements().checkpoint()
        || !context
            .roots()
            .contains(source.record(&request.scene_id)?.reference())
    {
        bail!("render recipe source differs from frozen context");
    }
    let (descriptor, scope) =
        prepare_with_descriptor(source, request, output, context.descriptor())?;
    let roots = scope
        .roots
        .iter()
        .map(|id| source.record(id).map(|r| r.reference().clone()))
        .collect::<Result<BTreeSet<_>>>()?;
    if descriptor != context.descriptor()
        || &roots != context.roots()
        || &scope.required != context.requirements()
    {
        bail!("render recipe differs from original native interpretation");
    }
    source.projected_recipe_for_ids(
        &scope.roots,
        &scope.profiles,
        &consumption(&scope.profiles),
        &payloads(&scope.profiles, request)?,
        &operation(producer, &descriptor),
        query_limits(),
    )
}

/// Replay original projected correspondence after independent native recompilation.
/// Original archive/request/producer admission is required. Writes candidate
/// scratch, reads enrolled controls/alignments, never renders or adopts a job.
/// The returned unsigned claim remains no evidence of execution or byte custody.
pub fn restore_render_claim<'a>(
    profile: ArchiveProfile,
    request: &Request,
    producer: &Object,
    claim: &'a graph::recipe::produced::Claim,
    output: &Path,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<(Source, produced::ReplayedClaim<'a>)> {
    if claim.operation().name != OPERATION
        || claim.operation().semantics != Object::from_bytes(SEMANTICS).sha256
        || &claim.operation().producer != producer
    {
        bail!("projected render claim operation differs from admission");
    }
    let native = profile.limits;
    let selection = Selection {
        root: claim.context().root.clone(),
        context: claim.context().identity.clone(),
    };
    let (mut source, context, scope) =
        restore_with_scope(profile, request, &selection, output, store, limits)?;
    let operation = operation(producer, context.descriptor());
    if claim.operation() != &operation {
        bail!("projected render claim parameters differ from admission");
    }
    let fields = consumption(&scope.profiles);
    let payloads = payloads(&scope.profiles, request)?;
    let query = query_limits();
    let admission = produced::Admission {
        profiles: &scope.profiles,
        consumption: &fields,
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
    let replay = source.replay_produced_with_payloads(claim, &admission, &payloads, store)?;
    if replay.context().selection() != context.selection() {
        bail!("projected render claim context differs from native admission");
    }
    Ok((source, replay))
}

fn prepare(
    source: &mut Source,
    request: &Request,
    output: &Path,
) -> Result<(Vec<u8>, LanguageScope)> {
    prepare_with_descriptor(
        source,
        request,
        output,
        &scoped::scoped_descriptor(request)?,
    )
}
fn prepare_with_descriptor(
    source: &mut Source,
    request: &Request,
    output: &Path,
    admitted: &[u8],
) -> Result<(Vec<u8>, LanguageScope)> {
    let (receipt, scope) = compile_with_scope(source, request, output, admitted)?;
    let mut descriptor = parameters(request, tig_core::Digest::parse(&scope.native_authoring)?)?;
    for (name, field) in INPUTS {
        let mut bytes = Vec::new();
        fs::File::open(output.join("delivery").join(name))?
            .take(crate::tig_inputs::JOB_BYTES_MAX as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.is_empty() || bytes.len() > crate::tig_inputs::JOB_BYTES_MAX {
            bail!("compiled render input exceeds budget");
        }
        let object = Object::from_bytes(&bytes);
        if receipt.get(field).and_then(Value::as_str) != Some(object.sha256.as_str()) {
            bail!("compiled render input differs from compiler result");
        }
        descriptor.compiled_inputs.insert(name.into(), object);
    }
    let raw = serde_json::to_vec(&descriptor)?;
    check_parameters(&raw, request)?;
    Ok((raw, scope))
}

/// Compile to a NEW caller-owned private directory before freezing parameters.
/// Accepts independently admitted unbound or matching language-context sources;
/// render/legacy-bound sources refuse. No media render, job adoption or custody.
/// Caller owns candidate scratch cleanup and metadata finish/reopen/readback.
pub fn freeze_render_context(
    source: &mut Source,
    request: &Request,
    output: &Path,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<ProductionContext> {
    let (descriptor, scope) = prepare(source, request, output)?;
    source.freeze_context_for_ids(
        &scope.roots,
        &scope.profiles,
        &descriptor,
        store,
        limits,
        query_limits(),
    )
}

/// Independently recompile original source meaning and compare frozen contracts.
/// Writes candidate controls/contracts to a NEW caller directory; even on refusal
/// caller owns cleanup. Named control/alignment bytes must exist in enrolled
/// caches. No Git checkout, render, recipe, active-job or eviction is inferred.
pub fn restore_render_context(
    profile: ArchiveProfile,
    request: &Request,
    selection: &Selection,
    output: &Path,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<(Source, ProductionContext)> {
    restore_with_scope(profile, request, selection, output, store, limits)
        .map(|(source, context, _)| (source, context))
}
fn restore_with_scope(
    profile: ArchiveProfile,
    request: &Request,
    selection: &Selection,
    output: &Path,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<(Source, ProductionContext, LanguageScope)> {
    let stored = tig_git::source::native::context::read_descriptor(selection, store, limits)?;
    check_parameters(&stored, request)?;
    let archive = profile.archive.clone();
    let native_limits = profile.limits;
    let mut source = Source::from_archive(profile, store, limits)?;
    let (descriptor, scope) = prepare(&mut source, request, output)?;
    if descriptor != stored {
        bail!("render context parameters differ from original interpretation");
    }
    let roots = scope
        .roots
        .iter()
        .map(|id| source.record(id).map(|r| r.reference().clone()))
        .collect::<Result<BTreeSet<_>>>()?;
    let context = source.replay_context(
        selection,
        &scope.profiles,
        store,
        native_limits,
        limits,
        query_limits(),
    )?;
    if context.archive() != &archive
        || context.roots() != &roots
        || context.descriptor() != descriptor
        || context.requirements() != &scope.required
    {
        bail!("render context differs from admitted native interpretation");
    }
    source.bind_native_context(context.descriptor());
    Ok((source, context, scope))
}
