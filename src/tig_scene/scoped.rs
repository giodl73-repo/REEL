//! Explicit native language context candidates; not execution or custody.
use super::*;

fn scoped_descriptor(request: &Request) -> Result<Vec<u8>> {
    let native: Value = serde_json::from_slice(&descriptor(request)?)?;
    let raw = serde_json::to_vec(&json!({
        "schema":"reel.tig-native-language-context.experimental-1",
        "native":native,
        "scope_adapter_sha256":Object::from_bytes(include_bytes!("scoped.rs")).sha256,
    }))?;
    if raw.len() > 65_536 {
        bail!("scoped descriptor exceeds budget");
    }
    Ok(raw)
}
/// Compile with explicit scoped-context descriptor admission. This preserves
/// native preflight and verified alignment reads; it does not execute a render,
/// adopt a job or establish a producer recipe. Legacy-mode bound sources refuse.
pub fn compile_language_context(
    source: &mut Source,
    request: &Request,
    output: &Path,
) -> Result<Value> {
    compile_with_descriptor(source, request, output, &scoped_descriptor(request)?)
}
/// Read exact named native controls and freeze the derived scoped Production
/// query through existing metadata ports. No primary media/alignment reads.
/// Candidate finish/readback and owner adoption remain the caller's work.
pub fn freeze_language_context(
    source: &mut Source,
    request: &Request,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<ProductionContext> {
    let descriptor = scoped_descriptor(request)?;
    source.check_native_context(&descriptor)?;
    let state = read_source(source, request)?;
    let scope = derive_scope(source, request, &state)?;
    source.freeze_context_for_ids(
        &scope.roots,
        &scope.profiles,
        &descriptor,
        store,
        limits,
        query_limits(),
    )
}
/// Replay original roots under independent archive/cache/request admission.
/// Reads named control bytes from existing caches; source checkout not needed.
/// This does not archive those bytes, prove media availability or admit a job.
/// Returned sources are scoped-mode bound and legacy compile/recipe refuse them.
pub fn restore_language_context(
    profile: ArchiveProfile,
    request: &Request,
    selection: &Selection,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<(Source, ProductionContext)> {
    let archive = profile.archive.clone();
    let native_limits = profile.limits;
    let descriptor = scoped_descriptor(request)?;
    if tig_git::source::native::context::read_descriptor(selection, store, limits)? != descriptor {
        bail!("scoped context descriptor differs from admitted request or mode");
    }
    let mut source = Source::from_archive(profile, store, limits)?;
    let state = read_source(&mut source, request)?;
    let scope = derive_scope(&mut source, request, &state)?;
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
        bail!("scoped context differs from admitted native interpretation");
    }
    source.bind_native_context(context.descriptor());
    Ok((source, context))
}
