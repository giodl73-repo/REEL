//! Windows CLI coverage. Hardware playback requires an explicit station request.
#![cfg(feature = "installation-native")]
#[path = "support/installation_capture.rs"]
mod capture;

use serde_json::json;
use std::{fs, process::Command};
fn tool() -> Command {
    Command::new(env!("CARGO_BIN_EXE_reel-installation"))
}
fn binding() -> serde_json::Value {
    json!({"profile":"reel.installation.win11-wasapi-pcm24.v1","device_id":"wasapi:{0.0.0.00000000}.{11111111-1111-1111-1111-111111111111}",
    "os_build":26300,"driver_version":"10.0.test-only","configuration_sha256":"a".repeat(64)})
}
fn session_request(root: &std::path::Path) -> serde_json::Value {
    let file =
        json!({"path":root.join("missing-source-control"),"sha256":"a".repeat(64),"bytes":1});
    json!({"schema":"reel.installation-session.v1","edition_id":"es-technical","passes":2,
        "original":{"declaration":file,"plan":file,"receipt":file,"processing":null,"access_manifest":file,
            "compiler_producer":file,"source_root":root.join("unopened-sources"),
            "candidate_root":root.join("unopened-candidate"),"access_root":root.join("unopened-access")},
        "playback":{"runtime":file,"source_commit":"b".repeat(40),"cargo_lock":file,"cpal_version":"0.18.2"},
        "endpoint":binding()})
}

#[cfg(windows)]
#[test]
fn actual_run_keeps_status_and_quit_available_after_refusing_start_in_fault() {
    let root = tempfile::tempdir().unwrap();
    let program = std::path::Path::new(env!("CARGO_BIN_EXE_reel-installation"));
    let lock = include_bytes!("../Cargo.lock");
    let lock_path = root.path().join("Cargo.lock");
    fs::write(&lock_path, lock).unwrap();
    let image = fs::read(program).unwrap();
    let mut request = session_request(root.path());
    request["playback"]["runtime"] = json!({
        "path":program, "sha256":reel::production::sha256_bytes(&image), "bytes":image.len()
    });
    request["playback"]["cargo_lock"] = json!({
        "path":lock_path, "sha256":reel::production::sha256_bytes(lock), "bytes":lock.len()
    });
    drop(image);
    let path = root.path().join("session.json");
    fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
    let (result, steps, _) = run_console(
        program,
        &path,
        &[
            (b"Fault", b"start\r"),
            (b"Command unavailable", b"status\r"),
            (b"1: Fault", b"quit\r"),
        ],
        10,
        None,
        &[],
        false,
    );
    let error = result.expect_err("a closed source fault must remain visible in the exit status");
    assert!(
        error.starts_with("Job step exited 2;"),
        "after {steps} steps: {error}"
    );
    assert_eq!(steps, 3, "expected Fault, refusal and Status before Quit");
}

#[cfg(windows)]
#[test]
#[ignore = "explicit station playback; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_run_exercises_playback_commands_and_reaps_worker() {
    run_station(
        &[
            (b"Ready p", b"start\r"),
            (b"Playing p1", b"pause\r"),
            (b"Paused p1", b"resume\r"),
            (b"LoopWait p1", b""),
            (b"Completed p2", b"restart\r"),
            (b"Restarting p", b""),
            (b"Playing p1", b"stop\r"),
            (b"Stopped p1", b"quit\r"),
        ],
        70,
    );
}

#[cfg(windows)]
#[test]
#[ignore = "uninterrupted station playback; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_run_plays_uninterrupted_marker_passes() {
    run_station(
        &[
            (b"Ready p", b"start\r"),
            (b"Playing p1", b""),
            (b"LoopWait p1", b""),
            (b"Playing p2", b""),
            (b"Completed p2", b"quit\r"),
        ],
        70,
    );
}

#[cfg(windows)]
#[test]
#[ignore = "two-minute station lifetime test; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_expiry_closes_paused_playback_without_quit() {
    let elapsed = std::time::Instant::now();
    run_station(
        &[
            (b"Ready p", b"start\r"),
            (b"Playing p1", b"pause\r"),
            (b"Paused p1", b""),
            (b"Stopped p1", b""),
        ],
        125,
    );
    assert!(elapsed.elapsed() >= std::time::Duration::from_secs(119));
}

