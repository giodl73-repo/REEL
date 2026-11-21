//! Actual operator CLI over synthetic pre-existing metadata/source/clock roots.
use git_asset_core::{
    Digest, LogicalPath, Object, identity,
    tree::{Tree, TreeLimits},
};
use git_asset_store::{
    Key, clock,
    ledger::CapacityLedger,
    metadata::{self, MetadataStore},
    source_claims::{
        Command, Kind, Places, Selection,
        admission::{Namespace, Policy, Select},
        age, subset,
    },
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command as Process};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn digest(v: &impl serde::Serialize) -> Digest {
    Digest::blob(&identity::canonical_json(&serde_json::to_value(v).unwrap()).unwrap())
}
struct Fixture {
    temp: tempfile::TempDir,
    profile: Value,
    request: Value,
    ns: Namespace,
    registry: Namespace,
    metadata: MetadataStore,
    clock_min: git_asset_store::ledger::Checkpoint,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let places = Places {
            current: vec![key("F"), key("M")],
            pending: vec![key("F"), key("M")],
            predecessor: vec![key("D"), key("M")],
        };
        let policy = Policy {
            prefer_active: true,
            classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
                .into_iter()
                .map(|k| (k, places.clone()))
                .collect(),
        };
        let mut ns = Namespace::create(
            &root.join("source"),
            key("project"),
            key("epoch"),
            policy.clone(),
        )
        .unwrap();
        let mut registry = Namespace::create(
            &root.join("registry"),
            key("registry"),
            key("epoch"),
            policy,
        )
        .unwrap();
        let mut metadata = MetadataStore::create(
            &root.join("metadata"),
            metadata::Limits {
                nodes: 10000,
                bytes: 8 * 1024 * 1024,
                database_bytes: 32 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let map = [
            ("old", Object::from_bytes(b"old")),
            ("keep", Object::from_bytes(b"keep")),
        ]
        .into_iter()
        .map(|(p, o)| (LogicalPath::parse(p).unwrap(), o))
        .collect();
        let root_tree = Tree::build(&map, &mut metadata, TreeLimits::default())
            .unwrap()
            .0
            .root()
            .clone();
        metadata.finish().unwrap();
        for (n, name, class) in [
            (&mut ns, "main", Kind::Branch),
            (&mut registry, "records", Kind::Retention),
        ] {
            n.apply(
                key("enroll"),
                Command::Enroll {
                    source: key(name),
                    owner: key("owner"),
                    class,
                },
                &metadata,
                &age::Limits::default().source,
            )
            .unwrap();
        }
        ns.select(
            &Select {
                prepare: key("prepare"),
                adopt: key("adopt"),
                source: key("main"),
                owner: key("owner"),
                expected_revision: 1,
                selection: Selection {
                    root: root_tree,
                    context: Digest::blob(b"fixture-context"),
                },
            },
            &metadata,
            &age::Limits::default().source,
        )
        .unwrap();
        ns.apply(
            key("withdraw"),
            Command::Withdraw {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 3,
            },
            &metadata,
            &age::Limits::default().source,
        )
        .unwrap();
        let ledger = CapacityLedger::create_clock(
            &root.join("clock"),
            key("clock"),
            key("epoch"),
            0,
            clock::Policy {
                authority: Digest::blob(b"authority"),
                epoch: Digest::blob(b"epoch"),
                boundary: Digest::blob(b"native-os"),
                max_sample_millis: 1000,
                max_drift_millis: 1000,
                max_session_millis: 60000,
            },
        )
        .unwrap();
        let clock_enrollment = ledger.clock_enrollment().unwrap();
        let clock_min = ledger.checkpoint();
        let enrollment = age::Enrollment {
            authority: Digest::blob(b"authority"),
            epoch: Digest::blob(b"epoch"),
            metadata: metadata.checkpoint().identity().clone(),
            registry: registry.identity().clone(),
            registry_policy: digest(registry.policy()),
            source: key("records"),
            owner: key("owner"),
            clock: clock_enrollment.id().unwrap(),
            observers: vec![age::Observer {
                namespace: ns.identity().clone(),
                policy: digest(ns.policy()),
                owners: vec![key("owner")],
                classes: [Kind::Branch, Kind::Draft].into_iter().collect(),
            }],
        };
        let subset = subset::Enrollment {
            authority: enrollment.authority.clone(),
            namespace: ns.identity().clone(),
            policy: digest(ns.policy()),
            owner: key("owner"),
            classes: vec![Kind::Branch, Kind::Draft],
            limits: age::Limits::default().source,
            writes: 10000,
            write_bytes: 8 * 1024 * 1024,
        };
        let profile = json!({"schema":"tig.age-profile.design-1","enrollment":enrollment,"registry":{"root":root.join("registry"),"minimum":registry.checkpoint()},"observers":[{"root":root.join("source"),"minimum":ns.checkpoint()}],"metadata":{"root":root.join("metadata"),"minimum":metadata.checkpoint()},"clock":{"root":root.join("clock"),"minimum":clock_min,"enrollment":clock_enrollment},"required_age_millis":0,"subset":subset});
        let request = json!({"namespace":ns.identity(),"source":"main","predecessor_revision":3});
        Self {
            temp,
            profile,
            request,
            ns,
            registry,
            metadata,
            clock_min,
        }
    }
    fn file(&self, n: &str) -> PathBuf {
        self.temp.path().join(n)
    }
    fn call(
        &self,
        op: &str,
        profile: &Value,
        request: Option<&Value>,
        plan: Option<&subset::Plan>,
    ) -> (bool, Value) {
        fs::write(
            self.file("profile.json"),
            serde_json::to_vec(profile).unwrap(),
        )
        .unwrap();
        let mut c = Process::new(env!("CARGO_BIN_EXE_git-asset-storage"));
        c.args(["age", op]).arg(self.file("profile.json"));
        if let Some(r) = request {
            fs::write(self.file("request.json"), serde_json::to_vec(r).unwrap()).unwrap();
            c.arg(self.file("request.json"));
        }
        if let Some(p) = plan {
            fs::write(self.file("plan.json"), p.to_json().unwrap()).unwrap();
            c.arg(self.file("plan.json"));
        }
        let out = c.output().unwrap();
        assert!(out.stderr.is_empty(), "{out:?}");
        let text = String::from_utf8(out.stdout.clone()).unwrap();
        assert!(!text.contains(self.temp.path().to_str().unwrap()));
        (
            out.status.success(),
            serde_json::from_slice(&out.stdout).unwrap(),
        )
    }
    fn enabled(&self) -> Value {
        let mut p = self.profile.clone();
        p["permissions"] = json!({"initialize":true,"capture":true,"inherit":true});
        p
    }
    fn checkpoints(
        &mut self,
    ) -> (
        git_asset_store::source_claims::Checkpoint,
        git_asset_store::source_claims::Checkpoint,
        metadata::Checkpoint,
        git_asset_store::ledger::Checkpoint,
    ) {
        let n = Namespace::open(&self.file("source"), &self.ns.checkpoint())
            .unwrap()
            .checkpoint();
        let r = Namespace::open(&self.file("registry"), &self.registry.checkpoint())
            .unwrap()
            .checkpoint();
        let m = MetadataStore::open(&self.file("metadata"), &self.metadata.checkpoint())
            .unwrap()
            .checkpoint();
        let c = CapacityLedger::open(&self.file("clock"), &self.clock_min)
            .unwrap()
            .checkpoint();
        (n, r, m, c)
    }
}
#[test]
fn permissions_precede_root_io_and_capture_has_no_timestamp_input() {
    let discovery = Process::new(env!("CARGO_BIN_EXE_git-asset-storage"))
        .arg("describe")
        .output()
        .unwrap();
    assert!(discovery.status.success());
    let discovery: Value = serde_json::from_slice(&discovery.stdout).unwrap();
    assert_eq!(
        discovery["additional_interfaces"]["age"],
        "git-asset-storage age describe"
    );
    let mut f = Fixture::new();
    let before = f.checkpoints();
    for op in ["initialize", "capture"] {
        let mut p = f.profile.clone();
        p["metadata"]["root"] = json!(f.file("absent-metadata"));
        p["registry"]["root"] = json!(f.file("absent-registry"));
        let (ok, value) = f.call(
            op,
            &p,
            if op == "capture" {
                Some(&f.request)
            } else {
                None
            },
            None,
        );
        assert!(!ok);
        assert_eq!(value["code"], "execution_disabled");
    }
    let p = f.enabled();
    let mut request = f.request.clone();
    request["observed_millis"] = json!(1);
    let (ok, value) = f.call("capture", &p, Some(&request), None);
    assert!(!ok);
    assert_eq!(value["code"], "invalid_document");
    assert_eq!(before, f.checkpoints());
    assert!(!f.file("absent-metadata").exists());
    assert!(!f.file("absent-registry").exists());
}
#[test]
fn actual_native_clock_capture_offline_retry_readonly_status_and_assessment() {
    let mut f = Fixture::new();
    let p = f.enabled();
    let before = f.checkpoints();
    assert!(f.call("initialize", &p, None, None).0);
    let (ok, capture) = f.call("capture", &p, Some(&f.request), None);
    assert!(ok, "{capture}");
    let observed = &capture["status"]["observed"];
    assert!(observed["earliest"].as_u64().unwrap() > 0);
    let captured = f.checkpoints();
    assert_eq!(before.0, captured.0);
    assert_ne!(before.3, captured.3);
    let mut offline = p.clone();
    offline["clock"]["root"] = json!(f.file("absent-clock"));
    let (ok, retry) = f.call("capture", &offline, Some(&f.request), None);
    assert!(ok, "{retry}");
    assert_eq!(capture["status"]["evidence"], retry["status"]["evidence"]);
    assert!(f.call("initialize", &p, None, None).0);
    assert_eq!(captured, f.checkpoints());
    offline["clock"] = Value::Null;
    let (ok, status) = f.call("status", &offline, Some(&f.request), None);
    assert!(ok);
    assert_eq!(status["observed"], *observed);
    assert_eq!(captured, f.checkpoints());
    let (ok, assessed) = f.call("assess", &p, Some(&f.request), None);
    assert!(ok, "{assessed}");
    assert_eq!(assessed["authorizes_removal"], false);
    let after = f.checkpoints();
    assert_eq!(
        (captured.0, captured.1, captured.2),
        (after.0, after.1, after.2)
    );
    assert_ne!(captured.3, after.3);
    let mut unknown = f.request.clone();
    unknown["predecessor_revision"] = json!(99);
    let mut offline = p.clone();
    offline["clock"]["root"] = json!(f.file("absent-clock"));
    let (ok, a) = f.call("assess", &offline, Some(&unknown), None);
    assert!(ok);
    assert_eq!(a["hold"], "unknown_age");
    assert!(!f.file("absent-clock").exists());
}
#[test]
fn cli_inheritance_requires_executed_subset_and_preserves_original_observation() {
    let mut f = Fixture::new();
    let p = f.enabled();
    assert!(f.call("initialize", &p, None, None).0);
    let (ok, captured) = f.call("capture", &p, Some(&f.request), None);
    assert!(ok);
    let e: subset::Enrollment = serde_json::from_value(p["subset"].clone()).unwrap();
    let plan = subset::plan(
        &e,
        &mut f.ns,
        subset::Request {
            source: key("main"),
            predecessor_revision: 3,
            paths: vec![LogicalPath::parse("old").unwrap()],
        },
        &f.metadata.finished_reader().unwrap(),
    )
    .unwrap();
    assert!(!f.call("inherit", &p, Some(&f.request), Some(&plan)).0);
    subset::apply(&e, &mut f.ns, &mut f.metadata, &plan).unwrap();
    let (ok, inherited) = f.call("inherit", &p, Some(&f.request), Some(&plan));
    assert!(ok, "{inherited}");
    assert_eq!(
        inherited["status"]["observed"],
        captured["status"]["observed"]
    );
    let before = f.checkpoints();
    let (ok, retry) = f.call("inherit", &p, Some(&f.request), Some(&plan));
    assert!(ok);
    assert_eq!(retry["status"]["evidence"], inherited["status"]["evidence"]);
    assert_eq!(before, f.checkpoints());
}
