use super::*;
use crate::{
    AckLevel,
    accounted::{self, PublicationPlan},
    ledger::{CapacityLedger, JournalRef},
    local::{LocalAttempt, LocalStore, MAX_SCOPE_FILES},
};
use std::io::Cursor;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Bindings {
    pub metadata: Digest,
    pub witness_job: JournalRef,
    pub action_jobs: Vec<JournalRef>,
}
#[derive(Serialize)]
pub(super) struct Intent<'a> {
    pub schema: &'static str,
    pub plan: &'a Plan,
    pub bindings: &'a Bindings,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Outcome {
    Completed,
    Cancelled,
}
#[derive(Serialize)]
pub(super) struct Terminal {
    pub schema: &'static str,
    pub plan: Digest,
    pub bindings: Digest,
    pub outcome: Outcome,
}
struct Document {
    request: Publication,
    plan: PublicationPlan,
    bytes: Vec<u8>,
}
pub(crate) struct Witness {
    documents: [Document; 3],
    control: Publication,
    control_binding: Digest,
    control_limit: u64,
    settle: Key,
    pub peak: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct State {
    pub admitted: bool,
    pub intent: bool,
    pub terminal: Option<bool>,
}
/// Shared fixed accounted records, not another journal/provider. Callers must
/// independently validate their exact enrollment and document semantics.
pub(crate) struct Layout {
    pub owner: Key,
    pub key: Digest,
    pub plan: Digest,
    pub control_binding: Digest,
    pub control_object: Object,
    pub documents: [Vec<u8>; 3],
    pub prefix: &'static str,
    pub max_metadata_bytes: u64,
}
impl Witness {
    pub(super) fn new(plan: &Plan, bindings: &Bindings, store: &LocalStore) -> Result<Self> {
        plan.id()?;
        if store.supports_retirement()
            || Place::local(store, plan.enrollment.witness.generation)
                .map_err(|_| Error::Authority)?
                != plan.enrollment.witness
            || bindings.action_jobs.len() != plan.actions.len()
        {
            return Err(Error::Authority);
        }
        let id = plan.id()?;
        let key = digest(&(
            "tig.bulk-final-key.design-1",
            &plan.enrollment.authority,
            &plan.enrollment.epoch,
            &plan.enrollment.owner,
            &plan.request.operation,
        ))?;
        let intent = Intent {
            schema: "tig.bulk-final-intent.design-1",
            plan,
            bindings,
        };
        let bindings_id = digest(bindings)?;
        let terminal = |outcome| Terminal {
            schema: "tig.bulk-final-terminal.design-1",
            plan: id.clone(),
            bindings: bindings_id.clone(),
            outcome,
        };
        let data = [
            encode(&intent)?,
            encode(&terminal(Outcome::Cancelled))?,
            encode(&terminal(Outcome::Completed))?,
        ];
        Self::from_records(
            Layout {
                owner: plan.enrollment.owner.clone(),
                key,
                plan: id.clone(),
                control_binding: digest(&("tig.bulk-final-control.design-1", id, bindings))?,
                control_object: Object::from_bytes(&plan.to_json()?),
                documents: data,
                prefix: "bulk",
                max_metadata_bytes: plan.enrollment.max_metadata_bytes,
            },
            store,
        )
    }
    pub(crate) fn from_records(layout: Layout, store: &LocalStore) -> Result<Self> {
        if store.supports_retirement() || layout.documents.iter().any(|b| b.len() > 65536) {
            return Err(Error::Authority);
        }
        let mut docs = Vec::new();
        let mut peak = 0u64;
        for (index, bytes) in layout.documents.into_iter().enumerate() {
            let request = Publication {
                operation: Key::parse(&format!(
                    "{}-{index}-{}",
                    layout.prefix,
                    layout.key.as_str()
                ))?,
                owner: layout.owner.clone(),
                object: Object::from_bytes(&bytes),
                required: AckLevel::ProcessRestart,
            };
            let pp = accounted::plan_bound(store, &request, layout.plan.clone())?;
            peak = peak.checked_add(pp.required_peak()).ok_or(Error::Budget)?;
            docs.push(Document {
                request,
                plan: pp,
                bytes,
            });
        }
        let q = store.allocation_quantum()?;
        let control_limit = 65_536u64
            .checked_add(q - 1)
            .and_then(|v| v.checked_div(q))
            .and_then(|v| v.checked_mul(q))
            .and_then(|v| v.checked_mul((MAX_SCOPE_FILES + 4) as u64))
            .ok_or(Error::Budget)?;
        peak = peak.checked_add(control_limit).ok_or(Error::Budget)?;
        if peak > layout.max_metadata_bytes {
            return Err(Error::Budget);
        }
        Ok(Self {
            documents: docs.try_into().map_err(|_| Error::Invalid)?,
            control: Publication {
                operation: Key::parse(&format!(
                    "{}-control-{}",
                    layout.prefix,
                    layout.key.as_str()
                ))?,
                owner: layout.owner,
                object: layout.control_object,
                required: AckLevel::ProcessRestart,
            },
            control_binding: layout.control_binding,
            control_limit,
            settle: Key::parse(&format!(
                "{}-control-settle-{}",
                layout.prefix,
                layout.key.as_str()
            ))?,
            peak,
        })
    }
    fn evidence(
        &self,
        index: usize,
        store: &LocalStore,
        ledger: &mut CapacityLedger,
        job: &JournalRef,
    ) -> Result<(bool, bool, bool)> {
        let doc = &self.documents[index];
        let mut recorded = store.optional_completed_receipt(&doc.request)?.is_some();
        let id = digest(&(store.identity(), &doc.request.operation))?;
        let allocation = ledger
            .allocation_reference(&Key::parse(&format!("publish-{}", id.as_str()))?, false)?;
        let (settled, discarded) = match &allocation {
            Some(a) => match ledger.publication_status(a, &digest(&(&doc.plan, job))?) {
                Ok(value) => (value.is_some(), false),
                Err(crate::ledger::LedgerError::Cancelled) => {
                    if ledger
                        .cleanup_status(a, &digest(&(&doc.plan, job))?)?
                        .is_none()
                    {
                        return Err(Error::Recovery);
                    }
                    (false, true)
                }
                Err(e) => return Err(e.into()),
            },
            None => (false, false),
        };
        if settled && !recorded {
            recorded = store.optional_completed_receipt(&doc.request)?.is_some();
        }
        if (settled && !recorded) || (recorded && (allocation.is_none() || discarded)) {
            return Err(Error::Recovery);
        }
        Ok((recorded, settled, discarded))
    }
    pub fn state(
        &self,
        store: &LocalStore,
        ledger: &mut CapacityLedger,
        job: &JournalRef,
    ) -> Result<State> {
        let admitted = match ledger.allocation_reference(&self.control.operation, true)? {
            Some(a) => {
                ledger.retention_status(&a, &self.control_binding)?;
                true
            }
            None => false,
        };
        let (cancelled, _, cancel_discarded) = self.evidence(1, store, ledger, job)?;
        let (completed, _, complete_discarded) = self.evidence(2, store, ledger, job)?;
        let (intent, _, intent_discarded) = self.evidence(0, store, ledger, job)?;
        if (cancelled && completed)
            || ((cancelled || completed) && !intent)
            || (intent && !admitted)
            || intent_discarded
            || (cancel_discarded && !completed)
            || (complete_discarded && !cancelled)
        {
            return Err(Error::Recovery);
        }
        Ok(State {
            admitted,
            intent,
            terminal: if cancelled {
                Some(true)
            } else if completed {
                Some(false)
            } else {
                None
            },
        })
    }
    /// All fixed recovery documents are admitted/bound before intent or source IO.
    pub fn admit(
        &self,
        store: &LocalStore,
        ledger: &mut CapacityLedger,
        job: &JournalRef,
    ) -> Result<LocalAttempt> {
        let allocation = ledger
            .begin_retention(
                self.control.operation.clone(),
                job,
                self.control.owner.clone(),
                self.control_binding.clone(),
                self.control_limit,
            )?
            .reference()
            .clone();
        let bound = (&self.control_binding, job, &allocation);
        let settled = ledger.retention_status(&allocation, &self.control_binding)?;
        let attempt = if settled.is_some() {
            store.existing_attempt(&self.control)?
        } else {
            store.attempt(&self.control)?
        };
        if settled.is_some() {
            attempt.check_binding(&bound)?;
        } else {
            attempt.bind(&bound)?;
        }
        if attempt.has_partials()? {
            return Err(Error::Recovery);
        }
        let charged = match settled {
            Some(n) => n,
            None => attempt.retained_metadata(&self.control)?,
        };
        if charged > self.control_limit {
            return Err(Error::Budget);
        }
        ledger.settle_retention(
            self.settle.clone(),
            &allocation,
            self.control_binding.clone(),
            charged,
        )?;
        let state = self.state(store, ledger, job)?;
        for (index, doc) in self.documents.iter().enumerate() {
            if state
                .terminal
                .is_some_and(|cancelled| index == if cancelled { 2 } else { 1 })
            {
                continue;
            }
            let a = accounted::admit(ledger, store, job, &doc.plan)?;
            if ledger
                .publication_status(&a, &digest(&(&doc.plan, job))?)?
                .is_none()
            {
                let a = accounted::prepare(ledger, store, job, &doc.plan, &a)?;
                if a.has_partials()? {
                    return Err(Error::Recovery);
                }
            }
        }
        Ok(attempt)
    }
    /// Explicitly retire the never-published alternate terminal reservation.
    /// The reviewed private-file set is empty; any unexpected bytes hold.
    pub fn close_unused(
        &self,
        store: &LocalStore,
        ledger: &mut CapacityLedger,
        job: &JournalRef,
        cancelled: bool,
    ) -> Result<()> {
        let state = self.state(store, ledger, job)?;
        if state.terminal != Some(cancelled) {
            return Err(Error::Recovery);
        }
        let doc = &self.documents[if cancelled { 2 } else { 1 }];
        let id = digest(&(store.identity(), &doc.request.operation))?;
        let allocation = ledger
            .allocation_reference(&Key::parse(&format!("publish-{}", id.as_str()))?, false)?
            .ok_or(Error::Recovery)?;
        let plan = accounted::cleanup::CleanupPlan::from_json(&encode(&serde_json::json!({
            "schema":"tig.private-cleanup.design-1", "publication":doc.plan,
            "job":job, "allocation":allocation, "files":[]
        }))?)
        .map_err(|_| Error::Recovery)?;
        accounted::cleanup::apply(
            ledger,
            store,
            &plan,
            &plan.authorize_discard().map_err(|_| Error::Recovery)?,
        )
        .map_err(|_| Error::Recovery)?;
        Ok(())
    }
    pub fn publish(
        &self,
        index: usize,
        store: &LocalStore,
        ledger: &mut CapacityLedger,
        job: &JournalRef,
    ) -> Result<()> {
        let doc = &self.documents[index];
        accounted::publish_accounted(ledger, store, job, &doc.plan, &mut Cursor::new(&doc.bytes))?;
        Ok(())
    }
}