#[cfg(windows)]
#[test]
#[ignore = "explicit station interruption; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_closes_live_playback_on_interrupt_and_eof() {
    for (label, input) in [("Ctrl+C", b"\x03".as_slice()), ("EOF", b"\x1a\r")] {
        let timings = run_station(&[(b"Ready p", b"start\r"), (b"Playing p1", input)], 45);
        let exit = timings
            .last()
            .copied()
            .flatten()
            .expect("exit timing missing");
        println!("{label} sent to confirmed process exit: {exit:?}");
        assert!(exit <= std::time::Duration::from_secs(2));
    }
}

#[cfg(windows)]
#[test]
#[ignore = "explicit station terminal backpressure; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_exits_while_terminal_output_is_undrained() {
    let (path, request) = station_request();
    let (result, steps, elapsed) = run_console(
        &request.playback.runtime.path,
        &path,
        &[(b"Ready p", b"start\r"), (b"Playing p1", b"\x1a\r")],
        45,
        None,
        &[],
        true,
    );
    assert_eq!(steps, 2, "station did not reach live playback: {result:?}");
    result.unwrap();
    let exit = elapsed
        .last()
        .copied()
        .flatten()
        .expect("missing exit time");
    println!("EOF to confirmed process exit with output undrained: {exit:?}");
    assert!(exit <= std::time::Duration::from_secs(2));
}

#[cfg(windows)]
#[test]
#[ignore = "explicit station metadata mismatch; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_refuses_changed_endpoint_configuration() {
    let (_, mut request) = station_request();
    let old = &request.endpoint.configuration_sha256;
    let first = if old.starts_with('0') { "1" } else { "0" };
    request.endpoint.configuration_sha256 = format!("{first}{}", &old[1..]);
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("different-configuration.json");
    fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
    let (result, steps, _) = run_console(
        &request.playback.runtime.path,
        &path,
        &[(b"Fault", b"quit\r")],
        45,
        None,
        &[],
        false,
    );
    let error = result.expect_err("a changed endpoint configuration must fault");
    assert!(
        error.starts_with("Job step exited 2;"),
        "after {steps} steps: {error}"
    );
    assert_eq!(steps, 1);
}

#[cfg(windows)]
#[test]
#[ignore = "requires a physically absent enrolled endpoint in REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_reports_missing_endpoint_before_start() {
    let (path, request) = station_request();
    let (result, steps, elapsed) = run_console(
        &request.playback.runtime.path,
        &path,
        &[(b"Fault", b"quit\r")],
        10,
        None,
        &[],
        false,
    );
    assert_eq!(
        steps, 1,
        "missing endpoint fault was not visible: {result:?}"
    );
    let error = result.expect_err("a missing endpoint must remain visible in the exit status");
    assert!(error.starts_with("Job step exited 2;"), "{error}");
    let exit = elapsed
        .last()
        .copied()
        .flatten()
        .expect("missing exit time");
    println!("Quit to confirmed fault exit: {exit:?}");
    assert!(exit <= std::time::Duration::from_secs(2));
}

#[cfg(windows)]
#[test]
#[ignore = "explicit station console timing; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_status_acknowledgement_meets_outer_deadline() {
    let elapsed = run_station(
        &[(b"Ready p", b"status\r"), (b"Command 1: Ready", b"quit\r")],
        45,
    );
    assert_acknowledgement_time("Status", elapsed[1]);
}

#[cfg(windows)]
#[test]
#[ignore = "explicit live command timing; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_pause_and_resume_acknowledgements_meet_outer_deadline() {
    let elapsed = run_station(
        &[
            (b"Ready p", b"start\r"),
            (b"Playing p1", b"pause\r"),
            (b"Command 2: Paused", b"resume\r"),
            (b"Command 3: Playing", b"quit\r"),
        ],
        45,
    );
    assert_acknowledgement_time("Pause", elapsed[2]);
    assert_acknowledgement_time("Resume", elapsed[3]);
}

