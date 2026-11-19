//! Typed create/close wire contracts sharing the tracker's durable transport.
use super::*;
use reel_workflow::tracker::close::{CloseDraft, Issue};
pub(super) trait Effect: Serialize {
    type Receipt: Serialize + serde::de::DeserializeOwned + Clone + PartialEq;
    const CALL: &'static str;
    const RESPONSE: &'static str;
    const DELIVERY: &'static str;
    const LOOKUP: &'static str;
    const LOOKUP_OPERATION: &'static str;
    fn check(&self) -> Result<()>;
    fn operation_id(&self) -> &str;
    fn repository(&self) -> &str;
    fn payload(&self) -> Result<Vec<u8>>;
    fn consume(&self, _registry: &Registry) -> Result<()> {
        Ok(())
    }
    fn extra_files() -> usize {
        0
    }
    fn check_target(&self, _prefix: &str) -> Result<()> {
        Ok(())
    }
    fn receipt(&self, receipt: &Self::Receipt, prefix: &str) -> Result<()>;
}
impl Effect for Draft {
    type Receipt = Receipt;
    const CALL: &'static str = "reel.workflow-issue-call.v1";
    const RESPONSE: &'static str = "reel.workflow-issue-response.v1";
    const DELIVERY: &'static str = "reel.workflow-issue-delivery.v1";
    const LOOKUP: &'static str = "reel.workflow-issue-lookup.v1";
    const LOOKUP_OPERATION: &'static str = "reel.workflow-issue-lookup-operation.v1";
    fn check(&self) -> Result<()> {
        Draft::check(self)
    }
    fn operation_id(&self) -> &str {
        &self.operation_id
    }
    fn repository(&self) -> &str {
        &self.repository
    }
    fn payload(&self) -> Result<Vec<u8>> {
        Draft::payload(self)
    }
    fn receipt(&self, r: &Receipt, prefix: &str) -> Result<()> {
        if r.schema != "reel.workflow-issue-receipt.v1"
            || r.operation_id != self.operation_id
            || r.repository != self.repository
            || r.request_sha256 != digest(&self.payload()?)
            || !crate::token(&r.issue_id)
            || r.issue_url != format!("{prefix}{}", r.issue_id)
        {
            return Err(error(
                ErrorKind::Integrity,
                "tracker-remote-receipt-binding",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CloseReceipt {
    pub schema: String,
    pub operation_id: String,
    pub repository: String,
    pub request_sha256: String,
    pub issue_id: String,
    pub issue_url: String,
    pub state: String,
    pub previous_revision: String,
    pub revision: String,
    pub resolution_sha256: String,
}
pub type Closed = Delivered<CloseReceipt>;
impl Effect for CloseDraft {
    type Receipt = CloseReceipt;
    const CALL: &'static str = "reel.workflow-issue-close-call.v1";
    const RESPONSE: &'static str = "reel.workflow-issue-close-response.v1";
    const DELIVERY: &'static str = "reel.workflow-issue-close-delivery.v1";
    const LOOKUP: &'static str = "reel.workflow-issue-close-lookup.v1";
    const LOOKUP_OPERATION: &'static str = "reel.workflow-issue-close-lookup-operation.v1";
    fn check(&self) -> Result<()> {
        CloseDraft::check(self)
    }
    fn operation_id(&self) -> &str {
        &self.operation_id
    }
    fn repository(&self) -> &str {
        &self.repository
    }
    fn payload(&self) -> Result<Vec<u8>> {
        CloseDraft::payload(self)
    }
    fn extra_files() -> usize {
        1
    }
    fn consume(&self, registry: &Registry) -> Result<()> {
        let target = close_target(self)?;
        registry.store(
            &target,
            "close-consumed",
            &Consumed {
                schema: "reel.workflow-issue-close-consumed.v1".into(),
                operation_id: self.operation_id.clone(),
                draft_sha256: canonical_digest(self)?,
            },
        )
    }
    fn check_target(&self, prefix: &str) -> Result<()> {
        if self.issue.issue_url != format!("{prefix}{}", self.issue.issue_id) {
            return Err(error(ErrorKind::Denied, "tracker-close-target-url"));
        }
        Ok(())
    }
    fn receipt(&self, r: &CloseReceipt, prefix: &str) -> Result<()> {
        if r.schema != "reel.workflow-issue-close-receipt.v1"
            || r.operation_id != self.operation_id
            || r.repository != self.repository
            || r.request_sha256 != digest(&self.payload()?)
            || r.issue_id != self.issue.issue_id
            || r.issue_url != self.issue.issue_url
            || r.issue_url != format!("{prefix}{}", r.issue_id)
            || r.state != "closed"
            || r.previous_revision != self.expected_revision
            || !reel_workflow::tracker::close::revision(&r.revision)
            || r.revision == r.previous_revision
            || r.resolution_sha256 != self.resolution_sha256
        {
            return Err(error(ErrorKind::Integrity, "tracker-close-receipt-binding"));
        }
        Ok(())
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CloseConfig {
    pub schema: String,
    /// Existing create delivery's registry/config; checked locally, never posted.
    pub issue_delivery: Config,
    /// Shared transport: `create` is the explicit close-effect endpoint here.
    pub transport: Config,
}
#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CloseOwner {
    schema: String,
    operation_id: String,
    draft_sha256: String,
    config_sha256: String,
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Consumed {
    schema: String,
    operation_id: String,
    draft_sha256: String,
}
fn close_target(draft: &CloseDraft) -> Result<String> {
    canonical_digest(&(
        "reel.workflow-issue-close-target.v1",
        &draft.repository,
        &draft.issue.create_operation_id,
        &draft.issue.issue_id,
    ))
}
impl CloseConfig {
    fn guard(&self, draft: &CloseDraft, create: bool) -> Result<(Registry, bool)> {
        self.transport.check(draft)?;
        let target = close_target(draft)?;
        let existed =
            fs::symlink_metadata(self.transport.registry.join(format!("{target}.lock"))).is_ok();
        if !create && !existed {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "tracker-close-target-owner-missing",
            ));
        }
        let registry = Registry::open(&self.transport.registry, &target)?;
        let owner = CloseOwner {
            schema: "reel.workflow-issue-close-owner.v1".into(),
            operation_id: draft.operation_id.clone(),
            draft_sha256: canonical_digest(draft)?,
            config_sha256: canonical_digest(&self.transport)?,
        };
        if registry.exists(&target, "close-owner") {
            let old: CloseOwner = decode(&registry.read(&target, "close-owner")?)
                .map_err(|_| error(ErrorKind::NeedsRecovery, "tracker-close-owner-unreadable"))?;
            if old != owner {
                return Err(error(
                    ErrorKind::Conflict,
                    "tracker-close-original-target-owner",
                ));
            }
        } else if create && !existed {
            registry.store(&target, "close-owner", &owner)?;
        } else {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "tracker-close-target-owner-missing",
            ));
        }
        let consumed = registry.exists(&target, "close-consumed");
        if consumed {
            let old: Consumed =
                decode(&registry.read(&target, "close-consumed")?).map_err(|_| {
                    error(
                        ErrorKind::NeedsRecovery,
                        "tracker-close-consumed-unreadable",
                    )
                })?;
            if old
                != (Consumed {
                    schema: "reel.workflow-issue-close-consumed.v1".into(),
                    operation_id: draft.operation_id.clone(),
                    draft_sha256: canonical_digest(draft)?,
                })
            {
                return Err(error(ErrorKind::Conflict, "tracker-close-consumed-binding"));
            }
        }
        Ok((registry, !consumed))
    }
    fn check(&self, original: &Draft, draft: &CloseDraft) -> Result<()> {
        if self.schema != "reel.workflow-tracker-close.v1" {
            return Err(error(ErrorKind::Invalid, "tracker-close-config"));
        }
        let delivered = self.issue_delivery.verified_delivery(original)?;
        let r = &delivered.receipt;
        let issue = Issue {
            create_operation_id: r.operation_id.clone(),
            create_request_sha256: r.request_sha256.clone(),
            issue_id: r.issue_id.clone(),
            issue_url: r.issue_url.clone(),
        };
        if issue != draft.issue
            || draft.repository != original.repository
            || draft.finding_key != original.finding_key
            || draft.baseline_report_sha256 != original.report_sha256
        {
            return Err(error(ErrorKind::Stale, "tracker-close-original-delivery"));
        }
        Ok(())
    }
    pub fn close(
        &self,
        original: &Draft,
        draft: &CloseDraft,
        resolve: impl Fn(&str) -> Option<String>,
    ) -> Result<Closed> {
        self.check(original, draft)?;
        let (_target, fresh) = self.guard(draft, true)?;
        self.transport.submit_effect(draft, resolve, fresh)
    }
    pub fn reconcile(
        &self,
        original: &Draft,
        draft: &CloseDraft,
        query: &str,
        resolve: impl Fn(&str) -> Option<String>,
    ) -> Result<Closed> {
        self.check(original, draft)?;
        let _target = self.guard(draft, false)?;
        self.transport.reconcile_effect(draft, query, resolve)
    }
}
