mod common;
use common::{git, git_executable, key};
use git_asset::{
    lifecycle::{Budget, Enrollment, Error, Execution, Plan},
    pointer::Pointer,
    source::{GitId, Limits, Repository},
};
use git_asset_core::{
    Digest, LogicalPath, Object, identity,
    tree::{MemoryNodes, NodeStore, Tree, TreeLimits},
};
use git_asset_store::{
    metadata::{Limits as MetadataLimits, MetadataStore},
    source_claims::{
        Command, Kind, Places,
        admission::{Namespace, Policy},
    },
};
use std::{fs, path::Path, process::Command as Process};

fn hash(value: &impl serde::Serialize) -> Digest {
    Digest::blob(&identity::canonical_json(&serde_json::to_value(value).unwrap()).unwrap())
}
fn policy() -> Policy {
    let places = Places {
        current: vec![key("working"), key("backup")],
        pending: vec![key("working"), key("backup")],
        predecessor: vec![key("history"), key("backup")],
    };
    Policy {
        prefer_active: true,
        classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
            .into_iter()
            .map(|k| (k, places.clone()))
            .collect(),
    }
}
struct Fixture {
    temp: tempfile::TempDir,
    repository: Repository,
    namespace: Namespace,
    metadata: MetadataStore,
    enrollment: Enrollment,
}
fn commit(root: &Path) -> GitId {
    git(root, &["add", "assets"]);
    git(
        root,
        &[
            "-c",
            "core.hooksPath=NUL",
            "commit",
            "-m",
            "synthetic selection",
        ],
    );
    GitId::parse(
        String::from_utf8(git(root, &["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim(),
    )
    .unwrap()
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::Builder::new()
            .prefix("git-asset lifecycle fixture ")
            .tempdir()
            .unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]);
        git(&repo, &["config", "user.name", "Fixture"]);
        git(&repo, &["config", "user.email", "fixture@example.invalid"]);
        fs::create_dir(repo.join("assets")).unwrap();
        for (name, bytes) in [
            ("shared.bin", b"shared".as_slice()),
            ("changed.bin", b"old".as_slice()),
        ] {
            fs::write(
                repo.join("assets").join(name),
                Pointer::new(Object::from_bytes(bytes)).to_bytes(),
            )
            .unwrap();
        }
        commit(&repo);
        let repository = Repository::new(git_executable(), repo, Limits::default()).unwrap();
        let mut namespace = Namespace::create(
            &temp.path().join("namespace"),
            key("project"),
            key("epoch"),
            policy(),
        )
        .unwrap();
        namespace
            .apply(
                key("enroll-main"),
                Command::Enroll {
                    source: key("main"),
                    owner: key("owner"),
                    class: Kind::Branch,
                },
                &MemoryNodes::default(),
                &admission_limits(),
            )
            .unwrap();
        let metadata = MetadataStore::create(
            &temp.path().join("metadata"),
            MetadataLimits {
                nodes: 10000,
                bytes: 16 * 1024 * 1024,
                database_bytes: 32 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let enrollment = Enrollment {
            repository: Digest::blob(b"enrolled repository"),
            authority: Digest::blob(b"trusted operator boundary"),
            namespace: namespace.identity().clone(),
            policy: hash(namespace.policy()),
            source: key("main"),
            owner: key("owner"),
            class: Kind::Branch,
            ref_name: "refs/heads/main".into(),
            scope: vec![LogicalPath::parse("assets").unwrap()],
            budget: Budget {
                entries: 1024,
                nodes: 10000,
                node_bytes: 16 * 1024 * 1024,
                millis: 60000,
            },
        };
        Self {
            temp,
            repository,
            namespace,
            metadata,
            enrollment,
        }
    }
    fn repo(&self) -> std::path::PathBuf {
        self.temp.path().join("repo")
    }
    fn execution(&mut self) -> Execution<'_> {
        Execution {
            enrollment: &self.enrollment,
            repository: &self.repository,
            namespace: &mut self.namespace,
            metadata: &mut self.metadata,
            allow_selection: true,
        }
    }
    fn plan(&mut self) -> Plan {
        self.execution().plan(None).unwrap()
    }
    fn update(&self, text: &[u8]) -> GitId {
        fs::write(
            self.repo().join("assets/changed.bin"),
            Pointer::new(Object::from_bytes(text)).to_bytes(),
        )
        .unwrap();
        commit(&self.repo())
    }
    fn absent(&mut self, object: &Object, place: &str) -> bool {
        let fence = self.namespace.fence(object).unwrap();
        self.namespace
            .absence(
                &fence,
                &key(place),
                &self.metadata.finished_reader().unwrap(),
                &admission_limits(),
            )
            .unwrap()
            .is_some()
    }
}
fn admission_limits() -> git_asset_store::protection::Limits {
    git_asset_store::protection::Limits {
        millis: 60000,
        roots: 256,
        bindings: 10000,
        associations: 100000,
        reads: 100000,
        read_bytes: 32 * 1024 * 1024,
    }
}

#[test]
fn committed_source_seam_preserves_original_git_listing_tree_and_context_identity() {
    use git_asset::source::Scope;
    let mut f = Fixture::new();
    let commit = f.repository.named_commit(&f.enrollment.ref_name).unwrap();
    let snapshot = f.repository.commit(commit.clone()).unwrap();
    let extraction = f
        .repository
        .extract(&snapshot, Scope::new(f.enrollment.scope.clone()).unwrap())
        .unwrap();
    let mut nodes = MemoryNodes::default();
    let root = extraction
        .write_tree(&mut nodes, TreeLimits::default())
        .unwrap()
        .root()
        .clone();
    let listing = snapshot.checkpoint().listing.clone();
    let expected = hash(
        &serde_json::json!({"schema":"tig.git-lifecycle-context.design-1","enrollment":f.enrollment.id().unwrap(),"commit":commit.as_str(),"listing":listing,"root":root}),
    );
    let before = f.namespace.checkpoint();
    let p = f.plan();
    assert_eq!(p.listing, listing);
    assert_eq!(p.selection.root, root);
    assert_eq!(p.selection.context, expected);
    assert_eq!(f.namespace.checkpoint(), before);
}

#[test]
fn real_merge_changes_desired_placement_preserves_history_and_duplicate_uses_no_commands() {
    let mut f = Fixture::new();
    let initial = f.plan();
    let before = f.namespace.checkpoint();
    let meta = f.metadata.checkpoint();
    assert_eq!(f.plan().to_json().unwrap(), initial.to_json().unwrap());
    assert_eq!(f.namespace.checkpoint(), before);
    assert_eq!(f.metadata.checkpoint(), meta);
    let applied = f.execution().apply(&initial).unwrap();
    assert!(applied.desired_current);
    assert_eq!(applied.storage_converged, None);
    git(&f.repo(), &["checkout", "-b", "feature"]);
    f.update(b"fixed");
    git(&f.repo(), &["checkout", "main"]);
    git(
        &f.repo(),
        &["merge", "--no-ff", "feature", "-m", "synthetic merge"],
    );
    let merged = f.plan();
    f.execution().apply(&merged).unwrap();
    assert!(!f.absent(&Object::from_bytes(b"fixed"), "working"));
    assert!(f.absent(&Object::from_bytes(b"old"), "working"));
    assert!(!f.absent(&Object::from_bytes(b"old"), "history"));
    assert!(f.absent(&Object::from_bytes(b"shared"), "history"));
    let checkpoint = f.namespace.checkpoint();
    let meta = f.metadata.checkpoint();
    let duplicate = f.plan();
    assert!(duplicate.no_op);
    f.execution().apply(&duplicate).unwrap();
    f.execution().apply(&merged).unwrap();
    assert_eq!(f.namespace.checkpoint(), checkpoint);
    assert_eq!(f.metadata.checkpoint(), meta);
    let historical = f.execution().status(&initial).unwrap();
    assert!(historical.historical.is_some());
    assert!(!historical.desired_current);
}
#[test]
fn active_job_retains_old_working_bytes_and_stale_events_cannot_replace_new_selection() {
    let mut f = Fixture::new();
    let first = f.plan();
    f.execution().apply(&first).unwrap();
    let old_commit = GitId::parse(&first.commit).unwrap();
    f.namespace
        .apply(
            key("enroll-job"),
            Command::Enroll {
                source: key("job"),
                owner: key("owner"),
                class: Kind::Job,
            },
            &MemoryNodes::default(),
            &admission_limits(),
        )
        .unwrap();
    let job = git_asset_store::source_claims::admission::Select {
        prepare: key("prepare-job"),
        adopt: key("adopt-job"),
        source: key("job"),
        owner: key("owner"),
        expected_revision: 1,
        selection: first.selection.clone(),
    };
    f.namespace
        .select(
            &job,
            &f.metadata.finished_reader().unwrap(),
            &admission_limits(),
        )
        .unwrap();
    let changed = f.update(b"second");
    let stale = f.plan();
    f.update(b"third");
    assert_eq!(f.execution().apply(&stale).unwrap_err(), Error::Stale);
    assert_eq!(
        f.execution().plan(Some(&changed)).unwrap_err(),
        Error::Stale
    );
    let fresh = f.plan();
    f.execution().apply(&fresh).unwrap();
    assert!(!f.absent(&Object::from_bytes(b"old"), "working"));
    assert!(f.absent(&Object::from_bytes(b"old"), "history"));
    let saved = f.namespace.checkpoint();
    assert_eq!(
        f.execution().plan(Some(&old_commit)).unwrap_err(),
        Error::Stale
    );
    let replay = f.execution().apply(&first).unwrap();
    assert!(replay.historical.is_some());
    assert!(!replay.desired_current);
    assert_eq!(f.namespace.checkpoint(), saved);
}
#[test]
fn git_revert_is_new_admitted_selection_with_old_bytes_and_new_receipts() {
    let mut f = Fixture::new();
    let first = f.plan();
    f.execution().apply(&first).unwrap();
    f.update(b"new");
    let second = f.plan();
    f.execution().apply(&second).unwrap();
    git(&f.repo(), &["revert", "--no-edit", "HEAD"]);
    let reverted = f.plan();
    assert_ne!(reverted.commit, first.commit);
    assert_eq!(reverted.selection.root, first.selection.root);
    assert_ne!(reverted.id().unwrap(), first.id().unwrap());
    f.execution().apply(&reverted).unwrap();
    assert!(!f.absent(&Object::from_bytes(b"old"), "working"));
    assert!(f.absent(&Object::from_bytes(b"new"), "working"));
    let saved = f.namespace.checkpoint();
    f.execution().apply(&second).unwrap();
    assert_eq!(f.namespace.checkpoint(), saved);
}
#[test]
fn incomplete_scope_wrong_enrollment_disabled_apply_and_changed_plan_hold_without_adoption() {
    let mut f = Fixture::new();
    let plan = f.plan();
    let saved = f.namespace.checkpoint();
    let mut execution = f.execution();
    execution.allow_selection = false;
    assert_eq!(execution.apply(&plan).unwrap_err(), Error::Disabled);
    f.enrollment.policy = Digest::blob(b"wrong policy");
    assert_eq!(f.execution().apply(&plan).unwrap_err(), Error::Authority);
    f.enrollment.policy = hash(f.namespace.policy());
    let mut v = serde_json::to_value(&plan).unwrap();
    v["no_op"] = true.into();
    let changed = Plan::from_json(&identity::canonical_json(&v).unwrap()).unwrap();
    assert_eq!(f.execution().apply(&changed).unwrap_err(), Error::Authority);
    fs::write(
        f.repo().join("assets/changed.bin"),
        b"unsupported ordinary bytes",
    )
    .unwrap();
    commit(&f.repo());
    assert_eq!(f.execution().plan(None).unwrap_err(), Error::Pending);
    assert_eq!(f.namespace.checkpoint(), saved);
    let mut bytes = plan.to_json().unwrap();
    bytes.push(b'\n');
    assert!(Plan::from_json(&bytes).is_err());
}
#[test]
fn resumed_selection_uses_persisted_receipts_after_git_objects_are_unavailable() {
    let mut f = Fixture::new();
    let plan = f.plan();
    f.execution().apply(&plan).unwrap();
    let saved = f.namespace.checkpoint();
    fs::rename(
        f.repo().join(".git/objects"),
        f.repo().join(".git/offline-objects"),
    )
    .unwrap();
    let replay = f.execution().apply(&plan).unwrap();
    assert!(replay.historical.unwrap().replayed);
    assert_eq!(f.namespace.checkpoint(), saved);
    f.metadata.put(b"another caller's staged node").unwrap();
    assert_eq!(
        f.metadata.finished_reader().err().unwrap(),
        git_asset_store::metadata::Error::NeedsRecovery
    );
    f.execution().apply(&plan).unwrap();
    assert_eq!(f.metadata.pending_nodes(), 1);
    f.metadata.finish().unwrap();
    let mut reader = f.metadata.finished_reader().unwrap();
    assert!(
        Tree::from_root(plan.selection.root)
            .entries(&mut reader, TreeLimits::default())
            .is_ok()
    );
}

fn profile(f: &Fixture) -> serde_json::Value {
    serde_json::json!({"schema":"tig.git-lifecycle-profile.design-1","enrollment":f.enrollment,"git_executable":git_executable(),"repository_root":f.repo(),"source_limits":Limits::default(),"namespace_root":f.temp.path().join("namespace"),"namespace_minimum":f.namespace.checkpoint(),"metadata_root":f.temp.path().join("metadata"),"metadata_minimum":f.metadata.checkpoint(),"allow_selection":true})
}
fn save(root: &Path, name: &str, v: &serde_json::Value) {
    fs::write(root.join(name), identity::canonical_json(v).unwrap()).unwrap();
}
fn cli(root: &Path, action: &str, input: &str) -> std::process::Output {
    Process::new(env!("CARGO_BIN_EXE_git-asset-lifecycle"))
        .arg(action)
        .arg(root.join("profile.json"))
        .arg(root.join(input))
        .output()
        .unwrap()
}
fn success(output: std::process::Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}
fn failure(output: std::process::Output, code: &str) {
    assert!(!output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["code"],
        code
    );
}

fn storage_profile(f: &Fixture) -> serde_json::Value {
    use git_asset_store::{
        ledger::CapacityLedger,
        local::LocalStore,
        local_capacity::LocalCapacityProbe,
        orchestration::Place,
        placement::{Authority, Budget, Config, Mapping, Provider, Role},
        store_protection::{Binding, Member},
    };
    let root = f.temp.path();
    let mut providers = Vec::new();
    let mut roots = Vec::new();
    for (name, role) in [
        ("working", Role::Cache),
        ("history", Role::Cache),
        ("backup", Role::Retained),
    ] {
        let path = root.join(name);
        let store = if role == Role::Cache {
            LocalStore::create_managed(
                &path,
                key(name),
                Binding::new(
                    1,
                    hash(&"trusted clients"),
                    vec![Member {
                        namespace: f.namespace.identity().clone(),
                        places: vec![key(name)],
                    }],
                )
                .unwrap(),
            )
            .unwrap()
        } else {
            LocalStore::create(&path, key(name)).unwrap()
        };
        let ledger_root = root.join(format!("ledger-{name}"));
        let mut ledger = CapacityLedger::create(&ledger_root, key(name), key("epoch"), 0).unwrap();
        LocalCapacityProbe::bind(&store, &mut ledger)
            .unwrap()
            .observe(&store, &mut ledger, key("probe"), key("sample"))
            .unwrap();
        let job = ledger
            .reserve(key("controller"), key("owner"), 256 * 1024 * 1024)
            .unwrap()
            .reference()
            .clone();
        providers.push(Provider {
            place: Place::local(&store, 1).unwrap(),
            role,
            failure_domain: key(name),
            capacity: ledger.identity().clone(),
            job,
            assignments: vec![Mapping {
                namespace: f.namespace.identity().clone(),
                place: key(name),
            }],
        });
        roots.push(serde_json::json!({"root":path,"ledger_root":ledger_root,"minimum":ledger.checkpoint()}));
    }
    let seed = root.join("seed");
    fs::create_dir_all(seed.join("objects")).unwrap();
    for bytes in [b"old".as_slice(), b"new".as_slice(), b"shared".as_slice()] {
        fs::write(
            seed.join("objects")
                .join(Object::from_bytes(bytes).sha256.as_str()),
            bytes,
        )
        .unwrap();
    }
    let byte_source = serde_json::json!({"places":[{"root":seed,"layout":"flat"}],"required":"cooperative-immutable"});
    let config = Config {
        schema: "tig.placement-enrollment.design-1".into(),
        owner: key("owner"),
        authority: f.enrollment.authority.clone(),
        seed: hash(&byte_source),
        providers,
        namespaces: vec![Authority {
            namespace: f.namespace.identity().clone(),
            policy: hash(f.namespace.policy()),
        }],
        minimum_domains: 1,
        budget: Budget {
            objects: 32,
            max_object_bytes: 8 * 1024 * 1024,
            max_stream_bytes: 32 * 1024 * 1024,
            max_peak_bytes: 256 * 1024 * 1024,
            limits: admission_limits(),
        },
    };
    serde_json::json!({"schema":"tig.placement-profile.design-1","config":config,"namespaces":[{"root":root.join("namespace"),"minimum":f.namespace.checkpoint()}],"providers":roots,"byte_source":byte_source,"allow_copy":true,"allow_eviction":true,"allow_reacquisition":true,"allow_cancellation":true})
}
fn reconcile(root: &Path) -> std::process::Output {
    Process::new(env!("CARGO_BIN_EXE_git-asset-lifecycle"))
        .arg("reconcile")
        .arg(root.join("profile.json"))
        .arg(root.join("request.json"))
        .arg(root.join("storage.json"))
        .output()
        .unwrap()
}
#[test]
fn actual_cli_selects_copies_demotes_and_reacquires_without_source_on_noop() {
    use git_asset_store::local::LocalStore;
    let f = Fixture::new();
    let root = f.temp.path();
    save(root, "profile.json", &profile(&f));
    save(
        root,
        "request.json",
        &serde_json::json!({"schema":"tig.git-lifecycle-request.design-1","repository":f.enrollment.repository,"ref_name":f.enrollment.ref_name,"commit":null}),
    );
    save(root, "storage.json", &storage_profile(&f));
    let first = success(reconcile(root));
    assert_eq!(first["storage_converged"], true, "{first}");
    let revision = first["selection"]["source_revision"].clone();
    let old = Object::from_bytes(b"old");
    let new = Object::from_bytes(b"new");
    let shared = Object::from_bytes(b"shared");
    assert!(
        LocalStore::open(&root.join("working"))
            .unwrap()
            .read_object(&old)
            .is_ok()
    );
    git(&f.repo(), &["checkout", "-b", "asset-fix"]);
    f.update(b"new");
    git(&f.repo(), &["checkout", "main"]);
    git(
        &f.repo(),
        &[
            "-c",
            "core.hooksPath=NUL",
            "merge",
            "--no-ff",
            "asset-fix",
            "-m",
            "release synthetic asset",
        ],
    );
    let advanced = success(reconcile(root));
    assert_eq!(advanced["storage_converged"], true, "{advanced}");
    assert_ne!(advanced["selection"]["source_revision"], revision);
    let working = LocalStore::open(&root.join("working")).unwrap();
    let history = LocalStore::open(&root.join("history")).unwrap();
    assert!(working.read_object(&old).is_err());
    assert!(history.read_object(&old).is_ok());
    assert!(working.read_object(&new).is_ok());
    assert!(history.read_object(&shared).is_err());
    fs::rename(root.join("seed"), root.join("offline-seed")).unwrap();
    let no_op = success(reconcile(root));
    assert_eq!(no_op["storage_converged"], true, "{no_op}");
    assert_eq!(no_op["placement"]["stream_bytes"], 0);
    assert_eq!(no_op["placement"]["effects"], serde_json::json!([]));
    assert_eq!(
        no_op["selection"]["source_revision"],
        advanced["selection"]["source_revision"]
    );
    f.update(b"old");
    let restored = success(reconcile(root));
    assert_eq!(restored["storage_converged"], true, "{restored}");
    assert!(working.read_object(&old).is_ok());
    assert!(history.read_object(&old).is_err());
}

#[test]
fn actual_cli_private_permissions_default_disabled_and_profile_drift_refuses() {
    let f = Fixture::new();
    let root = f.temp.path();
    save(root, "profile.json", &profile(&f));
    save(
        root,
        "request.json",
        &serde_json::json!({"schema":"tig.git-lifecycle-request.design-1","repository":f.enrollment.repository,"ref_name":f.enrollment.ref_name,"commit":null}),
    );
    let mut storage = storage_profile(&f);
    for flag in [
        "allow_copy",
        "allow_eviction",
        "allow_reacquisition",
        "allow_cancellation",
    ] {
        storage.as_object_mut().unwrap().remove(flag);
    }
    save(root, "storage.json", &storage);
    let held = success(reconcile(root));
    assert_eq!(held["storage_converged"], false);
    assert_eq!(held["placement"]["stream_bytes"], 0);
    assert!(
        held["placement"]["holds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "disabled")
    );
    storage["byte_source"]["places"][0]["root"] = serde_json::json!(root.join("different-seed"));
    save(root, "storage.json", &storage);
    failure(reconcile(root), "authority");
}

#[test]
fn actual_cli_pages_one_basis_and_refuses_cursor_after_new_selection() {
    let f = Fixture::new();
    let root = f.temp.path();
    save(root, "profile.json", &profile(&f));
    save(
        root,
        "request.json",
        &serde_json::json!({"schema":"tig.git-lifecycle-request.design-1","repository":f.enrollment.repository,"ref_name":f.enrollment.ref_name,"commit":null}),
    );
    let mut storage = storage_profile(&f);
    storage["config"]["budget"]["objects"] = serde_json::json!(1);
    save(root, "storage.json", &storage);
    let first = success(reconcile(root));
    assert_eq!(first["placement"]["page_converged"], true);
    assert!(first["storage_converged"].is_null());
    save(root, "cursor.json", &first["placement"]["next"]);
    let page = || {
        Process::new(env!("CARGO_BIN_EXE_git-asset-lifecycle"))
            .arg("reconcile")
            .arg(root.join("profile.json"))
            .arg(root.join("request.json"))
            .arg(root.join("storage.json"))
            .arg(root.join("cursor.json"))
            .output()
            .unwrap()
    };
    let second = success(page());
    assert_eq!(second["placement"]["page_converged"], true);
    assert!(second["storage_converged"].is_null());
    assert!(second["placement"]["next"].is_null());
    f.update(b"new");
    failure(page(), "stale");
}
#[test]
fn actual_cli_binds_operator_request_and_offline_status_adds_no_commands() {
    let described = success(
        Process::new(env!("CARGO_BIN_EXE_git-asset-lifecycle"))
            .arg("describe")
            .output()
            .unwrap(),
    );
    assert_eq!(described["source_format"], "git_lfs_pointers");
    assert_eq!(described["selection_only"], false);
    assert_eq!(described["physical_reconciliation"], true);
    assert_eq!(described["storage_converged"], serde_json::Value::Null);
    let f = Fixture::new();
    let root = f.temp.path().to_path_buf();
    save(&root, "profile.json", &profile(&f));
    let request = serde_json::json!({"schema":"tig.git-lifecycle-request.design-1","repository":f.enrollment.repository,"ref_name":f.enrollment.ref_name,"commit":null});
    save(&root, "request.json", &request);
    let before = f.namespace.checkpoint();
    let planned = success(cli(&root, "plan", "request.json"));
    save(&root, "plan.json", &planned);
    assert_eq!(f.namespace.checkpoint(), before);
    assert_eq!(
        success(cli(&root, "status", "plan.json"))["historical"],
        serde_json::Value::Null
    );
    let applied = success(cli(&root, "apply", "plan.json"));
    assert_eq!(applied["desired_current"], true);
    assert_eq!(applied["storage_converged"], serde_json::Value::Null);
    let mut ns = Namespace::open(&root.join("namespace"), &before).unwrap();
    let committed = ns.checkpoint();
    assert_eq!(committed.sequence(), before.sequence() + 2);
    success(cli(&root, "resume", "plan.json"));
    assert_eq!(ns.source(&key("main")).unwrap().unwrap().revision, 3);
    assert_eq!(ns.checkpoint(), committed);
    fs::rename(
        f.repo().join(".git/objects"),
        f.repo().join(".git/offline-objects"),
    )
    .unwrap();
    drop(f.metadata);
    fs::rename(root.join("metadata"), root.join("offline-metadata")).unwrap();
    let status = success(cli(&root, "status", "plan.json"));
    assert_eq!(status["historical"]["replayed"], true);
    assert_eq!(ns.source(&key("main")).unwrap().unwrap().revision, 3);
    assert_eq!(ns.checkpoint(), committed);
}
#[test]
fn ref_drift_before_admission_holds_and_drift_after_admission_is_reported_separately() {
    for stage in [
        git_asset::lifecycle::Stage::MetadataVerified,
        git_asset::lifecycle::Stage::Selected,
    ] {
        let mut f = Fixture::new();
        let plan = f.plan();
        let before = f.namespace.checkpoint();
        let repo = f.repo();
        let result = f.execution().apply_with_progress(&plan, |point| {
            if point == stage {
                fs::write(
                    repo.join("assets/changed.bin"),
                    Pointer::new(Object::from_bytes(b"advanced during reconciliation")).to_bytes(),
                )
                .unwrap();
                commit(&repo);
            }
        });
        if stage == git_asset::lifecycle::Stage::MetadataVerified {
            assert_eq!(result.unwrap_err(), Error::Stale);
            assert_eq!(f.namespace.checkpoint(), before);
        } else {
            let status = result.unwrap();
            assert!(status.desired_current);
            assert_eq!(status.observed_ref_current, Some(false));
            assert_eq!(status.storage_converged, None);
        }
        let latest = f.plan();
        let status = f.execution().apply(&latest).unwrap();
        assert_eq!(status.observed_ref_current, Some(true));
        assert!(
            f.namespace
                .source(&key("main"))
                .unwrap()
                .unwrap()
                .pending
                .is_none()
        );
    }
}
#[test]
fn cli_wrong_repository_ref_disabled_and_absent_metadata_refuse_without_initializing() {
    let f = Fixture::new();
    let root = f.temp.path();
    let mut p = profile(&f);
    save(root, "profile.json", &p);
    let mut request = serde_json::json!({"schema":"tig.git-lifecycle-request.design-1","repository":f.enrollment.repository,"ref_name":"refs/heads/main","commit":null});
    for field in ["repository", "ref_name"] {
        let mut bad = request.clone();
        bad[field] = serde_json::json!(if field == "repository" {
            Digest::blob(b"another repository").as_str().to_owned()
        } else {
            "refs/heads/other".into()
        });
        save(root, "request.json", &bad);
        failure(cli(root, "plan", "request.json"), "authority");
    }
    save(root, "request.json", &request);
    let plan = success(cli(root, "plan", "request.json"));
    save(root, "plan.json", &plan);
    p.as_object_mut().unwrap().remove("allow_selection");
    save(root, "profile.json", &p);
    failure(cli(root, "apply", "plan.json"), "selection_disabled");
    p["allow_selection"] = true.into();
    p["metadata_root"] = serde_json::json!(root.join("absent-metadata"));
    save(root, "profile.json", &p);
    failure(cli(root, "apply", "plan.json"), "metadata_unavailable");
    assert!(!root.join("absent-metadata").exists());
    request["unexpected"] = true.into();
    save(root, "request.json", &request);
    save(root, "profile.json", &profile(&f));
    failure(cli(root, "plan", "request.json"), "invalid_document");
    assert_eq!(f.namespace.checkpoint().sequence(), 1);
}
#[test]
fn exact_full_refs_support_unicode_and_never_select_a_prefix_neighbor_or_revision_expression() {
    let mut f = Fixture::new();
    let main = f.repository.named_commit("refs/heads/main").unwrap();
    git(&f.repo(), &["checkout", "-b", "main-neighbor"]);
    let other = f.update(b"neighbor");
    assert_ne!(main, other);
    assert_eq!(f.repository.named_commit("refs/heads/main").unwrap(), main);
    assert!(f.repository.named_commit("refs/heads/mai").is_err());
    assert!(f.repository.named_commit("refs/heads/main~1").is_err());
    assert!(
        f.repository
            .named_commit("refs/heads/main\nforged")
            .is_err()
    );
    git(&f.repo(), &["checkout", "-b", "révision"]);
    f.enrollment.ref_name = "refs/heads/révision".into();
    let plan = f.plan();
    assert_eq!(plan.commit, other.as_str());
    assert_eq!(
        f.execution().apply(&plan).unwrap().observed_ref_current,
        Some(true)
    );
}
