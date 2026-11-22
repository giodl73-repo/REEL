//! Original fixed control adoption with real ordinary/bare annex bytes.
use super::*;
use reel_workflow_git_asset::retention::{Budget, Seed};
fn budget() -> Budget {
    Budget {
        objects: 4,
        object_bytes: 1024 * 1024,
        total_bytes: 4 * 1024 * 1024,
        millis: 60000,
    }
}
fn exercise(bare: bool) {
    let f = Fixture::new(bare);
    let root = f.temp.path();
    let mut ns = Namespace::open(&f.profile.members[0].root, f.handle.minimum()).unwrap();
    ns.apply(
        key("enroll-controls"),
        Command::Enroll {
            source: key("controls"),
            owner: key("owner"),
            class: Kind::Retention,
        },
        &f.nodes,
        &limits(),
    )
    .unwrap();
    let seed = Seed::prepare(
        &mut ns,
        key("controls"),
        key("owner"),
        key("prepare-controls"),
        key("adopt-controls"),
    )
    .unwrap();
    let competitor = Seed::prepare(
        &mut ns,
        key("controls"),
        key("owner"),
        key("competing-prepare"),
        key("competing-adopt"),
    )
    .unwrap();
    let raw = seed.encode().unwrap();
    let pin = Object::from_bytes(&raw);
    let seed = Seed::decode(&raw, &pin).unwrap();
    assert!(Seed::decode(&raw, &Object::from_bytes(b"wrong seed")).is_err());
    let path = root.join("control-metadata");
    let mut writer = MetadataStore::create(
        &path,
        metadata::Limits {
            nodes: 10000,
            bytes: 4 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let object = Object::from_bytes(&vec![37u8; 256 * 1024]);
    let entries = BTreeMap::from([(LogicalPath::parse("binding").unwrap(), object.clone())]);
    let context = Digest::blob(b"original task control context");
    let request = seed
        .selection(&entries, context.clone(), &mut writer)
        .unwrap();
    let mut wrong = object.clone();
    wrong.bytes += 1;
    let bad = seed
        .selection(
            &BTreeMap::from([
                (LogicalPath::parse("one").unwrap(), object.clone()),
                (LogicalPath::parse("two").unwrap(), wrong),
            ]),
            context.clone(),
            &mut writer,
        )
        .unwrap_err();
    assert_eq!(bad.kind, ErrorKind::Invalid);
    let cp = writer.finish().unwrap();
    drop(writer);
    let mut nodes = MetadataReader::open(&path, &cp).unwrap();
    let before = ns.checkpoint();
    let mut small = budget();
    small.total_bytes = 1;
    assert!(
        matches!(seed.admit(&f.profile,&request,&mut nodes,&limits(),small),Err(e) if e.kind==ErrorKind::Budget)
    );
    assert_eq!(ns.observe(&before).unwrap(), before);
    let held = seed
        .admit(&f.profile, &request, &mut nodes, &limits(), budget())
        .unwrap();
    assert_eq!(held.entries(), &entries);
    assert_eq!(
        held.bytes(&LogicalPath::parse("binding").unwrap()).unwrap(),
        vec![37u8; 256 * 1024]
    );
    assert!(
        matches!(competitor.original(&f.profile,&context,&mut nodes,&limits(),budget()),Err(e) if e.kind==ErrorKind::Stale)
    );
    assert!(
        matches!(seed.original(&f.profile,&Digest::blob(b"different task"),&mut nodes,&limits(),budget()),Err(e) if e.kind==ErrorKind::Stale)
    );
    let second = seed
        .original(&f.profile, &context, &mut nodes, &limits(), budget())
        .unwrap();
    let withdraw = Command::Withdraw {
        source: key("controls"),
        owner: key("owner"),
        expected_revision: 3,
    };
    assert!(matches!(
        ns.apply(key("cancel-controls"), withdraw.clone(), &nodes, &limits()),
        Err(git_asset_store::source_claims::admission::Error::Journal(
            git_asset_store::source_claims::Error::Busy
        ))
    ));
    ns.apply(
        key("independent-enrollment"),
        Command::Enroll {
            source: key("independent-control"),
            owner: key("owner"),
            class: Kind::Retention,
        },
        &nodes,
        &limits(),
    )
    .unwrap();
    drop(held);
    assert!(matches!(
        ns.apply(key("cancel-controls"), withdraw, &nodes, &limits()),
        Err(git_asset_store::source_claims::admission::Error::Journal(
            git_asset_store::source_claims::Error::Busy
        ))
    ));
    drop(second);
    let exact = seed
        .hold(&f.profile, &request, &mut nodes, &limits(), budget())
        .unwrap();
    drop(exact);
    // Same control bytes, different source generation: never silently use latest.
    ns.select(
        &Select {
            prepare: key("advance-controls"),
            adopt: key("adopt-advanced-controls"),
            source: key("controls"),
            owner: key("owner"),
            expected_revision: 3,
            selection: request.selection.clone(),
        },
        &nodes,
        &limits(),
    )
    .unwrap();
    assert!(
        matches!(seed.original(&f.profile,&context,&mut nodes,&limits(),budget()),Err(e) if e.kind==ErrorKind::Stale)
    );
    assert!(
        matches!(seed.admit(&f.profile,&request,&mut nodes,&limits(),budget()),Err(e) if e.kind==ErrorKind::Stale)
    );
    assert_eq!(ns.source(&key("controls")).unwrap().unwrap().revision, 5);
}
#[test]
#[ignore = "explicit admitted native tools; fresh synthetic ordinary pool only"]
fn ordinary_original_controls_hold_fixed_adoption() {
    exercise(false)
}
#[test]
#[ignore = "explicit admitted native tools; fresh synthetic bare pool only"]
fn bare_original_controls_hold_fixed_adoption() {
    exercise(true)
}
