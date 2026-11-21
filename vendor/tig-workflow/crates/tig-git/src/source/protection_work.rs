//! Read-only local ref observation bound to durable exact protection work.
//! This does not authenticate a remote ref or hold Git writers out.
use super::{GitId, Repository, Scope, TreeLimits};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use tig_core::{Digest, LogicalPath, Object, identity, tree::NodeStore};
use tig_store::{
    Key,
    metadata::MetadataStore,
    source_claims::{
        Receipt, Selection, SourceJournal,
        protection::{
            self, Index, Update,
            work::{self, WorkRef},
        },
    },
};
const SCHEMA: &str = "tig.git-protection-work.design-1";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub repository: Digest,
    pub reference: String,
    pub actor: Key,
    pub owner: Key,
    pub scope: Vec<LogicalPath>,
}
pub struct Event {
    pub operation: Key,
    pub commit: GitId,
    pub expected_revision: u64,
    pub changes: BTreeMap<LogicalPath, Option<Object>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    schema: String,
    binding: Binding,
    endpoint: Digest,
    commit: String,
    selection: Selection,
}
#[derive(Debug)]
pub enum Error {
    Git(super::Error),
    Index(protection::Error),
    Invalid,
    Stale,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<super::Error> for Error {
    fn from(e: super::Error) -> Self {
        Self::Git(e)
    }
}
impl From<protection::Error> for Error {
    fn from(e: protection::Error) -> Self {
        Self::Index(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

fn validate(binding: &Binding) -> Result<()> {
    let name = &binding.reference;
    if name.len() > 256
        || !(name.starts_with("refs/heads/") || name.starts_with("refs/tags/"))
        || name.ends_with('/')
        || name.ends_with('.')
        || name.contains("..")
        || name.contains("//")
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"/-_.".contains(&c))
        || name
            .split('/')
            .any(|p| p.is_empty() || p.starts_with('.') || p.ends_with(".lock"))
    {
        return Err(Error::Invalid);
    }
    Scope::new(binding.scope.clone())?;
    Ok(())
}
fn endpoint(repository: &Repository) -> Result<Digest> {
    let bytes = repository.run(&["rev-parse", "--git-common-dir"], Vec::new(), 4096)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| Error::Invalid)?
        .trim_end_matches('\n');
    if text.is_empty() || text.contains(['\n', '\r']) {
        return Err(Error::Invalid);
    }
    let path = PathBuf::from(text);
    let path = if path.is_absolute() {
        path
    } else {
        repository.root.join(path)
    };
    let path = std::fs::canonicalize(path).map_err(|_| Error::Git(super::Error::Io))?;
    let text = path.to_str().ok_or(Error::Invalid)?;
    Ok(Digest::blob(
        &identity::canonical_json(&serde_json::json!([SCHEMA, "endpoint-binding", text]))
            .map_err(|_| Error::Invalid)?,
    ))
}
/// Observe the exact configured local ref. This is neither remote authentication
/// nor a lease against ref writers; execution must validate again.
pub fn observe_current(repository: &Repository, binding: &Binding) -> Result<GitId> {
    validate(binding)?;
    let expression = format!("{}^{{commit}}", binding.reference);
    let bytes = repository.run(&["rev-parse", "--verify", &expression], Vec::new(), 128)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| Error::Invalid)?
        .trim_end_matches('\n');
    Ok(GitId::parse(text)?)
}
fn current(repository: &Repository, binding: &Binding, commit: &GitId) -> Result<()> {
    if observe_current(repository, binding)? != *commit {
        return Err(Error::Stale);
    }
    Ok(())
}
/// Read-only validation of the caller-bound local ref; not a writer lease,
/// remote authentication or a globally latest-state guarantee.
pub fn validate_current(repository: &Repository, binding: &Binding, commit: &GitId) -> Result<()> {
    current(repository, binding, commit)
}
fn observation(nodes: &impl NodeStore, root: &Object) -> Result<Observation> {
    if root.bytes > 65536 {
        return Err(Error::Invalid);
    }
    let bytes = nodes
        .get(root)
        .map_err(|_| Error::Invalid)?
        .ok_or(Error::Invalid)?;
    if Object::from_bytes(&bytes) != *root {
        return Err(Error::Invalid);
    }
    let value = identity::parse_json(&bytes).map_err(|_| Error::Invalid)?;
    let record: Observation = serde_json::from_value(value).map_err(|_| Error::Invalid)?;
    if record.schema != SCHEMA
        || identity::canonical_json(&serde_json::to_value(&record).map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)?
            != bytes
    {
        return Err(Error::Invalid);
    }
    validate(&record.binding)?;
    GitId::parse(&record.commit)?;
    Ok(record)
}
fn checked(
    repository: &Repository,
    binding: &Binding,
    index: &mut Index,
    reference: &WorkRef,
    nodes: &impl NodeStore,
) -> Result<Observation> {
    validate(binding)?;
    let record = work::read(index, reference, nodes)?;
    let observed = observation(nodes, record.evidence.as_ref().ok_or(Error::Invalid)?)?;
    let update = record
        .updates()
        .find(|r| r.actor == binding.actor)
        .ok_or(Error::Invalid)?;
    if observed.binding != *binding
        || observed.endpoint != endpoint(repository)?
        || update.owner != binding.owner
        || update.selection != observed.selection
        || !update.ready
    {
        return Err(Error::Invalid);
    }
    Ok(observed)
}
/// New work observes the configured local ref twice around exact extraction.
/// Duplicate recorded work compares immutable inputs without recapturing a newer
/// branch or rebinding its event ID. A staged record is not actor admission.
pub fn stage(
    repository: &Repository,
    binding: &Binding,
    index: &mut Index,
    event: &Event,
    nodes: &mut MetadataStore,
) -> Result<WorkRef> {
    stage_inner(repository, binding, index, event, None, None, nodes)
}
/// Atomically project the captured current source with exact admitted companion
/// projections (for example deprecated-only demand). Other actor semantics and
/// source authority remain trusted controller inputs, not inferred Git history.
pub fn stage_batch(
    repository: &Repository,
    binding: &Binding,
    index: &mut Index,
    event: &Event,
    additional: &[Update],
    nodes: &mut MetadataStore,
) -> Result<WorkRef> {
    stage_inner(
        repository,
        binding,
        index,
        event,
        Some(additional),
        None,
        nodes,
    )
}
pub fn stage_coordinated(
    repository: &Repository,
    binding: &Binding,
    index: &mut Index,
    event: &Event,
    additional: &[Update],
    journal: &mut SourceJournal,
    nodes: &mut MetadataStore,
) -> Result<WorkRef> {
    stage_inner(
        repository,
        binding,
        index,
        event,
        Some(additional),
        Some(journal),
        nodes,
    )
}
fn stage_inner(
    repository: &Repository,
    binding: &Binding,
    index: &mut Index,
    event: &Event,
    additional: Option<&[Update]>,
    journal: Option<&mut SourceJournal>,
    nodes: &mut MetadataStore,
) -> Result<WorkRef> {
    validate(binding)?;
    if let Some(journal) = journal.as_ref() {
        let profile = journal.coordinator().ok_or(Error::Invalid)?;
        if profile.actor != binding.actor || profile.owner != binding.owner {
            return Err(Error::Invalid);
        }
    }
    if let Some(additional) = additional {
        if additional.is_empty() || additional.len() > 15 {
            return Err(Error::Invalid);
        }
        let mut changed = event.changes.len();
        for r in additional {
            if r.actor == binding.actor || r.operation != event.operation {
                return Err(Error::Invalid);
            }
            changed = changed.checked_add(r.changes.len()).ok_or(Error::Invalid)?;
        }
        if changed > protection::MAX_CHANGES {
            return Err(Error::Invalid);
        }
    }
    if let Some(reference) = work::lookup(index, &event.operation)? {
        let observed = checked(repository, binding, index, &reference, nodes)?;
        let record = work::read(index, &reference, nodes)?;
        let update = record
            .updates()
            .find(|r| r.actor == binding.actor)
            .ok_or(Error::Invalid)?;
        if observed.commit != event.commit.as_str()
            || update.expected_revision != event.expected_revision
            || update.changes != event.changes
            || additional.is_some() != record.additional.is_some()
            || journal.is_some() != record.adoption.is_some()
        {
            return Err(Error::Invalid);
        }
        if let Some(journal) = journal {
            work::adoption::verify_binding(
                index,
                journal,
                record.adoption.as_ref().ok_or(Error::Invalid)?,
            )?;
        }
        if let Some(additional) = additional {
            let mut expected = additional.to_vec();
            expected.sort_by(|a, b| a.actor.as_str().cmp(b.actor.as_str()));
            let recorded = record
                .updates()
                .filter(|r| r.actor != binding.actor)
                .collect::<Vec<_>>();
            if serde_json::to_value(&expected).map_err(|_| Error::Invalid)?
                != serde_json::to_value(&recorded).map_err(|_| Error::Invalid)?
            {
                return Err(Error::Invalid);
            }
        }
        return Ok(reference);
    }
    current(repository, binding, &event.commit)?;
    let extraction = repository.extract(
        &repository.commit(event.commit.clone())?,
        Scope::new(binding.scope.clone())?,
    )?;
    let tree = extraction.write_tree(nodes, TreeLimits::default())?;
    let selection = Selection {
        root: tree.root().clone(),
        context: extraction.selection_digest()?,
    };
    let observed = Observation {
        schema: SCHEMA.into(),
        binding: binding.clone(),
        endpoint: endpoint(repository)?,
        commit: event.commit.as_str().into(),
        selection: selection.clone(),
    };
    let bytes =
        identity::canonical_json(&serde_json::to_value(&observed).map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
    let evidence = nodes.put(&bytes).map_err(|_| Error::Invalid)?;
    if evidence != Object::from_bytes(&bytes) {
        return Err(Error::Invalid);
    }
    current(repository, binding, &event.commit)?;
    let update = Update {
        operation: event.operation.clone(),
        actor: binding.actor.clone(),
        owner: binding.owner.clone(),
        expected_revision: event.expected_revision,
        selection,
        changes: event.changes.clone(),
        ready: true,
    };
    match additional {
        Some(additional) => {
            let mut updates = additional.to_vec();
            updates.push(update);
            updates.sort_by(|a, b| a.actor.as_str().cmp(b.actor.as_str()));
            let batch = protection::batch::Batch {
                operation: event.operation.clone(),
                updates,
            };
            match journal {
                Some(journal) => Ok(work::adoption::stage(
                    index,
                    journal,
                    &batch,
                    Some(evidence),
                    nodes,
                )?),
                None => Ok(work::stage_batch(index, &batch, Some(evidence), nodes)?),
            }
        }
        None => Ok(work::stage(index, &update, Some(evidence), nodes)?),
    }
}
/// New application rejects known-stale observations before index admission and
/// again after index metadata IO. Already admitted work reconciles its original
/// scope even if the ref has since advanced; this is not latest-ref authority.
pub fn execute(
    repository: &Repository,
    binding: &Binding,
    index: &mut Index,
    reference: &WorkRef,
    nodes: &mut MetadataStore,
) -> Result<Receipt> {
    let observed = checked(repository, binding, index, reference, nodes)?;
    let commit = GitId::parse(&observed.commit)?;
    Ok(work::execute_validated(index, reference, nodes, |_| {
        current(repository, binding, &commit).map_err(|e| match e {
            Error::Stale => protection::Error::Stale,
            _ => protection::Error::Unknown,
        })
    })?)
}
pub fn execute_coordinated(
    repository: &Repository,
    binding: &Binding,
    index: &mut Index,
    journal: &mut SourceJournal,
    reference: &WorkRef,
    nodes: &mut MetadataStore,
) -> Result<Receipt> {
    let observed = checked(repository, binding, index, reference, nodes)?;
    let commit = GitId::parse(&observed.commit)?;
    Ok(work::adoption::execute_validated(
        index,
        journal,
        reference,
        nodes,
        |_| {
            current(repository, binding, &commit).map_err(|e| match e {
                Error::Stale => protection::Error::Stale,
                _ => protection::Error::Unknown,
            })
        },
        |_| Ok(()),
    )?)
}
