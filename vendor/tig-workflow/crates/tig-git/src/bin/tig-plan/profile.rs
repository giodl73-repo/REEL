use serde::Deserialize;
use std::{collections::BTreeSet, path::PathBuf};
use tig_core::{LogicalPath, Object, identity::parse_json};
use tig_git::source::{GitId, Scope};
pub const MAX_BYTES: usize = 65_536;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub git_executable: PathBuf,
    pub repository: PathBuf,
    pub selections: Vec<Selection>,
    #[serde(default)]
    pub supplied_copies: Vec<Copy>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub id: String,
    pub commit: String,
    pub scope: Vec<LogicalPath>,
    pub places: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Copy {
    pub object: Object,
    pub place: String,
}
fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
}
pub fn parse(bytes: &[u8]) -> Result<Profile, &'static str> {
    if bytes.len() > MAX_BYTES {
        return Err("profile byte budget exceeded");
    }
    let profile: Profile =
        serde_json::from_value(parse_json(bytes).map_err(|_| "invalid profile JSON")?)
            .map_err(|_| "invalid profile fields")?;
    if profile.schema != "tig.placement-inspection-profile.experimental-1"
        || !profile.git_executable.is_absolute()
        || !profile.repository.is_absolute()
        || !(1..=8).contains(&profile.selections.len())
        || profile.supplied_copies.len() > 256
    {
        return Err("unsupported or out-of-bounds inspection profile");
    }
    let mut names = BTreeSet::new();
    for selection in &profile.selections {
        if !token(&selection.id)
            || !names.insert(&selection.id)
            || !(1..=8).contains(&selection.places.len())
            || selection.places.iter().any(|p| !token(p))
            || selection.places.iter().collect::<BTreeSet<_>>().len() != selection.places.len()
            || !(1..=32).contains(&selection.scope.len())
        {
            return Err("invalid or duplicate selection");
        }
        GitId::parse(&selection.commit).map_err(|_| "exact commit ID required")?;
        Scope::new(selection.scope.clone()).map_err(|_| "invalid logical scope")?;
    }
    if profile.supplied_copies.iter().any(|p| !token(&p.place)) {
        return Err("invalid supplied copy place");
    }
    Ok(profile)
}
