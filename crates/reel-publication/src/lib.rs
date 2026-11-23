//! Named artifact contracts. No renderer, storage publication or approval.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};
pub mod film_review;
pub const PUBLICATION_SCHEMA: &str = "reel.publication.v1";
pub const MAX_CONTROL_BYTES: usize = 1024 * 1024;
pub const MAX_OUTPUTS: usize = 256;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorKind {
    Invalid,
    Missing,
    Conflict,
    Budget,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub code: &'static str,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.code)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
fn fail<T>(kind: ErrorKind, code: &'static str) -> Result<T> {
    Err(Error { kind, code })
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub logical_id: String,
    pub sha256: String,
    pub bytes: u64,
    pub media_type: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OutputContract {
    pub media_type: String,
    pub max_bytes: u64,
    pub required: bool,
}
pub type OutputContracts = BTreeMap<String, OutputContract>;
pub type ArtifactSet = BTreeMap<String, Artifact>;

pub fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
fn media(value: &str) -> bool {
    value.len() <= 128
        && value.split('/').count() == 2
        && value.split('/').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&b))
        })
}
pub fn check_artifact(value: &Artifact) -> Result<()> {
    if value.logical_id.len() > 256
        || !value.logical_id.split('/').all(valid_id)
        || value.sha256.len() != 64
        || !value
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || value.bytes == 0
        || !media(&value.media_type)
    {
        return fail(ErrorKind::Invalid, "artifact-descriptor");
    }
    Ok(())
}
pub fn check_contracts(contracts: &OutputContracts) -> Result<()> {
    if contracts.is_empty() || contracts.len() > MAX_OUTPUTS {
        return fail(ErrorKind::Budget, "output-count");
    }
    let mut total = 0u64;
    for (name, output) in contracts {
        if !valid_id(name) || !media(&output.media_type) || output.max_bytes == 0 {
            return fail(ErrorKind::Invalid, "output-contract");
        }
        total = total.checked_add(output.max_bytes).ok_or(Error {
            kind: ErrorKind::Budget,
            code: "output-total-overflow",
        })?;
    }
    if !contracts.values().any(|o| o.required) {
        return fail(ErrorKind::Invalid, "required-output-missing");
    }
    Ok(())
}

/// Descriptor checks only: no payload reads or retained-ownership inference.
pub fn check_artifact_set(artifacts: &ArtifactSet) -> Result<()> {
    if artifacts.is_empty() || artifacts.len() > MAX_OUTPUTS {
        return fail(ErrorKind::Budget, "artifact-count");
    }
    let mut logical = BTreeMap::new();
    for (name, value) in artifacts {
        if !valid_id(name) {
            return fail(ErrorKind::Invalid, "output-name");
        }
        check_artifact(value)?;
        if let Some(prior) = logical.insert(&value.logical_id, value) {
            if prior != value {
                return fail(ErrorKind::Conflict, "artifact-identity-conflict");
            }
        }
    }
    Ok(())
}
pub fn check_outputs(contracts: &OutputContracts, artifacts: &ArtifactSet) -> Result<()> {
    check_contracts(contracts)?;
    check_artifact_set(artifacts)?;
    for (name, contract) in contracts {
        if contract.required && !artifacts.contains_key(name) {
            return fail(ErrorKind::Missing, "required-output");
        }
    }
    for (name, value) in artifacts {
        let contract = contracts.get(name).ok_or(Error {
            kind: ErrorKind::Invalid,
            code: "undeclared-output",
        })?;
        if value.media_type != contract.media_type {
            return fail(ErrorKind::Invalid, "output-media-type");
        }
        if value.bytes > contract.max_bytes {
            return fail(ErrorKind::Budget, "output-bytes");
        }
    }
    Ok(())
}
