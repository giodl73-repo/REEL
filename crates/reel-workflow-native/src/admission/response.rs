//! Bounded workflow-specific extension of an active job's input envelope.
//! Uses existing SDK work and retains the original generation and all references.
use super::*;

pub(super) fn entries(
    selection: &Selection,
    nodes: &mut MetadataStore,
) -> Result<BTreeMap<LogicalPath, Object>> {
    Tree::from_root(selection.root.clone())
        .entries(nodes, TreeLimits::default())
        .map(|v| v.0)
        .map_err(|_| error(ErrorKind::Integrity, "response-claim-tree"))
}

impl Prepared {
    /// Reconcile ONLY already-admitted original response work before requiring
    /// a settled ClaimsGuard. Never starts a new admission through this path.
    pub(crate) fn response_guard(
        self,
        receipt: &Receipt,
        operation: &Key,
        evidence: &Object,
        outputs: &BTreeSet<Object>,
    ) -> Result<ClaimsGuard> {
        self.check_receipt(receipt)?;
        let (mut index, mut nodes) = self.stores_with_pending(Some(receipt), true)?;
        let fence = qualified::prepublication::actor_fence(
            index.identity(),
            &self.profile.scope.coordination_root,
            &self.profile.scope.actor,
        )
        .map_err(native_error)?;
        if let Some(reference) =
            work::lookup(&mut index, operation).map_err(|e| native_error(e.into()))?
        {
            let record =
                work::read(&mut index, &reference, &nodes).map_err(|e| native_error(e.into()))?;
            self.check_response(&record, receipt, evidence, outputs, &mut nodes)?;
            if !work::complete(&mut index, &reference).map_err(|e| native_error(e.into()))?
                && Index::observe(&self.profile.index_root, &index.checkpoint(), &nodes)
                    .map_err(|e| native_error(e.into()))?
                    .pending
            {
                work::resume_admitted(&mut index, &reference, &mut nodes)
                    .map_err(|e| native_error(e.into()))?;
            }
        }
        drop(nodes);
        drop(index);
        drop(fence);
        self.reopen(receipt)
    }

    pub(super) fn check_response(
        &self,
        record: &work::Record,
        receipt: &Receipt,
        evidence: &Object,
        outputs: &BTreeSet<Object>,
        nodes: &mut MetadataStore,
    ) -> Result<()> {
        let binding =
            envelope::read_binding(&record.update.selection, nodes, self.projection_limits())
                .map_err(|e| native_error(e.into()))?;
        let present: BTreeSet<_> = entries(&record.update.selection, nodes)?
            .into_values()
            .collect();
        if record.evidence.as_ref() != Some(evidence)
            || record.additional.is_some()
            || record.adoption.is_some()
            || record.update.actor != self.profile.scope.actor
            || record.update.owner != self.profile.scope.owner
            || !record.update.ready
            || binding.native != self.input
            || binding.policy != self.profile.scope.policy
            || !present.contains(&receipt.activation)
            || !present.contains(evidence)
            || !self.resources.objects().is_subset(&present)
            || !outputs.is_subset(&present)
        {
            return Err(error(ErrorKind::Conflict, "response-original-registration"));
        }
        Ok(())
    }
}

impl ClaimsGuard {
    /// Durable logical registration followed by positive source protection.
    /// No native byte publication, retained-owner ack or selection occurs here.
    pub(crate) fn protect_response(
        &self,
        receipt: &Receipt,
        operation: &Key,
        raw: &[u8],
        outputs: &BTreeSet<Object>,
        apply: bool,
    ) -> Result<WorkRef> {
        let p = &self._prepared;
        let evidence = Object::from_bytes(raw);
        let (mut index, mut nodes) = p.stores(Some(receipt))?;
        let reference = if let Some(reference) =
            work::lookup(&mut index, operation).map_err(|e| native_error(e.into()))?
        {
            let record =
                work::read(&mut index, &reference, &nodes).map_err(|e| native_error(e.into()))?;
            p.check_response(&record, receipt, &evidence, outputs, &mut nodes)?;
            reference
        } else {
            if raw.len() > metadata::MAX_NODE_BYTES {
                return Err(error(ErrorKind::Budget, "response-registration-bytes"));
            }
            nodes
                .put(raw)
                .map_err(|_| error(ErrorKind::NeedsRecovery, "response-metadata-write"))?;
            let current = index
                .actor(&mut nodes, &p.profile.scope.actor)
                .map_err(|e| native_error(e.into()))?;
            let old = entries(&current.selection, &mut nodes)?;
            let mut objects: BTreeSet<_> = old.values().cloned().collect();
            objects.extend(outputs.iter().cloned());
            objects.insert(current.selection.root.clone());
            objects.insert(evidence);
            let target = envelope::create(
                &Resources::admitted(&p.input, objects),
                &p.profile.scope.policy,
                &mut nodes,
                p.projection_limits(),
            )
            .map_err(|e| native_error(e.into()))?;
            let next = entries(target.selection(), &mut nodes)?;
            // Qualified slot close clears the full source tree in one bounded
            // phase. Never grow a job that cannot subsequently close safely.
            if next.len() > protection::MAX_CHANGES {
                return Err(error(ErrorKind::Budget, "response-job-close-path-limit"));
            }
            let mut changes = BTreeMap::new();
            for (path, object) in &next {
                if old.get(path) != Some(object) {
                    changes.insert(path.clone(), Some(object.clone()));
                }
            }
            for path in old.keys() {
                if !next.contains_key(path) {
                    changes.insert(path.clone(), None);
                }
            }
            if changes.len() > protection::MAX_CHANGES {
                return Err(error(ErrorKind::Budget, "response-changed-path-limit"));
            }
            let update = protection::Update {
                operation: operation.clone(),
                actor: p.profile.scope.actor.clone(),
                owner: p.profile.scope.owner.clone(),
                expected_revision: current.revision,
                selection: target.selection().clone(),
                changes,
                ready: true,
            };
            work::stage(
                &mut index,
                &update,
                Some(Object::from_bytes(raw)),
                &mut nodes,
            )
            .map_err(|e| native_error(e.into()))?
        };
        if !apply {
            return Ok(reference);
        }
        work::execute(&mut index, &reference, &mut nodes).map_err(|e| native_error(e.into()))?;
        let current = index
            .actor(&mut nodes, &p.profile.scope.actor)
            .map_err(|e| native_error(e.into()))?;
        let present: BTreeSet<_> = entries(&current.selection, &mut nodes)?
            .into_values()
            .collect();
        if !current.ready
            || !outputs.is_subset(&present)
            || !present.contains(&Object::from_bytes(raw))
        {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "response-positive-protection",
            ));
        }
        for object in outputs
            .iter()
            .chain(std::iter::once(&Object::from_bytes(raw)))
        {
            let coverage = index
                .inspect(&mut nodes, object, &p.profile.scope.store)
                .map_err(|e| native_error(e.into()))?;
            if !coverage.protected_by.contains(&p.profile.scope.actor) {
                return Err(error(
                    ErrorKind::NeedsRecovery,
                    "response-native-claim-coverage",
                ));
            }
        }
        nodes.finish().map_err(|e| native_error(e.into()))?;
        Ok(reference)
    }
}
