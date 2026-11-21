use super::*;
use crate::source_claims::admission::{Work as ProtectionWork, bulk as projection};

/// Persist a bounded metadata request. Caller separately finishes/readbacks the
/// metadata provider; this supplies neither source nor retirement authorization.
pub fn request(
    selections: Vec<Selection>,
    paging: Paging,
    nodes: &mut impl NodeStore,
    limits: &Limits,
) -> Result<Object> {
    limits.validate()?;
    paging.validate()?;
    if selections.is_empty() || selections.len() > limits.selections {
        return Err(Error::Budget);
    }
    let mut work = Work::new(nodes, limits)?;
    let mut roots = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for s in selections {
        if !seen.insert((
            s.namespace.clone(),
            s.source.as_str().to_owned(),
            s.predecessor_revision,
        )) {
            return Err(Error::Invalid);
        }
        let id = s.id()?;
        let object = work.doc(&s)?;
        roots.insert(path(id.as_str())?, object);
    }
    let roots = work.tree(&roots, limits.selections)?;
    work.doc(&Request {
        schema: REQUEST_SCHEMA.into(),
        paging,
        roots,
        count: seen.len(),
    })
}
pub(super) fn input(
    work: &mut Work<'_, impl NodeStore>,
    root: &Object,
) -> Result<(Request, Vec<Selection>)> {
    let r: Request = work.read(root)?;
    r.paging.validate()?;
    if r.schema != REQUEST_SCHEMA || r.count == 0 || r.count > work.limits.selections {
        return Err(Error::Invalid);
    }
    let roots = work.entries(&r.roots, work.limits.selections)?;
    if roots.len() != r.count {
        return Err(Error::Invalid);
    }
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for (p, o) in roots {
        let s: Selection = work.read(&o)?;
        if p.as_str() != s.id()?.as_str()
            || !seen.insert((
                s.namespace.clone(),
                s.source.as_str().to_owned(),
                s.predecessor_revision,
            ))
        {
            return Err(Error::Invalid);
        }
        result.push(s);
    }
    Ok((r, result))
}
/// Read a canonical indexed request without staging nodes or observing sources.
/// Binding membership/current protection and authority remain separate checks.
pub fn read_request(
    root: &Object,
    nodes: &mut impl NodeStore,
    limits: &Limits,
) -> Result<(Paging, Vec<Selection>)> {
    let mut work = Work::new(nodes, limits)?;
    let (header, selections) = input(&mut work, root)?;
    work.timed()?;
    Ok((header.paging, selections))
}
/// Freeze the whole exact historical selection and its hypothetical remaining
/// protection. No source/provider effect, custody claim or consent is produced.
pub fn build(
    e: &Enrollment,
    request: &Object,
    roster: &claims::Roster,
    namespaces: &mut [Namespace],
    nodes: &mut impl NodeStore,
    limits: &Limits,
) -> Result<Object> {
    e.id()?;
    roster.id()?;
    limits.validate()?;
    if roster.coverage.is_none()
        || roster.scope()? != e.scope
        || namespaces.len() != roster.members.len()
    {
        return Err(Error::Authority);
    }
    let mut work = Work::new(nodes, limits)?;
    let (input, selections) = input(&mut work, request)?;
    let mut basis = roster.clone();
    let mut order = Vec::new();
    let mut seen = BTreeSet::new();
    for (i, pin) in roster.members.iter().enumerate() {
        let at = namespaces
            .iter()
            .position(|n| n.identity() == &pin.namespace)
            .ok_or(Error::Authority)?;
        if !seen.insert(at) {
            return Err(Error::Authority);
        }
        namespaces[at].refresh_minimum(&pin.minimum)?;
        projection::snapshot(&mut namespaces[at])?;
        let current = claims::NamespacePin::current(&namespaces[at])?;
        if current.policy != pin.policy {
            return Err(Error::Authority);
        }
        basis.members[i] = current;
        order.push(at);
    }
    let mut cohorts = BTreeMap::new();
    let mut selected = BTreeMap::new();
    let mut exclusions = BTreeMap::<Digest, Vec<projection::Exclusion>>::new();
    let mut bindings = 0usize;
    for s in selections {
        work.timed()?;
        let at = namespaces
            .iter()
            .position(|n| n.identity() == &s.namespace)
            .ok_or(Error::Authority)?;
        let source = namespaces[at].source(&s.source)?.ok_or(Error::Missing)?;
        let class = source.kind.clone().ok_or(Error::Authority)?;
        if source.owner != e.owner || !e.classes.contains(&class) || source.pending.is_some() {
            return Err(Error::Authority);
        }
        let original = source
            .predecessors
            .iter()
            .find(|v| v.revision == s.predecessor_revision)
            .ok_or(Error::Missing)?
            .selection
            .clone();
        let chosen = work.entries(&s.bindings, limits.bindings)?;
        if chosen.is_empty() {
            return Err(Error::Invalid);
        }
        bindings = bindings.checked_add(chosen.len()).ok_or(Error::Budget)?;
        if bindings > limits.bindings {
            return Err(Error::Budget);
        }
        let actual = work.entries(&original.root, limits.source.bindings)?;
        for (p, o) in &chosen {
            if actual.get(p) != Some(o) {
                return Err(Error::Invalid);
            }
            if selected
                .insert(o.sha256.clone(), o.clone())
                .is_some_and(|old| old != *o)
            {
                return Err(Error::Invalid);
            }
            if selected.len() > limits.objects {
                return Err(Error::Budget);
            }
        }
        let pin = basis
            .members
            .iter()
            .find(|m| m.namespace == s.namespace)
            .ok_or(Error::Authority)?;
        let origin = Origin {
            namespace: s.namespace.clone(),
            source: s.source.clone(),
            predecessor_revision: s.predecessor_revision,
            original: original.clone(),
            owner: source.owner,
            class,
            policy: pin.policy.clone(),
        };
        let cohort = Cohort {
            origin,
            bindings: s.bindings,
            count: chosen.len(),
        };
        let id = cohort.origin.id()?;
        cohorts.insert(path(id.as_str())?, work.doc(&cohort)?);
        exclusions
            .entry(s.namespace)
            .or_default()
            .push(projection::Exclusion {
                source: s.source,
                owner: e.owner.clone(),
                predecessor: s.predecessor_revision,
                original,
                paths: chosen.into_keys().collect(),
            });
    }
    let mut held = BTreeSet::new();
    {
        let admitted = &roster.limits;
        let local = &limits.source;
        let projection_limits = protection::Limits {
            millis: admitted.millis.min(local.millis),
            roots: admitted.roots.min(local.roots),
            bindings: admitted.bindings.min(local.bindings),
            associations: admitted.associations.min(local.associations),
            reads: admitted.reads.min(local.reads),
            read_bytes: admitted.read_bytes.min(local.read_bytes),
        };
        // The outer metadata meter also covers every projection get. Inner
        // source counters count complete closure, including excluded bindings.
        let mut projected = ProtectionWork::new(&work, &projection_limits, work.start)?;
        for (pin, at) in basis.members.iter().zip(&order) {
            let excluded = exclusions
                .get(&pin.namespace)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let (checkpoint, remaining) = projection::remaining_selected(
                &mut namespaces[*at],
                excluded,
                &selected,
                &mut projected,
            )?;
            if checkpoint != pin.minimum {
                return Err(Error::Stale);
            }
            held.extend(remaining);
        }
    }
    for (pin, at) in basis.members.iter().zip(&order) {
        if projection::snapshot(&mut namespaces[*at])? != pin.minimum {
            return Err(Error::Stale);
        }
    }
    let mut subjects = BTreeMap::new();
    let mut unique_bytes = 0u64;
    for object in selected.values() {
        unique_bytes = unique_bytes
            .checked_add(object.bytes)
            .filter(|n| *n <= MAX_NUMBER)
            .ok_or(Error::Budget)?;
        let subject = Subject {
            object: object.clone(),
            held_by_surviving_claims: held.contains(&object.sha256),
        };
        subjects.insert(path(object.sha256.as_str())?, work.doc(&subject)?);
    }
    let summary = Summary {
        roots: cohorts.len(),
        bindings,
        unique_objects: subjects.len(),
        held_objects: held.len(),
        unique_bytes,
    };
    let cohorts = work.tree(&cohorts, limits.selections)?;
    let subjects = work.tree(&subjects, limits.objects)?;
    let result = work.doc(&Header {
        schema: MANIFEST_SCHEMA.into(),
        enrollment: e.id()?,
        request: request.clone(),
        basis,
        cohorts,
        subjects,
        summary,
        paging: input.paging,
    })?;
    // Includes all metadata publication work; later source changes still make
    // this a historical candidate, never a current removal lease.
    for (pin, at) in roster.members.iter().zip(&order) {
        let recorded: Header = work.read(&result)?;
        let expected = recorded
            .basis
            .members
            .iter()
            .find(|m| m.namespace == pin.namespace)
            .ok_or(Error::Authority)?;
        if projection::snapshot(&mut namespaces[*at])? != expected.minimum {
            return Err(Error::Stale);
        }
    }
    work.timed()?;
    Ok(result)
}
