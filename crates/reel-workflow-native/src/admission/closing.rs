//! Thin adapter to unchanged qualified close work. No payload retirement.
use super::*;
use crate::{
    closing::{Action, Outcome, Staged},
    retention::{Acknowledgement, Target},
};
use reel_workflow::ReturnedResult;

fn work_key(index: &Index, scope: &Scope, operation: &Key, domain: &str) -> Result<Key> {
    // Same qualified work namespace as slots::close and acceptance_operation.
    let raw = tig_core::identity::canonical_json(
        &serde_json::to_value((index.identity(), &scope.actor, operation))
            .map_err(|_| error(ErrorKind::Invalid, "close-operation-json"))?,
    )
    .map_err(|_| error(ErrorKind::Invalid, "close-operation-json"))?;
    Key::parse(&format!("{domain}-{}", Digest::blob(&raw).as_str()))
        .map_err(|_| error(ErrorKind::Invalid, "close-operation-key"))
}
impl Prepared {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn close_retained(
        &self,
        receipt: &Receipt,
        result: &ReturnedResult,
        ack: &Acknowledgement,
        target: &Target,
        operation: &Key,
        action: Action,
    ) -> Result<Outcome> {
        self.check_receipt(receipt)?;
        let (mut index, mut nodes) = self.stores_with_pending(Some(receipt), true)?;
        self.retained_scope(&index, target)?;
        let marker = slots::activation(
            &index,
            &self.profile.scope,
            &self.input,
            &self.source_binding,
            &self.candidate,
        )
        .map_err(native_error)?;
        let acceptance = self.acceptance_operation(&index)?;
        if Object::from_bytes(&marker) != receipt.activation
            || work::lookup(&mut index, &acceptance).map_err(|e| native_error(e.into()))?
                != Some(receipt.work.clone())
            || !work::complete(&mut index, &receipt.work).map_err(|e| native_error(e.into()))?
        {
            return Err(error(ErrorKind::Conflict, "close-original-admission"));
        }
        slots::verify_activation(
            &index,
            &self.profile.scope,
            &self.input,
            &self.source_binding,
            &receipt.activation,
            &nodes,
        )
        .map_err(native_error)?;
        let handoff_key = work_key(&index, &self.profile.scope, operation, "slot-handoff")?;
        let handoff = work::lookup(&mut index, &handoff_key).map_err(|e| native_error(e.into()))?;
        if action == Action::Recover && handoff.is_none() {
            return Err(error(ErrorKind::Missing, "close-original-work-not-staged"));
        }
        let candidate = self.response_candidate(receipt, result, &mut index, &mut nodes)?;
        if candidate != ack.candidate {
            return Err(error(ErrorKind::Conflict, "close-original-candidate"));
        }
        self.verify_ack(receipt, ack, &mut index, &mut nodes)?;
        if handoff.is_none() {
            // Preflight only: the qualified function repeats all checks while
            // holding its own ordered source/retained fences before effects.
            let source = index
                .actor(&mut nodes, &self.profile.scope.actor)
                .map_err(|e| native_error(e.into()))?;
            let retained = index
                .actor(&mut nodes, &target.actor)
                .map_err(|e| native_error(e.into()))?;
            if !source.ready || !retained.ready {
                return Err(error(ErrorKind::Busy, "close-actor-not-ready"));
            }
            let source_entries = Tree::from_root(source.selection.root.clone())
                .entries(&mut nodes, TreeLimits::default())
                .map_err(|_| error(ErrorKind::Integrity, "close-source-tree"))?
                .0;
            let prior = Tree::from_root(retained.selection.root.clone())
                .entries(&mut nodes, TreeLimits::default())
                .map_err(|_| error(ErrorKind::Integrity, "close-retained-tree"))?
                .0;
            if source_entries.len() > protection::MAX_CHANGES {
                return Err(error(ErrorKind::Budget, "close-clear-path-bound"));
            }
            let objects: BTreeSet<_> = source_entries
                .values()
                .chain(prior.values())
                .cloned()
                .chain([source.selection.root, retained.selection.root])
                .collect();
            let total = objects.iter().try_fold(0u64, |n, o| n.checked_add(o.bytes));
            let limits = self.projection_limits();
            if objects.len() > limits.objects || total.is_none_or(|n| n > limits.logical_bytes) {
                return Err(error(ErrorKind::Budget, "close-resource-bound"));
            }
            let mut next = BTreeMap::new();
            for object in objects {
                let path = LogicalPath::parse(&format!("objects/{}", object.sha256.as_str()))
                    .map_err(|_| error(ErrorKind::Invalid, "close-object-path"))?;
                if next
                    .insert(path, object.clone())
                    .is_some_and(|old| old != object)
                {
                    return Err(error(ErrorKind::Conflict, "close-object-size-conflict"));
                }
            }
            let keys: BTreeSet<_> = prior.keys().chain(next.keys()).collect();
            if keys
                .iter()
                .filter(|k| prior.get(**k) != next.get(**k))
                .count()
                > protection::MAX_CHANGES
            {
                return Err(error(ErrorKind::Budget, "close-retained-layout-path-bound"));
            }
        }
        let apply = action != Action::Preview;
        let result = slots::close(
            &mut index,
            &mut nodes,
            &self.profile.scope,
            &slots::Close {
                activation: &receipt.activation,
                operation,
                retained: &target.actor,
                retained_owner: &target.owner,
                index_root: &self.profile.index_root,
                metadata_root: &self.profile.metadata_root,
            },
            self.projection_limits(),
            apply,
            |stage| {
                if action == Action::Stage && stage == slots::Stage::HandoffStaged {
                    Err(Staged.into())
                } else {
                    Ok(())
                }
            },
        );
        let metadata_checkpoint = if apply {
            nodes.finish().map_err(|e| native_error(e.into()))?
        } else {
            receipt.metadata_checkpoint.clone()
        };
        let closed = match result {
            Ok(closed) => closed,
            Err(error) if error.downcast_ref::<Staged>().is_some() => slots::Closed {
                applied: true,
                slot_idle: false,
                handoff: work::lookup(&mut index, &handoff_key)
                    .map_err(|e| native_error(e.into()))?,
                clear: None,
            },
            Err(error) => return Err(native_error(error)),
        };
        let original_clear_complete = closed
            .clear
            .as_ref()
            .map(|reference| {
                work::complete(&mut index, reference).map_err(|e| native_error(e.into()))
            })
            .transpose()?
            .unwrap_or(false);
        Ok(Outcome {
            schema: "reel.workflow-close-outcome.v1".into(),
            operation: operation.clone(),
            admission_identity: receipt.operation_identity()?,
            activation: receipt.activation.clone(),
            target: target.clone(),
            applied: closed.applied,
            slot_idle: closed.slot_idle,
            original_clear_complete,
            handoff: closed.handoff,
            clear: closed.clear,
            index_checkpoint: index.checkpoint(),
            metadata_checkpoint,
        })
    }
}
