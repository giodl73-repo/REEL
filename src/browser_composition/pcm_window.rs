//! Exact stereo PCM24 sample window and integer linear envelope.
//! No resampling, padding, floating point, storage lookup or process launch.
#[cfg(any(test, all(windows, feature = "tig-publication")))]
use anyhow::Context;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
#[cfg(any(test, all(windows, feature = "tig-publication")))]
use sha2::{Digest as _, Sha256};
#[cfg(any(test, all(windows, feature = "tig-publication")))]
use std::io::{Read, Write};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PcmWindow {
    pub source_samples: u64,
    pub start_sample: u64,
    pub end_sample: u64,
    pub fade_in_samples: u64,
    pub fade_out_samples: u64,
}
impl PcmWindow {
    pub(crate) fn validate(&self, frames: u64, fps: u32) -> Result<()> {
        if self.source_samples == 0
            || self.source_samples > 9_007_199_254_740_991 / 6
            || self.start_sample >= self.end_sample
            || self.end_sample > self.source_samples
            || self.fade_in_samples > self.samples()
            || self.fade_out_samples > self.samples() - self.fade_in_samples
            || fps == 0
            || u128::from(self.samples()) * u128::from(fps) != u128::from(frames) * 48_000
        {
            bail!("invalid exact 48kHz stereo PCM window or frame duration");
        }
        Ok(())
    }
    pub(crate) fn samples(&self) -> u64 {
        self.end_sample.saturating_sub(self.start_sample)
    }
}

/// Read the complete normalized source, including the unselected suffix, so a
/// truncated/excess source cannot pass as the declared original. The caller owns
/// output retirement on error and provides cancellation/deadline checks.
#[cfg(any(test, all(windows, feature = "tig-publication")))]
pub(crate) fn transform(
    mut input: impl Read,
    mut output: impl Write,
    window: &PcmWindow,
    mut check: impl FnMut() -> Result<()>,
) -> Result<(String, String)> {
    // Validate the window independently of any caller's manifest validation.
    window.validate(window.samples(), 48_000)?;
    let mut selected = Sha256::new();
    let mut envelope = Sha256::new();
    for absolute in 0..window.source_samples {
        if absolute % 4096 == 0 {
            check()?;
        }
        let mut sample = [0u8; 6];
        input
            .read_exact(&mut sample)
            .context("normalized PCM source is truncated")?;
        if absolute < window.start_sample || absolute >= window.end_sample {
            continue;
        }
        let i = absolute - window.start_sample;
        selected.update(sample);
        let gain = if i < window.fade_in_samples {
            Some((i, window.fade_in_samples))
        } else if i >= window.samples() - window.fade_out_samples {
            Some((window.samples() - 1 - i, window.fade_out_samples))
        } else {
            None
        };
        if let Some((numerator, denominator)) = gain {
            for channel in [0, 3] {
                let value = i32::from_le_bytes([
                    0,
                    sample[channel],
                    sample[channel + 1],
                    sample[channel + 2],
                ]) >> 8;
                // Signed integer division truncates toward zero, exactly as the
                // original production envelope; FFmpeg afade is not equivalent.
                let value =
                    (i128::from(value) * i128::from(numerator) / i128::from(denominator)) as i32;
                sample[channel..channel + 3].copy_from_slice(&value.to_le_bytes()[..3]);
            }
        }
        envelope.update(sample);
        output.write_all(&sample)?;
    }
    let mut excess = [0];
    if input.read(&mut excess)? != 0 {
        bail!("normalized PCM source has excess samples");
    }
    check()?;
    output.flush()?;
    Ok((
        selected
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        envelope
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bytes(values: &[i32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|v| v.to_le_bytes()[..3].repeat(2))
            .collect()
    }
    fn window() -> PcmWindow {
        PcmWindow {
            source_samples: 8,
            start_sample: 1,
            end_sample: 7,
            fade_in_samples: 2,
            fade_out_samples: 3,
        }
    }
    #[test]
    fn signed_rounding_endpoints_and_nonzero_window_are_exact() {
        let raw = bytes(&[100, 7, -7, 8_388_607, -8_388_608, 7, -7, 100]);
        let mut output = vec![];
        let (selected, envelope) = transform(&raw[..], &mut output, &window(), || Ok(())).unwrap();
        assert_eq!(output, bytes(&[0, -3, 8_388_607, -5_592_405, 2, 0]));
        assert_eq!(selected, crate::production::sha256_bytes(&raw[6..42]));
        assert_eq!(envelope, crate::production::sha256_bytes(&output));
    }
    #[test]
    fn rejects_truncation_excess_overlap_and_cancellation() {
        let raw = bytes(&[1; 8]);
        assert!(transform(&raw[..47], Vec::new(), &window(), || Ok(())).is_err());
        assert!(
            transform(
                &[raw.clone(), vec![0]].concat()[..],
                Vec::new(),
                &window(),
                || Ok(())
            )
            .is_err()
        );
        let mut invalid = window();
        invalid.fade_out_samples = 5;
        assert!(transform(&raw[..], Vec::new(), &invalid, || Ok(())).is_err());
        assert!(transform(&raw[..], Vec::new(), &window(), || bail!("cancelled")).is_err());
        assert!(window().validate(1, 24).is_err());
    }
    /// Explicitly enrolled private normalized source; never an implicit lookup.
    #[test]
    fn enrolled_original_pcm_window_matches_retained_production() {
        let Some(path) = std::env::var_os("REEL_TEST_PCM24_FIXTURE") else {
            return;
        };
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fixture {
            source: std::path::PathBuf,
            window: PcmWindow,
            selected_sha256: String,
            envelope_sha256: String,
        }
        let fixture: Fixture = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let raw = std::io::BufReader::new(std::fs::File::open(fixture.source).unwrap());
        let (selected, envelope) =
            transform(raw, std::io::sink(), &fixture.window, || Ok(())).unwrap();
        assert_eq!(selected, fixture.selected_sha256);
        assert_eq!(envelope, fixture.envelope_sha256);
    }
}
