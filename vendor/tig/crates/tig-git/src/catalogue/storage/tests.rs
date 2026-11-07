use super::*;
use std::{fs, io::Write};
fn report() -> Value {
    json!({"schema":"tig.cache-observation.experimental-1","project_id":"fixture","registry_generation":4,"census_completed_unix_seconds":12,"coverage_complete":false,"tiers":[{"tier":0},{"tier":1}],"objects":[
        {"sha256":"a".repeat(64),"bytes":100,"category":"retained-noncurrent","noncurrent_reference":true,"direct_version_ids":["work/demo@r1","work/demo@r2"],"copies":[{"tier":0,"bytes":100,"allocated_bytes":4096,"physical_id":"a","hard_links":1,"other_hard_links":0,"descriptor_size_matches":true},{"tier":1,"bytes":100,"allocated_bytes":4096,"physical_id":"backup-a","hard_links":1}]},
        {"sha256":"b".repeat(64),"bytes":20,"category":"selected-output","required_by_selected_version":true,"deprecated_reference":true,"direct_version_ids":["work/demo@r1"],"copies":[{"tier":0,"bytes":20,"allocated_bytes":4096,"physical_id":"b","hard_links":1},{"tier":1,"bytes":20,"physical_id":"backup-b"}]},
        {"sha256":"c".repeat(64),"bytes":10,"category":"unclassified","direct_version_ids":[],"copies":[{"tier":0,"bytes":10}]}
    ]})
}
fn profile() -> Profile {
    serde_json::from_value(json!({"schema":"tig.catalogue-storage.experimental-1","inventory":Object::from_bytes(b"placeholder"),"encoding":"gzip","max_decoded_bytes":1048576,"places":[{"label":"Fast","place_index":0,"report_tier":0},{"label":"Archive","place_index":1,"report_tier":1},{"label":"Offline","place_index":2,"report_tier":null}],"owners":[{"pattern":"^work/([^@]+)@","family":"Documents","key":"work/${1}"}]})).unwrap()
}
fn load_value(value: &Value) -> (tempfile::TempDir, Profile, ByteProfile) {
    let t = tempfile::tempdir().unwrap();
    let mut zip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    zip.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
    let bytes = zip.finish().unwrap();
    let mut p = profile();
    p.inventory = Some(Object::from_bytes(&bytes));
    fs::create_dir(t.path().join("objects")).unwrap();
    fs::write(
        t.path()
            .join("objects")
            .join(p.inventory.as_ref().unwrap().sha256.as_str()),
        bytes,
    )
    .unwrap();
    let byte=serde_json::from_value(json!({"places":[{"root":t.path(),"layout":"flat"}],"required":if cfg!(windows){"prevent-mutation"}else{"cooperative-immutable"}})).unwrap();
    (t, p, byte)
}
fn query_all<'a>() -> Query<'a> {
    Query {
        cache: "Fast",
        family: "",
        owner: "",
        version: "",
        state: "",
        pivot: "versions",
        metric: "recorded_file_bytes",
        q: "",
        offset: 0,
    }
}
#[test]
fn historical_shared_versions_do_not_inflate_unique_summary_or_authorize_removal() {
    let (_t, p, byte) = load_value(&report());
    let data = load(&p, &byte).unwrap();
    let q = query_all();
    let r = query(&data, &p, &q, &BTreeSet::new()).unwrap();
    assert_eq!(r["summary"]["objects"], 3);
    assert_eq!(r["summary"]["recorded_file_bytes"], 130);
    assert_eq!(r["summary"]["measured_allocated_bytes"], 8192);
    assert_eq!(r["summary"]["copies_with_unknown_allocation"], 1);
    assert_eq!(r["summary"]["historical_review_allocation"], 4096);
    assert_eq!(r["summary"]["safe_reclaimable_bytes"], Value::Null);
    assert_eq!(r["group_totals_overlap"], true);
    assert_eq!(r["removal_authorized"], false);
    assert_eq!(r["places"][2]["observed"], false);
    let mut q = query_all();
    q.cache = "";
    assert_eq!(
        query(&data, &p, &q, &BTreeSet::new()).unwrap()["summary"]["historical_review_allocation"],
        0
    );
}
#[test]
fn current_selected_pins_links_aliases_and_known_references_hold_history() {
    for change in [
        "pinned",
        "leased",
        "required_by_current_version",
        "required_by_selected_version",
        "alias",
        "links",
        "unknown-links",
        "wrong-size",
    ] {
        let mut r = report();
        match change {
            "alias" => r["objects"][0]["copies"][1]["physical_id"] = json!("a"),
            "links" => r["objects"][0]["copies"][0]["hard_links"] = json!(2),
            "unknown-links" => r["objects"][0]["copies"][0]["hard_links"] = Value::Null,
            "wrong-size" => r["objects"][0]["copies"][0]["descriptor_size_matches"] = json!(false),
            flag => r["objects"][0][flag] = json!(true),
        }
        let (_t, p, b) = load_value(&r);
        let data = load(&p, &b).unwrap();
        assert_eq!(
            query(&data, &p, &query_all(), &BTreeSet::new()).unwrap()["summary"]["historical_review_allocation"],
            0,
            "{change}"
        );
    }
    let (_t, p, b) = load_value(&report());
    let data = load(&p, &b).unwrap();
    let known = BTreeSet::from(["a".repeat(64)]);
    assert_eq!(
        query(&data, &p, &query_all(), &known).unwrap()["summary"]["historical_review_allocation"],
        0
    );
}
#[test]
fn inventory_integrity_decode_and_admission_budgets_fail_closed() {
    let (t, mut p, b) = load_value(&report());
    let original = p.clone();
    p.max_decoded_bytes = 10;
    assert!(load(&p, &b).is_err());
    p = original;
    fs::write(
        t.path()
            .join("objects")
            .join(p.inventory.as_ref().unwrap().sha256.as_str()),
        b"corrupt",
    )
    .unwrap();
    assert!(load(&p, &b).is_err());
    let mut r = report();
    r["objects"][1]["sha256"] = r["objects"][0]["sha256"].clone();
    let (_t, p, b) = load_value(&r);
    assert!(load(&p, &b).is_err());
    let mut p = profile();
    p.owners[0].pattern = "unanchored".into();
    assert!(validate(&p).is_err());
    let mut p = profile();
    p.inventory = None;
    assert!(validate(&p).is_ok());
    p.probe_millis = 120001;
    assert!(validate(&p).is_err());
}
#[test]
fn filters_allocation_sort_and_accounting_overflow_are_explicit() {
    let (_t, p, b) = load_value(&report());
    let data = load(&p, &b).unwrap();
    let mut q = query_all();
    q.state = "history-refs";
    q.owner = "work/demo";
    let r = query(&data, &p, &q, &BTreeSet::new()).unwrap();
    assert_eq!(r["objects_total"], 1);
    q.cache = "Offline";
    assert!(query(&data, &p, &q, &BTreeSet::new()).is_err());
    q.cache = "Fast";
    q.metric = "measured_allocated_bytes";
    q.pivot = "objects";
    assert_eq!(
        query(&data, &p, &q, &BTreeSet::new()).unwrap()["groups_total"],
        1
    );
    let mut n = u64::MAX;
    assert!(add(&mut n, 1).is_err());
}
#[test]
fn physical_aliases_count_once_and_profile_never_probes_unenrolled_tiers() {
    let mut r = report();
    r["objects"][0]["copies"][1]["physical_id"] = json!("a");
    let (_t, p, b) = load_value(&r);
    let data = load(&p, &b).unwrap();
    let mut q = query_all();
    q.cache = "";
    q.state = "history-refs";
    let result = query(&data, &p, &q, &BTreeSet::new()).unwrap();
    assert_eq!(result["summary"]["recorded_file_bytes"], 100);
    assert_eq!(result["summary"]["measured_allocated_bytes"], 4096);
}

