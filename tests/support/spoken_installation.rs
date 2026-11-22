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
        let mut f = Fixture::new();
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
            for signal in &mut f.signals {
                signal[..4800 * 2].fill(0);
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
    assert!(actual[expected.len()..].iter().all(|v| *v == 0.0));
    assert_eq!(t.snapshot().pass, 2);
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
