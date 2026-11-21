//! Exact provider enrollment plus live all-namespace protection assessment.
//! No custody, physical removal, epoch migration or deletion grant.
use crate::{
    Key,
    local::{ExclusiveObject, LocalStore},
    protection::Limits,
    source_claims::{
        Checkpoint,
        admission::{self, Namespace, SubjectFence, Work},
    },
};
use git_asset_core::{Digest, Object, identity, tree::NodeStore};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, time::Instant};

pub const MAX_NAMESPACES: usize = 16;
pub const MAX_PLACES: usize = 16;
const DOMAIN: &str = "tig.store-protection-binding.design-1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidBinding,
    WrongEnrollment,
    IncompleteRoster,
    Required,
    Busy,
    Retained,
    Provider,
    Admission(admission::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<admission::Error> for Error {
    fn from(e: admission::Error) -> Self {
        Self::Admission(e)
    }
}
impl From<crate::Error> for Error {
    fn from(e: crate::Error) -> Self {
        match e {
            crate::Error::Busy => Self::Busy,
            crate::Error::Retained => Self::Retained,
            _ => Self::Provider,
        }
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub namespace: Digest,
    /// Namespace-local place aliases all assigned to this physical provider.
    pub places: Vec<Key>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    generation: u64,
    boundary: Digest,
    members: Vec<Member>,
    #[serde(default, skip_serializing_if = "HistoryScope::is_namespace")]
    history_scope: HistoryScope,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryScope {
    #[default]
    Namespace,
    Roster,
}
impl HistoryScope {
    fn is_namespace(&self) -> bool {
        *self == Self::Namespace
    }
}
impl Binding {
    pub fn new(generation: u64, boundary: Digest, members: Vec<Member>) -> Result<Self> {
        Self {
            generation,
            boundary,
            members,
            history_scope: HistoryScope::Namespace,
        }
        .normalized()
    }
    /// Fresh enrollment only: every member explicitly joins the same advisory
    /// precedence group. Hard retention and additive policies remain local.
    pub fn new_roster(generation: u64, boundary: Digest, members: Vec<Member>) -> Result<Self> {
        Self {
            generation,
            boundary,
            members,
            history_scope: HistoryScope::Roster,
        }
        .normalized()
    }
    pub fn history_scope(&self) -> HistoryScope {
        self.history_scope
    }
    fn normalized(&self) -> Result<Self> {
        if self.generation == 0
            || self.members.is_empty()
            || self.members.len() > MAX_NAMESPACES
            || self
                .members
                .iter()
                .any(|m| m.places.is_empty() || m.places.len() > MAX_PLACES)
        {
            return Err(Error::InvalidBinding);
        }
        let mut binding = self.clone();
        binding
            .members
            .sort_by(|a, b| a.namespace.cmp(&b.namespace));
        if binding
            .members
            .windows(2)
            .any(|p| p[0].namespace == p[1].namespace)
        {
            return Err(Error::InvalidBinding);
        }
        for member in &mut binding.members {
            member.places.sort_by(|a, b| a.as_str().cmp(b.as_str()));
            if member.places.windows(2).any(|p| p[0] == p[1]) {
                return Err(Error::InvalidBinding);
            }
        }
        Ok(binding)
    }
    pub(crate) fn valid(&self) -> bool {
        self.normalized().is_ok_and(|b| b == *self)
    }
    pub fn id(&self) -> Result<Digest> {
        if !self.valid() {
            return Err(Error::InvalidBinding);
        }
        let value = serde_json::to_value((DOMAIN, self)).map_err(|_| Error::InvalidBinding)?;
        Ok(Digest::blob(
            &identity::canonical_json(&value).map_err(|_| Error::InvalidBinding)?,
        ))
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn boundary(&self) -> &Digest {
        &self.boundary
    }
    pub fn members(&self) -> &[Member] {
        &self.members
    }
}

/// Portable enrollment only; must match an actual existing provider on use.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    store: Digest,
    binding: Digest,
    generation: u64,
}
impl Enrollment {
    pub fn bind(store: &LocalStore) -> Result<Self> {
        store.check_identity()?;
        let binding = store.protection().ok_or(Error::WrongEnrollment)?;
        Ok(Self {
            store: store.identity().clone(),
            binding: binding.id()?,
            generation: binding.generation,
        })
    }
    pub fn store(&self) -> &Digest {
        &self.store
    }
    pub fn binding(&self) -> &Digest {
        &self.binding
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn check(&self, store: &LocalStore) -> Result<()> {
        if *self != Self::bind(store)? {
            return Err(Error::WrongEnrollment);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checked {
    pub namespace: Digest,
    pub checkpoint: Checkpoint,
}
#[derive(Serialize)]
pub struct Summary<'a> {
    pub schema: &'static str,
    pub enrollment: &'a Enrollment,
    pub object: &'a Object,
    pub checked: &'a [Checked],
    pub removal_authority: bool,
}
/// Owns every live guard and borrows the admitted provider/namespace handles.
/// Unrelated operations can use independently opened handles to those instances.
pub struct Assessment<'a> {
    store: &'a LocalStore,
    namespaces: &'a [Namespace],
    enrollment: Enrollment,
    object: Object,
    checked: Vec<Checked>,
    _subjects: Vec<SubjectFence>,
    _provider: ExclusiveObject,
}
impl Assessment<'_> {
    pub(crate) fn provider(&self) -> &ExclusiveObject {
        &self._provider
    }
    pub fn summary(&self) -> Summary<'_> {
        Summary {
            schema: "tig.store-protection-assessment.design-1",
            enrollment: &self.enrollment,
            object: &self.object,
            checked: &self.checked,
            removal_authority: false,
        }
    }
    /// Marker checks do not release subject/provider protection. A changed
    /// unrelated source head does not invalidate the protected absence predicate.
    pub fn check_current(&self) -> Result<()> {
        self.enrollment.check(self.store)?;
        for namespace in self.namespaces {
            namespace.check_marker()?;
        }
        Ok(())
    }
}

pub fn assess<'a>(
    store: &'a LocalStore,
    enrollment: &Enrollment,
    object: &Object,
    namespaces: &'a mut [Namespace],
    nodes: &impl NodeStore,
    limits: &Limits,
) -> Result<Assessment<'a>> {
    let mut work = Work::new(nodes, limits, Instant::now())?;
    enrollment.check(store)?;
    let binding = store.protection().ok_or(Error::WrongEnrollment)?;
    if namespaces.len() != binding.members.len() {
        return Err(Error::IncompleteRoster);
    }
    let mut seen = BTreeSet::new();
    let mut order = Vec::with_capacity(namespaces.len());
    for member in &binding.members {
        let Some(index) = namespaces
            .iter()
            .position(|n| n.identity() == &member.namespace)
        else {
            return Err(Error::IncompleteRoster);
        };
        if !seen.insert(index) {
            return Err(Error::IncompleteRoster);
        }
        order.push(index);
    }
    let mut subjects = Vec::with_capacity(order.len());
    for index in &order {
        subjects.push(namespaces[*index].fence(object)?);
    }
    let provider = store.existing_exclusive_object(object)?;
    let mut checked = Vec::with_capacity(order.len());
    let roster_checkpoints = if binding.history_scope == HistoryScope::Roster {
        Some(
            admission::roster::absent(
                namespaces,
                &order,
                &subjects,
                &binding
                    .members
                    .iter()
                    .map(|m| m.places.clone())
                    .collect::<Vec<_>>(),
                &mut work,
            )?
            .ok_or(Error::Required)?,
        )
    } else {
        None
    };
    for (position, ((member, index), fence)) in
        binding.members.iter().zip(order).zip(&subjects).enumerate()
    {
        let namespace = &mut namespaces[index];
        let checkpoint = if let Some(checkpoints) = &roster_checkpoints {
            checkpoints[position].clone()
        } else {
            namespace
                .absent_places(fence, &member.places, &mut work)?
                .ok_or(Error::Required)?
        };
        checked.push(Checked {
            namespace: member.namespace.clone(),
            checkpoint,
        });
    }
    work.timed()?;
    let assessment = Assessment {
        store,
        namespaces,
        enrollment: enrollment.clone(),
        object: object.clone(),
        checked,
        _subjects: subjects,
        _provider: provider,
    };
    assessment.check_current()?;
    work.timed()?;
    Ok(assessment)
}

#[cfg(test)]
mod tests;
