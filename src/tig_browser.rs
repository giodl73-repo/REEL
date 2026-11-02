//! Native browser-composition planning over one exact Tig metadata view.
//! No browser, media IO, output creation or execution authority is supplied here.
use crate::{
    browser_composition::{self, BrowserCompositionManifest, HashedAsset, PixelRegion},
    scene_delivery::FileRef,
    tig_delivery_template::{Eligibility, PrivateReview},
    tig_inputs::{Payload, Source},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::Path,
};
use tig_assets::graph::Reference;
use tig_core::{Digest, LogicalPath, Object};

pub mod capture;
mod context;
#[cfg(all(windows, feature = "tig-publication"))]
pub mod render;
pub use context::{freeze_context, recipe, requirements, restore_context};

pub const SCHEMA: &str = "reel.tig-browser-source.experimental-1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub root_id: String,
    pub root_definition: String,
    pub primary_definitions: BTreeSet<String>,
    pub bounds: Bounds,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_review: Option<PrivateReview>,
}

/// Metadata planning bounds, not held execution/memory/capacity reservations.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub max_inputs: usize,
    pub max_input_bytes: u64,
    pub max_frames: u64,
    pub max_canvas_pixels: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    source: String,
    audio: Option<String>,
    inputs: BTreeMap<String, IdRef>,
    width: u32,
    height: u32,
    fps: u32,
    frame_count: u64,
    #[serde(default)]
    dynamic_regions: Vec<PixelRegion>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdRef {
    id: String,
}

/// Private resolved controls. Logical paths and source identities are not a
/// shareable receipt; no Debug/Serialize or implicit filesystem fallback.
pub struct Prepared {
    pub source_checkpoint: Digest,
    pub source_record: Reference,
    pub manifest: BrowserCompositionManifest,
    pub manifest_bytes: Vec<u8>,
    pub inputs: BTreeMap<String, FileRef>,
    pub alias_bytes: u64,
    pub unique_bytes: u64,
    pub unique_objects: usize,
}

fn token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

impl Request {
    pub fn read(path: &Path) -> Result<Self> {
        let mut raw = Vec::new();
        std::fs::File::open(path)?
            .take(65_537)
            .read_to_end(&mut raw)?;
        if raw.len() > 65_536 {
            bail!("browser request exceeds byte budget");
        }
        let value = tig_core::identity::parse_json(&raw)?;
        let request: Self = serde_json::from_value(value)?;
        request.validate()?;
        Ok(request)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA
            || !token(&self.root_id)
            || !token(&self.root_definition)
            || self.primary_definitions.is_empty()
            || self.primary_definitions.len() > 32
            || self.primary_definitions.contains(&self.root_definition)
            || self.primary_definitions.iter().any(|id| !token(id))
            || !(1..=4096).contains(&self.bounds.max_inputs)
            || self.bounds.max_input_bytes == 0
            || !(1..=1_000_000).contains(&self.bounds.max_frames)
            || !(1..=67_108_864).contains(&self.bounds.max_canvas_pixels)
        {
            bail!("invalid bounded browser source admission");
        }
        // Share exact private-review admission and its limits with delivery
        // planning instead of inventing another selection/approval vocabulary.
        crate::tig_delivery_template::validate_private_review(self.private_review.as_ref())
    }
}

impl Prepared {
    /// Private operational summary with no raw logical IDs or resource paths.
    /// These counts/hashes prove no payload, execution, approval or custody.
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "schema":"reel.tig-browser-plan.experimental-1",
            "source_checkpoint":self.source_checkpoint,
            "manifest":Object::from_bytes(&self.manifest_bytes),
            "input_aliases":self.inputs.len(),"unique_objects":self.unique_objects,
            "alias_bytes":self.alias_bytes,"unique_bytes":self.unique_bytes,
            "width":self.manifest.width,"height":self.manifest.height,
            "fps":self.manifest.fps,"frames":self.manifest.frame_count,
            "primary_media_opened":0,"production_executed":false,
            "complete_semantic_coverage":false,"authorizes_eviction":false
        })
    }
}

