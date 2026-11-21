//! Explicit native block/profile/category admission; metadata candidate only.
use super::*;
use git_asset::source::native::retirement;
use git_asset_model::{
    graph::{EdgeKind, NodeKind, Payloads, Profile},
    resolve::AssetResolver,
};
pub(super) fn run(
    root: &Path,
    snapshot: &mut native::AssetSnapshot,
    metadata: &mut MetadataStore,
) -> Result<Object> {
    let series = snapshot.resolve_id("work/series")?;
    let asset = series.reference().clone();
    let definition = series.definition().clone();
    let profile = Profile {
        definition_id: definition.id.clone(),
        definition: definition.object.clone(),
        class: NodeKind::Primary,
        covered: [EdgeKind::Production, EdgeKind::Composition]
            .into_iter()
            .collect(),
        rules: Vec::new(),
        payloads: Payloads::Named {
            selected: true,
            slots: ["reference-grid".to_string()].into_iter().collect(),
        },
    };
    let candidate = retirement::build(
        snapshot,
        [asset.clone()].into_iter().collect(),
        vec![profile.clone()],
        metadata,
        retirement::Limits::default(),
    )?;
    metadata.finish()?;
    let mut reader = git_asset_store::metadata::MetadataReader::open(
        &root.join("metadata"),
        &metadata.checkpoint(),
    )?;
    let inventory = retirement::inspect(
        &candidate,
        &candidate.context,
        &mut reader,
        retirement::Limits::default(),
    )?;
    let categories: retirement::Categories = [(
        "measurement-series".to_string(),
        [definition].into_iter().collect(),
    )]
    .into_iter()
    .collect();
    let filter = retirement::Filter {
        assets: BTreeSet::new(),
        categories: ["measurement-series".to_string()].into_iter().collect(),
    };
    let selected = retirement::select(&inventory, &categories, &filter)?;
    let unknown = retirement::Filter {
        assets: BTreeSet::new(),
        categories: ["unadmitted-category".to_string()].into_iter().collect(),
    };
    if retirement::select(&inventory, &categories, &unknown).is_ok()
        || selected.assets.len() != 1
        || selected.assets[0].asset != asset
        || selected.authorizes_removal
        || selected.payload_verified
        || selected.age_filter_supported
    {
        return Err("native category precursor changed authority or scope".into());
    }
    save(
        &root.join("native-category.json"),
        &json!({"schema":"tig.sample-native-category.design-1","candidate":candidate,
        "admitted_profile":profile,"admitted_categories":categories,"filter":filter,"preview":selected,
        "unknown_category_refused":true,"metadata_reopened":true,"candidate_adopted":false,
        "scope_limit":"exact original series block; category and retained age execution not yet composed"}),
    )
}
