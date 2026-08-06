use std::{fs, path::Path};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::{production, series::parse_srt};

pub const CAPTION_CHECK_SCHEMA: &str = "reel.caption-check.v0.1";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptionThresholds {
    pub max_chars_per_line: usize,
    pub max_lines_per_cue: usize,
    pub max_reading_speed_cps: f64,
    pub min_duration_ms: u64,
}

impl Default for CaptionThresholds {
    fn default() -> Self {
        Self {
            max_chars_per_line: 42,
            max_lines_per_cue: 2,
            max_reading_speed_cps: 20.0,
            min_duration_ms: 1_000,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CaptionThresholdReport {
    pub max_chars_per_line: usize,
    pub max_lines_per_cue: usize,
    pub max_reading_speed_cps: f64,
    pub min_duration_ms: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CaptionViolation {
    pub cue: usize,
    pub code: String,
    pub measured: f64,
    pub limit: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CaptionCheckReport {
    pub schema: String,
    pub captions_sha256: String,
    pub cues: usize,
    pub timeline_end_ms: u64,
    pub captioned_ms: u64,
    pub max_chars_per_line: usize,
    pub max_lines_per_cue: usize,
    pub max_reading_speed_cps: f64,
    pub thresholds: CaptionThresholdReport,
    pub violations: Vec<CaptionViolation>,
    pub passed: bool,
}

pub fn check(
    captions: impl AsRef<Path>,
    thresholds: CaptionThresholds,
) -> Result<CaptionCheckReport> {
    if thresholds.max_chars_per_line == 0
        || thresholds.max_lines_per_cue == 0
        || !thresholds.max_reading_speed_cps.is_finite()
        || thresholds.max_reading_speed_cps <= 0.0
        || thresholds.min_duration_ms == 0
    {
        bail!("caption thresholds must be positive and finite");
    }
    let captions = captions.as_ref();
    let bytes = fs::read(captions)?;
    let text = std::str::from_utf8(&bytes)?;
    let entries = parse_srt(text)?;
    if entries.is_empty() {
        bail!("SRT contains no cues");
    }
    if entries[0].index != 1 {
        bail!("SRT cue indexes must begin at 1");
    }

    let mut violations = Vec::new();
    let mut max_chars_per_line = 0;
    let mut max_lines_per_cue = 0;
    let mut max_reading_speed_cps = 0.0_f64;
    let mut captioned_ms = 0_u64;
    for entry in &entries {
        let duration_ms = entry.end_ms - entry.start_ms;
        captioned_ms = captioned_ms
            .checked_add(duration_ms)
            .ok_or_else(|| anyhow::anyhow!("captioned duration exceeds supported range"))?;
        let line_lengths = entry
            .text
            .lines()
            .map(|line| line.trim().chars().count())
            .collect::<Vec<_>>();
        let cue_max_line = line_lengths.iter().copied().max().unwrap_or_default();
        let cue_lines = line_lengths.len();
        let visible_chars = entry
            .text
            .chars()
            .filter(|character| !character.is_control())
            .count();
        let reading_speed_cps = visible_chars as f64 * 1000.0 / duration_ms as f64;
        max_chars_per_line = max_chars_per_line.max(cue_max_line);
        max_lines_per_cue = max_lines_per_cue.max(cue_lines);
        max_reading_speed_cps = max_reading_speed_cps.max(reading_speed_cps);

        if duration_ms < thresholds.min_duration_ms {
            violations.push(CaptionViolation {
                cue: entry.index,
                code: "minimum-duration".to_string(),
                measured: duration_ms as f64,
                limit: thresholds.min_duration_ms as f64,
            });
        }
        if cue_max_line > thresholds.max_chars_per_line {
            violations.push(CaptionViolation {
                cue: entry.index,
                code: "characters-per-line".to_string(),
                measured: cue_max_line as f64,
                limit: thresholds.max_chars_per_line as f64,
            });
        }
        if cue_lines > thresholds.max_lines_per_cue {
            violations.push(CaptionViolation {
                cue: entry.index,
                code: "lines-per-cue".to_string(),
                measured: cue_lines as f64,
                limit: thresholds.max_lines_per_cue as f64,
            });
        }
        if reading_speed_cps > thresholds.max_reading_speed_cps {
            violations.push(CaptionViolation {
                cue: entry.index,
                code: "reading-speed-cps".to_string(),
                measured: reading_speed_cps,
                limit: thresholds.max_reading_speed_cps,
            });
        }
    }

    Ok(CaptionCheckReport {
        schema: CAPTION_CHECK_SCHEMA.to_string(),
        captions_sha256: production::sha256_path(captions)?,
        cues: entries.len(),
        timeline_end_ms: entries.last().map(|entry| entry.end_ms).unwrap_or_default(),
        captioned_ms,
        max_chars_per_line,
        max_lines_per_cue,
        max_reading_speed_cps,
        thresholds: CaptionThresholdReport {
            max_chars_per_line: thresholds.max_chars_per_line,
            max_lines_per_cue: thresholds.max_lines_per_cue,
            max_reading_speed_cps: thresholds.max_reading_speed_cps,
            min_duration_ms: thresholds.min_duration_ms,
        },
        passed: violations.is_empty(),
        violations,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn reports_accessibility_metrics_without_caption_text_or_paths() {
        let report = check(
            "manifests/fixtures/vertical-sound-off/captions.srt",
            CaptionThresholds::default(),
        )
        .unwrap();
        assert!(report.passed);
        assert_eq!(report.cues, 2);
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("La lluvia"));
        assert!(!json.contains("vertical-sound-off"));
        assert!(!json.contains("path"));
    }

    #[test]
    fn collects_duration_line_and_reading_speed_violations() {
        let dir = tempdir().unwrap();
        let captions = dir.path().join("captions.srt");
        fs::write(
            &captions,
            "1\n00:00:00,000 --> 00:00:00,500\nThis deliberately overlong caption line cannot be read in time.\n\n2\n00:00:01,000 --> 00:00:03,000\nOne\nTwo\nThree\n",
        )
        .unwrap();
        let report = check(&captions, CaptionThresholds::default()).unwrap();
        assert!(!report.passed);
        assert!(
            report
                .violations
                .iter()
                .any(|item| item.code == "minimum-duration")
        );
        assert!(
            report
                .violations
                .iter()
                .any(|item| item.code == "characters-per-line")
        );
        assert!(
            report
                .violations
                .iter()
                .any(|item| item.code == "lines-per-cue")
        );
        assert!(
            report
                .violations
                .iter()
                .any(|item| item.code == "reading-speed-cps")
        );
    }

    #[test]
    fn rejects_out_of_range_or_overflowing_srt_timestamps() {
        let dir = tempdir().unwrap();
        let captions = dir.path().join("captions.srt");
        fs::write(
            &captions,
            "1\n00:99:00,000 --> 18446744073709551615:00:00,000\nHidden text\n",
        )
        .unwrap();
        assert!(check(&captions, CaptionThresholds::default()).is_err());
    }
}
