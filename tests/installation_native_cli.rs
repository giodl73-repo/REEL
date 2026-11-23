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
#[ignore = "explicit station console timing; requires REEL_INSTALLATION_STATION_SESSION"]
fn actual_station_status_acknowledgement_meets_outer_deadline() {
    let (path, request) = station_request();
    let (result, steps, elapsed) = run_console(
        &request.playback.runtime.path,
        &path,
        &[(b"Ready p", b"status\r"), (b"Command 1: Ready", b"quit\r")],
        45,
    );
    assert_eq!(steps, 2, "{result:?}");
    result.unwrap();
    let status = elapsed[1].expect("Status timing missing");
    println!("Status sent to console acknowledgement: {status:?}");
    assert!(status <= std::time::Duration::from_millis(100));
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
fn run_station(sequence: &[(&[u8], &[u8])], command_budget_seconds: u64) {
    let (path, request) = station_request();
    let (result, steps, _) = run_console(
        &request.playback.runtime.path,
        &path,
        sequence,
        command_budget_seconds,
    );
    assert_eq!(
        steps,
        sequence.len(),
        "station run stopped before the full control sequence: {result:?}"
    );
    result.unwrap();
}

#[cfg(windows)]
fn run_console(
    program: &std::path::Path,
    request: &std::path::Path,
    steps: &[(&[u8], &[u8])],
    command_budget_seconds: u64,
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
    let observed = Mutex::new(Vec::with_capacity(steps.len()));
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
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
                                observed.lock().unwrap().push(Instant::now());
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
            let mut last_command = None;
            for (index, (_, line)) in steps.iter().enumerate() {
                let mut seen = observed.lock().unwrap().get(index).copied();
                while seen.is_none() && !job.is_finished() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(2));
                    seen = observed.lock().unwrap().get(index).copied();
                }
                let Some(seen) = seen else {
                    break;
                };
                let duration = last_command.map(|sent| seen.duration_since(sent));
                if !line.is_empty() {
                    last_command = Some(Instant::now());
                    if input.write_all(line).is_err() {
                        break;
                    }
                }
                elapsed.push(duration);
            }
            (job.join().unwrap(), elapsed.len(), elapsed)
        });
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
