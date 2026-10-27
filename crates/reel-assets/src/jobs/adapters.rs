//! Explicit trusted admission/retention ports; no inferred registry or approval.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceRole {
    Input,
    Tool,
    InstalledTool,
    Checkpoint,
}
pub trait ResourceAuthority {
    fn require(&self, role: ResourceRole, sha256: &str, bytes: u64) -> Result<()>;
}
/// Immutable data from this runner's freshly verified retained artifacts.
/// No Deserialize/public constructor: arbitrary success JSON is not a batch.
pub struct RetainedObject {
    pub(super) sha256: String,
    pub(super) bytes: u64,
}
impl RetainedObject {
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}
pub struct RetainedJob {
    pub(super) id: String,
    pub(super) succeeded: bool,
    pub(super) request: RetainedObject,
    pub(super) evidence: RetainedObject,
    pub(super) outputs: BTreeMap<String, RetainedObject>,
    pub(super) registration: PathBuf,
}
impl RetainedJob {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn succeeded(&self) -> bool {
        self.succeeded
    }
    pub fn request(&self) -> &RetainedObject {
        &self.request
    }
    pub fn evidence(&self) -> &RetainedObject {
        &self.evidence
    }
    pub fn outputs(&self) -> &BTreeMap<String, RetainedObject> {
        &self.outputs
    }
}
/// Returning success promises the admitted owner reference is durably retained.
/// It grants no approval/custody authority beyond that provider contract. Failure
/// preserves the owned workspace; implementations must retain uncertain outcomes.
pub trait RetentionSink {
    fn retain(&mut self, batch: &RetainedJob) -> Result<()>;
}
pub struct RunContext<'a> {
    pub config: &'a crate::Config,
    pub authority: &'a dyn ResourceAuthority,
    pub retention: &'a mut dyn RetentionSink,
}
pub(super) struct RegistryAuthority(pub std::collections::HashMap<String, u64>);
impl ResourceAuthority for RegistryAuthority {
    fn require(&self, _: ResourceRole, sha256: &str, bytes: u64) -> Result<()> {
        if self.0.get(sha256) != Some(&bytes) {
            return Err("Job resource must be exactly registered".into());
        }
        Ok(())
    }
}
pub(super) struct RegistryRetention<'a>(pub &'a Path);
impl RetentionSink for RegistryRetention<'_> {
    fn retain(&mut self, batch: &RetainedJob) -> Result<()> {
        crate::registry::register_job_outputs(self.0, &batch.registration)?;
        Ok(())
    }
}
#[derive(Debug)]
pub(super) struct RetirementFailed(pub String);
impl std::fmt::Display for RetirementFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Job completed; staging preserved because retirement verification failed: {}",
            self.0
        )
    }
}
impl std::error::Error for RetirementFailed {}
