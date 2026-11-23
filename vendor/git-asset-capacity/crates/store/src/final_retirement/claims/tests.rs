use super::*;
use crate::{
    AckLevel, Key, Request,
    local::LocalStore,
    publish,
    source_claims::{
        Checkpoint, Command, Kind, Places, Selection,
        admission::{Policy, Select},
    },
    store_protection::{self, Enrollment, Member},
};
use git_asset_core::{
    LogicalPath,
    tree::{MemoryNodes, Tree, TreeError, TreeLimits, TreeResult},
};
use std::{
    cell::Cell, collections::BTreeMap, fs, io::Cursor, path::PathBuf, process::Command as Process,
    time::Duration,
};

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn digest_text(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn limits() -> Limits {
    Limits {
        millis: 60000,
        roots: 256,
        bindings: 10000,
        associations: 100000,
        reads: 100000,
        read_bytes: 32 * 1024 * 1024,
    }
}
fn policy() -> Policy {
    let active = Places {
        current: vec![key("F")],
        pending: vec![key("F")],
        predecessor: vec![key("D")],
    };
    let retained = Places {
        current: vec![key("M")],
        pending: vec![key("M")],
        predecessor: vec![key("M")],
    };
    Policy {
        prefer_active: true,
        classes: [
            (Kind::Branch, active.clone()),
            (Kind::Draft, active.clone()),
            (Kind::Job, active),
            (Kind::Retention, retained),
        ]
        .into_iter()
        .collect(),
    }
}
fn tree(nodes: &mut impl NodeStore, object: &Object) -> Selection {
    let (tree, _) = Tree::build(
        &[(LogicalPath::parse("entry").unwrap(), object.clone())]
            .into_iter()
            .collect(),
        nodes,
        TreeLimits::default(),
    )
    .unwrap();
    Selection {
        root: tree.root().clone(),
        context: digest_text("fixture-flat-closure"),
    }
}
fn enroll(ns: &mut Namespace, nodes: &impl NodeStore, name: &str, kind: Kind) {
    ns.apply(
        key(&format!("enroll-{name}")),
        Command::Enroll {
            source: key(name),
            owner: key("owner"),
            class: kind,
        },
        nodes,
        &limits(),
    )
    .unwrap();
}
fn select(
    ns: &mut Namespace,
    nodes: &impl NodeStore,
    source: &str,
    selection: Selection,
    label: &str,
) {
    let revision = ns.source(&key(source)).unwrap().unwrap().revision;
    ns.select(
        &Select {
            prepare: key(&format!("prepare-{label}")),
            adopt: key(&format!("adopt-{label}")),
            source: key(source),
            owner: key("owner"),
            expected_revision: revision,
            selection,
        },
        nodes,
        &limits(),
    )
    .unwrap();
}
fn prepare(
    ns: &mut Namespace,
    nodes: &impl NodeStore,
    source: &str,
    selection: Selection,
    op: &str,
) -> admission::Result<crate::source_claims::Receipt> {
    let revision = ns.source(&key(source))?.unwrap().revision;
    ns.apply(
        key(op),
        Command::Prepare {
            source: key(source),
            owner: key("owner"),
            expected_revision: revision,
            selection,
        },
        nodes,
        &limits(),
    )
}
struct Fixture {
    temp: tempfile::TempDir,
    namespaces: Vec<Namespace>,
    paths: BTreeMap<Digest, PathBuf>,
    roster: Roster,
    nodes: MemoryNodes,
    object: Object,
    selection: Selection,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut nodes = MemoryNodes::default();
        let object = Object::from_bytes(b"retirement-target");
        let selection = tree(&mut nodes, &object);
        let mut namespaces = Vec::new();
        let mut paths = BTreeMap::new();
        for i in 0..2 {
            let path = temp.path().join(format!("namespace-{i}"));
            let mut ns = Namespace::create(&path, key("project"), key("epoch"), policy()).unwrap();
            enroll(&mut ns, &nodes, "main", Kind::Branch);
            paths.insert(ns.identity().clone(), path);
            namespaces.push(ns);
        }
        let roster = Roster::flat(
            digest_text("final-authority"),
            digest_text("admitted-epoch"),
            Some(digest_text("complete-flat-registration-boundary")),
            namespaces
                .iter()
                .map(|n| NamespacePin::current(n).unwrap())
                .collect(),
            limits(),
        )
        .unwrap();
        Self {
            temp,
            namespaces,
            paths,
            roster,
            nodes,
            object,
            selection,
        }
    }
    fn peer(&self, index: usize) -> Namespace {
        Namespace::open(
            &self.paths[self.namespaces[index].identity()],
            &self.namespaces[index].checkpoint(),
        )
        .unwrap()
    }
    fn binding(&self, indices: &[usize]) -> Binding {
        Binding::new(
            1,
            digest_text("fixture-provider-boundary"),
            indices
                .iter()
                .map(|i| Member {
                    namespace: self.namespaces[*i].identity().clone(),
                    places: vec![key("M")],
                })
                .collect(),
        )
        .unwrap()
    }
}

