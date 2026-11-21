//! Explicit paged historical-claim and final-copy decisions over existing ports.
use super::*;
use crate::{
    final_retirement::{bulk, provider},
    orchestration::Place,
    source_claims::subset,
};
pub mod aged;
mod decision;
mod execute;
pub mod followup;
pub use decision::inspect;
pub use decision::{copies, extend_copies, plan, read_copies};
pub use execute::{Cancellation, Execution, Inspection, Phase, Report, RunLimits, Stage, StoreJob};

#[derive(Debug)]
pub enum Error {
    Invalid,
    Authority,
    Stale,
    Budget,
    Recovery,
    Injected,
    Selection(super::Error),
    Source(subset::Error),
    Admission(admission::Error),
    Claims(claims::Error),
    Provider(provider::Error),
    Witness(bulk::Error),
    Metadata(crate::metadata::Error),
    Tree(TreeError),
    Age(crate::source_claims::age::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
macro_rules! from {
    ($t:ty,$v:ident) => {
        impl From<$t> for Error {
            fn from(e: $t) -> Self {
                Self::$v(e)
            }
        }
    };
}
from!(super::Error, Selection);
from!(subset::Error, Source);
from!(admission::Error, Admission);
from!(claims::Error, Claims);
from!(provider::Error, Provider);
from!(bulk::Error, Witness);
from!(crate::metadata::Error, Metadata);
from!(TreeError, Tree);
from!(crate::source_claims::age::Error, Age);
pub type Result<T> = std::result::Result<T, Error>;
fn encode(v: &impl Serialize) -> Result<Vec<u8>> {
    Ok(super::encode(v)?)
}
fn hash(v: &impl Serialize) -> Result<Digest> {
    Ok(Digest::blob(&encode(v)?))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age: Option<aged::Policy>,
    pub selection: super::Enrollment,
    pub sources: Vec<subset::Enrollment>,
    pub context: Digest,
    pub clock: Digest,
    pub witness: Place,
    pub capacity: Digest,
    pub max_window: u64,
    pub max_metadata_bytes: u64,
}
impl Enrollment {
    pub fn id(&self) -> Result<Digest> {
        self.selection.id()?;
        if let Some(age) = &self.age {
            age.check(self)?;
        }
        if self.sources.is_empty()
            || self.sources.len() > 16
            || self
                .sources
                .windows(2)
                .any(|s| s[0].namespace >= s[1].namespace)
            || self.witness.generation == 0
            || self.max_window == 0
            || self.max_window > 604800000
            || self.max_metadata_bytes == 0
            || self.max_metadata_bytes > 1024 * 1024 * 1024
        {
            return Err(Error::Invalid);
        }
        for s in &self.sources {
            s.id()?;
            if s.authority != self.selection.authority
                || s.owner != self.selection.owner
                || s.classes
                    .iter()
                    .any(|c| !self.selection.classes.contains(c))
            {
                return Err(Error::Authority);
            }
        }
        hash(&serde_json::json!({"schema":"tig.paged-control-enrollment.design-1","value":self}))
    }
    fn source(&self, n: &Digest) -> Result<&subset::Enrollment> {
        self.sources
            .iter()
            .find(|s| &s.namespace == n)
            .ok_or(Error::Authority)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub operation: Key,
    pub manifest: Object,
    pub copies: Object,
    pub window: provider::Window,
}
impl Request {
    fn check(&self, e: &Enrollment) -> Result<()> {
        if self.window.expires <= self.window.not_before
            || self.window.expires - self.window.not_before > e.max_window
        {
            return Err(Error::Invalid);
        }
        encode(self)?;
        Ok(())
    }
}
/// Small candidate header. Only the privately admitted caller can approve it;
/// its own digest, serialized fields and metadata references prove no authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    age: Option<Object>,
    schema: String,
    enrollment: Digest,
    request: Request,
    prepared: Object,
    subjects: Object,
    actions: Object,
    pages: usize,
    bindings: usize,
    objects: usize,
    action_count: usize,
}
pub struct ClaimAuthorization {
    decision: Digest,
}
pub struct FinalAuthorization {
    decision: Digest,
}
impl Decision {
    pub fn id(&self) -> Result<Digest> {
        if self.schema != "tig.paged-control-decision.design-1"
            || self.pages == 0
            || self.pages > 4096
            || self.bindings == 0
            || self.bindings > 65536
            || self.objects == 0
            || self.objects > 65536
            || self.action_count > 65536
        {
            return Err(Error::Invalid);
        }
        hash(self)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.id()?;
        encode(self)
    }
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        if raw.len() > 65536 {
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
    pub fn request(&self) -> &Request {
        &self.request
    }
    /// Declared enrollment identity; callers must still admit it independently.
    pub fn enrollment_id(&self) -> &Digest {
        &self.enrollment
    }
    /// Explicit caller authority boundary, separate from approving final loss.
    pub fn authorize_claim_release(&self) -> Result<ClaimAuthorization> {
        Ok(ClaimAuthorization {
            decision: self.id()?,
        })
    }
    /// Explicit caller authority boundary, separate from settling selected history.
    pub fn authorize_final_loss(&self) -> Result<FinalAuthorization> {
        Ok(FinalAuthorization {
            decision: self.id()?,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hold {
    SurvivingClaims,
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
/// Original candidate explanations, not current protection or approval.
pub struct Inventory {
    decision: Digest,
    basis: claims::Roster,
    subjects: Vec<Subject>,
    actions: Vec<provider::Plan>,
}
impl Inventory {
    pub fn decision(&self) -> &Digest {
        &self.decision
    }
    pub fn basis(&self) -> &claims::Roster {
        &self.basis
    }
    pub fn subjects(&self) -> impl ExactSizeIterator<Item = &Subject> {
        self.subjects.iter()
    }
    pub fn actions(&self) -> impl ExactSizeIterator<Item = (&Digest, &provider::Plan)> {
        self.actions.iter().map(|p| (p.enrollment.store(), p))
    }
    pub fn authorizes_claim_release(&self) -> bool {
        false
    }
    pub fn authorizes_removal(&self) -> bool {
        false
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyIndex {
    schema: String,
    root: Object,
    count: usize,
}
struct Loaded {
    prepared: preparation::Header,
    steps: Vec<(preparation::Step, subset::Plan)>,
    subjects: Vec<Subject>,
    actions: Vec<provider::Plan>,
}
fn slot(i: usize) -> Result<LogicalPath> {
    Ok(super::path(&format!("{i:08}"))?)
}
