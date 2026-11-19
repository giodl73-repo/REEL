//! Additive retained actor registration over existing SDK work. This is not the
//! producer JobContext/SourceJournal outcome-adoption protocol.
use super::response::entries;
use super::*;
use crate::{
    response::Candidate,
    retention::{Acknowledgement, Target},
};

fn plan(
    profile: &Profile,
    receipt: &Receipt,
    candidate: &Candidate,
    target: &Target,
) -> Result<(Key, Vec<u8>)> {
    let operation = Key::parse(&format!(
        "workflow-retain-{}",
        Digest::blob(&encode(&(
            "reel.workflow-retain-key.v1",
            &profile.index_identity,
            &candidate.task_key,
            &candidate.attempt_id,
        ))?)
        .as_str()
    ))
    .map_err(|_| error(ErrorKind::Invalid, "retention-operation-key"))?;
    let raw = encode(
        &serde_json::json!({"schema":"reel.workflow-retained-registration.v1",
        "index":profile.index_identity,"admission":receipt.operation_identity()?,"candidate":candidate,"target":target}),
    )?;
    Ok((operation, raw))
}

impl Prepared {
    /// Read-only preflight; later retention still rechecks actual coverage.
    pub(crate) fn check_retained_target(&self, target: &Target) -> Result<()> {
        let (mut index, mut nodes) = self.stores(None)?;
        self.retained_scope(&index, target)?;
        if !index
            .actor(&mut nodes, &target.actor)
            .map_err(|e| native_error(e.into()))?
            .ready
        {
            return Err(error(ErrorKind::Busy, "retention-target-not-ready"));
        }
        Ok(())
    }
    pub(crate) fn validate_retained_target(
        &self,
        receipt: &Receipt,
        target: &Target,
    ) -> Result<()> {
        self.check_receipt(receipt)?;
        let (index, _) = self.stores_with_pending(Some(receipt), true)?;
        self.retained_scope(&index, target)?;
        Ok(())
    }
    pub(crate) fn has_retention_work(
        &self,
        receipt: &Receipt,
        result: &reel_workflow::ReturnedResult,
        target: &Target,
    ) -> Result<bool> {
        self.check_receipt(receipt)?;
        let (mut index, mut nodes) = self.stores_with_pending(Some(receipt), true)?;
        self.retained_scope(&index, target)?;
        let (operation, raw, outputs) = crate::response::identity(&self.profile, receipt, result)?;
        let Some(reference) =
            work::lookup(&mut index, &operation).map_err(|e| native_error(e.into()))?
        else {
            return Ok(false);
        };
        let record =
            work::read(&mut index, &reference, &nodes).map_err(|e| native_error(e.into()))?;
        self.check_response(
            &record,
            receipt,
            &Object::from_bytes(&raw),
            &outputs,
            &mut nodes,
        )?;
        if !work::complete(&mut index, &reference).map_err(|e| native_error(e.into()))? {
            return Ok(false);
        }
        let candidate = self.response_candidate(receipt, result, &mut index, &mut nodes)?;
        let (operation, _) = plan(&self.profile, receipt, &candidate, target)?;
        let Some(reference) =
            work::lookup(&mut index, &operation).map_err(|e| native_error(e.into()))?
        else {
            return Ok(false);
        };
        self.retained_record(
            receipt, &candidate, target, &reference, &mut nodes, &mut index,
        )?;
        Ok(true)
    }
    pub(crate) fn completed_retention(
        &self,
        receipt: &Receipt,
        result: &reel_workflow::ReturnedResult,
        target: &Target,
    ) -> Result<Option<Acknowledgement>> {
        self.check_receipt(receipt)?;
        let (mut index, mut nodes) = self.stores_with_pending(Some(receipt), true)?;
        self.retained_scope(&index, target)?;
        let _fence = qualified::prepublication::actor_fence(
            index.identity(),
            &self.profile.scope.coordination_root,
            &target.actor,
        )
        .map_err(native_error)?;
        let (response_operation, _, _) = crate::response::identity(&self.profile, receipt, result)?;
        if work::lookup(&mut index, &response_operation)
            .map_err(|e| native_error(e.into()))?
            .is_none()
        {
            return Ok(None);
        }
        let response_reference = work::lookup(&mut index, &response_operation)
            .map_err(|e| native_error(e.into()))?
            .ok_or_else(|| error(ErrorKind::NeedsRecovery, "retention-response-disappeared"))?;
        if !work::complete(&mut index, &response_reference).map_err(|e| native_error(e.into()))? {
            // No retained acknowledgement can exist before response admission.
            return Ok(None);
        }
        let candidate = self.response_candidate(receipt, result, &mut index, &mut nodes)?;
        let (operation, raw) = plan(&self.profile, receipt, &candidate, target)?;
        let Some(reference) =
            work::lookup(&mut index, &operation).map_err(|e| native_error(e.into()))?
        else {
            return Ok(None);
        };
        self.retained_record(
            receipt, &candidate, target, &reference, &mut nodes, &mut index,
        )?;
        if !work::complete(&mut index, &reference).map_err(|e| native_error(e.into()))? {
            return Ok(None);
        }
        let observed = Index::observe(&self.profile.index_root, &index.checkpoint(), &nodes)
            .map_err(|e| native_error(e.into()))?;
        if observed.pending {
            return Err(error(ErrorKind::NeedsRecovery, "retention-index-pending"));
        }
        let ack = Acknowledgement {
            schema: "reel.workflow-retained-acknowledgement.v1".into(),
            candidate,
            target: target.clone(),
            evidence: Object::from_bytes(&raw),
            work: reference,
            index_checkpoint: index.checkpoint(),
            metadata_checkpoint: nodes.finish().map_err(|e| native_error(e.into()))?,
        };
        self.verify_ack(receipt, &ack, &mut index, &mut nodes)?;
        Ok(Some(ack))
    }
    pub(super) fn retained_scope(&self, index: &Index, target: &Target) -> Result<Vec<Digest>> {
        let enrollment = index.enrollment_profile();
        let source = enrollment
            .actors
            .iter()
            .find(|a| a.id == self.profile.scope.actor)
            .ok_or_else(|| error(ErrorKind::Denied, "retention-source-enrollment"))?;
        let retained = enrollment
            .actors
            .iter()
            .find(|a| a.id == target.actor)
            .ok_or_else(|| error(ErrorKind::Denied, "retention-target-enrollment"))?;
        if target.schema != "reel.workflow-retained-target.v1"
            || target.actor == source.id
            || retained.owner != target.owner
            || index.routing().is_some_and(|r| r.actor == target.actor)
            || source.stores.iter().any(|s| !retained.stores.contains(s))
        {
            return Err(error(ErrorKind::Denied, "retention-target-scope"));
        }
        Ok(source.stores.clone())
    }
    pub(super) fn response_candidate(
        &self,
        receipt: &Receipt,
        result: &reel_workflow::ReturnedResult,
        index: &mut Index,
        nodes: &mut MetadataStore,
    ) -> Result<Candidate> {
        let (operation, raw, outputs) = crate::response::identity(&self.profile, receipt, result)?;
        let evidence = Object::from_bytes(&raw);
        let reference = work::lookup(index, &operation)
            .map_err(|e| native_error(e.into()))?
            .ok_or_else(|| error(ErrorKind::Missing, "retention-response-registration"))?;
        let record = work::read(index, &reference, nodes).map_err(|e| native_error(e.into()))?;
        self.check_response(&record, receipt, &evidence, &outputs, nodes)?;
        if !work::complete(index, &reference).map_err(|e| native_error(e.into()))? {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "retention-response-not-complete",
            ));
        }
        Ok(Candidate {
            schema: "reel.workflow-response-candidate.v1".into(),
            task_key: result.task_key.clone(),
            attempt_id: result.attempt_id.clone(),
            registration: evidence,
            work: reference,
            store: self.profile.scope.store.clone(),
        })
    }
    fn retained_record(
        &self,
        receipt: &Receipt,
        candidate: &Candidate,
        target: &Target,
        reference: &WorkRef,
        nodes: &mut MetadataStore,
        index: &mut Index,
    ) -> Result<work::Record> {
        let (operation, raw) = plan(&self.profile, receipt, candidate, target)?;
        let evidence = Object::from_bytes(&raw);
        let record = work::read(index, reference, nodes).map_err(|e| native_error(e.into()))?;
        let present: BTreeSet<_> = entries(&record.update.selection, nodes)?
            .into_values()
            .collect();
        let original =
            work::read(index, &candidate.work, nodes).map_err(|e| native_error(e.into()))?;
        let required: BTreeSet<_> = entries(&original.update.selection, nodes)?
            .into_values()
            .chain([original.update.selection.root])
            .collect();
        if record.evidence.as_ref() != Some(&evidence)
            || record.update.operation != operation
            || record.additional.is_some()
            || record.adoption.is_some()
            || !record.update.ready
            || record.update.actor != target.actor
            || record.update.owner != target.owner
            || record.update.selection.context != evidence.sha256
            || !present.contains(&evidence)
            || !required.is_subset(&present)
            || nodes
                .get(&evidence)
                .map_err(|_| error(ErrorKind::Integrity, "retention-evidence-read"))?
                .as_deref()
                != Some(&raw)
        {
            return Err(error(
                ErrorKind::Conflict,
                "retention-original-work-binding",
            ));
        }
        Ok(record)
    }
    pub(crate) fn retention_guard(
        self,
        receipt: &Receipt,
        result: &reel_workflow::ReturnedResult,
        target: &Target,
    ) -> Result<(ClaimsGuard, Candidate)> {
        self.check_receipt(receipt)?;
        let (mut index, mut nodes) = self.stores_with_pending(Some(receipt), true)?;
        self.retained_scope(&index, target)?;
        let fences = self.actor_fences(&index, Some(&target.actor))?;
        let candidate = self.response_candidate(receipt, result, &mut index, &mut nodes)?;
        let (operation, _) = plan(&self.profile, receipt, &candidate, target)?;
        if let Some(reference) =
            work::lookup(&mut index, &operation).map_err(|e| native_error(e.into()))?
        {
            self.retained_record(
                receipt, &candidate, target, &reference, &mut nodes, &mut index,
            )?;
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
        drop(fences);
        let guard = self.reopen_checked(receipt, Some(&target.actor))?;
        Ok((guard, candidate))
    }
    pub(crate) fn verify_retained(
        &self,
        receipt: &Receipt,
        result: &reel_workflow::ReturnedResult,
        acknowledgement: &Acknowledgement,
    ) -> Result<Fences> {
        let mut session = RetainedFences::default();
        self.verify_retained_in(receipt, result, acknowledgement, &mut session)?;
        Ok(session.locks)
    }
    pub(crate) fn verify_retained_in(
        &self,
        receipt: &Receipt,
        result: &reel_workflow::ReturnedResult,
        acknowledgement: &Acknowledgement,
        session: &mut RetainedFences,
    ) -> Result<()> {
        self.check_receipt(receipt)?;
        let (mut index, mut nodes) = self.stores(Some(receipt))?;
        self.retained_scope(&index, &acknowledgement.target)?;
        let identity = (
            index.identity().clone(),
            std::fs::canonicalize(&self.profile.scope.coordination_root)
                .map_err(crate::io_error)?,
            acknowledgement.target.actor.clone(),
        );
        if !session.identities.contains(&identity) {
            let fence = qualified::prepublication::actor_fence(
                index.identity(),
                &self.profile.scope.coordination_root,
                &acknowledgement.target.actor,
            )
            .map_err(native_error)?;
            session.locks.push(std::sync::Arc::new(fence));
            session.identities.push(identity);
        }
        let candidate = self.response_candidate(receipt, result, &mut index, &mut nodes)?;
        if candidate != acknowledgement.candidate {
            return Err(error(ErrorKind::Conflict, "retention-candidate-binding"));
        }
        self.verify_ack(receipt, acknowledgement, &mut index, &mut nodes)?;
        Ok(())
    }
    pub(super) fn verify_ack(
        &self,
        receipt: &Receipt,
        ack: &Acknowledgement,
        index: &mut Index,
        nodes: &mut MetadataStore,
    ) -> Result<()> {
        if ack.schema != "reel.workflow-retained-acknowledgement.v1" {
            return Err(error(ErrorKind::Invalid, "retention-ack-schema"));
        }
        // Both caller minima and original acknowledgement witnesses must hold.
        drop(
            Index::open(&self.profile.index_root, &ack.index_checkpoint)
                .map_err(|e| native_error(e.into()))?,
        );
        drop(
            MetadataStore::open(&self.profile.metadata_root, &ack.metadata_checkpoint)
                .map_err(|e| native_error(e.into()))?,
        );
        let stores = self.retained_scope(index, &ack.target)?;
        let (operation, raw) = plan(&self.profile, receipt, &ack.candidate, &ack.target)?;
        if ack.evidence != Object::from_bytes(&raw)
            || work::lookup(index, &operation)
                .map_err(|e| native_error(e.into()))?
                .as_ref()
                != Some(&ack.work)
            || !work::complete(index, &ack.work).map_err(|e| native_error(e.into()))?
        {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "retention-native-ack-not-complete",
            ));
        }
        let record = self.retained_record(
            receipt,
            &ack.candidate,
            &ack.target,
            &ack.work,
            nodes,
            index,
        )?;
        let current = index
            .actor(nodes, &ack.target.actor)
            .map_err(|e| native_error(e.into()))?;
        let present: BTreeSet<_> = entries(&current.selection, nodes)?.into_values().collect();
        let required: BTreeSet<_> = entries(&record.update.selection, nodes)?
            .into_values()
            .collect();
        if !current.ready || !required.is_subset(&present) {
            return Err(error(ErrorKind::Stale, "retention-current-coverage"));
        }
        for object in &required {
            for store in &stores {
                if !index
                    .inspect(nodes, object, store)
                    .map_err(|e| native_error(e.into()))?
                    .protected_by
                    .contains(&ack.target.actor)
                {
                    return Err(error(ErrorKind::NeedsRecovery, "retention-native-coverage"));
                }
            }
        }
        Ok(())
    }
}

