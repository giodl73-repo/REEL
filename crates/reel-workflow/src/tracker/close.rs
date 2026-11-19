//! Explicit remote-close intent. These contracts do not authenticate delivery,
//! grant retained custody, authorize public release or perform tracker effects.
use super::Draft;
use crate::*;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Issue {
    pub create_operation_id: String,
    pub create_request_sha256: String,
    pub issue_id: String,
    pub issue_url: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub schema: String,
    pub finding_key: String,
    pub expected_revision: String,
    pub note: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CloseDraft {
    pub schema: String,
    pub operation_id: String,
    pub repository: String,
    pub finding_key: String,
    pub baseline_report_sha256: String,
    pub issue: Issue,
    pub resolution_sha256: String,
    pub expected_revision: String,
    pub note: String,
}
pub fn operation(draft: &CloseDraft) -> Result<String> {
    canonical_digest(&(
        "reel.workflow-issue-close-operation.v1",
        &draft.repository,
        &draft.issue.create_operation_id,
        &draft.issue.issue_id,
        &draft.resolution_sha256,
    ))
}
pub fn revision(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && value.bytes().all(|b| b.is_ascii_graphic())
}
impl CloseDraft {
    pub fn check(&self) -> Result<()> {
        canonical_digest(self)?;
        let repo: Vec<_> = self.repository.split('/').collect();
        if self.schema != "reel.workflow-issue-close-draft.v1"
            || repo.len() != 2
            || repo.iter().any(|v| !token(v))
            || self.repository != self.repository.to_ascii_lowercase()
            || [
                &self.finding_key,
                &self.baseline_report_sha256,
                &self.resolution_sha256,
                &self.issue.create_request_sha256,
            ]
            .iter()
            .any(|s| !hex(s, &[64]))
            || self.issue.create_operation_id
                != super::operation(&self.repository, &self.finding_key)?
            || !token(&self.issue.issue_id)
            || self.issue.issue_url.len() > 8192
            || !self.issue.issue_url.starts_with("https://")
            || self.issue.issue_url.chars().any(char::is_whitespace)
            || !revision(&self.expected_revision)
            || self.note.trim().is_empty()
            || self.note.len() > 64 * 1024
            || self
                .note
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return fail(ErrorKind::Invalid, "tracker-close-draft-contract");
        }
        if self.operation_id != operation(self)? {
            return fail(ErrorKind::Integrity, "tracker-close-operation");
        }
        Ok(())
    }
    pub fn payload(&self) -> Result<Vec<u8>> {
        self.check()?;
        serde_json::to_vec(&serde_json::json!({"schema":"reel.workflow-issue-close.v1","operation_id":self.operation_id,"repository":self.repository,"create_operation_id":self.issue.create_operation_id,"issue_id":self.issue.issue_id,"issue_url":self.issue.issue_url,"expected_state":"open","expected_revision":self.expected_revision,"resolution_sha256":self.resolution_sha256,"note":self.note})).map_err(|_|Error{kind:ErrorKind::Invalid,code:"tracker-close-payload".into()})
    }
}
pub fn draft(
    issue_draft: &Draft,
    issue: &Issue,
    resolution: &resolution::Proposal,
    text: &Text,
) -> Result<CloseDraft> {
    issue_draft.check()?;
    if text.schema != "reel.workflow-issue-close-text.v1"
        || text.finding_key != issue_draft.finding_key
        || resolution.schema != "reel.workflow-resolution-proposal.v1"
        || resolution.baseline_report_sha256 != issue_draft.report_sha256
        || !resolution.finding_keys.contains(&text.finding_key)
        || resolution.remote_issues_closed
        || resolution.selected
        || resolution.approved
        || issue.create_operation_id != issue_draft.operation_id
        || issue.create_request_sha256 != digest(&issue_draft.payload()?)
    {
        return fail(ErrorKind::Stale, "tracker-close-resolution-binding");
    }
    let mut result = CloseDraft {
        schema: "reel.workflow-issue-close-draft.v1".into(),
        operation_id: String::new(),
        repository: issue_draft.repository.clone(),
        finding_key: text.finding_key.clone(),
        baseline_report_sha256: issue_draft.report_sha256.clone(),
        issue: issue.clone(),
        resolution_sha256: canonical_digest(resolution)?,
        expected_revision: text.expected_revision.clone(),
        note: text.note.clone(),
    };
    result.operation_id = operation(&result)?;
    result.check()?;
    Ok(result)
}
