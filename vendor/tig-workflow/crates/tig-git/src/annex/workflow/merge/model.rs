use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub objects: usize,
    pub actions: usize,
    pub copy_bytes: u64,
    pub verification_bytes: u64,
    pub child_millis: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    pub remote: String,
    pub enrollment: Enrollment,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub package: Package,
    pub policy: policy::Authority,
    pub caller: Enrollment,
    pub destinations: Vec<Destination>,
    pub reference: String,
    pub current: Key,
    pub history: Key,
    pub initial_commit: String,
    pub scope: Vec<LogicalPath>,
    pub custody: retirement::Custody,
    pub executable: PathBuf,
    pub limits: Limits,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Init {
    pub root: PathBuf,
    pub epoch: Key,
    pub config: Config,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handle {
    pub root: PathBuf,
    pub journal: Digest,
    pub minimum: Checkpoint,
    pub metadata: retirement::MetadataBinding,
    pub configuration: Object,
    pub index_minimum: Checkpoint,
    pub policy_minimum: Checkpoint,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub handle: Handle,
    pub operation: Digest,
    pub commit: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Enrolled {
    pub schema: String,
    pub config: Config,
    pub owner: Key,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Active {
    pub operation: Digest,
    pub commit: String,
    pub recipe: Object,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Head {
    pub settled: bool,
    pub index_minimum: Checkpoint,
    pub policy_minimum: Checkpoint,
    pub cursor: String,
    pub deferred: Object,
    pub active: Option<Active>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Recipe {
    pub before: String,
    pub after: String,
    pub current_before: Selection,
    pub current_after: Selection,
    pub projection: Option<Object>,
    pub copies: Object,
    pub cleanup: Object,
    pub copy_bytes: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CopyTask {
    pub object: Object,
    pub store: Digest,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Saved {
    pub index_minimum: Checkpoint,
    pub policy_minimum: Checkpoint,
    pub recipe: Object,
    pub head_budget: BudgetRef,
    pub run_budget: BudgetRef,
    pub profiles: Option<Object>,
    pub terminal: Option<Finished>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Projected,
    Prepared,
    Complete,
    Deferred,
    Superseded,
    Pending,
    Historical,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Finished {
    pub index_minimum: Checkpoint,
    pub policy_minimum: Checkpoint,
    pub status: Status,
    pub cursor: String,
    pub deferred: Object,
    pub outcomes: Object,
}
#[derive(Serialize)]
pub struct Report {
    pub work: Option<Object>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<Digest>,
    pub status: Status,
    pub operation: Digest,
    pub commit: String,
    pub handle: Handle,
    pub outcomes: Vec<retirement::Report>,
    pub deferred_objects: usize,
    pub reclaimed_bytes: Option<u64>,
}
#[derive(Clone, Copy)]
pub enum Mode {
    Project,
    Prepare,
    Run,
    Inspect,
    Cancel,
}
