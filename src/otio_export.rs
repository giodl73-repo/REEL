use std::{collections::BTreeMap, fs, io::Write, path::Path};

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::production::{self, MediaKind, TimingStatus, VisualAssetStatus};

const OTIO_TIMEBASE_RATE: i64 = 1000;
const OTIO_METADATA_SCHEMA: &str = "reel.otio-export-metadata.v0.1";
const DEFAULT_MEDIA_KEY: &str = "DEFAULT_MEDIA";

#[derive(Clone, Debug, Serialize)]
pub struct OtioExportReport {
    pub schema: String,
    pub source_manifest_sha256: String,
    pub output_sha256: String,
    pub work: String,
    pub timing_status: String,
    pub timebase_rate: i64,
    pub track_count: usize,
    pub clip_count: usize,
    pub duration_ms: u64,
    pub offline_media_references: usize,
    pub picture_track_only: bool,
    pub media_paths_exported: bool,
    pub transitions_mapped: bool,
    pub audio_exported: bool,
    pub human_authority_required: bool,
    pub creative_approved: bool,
    pub rights_approved: bool,
    pub publication_approved: bool,
    pub release_approved: bool,
}

#[derive(Clone, Debug, Serialize)]
struct OtioTimeline {
    #[serde(rename = "OTIO_SCHEMA")]
    schema: &'static str,
    global_start_time: Option<OtioRationalTime>,
    metadata: BTreeMap<String, Value>,
    name: String,
    tracks: OtioStack,
}

#[derive(Clone, Debug, Serialize)]
struct OtioStack {
    #[serde(rename = "OTIO_SCHEMA")]
    schema: &'static str,
    children: Vec<OtioTrack>,
    effects: Vec<Value>,
    markers: Vec<Value>,
    enabled: bool,
    metadata: BTreeMap<String, Value>,
    name: String,
    source_range: Option<OtioTimeRange>,
    color: Option<Value>,
}

#[derive(Clone, Debug, Serialize)]
struct OtioTrack {
    #[serde(rename = "OTIO_SCHEMA")]
    schema: &'static str,
    children: Vec<OtioClip>,
    effects: Vec<Value>,
    kind: &'static str,
    markers: Vec<Value>,
    enabled: bool,
    metadata: BTreeMap<String, Value>,
    name: String,
    source_range: Option<OtioTimeRange>,
    color: Option<Value>,
}

#[derive(Clone, Debug, Serialize)]
struct OtioClip {
    #[serde(rename = "OTIO_SCHEMA")]
    schema: &'static str,
    metadata: BTreeMap<String, Value>,
    name: String,
    source_range: OtioTimeRange,
    markers: Vec<Value>,
    enabled: bool,
    effects: Vec<Value>,
    active_media_reference_key: &'static str,
    color: Option<Value>,
    media_references: BTreeMap<String, OtioMissingReference>,
}

#[derive(Clone, Debug, Serialize)]
struct OtioMissingReference {
    #[serde(rename = "OTIO_SCHEMA")]
    schema: &'static str,
    available_range: Option<OtioTimeRange>,
    available_image_bounds: Option<Value>,
    metadata: BTreeMap<String, Value>,
    name: String,
}

#[derive(Clone, Debug, Serialize)]
struct OtioTimeRange {
    #[serde(rename = "OTIO_SCHEMA")]
    schema: &'static str,
    start_time: OtioRationalTime,
    duration: OtioRationalTime,
}

#[derive(Clone, Debug, Serialize)]
struct OtioRationalTime {
    #[serde(rename = "OTIO_SCHEMA")]
    schema: &'static str,
    rate: i64,
    value: i64,
}

