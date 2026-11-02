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
