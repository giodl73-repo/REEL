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

fn marker_offsets(frames: &[[f32; 2]]) -> Result<[usize; 4]> {
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
    Ok([*head_a, *tail_a, *head_b, *tail_b])
}

fn measure([head_a, tail_a, head_b, tail_b]: [usize; 4]) -> Result<(usize, [usize; 2])> {
    let gap = (head_b - tail_a)
        .checked_sub(SOURCE_FRAMES - LAST_MARKER + FIRST_MARKER)
        .ok_or_else(|| anyhow::anyhow!("capture has no measurable loop interval"))?;
    let spans = [tail_a - head_a, tail_b - head_b];
    Ok((gap, spans))
}

#[derive(serde::Deserialize)]
struct CaptureClock {
    sample_rate: usize,
    channels: usize,
    encoding: String,
    total_frames: usize,
    audio_sha256: String,
    audio_bytes: usize,
    request_sha256: Option<String>,
    packets: Vec<CapturePacket>,
}

#[derive(serde::Deserialize)]
struct ConsoleTrace {
    schema: String,
    clock: String,
    request_sha256: String,
    process_observed_exit_qpc_ns: u64,
    process_exited_successfully: bool,
    steps: Vec<ConsoleStep>,
}

#[derive(serde::Deserialize)]
struct ConsoleStep {
    command: String,
    sent_qpc_ns: Option<u64>,
}

#[derive(serde::Deserialize)]
struct CapturePacket {
    offset_frames: usize,
    frames: usize,
    capture_qpc_ns: u64,
    callback_qpc_ns: u64,
}

fn frame_ns(frames: usize) -> u64 {
    frames as u64 * 1_000_000_000 / RATE as u64
}

fn measure_capture_clock(
    clock: &CaptureClock,
    bytes: &[u8],
    markers: [usize; 4],
) -> Result<(u64, [u64; 2], u64)> {
    ensure!(
        clock.sample_rate == RATE
            && clock.channels == 2
            && clock.encoding == "f32le"
            && clock.total_frames == bytes.len() / 8
            && clock.audio_bytes == bytes.len()
            && clock.audio_sha256 == reel::production::sha256_bytes(bytes),
        "capture clock describes different audio"
    );
    let mut offset = 0;
    let mut previous: Option<&CapturePacket> = None;
    for packet in &clock.packets {
        ensure!(
            packet.offset_frames == offset
                && (1..=2048).contains(&packet.frames)
                && packet.capture_qpc_ns > 0
                && packet.callback_qpc_ns >= packet.capture_qpc_ns,
            "capture packet geometry or timestamp"
        );
        if let Some(previous) = previous {
            let elapsed = packet.capture_qpc_ns.checked_sub(previous.capture_qpc_ns);
            ensure!(
                elapsed.is_some_and(|time| time.abs_diff(frame_ns(previous.frames)) <= 2_000_000),
                "capture packet clock discontinuity"
            );
        }
        offset += packet.frames;
        previous = Some(packet);
    }
    ensure!(
        offset == clock.total_frames,
        "capture packet coverage differs"
    );
    let mut marker_clock = [0; 4];
    for (marker, time) in markers.into_iter().zip(&mut marker_clock) {
        let index = clock
            .packets
            .partition_point(|packet| packet.offset_frames <= marker);
        let packet = &clock.packets[index - 1];
        *time = packet
            .capture_qpc_ns
            .checked_add(frame_ns(marker - packet.offset_frames))
            .ok_or_else(|| anyhow::anyhow!("capture marker timestamp overflow"))?;
    }
    let [head_a, tail_a, head_b, tail_b] = marker_clock;
    let gap = head_b
        .checked_sub(tail_a)
        .and_then(|span| span.checked_sub(frame_ns(SOURCE_FRAMES - LAST_MARKER + FIRST_MARKER)))
        .ok_or_else(|| anyhow::anyhow!("capture clock has no measurable loop interval"))?;
    let spans = [
        tail_a
            .checked_sub(head_a)
            .ok_or_else(|| anyhow::anyhow!("first capture span reversed"))?,
        tail_b
            .checked_sub(head_b)
            .ok_or_else(|| anyhow::anyhow!("second capture span reversed"))?,
    ];
    Ok((gap, spans, head_a))
}

