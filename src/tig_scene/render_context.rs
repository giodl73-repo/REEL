//! Compile before freezing semantic parameters; no recipe or execution adoption.
use super::*;

const RENDER_SCHEMA: &str = "reel.tig-native-render-context.experimental-1";
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

fn prepare(
    source: &mut Source,
    request: &Request,
    output: &Path,
) -> Result<(Vec<u8>, LanguageScope)> {
    let (receipt, scope) = compile_with_scope(
        source,
        request,
        output,
        &scoped::scoped_descriptor(request)?,
    )?;
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
    Ok((source, context))
}