impl ClaimsGuard {
    pub(crate) fn retain_response(
        &self,
        receipt: &Receipt,
        candidate: &Candidate,
        target: &Target,
        apply: bool,
    ) -> Result<Acknowledgement> {
        let p = &self._prepared;
        let (mut index, mut nodes) = p.stores(Some(receipt))?;
        p.retained_scope(&index, target)?;
        let (operation, raw) = plan(&p.profile, receipt, candidate, target)?;
        let evidence = Object::from_bytes(&raw);
        let source = index
            .actor(&mut nodes, &p.profile.scope.actor)
            .map_err(|e| native_error(e.into()))?;
        let source_objects: BTreeSet<_> = entries(&source.selection, &mut nodes)?
            .into_values()
            .chain([source.selection.root.clone()])
            .collect();
        let original_response =
            work::read(&mut index, &candidate.work, &nodes).map_err(|e| native_error(e.into()))?;
        let required: BTreeSet<_> = entries(&original_response.update.selection, &mut nodes)?
            .into_values()
            .chain([original_response.update.selection.root])
            .collect();
        if !source.ready || !required.is_subset(&source_objects) {
            return Err(error(
                ErrorKind::Stale,
                "retention-original-response-coverage",
            ));
        }
        let reference = if let Some(reference) =
            work::lookup(&mut index, &operation).map_err(|e| native_error(e.into()))?
        {
            p.retained_record(
                receipt, candidate, target, &reference, &mut nodes, &mut index,
            )?;
            reference
        } else {
            let retained = index
                .actor(&mut nodes, &target.actor)
                .map_err(|e| native_error(e.into()))?;
            if !retained.ready {
                return Err(error(ErrorKind::Denied, "retention-target-unready"));
            }
            let mut next = entries(&retained.selection, &mut nodes)?;
            let mut objects = source_objects;
            objects.extend([retained.selection.root.clone(), evidence.clone()]);
            objects.extend(next.values().cloned());
            let total = objects.iter().try_fold(0u64, |n, o| n.checked_add(o.bytes));
            let limits = p.projection_limits();
            if objects.len() > limits.objects || total.is_none_or(|n| n > limits.logical_bytes) {
                return Err(error(ErrorKind::Budget, "retention-resource-bound"));
            }
            nodes
                .put(&raw)
                .map_err(|_| error(ErrorKind::NeedsRecovery, "retention-evidence-write"))?;
            let mut changes = BTreeMap::new();
            for object in objects {
                // Canonical qualified closer layout for new references. Preserve
                // every existing path; legacy history is never rewritten here.
                let path = LogicalPath::parse(&format!("objects/{}", object.sha256.as_str()))
                    .map_err(|_| error(ErrorKind::Invalid, "retention-object-path"))?;
                if let Some(old) = next.get(&path) {
                    if *old != object {
                        return Err(error(ErrorKind::Conflict, "retention-object-path-conflict"));
                    }
                } else {
                    changes.insert(path.clone(), Some(object.clone()));
                    next.insert(path, object);
                }
            }
            if changes.len() > protection::MAX_CHANGES {
                return Err(error(ErrorKind::Budget, "retention-changed-path-limit"));
            }
            let tree = Tree::build(&next, &mut nodes, TreeLimits::default())
                .map_err(|_| error(ErrorKind::Budget, "retention-tree-limit"))?
                .0;
            let update = protection::Update {
                operation,
                actor: target.actor.clone(),
                owner: target.owner.clone(),
                expected_revision: retained.revision,
                selection: Selection {
                    root: tree.root().clone(),
                    context: evidence.sha256.clone(),
                },
                changes,
                ready: true,
            };
            work::stage(&mut index, &update, Some(evidence.clone()), &mut nodes)
                .map_err(|e| native_error(e.into()))?
        };
        if apply {
            work::execute(&mut index, &reference, &mut nodes)
                .map_err(|e| native_error(e.into()))?;
        }
        let ack = Acknowledgement {
            schema: "reel.workflow-retained-acknowledgement.v1".into(),
            candidate: candidate.clone(),
            target: target.clone(),
            evidence,
            work: reference,
            index_checkpoint: index.checkpoint(),
            metadata_checkpoint: nodes.finish().map_err(|e| native_error(e.into()))?,
        };
        if apply {
            p.verify_ack(receipt, &ack, &mut index, &mut nodes)?;
        }
        Ok(ack)
    }
    pub(crate) fn verify_retention(&self, receipt: &Receipt, ack: &Acknowledgement) -> Result<()> {
        let (mut index, mut nodes) = self._prepared.stores(Some(receipt))?;
        self._prepared
            .verify_ack(receipt, ack, &mut index, &mut nodes)
    }
}
