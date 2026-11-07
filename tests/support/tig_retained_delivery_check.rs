use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tig_core::Object;
use tig_store::{metadata::MetadataReader, source_claims::SourceJournal};

/// An actual in-process renderer and the checking CLI have different host byte
/// identities. The checker must bind itself without rewriting that render recipe.
pub fn exercise_other_producer(
    root: &Path,
    frozen: &Value,
    candidate: &reel::tig_scene_production::RenderedCandidate,
    output: &Path,
    renderer_runtime: &reel::media_runtime::Bindings,
) {
    use reel::media_runtime::ByteIdentity;
    let cache = root.join("other-producer-check-cache");
    fs::create_dir_all(cache.join("objects/sha256")).unwrap();
    let coordination = root.join("other-producer-check-coordination");
    fs::create_dir(&coordination).unwrap();
    let store = |path: &Path| {
        let raw = fs::read(path).unwrap();
        let object = Object::from_bytes(&raw);
        let parent = cache
            .join("objects/sha256")
            .join(&object.sha256.as_str()[..2]);
        fs::create_dir_all(&parent).unwrap();
        fs::write(parent.join(object.sha256.as_str()), raw).unwrap();
        object
    };
    let receipt = store(&output.join("receipt.json"));
    assert_eq!(&receipt, candidate.receipt_object());
    let outputs = candidate
        .receipt()
        .outputs
        .iter()
        .map(|(name, file)| (name.clone(), store(&output.join(&file.path))))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut runtime = renderer_runtime.clone();
    runtime.host =
        ByteIdentity::observe_file(Path::new(env!("CARGO_BIN_EXE_reel-tig-job"))).unwrap();
    assert_ne!(runtime.host, renderer_runtime.host);
    let profile = json!({"schema":"reel.tig-retained-delivery-check.experimental-1",
        "metadata_root":frozen["metadata_root"],"metadata_minimum":frozen["metadata_minimum"],
        "source":frozen["source"],"context":frozen["context"],"request":frozen["request"],
        "runtime":runtime,"producer":candidate.recipe().operation().producer,
        "recipe":candidate.recipe().fingerprint(),"receipt":receipt,"outputs":outputs,
        "byte_source":{"required":"prevent-mutation","places":[{"root":cache,"layout":"reel-cas"}]},
        "cache":{"path_policy":"configured-local-roots","cache_roots":[cache],"coordination_root":coordination,
            "capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}},
        "preparation_cache":cache,"max_output_bytes":256*1024*1024_u64});
    let path = root.join("other-producer-check.json");
    let report = root.join("other-producer-check-report.json");
    fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();
    let metadata = Path::new(frozen["metadata_root"].as_str().unwrap());
    let minimum = serde_json::from_value(frozen["metadata_minimum"].clone()).unwrap();
    let before = MetadataReader::open(metadata, &minimum)
        .unwrap()
        .checkpoint()
        .clone();
    let result = Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
        .arg("check-retained-delivery")
        .arg(path)
        .arg(&report)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let checked: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(checked["native_checked"], true);
    assert_eq!(checked["producer_admission"], profile["producer"]);
    assert_ne!(checked["validator"], checked["producer_admission"]);
    assert_eq!(checked["execution_attested"], false);
    assert!(checked["production_claim"].is_null());
    assert_eq!(
        before,
        MetadataReader::open(metadata, &before)
            .unwrap()
            .checkpoint()
            .clone()
    );
}

pub fn exercise(root: &Path, original: &Value, completed: &Value, cache: &Path) {
    let worker: Value = serde_json::from_slice(
        &fs::read(
            completed["result"]["outputs"]["worker.json"]["path"]
                .as_str()
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let outputs = completed["result"]["outputs"].as_object().unwrap();
    let pin = |value: &Value| json!({"sha256":value["sha256"],"bytes":value["bytes"]});
    let media = outputs
        .iter()
        .filter_map(|(name, file)| {
            name.strip_prefix("native/")
                .filter(|name| *name != "receipt.json")
                .map(|name| (name.to_owned(), pin(file)))
        })
        .collect::<serde_json::Map<_, _>>();
    let mut profile = json!({"schema":"reel.tig-retained-delivery-check.experimental-1",
        "metadata_root":original["worker"]["metadata_root"],"metadata_minimum":original["worker"]["metadata_minimum"],
        "source":original["worker"]["source"],"context":original["worker"]["context"],
        "request":original["worker"]["request"],"runtime":original["worker"]["runtime"],
        "producer":worker["producer"],"recipe":worker["recipe"],
        "receipt":pin(&outputs["native/receipt.json"]),"outputs":media,
        "byte_source":{"required":"prevent-mutation","places":[{"root":cache,"layout":"reel-cas"}]},
        "cache":original["cache"],"preparation_cache":cache,"max_output_bytes":256*1024*1024_u64});
    let metadata = Path::new(original["worker"]["metadata_root"].as_str().unwrap());
    let minimum = serde_json::from_value(original["worker"]["metadata_minimum"].clone()).unwrap();
    let before = MetadataReader::open(metadata, &minimum)
        .unwrap()
        .checkpoint()
        .clone();
    let journal_root = Path::new(original["journal_root"].as_str().unwrap());
    let journal_minimum = serde_json::from_value(completed["journal_checkpoint"].clone()).unwrap();
    let before_journal = SourceJournal::open(journal_root, &journal_minimum)
        .unwrap()
        .checkpoint();
    let path = root.join("retained-delivery-check.json");
    let run = |profile: &Value, name: &str, pass: bool| {
        fs::write(&path, serde_json::to_vec(profile).unwrap()).unwrap();
        let report = root.join(format!("retained-delivery-{name}.json"));
        let result = Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
            .arg("check-retained-delivery")
            .arg(&path)
            .arg(&report)
            .output()
            .unwrap();
        assert_eq!(
            result.status.success(),
            pass,
            "{name}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        if pass {
            let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
            assert_eq!(summary["native_checked"], true);
            assert_eq!(summary["materialized_bytes"], 0);
            assert_eq!(summary["execution_attested"], false);
            assert!(summary["production_claim"].is_null());
            let report: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
            assert_eq!(report["recipe"], worker["recipe"]);
        } else {
            assert!(!report.exists());
        }
    };
    // The source Git/records are already offline in this fixture. The checker
    // needs only the frozen archive and retained media, with no producer claim.
    run(&profile, "exact", true);
    let success_report = root.join("retained-delivery-exact.json");
    let saved = fs::read(&success_report).unwrap();
    fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();
    assert!(
        !Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
            .arg("check-retained-delivery")
            .arg(&path)
            .arg(&success_report)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(saved, fs::read(success_report).unwrap());
    for case in [
        "recipe",
        "producer",
        "context",
        "request",
        "output-pin",
        "missing-output",
        "claim",
        "budget",
    ] {
        let mut bad = profile.clone();
        let foreign = serde_json::to_value(Object::from_bytes(b"foreign candidate")).unwrap();
        match case {
            "recipe" => bad["recipe"] = foreign["sha256"].clone(),
            "producer" => bad["producer"] = foreign,
            "context" => bad["context"]["root"] = foreign,
            "request" => bad["request"]["root_id"] = json!("foreign/source"),
            "output-pin" => bad["outputs"]["master.mkv"] = foreign,
            "missing-output" => {
                bad["outputs"].as_object_mut().unwrap().remove("master.mkv");
            }
            "claim" => bad["claim"] = foreign,
            "budget" => bad["max_output_bytes"] = json!(1),
            _ => unreachable!(),
        }
        run(&bad, case, false);
    }
    let master = Path::new(outputs["native/master.mkv"]["path"].as_str().unwrap());
    let saved = fs::read(master).unwrap();
    fs::write(master, b"corrupt retained media").unwrap();
    run(&profile, "corrupt-media", false);
    fs::write(master, saved).unwrap();
    // No hidden claim, selection or source-owner state may be created.
    assert_eq!(
        before,
        MetadataReader::open(metadata, &before)
            .unwrap()
            .checkpoint()
            .clone()
    );
    assert_eq!(
        before_journal,
        SourceJournal::open(journal_root, &before_journal)
            .unwrap()
            .checkpoint()
    );
    profile["request"]["unexpected"] = json!(true);
    run(&profile, "unknown-request-field", false);
}
