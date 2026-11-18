use serde::Deserialize;
use std::path::PathBuf;
use tig_core::{Digest, identity::parse_json};
use tig_store::{
    accounted::PublicationPlan,
    ledger::{Checkpoint, JournalRef},
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub destination: Destination,
    pub admission: Admission,
    pub plan: PublicationPlan,
    pub source: super::source_config::Config,
    #[serde(default)]
    pub provider: Option<crate::provider_profile::Envelope>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    pub root: PathBuf,
    pub place: Digest,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub ledger_root: PathBuf,
    pub ledger: Digest,
    pub minimum_checkpoint: Checkpoint,
    pub job: JournalRef,
}
pub fn parse(bytes: &[u8]) -> Result<Profile, String> {
    if bytes.len() > 65_536 {
        return Err("acquisition profile byte budget exceeded".into());
    }
    let value = parse_json(bytes).map_err(|_| "invalid acquisition JSON")?;
    if value["schema"] == "tig.acquisition-profile.experimental-1"
        && value.get("provider").is_some()
    {
        return Err("provider context requires the opt-in acquisition schema".into());
    }
    let profile: Profile =
        serde_json::from_value(value).map_err(|_| "invalid acquisition fields")?;
    if !matches!(
        profile.schema.as_str(),
        "tig.acquisition-profile.experimental-1" | "tig.acquisition-profile.experimental-2"
    ) || (profile.schema.ends_with("-2") != profile.provider.is_some())
        || !profile.destination.root.is_absolute()
        || !profile.admission.ledger_root.is_absolute()
    {
        return Err("invalid acquisition profile or absolute bindings required".into());
    }
    if let Some(provider) = &profile.provider {
        provider.validate(
            &profile.destination.place,
            profile.plan.request(),
            profile.admission.job.operation(),
        )?;
    }
    profile.source.validate(&profile.plan.request().object)?;
    Ok(profile)
}
