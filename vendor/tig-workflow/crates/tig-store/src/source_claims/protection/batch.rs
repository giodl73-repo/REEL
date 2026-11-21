//! Atomic, exact multi-actor projection through the existing index header.
use super::*;
const BATCH: &str = "tig.protection-batch.design-1";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub operation: Key,
    /// Strict actor order, unique actors, same operation ID for every member.
    pub updates: Vec<Update>,
}
impl Batch {
    pub(super) fn validate(&self, index: &Index) -> Result<()> {
        if self.updates.len() < 2
            || self.updates.len() > 16
            || self
                .updates
                .windows(2)
                .any(|w| w[0].actor.as_str() >= w[1].actor.as_str())
        {
            return Err(Error::Invalid);
        }
        let mut changes = 0usize;
        for r in &self.updates {
            if r.operation != self.operation || index.enrollment.actor(&r.actor)?.owner != r.owner {
                return Err(Error::Invalid);
            }
            changes = changes.checked_add(r.changes.len()).ok_or(Error::Budget)?;
            if changes > MAX_CHANGES {
                return Err(Error::Budget);
            }
        }
        Ok(())
    }
    pub(super) fn digest(&self) -> Result<Digest> {
        Ok(Digest::blob(&encode(&(BATCH, self))?))
    }
}
impl Index {
    pub fn update_batch(
        &mut self,
        batch: &Batch,
        nodes: &mut metadata::MetadataStore,
    ) -> Result<Receipt> {
        self.update_batch_hook(batch, nodes, |_| Ok(()))
    }
    pub fn update_batch_hook(
        &mut self,
        batch: &Batch,
        nodes: &mut metadata::MetadataStore,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        batch.validate(self)?;
        self.update_many(
            &batch.operation,
            &batch.updates,
            batch.digest()?,
            nodes,
            hook,
            UpdateMode::Select,
        )
    }
    /// Admit a pending full header without selecting it. Exact retries never
    /// adopt; another journal can safely publish while the old/new union holds.
    pub fn prepare_batch_hook(
        &mut self,
        batch: &Batch,
        nodes: &mut metadata::MetadataStore,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        batch.validate(self)?;
        self.update_many(
            &batch.operation,
            &batch.updates,
            batch.digest()?,
            nodes,
            hook,
            UpdateMode::Prepare,
        )
    }
    pub(super) fn prepare_coordinated_hook(
        &mut self,
        batch: &Batch,
        nodes: &mut metadata::MetadataStore,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        batch.validate(self)?;
        self.update_many(
            &batch.operation,
            &batch.updates,
            batch.digest()?,
            nodes,
            hook,
            UpdateMode::CoordinatedPrepare,
        )
    }
}
