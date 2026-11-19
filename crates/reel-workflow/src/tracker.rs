//! Explicit issue text, separated from private review context. Validation is not
//! rights approval, tracker delivery, issue closure or repair authorization.
use crate::*;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Text {
    pub schema: String,
    pub finding_key: String,
    pub repository: String,
    pub title: String,
    pub body: String,
    pub synthetic_reproduction: Option<Artifact>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub schema: String,
    pub operation_id: String,
    pub finding_key: String,
    pub report_sha256: String,
    pub repository: String,
    pub title: String,
    pub body: String,
    pub route: review::Route,
    pub synthetic_reproduction: Option<Artifact>,
}
pub fn operation(repository: &str, finding: &str) -> Result<String> {
    canonical_digest(&("reel.workflow-issue-operation.v1", repository, finding))
}
impl Draft {
    pub fn check(&self) -> Result<()> {
        canonical_digest(self)?;
        let repo: Vec<_> = self.repository.split('/').collect();
        if self.schema != "reel.workflow-issue-draft.v1"
            || repo.len() != 2
            || repo.iter().any(|s| !token(s))
            || self.repository != self.repository.to_ascii_lowercase()
            || !hex(&self.finding_key, &[64])
            || !hex(&self.report_sha256, &[64])
            || self.title.trim().is_empty()
            || self.title.len() > 256
            || self.title.chars().any(char::is_control)
            || self.body.trim().is_empty()
            || self.body.len() > 64 * 1024
            || self
                .body
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return fail(ErrorKind::Invalid, "tracker-draft-contract");
        }
        if self.operation_id != operation(&self.repository, &self.finding_key)? {
            return fail(ErrorKind::Integrity, "tracker-draft-operation");
        }
        if self.route != review::Route::Project && self.synthetic_reproduction.is_none() {
            return fail(ErrorKind::Denied, "tracker-generic-reproduction-required");
        }
        if let Some(pin) = &self.synthetic_reproduction {
            artifact(pin)?;
        }
        Ok(())
    }
    /// Only caller-authored text and routing enter the remote create payload.
    /// Private source descriptors, finding body and review evidence stay local.
    pub fn payload(&self) -> Result<Vec<u8>> {
        self.check()?;
        serde_json::to_vec(&serde_json::json!({"schema":"reel.workflow-issue-create.v1","operation_id":self.operation_id,"repository":self.repository,"title":self.title,"body":self.body}))
            .map_err(|_|Error {kind:ErrorKind::Invalid,code:"tracker-payload-json".into()})
    }
}
pub fn draft(
    report: &review::CheckedReport,
    routes: &review::Routes,
    text: &Text,
) -> Result<Draft> {
    if text.schema != "reel.workflow-issue-text.v1" {
        return fail(ErrorKind::Invalid, "tracker-text-schema");
    }
    let finding = report
        .findings()
        .get(&text.finding_key)
        .ok_or(Error {
            kind: ErrorKind::Missing,
            code: "tracker-finding".into(),
        })?
        .finding();
    let repository = match finding.route {
        review::Route::Project => &routes.project,
        review::Route::Reel => &routes.reel,
        review::Route::Tig => &routes.tig,
    }
    .to_ascii_lowercase();
    if repository != text.repository.to_ascii_lowercase() {
        return fail(ErrorKind::Conflict, "tracker-finding-route");
    }
    let draft = Draft {
        schema: "reel.workflow-issue-draft.v1".into(),
        operation_id: operation(&repository, &text.finding_key)?,
        finding_key: text.finding_key.clone(),
        report_sha256: report.report_sha256().into(),
        repository,
        title: text.title.clone(),
        body: text.body.clone(),
        route: finding.route,
        synthetic_reproduction: text.synthetic_reproduction.clone(),
    };
    draft.check()?;
    Ok(draft)
}
