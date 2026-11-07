use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tig_core::Object;
use tig_store::{metadata::MetadataReader, source_claims::SourceJournal};

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
