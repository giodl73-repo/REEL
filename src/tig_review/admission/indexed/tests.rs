use super::*;

fn protocol() -> Value {
    json!({
        "maximum_clocks_per_assignment":3,
        "role_names":ROLES,
        "verify_all_frozen_descriptors_before_any_view":true,
        "verify_each_exact_original_file_SHA_bytes_RGB8_header_and_dimensions":true,
        "view_each_role_at_original_detail":true,
        "no_montage_resize_crop_composite_or_reencoding":true,
        "all_four_actual_role_views_required":true,
        "assess_own_ASS_and_current_footer_codes_against_exact_active_events":true,
        "record_all_active_corrected_occurrences_and_no_out_of_span_targets":true,
        "compare_original_and_selected_raw_references_before_requested_feature_judgments":true,
        "preserve_original_per_role_false_null_missing_readability_fine_colour_source_and_opaque_motion_holds":true,
        "retain_exact_task_group_source_programme_Review_and_view_descriptors":true,
        "no_cross_episode_or_language_visual_outcomes":true,
        "no_global_human_continuous_motion_or_audio_acceptance":true,
        "only_metadata_may_return_to_root":true,
        "no_registry_Drive_or_card_edits":true,
        "no_dispatch_authorized_by_preparation":true
    })
}

struct Fixture {
    root: tempfile::TempDir,
    task: Value,
    task_pin: Object,
    index: Value,
    index_pin: Object,
    method: Object,
    method_bytes: Vec<u8>,
    policy: Policy,
}

fn put(root: &Path, raw: &[u8]) -> Object {
    let object = Object::from_bytes(raw);
    let sha = object.sha256.as_str();
    let path = root.join("objects/sha256").join(&sha[..2]).join(sha);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, raw).unwrap();
    object
}
fn doc(root: &Path, value: &Value) -> Object {
    put(root, &serde_json::to_vec(value).unwrap())
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let method_bytes = serde_json::to_vec(&protocol()).unwrap();
        let method = put(root.path(), &method_bytes);
        let programme = Object::from_bytes(b"programme");
        let review = Object::from_bytes(b"Review");
        let ass = Object::from_bytes(b"ASS");
        let chain = doc(root.path(), &json!({"segments":[]}));
        let cards = doc(root.path(), &json!({"cards":[]}));
        let occurrence = json!({"code":"C1","own_ES_span":[1,2],"selected_picture":Object::from_bytes(b"selected"),"original_picture":Object::from_bytes(b"original"),"raw_identity_only_producer_reference":doc(root.path(), &json!({"identity_only":true}))});
        let original = doc(
            root.path(),
            &json!({"episode":"example","language":"es","actual_programme":programme,"human_approval":false,"changed_source_occurrences":[occurrence]}),
        );
        let holds = json!({"phone_faint_colour":true,"opaque_motion":true,"human_approval":false});
        let wrapper = json!({"exact_own_source_occurrence":occurrence,"exact_requested_fix_lines_es":["Corrección de prueba."],"no_prior_visual_results_inherited":true});
        let notes = doc(
            root.path(),
            &json!({"episode":"example","language":"es","targets":[wrapper],"original_source_correspondence":original,"current_card_payload":cards,"source_holds":holds,"actual_pixel_QA_pending":true}),
        );
        let correspondence = doc(
            root.path(),
            &json!({"episode":"example","language":"es","actual_programme":programme,"actual_Review":review,"current_ASS":ass,"original_source_correspondence":original,"own_source_clock_state_chain":chain}),
        );
        let clocks = (0..7).map(|i| json!({"sample":{"sample_id":format!("sample-{i}"),"frame":i,"reasons":["original boundary"]},"views":{},"native_job_proof":{}})).collect::<Vec<_>>();
        let group = doc(
            root.path(),
            &json!({"schema":"fixture.group","episode":"example","language":"es","group_number":1,"actual_programme":programme,"actual_Review":review,"current_ASS":ass,"source_correspondence":correspondence,"clocks":clocks,"future_fresh_pixel_worker_max_clocks":3,"private_image_QA_pending":true,"human_approval":false}),
        );
        let geometry = json!({"programme_native":[4,3],"programme_true390":[2,1],"Review_native":[4,3],"Review_true390":[2,1]});
        let task = json!({"schema":"fixture.indexed-task","episode":"example","language":"es","original_frozen_group":group,"group_number":1,"part":1,"actual_programme":programme,"actual_Review":review,"current_ASS":ass,"role_geometry":geometry,"clocks":clocks[..3],"source_correspondence":correspondence,"own_source_clock_state_chain":chain,"source_holds":holds,"exact_own_correction_reference_notes":notes,"active_corrected_source_occurrences_by_sample":{"sample-0":[],"sample-1":[occurrence],"sample-2":[]},"private_pixel_observations_pending":true,"no_other_episode_language_or_visual_result_inheritance":true,"human_approval":false});
        let task_pin = doc(root.path(), &task);
        let index = json!({"summary":{"episode":"example","language":"es","bounded_tasks":1,"original_groups":1},"tasks":[task_pin],"original_groups":[group],"controls":put(root.path(),b"opaque retained controls"),"own_native_capture_parent":doc(root.path(),&json!({"retained":true})),"protocol":method,"notes":notes,"no_dispatch":true,"human_approval":false});
        let index_pin = doc(root.path(), &index);
        let policy = serde_json::from_value(json!({"schema":"reel.review-observation-policy.experimental-1","method":method,"task_schema":"fixture.indexed-task","group_schema":"fixture.group","indexed_task_index":index_pin,"observations":{"scope":"example/es","report_schema":"fixture.report","max_clocks":3,"footer_fields":FOOTER_FACTS,"footer_events_field":"current_footer_Review_ID_events","targets":[{"source_field":"indexed_source_targets","task_field":"indexed_active_targets","span_field":"own_ES_span","observation_field":"corrected_source_observations","exact_fields":["exact_requested_fix_lines_es"],"picture_fields":["original_picture","selected_picture"],"appearance_fields":["selected_correction_appearance_visible"]}]},"role_dimensions":geometry,"media_inputs":{"programme.mp4":"actual_programme","Review.mp4":"actual_Review"}})).unwrap();
        Self {
            root,
            task,
            task_pin,
            index,
            index_pin,
            method,
            method_bytes,
            policy,
        }
    }
    fn refresh(&mut self) {
        self.task_pin = doc(self.root.path(), &self.task);
        self.index["tasks"] = json!([self.task_pin]);
        self.index_pin = doc(self.root.path(), &self.index);
        self.policy.indexed_task_index = Some(self.index_pin.clone());
    }
    fn bind(&self) -> Result<Binding> {
        let profile = serde_json::from_value(
            json!({"places":[{"root":self.root.path(),"layout":"reel-cas"}],"required":if cfg!(windows){"prevent-mutation"}else{"cooperative-immutable"},"limits":{"object_bytes":4194304,"total_bytes":67108864,"requests":256,"live_readers":16,"verify_millis":60000}}),
        )?;
        bind(
            &self.task,
            &self.task_pin,
            &self.method,
            &self.method_bytes,
            &self.policy,
            &self.index_pin,
            &mut ReadCache::new(profile)?,
        )
    }
}

