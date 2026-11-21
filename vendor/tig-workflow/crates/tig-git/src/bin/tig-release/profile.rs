use serde::Deserialize;
use std::path::PathBuf;
use tig_core::{
    Digest, Object,
    identity::{canonical_json, parse_json},
};
use tig_store::{Key, ledger, metadata, retirement_intent::progress::ClaimProtocol, source_claims};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    pub root: PathBuf,
    pub identity: Digest,
    pub minimum_checkpoint: source_claims::Checkpoint,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub root: PathBuf,
    pub minimum_checkpoint: metadata::Checkpoint,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capacity {
    pub root: PathBuf,
    pub ledger: Digest,
    pub minimum_checkpoint: ledger::Checkpoint,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Native {
    pub root: PathBuf,
    pub place: Digest,
    pub capacity: Capacity,
}
impl Native {
    fn validate(&self) -> Result<(), String> {
        if !self.root.is_absolute() || !self.capacity.root.is_absolute() {
            return Err("absolute native bindings required".into());
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Mode {
    Retirement,
    Handoff { successor: Native },
}
impl Mode {
    pub fn protocol(&self) -> ClaimProtocol {
        match self {
            Self::Retirement => ClaimProtocol::Retirement,
            Self::Handoff { .. } => ClaimProtocol::Handoff,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub owner: Key,
    pub subject: Digest,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub enrollment: Enrollment,
    pub operation: Digest,
    pub decision: Digest,
    pub object: Object,
    pub attempt: u64,
    pub claim: Digest,
    pub source: Native,
    pub mode: Mode,
    pub journal: Authority,
    pub metadata: Metadata,
    pub maximum_object_bytes: u64,
    pub allow_cooperative: bool,
    pub intent_execution_profile: Option<Digest>,
}
pub struct Parsed {
    pub profile: Profile,
    pub digest: Digest,
}
pub fn parse(bytes: &[u8]) -> Result<Parsed, String> {
    if bytes.len() > 61_440 {
        return Err("claim-work profile byte budget".into());
    }
    let value = parse_json(bytes).map_err(|_| "invalid/over-budget claim-work JSON")?;
    let profiled = match value["schema"].as_str() {
        Some("tig.claim-execution-profile.experimental-1") => {
            if value.get("intent_execution_profile").is_some() {
                return Err("legacy claim input cannot name execution context".into());
            }
            false
        }
        Some("tig.claim-execution-profile.experimental-2") => true,
        _ => return Err("unsupported claim-work schema".into()),
    };
    let digest = Digest::blob(
        &canonical_json(&serde_json::json!([
            if profiled {
                "tig.claim-execution-input.experimental-2"
            } else {
                "tig.claim-execution-input.experimental-1"
            },
            value
        ]))
        .map_err(|_| "claim input fingerprint budget")?,
    );
    let p: Profile =
        serde_json::from_value(value).map_err(|_| "invalid claim-work profile fields")?;
    if (profiled && p.intent_execution_profile.is_none())
        || !p.allow_cooperative
        || p.attempt == 0
        || p.maximum_object_bytes == 0
        || p.object.bytes > p.maximum_object_bytes
        || !p.journal.root.is_absolute()
        || !p.metadata.root.is_absolute()
    {
        return Err("unsupported claim-work profile or limits".into());
    }
    p.source.validate()?;
    if let Mode::Handoff { successor } = &p.mode {
        successor.validate()?;
        if successor.place == p.source.place {
            return Err("handoff successor must be a distinct place".into());
        }
    }
    Ok(Parsed { profile: p, digest })
}
