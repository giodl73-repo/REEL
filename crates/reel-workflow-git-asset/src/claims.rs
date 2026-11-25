//! Restart-persistent declared attempt obligations through the existing Namespace.
//! Claim observations are not byte custody, a live lease or publication authority.
use crate::error;
use git_asset_core::Object;
use git_asset_store::{
    Key,
    metadata::MetadataReader,
    protection::Limits,
    source_claims::{
        self, Checkpoint, Command, Kind, Receipt, Source,
        admission::{self, Namespace, Select},
    },
};
use reel_workflow::{Error, ErrorKind, Result};
use serde::{Deserialize, Serialize};

const SCHEMA: &str = "reel.workflow-public-job-claim.v1";
const MAX_HANDLE: usize = 16 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    schema: String,
    minimum: Checkpoint,
    request: Select,
}
fn journal_error(e: source_claims::Error) -> Error {
    use source_claims::Error as E;
    error(
        match e {
            E::Busy => ErrorKind::Busy,
            E::Budget => ErrorKind::Budget,
            E::Missing => ErrorKind::Missing,
            E::Corrupt => ErrorKind::Integrity,
            E::Rollback | E::Stale => ErrorKind::Stale,
            E::Conflict | E::Pending => ErrorKind::Conflict,
            E::Owner | E::AdmissionRequired => ErrorKind::Denied,
            _ => ErrorKind::NeedsRecovery,
        },
        "public-git-asset-job-journal",
    )
}
pub(crate) fn admission_error(e: admission::Error) -> Error {
    match e {
        admission::Error::Journal(e) => journal_error(e),
        admission::Error::Busy => error(ErrorKind::Busy, "public-git-asset-job-busy"),
        admission::Error::Budget => error(ErrorKind::Budget, "public-git-asset-job-budget"),
        admission::Error::IncompleteClosure => error(
            ErrorKind::NeedsRecovery,
            "public-git-asset-job-closure-unknown",
        ),
        admission::Error::ConflictingSize => {
            error(ErrorKind::Integrity, "public-git-asset-job-size")
        }
        admission::Error::InvalidPolicy | admission::Error::WrongAuthority => {
            error(ErrorKind::Denied, "public-git-asset-job-authority")
        }
        admission::Error::Io => error(ErrorKind::NeedsRecovery, "public-git-asset-job-io"),
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Handle {
    schema: String,
    minimum: Checkpoint,
    request: Select,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    /// No historical receipt for this exact original request. This does not
    /// promise that applying it now can succeed or that its source is empty.
    Unapplied,
    /// Observed current original selection. Not a held fence or byte readback.
    Active,
    /// Original selection survives as a predecessor under fixed history policy.
    Historical,
    /// The source advanced/changed; an old receipt is never current authority.
    Advanced,
}
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub state: State,
    pub receipt: Option<Receipt>,
    pub checkpoint: Checkpoint,
}
impl Handle {
    /// Observe an explicitly pre-enrolled, empty, independently owned Job source.
    /// No enrollment or claim writes. The caller supplies the complete flat
    /// resource Selection and exact operation IDs; no task semantics inferred.
    pub fn prepare(namespace: &mut Namespace, request: Select) -> Result<Self> {
        let source = Self::source(namespace, &request)?;
        if request.prepare == request.adopt
            || source.revision != request.expected_revision
            || source.current.is_some()
            || source.pending.is_some()
            || !source.predecessors.is_empty()
        {
            return Err(error(
                ErrorKind::Stale,
                "public-git-asset-job-fresh-attempt",
            ));
        }
        request
            .expected_revision
            .checked_add(2)
            .ok_or_else(|| error(ErrorKind::Budget, "public-git-asset-job-revision"))?;
        Ok(Self {
            schema: SCHEMA.into(),
            minimum: namespace.checkpoint(),
            request,
        })
    }
    /// Private immutable operation control, intended to be retained asset-side.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(self)
            .map_err(|_| error(ErrorKind::Invalid, "public-git-asset-job-handle"))?;
        if bytes.len() > MAX_HANDLE {
            return Err(error(ErrorKind::Budget, "public-git-asset-job-handle"));
        }
        Ok(bytes)
    }
    /// Decode only against the original externally pinned hash/length. A decoded
    /// handle is not authentication; callers still own admitted owner authority.
    pub fn decode(bytes: &[u8], expected: &Object) -> Result<Self> {
        if bytes.len() > MAX_HANDLE {
            return Err(error(ErrorKind::Budget, "public-git-asset-job-handle"));
        }
        if &Object::from_bytes(bytes) != expected {
            return Err(error(ErrorKind::Integrity, "public-git-asset-job-handle"));
        }
        let wire: Wire = serde_json::from_slice(bytes)
            .map_err(|_| error(ErrorKind::Invalid, "public-git-asset-job-handle"))?;
        let handle = Self {
            schema: wire.schema,
            minimum: wire.minimum,
            request: wire.request,
        };
        if handle.schema != SCHEMA || handle.request.prepare == handle.request.adopt {
            return Err(error(ErrorKind::Invalid, "public-git-asset-job-handle"));
        }
        handle
            .request
            .expected_revision
            .checked_add(2)
            .ok_or_else(|| error(ErrorKind::Budget, "public-git-asset-job-revision"))?;
        Ok(handle)
    }
    pub fn request(&self) -> &Select {
        &self.request
    }
    pub fn minimum(&self) -> &Checkpoint {
        &self.minimum
    }
    fn source(namespace: &mut Namespace, request: &Select) -> Result<Source> {
        let source = namespace
            .source(&request.source)
            .map_err(admission_error)?
            .ok_or_else(|| error(ErrorKind::Missing, "public-git-asset-job-source"))?;
        if source.kind != Some(Kind::Job) || source.owner != request.owner {
            return Err(error(ErrorKind::Denied, "public-git-asset-job-source"));
        }
        Ok(source)
    }
    fn bind(&self, namespace: &mut Namespace) -> Result<()> {
        namespace.observe(&self.minimum).map_err(admission_error)?;
        Self::source(namespace, &self.request)?;
        Ok(())
    }
    /// Admit exact desired obligations from independently reopened, finished
    /// metadata. Hold managed byte guards across this call when bridging existing
    /// bytes to durable claims. Availability/full pool roster are separate gates.
    pub fn admit(
        &self,
        namespace: &mut Namespace,
        nodes: &MetadataReader,
        limits: &Limits,
    ) -> Result<Status> {
        self.bind(namespace)?;
        namespace
            .select(&self.request, nodes, limits)
            .map_err(admission_error)?;
        self.status(namespace)
    }
    /// Read-only exact recovery/renewal observation; appends no event. No timer
    /// expires a claim. It must not be interpreted as a held execution guard.
    pub fn status(&self, namespace: &mut Namespace) -> Result<Status> {
        self.bind(namespace)?;
        let receipt = namespace
            .selection_status(&self.request)
            .map_err(admission_error)?;
        let source = Self::source(namespace, &self.request)?;
        let revision = self.request.expected_revision + 2;
        let matches = |v: &source_claims::Version| {
            v.revision == revision && v.selection == self.request.selection
        };
        let state = if receipt.is_none() {
            State::Unapplied
        } else if source.revision == revision
            && source.pending.is_none()
            && source.current.as_ref().is_some_and(matches)
        {
            State::Active
        } else if source.predecessors.iter().any(matches) {
            State::Historical
        } else {
            State::Advanced
        };
        Ok(Status {
            state,
            receipt,
            checkpoint: namespace.checkpoint(),
        })
    }
    /// Hold only the exact original admitted job generation. This writes no
    /// claim and proves neither bytes nor native pool/roster authority. Callers
    /// must separately bind the actual namespace and retain real byte guards.
    pub fn hold(&self, namespace: &mut Namespace) -> Result<admission::source_use::Use> {
        use admission::source_use::UseError;
        self.bind(namespace)?;
        let revision = self.request.expected_revision + 2;
        let version = source_claims::Version {
            revision,
            selection: self.request.selection.clone(),
        };
        let held = namespace
            .source_use(
                &self.request.source,
                &self.request.owner,
                revision,
                &version,
            )
            .map_err(|e| match e {
                UseError::Unsupported => error(
                    ErrorKind::Unsupported,
                    "public-git-asset-source-use-unsupported",
                ),
                UseError::Admission(e) => admission_error(e),
                UseError::Owner => error(ErrorKind::Denied, "public-git-asset-source-use-owner"),
                UseError::Stale | UseError::Inactive => {
                    error(ErrorKind::Stale, "public-git-asset-source-use-generation")
                }
            })?;
        // Equal bytes and revision do not prove this handle's original command
        // IDs. Check the exact recorded operation while the source is stable.
        if self.status(namespace)?.state != State::Active {
            return Err(error(
                ErrorKind::Stale,
                "public-git-asset-source-use-original-receipt",
            ));
        }
        Ok(held)
    }
    /// Withdraw only the original adopted revision. Retains it as a predecessor;
    /// no Retire, payload deletion, scratch cleanup or creative selection occurs.
    /// Retry only with the same cancel operation; historical replay is observed.
    pub fn cancel(
        &self,
        namespace: &mut Namespace,
        operation: Key,
        nodes: &MetadataReader,
        limits: &Limits,
    ) -> Result<Status> {
        self.bind(namespace)?;
        if namespace
            .selection_status(&self.request)
            .map_err(admission_error)?
            .is_none()
        {
            return Err(error(
                ErrorKind::Missing,
                "public-git-asset-job-not-admitted",
            ));
        }
        namespace
            .apply(
                operation,
                Command::Withdraw {
                    source: self.request.source.clone(),
                    owner: self.request.owner.clone(),
                    expected_revision: self.request.expected_revision + 2,
                },
                nodes,
                limits,
            )
            .map_err(admission_error)?;
        self.status(namespace)
    }
}
