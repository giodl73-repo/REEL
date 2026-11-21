use serde::Deserialize;
use std::{collections::BTreeSet, path::PathBuf};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub work_file: PathBuf,
    pub places: Vec<String>,
    pub maximum_peak_per_action: u64,
    pub allow_cooperative: bool,
    pub previous_claims_file: Option<PathBuf>,
    pub witnesses_file: Option<PathBuf>,
    #[serde(default)]
    pub issuance: Option<super::issuer_profiles::Issuance>,
}
pub fn parse(bytes: &[u8]) -> Result<Profile, String> {
    let value = tig_core::identity::parse_json(bytes)
        .map_err(|_| "invalid or over-budget retention JSON")?;
    if value["schema"] == "tig.retained-placement-profile.experimental-1"
        && value.get("issuance").is_some()
    {
        return Err("issuer context requires the opt-in retention schema".into());
    }
    let input: Profile =
        serde_json::from_value(value).map_err(|_| "invalid retention profile fields")?;
    if !matches!(
        input.schema.as_str(),
        "tig.retained-placement-profile.experimental-1"
            | "tig.retained-placement-profile.experimental-2"
    ) || input.schema.ends_with("-2") != input.issuance.is_some()
        || !input.work_file.is_absolute()
        || input
            .previous_claims_file
            .as_ref()
            .is_some_and(|p| !p.is_absolute())
        || input
            .witnesses_file
            .as_ref()
            .is_some_and(|p| !p.is_absolute())
        || !(1..=16).contains(&input.places.len())
        || input.places.iter().any(|p| !super::work::token(p))
        || input.places.iter().collect::<BTreeSet<_>>().len() != input.places.len()
        || !input.allow_cooperative
    {
        return Err("unsupported retention profile, scope or cooperative enforcement".into());
    }
    if let Some(issuance) = &input.issuance {
        issuance.validate(&input.places)?;
    }
    Ok(input)
}
