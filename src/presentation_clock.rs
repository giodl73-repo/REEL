//! Shared immutable measured-source clock vocabulary.
use crate::scene_delivery::FileRef;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceClock<E = FileRef> {
    pub schema: String,
    pub source_sha256: String,
    pub sample_rate: u32,
    pub total_samples: u64,
    pub evidence: E,
    pub anchors: Vec<ClockAnchor>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClockAnchor {
    pub id: String,
    pub sample: u64,
}
