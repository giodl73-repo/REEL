//! Fresh copy-only decisions from sealed historical controller provenance.
//! No source commands and no permission inherited from an earlier decision.
use super::*;
use crate::{
    ledger::JournalRef,
    local::LocalStore,
    placement::Local,
    protection::budget::{Budget, Nodes},
};
mod execute;
pub use execute::{Cancellation, Execution, Inspection, Report, RunLimits};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub decision: super::Decision,
    pub witness_job: JournalRef,
    pub jobs: Vec<StoreJob>,
}
impl Origin {
    fn check(&self, e: &Enrollment) -> Result<()> {
        if self.decision.enrollment_id() != &e.id()?
            || self.jobs.len() > 4
            || self.jobs.windows(2).any(|j| j[0].store >= j[1].store)
        {
            return Err(Error::Authority);
        }
        self.decision.id()?;
        Ok(())
    }
    fn sealed(
        &self,
        e: &Enrollment,
        roster: &claims::Roster,
        witness: &mut Local<'_>,
        limits: &Limits,
    ) -> Result<bool> {
        self.check(e)?;
        if witness.job != &self.witness_job {
            return Err(Error::Authority);
        }
        let observed = super::Inspection {
            enrollment: e,
            roster,
            namespaces: &mut [],
            metadata: None,
            places: &mut [],
            jobs: &self.jobs,
            witness,
            admitted_context: &e.context,
            limits,
        }
        .status(&self.decision)?;
        match observed.phase {
            Phase::Completed => Ok(false),
            Phase::Cancelled => Ok(true),
            _ => Err(Error::Recovery),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub operation: Key,
    pub origin: Origin,
    pub copies: Object,
    pub window: provider::Window,
}
impl Request {
    fn check(&self, e: &Enrollment) -> Result<()> {
        self.origin.check(e)?;
        if self.window.expires <= self.window.not_before
            || self.window.expires - self.window.not_before > e.max_window
        {
            return Err(Error::Invalid);
        }
        encode(self)?;
        Ok(())
    }
}

/// Immutable candidate header. Its declared provenance is not an admission
/// witness, current protection proof or final-loss authorization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    schema: String,
    enrollment: Digest,
    request: Request,
    origin_cancelled: bool,
    basis: Object,
    subjects: Object,
    actions: Object,
    objects: usize,
    action_count: usize,
}
pub struct Authorization {
    decision: Digest,
}
impl Decision {
    pub fn id(&self) -> Result<Digest> {
        if self.schema != "tig.held-copy-followup-decision.design-1"
            || self.objects == 0
            || self.objects > 65536
            || self.action_count > 65536
        {
            return Err(Error::Invalid);
        }
        self.request.origin.decision.id()?;
        hash(self)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.id()?;
        encode(self)
    }
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        if raw.len() > 65536 {
            return Err(Error::Budget);
        }
        let d: Self =
            serde_json::from_value(identity::parse_json(raw).map_err(|_| Error::Invalid)?)
                .map_err(|_| Error::Invalid)?;
        if d.to_json()? != raw {
            return Err(Error::Invalid);
        }
        Ok(d)
    }
    pub fn request(&self) -> &Request {
        &self.request
    }
    pub fn enrollment_id(&self) -> &Digest {
        &self.enrollment
    }
    /// A new explicit caller approval; an old controller token cannot substitute.
    pub fn authorize_final_loss(&self) -> Result<Authorization> {
        Ok(Authorization {
            decision: self.id()?,
        })
    }
}

fn copy_key(c: &bulk::Copy) -> (Digest, Digest) {
    (c.store.clone(), c.publication.object.sha256.clone())
}
fn action(
    e: &Enrollment,
    r: &Request,
    basis: &Object,
    cancelled: bool,
    c: &bulk::Copy,
) -> Result<provider::Request> {
    let context = hash(
        &serde_json::json!({"schema":"tig.held-copy-followup-basis.design-1",
        "enrollment":e.id()?,"request":r,"basis":basis,"origin_cancelled":cancelled}),
    )?;
    let key = hash(
        &serde_json::json!({"schema":"tig.held-copy-followup-copy.design-1","basis":context,"copy":c}),
    )?;
    Ok(provider::Request {
        operation: Key::parse(&format!("followup-final-{}", key.as_str()))
            .map_err(|_| Error::Invalid)?,
        owner: e.selection.owner.clone(),
        publication: c.publication.clone(),
        capacity: c.capacity.clone(),
        context,
        window: r.window.clone(),
    })
}

fn current_basis(roster: &claims::Roster, namespaces: &mut [Namespace]) -> Result<claims::Roster> {
    roster.id()?;
    if roster.coverage.is_none() || namespaces.len() != roster.members.len() {
        return Err(Error::Authority);
    }
    let mut basis = roster.clone();
    for pin in &mut basis.members {
        let n = namespaces
            .iter_mut()
            .find(|n| n.identity() == &pin.namespace)
            .ok_or(Error::Authority)?;
        n.refresh_minimum(&pin.minimum)?;
        admission::bulk::snapshot(n)?;
        let now = claims::NamespacePin::current(n)?;
        if now.policy != pin.policy {
            return Err(Error::Authority);
        }
        *pin = now;
    }
    Ok(basis)
}

