//! Explicit namespace admission; never manufacture or infer an asset identity.
use super::*;

pub(super) fn validate(profile: &Profile, base: &Bindings) -> Result<()> {
    if profile
        .primary_ids
        .keys()
        .any(|key| !base.assets.contains_key(key))
        || profile
            .language_primary_ids
            .keys()
            .any(|tag| !profile.language_bindings_assets.contains_key(tag))
    {
        bail!("canonical identity map names an unknown binding or language");
    }
    for id in profile.primary_ids.values().chain(
        profile
            .language_primary_ids
            .values()
            .flat_map(|map| map.values()),
    ) {
        if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
            bail!("invalid canonical asset ID");
        }
    }
    Ok(())
}
pub(super) fn resolve(
    profile: &Profile,
    base: &Bindings,
    key: &str,
    original: &AssetRef,
    language: Option<&str>,
) -> Result<AssetRef> {
    let mut asset = original.clone();
    let explicit = language
        .and_then(|tag| profile.language_primary_ids.get(tag))
        .and_then(|map| map.get(key));
    let shared = base
        .assets
        .get(key)
        .is_some_and(|a| a.logical_id == original.logical_id);
    if let Some(id) = explicit.or_else(|| {
        if shared {
            profile.primary_ids.get(key)
        } else {
            None
        }
    }) {
        asset.logical_id.clone_from(id);
    }
    Ok(asset)
}
pub(super) fn provenance(
    metadata: &mut Map<String, Value>,
    original: &AssetRef,
    canonical: &AssetRef,
) {
    if original.logical_id != canonical.logical_id {
        metadata["native_registration"]["source_logical_id"] = json!(original.logical_id);
    }
}
