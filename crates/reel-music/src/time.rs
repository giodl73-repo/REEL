use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudioTimebase {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub samples_per_channel: u64,
}

impl AudioTimebase {
    pub fn validate(&self) -> Result<()> {
        if !(8_000..=384_000).contains(&self.sample_rate_hz) {
            bail!("timebase.sample_rate_hz must be between 8000 and 384000");
        }
        if !(1..=32).contains(&self.channels) {
            bail!("timebase.channels must be between 1 and 32");
        }
        if self.samples_per_channel == 0 {
            bail!("timebase.samples_per_channel must be positive");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MusicalTimebase {
    pub pulses_per_quarter: u32,
    pub rounding: RoundingMode,
}

impl MusicalTimebase {
    pub fn validate(&self) -> Result<()> {
        if !(24..=15_360).contains(&self.pulses_per_quarter) {
            bail!("musical_timebase.pulses_per_quarter must be between 24 and 15360");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RoundingMode {
    HalfAwayFromZero,
    Floor,
    Ceiling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SampleRange {
    pub start: u64,
    pub end: u64,
}

impl SampleRange {
    pub fn validate(&self, total: u64, field: &str) -> Result<()> {
        if self.start >= self.end {
            bail!("{field} must use a non-empty half-open range");
        }
        if self.end > total {
            bail!("{field} ends beyond source sample count {total}");
        }
        Ok(())
    }

    pub fn len(self) -> u64 {
        self.end - self.start
    }

    pub fn is_empty(self) -> bool {
        self.start >= self.end
    }

    pub fn intersects(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    pub fn contains(self, other: Self) -> bool {
        self.start <= other.start && self.end >= other.end
    }
}

pub(crate) fn validate_ordered_nonoverlapping(
    ranges: &[SampleRange],
    total: u64,
    field: &str,
) -> Result<()> {
    let mut prior_end = 0;
    for (index, range) in ranges.iter().enumerate() {
        range.validate(total, &format!("{field}[{}]", index + 1))?;
        if index > 0 && range.start < prior_end {
            bail!("{field} must be ordered and non-overlapping");
        }
        prior_end = range.end;
    }
    Ok(())
}
