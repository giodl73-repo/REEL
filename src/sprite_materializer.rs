use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Cursor, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use image::{DynamicImage, GenericImageView, ImageFormat, RgbaImage, imageops};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::{
    production,
    sprite_library::{CACHE_PLAN_SCHEMA, CachePlan, TransformStage},
};

pub const CATALOG_SCHEMA: &str = "reel.sprite-recipe-catalog.v0.1";
pub const RECEIPT_SCHEMA: &str = "reel.sprite-materialization-receipt.v0.1";

#[derive(Debug)]
pub struct LoadedCatalog {
    pub path: PathBuf,
    pub source_sha256: String,
    pub value: RecipeCatalog,
}

#[derive(Debug)]
pub struct LoadedCachePlan {
    pub path: PathBuf,
    pub source_sha256: String,
    pub value: CachePlan,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeCatalog {
    pub schema: String,
    pub library: String,
    pub library_sha256: String,
    pub recipes: BTreeMap<String, RecipeSource>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MirrorBehavior {
    #[default]
    Inherit,
    Preserve,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RecipeSource {
    Transparent,
    Asset {
        path: String,
        sha256: String,
        #[serde(default)]
        variants: BTreeMap<String, AssetVariant>,
        #[serde(default)]
        mirror_behavior: MirrorBehavior,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetVariant {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RasterParameters {
    pub width: u32,
    pub height: u32,
    pub fit: String,
    pub color_space: String,
    pub alpha: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MaterializationReceipt {
    pub schema: String,
    pub cache_plan_sha256: String,
    pub catalog_sha256: String,
    pub library: String,
    pub library_sha256: String,
    pub parameters: RasterParameters,
    pub outputs: Vec<MaterializedOutput>,
    pub reused_outputs: usize,
    pub written_outputs: usize,
    pub passed: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MaterializedOutput {
    pub character: String,
    pub request: String,
    pub composition_cache_key: String,
    pub raster_cache_key: String,
    #[serde(default)]
    pub source_fingerprint_sha256: String,
    pub sha256: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub reused: bool,
}

#[derive(Debug, Serialize)]
pub struct ContactSheetReport {
    pub schema: String,
    pub materialization_receipt_sha256: String,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub columns: u32,
    pub cells: Vec<ContactSheetCell>,
    pub passed: bool,
}

#[derive(Debug, Serialize)]
pub struct ContactSheetCell {
    pub index: usize,
    pub character: String,
    pub request: String,
    pub raster_cache_key: String,
}

pub fn load_catalog(path: impl AsRef<Path>) -> Result<LoadedCatalog> {
    let path = path.as_ref();
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read sprite recipe catalog {}", path.display()))?;
    let value = serde_yaml::from_str(&source)
        .with_context(|| format!("failed to parse sprite recipe catalog {}", path.display()))?;
    Ok(LoadedCatalog {
        path: path.to_path_buf(),
        source_sha256: digest_bytes(source.as_bytes()),
        value,
    })
}

pub fn load_cache_plan(path: impl AsRef<Path>) -> Result<LoadedCachePlan> {
    let path = path.as_ref();
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read sprite cache plan {}", path.display()))?;
    let value = serde_json::from_str(&source)
        .with_context(|| format!("failed to parse sprite cache plan {}", path.display()))?;
    Ok(LoadedCachePlan {
        path: path.to_path_buf(),
        source_sha256: digest_bytes(source.as_bytes()),
        value,
    })
}

pub fn validate_catalog(catalog: &LoadedCatalog, plan: &LoadedCachePlan) -> Result<()> {
    if catalog.value.schema != CATALOG_SCHEMA {
        bail!("unsupported recipe catalog schema {}", catalog.value.schema);
    }
    if plan.value.schema != CACHE_PLAN_SCHEMA || !plan.value.passed {
        bail!("materialization requires a passing sprite cache plan");
    }
    if catalog.value.library != plan.value.library
        || catalog.value.library_sha256 != plan.value.library_sha256
    {
        bail!("recipe catalog library identity or hash does not match cache plan");
    }
    let required = plan
        .value
        .items
        .iter()
        .flat_map(|item| {
            item.ordered_layers
                .iter()
                .map(|layer| layer.recipe.as_str())
        })
        .collect::<BTreeSet<_>>();
    for recipe in required {
        let source = catalog
            .value
            .recipes
            .get(recipe)
            .ok_or_else(|| anyhow::anyhow!("recipe catalog is missing {recipe}"))?;
        validate_source(catalog, recipe, source)?;
    }
    Ok(())
}

pub fn materialize(
    catalog: &LoadedCatalog,
    plan: &LoadedCachePlan,
    output_root: impl AsRef<Path>,
    width: u32,
    height: u32,
) -> Result<MaterializationReceipt> {
    validate_catalog(catalog, plan)?;
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        bail!("sprite raster dimensions must be between 1 and 4096 pixels");
    }
    let parameters = RasterParameters {
        width,
        height,
        fit: "contain".to_string(),
        color_space: "srgb".to_string(),
        alpha: "straight".to_string(),
    };
    let output_root = output_root.as_ref();
    fs::create_dir_all(output_root)?;
    let mut outputs = Vec::new();
    let mut reused_outputs = 0;
    let mut written_outputs = 0;
    for item in &plan.value.items {
        let source_fingerprint_sha256 = source_fingerprint(catalog, item)?;
        let raster_input = serde_json::json!({
            "composition_cache_key": item.logical_cache_key,
            "source_fingerprint_sha256": source_fingerprint_sha256,
            "parameters": parameters,
        });
        let raster_digest = digest_bytes(&serde_json::to_vec(&raster_input)?);
        let raster_cache_key = format!("{}/rasters/{raster_digest}", item.logical_cache_key);
        let output = output_root
            .join(path_from_logical_key(&raster_cache_key)?)
            .with_extension("png");
        let png = render_item(catalog, item, width, height)?;
        let expected_sha = digest_bytes(&png);
        let reused = if output.exists() {
            let existing_sha = production::sha256_path(&output)?;
            if existing_sha != expected_sha {
                bail!("existing cache output hash mismatch for {raster_cache_key}");
            }
            reused_outputs += 1;
            true
        } else {
            match write_atomic(&output, &png) {
                Ok(()) => {
                    written_outputs += 1;
                    false
                }
                Err(error) if output.exists() => {
                    let winning_sha = production::sha256_path(&output)?;
                    if winning_sha != expected_sha {
                        return Err(error).with_context(|| {
                            format!("concurrent cache output hash mismatch for {raster_cache_key}")
                        });
                    }
                    reused_outputs += 1;
                    true
                }
                Err(error) => return Err(error),
            }
        };
        outputs.push(MaterializedOutput {
            character: item.character.clone(),
            request: item.request.clone(),
            composition_cache_key: item.logical_cache_key.clone(),
            raster_cache_key,
            source_fingerprint_sha256,
            sha256: expected_sha,
            bytes: png.len() as u64,
            width,
            height,
            reused,
        });
    }
    Ok(MaterializationReceipt {
        schema: RECEIPT_SCHEMA.to_string(),
        cache_plan_sha256: plan.source_sha256.clone(),
        catalog_sha256: catalog.source_sha256.clone(),
        library: plan.value.library.clone(),
        library_sha256: plan.value.library_sha256.clone(),
        parameters,
        outputs,
        reused_outputs,
        written_outputs,
        passed: true,
    })
}

pub fn write_receipt(receipt: &MaterializationReceipt, output: impl AsRef<Path>) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(receipt)?;
    write_new_atomic(output.as_ref(), &[bytes.as_slice(), b"\n"].concat())
}

pub fn create_contact_sheet(
    receipt_path: impl AsRef<Path>,
    cache_root: impl AsRef<Path>,
    output: impl AsRef<Path>,
    columns: u32,
    tile_size: u32,
) -> Result<ContactSheetReport> {
    if columns == 0 || !(32..=1024).contains(&tile_size) {
        bail!("contact sheet requires columns > 0 and tile size from 32 to 1024");
    }
    let receipt_path = receipt_path.as_ref();
    let receipt_bytes = fs::read(receipt_path)
        .with_context(|| format!("failed to read {}", receipt_path.display()))?;
    let receipt: MaterializationReceipt = serde_json::from_slice(&receipt_bytes)?;
    if receipt.schema != RECEIPT_SCHEMA || !receipt.passed || receipt.outputs.is_empty() {
        bail!("contact sheet requires a passing, non-empty materialization receipt");
    }
    let output_count = u32::try_from(receipt.outputs.len())
        .map_err(|_| anyhow::anyhow!("contact sheet contains too many outputs"))?;
    let rows = output_count.div_ceil(columns);
    let width = columns
        .checked_mul(tile_size)
        .ok_or_else(|| anyhow::anyhow!("contact sheet width overflows"))?;
    let height = rows
        .checked_mul(tile_size)
        .ok_or_else(|| anyhow::anyhow!("contact sheet height overflows"))?;
    if width > 16_384 || height > 16_384 || u64::from(width) * u64::from(height) > 64 * 1024 * 1024
    {
        bail!("contact sheet exceeds the 16384px or 64-megapixel safety limit");
    }
    let mut sheet = checkerboard(width, height, 16);
    let cache_root = cache_root.as_ref();
    let mut cells = Vec::new();
    for (index, item) in receipt.outputs.iter().enumerate() {
        let source = cache_root
            .join(path_from_logical_key(&item.raster_cache_key)?)
            .with_extension("png");
        let source_bytes = fs::read(&source)?;
        let source_sha256 = digest_bytes(&source_bytes);
        if source_sha256 != item.sha256 {
            bail!("cache output hash mismatch for {}", item.raster_cache_key);
        }
        let image = image::load_from_memory(&source_bytes)
            .with_context(|| format!("failed to decode cache output {}", source.display()))?;
        let resized = image.resize(tile_size, tile_size, imageops::FilterType::Lanczos3);
        let column = index as u32 % columns;
        let row = index as u32 / columns;
        let x = i64::from(column * tile_size + (tile_size - resized.width()) / 2);
        let y = i64::from(row * tile_size + (tile_size - resized.height()) / 2);
        imageops::overlay(&mut sheet, &resized.to_rgba8(), x, y);
        cells.push(ContactSheetCell {
            index,
            character: item.character.clone(),
            request: item.request.clone(),
            raster_cache_key: item.raster_cache_key.clone(),
        });
    }
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(sheet).write_to(&mut bytes, ImageFormat::Png)?;
    let bytes = bytes.into_inner();
    write_new_atomic(output.as_ref(), &bytes)?;
    Ok(ContactSheetReport {
        schema: "reel.sprite-contact-sheet-report.v0.1".to_string(),
        materialization_receipt_sha256: digest_bytes(&receipt_bytes),
        sha256: digest_bytes(&bytes),
        width,
        height,
        columns,
        cells,
        passed: true,
    })
}

pub fn write_contact_sheet_report(
    report: &ContactSheetReport,
    output: impl AsRef<Path>,
) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(report)?;
    write_new_atomic(output.as_ref(), &[bytes.as_slice(), b"\n"].concat())
}

fn render_item(
    catalog: &LoadedCatalog,
    item: &crate::sprite_library::ResolvedPose,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    let mut canvas = RgbaImage::new(width, height);
    for layer in &item.ordered_layers {
        let source = &catalog.value.recipes[&layer.recipe];
        let RecipeSource::Asset {
            path,
            sha256,
            variants,
            mirror_behavior,
        } = source
        else {
            continue;
        };
        let selected = variants.get(&item.request);
        let (relative, expected_sha256) = selected
            .map_or((path.as_str(), sha256.as_str()), |variant| {
                (variant.path.as_str(), variant.sha256.as_str())
            });
        let source_path = resolve_relative(&catalog.path, relative)?;
        let source_bytes = fs::read(&source_path)?;
        if digest_bytes(&source_bytes) != expected_sha256 {
            bail!(
                "sprite source hash changed while materializing {}",
                layer.recipe
            );
        }
        let mut image = image::load_from_memory(&source_bytes)
            .with_context(|| format!("failed to decode sprite source {}", source_path.display()))?;
        if item.mirror_x
            && layer.transform_stage == TransformStage::PoseSpace
            && *mirror_behavior == MirrorBehavior::Inherit
        {
            image = DynamicImage::ImageRgba8(imageops::flip_horizontal(&image.to_rgba8()));
        }
        let (source_width, source_height) = image.dimensions();
        let scale = f64::min(
            width as f64 / source_width as f64,
            height as f64 / source_height as f64,
        );
        let target_width = ((source_width as f64 * scale).round() as u32).max(1);
        let target_height = ((source_height as f64 * scale).round() as u32).max(1);
        let resized =
            image.resize_exact(target_width, target_height, imageops::FilterType::Lanczos3);
        let x = i64::from((width - target_width) / 2);
        let y = i64::from((height - target_height) / 2);
        imageops::overlay(&mut canvas, &resized.to_rgba8(), x, y);
    }
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(canvas).write_to(&mut bytes, ImageFormat::Png)?;
    Ok(bytes.into_inner())
}

fn source_fingerprint(
    catalog: &LoadedCatalog,
    item: &crate::sprite_library::ResolvedPose,
) -> Result<String> {
    let layers = item
        .ordered_layers
        .iter()
        .map(|layer| {
            match &catalog.value.recipes[&layer.recipe] {
                RecipeSource::Transparent => serde_json::json!({
                    "slot": layer.slot,
                    "recipe": layer.recipe,
                    "transform_stage": layer.transform_stage,
                    "kind": "transparent",
                }),
                RecipeSource::Asset {
                    sha256,
                    variants,
                    mirror_behavior,
                    ..
                } => {
                    let selected = variants.get(&item.request);
                    serde_json::json!({
                        "slot": layer.slot,
                        "recipe": layer.recipe,
                        "transform_stage": layer.transform_stage,
                        "kind": "asset",
                        "source_sha256": selected.map_or(sha256.as_str(), |variant| variant.sha256.as_str()),
                        "mirror_behavior": mirror_behavior,
                        "effective_mirror_x": item.mirror_x
                            && layer.transform_stage == TransformStage::PoseSpace
                            && *mirror_behavior == MirrorBehavior::Inherit,
                    })
                }
            }
        })
        .collect::<Vec<_>>();
    Ok(digest_bytes(&serde_json::to_vec(&layers)?))
}

fn validate_source(catalog: &LoadedCatalog, recipe: &str, source: &RecipeSource) -> Result<()> {
    let RecipeSource::Asset {
        path,
        sha256,
        variants,
        ..
    } = source
    else {
        return Ok(());
    };
    validate_asset(catalog, recipe, path, sha256)?;
    for (request, variant) in variants {
        if request.trim().is_empty() {
            bail!("recipe {recipe} contains an empty request variant");
        }
        validate_asset(catalog, recipe, &variant.path, &variant.sha256)?;
    }
    Ok(())
}

fn validate_asset(catalog: &LoadedCatalog, recipe: &str, path: &str, sha256: &str) -> Result<()> {
    let resolved = resolve_relative(&catalog.path, path)?;
    let actual = production::sha256_path(&resolved)
        .with_context(|| format!("failed to hash recipe {recipe} source"))?;
    if actual != sha256 {
        bail!("recipe {recipe} source hash does not match catalog");
    }
    Ok(())
}

fn resolve_relative(catalog_path: &Path, value: &str) -> Result<PathBuf> {
    let relative = Path::new(value);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        bail!("recipe asset paths must be relative and cannot traverse parents");
    }
    Ok(catalog_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(relative))
}

fn path_from_logical_key(key: &str) -> Result<PathBuf> {
    let path = Path::new(key);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("logical cache key cannot escape the cache root");
    }
    Ok(path.to_path_buf())
}

fn digest_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn checkerboard(width: u32, height: u32, block: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        let light = ((x / block) + (y / block)) % 2 == 0;
        if light {
            image::Rgba([232, 232, 232, 255])
        } else {
            image::Rgba([196, 196, 196, 255])
        }
    })
}

fn write_atomic(output: &Path, bytes: &[u8]) -> Result<()> {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary
        .persist_noclobber(output)
        .map_err(|error| error.error)?;
    Ok(())
}

fn write_new_atomic(output: &Path, bytes: &[u8]) -> Result<()> {
    if output.exists() {
        bail!("refusing to overwrite {}", output.display());
    }
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary
        .persist_noclobber(output)
        .map_err(|error| error.error)?;
    Ok(())
}
