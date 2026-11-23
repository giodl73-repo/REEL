//! Test-owned signals only. No device, recording, playback or cache enrollment.
use super::*;
use reel::spoken_programme::{checker, installation as api};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

struct InstallationFixture {
    root: PathBuf,
    candidate: PathBuf,
    access: PathBuf,
    f: Fixture,
    plan: Vec<u8>,
    receipt: Vec<u8>,
    access_manifest: Vec<u8>,
    runtime: compiler::CompilerIdentity,
    policy: ProcessingPolicy,
}
impl InstallationFixture {
    fn new(quiet: bool) -> Self {
        Self::from_fixture(Fixture::new(), quiet)
    }
    fn from_fixture(mut f: Fixture, quiet: bool) -> Self {
        f.document.modules.truncate(1);
        f.document.performances = vec![
            f.document.performances[0].clone(),
            f.document.performances[3].clone(),
        ];
        f.signals = vec![f.signals[0].clone(), f.signals[3].clone()];
        f.document.editions.retain(|e| e.id.ends_with("-technical"));
        for e in &mut f.document.editions {
            e.required_modules = vec!["opening".into()];
            e.selections.truncate(1);
            e.joins.clear();
        }
        for p in &mut f.document.performances {
            p.scope.edition_ids.retain(|id| id.ends_with("-technical"));
        }
        if quiet {
            // Stereo capture markers have independently known frame offsets.
            // Keep the low-level PCM conversion oracle at frame 5000 intact.
            for signal in &mut f.signals {
                let frames = signal.len() / 2;
                signal[..4800 * 2].fill(0);
                signal[(frames - 4800) * 2..].fill(0);
                for (offset, left, right) in [
                    (5040, 524_288, -262_144),
                    (frames - 5280, -262_144, 524_288),
                ] {
                    for (index, frame) in signal[offset * 2..(offset + 240) * 2]
                        .chunks_exact_mut(2)
                        .enumerate()
                    {
                        let polarity = if index / 24 % 2 == 0 { 1 } else { -1 };
                        frame.copy_from_slice(&[polarity * left, polarity * right]);
                    }
                }
            }
        }
        f.signals[0][5000 * 2..5000 * 2 + 5].copy_from_slice(&[-32768, -1, 0, 1, 32767]);
        f.sync();
        let root = f.temp.path().to_owned();
        let candidate = f.output();
        let access = root.join("access");
        let policy = ProcessingPolicy::identity();
        let runtime = producer::current_runtime().unwrap();
        let plan = serde_json::to_vec_pretty(
            &compiler::compile_with_inputs(
                &f.document,
                &root,
                &runtime,
                CompilationProfile::Engineering,
            )
            .unwrap(),
        )
        .unwrap();
        let receipt = serde_json::to_vec_pretty(
            &producer::produce(&f.document, &plan, &policy, &root, &candidate, || Ok(())).unwrap(),
        )
        .unwrap();
        let mut checked = checker::check(
            checker::CheckRequest {
                document: &f.document,
                plan_bytes: &plan,
                receipt_bytes: &receipt,
                processing: &policy,
                expected_runtime: &runtime,
            },
            &root,
            &candidate,
            control(),
        )
        .unwrap();
        checker::write_access(&mut checked, &access, || Ok(())).unwrap();
        let access_manifest = fs::read(access.join("access.json")).unwrap();
        Self {
            root,
            candidate,
            access,
            f,
            plan,
            receipt,
            access_manifest,
            runtime,
            policy,
        }
    }
    fn inputs(&self) -> api::Inputs<'_> {
        api::Inputs {
            spoken: checker::CheckRequest {
                document: &self.f.document,
                plan_bytes: &self.plan,
                receipt_bytes: &self.receipt,
                processing: &self.policy,
                expected_runtime: &self.runtime,
            },
            sources: &self.root,
            candidate: &self.candidate,
            access: &self.access,
            access_manifest: &self.access_manifest,
        }
    }
    fn prepared(&self, language: &str, passes: u8) -> api::Prepared<'_> {
        api::prepare(
            &api::Selection {
                edition_id: format!("{language}-technical"),
                passes,
            },
            self.inputs(),
            control(),
        )
        .unwrap()
    }
}
fn control() -> producer::ProductionControl<impl FnMut() -> Result<()>> {
    producer::ProductionControl {
        timeout: Duration::from_secs(30),
        cancellation: || Ok(()),
    }
}
fn selection() -> api::Selection {
    api::Selection {
        edition_id: "en-technical".into(),
        passes: 1,
    }
}

