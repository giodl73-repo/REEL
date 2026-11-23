//! Portable editorial intent. No source custody, approval or rendering proof.
use crate::*;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    Film,
    ComicIssue,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Edition {
    pub language: String,
    /// Authored editorial order; producers must check their actual controls.
    pub components: Vec<String>,
    /// Other directly consumed parents, excluded from editorial order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<String>,
    pub final_task: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub schema: String,
    pub project: String,
    pub format: Format,
    pub editions: BTreeMap<String, Edition>,
}
impl Intent {
    pub fn check(&self) -> Result<()> {
        if serde_json::to_vec(self)
            .map_err(|_| Error {
                kind: ErrorKind::Invalid,
                code: "composition-intent-json",
            })?
            .len()
            > MAX_CONTROL_BYTES
        {
            return fail(ErrorKind::Budget, "composition-intent-bytes");
        }
        if self.schema != "reel.workflow-composition-intent.v1"
            || !valid_id(&self.project)
            || self.editions.is_empty()
            || self.editions.len() > 32
        {
            return fail(ErrorKind::Invalid, "composition-intent-contract");
        }
        for (id, e) in &self.editions {
            let parents: BTreeSet<_> = e.components.iter().chain(&e.controls).collect();
            if !valid_id(id)
                || !valid_id(&e.language)
                || !valid_id(&e.final_task)
                || e.components.is_empty()
                || parents.len() != e.components.len() + e.controls.len()
                || parents.len() > 128
                || parents
                    .iter()
                    .any(|p| !valid_id(p) || p.as_str() == e.final_task)
            {
                return fail(ErrorKind::Invalid, "composition-intent-edition");
            }
        }
        Ok(())
    }
}
