//! Exact bounded historical selection and explicitly authorized final batches.
use super::{claims, provider};
use crate::{
    Key, Request as Publication,
    orchestration::Place,
    source_claims::{
        self, Command,
        admission::{self, Namespace},
        subset,
    },
};
use git_asset_core::{Digest, LogicalPath, Object, identity};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod execute;
mod selection;
pub(crate) mod witness;
pub use crate::placement::Local;
pub use execute::{Cancellation, Execution, Inspection, Phase, Report, Stage};
pub use selection::plan;

const MAX_PLAN: usize = 49_152;
pub const MAX_ROOTS: usize = 16;
pub const MAX_OBJECTS: usize = 32;
pub const MAX_PATHS: usize = 1024;

#[derive(Debug)]
pub enum Error {
    Invalid,
    Authority,
    Stale,
    Budget,
    Recovery,
    Injected,
    Source(subset::Error),
    Admission(admission::Error),
    Claims(claims::Error),
    Provider(provider::Error),
    Store(crate::Error),
    Ledger(crate::ledger::LedgerError),
    Publication(crate::accounted::AccountedError),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
macro_rules! convert {
    ($t:ty, $v:ident) => {
        impl From<$t> for Error {
            fn from(e: $t) -> Self {
                Self::$v(e)
            }
        }
    };
}
convert!(subset::Error, Source);
convert!(admission::Error, Admission);
convert!(claims::Error, Claims);
convert!(provider::Error, Provider);
convert!(crate::Error, Store);
convert!(crate::ledger::LedgerError, Ledger);
convert!(crate::accounted::AccountedError, Publication);
pub type Result<T> = std::result::Result<T, Error>;
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let raw = identity::canonical_json(&serde_json::to_value(value).map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)?;
    if raw.len() > 65_536 {
        return Err(Error::Budget);
    }
    Ok(raw)
}
fn digest(value: &impl Serialize) -> Result<Digest> {
    Ok(Digest::blob(&encode(value)?))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub authority: Digest,
    pub epoch: Digest,
    pub owner: Key,
    pub context: Digest,
    pub scope: Digest,
    pub clock: Digest,
    pub witness: Place,
    pub capacity: Digest,
    pub max_window: u64,
    pub max_metadata_bytes: u64,
    pub sources: Vec<subset::Enrollment>,
}
impl Enrollment {
    pub fn id(&self) -> Result<Digest> {
        if self.sources.is_empty()
            || self.sources.len() > MAX_ROOTS
            || self.max_window == 0
            || self.max_window > 604_800_000
            || self.max_metadata_bytes == 0
            || self.max_metadata_bytes > 1024 * 1024 * 1024
            || self.witness.generation == 0
            || self
                .sources
                .windows(2)
                .any(|s| s[0].namespace >= s[1].namespace)
        {
            return Err(Error::Invalid);
        }
        for source in &self.sources {
            source.id()?;
            if source.owner != self.owner {
                return Err(Error::Authority);
            }
        }
        digest(&serde_json::json!({"schema":"tig.bulk-final-enrollment.design-1","value":self}))
    }
    fn source(&self, namespace: &Digest) -> Result<&subset::Enrollment> {
        self.sources
            .iter()
            .find(|s| &s.namespace == namespace)
            .ok_or(Error::Authority)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub namespace: Digest,
    pub request: subset::Request,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Copy {
    pub store: Digest,
    pub capacity: Digest,
    pub publication: Publication,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub operation: Key,
    pub selections: Vec<Selection>,
    pub copies: Vec<Copy>,
    pub window: provider::Window,
}
fn selection_key(s: &Selection) -> (&Digest, &str, u64) {
    (
        &s.namespace,
        s.request.source.as_str(),
        s.request.predecessor_revision,
    )
}
fn copy_key(c: &Copy) -> (&Digest, &Digest) {
    (&c.store, &c.publication.object.sha256)
}
impl Request {
    fn check(&self, e: &Enrollment) -> Result<()> {
        if self.selections.is_empty()
            || self.selections.len() > MAX_ROOTS
            || self.copies.len() > MAX_OBJECTS
            || self.window.expires <= self.window.not_before
            || self.window.expires - self.window.not_before > e.max_window
            || self
                .selections
                .windows(2)
                .any(|s| selection_key(&s[0]) >= selection_key(&s[1]))
            || self
                .copies
                .windows(2)
                .any(|c| copy_key(&c[0]) >= copy_key(&c[1]))
            || self
                .selections
                .iter()
                .map(|s| s.request.paths.len())
                .sum::<usize>()
                > MAX_PATHS
        {
            return Err(Error::Invalid);
        }
        for s in &self.selections {
            e.source(&s.namespace)?;
            if s.request.predecessor_revision == 0
                || s.request.paths.is_empty()
                || s.request.paths.windows(2).any(|p| p[0] >= p[1])
            {
                return Err(Error::Invalid);
            }
        }
        for c in &self.copies {
            if c.publication.owner != e.owner {
                return Err(Error::Authority);
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub namespace: Digest,
    pub source: Key,
    pub predecessor: u64,
    pub original: source_claims::Selection,
    pub path: LogicalPath,
    pub object: Object,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hold {
    Protected,
    NoCopyScope,
    ProviderUnavailable,
    ProviderProtection,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    pub object: Object,
    pub hold: Option<Hold>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settlement {
    namespace: Digest,
    plan: subset::Plan,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    schema: String,
    enrollment: Enrollment,
    request: Request,
    roster: claims::Roster,
    bindings: Vec<Binding>,
    subjects: Vec<Subject>,
    settlements: Vec<Settlement>,
    actions: Vec<provider::Plan>,
}
fn action_request(e: &Enrollment, request: &Request, copy: &Copy) -> Result<provider::Request> {
    let context = digest(&("tig.bulk-final-action.design-1", e.id()?, request, copy))?;
    Ok(provider::Request {
        operation: Key::parse(&format!("bulk-final-{}", context.as_str()))?,
        owner: e.owner.clone(),
        publication: copy.publication.clone(),
        capacity: copy.capacity.clone(),
        context,
        window: request.window.clone(),
    })
}
impl Plan {
    /// Proposed enrollment identity for comparison with independent admission.
    /// Reading it grants no authority.
    pub fn enrollment(&self) -> &Enrollment {
        &self.enrollment
    }
    /// Map exact action order to independently admitted store jobs without
    /// parsing private serialized fields or deriving authority from the plan.
    pub fn actions(&self) -> impl ExactSizeIterator<Item = (&Digest, &provider::Plan)> {
        self.actions
            .iter()
            .map(|plan| (plan.enrollment.store(), plan))
    }
    pub fn subjects(&self) -> &[Subject] {
        &self.subjects
    }
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }
    pub fn request(&self) -> &Request {
        &self.request
    }
    pub fn id(&self) -> Result<Digest> {
        self.enrollment.id()?;
        self.request.check(&self.enrollment)?;
        self.roster.id()?;
        if self.schema != "tig.bulk-final-plan.design-1"
            || self.roster.coverage.is_none()
            || self.roster.scope()? != self.enrollment.scope
            || self.bindings.is_empty()
            || self.bindings.len() > MAX_PATHS
            || self.subjects.is_empty()
            || self.subjects.len() > MAX_OBJECTS
            || self.settlements.len() > MAX_ROOTS
            || self.actions.len() > MAX_OBJECTS
            || self
                .subjects
                .windows(2)
                .any(|s| s[0].object.sha256 >= s[1].object.sha256)
        {
            return Err(Error::Invalid);
        }
        let mut expected =
            BTreeMap::<(Digest, String, u64), (source_claims::Selection, Vec<LogicalPath>)>::new();
        let mut seen = BTreeSet::new();
        for b in &self.bindings {
            let subject = self
                .subjects
                .iter()
                .find(|s| s.object == b.object)
                .ok_or(Error::Invalid)?;
            let selection = self
                .request
                .selections
                .iter()
                .find(|s| {
                    s.namespace == b.namespace
                        && s.request.source == b.source
                        && s.request.predecessor_revision == b.predecessor
                        && s.request.paths.contains(&b.path)
                })
                .ok_or(Error::Invalid)?;
            if !seen.insert((selection_key(selection), &b.path)) {
                return Err(Error::Invalid);
            }
            if subject.hold.is_none() {
                let (original, paths) = expected
                    .entry((b.namespace.clone(), b.source.as_str().into(), b.predecessor))
                    .or_insert_with(|| (b.original.clone(), Vec::new()));
                if original != &b.original {
                    return Err(Error::Invalid);
                }
                paths.push(b.path.clone());
            }
        }
        if self.bindings.len()
            != self
                .request
                .selections
                .iter()
                .map(|s| s.request.paths.len())
                .sum::<usize>()
            || self.settlements.len() != expected.len()
        {
            return Err(Error::Invalid);
        }
        for settlement in &self.settlements {
            let r = settlement.plan.request();
            let (original, mut paths) = expected
                .remove(&(
                    settlement.namespace.clone(),
                    r.source.as_str().into(),
                    r.predecessor_revision,
                ))
                .ok_or(Error::Invalid)?;
            paths.sort();
            if r.paths != paths || settlement.plan.original() != &original {
                return Err(Error::Invalid);
            }
            settlement
                .plan
                .commands(self.enrollment.source(&settlement.namespace)?)?;
        }
        let mut copies = BTreeSet::new();
        for action in &self.actions {
            action.id()?;
            let r = action.request();
            let copy = self
                .request
                .copies
                .iter()
                .find(|c| &c.store == action.enrollment.store() && c.publication == r.publication)
                .ok_or(Error::Invalid)?;
            if digest(r)? != digest(&action_request(&self.enrollment, &self.request, copy)?)?
                || !copies.insert(copy_key(copy))
                || action.policy.scope != self.enrollment.scope
                || action.policy.clock != self.enrollment.clock
                || !self
                    .subjects
                    .iter()
                    .any(|s| s.object == r.publication.object && s.hold.is_none())
            {
                return Err(Error::Invalid);
            }
        }
        for subject in &self.subjects {
            if !self.bindings.iter().any(|b| b.object == subject.object) {
                return Err(Error::Invalid);
            }
            let planned = self
                .actions
                .iter()
                .filter(|p| p.request().publication.object == subject.object)
                .count();
            let requested = self
                .request
                .copies
                .iter()
                .filter(|c| c.publication.object == subject.object)
                .count();
            if (subject.hold.is_none() && (requested == 0 || planned != requested))
                || (subject.hold.is_some() && planned != 0)
            {
                return Err(Error::Invalid);
            }
        }
        let raw = encode(self)?;
        if raw.len() > MAX_PLAN {
            return Err(Error::Budget);
        }
        Ok(Digest::blob(&raw))
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.id()?;
        encode(self)
    }
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        if raw.len() > MAX_PLAN {
            return Err(Error::Budget);
        }
        let p: Self =
            serde_json::from_value(identity::parse_json(raw).map_err(|_| Error::Invalid)?)
                .map_err(|_| Error::Invalid)?;
        if p.to_json()? != raw {
            return Err(Error::Invalid);
        }
        Ok(p)
    }
    pub fn authorize(&self) -> Result<Authorization> {
        Ok(Authorization { plan: self.id()? })
    }
}
pub struct Authorization {
    plan: Digest,
}

#[cfg(test)]
mod tests;