pub struct Planning<'a, 'b, S> {
    pub enrollment: &'a Enrollment,
    pub roster: &'a claims::Roster,
    pub namespaces: &'a mut [Namespace],
    pub nodes: &'a mut S,
    pub stores: &'a [&'a LocalStore],
    /// Actual original witness with the privately admitted original job binding.
    pub origin_witness: &'a mut Local<'b>,
    pub clock: &'a dyn provider::Clock,
    pub age: Option<aged::Context<'a>>,
    pub limits: &'a Limits,
}
impl<S: NodeStore> Planning<'_, '_, S> {
    /// Stage new metadata only. Finish and independent readback remain required.
    pub fn plan(&mut self, r: Request) -> Result<Decision> {
        let e = self.enrollment;
        e.id()?;
        let budget = Budget::new(
            &self.limits.source,
            self.limits.writes,
            self.limits.write_bytes,
        )?;
        r.check(e)?;
        if self.roster.scope()? != e.selection.scope
            || self.stores.len() > 4
            || self
                .stores
                .iter()
                .map(|s| s.identity())
                .collect::<BTreeSet<_>>()
                .len()
                != self.stores.len()
            || e.age.is_some() != self.age.is_some()
        {
            return Err(Error::Authority);
        }
        r.window.check_clock(self.clock, &e.clock, 0)?;
        let cancelled = r
            .origin
            .sealed(e, self.roster, self.origin_witness, self.limits)?;
        let mut nodes = Nodes {
            inner: &mut *self.nodes,
            budget: Some(&budget),
        };
        let original = decision::load(e, &r.origin.decision, &mut nodes, self.limits)?;
        if let Some(ctx) = &mut self.age {
            aged::read(e, &r.origin.decision, &original, &mut nodes, self.limits)?.verify(
                e,
                self.roster,
                self.namespaces,
                &mut nodes,
                ctx,
                &budget,
                Some(self.clock),
            )?;
        }
        let copies = super::read_copies(&r.copies, &mut nodes, self.limits)?;
        let objects = original
            .subjects
            .iter()
            .map(|s| (s.object.sha256.clone(), &s.object))
            .collect::<BTreeMap<_, _>>();
        for c in &copies {
            if c.publication.owner != e.selection.owner
                || objects.get(&c.publication.object.sha256) != Some(&&c.publication.object)
            {
                return Err(Error::Invalid);
            }
        }
        let mut by_object = BTreeMap::<Digest, Vec<&bulk::Copy>>::new();
        for copy in &copies {
            by_object
                .entry(copy.publication.object.sha256.clone())
                .or_default()
                .push(copy);
        }
        let basis = current_basis(self.roster, self.namespaces)?;
        let mut work = Work::new(&mut nodes, self.limits)?;
        let basis_object = work.doc(&basis)?;
        let mut subjects = BTreeMap::new();
        let mut actions = BTreeMap::new();
        for s in &original.subjects {
            let requested = by_object
                .get(&s.object.sha256)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let mut hold = if requested.is_empty() {
                Some(Hold::NoCopyScope)
            } else {
                None
            };
            if hold.is_none() {
                match claims::acquire(&basis, &s.object, self.namespaces, &work) {
                    Ok(_) => (),
                    Err(claims::Error::Protected) => hold = Some(Hold::SurvivingClaims),
                    Err(error) => return Err(error.into()),
                }
            }
            let mut planned = Vec::new();
            if hold.is_none() {
                for c in requested {
                    let Some(store) = self
                        .stores
                        .iter()
                        .find(|store| store.identity() == &c.store)
                    else {
                        hold = Some(Hold::ProviderUnavailable);
                        break;
                    };
                    match provider::plan(store, action(e, &r, &basis_object, cancelled, c)?) {
                        Ok(p)
                            if p.policy.scope == e.selection.scope && p.policy.clock == e.clock =>
                        {
                            planned.push(p)
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
                for p in planned {
                    actions.insert(slot(actions.len())?, work.doc(&p)?);
                }
            }
            subjects.insert(
                super::super::path(s.object.sha256.as_str())?,
                work.doc(&Subject {
                    object: s.object.clone(),
                    hold,
                })?,
            );
        }
        let subject_root = work.tree(&subjects, 65536)?;
        let action_root = work.tree(&actions, 65536)?;
        if current_basis(self.roster, self.namespaces)? != basis
            || r.origin
                .sealed(e, self.roster, self.origin_witness, self.limits)?
                != cancelled
        {
            return Err(Error::Stale);
        }
        budget.timed()?;
        Ok(Decision {
            schema: "tig.held-copy-followup-decision.design-1".into(),
            enrollment: e.id()?,
            request: r,
            origin_cancelled: cancelled,
            basis: basis_object,
            subjects: subject_root,
            actions: action_root,
            objects: subjects.len(),
            action_count: actions.len(),
        })
    }
}

struct Loaded {
    original: super::Loaded,
    basis: claims::Roster,
    subjects: Vec<Subject>,
    actions: Vec<provider::Plan>,
}
fn load(
    e: &Enrollment,
    d: &Decision,
    nodes: &mut impl NodeStore,
    limits: &Limits,
) -> Result<Loaded> {
    e.id()?;
    d.id()?;
    d.request.check(e)?;
    if d.enrollment != e.id()? {
        return Err(Error::Authority);
    }
    let mut work = Work::new(nodes, limits)?;
    let original = decision::load(e, &d.request.origin.decision, &mut work, limits)?;
    let basis: claims::Roster = work.read(&d.basis)?;
    basis.id()?;
    if basis.scope()? != e.selection.scope || basis.coverage.is_none() {
        return Err(Error::Authority);
    }
    let copies = super::read_copies(&d.request.copies, &mut work, limits)?;
    let mut requested = copies
        .iter()
        .map(|c| (copy_key(c), c))
        .collect::<BTreeMap<_, _>>();
    if requested.len() != copies.len() {
        return Err(Error::Invalid);
    }
    let original_objects = original
        .subjects
        .iter()
        .map(|s| (&s.object.sha256, &s.object))
        .collect::<BTreeMap<_, _>>();
    let copy_objects = copies
        .iter()
        .map(|c| &c.publication.object.sha256)
        .collect::<BTreeSet<_>>();
    let mut subjects = Vec::new();
    let rows = work.entries(&d.subjects, 65536)?;
    if rows.len() != d.objects || rows.len() != original.subjects.len() {
        return Err(Error::Invalid);
    }
    for (key, object) in rows {
        let s: Subject = work.read(&object)?;
        if key.as_str() != s.object.sha256.as_str()
            || original_objects.get(&s.object.sha256) != Some(&&s.object)
            || (s.hold == Some(Hold::NoCopyScope)) != !copy_objects.contains(&s.object.sha256)
        {
            return Err(Error::Invalid);
        }
        subjects.push(s);
    }
    let subject_index = subjects
        .iter()
        .map(|s| (&s.object.sha256, s))
        .collect::<BTreeMap<_, _>>();
    for c in &copies {
        if c.publication.owner != e.selection.owner
            || subject_index
                .get(&c.publication.object.sha256)
                .is_none_or(|s| s.object != c.publication.object)
        {
            return Err(Error::Invalid);
        }
    }
    let rows = work.entries(&d.actions, 65536)?;
    if rows.len() != d.action_count {
        return Err(Error::Invalid);
    }
    let mut actions = Vec::new();
    for (i, (key, object)) in rows.into_iter().enumerate() {
        if key != slot(i)? {
            return Err(Error::Invalid);
        }
        let p: provider::Plan = work.read(&object)?;
        p.id()?;
        let c = requested
            .remove(&(
                p.enrollment.store().clone(),
                p.request().publication.object.sha256.clone(),
            ))
            .ok_or(Error::Invalid)?;
        if encode(p.request())? != encode(&action(e, &d.request, &d.basis, d.origin_cancelled, c)?)?
            || p.policy.scope != e.selection.scope
            || p.policy.clock != e.clock
            || subject_index
                .get(&c.publication.object.sha256)
                .is_none_or(|s| s.hold.is_some())
        {
            return Err(Error::Invalid);
        }
        actions.push(p);
    }
    if requested.values().any(|c| {
        subject_index
            .get(&c.publication.object.sha256)
            .is_some_and(|s| s.hold.is_none())
    }) {
        return Err(Error::Invalid);
    }
    Ok(Loaded {
        original,
        basis,
        subjects,
        actions,
    })
}

pub struct Inventory {
    decision: Digest,
    loaded: Loaded,
}
impl Inventory {
    pub fn decision(&self) -> &Digest {
        &self.decision
    }
    pub fn basis(&self) -> &claims::Roster {
        &self.loaded.basis
    }
    pub fn subjects(&self) -> impl ExactSizeIterator<Item = &Subject> {
        self.loaded.subjects.iter()
    }
    pub fn actions(&self) -> impl ExactSizeIterator<Item = (&Digest, &provider::Plan)> {
        self.loaded
            .actions
            .iter()
            .map(|p| (p.enrollment.store(), p))
    }
    pub fn authorizes_removal(&self) -> bool {
        false
    }
}
/// Pure original candidate metadata readback; no witness, source or clock IO.
pub fn inspect(
    e: &Enrollment,
    d: &Decision,
    nodes: &mut impl NodeStore,
    limits: &Limits,
) -> Result<Inventory> {
    Ok(Inventory {
        decision: d.id()?,
        loaded: load(e, d, nodes, limits)?,
    })
}
