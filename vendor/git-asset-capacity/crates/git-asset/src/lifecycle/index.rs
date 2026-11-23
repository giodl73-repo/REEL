//! Staged pointer obligations, independent of a merged Git content selection.
use super::*;
use crate::source::IssueKind;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub repository: Digest,
    pub worktree: Digest,
    pub authority: Digest,
    pub namespace: Digest,
    pub policy: Digest,
    pub source: Key,
    pub owner: Key,
    pub scope: Vec<LogicalPath>,
    pub budget: Budget,
}
impl Enrollment {
    pub fn id(&self) -> Result<Digest> {
        self.budget.validate()?;
        let scope = Scope::new(self.scope.clone()).map_err(|_| Error::Invalid)?;
        if scope.prefixes().cloned().collect::<Vec<_>>() != self.scope {
            return Err(Error::Invalid);
        }
        digest(&json!({"schema":"tig.git-index-enrollment.design-1","value":self}))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    schema: String,
    pub enrollment: Digest,
    pub listing: Digest,
    pub selection: Selection,
    pub expected_revision: u64,
    pub before: Digest,
    pub associations: usize,
    pub unresolved_stages: usize,
    pub no_op: bool,
}
impl Plan {
    pub fn id(&self) -> Result<Digest> {
        if self.schema != "tig.git-index-plan.design-1"
            || self.expected_revision == 0
            || self.associations > 32768
            || self.unresolved_stages > self.associations
        {
            return Err(Error::Invalid);
        }
        digest(self)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.id()?;
        encode(self)
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 16384 {
            return Err(Error::Budget);
        }
        let value: Self =
            serde_json::from_value(identity::parse_json(bytes).map_err(|_| Error::Invalid)?)
                .map_err(|_| Error::Invalid)?;
        if value.to_json()? != bytes {
            return Err(Error::Invalid);
        }
        Ok(value)
    }
    fn command(&self, enrollment: &Enrollment) -> Result<Select> {
        let id = self.id()?;
        Ok(Select {
            prepare: Key::parse(&format!("index-prepare-{}", id.as_str()))
                .map_err(|_| Error::Invalid)?,
            adopt: Key::parse(&format!("index-adopt-{}", id.as_str()))
                .map_err(|_| Error::Invalid)?,
            source: enrollment.source.clone(),
            owner: enrollment.owner.clone(),
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
    pub observed_index_current: Option<bool>,
    pub unresolved_stages: usize,
    pub merged_selection: bool,
    pub storage_converged: Option<bool>,
}
fn source(enrollment: &Enrollment, namespace: &mut Namespace) -> Result<Source> {
    enrollment.id()?;
    if namespace.identity() != &enrollment.namespace
        || digest(namespace.policy())? != enrollment.policy
    {
        return Err(Error::Authority);
    }
    let source = namespace
        .source(&enrollment.source)?
        .ok_or(Error::Authority)?;
    if source.owner != enrollment.owner || source.kind.as_ref() != Some(&Kind::Draft) {
        return Err(Error::Authority);
    }
    Ok(source)
}
/// Historical receipt and current desired state; no Git or payload IO.
pub fn inspect(enrollment: &Enrollment, namespace: &mut Namespace, plan: &Plan) -> Result<Status> {
    source(enrollment, namespace)?;
    if plan.enrollment != enrollment.id()? {
        return Err(Error::Authority);
    }
    let historical = if plan.no_op {
        None
    } else {
        namespace.selection_status(&plan.command(enrollment)?)?
    };
    let current = source(enrollment, namespace)?;
    Ok(Status {
        schema: "tig.git-index-status.design-1",
        plan: plan.id()?,
        historical,
        desired_current: current
            .current
            .as_ref()
            .is_some_and(|v| v.selection == plan.selection),
        source_revision: current.revision,
        observed_index_current: None,
        unresolved_stages: plan.unresolved_stages,
        merged_selection: false,
        storage_converged: None,
    })
}
struct Capture {
    listing: Digest,
    selection: Selection,
    nodes: MemoryNodes,
    associations: usize,
    unresolved_stages: usize,
}
pub struct Execution<'a> {
    pub enrollment: &'a Enrollment,
    pub repository: &'a Repository,
    pub namespace: &'a mut Namespace,
    pub metadata: &'a mut MetadataStore,
    pub allow_selection: bool,
}
impl Execution<'_> {
    fn capture(&self) -> Result<Capture> {
        let start = Instant::now();
        let snapshot = self.repository.index().map_err(|_| Error::Git)?;
        let extracted = self
            .repository
            .extract(
                &snapshot,
                Scope::new(self.enrollment.scope.clone()).map_err(|_| Error::Invalid)?,
            )
            .map_err(|_| Error::Git)?;
        if extracted
            .issues()
            .iter()
            .any(|i| i.kind != IssueKind::Unmerged)
        {
            return Err(Error::Pending);
        }
        if extracted.bindings().len() > self.enrollment.budget.entries as usize {
            return Err(Error::Budget);
        }
        let bindings = extracted
            .bindings()
            .iter()
            .map(|b| {
                Ok((
                    LogicalPath::parse(b.object.sha256.as_str()).map_err(|_| Error::Invalid)?,
                    b.object.clone(),
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let mut nodes = MemoryNodes::default();
        let (tree, _) = Tree::build(&bindings, &mut nodes, self.enrollment.budget.tree())
            .map_err(|_| Error::Budget)?;
        let listing = snapshot.checkpoint().listing.clone();
        let context = digest(
            &json!({"schema":"tig.git-index-context.design-1","enrollment":self.enrollment.id()?,"listing":listing,"root":tree.root(),"associations":extracted.bindings().len(),"unresolved_stages":extracted.issues().len()}),
        )?;
        self.enrollment.budget.timed(start)?;
        Ok(Capture {
            listing,
            selection: Selection {
                root: tree.root().clone(),
                context,
            },
            nodes,
            associations: extracted.bindings().len(),
            unresolved_stages: extracted.issues().len(),
        })
    }
    fn require_current(&self, listing: &Digest) -> Result<()> {
        let current = self.repository.index().map_err(|_| Error::Git)?;
        if &current.checkpoint().listing != listing {
            return Err(Error::Stale);
        }
        Ok(())
    }
    pub fn plan(&mut self) -> Result<Plan> {
        let start = Instant::now();
        let before = source(self.enrollment, self.namespace)?;
        if before.pending.is_some() {
            return Err(Error::Pending);
        }
        let capture = self.capture()?;
        self.require_current(&capture.listing)?;
        self.enrollment.budget.timed(start)?;
        let plan = Plan {
            schema: "tig.git-index-plan.design-1".into(),
            enrollment: self.enrollment.id()?,
            listing: capture.listing,
            selection: capture.selection.clone(),
            expected_revision: before.revision,
            before: digest(&before)?,
            associations: capture.associations,
            unresolved_stages: capture.unresolved_stages,
            no_op: before
                .current
                .as_ref()
                .is_some_and(|v| v.selection == capture.selection),
        };
        plan.id()?;
        Ok(plan)
    }
    pub fn status(&mut self, plan: &Plan) -> Result<Status> {
        inspect(self.enrollment, self.namespace, plan)
    }
    pub fn apply(&mut self, plan: &Plan) -> Result<Status> {
        self.apply_with_progress(plan, |_| {})
    }
    pub fn apply_with_progress(
        &mut self,
        plan: &Plan,
        mut progress: impl FnMut(Stage),
    ) -> Result<Status> {
        let start = Instant::now();
        let status = self.status(plan)?;
        if !self.allow_selection {
            return Err(Error::Disabled);
        }
        if status.historical.is_some() {
            return Ok(status);
        }
        let before = source(self.enrollment, self.namespace)?;
        if before.revision != plan.expected_revision || digest(&before)? != plan.before {
            return Err(Error::Stale);
        }
        if before.pending.is_some() {
            return Err(Error::Pending);
        }
        let mut capture = self.capture()?;
        if capture.listing != plan.listing
            || capture.selection != plan.selection
            || capture.associations != plan.associations
            || capture.unresolved_stages != plan.unresolved_stages
            || plan.no_op
                != before
                    .current
                    .as_ref()
                    .is_some_and(|v| v.selection == plan.selection)
        {
            return Err(Error::Stale);
        }
        if !plan.no_op {
            if self.metadata.pending_nodes() != 0 {
                return Err(Error::Busy);
            }
            for (_, bytes) in capture.nodes.objects() {
                self.enrollment.budget.timed(start)?;
                if self.metadata.put(bytes).map_err(|_| Error::Metadata)?
                    != Object::from_bytes(bytes)
                {
                    return Err(Error::Metadata);
                }
            }
            self.metadata.finish().map_err(|_| Error::Metadata)?;
            let mut reader = self
                .metadata
                .finished_reader()
                .map_err(|_| Error::Metadata)?;
            let actual = Tree::from_root(plan.selection.root.clone())
                .entries(&mut reader, self.enrollment.budget.tree())
                .map_err(|_| Error::Metadata)?
                .0;
            let expected = Tree::from_root(plan.selection.root.clone())
                .entries(&mut capture.nodes, self.enrollment.budget.tree())
                .map_err(|_| Error::Metadata)?
                .0;
            if actual != expected {
                return Err(Error::Metadata);
            }
            progress(Stage::MetadataVerified);
            self.require_current(&plan.listing)?;
            self.enrollment.budget.timed(start)?;
            self.namespace.select(
                &plan.command(self.enrollment)?,
                &reader,
                &self.enrollment.budget.admission(),
            )?;
            progress(Stage::Selected);
        } else {
            self.require_current(&plan.listing)?;
        }
        let mut status = self.status(plan)?;
        status.observed_index_current = self
            .repository
            .index()
            .ok()
            .map(|s| s.checkpoint().listing == plan.listing);
        Ok(status)
    }
}