#[cfg(windows)]
#[test]
#[ignore = "explicit live command timing; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_start_and_stop_acknowledgements_meet_outer_deadline() {
    let elapsed = run_station(
        &[
            (b"Ready p", b"start\r"),
            (b"Command 1: Starting", b""),
            (b"Playing p1", b"stop\r"),
            (b"Command 2: Stopped", b"quit\r"),
        ],
        45,
    );
    assert_acknowledgement_time("Start", elapsed[1]);
    assert_playing_time("Start", elapsed[2], 2);
    assert_acknowledgement_time("Stop", elapsed[3]);
}

#[cfg(windows)]
#[test]
#[ignore = "dedicated Stop capture stimulus; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_stop_leaves_a_quiescence_observation_window() {
    station_quiescence_window(b"stop\r", b"Command 2: Stopped", "Stop");
}

#[cfg(windows)]
#[test]
#[ignore = "dedicated Pause capture stimulus; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_pause_leaves_a_quiescence_observation_window() {
    station_quiescence_window(b"pause\r", b"Command 2: Paused", "Pause");
}

#[cfg(windows)]
fn station_quiescence_window(command: &[u8], acknowledgement: &[u8], label: &str) {
    use std::time::Duration;
    let elapsed = run_station_with_delays(
        &[
            (b"Ready p", b"start\r"),
            (b"Playing p1", command),
            (acknowledgement, b"quit\r"),
        ],
        45,
        &[
            Duration::ZERO,
            Duration::from_millis(300),
            Duration::from_millis(350),
        ],
    );
    assert_acknowledgement_time(label, elapsed[2]);
}

#[cfg(windows)]
#[test]
#[ignore = "explicit native restart timing; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_restart_acknowledges_and_returns_to_pass_one() {
    let elapsed = run_station(
        &[
            (b"Ready p", b"start\r"),
            (b"Playing p1", b"stop\r"),
            (b"Stopped p1", b"restart\r"),
            (b"Command 3: Restarting", b""),
            (b"Playing p1", b"quit\r"),
        ],
        50,
    );
    assert_acknowledgement_time("Restart", elapsed[3]);
    assert_playing_time("Restart", elapsed[4], 35);
}

#[cfg(windows)]
#[test]
#[ignore = "dedicated Restart capture stimulus; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_restart_leaves_a_fresh_onset_observation_window() {
    use std::time::Duration;
    let elapsed = run_station_with_delays(
        &[
            (b"Ready p", b"start\r"),
            (b"Playing p1", b"stop\r"),
            (b"Command 2: Stopped", b"restart\r"),
            (b"Command 3: Restarting", b""),
            (b"Playing p1", b"quit\r"),
        ],
        45,
        &[
            Duration::ZERO,
            Duration::from_millis(300),
            Duration::from_millis(350),
            Duration::ZERO,
            Duration::from_millis(350),
        ],
    );
    assert_acknowledgement_time("Restart", elapsed[3]);
    assert_playing_time("Restart", elapsed[4], 35);
}

#[cfg(windows)]
#[test]
#[ignore = "explicit native quit timing; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_quit_acknowledges_and_closes_playback() {
    let elapsed = run_station(
        &[
            (b"Ready p", b"start\r"),
            (b"Playing p1", b"quit\r"),
            (b"Command 2: Stopped", b""),
        ],
        45,
    );
    assert_acknowledgement_time("Quit", elapsed[2]);
    let exit = elapsed
        .last()
        .copied()
        .flatten()
        .expect("exit timing missing");
    println!("Quit sent to confirmed process exit: {exit:?}");
    assert!(exit <= std::time::Duration::from_secs(2));
}

#[cfg(windows)]
fn assert_playing_time(command: &str, duration: Option<std::time::Duration>, limit_seconds: u64) {
    let duration = duration.expect("playback observation missing");
    println!("{command} sent to visible Playing: {duration:?}");
    assert!(duration <= std::time::Duration::from_secs(limit_seconds));
}

#[cfg(windows)]
fn assert_acknowledgement_time(command: &str, duration: Option<std::time::Duration>) {
    let duration = duration.expect("command timing missing");
    println!("{command} sent to console acknowledgement: {duration:?}");
    assert!(duration <= std::time::Duration::from_millis(100));
}

