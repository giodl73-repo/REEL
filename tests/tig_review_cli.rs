#![cfg(feature = "tig-publication")]
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tig_core::Object;

struct Fixture {
    _temp: tempfile::TempDir,
    cache: PathBuf,
    control: PathBuf,
    profile_path: PathBuf,
    profile: Value,
    evidence: Value,
    image: Object,
}
fn put(root: &Path, bytes: &[u8]) -> Object {
    let object = Object::from_bytes(bytes);
    let path = cas(root, &object);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    object
}
fn cas(root: &Path, object: &Object) -> PathBuf {
    let sha = object.sha256.as_str();
    root.join("objects/sha256").join(&sha[..2]).join(sha)
}
fn document(root: &Path, value: &Value) -> Object {
    put(root, &serde_json::to_vec_pretty(value).unwrap())
}
fn png(root: &Path, w: u32, h: u32, color: [u8; 3]) -> Object {
    let mut encoded = Cursor::new(Vec::new());
    image::RgbImage::from_pixel(w, h, image::Rgb(color))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    put(root, &encoded.into_inner())
}
impl Fixture {
    fn new(targets: bool, corrupt_snapshot: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let cache = temp.path().join("cache");
        let control = temp.path().join("control");
        fs::create_dir(&cache).unwrap();
        fs::create_dir(&control).unwrap();
        let large = png(&cache, 4, 3, [10, 20, 30]);
        let small = png(&cache, 2, 1, [10, 20, 30]);
        let method = put(&cache, b"synthetic frozen review method, never executed");
        let programme = put(&cache, b"synthetic programme identity");
        let request_bytes=serde_json::to_vec_pretty(&json!({"inputs":{"programme.file":programme},"outputs":{"large-output":{"path":"large.png","max_bytes":4096},"small-output":{"path":"small.png","max_bytes":4096}}})).unwrap();
        let request = put(&cache, &request_bytes);
        let result_value = json!({"schema":"fixture.result","manifest_sha256":request.sha256,"original_inputs_modified":false,"daemon_started":false,"workspace_retained":true,"outputs":{"large-output":large,"small-output":small}});
        let mut snapshot = json!({"schema":"reel.job-retirement-evidence.v1","request_utf8":String::from_utf8(request_bytes).unwrap(),"result":result_value,"declared_retained_outputs":{"large-output":large,"small-output":small}});
        if corrupt_snapshot {
            snapshot["result"]["daemon_started"] = json!(true);
        }
        let mut compressor = GzEncoder::new(Vec::new(), Compression::default());
        compressor
            .write_all(&serde_json::to_vec(&snapshot).unwrap())
            .unwrap();
        let retirement = put(&cache, &compressor.finish().unwrap());
        let mut terminal_result = result_value.clone();
        terminal_result["workspace_retained"] = json!(false);
        terminal_result["retirement_evidence"] = json!(retirement);
        let result = document(&cache, &terminal_result);
        let root = document(
            &cache,
            &json!({"job":1,"request":request,"retirement_evidence":retirement,"frames":[12],"sample_ids":["one"],"positive_terminal_retired":true,"all_output_SHA_bytes_caps_and_dimensions_verified":true,"human_approval":false}),
        );
        let proof = json!({"job":1,"request":request,"result":result,"root_verified":root,"retirement_evidence":retirement,"frames":[12],"sample_ids":["one"],"workspace_absent":true,"positive_terminal_retired":true,"exact_request_gzip_outputs_tables_verified":true});
        let clock = json!({"sample":{"sample_id":"one","frame":12,"seconds":0.5,"reasons":["synthetic boundary"],"footer":[{"code":"example"}]},"views":{"large":{"sha256":large.sha256,"bytes":large.bytes,"dimensions":[4,3],"native_output_role":"large-output","request":request,"job":1,"canonical_path":"historical-unavailable/large.png"},"small":{"sha256":small.sha256,"bytes":small.bytes,"dimensions":[2,1],"native_output_role":"small-output","request":request,"job":1,"canonical_path":"historical-unavailable/small.png"}},"native_job_proof":proof});
        let group = document(
            &cache,
            &json!({"schema":"fixture.group","episode":"example","language":"xx","group_number":1,"actual_programme":programme,"clocks":[clock]}),
        );
        let mut feature_value = json!({"corrections":[]});
        let mut raw = Vec::new();
        let mut target_observations = Vec::new();
        let mut rules = Vec::new();
        if targets {
            let original = png(&cache, 5, 4, [1, 2, 3]);
            let selected = png(&cache, 6, 4, [4, 5, 6]);
            feature_value["corrections"] = json!([{"code":"example","span":[12,13],"original":original,"selected":selected,"note":{"lines":["Corrección exacta de prueba"]}}]);
            for object in [&original, &selected] {
                raw.push(json!({"sha256":object.sha256,"bytes":object.bytes,"canonical_path":"historical-unavailable/raw.png"}));
            }
            target_observations.push(json!({"code":"example","original":original,"selected":selected,"fix_lines":["Corrección exacta de prueba"],"per_role":{"large":{"visible":false},"small":{"visible":null}}}));
            rules.push(json!({"source_field":"corrections","task_field":"corrections","source_exact_fields":{"fix_lines":"/note/lines"},"observation_field":"corrected","span_field":"span","picture_fields":["original","selected"],"exact_fields":["fix_lines"],"appearance_fields":["visible"]}));
        }
        let features = document(&cache, &feature_value);
        let mut selected_targets = feature_value["corrections"].clone();
        for t in selected_targets.as_array_mut().unwrap() {
            t["fix_lines"] = t["note"]["lines"].clone();
        }
        let task = document(
            &cache,
            &json!({"schema":"fixture.task","scope":"fixture","episode":"example","language":"xx","original_group_number":1,"original_frozen_group":group,"original_clock_indices":[0],"clocks":[clock],"clock_count":1,"image_occurrences":2,"roles":["large","small"],"actual_programme":programme,"actual_Review":null,"current_ASS":null,"raw_feature_source":features,"raw_references":raw,"corrections":selected_targets,"protocol_method":method,"source_group_all_clocks_remain_immutable":true,"technical_AAC_r1_not_final_playback_copy":false,"all_scope_qualification_pending":true,"human_approval":false}),
        );
        let policy = document(
            &cache,
            &json!({"schema":"reel.review-observation-policy.experimental-1","method":method,"task_schema":"fixture.task","group_schema":"fixture.group","observations":{"scope":"fixture","report_schema":"fixture.subreport","max_clocks":3,"footer_fields":["legible"],"footer_events_field":"footer","targets":rules},"role_dimensions":{"large":[4,3],"small":[2,1]},"media_inputs":{"programme.file":"actual_programme"}}),
        );
        let mut evidence = json!({"fresh_observations_not_inherited":true,"finite_scope_qualified_with_holds":false,"human_approval":false,"holds_preserved":["synthetic global hold"],"raw_source_observations":[],"raw_reference_view_occurrences":raw.len(),"observations":[{"sample_id":"one","frame":12,"major_obstruction_observed":false,"minor_advisory":true,"legible":null,"fresh_role_observations":{"large":{"privately_viewed_detail_original":true,"detail":"Synthetic large-view observation"},"small":{"privately_viewed_detail_original":true,"detail":"Synthetic small-view observation"}},"corrected":target_observations}]});
        evidence["raw_source_observations"]=json!(raw.iter().map(|r|json!({"asset":r,"privately_viewed_detail_original":true,"fresh_hash_bytes_verified":true,"detail":"Synthetic raw-reference observation"})).collect::<Vec<_>>());
        let profile = json!({"schema":"reel.review-scope-profile.experimental-1","task":task,"method":method,"policy":policy,"control_root":control,"byte_source":{"places":[{"root":cache,"layout":"reel-cas"}],"required":if cfg!(windows){"prevent-mutation"}else{"cooperative-immutable"},"limits":{"object_bytes":4194304,"total_bytes":67108864,"requests":256,"live_readers":16,"verify_millis":60000}},"state":{"kind":"unseen"},"receipt":null,"evidence":null});
        let profile_path = temp.path().join("profile.json");
        let fixture = Self {
            _temp: temp,
            cache,
            control,
            profile_path,
            profile,
            evidence,
            image: large,
        };
        fixture.save();
        fixture
    }
    fn save(&self) {
        fs::write(
            &self.profile_path,
            serde_json::to_vec_pretty(&self.profile).unwrap(),
        )
        .unwrap();
    }
    fn run(&self, operation: &str) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_reel-tig-review"));
        command.arg(operation).arg(&self.profile_path);
        if operation == "reserve" {
            command.args(["--owner", "synthetic-worker"]);
        }
        command.output().unwrap()
    }
    fn ok(&self, operation: &str) -> Value {
        let output = self.run(operation);
        assert!(
            output.status.success(),
            "{operation}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

#[test]
fn cli_round_trip_recovery_and_counted_skip_preserve_negative_null_holds() {
    let mut f = Fixture::new(false, false);
    assert_eq!(f.ok("inspect")["state"], "metadata-verified-unreserved");
    assert_eq!(fs::read_dir(&f.control).unwrap().count(), 0);
    let reserved = f.ok("reserve");
    assert!(!f.run("reserve").status.success());
    f.profile["receipt"] = reserved["receipt"].clone();
    f.save();
    assert_eq!(f.ok("resume")["automatic_view_permission"], false);
    f.profile["evidence"] = json!(document(&f.cache, &f.evidence));
    f.save();
    let submitted = f.ok("submit");
    assert_eq!(submitted["finite_scope_qualified_with_holds"], false);
    let report_path = PathBuf::from(submitted["report_path"].as_str().unwrap());
    let report_bytes = fs::read(&report_path).unwrap();
    let report: Value = serde_json::from_slice(&report_bytes).unwrap();
    assert!(report["observations"][0]["legible"].is_null());
    assert_eq!(report["holds_preserved"], json!(["synthetic global hold"]));
    assert_eq!(f.ok("submit")["state"], "completed-validated-skip");
    fs::remove_file(report_path.parent().unwrap().join("complete.json")).unwrap();
    assert_eq!(f.ok("resume")["state"], "completed-validated-skip");
    assert_eq!(fs::read(&report_path).unwrap(), report_bytes);
    let retained = put(&f.cache, &report_bytes);
    let other_control = f._temp.path().join("counted-control");
    fs::create_dir(&other_control).unwrap();
    f.profile["control_root"] = json!(other_control);
    f.profile["state"] = json!({"kind":"counted","report":retained});
    f.profile["receipt"] = Value::Null;
    f.profile["evidence"] = Value::Null;
    f.save();
    let skipped = f.ok("reserve");
    assert_eq!(skipped["new_image_views"], 0);
    assert_eq!(skipped["new_receipts"], 0);
    assert_eq!(fs::read_dir(&other_control).unwrap().count(), 0);
    let task: Value = serde_json::from_slice(
        &fs::read(cas(
            &f.cache,
            &serde_json::from_value(f.profile["task"].clone()).unwrap(),
        ))
        .unwrap(),
    )
    .unwrap();
    let config = json!({"schema":"reel.review-aggregation-profile.experimental-1","method":f.profile["method"],"policy":f.profile["policy"],"group":task["original_frozen_group"],"control_root":other_control,"byte_source":f.profile["byte_source"],"parts":[{"task":f.profile["task"],"report":retained}],"require_complete":true});
    let config_path = f._temp.path().join("aggregate-profile.json");
    fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let output_path = other_control.join("aggregate.json");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_reel-tig-review"))
            .arg("aggregate")
            .arg(&config_path)
            .arg(&output_path)
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["group_coverage_complete"], true);
    assert_eq!(summary["new_image_views"], 0);
    let report: Value = serde_json::from_slice(&fs::read(&output_path).unwrap()).unwrap();
    assert_eq!(report["finite_scope_qualified_with_holds"], false);
    assert_eq!(report["source_subreports"], json!([retained]));
    assert_eq!(report["human_approval"], false);
    assert!(run().status.success());
    let mut duplicate = config.clone();
    duplicate["parts"]
        .as_array_mut()
        .unwrap()
        .push(config["parts"][0].clone());
    fs::write(&config_path, serde_json::to_vec_pretty(&duplicate).unwrap()).unwrap();
    assert!(!run().status.success());
}
#[test]
fn cli_optional_raw_targets_keep_exact_text_sources_and_nullable_facts() {
    let mut f = Fixture::new(true, false);
    assert_eq!(f.ok("inspect")["verified_unique_images"], 4);
    let reserved = f.ok("reserve");
    f.profile["receipt"] = reserved["receipt"].clone();
    let mut bad = f.evidence.clone();
    bad["observations"][0]["corrected"][0]["fix_lines"] = json!(["changed"]);
    f.profile["evidence"] = json!(document(&f.cache, &bad));
    f.save();
    assert!(!f.run("submit").status.success());
    f.profile["evidence"] = json!(document(&f.cache, &f.evidence));
    f.save();
    let submitted = f.ok("submit");
    let report: Value =
        serde_json::from_slice(&fs::read(submitted["report_path"].as_str().unwrap()).unwrap())
            .unwrap();
    assert_eq!(report["raw_reference_view_occurrences"], 2);
    assert_eq!(
        report["observations"][0]["corrected"][0]["per_role"]["large"]["visible"],
        false
    );
    assert!(report["observations"][0]["corrected"][0]["per_role"]["small"]["visible"].is_null());
}
#[test]
fn cli_corrupt_or_changed_sources_refuse_before_receipts() {
    let f = Fixture::new(false, true);
    assert!(!f.run("reserve").status.success());
    assert_eq!(fs::read_dir(&f.control).unwrap().count(), 0);
    let f = Fixture::new(false, false);
    let path = cas(&f.cache, &f.image);
    let mut bytes = fs::read(&path).unwrap();
    bytes[20] ^= 1;
    fs::write(path, bytes).unwrap();
    assert!(!f.run("reserve").status.success());
    assert_eq!(fs::read_dir(&f.control).unwrap().count(), 0);
    let mut f = Fixture::new(false, false);
    f.profile["method"] = json!(put(&f.cache, b"changed method"));
    f.save();
    assert!(!f.run("reserve").status.success());
    assert_eq!(fs::read_dir(&f.control).unwrap().count(), 0);
}

#[test]
fn concurrent_cli_reservations_have_one_winner_and_method_binding_is_immutable() {
    use std::process::Stdio;
    let mut f = Fixture::new(false, false);
    let spawn = || {
        Command::new(env!("CARGO_BIN_EXE_reel-tig-review"))
            .arg("reserve")
            .arg(&f.profile_path)
            .args(["--owner", "synthetic-worker"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let a = spawn();
    let b = spawn();
    let results = [a.wait_with_output().unwrap(), b.wait_with_output().unwrap()];
    assert_eq!(results.iter().filter(|r| r.status.success()).count(), 1);
    let reserved: Value =
        serde_json::from_slice(&results.iter().find(|r| r.status.success()).unwrap().stdout)
            .unwrap();
    f.profile["receipt"] = reserved["receipt"].clone();
    f.save();
    assert_eq!(f.ok("resume")["automatic_view_permission"], false);
    let policy_pin: Object = serde_json::from_value(f.profile["policy"].clone()).unwrap();
    let mut policy: Value =
        serde_json::from_slice(&fs::read(cas(&f.cache, &policy_pin)).unwrap()).unwrap();
    policy["observations"]["max_clocks"] = json!(2);
    f.profile["policy"] = json!(document(&f.cache, &policy));
    f.save();
    assert!(!f.run("resume").status.success());
}
