//! Explicit bounded last-picture/zero-audio holds. Adapted from fa1d382.
use super::*;

pub(super) fn hold_samples(
    frames: u64,
    start_frame: u64,
    sample_rate: u32,
    fps_num: u64,
    fps_den: u64,
) -> Result<u64> {
    if fps_num == 0
        || fps_den == 0
        || sample_rate == 0
        || u128::from(frames) * u128::from(fps_den) > u128::from(fps_num) * 60
    {
        bail!("silent picture hold requires a valid clock and at most sixty seconds");
    }
    let boundary = |frame: u64| -> Result<u64> {
        let value = u128::from(frame)
            .checked_mul(u128::from(sample_rate))
            .and_then(|v| v.checked_mul(u128::from(fps_den)))
            .and_then(|v| v.checked_add(u128::from(fps_num) / 2))
            .context("hold sample clock overflow")?
            / u128::from(fps_num);
        u64::try_from(value).context("hold sample clock overflow")
    };
    boundary(
        start_frame
            .checked_add(frames)
            .context("hold frame clock overflow")?,
    )?
    .checked_sub(boundary(start_frame)?)
    .context("hold sample clock order")
}

/// Verify the entire prefix and the added tail independently of the encoder.
type MediaDigest = (String, u64);

pub(super) fn append_silent_picture_hold(
    source: &Path,
    output: &Path,
    frames: u64,
    samples: u64,
    video_codec: &str,
    programs: &(impl scene_delivery::InputFiles + ?Sized),
) -> Result<(MediaDigest, MediaDigest)> {
    if frames == 0 || samples == 0 || !matches!(video_codec, "h264" | "ffv1") {
        bail!("invalid silent picture hold");
    }
    let filter = format!("tpad=stop_mode=clone:stop={frames}");
    let expected_picture =
        decoded_digest_with_options(source, true, 0, Some(&filter), "yuv444p", programs)?;
    let expected_audio =
        decoded_digest_with_options(source, false, samples, None, "yuv444p", programs)?;
    let expected_rgb =
        decoded_digest_with_options(source, true, 0, Some(&filter), "rgb24", programs)?;
    let source_facts = probe_with_inputs(source, programs)?;
    let mut encoder = command_with_inputs(programs, scene_delivery::MediaProgram::Ffmpeg)?;
    encoder
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(source)
        .args(["-map", "0:v:0", "-map", "0:a:0", "-vf", &filter, "-af"])
        .arg(format!("apad=pad_len={samples},asetpts=N/SR/TB"));
    if video_codec == "h264" {
        encoder.args([
            "-c:v", "libx264", "-crf", "0", "-preset", "veryfast", "-bf", "0",
        ]);
    } else {
        encoder.args(["-c:v", "ffv1"]);
    }
    for (key, flag) in [
        ("color_range", "-color_range"),
        ("color_space", "-colorspace"),
        ("color_transfer", "-color_trc"),
        ("color_primaries", "-color_primaries"),
    ] {
        let value = &source_facts.color_properties[key];
        if value != "unknown" {
            encoder.args([flag, value]);
        }
    }
    if !encoder
        .args([
            "-pix_fmt",
            "yuv444p",
            "-fps_mode",
            "passthrough",
            "-c:a",
            "pcm_s24le",
        ])
        .arg(output)
        .status()?
        .success()
    {
        bail!("silent picture hold encoding failed");
    }
    let picture = decoded_digest_with_inputs(output, true, programs)?;
    let audio = decoded_digest_with_inputs(output, false, programs)?;
    let rgb = decoded_digest_with_options(output, true, 0, None, "rgb24", programs)?;
    if picture != expected_picture || audio != expected_audio || rgb != expected_rgb {
        bail!("silent picture hold changed source prefix, last frame or zero-audio tail");
    }
    Ok((picture, audio))
}

#[cfg(test)]
mod silent_hold_tests {
    use super::*;

    #[test]
    fn explicit_tool_denial_never_uses_path_or_creates_a_hold() {
        struct Denied;
        impl scene_delivery::InputFiles for Denied {
            fn verified_path(&self, _: &scene_delivery::FileRef) -> Result<PathBuf> {
                bail!("denied")
            }
            fn media_program(&self, _: scene_delivery::MediaProgram) -> Result<PathBuf> {
                bail!("denied")
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let out = temp.path().join("must-not-exist.mkv");
        assert!(
            append_silent_picture_hold(Path::new("absent"), &out, 1, 2000, "ffv1", &Denied)
                .is_err()
        );
        assert!(!out.exists());
    }

    #[test]
    fn hold_clock_is_bounded_checked_and_uses_global_fractional_frame_boundaries() {
        assert_eq!(hold_samples(48, 24, 48000, 24, 1).unwrap(), 96000);
        assert_eq!(hold_samples(60, 17, 44100, 30000, 1001).unwrap(), 88288);
        assert!(hold_samples(1441, 0, 48000, 24, 1).is_err());
        assert!(hold_samples(48, u64::MAX, 48000, 24, 1).is_err());
        assert!(hold_samples(1, u64::MAX - 1, u32::MAX, u64::MAX, u64::MAX).is_err());
        assert!(hold_samples(1, 0, 48000, 0, 1).is_err());
    }

    #[test]
    fn lossless_hold_keeps_the_moving_picture_and_tone_prefix_then_adds_only_last_picture_and_zero_audio()
     {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.mkv");
        assert!(
            command("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=s=32x32:r=24:d=1",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=937:sample_rate=48000:duration=1",
                    "-c:v",
                    "ffv1",
                    "-pix_fmt",
                    "yuv444p",
                    "-colorspace",
                    "bt709",
                    "-color_trc",
                    "bt709",
                    "-color_primaries",
                    "bt709",
                    "-color_range",
                    "tv",
                    "-c:a",
                    "pcm_s24le",
                    "-ac",
                    "2"
                ])
                .arg(&source)
                .status()
                .unwrap()
                .success()
        );
        let original_picture = decoded_digest(&source, true).unwrap();
        let original_audio = decoded_digest(&source, false).unwrap();
        for codec in ["ffv1", "h264"] {
            let output = temp.path().join(format!("held-{codec}.mkv"));
            let (picture, audio) =
                append_silent_picture_hold(&source, &output, 48, 96000, codec, Path::new(""))
                    .unwrap();
            assert_eq!(picture.1, 72 * 32 * 32 * 3);
            assert_eq!(audio.1, 144000 * 6);
            assert_eq!(
                decoded_digest_with_filter(&output, true, 0, Some("trim=end_frame=24")).unwrap(),
                original_picture
            );
            let pcm = command("ffmpeg")
                .args(["-v", "error", "-i"])
                .arg(&output)
                .args(["-map", "0:a:0", "-c:a", "pcm_s24le", "-f", "s24le", "-"])
                .output()
                .unwrap();
            assert!(pcm.status.success());
            let prefix = Sha256::digest(&pcm.stdout[..48000 * 6])
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(prefix, original_audio.0);
            assert!(pcm.stdout[48000 * 6..].iter().all(|v| *v == 0));
            let source_bytes = fs::read(&source).unwrap();
            assert!(!source_bytes.is_empty());
        }
    }
}