#[cfg(all(windows, feature = "installation-native"))]
fn fixture_file(root: &std::path::Path, name: &str, raw: &[u8]) -> reel::scene_delivery::FileRef {
    use reel::scene_delivery::FileRef;
    let path = root.join(name);
    fs::write(&path, raw).unwrap();
    FileRef {
        path,
        sha256: hash(raw),
        bytes: raw.len() as u64,
    }
}

#[cfg(all(windows, feature = "installation-native"))]
fn native_original(f: &InstallationFixture) -> api::native::OriginalProgramme {
    use api::native::OriginalProgramme;
    use reel::scene_delivery::FileRef;
    let put = |name, raw: &[u8]| fixture_file(&f.root, name, raw);
    // Provenance bytes need not be an executable on the playback OS. They are
    // retained/hashed, never run. This also proves no implicit current-player
    // identity can replace the original compiler/producer identity.
    let original_path = f.root.join("retained-original-runtime.data");
    fs::copy(std::env::current_exe().unwrap(), &original_path).unwrap();
    OriginalProgramme {
        declaration: put(
            "invocation-declaration.json",
            &serde_json::to_vec(&f.f.document).unwrap(),
        ),
        plan: put("invocation-plan.json", &f.plan),
        receipt: put("invocation-receipt.json", &f.receipt),
        processing: Some(put(
            "invocation-processing.json",
            &serde_json::to_vec(&f.policy).unwrap(),
        )),
        access_manifest: put("invocation-access.json", &f.access_manifest),
        compiler_producer: FileRef {
            path: original_path,
            sha256: f.runtime.sha256.clone(),
            bytes: f.runtime.bytes,
        },
        source_root: f.root.clone(),
        candidate_root: f.candidate.clone(),
        access_root: f.access.clone(),
    }
}

#[cfg(all(windows, feature = "installation-native"))]
fn native_request(f: &InstallationFixture) -> api::native::SessionRequest {
    use api::native::{EndpointBinding, PlaybackBuild, SessionRequest};
    let put = |name, raw: &[u8]| fixture_file(&f.root, name, raw);
    SessionRequest {
        schema: "reel.installation-session.v1".into(),
        edition_id: "en-technical".into(),
        passes: 2,
        original: native_original(f),
        playback: PlaybackBuild {
            runtime: put(
                "different-unexecuted-player.exe",
                b"unexecuted player fixture",
            ),
            source_commit: "a".repeat(40),
            cargo_lock: put("invocation-Cargo.lock", b"fixture build context only"),
            cpal_version: "0.18.2".into(),
        },
        endpoint: EndpointBinding {
            profile: api::native::PROFILE.into(),
            device_id: "wasapi:{0.0.0.00000000}.{11111111-1111-1111-1111-111111111111}".into(),
            os_build: 26300,
            driver_version: "fictional metadata only".into(),
            configuration_sha256: "b".repeat(64),
        },
    }
}

#[cfg(all(windows, feature = "installation-native"))]
#[test]
#[ignore = "explicit synthetic station fixture verification and optional retention"]
fn export_synthetic_station_originals() {
    let parent = std::env::var_os("REEL_INSTALLATION_FIXTURE_PARENT").map(PathBuf::from);
    let mut builder = tempfile::Builder::new();
    builder.prefix("reel-installation-fixture-");
    let temp = match &parent {
        Some(parent) => {
            assert!(parent.is_absolute() && parent.is_dir());
            builder.tempdir_in(parent)
        }
        None => builder.tempdir(),
    }
    .unwrap();
    let mut fixture = InstallationFixture::from_fixture(Fixture::with_temp(temp), true);
    let original = native_original(&fixture);
    let prepared = fixture.prepared("en", 2);
    assert_eq!(prepared.summary().source_frames, 144_000);
    prepared.transport(0, control()).unwrap();
    fs::write(
        fixture.root.join("original.json"),
        serde_json::to_vec_pretty(&original).unwrap(),
    )
    .unwrap();
    if parent.is_some() {
        fixture.f.temp.disable_cleanup(true);
        println!(
            "Synthetic installation originals: {}",
            fixture.root.display()
        );
    }
}