#[test]
fn a_job_requiring_f_blocks_final_clearance_even_when_m_is_locally_surplus() {
    let mut f = Fixture::new();
    enroll(&mut f.namespaces[0], &f.nodes, "job", Kind::Job);
    select(
        &mut f.namespaces[0],
        &f.nodes,
        "job",
        f.selection.clone(),
        "job",
    );
    let mut store =
        LocalStore::create_protected(&f.temp.path().join("M"), key("M"), f.binding(&[0, 1]))
            .unwrap();
    publish(
        &mut store,
        &Request {
            operation: key("copy"),
            owner: key("owner"),
            object: f.object.clone(),
            required: AckLevel::ProcessRestart,
        },
        &mut Cursor::new(b"retirement-target"),
    )
    .unwrap();
    let enrollment = Enrollment::bind(&store).unwrap();
    let ordinary = store_protection::assess(
        &store,
        &enrollment,
        &f.object,
        &mut f.namespaces,
        &f.nodes,
        &limits(),
    )
    .unwrap();
    assert!(!ordinary.summary().removal_authority);
    drop(ordinary);
    assert!(
        f.roster
            .covers_binding(store.protection().unwrap())
            .unwrap()
    );
    assert!(matches!(
        acquire(&f.roster, &f.object, &mut f.namespaces, &f.nodes),
        Err(Error::Protected)
    ));
    assert_eq!(
        fs::read(
            f.temp
                .path()
                .join("M/objects")
                .join(f.object.sha256.as_str())
        )
        .unwrap(),
        b"retirement-target"
    );
}

#[test]
fn all_source_classes_and_current_pending_history_membership_hold() {
    for kind in [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention] {
        for role in ["pending", "current", "history"] {
            let mut f = Fixture::new();
            enroll(&mut f.namespaces[1], &f.nodes, "protected", kind.clone());
            if role == "pending" {
                prepare(
                    &mut f.namespaces[1],
                    &f.nodes,
                    "protected",
                    f.selection.clone(),
                    "pending",
                )
                .unwrap();
            } else {
                let s = Select {
                    prepare: key("protected-prepare"),
                    adopt: key("protected-adopt"),
                    source: key("protected"),
                    owner: key("owner"),
                    expected_revision: 1,
                    selection: f.selection.clone(),
                };
                if role == "history" {
                    f.namespaces[1]
                        .select_predecessor(&s, key("protected-withdraw"), &f.nodes, &limits())
                        .unwrap();
                } else {
                    f.namespaces[1].select(&s, &f.nodes, &limits()).unwrap();
                }
            }
            assert!(
                matches!(
                    acquire(&f.roster, &f.object, &mut f.namespaces, &f.nodes),
                    Err(Error::Protected)
                ),
                "{kind:?}/{role}"
            );
        }
    }
}

