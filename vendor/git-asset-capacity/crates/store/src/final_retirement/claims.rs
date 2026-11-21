//! Live membership clearance across an independently admitted content scope.
//! This is stricter than place-local absence and grants no deletion authority.
use crate::{
    protection::Limits,
    source_claims::{
        Checkpoint,
        admission::{self, Namespace, SubjectFence, Work},
    },
    store_protection::{Binding, Checked},
};
use git_asset_core::{Digest, Object, identity, tree::NodeStore};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, time::Instant};

pub const MAX_NAMESPACES: usize = 16;
const MAX_RECORD: usize = 65_536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidRoster,
    UnknownCoverage,
    IncompleteRoster,
    WrongEnrollment,
    Protected,
    Budget,
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
pub type Result<T> = std::result::Result<T, Error>;
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let raw =
        identity::canonical_json(&serde_json::to_value(value).map_err(|_| Error::InvalidRoster)?)
            .map_err(|_| Error::InvalidRoster)?;
    if raw.len() > MAX_RECORD {
        return Err(Error::Budget);
    }
    Ok(raw)
}
fn digest(value: &impl Serialize) -> Result<Digest> {
    Ok(Digest::blob(&encode(value)?))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamespacePin {
    pub namespace: Digest,
    pub policy: Digest,
    pub minimum: Checkpoint,
}
impl NamespacePin {
    /// This handle's known verified checkpoint, not discovery of global latest.
    /// The caller must independently admit the required trusted minimum.
    pub fn current(namespace: &Namespace) -> Result<Self> {
        namespace.check_marker()?;
        Ok(Self {
            namespace: namespace.identity().clone(),
            policy: digest(namespace.policy())?,
            minimum: namespace.checkpoint(),
        })
    }
    fn check(&self, namespace: &Namespace) -> Result<()> {
        namespace.check_marker()?;
        if self.namespace != *namespace.identity() || self.policy != digest(namespace.policy())? {
            return Err(Error::WrongEnrollment);
        }
        namespace.verify_minimum(&self.minimum)?;
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    FlatBindingClosure,
}

/// Trusted caller admission, not proof that unregistered external demand is absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roster {
    schema: String,
    pub authority: Digest,
    pub epoch: Digest,
    /// Exact admitted scope/unmanaged-boundary declaration; None holds clearance.
    pub coverage: Option<Digest>,
    pub format: Format,
    pub members: Vec<NamespacePin>,
    pub limits: Limits,
}
impl Roster {
    pub fn flat(
        authority: Digest,
        epoch: Digest,
        coverage: Option<Digest>,
        mut members: Vec<NamespacePin>,
        limits: Limits,
    ) -> Result<Self> {
        members.sort_by(|a, b| a.namespace.cmp(&b.namespace));
        let roster = Self {
            schema: "tig.final-claim-roster.design-1".into(),
            authority,
            epoch,
            coverage,
            format: Format::FlatBindingClosure,
            members,
            limits,
        };
        roster.id()?;
        Ok(roster)
    }
    pub fn id(&self) -> Result<Digest> {
        self.limits.validate().map_err(|_| Error::Budget)?;
        if self.schema != "tig.final-claim-roster.design-1"
            || self.members.is_empty()
            || self.members.len() > MAX_NAMESPACES
            || self
                .members
                .iter()
                .any(|p| p.minimum.identity() != &p.namespace)
            || self
                .members
                .windows(2)
                .any(|p| p[0].namespace >= p[1].namespace)
        {
            return Err(Error::InvalidRoster);
        }
        digest(self)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.id()?;
        encode(self)
    }
    /// Stable authority/scope identity, excluding budgets and advancing minima.
    pub fn scope(&self) -> Result<Digest> {
        self.id()?;
        digest(
            &serde_json::json!({"schema":"tig.final-claim-scope.design-1",
            "authority":self.authority,"epoch":self.epoch,"coverage":self.coverage,
            "format":self.format,"members":self.members.iter().map(|p|(&p.namespace,&p.policy)).collect::<Vec<_>>()}),
        )
    }
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        if raw.len() > MAX_RECORD {
            return Err(Error::Budget);
        }
        let value = identity::parse_json(raw).map_err(|_| Error::InvalidRoster)?;
        let roster: Self = serde_json::from_value(value).map_err(|_| Error::InvalidRoster)?;
        if roster.to_json()? != raw {
            return Err(Error::InvalidRoster);
        }
        Ok(roster)
    }
    /// Necessary subset check only. The caller must independently admit the
    /// complete content roster; a provider's own binding need not be complete.
    pub fn covers_binding(&self, binding: &Binding) -> Result<bool> {
        self.id()?;
        if self.coverage.is_none() {
            return Err(Error::UnknownCoverage);
        }
        binding.id().map_err(|_| Error::WrongEnrollment)?;
        Ok(binding
            .members()
            .iter()
            .all(|m| self.members.iter().any(|p| p.namespace == m.namespace)))
    }
}

