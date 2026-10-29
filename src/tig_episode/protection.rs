//! Per-source ownership of exact current child jobs; not byte/custody approval.
use super::*;
use crate::tig_scene_production::jobs::{Coordination, Identity, ProtectedJob, Readback};
use tig_store::{
    Key,
    source_claims::{Selection, SourceJournal},
};

pub struct ProtectionBudget {
    pub max_objects: usize,
    pub max_logical_bytes: u64,
}
pub struct SceneAdmission<'p, 'a> {
    pub readback: Readback<'p, 'a>,
    pub identity: Identity,
    pub expected_revision: u64,
    pub operation: Key,
}
/// Opaque source guards, not complete episode/current presentation/byte guards.
/// Drop never silently withdraws or retires durably adopted source obligations.
pub struct ProtectedScenes<'p, 'a> {
    checkpoint: Digest,
    jobs: BTreeMap<String, ProtectedJob<'p, 'a>>,
    recipes: BTreeMap<String, Digest>,
    objects: BTreeSet<Object>,
    primary: BTreeSet<Object>,
    logical_bytes: u64,
}
impl ProtectedScenes<'_, '_> {
    pub(super) fn matches(&self, checkpoint: &Digest, recipes: &BTreeMap<String, Digest>) -> bool {
        &self.checkpoint == checkpoint && &self.recipes == recipes
    }
    pub(super) fn objects(&self) -> &BTreeSet<Object> {
        &self.objects
    }
    pub(super) fn contains_source(&self, source: &Key) -> bool {
        self.jobs
            .values()
            .any(|job| job.source_identity() == source)
    }
    pub fn primary_objects(&self) -> &BTreeSet<Object> {
        &self.primary
    }
    /// Unique declared objects across every role, not disk allocation/capacity.
    pub fn logical_bytes(&self) -> u64 {
        self.logical_bytes
    }
    pub fn selections(&self) -> impl Iterator<Item = (&str, &Selection)> {
        self.jobs
            .iter()
            .map(|(id, job)| (id.as_str(), job.selection()))
    }
    pub fn verify_current(&self, source: &Source, journal: &mut SourceJournal) -> Result<()> {
        if source.checkpoint() != &self.checkpoint {
            bail!("episode child ownership belongs to another source snapshot");
        }
        for job in self.jobs.values() {
            job.verify_inputs(journal)?;
        }
        Ok(())
    }
    /// Caller supplies the admitted snapshot, actual byte guards and action.
    /// Rechecks even when the action fails; no result escapes a stale ownership
    /// check. Does not publish/approve, delete artifacts, or discover a Git tip.
    pub fn with_current<T>(
        &self,
        source: &Source,
        journal: &mut SourceJournal,
        action: impl FnOnce(&mut SourceJournal) -> Result<T>,
    ) -> Result<T> {
        self.verify_current(source, journal)?;
        let result = action(journal);
        self.verify_current(source, journal)?;
        result
    }
}
/// All metadata preflight precedes ownership changes. A late adoption error keeps
/// already adopted roots; callers reconcile/retry the exact independent sources.
/// Presentation and assembly ownership/bytes remain separate prerequisites.
pub fn protect_current_scenes<'p, 'a>(
    source: &mut Source,
    request: &Request,
    proofs: &BTreeMap<String, CurrentScene>,
    admissions: BTreeMap<String, SceneAdmission<'p, 'a>>,
    journal: &mut SourceJournal,
    coordination: &Coordination,
    budget: &ProtectionBudget,
) -> Result<ProtectedScenes<'p, 'a>> {
    if budget.max_objects == 0
        || budget.max_objects > 1_000_000
        || budget.max_logical_bytes == 0
        || budget.max_logical_bytes > 9_007_199_254_740_991
    {
        bail!("invalid episode child resource budget");
    }
    let plan = require_current_scenes(source, request, proofs)?;
    let count = plan.components.iter().filter(|c| c.kind == "scene").count();
    if count == 0 || admissions.len() != count {
        bail!("episode requires exact child job admission coverage");
    }
    let mut sources = BTreeSet::new();
    let mut objects = BTreeMap::new();
    let mut primary = BTreeSet::new();
    let mut logical_bytes = 0u64;
    for (id, child) in &admissions {
        if !proofs
            .get(id)
            .is_some_and(|proof| proof.matches_job(child.readback.plan()))
            || !sources.insert(child.identity.source.as_str())
        {
            bail!("episode child job, source identity or current proof differs");
        }
        for object in child.readback.resources().objects() {
            match objects.insert(object.sha256.clone(), object.bytes) {
                Some(bytes) if bytes != object.bytes => bail!("episode child byte sizes conflict"),
                Some(_) => {}
                None => {
                    logical_bytes = logical_bytes
                        .checked_add(object.bytes)
                        .context("episode child logical byte overflow")?
                }
            }
            if objects.len() > budget.max_objects || logical_bytes > budget.max_logical_bytes {
                bail!("episode child resource budget exceeded");
            }
        }
        primary.extend(
            child
                .readback
                .resources()
                .of(tig_git::source::native::job::Role::Primary)
                .iter()
                .cloned(),
        );
    }
    let mut jobs = BTreeMap::new();
    let recipes = admissions
        .iter()
        .map(|(id, child)| {
            (
                id.clone(),
                child.readback.plan().recipe().fingerprint().clone(),
            )
        })
        .collect();
    for (id, child) in admissions {
        let job = child.readback.protect(
            journal,
            coordination,
            child.identity,
            child.expected_revision,
            &child.operation,
        )?;
        jobs.insert(id, job);
    }
    Ok(ProtectedScenes {
        checkpoint: plan.source_checkpoint,
        jobs,
        recipes,
        objects: objects
            .into_iter()
            .map(|(sha256, bytes)| Object { sha256, bytes })
            .collect(),
        primary,
        logical_bytes,
    })
}