fn start_to_marker_bound(
    trace: &ConsoleTrace,
    clock: &CaptureClock,
    first_marker_ns: u64,
) -> Result<u64> {
    ensure!(
        trace.schema == "reel.installation-station-console-trace.v1"
            && trace.clock == "windows-qpc-nanoseconds"
            && trace.process_exited_successfully
            && clock.request_sha256.as_deref() == Some(trace.request_sha256.as_str())
            && trace.process_observed_exit_qpc_ns > first_marker_ns,
        "console trace and capture session differ"
    );
    let mut starts = trace.steps.iter().filter(|step| step.command == "start\r");
    let sent = starts
        .next()
        .and_then(|step| step.sent_qpc_ns)
        .ok_or_else(|| anyhow::anyhow!("console trace lacks Start time"))?;
    ensure!(
        sent > 0 && starts.next().is_none(),
        "ambiguous Start observation"
    );
    first_marker_ns
        .checked_sub(sent)
        .ok_or_else(|| anyhow::anyhow!("capture marker precedes Start"))
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
    let offsets = marker_offsets(&frames).unwrap();
    let (gap, spans) = measure(offsets).unwrap();
    assert_eq!((gap, spans), (12_000, [133_680; 2]));
    assert!(capture_passes(gap, spans));
    assert!(!capture_passes(gap + 2_401, spans));
    assert!(!capture_passes(gap, [spans[0], spans[1] - 961]));
    frames[LAST_MARKER..LAST_MARKER + MARKER_FRAMES].fill([0.0; 2]);
    assert!(marker_offsets(&frames).is_err());
    let bytes = vec![0; frames.len() * 8];
    let mut clock = CaptureClock {
        sample_rate: RATE,
        channels: 2,
        encoding: "f32le".into(),
        total_frames: frames.len(),
        audio_sha256: reel::production::sha256_bytes(&bytes),
        audio_bytes: bytes.len(),
        request_sha256: Some("fixture-request".into()),
        packets: (0..frames.len())
            .step_by(480)
            .map(|offset_frames| CapturePacket {
                offset_frames,
                frames: (frames.len() - offset_frames).min(480),
                capture_qpc_ns: 1 + frame_ns(offset_frames),
                callback_qpc_ns: 2 + frame_ns(offset_frames),
            })
            .collect(),
    };
    let (gap_ns, spans_ns, first_marker_ns) =
        measure_capture_clock(&clock, &bytes, offsets).unwrap();
    assert_eq!((gap_ns, spans_ns), (250_000_000, [2_785_000_000; 2]));
    let mut trace = ConsoleTrace {
        schema: "reel.installation-station-console-trace.v1".into(),
        clock: "windows-qpc-nanoseconds".into(),
        request_sha256: "fixture-request".into(),
        process_observed_exit_qpc_ns: first_marker_ns + 6_000_000_000,
        process_exited_successfully: true,
        steps: vec![ConsoleStep {
            command: "start\r".into(),
            sent_qpc_ns: Some(first_marker_ns - 100_000_000),
        }],
    };
    assert_eq!(
        start_to_marker_bound(&trace, &clock, first_marker_ns).unwrap(),
        100_000_000
    );
    trace.request_sha256 = "other-request".into();
    assert!(start_to_marker_bound(&trace, &clock, first_marker_ns).is_err());
    clock.packets[10].capture_qpc_ns += 3_000_000;
    clock.packets[10].callback_qpc_ns += 3_000_000;
    assert!(measure_capture_clock(&clock, &bytes, offsets).is_err());
}

#[test]
#[ignore = "explicit analysis of retained digital fixture capture"]
fn measure_retained_station_capture() {
    let path = std::env::var_os("REEL_INSTALLATION_CAPTURE_F32LE")
        .expect("provide the retained stereo 48 kHz f32le capture path");
    const MAX_BYTES: usize = 120 * RATE * 8;
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
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
    let markers = marker_offsets(&frames).unwrap();
    let (gap, spans) = measure(markers).unwrap();
    println!(
        "capture SHA-256 {}; loop gap {} frames; source spans {:?} frames",
        reel::production::sha256_bytes(&bytes),
        gap,
        spans
    );
    assert!(capture_passes(gap, spans), "capture failed timing gates");
    let clock_path = std::path::Path::new(&path).with_extension("clock.json");
    let trace_path = std::env::var_os("REEL_INSTALLATION_CONSOLE_TRACE");
    if clock_path.is_file() {
        let mut raw = Vec::new();
        std::fs::File::open(clock_path)
            .unwrap()
            .take(2_000_001)
            .read_to_end(&mut raw)
            .unwrap();
        assert!(raw.len() <= 2_000_000, "capture clock sidecar too large");
        let clock: CaptureClock = serde_json::from_slice(&raw).unwrap();
        let (gap_ns, spans_ns, first_marker_ns) =
            measure_capture_clock(&clock, &bytes, markers).unwrap();
        println!("capture QPC loop gap {gap_ns} ns; source spans {spans_ns:?} ns");
        assert!(gap_ns.abs_diff(250_000_000) <= 50_000_000);
        assert!(
            spans_ns
                .into_iter()
                .all(|span| span.abs_diff(frame_ns(LAST_MARKER - FIRST_MARKER)) <= 20_000_000)
        );
        if let Some(trace_path) = trace_path {
            let mut raw = Vec::new();
            std::fs::File::open(trace_path)
                .unwrap()
                .take(65_537)
                .read_to_end(&mut raw)
                .unwrap();
            assert!(raw.len() <= 65_536, "console trace too large");
            let trace: ConsoleTrace = serde_json::from_slice(&raw).unwrap();
            let onset_bound = start_to_marker_bound(&trace, &clock, first_marker_ns).unwrap();
            println!("Start sent to first captured source marker: {onset_bound} ns");
            assert!(onset_bound <= 2_000_000_000);
        }
    } else {
        assert!(
            trace_path.is_none(),
            "console alignment requires a capture clock sidecar"
        );
        println!("sample positions only; no capture clock sidecar");
    }
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
    let request_bytes = std::fs::read(request_path).unwrap();
    let request =
        reel::spoken_programme::installation::native::SessionRequest::parse(&request_bytes)
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
            "request_sha256": reel::production::sha256_bytes(&request_bytes),
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
