use super::*;
use reel::scene_delivery::Receipt;
use tig_store::metadata::MetadataReader;

fn cached(cache: &Path, path: &Path) -> Object {
    let bytes = fs::read(path).unwrap();
    let object = Object::from_bytes(&bytes);
    let parent = cache
        .join("objects/sha256")
        .join(&object.sha256.as_str()[..2]);
    fs::create_dir_all(&parent).unwrap();
    let target = parent.join(object.sha256.as_str());
    if !target.exists() {
        fs::write(target, bytes).unwrap();
    }
    object
}
fn clean(cache: &Path) {
    assert!(!cache.read_dir().unwrap().any(|entry| {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        name.starts_with("native-prepare-") || name.starts_with("native-compare-")
    }));
}
pub fn exercise(
    root: &Path,
    profile: &Value,
    completed: &Value,
    reference: &Receipt,
    cache: &Path,
) {
    let retained = &completed["result"]["outputs"];
    let outputs: serde_json::Map<_, _> = retained
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(name, value)| {
            name.strip_prefix("native/")
                .filter(|name| *name != "receipt.json")
                .map(|name| {
                    (
                        name.to_owned(),
                        json!({"sha256":value["sha256"],"bytes":value["bytes"]}),
                    )
                })
        })
        .collect();
    let baseline_path = root
        .join("compiled-cli-reference-output")
        .join(&reference.outputs["master.mkv"].path);
    let baseline = cached(cache, &baseline_path);
    let input = root.join("retained-comparison-profile.json");
    let report = root.join("retained-comparison-report.json");
    let binary = Path::new(env!("CARGO_BIN_EXE_reel-tig-job"));
    let comparison = json!({"schema":"reel.tig-retained-comparison.experimental-1",
        "metadata_root":profile["worker"]["metadata_root"],
        "metadata_minimum":profile["worker"]["metadata_minimum"],
        "source":profile["worker"]["source"],"context":profile["worker"]["context"],
        "request":profile["worker"]["request"],"producer":completed["planning"]["producer"],
        "claim":completed["record_candidates"]["claim"],"receipt":completed["record_candidates"]["receipt"],
        "outputs":outputs,"reference":baseline,"runtime":profile["worker"]["runtime"],
        "byte_source":{"places":[{"root":cache,"layout":"reel-cas"}],"required":"prevent-mutation"},
        "cache":profile["cache"],"preparation_cache":cache,"max_output_bytes":134217728_u64});
    // The validator is this independently pinned executable, not whichever host
    // originally rendered the scene. Preserve producer and all original claims.
    let mut comparison = comparison;
    comparison["runtime"]["host"] =
        json!(reel::media_runtime::ByteIdentity::observe_file(binary).unwrap());
    let minimum = serde_json::from_value(profile["worker"]["metadata_minimum"].clone()).unwrap();
    let metadata = Path::new(profile["worker"]["metadata_root"].as_str().unwrap());
    let before = MetadataReader::open(metadata, &minimum)
        .unwrap()
        .checkpoint()
        .clone();
    let invoke = |value: &Value, destination: &Path| {
        fs::write(&input, serde_json::to_vec(value).unwrap()).unwrap();
        Command::new(binary)
            .arg("compare-retained")
            .arg(&input)
            .arg(destination)
            .env("PATH", "")
            .output()
            .unwrap()
    };
    // Source Git/records/definitions are still physically offline here. All
    // validation tools must come from their held declarations, never PATH.
    let checked = invoke(&comparison, &report);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let summary: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(summary["equal"], true);
    assert_eq!(summary["native_checked"], true);
    assert_eq!(summary["materialized_bytes"], 0);
    assert!(!String::from_utf8_lossy(&checked.stdout).contains("sha256"));
    let evidence: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(evidence["parity"]["native_clock_checked"], true);
    assert_eq!(evidence["parity"]["approval_transferred"], false);
    assert_eq!(evidence["execution_attested"], false);
    assert_eq!(evidence["producer"], comparison["producer"]);
    let producer_host = &profile["worker"]["runtime"]["host"];
    if producer_host != &comparison["runtime"]["host"] {
        assert_ne!(evidence["producer"], evidence["validator"]);
    }
    clean(cache);
    // Receipt-derived v2 keeps one manifest and shares one charged reader across
    // claim, receipt, baseline and all unique native outputs. Exact allowance
    // succeeds; one fewer request must refuse, rather than resetting a cache.
    let mut derived = comparison.clone();
    derived["schema"] = json!("reel.tig-retained-comparison.experimental-2");
    derived.as_object_mut().unwrap().remove("outputs");
    let mut unique = std::collections::BTreeMap::new();
    let mut named_bytes = comparison["receipt"]["bytes"].as_u64().unwrap();
    for object in comparison["outputs"].as_object().unwrap().values() {
        unique.insert(
            object["sha256"].as_str().unwrap(),
            object["bytes"].as_u64().unwrap(),
        );
        named_bytes += object["bytes"].as_u64().unwrap();
    }
    let requests = unique.len() as u32 + 3;
    let verified_bytes = unique.values().sum::<u64>()
        + comparison["claim"]["bytes"].as_u64().unwrap()
        + comparison["receipt"]["bytes"].as_u64().unwrap()
        + baseline.bytes;
    derived["max_output_bytes"] = json!(named_bytes);
    derived["byte_source"]["limits"] = json!({"requests":requests,"total_bytes":verified_bytes});
    let derived_report = root.join("receipt-derived-comparison.json");
    let checked = invoke(&derived, &derived_report);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let summary: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(summary["materialized_bytes"], 0);
    assert_eq!(summary["verified_output_bytes"], named_bytes);
    let derived_evidence: Value =
        serde_json::from_slice(&fs::read(derived_report).unwrap()).unwrap();
    assert_eq!(
        derived_evidence["schema"],
        "reel.tig-retained-comparison-report.experimental-2"
    );
    assert!(derived_evidence.get("outputs").is_none());
    assert_eq!(derived_evidence["output_manifest"], comparison["receipt"]);
    assert_eq!(
        derived_evidence["output_count"],
        evidence["outputs"].as_object().unwrap().len()
    );
    for case in ["requests", "bytes", "duplicate-map", "missing-v1-map"] {
        let mut denied = derived.clone();
        match case {
            "requests" => denied["byte_source"]["limits"]["requests"] = json!(requests - 1),
            "bytes" => denied["byte_source"]["limits"]["total_bytes"] = json!(verified_bytes - 1),
            "duplicate-map" => denied["outputs"] = comparison["outputs"].clone(),
            _ => denied["schema"] = comparison["schema"].clone(),
        }
        let output = root.join(format!("refused-derived-{case}.json"));
        assert!(!invoke(&denied, &output).status.success());
        assert!(!output.exists());
        clean(cache);
    }
    let original_report = fs::read(&report).unwrap();
    assert!(!invoke(&comparison, &report).status.success());
    assert_eq!(fs::read(&report).unwrap(), original_report);
    for case in [
        "budget", "context", "producer", "output", "missing", "receipt",
    ] {
        let mut bad = comparison.clone();
        match case {
            "budget" => bad["max_output_bytes"] = json!(1),
            "context" => {
                bad["context"]["context"] = json!(tig_core::Digest::blob(b"other context"))
            }
            "producer" => bad["producer"] = json!(Object::from_bytes(b"other producer")),
            "output" => bad["outputs"]["master.mkv"] = bad["outputs"]["picture.mkv"].clone(),
            "missing" => {
                bad["outputs"].as_object_mut().unwrap().remove("master.mkv");
            }
            _ => bad["receipt"] = bad["claim"].clone(),
        }
        let destination = root.join(format!("refused-comparison-{case}.json"));
        let refused = invoke(&bad, &destination);
        assert!(!refused.status.success());
        assert!(!destination.exists());
        assert!(!String::from_utf8_lossy(&refused.stderr).contains(&root.display().to_string()));
        clean(cache);
    }
    let master_path = Path::new(retained["native/master.mkv"]["path"].as_str().unwrap());
    let original = fs::read(master_path).unwrap();
    fs::write(master_path, vec![0u8; original.len()]).unwrap();
    let corrupt = invoke(&comparison, &root.join("corrupt-comparison.json"));
    fs::write(master_path, original).unwrap();
    assert!(!corrupt.status.success());
    assert!(!root.join("corrupt-comparison.json").exists());
    clean(cache);
    // Valid native geometry/clock with different pixels: preserve the report and
    // return failure, while never turning the baseline into a production input.
    let different = root.join("different-comparison-baseline.mkv");
    let generated = Command::new(
        profile["worker"]["runtime"]["ffmpeg"]["path"]
            .as_str()
            .unwrap(),
    )
    .args(["-v", "error", "-nostdin", "-i"])
    .arg(&baseline_path)
    .args([
        "-vf", "negate", "-c:v", "ffv1", "-pix_fmt", "yuv444p", "-c:a", "copy",
    ])
    .arg(&different)
    .output()
    .unwrap();
    assert!(generated.status.success());
    let mut mismatch = comparison.clone();
    mismatch["reference"] = json!(cached(cache, &different));
    let destination = root.join("different-comparison-report.json");
    let differed = invoke(&mismatch, &destination);
    assert!(!differed.status.success());
    let evidence: Value = serde_json::from_slice(&fs::read(destination).unwrap()).unwrap();
    assert_eq!(evidence["parity"]["equal"], false);
    assert_eq!(evidence["parity"]["approval_transferred"], false);
    clean(cache);
    assert_eq!(
        MetadataReader::open(metadata, &minimum)
            .unwrap()
            .checkpoint(),
        &before
    );
}
