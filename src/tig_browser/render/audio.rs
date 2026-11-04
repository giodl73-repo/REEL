//! Owned, bounded intermediates from a held original PCM primary.
use super::*;
use crate::browser_composition::{PcmWindow, pcm_window};
use std::io::{BufReader, BufWriter};

pub(super) struct ExactAudio {
    pub path: tempfile::TempPath,
    pub sha256: String,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    programs: &BoundPrograms,
    inputs: &dyn InputFiles,
    source: &Path,
    window: &PcmWindow,
    workspace: &Path,
    limits: &Limits,
    deadline: Instant,
    authority: &mut impl FnMut() -> Result<()>,
) -> Result<ExactAudio> {
    window.validate(window.samples(), 48_000)?;
    let raw_bytes = window
        .source_samples
        .checked_mul(6)
        .context("PCM source size overflow")?;
    let scratch = raw_bytes
        .checked_add(window.samples() * 6)
        .context("PCM workspace overflow")?;
    if scratch
        >= limits
            .max_workspace_bytes
            .saturating_sub(limits.max_output_bytes)
    {
        bail!("exact PCM intermediates exceed browser workspace budget");
    }
    let metadata = tempfile::Builder::new()
        .prefix("pcm-probe-")
        .suffix(".json")
        .tempfile_in(workspace)?;
    let run = |tool, args, authority: &mut dyn FnMut() -> Result<()>| -> Result<()> {
        authority()?;
        let mut task = Task::start(
            &programs.with_inputs(inputs).media_program(tool)?,
            args,
            workspace,
            limits.max_workspace_bytes,
            deadline
                .checked_duration_since(Instant::now())
                .context("PCM preparation deadline elapsed")?,
            limits.encoder(),
        )?;
        // Keep source authority live while quiet child normalization runs.
        while !task.ended() {
            authority()?;
            pause(deadline)?;
        }
        task.wait(deadline)?;
        authority()
    };
    run(
        MediaProgram::Ffprobe,
        vec![
            "-v".into(),
            "error".into(),
            "-show_entries".into(),
            "stream=codec_type,codec_name,sample_rate,channels".into(),
            "-of".into(),
            "json".into(),
            "-o".into(),
            native_path(metadata.path())?,
            native_path(source)?,
        ],
        authority,
    )?;
    if metadata.as_file().metadata()?.len() > 65_536 {
        bail!("PCM probe exceeds metadata budget");
    }
    let report: serde_json::Value = serde_json::from_slice(&fs::read(metadata.path())?)?;
    let streams = report["streams"]
        .as_array()
        .context("PCM source streams missing")?;
    if streams.len() != 1
        || streams[0]["codec_type"] != "audio"
        || streams[0]["sample_rate"] != "48000"
        || streams[0]["channels"] != 2
        || !matches!(
            streams[0]["codec_name"].as_str(),
            Some("pcm_s16le" | "pcm_s24le" | "pcm_s32le" | "pcm_f32le" | "pcm_f64le")
        )
    {
        bail!("exact PCM window needs original stereo PCM at 48kHz");
    }
    let normalized = tempfile::Builder::new()
        .prefix("pcm-original-")
        .suffix(".s24le")
        .tempfile_in(workspace)?;
    run(
        MediaProgram::Ffmpeg,
        vec![
            "-y".into(),
            "-v".into(),
            "error".into(),
            "-nostdin".into(),
            "-threads".into(),
            "1".into(),
            "-i".into(),
            native_path(source)?,
            "-map".into(),
            "0:a:0".into(),
            "-c:a".into(),
            "pcm_s24le".into(),
            "-f".into(),
            "s24le".into(),
            "-fs".into(),
            (raw_bytes + 6).to_string(),
            native_path(normalized.path())?,
        ],
        authority,
    )?;
    if normalized.as_file().metadata()?.len() != raw_bytes {
        bail!("original PCM source sample count differs");
    }
    let output = tempfile::Builder::new()
        .prefix("pcm-envelope-")
        .suffix(".s24le")
        .tempfile_in(workspace)?;
    let (_, sha256) = pcm_window::transform(
        BufReader::new(fs::File::open(normalized.path())?),
        BufWriter::new(output.as_file()),
        window,
        || {
            authority()?;
            if Instant::now() >= deadline {
                bail!("PCM envelope deadline elapsed");
            }
            Ok(())
        },
    )?;
    Ok(ExactAudio {
        path: output.into_temp_path(),
        sha256,
    })
}
