//! Admitted occurrence-age proof and survivor acknowledgement for paged control.
use super::*;
use crate::{
    local::LocalStore,
    metadata::MetadataStore,
    protection::budget::{Budget, Nodes},
    source_claims::{Checkpoint, age},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub enrollment: age::Enrollment,
    pub required_age_millis: u64,
    /// Independently admitted metadata-only namespace classification, not a
    /// credential and not a wildcard exemption from payload protection.
    pub metadata_boundary: Digest,
}
impl Policy {
    pub(super) fn check(&self, e: &Enrollment) -> Result<()> {
        self.enrollment.id()?;
        if self.required_age_millis == 0
            || self.required_age_millis > MAX_NUMBER
            || self.enrollment.metadata != e.selection.metadata
            || self.enrollment.authority != e.selection.authority
            || self.enrollment.epoch != e.selection.epoch
            || self.enrollment.owner != e.selection.owner
            || self.enrollment.clock != e.clock
            || e.sources
                .iter()
                .any(|s| s.namespace == self.enrollment.registry)
        {
            return Err(Error::Authority);
        }
        for source in &e.sources {
            let observer = self
                .enrollment
                .observers
                .iter()
                .find(|o| o.namespace == source.namespace)
                .ok_or(Error::Authority)?;
            if observer.policy != source.policy
                || !observer.owners.contains(&source.owner)
                || source.classes.iter().any(|c| !observer.classes.contains(c))
            {
                return Err(Error::Authority);
            }
        }
        Ok(())
    }
}
/// Explicit private registry/clock ports. The registry is never discovered and
/// must be independently classified as metadata-only outside the payload roster.
pub struct Context<'a> {
    pub registry: &'a mut Namespace,
    pub minimum: &'a Checkpoint,
    pub limits: &'a age::Limits,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Proof {
    schema: String,
    origin: Origin,
    evidence: Object,
    observed: age::Stamp,
}
pub(super) struct Proofs {
    rows: BTreeMap<Digest, Proof>,
}
fn request(o: &Origin) -> age::Request {
    age::Request {
        namespace: o.namespace.clone(),
        source: o.source.clone(),
        predecessor_revision: o.predecessor_revision,
    }
}
fn actual_target(o: &Origin) -> age::Target {
    age::Target {
        request: request(o),
        policy: o.policy.clone(),
        owner: o.owner.clone(),
        class: o.class.clone(),
        selection: o.original.clone(),
    }
}
pub(super) fn boundary(e: &Enrollment, roster: &claims::Roster, ctx: &Context<'_>) -> Result<()> {
    let p = e.age.as_ref().ok_or(Error::Authority)?;
    p.check(e)?;
    ctx.limits.validate()?;
    if ctx.registry.identity() != &p.enrollment.registry
        || roster
            .members
            .iter()
            .any(|m| m.namespace == p.enrollment.registry)
    {
        return Err(Error::Authority);
    }
    ctx.registry.verify_minimum(ctx.minimum)?;
    Ok(())
}
fn eligible(p: &Policy, observed: age::Stamp, clock: &dyn provider::Clock) -> Result<()> {
    let now = clock.bounds(&p.enrollment.clock)?;
    let deadline = observed
        .latest
        .checked_add(p.required_age_millis)
        .filter(|v| *v <= MAX_NUMBER)
        .ok_or(Error::Budget)?;
    if now.earliest == 0
        || now.latest < now.earliest
        || now.latest > MAX_NUMBER
        || now.earliest < deadline
    {
        return Err(Error::Authority);
    }
    Ok(())
}
/// Stage exact original proofs with the ordinary provider/subject decision.
/// Actual finish/readback and both separate caller permissions remain required.
#[allow(clippy::too_many_arguments)]
pub fn plan(
    e: &Enrollment,
    r: Request,
    roster: &claims::Roster,
    namespaces: &mut [Namespace],
    nodes: &mut impl NodeStore,
    stores: &[&LocalStore],
    limits: &Limits,
    ctx: &mut Context<'_>,
    clock: &dyn provider::Clock,
) -> Result<Decision> {
    boundary(e, roster, ctx)?;
    let budget = Budget::new(&limits.source, limits.writes, limits.write_bytes)?;
    r.window.check_clock(clock, &e.clock, 0)?;
    let mut d = decision::plan_inner(
        e,
        r,
        roster,
        namespaces,
        &mut Nodes {
            inner: nodes,
            budget: Some(&budget),
        },
        stores,
        limits,
    )?;
    let (_, steps) = preparation::read(
        &e.selection,
        &d.prepared,
        &mut Nodes {
            inner: nodes,
            budget: Some(&budget),
        },
        limits,
    )?;
    let p = e.age.as_ref().ok_or(Error::Authority)?;
    let mut rows = BTreeMap::new();
    for (step, _) in &steps {
        let id = step.origin.id()?;
        if rows.contains_key(&id) {
            continue;
        }
        let observer = namespaces
            .iter_mut()
            .find(|n| n.identity() == &step.origin.namespace)
            .ok_or(Error::Authority)?;
        let minimum = roster
            .members
            .iter()
            .find(|m| m.namespace == step.origin.namespace)
            .ok_or(Error::Authority)?
            .minimum
            .clone();
        let status = age::Inspection {
            enrollment: &p.enrollment,
            registry: ctx.registry,
            minimum: ctx.minimum,
            metadata: e.selection.metadata.clone(),
            nodes: &mut *nodes,
            limits: ctx.limits,
        }
        .status_metered(&request(&step.origin), observer, &minimum, &budget)?;
        if status.target != Some(actual_target(&step.origin)) {
            return Err(Error::Authority);
        }
        let observed = status.observed.ok_or(Error::Authority)?;
        eligible(p, observed, clock)?;
        rows.insert(
            id,
            Proof {
                schema: "tig.paged-age-proof.design-1".into(),
                origin: step.origin.clone(),
                evidence: status.evidence.ok_or(Error::Authority)?,
                observed,
            },
        );
    }
    let mut wrapped = Nodes {
        inner: nodes,
        budget: Some(&budget),
    };
    let mut work = Work::new(&mut wrapped, limits)?;
    let mut index = BTreeMap::new();
    for (id, proof) in rows {
        index.insert(super::super::path(id.as_str())?, work.doc(&proof)?);
    }
    d.age = Some(work.tree(&index, limits.selections)?);
    d.id()?;
    budget.timed()?;
    Ok(d)
}
pub(super) fn read(
    e: &Enrollment,
    d: &Decision,
    loaded: &Loaded,
    nodes: &mut impl NodeStore,
    limits: &Limits,
) -> Result<Proofs> {
    let root = d.age.as_ref().ok_or(Error::Authority)?;
    let mut work = Work::new(nodes, limits)?;
    let index = work.entries(root, limits.selections)?;
    let expected = loaded
        .steps
        .iter()
        .map(|(s, _)| Ok((s.origin.id()?, &s.origin)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    if index.len() != expected.len() {
        return Err(Error::Invalid);
    }
    let mut rows = BTreeMap::new();
    for (key, object) in index {
        let proof: Proof = work.read(&object)?;
        let id = proof.origin.id()?;
        if proof.schema != "tig.paged-age-proof.design-1"
            || key.as_str() != id.as_str()
            || expected.get(&id) != Some(&&proof.origin)
            || proof.observed.earliest == 0
            || proof.observed.latest < proof.observed.earliest
            || proof.observed.latest > MAX_NUMBER
        {
            return Err(Error::Invalid);
        }
        rows.insert(id, proof);
    }
    e.age.as_ref().ok_or(Error::Authority)?.check(e)?;
    Ok(Proofs { rows })
}
impl Proofs {
    pub(super) fn deadline(&self, e: &Enrollment) -> Result<u64> {
        let required = e.age.as_ref().ok_or(Error::Authority)?.required_age_millis;
        self.rows.values().try_fold(0, |latest, p| {
            let deadline = p
                .observed
                .latest
                .checked_add(required)
                .filter(|v| *v <= MAX_NUMBER)
                .ok_or(Error::Budget)?;
            Ok(latest.max(deadline))
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn verify(
        &self,
        e: &Enrollment,
        roster: &claims::Roster,
        namespaces: &mut [Namespace],
        metadata: &mut impl NodeStore,
        ctx: &mut Context<'_>,
        budget: &Budget,
        clock: Option<&dyn provider::Clock>,
    ) -> Result<()> {
        boundary(e, roster, ctx)?;
        let p = e.age.as_ref().ok_or(Error::Authority)?;
        for proof in self.rows.values() {
            let observer = namespaces
                .iter_mut()
                .find(|n| n.identity() == &proof.origin.namespace)
                .ok_or(Error::Authority)?;
            let minimum = roster
                .members
                .iter()
                .find(|m| m.namespace == proof.origin.namespace)
                .ok_or(Error::Authority)?
                .minimum
                .clone();
            let status = age::Inspection {
                enrollment: &p.enrollment,
                registry: ctx.registry,
                minimum: ctx.minimum,
                metadata: e.selection.metadata.clone(),
                nodes: &mut *metadata,
                limits: ctx.limits,
            }
            .proof_metered(
                &request(&proof.origin),
                &proof.evidence,
                observer,
                &minimum,
                budget,
            )?;
            if status.target != Some(actual_target(&proof.origin))
                || status.observed != Some(proof.observed)
            {
                return Err(Error::Recovery);
            }
            if let Some(clock) = clock {
                eligible(p, proof.observed, clock)?;
            }
        }
        budget.timed()?;
        Ok(())
    }
    /// Reconstruct a deterministic exact chain, never a best-current fact.
    /// Completed pages verify existing acknowledgements rather than recreating them.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn page(
        &self,
        e: &Enrollment,
        step: &preparation::Step,
        plan: &subset::Plan,
        observer: &mut Namespace,
        metadata: &mut MetadataStore,
        ctx: &mut Context<'_>,
        budget: &Budget,
        parent: Option<&age::Status>,
        completed: bool,
    ) -> Result<Option<age::Status>> {
        if completed {
            return self.acknowledged(
                e,
                step,
                plan,
                observer,
                &mut metadata.finished_reader()?,
                ctx,
                budget,
                parent,
            );
        }
        let proof = self.rows.get(&step.origin.id()?).ok_or(Error::Authority)?;
        let parent_request = parent
            .map(|p| p.request.clone())
            .unwrap_or_else(|| request(&proof.origin));
        let evidence = parent
            .and_then(|p| p.evidence.as_ref())
            .unwrap_or(&proof.evidence);
        if plan.request().source != parent_request.source
            || plan.request().predecessor_revision != parent_request.predecessor_revision
        {
            return Err(Error::Recovery);
        }
        if plan.survivor_source()?.is_none() {
            return Ok(None);
        }
        let policy = e.age.as_ref().ok_or(Error::Authority)?;
        let minimum = observer.checkpoint();
        let status = age::Capture {
            enrollment: &policy.enrollment,
            registry: ctx.registry,
            minimum: ctx.minimum,
            metadata,
            limits: ctx.limits,
        }
        .inherit_from_metered(
            &parent_request,
            evidence,
            observer,
            &minimum,
            e.source(&step.origin.namespace)?,
            plan,
            budget,
        )?;
        if status.observed != Some(proof.observed) {
            return Err(Error::Recovery);
        }
        Ok(Some(status))
    }
    #[allow(clippy::too_many_arguments)]
    fn acknowledged(
        &self,
        e: &Enrollment,
        step: &preparation::Step,
        plan: &subset::Plan,
        observer: &mut Namespace,
        nodes: &mut impl NodeStore,
        ctx: &mut Context<'_>,
        budget: &Budget,
        parent: Option<&age::Status>,
    ) -> Result<Option<age::Status>> {
        let proof = self.rows.get(&step.origin.id()?).ok_or(Error::Authority)?;
        let parent_request = parent
            .map(|p| p.request.clone())
            .unwrap_or_else(|| request(&proof.origin));
        if plan.request().source != parent_request.source
            || plan.request().predecessor_revision != parent_request.predecessor_revision
        {
            return Err(Error::Recovery);
        }
        if plan.survivor_source()?.is_none() {
            return Ok(None);
        }
        let evidence = parent
            .and_then(|p| p.evidence.as_ref())
            .unwrap_or(&proof.evidence);
        let minimum = observer.checkpoint();
        let status = age::Inspection {
            enrollment: &e.age.as_ref().ok_or(Error::Authority)?.enrollment,
            registry: ctx.registry,
            minimum: ctx.minimum,
            metadata: e.selection.metadata.clone(),
            nodes,
            limits: ctx.limits,
        }
        .inherited_metered(
            evidence,
            observer,
            &minimum,
            e.source(&step.origin.namespace)?,
            plan,
            budget,
        )?;
        if status.observed != Some(proof.observed) {
            return Err(Error::Recovery);
        }
        Ok(Some(status))
    }
}

/// Resolve historical inheritance uncertainty without a writer or clock. This
/// never publishes a missing fact, completes a page or grants execution authority.
pub fn status(
    inspection: &mut Inspection<'_, '_>,
    d: &Decision,
    ctx: &mut Context<'_>,
) -> Result<Report> {
    let budget = Budget::new(&inspection.limits.source, 0, 0)?;
    let mut report = inspection.status_metered(d, &budget)?;
    boundary(inspection.enrollment, inspection.roster, ctx)?;
    let Some(reader) = inspection.metadata.as_mut() else {
        return Ok(report);
    };
    let mut wrapped = Nodes {
        inner: &mut *reader,
        budget: Some(&budget),
    };
    let loaded = decision::load(inspection.enrollment, d, &mut wrapped, inspection.limits)?;
    let proofs = read(
        inspection.enrollment,
        d,
        &loaded,
        &mut wrapped,
        inspection.limits,
    )?;
    let verified = (|| -> Result<()> {
        proofs.verify(
            inspection.enrollment,
            inspection.roster,
            inspection.namespaces,
            reader,
            ctx,
            &budget,
            None,
        )?;
        let mut inherited = BTreeMap::new();
        for (s, p) in &loaded.steps {
            let observer = inspection
                .namespaces
                .iter_mut()
                .find(|n| n.identity() == &s.origin.namespace)
                .ok_or(Error::Authority)?;
            if observer.source(&p.request().source)?.is_none() {
                continue;
            }
            let actual = subset::inspect(
                inspection.enrollment.source(&s.origin.namespace)?,
                observer,
                p,
            )?;
            if actual.historical_retirement.is_none() {
                continue;
            }
            let id = s.origin.id()?;
            let status = proofs.acknowledged(
                inspection.enrollment,
                s,
                p,
                observer,
                reader,
                ctx,
                &budget,
                inherited.get(&id),
            )?;
            if let Some(status) = status {
                inherited.insert(id, status);
            }
        }
        Ok(())
    })();
    report.age_inheritance_unknown = verified.is_err();
    report.cleanup_unknown = !report.metadata_available
        || report.historical_retired_pages.is_none()
        || report.historical_deleted_actions.is_none()
        || report.pending_cancellations.is_none()
        || report.age_inheritance_unknown;
    if verified.is_err() {
        if report.holds.len() < 64 {
            report.holds.push(execute::Held {
                kind: "age_inheritance_unknown",
                index: 0,
            });
        } else {
            report.unreported_holds += 1;
        }
    }
    budget.timed()?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_aged_frames_preserve_manual_omission_and_bind_exact_proofs() {
        let v: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/vectors/paged-age.design-1.json"
        )))
        .unwrap();
        let raw = v["enrollment"].as_str().unwrap().as_bytes();
        let e: Enrollment = serde_json::from_value(identity::parse_json(raw).unwrap()).unwrap();
        assert_eq!(encode(&e).unwrap(), raw);
        assert_eq!(e.id().unwrap().as_str(), v["enrollment_id"]);
        let raw = v["decision"].as_str().unwrap().as_bytes();
        let d = Decision::from_json(raw).unwrap();
        assert_eq!(d.id().unwrap().as_str(), v["decision_id"]);
        let raw = v["proof"].as_str().unwrap().as_bytes();
        let proof: Proof = serde_json::from_value(identity::parse_json(raw).unwrap()).unwrap();
        assert_eq!(encode(&proof).unwrap(), raw);
        assert_eq!(hash(&proof).unwrap().as_str(), v["proof_id"]);
        let mut changed = e.clone();
        changed.age.as_mut().unwrap().required_age_millis += 1;
        assert_ne!(changed.id().unwrap(), e.id().unwrap());
        changed = e.clone();
        changed.age.as_mut().unwrap().metadata_boundary = Digest::blob(b"other-private-boundary");
        assert_ne!(changed.id().unwrap(), e.id().unwrap());
        let mut manual = e;
        manual.age = None;
        assert!(
            !String::from_utf8(encode(&manual).unwrap())
                .unwrap()
                .contains("\"age\"")
        );
        let mut changed = d.clone();
        changed.age = Some(Object::from_bytes(b"different-proof-index"));
        assert_ne!(d.id().unwrap(), changed.id().unwrap());
        let mut invalid = v["decision"].as_str().unwrap().to_owned();
        invalid.insert_str(1, "\"unknown\":true,");
        assert!(Decision::from_json(invalid.as_bytes()).is_err());
    }
}
