//! Metadata only. No real endpoint ID or playback/capture enrollment.
#![cfg(feature = "installation-native")]
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
    use reel_assets::jobs::{BorrowedPseudoConsole, QuietLimits, run_pseudoconsole_observed};
    use std::{
        fs::File,
        io::{Read, Write},
        os::windows::io::{AsRawHandle, FromRawHandle},
        sync::atomic::{AtomicBool, Ordering},
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
    fs::write(
        root.path().join("session.json"),
        serde_json::to_vec(&request).unwrap(),
    )
    .unwrap();

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
    let markers: [&[u8]; 3] = [b"Fault", b"Command unavailable", b"1: Fault"];
    let observed = markers.map(|_| AtomicBool::new(false));
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
            let mut chunk = [0; 4096];
            let mut matched = [0; 3];
            while let Ok(count) = output.read(&mut chunk) {
                if count == 0 {
                    break;
                }
                for &byte in &chunk[..count] {
                    for (index, marker) in markers.iter().enumerate() {
                        let offset = &mut matched[index];
                        if byte == marker[*offset] {
                            *offset += 1;
                            if *offset == marker.len() {
                                observed[index].store(true, Ordering::Release);
                                *offset = 0;
                            }
                        } else {
                            *offset = usize::from(byte == marker[0]);
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
                    &["run".into(), "session.json".into()],
                    root.path(),
                    Duration::from_secs(15),
                    QuietLimits {
                        memory_bytes: 512 * 1024 * 1024,
                        processes: 2,
                    },
                    borrowed,
                    |_| Ok(()),
                )
                .map_err(|error| error.to_string())
            });
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut sent = 0;
            for (observed, line) in
                observed
                    .iter()
                    .zip([b"start\r".as_slice(), b"status\r", b"quit\r"])
            {
                while !observed.load(Ordering::Acquire)
                    && !job.is_finished()
                    && Instant::now() < deadline
                {
                    std::thread::sleep(Duration::from_millis(2));
                }
                if !observed.load(Ordering::Acquire) || input.write_all(line).is_err() {
                    break;
                }
                sent += 1;
            }
            (job.join().unwrap(), sent)
        });
        drop(input);
        drop(terminal);
        reader.join().unwrap();
        let error = result
            .0
            .expect_err("a closed source fault must remain visible in the exit status");
        assert!(
            error.starts_with("Job step exited 2;"),
            "after {} lines: {error}",
            result.1
        );
        assert_eq!(
            result.1, 3,
            "expected Fault, refusal and Status before Quit"
        );
    });
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
