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

#[cfg(windows)]
#[test]
#[ignore = "records an explicitly isolated synthetic WASAPI render endpoint"]
fn record_isolated_station_loopback() {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use reel::media_runtime::ByteIdentity;
    use std::{
        io::Write,
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc::{RecvTimeoutError, sync_channel},
        },
        time::{Duration, Instant},
    };

    assert_eq!(
        std::env::var("REEL_INSTALLATION_CAPTURE_SCOPE").as_deref(),
        Ok("isolated-synthetic-render")
    );
    let request_path = std::env::var_os("REEL_INSTALLATION_STATION_SESSION").unwrap();
    let request = reel::spoken_programme::installation::native::SessionRequest::parse(
        &std::fs::read(request_path).unwrap(),
    )
    .unwrap();
    assert_eq!(
        (request.edition_id.as_str(), request.passes),
        ("en-technical", 2)
    );
    let output = PathBuf::from(std::env::var_os("REEL_INSTALLATION_CAPTURE_OUTPUT").unwrap());
    let parent = output.parent().unwrap();
    assert!(output.is_absolute() && !output.exists());
    let clock_path = output.with_extension("clock.json");
    assert!(!clock_path.exists());

    let id: cpal::DeviceId = request.endpoint.device_id.parse().unwrap();
    assert_eq!(id.host(), cpal::HostId::Wasapi);
    let host = cpal::host_from_id(cpal::HostId::Wasapi).unwrap();
    let mut devices = host
        .output_devices()
        .unwrap()
        .filter(|device| device.id().is_ok_and(|found| found == id));
    let device = devices.next().expect("pinned render endpoint unavailable");
    assert!(devices.next().is_none(), "ambiguous render endpoint");
    let mix = device.default_output_config().unwrap();
    assert_eq!(
        (mix.sample_rate(), mix.channels(), mix.sample_format()),
        (RATE as u32, 2, cpal::SampleFormat::F32)
    );

    // This test owns the capture stream. Its error callback and bounded queue
    // make discontinuities and consumer stalls visible to the recorder.
    let (send, receive) = sync_channel(32);
    let failed = Arc::new(AtomicBool::new(false));
    let data_failed = Arc::clone(&failed);
    let error_failed = Arc::clone(&failed);
    let stream = device
        .build_input_stream::<f32, _, _>(
            mix.config(),
            move |data, info| {
                if data.is_empty()
                    || data.len() > 4096
                    || data.len() % 2 != 0
                    || send.try_send((data.to_vec(), info.timestamp())).is_err()
                {
                    data_failed.store(true, Ordering::Release);
                }
            },
            move |_| error_failed.store(true, Ordering::Release),
            Some(Duration::from_secs(2)),
        )
        .unwrap();
    let mut audio = tempfile::NamedTempFile::new_in(parent).unwrap();
    let mut clock = tempfile::NamedTempFile::new_in(parent).unwrap();
    let mut packets = Vec::new();
    let mut frames = 0;
    stream.play().unwrap();
    println!("Isolated render loopback started. Run the two-pass playback now.");
    let deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < deadline && !failed.load(Ordering::Acquire) {
        match receive.recv_timeout(Duration::from_millis(25)) {
            Ok((data, timestamp)) => {
                packets.push(serde_json::json!({
                    "offset_frames": frames,
                    "frames": data.len() / 2,
                    "capture_qpc_ns": u64::try_from(timestamp.capture.as_nanos()).unwrap(),
                    "callback_qpc_ns": u64::try_from(timestamp.callback.as_nanos()).unwrap(),
                }));
                frames += data.len() / 2;
                assert!(frames <= 46 * RATE);
                let bytes: Vec<u8> = data.into_iter().flat_map(f32::to_le_bytes).collect();
                audio.write_all(&bytes).unwrap();
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => panic!("capture stream ended early"),
        }
    }
    drop(stream);
    assert!(
        !failed.load(Ordering::Acquire),
        "capture callback, geometry or queue failure"
    );
    assert!(frames > 0, "capture delivered no frames");
    audio.as_file().sync_all().unwrap();
    let audio_identity = ByteIdentity::observe_file(audio.path()).unwrap();
    let recorder_identity = ByteIdentity::observe_file(&std::env::current_exe().unwrap()).unwrap();
    serde_json::to_writer_pretty(
        &mut clock,
        &serde_json::json!({
            "capture_point": "WASAPI endpoint loopback before physical output",
            "binding": request.endpoint,
            "cpal_version": "0.18.2",
            "sample_rate": RATE,
            "channels": 2,
            "encoding": "f32le",
            "audio_sha256": audio_identity.sha256,
            "audio_bytes": audio_identity.bytes,
            "recorder_sha256": recorder_identity.sha256,
            "recorder_bytes": recorder_identity.bytes,
            "total_frames": frames,
            "packets": packets,
        }),
    )
    .unwrap();
    clock.as_file().sync_all().unwrap();
    audio.persist_noclobber(&output).unwrap();
    clock.persist_noclobber(clock_path).unwrap();
}
