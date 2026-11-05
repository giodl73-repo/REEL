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

const ROLES: [&str; 4] = [
    "programme_native",
    "programme_true390",
    "Review_native",
    "Review_true390",
];
const FOOTERS: [&str; 6] = [
    "code_readable_Review_native",
    "code_readable_Review_true390",
    "code_wholly_footer_Review_native",
    "code_wholly_footer_Review_true390",
    "code_matches_declared_events_Review_native",
    "code_matches_declared_events_Review_true390",
];
fn cas(root: &Path, object: &Object) -> PathBuf {
    let sha = object.sha256.as_str();
    root.join("objects/sha256").join(&sha[..2]).join(sha)
}
fn put(root: &Path, raw: &[u8]) -> Object {
    let object = Object::from_bytes(raw);
    let path = cas(root, &object);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, raw).unwrap();
    object
}
fn document(root: &Path, value: &Value) -> Object {
    put(root, &serde_json::to_vec(value).unwrap())
}
fn png(root: &Path, w: u32, h: u32, color: [u8; 3]) -> Object {
    let mut encoded = Cursor::new(Vec::new());
    image::RgbImage::from_pixel(w, h, image::Rgb(color))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    put(root, &encoded.into_inner())
}
struct Fixture {
    _temp: tempfile::TempDir,
    cache: PathBuf,
    control: PathBuf,
    path: PathBuf,
    profile: Value,
    evidence: Value,
    task_bytes: Vec<u8>,
    index: Object,
    image: Object,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let cache = temp.path().join("cache");
        let control = temp.path().join("control");
        fs::create_dir(&cache).unwrap();
        fs::create_dir(&control).unwrap();
        let large = png(&cache, 4, 3, [10, 20, 30]);
        let small = png(&cache, 2, 1, [10, 20, 30]);
        let original_picture = png(&cache, 5, 4, [1, 2, 3]);
        let selected_picture = png(&cache, 6, 4, [4, 5, 6]);
        let identity = document(
            &cache,
            &json!({"identity_only":true,"human_approval":false}),
        );
        let programme = put(&cache, b"synthetic programme");
        let review = put(&cache, b"synthetic Review");
        let ass = put(&cache, b"synthetic declared footer source");
        let method = put(
            &cache,
            include_bytes!("fixtures/indexed-review-protocol.json"),
        );
        let mut outputs = json!({});
        let mut objects = json!({});
        let mut geometry = json!({});
        for role in ROLES {
            let phone = role.contains("true390");
            outputs[format!("{role}-output")] =
                json!({"path":format!("{role}.png"),"max_bytes":4096});
            objects[format!("{role}-output")] = json!(if phone { &small } else { &large });
            geometry[role] = if phone { json!([2, 1]) } else { json!([4, 3]) };
        }
        let request_bytes = serde_json::to_vec(
            &json!({"inputs":{"programme.mp4":programme,"Review.mp4":review},"outputs":outputs}),
        )
        .unwrap();
        let request = put(&cache, &request_bytes);
        let initial_result = json!({"schema":"fixture.result","manifest_sha256":request.sha256,"original_inputs_modified":false,"daemon_started":false,"workspace_retained":true,"outputs":objects});
        let snapshot = json!({"schema":"reel.job-retirement-evidence.v1","request_utf8":String::from_utf8(request_bytes).unwrap(),"result":initial_result,"declared_retained_outputs":objects});
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(&serde_json::to_vec(&snapshot).unwrap())
            .unwrap();
        let retirement = put(&cache, &gz.finish().unwrap());
        let mut terminal = initial_result;
        terminal["workspace_retained"] = json!(false);
        terminal["retirement_evidence"] = json!(retirement);
        let result = document(&cache, &terminal);
        let root = document(
            &cache,
            &json!({"job":1,"request":request,"retirement_evidence":retirement,"frames":[12],"sample_ids":["one"],"positive_terminal_retired":true,"all_output_SHA_bytes_caps_and_dimensions_verified":true,"human_approval":false}),
        );
        let proof = json!({"job":1,"request":request,"result":result,"root_verified":root,"retirement_evidence":retirement,"frames":[12],"sample_ids":["one"],"workspace_absent":true,"positive_terminal_retired":true,"exact_request_gzip_outputs_tables_verified":true});
        let mut views = json!({});
        for role in ROLES {
            let object = if role.contains("true390") {
                &small
            } else {
                &large
            };
            views[role] = json!({"sha256":object.sha256,"bytes":object.bytes,"dimensions":geometry[role],"native_output_role":format!("{role}-output"),"request":request,"job":1});
        }
        let clock = json!({"sample":{"sample_id":"one","frame":12,"reasons":["original boundary"],"current_footer_Review_ID_events":[{"code":"C1"}]},"views":views,"native_job_proof":proof});
        let chain = document(&cache, &json!({"segments":[]}));
        let cards = document(&cache, &json!({"cards":[]}));
        let occurrence = json!({"code":"C1","own_ES_span":[12,13],"original_picture":original_picture,"selected_picture":selected_picture,"raw_identity_only_producer_reference":identity});
        let original_source = document(
            &cache,
            &json!({"episode":"example","language":"es","actual_programme":programme,"changed_source_occurrences":[occurrence],"human_approval":false}),
        );
        let holds = json!({"phone_faint_colour":true,"opaque_motion":true,"human_approval":false});
        let notes = document(
            &cache,
            &json!({"episode":"example","language":"es","targets":[{"exact_own_source_occurrence":occurrence,"exact_requested_fix_lines_es":["Corrección de prueba."],"no_prior_visual_results_inherited":true}],"original_source_correspondence":original_source,"current_card_payload":cards,"source_holds":holds,"actual_pixel_QA_pending":true}),
        );
        let correspondence = document(
            &cache,
            &json!({"episode":"example","language":"es","actual_programme":programme,"actual_Review":review,"current_ASS":ass,"original_source_correspondence":original_source,"own_source_clock_state_chain":chain}),
        );
        let group = document(
            &cache,
            &json!({"schema":"fixture.group","episode":"example","language":"es","group_number":1,"actual_programme":programme,"actual_Review":review,"current_ASS":ass,"source_correspondence":correspondence,"clock_count":1,"clocks":[clock],"future_fresh_pixel_worker_max_clocks":3,"private_image_QA_pending":true,"human_approval":false}),
        );
        let task_value = json!({"schema":"fixture.indexed-task","episode":"example","language":"es","original_frozen_group":group,"group_number":1,"part":1,"actual_programme":programme,"actual_Review":review,"current_ASS":ass,"role_geometry":geometry,"clocks":[clock],"source_correspondence":correspondence,"own_source_clock_state_chain":chain,"source_holds":holds,"exact_own_correction_reference_notes":notes,"active_corrected_source_occurrences_by_sample":{"one":[occurrence]},"private_pixel_observations_pending":true,"no_other_episode_language_or_visual_result_inheritance":true,"human_approval":false});
        let task_bytes = serde_json::to_vec(&task_value).unwrap();
        let task = put(&cache, &task_bytes);
        let index = document(
            &cache,
            &json!({"summary":{"episode":"example","language":"es","bounded_tasks":1,"original_groups":1},"tasks":[task],"original_groups":[group],"controls":put(&cache,b"opaque retained controls"),"own_native_capture_parent":document(&cache,&json!({"retained":true})),"protocol":method,"notes":notes,"no_dispatch":true,"human_approval":false}),
        );
        let policy = document(
            &cache,
            &json!({"schema":"reel.review-observation-policy.experimental-1","method":method,"task_schema":"fixture.indexed-task","group_schema":"fixture.group","indexed_task_index":index,"observations":{"scope":"example/es","report_schema":"fixture.report","max_clocks":3,"footer_fields":FOOTERS,"footer_events_field":"current_footer_Review_ID_events","targets":[{"source_field":"indexed_source_targets","task_field":"indexed_active_targets","span_field":"own_ES_span","observation_field":"corrected_source_observations","exact_fields":["exact_requested_fix_lines_es"],"picture_fields":["original_picture","selected_picture"],"appearance_fields":["selected_correction_appearance_visible"]}]},"role_dimensions":geometry,"media_inputs":{"programme.mp4":"actual_programme","Review.mp4":"actual_Review"}}),
        );
        let profile = json!({"schema":"reel.review-scope-profile.experimental-1","task":task,"method":method,"policy":policy,"control_root":control,"byte_source":{"places":[{"root":cache,"layout":"reel-cas"}],"required":if cfg!(windows){"prevent-mutation"}else{"cooperative-immutable"},"limits":{"object_bytes":4194304,"total_bytes":67108864,"requests":256,"live_readers":16,"verify_millis":60000}},"state":{"kind":"unseen"},"receipt":null,"evidence":null});
        let mut observation = json!({"sample_id":"one","frame":12,"major_obstruction_observed":false,"minor_advisory":true,"fresh_role_observations":{},"corrected_source_observations":[{"code":"C1","original_picture":original_picture,"selected_picture":selected_picture,"exact_requested_fix_lines_es":["Corrección de prueba."],"per_role":{}}]});
        for role in ROLES {
            observation["fresh_role_observations"][role] = json!({"privately_viewed_detail_original":true,"detail":"Synthetic fixture role observation"});
            observation["corrected_source_observations"][0]["per_role"][role] =
                json!({"selected_correction_appearance_visible":null});
        }
        for field in FOOTERS {
            observation[field] = Value::Null;
        }
        let raw_observations = [original_picture, selected_picture].iter().map(|o|json!({"asset":o,"privately_viewed_detail_original":true,"fresh_hash_bytes_verified":true,"detail":"Synthetic raw context; no human acceptance"})).collect::<Vec<_>>();
        let evidence = json!({"fresh_observations_not_inherited":true,"finite_scope_qualified_with_holds":false,"human_approval":false,"holds_preserved":{"original_source_holds":holds},"raw_reference_view_occurrences":2,"raw_source_observations":raw_observations,"observations":[observation]});
        let path = temp.path().join("profile.json");
        let fixture = Self {
            _temp: temp,
            cache,
            control,
            path,
            profile,
            evidence,
            task_bytes,
            index,
            image: large,
        };
        fixture.save();
        fixture
    }
    fn save(&self) {
        fs::write(&self.path, serde_json::to_vec(&self.profile).unwrap()).unwrap();
    }
    fn run(&self, operation: &str) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_reel-tig-review"));
        cmd.arg(operation).arg(&self.path);
        if operation == "reserve" {
            cmd.args(["--owner", "synthetic-indexed-worker"]);
        }
        cmd.output().unwrap()
    }
    fn ok(&self, operation: &str) -> Value {
        let out = self.run(operation);
        assert!(
            out.status.success(),
            "{operation}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

#[test]
fn indexed_lifecycle_keeps_original_task_and_recovers_without_new_observations() {
    let mut f = Fixture::new();
    assert_eq!(f.ok("inspect")["state"], "metadata-verified-unreserved");
    assert_eq!(fs::read_dir(&f.control).unwrap().count(), 0);
    let reserved = f.ok("reserve");
    assert_eq!(reserved["task"], f.profile["task"]);
    assert!(!f.run("reserve").status.success());
    f.profile["receipt"] = reserved["receipt"].clone();
    f.save();
    assert_eq!(f.ok("resume")["automatic_view_permission"], false);
    // A missing selected picture cannot complete the original protocol.
    let mut missing = f.evidence.clone();
    missing["raw_source_observations"]
        .as_array_mut()
        .unwrap()
        .pop();
    missing["raw_reference_view_occurrences"] = json!(1);
    f.profile["evidence"] = json!(document(&f.cache, &missing));
    f.save();
    assert!(!f.run("submit").status.success());
    f.profile["evidence"] = json!(document(&f.cache, &f.evidence));
    f.save();
    let submitted = f.ok("submit");
    let report_bytes = fs::read(submitted["report_path"].as_str().unwrap()).unwrap();
    let report: Value = serde_json::from_slice(&report_bytes).unwrap();
    assert_eq!(report["frozen_subtask"], f.profile["task"]);
    assert_eq!(report["indexed_source_context"]["index"], json!(f.index));
    assert_eq!(report["observed_image_occurrences"], 4);
    assert!(report["observations"][0][FOOTERS[0]].is_null());
    assert_eq!(report["human_approval"], false);
    let task: Object = serde_json::from_value(f.profile["task"].clone()).unwrap();
    assert_eq!(fs::read(cas(&f.cache, &task)).unwrap(), f.task_bytes);
    let retained = put(&f.cache, &report_bytes);
    f.profile["state"] = json!({"kind":"counted","report":retained});
    f.profile["receipt"] = Value::Null;
    f.profile["evidence"] = Value::Null;
    f.save();
    let skipped = f.ok("reserve");
    assert_eq!(skipped["state"], "completed-validated-skip");
    assert_eq!(skipped["new_image_views"], 0);
    assert_eq!(skipped["new_receipts"], 0);
}

#[test]
fn indexed_source_corruption_refuses_before_any_receipt() {
    let f = Fixture::new();
    fs::write(
        cas(&f.cache, &f.image),
        b"corrupt first existing role image",
    )
    .unwrap();
    assert!(!f.run("inspect").status.success());
    assert!(!f.run("reserve").status.success());
    assert_eq!(fs::read_dir(&f.control).unwrap().count(), 0);
}