#[test]
fn family_and_work_filters_keep_shared_package_owners_out_of_work_groups() {
    let (_t, p, b) = load_value(&report());
    let mut data = load(&p, &b).unwrap();
    data.entries[0].owners.extend([
        ("Packages".into(), "package/demo".into()),
        ("Other".into(), "work/other".into()),
    ]);
    let known = BTreeSet::new();
    let mut q = query_all();
    q.pivot = "works";
    q.family = "Documents";
    let result = query(&data, &p, &q, &known).unwrap();
    assert_eq!(result["groups_total"], 1);
    assert_eq!(result["groups"][0]["key"], "work/demo");
    assert_eq!(result["summary"]["recorded_file_bytes"], 120);
    assert_eq!(result["groups"][0]["summary"]["recorded_file_bytes"], 120);
    assert_eq!(result["removal_authorized"], false);
    q.owner = "work/other";
    assert_eq!(query(&data, &p, &q, &known).unwrap()["objects_total"], 0);
    q.family = "";
    q.owner = "work/demo";
    let result = query(&data, &p, &q, &known).unwrap();
    assert_eq!(result["groups_total"], 1);
    assert_eq!(result["summary"]["recorded_file_bytes"], 120);
    q.owner = "";
    assert!(
        query(&data, &p, &q, &known).unwrap()["groups_total"]
            .as_u64()
            .unwrap()
            > 1
    );
}
