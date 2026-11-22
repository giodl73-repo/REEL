//! Explicit bounded preparation for a legacy retained tree's close layout.
//! Add canonical references before removing aliases; payloads/history stay held.
use super::*;
use crate::{closing::LayoutOutcome, retention::Acknowledgement};

fn canonical(
    prior: &BTreeMap<LogicalPath, Object>,
    root: &Object,
    cleanup: bool,
) -> Result<BTreeMap<LogicalPath, Object>> {
    let objects: BTreeSet<_> = prior.values().cloned().chain([root.clone()]).collect();
    if objects.len() > 512
        || objects
            .iter()
            .try_fold(0u64, |n, o| n.checked_add(o.bytes))
            .is_none_or(|n| n > limits().logical_bytes)
    {
        return Err(error(ErrorKind::Budget, "close-layout-resource-bound"));
    }
    let mut next = if cleanup {
        BTreeMap::new()
    } else {
        prior.clone()
    };
    for object in objects {
        let path = LogicalPath::parse(&format!("objects/{}", object.sha256.as_str()))
            .map_err(|_| error(ErrorKind::Invalid, "close-layout-object-path"))?;
        if next
            .insert(path, object.clone())
            .is_some_and(|old| old != object)
        {
            return Err(error(ErrorKind::Conflict, "close-layout-object-size"));
        }
    }
    Ok(next)
}
fn changes(
    prior: &BTreeMap<LogicalPath, Object>,
    next: &BTreeMap<LogicalPath, Object>,
) -> Result<BTreeMap<LogicalPath, Option<Object>>> {
    let keys: BTreeSet<_> = prior.keys().chain(next.keys()).cloned().collect();
    let changes: BTreeMap<_, _> = keys
        .into_iter()
        .filter(|k| prior.get(k) != next.get(k))
        .map(|k| {
            let value = next.get(&k).cloned();
            (k, value)
        })
        .collect();
    if changes.len() > protection::MAX_CHANGES {
        return Err(error(ErrorKind::Budget, "close-layout-phase-path-bound"));
    }
    Ok(changes)
}
#[allow(clippy::too_many_arguments)]
fn phase(
    prepared: &Prepared,
    receipt: &Receipt,
    ack: &Acknowledgement,
    operation: &Key,
    cleanup: bool,
    index: &mut Index,
    nodes: &mut MetadataStore,
) -> Result<(WorkRef, usize)> {
    let kind = if cleanup { "cleanup" } else { "add" };
    let raw = encode(
        &serde_json::json!({"schema":"reel.workflow-close-layout.v1", "index":prepared.profile.index_identity,
        "admission":receipt.operation_identity()?, "retained":ack.artifact()?,"operation":operation,"phase":kind}),
    )?;
    let evidence = Object::from_bytes(&raw);
    let key = Key::parse(&format!("close-layout-{}", Digest::blob(&raw).as_str()))
        .map_err(|_| error(ErrorKind::Invalid, "close-layout-operation"))?;
    let reference = if let Some(reference) =
        work::lookup(index, &key).map_err(|e| native_error(e.into()))?
    {
        let record = work::read(index, &reference, nodes).map_err(|e| native_error(e.into()))?;
        if record.evidence.as_ref() != Some(&evidence)
            || record.update.actor != ack.target.actor
            || record.update.owner != ack.target.owner
            || !record.update.ready
            || record.additional.is_some()
            || record.adoption.is_some()
        {
            return Err(error(ErrorKind::Conflict, "close-layout-original-work"));
        }
        reference
    } else {
        let retained = index
            .actor(nodes, &ack.target.actor)
            .map_err(|e| native_error(e.into()))?;
        if !retained.ready {
            return Err(error(ErrorKind::Busy, "close-layout-target-unready"));
        }
        let prior = Tree::from_root(retained.selection.root.clone())
            .entries(nodes, TreeLimits::default())
            .map_err(|_| error(ErrorKind::Integrity, "close-layout-prior"))?
            .0;
        let next = canonical(&prior, &retained.selection.root, cleanup)?;
        let changes = changes(&prior, &next)?;
        nodes
            .put(&raw)
            .map_err(|_| error(ErrorKind::NeedsRecovery, "close-layout-evidence-write"))?;
        let tree = Tree::build(&next, nodes, TreeLimits::default())
            .map_err(|_| error(ErrorKind::Budget, "close-layout-tree"))?
            .0;
        work::stage(
            index,
            &protection::Update {
                operation: key,
                actor: ack.target.actor.clone(),
                owner: ack.target.owner.clone(),
                expected_revision: retained.revision,
                selection: Selection {
                    root: tree.root().clone(),
                    context: evidence.sha256.clone(),
                },
                changes,
                ready: true,
            },
            Some(evidence),
            nodes,
        )
        .map_err(|e| native_error(e.into()))?
    };
    if !work::complete(index, &reference).map_err(|e| native_error(e.into()))?
        && Index::observe(&prepared.profile.index_root, &index.checkpoint(), nodes)
            .map_err(|e| native_error(e.into()))?
            .pending
    {
        work::resume_admitted(index, &reference, nodes).map_err(|e| native_error(e.into()))?;
    } else {
        work::execute(index, &reference, nodes).map_err(|e| native_error(e.into()))?;
    }
    let record = work::read(index, &reference, nodes).map_err(|e| native_error(e.into()))?;
    Ok((reference, record.update.changes.len()))
}
impl Prepared {
    pub(crate) fn prepare_close_layout(
        &self,
        receipt: &Receipt,
        result: &reel_workflow::ReturnedResult,
        ack: &Acknowledgement,
        operation: &Key,
        apply: bool,
    ) -> Result<LayoutOutcome> {
        self.check_receipt(receipt)?;
        let (mut index, mut nodes) = self.stores_with_pending(Some(receipt), true)?;
        self.retained_scope(&index, &ack.target)?;
        let _fences = self.actor_fences(&index, Some(&ack.target.actor))?;
        let candidate = self.response_candidate(receipt, result, &mut index, &mut nodes)?;
        if candidate != ack.candidate {
            return Err(error(ErrorKind::Conflict, "close-layout-candidate"));
        }
        self.verify_ack(receipt, ack, &mut index, &mut nodes)?;
        if !apply {
            let retained = index
                .actor(&mut nodes, &ack.target.actor)
                .map_err(|e| native_error(e.into()))?;
            let prior = Tree::from_root(retained.selection.root.clone())
                .entries(&mut nodes, TreeLimits::default())
                .map_err(|_| error(ErrorKind::Integrity, "close-layout-prior"))?
                .0;
            let added = canonical(&prior, &retained.selection.root, false)?;
            let add_changes = changes(&prior, &added)?.len();
            let mut memory = MemoryNodes::default();
            let add_root = Tree::build(&added, &mut memory, TreeLimits::default())
                .map_err(|_| error(ErrorKind::Budget, "close-layout-preview"))?
                .0
                .root()
                .clone();
            let next = canonical(&added, &add_root, true)?;
            return Ok(LayoutOutcome {
                schema: "reel.workflow-close-layout-outcome.v1".into(),
                operation: operation.clone(),
                add_changes,
                cleanup_changes: changes(&added, &next)?.len(),
                applied: false,
                add: None,
                cleanup: None,
            });
        }
        let (add, add_changes) =
            phase(self, receipt, ack, operation, false, &mut index, &mut nodes)?;
        let (cleanup, cleanup_changes) =
            phase(self, receipt, ack, operation, true, &mut index, &mut nodes)?;
        self.verify_ack(receipt, ack, &mut index, &mut nodes)?;
        Ok(LayoutOutcome {
            schema: "reel.workflow-close-layout-outcome.v1".into(),
            operation: operation.clone(),
            add_changes,
            cleanup_changes,
            applied: true,
            add: Some(add),
            cleanup: Some(cleanup),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_aliases_use_two_bounded_phases_without_losing_any_object() {
        let prior: BTreeMap<_, _> = (0..70)
            .map(|n| {
                (
                    LogicalPath::parse(&format!("workflow/legacy/{n}")).unwrap(),
                    Object::from_bytes(format!("object-{n}").as_bytes()),
                )
            })
            .collect();
        let mut nodes = MemoryNodes::default();
        let original_root = Tree::build(&prior, &mut nodes, TreeLimits::default())
            .unwrap()
            .0
            .root()
            .clone();
        let direct = canonical(&prior, &original_root, true).unwrap();
        assert_eq!(
            changes(&prior, &direct).unwrap_err().kind,
            ErrorKind::Budget
        );
        let added = canonical(&prior, &original_root, false).unwrap();
        assert!(changes(&prior, &added).unwrap().len() <= 128);
        let add_root = Tree::build(&added, &mut nodes, TreeLimits::default())
            .unwrap()
            .0
            .root()
            .clone();
        let clean = canonical(&added, &add_root, true).unwrap();
        assert!(changes(&added, &clean).unwrap().len() <= 128);
        let held: BTreeSet<_> = clean.values().cloned().collect();
        assert!(prior.values().all(|o| held.contains(o)));
        assert!(held.contains(&original_root) && held.contains(&add_root));
    }
}
