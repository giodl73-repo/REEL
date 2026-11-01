//! Shared renderer/checker contract over an already validated compiled job/plan.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OutputContract {
    /// Native receipt outputs; receipt.json is a separate control artifact.
    pub names: BTreeSet<String>,
    /// Data bytes for one stereo PCM24 stream, excluding container headers.
    pub raw_pcm24_stereo_bytes: u64,
    /// Data bytes for one RGB24 video stream; not a compressed-size bound.
    pub raw_rgb24_video_bytes: u64,
}
pub fn output_contract(job: &Job, plan: &Plan) -> Result<OutputContract> {
    let mut names: BTreeSet<String> = [
        "picture.mkv",
        "D.wav",
        "M.wav",
        "E.wav",
        "mix.wav",
        "master.mkv",
        "review.mp4",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let overlays: Vec<_> = job
        .external_layers
        .iter()
        .filter(|layer| {
            matches!(
                layer.render_mode,
                ExternalLayerRenderMode::AssOverlay
                    | ExternalLayerRenderMode::TimedVideoOverlay
                    | ExternalLayerRenderMode::TimedStillOverlay
                    | ExternalLayerRenderMode::TimedPngSequenceOverlay
            )
        })
        .collect();
    if !overlays.is_empty() {
        names.insert("clean-picture.mkv".into());
    }
    if overlays.len() > 1 {
        for (index, layer) in overlays.iter().enumerate() {
            names.insert(indexed_layer_source_name(layer, index));
            if index + 1 < overlays.len() {
                names.insert(format!("layered-picture-{index:03}.mkv"));
            }
        }
    } else if let Some(layer) = overlays.first() {
        names.insert(single_overlay_source_name(layer).into());
    }
    if job.post_compose_camera.is_some() {
        names.insert("pre-camera-picture.mkv".into());
    }
    names.extend(crate::motioncraft_review::output_names(job, plan)?);
    if let Some(font) = overlays.iter().find_map(|layer| layer.font.as_ref()) {
        names.insert(format!("fonts/{}", font_stage_name(font)?));
    }
    let pcm = plan
        .duration_samples
        .checked_mul(6)
        .context("PCM planning size overflow")?;
    let rgb = plan
        .frame_count
        .checked_mul(u64::from(job.width))
        .and_then(|n| n.checked_mul(u64::from(job.height)))
        .and_then(|n| n.checked_mul(3))
        .context("RGB planning size overflow")?;
    Ok(OutputContract {
        names,
        raw_pcm24_stereo_bytes: pcm,
        raw_rgb24_video_bytes: rgb,
    })
}
