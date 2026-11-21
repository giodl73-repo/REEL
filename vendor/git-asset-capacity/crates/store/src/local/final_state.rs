//! Final-capable profile glue; exact immutable records under existing object guards.
use super::*;
use crate::{
    final_retirement::{
        claims::Clearance,
        provider::{Intent, Outcome, Release, State, Terminal},
    },
    local::lifecycle::{Birth, digest},
};

impl LocalStore {
    pub(crate) fn load_final_state(&self, birth: &Birth) -> Result<Option<State>> {
        self.guard()?;
        let object = &birth.object;
        let p = self.life_path(object, birth.index, "final-intent");
        let r = self.life_path(object, birth.index, "final-release");
        let t = self.life_path(object, birth.index, "final-terminal");
        if !path_present(&p)? {
            if path_present(&r)? || path_present(&t)? {
                return Err(Error::CorruptRecord);
            }
            return Ok(None);
        }
        if self.final_policy.is_none() {
            return Err(Error::Unsupported);
        }
        let state = State {
            intent: read_record(&p)?,
            release: if path_present(&r)? {
                Some(read_record(&r)?)
            } else {
                None
            },
            terminal: if path_present(&t)? {
                Some(read_record(&t)?)
            } else {
                None
            },
        };
        state.validate(self, birth)?;
        Ok(Some(state))
    }
    pub(crate) fn active_final_grants(
        &self,
        object: &Object,
    ) -> Result<Vec<crate::retention::RetentionGrant>> {
        let mut result = Vec::new();
        for path in self.retention_files(object)? {
            let record: crate::retention::RetentionRecord = read_record(&path)?;
            if !self.retention_released(&record)? {
                result.push(crate::retention::RetentionGrant {
                    sha256: digest(&record)?,
                    record,
                });
            }
        }
        result.sort_by(|a, b| a.sha256.cmp(&b.sha256));
        Ok(result)
    }
    pub(crate) fn final_grant_released(
        &self,
        record: &crate::retention::RetentionRecord,
    ) -> Result<bool> {
        if self.final_policy.is_none() {
            return Ok(false);
        }
        let Some(head) = self.life_head(&record.plan.request.publication.object)? else {
            return Ok(false);
        };
        let hash = digest(record)?;
        for index in 0..=head.birth.index {
            let birth: Birth = read_record(&self.life_path(&head.birth.object, index, "birth"))?;
            if self.load_final_state(&birth)?.is_some_and(|s| {
                s.release.is_some() && s.intent.plan.grants.iter().any(|g| g.sha256 == hash)
            }) {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub(crate) fn install_final_intent(
        &self,
        intent: &Intent,
        fence: &ExclusiveObject,
    ) -> Result<()> {
        let p = &intent.plan;
        self.check_handoff_fence(&p.request.publication.object, fence)?;
        let head = self
            .life_head(&p.request.publication.object)?
            .ok_or(Error::CorruptRecord)?;
        if digest(&head.birth)? != digest(&p.birth)? || head.intent.is_some() {
            return Err(Error::OperationConflict);
        }
        intent.validate(self, &p.birth)?;
        self.write_record(
            &self.life_path(&p.birth.object, p.birth.index, "final-intent"),
            intent,
        )
    }
    pub(crate) fn write_final_release(
        &self,
        intent: &Intent,
        release: &Release,
        fence: &ExclusiveObject,
    ) -> Result<()> {
        self.check_final_head(intent, fence)?;
        let state = State {
            intent: intent.clone(),
            release: Some(release.clone()),
            terminal: None,
        };
        state.validate(self, &intent.plan.birth)?;
        let old = self
            .load_final_state(&intent.plan.birth)?
            .ok_or(Error::CorruptRecord)?;
        if old.terminal.is_some() {
            return Err(Error::OperationConflict);
        }
        self.write_record(
            &self.life_path(
                &intent.plan.birth.object,
                intent.plan.birth.index,
                "final-release",
            ),
            release,
        )
    }
    fn check_final_head(&self, intent: &Intent, fence: &ExclusiveObject) -> Result<()> {
        self.check_handoff_fence(&intent.plan.birth.object, fence)?;
        let head = self
            .life_head(&intent.plan.birth.object)?
            .ok_or(Error::CorruptRecord)?;
        if digest(&head.birth)? != digest(&intent.plan.birth)?
            || head
                .final_state
                .as_ref()
                .map(|s| digest(&s.intent))
                .transpose()?
                != Some(digest(intent)?)
        {
            return Err(Error::OperationConflict);
        }
        Ok(())
    }
    pub(crate) fn cleanup_final_grants(
        &self,
        intent: &Intent,
        fence: &ExclusiveObject,
    ) -> Result<()> {
        self.check_final_head(intent, fence)?;
        let state = self
            .load_final_state(&intent.plan.birth)?
            .ok_or(Error::CorruptRecord)?;
        if state.release.is_none() {
            return Err(Error::OperationConflict);
        }
        for grant in &intent.plan.grants {
            let path = self
                .retention_dir(&intent.plan.birth.object)
                .join(format!("{}.json", grant.request().pin_id()?.as_str()));
            if path_present(&path)? {
                let record: crate::retention::RetentionRecord = read_record(&path)?;
                if digest(&record)? != grant.sha256 {
                    return Err(Error::CorruptRecord);
                }
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }
    pub(crate) fn verify_final_source(
        &self,
        intent: &Intent,
        fence: &ExclusiveObject,
    ) -> Result<u64> {
        self.verify_bound_source(
            &intent.plan.request.publication,
            &intent.plan.birth,
            fence,
            true,
        )
    }
    pub(crate) fn verify_final_plan_source(
        &self,
        publication: &Request,
        birth: &Birth,
        fence: &ExclusiveObject,
    ) -> Result<u64> {
        self.verify_bound_source(publication, birth, fence, true)
    }
    pub(crate) fn unlink_final(
        &self,
        intent: &Intent,
        fence: &ExclusiveObject,
        clearance: &Clearance<'_>,
    ) -> Result<()> {
        self.check_final_head(intent, fence)?;
        if clearance.object() != &intent.plan.birth.object
            || clearance.scope() != &intent.plan.policy.scope
        {
            return Err(Error::OperationConflict);
        }
        clearance
            .check_current()
            .map_err(|_| Error::NeedsRecovery)?;
        let state = self
            .load_final_state(&intent.plan.birth)?
            .ok_or(Error::CorruptRecord)?;
        if state.release.is_none() || state.terminal.is_some() {
            return Err(Error::OperationConflict);
        }
        self.check_unretained(&intent.plan.birth.object)?;
        let path = self.object_path(&intent.plan.birth.object);
        regular(&path)?;
        fs::remove_file(path)?;
        Ok(())
    }
    pub(crate) fn write_final_terminal(
        &self,
        intent: &Intent,
        terminal: &Terminal,
        fence: &ExclusiveObject,
    ) -> Result<()> {
        self.check_final_head(intent, fence)?;
        let mut state = self
            .load_final_state(&intent.plan.birth)?
            .ok_or(Error::CorruptRecord)?;
        state.terminal = Some(terminal.clone());
        state.validate(self, &intent.plan.birth)?;
        if (terminal.outcome == Outcome::Deleted)
            == self.object_present(&intent.plan.birth.object)?
        {
            return Err(Error::Integrity);
        }
        self.write_record(
            &self.life_path(
                &intent.plan.birth.object,
                intent.plan.birth.index,
                "final-terminal",
            ),
            terminal,
        )
    }
    pub(crate) fn continue_final_cancel(
        &self,
        intent: &Intent,
        fence: &ExclusiveObject,
    ) -> Result<()> {
        self.check_handoff_fence(&intent.plan.birth.object, fence)?;
        let state = self
            .load_final_state(&intent.plan.birth)?
            .ok_or(Error::CorruptRecord)?;
        if !state.cancelled() {
            return Err(Error::OperationConflict);
        }
        let head = self
            .life_head(&intent.plan.birth.object)?
            .ok_or(Error::CorruptRecord)?;
        if digest(&head.birth)? != digest(&intent.plan.birth)? {
            return Ok(());
        }
        self.verify_final_source(intent, fence)?;
        let birth = self.next_final_birth(intent, &state, None, None)?;
        self.install_birth(&birth)
    }
    pub(crate) fn next_final_birth(
        &self,
        intent: &Intent,
        state: &State,
        reacquire: Option<Request>,
        context: Option<Digest>,
    ) -> Result<Birth> {
        let terminal = state.terminal.as_ref().ok_or(Error::CorruptRecord)?;
        if state.retired() != reacquire.is_some()
            || state.retired() != context.is_some()
            || intent.plan.birth.index + 1 >= lifecycle::MAX_BIRTHS
        {
            return Err(Error::OperationConflict);
        }
        let previous = digest(terminal)?;
        let placement = if let Some(request) = &reacquire {
            digest(&(
                "final-reacquired-placement",
                &self.instance,
                &intent.plan.birth.object,
                &previous,
                request,
                context.as_ref().ok_or(Error::OperationConflict)?,
            ))?
        } else {
            intent.plan.birth.placement.clone()
        };
        Ok(Birth {
            schema: lifecycle::SCHEMA.into(),
            store: self.instance.clone(),
            object: intent.plan.birth.object.clone(),
            index: intent.plan.birth.index + 1,
            previous: Some(previous),
            reacquire,
            placement,
            final_context: context,
        })
    }
    pub(crate) fn begin_final_reacquisition(
        &self,
        intent: &Intent,
        birth: &Birth,
        fence: &ExclusiveObject,
    ) -> Result<()> {
        self.check_handoff_fence(&birth.object, fence)?;
        let head = self.life_head(&birth.object)?.ok_or(Error::CorruptRecord)?;
        if digest(&head.birth)? == digest(birth)? {
            return Ok(());
        }
        if digest(&head.birth)? != digest(&intent.plan.birth)?
            || self.object_present(&birth.object)?
        {
            return Err(Error::OperationConflict);
        }
        let state = self
            .load_final_state(&intent.plan.birth)?
            .ok_or(Error::CorruptRecord)?;
        let expected = self.next_final_birth(
            intent,
            &state,
            birth.reacquire.clone(),
            birth.final_context.clone(),
        )?;
        if digest(&expected)? != digest(birth)? {
            return Err(Error::OperationConflict);
        }
        let request = birth.reacquire.as_ref().ok_or(Error::OperationConflict)?;
        if path_present(&self.op_path(&request.operation, "intent"))?
            || path_present(&self.op_path(&request.operation, "done"))?
        {
            return Err(Error::OperationConflict);
        }
        self.install_birth(birth)
    }
}