#[cfg(all(windows, feature = "installation-native"))]
#[test]
#[ignore = "explicit protected original-input session qualification; no devices"]
fn native_original_check_preserves_producer_identity_separately_from_player() {
    let f = InstallationFixture::new(true);
    let mut request = native_request(&f);
    fs::remove_file(&request.playback.runtime.path).unwrap();
    let result = request.check_originals(control()).unwrap();
    assert!(
        result.programme.original_pcm_and_access_checked
            && result.original_runtime_bytes_checked
            && result.original_reads_guarded
    );
    assert_eq!(result.programme.source_frames, 144000);
    assert!(
        !result.playback_runtime_checked && !result.host_profile_checked && !result.endpoint_probed
    );
    assert!(
        !result.programme.device_playback_checked
            && !result.programme.listening_reviewed
            && !result.programme.authority_verified
            && !result.programme.release_authorized
    );
    let portable = serde_json::to_string(&result).unwrap();
    assert!(
        !portable.contains("technical")
            && !portable.contains("fictional")
            && !portable.contains("retained-original")
    );
    let control_path = f.root.join("private-session-request.json");
    fs::write(&control_path, serde_json::to_vec(&request).unwrap()).unwrap();
    // A different executable must retain the original test producer identity.
    let output = Command::new(env!("CARGO_BIN_EXE_reel-installation"))
        .arg("check-session")
        .arg(&control_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["programme"]["source_frames"], 144000);
    assert_eq!(report["original_runtime_bytes_checked"], true);
    assert_eq!(report["playback_runtime_checked"], false);
    assert_eq!(report["host_profile_checked"], false);
    assert_eq!(report["endpoint_probed"], false);
    // This is a valid, byte-matching alternative file, not a deliberately broken
    // hash. It must still fail against the original production relationship.
    fs::write(&request.playback.runtime.path, b"unexecuted player fixture").unwrap();
    request.original.compiler_producer = request.playback.runtime.clone();
    let failure = request
        .check_originals(control())
        .err()
        .unwrap()
        .to_string();
    assert!(failure.contains("producer runtime differs"), "{failure}");
}

#[test]
#[ignore = "explicit original-input installation qualification"]
fn checked_selection_requires_original_pcm_and_access() {
    let f = InstallationFixture::new(true);
    for (language, frames) in [("en", 144000), ("es", 168000)] {
        let prepared = f.prepared(language, 3);
        let summary = prepared.summary();
        assert_eq!(summary.source_frames, frames);
        assert_eq!(summary.passes, 3);
        assert!(summary.original_pcm_and_access_checked);
        assert!(
            !summary.device_playback_checked
                && !summary.listening_reviewed
                && !summary.authority_verified
                && !summary.release_authorized
        );
        assert!(
            prepared
                .transcript()
                .contains("Engineering signal; no spoken-word evidence")
        );
        let t = prepared.transport(0, control()).unwrap();
        assert_eq!(t.snapshot().state, api::State::Ready);
        assert_eq!(t.transcript(), prepared.transcript());
        let portable = serde_json::to_string(&summary).unwrap();
        for private in [
            "technical",
            "Speaker",
            "Engineering",
            "take-",
            "fixture-private",
        ] {
            assert!(!portable.contains(private));
        }
    }
    let prepared = f.prepared("en", 1);
    let file = f.candidate.join("edition-000.wav");
    let raw = fs::read(&file).unwrap();
    let mut altered = raw.clone();
    altered[44 + 5000 * 6] ^= 1;
    fs::write(&file, altered).unwrap();
    assert!(prepared.transport(0, control()).is_err());
    fs::write(&file, raw).unwrap();
    let transcript = f.access.join("edition-000.txt");
    let original = fs::read(&transcript).unwrap();
    fs::write(&transcript, b"Not the original text\n").unwrap();
    assert!(prepared.transport(0, control()).is_err());
    fs::write(&transcript, original).unwrap();
    fs::remove_file(&file).unwrap();
    assert!(prepared.transport(0, control()).is_err());
}

