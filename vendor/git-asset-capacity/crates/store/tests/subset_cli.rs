//! Actual CLI, synthetic pre-existing namespaces/metadata only. No payload IO.
use git_asset_core::{
    Digest, LogicalPath, Object, identity,
    tree::{Tree, TreeLimits},
};
use git_asset_store::{
    Key,
    metadata::{self, MetadataStore},
    protection::Limits,
    source_claims::{
        Command, Kind, Places, Selection, SourceJournal,
        admission::{Namespace, Policy, Select},
        subset::{Enrollment, Plan},
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command as Process, Output},
};

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn limits() -> Limits {
    Limits {
        millis: 60_000,
        roots: 100,
        bindings: 10000,
        associations: 10000,
        reads: 100000,
        read_bytes: 32 * 1024 * 1024,
    }
}
fn canonical(v: &impl serde::Serialize) -> Vec<u8> {
    identity::canonical_json(&serde_json::to_value(v).unwrap()).unwrap()
}
fn run(mode: &str, profile: &Path, input: &Path) -> Output {
    Process::new(env!("CARGO_BIN_EXE_git-asset-source"))
        .arg("subset")
        .arg(mode)
        .arg(profile)
        .arg(input)
        .output()
        .unwrap()
}
fn success(output: Output) -> Vec<u8> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
struct Fixture {
    temp: tempfile::TempDir,
    profile: Value,
    ns: Namespace,
    a: Object,
    b: Object,
}
impl Fixture {
    fn new(class: Kind) -> Self {
        let temp = tempfile::tempdir().unwrap();
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
            &temp.path().join("ns"),
            key("project"),
            key("epoch"),
            policy,
        )
        .unwrap();
        let mut metadata = MetadataStore::create(
            &temp.path().join("metadata"),
            metadata::Limits {
                nodes: 1000,
                bytes: 1024 * 1024,
                database_bytes: 8 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let a = Object::from_bytes(b"old-selected-cue");
        let b = Object::from_bytes(b"held-cel");
        let bindings = [
            (LogicalPath::parse("old-cue").unwrap(), a.clone()),
            (LogicalPath::parse("held-cel").unwrap(), b.clone()),
        ]
        .into_iter()
        .collect();
        let (tree, _) = Tree::build(&bindings, &mut metadata, TreeLimits::default()).unwrap();
        metadata.finish().unwrap();
        ns.apply(
            key("enroll"),
            Command::Enroll {
                source: key("source"),
                owner: key("owner"),
                class: class.clone(),
            },
            &metadata,
            &limits(),
        )
        .unwrap();
        ns.select(
            &Select {
                prepare: key("prepare"),
                adopt: key("adopt"),
                source: key("source"),
                owner: key("owner"),
                expected_revision: 1,
                selection: Selection {
                    root: tree.root().clone(),
                    context: Digest::blob(b"old"),
                },
            },
            &metadata,
            &limits(),
        )
        .unwrap();
        ns.apply(
            key("withdraw"),
            Command::Withdraw {
                source: key("source"),
                owner: key("owner"),
                expected_revision: 3,
            },
            &metadata,
            &limits(),
        )
        .unwrap();
        let enrollment = Enrollment {
            authority: Digest::blob(b"operator"),
            namespace: ns.identity().clone(),
            policy: Digest::blob(&canonical(ns.policy())),
            owner: key("owner"),
            classes: vec![class],
            limits: limits(),
            writes: 1000,
            write_bytes: 1024 * 1024,
        };
        let profile = json!({"schema":"tig.subset-profile.design-1","enrollment":enrollment,
            "namespace_root":temp.path().join("ns"),"namespace_minimum":ns.checkpoint(),
            "metadata_root":temp.path().join("metadata"),"metadata_minimum":metadata.checkpoint()});
        fs::write(temp.path().join("profile.json"), canonical(&profile)).unwrap();
        fs::write(
            temp.path().join("request.json"),
            canonical(&json!({"source":"source","predecessor_revision":3,"paths":["old-cue"]})),
        )
        .unwrap();
        Self {
            temp,
            profile,
            ns,
            a,
            b,
        }
    }
    fn file(&self, name: &str) -> std::path::PathBuf {
        self.temp.path().join(name)
    }
    fn enable(&mut self) {
        self.profile["allow_settlement"] = true.into();
        fs::write(self.file("profile.json"), canonical(&self.profile)).unwrap();
    }
}

#[test]
fn cli_defaults_resume_and_metadata_offline_status_preserve_exact_survivors() {
    for class in [Kind::Branch, Kind::Draft] {
        let mut f = Fixture::new(class);
        let before = f.ns.checkpoint();
        let bytes = success(run(
            "plan",
            &f.file("profile.json"),
            &f.file("request.json"),
        ));
        let plan = Plan::from_json(&bytes).unwrap();
        assert_eq!(plan.summary.selected_bindings, 1);
        assert_eq!(plan.summary.survivor_bindings, 1);
        let serialized = String::from_utf8(bytes.clone()).unwrap();
        assert!(!serialized.contains(f.temp.path().to_str().unwrap()));
        fs::write(f.file("plan.json"), bytes).unwrap();
        assert!(
            !run("apply", &f.file("profile.json"), &f.file("plan.json"))
                .status
                .success()
        );
        assert_eq!(
            SourceJournal::observe(&f.file("ns"), &before, &key("source"))
                .unwrap()
                .checkpoint,
            before
        );
        f.enable();
        let result: Value = serde_json::from_slice(&success(run(
            "apply",
            &f.file("profile.json"),
            &f.file("plan.json"),
        )))
        .unwrap();
        assert_eq!(result["settlement_current"], true);
        assert_eq!(result["authorizes_payload_deletion"], false);
        let checkpoint =
            f.ns.requirements(
                &metadata::MetadataReader::open(
                    &f.file("metadata"),
                    &serde_json::from_value(f.profile["metadata_minimum"].clone()).unwrap(),
                )
                .unwrap(),
                &limits(),
            )
            .unwrap();
        assert!(checkpoint.entries.iter().all(|e| e.object != f.a));
        assert_eq!(checkpoint.entries[0].object, f.b);
        assert_eq!(checkpoint.entries[0].places, vec![key("D"), key("M")]);
        let repeated: Value = serde_json::from_slice(&success(run(
            "resume",
            &f.file("profile.json"),
            &f.file("plan.json"),
        )))
        .unwrap();
        assert_eq!(repeated["historical_retirement"]["replayed"], true);
        fs::rename(f.file("metadata"), f.file("metadata-offline")).unwrap();
        let offline: Value = serde_json::from_slice(&success(run(
            "status",
            &f.file("profile.json"),
            &f.file("plan.json"),
        )))
        .unwrap();
        assert_eq!(offline["settlement_current"], true);
    }
}

#[test]
fn cli_bad_profile_and_noncanonical_plan_refuse_without_source_changes() {
    let mut f = Fixture::new(Kind::Branch);
    f.enable();
    let before = f.ns.checkpoint();
    let mut bytes = success(run(
        "plan",
        &f.file("profile.json"),
        &f.file("request.json"),
    ));
    bytes.push(b'\n');
    fs::write(f.file("plan.json"), bytes).unwrap();
    let failure = run("apply", &f.file("profile.json"), &f.file("plan.json"));
    assert!(!failure.status.success());
    assert!(!String::from_utf8_lossy(&failure.stderr).contains(f.temp.path().to_str().unwrap()));
    f.profile["enrollment"]["owner"] = "foreign".into();
    fs::write(f.file("profile.json"), canonical(&f.profile)).unwrap();
    assert!(
        !run("plan", &f.file("profile.json"), &f.file("request.json"))
            .status
            .success()
    );
    assert_eq!(
        SourceJournal::observe(&f.file("ns"), &before, &key("source"))
            .unwrap()
            .checkpoint,
        before
    );
}
