//! Read-only original-context/recipe replay of an unsigned produced claim.
use super::{AssetSnapshot, Error, Limits, Result, context::ProductionContext};
use std::collections::{BTreeMap, BTreeSet};
use tig_assets::graph::{
    self,
    recipe::{
        self, Consumption, Operation,
        produced::{Claim, PlanCheckedClaim},
    },
};
use tig_core::{
    Object,
    tree::{NodeStore, TreeLimits},
};
use tig_store::source_claims::Selection;

/// All policy/producer/parameter authority and resource bounds come from the caller.
pub struct Admission<'a> {
    pub profiles: &'a [graph::Profile],
    pub consumption: &'a [Consumption],
    pub operation: &'a Operation,
    pub native: Limits,
    pub tree: TreeLimits,
    pub query: graph::Limits,
    pub recipe: recipe::Limits,
}
/// Metadata replay only, never producer execution or output-byte verification.
pub struct ReplayedClaim<'a> {
    context: ProductionContext,
    checked: PlanCheckedClaim<'a>,
}
impl ReplayedClaim<'_> {
    pub fn context(&self) -> &ProductionContext {
        &self.context
    }
    pub fn checked(&self) -> &PlanCheckedClaim<'_> {
        &self.checked
    }
}

pub fn replay<'a>(
    claim: &'a Claim,
    admission: &Admission<'_>,
    store: &mut impl NodeStore,
) -> Result<(AssetSnapshot, ReplayedClaim<'a>)> {
    if claim.operation() != admission.operation {
        return Err(Error::Invalid);
    }
    let pin = claim.context();
    let selection = Selection {
        root: pin.root.clone(),
        context: pin.identity.clone(),
    };
    let (mut source, context) = AssetSnapshot::from_production_context(
        &selection,
        admission.profiles,
        store,
        admission.native,
        admission.tree,
        admission.query,
    )?;
    if Object::from_bytes(context.descriptor()) != admission.operation.parameters {
        return Err(Error::Integrity);
    }
    // Bounded by claim output count, not a second full input inventory.
    let outputs: BTreeMap<_, _> = claim
        .outputs()
        .iter()
        .flat_map(|subject| {
            [
                (&subject.object().sha256, subject.object().bytes),
                (
                    &subject.definition().object.sha256,
                    subject.definition().object.bytes,
                ),
            ]
        })
        .collect();
    let required = context.requirements();
    for known in required
        .primary()
        .iter()
        .chain(required.controls())
        .chain(required.metadata())
    {
        if outputs
            .get(&known.sha256)
            .is_some_and(|bytes| *bytes != known.bytes)
        {
            return Err(Error::Integrity);
        }
    }
    let query = graph::Query {
        checkpoint: context.requirements().checkpoint().clone(),
        roots: context.roots().clone(),
        kinds: BTreeSet::from([graph::EdgeKind::Production]),
        limits: admission.query,
    };
    let graph =
        graph::resolve(&mut source, &query, admission.profiles).map_err(|_| Error::Invalid)?;
    let plan = recipe::plan(
        &graph,
        &mut source,
        admission.profiles,
        admission.consumption,
        admission.operation,
        admission.recipe,
    )
    .map_err(|_| Error::Invalid)?;
    let checked = claim.check_plan(&plan).map_err(|_| Error::Integrity)?;
    Ok((source, ReplayedClaim { context, checked }))
}
