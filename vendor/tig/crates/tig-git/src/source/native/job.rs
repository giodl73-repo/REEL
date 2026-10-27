//! Declared job resource roots through existing metadata and source-claim ports.
//! No execution, payload availability, complete enrollment or removal authority.
use super::{
    Error, Result,
    archive::{put, read, tree_error},
    context::ProductionContext,
};
use std::collections::{BTreeMap, BTreeSet};
use tig_assets::graph::recipe::{Plan, produced::Claim};
use tig_core::{
    LogicalPath, Object, identity,
    tree::{NodeStore, Tree, TreeLimits},
};
use tig_store::source_claims::Selection;

pub struct OutputEvidence<'a> {
    pub claim: &'a Claim,
    pub attachments: &'a BTreeMap<LogicalPath, Object>,
}
/// Caller admits all runtime interpretation and original plan authority.
pub struct Admission<'a> {
    pub context: &'a ProductionContext,
    pub plan: &'a Plan,
    pub runtime: &'a BTreeMap<LogicalPath, Object>,
    pub output: Option<OutputEvidence<'a>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Primary,
    Control,
    Interpretation,
    Runtime,
    ClaimedOutput,
    Evidence,
}
pub struct JobContext {
    selection: Selection,
    roles: BTreeMap<Role, BTreeSet<Object>>,
    objects: BTreeSet<Object>,
    logical_bytes: u64,
}
impl JobContext {
    pub fn selection(&self) -> &Selection {
        &self.selection
    }
    pub fn objects(&self) -> &BTreeSet<Object> {
        &self.objects
    }
    pub fn of(&self, role: Role) -> &BTreeSet<Object> {
        &self.roles[&role]
    }
    /// Deduplicated declared payload/reference lengths, not disk allocation.
    pub fn logical_bytes(&self) -> u64 {
        self.logical_bytes
    }
    pub fn complete_claim_coverage(&self) -> bool {
        false
    }
}
fn path(value: &str) -> Result<LogicalPath> {
    LogicalPath::parse(value).map_err(|_| Error::Invalid)
}
struct Prepared {
    header: Vec<u8>,
    entries: BTreeMap<LogicalPath, Object>,
    roles: BTreeMap<Role, BTreeSet<Object>>,
}
fn prepare(
    admission: &Admission<'_>,
    variants: Option<&super::variants::Packet>,
    limits: TreeLimits,
) -> Result<Prepared> {
    let context = admission.context;
    let plan = admission.plan;
    let required = context.requirements();
    if variants.is_some_and(|packet| packet.source() != context.selection()) {
        return Err(Error::Integrity);
    }
    if admission.runtime.len() > 64
        || admission
            .output
            .as_ref()
            .is_some_and(|o| o.attachments.len() > 64)
    {
        return Err(Error::Budget);
    }
    for names in [
        Some(admission.runtime),
        admission.output.as_ref().map(|o| o.attachments),
    ]
    .into_iter()
    .flatten()
    {
        for name in names.keys() {
            if names.keys().any(|other| {
                name != other && name.as_str().starts_with(&format!("{}/", other.as_str()))
            }) {
                return Err(Error::Invalid);
            }
        }
    }
    if plan.checkpoint() != required.checkpoint()
        || plan.binding() != required.binding_digest()
        || plan.admission() != required.admission()
        || plan.operation().parameters != Object::from_bytes(context.descriptor())
    {
        return Err(Error::Invalid);
    }
    if let Some(output) = &admission.output {
        output
            .claim
            .check_plan(plan)
            .map_err(|_| Error::Integrity)?;
        if output.claim.context().root != context.selection().root
            || output.claim.context().identity != context.selection().context
        {
            return Err(Error::Integrity);
        }
    }
    let mut header = serde_json::json!({
        "schema":"tig.native-job-context.experimental-1",
        "source":context.selection(),"checkpoint":plan.checkpoint(),
        "admission":plan.admission(),"binding":plan.binding(),
        "consumption":plan.consumption(),"recipe":plan.fingerprint(),
        "operation":plan.operation(),"runtime":admission.runtime.len(),
        "claim":admission.output.as_ref().map(|o|o.claim.object()),
        "evidence":admission.output.as_ref().map_or(0,|o|o.attachments.len()),
    });
    if let Some(packet) = variants {
        header["schema"] = "tig.native-job-context.experimental-2".into();
        header["variants"] =
            serde_json::to_value(packet.selection()).map_err(|_| Error::Invalid)?;
    }
    let header = identity::canonical_json(&header).map_err(|_| Error::Invalid)?;
    let header_object = Object::from_bytes(&header);
    let mut entries = BTreeMap::from([
        (path("job/identity")?, header_object.clone()),
        (path("source/context")?, context.selection().root.clone()),
        (
            path("operation/producer")?,
            plan.operation().producer.clone(),
        ),
        (
            path("operation/parameters")?,
            plan.operation().parameters.clone(),
        ),
    ]);
    for (name, object) in admission.runtime {
        entries.insert(path(&format!("runtime/{}", name.as_str()))?, object.clone());
    }
    let mut roles = BTreeMap::from([
        (Role::Primary, required.primary().clone()),
        (Role::Control, required.controls().clone()),
        (Role::Interpretation, required.metadata().clone()),
        (
            Role::Runtime,
            BTreeSet::from([plan.operation().producer.clone()]),
        ),
        (Role::ClaimedOutput, BTreeSet::new()),
        (Role::Evidence, BTreeSet::new()),
    ]);
    roles.get_mut(&Role::Interpretation).unwrap().extend([
        header_object,
        context.selection().root.clone(),
        context.archive().clone(),
        context.policy().clone(),
        plan.operation().parameters.clone(),
    ]);
    if let Some(packet) = variants {
        entries.insert(path("source/variants")?, packet.selection().root.clone());
        roles
            .get_mut(&Role::Interpretation)
            .unwrap()
            .extend(packet.objects().iter().cloned());
    }
    roles
        .get_mut(&Role::Runtime)
        .unwrap()
        .extend(admission.runtime.values().cloned());
    if let Some(output) = &admission.output {
        entries.insert(path("claim/record")?, output.claim.object().clone());
        roles
            .get_mut(&Role::Interpretation)
            .unwrap()
            .insert(output.claim.object().clone());
        for subject in output.claim.outputs() {
            roles
                .get_mut(&Role::ClaimedOutput)
                .unwrap()
                .insert(subject.object().clone());
            roles
                .get_mut(&Role::Interpretation)
                .unwrap()
                .insert(subject.definition().object.clone());
        }
        for (name, object) in output.attachments {
            entries.insert(
                path(&format!("evidence/{}", name.as_str()))?,
                object.clone(),
            );
            roles
                .get_mut(&Role::Evidence)
                .unwrap()
                .insert(object.clone());
        }
    }
    if entries.len() > limits.max_entries {
        return Err(Error::Budget);
    }
    // Cross-role aliases are allowed; contradictory lengths and unsafe numbers
    // are not. Check before any metadata write or provider read.
    let mut sizes = BTreeMap::new();
    for object in roles.values().flatten() {
        if object.bytes > 9_007_199_254_740_991
            || sizes
                .insert(object.sha256.clone(), object.bytes)
                .is_some_and(|n| n != object.bytes)
        {
            return Err(Error::Integrity);
        }
    }
    sizes
        .values()
        .try_fold(65_536u64, |sum, n| sum.checked_add(*n).ok_or(Error::Budget))?;
    Ok(Prepared {
        header,
        entries,
        roles,
    })
}
fn result(prepared: Prepared, selection: Selection) -> Result<JobContext> {
    let mut roles = prepared.roles;
    roles
        .get_mut(&Role::Interpretation)
        .unwrap()
        .insert(selection.root.clone());
    let objects: BTreeSet<_> = roles.values().flatten().cloned().collect();
    let logical_bytes = objects
        .iter()
        .try_fold(0u64, |sum, o| sum.checked_add(o.bytes).ok_or(Error::Budget))?;
    Ok(JobContext {
        selection,
        roles,
        objects,
        logical_bytes,
    })
}
/// Candidate metadata only. Caller must finish/reopen/readback before adoption.
pub fn freeze(
    admission: &Admission<'_>,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<JobContext> {
    freeze_inner(admission, None, store, limits)
}
/// Attach an independently checked variant packet to this exact job source.
/// Candidate finish/readback and owner adoption remain external.
pub fn freeze_with_variants(
    admission: &Admission<'_>,
    variants: &super::variants::Packet,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<JobContext> {
    freeze_inner(admission, Some(variants), store, limits)
}
fn freeze_inner(
    admission: &Admission<'_>,
    variants: Option<&super::variants::Packet>,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<JobContext> {
    let prepared = prepare(admission, variants, limits)?;
    let header = put(store, &prepared.header)?;
    if let Some(output) = &admission.output {
        put(store, output.claim.bytes())?;
    }
    let tree = Tree::build(&prepared.entries, store, limits)
        .map_err(tree_error)?
        .0;
    result(
        prepared,
        Selection {
            root: tree.root().clone(),
            context: header.sha256,
        },
    )
}
/// Exact external admission plus retained metadata; never writes or discovers.
pub fn restore(
    selection: &Selection,
    admission: &Admission<'_>,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<JobContext> {
    restore_inner(selection, admission, None, store, limits)
}
/// Restore only with an already replay-checked packet and exact job admission.
pub fn restore_with_variants(
    selection: &Selection,
    admission: &Admission<'_>,
    variants: &super::variants::Packet,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<JobContext> {
    restore_inner(selection, admission, Some(variants), store, limits)
}
fn restore_inner(
    selection: &Selection,
    admission: &Admission<'_>,
    variants: Option<&super::variants::Packet>,
    store: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<JobContext> {
    let prepared = prepare(admission, variants, limits)?;
    let header = Object::from_bytes(&prepared.header);
    if selection.context != header.sha256 {
        return Err(Error::Integrity);
    }
    let (entries, _) = Tree::from_root(selection.root.clone())
        .entries(store, limits)
        .map_err(tree_error)?;
    if entries != prepared.entries
        || read(store, &header, prepared.header.len())? != prepared.header
    {
        return Err(Error::Integrity);
    }
    if let Some(output) = &admission.output
        && read(store, output.claim.object(), output.claim.bytes().len())? != output.claim.bytes()
    {
        return Err(Error::Integrity);
    }
    result(prepared, selection.clone())
}
