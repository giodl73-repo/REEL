use super::*;

fn header(e: &Enrollment, manifest: &Object, work: &Work<'_, impl NodeStore>) -> Result<Header> {
    e.id()?;
    let h: Header = work.read(manifest)?;
    h.paging.validate()?;
    h.basis.id()?;
    if h.schema != MANIFEST_SCHEMA
        || h.enrollment != e.id()?
        || h.basis.coverage.is_none()
        || h.basis.scope()? != e.scope
        || h.summary.roots == 0
        || h.summary.roots > work.limits.selections
        || h.summary.bindings == 0
        || h.summary.bindings > work.limits.bindings
        || h.summary.unique_objects == 0
        || h.summary.unique_objects > work.limits.objects
        || h.summary.held_objects > h.summary.unique_objects
        || h.summary.unique_bytes > MAX_NUMBER
    {
        return Err(Error::Authority);
    }
    let request: Request = work.read(&h.request)?;
    if request.schema != REQUEST_SCHEMA
        || request.count != h.summary.roots
        || request.paging != h.paging
    {
        return Err(Error::Invalid);
    }
    Ok(h)
}
fn cohort(
    e: &Enrollment,
    h: &Header,
    id: &LogicalPath,
    o: &Object,
    work: &mut Work<'_, impl NodeStore>,
) -> Result<Cohort> {
    let c: Cohort = work.read(o)?;
    if c.origin.id()?.as_str() != id.as_str()
        || c.origin.owner != e.owner
        || !e.classes.contains(&c.origin.class)
        || c.origin.predecessor_revision == 0
        || c.origin.predecessor_revision > MAX_NUMBER
        || c.count == 0
        || c.count > work.limits.bindings
        || !h
            .basis
            .members
            .iter()
            .any(|p| p.namespace == c.origin.namespace && p.policy == c.origin.policy)
    {
        return Err(Error::Authority);
    }
    let request: Request = work.read(&h.request)?;
    let expected = Selection {
        namespace: c.origin.namespace.clone(),
        source: c.origin.source.clone(),
        predecessor_revision: c.origin.predecessor_revision,
        bindings: c.bindings.clone(),
    };
    let key = path(expected.id()?.as_str())?;
    let limits = work.limits.tree(work.limits.selections);
    let object = Tree::from_root(request.roots)
        .get(&key, work, limits)?
        .0
        .ok_or(Error::Invalid)?;
    if work.read::<Selection>(&object)? != expected {
        return Err(Error::Invalid);
    }
    Ok(c)
}
fn chosen(
    c: &Cohort,
    work: &mut Work<'_, impl NodeStore>,
) -> Result<BTreeMap<LogicalPath, Object>> {
    let picked = work.entries(&c.bindings, work.limits.bindings)?;
    if picked.len() != c.count {
        return Err(Error::Invalid);
    }
    let original = work.entries(&c.origin.original.root, work.limits.source.bindings)?;
    if picked.iter().any(|(p, o)| original.get(p) != Some(o)) {
        return Err(Error::Invalid);
    }
    Ok(picked)
}
/// Self-consistent historical inventory under the admitted manifest identity.
/// Protection bits are observations, never authentication or a live lease.
pub fn inspect(
    e: &Enrollment,
    manifest: &Object,
    nodes: &mut impl NodeStore,
    limits: &Limits,
) -> Result<Inventory> {
    let mut work = Work::new(nodes, limits)?;
    let h = header(e, manifest, &work)?;
    let (request, selections) = compile::input(&mut work, &h.request)?;
    if h.paging != request.paging || h.summary.roots != selections.len() {
        return Err(Error::Invalid);
    }
    let index = work.entries(&h.cohorts, limits.selections)?;
    if index.len() != h.summary.roots {
        return Err(Error::Invalid);
    }
    let mut remaining: BTreeMap<_, _> = selections
        .into_iter()
        .map(|s| {
            (
                (
                    s.namespace.clone(),
                    s.source.as_str().to_owned(),
                    s.predecessor_revision,
                ),
                s,
            )
        })
        .collect();
    let mut cohorts = BTreeMap::new();
    let mut candidates = BTreeMap::new();
    let mut bindings = 0usize;
    for (id, o) in index {
        let c = cohort(e, &h, &id, &o, &mut work)?;
        let expected = remaining
            .remove(&(
                c.origin.namespace.clone(),
                c.origin.source.as_str().to_owned(),
                c.origin.predecessor_revision,
            ))
            .ok_or(Error::Invalid)?;
        if c.bindings != expected.bindings {
            return Err(Error::Invalid);
        }
        let entries = chosen(&c, &mut work)?;
        bindings = bindings.checked_add(entries.len()).ok_or(Error::Budget)?;
        if bindings > limits.bindings {
            return Err(Error::Budget);
        }
        for o in entries.into_values() {
            if candidates
                .insert(o.sha256.clone(), o.clone())
                .is_some_and(|old| old != o)
            {
                return Err(Error::Invalid);
            }
            if candidates.len() > limits.objects {
                return Err(Error::Budget);
            }
        }
        cohorts.insert(c.origin.id()?, c);
    }
    if !remaining.is_empty() {
        return Err(Error::Invalid);
    }
    let mut subjects = BTreeMap::new();
    let mut held = 0usize;
    let mut bytes = 0u64;
    for (p, o) in work.entries(&h.subjects, limits.objects)? {
        let s: Subject = work.read(&o)?;
        if p.as_str() != s.object.sha256.as_str()
            || candidates.get(&s.object.sha256) != Some(&s.object)
        {
            return Err(Error::Invalid);
        }
        held += usize::from(s.held_by_surviving_claims);
        bytes = bytes
            .checked_add(s.object.bytes)
            .filter(|n| *n <= MAX_NUMBER)
            .ok_or(Error::Budget)?;
        subjects.insert(s.object.sha256.clone(), s);
    }
    let actual = Summary {
        roots: cohorts.len(),
        bindings,
        unique_objects: subjects.len(),
        held_objects: held,
        unique_bytes: bytes,
    };
    if subjects.len() != candidates.len() || actual != h.summary {
        return Err(Error::Invalid);
    }
    work.timed()?;
    Ok(Inventory {
        header: h,
        cohorts,
        subjects,
    })
}

