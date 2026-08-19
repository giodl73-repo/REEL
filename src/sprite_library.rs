use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

pub const LIBRARY_SCHEMA: &str = "reel.sprite-library.v0.1";
pub const PROFILE_SCHEMA: &str = "reel.sprite-profile.v0.1";
pub const CAST_SCHEMA: &str = "reel.sprite-cast.v0.1";
pub const CACHE_PLAN_SCHEMA: &str = "reel.sprite-cache-plan.v0.1";
pub const COVERAGE_SCHEMA: &str = "reel.sprite-selector-coverage.v0.1";

#[derive(Debug)]
pub struct LoadedLibrary {
    pub path: PathBuf,
    pub source_sha256: String,
    pub value: SpriteLibrary,
}

#[derive(Debug)]
pub struct LoadedProfile {
    pub path: PathBuf,
    pub source_sha256: String,
    pub value: SpriteProfile,
}

#[derive(Debug)]
pub struct LoadedCast {
    pub path: PathBuf,
    pub source_sha256: String,
    pub value: SpriteCast,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpriteLibrary {
    pub schema: String,
    pub library: String,
    pub version: String,
    pub cache_namespace: String,
    pub layer_slots: Vec<LayerSlot>,
    pub poses: Vec<LibraryPose>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LayerSlot {
    pub id: String,
    pub order: i32,
    pub source: LayerSource,
    pub transform_stage: TransformStage,
    #[serde(default)]
    pub required: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayerSource {
    Pose,
    Skin,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransformStage {
    PoseSpace,
    PostTransform,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryPose {
    pub id: String,
    pub asset_recipe: String,
    pub base_facing: String,
    #[serde(default)]
    pub mirror_x_allowed: bool,
    pub layers: BTreeMap<String, String>,
    pub anchors: BTreeMap<String, Point>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpriteProfile {
    pub schema: String,
    pub profile: String,
    pub version: String,
    pub domain: String,
    pub library: String,
    pub library_sha256: String,
    pub selector_dimensions: Vec<String>,
    pub bindings: Vec<ProfileBinding>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileBinding {
    pub id: String,
    pub selectors: BTreeMap<String, String>,
    pub pose: String,
    #[serde(default)]
    pub mirror_x: bool,
    pub quality: BindingQuality,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub required_anchors: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BindingQuality {
    Exact,
    DeclaredFallback,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpriteCast {
    pub schema: String,
    pub cast: String,
    pub library: String,
    pub library_sha256: String,
    pub profile: String,
    pub profile_sha256: String,
    pub skins: BTreeMap<String, Skin>,
    pub characters: Vec<Character>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Skin {
    pub layers: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Character {
    pub id: String,
    pub stable_subject_id: String,
    pub skin: String,
    #[serde(default)]
    pub layers: BTreeMap<String, String>,
    #[serde(default)]
    pub traits: BTreeMap<String, String>,
    pub pose_requests: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Serialize)]
pub struct LibraryReport {
    pub schema: String,
    pub source_sha256: String,
    pub library: String,
    pub version: String,
    pub cache_namespace: String,
    pub layer_slots: usize,
    pub poses: usize,
    pub mirrorable_poses: usize,
    pub passed: bool,
}

#[derive(Debug, Serialize)]
pub struct ProfileReport {
    pub schema: String,
    pub source_sha256: String,
    pub profile: String,
    pub version: String,
    pub domain: String,
    pub selector_dimensions: usize,
    pub bindings: usize,
    pub declared_fallbacks: usize,
    pub passed: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CachePlan {
    pub schema: String,
    pub library: String,
    pub library_sha256: String,
    pub profile: String,
    pub profile_sha256: String,
    pub cast: String,
    pub cast_sha256: String,
    pub cache_namespace: String,
    pub resolved_requests: usize,
    pub declared_fallbacks: usize,
    pub items: Vec<ResolvedPose>,
    pub passed: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedPose {
    pub character: String,
    pub request: String,
    pub binding: String,
    pub pose: String,
    pub mirror_x: bool,
    pub quality: BindingQuality,
    pub selectors: BTreeMap<String, String>,
    pub ordered_layers: Vec<ResolvedLayer>,
    pub logical_cache_key: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedLayer {
    pub slot: String,
    pub recipe: String,
    pub transform_stage: TransformStage,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoverageSource {
    CachePlan,
    CastResolution,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequestCoverage {
    Exact,
    DeclaredFallback,
    Unresolved,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelectorCoverageReport {
    pub schema: String,
    pub source: CoverageSource,
    pub library: String,
    pub library_sha256: String,
    pub profile: String,
    pub profile_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cast: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cast_sha256: Option<String>,
    pub exact: usize,
    pub declared_fallback: usize,
    pub unresolved: usize,
    pub characters: Vec<CharacterCoverage>,
    pub complete: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterCoverage {
    pub character: String,
    pub requests: Vec<RequestCoverageCell>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequestCoverageCell {
    pub request: String,
    pub coverage: RequestCoverage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pose: Option<String>,
}

pub fn load_library(path: impl AsRef<Path>) -> Result<LoadedLibrary> {
    let path = path.as_ref();
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read sprite library {}", path.display()))?;
    let value = serde_yaml::from_str(&source)
        .with_context(|| format!("failed to parse sprite library {}", path.display()))?;
    Ok(LoadedLibrary {
        path: path.to_path_buf(),
        source_sha256: digest_bytes(source.as_bytes()),
        value,
    })
}

pub fn load_profile(path: impl AsRef<Path>) -> Result<LoadedProfile> {
    let path = path.as_ref();
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read sprite profile {}", path.display()))?;
    let value = serde_yaml::from_str(&source)
        .with_context(|| format!("failed to parse sprite profile {}", path.display()))?;
    Ok(LoadedProfile {
        path: path.to_path_buf(),
        source_sha256: digest_bytes(source.as_bytes()),
        value,
    })
}

pub fn load_cast(path: impl AsRef<Path>) -> Result<LoadedCast> {
    let path = path.as_ref();
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read sprite cast {}", path.display()))?;
    let value = serde_yaml::from_str(&source)
        .with_context(|| format!("failed to parse sprite cast {}", path.display()))?;
    Ok(LoadedCast {
        path: path.to_path_buf(),
        source_sha256: digest_bytes(source.as_bytes()),
        value,
    })
}

pub fn validate_library(loaded: &LoadedLibrary) -> Result<LibraryReport> {
    let library = &loaded.value;
    require_schema(&library.schema, LIBRARY_SCHEMA)?;
    require_token(&library.library, "library")?;
    require_token(&library.version, "library version")?;
    require_token(&library.cache_namespace, "cache namespace")?;
    require_portable_relative_path(&library.cache_namespace, "cache namespace")?;
    if library.layer_slots.is_empty() || library.poses.is_empty() {
        bail!("sprite library requires layer_slots and poses");
    }
    unique(
        library.layer_slots.iter().map(|slot| slot.id.as_str()),
        "layer slot",
    )?;
    let mut orders = BTreeSet::new();
    for slot in &library.layer_slots {
        require_token(&slot.id, "layer slot id")?;
        if !orders.insert(slot.order) {
            bail!("sprite library layer order {} is duplicated", slot.order);
        }
        if slot.transform_stage == TransformStage::PostTransform && slot.source != LayerSource::Skin
        {
            bail!(
                "post-transform layer {} must be supplied by a skin",
                slot.id
            );
        }
    }
    unique(library.poses.iter().map(|pose| pose.id.as_str()), "pose")?;
    let slots = library
        .layer_slots
        .iter()
        .map(|slot| (slot.id.as_str(), slot))
        .collect::<BTreeMap<_, _>>();
    for pose in &library.poses {
        require_token(&pose.id, "pose id")?;
        require_token(&pose.asset_recipe, "pose asset recipe")?;
        require_token(&pose.base_facing, "pose base facing")?;
        for (slot_id, recipe) in &pose.layers {
            let slot = slots.get(slot_id.as_str()).ok_or_else(|| {
                anyhow::anyhow!("pose {} uses unknown layer slot {slot_id}", pose.id)
            })?;
            if slot.source != LayerSource::Pose {
                bail!("pose {} supplies skin-owned layer {slot_id}", pose.id);
            }
            require_token(recipe, "pose layer recipe")?;
        }
        for slot in library
            .layer_slots
            .iter()
            .filter(|slot| slot.source == LayerSource::Pose && slot.required)
        {
            if !pose.layers.contains_key(&slot.id) {
                bail!("pose {} is missing required layer {}", pose.id, slot.id);
            }
        }
        if !pose.anchors.contains_key("pivot") {
            bail!("pose {} requires pivot anchor", pose.id);
        }
        for (anchor, point) in &pose.anchors {
            require_token(anchor, "anchor id")?;
            if !in_unit(point.x) || !in_unit(point.y) {
                bail!(
                    "pose {} anchor {anchor} is outside normalized space",
                    pose.id
                );
            }
        }
    }
    Ok(LibraryReport {
        schema: LIBRARY_SCHEMA.to_string(),
        source_sha256: loaded.source_sha256.clone(),
        library: library.library.clone(),
        version: library.version.clone(),
        cache_namespace: library.cache_namespace.clone(),
        layer_slots: library.layer_slots.len(),
        poses: library.poses.len(),
        mirrorable_poses: library
            .poses
            .iter()
            .filter(|pose| pose.mirror_x_allowed)
            .count(),
        passed: true,
    })
}

pub fn validate_profile(library: &LoadedLibrary, profile: &LoadedProfile) -> Result<ProfileReport> {
    validate_library(library)?;
    let value = &profile.value;
    require_schema(&value.schema, PROFILE_SCHEMA)?;
    require_token(&value.profile, "profile")?;
    require_token(&value.version, "profile version")?;
    require_token(&value.domain, "profile domain")?;
    if value.library != library.value.library || value.library_sha256 != library.source_sha256 {
        bail!("sprite profile library identity or hash does not match supplied library");
    }
    if value.selector_dimensions.is_empty() || value.bindings.is_empty() {
        bail!("sprite profile requires selector_dimensions and bindings");
    }
    unique(
        value.selector_dimensions.iter().map(String::as_str),
        "selector dimension",
    )?;
    unique(
        value.bindings.iter().map(|binding| binding.id.as_str()),
        "binding",
    )?;
    let dimensions = value
        .selector_dimensions
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let poses = library
        .value
        .poses
        .iter()
        .map(|pose| (pose.id.as_str(), pose))
        .collect::<BTreeMap<_, _>>();
    let mut selector_keys = BTreeSet::new();
    for binding in &value.bindings {
        require_token(&binding.id, "binding id")?;
        validate_selectors(&binding.selectors, &dimensions)?;
        let selector_key = canonical_map(&binding.selectors)?;
        if !selector_keys.insert(selector_key) {
            bail!("sprite profile contains duplicate selector binding");
        }
        let pose = poses.get(binding.pose.as_str()).ok_or_else(|| {
            anyhow::anyhow!(
                "binding {} references unknown pose {}",
                binding.id,
                binding.pose
            )
        })?;
        if binding.mirror_x && !pose.mirror_x_allowed {
            bail!(
                "binding {} mirrors a pose that forbids mirroring",
                binding.id
            );
        }
        if binding.quality == BindingQuality::DeclaredFallback
            && binding.reason.as_deref().is_none_or(str::is_empty)
        {
            bail!("fallback binding {} requires reason", binding.id);
        }
        for anchor in &binding.required_anchors {
            if !pose.anchors.contains_key(anchor) {
                bail!(
                    "binding {} requires missing pose anchor {anchor}",
                    binding.id
                );
            }
        }
    }
    Ok(ProfileReport {
        schema: PROFILE_SCHEMA.to_string(),
        source_sha256: profile.source_sha256.clone(),
        profile: value.profile.clone(),
        version: value.version.clone(),
        domain: value.domain.clone(),
        selector_dimensions: value.selector_dimensions.len(),
        bindings: value.bindings.len(),
        declared_fallbacks: value
            .bindings
            .iter()
            .filter(|binding| binding.quality == BindingQuality::DeclaredFallback)
            .count(),
        passed: true,
    })
}

pub fn resolve_cast(
    library: &LoadedLibrary,
    profile: &LoadedProfile,
    cast: &LoadedCast,
) -> Result<CachePlan> {
    validate_profile(library, profile)?;
    let value = &cast.value;
    require_schema(&value.schema, CAST_SCHEMA)?;
    require_token(&value.cast, "cast")?;
    if value.library != library.value.library || value.library_sha256 != library.source_sha256 {
        bail!("sprite cast library identity or hash does not match supplied library");
    }
    if value.profile != profile.value.profile || value.profile_sha256 != profile.source_sha256 {
        bail!("sprite cast profile identity or hash does not match supplied profile");
    }
    if value.skins.is_empty() || value.characters.is_empty() {
        bail!("sprite cast requires skins and characters");
    }
    unique(
        value
            .characters
            .iter()
            .map(|character| character.id.as_str()),
        "character",
    )?;
    unique(
        value
            .characters
            .iter()
            .map(|character| character.stable_subject_id.as_str()),
        "stable subject",
    )?;
    let slots = library
        .value
        .layer_slots
        .iter()
        .map(|slot| (slot.id.as_str(), slot))
        .collect::<BTreeMap<_, _>>();
    for (skin_id, skin) in &value.skins {
        require_token(skin_id, "skin id")?;
        for (slot_id, recipe) in &skin.layers {
            let slot = slots.get(slot_id.as_str()).ok_or_else(|| {
                anyhow::anyhow!("skin {skin_id} uses unknown layer slot {slot_id}")
            })?;
            if slot.source != LayerSource::Skin {
                bail!("skin {skin_id} supplies pose-owned layer {slot_id}");
            }
            require_token(recipe, "skin layer recipe")?;
        }
    }
    let dimensions = profile
        .value
        .selector_dimensions
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let poses = library
        .value
        .poses
        .iter()
        .map(|pose| (pose.id.as_str(), pose))
        .collect::<BTreeMap<_, _>>();
    let mut items = Vec::new();
    for character in &value.characters {
        require_token(&character.id, "character id")?;
        require_token(&character.stable_subject_id, "stable subject id")?;
        let skin = value.skins.get(&character.skin).ok_or_else(|| {
            anyhow::anyhow!(
                "character {} uses unknown skin {}",
                character.id,
                character.skin
            )
        })?;
        for (slot_id, recipe) in &character.layers {
            let slot = slots.get(slot_id.as_str()).ok_or_else(|| {
                anyhow::anyhow!(
                    "character {} uses unknown layer slot {slot_id}",
                    character.id
                )
            })?;
            if slot.source != LayerSource::Skin {
                bail!(
                    "character {} supplies pose-owned layer {slot_id}",
                    character.id
                );
            }
            require_token(recipe, "character layer recipe")?;
        }
        for slot in library
            .value
            .layer_slots
            .iter()
            .filter(|slot| slot.source == LayerSource::Skin && slot.required)
        {
            if !character.layers.contains_key(&slot.id) && !skin.layers.contains_key(&slot.id) {
                bail!(
                    "character {} and skin {} are missing required layer {}",
                    character.id,
                    character.skin,
                    slot.id
                );
            }
        }
        for (request_id, selectors) in &character.pose_requests {
            require_token(request_id, "pose request id")?;
            let mut effective = character.traits.clone();
            for (key, selector) in selectors {
                effective.insert(key.clone(), selector.clone());
            }
            validate_selectors(&effective, &dimensions)?;
            let matches = profile
                .value
                .bindings
                .iter()
                .filter(|binding| binding.selectors == effective)
                .collect::<Vec<_>>();
            if matches.len() != 1 {
                bail!(
                    "character {} request {request_id} resolved {} bindings; expected exactly one",
                    character.id,
                    matches.len()
                );
            }
            let binding = matches[0];
            let pose = poses[&binding.pose.as_str()];
            let ordered_layers = resolve_layers(&library.value, pose, skin, &character.layers)?;
            let key_input = serde_json::json!({
                "library_sha256": library.source_sha256,
                "profile_sha256": profile.source_sha256,
                "cast_sha256": cast.source_sha256,
                "character": character.id,
                "stable_subject_id": character.stable_subject_id,
                "request": request_id,
                "binding": binding.id,
                "pose": binding.pose,
                "mirror_x": binding.mirror_x,
                "selectors": effective,
                "layers": ordered_layers,
            });
            let digest = Sha256::digest(serde_json::to_vec(&key_input)?);
            let digest_hex = digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            items.push(ResolvedPose {
                character: character.id.clone(),
                request: request_id.clone(),
                binding: binding.id.clone(),
                pose: binding.pose.clone(),
                mirror_x: binding.mirror_x,
                quality: binding.quality,
                selectors: effective,
                ordered_layers,
                logical_cache_key: format!(
                    "{}/composites/{digest_hex}",
                    library.value.cache_namespace
                ),
            });
        }
    }
    items.sort_by(|left, right| {
        (&left.character, &left.request).cmp(&(&right.character, &right.request))
    });
    Ok(CachePlan {
        schema: CACHE_PLAN_SCHEMA.to_string(),
        library: library.value.library.clone(),
        library_sha256: library.source_sha256.clone(),
        profile: profile.value.profile.clone(),
        profile_sha256: profile.source_sha256.clone(),
        cast: value.cast.clone(),
        cast_sha256: cast.source_sha256.clone(),
        cache_namespace: library.value.cache_namespace.clone(),
        resolved_requests: items.len(),
        declared_fallbacks: items
            .iter()
            .filter(|item| item.quality == BindingQuality::DeclaredFallback)
            .count(),
        items,
        passed: true,
    })
}

fn digest_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn write_cache_plan(plan: &CachePlan, output: impl AsRef<Path>) -> Result<()> {
    let output = output.as_ref();
    if output.exists() {
        bail!("refusing to overwrite {}", output.display());
    }
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(serde_json::to_string_pretty(plan)?.as_bytes())?;
    temporary.write_all(b"\n")?;
    temporary
        .persist_noclobber(output)
        .map_err(|error| error.error)?;
    Ok(())
}

pub fn coverage_from_cache_plan(
    cache_plan_path: impl AsRef<Path>,
) -> Result<SelectorCoverageReport> {
    let cache_plan_path = cache_plan_path.as_ref();
    let plan: CachePlan =
        serde_yaml::from_slice(&fs::read(cache_plan_path)?).with_context(|| {
            format!(
                "failed to parse sprite cache plan {}",
                cache_plan_path.display()
            )
        })?;
    require_schema(&plan.schema, CACHE_PLAN_SCHEMA)?;
    if !plan.passed {
        bail!("sprite selector coverage requires a passing cache plan");
    }
    let mut matrix: BTreeMap<String, Vec<RequestCoverageCell>> = BTreeMap::new();
    let mut keys = BTreeSet::new();
    for item in plan.items {
        if !keys.insert((item.character.clone(), item.request.clone())) {
            bail!(
                "sprite cache plan repeats character {} request {}",
                item.character,
                item.request
            );
        }
        let coverage = match item.quality {
            BindingQuality::Exact => RequestCoverage::Exact,
            BindingQuality::DeclaredFallback => RequestCoverage::DeclaredFallback,
        };
        matrix
            .entry(item.character)
            .or_default()
            .push(RequestCoverageCell {
                request: item.request,
                coverage,
                binding: Some(item.binding),
                pose: Some(item.pose),
            });
    }
    Ok(selector_coverage_report(
        CoverageIdentity {
            source: CoverageSource::CachePlan,
            library: plan.library,
            library_sha256: plan.library_sha256,
            profile: plan.profile,
            profile_sha256: plan.profile_sha256,
            cast: Some(plan.cast),
            cast_sha256: Some(plan.cast_sha256),
        },
        matrix,
    ))
}

pub fn coverage_from_cast(
    library: &LoadedLibrary,
    profile: &LoadedProfile,
    cast: &LoadedCast,
) -> Result<SelectorCoverageReport> {
    validate_profile(library, profile)?;
    let value = &cast.value;
    require_schema(&value.schema, CAST_SCHEMA)?;
    require_token(&value.cast, "cast")?;
    if value.library != library.value.library || value.library_sha256 != library.source_sha256 {
        bail!("sprite cast library identity or hash does not match supplied library");
    }
    if value.profile != profile.value.profile || value.profile_sha256 != profile.source_sha256 {
        bail!("sprite cast profile identity or hash does not match supplied profile");
    }
    if value.skins.is_empty() || value.characters.is_empty() {
        bail!("sprite cast requires skins and characters");
    }
    unique(
        value
            .characters
            .iter()
            .map(|character| character.id.as_str()),
        "character",
    )?;
    unique(
        value
            .characters
            .iter()
            .map(|character| character.stable_subject_id.as_str()),
        "stable subject",
    )?;
    let slots = library
        .value
        .layer_slots
        .iter()
        .map(|slot| (slot.id.as_str(), slot))
        .collect::<BTreeMap<_, _>>();
    for (skin_id, skin) in &value.skins {
        require_token(skin_id, "skin id")?;
        for (slot_id, recipe) in &skin.layers {
            let slot = slots.get(slot_id.as_str()).ok_or_else(|| {
                anyhow::anyhow!("skin {skin_id} uses unknown layer slot {slot_id}")
            })?;
            if slot.source != LayerSource::Skin {
                bail!("skin {skin_id} supplies pose-owned layer {slot_id}");
            }
            require_token(recipe, "skin layer recipe")?;
        }
    }
    let dimensions = profile
        .value
        .selector_dimensions
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let poses = library
        .value
        .poses
        .iter()
        .map(|pose| (pose.id.as_str(), pose))
        .collect::<BTreeMap<_, _>>();
    let mut matrix = BTreeMap::new();
    for character in &value.characters {
        require_token(&character.id, "character id")?;
        require_token(&character.stable_subject_id, "stable subject id")?;
        let skin = value.skins.get(&character.skin).ok_or_else(|| {
            anyhow::anyhow!(
                "character {} uses unknown skin {}",
                character.id,
                character.skin
            )
        })?;
        for (slot_id, recipe) in &character.layers {
            let slot = slots.get(slot_id.as_str()).ok_or_else(|| {
                anyhow::anyhow!(
                    "character {} uses unknown layer slot {slot_id}",
                    character.id
                )
            })?;
            if slot.source != LayerSource::Skin {
                bail!(
                    "character {} supplies pose-owned layer {slot_id}",
                    character.id
                );
            }
            require_token(recipe, "character layer recipe")?;
        }
        for slot in library
            .value
            .layer_slots
            .iter()
            .filter(|slot| slot.source == LayerSource::Skin && slot.required)
        {
            if !character.layers.contains_key(&slot.id) && !skin.layers.contains_key(&slot.id) {
                bail!(
                    "character {} and skin {} are missing required layer {}",
                    character.id,
                    character.skin,
                    slot.id
                );
            }
        }
        let mut requests = Vec::new();
        for (request_id, selectors) in &character.pose_requests {
            require_token(request_id, "pose request id")?;
            let mut effective = character.traits.clone();
            for (key, selector) in selectors {
                effective.insert(key.clone(), selector.clone());
            }
            validate_selectors(&effective, &dimensions)?;
            let matches = profile
                .value
                .bindings
                .iter()
                .filter(|binding| binding.selectors == effective)
                .collect::<Vec<_>>();
            let cell = match matches.as_slice() {
                [] => RequestCoverageCell {
                    request: request_id.clone(),
                    coverage: RequestCoverage::Unresolved,
                    binding: None,
                    pose: None,
                },
                [binding] => {
                    let pose = poses[binding.pose.as_str()];
                    resolve_layers(&library.value, pose, skin, &character.layers)?;
                    RequestCoverageCell {
                        request: request_id.clone(),
                        coverage: match binding.quality {
                            BindingQuality::Exact => RequestCoverage::Exact,
                            BindingQuality::DeclaredFallback => RequestCoverage::DeclaredFallback,
                        },
                        binding: Some(binding.id.clone()),
                        pose: Some(binding.pose.clone()),
                    }
                }
                _ => bail!(
                    "character {} request {request_id} resolves multiple bindings",
                    character.id
                ),
            };
            requests.push(cell);
        }
        matrix.insert(character.id.clone(), requests);
    }
    Ok(selector_coverage_report(
        CoverageIdentity {
            source: CoverageSource::CastResolution,
            library: library.value.library.clone(),
            library_sha256: library.source_sha256.clone(),
            profile: profile.value.profile.clone(),
            profile_sha256: profile.source_sha256.clone(),
            cast: Some(value.cast.clone()),
            cast_sha256: Some(cast.source_sha256.clone()),
        },
        matrix,
    ))
}

pub fn write_coverage_report(
    report: &SelectorCoverageReport,
    output: impl AsRef<Path>,
) -> Result<()> {
    let output = output.as_ref();
    if output.exists() {
        bail!("refusing to overwrite {}", output.display());
    }
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(serde_json::to_string_pretty(report)?.as_bytes())?;
    temporary.write_all(b"\n")?;
    temporary
        .persist_noclobber(output)
        .map_err(|error| error.error)?;
    Ok(())
}

struct CoverageIdentity {
    source: CoverageSource,
    library: String,
    library_sha256: String,
    profile: String,
    profile_sha256: String,
    cast: Option<String>,
    cast_sha256: Option<String>,
}

fn selector_coverage_report(
    identity: CoverageIdentity,
    mut matrix: BTreeMap<String, Vec<RequestCoverageCell>>,
) -> SelectorCoverageReport {
    let mut exact = 0;
    let mut declared_fallback = 0;
    let mut unresolved = 0;
    let mut characters = Vec::new();
    for (character, requests) in matrix.iter_mut() {
        requests.sort_by(|left, right| left.request.cmp(&right.request));
        for request in requests.iter() {
            match request.coverage {
                RequestCoverage::Exact => exact += 1,
                RequestCoverage::DeclaredFallback => declared_fallback += 1,
                RequestCoverage::Unresolved => unresolved += 1,
            }
        }
        characters.push(CharacterCoverage {
            character: character.clone(),
            requests: std::mem::take(requests),
        });
    }
    SelectorCoverageReport {
        schema: COVERAGE_SCHEMA.to_string(),
        source: identity.source,
        library: identity.library,
        library_sha256: identity.library_sha256,
        profile: identity.profile,
        profile_sha256: identity.profile_sha256,
        cast: identity.cast,
        cast_sha256: identity.cast_sha256,
        exact,
        declared_fallback,
        unresolved,
        characters,
        complete: unresolved == 0,
    }
}

fn resolve_layers(
    library: &SpriteLibrary,
    pose: &LibraryPose,
    skin: &Skin,
    character_layers: &BTreeMap<String, String>,
) -> Result<Vec<ResolvedLayer>> {
    let mut slots = library.layer_slots.iter().collect::<Vec<_>>();
    slots.sort_by_key(|slot| slot.order);
    slots
        .into_iter()
        .filter_map(|slot| {
            let recipe = match slot.source {
                LayerSource::Pose => pose.layers.get(&slot.id),
                LayerSource::Skin => character_layers
                    .get(&slot.id)
                    .or_else(|| skin.layers.get(&slot.id)),
            }?;
            Some(Ok(ResolvedLayer {
                slot: slot.id.clone(),
                recipe: recipe.clone(),
                transform_stage: slot.transform_stage,
            }))
        })
        .collect()
}

fn validate_selectors(
    selectors: &BTreeMap<String, String>,
    dimensions: &BTreeSet<&str>,
) -> Result<()> {
    if selectors.len() != dimensions.len() {
        bail!("selector set must supply every profile dimension exactly once");
    }
    for (key, value) in selectors {
        if !dimensions.contains(key.as_str()) {
            bail!("selector uses unknown dimension {key}");
        }
        require_token(value, "selector value")?;
    }
    Ok(())
}

fn canonical_map(value: &BTreeMap<String, String>) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn require_schema(actual: &str, expected: &str) -> Result<()> {
    if actual != expected {
        bail!("unsupported schema {actual}; expected {expected}");
    }
    Ok(())
}

fn require_token(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} cannot be empty");
    }
    Ok(())
}

fn require_portable_relative_path(value: &str, field: &str) -> Result<()> {
    if value.contains('\\')
        || value.starts_with('/')
        || value.ends_with('/')
        || value.split('/').any(|part| {
            part.is_empty()
                || matches!(part, "." | "..")
                || part.contains(':')
                || part.chars().any(char::is_control)
        })
    {
        bail!("{field} must be a portable slash-separated relative path");
    }
    Ok(())
}

fn unique<'a>(values: impl Iterator<Item = &'a str>, label: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            bail!("duplicate {label} {value}");
        }
    }
    Ok(())
}

fn in_unit(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}
