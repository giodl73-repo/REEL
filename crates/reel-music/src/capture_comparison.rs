use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{hash::sha256_bytes, validate_sha256};

#[derive(Deserialize)]
struct Capture {
    schema: String,
    input_sha256: String,
    events: Vec<Event>,
}

#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    instrument: Option<String>,
    pitch: Option<u8>,
    start_time: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct NoteMatch {
    pitch: u8,
    left_start_seconds: f64,
    right_start_seconds: f64,
}

#[derive(Debug, Serialize)]
pub struct Comparison {
    schema: &'static str,
    left_capture_sha256: String,
    right_capture_sha256: String,
    left_input_sha256: String,
    right_input_sha256: String,
    left_label: String,
    right_label: String,
    tolerance_milliseconds: u32,
    left_note_starts: usize,
    right_note_starts: usize,
    matched: Vec<NoteMatch>,
    limitation: &'static str,
}

fn notes(capture: Capture, label: &str) -> Result<BTreeMap<u8, Vec<f64>>> {
    if capture.schema != "reel.muscriptor-capture.v1" {
        bail!("unsupported capture schema");
    }
    validate_sha256("capture input", &capture.input_sha256)?;
    let mut result = BTreeMap::<_, Vec<f64>>::new();
    for event in capture.events {
        if event.kind == "start" && event.instrument.as_deref() == Some(label) {
            let start = event.start_time.context("note start time missing")?;
            let pitch = event.pitch.context("note pitch missing")?;
            if !start.is_finite() || start < 0.0 || pitch > 127 {
                bail!("invalid capture note");
            }
            result.entry(pitch).or_default().push(start);
        }
    }
    for group in result.values_mut() {
        group.sort_by(f64::total_cmp);
    }
    Ok(result)
}

/// Compare candidate note starts on a caller-established common excerpt clock.
/// Agreements can localize musical material even when instrument labels conflict.
pub fn compare(
    left: &Path,
    right: &Path,
    left_label: &str,
    right_label: &str,
    tolerance_milliseconds: u32,
) -> Result<Comparison> {
    if !(1..=250).contains(&tolerance_milliseconds) {
        bail!("note start tolerance must be between 1 and 250 milliseconds");
    }
    crate::nonempty("left instrument label", left_label)?;
    crate::nonempty("right instrument label", right_label)?;
    let left_bytes = fs::read(left)?;
    let right_bytes = fs::read(right)?;
    let left: Capture = serde_json::from_slice(&left_bytes)?;
    let right: Capture = serde_json::from_slice(&right_bytes)?;
    let left_input_sha256 = left.input_sha256.clone();
    let right_input_sha256 = right.input_sha256.clone();
    let left = notes(left, left_label)?;
    let right = notes(right, right_label)?;
    let tolerance = f64::from(tolerance_milliseconds) / 1000.0;
    let mut matched = Vec::new();
    for (&pitch, left_starts) in &left {
        let Some(right_starts) = right.get(&pitch) else {
            continue;
        };
        let (mut i, mut j) = (0, 0);
        while i < left_starts.len() && j < right_starts.len() {
            let (a, b) = (left_starts[i], right_starts[j]);
            if a + tolerance < b {
                i += 1;
            } else if b + tolerance < a {
                j += 1;
            } else {
                matched.push(NoteMatch {
                    pitch,
                    left_start_seconds: a,
                    right_start_seconds: b,
                });
                i += 1;
                j += 1;
            }
        }
    }
    matched.sort_by(|a, b| a.left_start_seconds.total_cmp(&b.left_start_seconds));
    Ok(Comparison {
        schema: "reel.music-capture-comparison.v1",
        left_capture_sha256: sha256_bytes(&left_bytes),
        right_capture_sha256: sha256_bytes(&right_bytes),
        left_input_sha256,
        right_input_sha256,
        left_label: left_label.into(),
        right_label: right_label.into(),
        tolerance_milliseconds,
        left_note_starts: left.values().map(Vec::len).sum(),
        right_note_starts: right.values().map(Vec::len).sum(),
        matched,
        limitation: "Candidate pitch and onset agreement on a caller-established common clock. Instrument identity and clock lineage require independent evidence.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_repeated_pitches_one_to_one_across_different_labels() {
        let folder = tempfile::tempdir().unwrap();
        let left = folder.path().join("left.json");
        let right = folder.path().join("right.json");
        let fixture = |label, clocks: &[f64]| {
            let events: Vec<_> = clocks.iter().map(|start| {
                serde_json::json!({"type":"start","instrument":label,"pitch":60,"start_time":start})
            }).collect();
            serde_json::to_vec(&serde_json::json!({
                "schema":"reel.muscriptor-capture.v1","input_sha256":"1".repeat(64),"events":events
            }))
            .unwrap()
        };
        fs::write(&left, fixture("trumpet", &[1.0, 1.09, 2.0])).unwrap();
        fs::write(&right, fixture("sax", &[1.04, 2.1])).unwrap();
        let report = compare(&left, &right, "trumpet", "sax", 120).unwrap();
        assert_eq!(report.left_note_starts, 3);
        assert_eq!(report.right_note_starts, 2);
        assert_eq!(report.matched.len(), 2);
        assert_eq!(report.matched[0].left_start_seconds, 1.0);
        assert_eq!(report.matched[1].left_start_seconds, 2.0);
    }
}
