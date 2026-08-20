use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::{
    production,
    production_binding::{self, ProductionBinding},
};

pub const EXPOSURE_SHEET_SCHEMA: &str = "reel.exposure-sheet.v0.1";
pub const EXPOSURE_SHEET_REPORT_SCHEMA: &str = "reel.exposure-sheet-report.v0.1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExposureSheet {
    pub schema: String,
    pub sheet_id: String,
    pub fps: u32,
    pub duration_frames: u32,
    pub shot_ref: String,
    pub production_binding: ProductionBinding,
    pub tracks: Vec<ExposureTrack>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExposureTrack {
    pub track_id: String,
    pub kind: TrackKind,
    pub coverage: Coverage,
    pub exposures: Vec<Exposure>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrackKind {
    Drawing,
    Pose,
    Prop,
    Effect,
    Camera,
    Dialogue,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Coverage {
    Complete,
    Sparse,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Exposure {
    pub start_frame: u32,
    pub end_frame: u32,
    pub exposure_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cue_ids: Vec<String>,
}

#[derive(Debug)]
pub struct LoadedExposureSheet {
    pub path: PathBuf,
    pub source_sha256: String,
    pub sheet: ExposureSheet,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExposureSheetReport {
    pub schema: String,
    pub sheet_id: String,
    pub sheet_sha256: String,
    pub production_manifest_sha256: String,
    pub work: String,
    pub shot_id: String,
    pub shot_duration_ms: u64,
    pub fps: u32,
    pub duration_frames: u32,
    pub duration_delta_milli_frames: u64,
    pub duration_within_half_frame: bool,
    pub tracks: Vec<TrackReport>,
    pub exposure_count: usize,
    pub declared_asset_hash_exposures: usize,
    pub planned_exposures: usize,
    pub cue_bindings: usize,
    pub exact_frame_ranges_verified: bool,
    pub exposures_supplied_by_input: bool,
    pub asset_bytes_verified: bool,
    pub reel_selected_exposures: bool,
    pub rendered_by_reel: bool,
    pub dcc_project_mutated: bool,
    pub delivery_frame_rate_claimed: bool,
    pub passed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct TrackReport {
    pub track_id: String,
    pub kind: TrackKind,
    pub coverage: Coverage,
    pub exposure_count: usize,
    pub covered_frames: u64,
    pub gaps: Vec<FrameSpan>,
    pub declared_asset_hash_exposures: usize,
    pub cue_bindings: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct FrameSpan {
    pub start_frame: u32,
    pub end_frame: u32,
}

pub fn load(path: impl AsRef<Path>) -> Result<LoadedExposureSheet> {
    let path = path.as_ref();
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read exposure sheet {}", path.display()))?;
    let sheet = serde_yaml::from_slice(&bytes)
        .with_context(|| format!("failed to parse exposure sheet {}", path.display()))?;
    Ok(LoadedExposureSheet {
        path: path.to_path_buf(),
        source_sha256: production::sha256_bytes(&bytes),
        sheet,
    })
}

pub fn validate(loaded: &LoadedExposureSheet) -> Result<ExposureSheetReport> {
    let sheet = &loaded.sheet;
    if sheet.schema != EXPOSURE_SHEET_SCHEMA {
        bail!(
            "unsupported exposure sheet schema {}; expected {EXPOSURE_SHEET_SCHEMA}",
            sheet.schema
        );
    }
    validate_id("sheet", &sheet.sheet_id)?;
    validate_id("shot reference", &sheet.shot_ref)?;
    if !(1..=120).contains(&sheet.fps) {
        bail!("exposure sheet fps must be between 1 and 120");
    }
    if sheet.duration_frames == 0 {
        bail!("exposure sheet duration_frames must be positive");
    }
    if sheet.production_binding.shots.len() != 1
        || !sheet.production_binding.shots.contains_key(&sheet.shot_ref)
    {
        bail!("exposure sheet production binding must map exactly its one shot_ref");
    }
    if !sheet.production_binding.beats.is_empty() {
        bail!("exposure sheet production binding does not accept unused beat mappings");
    }
    if sheet.tracks.is_empty() {
        bail!("exposure sheet must declare at least one track");
    }

    let bound = production_binding::resolve(&loaded.path, &sheet.production_binding)?;
    let shot = production_binding::require_shot(&bound.resolved, &sheet.shot_ref)?.clone();
    validate_id("bound production work", &bound.resolved.work)?;
    validate_id("bound production shot", &shot.shot_id)?;
    let frame_milliseconds = u64::from(sheet.duration_frames)
        .checked_mul(1_000)
        .ok_or_else(|| anyhow!("exposure sheet frame duration overflows"))?;
    let shot_milli_frames = shot
        .duration_ms
        .checked_mul(u64::from(sheet.fps))
        .ok_or_else(|| anyhow!("bound shot duration overflows exposure sheet timebase"))?;
    let duration_delta_milli_frames = frame_milliseconds.abs_diff(shot_milli_frames);
    if duration_delta_milli_frames > 500 {
        bail!(
            "exposure sheet duration {} frames at {} fps differs from bound shot {} duration {}ms by more than half a frame",
            sheet.duration_frames,
            sheet.fps,
            shot.shot_id,
            shot.duration_ms
        );
    }

    let shot_side_cues = bound
        .loaded
        .manifest
        .shots
        .iter()
        .find(|candidate| candidate.id == shot.shot_id)
        .expect("resolved shot must remain present in loaded production manifest")
        .narration_cue_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let shot_cues = bound
        .loaded
        .manifest
        .narration_cues
        .iter()
        .filter(|cue| {
            if cue.shot_ids.is_empty() {
                shot_side_cues.contains(cue.id.as_str())
            } else {
                cue.shot_ids.iter().any(|shot_id| shot_id == &shot.shot_id)
            }
        })
        .map(|cue| cue.id.as_str())
        .collect::<BTreeSet<_>>();
    let all_cues = bound
        .loaded
        .manifest
        .narration_cues
        .iter()
        .map(|cue| cue.id.as_str())
        .collect::<BTreeSet<_>>();

    let mut track_ids = BTreeSet::new();
    let mut previous_track_id: Option<&str> = None;
    let mut track_reports = Vec::with_capacity(sheet.tracks.len());
    let mut exposure_count = 0usize;
    let mut declared_asset_hash_exposures = 0usize;
    let mut cue_bindings = 0usize;

    for track in &sheet.tracks {
        validate_id("track", &track.track_id)?;
        if !track_ids.insert(track.track_id.as_str()) {
            bail!("duplicate exposure sheet track {}", track.track_id);
        }
        if previous_track_id.is_some_and(|id| id >= track.track_id.as_str()) {
            bail!("exposure sheet tracks must be strictly sorted by track_id");
        }
        previous_track_id = Some(&track.track_id);
        if track.exposures.is_empty() {
            bail!("exposure sheet track {} has no exposures", track.track_id);
        }

        let mut previous: Option<&Exposure> = None;
        let mut gaps = Vec::new();
        let mut covered_frames = 0u64;
        let mut track_hashes = 0usize;
        let mut track_cues = 0usize;
        for exposure in &track.exposures {
            validate_id("exposure", &exposure.exposure_id)?;
            if exposure.start_frame > exposure.end_frame {
                bail!(
                    "track {} exposure {} has start_frame after end_frame",
                    track.track_id,
                    exposure.exposure_id
                );
            }
            if exposure.end_frame >= sheet.duration_frames {
                bail!(
                    "track {} exposure {} falls outside sheet duration",
                    track.track_id,
                    exposure.exposure_id
                );
            }
            if let Some(previous) = previous {
                if exposure.start_frame <= previous.end_frame {
                    bail!(
                        "track {} exposures overlap or are not strictly ordered at frame {}",
                        track.track_id,
                        exposure.start_frame
                    );
                }
                if exposure.start_frame == previous.end_frame + 1
                    && same_exposure_binding(previous, exposure)
                {
                    bail!(
                        "track {} repeats adjacent exposure {}; merge the frame spans",
                        track.track_id,
                        exposure.exposure_id
                    );
                }
                if exposure.start_frame > previous.end_frame + 1 {
                    gaps.push(FrameSpan {
                        start_frame: previous.end_frame + 1,
                        end_frame: exposure.start_frame - 1,
                    });
                }
            } else if exposure.start_frame > 0 {
                gaps.push(FrameSpan {
                    start_frame: 0,
                    end_frame: exposure.start_frame - 1,
                });
            }
            if let Some(hash) = &exposure.asset_sha256 {
                validate_hash("exposure asset_sha256", hash)?;
                track_hashes += 1;
            }
            validate_cues(
                &track.track_id,
                exposure,
                &all_cues,
                &shot_cues,
                &shot.shot_id,
            )?;
            track_cues += exposure.cue_ids.len();
            covered_frames = covered_frames
                .checked_add(u64::from(exposure.end_frame - exposure.start_frame) + 1)
                .ok_or_else(|| anyhow!("track {} frame coverage overflows", track.track_id))?;
            previous = Some(exposure);
        }
        let last = previous.expect("non-empty exposures checked");
        if last.end_frame < sheet.duration_frames - 1 {
            gaps.push(FrameSpan {
                start_frame: last.end_frame + 1,
                end_frame: sheet.duration_frames - 1,
            });
        }
        if matches!(track.coverage, Coverage::Complete) && !gaps.is_empty() {
            bail!(
                "complete track {} has uncovered frame range {}-{}",
                track.track_id,
                gaps[0].start_frame,
                gaps[0].end_frame
            );
        }

        exposure_count += track.exposures.len();
        declared_asset_hash_exposures += track_hashes;
        cue_bindings += track_cues;
        track_reports.push(TrackReport {
            track_id: track.track_id.clone(),
            kind: track.kind,
            coverage: track.coverage,
            exposure_count: track.exposures.len(),
            covered_frames,
            gaps,
            declared_asset_hash_exposures: track_hashes,
            cue_bindings: track_cues,
        });
    }

    Ok(ExposureSheetReport {
        schema: EXPOSURE_SHEET_REPORT_SCHEMA.to_string(),
        sheet_id: sheet.sheet_id.clone(),
        sheet_sha256: loaded.source_sha256.clone(),
        production_manifest_sha256: bound.resolved.manifest_sha256,
        work: bound.resolved.work,
        shot_id: shot.shot_id,
        shot_duration_ms: shot.duration_ms,
        fps: sheet.fps,
        duration_frames: sheet.duration_frames,
        duration_delta_milli_frames,
        duration_within_half_frame: true,
        tracks: track_reports,
        exposure_count,
        declared_asset_hash_exposures,
        planned_exposures: exposure_count - declared_asset_hash_exposures,
        cue_bindings,
        exact_frame_ranges_verified: true,
        exposures_supplied_by_input: true,
        asset_bytes_verified: false,
        reel_selected_exposures: false,
        rendered_by_reel: false,
        dcc_project_mutated: false,
        delivery_frame_rate_claimed: false,
        passed: true,
    })
}

pub fn write_report(
    loaded: &LoadedExposureSheet,
    output_path: impl AsRef<Path>,
) -> Result<ExposureSheetReport> {
    let report = validate(loaded)?;
    let mut bytes = serde_json::to_vec_pretty(&report)?;
    bytes.push(b'\n');
    write_atomic_new(output_path.as_ref(), &bytes)?;
    Ok(report)
}

fn validate_cues(
    track_id: &str,
    exposure: &Exposure,
    all_cues: &BTreeSet<&str>,
    shot_cues: &BTreeSet<&str>,
    shot_id: &str,
) -> Result<()> {
    let mut previous: Option<&str> = None;
    for cue_id in &exposure.cue_ids {
        validate_id("cue", cue_id)?;
        if previous.is_some_and(|id| id >= cue_id.as_str()) {
            bail!(
                "track {} exposure {} cue_ids must be strictly sorted and unique",
                track_id,
                exposure.exposure_id
            );
        }
        if !all_cues.contains(cue_id.as_str()) {
            bail!("exposure references unknown narration cue {cue_id}");
        }
        if !shot_cues.contains(cue_id.as_str()) {
            bail!("narration cue {cue_id} is not bound to exposure sheet shot {shot_id}");
        }
        previous = Some(cue_id);
    }
    Ok(())
}

fn same_exposure_binding(left: &Exposure, right: &Exposure) -> bool {
    left.exposure_id == right.exposure_id
        && left.asset_sha256 == right.asset_sha256
        && left.cue_ids == right.cue_ids
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