#[derive(Serialize)]
pub struct Summary<'a> {
    pub schema: &'static str,
    pub roster: &'a Digest,
    pub scope: &'a Digest,
    pub authority: &'a Digest,
    pub epoch: &'a Digest,
    pub object: &'a Object,
    pub checked: &'a [Checked],
    pub declared_flat_membership_clear: bool,
    pub authorizes_retention_release: bool,
    pub authorizes_removal: bool,
}

/// Owns every live namespace fence and borrows its enrolled handles. Not Clone
/// or Serialize. A future executor must additionally hold the provider fence and
/// satisfy exact retention-release/final-deletion authority through removal.
pub struct Clearance<'a> {
    roster: &'a Roster,
    id: Digest,
    scope: Digest,
    object: Object,
    checked: Vec<Checked>,
    order: Vec<usize>,
    namespaces: &'a [Namespace],
    _subjects: Vec<SubjectFence>,
}
impl Clearance<'_> {
    /// Batch consent history is checked while the original subject fences live.
    pub(crate) fn only_owned_commands(
        &self,
        basis: &Roster,
        allowed: &std::collections::BTreeMap<
            Digest,
            Vec<(crate::Key, crate::source_claims::Command)>,
        >,
    ) -> Result<bool> {
        basis.id()?;
        if basis.scope()? != self.scope || basis.members.len() != self.order.len() {
            return Err(Error::WrongEnrollment);
        }
        for (pin, at) in basis.members.iter().zip(&self.order) {
            if pin.namespace != *self.namespaces[*at].identity() {
                return Err(Error::WrongEnrollment);
            }
            let commands = allowed
                .get(&pin.namespace)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if !crate::source_claims::admission::bulk::only_owned_since(
                &self.namespaces[*at],
                &pin.minimum,
                commands,
            )? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub fn summary(&self) -> Summary<'_> {
        Summary {
            schema: "tig.final-claim-clearance.design-1",
            roster: &self.id,
            scope: &self.scope,
            authority: &self.roster.authority,
            epoch: &self.roster.epoch,
            object: &self.object,
            checked: &self.checked,
            declared_flat_membership_clear: true,
            authorizes_retention_release: false,
            authorizes_removal: false,
        }
    }
    pub fn object(&self) -> &Object {
        &self.object
    }
    pub fn roster(&self) -> &Digest {
        &self.id
    }
    pub fn scope(&self) -> &Digest {
        &self.scope
    }
    /// Marker/policy checks retain the subject fences. Unrelated journal heads
    /// may advance; they cannot change this object's membership while held.
    pub fn check_current(&self) -> Result<()> {
        for (pin, index) in self.roster.members.iter().zip(&self.order) {
            pin.check(&self.namespaces[*index])?;
        }
        for (checked, index) in self.checked.iter().zip(&self.order) {
            self.namespaces[*index].verify_minimum(&checked.checkpoint)?;
        }
        Ok(())
    }
}

pub fn acquire<'a>(
    roster: &'a Roster,
    object: &Object,
    namespaces: &'a mut [Namespace],
    nodes: &impl NodeStore,
) -> Result<Clearance<'a>> {
    let id = roster.id()?;
    if roster.coverage.is_none() {
        return Err(Error::UnknownCoverage);
    }
    let mut work = Work::new(nodes, &roster.limits, Instant::now())?;
    if namespaces.len() != roster.members.len() {
        return Err(Error::IncompleteRoster);
    }
    let mut order = Vec::with_capacity(namespaces.len());
    let mut seen = BTreeSet::new();
    for pin in &roster.members {
        let index = namespaces
            .iter()
            .position(|n| n.identity() == &pin.namespace)
            .ok_or(Error::IncompleteRoster)?;
        if !seen.insert(index) {
            return Err(Error::IncompleteRoster);
        }
        namespaces[index].refresh_minimum(&pin.minimum)?;
        pin.check(&namespaces[index])?;
        order.push(index);
    }
    let mut subjects = Vec::with_capacity(order.len());
    for index in &order {
        work.timed()?;
        subjects.push(namespaces[*index].fence(object)?);
    }
    let mut checked = Vec::with_capacity(order.len());
    for ((pin, index), fence) in roster.members.iter().zip(&order).zip(&subjects) {
        pin.check(&namespaces[*index])?;
        let checkpoint = namespaces[*index]
            .absent_object(fence, &mut work)?
            .ok_or(Error::Protected)?;
        checked.push(Checked {
            namespace: pin.namespace.clone(),
            checkpoint,
        });
    }
    let guard = Clearance {
        roster,
        id,
        scope: roster.scope()?,
        object: object.clone(),
        checked,
        order,
        namespaces,
        _subjects: subjects,
    };
    guard.check_current()?;
    work.timed()?;
    Ok(guard)
}

#[cfg(test)]
mod tests;