#[test]
fn another_authority_outside_the_target_store_binding_still_protects_content() {
    let mut f = Fixture::new();
    let binding = f.binding(&[0]);
    assert!(f.roster.covers_binding(&binding).unwrap());
    f.namespaces[1]
        .apply(
            key("enroll-foreign"),
            Command::Enroll {
                source: key("foreign"),
                owner: key("other-owner"),
                class: Kind::Job,
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    f.namespaces[1]
        .select(
            &Select {
                prepare: key("foreign-prepare"),
                adopt: key("foreign-adopt"),
                source: key("foreign"),
                owner: key("other-owner"),
                expected_revision: 1,
                selection: f.selection.clone(),
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    assert!(matches!(
        acquire(&f.roster, &f.object, &mut f.namespaces, &f.nodes),
        Err(Error::Protected)
    ));
    let mut smaller = f.roster.clone();
    smaller
        .members
        .retain(|p| p.namespace == *f.namespaces[0].identity());
    assert_ne!(smaller.scope().unwrap(), f.roster.scope().unwrap());
    assert!(!smaller.covers_binding(&f.binding(&[0, 1])).unwrap());
}

#[test]
fn only_explicit_retirement_of_every_owned_root_clears_shared_content() {
    let mut f = Fixture::new();
    for ns in &mut f.namespaces {
        ns.select_predecessor(
            &Select {
                prepare: key("history-prepare"),
                adopt: key("history-adopt"),
                source: key("main"),
                owner: key("owner"),
                expected_revision: 1,
                selection: f.selection.clone(),
            },
            key("history-withdraw"),
            &f.nodes,
            &limits(),
        )
        .unwrap();
    }
    for index in 0..2 {
        assert!(matches!(
            acquire(&f.roster, &f.object, &mut f.namespaces, &f.nodes),
            Err(Error::Protected)
        ));
        f.namespaces[index]
            .apply(
                key("retire-own-history"),
                Command::Retire {
                    source: key("main"),
                    owner: key("owner"),
                    expected_revision: 4,
                    predecessor_revision: 3,
                },
                &f.nodes,
                &limits(),
            )
            .unwrap();
    }
    let clearance = acquire(&f.roster, &f.object, &mut f.namespaces, &f.nodes).unwrap();
    assert_eq!(clearance.summary().checked.len(), 2);
    assert_eq!(clearance.object(), &f.object);
    assert!(!clearance.summary().authorizes_removal);
}

#[test]
fn strict_roster_scope_and_unknown_missing_extra_or_policy_substitution_hold() {
    let mut f = Fixture::new();
    let raw = f.roster.to_json().unwrap();
    assert_eq!(Roster::from_json(&raw).unwrap(), f.roster);
    let mut noncanonical = raw.clone();
    noncanonical.push(b'\n');
    assert!(Roster::from_json(&noncanonical).is_err());
    let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    value["unknown"] = true.into();
    assert!(Roster::from_json(&identity::canonical_json(&value).unwrap()).is_err());
    let mut unknown = f.roster.clone();
    unknown.coverage = None;
    assert!(matches!(
        acquire(&unknown, &f.object, &mut f.namespaces, &f.nodes),
        Err(Error::UnknownCoverage)
    ));
    let mut duplicate = f.roster.clone();
    duplicate.members[1] = duplicate.members[0].clone();
    assert!(duplicate.id().is_err());
    assert!(matches!(
        acquire(&f.roster, &f.object, &mut f.namespaces[..1], &f.nodes),
        Err(Error::IncompleteRoster)
    ));
    let mut wrong = f.roster.clone();
    wrong.members[0].policy = digest_text("wrong-policy");
    assert!(matches!(
        acquire(&wrong, &f.object, &mut f.namespaces, &f.nodes),
        Err(Error::WrongEnrollment)
    ));
    let mut extra = f.roster.clone();
    extra.members.pop();
    assert!(matches!(
        acquire(&extra, &f.object, &mut f.namespaces, &f.nodes),
        Err(Error::IncompleteRoster)
    ));
    let mut different_budget = f.roster.clone();
    different_budget.limits.reads -= 1;
    assert_ne!(different_budget.id().unwrap(), f.roster.id().unwrap());
    assert_eq!(different_budget.scope().unwrap(), f.roster.scope().unwrap());
}

#[test]
fn independent_canonical_vector_pins_scope_without_budgets_or_minima() {
    let value: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/vectors/final-claim-roster.design-1.json"
    )))
    .unwrap();
    let raw = value["canonical_roster"].as_str().unwrap().as_bytes();
    let roster = Roster::from_json(raw).unwrap();
    assert_eq!(
        roster.id().unwrap().as_str(),
        value["roster_sha256"].as_str().unwrap()
    );
    assert_eq!(
        roster.scope().unwrap().as_str(),
        value["scope_sha256"].as_str().unwrap()
    );
    assert_eq!(
        Digest::blob(value["canonical_scope"].as_str().unwrap().as_bytes()).as_str(),
        value["scope_sha256"].as_str().unwrap()
    );
    let mut adjusted = roster.clone();
    adjusted.limits.reads = 128;
    let mut minimum = serde_json::to_value(&adjusted.members[0].minimum).unwrap();
    minimum["sequence"] = 1.into();
    minimum["hash"] = "7".repeat(64).into();
    adjusted.members[0].minimum = serde_json::from_value(minimum).unwrap();
    assert_ne!(adjusted.id().unwrap(), roster.id().unwrap());
    assert_eq!(adjusted.scope().unwrap(), roster.scope().unwrap());
}

#[test]
fn held_clearance_blocks_new_target_demand_but_allows_other_partitions_and_heads() {
    let mut f = Fixture::new();
    let mut peer = f.peer(0);
    let other = (0..1000)
        .map(|i| Object::from_bytes(format!("unrelated-{i}").as_bytes()))
        .find(|o| o.sha256.as_str()[..2] != f.object.sha256.as_str()[..2])
        .unwrap();
    let selection = tree(&mut f.nodes, &other);
    // Caller handle order is irrelevant; guards/checked summaries use roster order.
    f.namespaces.reverse();
    let guard = acquire(&f.roster, &f.object, &mut f.namespaces, &f.nodes).unwrap();
    assert_eq!(guard.summary().checked.len(), 2);
    assert!(!guard.summary().authorizes_removal);
    assert!(!guard.summary().authorizes_retention_release);
    enroll(&mut peer, &f.nodes, "other", Kind::Job);
    select(&mut peer, &f.nodes, "other", selection, "other");
    guard.check_current().unwrap();
    assert_eq!(
        prepare(&mut peer, &f.nodes, "main", f.selection.clone(), "target"),
        Err(admission::Error::Busy)
    );
    let summary = serde_json::to_value(guard.summary()).unwrap();
    assert_eq!(summary["declared_flat_membership_clear"], true);
    drop(guard);
    prepare(&mut peer, &f.nodes, "main", f.selection.clone(), "target").unwrap();
    assert!(matches!(
        acquire(&f.roster, &f.object, &mut f.namespaces, &f.nodes),
        Err(Error::Protected)
    ));
}

#[test]
fn acquisition_failure_drops_earlier_subject_fences() {
    let mut f = Fixture::new();
    let first = f
        .namespaces
        .iter()
        .position(|n| n.identity() == &f.roster.members[0].namespace)
        .unwrap();
    let second = 1 - first;
    let mut peer = f.peer(first);
    let busy = f.namespaces[second].fence(&f.object).unwrap();
    assert!(matches!(
        acquire(&f.roster, &f.object, &mut f.namespaces, &f.nodes),
        Err(Error::Admission(admission::Error::Busy))
    ));
    prepare(
        &mut peer,
        &f.nodes,
        "main",
        f.selection.clone(),
        "after-failed-acquisition",
    )
    .unwrap();
    drop(busy);
}

struct Broken {
    corrupt: bool,
}
impl NodeStore for Broken {
    fn get(&self, _: &Object) -> TreeResult<Option<Vec<u8>>> {
        Ok(if self.corrupt {
            Some(b"wrong node".to_vec())
        } else {
            None
        })
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("clearance is read only")
    }
}
#[test]
fn incomplete_corrupt_conflicting_size_and_aggregate_budget_never_clear() {
    let mut f = Fixture::new();
    select(
        &mut f.namespaces[0],
        &f.nodes,
        "main",
        f.selection.clone(),
        "object",
    );
    let mut wrong_size = f.object.clone();
    wrong_size.bytes += 1;
    assert!(matches!(
        acquire(&f.roster, &wrong_size, &mut f.namespaces, &f.nodes),
        Err(Error::Admission(admission::Error::ConflictingSize))
    ));
    let absent = Object::from_bytes(b"not-in-roots");
    for corrupt in [false, true] {
        assert!(matches!(
            acquire(&f.roster, &absent, &mut f.namespaces, &Broken { corrupt }),
            Err(Error::Admission(admission::Error::IncompleteClosure))
        ));
    }
    select(
        &mut f.namespaces[1],
        &f.nodes,
        "main",
        f.selection.clone(),
        "object",
    );
    let mut small = f.roster.clone();
    small.limits.roots = 1;
    assert!(matches!(
        acquire(&small, &absent, &mut f.namespaces, &f.nodes),
        Err(Error::Admission(admission::Error::Budget))
    ));
    let guard = acquire(&f.roster, &absent, &mut f.namespaces, &f.nodes).unwrap();
    guard.check_current().unwrap();
}

struct Slow<'a> {
    nodes: &'a MemoryNodes,
    calls: Cell<usize>,
}

