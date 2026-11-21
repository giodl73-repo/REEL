//! Owner-authorized active-source withdrawal; historical obligations remain.
use super::*;
use git_asset_store::source_claims::{Command, admission::SourceCondition};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Enrollment {
    Branch {
        branch: super::Enrollment,
    },
    Draft {
        draft: index::Enrollment,
        successor: Box<super::Enrollment>,
    },
}
impl Enrollment {
    fn parts(&self) -> (&Digest, &Digest, &Key, &Key, &Budget, Kind) {
        match self {
            Self::Branch { branch: b } => (
                &b.namespace,
                &b.policy,
                &b.source,
                &b.owner,
                &b.budget,
                Kind::Branch,
            ),
            Self::Draft { draft: d, .. } => (
                &d.namespace,
                &d.policy,
                &d.source,
                &d.owner,
                &d.budget,
                Kind::Draft,
            ),
        }
    }
    pub fn id(&self) -> Result<Digest> {
        match self {
            Self::Branch { branch: b } => {
                b.id()?;
                if b.class != Kind::Branch {
                    return Err(Error::Authority);
                }
            }
            Self::Draft {
                draft: d,
                successor: s,
            } => {
                d.id()?;
                s.id()?;
                if s.class != Kind::Branch
                    || d.source == s.source
                    || d.namespace != s.namespace
                    || d.policy != s.policy
                    || d.authority != s.authority
                    || d.repository != s.repository
                {
                    return Err(Error::Authority);
                }
            }
        }
        digest(&json!({"schema":"tig.source-settlement-enrollment.design-1","value":self}))
    }
    fn source(&self, namespace: &mut Namespace) -> Result<Source> {
        self.id()?;
        let (id, policy, key, owner, _, kind) = self.parts();
        if namespace.identity() != id || &digest(namespace.policy())? != policy {
            return Err(Error::Authority);
        }
        let source = namespace.source(key)?.ok_or(Error::Authority)?;
        if &source.owner != owner || source.kind.as_ref() != Some(&kind) {
            return Err(Error::Authority);
        }
        Ok(source)
    }
    pub fn authority(&self) -> &Digest {
        match self {
            Self::Branch { branch } => &branch.authority,
            Self::Draft { draft, .. } => &draft.authority,
        }
    }
    pub fn namespace(&self) -> &Digest {
        self.parts().0
    }
    pub fn policy(&self) -> &Digest {
        self.parts().1
    }
    pub fn owner(&self) -> &Key {
        self.parts().3
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Evidence {
    Absent,
    Committed {
        commit: String,
        index_listing: Digest,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    schema: String,
    pub enrollment: Digest,
    pub source: Key,
    pub owner: Key,
    pub expected_revision: u64,
    pub before: Digest,
    pub evidence: Evidence,
    pub conditions: Vec<SourceCondition>,
    pub no_op: bool,
}
impl Plan {
    pub fn id(&self) -> Result<Digest> {
        if self.schema != "tig.source-settlement-plan.design-1"
            || self.expected_revision == 0
            || self.conditions.len() > 8
            || self.conditions.iter().any(|c| c.revision == 0)
            || self
                .conditions
                .windows(2)
                .any(|p| p[0].source.as_str() >= p[1].source.as_str())
        {
            return Err(Error::Invalid);
        }
        match &self.evidence {
            Evidence::Absent if !self.conditions.is_empty() => return Err(Error::Invalid),
            Evidence::Committed { commit, .. } => {
                GitId::parse(commit).map_err(|_| Error::Invalid)?;
                if self.conditions.len() != 1 {
                    return Err(Error::Invalid);
                }
            }
            _ => {}
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
        let plan: Self =
            serde_json::from_value(identity::parse_json(bytes).map_err(|_| Error::Invalid)?)
                .map_err(|_| Error::Invalid)?;
        if plan.to_json()? != bytes {
            return Err(Error::Invalid);
        }
        Ok(plan)
    }
    fn operation(&self) -> Result<Key> {
        Key::parse(&format!("source-settle-{}", self.id()?.as_str())).map_err(|_| Error::Invalid)
    }
    fn command(&self) -> Command {
        Command::Withdraw {
            source: self.source.clone(),
            owner: self.owner.clone(),
            expected_revision: self.expected_revision,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub schema: &'static str,
    pub plan: Digest,
    pub historical: Option<Receipt>,
    pub settled: bool,
    pub source_revision: u64,
    pub observed_evidence_current: Option<bool>,
    pub conditions_current: Option<bool>,
    pub storage_converged: Option<bool>,
}
pub fn inspect(enrollment: &Enrollment, namespace: &mut Namespace, plan: &Plan) -> Result<Status> {
    enrollment.source(namespace)?;
    let (_, _, source, owner, _, _) = enrollment.parts();
    if plan.enrollment != enrollment.id()? || &plan.source != source || &plan.owner != owner {
        return Err(Error::Authority);
    }
    let historical = if plan.no_op {
        None
    } else {
        namespace.command_status(plan.operation()?, plan.command())?
    };
    let current = enrollment.source(namespace)?;
    Ok(Status {
        schema: "tig.source-settlement-status.design-1",
        plan: plan.id()?,
        historical,
        settled: current.current.is_none() && current.pending.is_none(),
        source_revision: current.revision,
        observed_evidence_current: None,
        conditions_current: None,
        storage_converged: None,
    })
}
/// Fresh observations remain separate from historical command acknowledgement.
pub fn observe(
    enrollment: &Enrollment,
    repository: &Repository,
    namespace: &mut Namespace,
    plan: &Plan,
) -> Result<Status> {
    let mut status = inspect(enrollment, namespace, plan)?;
    status.observed_evidence_current = match check_git(enrollment, repository, &plan.evidence) {
        Ok(()) => Some(true),
        Err(Error::Stale) => Some(false),
        _ => None,
    };
    let mut current = true;
    for condition in &plan.conditions {
        current &= namespace
            .source(&condition.source)?
            .is_some_and(|s| s.revision == condition.revision);
    }
    status.conditions_current = Some(current);
    Ok(status)
}
fn check_git(enrollment: &Enrollment, repository: &Repository, evidence: &Evidence) -> Result<()> {
    match (enrollment, evidence) {
        (Enrollment::Branch { branch }, Evidence::Absent) => {
            if repository
                .optional_named_commit(&branch.ref_name)
                .map_err(|_| Error::Git)?
                .is_some()
            {
                return Err(Error::Stale);
            }
        }
        (
            Enrollment::Draft { successor, .. },
            Evidence::Committed {
                commit,
                index_listing,
            },
        ) => {
            if repository
                .named_commit(&successor.ref_name)
                .map_err(|_| Error::Git)?
                .as_str()
                != commit
                || &repository
                    .index()
                    .map_err(|_| Error::Git)?
                    .checkpoint()
                    .listing
                    != index_listing
            {
                return Err(Error::Stale);
            }
        }
        _ => return Err(Error::Authority),
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    EvidenceVerified,
    Settled,
}
pub struct Execution<'a> {
    pub enrollment: &'a Enrollment,
    pub repository: &'a Repository,
    pub namespace: &'a mut Namespace,
    pub metadata: &'a mut MetadataStore,
    pub allow_withdraw: bool,
}
impl Execution<'_> {
    fn evidence(&mut self, before: &Source) -> Result<(Evidence, Vec<SourceCondition>)> {
        match self.enrollment {
            Enrollment::Branch { branch } => {
                if self
                    .repository
                    .optional_named_commit(&branch.ref_name)
                    .map_err(|_| Error::Git)?
                    .is_some()
                {
                    return Err(Error::Stale);
                }
                Ok((Evidence::Absent, vec![]))
            }
            Enrollment::Draft { draft, successor } => {
                let staged = index::Execution {
                    enrollment: draft,
                    repository: self.repository,
                    namespace: self.namespace,
                    metadata: self.metadata,
                    allow_selection: false,
                }
                .plan()?;
                if staged.unresolved_stages != 0
                    || (before.current.is_some() && !staged.no_op)
                    || (before.current.is_none()
                        && !before
                            .predecessors
                            .iter()
                            .any(|p| p.selection == staged.selection))
                {
                    return Err(Error::Pending);
                }
                let committed = super::Execution {
                    enrollment: successor,
                    repository: self.repository,
                    namespace: self.namespace,
                    metadata: self.metadata,
                    allow_selection: false,
                }
                .plan(None)?;
                if !committed.no_op {
                    return Err(Error::Pending);
                }
                let mut reader = self
                    .metadata
                    .finished_reader()
                    .map_err(|_| Error::Metadata)?;
                let draft_entries = Tree::from_root(staged.selection.root)
                    .entries(&mut reader, draft.budget.tree())
                    .map_err(|_| Error::Metadata)?
                    .0;
                let branch_entries = Tree::from_root(committed.selection.root.clone())
                    .entries(&mut reader, successor.budget.tree())
                    .map_err(|_| Error::Metadata)?
                    .0;
                let branch_objects = branch_entries
                    .into_values()
                    .map(|o| (o.sha256, o.bytes))
                    .collect::<BTreeMap<_, _>>();
                if draft_entries
                    .values()
                    .any(|o| branch_objects.get(&o.sha256) != Some(&o.bytes))
                {
                    return Err(Error::Pending);
                }
                let accepted = self
                    .namespace
                    .source(&successor.source)?
                    .ok_or(Error::Authority)?;
                if accepted
                    .current
                    .as_ref()
                    .is_none_or(|v| v.selection != committed.selection)
                    || accepted.pending.is_some()
                {
                    return Err(Error::Stale);
                }
                Ok((
                    Evidence::Committed {
                        commit: committed.commit,
                        index_listing: staged.listing,
                    },
                    vec![SourceCondition {
                        source: successor.source.clone(),
                        revision: accepted.revision,
                    }],
                ))
            }
        }
    }
    fn check_git(&self, evidence: &Evidence) -> Result<()> {
        check_git(self.enrollment, self.repository, evidence)
    }
    pub fn plan(&mut self) -> Result<Plan> {
        let start = Instant::now();
        let before = self.enrollment.source(self.namespace)?;
        if before.pending.is_some() {
            return Err(Error::Pending);
        }
        let (evidence, conditions) = self.evidence(&before)?;
        self.check_git(&evidence)?;
        if self.enrollment.source(self.namespace)? != before {
            return Err(Error::Stale);
        }
        self.enrollment.parts().4.timed(start)?;
        let plan = Plan {
            schema: "tig.source-settlement-plan.design-1".into(),
            enrollment: self.enrollment.id()?,
            source: self.enrollment.parts().2.clone(),
            owner: self.enrollment.parts().3.clone(),
            expected_revision: before.revision,
            before: digest(&before)?,
            evidence,
            conditions,
            no_op: before.current.is_none(),
        };
        plan.id()?;
        Ok(plan)
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
        let status = inspect(self.enrollment, self.namespace, plan)?;
        if !self.allow_withdraw {
            return Err(Error::Disabled);
        }
        if status.historical.is_some() {
            return Ok(status);
        }
        let fresh = self.plan()?;
        if &fresh != plan {
            return Err(Error::Stale);
        }
        progress(Stage::EvidenceVerified);
        self.check_git(&plan.evidence)?;
        if !plan.no_op {
            self.enrollment.parts().4.timed(start)?;
            let reader = self
                .metadata
                .finished_reader()
                .map_err(|_| Error::Metadata)?;
            self.namespace.apply_if_sources(
                plan.operation()?,
                plan.command(),
                &plan.conditions,
                &reader,
                &self.enrollment.parts().4.admission(),
            )?;
            progress(Stage::Settled);
        }
        observe(self.enrollment, self.repository, self.namespace, plan)
    }
}
