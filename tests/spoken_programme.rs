use reel::spoken_programme::{self, Document};
use serde_json::{Value, json};

#[path = "support/spoken_compilation.rs"]
mod compilation;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/spoken-programme/four-editions.json")).unwrap()
}

fn parse(value: &Value) -> anyhow::Result<Document> {
    spoken_programme::parse(&serde_json::to_vec(value).unwrap())
}

#[test]
fn validates_four_editions_and_multicue_modules() {
    let base = fixture();
    let document = parse(&base).unwrap();
    let report = spoken_programme::validate(&document).unwrap();
    assert_eq!(
        (report.modules, report.performances, report.editions),
        (3, 6, 4)
    );
    assert!(!report.media_verified && !report.authority_verified);

    // A module can contain another speaker cue before M06 adds measured assembly coverage.
    let mut multi = base.clone();
    let mut cue = multi["modules"][0]["cues"][0].clone();
    cue["id"] = json!("opening-response");
    cue["speaker_id"] = json!("expert");
    multi["modules"][0]["cues"]
        .as_array_mut()
        .unwrap()
        .push(cue);
    for language in ["en", "es"] {
        let mut p = multi["performances"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == format!("{language}-opening"))
            .unwrap()
            .clone();
        p["id"] = json!(format!("{language}-response"));
        p["cue_id"] = json!("opening-response");
        p["scope"]["speaker_id"] = json!("expert");
        multi["performances"].as_array_mut().unwrap().push(p);
    }
    for edition in multi["editions"].as_array_mut().unwrap() {
        let language = edition["language"].as_str().unwrap().to_owned();
        let end = edition["selections"][0]["takes"][0]["end_sample"].clone();
        edition["selections"][0]["takes"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "performance_id":format!("{language}-response"),"start_sample":0,"end_sample":end
            }));
        edition["joins"][0]["from"] = json!(format!("{language}-response"));
        edition["joins"].as_array_mut().unwrap().insert(
            0,
            json!({
                "from":format!("{language}-opening"),"to":format!("{language}-response"),
                "added_pause_samples":12000,"overlap_samples":0
            }),
        );
    }
    assert_eq!(parse(&multi).unwrap().performances.len(), 8);
}

#[test]
fn audience_script_variants_in_one_language_remain_explicit() {
    use sha2::{Digest, Sha256};
    let mut value = fixture();
    let text = "Reviewed speech can serve multiple briefings.";
    let hash = Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let mut script = value["modules"][0]["cues"][0]["scripts"][0].clone();
    script["id"] = json!("en-executive");
    script["text"] = json!(text);
    script["sha256"] = json!(hash);
    value["modules"][0]["cues"][0]["scripts"]
        .as_array_mut()
        .unwrap()
        .push(script);
    let mut performance = value["performances"][0].clone();
    performance["id"] = json!("en-opening-executive");
    performance["script_id"] = json!("en-executive");
    performance["script_sha256"] = json!(hash);
    performance["scope"]["edition_ids"] = json!(["en-executive"]);
    value["performances"]
        .as_array_mut()
        .unwrap()
        .push(performance);
    value["editions"][0]["selections"][0]["takes"][0]["performance_id"] =
        json!("en-opening-executive");
    value["editions"][0]["joins"][0]["from"] = json!("en-opening-executive");
    assert!(parse(&value).is_ok());
    value["performances"][6]["script_id"] = json!("en-standard");
    assert!(parse(&value).unwrap_err().to_string().contains("script"));
}

#[test]
fn rejects_unknown_duplicate_stale_and_unscoped_declarations() {
    let mut v = fixture();
    v["unknown"] = json!(true);
    assert!(parse(&v).is_err());
    let mut v = fixture();
    v["performances"][1]["id"] = v["performances"][0]["id"].clone();
    assert!(parse(&v).unwrap_err().to_string().contains("duplicate"));
    let mut v = fixture();
    v["modules"][0]["cues"][0]["scripts"][0]["text"] = json!("silently changed script");
    assert!(
        parse(&v)
            .unwrap_err()
            .to_string()
            .contains("script identity")
    );
    let mut v = fixture();
    v["performances"][0]["scope"]["edition_ids"] = json!(["en-technical"]);
    assert!(parse(&v).unwrap_err().to_string().contains("scope"));
    let mut v = fixture();
    v["performances"][0]["scope"]["speaker_id"] = json!("someone-else");
    assert!(parse(&v).is_err());
    let mut v = fixture();
    v["performances"][0]["script_sha256"] = json!("0".repeat(64));
    assert!(parse(&v).is_err());
}

