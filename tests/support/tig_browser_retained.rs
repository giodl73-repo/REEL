use super::*;
use tig_store::metadata::MetadataReader;

fn publish(cache: &Path, bytes: &[u8]) -> Object {
    let object = Object::from_bytes(bytes);
    let parent = cache
        .join("objects/sha256")
        .join(&object.sha256.as_str()[..2]);
    fs::create_dir_all(&parent).unwrap();
    let path = parent.join(object.sha256.as_str());
    if !path.exists() {
        fs::write(path, bytes).unwrap();
    }
    object
}
pub fn exercise(f: &Fixture, profile: &Value, completed: &Value, cache: &Path) {
    let binary = Path::new(env!("CARGO_BIN_EXE_reel-tig-job"))
        .canonicalize()
        .unwrap();
    let retained = &completed["result"]["outputs"];
    let receipt_bytes =
        fs::read(retained["native/receipt.json"]["path"].as_str().unwrap()).unwrap();
    let receipt: Value = serde_json::from_slice(&receipt_bytes).unwrap();
    let mut runtime = profile["worker"]["runtime"].clone();
    runtime["host"] = json!(reel::media_runtime::ByteIdentity::observe_file(&binary).unwrap());
    assert_ne!(runtime["host"], profile["worker"]["runtime"]["host"]);
    let master: Object = serde_json::from_value(json!({
        "sha256":retained["native/master.mkv"]["sha256"],
        "bytes":retained["native/master.mkv"]["bytes"]}))
    .unwrap();
    let comparison = json!({"schema":"reel.tig-browser-retained-comparison.experimental-1",
        "metadata_root":profile["worker"]["metadata_root"],"metadata_minimum":profile["worker"]["metadata_minimum"],
        "source":profile["worker"]["source"],"context":profile["worker"]["context"],"request":profile["worker"]["request"],
        "producer":receipt["producer"],"claim":completed["record_candidates"]["claim"],
        "receipt":completed["record_candidates"]["receipt"],"reference":master,"runtime":runtime,
        "byte_source":{"places":[{"root":cache,"layout":"reel-cas"}],"required":"prevent-mutation"},
        "cache":profile["cache"],"preparation_cache":cache,"max_verified_bytes":134217728_u64});
    let metadata_root = Path::new(profile["worker"]["metadata_root"].as_str().unwrap());
    let minimum = serde_json::from_value(profile["worker"]["metadata_minimum"].clone()).unwrap();
    let before = MetadataReader::open(metadata_root, &minimum)
        .unwrap()
        .checkpoint()
        .clone();
    let invoke = |value: &Value, output: &Path| {
        let input = f.root().join("browser-comparison-profile.json");
        fs::write(&input, serde_json::to_vec(value).unwrap()).unwrap();
        Command::new(&binary)
            .arg("compare-browser-retained")
            .arg(input)
            .arg(output)
            .env("PATH", "")
            .output()
            .unwrap()
    };
    let output = f.root().join("browser-retained-report.json");
    let positive_input = f.root().join("browser-positive-profile.json");
    fs::write(&positive_input, serde_json::to_vec(&comparison).unwrap()).unwrap();
    let success_failure = f.root().join("browser-success-failure.json");
    let checked = Command::new(&binary)
        .arg("compare-browser-retained")
        .arg(&positive_input)
        .arg(&output)
        .arg("--failure-report")
        .arg(&success_failure)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(!success_failure.exists());
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let summary: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(summary["equal"], true);
    assert_eq!(summary["materialized_bytes"], 0);
    assert!(!String::from_utf8_lossy(&checked.stdout).contains(f.root().to_str().unwrap()));
    let original_report = fs::read(&output).unwrap();
    let report: Value = serde_json::from_slice(&original_report).unwrap();
    assert_eq!(report["original_producer_admission"], receipt["producer"]);
    assert_ne!(report["validator"], receipt["producer"]);
    assert_eq!(report["context"], comparison["context"]);
    assert_eq!(report["parity"]["native_clock_checked"], true);
    assert_eq!(report["parity"]["pcm_equal"], true);
    for flag in [
        "execution_attested",
        "complete_semantic_coverage",
        "approval_transferred",
        "authorizes_eviction",
    ] {
        assert_eq!(report[flag], false);
    }
    assert!(!invoke(&comparison, &output).status.success());
    assert_eq!(fs::read(&output).unwrap(), original_report);
    for case in [
        "producer",
        "context",
        "budget",
        "unprotected",
        "frames",
        "approval",
        "pcm",
        "reader-object",
        "reader-total",
        "reader-requests",
        "reader-live",
    ] {
        let mut bad = comparison.clone();
        match case {
            "producer" => bad["producer"] = json!(Object::from_bytes(b"foreign original producer")),
            "context" => {
                bad["context"]["context"] = json!(Object::from_bytes(b"foreign context").sha256)
            }
            "budget" => bad["max_verified_bytes"] = json!(1),
            "reader-object" => {
                bad["byte_source"]["limits"] = json!({"object_bytes":master.bytes-1})
            }
            "reader-total" => {
                bad["byte_source"]["limits"] = json!({"total_bytes":master.bytes+4096})
            }
            "reader-requests" => bad["byte_source"]["limits"] = json!({"requests":3}),
            "reader-live" => bad["byte_source"]["limits"] = json!({"live_readers":1}),
            "unprotected" => {
                bad["source"]["byte_source"]["required"] = json!("cooperative-immutable")
            }
            _ => {
                let mut wrong = receipt.clone();
                match case {
                    "frames" => wrong["frames"] = json!(7),
                    "approval" => wrong["approved"] = json!(true),
                    _ => wrong["pcm_sha256"] = json!(Object::from_bytes(b"wrong PCM").sha256),
                }
                bad["receipt"] = json!(publish(cache, &serde_json::to_vec(&wrong).unwrap()));
            }
        }
        let denied = f.root().join(format!("browser-denied-{case}.json"));
        let result = invoke(&bad, &denied);
        assert!(!result.status.success(), "{case}");
        assert!(!denied.exists(), "{case}");
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(
            error.contains("retained browser comparison refused"),
            "{case}: {error}"
        );
        assert!(!error.contains(f.root().to_str().unwrap()));
    }
    let mut diagnostic_case = comparison.clone();
    diagnostic_case["max_verified_bytes"] = json!(1);
    let diagnostic_input = f.root().join("browser-diagnostic-profile.json");
    fs::write(
        &diagnostic_input,
        serde_json::to_vec(&diagnostic_case).unwrap(),
    )
    .unwrap();
    let refused_output = f.root().join("browser-diagnostic-comparison.json");
    let private_failure = f.root().join("browser-private-failure.json");
    let diagnostic = Command::new(&binary)
        .arg("compare-browser-retained")
        .arg(&diagnostic_input)
        .arg(&refused_output)
        .arg("--failure-report")
        .arg(&private_failure)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(!diagnostic.status.success());
    assert!(!refused_output.exists());
    let failure_bytes = fs::read(&private_failure).unwrap();
    assert!(failure_bytes.len() <= 32 * 1024);
    let failure: Value = serde_json::from_slice(&failure_bytes).unwrap();
    assert_eq!(failure["private"], true);
    assert_eq!(failure["stage"], "retained metadata");
    assert!(!failure["detail"].as_str().unwrap().is_empty());
    assert!(!String::from_utf8_lossy(&diagnostic.stderr).contains("byte allowance"));
    let again = Command::new(&binary)
        .arg("compare-browser-retained")
        .arg(&diagnostic_input)
        .arg(&refused_output)
        .arg("--failure-report")
        .arg(&private_failure)
        .output()
        .unwrap();
    assert!(!again.status.success());
    assert_eq!(fs::read(&private_failure).unwrap(), failure_bytes);
    // A valid same-clock native file with different pictures/audio must produce
    // an unequal report, not a successful claim or a profile-admission refusal.
    let changed = f.root().join("different-browser-reference.mkv");
    let generated = Command::new(
        profile["worker"]["runtime"]["ffmpeg"]["path"]
            .as_str()
            .unwrap(),
    )
    .args([
        "-v",
        "error",
        "-nostdin",
        "-f",
        "lavfi",
        "-i",
        "color=black:s=8x8:r=24:d=0.25",
        "-f",
        "lavfi",
        "-i",
        "anullsrc=r=48000:cl=stereo",
        "-frames:v",
        "6",
        "-t",
        "0.25",
        "-c:v",
        "ffv1",
        "-level",
        "3",
        "-pix_fmt",
        "yuv444p",
        "-c:a",
        "pcm_s24le",
    ])
    .arg(&changed)
    .output()
    .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let mut unequal = comparison.clone();
    unequal["reference"] = json!(publish(cache, &fs::read(changed).unwrap()));
    let different = f.root().join("browser-unequal-report.json");
    let result = invoke(&unequal, &different);
    assert!(!result.status.success());
    let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["equal"], false);
    let report: Value = serde_json::from_slice(&fs::read(different).unwrap()).unwrap();
    assert_eq!(report["parity"]["picture_planes_equal"], false);
    fresh_primary_reference(f, profile, &master, &unequal["reference"]);
    assert_eq!(
        MetadataReader::open(metadata_root, &minimum)
            .unwrap()
            .checkpoint(),
        &before
    );
    assert!(!cache.read_dir().unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("native-prepare-")
    }));
}

