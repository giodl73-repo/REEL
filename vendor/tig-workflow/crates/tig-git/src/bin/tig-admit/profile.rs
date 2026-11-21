use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tig_core::{Digest, identity::parse_json};
use tig_store::{Key, Request, ledger::Checkpoint};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub schema: String,
    pub destination: Destination,
    pub capacity: Capacity,
    pub request: Request,
    pub job: Key,
    pub maximum_peak: u64,
    pub source: super::source_config::Config,
    #[serde(default)]
    pub provider: Option<crate::provider_profile::Envelope>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    pub root: PathBuf,
    pub place: Digest,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capacity {
    pub root: PathBuf,
    pub ledger: Digest,
    pub minimum_checkpoint: Checkpoint,
}
pub fn parse(bytes: &[u8]) -> Result<Input, String> {
    if bytes.len() > 65_536 {
        return Err("admission input byte budget exceeded".into());
    }
    let value = parse_json(bytes).map_err(|_| "invalid admission JSON")?;
    if value["schema"] == "tig.publication-admission-profile.experimental-1"
        && value.get("provider").is_some()
    {
        return Err("provider context requires the opt-in admission schema".into());
    }
    let input: Input = serde_json::from_value(value).map_err(|_| "invalid admission fields")?;
    if !matches!(
        input.schema.as_str(),
        "tig.publication-admission-profile.experimental-1"
            | "tig.publication-admission-profile.experimental-2"
    ) || (input.schema.ends_with("-2") != input.provider.is_some())
        || !input.destination.root.is_absolute()
        || !input.capacity.root.is_absolute()
        || input.maximum_peak == 0
    {
        return Err("invalid admission profile, bounds or absolute bindings".into());
    }
    if let Some(provider) = &input.provider {
        provider.validate(&input.destination.place, &input.request, &input.job)?;
    }
    input.source.validate(&input.request.object)?;
    Ok(input)
}
