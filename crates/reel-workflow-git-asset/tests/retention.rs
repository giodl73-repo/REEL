//! Control seed and metadata builder boundaries; no native IO or custody claims.
use git_asset_core::{Digest, LogicalPath, Object, tree::MemoryNodes};
use git_asset_store::{
    Key,
    protection::Limits,
    source_claims::{
        Command, Kind, Places,
        admission::{Namespace, Policy},
    },
};
use reel_workflow::ErrorKind;
use reel_workflow_git_asset::retention::Seed;
use std::collections::BTreeMap;
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn policy() -> Policy {
    Policy {
        prefer_active: true,
        classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
            .into_iter()
            .map(|k| {
                (
                    k,
                    Places {
                        current: vec![key("pool")],
                        pending: vec![key("pool")],
                        predecessor: vec![key("pool")],
                    },
                )
            })
            .collect(),
    }
}
fn limits() -> Limits {
    Limits {
        millis: 60000,
        roots: 128,
        bindings: 4096,
        associations: 10000,
        reads: 10000,
        read_bytes: 4 * 1024 * 1024,
    }
}
fn make(ns: &mut Namespace) -> reel_workflow::Result<Seed> {
    Seed::prepare(
        ns,
        key("control"),
        key("owner"),
        key("prepare"),
        key("adopt"),
    )
}
#[test]
fn seed_pins_original_empty_retention_source_and_decoding_is_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let mut ns = Namespace::create_fenced(
        &temp.path().join("ns"),
        key("project"),
        key("epoch"),
        policy(),
    )
    .unwrap();
    let mut nodes = MemoryNodes::default();
    assert!(matches!(make(&mut ns),Err(e) if e.kind==ErrorKind::Missing));
    ns.apply(
        key("enroll"),
        Command::Enroll {
            source: key("control"),
            owner: key("owner"),
            class: Kind::Retention,
        },
        &nodes,
        &limits(),
    )
    .unwrap();
    let before = ns.checkpoint();
    let seed = make(&mut ns).unwrap();
    assert_eq!(ns.checkpoint(), before);
    let raw = seed.encode().unwrap();
    let pin = Object::from_bytes(&raw);
    assert_eq!(Seed::decode(&raw, &pin).unwrap(), seed);
    assert!(
        matches!(Seed::decode(&raw,&Object::from_bytes(b"wrong")),Err(e) if e.kind==ErrorKind::Integrity)
    );
    let huge = vec![b' '; 16385];
    assert!(
        matches!(Seed::decode(&huge,&Object::from_bytes(&huge)),Err(e) if e.kind==ErrorKind::Budget)
    );
    let mut wire: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    wire["untrusted_extension"] = true.into();
    let extra = serde_json::to_vec(&wire).unwrap();
    assert!(
        matches!(Seed::decode(&extra,&Object::from_bytes(&extra)),Err(e) if e.kind==ErrorKind::Invalid)
    );
    assert!(
        matches!(seed.selection(&BTreeMap::new(),Digest::blob(b"ctx"),&mut nodes),Err(e) if e.kind==ErrorKind::Budget)
    );
    let entries = BTreeMap::from([(
        LogicalPath::parse("binding").unwrap(),
        Object::from_bytes(b"declared metadata-only control"),
    )]);
    let request = seed
        .selection(&entries, Digest::blob(b"ctx"), &mut nodes)
        .unwrap();
    assert_eq!(request.expected_revision, 1);
    assert_eq!(request.source, key("control"));
    ns.select(&request, &nodes, &limits()).unwrap();
    assert!(matches!(make(&mut ns),Err(e) if e.kind==ErrorKind::Stale));
}
#[test]
fn wrong_class_owner_ids_and_legacy_format_cannot_prepare_control_seed() {
    let temp = tempfile::tempdir().unwrap();
    let mut old =
        Namespace::create(&temp.path().join("old"), key("p"), key("e"), policy()).unwrap();
    let before = old.checkpoint();
    assert!(matches!(make(&mut old),Err(e) if e.kind==ErrorKind::Unsupported));
    assert_eq!(old.checkpoint(), before);
    assert!(!temp.path().join("old/source-gates").exists());
    for kind in [Kind::Job, Kind::Retention] {
        let path = temp.path().join(if kind == Kind::Job {
            "job"
        } else {
            "retention"
        });
        let mut ns = Namespace::create_fenced(&path, key("p"), key("e"), policy()).unwrap();
        ns.apply(
            key("enroll"),
            Command::Enroll {
                source: key("control"),
                owner: key("owner"),
                class: kind.clone(),
            },
            &MemoryNodes::default(),
            &limits(),
        )
        .unwrap();
        let before = ns.checkpoint();
        if kind == Kind::Job {
            assert!(matches!(make(&mut ns),Err(e) if e.kind==ErrorKind::Denied));
        }
        assert!(
            matches!(Seed::prepare(&mut ns,key("control"),key("wrong-owner"),key("prepare"),key("adopt")),Err(e) if e.kind==ErrorKind::Denied)
        );
        if kind == Kind::Retention {
            assert!(
                matches!(Seed::prepare(&mut ns,key("control"),key("owner"),key("same"),key("same")),Err(e) if e.kind==ErrorKind::Stale)
            );
        }
        assert_eq!(ns.checkpoint(), before);
    }
}