/// Lower ID references into the existing browser manifest meaning. Source hashes
/// are derived from leaves, never maintained as a competing authored list.
pub fn lower(source: &mut Source, request: &Request) -> Result<Prepared> {
    request.validate()?;
    let mut eligibility = Eligibility::for_review(source, request.private_review.as_ref())?;
    let root = source.record(&request.root_id)?;
    if root.definition().id != request.root_definition {
        bail!("browser source definition differs");
    }
    eligibility.check(&root)?;
    let document: Document = serde_json::from_value(
        root.authored()
            .get("document")
            .context("browser document missing")?
            .clone(),
    )?;
    let pixels = u64::from(document.width)
        .checked_mul(u64::from(document.height))
        .context("browser canvas overflow")?;
    if document.inputs.is_empty()
        || document.inputs.len() > request.bounds.max_inputs
        || pixels > request.bounds.max_canvas_pixels
        || document.frame_count > request.bounds.max_frames
        || document.dynamic_regions.len() > 128
        || document.audio.as_ref() == Some(&document.source)
    {
        bail!("browser document exceeds admitted scope");
    }
    let mut inputs = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut objects = BTreeMap::new();
    let mut alias_bytes = 0u64;
    for (path, reference) in &document.inputs {
        LogicalPath::parse(path).map_err(|_| anyhow::anyhow!("invalid browser resource path"))?;
        if !names.insert(path.to_lowercase()) {
            bail!("browser resource paths alias on case-insensitive hosts");
        }
        let record = source.record(&reference.id)?;
        if !request
            .primary_definitions
            .contains(&record.definition().id)
        {
            bail!("browser resource is not an admitted primary");
        }
        eligibility.check(&record)?;
        let object = match &source.admission(&record.definition().id)?.payload {
            Payload::Selected {} => record.selected()?,
            Payload::Stream { slot } => record.stream(slot)?,
        };
        if object.bytes == 0 {
            bail!("browser primary has no bytes");
        }
        if objects
            .insert(object.sha256.clone(), object.bytes)
            .is_some_and(|n| n != object.bytes)
        {
            bail!("browser object aliases disagree on byte length");
        }
        alias_bytes = alias_bytes
            .checked_add(object.bytes)
            .context("browser input byte overflow")?;
        if alias_bytes > request.bounds.max_input_bytes {
            bail!("browser inputs exceed admitted byte budget");
        }
        LogicalPath::parse(&reference.id)
            .map_err(|_| anyhow::anyhow!("invalid browser primary identity"))?;
        inputs.insert(
            path.clone(),
            FileRef {
                path: reference.id.clone().into(),
                sha256: object.sha256.as_str().into(),
                bytes: object.bytes,
            },
        );
    }
    for path in &names {
        for (index, _) in path.match_indices('/') {
            if names.contains(&path[..index]) {
                bail!("browser file paths have a prefix collision");
            }
        }
    }
    let asset = |path: &str| -> Result<HashedAsset> {
        Ok(HashedAsset {
            path: path.into(),
            sha256: inputs
                .get(path)
                .context("browser resource is not declared")?
                .sha256
                .clone(),
        })
    };
    let manifest = browser_composition::validate_manifest(BrowserCompositionManifest {
        schema: browser_composition::MANIFEST_SCHEMA.into(),
        source: asset(&document.source)?,
        assets: inputs
            .keys()
            .filter(|path| *path != &document.source && Some(*path) != document.audio.as_ref())
            .map(|path| asset(path))
            .collect::<Result<_>>()?,
        audio: document.audio.as_deref().map(asset).transpose()?,
        width: document.width,
        height: document.height,
        fps: document.fps,
        frame_count: document.frame_count,
        dynamic_regions: document.dynamic_regions,
    })?;
    eligibility.finish()?;
    let unique_bytes = objects.values().try_fold(0u64, |sum, bytes| {
        sum.checked_add(*bytes)
            .context("browser unique byte overflow")
    })?;
    let manifest_bytes = serde_json::to_vec(&manifest)?;
    if manifest_bytes.len() > crate::tig_inputs::JOB_BYTES_MAX {
        bail!("lowered browser manifest exceeds byte budget");
    }
    Ok(Prepared {
        source_checkpoint: source.checkpoint().clone(),
        source_record: root.reference().clone(),
        manifest,
        manifest_bytes,
        inputs,
        alias_bytes,
        unique_bytes,
        unique_objects: objects.len(),
    })
}
