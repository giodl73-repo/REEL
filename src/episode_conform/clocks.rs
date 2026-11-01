//! Explicit repeat-only clock conversion, adapted from 21c3341.
use super::*;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutputFrameRate {
    pub numerator: u32,
    pub denominator: u32,
    pub policy: String,
}

impl OutputFrameRate {
    pub(super) fn clock(&self) -> Result<(u64, u64)> {
        let (mut a, mut b) = (u64::from(self.numerator), u64::from(self.denominator));
        if a == 0
            || b == 0
            || a > 1_000_000
            || b > 1_000_000
            || a > 120 * b
            || self.policy != "repeat-only"
        {
            bail!("invalid repeat-only output frame rate");
        }
        let (n, d) = (a, b);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        Ok((n / a, d / a))
    }

    pub(super) fn filter(&self, source: &str) -> Result<Option<String>> {
        let (n, d) = self.clock()?;
        let (sn, sd) = fps_parts(source)?;
        if u128::from(n) * u128::from(sd) < u128::from(sn) * u128::from(d) {
            bail!("repeat-only output frame rate would discard source frames");
        }
        Ok(
            (u128::from(n) * u128::from(sd) != u128::from(sn) * u128::from(d))
                .then(|| format!("fps=fps={n}/{d}:start_time=0:round=up:eof_action=pass")),
        )
    }

    pub(super) fn frames(&self, input_frames: u64, source: &str) -> Result<u64> {
        let (n, d) = self.clock()?;
        let (sn, sd) = fps_parts(source)?;
        if u128::from(n) * u128::from(sd) < u128::from(sn) * u128::from(d) {
            bail!("repeat-only output frame rate would discard source frames");
        }
        let numerator = u128::from(input_frames)
            .checked_mul(u128::from(n))
            .and_then(|v| v.checked_mul(u128::from(sd)))
            .context("repeat-only frame count overflow")?;
        let denominator = u128::from(d) * u128::from(sn);
        u64::try_from(numerator.div_ceil(denominator)).context("repeat-only frame count overflow")
    }

    /// Complete an original partial frame and, when changing clocks, the
    /// additional output rounding interval. No source PCM may be trimmed.
    pub(super) fn padding_limit(&self, sample_rate: u32, source: &str) -> Result<u64> {
        if sample_rate == 0 {
            bail!("invalid padding sample rate");
        }
        let (n, d) = self.clock()?;
        let (sn, sd) = fps_parts(source)?;
        let converting = self.filter(source)?.is_some();
        let denominator = u128::from(n) * u128::from(sn);
        let target = u128::from(sample_rate) * u128::from(d) * u128::from(sn);
        let source = if converting {
            u128::from(sample_rate) * u128::from(sd) * u128::from(n)
        } else {
            0
        };
        u64::try_from((target + source) / denominator + 1).context("frame padding clock overflow")
    }
}

#[derive(Debug, Serialize)]
pub struct FrameRateVerification {
    pub input_fps: String,
    pub output_fps: String,
    pub input_frames: u64,
    /// Excludes separately recorded silent tail holds.
    pub output_frames: u64,
    pub repeated_frames: u64,
    pub transformed_source_matches_selected: bool,
}

#[cfg(test)]
mod repeat_only_tests {
    use super::*;

    #[test]
    fn clock_is_explicit_reduced_bounded_and_cannot_drop_frames() {
        let rate = OutputFrameRate {
            numerator: 60,
            denominator: 2,
            policy: "repeat-only".into(),
        };
        assert_eq!(rate.clock().unwrap(), (30, 1));
        assert!(rate.filter("30/1").unwrap().is_none());
        assert!(
            rate.filter("25/1")
                .unwrap()
                .unwrap()
                .contains("round=up:eof_action=pass")
        );
        assert_eq!(rate.frames(11, "25/1").unwrap(), 14);
        assert_eq!(rate.frames(3015, "30/1").unwrap(), 3015);
        assert!(rate.filter("50/1").is_err());
        assert!(rate.frames(10, "50/1").is_err());
        assert!(rate.frames(u64::MAX, &format!("1/{}", u64::MAX)).is_err());
        assert_eq!(rate.padding_limit(48000, "24/1").unwrap(), 3601);
        let target_samples = rate.frames(25, "24/1").unwrap() * 1600;
        assert_eq!(target_samples - 48001, 3199);
        assert!(target_samples - 48001 <= rate.padding_limit(48000, "24/1").unwrap());
        for (numerator, denominator, policy) in [
            (0, 1, "repeat-only"),
            (30, 0, "repeat-only"),
            (121, 1, "repeat-only"),
            (30, 1, "nearest"),
        ] {
            assert!(
                OutputFrameRate {
                    numerator,
                    denominator,
                    policy: policy.into()
                }
                .clock()
                .is_err()
            );
        }
    }

    #[test]
    fn moving_source_retains_every_frame_and_audio_through_fractional_end() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.mkv");
        let selected = temp.path().join("selected.mkv");
        let status = command("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=32x32:r=25:d=0.44",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=437:sample_rate=48000:duration=0.44",
                "-ac",
                "2",
                "-shortest",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuv444p",
                "-c:a",
                "pcm_s24le",
            ])
            .arg(&source)
            .status()
            .unwrap();
        assert!(status.success());
        let rate = OutputFrameRate {
            numerator: 30,
            denominator: 1,
            policy: "repeat-only".into(),
        };
        let status = command("ffmpeg")
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(&source)
            .args(["-map", "0:v:0", "-map", "0:a:0", "-vf"])
            .arg(rate.filter("25/1").unwrap().unwrap())
            .args(["-c:v", "ffv1", "-pix_fmt", "yuv444p", "-c:a", "copy"])
            .arg(&selected)
            .status()
            .unwrap();
        assert!(status.success());
        let raw = |path: &Path| {
            let output = command("ffmpeg")
                .args(["-v", "error", "-nostdin", "-i"])
                .arg(path)
                .args(["-map", "0:v:0", "-pix_fmt", "rgb24", "-f", "rawvideo", "-"])
                .output()
                .unwrap();
            assert!(output.status.success());
            output.stdout
        };
        let before = raw(&source);
        let after = raw(&selected);
        let bytes_per_frame = 32 * 32 * 3;
        assert_eq!(before.len() / bytes_per_frame, 11);
        assert_eq!(after.len() / bytes_per_frame, 14);
        let mut original = before.chunks_exact(bytes_per_frame).peekable();
        for frame in after.chunks_exact(bytes_per_frame) {
            if original.peek().is_some_and(|next| *next == frame) {
                original.next();
            }
        }
        assert!(
            original.next().is_none(),
            "an original moving frame was lost or reordered"
        );
        assert_eq!(
            probe_with_inputs(&selected, Path::new("")).unwrap().fps,
            "30/1"
        );
        assert_eq!(
            decoded_digest(&source, false).unwrap(),
            decoded_digest(&selected, false).unwrap()
        );
    }
}