#[cfg(windows)]
fn station_request() -> (
    std::path::PathBuf,
    reel::spoken_programme::installation::native::SessionRequest,
) {
    use reel::spoken_programme::installation::native::SessionRequest;
    let path = std::path::PathBuf::from(
        std::env::var_os("REEL_INSTALLATION_STATION_SESSION")
            .expect("provide the enrolled station request path"),
    );
    assert!(path.is_absolute());
    let request = SessionRequest::parse(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        (request.edition_id.as_str(), request.passes),
        ("en-technical", 2)
    );
    (path, request)
}

#[cfg(windows)]
#[test]
#[ignore = "explicit native buffer observation; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_observes_native_period_and_silent_callbacks() {
    run_native_buffer_fixture("silent_native_buffer_fixture");
}

#[cfg(windows)]
#[test]
#[ignore = "explicit native playback observation; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_observes_callbacks_through_two_native_passes() {
    run_native_buffer_fixture("playing_native_buffer_fixture");
}

#[cfg(windows)]
fn run_native_buffer_fixture(child_test: &str) {
    use reel_assets::jobs::{QuietLimits, run_quiet};
    use std::time::Duration;
    let (path, _) = station_request();
    let root = tempfile::tempdir().unwrap();
    fs::copy(path, root.path().join("session.json")).unwrap();
    let result = run_quiet(
        &std::env::current_exe().unwrap(),
        &["--ignored".into(), "--exact".into(), child_test.into()],
        root.path(),
        Duration::from_secs(35),
        QuietLimits {
            memory_bytes: 512 * 1024 * 1024,
            processes: 1,
        },
        || Ok(()),
    );
    if let Err(error) = result {
        let detail = fs::read_to_string(root.path().join("buffer-error.txt")).unwrap_or_default();
        panic!("{error}: {detail}");
    }
    println!(
        "{}",
        fs::read_to_string(root.path().join("buffer-observation.json")).unwrap()
    );
}

#[cfg(windows)]
#[test]
#[ignore = "contained child for the explicit silent native buffer test"]
fn silent_native_buffer_fixture() {
    native_buffer_fixture(false);
}

#[cfg(windows)]
#[test]
#[ignore = "contained child for the explicit native playback observation"]
fn playing_native_buffer_fixture() {
    native_buffer_fixture(true);
}

