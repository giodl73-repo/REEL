mod common;
#[path = "index_lifecycle/settlement_cases.rs"]
mod settlement_cases;
#[path = "index_lifecycle/store_cases.rs"]
mod store_cases;
use common::{command, git, git_executable, key};
use git_asset::{
    lifecycle::{
        Budget, Error, Stage,
        index::{self, Enrollment, Execution, Plan},
    },
    pointer::Pointer,
    source::{Limits, Repository},
};
use git_asset_core::{
    Digest, LogicalPath, Object, identity,
    tree::{MemoryNodes, Tree, TreeLimits},
};
use git_asset_store::{
    metadata::{Limits as MetadataLimits, MetadataStore},
    protection,
    source_claims::{
        Command, Kind, Places,
        admission::{Namespace, Policy},
    },
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command as Process,
};
fn hash(value: &impl serde::Serialize) -> Digest {
    Digest::blob(&identity::canonical_json(&serde_json::to_value(value).unwrap()).unwrap())
}
fn limits() -> protection::Limits {
    protection::Limits {
        millis: 30000,
        roots: 130,
        bindings: 10000,
        associations: 20000,
        reads: 100000,
        read_bytes: 16 * 1024 * 1024,
    }
}
struct Fixture {
    temp: tempfile::TempDir,
    repository: Repository,
    namespace: Namespace,
    metadata: MetadataStore,
    enrollment: Enrollment,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::Builder::new()
            .prefix("git-asset index lifecycle ")
            .tempdir()
            .unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]);
        git(&repo, &["config", "user.name", "Fixture"]);
        git(&repo, &["config", "user.email", "fixture@example.invalid"]);
        fs::create_dir(repo.join("assets")).unwrap();
        stage(&repo, b"base");
        commit(&repo);
        let places = Places {
            current: vec![key("working"), key("backup")],
            pending: vec![key("working"), key("backup")],
            predecessor: vec![key("history"), key("backup")],
        };
        let mut namespace = Namespace::create(
            &temp.path().join("namespace"),
            key("project"),
            key("epoch"),
            Policy {
                prefer_active: true,
                classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
                    .into_iter()
                    .map(|k| (k, places.clone()))
                    .collect(),
            },
        )
        .unwrap();
        namespace
            .apply(
                key("enroll-index"),
                Command::Enroll {
                    source: key("index"),
                    owner: key("owner"),
                    class: Kind::Draft,
                },
                &MemoryNodes::default(),
                &limits(),
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
            repository: Digest::blob(b"repo"),
            worktree: Digest::blob(b"worktree"),
            authority: Digest::blob(b"owner authority"),
            namespace: namespace.identity().clone(),
            policy: hash(namespace.policy()),
            source: key("index"),
            owner: key("owner"),
            scope: vec![LogicalPath::parse("assets").unwrap()],
            budget: Budget {
                entries: 1000,
                nodes: 100000,
                node_bytes: 16 * 1024 * 1024,
                millis: 30000,
            },
        };
        let repository = Repository::new(git_executable(), repo, Limits::default()).unwrap();
        Self {
            temp,
            repository,
            namespace,
            metadata,
            enrollment,
        }
    }
    fn repo(&self) -> PathBuf {
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
        self.execution().plan().unwrap()
    }
    fn protected(&mut self, bytes: &[u8], place: &str) -> bool {
        let object = Object::from_bytes(bytes);
        let fence = self.namespace.fence(&object).unwrap();
        self.namespace
            .absence(
                &fence,
                &key(place),
                &self.metadata.finished_reader().unwrap(),
                &limits(),
            )
            .unwrap()
            .is_none()
    }
    fn profile(&self) -> PathBuf {
        let path = self.temp.path().join("profile.json");
        fs::write(&path,identity::canonical_json(&serde_json::json!({"schema":"tig.git-index-profile.design-1","enrollment":self.enrollment,"git_executable":git_executable(),"repository_root":self.repo(),"source_limits":Limits::default(),"namespace_root":self.temp.path().join("namespace"),"namespace_minimum":self.namespace.checkpoint(),"metadata_root":self.temp.path().join("metadata"),"metadata_minimum":self.metadata.checkpoint(),"allow_selection":true})).unwrap()).unwrap();
        path
    }
}
fn stage(repo: &Path, bytes: &[u8]) {
    fs::write(
        repo.join("assets/one.bin"),
        Pointer::new(Object::from_bytes(bytes)).to_bytes(),
    )
    .unwrap();
    git(repo, &["add", "assets"]);
}
fn commit(repo: &Path) {
    git(
        repo,
        &[
            "-c",
            "core.hooksPath=NUL",
            "commit",
            "-m",
            "synthetic index fixture",
        ],
    );
}
#[test]
fn staged_adoption_ignores_working_bytes_and_noop_preserves_original_commands() {
    let mut f = Fixture::new();
    let first = f.plan();
    f.execution().apply(&first).unwrap();
    stage(&f.repo(), b"staged");
    fs::write(f.repo().join("assets/one.bin"), b"unstaged private payload").unwrap();
    let plan = f.plan();
    assert_eq!(Plan::from_json(&plan.to_json().unwrap()).unwrap(), plan);
    let status = f.execution().apply(&plan).unwrap();
    let entries = Tree::from_root(plan.selection.root.clone())
        .entries(
            &mut f.metadata.finished_reader().unwrap(),
            TreeLimits::default(),
        )
        .unwrap()
        .0;
    assert_eq!(
        entries.values().cloned().collect::<Vec<_>>(),
        vec![Object::from_bytes(b"staged")]
    );
    assert!(status.desired_current);
    assert_eq!(status.observed_index_current, Some(true));
    assert!(!status.merged_selection);
    assert!(f.protected(b"staged", "working"));
    assert!(f.protected(b"base", "history"));
    assert!(!f.protected(b"unstaged private payload", "working"));
    let before = f.namespace.checkpoint();
    let noop = f.plan();
    assert!(noop.no_op);
    f.execution().apply(&noop).unwrap();
    assert_eq!(f.namespace.checkpoint(), before);
    let historical = f.execution().apply(&plan).unwrap();
    assert!(historical.historical.unwrap().replayed);
    assert_eq!(historical.observed_index_current, None);
}
#[test]
fn conflicted_index_protects_all_alternatives_without_publishing_a_merged_selection() {
    let mut f = Fixture::new();
    git(&f.repo(), &["checkout", "-b", "side"]);
    stage(&f.repo(), b"side");
    commit(&f.repo());
    git(&f.repo(), &["checkout", "main"]);
    stage(&f.repo(), b"main");
    commit(&f.repo());
    assert!(
        !command(&f.repo(), &["-c", "core.hooksPath=NUL", "merge", "side"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let working = fs::read(f.repo().join("assets/one.bin")).unwrap();
    let plan = f.plan();
    assert_eq!(plan.unresolved_stages, 3);
    assert_eq!(plan.associations, 3);
    let status = f.execution().apply(&plan).unwrap();
    assert_eq!(status.unresolved_stages, 3);
    assert!(!status.merged_selection);
    for bytes in [b"base".as_slice(), b"main", b"side"] {
        assert!(f.protected(bytes, "working"));
    }
    assert_eq!(fs::read(f.repo().join("assets/one.bin")).unwrap(), working);
    stage(&f.repo(), b"resolved");
    let resolved = f.plan();
    assert_eq!(resolved.unresolved_stages, 0);
    f.execution().apply(&resolved).unwrap();
    assert!(f.protected(b"resolved", "working"));
    for bytes in [b"base".as_slice(), b"main", b"side"] {
        assert!(f.protected(bytes, "history"));
    }
}
#[test]
fn unsupported_capture_and_stale_plan_cannot_shrink_existing_draft() {
    let mut f = Fixture::new();
    let old = f.plan();
    f.execution().apply(&old).unwrap();
    stage(&f.repo(), b"candidate");
    let candidate = f.plan();
    stage(&f.repo(), b"newer");
    let before = f.namespace.checkpoint();
    assert!(matches!(f.execution().apply(&candidate), Err(Error::Stale)));
    assert_eq!(f.namespace.checkpoint(), before);
    fs::write(f.repo().join("assets/one.bin"), b"not a pointer").unwrap();
    git(&f.repo(), &["add", "assets"]);
    assert!(matches!(f.execution().plan(), Err(Error::Pending)));
    assert_eq!(f.namespace.checkpoint(), before);
    assert!(f.protected(b"base", "working"));
}
#[test]
fn index_drift_before_and_after_admission_have_separate_truthful_outcomes() {
    for boundary in [Stage::MetadataVerified, Stage::Selected] {
        let mut f = Fixture::new();
        let plan = f.plan();
        let repo = f.repo();
        let before = f.namespace.checkpoint();
        let result = f.execution().apply_with_progress(&plan, |stage_now| {
            if stage_now == boundary {
                stage(&repo, b"late index");
            }
        });
        if boundary == Stage::MetadataVerified {
            assert!(matches!(result, Err(Error::Stale)));
            assert_eq!(f.namespace.checkpoint(), before);
        } else {
            let status = result.unwrap();
            assert!(status.desired_current);
            assert_eq!(status.observed_index_current, Some(false));
            assert!(f.protected(b"base", "working"));
        }
    }
}
#[test]
fn metadata_interruption_and_offline_historical_status_recover_without_index_writes() {
    let mut f = Fixture::new();
    let plan = f.plan();
    let before = f.namespace.checkpoint();
    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        f.execution().apply_with_progress(&plan, |s| {
            if s == Stage::MetadataVerified {
                panic!("synthetic interruption")
            }
        })
    }));
    assert!(stopped.is_err());
    assert_eq!(f.namespace.checkpoint(), before);
    let metadata_path = f.temp.path().join("metadata");
    let minimum = f.metadata.checkpoint();
    drop(f.metadata);
    f.metadata = MetadataStore::open(&metadata_path, &minimum).unwrap();
    f.execution().apply(&plan).unwrap();
    fs::rename(f.repo(), f.temp.path().join("offline-repo")).unwrap();
    let status = index::inspect(&f.enrollment, &mut f.namespace, &plan).unwrap();
    assert!(status.historical.is_some());
    assert!(status.desired_current);
    assert_eq!(status.observed_index_current, None);
}
#[test]
fn foreign_worktree_owner_policy_and_non_draft_sources_refuse() {
    let mut f = Fixture::new();
    let plan = f.plan();
    let before = f.namespace.checkpoint();
    for field in 0..4 {
        let original = f.enrollment.clone();
        match field {
            0 => f.enrollment.worktree = Digest::blob(b"other"),
            1 => f.enrollment.owner = key("foreign"),
            2 => f.enrollment.policy = Digest::blob(b"other"),
            _ => f.enrollment.namespace = Digest::blob(b"other"),
        };
        assert!(matches!(f.execution().apply(&plan), Err(Error::Authority)));
        f.enrollment = original;
    }
    f.namespace
        .apply(
            key("branch-enrollment"),
            Command::Enroll {
                source: key("branch"),
                owner: key("owner"),
                class: Kind::Branch,
            },
            &MemoryNodes::default(),
            &limits(),
        )
        .unwrap();
    f.enrollment.source = key("branch");
    assert!(matches!(f.execution().plan(), Err(Error::Authority)));
    assert_eq!(before.sequence() + 1, f.namespace.checkpoint().sequence());
}
#[test]
fn linked_worktrees_keep_independent_index_sources() {
    let mut f = Fixture::new();
    let main = f.plan();
    let linked = f.temp.path().join("linked");
    git(
        &f.repo(),
        &["worktree", "add", "-b", "draft", linked.to_str().unwrap()],
    );
    stage(&linked, b"linked staged");
    let linked_repository = Repository::new(git_executable(), linked, Limits::default()).unwrap();
    f.namespace
        .apply(
            key("linked-enrollment"),
            Command::Enroll {
                source: key("linked-index"),
                owner: key("owner"),
                class: Kind::Draft,
            },
            &MemoryNodes::default(),
            &limits(),
        )
        .unwrap();
    let mut enrollment = f.enrollment.clone();
    enrollment.worktree = Digest::blob(b"linked worktree");
    enrollment.source = key("linked-index");
    let mut execution = Execution {
        enrollment: &enrollment,
        repository: &linked_repository,
        namespace: &mut f.namespace,
        metadata: &mut f.metadata,
        allow_selection: true,
    };
    let linked = execution.plan().unwrap();
    execution.apply(&linked).unwrap();
    f.execution().apply(&main).unwrap();
    assert!(f.protected(b"linked staged", "working"));
    assert!(f.protected(b"base", "working"));
}
#[test]
fn canonical_plan_rejects_unknown_fields_bad_counts_and_whitespace() {
    let mut f = Fixture::new();
    let plan = f.plan();
    let mut bytes = plan.to_json().unwrap();
    bytes.push(b'\n');
    assert!(Plan::from_json(&bytes).is_err());
    let mut json = serde_json::to_value(&plan).unwrap();
    json["unresolved_stages"] = serde_json::json!(plan.associations + 1);
    assert!(Plan::from_json(&identity::canonical_json(&json).unwrap()).is_err());
    json = serde_json::to_value(&plan).unwrap();
    json["allow_selection"] = serde_json::json!(true);
    assert!(Plan::from_json(&identity::canonical_json(&json).unwrap()).is_err());
    f.enrollment.budget.entries = 1;
    stage(&f.repo(), b"one");
    fs::write(
        f.repo().join("assets/two.bin"),
        Pointer::new(Object::from_bytes(b"two")).to_bytes(),
    )
    .unwrap();
    git(&f.repo(), &["add", "assets"]);
    assert!(matches!(f.execution().plan(), Err(Error::Budget)));
}
fn cli(args: &[&Path], action: &str) -> std::process::Output {
    Process::new(env!("CARGO_BIN_EXE_git-asset-lifecycle"))
        .args(["index", action])
        .args(args)
        .output()
        .unwrap()
}
fn success(output: std::process::Output) -> serde_json::Value {
    assert!(output.stderr.is_empty());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn actual_index_cli_plan_apply_resume_and_offline_status_share_exact_receipts() {
    let f = Fixture::new();
    let profile = f.profile();
    let before = f.namespace.checkpoint();
    let described = success(
        Process::new(env!("CARGO_BIN_EXE_git-asset-lifecycle"))
            .arg("describe")
            .output()
            .unwrap(),
    );
    assert_eq!(
        described["index_profile_schema"],
        "tig.git-index-profile.design-1"
    );
    let plan = success(cli(&[&profile], "plan"));
    let path = f.temp.path().join("plan.json");
    fs::write(&path, identity::canonical_json(&plan).unwrap()).unwrap();
    let selected = success(cli(&[&profile, &path], "apply"));
    assert_eq!(selected["observed_index_current"], true);
    assert_eq!(selected["merged_selection"], false);
    assert!(selected["source_revision"].as_u64().unwrap() > before.sequence());
    let resumed = success(cli(&[&profile, &path], "resume"));
    assert_eq!(resumed["historical"]["replayed"], true);
    assert_eq!(resumed["source_revision"], selected["source_revision"]);
    let mut unavailable: serde_json::Value =
        serde_json::from_slice(&fs::read(&profile).unwrap()).unwrap();
    let absent = f.temp.path().join("must-not-create");
    unavailable["repository_root"] = serde_json::json!(absent.join("repo"));
    unavailable["git_executable"] = serde_json::json!(absent.join("git"));
    unavailable["metadata_root"] = serde_json::json!(absent.join("metadata"));
    fs::write(&profile, identity::canonical_json(&unavailable).unwrap()).unwrap();
    let historical = success(cli(&[&profile, &path], "status"));
    assert_eq!(historical["desired_current"], true);
    assert_eq!(
        historical["observed_index_current"],
        serde_json::Value::Null
    );
    assert!(!absent.exists());
}
#[test]
fn actual_index_cli_permissions_default_disabled_and_changed_enrollment_refuses() {
    let f = Fixture::new();
    let profile = f.profile();
    let plan = success(cli(&[&profile], "plan"));
    let path = f.temp.path().join("plan.json");
    fs::write(&path, identity::canonical_json(&plan).unwrap()).unwrap();
    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&profile).unwrap()).unwrap();
    document.as_object_mut().unwrap().remove("allow_selection");
    fs::write(&profile, identity::canonical_json(&document).unwrap()).unwrap();
    let result = cli(&[&profile, &path], "apply");
    assert!(!result.status.success());
    assert!(result.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap()["code"],
        "selection_disabled"
    );
    document["allow_selection"] = serde_json::json!(true);
    document["enrollment"]["worktree"] = serde_json::json!(Digest::blob(b"other worktree"));
    fs::write(&profile, identity::canonical_json(&document).unwrap()).unwrap();
    let result = cli(&[&profile, &path], "apply");
    assert!(!result.status.success());
    assert!(result.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap()["code"],
        "authority"
    );
}
#[test]
fn index_interruption_child() {
    let Ok(profile) = std::env::var("GIT_ASSET_INDEX_CRASH_PROFILE") else {
        return;
    };
    let value: serde_json::Value = serde_json::from_slice(&fs::read(profile).unwrap()).unwrap();
    let enrollment: Enrollment = serde_json::from_value(value["enrollment"].clone()).unwrap();
    let repository = Repository::new(
        serde_json::from_value(value["git_executable"].clone()).unwrap(),
        serde_json::from_value(value["repository_root"].clone()).unwrap(),
        Limits::default(),
    )
    .unwrap();
    let namespace_root: PathBuf = serde_json::from_value(value["namespace_root"].clone()).unwrap();
    let mut namespace = Namespace::open(
        &namespace_root,
        &serde_json::from_value(value["namespace_minimum"].clone()).unwrap(),
    )
    .unwrap();
    let metadata_root: PathBuf = serde_json::from_value(value["metadata_root"].clone()).unwrap();
    let mut metadata = MetadataStore::open(
        &metadata_root,
        &serde_json::from_value(value["metadata_minimum"].clone()).unwrap(),
    )
    .unwrap();
    let plan =
        Plan::from_json(&fs::read(std::env::var("GIT_ASSET_INDEX_CRASH_PLAN").unwrap()).unwrap())
            .unwrap();
    let boundary = if std::env::var("GIT_ASSET_INDEX_CRASH_BOUNDARY").unwrap() == "metadata" {
        Stage::MetadataVerified
    } else {
        Stage::Selected
    };
    Execution {
        enrollment: &enrollment,
        repository: &repository,
        namespace: &mut namespace,
        metadata: &mut metadata,
        allow_selection: true,
    }
    .apply_with_progress(&plan, |s| {
        if s == boundary {
            std::process::exit(73)
        }
    })
    .unwrap();
    panic!("requested interruption was not reached");
}
#[test]
fn abrupt_process_exits_recover_verified_metadata_and_original_adoption() {
    for boundary in ["metadata", "selected"] {
        let mut f = Fixture::new();
        let plan = f.plan();
        let profile = f.profile();
        let plan_path = f.temp.path().join("exact-plan.json");
        fs::write(&plan_path, plan.to_json().unwrap()).unwrap();
        let listing = f.repository.index().unwrap().checkpoint().listing.clone();
        let status = Process::new(std::env::current_exe().unwrap())
            .args(["--exact", "index_interruption_child", "--nocapture"])
            .env("GIT_ASSET_INDEX_CRASH_PROFILE", profile)
            .env("GIT_ASSET_INDEX_CRASH_PLAN", plan_path)
            .env("GIT_ASSET_INDEX_CRASH_BOUNDARY", boundary)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73));
        f.metadata =
            MetadataStore::open(&f.temp.path().join("metadata"), &f.metadata.checkpoint()).unwrap();
        let status = f.execution().apply(&plan).unwrap();
        assert!(status.desired_current);
        assert!(status.historical.is_some());
        assert!(f.protected(b"base", "working"));
        assert_eq!(f.repository.index().unwrap().checkpoint().listing, listing);
    }
}