#[test]
fn rejects_context_trim_marker_and_handoff_errors() {
    let mut v = fixture();
    v["modules"][2]["requires"] = json!(["detail"]);
    assert!(
        parse(&v)
            .unwrap_err()
            .to_string()
            .contains("required context")
    );
    let mut v = fixture();
    v["editions"][0]["selections"][0]["takes"][0]["start_sample"] = json!(2401);
    assert!(parse(&v).unwrap_err().to_string().contains("protected"));
    let mut v = fixture();
    v["performances"][0]["markers"] = json!([{
        "id":"bad","kind":"word","start_sample":143999,"end_sample":144001
    }]);
    assert!(
        parse(&v)
            .unwrap_err()
            .to_string()
            .contains("marker interval")
    );
    let mut v = fixture();
    v["editions"][0]["joins"][0]["overlap_samples"] = json!(2400);
    assert!(parse(&v).unwrap_err().to_string().contains("handoff"));
    v["editions"][0]["joins"][0]["added_pause_samples"] = json!(0);
    assert!(parse(&v).unwrap_err().to_string().contains("protected"));
    let mut v = fixture();
    v["editions"][0]["joins"] = json!([]);
    assert!(parse(&v).unwrap_err().to_string().contains("adjacency"));
}

#[test]
fn explicit_overlap_accepts_only_unprotected_selected_regions() {
    let mut value = fixture();
    // This variant deliberately has no protected ending on the outgoing signal.
    value["performances"][0]["protected"] = json!([]);
    value["editions"][0]["joins"][0]["added_pause_samples"] = json!(0);
    value["editions"][0]["joins"][0]["overlap_samples"] = json!(2400);
    assert!(parse(&value).is_ok());
    // One sample further enters the incoming protected word region.
    value["editions"][0]["joins"][0]["overlap_samples"] = json!(2401);
    assert!(parse(&value).unwrap_err().to_string().contains("protected"));
}

#[test]
fn strict_json_budget_and_portable_reports_hide_private_canaries() {
    assert!(
        spoken_programme::parse(&vec![b' '; spoken_programme::MAX_DECLARATION_BYTES + 1]).is_err()
    );
    let duplicate = br#"{"schema":"a","schema":"b"}"#;
    assert!(spoken_programme::parse(duplicate).is_err());
    let mut v = fixture();
    v["id"] = json!("private-canary-identity");
    let doc = parse(&v).unwrap();
    let report = serde_json::to_string(&spoken_programme::validate(&doc).unwrap()).unwrap();
    assert!(!report.contains("private-canary"));
    v["performances"][0]["source"]["path"] = json!("C:/private-canary/secret.wav");
    let error = parse(&v).unwrap_err().to_string();
    assert!(!error.contains("private-canary"));
    assert!(!error.contains("secret.wav"));
    assert!(error.contains("reference name"));
}

#[test]
fn declaration_validation_never_opens_referenced_inputs_or_creates_output() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("edition.json");
    std::fs::write(&path, serde_json::to_vec(&fixture()).unwrap()).unwrap();
    // All input paths are nonexistent. Validation has no input-provider or runtime port.
    assert!(spoken_programme::load(&path).is_ok());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[test]
fn rejects_cycles_wrong_language_unknown_inputs_and_invalid_pins() {
    let mutations = [
        (
            "/editions/0/required_modules",
            json!(["detail"]),
            "selection",
        ),
        ("/modules/0/requires", json!(["closing"]), "cycle"),
        ("/editions/0/language", json!("es"), "scope"),
        (
            "/editions/0/selections/0/module_id",
            json!("absent"),
            "selection",
        ),
        ("/performances/0/duration_samples", json!(0), "duration"),
        ("/performances/0/source/sha256", json!("invalid"), "pin"),
        ("/performances/0/source/bytes", json!(0), "pin"),
        ("/editions/0/joins/0/to", json!("wrong"), "handoff"),
    ];
    for (pointer, replacement, diagnostic) in mutations {
        let mut value = fixture();
        *value.pointer_mut(pointer).unwrap() = replacement;
        let error = parse(&value).unwrap_err().to_string();
        assert!(error.contains(diagnostic), "{pointer}: {error}");
    }
}

#[test]
fn cli_validates_declarations_without_reading_media_and_redacts_errors() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("private-canary-declaration.json");
    std::fs::write(&path, serde_json::to_vec(&fixture()).unwrap()).unwrap();
    let run = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_reel"))
            .arg("spoken-edition-validate")
            .arg(&path)
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["editions"], 4);
    assert_eq!(report["media_verified"], false);
    std::fs::write(&path, br#"{"private-canary":"bad"}"#).unwrap();
    let output = run();
    assert!(!output.status.success());
    for bytes in [&output.stdout, &output.stderr] {
        assert!(!String::from_utf8_lossy(bytes).contains("private-canary"));
    }
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
}
