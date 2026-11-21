#[path = "../tig-remove/execution.rs"]
mod execution;
#[path = "../tig-remove/profile.rs"]
mod profile;
pub fn apply(
    value: &serde_json::Value,
    authority: Option<std::sync::Arc<tig_store::retirement_intent::policy::NativeAuthority>>,
) -> Result<(serde_json::Value, u8), String> {
    let input = super::codec::encode(value)?;
    let parsed = profile::parse(&input)?;
    let (out, code) = if authority.is_some() {
        execution::execute_authorized(parsed, authority)?
    } else {
        execution::execute(parsed)?
    };
    Ok((
        serde_json::from_slice(&out)
            .map_err(|_| "invalid original removal executor output; effects may exist")?,
        code,
    ))
}
