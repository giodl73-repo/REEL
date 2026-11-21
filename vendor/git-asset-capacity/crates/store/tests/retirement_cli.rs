use git_asset_core::{
    Digest, LogicalPath, Object, identity,
    tree::{MemoryNodes, Tree, TreeLimits},
};
use git_asset_store::{
    AckLevel, Key, Request, accounted,
    ledger::{CapacityLedger, Checkpoint},
    local::LocalStore,
    local_capacity::LocalCapacityProbe,
    metadata::{Limits as MetadataLimits, MetadataStore},
    protection::Limits,
    replica::ReplicaEnrollment,
    retention, retirement,
    source_claims::{
        Command, Kind, Places, Selection,
        admission::{Namespace, Policy},
    },
    store_protection::{Binding, Enrollment, Member},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Cursor, Read},
    path::Path,
    process::{Command as Process, Output},
};

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn digest(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn limits() -> Limits {
    Limits {
        millis: 60000,
        roots: 64,
        bindings: 10000,
        associations: 100000,
        reads: 100000,
        read_bytes: 32 * 1024 * 1024,
    }
}
fn write(root: &Path, name: &str, value: &Value) {
    fs::write(root.join(name), serde_json::to_vec(value).unwrap()).unwrap();
}
fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}
struct Fixture {
    root: tempfile::TempDir,
    profile: Value,
    publication: Request,
    minimum: Checkpoint,
}
impl Fixture {
    fn new() -> Self {
        Self::with_final_provider(false)
    }
    fn with_final_provider(final_provider: bool) -> Self {
        Self::with_provider(final_provider, false)
    }
    fn with_provider(final_provider: bool, roster: bool) -> Self {
        Self::with_clock_ledger(final_provider, roster, false)
    }
    fn with_clock_ledger(final_provider: bool, roster: bool, clock: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let places = Places {
            current: vec![key("working"), key("backup")],
            pending: vec![key("working"), key("backup")],
            predecessor: vec![key("history"), key("backup")],
        };
        let policy = Policy {
            prefer_active: true,
            classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
                .into_iter()
                .map(|k| (k, places.clone()))
                .collect(),
        };
        let mut namespace = Namespace::create(
            &root.path().join("namespace"),
            key("project"),
            key("epoch"),
            policy,
        )
        .unwrap();
        namespace
            .apply(
                key("enroll"),
                Command::Enroll {
                    source: key("main"),
                    owner: key("owner"),
                    class: Kind::Branch,
                },
                &MemoryNodes::default(),
                &limits(),
            )
            .unwrap();
        let constructor = if roster {
            Binding::new_roster
        } else {
            Binding::new
        };
        let binding = constructor(
            1,
            digest("managed-boundary"),
            vec![Member {
                namespace: namespace.identity().clone(),
                places: vec![key("working")],
            }],
        )
        .unwrap();
        let store = if final_provider {
            use git_asset_store::final_retirement::{claims, provider};
            let roster = claims::Roster::flat(
                digest("authority"),
                digest("epoch"),
                Some(digest("complete-content-scope")),
                vec![claims::NamespacePin::current(&namespace).unwrap()],
                limits(),
            )
            .unwrap();
            let policy = provider::Policy::new(
                digest("authority"),
                digest("epoch"),
                roster.scope().unwrap(),
                digest("clock"),
                vec![key("owner")],
                1000,
            )
            .unwrap();
            LocalStore::create_final(
                &root.path().join("working"),
                key("working"),
                binding,
                policy,
            )
            .unwrap()
        } else {
            LocalStore::create_managed(&root.path().join("working"), key("working"), binding)
                .unwrap()
        };
        let mut ledger = if clock {
            CapacityLedger::create_clock(
                &root.path().join("ledger"),
                key("capacity"),
                key("epoch"),
                0,
                git_asset_store::clock::Policy {
                    authority: digest("clock-authority"),
                    epoch: digest("clock-epoch"),
                    boundary: digest("native-os"),
                    max_sample_millis: 5000,
                    max_drift_millis: 1000,
                    max_session_millis: 60000,
                },
            )
        } else {
            CapacityLedger::create(
                &root.path().join("ledger"),
                key("capacity"),
                key("epoch"),
                0,
            )
        }
        .unwrap();
        LocalCapacityProbe::bind(&store, &mut ledger)
            .unwrap()
            .observe(&store, &mut ledger, key("probe"), key("sample"))
            .unwrap();
        let job = ledger
            .reserve(key("job"), key("owner"), 512 * 1024 * 1024)
            .unwrap()
            .reference()
            .clone();
        let bytes = b"synthetic retirement and exact reacquisition";
        let publication = Request {
            operation: key("initial"),
            owner: key("owner"),
            object: Object::from_bytes(bytes),
            required: AckLevel::ProcessRestart,
        };
        let plan = accounted::plan(&store, &publication).unwrap();
        accounted::publish_accounted(&mut ledger, &store, &job, &plan, &mut Cursor::new(bytes))
            .unwrap();
        let minimum = ledger.checkpoint();
        let mut backup = LocalStore::create(&root.path().join("backup"), key("backup")).unwrap();
        git_asset_store::publish(&mut backup, &publication, &mut Cursor::new(bytes)).unwrap();
        let mut capacity = CapacityLedger::create(
            &root.path().join("backup-ledger"),
            key("backup-capacity"),
            key("epoch"),
            0,
        )
        .unwrap();
        let retained = retention::plan(
            &backup,
            &retention::RetentionRequest {
                operation: key("retain"),
                owner: key("owner"),
                publication: publication.clone(),
                minimum_ack: AckLevel::ProcessRestart,
            },
        )
        .unwrap();
        let probe = capacity.begin_probe(key("probe")).unwrap();
        capacity
            .observe(key("sample"), probe.reference(), 1024 * 1024 * 1024)
            .unwrap();
        let backup_job = capacity
            .reserve(key("job"), key("owner"), retained.required_peak())
            .unwrap()
            .reference()
            .clone();
        let grant = retention::retain(&mut capacity, &backup, &backup_job, &retained)
            .unwrap()
            .grant;
        let mut metadata = MetadataStore::create(
            &root.path().join("metadata"),
            MetadataLimits {
                nodes: 10000,
                bytes: 16 * 1024 * 1024,
                database_bytes: 32 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        // An actual empty immutable tree is used later to withdraw target demand.
        Tree::build(&Default::default(), &mut metadata, TreeLimits::default()).unwrap();
        let metadata_minimum = metadata.finish().unwrap();
        let profile = json!({"schema":"tig.storage-retirement-profile.design-1","source_root":root.path().join("working"),
            "enrollment":Enrollment::bind(&store).unwrap(),"ledger_root":root.path().join("ledger"),"minimum":minimum,"job":job,
            "source":ReplicaEnrollment {store:store.identity().clone(),failure_domain:key("working-domain")},
            "metadata":{"root":root.path().join("metadata"),"minimum":metadata_minimum},
            "namespaces":[{"root":root.path().join("namespace"),"minimum":namespace.checkpoint()}],
            "backups":[{"root":root.path().join("backup"),"ledger_root":root.path().join("backup-ledger"),"minimum":capacity.checkpoint(),
                "enrollment":ReplicaEnrollment {store:backup.identity().clone(),failure_domain:key("backup-domain")},"grant":grant,
                "places":[{"namespace":namespace.identity(),"place":"backup"},{"namespace":namespace.identity(),"place":"history"}]}],
            "limits":limits(),"policy":{"owner":"owner","context":digest("trusted-policy"),"minimum_domains":1,
                "max_object_bytes":1024,"max_peak_bytes":128*1024*1024,"allow_eviction":true,"allow_reacquisition":true,"allow_cancellation":true}});
        write(root.path(), "profile.json", &profile);
        write(
            root.path(),
            "request.json",
            &json!({"schema":"tig.retirement-request.design-1","operation":"evict","publication":publication}),
        );
        Self {
            root,
            profile,
            publication,
            minimum,
        }
    }
    fn run(&self, action: &str) -> Output {
        let mut p = Process::new(env!("CARGO_BIN_EXE_git-asset-storage"));
        p.args(["retire", action])
            .arg(self.root.path().join("profile.json"));
        p.arg(self.root.path().join(if action == "plan" {
            "request.json"
        } else if matches!(action, "reacquire" | "reacquire-status") {
            "restore.json"
        } else {
            "plan.json"
        }));
        if action == "reacquire-plan" {
            p.arg(self.root.path().join("restore-request.json"));
        }
        p.output().unwrap()
    }
    fn make_plan(&self) {
        let output = self.run("plan");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        retirement::Plan::from_json(&output.stdout).unwrap();
        fs::write(self.root.path().join("plan.json"), output.stdout).unwrap();
    }
    fn sequence(&self) -> u64 {
        CapacityLedger::open(&self.root.path().join("ledger"), &self.minimum)
            .unwrap()
            .checkpoint()
            .sequence()
    }
    fn set_profile(&self, p: &Value) {
        write(self.root.path(), "profile.json", p);
    }
    fn restore_plan(&self) {
        let mut r = self.publication.clone();
        r.operation = key("restore");
        write(
            self.root.path(),
            "restore-request.json",
            &json!({"schema":"tig.reacquisition-request.design-1","publication":r}),
        );
        let output = self.run("reacquire-plan");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        retirement::ReacquisitionPlan::from_json(&output.stdout).unwrap();
        fs::write(self.root.path().join("restore.json"), output.stdout).unwrap();
    }
    fn error(&self, action: &str, code: &str) {
        let out = self.run(action);
        assert!(!out.status.success());
        assert!(out.stderr.is_empty());
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(!text.contains(&self.root.path().to_string_lossy().to_string()));
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["code"], code);
    }
}
#[test]
fn processes_unlink_retry_restore_and_preserve_historical_outcome_without_status_commands() {
    let f = Fixture::new();
    let before = f.sequence();
    f.make_plan();
    let s = success(f.run("status"));
    assert_eq!(s["phase"], "not_started");
    assert_eq!(s["observed"], "verified_present");
    assert_eq!(s["removal_authority"], false);
    assert_eq!(f.sequence(), before);
    success(f.run("apply"));
    let path = f
        .root
        .path()
        .join("working/objects")
        .join(f.publication.object.sha256.as_str());
    assert!(!path.exists());
    let after = f.sequence();
    let s = success(f.run("status"));
    assert_eq!(s["phase"], "retired");
    assert_eq!(s["observed"], "authorized_absence");
    assert_eq!(s["historical_outcome"], true);
    assert_eq!(success(f.run("resume"))["historical_replay"], true);
    assert_eq!(f.sequence(), after);
    f.restore_plan();
    assert_eq!(success(f.run("reacquire-status"))["phase"], "not_started");
    assert_eq!(f.sequence(), after);
    assert_eq!(success(f.run("reacquire"))["phase"], "published");
    let restored = f.sequence();
    assert_eq!(success(f.run("reacquire-status"))["phase"], "published");
    success(f.run("reacquire"));
    assert_eq!(f.sequence(), restored);
    let s = success(f.run("status"));
    assert_eq!(s["phase"], "retired");
    assert_eq!(s["observed"], "verified_present");
    success(f.run("resume"));
    assert_eq!(f.sequence(), restored);
    assert!(path.exists());
    let mut read = LocalStore::open(&f.root.path().join("working"))
        .unwrap()
        .read_object(&f.publication.object)
        .unwrap();
    let mut bytes = Vec::new();
    read.read_to_end(&mut bytes).unwrap();
    assert_eq!(Object::from_bytes(&bytes), f.publication.object);
}

#[test]
#[ignore = "requires explicitly pinned pre-final git-asset-storage executable"]
fn pre_final_writer_refuses_new_manifest_and_accepts_legacy_control() {
    let baseline =
        std::env::var_os("GIT_ASSET_PRE_FINAL_CLI").expect("explicit pre-final artifact");
    for (final_provider, roster, clock) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let f = Fixture::with_clock_ledger(final_provider, roster, clock);
        let before = f.sequence();
        let output = Process::new(&baseline)
            .args(["retire", "plan"])
            .arg(f.root.path().join("profile.json"))
            .arg(f.root.path().join("request.json"))
            .output()
            .unwrap();
        assert!(output.stderr.is_empty());
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        if final_provider || roster || clock {
            assert!(
                !output.status.success(),
                "old writer accepted final-capable, roster-precedence or clock-ledger profile"
            );
            assert_eq!(response["ok"], false);
        } else {
            assert!(output.status.success(), "legacy control: {response}");
            retirement::Plan::from_json(&output.stdout).unwrap();
        }
        assert_eq!(f.sequence(), before);
        assert!(
            LocalStore::open(&f.root.path().join("working"))
                .unwrap()
                .read_object(&f.publication.object)
                .is_ok()
        );
    }
}
#[test]
fn independent_policy_refuses_disabled_owner_context_domain_budget_and_forged_plan() {
    let f = Fixture::new();
    f.make_plan();
    let before = f.sequence();
    for (field, value) in [
        ("allow_eviction", json!(false)),
        ("owner", json!("foreign")),
        ("context", json!(digest("changed"))),
        ("max_object_bytes", json!(1)),
        ("max_peak_bytes", json!(1)),
    ] {
        let mut p = f.profile.clone();
        p["policy"][field] = value;
        f.set_profile(&p);
        f.error("apply", "policy_denied");
        assert_eq!(f.sequence(), before);
    }
    let mut p = f.profile.clone();
    p["backups"][0]["enrollment"]["failure_domain"] = json!("forged-domain");
    f.set_profile(&p);
    f.error("apply", "policy_denied");
    f.set_profile(&f.profile);
    let path = f.root.path().join("plan.json");
    let original = fs::read(&path).unwrap();
    let mut p: Value = serde_json::from_slice(&original).unwrap();
    p["request"]["required_domains"] = json!(0);
    fs::write(&path, identity::canonical_json(&p).unwrap()).unwrap();
    f.error("apply", "policy_denied");
    fs::write(&path, original).unwrap();
    assert_eq!(f.sequence(), before);
    success(f.run("apply"));
}
#[test]
fn unmapped_roles_missing_keeper_and_reader_hold_preserve_bytes() {
    let f = Fixture::new();
    f.make_plan();
    let mut p = f.profile.clone();
    p["backups"][0]["places"] = json!([p["backups"][0]["places"][0].clone()]);
    f.set_profile(&p);
    f.error("apply", "unmapped_place");
    let mut p = f.profile.clone();
    p["backups"][0]["root"] = json!(f.root.path().join("unknown-keeper"));
    f.set_profile(&p);
    f.error("apply", "keeper_unavailable");
    assert!(!f.root.path().join("unknown-keeper").exists());
    f.set_profile(&f.profile);
    let store = LocalStore::open(&f.root.path().join("working")).unwrap();
    let read = store.read_object(&f.publication.object).unwrap();
    f.error("apply", "busy");
    drop(read);
    success(f.run("resume"));
}
#[test]
fn actual_source_claim_holds_removal_and_status_needs_no_keeper_or_metadata() {
    let f = Fixture::new();
    f.make_plan();
    let minimum = serde_json::from_value(f.profile["namespaces"][0]["minimum"].clone()).unwrap();
    let mut n = Namespace::open(&f.root.path().join("namespace"), &minimum).unwrap();
    let mut metadata = MetadataStore::open(
        &f.root.path().join("metadata"),
        &serde_json::from_value(f.profile["metadata"]["minimum"].clone()).unwrap(),
    )
    .unwrap();
    let entries = [(
        LogicalPath::parse("asset").unwrap(),
        f.publication.object.clone(),
    )]
    .into_iter()
    .collect();
    let (t, _) = Tree::build(&entries, &mut metadata, TreeLimits::default()).unwrap();
    metadata.finish().unwrap();
    let revision = n.source(&key("main")).unwrap().unwrap().revision;
    let prepared = n
        .apply(
            key("prepare"),
            Command::Prepare {
                source: key("main"),
                owner: key("owner"),
                expected_revision: revision,
                selection: Selection {
                    root: t.root().clone(),
                    context: digest("source"),
                },
            },
            &metadata,
            &limits(),
        )
        .unwrap();
    n.apply(
        key("adopt"),
        Command::Adopt {
            source: key("main"),
            owner: key("owner"),
            expected_revision: revision + 1,
            prepared: prepared.reference,
        },
        &metadata,
        &limits(),
    )
    .unwrap();
    f.error("apply", "required_by_claim");
    let before = f.sequence();
    let mut p = f.profile.clone();
    p["backups"][0]["root"] = json!(f.root.path().join("offline"));
    p["metadata"]["root"] = json!(f.root.path().join("offline-tree"));
    p["namespaces"][0]["root"] = json!(f.root.path().join("offline-namespace"));
    f.set_profile(&p);
    assert_eq!(success(f.run("status"))["observed"], "verified_present");
    assert_eq!(f.sequence(), before);
}
#[test]
fn strict_documents_and_absent_roots_fail_without_bootstrap() {
    let f = Fixture::new();
    f.make_plan();
    let before = f.sequence();
    let mut p = f.profile.clone();
    p["source_root"] = json!(f.root.path().join("unknown"));
    f.set_profile(&p);
    f.error("apply", "source_unavailable");
    assert!(!f.root.path().join("unknown").exists());
    f.set_profile(&f.profile);
    let path = f.root.path().join("plan.json");
    let original = fs::read(&path).unwrap();
    fs::write(&path, [original.as_slice(), b"\n"].concat()).unwrap();
    f.error("apply", "invalid_plan");
    fs::write(&path, vec![b' '; 65537]).unwrap();
    f.error("apply", "invalid_document");
    fs::write(&path, original).unwrap();
    let mut p = f.profile.clone();
    p["extra"] = json!(true);
    f.set_profile(&p);
    f.error("apply", "invalid_document");
    assert_eq!(f.sequence(), before);
}
#[test]
fn discovery_reports_separate_execution_flags_and_no_final_deletion() {
    let r = success(
        Process::new(env!("CARGO_BIN_EXE_git-asset-storage"))
            .args(["retire", "describe"])
            .output()
            .unwrap(),
    );
    assert_eq!(r["default_apply"], false);
    assert_eq!(r["final_deletion"], false);
    assert_eq!(r["initializes_stores"], false);
}

