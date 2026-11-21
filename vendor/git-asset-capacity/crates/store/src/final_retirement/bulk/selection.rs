use super::*;
use crate::{
    local::LocalStore,
    protection::BudgetNodes,
    source_claims::admission::{Work, bulk as projected},
};
use git_asset_core::tree::NodeStore;
use std::time::Instant;

/// Read-only exact selection. No provider/source mutation or removal grant.
pub fn plan(
    e: &Enrollment,
    request: Request,
    roster: &claims::Roster,
    namespaces: &mut [Namespace],
    nodes: &impl NodeStore,
    stores: &[&LocalStore],
) -> Result<Plan> {
    e.id()?;
    request.check(e)?;
    roster.id()?;
    if roster.coverage.is_none()
        || roster.scope()? != e.scope
        || namespaces.len() != roster.members.len()
        || stores.len() > 4
    {
        return Err(Error::Authority);
    }
    let mut order = Vec::new();
    let mut observed = roster.clone();
    let mut seen = BTreeSet::new();
    for (index, pin) in roster.members.iter().enumerate() {
        let at = namespaces
            .iter()
            .position(|n| n.identity() == &pin.namespace)
            .ok_or(Error::Authority)?;
        if !seen.insert(at) {
            return Err(Error::Authority);
        }
        namespaces[at].refresh_minimum(&pin.minimum)?;
        projected::snapshot(&mut namespaces[at])?;
        let current = claims::NamespacePin::current(&namespaces[at])?;
        if current.policy != pin.policy {
            return Err(Error::Authority);
        }
        observed.members[index] = current;
        order.push(at);
    }
    let mut store_ids = BTreeSet::new();
    for store in stores {
        if !store_ids.insert(store.identity()) {
            return Err(Error::Authority);
        }
    }
    let start = Instant::now();
    let budget = BudgetNodes::new(nodes, &roster.limits, start);
    let mut bindings = Vec::new();
    let mut candidates = BTreeMap::new();
    let mut exclusions = BTreeMap::<Digest, Vec<projected::Exclusion>>::new();
    for selected in &request.selections {
        let at = namespaces
            .iter()
            .position(|n| n.identity() == &selected.namespace)
            .ok_or(Error::Authority)?;
        let source = e.source(&selected.namespace)?;
        let pin = observed
            .members
            .iter()
            .find(|m| m.namespace == selected.namespace)
            .ok_or(Error::Authority)?;
        if source.policy != pin.policy {
            return Err(Error::Authority);
        }
        let subset = subset::plan(
            source,
            &mut namespaces[at],
            selected.request.clone(),
            &budget,
        )?;
        for (path, object) in subset.selected_bindings(source, &mut namespaces[at], &budget)? {
            if let Some(previous) = candidates.insert(object.sha256.clone(), object.clone())
                && previous != object
            {
                return Err(Error::Invalid);
            }
            bindings.push(Binding {
                namespace: selected.namespace.clone(),
                source: selected.request.source.clone(),
                predecessor: selected.request.predecessor_revision,
                original: subset.original().clone(),
                path,
                object,
            });
        }
        if candidates.len() > MAX_OBJECTS {
            return Err(Error::Budget);
        }
        exclusions
            .entry(selected.namespace.clone())
            .or_default()
            .push(projected::Exclusion {
                source: selected.request.source.clone(),
                owner: e.owner.clone(),
                predecessor: selected.request.predecessor_revision,
                original: subset.original().clone(),
                paths: selected.request.paths.iter().cloned().collect(),
            });
    }
    for copy in &request.copies {
        if candidates.get(&copy.publication.object.sha256) != Some(&copy.publication.object) {
            return Err(Error::Invalid);
        }
    }
    let mut work = Work::new(&budget, &roster.limits, start)?;
    let mut protected = BTreeSet::new();
    for (pin, at) in observed.members.iter().zip(&order) {
        let excluded = exclusions
            .get(&pin.namespace)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let (checkpoint, remaining) =
            projected::remaining(&mut namespaces[*at], excluded, &candidates, &mut work)?;
        if checkpoint != pin.minimum {
            return Err(Error::Stale);
        }
        protected.extend(remaining);
    }
    let mut subjects = Vec::new();
    let mut actions = Vec::new();
    for object in candidates.values() {
        let mut hold = protected
            .contains(&object.sha256)
            .then_some(Hold::Protected);
        let copies = request
            .copies
            .iter()
            .filter(|c| c.publication.object == *object)
            .collect::<Vec<_>>();
        if copies.is_empty() {
            hold = Some(Hold::NoCopyScope);
        }
        let mut object_actions = Vec::new();
        if hold.is_none() {
            for copy in copies {
                let Some(store) = stores.iter().find(|s| s.identity() == &copy.store) else {
                    hold = Some(Hold::ProviderUnavailable);
                    break;
                };
                match provider::plan(store, action_request(e, &request, copy)?) {
                    Ok(plan) if plan.policy.scope == e.scope && plan.policy.clock == e.clock => {
                        object_actions.push(plan)
                    }
                    Ok(_)
                    | Err(
                        provider::Error::ProtectedGrant
                        | provider::Error::Invalid
                        | provider::Error::Unsupported,
                    ) => {
                        hold = Some(Hold::ProviderProtection);
                        break;
                    }
                    Err(_) => {
                        hold = Some(Hold::ProviderUnavailable);
                        break;
                    }
                }
            }
        }
        if hold.is_none() {
            actions.extend(object_actions);
        }
        subjects.push(Subject {
            object: object.clone(),
            hold,
        });
    }
    actions.sort_by(|a, b| {
        a.enrollment
            .store()
            .cmp(b.enrollment.store())
            .then_with(|| {
                a.request()
                    .publication
                    .object
                    .sha256
                    .cmp(&b.request().publication.object.sha256)
            })
    });
    let eligible = subjects
        .iter()
        .filter(|s| s.hold.is_none())
        .map(|s| &s.object.sha256)
        .collect::<BTreeSet<_>>();
    let mut settlements = Vec::new();
    for profile in &e.sources {
        let requests = request
            .selections
            .iter()
            .filter(|s| s.namespace == profile.namespace)
            .filter_map(|s| {
                let mut request = s.request.clone();
                request.paths.retain(|path| {
                    bindings.iter().any(|b| {
                        b.namespace == s.namespace
                            && b.source == s.request.source
                            && b.predecessor == s.request.predecessor_revision
                            && &b.path == path
                            && eligible.contains(&b.object.sha256)
                    })
                });
                (!request.paths.is_empty()).then_some(request)
            })
            .collect::<Vec<_>>();
        if !requests.is_empty() {
            let at = namespaces
                .iter()
                .position(|n| n.identity() == &profile.namespace)
                .ok_or(Error::Authority)?;
            for plan in subset::plan_sequence(profile, &mut namespaces[at], requests, &budget)? {
                settlements.push(Settlement {
                    namespace: profile.namespace.clone(),
                    plan,
                });
            }
        }
    }
    for (pin, at) in observed.members.iter().zip(&order) {
        if projected::snapshot(&mut namespaces[*at])? != pin.minimum {
            return Err(Error::Stale);
        }
    }
    work.timed()?;
    let plan = Plan {
        schema: "tig.bulk-final-plan.design-1".into(),
        enrollment: e.clone(),
        request,
        roster: observed,
        bindings,
        subjects,
        settlements,
        actions,
    };
    plan.id()?;
    Ok(plan)
}
