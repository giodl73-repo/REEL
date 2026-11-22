//! Real temporary metadata/namespace claims; no payload custody or cache setup.
use git_asset_core::{
    Digest, LogicalPath, Object,
    tree::{Tree, TreeLimits},
};
use git_asset_store::{
    Key,
    metadata::{self, MetadataReader, MetadataStore},
    protection::Limits,
    source_claims::{
        Checkpoint, Command, Kind, Places, Selection,
        admission::{Namespace, Policy, Select},
    },
};
use reel_workflow::ErrorKind;
use reel_workflow_git_asset::claims::{Handle, State};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::Path,
    process::{Command as Process, ExitStatus, Stdio},
    time::{Duration, Instant},
};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn limits() -> Limits {
    Limits {
        millis: 60_000,
        roots: 128,
        bindings: 4096,
        associations: 10000,
        reads: 10000,
        read_bytes: 4 * 1024 * 1024,
    }
}
fn policy() -> Policy {
    let places = Places {
        current: vec![key("working"), key("backup")],
        pending: vec![key("working"), key("backup")],
        predecessor: vec![key("backup")],
    };
    Policy {
        prefer_active: true,
        classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
            .into_iter()
            .map(|kind| (kind, places.clone()))
            .collect(),
    }
}
struct Fixture {
    temp: tempfile::TempDir,
    namespace: Namespace,
    nodes: MetadataReader,
    selection: Selection,
    object: Object,
    metadata_checkpoint: metadata::Checkpoint,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::Builder::new()
            .prefix("reel-public-job-fixture-")
            .tempdir()
            .unwrap();
        let mut namespace = Namespace::create(
            &temp.path().join("claims"),
            key("project"),
            key("epoch"),
            policy(),
        )
        .unwrap();
        let mut writer = MetadataStore::create(
            &temp.path().join("nodes"),
            metadata::Limits {
                nodes: 10000,
                bytes: 4 * 1024 * 1024,
                database_bytes: 16 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let object =
            Object::from_bytes(b"synthetic input payload, deliberately not stored as metadata");
        let entries = BTreeMap::from([(LogicalPath::parse("input/item").unwrap(), object.clone())]);
        let (tree, _) = Tree::build(&entries, &mut writer, TreeLimits::default()).unwrap();
        let selection = Selection {
            root: tree.root().clone(),
            context: Digest::blob(b"immutable task/attempt binding"),
        };
        let metadata_checkpoint = writer.finish().unwrap();
        drop(writer);
        let nodes = MetadataReader::open(&temp.path().join("nodes"), &metadata_checkpoint).unwrap();
        namespace
            .apply(
                key("enroll-attempt"),
                Command::Enroll {
                    source: key("attempt"),
                    owner: key("owner"),
                    class: Kind::Job,
                },
                &nodes,
                &limits(),
            )
            .unwrap();
        Self {
            temp,
            namespace,
            nodes,
            selection,
            object,
            metadata_checkpoint,
        }
    }
    fn request(&mut self) -> Select {
        Select {
            prepare: key("prepare-attempt"),
            adopt: key("adopt-attempt"),
            source: key("attempt"),
            owner: key("owner"),
            expected_revision: self
                .namespace
                .source(&key("attempt"))
                .unwrap()
                .unwrap()
                .revision,
            selection: self.selection.clone(),
        }
    }
    fn handle(&mut self) -> Handle {
        let r = self.request();
        Handle::prepare(&mut self.namespace, r).unwrap()
    }
    fn peer(&self, minimum: &Checkpoint) -> Namespace {
        Namespace::open(&self.temp.path().join("claims"), minimum).unwrap()
    }
    fn required(&mut self, place: &str) -> bool {
        self.namespace
            .requirements(&self.nodes, &limits())
            .unwrap()
            .entries
            .iter()
            .any(|a| a.object == self.object && a.places.contains(&key(place)))
    }
}
#[test]
fn persistent_claim_reopen_status_cancel_and_exact_retry_preserve_history() {
    let mut f = Fixture::new();
    let handle = f.handle();
    let bytes = handle.encode().unwrap();
    let object = Object::from_bytes(&bytes);
    assert_eq!(
        handle.status(&mut f.namespace).unwrap().state,
        State::Unapplied
    );
    let first = handle.admit(&mut f.namespace, &f.nodes, &limits()).unwrap();
    assert_eq!(first.state, State::Active);
    assert!(f.required("working"));
    assert!(f.required("backup"));
    let mut peer = f.peer(handle.minimum());
    let recovered = Handle::decode(&bytes, &object).unwrap();
    let before = peer.checkpoint();
    for _ in 0..3 {
        assert_eq!(recovered.status(&mut peer).unwrap().state, State::Active);
    }
    assert_eq!(peer.checkpoint(), before); // Renewal/status appends nothing; no timer.
    let replay = recovered.admit(&mut peer, &f.nodes, &limits()).unwrap();
    assert_eq!(replay.state, State::Active);
    assert!(replay.receipt.unwrap().replayed);
    assert_eq!(peer.checkpoint(), before);
    let cancelled = recovered
        .cancel(&mut peer, key("cancel-attempt"), &f.nodes, &limits())
        .unwrap();
    assert_eq!(cancelled.state, State::Historical);
    let after = peer.checkpoint();
    recovered
        .cancel(&mut peer, key("cancel-attempt"), &f.nodes, &limits())
        .unwrap();
    assert_eq!(peer.checkpoint(), after);
    // Cancellation changes proximity policy but does not retire the predecessor.
    assert!(!f.required("working"));
    assert!(f.required("backup"));
    assert_eq!(
        recovered.status(&mut f.namespace).unwrap().state,
        State::Historical
    );
    assert_eq!(
        recovered
            .admit(&mut f.namespace, &f.nodes, &limits())
            .unwrap()
            .state,
        State::Historical
    );
    assert_eq!(
        recovered
            .cancel(&mut f.namespace, key("other-cancel"), &f.nodes, &limits())
            .unwrap_err()
            .kind,
        ErrorKind::Stale
    );
}
#[test]
fn advanced_writer_is_not_reactivated_by_old_receipt_or_cancellation() {
    let mut f = Fixture::new();
    let original = f.handle();
    original
        .admit(&mut f.namespace, &f.nodes, &limits())
        .unwrap();
    let state = f.namespace.source(&key("attempt")).unwrap().unwrap();
    let next = Select {
        prepare: key("new-prepare"),
        adopt: key("new-adopt"),
        source: key("attempt"),
        owner: key("owner"),
        expected_revision: state.revision,
        selection: Selection {
            root: f.selection.root.clone(),
            context: Digest::blob(b"new task binding"),
        },
    };
    f.namespace.select(&next, &f.nodes, &limits()).unwrap();
    let current = f.namespace.source(&key("attempt")).unwrap().unwrap();
    assert_eq!(
        original
            .admit(&mut f.namespace, &f.nodes, &limits())
            .unwrap()
            .state,
        State::Historical
    );
    assert_eq!(
        original
            .cancel(&mut f.namespace, key("stale-cancel"), &f.nodes, &limits())
            .unwrap_err()
            .kind,
        ErrorKind::Stale
    );
    assert_eq!(
        f.namespace.source(&key("attempt")).unwrap().unwrap(),
        current
    );
}
#[test]
fn wrong_authority_owner_class_handle_and_missing_metadata_hold_existing_state() {
    let mut f = Fixture::new();
    let handle = f.handle();
    let before = f.namespace.checkpoint();
    let mut request = f.request();
    request.owner = key("other-owner");
    assert_eq!(
        Handle::prepare(&mut f.namespace, request).unwrap_err().kind,
        ErrorKind::Denied
    );
    assert_eq!(f.namespace.checkpoint(), before);
    let mut missing = f.request();
    missing.selection.root = Object::from_bytes(b"missing metadata");
    let missing = Handle::prepare(&mut f.namespace, missing).unwrap();
    assert_eq!(
        missing
            .admit(&mut f.namespace, &f.nodes, &limits())
            .unwrap_err()
            .kind,
        ErrorKind::NeedsRecovery
    );
    assert_eq!(
        handle.status(&mut f.namespace).unwrap().state,
        State::Unapplied
    );
    assert_eq!(f.namespace.checkpoint(), before);
    assert_eq!(
        handle
            .cancel(&mut f.namespace, key("not-yet"), &f.nodes, &limits())
            .unwrap_err()
            .kind,
        ErrorKind::Missing
    );
    let bytes = handle.encode().unwrap();
    let expected = Object::from_bytes(&bytes);
    let mut wrong = bytes.clone();
    wrong.push(b' ');
    assert_eq!(
        Handle::decode(&wrong, &expected).unwrap_err().kind,
        ErrorKind::Integrity
    );
    assert_eq!(
        Handle::decode(&vec![b' '; 16385], &expected)
            .unwrap_err()
            .kind,
        ErrorKind::Budget
    );
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["extra"] = serde_json::json!(true);
    let bytes = serde_json::to_vec(&value).unwrap();
    assert_eq!(
        Handle::decode(&bytes, &Object::from_bytes(&bytes))
            .unwrap_err()
            .kind,
        ErrorKind::Invalid
    );
    let other = Namespace::create(
        &f.temp.path().join("other"),
        key("project"),
        key("other-epoch"),
        policy(),
    )
    .unwrap();
    drop(other);
    let other = Namespace::open(&f.temp.path().join("other"), handle.minimum());
    assert!(other.is_err());
    let mut other = Namespace::create(
        &f.temp.path().join("third"),
        key("project"),
        key("third-epoch"),
        policy(),
    )
    .unwrap();
    assert!(handle.status(&mut other).is_err());
    // A Branch is not a Job even if its root happens to be identical.
    f.namespace
        .apply(
            key("enroll-branch"),
            Command::Enroll {
                source: key("branch"),
                owner: key("owner"),
                class: Kind::Branch,
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    let mut request = f.request();
    request.source = key("branch");
    request.expected_revision = 1;
    assert_eq!(
        Handle::prepare(&mut f.namespace, request).unwrap_err().kind,
        ErrorKind::Denied
    );
}
#[test]
fn same_object_retirement_fence_blocks_admission_and_unrelated_jobs_progress() {
    let mut f = Fixture::new();
    let handle = f.handle();
    let peer = f.peer(handle.minimum());
    let fence = peer.fence(&f.object).unwrap();
    assert_eq!(
        handle
            .admit(&mut f.namespace, &f.nodes, &limits())
            .unwrap_err()
            .kind,
        ErrorKind::Busy
    );
    f.namespace
        .apply(
            key("enroll-unrelated"),
            Command::Enroll {
                source: key("unrelated"),
                owner: key("other"),
                class: Kind::Job,
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    drop(fence);
    assert_eq!(
        handle
            .admit(&mut f.namespace, &f.nodes, &limits())
            .unwrap()
            .state,
        State::Active
    );
}
fn child(path: &Path) -> ExitStatus {
    let mut child = Process::new(std::env::current_exe().unwrap())
        .args(["--exact", "claim_child_exit_worker", "--ignored"])
        .env("REEL_PUBLIC_JOB_FIXTURE", path)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if start.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("owned claim fixture timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Case {
    handle: Vec<u8>,
    handle_object: Object,
    metadata: metadata::Checkpoint,
    minimum: Checkpoint,
    cancel: bool,
}
#[test]
fn original_claim_survives_actual_process_exit_without_rust_drop() {
    let mut f = Fixture::new();
    let handle = f.handle();
    let bytes = handle.encode().unwrap();
    let case = Case {
        handle_object: Object::from_bytes(&bytes),
        handle: bytes,
        metadata: f.metadata_checkpoint.clone(),
        minimum: handle.minimum().clone(),
        cancel: false,
    };
    let casepath = f.temp.path().join("case.json");
    fs::write(&casepath, serde_json::to_vec(&case).unwrap()).unwrap();
    let status = child(&casepath);
    assert_eq!(status.code(), Some(73));
    let mut peer = f.peer(handle.minimum());
    let recovered = handle.status(&mut peer).unwrap();
    assert_eq!(recovered.state, State::Active);
    assert!(f.required("working"));
    assert!(f.required("backup"));
    assert!(
        handle
            .admit(&mut peer, &f.nodes, &limits())
            .unwrap()
            .receipt
            .unwrap()
            .replayed
    );
    let cancelled = Case {
        cancel: true,
        ..case
    };
    fs::write(&casepath, serde_json::to_vec(&cancelled).unwrap()).unwrap();
    let status = child(&casepath);
    assert_eq!(status.code(), Some(74));
    let mut peer = f.peer(handle.minimum());
    assert_eq!(handle.status(&mut peer).unwrap().state, State::Historical);
    assert!(!f.required("working"));
    assert!(f.required("backup"));
    assert_eq!(
        handle
            .cancel(&mut peer, key("child-cancel"), &f.nodes, &limits())
            .unwrap()
            .state,
        State::Historical
    );
}
#[test]
#[ignore = "only launched by the owned synthetic fixture"]
fn claim_child_exit_worker() {
    let path = std::env::var_os("REEL_PUBLIC_JOB_FIXTURE").expect("synthetic case required");
    let path = Path::new(&path);
    assert_eq!(path.file_name().unwrap(), "case.json");
    assert!(
        path.parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("reel-public-job-fixture-")
    );
    let mut raw = Vec::new();
    fs::File::open(path)
        .unwrap()
        .take(32769)
        .read_to_end(&mut raw)
        .unwrap();
    assert!(raw.len() <= 32768);
    let case: Case = serde_json::from_slice(&raw).unwrap();
    let root = path.parent().unwrap();
    let handle = Handle::decode(&case.handle, &case.handle_object).unwrap();
    let nodes = MetadataReader::open(&root.join("nodes"), &case.metadata).unwrap();
    let mut namespace = Namespace::open(&root.join("claims"), &case.minimum).unwrap();
    assert_eq!(
        handle
            .admit(&mut namespace, &nodes, &limits())
            .unwrap()
            .state,
        State::Active
    );
    if case.cancel {
        assert_eq!(
            handle
                .cancel(&mut namespace, key("child-cancel"), &nodes, &limits())
                .unwrap()
                .state,
            State::Historical
        );
        std::process::exit(74);
    }
    std::process::exit(73);
}

#[test]
fn two_writers_from_one_baseline_conflict_without_replacing_the_original_job() {
    let mut f = Fixture::new();
    let original = f.handle();
    let mut request = f.request();
    request.prepare = key("peer-prepare");
    request.adopt = key("peer-adopt");
    request.selection.context = Digest::blob(b"competing attempt identity");
    let contender = Handle::prepare(&mut f.namespace, request).unwrap();
    original
        .admit(&mut f.namespace, &f.nodes, &limits())
        .unwrap();
    let checkpoint = f.namespace.checkpoint();
    assert_eq!(
        contender
            .admit(&mut f.namespace, &f.nodes, &limits())
            .unwrap_err()
            .kind,
        ErrorKind::Stale
    );
    assert_eq!(f.namespace.checkpoint(), checkpoint);
    assert_eq!(
        original.status(&mut f.namespace).unwrap().state,
        State::Active
    );
    assert_eq!(
        contender.status(&mut f.namespace).unwrap().state,
        State::Unapplied
    );
}
#[test]
fn refusal_on_closure_budget_or_changed_operation_preserves_original_claim() {
    let mut f = Fixture::new();
    let original = f.handle();
    let before = f.namespace.checkpoint();
    let mut small = limits();
    small.reads = 1;
    small.read_bytes = 1;
    assert_eq!(
        original
            .admit(&mut f.namespace, &f.nodes, &small)
            .unwrap_err()
            .kind,
        ErrorKind::Budget
    );
    assert_eq!(f.namespace.checkpoint(), before);
    let mut changed = f.request();
    changed.selection.context = Digest::blob(b"changed operation");
    let changed = Handle::prepare(&mut f.namespace, changed).unwrap();
    original
        .admit(&mut f.namespace, &f.nodes, &limits())
        .unwrap();
    let checkpoint = f.namespace.checkpoint();
    assert_eq!(
        changed
            .admit(&mut f.namespace, &f.nodes, &limits())
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
    assert_eq!(f.namespace.checkpoint(), checkpoint);
    assert!(f.required("backup"));
}

#[test]
fn pending_new_generation_makes_original_receipt_advanced_without_releasing_it() {
    let mut f = Fixture::new();
    let original = f.handle();
    original
        .admit(&mut f.namespace, &f.nodes, &limits())
        .unwrap();
    let current = f.namespace.source(&key("attempt")).unwrap().unwrap();
    f.namespace
        .apply(
            key("pending-new-generation"),
            Command::Prepare {
                source: key("attempt"),
                owner: key("owner"),
                expected_revision: current.revision,
                selection: Selection {
                    root: f.selection.root.clone(),
                    context: Digest::blob(b"pending binding"),
                },
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    let pending = f.namespace.source(&key("attempt")).unwrap().unwrap();
    let checkpoint = f.namespace.checkpoint();
    assert_eq!(
        original.status(&mut f.namespace).unwrap().state,
        State::Advanced
    );
    assert_eq!(
        original
            .admit(&mut f.namespace, &f.nodes, &limits())
            .unwrap()
            .state,
        State::Advanced
    );
    assert_eq!(
        original
            .cancel(
                &mut f.namespace,
                key("old-generation-cancel"),
                &f.nodes,
                &limits()
            )
            .unwrap_err()
            .kind,
        ErrorKind::Stale
    );
    assert_eq!(
        f.namespace.source(&key("attempt")).unwrap().unwrap(),
        pending
    );
    assert_eq!(f.namespace.checkpoint(), checkpoint);
    assert!(f.required("backup"));
    assert!(f.required("working"));
}