#[test]
fn cancellation_and_reacquisition_have_independent_disabled_defaults() {
    let f = Fixture::new();
    f.make_plan();
    let before = f.sequence();
    let mut p = f.profile.clone();
    p["policy"]
        .as_object_mut()
        .unwrap()
        .remove("allow_cancellation");
    f.set_profile(&p);
    f.error("cancel", "policy_denied");
    assert_eq!(f.sequence(), before);
    let mut p = f.profile.clone();
    p["policy"]
        .as_object_mut()
        .unwrap()
        .remove("allow_eviction");
    f.set_profile(&p);
    f.error("apply", "policy_denied");
    assert_eq!(f.sequence(), before);
    f.set_profile(&f.profile);
    success(f.run("apply"));
    f.restore_plan();
    let retired = f.sequence();
    let mut p = f.profile.clone();
    p["policy"]
        .as_object_mut()
        .unwrap()
        .remove("allow_reacquisition");
    f.set_profile(&p);
    f.error("reacquire", "policy_denied");
    assert_eq!(f.sequence(), retired);
    assert_eq!(success(f.run("reacquire-status"))["phase"], "not_started");
    f.set_profile(&f.profile);
    f.error("cancel", "invalid_plan");
    assert_eq!(f.sequence(), retired);
    let mut p = f.profile.clone();
    p["backups"][0]["root"] = json!(f.root.path().join("offline-backup"));
    f.set_profile(&p);
    f.error("reacquire", "keeper_unavailable");
    assert_eq!(f.sequence(), retired);
    f.set_profile(&f.profile);
    success(f.run("reacquire"));
}

#[test]
fn corrupt_reacquired_bytes_are_unproven_and_never_repaired_by_status() {
    let f = Fixture::new();
    f.make_plan();
    success(f.run("apply"));
    f.restore_plan();
    success(f.run("reacquire"));
    let path = f
        .root
        .path()
        .join("working/objects")
        .join(f.publication.object.sha256.as_str());
    fs::write(&path, b"corrupt").unwrap();
    let before = f.sequence();
    let status = success(f.run("status"));
    assert_eq!(status["phase"], "retired");
    assert_eq!(status["observed"], "unproven");
    let status = success(f.run("reacquire-status"));
    assert_eq!(status["phase"], "recovery_required");
    assert_eq!(status["historical_outcome"], false);
    assert_eq!(f.sequence(), before);
    assert_eq!(fs::read(&path).unwrap(), b"corrupt");
    fs::remove_file(&path).unwrap();
    let status = success(f.run("reacquire-status"));
    assert_eq!(status["phase"], "recovery_required");
    assert_ne!(status["observed"], "authorized_absence");
    assert!(!path.exists());
    assert_eq!(f.sequence(), before);
}
