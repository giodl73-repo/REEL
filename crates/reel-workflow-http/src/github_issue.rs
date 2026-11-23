//! Explicit issue submission over the existing owned HTTP operation. The pinned
//! request authorizes its text; this adapter does not authenticate a film review.
#[cfg(windows)]
use crate::Prepared;
use crate::{Config, error};
use reel_workflow::{ErrorKind, Result};
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "reel.workflow-github-issue.v1";
pub const API_VERSION: &str = "2026-03-10";
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Body {
    pub title: String,
    pub body: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub schema: String,
    pub repository: String,
    pub finding_identity: String,
    pub draft_key: String,
    pub issue: Body,
}
fn sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Spec {
    /// Bind an exported draft's canonical subject/version to an explicit target.
    /// This does not re-read the film or authenticate its original observation.
    pub fn from_draft(
        repository: String,
        draft: &reel_workflow::film_review::IssueDraft,
    ) -> Result<Self> {
        let finding = reel_workflow::film_review::Finding {
            id: draft.finding_id.clone(),
            target_id: draft.target.attachment_id.clone(),
            frame: draft.frame,
            category: draft.category,
            method: draft.method,
            observation: draft.observation.clone(),
            evidence: draft.evidence.clone(),
        };
        if draft.schema != "reel.workflow-film-issue-draft.v1"
            || draft.viewing_verified
            || draft.authority_verified
            || draft.native_retention_verified
            || draft.issue_submitted
            || draft.selection_applied
            || draft.frame < draft.target.start_frame
            || draft.frame >= draft.target.end_frame
            || draft.finding_identity
                != reel_workflow::canonical_digest(&(
                    &draft.review,
                    &draft.map,
                    &draft.receipt,
                    &draft.target,
                    &draft.finding_id,
                ))?
            || draft.key
                != reel_workflow::canonical_digest(&(
                    &draft.review,
                    &draft.map,
                    &draft.receipt,
                    &draft.target,
                    &finding,
                ))?
        {
            return Err(error(ErrorKind::Conflict, "github-issue-draft-binding"));
        }
        let spec = Self {
            schema: SCHEMA.into(),
            repository,
            finding_identity: draft.finding_identity.clone(),
            draft_key: draft.key.clone(),
            issue: Body {
                title: draft.title.clone(),
                body: draft.body.clone(),
            },
        };
        spec.validate()?;
        Ok(spec)
    }
    fn validate(&self) -> Result<()> {
        let parts: Vec<_> = self.repository.split('/').collect();
        if self.schema != SCHEMA
            || parts.len() != 2
            || parts.iter().any(|p| {
                p.is_empty()
                    || p.len() > 100
                    || *p == "."
                    || *p == ".."
                    || !p
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            })
            || !sha(&self.finding_identity)
            || !sha(&self.draft_key)
            || self.issue.title.trim().is_empty()
            || self.issue.title.len() > 256
            || self.issue.title.chars().any(char::is_control)
            || self.issue.body.len() > 32768
            || self
                .issue
                .body
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
            || self.issue.body.matches("<!-- reel-film-finding:").count() != 1
            || !self.issue.body.ends_with(&format!(
                "\nDraft version: `{}`\n\n<!-- reel-film-finding:{} -->\n",
                self.draft_key, self.finding_identity
            ))
        {
            return Err(error(ErrorKind::Invalid, "github-issue-spec"));
        }
        Ok(())
    }
    pub fn operation_id(&self) -> Result<String> {
        self.validate()?;
        // A refinement/version/config change cannot silently allocate another
        // operation for this logical finding in the original registry.
        Ok(format!(
            "gh-issue-{}",
            reel_workflow::canonical_digest(&(
                self.repository.to_ascii_lowercase(),
                &self.finding_identity
            ))?
        ))
    }
    pub fn body(&self, config: &Config) -> Result<Vec<u8>> {
        self.validate()?;
        if config.response_byte_limit == 0 || config.response_byte_limit > 1024 * 1024 {
            return Err(error(ErrorKind::Budget, "github-issue-response-allowance"));
        }
        let endpoint = reqwest::Url::parse(&config.endpoint)
            .map_err(|_| error(ErrorKind::Invalid, "github-issue-endpoint"))?;
        let official =
            config.endpoint == format!("https://api.github.com/repos/{}/issues", self.repository);
        let fixture = config.allow_loopback_http
            && endpoint.scheme() == "http"
            && matches!(endpoint.host_str(), Some("127.0.0.1") | Some("[::1]"))
            && endpoint.path() == format!("/repos/{}/issues", self.repository)
            && endpoint.username().is_empty()
            && endpoint.password().is_none()
            && endpoint.query().is_none()
            && endpoint.fragment().is_none();
        if !official && !fixture {
            return Err(error(ErrorKind::Denied, "github-issue-endpoint"));
        }
        let raw = serde_json::to_vec(&self.issue)
            .map_err(|_| error(ErrorKind::Invalid, "github-issue-json"))?;
        if raw.len() as u64 > config.request_byte_limit {
            return Err(error(ErrorKind::Budget, "github-issue-request-bytes"));
        }
        Ok(raw)
    }
    /// Only a complete, byte-verified original response enters this function in
    /// the owned runner. No retry or remote lookup is performed here.
    pub fn interpret(&self, config: &Config, status: u16, raw: &[u8]) -> Result<serde_json::Value> {
        self.body(config)?;
        if status != 201 {
            return Err(error(
                match status {
                    401 | 403 => ErrorKind::Denied,
                    409 => ErrorKind::Conflict,
                    429 => ErrorKind::Busy,
                    _ => ErrorKind::NeedsRecovery,
                },
                "github-original-operation-reconcile",
            ));
        }
        if raw.len() > 1024 * 1024 {
            return Err(error(ErrorKind::Budget, "github-issue-response-bytes"));
        }
        let value: serde_json::Value = serde_json::from_slice(raw)
            .map_err(|_| error(ErrorKind::NeedsRecovery, "github-issue-response-json"))?;
        let number = value["number"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or_else(|| error(ErrorKind::NeedsRecovery, "github-issue-response-identity"))?;
        let html = format!("https://github.com/{}/issues/{number}", self.repository);
        let api = format!(
            "https://api.github.com/repos/{}/issues/{number}",
            self.repository
        );
        if value["html_url"] != html
            || value["url"] != api
            || value["title"] != self.issue.title
            || value["body"] != self.issue.body
            || value.get("pull_request").is_some()
        {
            return Err(error(ErrorKind::Conflict, "github-issue-response-binding"));
        }
        let simulated = config.endpoint.starts_with("http://");
        Ok(
            serde_json::json!({"schema":"reel.workflow-github-issue-receipt.v1",
            "operation_id":self.operation_id()?,"repository":self.repository,
            "finding_identity":self.finding_identity,"draft_key":self.draft_key,
            "number":number,"html_url":html,"response_sha256":reel_workflow::digest(raw),
            "issue_submitted":!simulated,"simulated":simulated,"viewing_verified":false,"authority_verified":false,
            "native_retained":false,"task_completed":false,"selection_applied":false}),
        )
    }
}
#[cfg(windows)]
impl Prepared {
    pub(crate) fn github_issue_headers(mut self) -> Self {
        use reqwest::header::HeaderValue;
        let headers = self.request.headers_mut();
        headers.insert(
            "accept",
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "x-github-api-version",
            HeaderValue::from_static(API_VERSION),
        );
        headers.insert(
            "user-agent",
            HeaderValue::from_static("REEL-workflow/0.1.0"),
        );
        self
    }
}
