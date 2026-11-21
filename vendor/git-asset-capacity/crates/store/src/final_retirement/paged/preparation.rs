//! Asset-side frozen conditional source plans. Predictions confer no authority
//! and are never evidence that source transitions have actually executed.
use super::*;
use crate::source_claims::subset;

const SCHEMA: &str = "tig.paged-preparation.design-1";
const STEP_SCHEMA: &str = "tig.paged-preparation-step.design-1";
const MAX_PAGES: usize = 4096;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    schema: String,
    pub enrollment: Digest,
    pub manifest: Object,
    pub basis: claims::Roster,
    pub sources: Vec<subset::Enrollment>,
    pub steps: Object,
    pub count: usize,
    pub bindings: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    schema: String,
    pub index: usize,
    pub origin: Origin,
    pub bindings: Vec<Binding>,
    pub plan: Object,
    pub retirement_checkpoint: source_claims::Checkpoint,
}
fn slot(i: usize) -> Result<LogicalPath> {
    path(&format!("{i:08}"))
}
fn profiles(e: &Enrollment, sources: &[subset::Enrollment]) -> Result<()> {
    e.id()?;
    if sources.is_empty()
        || sources.len() > 16
        || sources.windows(2).any(|s| s[0].namespace >= s[1].namespace)
    {
        return Err(Error::Authority);
    }
    for s in sources {
        s.id()?;
        if s.owner != e.owner
            || s.authority != e.authority
            || s.classes.iter().any(|c| !e.classes.contains(c))
        {
            return Err(Error::Authority);
        }
    }
    Ok(())
}
fn unchanged(basis: &claims::Roster, namespaces: &mut [Namespace]) -> Result<()> {
    if basis.members.len() != namespaces.len() {
        return Err(Error::Authority);
    }
    for pin in &basis.members {
        let n = namespaces
            .iter_mut()
            .find(|n| n.identity() == &pin.namespace)
            .ok_or(Error::Authority)?;
        n.refresh_minimum(&pin.minimum)?;
        admission::bulk::snapshot(n)?;
        if claims::NamespacePin::current(n)? != *pin {
            return Err(Error::Stale);
        }
    }
    Ok(())
}
/// Reconstruct the manifest against independently current inputs, translate all
/// original pages through the actual pure journal model, then persist a bounded
/// index of exact conditional plans. Caller must separately finish/read back.
/// No journal command, witness intent, authorization or provider action occurs.
pub fn prepare(
    e: &Enrollment,
    manifest: &Object,
    sources: &[subset::Enrollment],
    roster: &claims::Roster,
    namespaces: &mut [Namespace],
    nodes: &mut impl NodeStore,
    limits: &Limits,
) -> Result<Object> {
    profiles(e, sources)?;
    let mut work = Work::new(nodes, limits)?;
    let inventory = inspect(e, manifest, &mut work, limits)?;
    if build(
        e,
        &inventory.header.request,
        roster,
        namespaces,
        &mut work,
        limits,
    )? != *manifest
    {
        return Err(Error::Stale);
    }
    let mut pages = Vec::new();
    let mut cursor = None;
    while let Some(p) = page(e, manifest, cursor.as_ref(), &mut work, limits)? {
        if pages.len() >= MAX_PAGES {
            return Err(Error::Budget);
        }
        cursor = p.next.clone();
        pages.push(p);
        if cursor.is_none() {
            break;
        }
    }
    let mut grouped = BTreeMap::<Digest, Vec<(usize, subset::Request)>>::new();
    for (i, p) in pages.iter().enumerate() {
        grouped
            .entry(p.cohort.origin.namespace.clone())
            .or_default()
            .push((
                i,
                subset::Request {
                    source: p.cohort.origin.source.clone(),
                    predecessor_revision: p.cohort.origin.predecessor_revision,
                    paths: p.bindings.iter().map(|b| b.path.clone()).collect(),
                },
            ));
    }
    let mut steps = BTreeMap::new();
    for (ns, requests) in grouped {
        let source = sources
            .iter()
            .find(|s| s.namespace == ns)
            .ok_or(Error::Authority)?;
        let namespace = namespaces
            .iter_mut()
            .find(|n| n.identity() == &ns)
            .ok_or(Error::Authority)?;
        let pin = inventory
            .header
            .basis
            .members
            .iter()
            .find(|p| p.namespace == ns)
            .ok_or(Error::Authority)?;
        if source.policy != pin.policy {
            return Err(Error::Authority);
        }
        let projected = subset::plan_pages(
            source,
            namespace,
            requests.iter().map(|(_, r)| r.clone()).collect(),
            &mut work,
        )?;
        if projected.len() != requests.len() {
            return Err(Error::Invalid);
        }
        for ((i, _), projected) in requests.into_iter().zip(projected) {
            let p = &pages[i];
            let plan = work.put(&projected.plan.to_json()?)?;
            let row = work.doc(&Step {
                schema: STEP_SCHEMA.into(),
                index: i,
                origin: p.cohort.origin.clone(),
                bindings: p.bindings.clone(),
                plan,
                retirement_checkpoint: projected.retirement_checkpoint,
            })?;
            steps.insert(slot(i)?, row);
        }
    }
    if steps.len() != pages.len() || pages.is_empty() {
        return Err(Error::Invalid);
    }
    let bindings = pages.iter().try_fold(0usize, |a, p| {
        a.checked_add(p.bindings.len()).ok_or(Error::Budget)
    })?;
    if bindings != inventory.summary().bindings {
        return Err(Error::Invalid);
    }
    let root = work.tree(&steps, MAX_PAGES)?;
    let result = work.doc(&Header {
        schema: SCHEMA.into(),
        enrollment: e.id()?,
        manifest: manifest.clone(),
        basis: inventory.header.basis.clone(),
        sources: sources.to_vec(),
        steps: root,
        count: pages.len(),
        bindings,
    })?;
    unchanged(&inventory.header.basis, namespaces)?;
    work.timed()?;
    Ok(result)
}
/// Bounded historical readback. This validates metadata structure, not current
/// source eligibility, approval, execution progress or actual receipt history.
pub fn read(
    e: &Enrollment,
    prepared: &Object,
    nodes: &mut impl NodeStore,
    limits: &Limits,
) -> Result<(Header, Vec<(Step, subset::Plan)>)> {
    let mut work = Work::new(nodes, limits)?;
    let h: Header = work.read(prepared)?;
    profiles(e, &h.sources)?;
    h.basis.id()?;
    if h.schema != SCHEMA
        || h.enrollment != e.id()?
        || h.basis.scope()? != e.scope
        || h.count == 0
        || h.count > MAX_PAGES
        || h.bindings == 0
        || h.bindings > limits.bindings
    {
        return Err(Error::Invalid);
    }
    let rows = work.entries(&h.steps, MAX_PAGES)?;
    if rows.len() != h.count {
        return Err(Error::Invalid);
    }
    let inventory = inspect(e, &h.manifest, &mut work, limits)?;
    if inventory.header.basis != h.basis || inventory.summary().bindings != h.bindings {
        return Err(Error::Invalid);
    }
    let mut result = Vec::new();
    let mut bindings = 0usize;
    let mut cursor = None;
    let mut lineage = BTreeMap::<Digest, (Key, u64, source_claims::Selection)>::new();
    for (i, (key, o)) in rows.into_iter().enumerate() {
        let s: Step = work.read(&o)?;
        let plan: subset::Plan = work.read(&s.plan)?;
        plan.id()?;
        if key != slot(i)?
            || s.index != i
            || s.schema != STEP_SCHEMA
            || s.bindings.is_empty()
            || s.bindings.len() > 1024
            || s.bindings.windows(2).any(|b| b[0].path >= b[1].path)
            || plan.request().paths
                != s.bindings
                    .iter()
                    .map(|b| b.path.clone())
                    .collect::<Vec<_>>()
            || !h
                .sources
                .iter()
                .any(|p| p.namespace == s.origin.namespace && p.policy == s.origin.policy)
        {
            return Err(Error::Invalid);
        }
        let p = page(e, &h.manifest, cursor.as_ref(), &mut work, limits)?.ok_or(Error::Invalid)?;
        if p.cohort.origin != s.origin
            || p.bindings != s.bindings
            || (p.next.is_none() != (i + 1 == h.count))
        {
            return Err(Error::Invalid);
        }
        cursor = p.next;
        let occurrence = s.origin.id()?;
        let expected = lineage.get(&occurrence).cloned().unwrap_or((
            s.origin.source.clone(),
            s.origin.predecessor_revision,
            s.origin.original.clone(),
        ));
        if plan.request().source != expected.0
            || plan.request().predecessor_revision != expected.1
            || plan.original() != &expected.2
            || plan.class() != &s.origin.class
        {
            return Err(Error::Invalid);
        }
        for binding in &s.bindings {
            let limits = work.limits.tree(work.limits.source.bindings);
            if Tree::from_root(plan.original().root.clone())
                .get(&binding.path, &mut work, limits)?
                .0
                != Some(binding.object.clone())
            {
                return Err(Error::Invalid);
            }
        }
        if let Some(survivor) = plan.survivor_selection() {
            lineage.insert(
                occurrence,
                (
                    plan.survivor_source()?.ok_or(Error::Invalid)?,
                    3,
                    survivor.clone(),
                ),
            );
        }
        let source = h
            .sources
            .iter()
            .find(|p| p.namespace == s.origin.namespace)
            .ok_or(Error::Authority)?;
        plan.commands(source)?;
        plan.validate_projection(source, &work)?;
        bindings = bindings
            .checked_add(s.bindings.len())
            .ok_or(Error::Budget)?;
        if bindings > h.bindings {
            return Err(Error::Invalid);
        }
        result.push((s, plan));
    }
    if bindings != h.bindings {
        return Err(Error::Invalid);
    }
    Ok((h, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_vectors_pin_preparation_and_step_frames() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/vectors/paged-preparation.design-1.json"
        )))
        .unwrap();
        for case in vectors["cases"].as_array().unwrap() {
            let raw = case["canonical"].as_str().unwrap().as_bytes();
            let actual = match case["kind"].as_str().unwrap() {
                "header" => {
                    let h: Header = serde_json::from_slice(raw).unwrap();
                    assert_eq!(h.schema, SCHEMA);
                    assert_eq!(h.count, 1);
                    h.basis.id().unwrap();
                    for source in &h.sources {
                        source.id().unwrap();
                    }
                    encode(&h).unwrap()
                }
                "step" => {
                    let s: Step = serde_json::from_slice(raw).unwrap();
                    assert_eq!(s.schema, STEP_SCHEMA);
                    s.origin.id().unwrap();
                    encode(&s).unwrap()
                }
                _ => panic!("unknown frame"),
            };
            assert_eq!(actual, raw);
            assert_eq!(raw.len() as u64, case["bytes"].as_u64().unwrap());
            assert_eq!(Digest::blob(raw).as_str(), case["sha256"].as_str().unwrap());
        }
    }
}