#[cfg(all(windows, feature = "installation-native"))]
fn source_event(
    worker: &mut api::native::source::SourceWorker,
) -> api::native::source::SourceEvent {
    let until = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(event) = worker.event() {
            return event;
        }
        assert!(
            std::time::Instant::now() < until,
            "original worker event deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(all(windows, feature = "installation-native"))]
#[test]
#[ignore = "explicit current-player binding qualification; no originals/devices"]
fn native_player_binding_checks_current_image_and_embedded_lock_independently() {
    use api::native::protocol::{Output, RuntimeBinding, WorkerGate};
    let f = InstallationFixture::new(true);
    let mut request = native_request(&f);
    fs::remove_file(&request.original.declaration.path).unwrap();
    request.playback.runtime = reel::scene_delivery::FileRef {
        path: std::env::current_exe().unwrap(),
        sha256: f.runtime.sha256.clone(),
        bytes: f.runtime.bytes,
    };
    let lock = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock"));
    fs::write(&request.playback.cargo_lock.path, lock).unwrap();
    request.playback.cargo_lock.sha256 = hash(lock);
    request.playback.cargo_lock.bytes = lock.len() as u64;
    let raw = serde_json::to_vec(&request).unwrap();
    let binding = RuntimeBinding::bind(&raw).unwrap();
    binding.recheck().unwrap();
    assert_eq!(
        binding.runtime_path().canonicalize().unwrap(),
        std::env::current_exe().unwrap().canonicalize().unwrap()
    );
    let gate = WorkerGate::new(binding.context(), std::process::id(), [1; 16]).unwrap();
    match gate.hello() {
        Output::Hello {
            request_sha256,
            runtime_sha256,
            process_id,
        } => {
            assert_eq!(request_sha256, hash(&raw));
            assert_eq!(runtime_sha256, f.runtime.sha256);
            assert_eq!(process_id, std::process::id());
        }
        _ => panic!("unexpected player handshake"),
    }
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(&request.playback.cargo_lock.path)
            .is_err()
    );
    drop(binding);
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(&request.playback.cargo_lock.path)
            .is_err()
    );
    drop(gate);
    fs::write(
        &request.playback.cargo_lock.path,
        b"independently pinned incorrect lock",
    )
    .unwrap();
    request.playback.cargo_lock.sha256 = hash(b"independently pinned incorrect lock");
    request.playback.cargo_lock.bytes = b"independently pinned incorrect lock".len() as u64;
    assert!(
        RuntimeBinding::bind(&serde_json::to_vec(&request).unwrap())
            .err()
            .unwrap()
            .to_string()
            .contains("compiled lock")
    );
    fs::write(&request.playback.cargo_lock.path, lock).unwrap();
    request.playback.cargo_lock.sha256 = hash(lock);
    request.playback.cargo_lock.bytes = lock.len() as u64;
    let alternate = f.root.join("byte-correct-other-player.exe");
    fs::copy(std::env::current_exe().unwrap(), &alternate).unwrap();
    request.playback.runtime.path = alternate;
    assert!(
        RuntimeBinding::bind(&serde_json::to_vec(&request).unwrap())
            .err()
            .unwrap()
            .to_string()
            .contains("current retained image")
    );
}

#[cfg(all(windows, feature = "installation-native"))]
#[test]
#[ignore = "explicit guarded source-worker qualification; no devices"]
fn native_source_worker_retains_guards_and_checks_real_opaque_restart() {
    use api::native::{
        output::OutputClock,
        source::{SourceEvent, SourceWorker},
    };
    let f = InstallationFixture::new(true);
    let request = native_request(&f);
    let declaration = request.original.declaration.path.clone();
    let original = request.original.compiler_producer.path.clone();
    // This port checks originals; the distinct player remains unavailable and
    // is never executed or substituted for their compiler identity.
    fs::remove_file(&request.playback.runtime.path).unwrap();
    let clock = OutputClock::start().unwrap();
    let mut worker =
        SourceWorker::new(request, clock, clock.now_us().unwrap() + 30_000_000).unwrap();
    let mut actual = match source_event(&mut worker) {
        SourceEvent::Ready(actual) => actual,
        _ => panic!("original worker did not prepare the actual programme"),
    };
    assert_eq!(actual.snapshot().state, api::State::Ready);
    assert!(
        actual
            .transcript()
            .contains("Engineering signal; no spoken-word evidence")
    );
    let address = &*actual as *const api::Transport;
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(&declaration)
            .is_err()
    );
    assert!(fs::OpenOptions::new().write(true).open(&original).is_err());
    actual.enqueue("stop").unwrap();
    actual.poll(clock.now_us().unwrap() / 1000, api::Notice::None);
    actual.enqueue("restart").unwrap();
    actual.poll(clock.now_us().unwrap() / 1000, api::Notice::None);
    let ticket = actual.restart_ticket().unwrap();
    worker
        .restart(ticket, clock.now_us().unwrap() + 30_000_000)
        .unwrap();
    let permit = match source_event(&mut worker) {
        SourceEvent::RestartChecked {
            generation: 1,
            permit,
        } => permit,
        _ => panic!("original recheck did not return its opaque permit"),
    };
    assert_eq!(
        actual
            .accept_restart(permit, clock.now_us().unwrap() / 1000)
            .unwrap()
            .open,
        Some(1)
    );
    assert_eq!(&*actual as *const api::Transport, address);
    // A second real check may finish after Stop. Its valid check result still
    // cannot revive the core: applicability is enforced by the same transport.
    let mut out = [0.0; 2];
    actual.poll(
        clock.now_us().unwrap() / 1000,
        api::Notice::Opened { generation: 1 },
    );
    actual
        .fill(1, clock.now_us().unwrap() / 1000, &mut out)
        .unwrap();
    actual.enqueue("restart").unwrap();
    actual.poll(clock.now_us().unwrap() / 1000, api::Notice::None);
    worker
        .restart(
            actual.restart_ticket().unwrap(),
            clock.now_us().unwrap() + 30_000_000,
        )
        .unwrap();
    actual.enqueue("stop").unwrap();
    actual.poll(clock.now_us().unwrap() / 1000, api::Notice::None);
    match source_event(&mut worker) {
        SourceEvent::RestartChecked {
            generation: 2,
            permit,
        } => {
            assert_eq!(
                actual
                    .accept_restart(permit, clock.now_us().unwrap() / 1000)
                    .unwrap_err(),
                api::TransportError::StaleGeneration
            );
        }
        _ => panic!("second original check unexpectedly failed"),
    }
    assert_eq!(actual.snapshot().state, api::State::Stopped);
    drop(worker);
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(&declaration)
            .is_ok()
    );
    assert!(fs::OpenOptions::new().write(true).open(&original).is_ok());
}

