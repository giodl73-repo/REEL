use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use image::ImageFormat;
use same_file::is_same_file;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::production;

pub const SCREEN_DEMO_INPUT_SCHEMA: &str = "reel.screen-demo-capture-input.v0.1";
pub const SCREEN_DEMO_RECEIPT_SCHEMA: &str = "reel.screen-demo-capture-receipt.v0.1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenDemoCaptureInput {
    pub schema: String,
    pub demo_id: String,
    pub owner_state_ref_sha256: String,
    pub state_document: LocalStateDocument,
    pub required_surfaces: Vec<ScreenSurface>,
    pub captures: Vec<ScreenCapture>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalStateDocument {
    pub file_id: String,
    pub path: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScreenSurface {
    Cli,
    Tui,
    Web,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenCapture {
    pub capture_id: String,
    pub sequence: u32,
    pub surface: ScreenSurface,
    pub viewport_id: String,
    pub path: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScreenDemoCaptureReceipt {
    pub schema: String,
    pub demo_id: String,
    pub input_sha256: String,
    pub owner_state_ref_sha256: String,
    pub state_document: StateDocumentEvidence,
    pub required_surfaces: Vec<ScreenSurface>,
    pub captures: Vec<ScreenCaptureEvidence>,
    pub capture_count: usize,
    pub exact_required_surface_coverage: bool,
    pub capture_bytes_verified: bool,
    pub captures_supplied_by_input: bool,
    pub capture_state_correspondence_verified: bool,
    pub capture_semantics_verified: bool,
    pub privacy_review_required: bool,
    pub redaction_verified: bool,
    pub accessibility_verified: bool,
    pub commands_executed_by_reel: bool,
    pub browser_controlled_by_reel: bool,
    pub captures_created_by_reel: bool,
    pub selected_by_reel: bool,
    pub publication_approved: bool,
    pub release_approved: bool,
    pub passed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct StateDocumentEvidence {
    pub file_id: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScreenCaptureEvidence {
    pub capture_id: String,
    pub sequence: u32,
    pub surface: ScreenSurface,
    pub viewport_id: String,
    pub sha256: String,
    pub bytes: u64,
    pub media_type: String,
    pub width: u32,
    pub height: u32,
}

pub fn write_capture_receipt(
    input_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> Result<ScreenDemoCaptureReceipt> {
    let input_path = input_path.as_ref();
    let input_bytes = fs::read(input_path)
        .with_context(|| format!("failed to read screen demo input {}", input_path.display()))?;
    let input: ScreenDemoCaptureInput = serde_json::from_slice(&input_bytes)
        .context("screen demo capture input is not valid strict JSON")?;
    let receipt = build_capture_receipt(input_path, production::sha256_bytes(&input_bytes), input)?;
    let mut bytes = serde_json::to_vec_pretty(&receipt)?;
    bytes.push(b'\n');
    write_atomic_new(output_path.as_ref(), &bytes)?;
    Ok(receipt)
}

fn build_capture_receipt(
    input_path: &Path,
    input_sha256: String,
    input: ScreenDemoCaptureInput,
) -> Result<ScreenDemoCaptureReceipt> {
    if input.schema != SCREEN_DEMO_INPUT_SCHEMA {
        bail!(
            "unsupported screen demo schema {}; expected {SCREEN_DEMO_INPUT_SCHEMA}",
            input.schema
        );
    }
    validate_id("demo", &input.demo_id)?;
    validate_hash("owner_state_ref_sha256", &input.owner_state_ref_sha256)?;
    validate_id("state document file", &input.state_document.file_id)?;
    validate_required_surfaces(&input.required_surfaces)?;
    if input.captures.is_empty() {
        bail!("screen demo input must declare at least one capture");
    }

    let state_path = resolve_relative(input_path, &input.state_document.path);
    require_regular_file("state document", &state_path)?;
    let state_bytes = fs::read(&state_path)
        .with_context(|| format!("failed to read state document {}", state_path.display()))?;
    if state_bytes.is_empty() {
        bail!("screen demo state document must not be empty");
    }
    let state_evidence = StateDocumentEvidence {
        file_id: input.state_document.file_id,
        sha256: production::sha256_bytes(&state_bytes),
        bytes: state_bytes.len() as u64,
    };

    let required = input
        .required_surfaces
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut observed = BTreeMap::<ScreenSurface, usize>::new();
    let mut capture_ids = BTreeSet::new();
    let mut content_hashes = BTreeSet::new();
    let mut capture_paths = Vec::<PathBuf>::new();
    let mut evidence = Vec::with_capacity(input.captures.len());

    for (index, capture) in input.captures.into_iter().enumerate() {
        validate_id("capture", &capture.capture_id)?;
        validate_id("viewport", &capture.viewport_id)?;
        if !capture_ids.insert(capture.capture_id.clone()) {
            bail!("duplicate screen demo capture {}", capture.capture_id);
        }
        if capture.sequence != index as u32 {
            bail!("screen demo capture sequence must be contiguous from zero");
        }
        if !required.contains(&capture.surface) {
            bail!(
                "capture {} uses a surface not declared in required_surfaces",
                capture.capture_id
            );
        }
        if capture.width == 0 || capture.height == 0 {
            bail!("capture {} dimensions must be positive", capture.capture_id);
        }

        let path = resolve_relative(input_path, &capture.path);
        require_regular_file("screen capture", &path)?;
        if is_same_file(&state_path, &path).with_context(|| {
            format!(
                "failed to compare state document and capture {}",
                capture.capture_id
            )
        })? {
            bail!(
                "capture {} aliases the screen demo state document",
                capture.capture_id
            );
        }
        for previous in &capture_paths {
            if is_same_file(previous, &path).with_context(|| {
                format!(
                    "failed to compare physical file identity for capture {}",
                    capture.capture_id
                )
            })? {
                bail!(
                    "capture {} aliases another screen demo capture",
                    capture.capture_id
                );
            }
        }

        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read screen capture {}", path.display()))?;
        if image::guess_format(&bytes).context("failed to identify screen capture media type")?
            != ImageFormat::Png
        {
            bail!("capture {} must be an exact PNG", capture.capture_id);
        }
        let decoded = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
            .with_context(|| format!("failed to decode screen capture {}", capture.capture_id))?;
        if decoded.width() != capture.width || decoded.height() != capture.height {
            bail!(
                "capture {} dimensions mismatch: expected {}x{}, found {}x{}",
                capture.capture_id,
                capture.width,
                capture.height,
                decoded.width(),
                decoded.height()
            );
        }
        let sha256 = production::sha256_bytes(&bytes);
        if !content_hashes.insert(sha256.clone()) {
            bail!(
                "capture {} duplicates another capture's exact bytes",
                capture.capture_id
            );
        }
        *observed.entry(capture.surface).or_default() += 1;
        capture_paths.push(path);
        evidence.push(ScreenCaptureEvidence {
            capture_id: capture.capture_id,
            sequence: capture.sequence,
            surface: capture.surface,
            viewport_id: capture.viewport_id,
            sha256,
            bytes: bytes.len() as u64,
            media_type: "image/png".to_string(),
            width: decoded.width(),
            height: decoded.height(),
        });
    }

    let missing = required
        .iter()
        .filter(|surface| !observed.contains_key(surface))
        .map(surface_name)
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!(
            "screen demo is missing required surfaces: {}",
            missing.join(",")
        );
    }

    Ok(ScreenDemoCaptureReceipt {
        schema: SCREEN_DEMO_RECEIPT_SCHEMA.to_string(),
        demo_id: input.demo_id,
        input_sha256,
        owner_state_ref_sha256: input.owner_state_ref_sha256,
        state_document: state_evidence,
        required_surfaces: input.required_surfaces,
        capture_count: evidence.len(),
        captures: evidence,
        exact_required_surface_coverage: true,
        capture_bytes_verified: true,
        captures_supplied_by_input: true,
        capture_state_correspondence_verified: false,
        capture_semantics_verified: false,
        privacy_review_required: true,
        redaction_verified: false,
        accessibility_verified: false,
        commands_executed_by_reel: false,
        browser_controlled_by_reel: false,
        captures_created_by_reel: false,
        selected_by_reel: false,
        publication_approved: false,
        release_approved: false,
        passed: true,
    })
}

fn validate_required_surfaces(surfaces: &[ScreenSurface]) -> Result<()> {
    if surfaces.is_empty() {
        bail!("screen demo required_surfaces must not be empty");
    }
    let mut previous = None;
    for surface in surfaces {
        if previous.is_some_and(|value| value >= *surface) {
            bail!("screen demo required_surfaces must be strictly sorted and unique");
        }
        previous = Some(*surface);
    }
    Ok(())
}

fn surface_name(surface: &ScreenSurface) -> &'static str {
    match surface {
        ScreenSurface::Cli => "cli",
        ScreenSurface::Tui => "tui",
        ScreenSurface::Web => "web",
    }
}

fn resolve_relative(contract_path: &Path, value: &str) -> PathBuf {
    let candidate = PathBuf::from(value);
    if candidate.is_absolute() {
        candidate
    } else {
        contract_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(candidate)
    }
}

fn require_regular_file(kind: &str, path: &Path) -> Result<()> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to inspect {kind} {}", path.display()))?;
    if !metadata.is_file() {
        bail!("{kind} {} must be a regular file", path.display());
    }
    Ok(())
}

fn validate_id(kind: &str, id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        bail!("{kind} id {id:?} must use ASCII letters, numbers, hyphens, or underscores");
    }
    Ok(())
}

fn validate_hash(kind: &str, value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .chars()
            .all(|character| character.is_ascii_digit() || matches!(character, 'a'..='f'))
    {
        bail!("{kind} must be a 64-character lowercase hexadecimal hash");
    }
    Ok(())
}

fn write_atomic_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }
    let temp_parent = parent.unwrap_or_else(|| Path::new("."));
    let mut temp = NamedTempFile::new_in(temp_parent)?;
    temp.write_all(bytes)?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(path)
        .map_err(|error| error.error)
        .with_context(|| format!("refusing to overwrite existing output {}", path.display()))?;
    Ok(())
}