fn fresh_primary_reference(f: &Fixture, profile: &Value, master: &Object, different: &Value) {
    use reel::{
        media_runtime::{Bindings, BoundPrograms, BoundTool, ByteIdentity},
        scene_delivery::{FileRef, InputFiles},
    };
    use tig_store::existing::{ByteSource, ReadCache, VerifiedRead};
    struct Held {
        file: FileRef,
        guard: VerifiedRead,
    }
    impl InputFiles for Held {
        fn verified_path(&self, file: &FileRef) -> anyhow::Result<PathBuf> {
            anyhow::ensure!(file == &self.file, "unexpected retained browser reference");
            Ok(self.guard.with_path(Path::to_path_buf)?)
        }
    }
    let mut metadata = MetadataReader::open(
        Path::new(profile["worker"]["metadata_root"].as_str().unwrap()),
        &serde_json::from_value(profile["worker"]["metadata_minimum"].clone()).unwrap(),
    )
    .unwrap();
    let request = serde_json::from_value(profile["worker"]["request"].clone()).unwrap();
    let (mut source, _context) = reel::tig_browser::restore_context(
        serde_json::from_value(profile["worker"]["source"].clone()).unwrap(),
        &request,
        &serde_json::from_value(profile["worker"]["context"].clone()).unwrap(),
        &mut metadata,
        Default::default(),
    )
    .unwrap();
    let plan = reel::tig_browser::lower(&mut source, &request).unwrap();
    let inputs = source
        .prepare_files(&plan.inputs.values().cloned().collect::<Vec<_>>())
        .unwrap();
    let mut bindings: Bindings =
        serde_json::from_value(profile["worker"]["runtime"].clone()).unwrap();
    bindings.host = ByteIdentity::observe_file(&std::env::current_exe().unwrap()).unwrap();
    let programs = BoundPrograms::bind(&bindings).unwrap();
    let browser = BoundTool::bind(
        &serde_json::from_value(profile["worker"]["browser"]["program"].clone()).unwrap(),
    )
    .unwrap();
    let limits = serde_json::from_value(profile["worker"]["browser"]["limits"].clone()).unwrap();
    let mut cache = ReadCache::new(
        serde_json::from_value(json!({
            "places":[{"root":f.root().join("capture-capacity"),"layout":"reel-cas"}],
            "required":"prevent-mutation"
        }))
        .unwrap(),
    )
    .unwrap();
    let held = Held {
        file: FileRef {
            path: "selected-browser-master.mkv".into(),
            sha256: master.sha256.as_str().into(),
            bytes: master.bytes,
        },
        guard: cache.open_verified(master).unwrap(),
    };
    let stage = f.root().join("fresh-browser-reference-stage");
    fs::create_dir(&stage).unwrap();
    let mut checks = 0;
    let compared = reel::tig_browser_reference::compare_master_to_primaries(
        &plan,
        &inputs,
        &programs,
        &browser,
        &limits,
        false,
        &held.file,
        &held,
        &stage,
        || {
            checks += 1;
            Ok(())
        },
    )
    .unwrap();
    assert!(checks > 2);
    assert_eq!(compared.master(), &held.file);
    assert_eq!(
        compared.manifest(),
        &Object::from_bytes(&plan.manifest_bytes)
    );
    assert_eq!(compared.frames(), 6);
    assert_eq!(compared.samples(), 12000);
    assert_eq!(compared.audio_sha256().len(), 64);
    assert_eq!(compared.picture_sha256().0.len(), 64);
    assert_eq!(stage.read_dir().unwrap().count(), 0);
    let wrong: Object = serde_json::from_value(different.clone()).unwrap();
    let wrong = Held {
        file: FileRef {
            path: "wrong-browser-master.mkv".into(),
            sha256: wrong.sha256.as_str().into(),
            bytes: wrong.bytes,
        },
        guard: cache.open_verified(&wrong).unwrap(),
    };
    let refused = reel::tig_browser_reference::compare_master_to_primaries(
        &plan,
        &inputs,
        &programs,
        &browser,
        &limits,
        false,
        &wrong.file,
        &wrong,
        &stage,
        || Ok(()),
    );
    let refused = refused
        .err()
        .expect("different browser master was accepted");
    assert!(
        format!("{refused:#}").contains("differs from fresh primary reference"),
        "unexpected different-master refusal: {refused:#}"
    );
    assert_eq!(stage.read_dir().unwrap().count(), 0);
    let refused = reel::tig_browser_reference::compare_master_to_primaries(
        &plan,
        &inputs,
        &programs,
        &browser,
        &limits,
        false,
        &held.file,
        &held,
        &stage,
        || anyhow::bail!("source ownership changed"),
    );
    assert!(
        refused
            .err()
            .unwrap()
            .to_string()
            .contains("source ownership changed")
    );
    assert_eq!(stage.read_dir().unwrap().count(), 0);
    super::browser_owned::exercise(f, profile, master, &programs, &browser, &limits);
}