#[test]
fn binds_original_identity_exact_partition_targets_and_auxiliary_reference() {
    let f = Fixture::new();
    let original = f.task.clone();
    let bound = f.bind().unwrap();
    assert_eq!(f.task, original);
    assert_eq!(bound.task["original_clock_indices"], json!([0, 1, 2]));
    assert_eq!(bound.task["clocks"], original["clocks"]);
    assert_eq!(bound.task["raw_references"].as_array().unwrap().len(), 2);
    assert_eq!(
        bound.task["indexed_source_context"]["identity_only_metadata"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        bound.task["indexed_active_targets"][0]["exact_requested_fix_lines_es"],
        json!(["Corrección de prueba."])
    );
    assert_eq!(
        bound.task["required_original_holds"],
        original["source_holds"]
    );
    assert_eq!(bound.task["human_approval"], false);
}

#[test]
fn refuses_unindexed_task_even_when_other_source_descriptors_match() {
    let mut f = Fixture::new();
    f.task["extra_note"] = json!("different immutable task");
    f.task_pin = doc(f.root.path(), &f.task);
    assert!(f.bind().is_err());
}

#[test]
fn refuses_corrupt_identity_metadata_without_treating_it_as_a_picture() {
    let f = Fixture::new();
    let object = pin(&f.task["active_corrected_source_occurrences_by_sample"]["sample-1"][0]["raw_identity_only_producer_reference"]).unwrap();
    let sha = object.sha256.as_str();
    let path = f
        .root
        .path()
        .join("objects/sha256")
        .join(&sha[..2])
        .join(sha);
    fs::write(path, b"changed identity metadata").unwrap();
    assert!(f.bind().is_err());
}

#[test]
fn refuses_registered_clock_omission_reordering_and_wrong_part() {
    for mode in 0..3 {
        let mut f = Fixture::new();
        match mode {
            0 => {
                f.task["clocks"].as_array_mut().unwrap().pop();
            }
            1 => {
                f.task["clocks"].as_array_mut().unwrap().swap(0, 1);
            }
            _ => f.task["part"] = json!(2),
        }
        f.refresh();
        assert!(f.bind().is_err());
    }
}

#[test]
fn refuses_omitted_or_out_of_span_original_target_selection() {
    for frame in ["sample-0", "sample-1"] {
        let mut f = Fixture::new();
        if frame == "sample-1" {
            f.task["active_corrected_source_occurrences_by_sample"][frame] = json!([]);
        } else {
            f.task["active_corrected_source_occurrences_by_sample"][frame] =
                f.task["active_corrected_source_occurrences_by_sample"]["sample-1"].clone();
        }
        f.refresh();
        assert!(f.bind().is_err());
    }
}

#[test]
fn refuses_cross_language_geometry_hold_and_approval_changes() {
    for mode in 0..4 {
        let mut f = Fixture::new();
        match mode {
            0 => f.task["language"] = json!("en"),
            1 => f.task["role_geometry"]["programme_native"] = json!([2, 1]),
            2 => f.task["source_holds"]["phone_faint_colour"] = json!(false),
            _ => f.task["human_approval"] = json!(true),
        }
        f.refresh();
        assert!(f.bind().is_err());
    }
}

#[test]
fn refuses_policy_that_drops_required_correction_or_footer_checks() {
    let mut f = Fixture::new();
    f.policy.observations.targets.clear();
    assert!(f.bind().is_err());
    let mut f = Fixture::new();
    f.policy.observations.footer_fields.pop();
    assert!(f.bind().is_err());
    let mut f = Fixture::new();
    f.policy.media_inputs.remove("Review.mp4");
    assert!(f.bind().is_err());
}

#[test]
fn every_original_protocol_predicate_remains_required() {
    for (name, value) in protocol().as_object().unwrap() {
        if value != &Value::Bool(true) {
            continue;
        }
        let mut f = Fixture::new();
        let mut bad = protocol();
        bad[name] = json!(false);
        f.method_bytes = serde_json::to_vec(&bad).unwrap();
        f.method = put(f.root.path(), &f.method_bytes);
        f.policy.method = f.method.clone();
        f.index["protocol"] = json!(f.method);
        f.refresh();
        assert!(f.bind().is_err(), "omitted protocol predicate {name}");
    }
}