#[test]
fn admitted_minimum_refuses_restored_history_even_if_handle_used_weaker_minimum() {
    let mut f = Fixture::new();
    let old_scope = f.roster.scope().unwrap();
    let namespace = f.namespaces[0].identity().clone();
    let old: Vec<_> = f
        .namespaces
        .iter()
        .map(|n| (f.paths[n.identity()].clone(), n.checkpoint()))
        .collect();
    let database = f.paths[&namespace].join("claims.sqlite");
    let snapshot = f.temp.path().join("saved-prefix.sqlite");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute("VACUUM INTO ?1", [snapshot.to_str().unwrap()])
        .unwrap();
    drop(connection);
    let mut peer = f.peer(0);
    select(
        &mut peer,
        &f.nodes,
        "main",
        f.selection.clone(),
        "acknowledged-target",
    );
    let pin = f
        .roster
        .members
        .iter_mut()
        .find(|p| p.namespace == namespace)
        .unwrap();
    pin.minimum = peer.checkpoint();
    assert_eq!(f.roster.scope().unwrap(), old_scope);
    drop(peer);
    f.namespaces.clear();
    fs::copy(snapshot, &database).unwrap();
    f.namespaces = old
        .iter()
        .map(|(root, min)| Namespace::open(root, min).unwrap())
        .collect();
    assert!(matches!(
        acquire(&f.roster, &f.object, &mut f.namespaces, &f.nodes),
        Err(Error::Admission(admission::Error::Journal(
            crate::source_claims::Error::Rollback
        )))
    ));
    let mut value = serde_json::to_value(&old[0].1).unwrap();
    value["hash"] = digest_text("forged-chain").as_str().into();
    f.roster
        .members
        .iter_mut()
        .find(|p| p.namespace == namespace)
        .unwrap()
        .minimum = serde_json::from_value(value).unwrap();
    assert!(matches!(
        acquire(&f.roster, &f.object, &mut f.namespaces, &f.nodes),
        Err(Error::Admission(admission::Error::Journal(
            crate::source_claims::Error::Rollback
        )))
    ));
}
impl NodeStore for Slow<'_> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.calls.set(self.calls.get() + 1);
        std::thread::sleep(Duration::from_millis(20));
        self.nodes.get(object)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Err(TreeError::Store("read only".into()))
    }
}
#[test]
fn deadline_covers_provider_reads_and_marker_drift_invalidates_live_clearance() {
    let mut f = Fixture::new();
    select(
        &mut f.namespaces[0],
        &f.nodes,
        "main",
        f.selection.clone(),
        "source",
    );
    let absent = Object::from_bytes(b"absent");
    let mut brief = f.roster.clone();
    brief.limits.millis = 1;
    let slow = Slow {
        nodes: &f.nodes,
        calls: Cell::new(0),
    };
    assert!(matches!(
        acquire(&brief, &absent, &mut f.namespaces, &slow),
        Err(Error::Admission(admission::Error::Budget))
    ));
    assert!(slow.calls.get() <= 1);
    let path = f.paths[f.namespaces[0].identity()].join("claims.json");
    let saved = fs::read(&path).unwrap();
    let guard = acquire(&f.roster, &absent, &mut f.namespaces, &f.nodes).unwrap();
    fs::write(&path, b"{}").unwrap();
    assert!(guard.check_current().is_err());
    fs::write(path, saved).unwrap();
    guard.check_current().unwrap();
}