#[cfg(all(windows, feature = "installation-native"))]
#[test]
#[ignore = "explicit combined runtime/original source-thread qualification; no devices"]
fn native_source_thread_retains_and_rechecks_current_runtime_and_originals() {
    use api::native::{
        output::OutputClock,
        protocol::RuntimeBinding,
        source::{SourceEvent, SourceWorker},
    };
    let f = InstallationFixture::new(true);
    let mut request = native_request(&f);
    request.playback.runtime = reel::scene_delivery::FileRef {
        path: std::env::current_exe().unwrap(),
        sha256: f.runtime.sha256.clone(),
        bytes: f.runtime.bytes,
    };
    let lock = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock"));
    fs::write(&request.playback.cargo_lock.path, lock).unwrap();
    request.playback.cargo_lock.sha256 = hash(lock);
    request.playback.cargo_lock.bytes = lock.len() as u64;
    let lock_path = request.playback.cargo_lock.path.clone();
    let binding =
        std::sync::Arc::new(RuntimeBinding::bind(&serde_json::to_vec(&request).unwrap()).unwrap());
    let clock = OutputClock::start().unwrap();
    let mut worker =
        SourceWorker::with_runtime(binding.clone(), clock, clock.now_us().unwrap() + 30_000_000)
            .unwrap();
    drop(binding);
    let core = match source_event(&mut worker) {
        SourceEvent::Ready(core) => core,
        _ => panic!("combined source check failed"),
    };
    assert!(fs::OpenOptions::new().write(true).open(&lock_path).is_err());
    // The real source/controller connection stops before native probing: this
    // proves checking/staging without querying an actual audio endpoint.
    let output = api::native::output::OutputWorker::new(request.endpoint.clone(), clock).unwrap();
    let mut controller = api::native::controller::Controller::new(core, output, clock);
    controller
        .command(1, api::Command::Stop, 0, clock.now_us().unwrap())
        .unwrap();
    controller
        .command(2, api::Command::Restart, 0, clock.now_us().unwrap())
        .unwrap();
    let until = std::time::Instant::now() + Duration::from_secs(30);
    while !controller.has_staged_restart() {
        controller.drive_source(&mut worker).unwrap();
        assert!(
            std::time::Instant::now() < until,
            "source/controller check deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(controller.observed().state, api::State::Restarting);
    assert_eq!(controller.observed().generation, 1);
    drop(controller);
    drop(worker);
    assert!(fs::OpenOptions::new().write(true).open(&lock_path).is_ok());
}

#[test]
#[ignore = "explicit original-input installation qualification"]
fn exact_checked_pcm_reaches_finite_transport_without_device() {
    let f = InstallationFixture::new(true);
    let p = f.prepared("en", 2);
    let mut t = p.transport(0, control()).unwrap();
    t.enqueue("start").unwrap();
    assert_eq!(t.poll(0, api::Notice::None).open, Some(1));
    t.poll(1, api::Notice::Opened { generation: 1 });
    let source: Vec<f32> = f.f.signals[0]
        .iter()
        .map(|n| *n as f32 / 8388608.0)
        .collect();
    let mut expected = source.clone();
    expected.extend(vec![0.0; 24000]);
    expected.extend(source);
    let mut actual = Vec::new();
    let mut block = vec![3.0; 960];
    let mut at = 2;
    while t.snapshot().state != api::State::Completed {
        t.fill(1, at, &mut block).unwrap();
        actual.extend_from_slice(&block);
        at += 10;
    }
    assert_eq!(&actual[..expected.len()], expected);
    let bits: Vec<_> = actual[5000 * 2..5000 * 2 + 5]
        .iter()
        .map(|n| n.to_bits())
        .collect();
    assert_eq!(bits, [0xbb800000, 0xb4000000, 0, 0x34000000, 0x3b7ffe00]);
    // Independent offsets and amplitudes for the checked English edition.
    for pass_start in [0, 156_000] {
        assert!(
            actual[pass_start * 2..(pass_start + 4800) * 2]
                .iter()
                .all(|v| *v == 0.0)
        );
        assert!(
            actual[(pass_start + 139_200) * 2..(pass_start + 144_000) * 2]
                .iter()
                .all(|v| *v == 0.0)
        );
        for (offset, left, right) in [(5040, 0.0625, -0.03125), (138_720, -0.03125, 0.0625)] {
            let start = (pass_start + offset) * 2;
            let marker: Vec<_> = [left, right]
                .repeat(24)
                .into_iter()
                .chain([-left, -right].repeat(24))
                .collect();
            assert_eq!(&actual[start..start + 480], marker.repeat(5));
        }
    }
    assert!(actual[expected.len()..].iter().all(|v| *v == 0.0));
    assert_eq!(t.snapshot().pass, 2);
}

#[cfg(all(windows, feature = "installation-native"))]
#[test]
#[ignore = "explicit authenticated worker/original integration; no endpoint admission"]
fn native_worker_routes_exact_spanish_originals_and_refuses_start_before_display() {
    use api::native::{
        output::OutputClock,
        pipe::Channel,
        protocol::{Input, Output, ParentGate, Preparation, RuntimeBinding},
        worker::WorkerSession,
    };
    let f = InstallationFixture::new(true);
    let package: checker::AccessPackage = serde_json::from_slice(&f.access_manifest).unwrap();
    let transcript = &package
        .editions
        .iter()
        .find(|edition| edition.edition_id == "es-technical")
        .unwrap()
        .transcript;
    // Independent byte oracle: retained original access file, not a second
    // invocation of the player preparation path under test.
    let expected = fs::read_to_string(f.access.join(&transcript.path)).unwrap();
    let mut request = native_request(&f);
    request.edition_id = "es-technical".into();
    request.playback.runtime = reel::scene_delivery::FileRef {
        path: std::env::current_exe().unwrap(),
        sha256: f.runtime.sha256.clone(),
        bytes: f.runtime.bytes,
    };
    let lock = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock"));
    fs::write(&request.playback.cargo_lock.path, lock).unwrap();
    request.playback.cargo_lock.sha256 = hash(lock);
    request.playback.cargo_lock.bytes = lock.len() as u64;
    let binding =
        std::sync::Arc::new(RuntimeBinding::bind(&serde_json::to_vec(&request).unwrap()).unwrap());
    let (invitation, mut parent_channel) = Channel::server().unwrap();
    let channel = Channel::connect(&invitation).unwrap();
    let clock = OutputClock::start().unwrap();
    let mut worker = WorkerSession::new(binding.clone(), channel, &invitation, clock).unwrap();
    let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    struct Stop(std::sync::Arc<std::sync::atomic::AtomicBool>);
    impl Drop for Stop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let counter = || {
        let mut at = 0;
        unsafe {
            windows::Win32::System::Performance::QueryPerformanceCounter(&mut at).unwrap();
        }
        at
    };
    std::thread::scope(|scope| {
        let cancel = stopped.clone();
        let child = scope.spawn(move || {
            while !worker.closing() {
                if cancel.load(Ordering::Acquire) {
                    return;
                }
                worker.tick().unwrap();
                assert!(worker.observed().is_none());
                std::thread::sleep(Duration::from_millis(2));
            }
            let mut closure = worker.finish().unwrap();
            assert!(matches!(
                closure.report,
                Output::PreparationEnded {
                    phase: Preparation::Stopped,
                    ..
                }
            ));
            closure
                .channel
                .send(closure.generation, &closure.report.bytes().unwrap())
                .unwrap();
            // Test-owned fence, not the production console/delivery proof.
            while !cancel.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(2));
            }
            closure.channel.shutdown().unwrap();
        });
        let _stop = Stop(stopped.clone());
        let until = std::time::Instant::now() + Duration::from_secs(40);
        let mut transcript = false;
        let mut refused = false;
        let mut ended = false;
        while !ended {
            assert!(
                std::time::Instant::now() < until,
                "worker original integration deadline"
            );
            assert!(parent_channel.verify_peer(std::process::id()).is_ok());
            while let Some(packet) = parent_channel.receive() {
                match Output::parse(&packet).unwrap() {
                    Output::Hello { .. } => {
                        let mut parent = ParentGate::new(
                            binding.context(),
                            std::process::id(),
                            invitation.nonce,
                        )
                        .unwrap();
                        let auth = parent.authorize(&parent_channel, &packet).unwrap();
                        parent_channel.send(0, &auth.bytes().unwrap()).unwrap();
                    }
                    Output::Preparing {
                        phase: Preparation::Checking | Preparation::AwaitingText,
                        ..
                    } => {}
                    Output::Transcript { text, sha256 } => {
                        assert_eq!(text, expected);
                        assert_eq!(sha256, hash(expected.as_bytes()));
                        transcript = true;
                        parent_channel
                            .send(
                                0,
                                &Input::Control {
                                    id: 1,
                                    command: api::Command::Start,
                                    received_counter: counter(),
                                }
                                .bytes()
                                .unwrap(),
                            )
                            .unwrap();
                    }
                    Output::PreparationAck {
                        id: 1,
                        phase: Preparation::AwaitingText,
                        refusal: Some(api::TransportError::InvalidState),
                        ..
                    } => {
                        assert!(transcript);
                        refused = true;
                        parent_channel
                            .send(
                                0,
                                &Input::AckShown {
                                    id: 1,
                                    observed_counter: counter(),
                                }
                                .bytes()
                                .unwrap(),
                            )
                            .unwrap();
                        parent_channel
                            .send(
                                0,
                                &Input::Control {
                                    id: 2,
                                    command: api::Command::Quit,
                                    received_counter: counter(),
                                }
                                .bytes()
                                .unwrap(),
                            )
                            .unwrap();
                    }
                    Output::PreparationAck {
                        id: 2,
                        phase: Preparation::AwaitingText,
                        refusal: None,
                        ..
                    } => assert!(refused),
                    Output::CreditReleased {
                        id: 1,
                        generation: 0,
                        ..
                    } => assert!(refused),
                    Output::PreparationEnded {
                        phase: Preparation::Stopped,
                        ..
                    } => {
                        assert!(refused);
                        ended = true;
                    }
                    _ => panic!("undisplayed original reached unexpected native readiness"),
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        stopped.store(true, Ordering::Release);
        child.join().unwrap();
        parent_channel.shutdown().unwrap();
    });
}

#[test]
#[ignore = "explicit original-input installation qualification"]
fn restart_rechecks_same_originals_and_stale_input_faults() {
    let f = InstallationFixture::new(true);
    let p = f.prepared("en", 1);
    let mut t = p.transport(0, control()).unwrap();
    t.enqueue("start").unwrap();
    t.poll(0, api::Notice::None);
    t.poll(1, api::Notice::Opened { generation: 1 });
    t.enqueue("restart").unwrap();
    assert_eq!(t.poll(2, api::Notice::None).recheck, Some(2));
    assert_eq!(
        t.accept_restart(
            p.restart_permit(t.restart_ticket().unwrap(), control())
                .unwrap(),
            3
        )
        .unwrap()
        .open,
        Some(2)
    );
    t.poll(4, api::Notice::Opened { generation: 2 });
    assert_eq!(t.snapshot().state, api::State::Playing);
    t.enqueue("restart").unwrap();
    t.poll(5, api::Notice::None);
    fs::write(f.access.join("edition-000.txt"), b"stale\n").unwrap();
    assert!(
        p.restart_permit(t.restart_ticket().unwrap(), control())
            .is_err()
    );
    t.poll(6, api::Notice::RestartRejected { generation: 3 });
    assert_eq!(t.snapshot().state, api::State::Fault);
    assert!(t.enqueue("resume").is_err());
}

#[test]
#[ignore = "explicit original-input installation qualification"]
fn stop_remains_responsive_while_original_restart_check_is_in_flight() {
    let f = InstallationFixture::new(true);
    let p = f.prepared("en", 1);
    let mut t = p.transport(0, control()).unwrap();
    t.enqueue("start").unwrap();
    t.poll(0, api::Notice::None);
    t.poll(1, api::Notice::Opened { generation: 1 });
    t.enqueue("restart").unwrap();
    t.poll(2, api::Notice::None);
    let barrier = std::sync::Barrier::new(2);
    let ticket = t.restart_ticket().unwrap();
    std::thread::scope(|scope| {
        let job = scope.spawn(|| {
            let mut first = true;
            p.restart_permit(
                ticket,
                producer::ProductionControl {
                    timeout: Duration::from_secs(30),
                    cancellation: || {
                        if first {
                            first = false;
                            barrier.wait();
                            barrier.wait();
                        }
                        Ok(())
                    },
                },
            )
            .unwrap()
        });
        barrier.wait();
        // The checker is blocked at its original-input boundary on another
        // thread; the controller still owns and can stop its transport.
        t.enqueue("stop").unwrap();
        assert!(t.poll(3, api::Notice::None).close);
        assert_eq!(t.snapshot().state, api::State::Stopped);
        barrier.wait();
        let permit = job.join().unwrap();
        assert_eq!(
            t.accept_restart(permit, 4),
            Err(api::TransportError::StaleGeneration)
        );
        assert_eq!(t.snapshot().state, api::State::Stopped);
    });
}

#[test]
#[ignore = "explicit original-input installation qualification"]
fn selection_deadline_and_cancellation_refuse_without_fallback() {
    let f = InstallationFixture::new(true);
    struct Refuses(AtomicUsize);
    impl reel::scene_delivery::InputFiles for Refuses {
        fn verified_path(&self, _: &reel::scene_delivery::FileRef) -> Result<PathBuf> {
            self.0.fetch_add(1, Ordering::SeqCst);
            bail!("unavailable")
        }
    }
    let provider = Refuses(AtomicUsize::new(0));
    let mut inputs = f.inputs();
    inputs.sources = &provider;
    inputs.candidate = &provider;
    inputs.access = &provider;
    for passes in [0, 4, 255] {
        assert!(
            api::prepare(
                &api::Selection {
                    passes,
                    ..selection()
                },
                inputs,
                control()
            )
            .is_err()
        );
    }
    assert_eq!(provider.0.load(Ordering::SeqCst), 0);
    assert!(api::prepare(&selection(), inputs, control()).is_err());
    assert!(provider.0.load(Ordering::SeqCst) > 0);
    assert!(
        api::prepare(
            &selection(),
            f.inputs(),
            producer::ProductionControl {
                timeout: Duration::ZERO,
                cancellation: || Ok(())
            }
        )
        .is_err()
    );
    assert!(
        api::prepare(
            &selection(),
            f.inputs(),
            producer::ProductionControl {
                timeout: Duration::from_secs(31),
                cancellation: || Ok(())
            }
        )
        .is_err()
    );
    assert!(
        api::prepare(
            &selection(),
            f.inputs(),
            producer::ProductionControl {
                timeout: Duration::from_secs(30),
                cancellation: || bail!("cancelled")
            }
        )
        .is_err()
    );
    assert!(
        api::prepare(
            &api::Selection {
                edition_id: "missing".into(),
                passes: 1
            },
            f.inputs(),
            control()
        )
        .is_err()
    );
    assert!(!f.root.join("installation-report.json").exists());
}

#[test]
#[ignore = "explicit original-input installation qualification"]
fn nonquiet_seam_and_self_rehashed_candidate_refuse() {
    let f = InstallationFixture::new(false);
    let err = api::prepare(&selection(), f.inputs(), control())
        .err()
        .unwrap()
        .to_string();
    assert!(err.contains("quiet head and tail"), "{err}");
    let mut f = InstallationFixture::new(true);
    let file = f.candidate.join("edition-000.wav");
    let mut raw = fs::read(&file).unwrap();
    raw[44 + 5000 * 6] ^= 1;
    fs::write(file, &raw).unwrap();
    let mut receipt: serde_json::Value = serde_json::from_slice(&f.receipt).unwrap();
    receipt["editions"][0]["wave"]["sha256"] = json!(hash(&raw));
    receipt["editions"][0]["pcm_sha256"] = json!(hash(&raw[44..]));
    f.receipt = serde_json::to_vec(&receipt).unwrap();
    let error = api::prepare(&selection(), f.inputs(), control())
        .err()
        .unwrap();
    assert!(
        format!("{error:#}").contains("independent PCM comparison failed"),
        "{error:#}"
    );
}
