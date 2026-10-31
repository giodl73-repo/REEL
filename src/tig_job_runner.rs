//! Existing runner capacity/containment with scoped Tig resource/retention ports.
//! Generic command results remain unproven, not native producer claims.
use crate::tig_scene_production::jobs::{Coordination, Identity, fence};
use anyhow::{Result, bail};
use reel_assets::{
    Config,
    jobs::{
        self, ResourceAuthority, ResourceRole, RetainedJob, RetainedObject, RetentionSink,
        RunContext,
    },
};
use std::{cell::RefCell, collections::BTreeMap, path::Path};
use tig_core::{
    Digest, LogicalPath, Object, identity,
    tree::{NodeStore, Tree, TreeLimits},
};
use tig_git::source::native::job::{JobContext, Role};
use tig_store::{
    Key,
    metadata::{Checkpoint, MetadataReader, MetadataStore},
    source_claims::{Command, Selection, SourceJournal},
};
mod recovery;
pub use recovery::cli::{adopt_prepared, retain_published, retire_workspace};
pub use recovery::{RecoveryBudget, adopt_prepared_result};

/// Retain declared candidates from one interrupted, unpublished workspace.
/// The exact original input owner/revision remains required. Dry-run changes
/// nothing; apply retains a failed-job packet and leaves scratch intact.
pub fn salvage_unpublished(
    root: &Path,
    spec: jobs::Salvage<'_>,
    resources: &JobContext,
    journal: &mut SourceJournal,
    profile: &Profile<'_>,
) -> Result<serde_json::Value> {
    if !resources.of(Role::ClaimedOutput).is_empty() {
        bail!("unpublished salvage requires original input context");
    }
    let _fence = fence(profile.coordination, journal, &profile.identity)?;
    let journal_identity = journal.identity().clone();
    let cell = RefCell::new(Session {
        profile,
        resources,
        journal,
        journal_identity,
        completed: false,
        dependencies: true,
    });
    cell.borrow_mut().check()?;
    let authority = Authority(&cell);
    let mut sink = Sink(&cell);
    let mut result = jobs::salvage_admitted(
        root,
        spec,
        &mut RunContext {
            config: profile.config,
            authority: &authority,
            retention: &mut sink,
        },
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?;
    result["source_revision"] = serde_json::json!(
        cell.borrow_mut()
            .journal
            .source(&profile.identity.source)?
            .ok_or_else(|| anyhow::anyhow!("salvage source missing"))?
            .revision
    );
    Ok(result)
}

pub struct Profile<'a> {
    pub config: &'a Config,
    pub coordination: &'a Coordination,
    pub identity: Identity,
    pub expected_revision: u64,
    pub operation: Key,
    pub metadata_root: &'a Path,
    pub metadata_minimum: &'a Checkpoint,
    pub tree_limits: TreeLimits,
}
fn provider(error: impl std::fmt::Display) -> Box<dyn std::error::Error> {
    error.to_string().into()
}
fn pin(object: &RetainedObject) -> Result<Object> {
    Ok(Object {
        sha256: Digest::parse(object.sha256())?,
        bytes: object.bytes(),
    })
}
struct Session<'a> {
    profile: &'a Profile<'a>,
    resources: &'a JobContext,
    journal: &'a mut SourceJournal,
    journal_identity: Digest,
    completed: bool,
    dependencies: bool,
}
impl Session<'_> {
    fn check(&mut self) -> Result<()> {
        if self.completed || self.journal.identity() != &self.journal_identity {
            bail!("runner source authority or phase changed");
        }
        let state = self
            .journal
            .source(&self.profile.identity.source)?
            .ok_or_else(|| anyhow::anyhow!("runner source missing"))?;
        if state.owner != self.profile.identity.owner
            || state.revision != self.profile.expected_revision
            || state.pending.is_some()
            || state
                .current
                .as_ref()
                .is_none_or(|v| v.selection != *self.resources.selection())
        {
            bail!("runner source owner, revision or selection changed");
        }
        Ok(())
    }
    fn phase(&self, label: &str) -> Result<Key> {
        retention_phase(self.profile, label)
    }
}
fn retention_phase(profile: &Profile<'_>, label: &str) -> Result<Key> {
    let bytes = identity::canonical_json(
        &serde_json::json!({"schema":"reel.tig-runner-operation.experimental-1","source":profile.identity.source.as_str(),"operation":profile.operation.as_str(),"phase":label}),
    )?;
    Ok(Key::parse(Digest::blob(&bytes).as_str())?)
}
struct Authority<'a, 's>(&'a RefCell<Session<'s>>);
impl ResourceAuthority for Authority<'_, '_> {
    fn require(&self, role: ResourceRole, sha256: &str, bytes: u64) -> reel_assets::Result<()> {
        let mut session = self.0.borrow_mut();
        session.check().map_err(provider)?;
        let object = Object {
            sha256: Digest::parse(sha256).map_err(provider)?,
            bytes,
        };
        let kinds: &[Role] = match role {
            ResourceRole::Input if session.dependencies => &[
                Role::Primary,
                Role::Control,
                Role::Interpretation,
                Role::Composition,
                Role::Evidence,
            ],
            ResourceRole::Input => &[Role::Primary, Role::Control, Role::Interpretation],
            ResourceRole::Tool | ResourceRole::InstalledTool => &[Role::Runtime],
            ResourceRole::Checkpoint => &[Role::Interpretation],
        };
        if !kinds
            .iter()
            .any(|role| session.resources.of(*role).contains(&object))
        {
            return Err("Runner resource is outside its admitted Tig role or size".into());
        }
        Ok(())
    }
}
struct Sink<'a, 's>(&'a RefCell<Session<'s>>);
impl RetentionSink for Sink<'_, '_> {
    fn retain(&mut self, batch: &RetainedJob) -> reel_assets::Result<()> {
        self.retain_checked(batch).map_err(provider)
    }
}
impl Sink<'_, '_> {
    fn retain_checked(&mut self, batch: &RetainedJob) -> Result<()> {
        let mut session = self.0.borrow_mut();
        session.check()?;
        let p = session.profile;
        let header = identity::canonical_json(&serde_json::json!({
            "schema":"reel.tig-runner-retention.experimental-1","job_id":batch.id(),"succeeded":batch.succeeded(),
            "source":session.resources.selection(),"journal":session.journal_identity,
            "owner":p.identity.owner.as_str(),"source_id":p.identity.source.as_str(),
            "revision":p.expected_revision,"outputs":batch.outputs().len(),"production_claim":null,
            "retention_operation":p.operation.as_str(),
        }))?;
        let header_object = Object::from_bytes(&header);
        let mut entries = BTreeMap::from([
            (
                LogicalPath::parse("retention/identity")?,
                header_object.clone(),
            ),
            (
                LogicalPath::parse("source/context")?,
                session.resources.selection().root.clone(),
            ),
            (LogicalPath::parse("result/request")?, pin(batch.request())?),
            (
                LogicalPath::parse("result/evidence")?,
                pin(batch.evidence())?,
            ),
        ]);
        for (name, object) in batch.outputs() {
            entries.insert(
                LogicalPath::parse(&format!(
                    "outputs/{}",
                    Digest::blob(name.as_bytes()).as_str()
                ))?,
                pin(object)?,
            );
        }
        if entries.len() > p.tree_limits.max_entries {
            bail!("runner result metadata entry budget exceeded");
        }
        let mut store = MetadataStore::open(p.metadata_root, p.metadata_minimum)?;
        if store.put(&header)? != header_object {
            bail!("runner metadata provider substituted header");
        }
        let tree = Tree::build(&entries, &mut store, p.tree_limits)?.0;
        let witness = store.finish()?;
        drop(store);
        let mut reader = MetadataReader::open(p.metadata_root, &witness)?;
        if reader.get(&header_object)?.as_deref() != Some(header.as_slice())
            || Tree::from_root(tree.root().clone())
                .entries(&mut reader, p.tree_limits)?
                .0
                != entries
        {
            bail!("runner retained result metadata readback differs");
        }
        session.check()?;
        let selection = Selection {
            root: tree.root().clone(),
            context: header_object.sha256,
        };
        let prepare_operation = session.phase("prepare")?;
        let adopt_operation = session.phase("adopt")?;
        let prepared = session.journal.apply(
            prepare_operation,
            Command::Prepare {
                source: p.identity.source.clone(),
                owner: p.identity.owner.clone(),
                expected_revision: p.expected_revision,
                selection: selection.clone(),
            },
        )?;
        let adopted = session.journal.apply(
            adopt_operation,
            Command::Adopt {
                source: p.identity.source.clone(),
                owner: p.identity.owner.clone(),
                expected_revision: prepared.source_revision,
                prepared: prepared.reference,
            },
        )?;
        let current = session
            .journal
            .source(&p.identity.source)?
            .ok_or_else(|| anyhow::anyhow!("runner result owner missing"))?;
        if current.owner != p.identity.owner
            || current.revision != adopted.source_revision
            || current.pending.is_some()
            || current
                .current
                .as_ref()
                .is_none_or(|v| v.selection != selection)
        {
            bail!("runner result handoff no longer current");
        }
        session.completed = true;
        Ok(())
    }
}
/// Resources must already be replayed and owner-adopted. Reuses existing runner
/// budgets/process groups/publication/cleanup; generic outputs stay unproven.
pub fn run(
    root: &Path,
    manifest: &Path,
    resources: &JobContext,
    journal: &mut SourceJournal,
    profile: &Profile<'_>,
) -> Result<serde_json::Value> {
    run_inner(root, manifest, resources, journal, profile, false)
}
/// Explicit generic consumption of already admitted Composition/Evidence bytes.
/// Roles remain distinct; this is not native episode freshness, primary closure,
/// producer correspondence or creative approval. Claimed-output contexts refuse.
pub fn run_with_dependencies(
    root: &Path,
    manifest: &Path,
    resources: &JobContext,
    journal: &mut SourceJournal,
    profile: &Profile<'_>,
) -> Result<serde_json::Value> {
    run_inner(root, manifest, resources, journal, profile, true)
}
fn run_inner(
    root: &Path,
    manifest: &Path,
    resources: &JobContext,
    journal: &mut SourceJournal,
    profile: &Profile<'_>,
    dependencies: bool,
) -> Result<serde_json::Value> {
    if !resources.of(Role::ClaimedOutput).is_empty()
        || (!dependencies
            && (!resources.of(Role::Evidence).is_empty()
                || !resources.of(Role::Composition).is_empty()))
    {
        bail!("runner requires a planned input context");
    }
    let _fence = fence(profile.coordination, journal, &profile.identity)?;
    let journal_identity = journal.identity().clone();
    let cell = RefCell::new(Session {
        profile,
        resources,
        journal,
        journal_identity,
        completed: false,
        dependencies,
    });
    cell.borrow_mut().check()?;
    let authority = Authority(&cell);
    let mut sink = Sink(&cell);
    let result = jobs::run_admitted(
        root,
        manifest,
        &mut RunContext {
            config: profile.config,
            authority: &authority,
            retention: &mut sink,
        },
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(result)
}
