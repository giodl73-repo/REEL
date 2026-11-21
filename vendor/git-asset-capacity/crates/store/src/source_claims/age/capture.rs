use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Observed,
    Sampled,
    MetadataFinished,
    RegistryAdopted,
    BeforeRetire,
    Retired,
}
pub struct Capture<'a> {
    pub enrollment: &'a Enrollment,
    pub registry: &'a mut Namespace,
    pub minimum: &'a Checkpoint,
    pub metadata: &'a mut MetadataStore,
    pub limits: &'a Limits,
}
impl Capture<'_> {
    fn session(&mut self) -> Session<'_> {
        Session {
            enrollment: self.enrollment,
            registry: self.registry,
            minimum: self.minimum,
            metadata: self.metadata,
            limits: self.limits,
            meter: Meter::new(),
        }
    }
    pub fn initialize(&mut self) -> Result<Checkpoint> {
        self.session().initialize()
    }
    pub fn capture(
        &mut self,
        r: &Request,
        observer: &mut Namespace,
        minimum: &Checkpoint,
        clock: &dyn Clock,
    ) -> Result<Status> {
        self.session().capture(r, observer, minimum, clock)
    }
    pub fn capture_hook(
        &mut self,
        r: &Request,
        observer: &mut Namespace,
        minimum: &Checkpoint,
        clock: &dyn Clock,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Status> {
        self.session()
            .capture_hook(r, observer, minimum, clock, hook)
    }
    pub fn inherit(
        &mut self,
        parent: &Request,
        observer: &mut Namespace,
        minimum: &Checkpoint,
        enrollment: &subset::Enrollment,
        plan: &subset::Plan,
    ) -> Result<Status> {
        self.session()
            .inherit(parent, None, observer, minimum, enrollment, plan)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn inherit_from_metered(
        &mut self,
        parent: &Request,
        evidence: &Object,
        observer: &mut Namespace,
        minimum: &Checkpoint,
        enrollment: &subset::Enrollment,
        plan: &subset::Plan,
        budget: &crate::protection::budget::Budget,
    ) -> Result<Status> {
        let mut session = self.session();
        session.meter = Meter::with_budget(Some(budget));
        session.inherit(parent, Some(evidence), observer, minimum, enrollment, plan)
    }
}
struct Session<'a> {
    enrollment: &'a Enrollment,
    registry: &'a mut Namespace,
    minimum: &'a Checkpoint,
    metadata: &'a mut MetadataStore,
    limits: &'a Limits,
    meter: Rc<Meter>,
}
impl Session<'_> {
    fn common(&mut self) -> Result<Source> {
        self.meter.timed(self.limits)?;
        self.limits.validate()?;
        if self.metadata.checkpoint().identity() != &self.enrollment.metadata {
            return Err(Error::Authority);
        }
        self.metadata.finished_reader()?;
        self.enrollment.registry(self.registry, self.minimum)
    }
    fn current(&mut self) -> Result<(Source, BTreeMap<LogicalPath, Object>)> {
        let s = self.common()?;
        let mut w = Work::shared(self.metadata, self.limits, self.meter.clone())?;
        let map = entries(self.enrollment, &s, &mut w)?;
        Ok((s, map))
    }
    pub fn initialize(&mut self) -> Result<Checkpoint> {
        let s = self.common()?;
        if s.current.is_some() {
            self.current()?;
            return Ok(self.registry.checkpoint());
        }
        if s.revision != 1 || !s.predecessors.is_empty() {
            return Err(Error::Recovery);
        }
        let id = self.enrollment.id()?;
        let root = {
            let mut w = Work::shared(self.metadata, self.limits, self.meter.clone())?;
            let (o, raw) = identity_object(self.enrollment)?;
            if w.put(&raw)? != o {
                return Err(Error::Recovery);
            }
            let limits = w.limits.tree();
            Tree::build(&BTreeMap::from([(path("identity")?, o)]), &mut w, limits)?
                .0
                .root()
                .clone()
        };
        self.metadata.finish()?;
        let mut reader = self.metadata.finished_reader()?;
        let readback = Work::shared(&mut reader, self.limits, self.meter.clone())?;
        let (identity, raw) = identity_object(self.enrollment)?;
        if readback.get(&identity)?.as_ref() != Some(&raw) {
            return Err(Error::Recovery);
        }
        let select = Select {
            prepare: operation("init-prepare", &id)?,
            adopt: operation("init-adopt", &id)?,
            source: self.enrollment.source.clone(),
            owner: self.enrollment.owner.clone(),
            expected_revision: 1,
            selection: Selection { root, context: id },
        };
        self.registry
            .select(&select, &readback, &self.limits.source)?;
        self.current()?;
        Ok(self.registry.checkpoint())
    }
    fn cleanup(&mut self, hook: &mut impl FnMut(Stage) -> Result<()>) -> Result<()> {
        loop {
            let (s, current) = self.current()?;
            let Some(old) = s.predecessors.first() else {
                return Ok(());
            };
            let mut historical = s.clone();
            historical.current = Some(old.clone());
            let old_map = {
                let mut w = Work::shared(self.metadata, self.limits, self.meter.clone())?;
                entries(self.enrollment, &historical, &mut w)?
            };
            if old_map.iter().any(|(p, o)| current.get(p) != Some(o)) {
                return Err(Error::Recovery);
            }
            let current_root = &s.current.as_ref().ok_or(Error::Recovery)?.selection.root;
            let key = hash(
                &serde_json::json!({"schema":"tig.age-registry-retirement.design-1","enrollment":self.enrollment.id()?,"old":old.revision,"current":current_root}),
            )?;
            hook(Stage::BeforeRetire)?;
            let mut reader = self.metadata.finished_reader()?;
            let readback = Work::shared(&mut reader, self.limits, self.meter.clone())?;
            self.registry.apply(
                operation("retire", &key)?,
                Command::Retire {
                    source: self.enrollment.source.clone(),
                    owner: self.enrollment.owner.clone(),
                    expected_revision: s.revision,
                    predecessor_revision: old.revision,
                },
                &readback,
                &self.limits.source,
            )?;
            hook(Stage::Retired)?;
        }
    }
    fn status(
        &mut self,
        r: &Request,
        observer: &mut Namespace,
        minimum: &Checkpoint,
    ) -> Result<Status> {
        let mut reader = self.metadata.finished_reader()?;
        Inspection {
            enrollment: self.enrollment,
            registry: self.registry,
            minimum: self.minimum,
            metadata: self.enrollment.metadata.clone(),
            nodes: &mut reader,
            limits: self.limits,
        }
        .status_with(r, observer, minimum, self.meter.clone())
    }
    pub fn capture(
        &mut self,
        r: &Request,
        observer: &mut Namespace,
        minimum: &Checkpoint,
        clock: &dyn Clock,
    ) -> Result<Status> {
        self.capture_hook(r, observer, minimum, clock, |_| Ok(()))
    }
    pub fn capture_hook(
        &mut self,
        r: &Request,
        observer: &mut Namespace,
        minimum: &Checkpoint,
        clock: &dyn Clock,
        mut hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Status> {
        r.id()?;
        self.enrollment.observer(observer, minimum)?;
        if r.namespace != *observer.identity() {
            return Err(Error::Authority);
        }
        if self.status(r, observer, minimum)?.evidence.is_some() {
            self.cleanup(&mut hook)?;
            return self.status(r, observer, minimum);
        }
        target(self.enrollment, observer, minimum, r)?;
        self.cleanup(&mut hook)?;
        let target = target(self.enrollment, observer, minimum, r)?;
        let checkpoint = observer.checkpoint();
        hook(Stage::Observed)?;
        self.meter.timed(self.limits)?;
        let observed = Stamp::sample(clock, &self.enrollment.clock)?;
        self.meter.timed(self.limits)?;
        hook(Stage::Sampled)?;
        let record = Record {
            schema: "tig.age-observation.design-1".into(),
            enrollment: self.enrollment.id()?,
            target,
            origin: Origin::Captured {
                checkpoint,
                observed,
            },
        };
        self.publish(&record, &mut hook)?;
        self.status(r, observer, minimum)
    }
    fn publish(
        &mut self,
        record: &Record,
        hook: &mut impl FnMut(Stage) -> Result<()>,
    ) -> Result<()> {
        let (s, map) = self.current()?;
        let key = record.path()?;
        if map.contains_key(&key) {
            return Ok(());
        }
        if map.len() > self.limits.entries {
            return Err(Error::Budget);
        }
        let original = &s.current.as_ref().ok_or(Error::Recovery)?.selection;
        let (root, object) = {
            let mut w = Work::shared(self.metadata, self.limits, self.meter.clone())?;
            let o = w.put(&encode(record)?)?;
            let changes = BTreeMap::from([(key.clone(), Some(o.clone()))]);
            let limits = w.limits.tree();
            let root = Tree::from_root(original.root.clone())
                .edit(&original.root, &changes, &mut w, limits)?
                .0
                .root()
                .clone();
            (root, o)
        };
        self.metadata.finish()?;
        // Independent finished-provider readback precedes root adoption. Never
        // acknowledge a registry reference whose immutable record is unreadable.
        let mut reader = self.metadata.finished_reader()?;
        let mut readback = Work::shared(&mut reader, self.limits, self.meter.clone())?;
        if readback.record(&object, self.enrollment)? != *record {
            return Err(Error::Recovery);
        }
        let mut projected = s.clone();
        projected.current = Some(super::super::Version {
            revision: s.revision,
            selection: Selection {
                root: root.clone(),
                context: self.enrollment.id()?,
            },
        });
        let actual = entries(self.enrollment, &projected, &mut readback)?;
        if actual.get(&key) != Some(&object) || map.iter().any(|(p, o)| actual.get(p) != Some(o)) {
            return Err(Error::Recovery);
        }
        hook(Stage::MetadataFinished)?;
        self.meter.timed(self.limits)?;
        let id = hash(&(self.enrollment.id()?, record.slot()?))?;
        self.registry.select(
            &Select {
                prepare: operation("prepare", &id)?,
                adopt: operation("adopt", &id)?,
                source: self.enrollment.source.clone(),
                owner: self.enrollment.owner.clone(),
                expected_revision: s.revision,
                selection: Selection {
                    root,
                    context: self.enrollment.id()?,
                },
            },
            &readback,
            &self.limits.source,
        )?;
        hook(Stage::RegistryAdopted)?;
        self.cleanup(hook)
    }
    /// Carry a proven historical unchanged-subset observation forward. It accepts
    /// no caller timestamp and does not manufacture age from equal hashes.
    #[allow(clippy::too_many_arguments)]
    pub fn inherit(
        &mut self,
        parent: &Request,
        pinned: Option<&Object>,
        observer: &mut Namespace,
        minimum: &Checkpoint,
        enrollment: &subset::Enrollment,
        plan: &subset::Plan,
    ) -> Result<Status> {
        self.enrollment.observer(observer, minimum)?;
        let p = if let Some(evidence) = pinned {
            let mut reader = self.metadata.finished_reader()?;
            Inspection {
                enrollment: self.enrollment,
                registry: self.registry,
                minimum: self.minimum,
                metadata: self.enrollment.metadata.clone(),
                nodes: &mut reader,
                limits: self.limits,
            }
            .proof_with(parent, evidence, observer, minimum, self.meter.clone())?
        } else {
            self.status(parent, observer, minimum)?
        };
        let parent_object = p.evidence.ok_or(Error::Missing)?;
        let old = p.target.ok_or(Error::Missing)?;
        if enrollment.namespace != old.request.namespace
            || enrollment.owner != old.owner
            || plan.request().source != old.request.source
            || plan.request().predecessor_revision != old.request.predecessor_revision
            || plan.original() != &old.selection
        {
            return Err(Error::Authority);
        }
        self.cleanup(&mut |_| Ok(()))?;
        let (source, v) = {
            let w = Work::shared(self.metadata, self.limits, self.meter.clone())?;
            subset::historical_survivor(enrollment, observer, plan, &w)?.ok_or(Error::Missing)?
        };
        let request = Request {
            namespace: old.request.namespace,
            source,
            predecessor_revision: v.revision,
        };
        let plan_object = Object::from_bytes(&plan.to_json()?);
        let key = path(&format!(
            "age/{}",
            inheritance_key(&request, &parent_object, &plan_object)?.as_str()
        ))?;
        let (_, map) = self.current()?;
        // Acknowledged inheritance remains replayable after later owned pages
        // retire this survivor. It never recreates the missing source claim.
        if let Some(evidence) = map.get(&key) {
            return self.inherited_status(&request, evidence, observer, minimum, pinned.is_some());
        }
        let survivor = observer.source(&request.source)?.ok_or(Error::Missing)?;
        if survivor.owner != old.owner || survivor.kind.as_ref() != Some(&old.class) {
            return Err(Error::Authority);
        }
        // Existing exact enrollment/prepare/adopt/withdraw/retire receipts prove
        // this historical version even after a later page retired its leaf.
        // Assessment still requires the occurrence to be currently present.
        let target = Target {
            request: request.clone(),
            policy: old.policy,
            owner: old.owner,
            class: old.class,
            selection: v.selection,
        };
        let checkpoint = observer.checkpoint();
        let plan = {
            let mut w = Work::shared(self.metadata, self.limits, self.meter.clone())?;
            w.put(&plan.to_json()?)?
        };
        self.metadata.finish()?;
        let record = Record {
            schema: "tig.age-observation.design-1".into(),
            enrollment: self.enrollment.id()?,
            target,
            origin: Origin::Inherited {
                checkpoint,
                parent: parent_object,
                plan,
                enrollment: Box::new(enrollment.clone()),
            },
        };
        self.publish(&record, &mut |_| Ok(()))?;
        let (_, map) = self.current()?;
        let evidence = map.get(&key).ok_or(Error::Recovery)?;
        self.inherited_status(&request, evidence, observer, minimum, pinned.is_some())
    }
    fn inherited_status(
        &mut self,
        request: &Request,
        evidence: &Object,
        observer: &mut Namespace,
        minimum: &Checkpoint,
        exact: bool,
    ) -> Result<Status> {
        if !exact {
            return self.status(request, observer, minimum);
        }
        let mut reader = self.metadata.finished_reader()?;
        Inspection {
            enrollment: self.enrollment,
            registry: self.registry,
            minimum: self.minimum,
            metadata: self.enrollment.metadata.clone(),
            nodes: &mut reader,
            limits: self.limits,
        }
        .proof_with(request, evidence, observer, minimum, self.meter.clone())
    }
}
