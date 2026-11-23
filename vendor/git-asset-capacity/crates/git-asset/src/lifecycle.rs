//! Exact Git-selected desired protection. Provider execution is composed separately.
pub mod index;
pub mod settlement;
use crate::source::{GitId, Repository, Scope};
use git_asset_core::{
    Digest, LogicalPath, Object, identity,
    tree::{MemoryNodes, NodeStore, Tree, TreeLimits},
};
use git_asset_store::{
    Key,
    metadata::MetadataStore,
    protection,
    source_claims::{
        self, Kind, Receipt, Selection, Source,
        admission::{Namespace, Select},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Authority,
    Stale,
    Pending,
    Disabled,
    Budget,
    Git,
    Metadata,
    Journal,
    Busy,
    Recovery,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
impl From<source_claims::admission::Error> for Error {
    fn from(e: source_claims::admission::Error) -> Self {
        use source_claims::{Error as J, admission::Error as A};
        match e {
            A::Busy | A::Journal(J::Busy) => Self::Busy,
            A::Budget | A::Journal(J::Budget) => Self::Budget,
            A::Journal(J::Stale) => Self::Stale,
            A::Journal(J::Pending) => Self::Pending,
            A::Journal(J::NeedsRecovery | J::Corrupt | J::Rollback) => Self::Recovery,
            A::WrongAuthority | A::Journal(J::Owner | J::Conflict | J::AdmissionRequired) => {
                Self::Authority
            }
            _ => Self::Journal,
        }
    }
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let b = identity::canonical_json(&serde_json::to_value(value).map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)?;
    if b.len() > 16384 {
        return Err(Error::Budget);
    }
    Ok(b)
}
fn digest(value: &impl Serialize) -> Result<Digest> {
    Ok(Digest::blob(&encode(value)?))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub entries: u32,
    pub nodes: u32,
    pub node_bytes: u64,
    pub millis: u64,
}
impl Budget {
    fn validate(&self) -> Result<()> {
        if self.entries == 0
            || self.entries > 32768
            || self.nodes == 0
            || self.nodes > 500000
            || self.node_bytes == 0
            || self.node_bytes > 32 * 1024 * 1024
            || self.millis == 0
            || self.millis > 600000
        {
            return Err(Error::Budget);
        }
        Ok(())
    }
    fn tree(&self) -> TreeLimits {
        TreeLimits {
            max_entries: self.entries as usize,
            max_reads: self.nodes as usize,
            max_writes: self.nodes as usize,
            max_read_bytes: self.node_bytes as usize,
            max_write_bytes: self.node_bytes as usize,
            ..TreeLimits::default()
        }
    }
    fn admission(&self) -> protection::Limits {
        protection::Limits {
            millis: self.millis,
            roots: 130,
            bindings: 500000,
            associations: 500000,
            reads: 2000000,
            read_bytes: 512 * 1024 * 1024,
        }
    }
    fn timed(&self, start: Instant) -> Result<()> {
        if start.elapsed() >= Duration::from_millis(self.millis) {
            Err(Error::Budget)
        } else {
            Ok(())
        }
    }
}
/// Independently trusted operator enrollment, not a grant supplied by an event.
/// Machine paths and external event authentication are outside this portable value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub repository: Digest,
    pub authority: Digest,
    pub namespace: Digest,
    pub policy: Digest,
    pub source: Key,
    pub owner: Key,
    pub class: Kind,
    pub ref_name: String,
    pub scope: Vec<LogicalPath>,
    pub budget: Budget,
}
impl Enrollment {
    pub fn id(&self) -> Result<Digest> {
        self.budget.validate()?;
        if !self.ref_name.starts_with("refs/")
            || self.ref_name.len() > 512
            || self.ref_name.bytes().any(|b| b < 33 || b == 127)
            || self.ref_name.chars().any(char::is_control)
        {
            return Err(Error::Invalid);
        }
        let scope = Scope::new(self.scope.clone()).map_err(|_| Error::Invalid)?;
        if scope.prefixes().cloned().collect::<Vec<_>>() != self.scope {
            return Err(Error::Invalid);
        }
        digest(self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    schema: String,
    pub enrollment: Digest,
    pub namespace: Digest,
    pub source: Key,
    pub owner: Key,
    pub commit: String,
    pub listing: Digest,
    pub selection: Selection,
    pub expected_revision: u64,
    pub before: Digest,
    pub no_op: bool,
}
impl Plan {
    pub fn id(&self) -> Result<Digest> {
        if self.schema != "tig.git-lifecycle-selection.design-1" || self.expected_revision == 0 {
            return Err(Error::Invalid);
        }
        GitId::parse(&self.commit).map_err(|_| Error::Invalid)?;
        digest(self)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.id()?;
        encode(self)
    }
    pub fn from_json(b: &[u8]) -> Result<Self> {
        if b.len() > 16384 {
            return Err(Error::Budget);
        }
        let p: Self = serde_json::from_value(identity::parse_json(b).map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
        if p.to_json()? != b {
            return Err(Error::Invalid);
        }
        Ok(p)
    }
    fn command(&self) -> Result<Select> {
        let id = self.id()?;
        Ok(Select {
            prepare: Key::parse(&format!("git-prepare-{}", id.as_str()))
                .map_err(|_| Error::Invalid)?,
            adopt: Key::parse(&format!("git-adopt-{}", id.as_str())).map_err(|_| Error::Invalid)?,
            source: self.source.clone(),
            owner: self.owner.clone(),
            expected_revision: self.expected_revision,
            selection: self.selection.clone(),
        })
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub schema: &'static str,
    pub plan: Digest,
    pub historical: Option<Receipt>,
    pub desired_current: bool,
    pub source_revision: u64,
    /// A fresh local Git sample, separate from admitted desired state/history.
    pub observed_ref_current: Option<bool>,
    /// Selection admission does not observe physical stores.
    pub storage_converged: Option<bool>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    MetadataVerified,
    Selected,
}
struct Capture {
    selection: Selection,
    listing: Digest,
    nodes: MemoryNodes,
}
fn check_enrollment(enrollment: &Enrollment, namespace: &Namespace) -> Result<()> {
    enrollment.id()?;
    if namespace.identity() != &enrollment.namespace
        || digest(namespace.policy())? != enrollment.policy
    {
        return Err(Error::Authority);
    }
    Ok(())
}
fn admit_plan(enrollment: &Enrollment, namespace: &Namespace, p: &Plan) -> Result<()> {
    check_enrollment(enrollment, namespace)?;
    p.id()?;
    if p.enrollment != enrollment.id()?
        || p.namespace != enrollment.namespace
        || p.source != enrollment.source
        || p.owner != enrollment.owner
    {
        return Err(Error::Authority);
    }
    Ok(())
}
/// Read-only journal inspection; neither Git nor the metadata/payload provider
/// must be online to recover an exact historical selection acknowledgement.
pub fn inspect_selection(
    enrollment: &Enrollment,
    namespace: &mut Namespace,
    p: &Plan,
) -> Result<Status> {
    admit_plan(enrollment, namespace, p)?;
    let historical = if p.no_op {
        None
    } else {
        namespace.selection_status(&p.command()?)?
    };
    let source = namespace
        .source(&enrollment.source)?
        .ok_or(Error::Authority)?;
    if source.owner != enrollment.owner || source.kind.as_ref() != Some(&enrollment.class) {
        return Err(Error::Authority);
    }
    Ok(Status {
        schema: "tig.git-lifecycle-status.design-1",
        plan: p.id()?,
        historical,
        desired_current: source
            .current
            .as_ref()
            .is_some_and(|v| v.selection == p.selection),
        source_revision: source.revision,
        observed_ref_current: None,
        storage_converged: None,
    })
}
/// Complete explicit pointer scope from an independently admitted source. An
/// implementation must enforce its own cumulative IO/response/time budgets.
pub struct FlatCapture {
    pub listing: Digest,
    pub bindings: BTreeMap<LogicalPath, Object>,
}
pub trait CommittedSource {
    fn named_commit(&self, name: &str) -> Result<GitId>;
    fn flat_capture(
        &self,
        commit: GitId,
        scope: &[LogicalPath],
        budget: &Budget,
    ) -> Result<FlatCapture>;
}
impl CommittedSource for Repository {
    fn named_commit(&self, name: &str) -> Result<GitId> {
        Repository::named_commit(self, name).map_err(|_| Error::Git)
    }
    fn flat_capture(
        &self,
        commit: GitId,
        scope: &[LogicalPath],
        budget: &Budget,
    ) -> Result<FlatCapture> {
        let snapshot = self.commit(commit).map_err(|_| Error::Git)?;
        let extraction = self
            .extract(
                &snapshot,
                Scope::new(scope.to_vec()).map_err(|_| Error::Invalid)?,
            )
            .map_err(|_| Error::Git)?;
        if !extraction.complete() {
            return Err(Error::Pending);
        }
        if extraction.bindings().len() > budget.entries as usize {
            return Err(Error::Budget);
        }
        Ok(FlatCapture {
            listing: snapshot.checkpoint().listing.clone(),
            bindings: extraction
                .bindings()
                .iter()
                .map(|b| (b.path.clone(), b.object.clone()))
                .collect(),
        })
    }
}
pub struct Execution<'a, R: CommittedSource + ?Sized = Repository> {
    pub enrollment: &'a Enrollment,
    pub repository: &'a R,
    pub namespace: &'a mut Namespace,
    pub metadata: &'a mut MetadataStore,
    pub allow_selection: bool,
}
impl<R: CommittedSource + ?Sized> Execution<'_, R> {
    fn check(&self) -> Result<()> {
        check_enrollment(self.enrollment, self.namespace)
    }
    fn source(&mut self) -> Result<Source> {
        self.check()?;
        let s = self
            .namespace
            .source(&self.enrollment.source)?
            .ok_or(Error::Authority)?;
        if s.owner != self.enrollment.owner || s.kind.as_ref() != Some(&self.enrollment.class) {
            return Err(Error::Authority);
        }
        Ok(s)
    }
    fn current(&self) -> Result<GitId> {
        self.repository.named_commit(&self.enrollment.ref_name)
    }
    fn require_current(&self, commit: &str) -> Result<()> {
        if self.current()?.as_str() != commit {
            return Err(Error::Stale);
        }
        Ok(())
    }
    fn capture(&self, commit: GitId) -> Result<Capture> {
        let captured = self.repository.flat_capture(
            commit.clone(),
            &self.enrollment.scope,
            &self.enrollment.budget,
        )?;
        if captured.bindings.len() > self.enrollment.budget.entries as usize {
            return Err(Error::Budget);
        }
        let mut nodes = MemoryNodes::default();
        let (tree, _) = Tree::build(
            &captured.bindings,
            &mut nodes,
            self.enrollment.budget.tree(),
        )
        .map_err(|_| Error::Budget)?;
        let (root, listing) = (tree.root().clone(), captured.listing);
        let context = digest(
            &json!({"schema":"tig.git-lifecycle-context.design-1","enrollment":self.enrollment.id()?,"commit":commit.as_str(),"listing":listing,"root":root}),
        )?;
        Ok(Capture {
            selection: Selection { root, context },
            listing,
            nodes,
        })
    }
    /// Catch up the exact local enrolled ref. A requested event cannot choose a
    /// different repository/ref; its commit must equal the observed local ref.
    pub fn plan(&mut self, requested: Option<&GitId>) -> Result<Plan> {
        let start = Instant::now();
        let before = self.source()?;
        if before.pending.is_some() {
            return Err(Error::Pending);
        }
        let commit = self.current()?;
        if requested.is_some_and(|c| c != &commit) {
            return Err(Error::Stale);
        }
        let captured = self.capture(commit.clone())?;
        self.require_current(commit.as_str())?;
        self.enrollment.budget.timed(start)?;
        let p = Plan {
            schema: "tig.git-lifecycle-selection.design-1".into(),
            enrollment: self.enrollment.id()?,
            namespace: self.enrollment.namespace.clone(),
            source: self.enrollment.source.clone(),
            owner: self.enrollment.owner.clone(),
            commit: commit.as_str().into(),
            listing: captured.listing,
            selection: captured.selection.clone(),
            expected_revision: before.revision,
            before: digest(&before)?,
            no_op: before
                .current
                .as_ref()
                .is_some_and(|v| v.selection == captured.selection),
        };
        p.id()?;
        Ok(p)
    }
    fn admit(&self, p: &Plan) -> Result<()> {
        admit_plan(self.enrollment, self.namespace, p)
    }
    pub fn status(&mut self, p: &Plan) -> Result<Status> {
        inspect_selection(self.enrollment, self.namespace, p)
    }
    pub fn apply(&mut self, p: &Plan) -> Result<Status> {
        self.apply_with_progress(p, |_| {})
    }
    pub fn apply_with_progress(
        &mut self,
        p: &Plan,
        mut progress: impl FnMut(Stage),
    ) -> Result<Status> {
        let start = Instant::now();
        self.admit(p)?;
        if !self.allow_selection {
            return Err(Error::Disabled);
        }
        let status = self.status(p)?;
        if status.historical.is_some() {
            return Ok(status);
        }
        let before = self.source()?;
        if before.revision != p.expected_revision || digest(&before)? != p.before {
            return Err(Error::Stale);
        }
        if before.pending.is_some() {
            return Err(Error::Pending);
        }
        self.require_current(&p.commit)?;
        let mut capture = self.capture(GitId::parse(&p.commit).map_err(|_| Error::Invalid)?)?;
        if capture.selection != p.selection
            || capture.listing != p.listing
            || p.no_op
                != before
                    .current
                    .as_ref()
                    .is_some_and(|v| v.selection == p.selection)
        {
            return Err(Error::Authority);
        }
        self.enrollment.budget.timed(start)?;
        if p.no_op {
            return self.observed_status(p);
        }
        // No hidden flush of another caller's staged nodes.
        if self.metadata.pending_nodes() != 0 {
            return Err(Error::Busy);
        }
        for (_, bytes) in capture.nodes.objects() {
            self.enrollment.budget.timed(start)?;
            if self.metadata.put(bytes).map_err(|_| Error::Metadata)? != Object::from_bytes(bytes) {
                return Err(Error::Metadata);
            }
        }
        self.metadata.finish().map_err(|_| Error::Metadata)?;
        let mut reader = self
            .metadata
            .finished_reader()
            .map_err(|_| Error::Metadata)?;
        let (actual, _) = Tree::from_root(p.selection.root.clone())
            .entries(&mut reader, self.enrollment.budget.tree())
            .map_err(|_| Error::Metadata)?;
        let (expected, _) = Tree::from_root(p.selection.root.clone())
            .entries(&mut capture.nodes, self.enrollment.budget.tree())
            .map_err(|_| Error::Metadata)?;
        if actual != expected {
            return Err(Error::Metadata);
        }
        progress(Stage::MetadataVerified);
        self.require_current(&p.commit)?;
        self.enrollment.budget.timed(start)?;
        self.namespace
            .select(&p.command()?, &reader, &self.enrollment.budget.admission())?;
        progress(Stage::Selected);
        self.observed_status(p)
    }
    fn observed_status(&mut self, p: &Plan) -> Result<Status> {
        let mut status = self.status(p)?;
        status.observed_ref_current = self.current().ok().map(|c| c.as_str() == p.commit);
        Ok(status)
    }
}
