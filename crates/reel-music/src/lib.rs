pub mod analysis;
pub mod comparison;
pub mod edl;
pub mod evidence;
pub mod export;
pub mod hash;
pub mod interchange;
pub mod language_adaptation;
pub mod model;
pub mod model_draft;
pub mod neutral;
pub mod repair;
pub mod repair_candidate;
pub mod repair_intent;
pub mod semantic_import;
pub mod source;
pub mod time;

use std::collections::BTreeSet;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionRef {
    pub artifact_id: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityRef {
    pub namespace: String,
    pub artifact_id: String,
    pub content_sha256: String,
    pub status: String,
    pub required_roles: Vec<String>,
    #[serde(default)]
    pub decision_refs: Vec<DecisionRef>,
}

pub(crate) fn validate_authority(authority: &AuthorityRef) -> Result<()> {
    nonempty("authority.namespace", &authority.namespace)?;
    nonempty("authority.artifact_id", &authority.artifact_id)?;
    validate_sha256("authority.content_sha256", &authority.content_sha256)?;
    nonempty("authority.status", &authority.status)?;
    unique_nonempty("authority.required_roles", &authority.required_roles)?;
    if authority.required_roles.is_empty() {
        bail!("authority.required_roles must not be empty");
    }
    let mut decision_ids = BTreeSet::new();
    for decision in &authority.decision_refs {
        nonempty(
            "authority.decision_refs[].artifact_id",
            &decision.artifact_id,
        )?;
        validate_sha256("authority.decision_refs[].sha256", &decision.sha256)?;
        if !decision_ids.insert(&decision.artifact_id) {
            bail!("authority.decision_refs artifact ids must be unique");
        }
    }
    if status_requires_decision(&authority.status) && authority.decision_refs.is_empty() {
        bail!(
            "authority.status {} requires decision_refs",
            authority.status
        );
    }
    Ok(())
}

pub(crate) fn status_requires_decision(status: &str) -> bool {
    matches!(status, "reviewed" | "approved" | "selected" | "released")
}

pub(crate) fn nonempty(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(())
}

pub(crate) fn validate_sha256(field: &str, value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{field} must be a 64-character SHA-256 value");
    }
    Ok(())
}

pub(crate) fn unique_nonempty(field: &str, values: &[String]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        nonempty(field, value)?;
        if !seen.insert(value) {
            bail!("{field} must be unique; duplicate: {value}");
        }
    }
    Ok(())
}