pub fn export(
    manifest_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> Result<OtioExportReport> {
    let manifest_path = manifest_path.as_ref();
    let output_path = output_path.as_ref();
    if output_path.extension().and_then(|value| value.to_str()) != Some("otio") {
        bail!("OpenTimelineIO output must use the .otio extension");
    }
    let loaded = production::load(manifest_path)?;
    let validation = production::validate(&loaded)?;
    if !matches!(
        loaded.manifest.timing_status,
        TimingStatus::Conformed | TimingStatus::Locked
    ) {
        bail!("OpenTimelineIO export requires conformed or locked timing");
    }
    let duration_ms = validation
        .duration_ms
        .ok_or_else(|| anyhow!("OpenTimelineIO export requires exact timeline duration"))?;
    let source_manifest_sha256 = hash_bytes(&loaded.bytes);
    let timeline = build_timeline(&loaded, &source_manifest_sha256)?;
    let bytes = format!("{}\n", serde_json::to_string_pretty(&timeline)?).into_bytes();
    write_new(&bytes, output_path)?;

    Ok(OtioExportReport {
        schema: "reel.otio-export-report.v0.1".to_string(),
        source_manifest_sha256,
        output_sha256: hash_bytes(&bytes),
        work: loaded.manifest.work,
        timing_status: loaded.manifest.timing_status.as_str().to_string(),
        timebase_rate: OTIO_TIMEBASE_RATE,
        track_count: 1,
        clip_count: loaded.manifest.shots.len(),
        duration_ms,
        offline_media_references: loaded.manifest.shots.len(),
        picture_track_only: true,
        media_paths_exported: false,
        transitions_mapped: false,
        audio_exported: false,
        human_authority_required: true,
        creative_approved: false,
        rights_approved: false,
        publication_approved: false,
        release_approved: false,
    })
}

fn build_timeline(
    loaded: &production::LoadedProductionManifest,
    source_manifest_sha256: &str,
) -> Result<OtioTimeline> {
    let manifest = &loaded.manifest;
    require_portable_id("work", &manifest.work)?;
    let timeline_reel = serde_json::json!({
        "schema": OTIO_METADATA_SCHEMA,
        "source_manifest_sha256": source_manifest_sha256,
        "manifest_version": manifest.manifest_version,
        "work": manifest.work,
        "timing_status": manifest.timing_status.as_str(),
        "timebase_rate": OTIO_TIMEBASE_RATE,
        "picture_track_only": true,
        "media_paths_exported": false,
        "transitions_mapped": false,
        "audio_exported": false,
        "human_authority_required": true,
        "creative_approved": false,
        "rights_approved": false,
        "publication_approved": false,
        "release_approved": false
    });
    let clips = manifest
        .shots
        .iter()
        .map(|shot| {
            require_portable_id("shot id", &shot.id)?;
            require_portable_id("scene id", &shot.scene_id)?;
            let start_ms = milliseconds(shot.start_seconds, "shot start")?;
            let duration_ms = milliseconds(shot.duration_seconds, "shot duration")?;
            let source_in_ms = milliseconds(Some(shot.source_in_seconds), "shot source in")?;
            let asset_status = shot
                .visual_asset_status
                .map(visual_asset_status)
                .unwrap_or("unspecified");
            let clip_reel = serde_json::json!({
                "schema": OTIO_METADATA_SCHEMA,
                "shot_id": shot.id,
                "scene_id": shot.scene_id,
                "timeline_start_ms": start_ms,
                "duration_ms": duration_ms,
                "source_in_ms": source_in_ms,
                "media_kind": media_kind(shot.media_kind),
                "visual_asset_status": asset_status,
                "visual_asset_declared": shot.visual_asset.is_some(),
                "transition_intent_present": !shot.transition_out.is_empty(),
                "transition_intent_sha256": hash_bytes(shot.transition_out.as_bytes()),
                "transition_mapped": false,
                "media_relinked": false,
                "output_selected": false
            });
            Ok(OtioClip {
                schema: "Clip.2",
                metadata: BTreeMap::from([("reel".to_string(), clip_reel)]),
                name: shot.id.clone(),
                source_range: time_range(source_in_ms, duration_ms)?,
                markers: Vec::new(),
                enabled: true,
                effects: Vec::new(),
                active_media_reference_key: DEFAULT_MEDIA_KEY,
                color: None,
                media_references: BTreeMap::from([(
                    DEFAULT_MEDIA_KEY.to_string(),
                    OtioMissingReference {
                        schema: "MissingReference.1",
                        available_range: None,
                        available_image_bounds: None,
                        metadata: BTreeMap::from([(
                            "reel".to_string(),
                            serde_json::json!({
                                "schema": OTIO_METADATA_SCHEMA,
                                "shot_id": shot.id,
                                "reason": "owner-media-relink-required"
                            }),
                        )]),
                        name: format!("offline-{}", shot.id),
                    },
                )]),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(OtioTimeline {
        schema: "Timeline.1",
        global_start_time: None,
        metadata: BTreeMap::from([("reel".to_string(), timeline_reel)]),
        name: manifest.work.clone(),
        tracks: OtioStack {
            schema: "Stack.1",
            children: vec![OtioTrack {
                schema: "Track.1",
                children: clips,
                effects: Vec::new(),
                kind: "Video",
                markers: Vec::new(),
                enabled: true,
                metadata: BTreeMap::from([(
                    "reel".to_string(),
                    serde_json::json!({
                        "schema": OTIO_METADATA_SCHEMA,
                        "picture_track_only": true
                    }),
                )]),
                name: "REEL Picture".to_string(),
                source_range: None,
                color: None,
            }],
            effects: Vec::new(),
            markers: Vec::new(),
            enabled: true,
            metadata: BTreeMap::new(),
            name: "tracks".to_string(),
            source_range: None,
            color: None,
        },
    })
}

fn time_range(start_ms: u64, duration_ms: u64) -> Result<OtioTimeRange> {
    Ok(OtioTimeRange {
        schema: "TimeRange.1",
        start_time: rational_time(start_ms)?,
        duration: rational_time(duration_ms)?,
    })
}

fn rational_time(value: u64) -> Result<OtioRationalTime> {
    Ok(OtioRationalTime {
        schema: "RationalTime.1",
        rate: OTIO_TIMEBASE_RATE,
        value: i64::try_from(value).context("OpenTimelineIO time exceeds signed 64-bit range")?,
    })
}

fn milliseconds(value: Option<f64>, label: &str) -> Result<u64> {
    let value = value.ok_or_else(|| anyhow!("OpenTimelineIO export requires {label}"))?;
    if !value.is_finite() || value < 0.0 {
        bail!("OpenTimelineIO export requires finite non-negative {label}");
    }
    Ok((value * 1000.0).round() as u64)
}

fn media_kind(value: MediaKind) -> &'static str {
    match value {
        MediaKind::Still => "still",
        MediaKind::Video => "video",
        MediaKind::Animation => "animation",
        MediaKind::SpriteAnimation => "sprite-animation",
    }
}

fn visual_asset_status(value: VisualAssetStatus) -> &'static str {
    match value {
        VisualAssetStatus::PlannedUnrendered => "planned-unrendered",
        VisualAssetStatus::Candidate => "candidate",
        VisualAssetStatus::Selected => "selected",
        VisualAssetStatus::Approved => "approved",
        VisualAssetStatus::Missing => "missing",
    }
}

fn require_portable_id(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 200
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        bail!("{label} must be a portable identifier of at most 200 ASCII characters");
    }
    Ok(())
}

fn write_new(bytes: &[u8], output_path: &Path) -> Result<()> {
    if output_path.exists() {
        bail!("refusing to overwrite {}", output_path.display());
    }
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary
        .persist_noclobber(output_path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish {}", output_path.display()))?;
    Ok(())
}

fn hash_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
