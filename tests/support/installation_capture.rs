//! Offline measurement of the synthetic English fixture at a digital capture point.
use anyhow::{Result, ensure};
use std::io::Read;

const RATE: usize = 48_000;
const MARKER_FRAMES: usize = 240;
const SOURCE_FRAMES: usize = 144_000;
const FIRST_MARKER: usize = 5_040;
const LAST_MARKER: usize = 138_720;
const TRANSPORT_GAP: usize = 12_000;
const MARKER_THRESHOLD: f32 = 0.015625;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    First,
    Last,
}

fn marker_at(frames: &[[f32; 2]]) -> Option<Marker> {
    let [left, right] = frames[0];
    let (kind, expected): (Marker, [f32; 2]) = if left > -right && right < -MARKER_THRESHOLD {
        (Marker::First, [2.0, -1.0])
    } else if -left < right && left < -MARKER_THRESHOLD {
        (Marker::Last, [-1.0, 2.0])
    } else {
        return None;
    };
    let matches = frames.iter().enumerate().all(|(frame, &[left, right])| {
        let polarity = if frame / 24 % 2 == 0 { 1.0 } else { -1.0 };
        left * polarity * expected[0].signum() > MARKER_THRESHOLD
            && right * polarity * expected[1].signum() > MARKER_THRESHOLD
            && ((left * expected[1] - right * expected[0]).abs()
                <= 0.20 * left.abs().max(right.abs()))
    });
    matches.then_some(kind)
}

fn measure(frames: &[[f32; 2]]) -> Result<(usize, [usize; 2])> {
    ensure!(
        frames.iter().flatten().all(|sample| sample.is_finite()),
        "capture contains a nonfinite sample"
    );
    let markers: Vec<_> = frames
        .windows(MARKER_FRAMES)
        .enumerate()
        .filter_map(|(offset, window)| marker_at(window).map(|marker| (offset, marker)))
        .take(5)
        .collect();
    let [
        (head_a, Marker::First),
        (tail_a, Marker::Last),
        (head_b, Marker::First),
        (tail_b, Marker::Last),
    ] = markers.as_slice()
    else {
        anyhow::bail!("expected exactly two ordered fixture marker pairs");
    };
    let gap = (head_b - tail_a)
        .checked_sub(SOURCE_FRAMES - LAST_MARKER + FIRST_MARKER)
        .ok_or_else(|| anyhow::anyhow!("capture has no measurable loop interval"))?;
    let spans = [tail_a - head_a, tail_b - head_b];
    Ok((gap, spans))
}

fn capture_passes(gap: usize, spans: [usize; 2]) -> bool {
    gap.abs_diff(TRANSPORT_GAP) <= RATE * 50 / 1000
        && spans
            .into_iter()
            .all(|span| span.abs_diff(LAST_MARKER - FIRST_MARKER) <= RATE * 20 / 1000)
}

#[test]
fn station_capture_measurement_detects_markers_and_applies_both_limits() {
    let mut frames = vec![[0.0; 2]; SOURCE_FRAMES * 2 + TRANSPORT_GAP];
    for pass in [0, SOURCE_FRAMES + TRANSPORT_GAP] {
        for (offset, pair) in [
            (FIRST_MARKER, [0.0625, -0.03125]),
            (LAST_MARKER, [-0.03125, 0.0625]),
        ] {
            for (index, frame) in frames[pass + offset..pass + offset + MARKER_FRAMES]
                .iter_mut()
                .enumerate()
            {
                *frame = if index / 24 % 2 == 0 {
                    pair
                } else {
                    [-pair[0], -pair[1]]
                };
            }
        }
    }
    let (gap, spans) = measure(&frames).unwrap();
    assert_eq!((gap, spans), (12_000, [133_680; 2]));
    assert!(capture_passes(gap, spans));
    assert!(!capture_passes(gap + 2_401, spans));
    assert!(!capture_passes(gap, [spans[0], spans[1] - 961]));
    frames[LAST_MARKER..LAST_MARKER + MARKER_FRAMES].fill([0.0; 2]);
    assert!(measure(&frames).is_err());
}

#[test]
#[ignore = "explicit analysis of retained digital fixture capture"]
fn measure_retained_station_capture() {
    let path = std::env::var_os("REEL_INSTALLATION_CAPTURE_F32LE")
        .expect("provide the retained stereo 48 kHz f32le capture path");
    const MAX_BYTES: usize = 120 * RATE * 8;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .unwrap()
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() <= MAX_BYTES && bytes.len() % 8 == 0);
    let frames: Vec<_> = bytes
        .chunks_exact(8)
        .map(|frame| {
            [
                f32::from_le_bytes(frame[..4].try_into().unwrap()),
                f32::from_le_bytes(frame[4..].try_into().unwrap()),
            ]
        })
        .collect();
    let (gap, spans) = measure(&frames).unwrap();
    println!(
        "capture SHA-256 {}; loop gap {} frames; source spans {:?} frames",
        reel::production::sha256_bytes(&bytes),
        gap,
        spans
    );
    assert!(capture_passes(gap, spans), "capture failed timing gates");
}