/// Read one deterministic original-binding page. It does not inspect current
/// namespace/provider state, advance execution or authorize applying a subset.
pub fn page(
    e: &Enrollment,
    manifest: &Object,
    cursor: Option<&Cursor>,
    nodes: &mut impl NodeStore,
    limits: &Limits,
) -> Result<Option<Page>> {
    let mut work = Work::new(nodes, limits)?;
    let h = header(e, manifest, &work)?;
    if cursor.is_some_and(|c| c.manifest != manifest.sha256) {
        return Err(Error::Invalid);
    }
    let index = work.entries(&h.cohorts, limits.selections)?;
    if index.len() != h.summary.roots {
        return Err(Error::Invalid);
    }
    let mut position = None;
    if let Some(c) = cursor {
        let key = path(c.occurrence.as_str())?;
        let o = index.get(&key).ok_or(Error::Invalid)?;
        let cohort = cohort(e, &h, &key, o, &mut work)?;
        let entries = chosen(&cohort, &mut work)?;
        if !entries.contains_key(&c.after) {
            return Err(Error::Invalid);
        }
        if entries.keys().any(|p| p > &c.after) {
            position = Some((key, cohort, entries, Some(c.after.clone())))
        }
    }
    if position.is_none() {
        let next = index
            .iter()
            .find(|(p, _)| cursor.is_none_or(|c| p.as_str() > c.occurrence.as_str()));
        let Some((key, o)) = next else {
            return Ok(None);
        };
        let cohort = cohort(e, &h, key, o, &mut work)?;
        let entries = chosen(&cohort, &mut work)?;
        position = Some((key.clone(), cohort, entries, None));
    }
    let (key, cohort, entries, after) = position.ok_or(Error::Invalid)?;
    let mut result = Page {
        schema: PAGE_SCHEMA,
        manifest: manifest.clone(),
        cohort,
        bindings: Vec::new(),
        next: None,
        historical_only: true,
        authorizes_claim_release: false,
        authorizes_removal: false,
    };
    let occurrence = result.cohort.origin.id()?;
    for (p, o) in &entries {
        if after.as_ref().is_some_and(|a| p <= a) {
            continue;
        }
        work.timed()?;
        if result.bindings.len() >= h.paging.max_bindings {
            break;
        }
        let binding = Binding {
            path: p.clone(),
            object: o.clone(),
        };
        result.bindings.push(binding);
        let more = entries.keys().next_back() != Some(p) || index.keys().next_back() != Some(&key);
        result.next = more.then(|| Cursor {
            manifest: manifest.sha256.clone(),
            occurrence: occurrence.clone(),
            after: p.clone(),
        });
        // Enforce complete canonical response bytes, not just path count. The
        // previous page boundary is recovered below if this item does not fit.
        if encode(&result)?.len() > h.paging.max_bytes {
            result.bindings.pop();
            let last = result.bindings.last().ok_or(Error::Budget)?;
            result.next = Some(Cursor {
                manifest: manifest.sha256.clone(),
                occurrence: occurrence.clone(),
                after: last.path.clone(),
            });
            break;
        }
    }
    if result.bindings.is_empty() {
        return Err(Error::Invalid);
    }
    work.timed()?;
    Ok(Some(result))
}
