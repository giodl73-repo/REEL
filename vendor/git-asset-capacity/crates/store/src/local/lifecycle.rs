//! Bounded immutable per-object incarnation chain, accessed under provider guards.
use super::*;
use crate::retirement::{Completion, Intent};
use std::collections::BTreeSet;

pub(crate) const SCHEMA: &str = "tig.local-lifecycle.design-1";
pub(crate) const MAX_BIRTHS: usize = 16;
pub(crate) const MAX_FILES: usize = MAX_BIRTHS * 4 + 1;
pub(crate) const MAX_FINAL_FILES: usize = MAX_BIRTHS * 7 + 1;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Birth {
    pub(super) schema: String,
    pub(crate) store: Digest,
    pub(crate) object: Object,
    pub index: usize,
    pub(super) previous: Option<Digest>,
    pub reacquire: Option<Request>,
    pub placement: Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) final_context: Option<Digest>,
}
#[derive(Clone)]
pub(crate) struct Head {
    pub birth: Birth,
    pub intent: Option<Intent>,
    pub done: Option<Completion>,
    pub aborted: Option<Completion>,
    pub final_state: Option<crate::final_retirement::provider::State>,
}
impl Head {
    pub(crate) fn retired(&self) -> bool {
        self.done.is_some() || self.final_state.as_ref().is_some_and(|s| s.retired())
    }
}
pub(crate) fn digest(value: &impl Serialize) -> Result<Digest> {
    Ok(Digest::blob(&encode(value)?))
}
pub(crate) fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let raw =
        identity::canonical_json(&serde_json::to_value(value).map_err(|_| Error::CorruptRecord)?)
            .map_err(|_| Error::CorruptRecord)?;
    if raw.len() > 65536 {
        return Err(Error::Budget);
    }
    Ok(raw)
}
struct MetadataLock(File);
impl Drop for MetadataLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
impl LocalStore {
    pub(crate) fn bootstrap_incarnation(&self, object: &Object) -> Result<()> {
        if self.object_present(object)? {
            return Err(Error::CorruptRecord);
        }
        self.ensure_incarnation(&Request {
            operation: Key::parse("bootstrap")?,
            owner: Key::parse("bootstrap")?,
            object: object.clone(),
            required: AckLevel::ProcessRestart,
        })?;
        Ok(())
    }
    pub(crate) fn has_lifecycle_metadata(&self, object: &Object) -> Result<bool> {
        Ok(path_present(&self.life_dir(object))?
            || path_present(
                &self
                    .root
                    .join("fences")
                    .join(format!("lifecycle-{}", object.sha256.as_str())),
            )?)
    }
    pub(crate) fn require_accounted_managed(&self) -> Result<()> {
        if self.managed && self.staging.is_none() {
            return Err(Error::Unsupported);
        }
        Ok(())
    }
    pub(super) fn life_dir(&self, object: &Object) -> PathBuf {
        self.root.join("lifecycle").join(object.sha256.as_str())
    }
    pub(super) fn life_path(&self, object: &Object, index: usize, kind: &str) -> PathBuf {
        self.life_dir(object)
            .join(format!("{index:02x}.{kind}.json"))
    }
    pub(crate) fn lifecycle_files(&self, object: &Object) -> Result<Vec<PathBuf>> {
        self.guard()?;
        if !self.managed {
            return Err(Error::Unsupported);
        }
        let dir = self.life_dir(object);
        if !path_present(&dir)? {
            if path_present(
                &self
                    .root
                    .join("fences")
                    .join(format!("lifecycle-{}", object.sha256.as_str())),
            )? {
                return Err(Error::CorruptRecord);
            }
            return Ok(Vec::new());
        }
        directory(&dir)?;
        let mut result = Vec::new();
        for entry in fs::read_dir(dir)? {
            if result.len()
                >= (if self.final_policy.is_some() {
                    MAX_FINAL_FILES
                } else {
                    MAX_FILES
                }) - 1
            {
                return Err(Error::Budget);
            }
            let path = entry?.path();
            regular(&path)?;
            result.push(path);
        }
        let anchor = self
            .root
            .join("fences")
            .join(format!("lifecycle-{}", object.sha256.as_str()));
        if path_present(&anchor)? {
            regular(&anchor)?;
            result.push(anchor);
        } else {
            return Err(Error::CorruptRecord);
        }
        Ok(result)
    }
    pub(crate) fn life_head(&self, object: &Object) -> Result<Option<Head>> {
        let files = self.lifecycle_files(object)?;
        let mut remaining = files
            .into_iter()
            .filter(|p| p.parent() == Some(self.life_dir(object).as_path()))
            .collect::<BTreeSet<_>>();
        let mut last: Option<Head> = None;
        for index in 0..MAX_BIRTHS {
            let path = self.life_path(object, index, "birth");
            if !remaining.remove(&path) {
                break;
            }
            let birth: Birth = read_record(&path)?;
            let (expected_previous, expected_placement, expected_reacquire) = match &last {
                Some(head) => {
                    if let Some(final_state) = &head.final_state {
                        let terminal = final_state.terminal.as_ref().ok_or(Error::CorruptRecord)?;
                        if final_state.cancelled() {
                            (Some(digest(terminal)?), head.birth.placement.clone(), false)
                        } else {
                            let previous = digest(terminal)?;
                            let request = birth.reacquire.as_ref().ok_or(Error::CorruptRecord)?;
                            (
                                Some(previous.clone()),
                                digest(&(
                                    "final-reacquired-placement",
                                    &self.instance,
                                    object,
                                    &previous,
                                    request,
                                    birth.final_context.as_ref().ok_or(Error::CorruptRecord)?,
                                ))?,
                                true,
                            )
                        }
                    } else if let Some(aborted) = &head.aborted {
                        (Some(digest(aborted)?), head.birth.placement.clone(), false)
                    } else {
                        let previous = digest(head.done.as_ref().ok_or(Error::CorruptRecord)?)?;
                        let request = birth.reacquire.as_ref().ok_or(Error::CorruptRecord)?;
                        (
                            Some(previous.clone()),
                            digest(&(
                                "reacquired-placement",
                                &self.instance,
                                object,
                                &previous,
                                request,
                            ))?,
                            true,
                        )
                    }
                }
                None => (
                    None,
                    digest(&("initial-placement", &self.instance, object))?,
                    false,
                ),
            };
            if birth.schema != SCHEMA
                || birth.final_context.is_some()
                    != last
                        .as_ref()
                        .is_some_and(|h| h.final_state.as_ref().is_some_and(|s| s.retired()))
                || birth.store != self.instance
                || birth.object != *object
                || birth.index != index
                || birth.previous != expected_previous
                || birth.reacquire.is_some() != expected_reacquire
                || birth.placement != expected_placement
                || birth
                    .reacquire
                    .as_ref()
                    .is_some_and(|r| r.object != *object || r.required > AckLevel::ProcessRestart)
            {
                return Err(Error::CorruptRecord);
            }
            let intent_path = self.life_path(object, index, "intent");
            let intent = if remaining.remove(&intent_path) {
                let intent: Intent = read_record(&intent_path)?;
                intent.validate(self, &birth)?;
                Some(intent)
            } else {
                None
            };
            let done_path = self.life_path(object, index, "done");
            let done = if remaining.remove(&done_path) {
                let done: Completion = read_record(&done_path)?;
                done.validate(intent.as_ref().ok_or(Error::CorruptRecord)?)?;
                if done.cancelled {
                    return Err(Error::CorruptRecord);
                }
                Some(done)
            } else {
                None
            };
            let abort_path = self.life_path(object, index, "abort");
            let aborted = if remaining.remove(&abort_path) {
                let aborted: Completion = read_record(&abort_path)?;
                aborted.validate(intent.as_ref().ok_or(Error::CorruptRecord)?)?;
                if !aborted.cancelled || done.is_some() {
                    return Err(Error::CorruptRecord);
                }
                Some(aborted)
            } else {
                None
            };
            let final_state = if self.final_policy.is_some() {
                self.load_final_state(&birth)?
            } else {
                None
            };
            if self.final_policy.is_some() {
                for kind in ["final-intent", "final-release", "final-terminal"] {
                    remaining.remove(&self.life_path(object, index, kind));
                }
            }
            if final_state.is_some() && intent.is_some() {
                return Err(Error::CorruptRecord);
            }
            last = Some(Head {
                birth,
                intent,
                done,
                aborted,
                final_state,
            });
        }
        if !remaining.is_empty() || (last.is_none() && path_present(&self.life_dir(object))?) {
            return Err(Error::CorruptRecord);
        }
        Ok(last)
    }
    pub(crate) fn lifecycle_readable(&self, object: &Object) -> Result<()> {
        let Some(head) = self.life_head(object)? else {
            return Err(Error::ReacquisitionRequired);
        };
        if head.retired() {
            return Err(Error::AuthorizedAbsence);
        }
        if (head.intent.is_some() || head.final_state.is_some()) && !self.object_present(object)? {
            return Err(Error::NeedsRecovery);
        }
        Ok(())
    }
    /// Original birth of the current physical placement, preserving publication
    /// identity across cancellation-only action slots. No census or mutation.
    pub(crate) fn placement_origin(&self, object: &Object) -> Result<Option<Birth>> {
        let Some(head) = self.life_head(object)? else {
            return Ok(None);
        };
        let mut origin = head.birth.clone();
        for index in (0..=head.birth.index).rev() {
            let birth: Birth = read_record(&self.life_path(object, index, "birth"))?;
            if birth.placement != head.birth.placement {
                break;
            }
            origin = birth;
            if origin.reacquire.is_some() {
                break;
            }
        }
        Ok(Some(origin))
    }
    pub(crate) fn check_incarnation(
        &self,
        request: &Request,
        incarnation: Option<&Digest>,
    ) -> Result<()> {
        if !self.managed {
            return if incarnation.is_none() {
                Ok(())
            } else {
                Err(Error::CorruptRecord)
            };
        }
        let wanted = incarnation.ok_or(Error::CorruptRecord)?;
        let head = self
            .life_head(&request.object)?
            .ok_or(Error::CorruptRecord)?;
        if head.birth.placement == *wanted {
            if head.retired() {
                return Err(Error::AuthorizedAbsence);
            }
            if head.final_state.is_some() || head.intent.is_some() && head.aborted.is_none() {
                return Err(Error::NeedsRecovery);
            }
            return Ok(());
        }
        // Only actual exact retirement evidence can explain an old incarnation.
        for index in 0..head.birth.index {
            let birth: Birth = read_record(&self.life_path(&request.object, index, "birth"))?;
            if birth.placement == *wanted
                && self.load_final_state(&birth)?.is_some_and(|s| s.retired())
            {
                return Err(Error::AuthorizedAbsence);
            }
            if birth.placement == *wanted
                && path_present(&self.life_path(&request.object, index, "done"))?
            {
                let intent: Intent =
                    read_record(&self.life_path(&request.object, index, "intent"))?;
                let done: Completion =
                    read_record(&self.life_path(&request.object, index, "done"))?;
                intent.validate(self, &birth)?;
                done.validate(&intent)?;
                return Err(Error::AuthorizedAbsence);
            }
        }
        Err(Error::CorruptRecord)
    }
    pub(crate) fn ensure_incarnation(&self, request: &Request) -> Result<Option<Digest>> {
        if !self.managed {
            return Ok(None);
        }
        self.require_accounted_managed()?;
        let path = self
            .root
            .join("fences")
            .join(format!("lifecycle-{}", request.object.sha256.as_str()));
        if path_present(&path)? {
            regular(&path)?;
        } else if path_present(&self.life_dir(&request.object))? {
            return Err(Error::CorruptRecord);
        }
        let (file, created) = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => (file, true),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (
                OpenOptions::new().read(true).write(true).open(&path)?,
                false,
            ),
            Err(e) => return Err(e.into()),
        };
        regular(&path)?;
        if file.metadata()?.len() != 0 {
            return Err(Error::CorruptRecord);
        }
        file.try_lock().map_err(lock_error)?;
        let _lock = MetadataLock(file);
        let head = if created && !path_present(&self.life_dir(&request.object))? {
            None
        } else {
            self.life_head(&request.object)?
        };
        if let Some(head) = head {
            if head.retired() {
                return Err(Error::ReacquisitionRequired);
            }
            if head.intent.is_some() || head.final_state.is_some() {
                return Err(Error::NeedsRecovery);
            }
            if let Some(first) = &head.birth.reacquire
                && first != request
                && !path_present(&self.op_path(&first.operation, "done"))?
            {
                return Err(Error::ReacquisitionRequired);
            }
            return Ok(Some(head.birth.placement));
        }
        let birth = Birth {
            schema: SCHEMA.into(),
            store: self.instance.clone(),
            object: request.object.clone(),
            index: 0,
            previous: None,
            reacquire: None,
            placement: digest(&("initial-placement", &self.instance, &request.object))?,
            final_context: None,
        };
        self.install_birth(&birth)?;
        Ok(Some(birth.placement))
    }
    pub(super) fn install_birth(&self, birth: &Birth) -> Result<()> {
        let dir = self.life_dir(&birth.object);
        if !path_present(&dir)? {
            fs::create_dir(&dir)?;
        }
        directory(&dir)?;
        self.write_record(&self.life_path(&birth.object, birth.index, "birth"), birth)
    }
    pub(crate) fn current_birth(&self, request: &Request) -> Result<Birth> {
        self.guard()?;
        let head = self
            .life_head(&request.object)?
            .ok_or(Error::CorruptRecord)?;
        if head.intent.is_some() || head.final_state.is_some() {
            return Err(if head.retired() {
                Error::AuthorizedAbsence
            } else {
                Error::NeedsRecovery
            });
        }
        let record: Record = read_record(&self.op_path(&request.operation, "done"))?;
        self.check_record(
            &self.op_path(&request.operation, "done"),
            request,
            "tig.publication-completion.design-1",
        )?;
        if record.incarnation != Some(head.birth.placement.clone()) {
            return Err(Error::CorruptRecord);
        }
        Ok(head.birth)
    }
    pub(crate) fn retirement_intent(
        &self,
        object: &Object,
        index: usize,
    ) -> Result<Option<Intent>> {
        let path = self.life_path(object, index, "intent");
        if !path_present(&path)? {
            return Ok(None);
        }
        Ok(Some(read_record(&path)?))
    }
    pub(crate) fn retirement_done(
        &self,
        object: &Object,
        index: usize,
    ) -> Result<Option<Completion>> {
        let path = self.life_path(object, index, "done");
        if !path_present(&path)? {
            return Ok(None);
        }
        let done: Completion = read_record(&path)?;
        if done.cancelled {
            return Err(Error::CorruptRecord);
        }
        Ok(Some(done))
    }
    pub(crate) fn retirement_aborted(
        &self,
        object: &Object,
        index: usize,
    ) -> Result<Option<Completion>> {
        let path = self.life_path(object, index, "abort");
        if !path_present(&path)? {
            return Ok(None);
        }
        let aborted: Completion = read_record(&path)?;
        if !aborted.cancelled {
            return Err(Error::CorruptRecord);
        }
        Ok(Some(aborted))
    }
    pub(crate) fn abort_retirement(&self, intent: &Intent, fence: &ExclusiveObject) -> Result<()> {
        let object = &intent.plan.publication.object;
        self.verify_retirement_source(&intent.plan.publication, &intent.plan.birth, fence)?;
        let aborted = Completion::aborted(intent)?;
        self.write_record(
            &self.life_path(object, intent.plan.birth.index, "abort"),
            &aborted,
        )
    }
    pub(crate) fn continue_cancelled_retirement(
        &self,
        intent: &Intent,
        fence: &ExclusiveObject,
    ) -> Result<()> {
        let object = &intent.plan.publication.object;
        self.check_handoff_fence(object, fence)?;
        let aborted = self
            .retirement_aborted(object, intent.plan.birth.index)?
            .ok_or(Error::CorruptRecord)?;
        aborted.validate(intent)?;
        if !aborted.cancelled {
            return Err(Error::CorruptRecord);
        }
        let head = self.life_head(object)?.ok_or(Error::CorruptRecord)?;
        if digest(&head.birth)? != digest(&intent.plan.birth)? {
            return Ok(());
        }
        self.verify_retirement_source(&intent.plan.publication, &intent.plan.birth, fence)?;
        if head.birth.index + 1 >= MAX_BIRTHS {
            return Err(Error::Budget);
        }
        let birth = Birth {
            schema: SCHEMA.into(),
            store: self.instance.clone(),
            object: object.clone(),
            index: head.birth.index + 1,
            previous: Some(digest(&aborted)?),
            reacquire: None,
            placement: head.birth.placement,
            final_context: None,
        };
        self.install_birth(&birth)
    }
    pub(crate) fn verify_retirement_source(
        &self,
        request: &Request,
        birth: &Birth,
        fence: &ExclusiveObject,
    ) -> Result<u64> {
        self.verify_bound_source(request, birth, fence, false)
    }
    pub(super) fn verify_bound_source(
        &self,
        request: &Request,
        birth: &Birth,
        fence: &ExclusiveObject,
        allow_final: bool,
    ) -> Result<u64> {
        self.check_handoff_fence(&request.object, fence)?;
        let head = self
            .life_head(&request.object)?
            .ok_or(Error::CorruptRecord)?;
        if digest(&head.birth)? != digest(birth)?
            || head.retired()
            || !allow_final && head.final_state.is_some()
        {
            return Err(Error::OperationConflict);
        }
        for (kind, schema) in [
            ("intent", "tig.publication-intent.design-1"),
            ("done", "tig.publication-completion.design-1"),
        ] {
            let record: Record = read_record(&self.op_path(&request.operation, kind))?;
            if record.schema != schema
                || record.request != *request
                || record.incarnation != Some(birth.placement.clone())
            {
                return Err(Error::OperationConflict);
            }
        }
        let path = self.object_path(&request.object);
        regular(&path)?;
        let mut file = File::open(&path)?;
        if file.metadata()?.len() != request.object.bytes {
            return Err(Error::Integrity);
        }
        copy_verified(&mut file, &mut io::sink(), &request.object)?;
        Ok(fs2::FileExt::allocated_size(&file)?)
    }
    pub(crate) fn object_present(&self, object: &Object) -> Result<bool> {
        path_present(&self.object_path(object))
    }
    pub(crate) fn install_retirement(
        &self,
        intent: &Intent,
        fence: &ExclusiveObject,
    ) -> Result<()> {
        self.check_handoff_fence(&intent.plan.publication.object, fence)?;
        intent.validate(self, &intent.plan.birth)?;
        self.write_record(
            &self.life_path(
                &intent.plan.publication.object,
                intent.plan.birth.index,
                "intent",
            ),
            intent,
        )
    }
    pub(crate) fn unlink_retirement(&self, intent: &Intent, fence: &ExclusiveObject) -> Result<()> {
        self.check_handoff_fence(&intent.plan.publication.object, fence)?;
        let head = self
            .life_head(&intent.plan.publication.object)?
            .ok_or(Error::CorruptRecord)?;
        if digest(&head.birth)? != digest(&intent.plan.birth)?
            || head.intent.as_ref().map(digest).transpose()? != Some(digest(intent)?)
            || head.done.is_some()
            || head.aborted.is_some()
        {
            return Err(Error::OperationConflict);
        }
        regular(&self.object_path(&intent.plan.publication.object))?;
        fs::remove_file(self.object_path(&intent.plan.publication.object))?;
        Ok(())
    }
    pub(crate) fn complete_retirement(
        &self,
        intent: &Intent,
        fence: &ExclusiveObject,
    ) -> Result<Completion> {
        self.check_handoff_fence(&intent.plan.publication.object, fence)?;
        let head = self
            .life_head(&intent.plan.publication.object)?
            .ok_or(Error::CorruptRecord)?;
        if digest(&head.birth)? != digest(&intent.plan.birth)?
            || head.intent.as_ref().map(digest).transpose()? != Some(digest(intent)?)
            || head.aborted.is_some()
        {
            return Err(Error::OperationConflict);
        }
        if self.object_present(&intent.plan.publication.object)? {
            return Err(Error::Integrity);
        }
        let done = Completion::new(intent)?;
        self.write_record(
            &self.life_path(
                &intent.plan.publication.object,
                intent.plan.birth.index,
                "done",
            ),
            &done,
        )?;
        Ok(done)
    }
    pub(crate) fn next_birth(
        &self,
        object: &Object,
        previous: &Completion,
        request: &Request,
    ) -> Result<Birth> {
        let head = self.life_head(object)?.ok_or(Error::CorruptRecord)?;
        if head.done.as_ref().map(digest).transpose()? != Some(digest(previous)?)
            || request.object != *object
        {
            return Err(Error::OperationConflict);
        }
        if head.birth.index + 1 >= MAX_BIRTHS {
            return Err(Error::Budget);
        }
        if path_present(&self.op_path(&request.operation, "intent"))?
            || path_present(&self.op_path(&request.operation, "done"))?
        {
            return Err(Error::OperationConflict);
        }
        Ok(Birth {
            schema: SCHEMA.into(),
            store: self.instance.clone(),
            object: object.clone(),
            index: head.birth.index + 1,
            previous: Some(digest(previous)?),
            reacquire: Some(request.clone()),
            placement: digest(&(
                "reacquired-placement",
                &self.instance,
                object,
                digest(previous)?,
                request,
            ))?,
            final_context: None,
        })
    }
    pub(crate) fn begin_reacquisition(&self, birth: &Birth, fence: &ExclusiveObject) -> Result<()> {
        if birth.index == 0 {
            return Err(Error::InvalidRequest);
        }
        self.check_handoff_fence(&birth.object, fence)?;
        if self.object_present(&birth.object)? {
            return Err(Error::Integrity);
        }
        let expected = self.next_birth(
            &birth.object,
            &read_record::<Completion>(&self.life_path(&birth.object, birth.index - 1, "done"))?,
            birth.reacquire.as_ref().ok_or(Error::InvalidRequest)?,
        )?;
        if digest(&expected)? != digest(birth)? {
            return Err(Error::OperationConflict);
        }
        self.install_birth(birth)
    }
}
impl LocalAttempt {
    pub(crate) fn managed_store(&self) -> &LocalStore {
        &self.store
    }
    pub(crate) fn lifecycle_footprint(&self, object: &Object) -> Result<u64> {
        let mut paths = self.files()?;
        paths.extend(self.store.lifecycle_files(object)?);
        paths.into_iter().try_fold(0u64, |total, p| {
            regular(&p)?;
            total
                .checked_add(fs2::FileExt::allocated_size(&File::open(p)?)?)
                .ok_or(Error::Budget)
        })
    }
}
