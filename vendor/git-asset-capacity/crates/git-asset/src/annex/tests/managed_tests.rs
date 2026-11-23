use super::*;
use crate::annex::managed;
use git_asset_store::{
    Key,
    source_claims::{
        Kind, Places,
        admission::{Namespace, Policy},
    },
    store_protection::{Binding, Member as BindingMember},
};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
pub(super) fn roster(pool: Profile, root: &Path) -> (managed::Profile, Vec<Namespace>) {
    let places = Places {
        current: vec![key("pool")],
        pending: vec![key("pool")],
        predecessor: vec![key("pool")],
    };
    let policy = Policy {
        prefer_active: true,
        classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
            .into_iter()
            .map(|kind| (kind, places.clone()))
            .collect(),
    };
    let namespaces: Vec<_> = (0..2)
        .map(|i| {
            Namespace::create(
                &root.join(format!("namespace-{i}")),
                key("fixture"),
                key("epoch"),
                policy.clone(),
            )
            .unwrap()
        })
        .collect();
    let binding = Binding::new(
        1,
        Digest::blob(b"synthetic participating pruner boundary"),
        namespaces
            .iter()
            .map(|n| BindingMember {
                namespace: n.identity().clone(),
                places: vec![key("pool")],
            })
            .collect(),
    )
    .unwrap();
    let members = namespaces
        .iter()
        .enumerate()
        .map(|(i, n)| managed::Member {
            root: root.join(format!("namespace-{i}")),
            minimum: n.checkpoint(),
        })
        .collect();
    (
        managed::Profile {
            pool,
            binding,
            members,
        },
        namespaces,
    )
}
#[test]
fn managed_constructor_requires_exact_roster_but_performs_no_discovery() {
    let root = tempfile::tempdir().unwrap();
    let p = profile(
        &root.path().join("absent-pool"),
        &root.path().join("absent-tool"),
    );
    let (mut p, _namespaces) = roster(p, root.path());
    for member in &mut p.members {
        member.root = member.root.join("absent-location");
    }
    assert!(managed::Reader::new(p.clone()).is_ok());
    assert!(!p.pool.repository.exists());
    let mut invalid = p.clone();
    invalid.members.pop();
    assert!(matches!(
        managed::Reader::new(invalid),
        Err(managed::Error::InvalidRoster)
    ));
    let mut invalid = p.clone();
    invalid.members[1] = invalid.members[0].clone();
    assert!(matches!(
        managed::Reader::new(invalid),
        Err(managed::Error::InvalidRoster)
    ));
    #[cfg(not(windows))]
    assert!(matches!(
        managed::Reader::new(p)
            .unwrap()
            .open(&Object::from_bytes(b"x")),
        Err(managed::Error::Annex(Error::Unsupported))
    ));
}
#[cfg(windows)]
fn fixture_managed() -> (
    tempfile::TempDir,
    managed::Profile,
    Vec<Namespace>,
    Object,
    PathBuf,
) {
    let (root, pool, object, path) = fixture();
    let (profile, namespaces) = roster(pool, root.path());
    fs::write(
        root.path().join("binding"),
        profile.binding.id().unwrap().as_str(),
    )
    .unwrap();
    (root, profile, namespaces, object, path)
}
#[cfg(windows)]
#[test]
fn managed_read_holds_every_namespace_while_claim_adoption_remains_possible() {
    use git_asset_core::{
        LogicalPath,
        tree::{MemoryNodes, Tree, TreeLimits},
    };
    use git_asset_store::source_claims::{Command, admission::Select};
    let (root, profile, mut namespaces, object, path) = fixture_managed();
    let mut reader = managed::Reader::new(profile).unwrap();
    let mut held = reader.open(&object).unwrap();
    let mut bytes = vec![];
    held.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"source");
    assert_eq!(held.object(), &object);
    assert_eq!(held.held_namespaces(), 2);
    assert_eq!(reader.attempts(), 1);
    assert_eq!(reader.verification_bytes(), object.bytes);
    assert_eq!(
        fs::read_to_string(root.path().join("calls"))
            .unwrap()
            .lines()
            .count(),
        4
    );
    let through_port = existing::ByteSource::open_verified(&mut reader, &object).unwrap();
    assert_eq!(through_port.held_namespaces(), 2);
    drop(through_port);
    let mut nodes = MemoryNodes::default();
    let tree = Tree::build(
        &[(LogicalPath::parse("input").unwrap(), object.clone())]
            .into_iter()
            .collect(),
        &mut nodes,
        TreeLimits::default(),
    )
    .unwrap()
    .0;
    let limits = git_asset_store::protection::Limits {
        millis: 60000,
        roots: 16,
        bindings: 128,
        associations: 128,
        reads: 128,
        read_bytes: 65536,
    };
    for namespace in &mut namespaces {
        assert!(matches!(
            namespace.fence(&object),
            Err(git_asset_store::source_claims::admission::Error::Busy)
        ));
        namespace
            .apply(
                key("enroll"),
                Command::Enroll {
                    source: key("job"),
                    owner: key("owner"),
                    class: Kind::Job,
                },
                &nodes,
                &limits,
            )
            .unwrap();
        namespace
            .select(
                &Select {
                    prepare: key("prepare"),
                    adopt: key("adopt"),
                    source: key("job"),
                    owner: key("owner"),
                    expected_revision: 1,
                    selection: git_asset_store::source_claims::Selection {
                        root: tree.root().clone(),
                        context: Digest::blob(b"request"),
                    },
                },
                &nodes,
                &limits,
            )
            .unwrap();
    }
    drop(reader);
    assert!(fs::remove_file(&path).is_err());
    drop(held);
    for namespace in &mut namespaces {
        drop(namespace.fence(&object).unwrap());
        assert!(
            namespace
                .source(&key("job"))
                .unwrap()
                .unwrap()
                .current
                .is_some()
        );
    }
}
#[cfg(windows)]
#[test]
fn wrong_binding_unknown_alias_and_failed_member_acquisition_refuse_and_release_guards() {
    let (root, profile, namespaces, object, _path) = fixture_managed();
    fs::write(
        root.path().join("binding"),
        Digest::blob(b"different binding").as_str(),
    )
    .unwrap();
    assert!(matches!(
        managed::Reader::new(profile).unwrap().open(&object),
        Err(managed::Error::WrongBinding)
    ));
    assert_eq!(
        fs::read_to_string(root.path().join("calls"))
            .unwrap()
            .lines()
            .count(),
        3
    );
    for namespace in namespaces {
        drop(namespace.fence(&object).unwrap());
    }

    let (root, mut profile, namespaces, object, _) = fixture_managed();
    let members = profile
        .binding
        .members()
        .iter()
        .map(|m| BindingMember {
            namespace: m.namespace.clone(),
            places: vec![key("unknown")],
        })
        .collect();
    profile.binding = Binding::new(1, profile.binding.boundary().clone(), members).unwrap();
    fs::write(
        root.path().join("binding"),
        profile.binding.id().unwrap().as_str(),
    )
    .unwrap();
    assert!(matches!(
        managed::Reader::new(profile).unwrap().open(&object),
        Err(managed::Error::InvalidRoster)
    ));
    for namespace in namespaces {
        drop(namespace.fence(&object).unwrap());
    }

    let (_root, profile, namespaces, object, _) = fixture_managed();
    let last = profile.binding.members().last().unwrap().namespace.clone();
    let blocked = namespaces.iter().find(|n| n.identity() == &last).unwrap();
    let exclusive = blocked.fence(&object).unwrap();
    assert!(matches!(
        managed::Reader::new(profile).unwrap().open(&object),
        Err(managed::Error::Admission(
            git_asset_store::source_claims::admission::Error::Busy
        ))
    ));
    for namespace in namespaces.iter().filter(|n| n.identity() != &last) {
        drop(namespace.fence(&object).unwrap());
    }
    drop(exclusive);
}
#[cfg(windows)]
#[test]
fn corrupt_payload_and_async_context_do_not_weaken_managed_admission() {
    let (_root, profile, namespaces, object, path) = fixture_managed();
    fs::write(path, b"wrong!").unwrap();
    assert!(matches!(
        managed::Reader::new(profile).unwrap().open(&object),
        Err(managed::Error::Annex(Error::Bytes(
            existing::Error::Integrity
        )))
    ));
    for namespace in namespaces {
        drop(namespace.fence(&object).unwrap());
    }
    let (root, profile, _namespaces, object, _) = fixture_managed();
    let mut reader = managed::Reader::new(profile).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            assert!(matches!(
                reader.open(&object),
                Err(managed::Error::Annex(Error::Unsupported))
            ));
        });
    assert!(!root.path().join("calls").exists());
}