#[cfg(windows)]
fn native_buffer_fixture(play: bool) {
    use reel::spoken_programme::installation::native::{
        SessionRequest, callback_pair,
        output::{OutputClock, OutputEvent, OutputWorker},
        source::{SourceEvent, SourceWorker},
    };
    use reel::spoken_programme::installation::{Notice, State};
    use std::time::Duration;
    std::panic::set_hook(Box::new(|failure| {
        let _ = fs::write("buffer-error.txt", failure.to_string());
    }));
    let request = SessionRequest::parse(&fs::read("session.json").unwrap()).unwrap();
    let clock = OutputClock::start().unwrap();
    let mut output = OutputWorker::new(request.endpoint.clone(), clock).unwrap();
    let mut source = play.then(|| SourceWorker::new(request, clock, 20_000_000).unwrap());
    let core = source.as_mut().map(|source| {
        loop {
            match source.event() {
                Some(SourceEvent::Ready(core)) => break core,
                Some(_) => panic!("checked station source did not reach Ready"),
                None => {}
            }
            assert!(
                clock.now_us().unwrap() < 20_000_000,
                "station source deadline"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    });
    let open_deadline = clock.now_us().unwrap() + 2_000_000;
    output.open(1, open_deadline).unwrap();
    let (period, mut sink) = loop {
        match output.event() {
            Some(OutputEvent::Opened {
                generation: 1,
                reported_period_frames,
                sink,
            }) => {
                break (reported_period_frames, sink);
            }
            Some(_) => panic!("native stream did not open"),
            None => {}
        }
        assert!(
            clock.now_us().unwrap() < open_deadline,
            "native open deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    };
    while sink.stats().callbacks < 5 {
        assert!(
            !sink.failed() && clock.now_us().unwrap() < open_deadline,
            "silent callback deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(sink.ready());
    if let Some(mut core) = core {
        let source_frames = core.source_frames();
        assert_eq!(source_frames, 144_000);
        core.enqueue("start").unwrap();
        assert_eq!(
            core.poll(clock.now_us().unwrap() / 1000, Notice::None).open,
            Some(1)
        );
        let (mut control, callback) = callback_pair(core);
        assert!(sink.attach(callback).is_ok(), "native callback handoff");
        let playing_deadline = clock.now_us().unwrap() + 10_000_000;
        loop {
            if let Some(report) = control.report() {
                assert_ne!(report.snapshot.state, State::Fault);
                if report.snapshot.state == State::Completed {
                    assert_eq!(report.snapshot.pass, 2);
                    break;
                }
                continue;
            }
            assert!(
                !sink.failed() && clock.now_us().unwrap() < playing_deadline,
                "native playback deadline"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(sink.stats().submitted_source_frames, 2 * source_frames);
    }
    let stats = sink.stats();
    assert!((1..=2048).contains(&period));
    assert!((1..=2048).contains(&stats.largest_callback_frames));
    if !play {
        assert_eq!(
            (stats.owned_callbacks, stats.submitted_source_frames),
            (0, 0)
        );
    }
    let close_deadline = clock.now_us().unwrap() + 2_000_000;
    output.close(1, close_deadline).unwrap();
    loop {
        match output.event() {
            Some(OutputEvent::Closed { generation: 1 }) => break,
            Some(_) => panic!("native stream did not close"),
            None => {}
        }
        assert!(
            clock.now_us().unwrap() < close_deadline,
            "native close deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    output.shutdown().unwrap();
    fs::write(
        "buffer-observation.json",
        serde_json::to_vec(&json!({
            "requested_frames": 480,
            "reported_period_frames": period,
            "largest_observed_callback_frames": stats.largest_callback_frames,
            "callbacks": stats.callbacks,
            "source_owned_callbacks": stats.owned_callbacks,
            "submitted_source_frames": stats.submitted_source_frames,
        }))
        .unwrap(),
    )
    .unwrap();
}

#[cfg(windows)]
fn run_station(
    sequence: &[(&[u8], &[u8])],
    command_budget_seconds: u64,
) -> Vec<Option<std::time::Duration>> {
    run_station_with_delays(sequence, command_budget_seconds, &[])
}

#[cfg(windows)]
fn run_station_with_delays(
    sequence: &[(&[u8], &[u8])],
    command_budget_seconds: u64,
    command_delays: &[std::time::Duration],
) -> Vec<Option<std::time::Duration>> {
    let (path, request) = station_request();
    let trace = std::env::var_os("REEL_INSTALLATION_CONSOLE_TRACE").map(std::path::PathBuf::from);
    let (result, steps, elapsed) = run_console(
        &request.playback.runtime.path,
        &path,
        sequence,
        command_budget_seconds,
        trace.as_deref(),
        command_delays,
        false,
    );
    assert_eq!(
        steps,
        sequence.len(),
        "station run stopped before the full control sequence: {result:?}"
    );
    result.unwrap();
    elapsed
}

#[cfg(windows)]
fn run_console(
    program: &std::path::Path,
    request: &std::path::Path,
    steps: &[(&[u8], &[u8])],
    command_budget_seconds: u64,
    trace_path: Option<&std::path::Path>,
    command_delays: &[std::time::Duration],
    hold_output_after_final_marker: bool,
) -> (Result<(), String>, usize, Vec<Option<std::time::Duration>>) {
    use reel_assets::jobs::{BorrowedPseudoConsole, QuietLimits, run_pseudoconsole_observed};
    use std::{
        fs::File,
        io::{Read, Write},
        os::windows::io::{AsRawHandle, FromRawHandle},
        sync::Mutex,
        time::{Duration, Instant},
    };
    use windows::Win32::{
        Foundation::HANDLE,
        System::{
            Console::{COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON},
            Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
            Pipes::CreatePipe,
        },
    };
    struct Terminal(HPCON);
    impl Drop for Terminal {
        fn drop(&mut self) {
            unsafe { ClosePseudoConsole(self.0) };
        }
    }
    fn pipe() -> (File, File) {
        let (mut read, mut write) = (HANDLE::default(), HANDLE::default());
        unsafe {
            CreatePipe(&mut read, &mut write, None, 0).unwrap();
            (
                File::from_raw_handle(read.0),
                File::from_raw_handle(write.0),
            )
        }
    }

    let (in_read, mut input) = pipe();
    let (mut output, out_write) = pipe();
    let terminal = Terminal(unsafe {
        CreatePseudoConsole(
            COORD { X: 80, Y: 30 },
            HANDLE(in_read.as_raw_handle()),
            HANDLE(out_write.as_raw_handle()),
            0,
        )
        .unwrap()
    });
    drop(in_read);
    drop(out_write);
    let capture_clock = trace_path.map(|path| {
        assert!(path.is_absolute() && !path.exists());
        let mut frequency = 0;
        unsafe { QueryPerformanceFrequency(&mut frequency).unwrap() };
        assert!(frequency > 0);
        frequency as u128
    });
    let qpc_ns = || {
        capture_clock.map(|frequency| {
            let mut ticks = 0;
            unsafe { QueryPerformanceCounter(&mut ticks).unwrap() };
            assert!(ticks > 0);
            u64::try_from(ticks as u128 * 1_000_000_000 / frequency).unwrap()
        })
    };
    let observed = Mutex::new(Vec::with_capacity(steps.len()));
    std::thread::scope(|scope| {
        let (release_output, held_output) = std::sync::mpsc::channel::<()>();
        let observed_markers = &observed;
        let reader = scope.spawn(move || {
            let mut chunk = [0; 4096];
            let mut next = 0;
            let mut matched = 0;
            while let Ok(count) = output.read(&mut chunk) {
                if count == 0 {
                    break;
                }
                for &byte in &chunk[..count] {
                    if let Some((marker, _)) = steps.get(next) {
                        if byte == marker[matched] {
                            matched += 1;
                            if matched == marker.len() {
                                next += 1;
                                observed_markers
                                    .lock()
                                    .unwrap()
                                    .push((Instant::now(), qpc_ns()));
                                if hold_output_after_final_marker && next == steps.len() {
                                    // Stop reading until the parent observes child exit.
                                    let _ = held_output.recv();
                                }
                                matched = 0;
                            }
                        } else {
                            matched = usize::from(byte == marker[0]);
                        }
                    }
                }
            }
        });
        let borrowed = unsafe { BorrowedPseudoConsole::from_raw(terminal.0.0).unwrap() };
        let result = std::thread::scope(|job_scope| {
            let job = job_scope.spawn(|| {
                run_pseudoconsole_observed(
                    program,
                    &["run".into(), request.to_str().unwrap().into()],
                    request.parent().unwrap(),
                    Duration::from_secs(command_budget_seconds + 5),
                    QuietLimits {
                        memory_bytes: 512 * 1024 * 1024,
                        processes: 2,
                    },
                    borrowed,
                    |_| Ok(()),
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
            });
            let deadline = Instant::now() + Duration::from_secs(command_budget_seconds);
            let mut elapsed = Vec::with_capacity(steps.len());
            let mut trace = Vec::with_capacity(steps.len());
            let mut last_command = None;
            for (index, (marker, line)) in steps.iter().enumerate() {
                let mut seen = observed.lock().unwrap().get(index).copied();
                while seen.is_none() && !job.is_finished() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(2));
                    seen = observed.lock().unwrap().get(index).copied();
                }
                let Some((seen, observed_ns)) = seen else {
                    break;
                };
                let duration = last_command.map(|sent| seen.duration_since(sent));
                let mut sent_ns = None;
                if !line.is_empty() {
                    if let Some(delay) = command_delays.get(index) {
                        std::thread::sleep(*delay);
                    }
                    last_command = Some(Instant::now());
                    sent_ns = qpc_ns();
                    if input.write_all(line).is_err() {
                        break;
                    }
                }
                elapsed.push(duration);
                if capture_clock.is_some() {
                    trace.push(json!({
                        "marker": std::str::from_utf8(marker).unwrap(),
                        "observed_qpc_ns": observed_ns,
                        "command": std::str::from_utf8(line).unwrap(),
                        "sent_qpc_ns": sent_ns,
                    }));
                }
            }
            let result = job.join().unwrap();
            let steps = elapsed.len();
            // Final entry bounds the time from the last write to observed
            // process cleanup. The step count covers console markers only.
            elapsed.push(last_command.map(|sent| sent.elapsed()));
            if let Some(path) = trace_path {
                let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap()).unwrap();
                let evidence = json!({
                    "schema": "reel.installation-station-console-trace.v1",
                    "clock": "windows-qpc-nanoseconds",
                    "request_sha256": reel::production::sha256_bytes(&fs::read(request).unwrap()),
                    "process_observed_exit_qpc_ns": qpc_ns(),
                    "process_exited_successfully": result.is_ok(),
                    "steps": trace,
                });
                serde_json::to_writer_pretty(&mut file, &evidence).unwrap();
                file.as_file().sync_all().unwrap();
                file.persist_noclobber(path).unwrap();
            }
            (result, steps, elapsed)
        });
        drop(release_output);
        drop(input);
        drop(terminal);
        reader.join().unwrap();
        result
    })
}
#[test]
fn private_session_shape_checks_no_originals_or_endpoints_and_rejects_forged_status() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.json");
    let mut request = session_request(root.path());
    fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
    let output = tool().arg("validate-session").arg(&path).output().unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["session_shape_checked"], true);
    for field in [
        "originals_checked",
        "endpoint_probed",
        "device_playback_checked",
    ] {
        assert_eq!(report[field], false);
    }
    let text = String::from_utf8(output.stdout).unwrap();
    for private in ["es-technical", "11111111", "missing-source", "unopened-"] {
        assert!(!text.contains(private));
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    request["authority_verified"] = json!(true);
    fs::write(&path, serde_json::to_vec(&request).unwrap()).unwrap();
    assert!(
        !tool()
            .arg("validate-session")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    fs::write(&path, vec![b' '; 16 * 1024 + 1]).unwrap();
    assert!(
        !tool()
            .arg("validate-session")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}
#[test]
fn missing_originals_produce_no_source_success_or_private_error_output() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.json");
    fs::write(
        &path,
        serde_json::to_vec(&session_request(root.path())).unwrap(),
    )
    .unwrap();
    let output = tool().arg("check-session").arg(&path).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("no playback qualification"));
    assert!(!error.contains("missing-source") && !error.contains("11111111"));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}
#[test]
fn binding_shape_validation_is_private_and_needs_no_device() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("binding.json");
    fs::write(&path, serde_json::to_vec(&binding()).unwrap()).unwrap();
    let output = tool().arg("validate-binding").arg(&path).output().unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["binding_shape_checked"], true);
    assert_eq!(report["endpoint_probed"], false);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("11111111") && !text.contains("test-only"));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    for id in ["", "default", "wasapi:default", "alsa:hw:0,0"] {
        let mut value = binding();
        value["device_id"] = json!(id);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            !tool()
                .arg("validate-binding")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
    }
}
#[test]
fn malformed_binding_and_existing_private_output_refuse_without_probe() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("binding.json");
    let mut value = binding();
    value["device_playback_checked"] = json!(true);
    fs::write(&input, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        !tool()
            .arg("validate-binding")
            .arg(&input)
            .status()
            .unwrap()
            .success()
    );
    fs::write(&input, vec![b' '; 4097]).unwrap();
    assert!(
        !tool()
            .arg("validate-binding")
            .arg(&input)
            .status()
            .unwrap()
            .success()
    );
    let output = root.path().join("keep.json");
    fs::write(&output, b"keep original").unwrap();
    assert!(
        !tool()
            .arg("probe")
            .arg(root.path().join("does-not-exist.json"))
            .arg("--output")
            .arg(&output)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(fs::read(output).unwrap(), b"keep original");
}
#[test]
fn wrong_runtime_pin_refuses_before_native_query_and_creates_no_final_output() {
    let root = tempfile::tempdir().unwrap();
    let request = root.path().join("probe.json");
    let output = root.path().join("new.json");
    let value = json!({"schema":"reel.installation-endpoint-probe.v1","endpoint_id":binding()["device_id"],
        "runtime":{"path":env!("CARGO_BIN_EXE_reel-installation"),"sha256":"b".repeat(64),"bytes":1}});
    fs::write(&request, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        !tool()
            .arg("probe")
            .arg(&request)
            .arg("--output")
            .arg(&output)
            .status()
            .unwrap()
            .success()
    );
    assert!(!output.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}
