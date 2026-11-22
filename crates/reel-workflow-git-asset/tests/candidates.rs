//! Pure binding/plan controls only. Fake references grant no custody or effects.
mod common;
use common::*;
use git_asset::annex::{
    self, managed,
    publication::{self, Plan},
};
use git_asset_core::{
    Digest, Object,
    tree::{MemoryNodes, NodeStore, TreeLimits},
};
use git_asset_store::{
    Key,
    existing::ByteSource,
    source_claims::Checkpoint,
    store_protection::{Binding, Member},
};
use reel_workflow::ErrorKind;
use reel_workflow_git_asset::candidates::{Candidates, RETURN_PACKET};
use std::{collections::BTreeMap, io::Read, path::Path};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn plans(set: &Candidates, root: &Path) -> BTreeMap<String, Plan> {
    let namespace = Digest::blob(b"synthetic nonexistent namespace");
    let cp: Checkpoint = serde_json::from_value(
        serde_json::json!({"genesis":namespace,"sequence":0,"hash":namespace}),
    )
    .unwrap();
    let profile = publication::Profile {
        pool: managed::Profile {
            pool: annex::Profile {
                git: root.join("absent-git"),
                annex: root.join("absent-annex"),
                repository: root.join("absent-pool"),
                bare: false,
                uuid: "00000000-0000-0000-0000-000000000001".into(),
                configuration: Object::from_bytes(b"synthetic configuration"),
                tools: vec![root.into()],
                empty_config: root.join("absent-config"),
                empty_hooks: root.join("absent-hooks"),
                temporary: root.join("absent-temp"),
                command_millis: 1000,
                output_bytes: 4096,
                limits: Default::default(),
            },
            binding: Binding::new(
                1,
                Digest::blob(b"synthetic boundary"),
                vec![Member {
                    namespace: namespace.clone(),
                    places: vec![key("pool")],
                }],
            )
            .unwrap(),
            members: vec![managed::Member {
                root: root.join("absent-namespace"),
                minimum: cp,
            }],
        },
        retained_place: key("pool"),
        staging: root.join("absent-stage"),
        staging_marker: Object::from_bytes(b"synthetic marker"),
        budget: publication::Budget {
            object_bytes: 1024 * 1024,
            transfer_bytes: 4 * 1024 * 1024,
            millis: 1000,
        },
        allow_publish: false,
    };
    let mut nodes = MemoryNodes::default();
    let provenance = nodes.put(set.provenance()).unwrap();
    set.expected().iter().enumerate().map(|(index,(name,a))|{
        let request=publication::Request{operation:key(&format!("op-{index}")),owner:key("owner"),source:key(&format!("retention-{index}")),expected_revision:1,
            candidate:publication::Candidate{logical_id:a.logical_id.clone(),object:Object{sha256:Digest::parse(&a.sha256).unwrap(),bytes:a.bytes},provenance:provenance.clone()}};
        let frozen=publication::freeze(&request,&mut nodes,TreeLimits::default()).unwrap();
        let job=serde_json::from_value(serde_json::json!({"genesis":Digest::blob(b"synthetic ledger"),"operation":"synthetic-job"})).unwrap();
        let plan=Plan::new(&profile,request,frozen,publication::Admission{namespace:namespace.clone(),witness:Digest::blob(b"synthetic witness"),capacity:Digest::blob(b"synthetic ledger"),job}).unwrap();
        (name.clone(),plan)
    }).collect()
}
#[test]
fn original_json_bytes_and_provenance_remain_immutable_and_named() {
    let (ticket, raw) = returned(&"a".repeat(40), vec![]);
    let set = Candidates::bind(&ticket, &raw, "returns/attempt-1").unwrap();
    assert_eq!(set.original_packet(), raw);
    assert_eq!(
        set.expected()[RETURN_PACKET].sha256,
        Object::from_bytes(&raw).sha256.as_str()
    );
    let mut source = set.packet_source();
    let mut bytes = vec![];
    source
        .open_verified(&Object::from_bytes(&raw))
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, raw);
    assert!(
        source
            .open_verified(&Object::from_bytes(b"another object"))
            .is_err()
    );
    assert_eq!(
        set.expected()["output/note"],
        artifact("candidate/note", NOTE, "text/plain")
    );
    assert_eq!(set.result().attempt_id, ticket.attempt_id);
}
#[test]
fn malformed_wrong_attempt_and_identity_conflicts_refuse_before_io() {
    let (ticket, raw) = returned(&"a".repeat(40), vec![]);
    assert!(Candidates::bind(&ticket, b"{}", "returns/attempt-1").is_err());
    assert_eq!(
        Candidates::bind(&ticket, &vec![b' '; 1024 * 1024 + 1], "returns/attempt-1")
            .err()
            .unwrap()
            .kind,
        ErrorKind::Budget
    );
    assert_eq!(
        Candidates::bind(&ticket, &raw, "candidate/note")
            .err()
            .unwrap()
            .kind,
        ErrorKind::Conflict
    );
    let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    value["attempt_id"] = serde_json::json!("another-attempt");
    assert_eq!(
        Candidates::bind(
            &ticket,
            &serde_json::to_vec(&value).unwrap(),
            "returns/attempt-1"
        )
        .err()
        .unwrap()
        .kind,
        ErrorKind::Conflict
    );
    value["extra"] = serde_json::json!(true);
    assert!(
        Candidates::bind(
            &ticket,
            &serde_json::to_vec(&value).unwrap(),
            "returns/attempt-1"
        )
        .is_err()
    );
}
#[test]
fn original_plan_cohort_is_hash_pinned_and_rejects_rewritten_returns_or_controls() {
    let temp = tempfile::tempdir().unwrap();
    let (ticket, raw) = returned(&"a".repeat(40), vec![]);
    let set = Candidates::bind(&ticket, &raw, "returns/attempt-1").unwrap();
    let cohort = plans(&set, temp.path());
    let prepared = set.prepare(cohort).unwrap();
    let control = prepared.control().to_vec();
    let expected = Object::from_bytes(&control);
    let restored = Candidates::bind(&ticket, &raw, "returns/attempt-1")
        .unwrap()
        .restore(&control, &expected)
        .unwrap();
    assert_eq!(restored.control(), control);
    assert_eq!(restored.plans().len(), 2);
    assert_eq!(
        restored.gather(vec![]).err().unwrap().kind,
        ErrorKind::Missing
    );
    let mut altered = control.clone();
    altered.push(b' ');
    assert_eq!(
        Candidates::bind(&ticket, &raw, "returns/attempt-1")
            .unwrap()
            .restore(&altered, &expected)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Integrity
    );
    let mut reformatted = raw.clone();
    reformatted.push(b' ');
    assert_eq!(
        Candidates::bind(&ticket, &reformatted, "returns/attempt-1")
            .unwrap()
            .restore(&control, &expected)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        Candidates::bind(&ticket, &raw, "returns/another-id")
            .unwrap()
            .restore(&control, &expected)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Conflict
    );
    assert!(!temp.path().join("absent-pool").exists()); // pure constructors/planning
}
#[test]
fn missing_mismatched_and_legacy_plans_cannot_become_a_retained_cohort() {
    let temp = tempfile::tempdir().unwrap();
    let (ticket, raw) = returned(&"a".repeat(40), vec![]);
    let set = Candidates::bind(&ticket, &raw, "returns/attempt-1").unwrap();
    let mut cohort = plans(&set, temp.path());
    let packet = cohort.remove(RETURN_PACKET).unwrap();
    assert_eq!(
        set.check_plan("output/note", &packet).unwrap_err().kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        set.check_plan("absent", &packet).unwrap_err().kind,
        ErrorKind::Missing
    );
    let mut legacy = serde_json::to_value(&packet).unwrap();
    legacy.as_object_mut().unwrap().remove("effect_protocol");
    let legacy: Plan = serde_json::from_value(legacy).unwrap();
    assert_eq!(
        set.check_plan(RETURN_PACKET, &legacy).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    assert_eq!(set.prepare(cohort).err().unwrap().kind, ErrorKind::Missing);
}