#[derive(Serialize, Deserialize)]
struct ChildInput {
    roster: Roster,
    object: Object,
    namespaces: Vec<(PathBuf, Checkpoint)>,
}
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn clearance_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_FINAL_CLAIMS_CHILD") else {
        return;
    };
    let root = PathBuf::from(root);
    let input: ChildInput =
        serde_json::from_slice(&fs::read(root.join("child-input.json")).unwrap()).unwrap();
    let mut namespaces: Vec<_> = input
        .namespaces
        .iter()
        .map(|(p, c)| Namespace::open(p, c).unwrap())
        .collect();
    let nodes = MemoryNodes::default();
    let guard = acquire(&input.roster, &input.object, &mut namespaces, &nodes).unwrap();
    fs::write(root.join("ready"), b"ready").unwrap();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(15) {
        guard.check_current().unwrap();
        if root.join("exit").exists() {
            std::process::exit(78);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("parent did not finish the isolated child fixture");
}
#[test]
fn actual_process_holds_all_namespaces_and_exit_releases_only_mutexes() {
    let mut f = Fixture::new();
    let input = ChildInput {
        roster: f.roster.clone(),
        object: f.object.clone(),
        namespaces: f
            .namespaces
            .iter()
            .map(|n| (f.paths[n.identity()].clone(), n.checkpoint()))
            .collect(),
    };
    fs::write(
        f.temp.path().join("child-input.json"),
        serde_json::to_vec(&input).unwrap(),
    )
    .unwrap();
    let mut child = Child(
        Process::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "final_retirement::claims::tests::clearance_child",
                "--nocapture",
            ])
            .env("GIT_ASSET_FINAL_CLAIMS_CHILD", f.temp.path())
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    while !f.temp.path().join("ready").exists() {
        assert!(child.0.try_wait().unwrap().is_none());
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(20));
    }
    for n in &mut f.namespaces {
        assert_eq!(
            prepare(n, &f.nodes, "main", f.selection.clone(), "late-target"),
            Err(admission::Error::Busy)
        );
    }
    fs::write(f.temp.path().join("exit"), b"exit").unwrap();
    assert_eq!(child.0.wait().unwrap().code(), Some(78));
    for n in &mut f.namespaces {
        prepare(n, &f.nodes, "main", f.selection.clone(), "late-target").unwrap();
    }
    assert!(matches!(
        acquire(&f.roster, &f.object, &mut f.namespaces, &f.nodes),
        Err(Error::Protected)
    ));
}
