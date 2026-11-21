use super::*;
use crate::{
    AckLevel, accounted,
    ledger::{CapacityLedger, JournalRef},
    local::LocalStore,
    local_capacity::LocalCapacityProbe,
    metadata::{Limits as MetadataLimits, MetadataStore},
    retention,
    source_claims::{
        Kind, Places, Selection as RootSelection,
        admission::{Policy, Select},
    },
    store_protection::{Binding as StoreBinding, Member},
};
use git_asset_core::tree::{Tree, TreeLimits};
use std::{cell::Cell, io::Cursor};

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn d(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
fn path(s: &str) -> LogicalPath {
    LogicalPath::parse(s).unwrap()
}
fn limits() -> crate::protection::Limits {
    crate::protection::Limits {
        millis: 60_000,
        roots: 256,
        bindings: 100_000,
        associations: 100_000,
        reads: 100_000,
        read_bytes: 64 * 1024 * 1024,
    }
}
fn policy() -> Policy {
    let regular = Places {
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
            (Kind::Branch, regular.clone()),
            (Kind::Draft, regular.clone()),
            (Kind::Job, regular),
            (Kind::Retention, retained),
        ]
        .into_iter()
        .collect(),
    }
}
struct Clock(Cell<u64>, Digest);
impl provider::Clock for Clock {
    fn now(&self, policy: &Digest) -> provider::Result<u64> {
        if policy != &self.1 {
            return Err(provider::Error::Clock);
        }
        Ok(self.0.get())
    }
}
fn root(metadata: &mut MetadataStore, entries: &[(&str, Object)]) -> RootSelection {
    let map = entries.iter().map(|(p, o)| (path(p), o.clone())).collect();
    let (tree, _) = Tree::build(&map, metadata, TreeLimits::default()).unwrap();
    metadata.finish().unwrap();
    RootSelection {
        root: tree.root().clone(),
        context: d("root-context"),
    }
}
fn enroll(ns: &mut Namespace, metadata: &MetadataStore, source: &str, owner: &str, class: Kind) {
    ns.apply(
        key(&format!("enroll-{source}")),
        Command::Enroll {
            source: key(source),
            owner: key(owner),
            class,
        },
        metadata,
        &limits(),
    )
    .unwrap();
}
fn select(
    ns: &mut Namespace,
    metadata: &MetadataStore,
    source: &str,
    owner: &str,
    selection: RootSelection,
    label: &str,
) {
    let expected_revision = ns.source(&key(source)).unwrap().unwrap().revision;
    ns.select(
        &Select {
            prepare: key(&format!("prepare-{label}")),
            adopt: key(&format!("adopt-{label}")),
            source: key(source),
            owner: key(owner),
            expected_revision,
            selection,
        },
        metadata,
        &limits(),
    )
    .unwrap();
}
struct Fixture {
    temp: tempfile::TempDir,
    namespaces: Vec<Namespace>,
    metadata: MetadataStore,
    roster: claims::Roster,
    store: LocalStore,
    ledger: CapacityLedger,
    job: JournalRef,
    witness: LocalStore,
    witness_ledger: CapacityLedger,
    witness_job: JournalRef,
    enrollment: Enrollment,
    clock: Clock,
    publications: BTreeMap<String, Publication>,
}

#[test]
fn bulk_requires_the_whole_clock_interval_before_intent_or_source_settlement() {
    struct Uncertain;
    impl provider::Clock for Uncertain {
        fn now(&self, _: &Digest) -> provider::Result<u64> {
            panic!("bulk bypassed interval")
        }
        fn bounds(&self, _: &Digest) -> provider::Result<provider::TimeBounds> {
            Ok(provider::TimeBounds {
                earliest: 100,
                latest: 500,
            })
        }
    }
    let mut f = Fixture::new();
    let p = f.plan();
    let before = (f.ledger.checkpoint(), f.witness_ledger.checkpoint());
    let sources = f
        .namespaces
        .iter()
        .map(Namespace::checkpoint)
        .collect::<Vec<_>>();
    let jobs = vec![f.job.clone(); p.actions.len()];
    let mut places = [Local {
        store: &f.store,
        ledger: &mut f.ledger,
        job: &f.job,
    }];
    let mut e = Execution {
        enrollment: &f.enrollment,
        roster: &f.roster,
        namespaces: &mut f.namespaces,
        metadata: &mut f.metadata,
        places: &mut places,
        action_jobs: &jobs,
        witness: Local {
            store: &f.witness,
            ledger: &mut f.witness_ledger,
            job: &f.witness_job,
        },
        clock: &Uncertain,
        admitted_context: &f.enrollment.context,
    };
    assert!(matches!(
        e.apply(&p, &p.authorize().unwrap()),
        Err(Error::Provider(provider::Error::Clock))
    ));
    assert_eq!(
        before,
        (f.ledger.checkpoint(), f.witness_ledger.checkpoint())
    );
    assert_eq!(
        sources,
        f.namespaces
            .iter()
            .map(Namespace::checkpoint)
            .collect::<Vec<_>>()
    );
}

#[test]
fn inspection_needs_no_metadata_writer_or_clock_and_keeps_historical_bindings_exact() {
    for phase in [Phase::NotStarted, Phase::Intent, Phase::Completed] {
        let mut f = Fixture::new();
        let p = f.plan();
        if phase == Phase::Intent {
            assert!(
                f.run(&p, |e| e.apply_hook(&p, &p.authorize().unwrap(), |s| {
                    if s == Stage::Intent {
                        Err(Error::Injected)
                    } else {
                        Ok(())
                    }
                }))
                .is_err()
            );
        } else if phase == Phase::Completed {
            f.run(&p, |e| e.apply(&p, &p.authorize().unwrap())).unwrap();
        }
        let metadata = f.metadata.checkpoint().identity().clone();
        drop(f.metadata);
        std::fs::rename(
            f.temp.path().join("metadata"),
            f.temp.path().join("metadata-offline"),
        )
        .unwrap();
        let before = (f.ledger.checkpoint(), f.witness_ledger.checkpoint());
        let sources = f
            .namespaces
            .iter()
            .map(Namespace::checkpoint)
            .collect::<Vec<_>>();
        let jobs = vec![f.job.clone(); p.actions.len()];
        let mut places = [Local {
            store: &f.store,
            ledger: &mut f.ledger,
            job: &f.job,
        }];
        let mut witness = Local {
            store: &f.witness,
            ledger: &mut f.witness_ledger,
            job: &f.witness_job,
        };
        let mut inspection = Inspection {
            enrollment: &f.enrollment,
            roster: &f.roster,
            namespaces: &mut f.namespaces,
            metadata,
            places: &mut places,
            action_jobs: &jobs,
            witness: &mut witness,
            admitted_context: &f.enrollment.context,
        };
        let report = inspection.status(&p).unwrap();
        assert_eq!(report.phase, phase);
        assert!(!report.removal_authority);
        assert!(report.effects.is_empty());
        assert_eq!(report.new_logical_copy_bytes_removed, 0);
        assert!(inspection.required_metadata_peak(&p).unwrap() > 0);
        if phase != Phase::NotStarted {
            inspection.metadata = d("wrong-private-metadata-identity");
            assert!(inspection.status(&p).is_err());
        }
        assert_eq!(
            before,
            (f.ledger.checkpoint(), f.witness_ledger.checkpoint())
        );
        assert_eq!(
            sources,
            f.namespaces
                .iter()
                .map(Namespace::checkpoint)
                .collect::<Vec<_>>()
        );
        assert!(!f.temp.path().join("metadata").exists());
    }
}

fn inspection_profile(f: &Fixture) -> serde_json::Value {
    serde_json::json!({
        "schema":"tig.bulk-inspection-profile.design-1", "enrollment":f.enrollment,
        "roster":f.roster, "admitted_context":f.enrollment.context,
        "metadata":{"root":f.temp.path().join("metadata"),"minimum":f.metadata.checkpoint()},
        "namespaces":f.namespaces.iter().enumerate().map(|(i,n)| serde_json::json!({
            "root":f.temp.path().join(format!("ns-{i}")),
            "minimum":f.roster.members.iter().find(|p| &p.namespace == n.identity()).unwrap().minimum,
        })).collect::<Vec<_>>(),
        "stores":[{"root":f.temp.path().join("M"),"ledger_root":f.temp.path().join("ledger"),
            "minimum":f.ledger.checkpoint(),"job":f.job,"place":Place::local(&f.store,1).unwrap()}],
        "witness":{"root":f.temp.path().join("witness"),"ledger_root":f.temp.path().join("witness-ledger"),
            "minimum":f.witness_ledger.checkpoint(),"job":f.witness_job,"place":f.enrollment.witness},
    })
}

fn inspection_files(root: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    fn walk(
        base: &std::path::Path,
        at: &std::path::Path,
        out: &mut Vec<(std::path::PathBuf, Vec<u8>)>,
    ) {
        for entry in std::fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                out.push((
                    path.strip_prefix(base).unwrap().to_owned(),
                    std::fs::read(path).unwrap(),
                ));
            }
        }
    }
    let mut files = Vec::new();
    walk(root, root, &mut files);
    files.sort();
    files
}

#[test]
#[ignore = "requires explicitly fresh git-asset-storage executable"]
fn explicit_cli_bulk_inspection_qualification() {
    let cli = std::env::var_os("GIT_ASSET_BULK_CLI").expect("explicit fresh CLI artifact");
    let describe = std::process::Command::new(&cli)
        .args(["bulk", "describe"])
        .output()
        .unwrap();
    assert!(describe.status.success());
    assert!(describe.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&describe.stdout).unwrap();
    assert_eq!(value["inspect_only"], true);
    assert_eq!(value["final_deletion"], false);
    for expected in ["not_started", "intent", "completed"] {
        let mut f = Fixture::new();
        let p = f.plan();
        if expected == "intent" {
            assert!(
                f.run(&p, |e| e.apply_hook(&p, &p.authorize().unwrap(), |s| {
                    if s == Stage::Intent {
                        Err(Error::Injected)
                    } else {
                        Ok(())
                    }
                }))
                .is_err()
            );
        } else if expected == "completed" {
            f.run(&p, |e| e.apply(&p, &p.authorize().unwrap())).unwrap();
        }
        let dir = f.temp.path();
        let private_prefix = serde_json::to_string(&dir.to_string_lossy()).unwrap();
        let private_prefix = &private_prefix[1..private_prefix.len() - 1];
        let profile_path = dir.join("inspection.json");
        let input = dir.join("input.json");
        let mut profile = inspection_profile(&f);
        let write_profile = |value: &serde_json::Value| {
            std::fs::write(&profile_path, serde_json::to_vec(value).unwrap()).unwrap()
        };
        write_profile(&profile);
        let call = |op: &str| {
            std::process::Command::new(&cli)
                .args(["bulk", op])
                .arg(&profile_path)
                .arg(&input)
                .output()
                .unwrap()
        };
        let assert_error = |op: &str, expected: &str| {
            let output = call(op);
            assert!(!output.status.success(), "{output:?}");
            assert!(output.stderr.is_empty());
            let error: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(error["code"], expected);
            assert!(
                !String::from_utf8(output.stdout)
                    .unwrap()
                    .contains(private_prefix)
            );
        };
        let before = (f.ledger.checkpoint(), f.witness_ledger.checkpoint());
        let provider_before = (
            inspection_files(&dir.join("M")),
            inspection_files(&dir.join("witness")),
        );
        let sources = f
            .namespaces
            .iter()
            .map(Namespace::checkpoint)
            .collect::<Vec<_>>();
        if expected == "not_started" {
            std::fs::write(&input, encode(p.request()).unwrap()).unwrap();
            let output = call("plan");
            assert!(output.status.success(), "{output:?}");
            assert!(output.stderr.is_empty());
            assert_eq!(output.stdout, p.to_json().unwrap());
            let mut missing = profile.clone();
            missing["stores"][0]["root"] = serde_json::json!(dir.join("absent-provider"));
            write_profile(&missing);
            let output = call("plan");
            assert!(output.status.success(), "{output:?}");
            let held = Plan::from_json(&output.stdout).unwrap();
            assert!(held.subjects().iter().all(|s| s.hold.is_some()));
            assert_eq!(held.actions().len(), 0);
            missing["namespaces"][0]["root"] = serde_json::json!(dir.join("absent-source"));
            write_profile(&missing);
            assert_error("plan", "source_unavailable");
            write_profile(&profile);
        }
        std::fs::write(&input, p.to_json().unwrap()).unwrap();
        let mut wrong = profile.clone();
        wrong["admitted_context"] = serde_json::json!(d("forged-context"));
        write_profile(&wrong);
        assert_error("status", "invalid_profile");
        wrong = profile.clone();
        wrong["unexpected_permission"] = serde_json::json!(true);
        write_profile(&wrong);
        assert_error("status", "invalid_document");
        wrong = profile.clone();
        wrong["stores"][0]["minimum"]["genesis"] = serde_json::json!(d("wrong-capacity"));
        write_profile(&wrong);
        assert_error("status", "wrong_capacity");
        wrong = profile.clone();
        wrong["stores"][0]["root"] = serde_json::json!(dir.join("witness"));
        write_profile(&wrong);
        assert_error("status", "wrong_store");
        write_profile(&profile);
        std::fs::write(&input, vec![b' '; 65_537]).unwrap();
        assert_error("status", "invalid_document");
        std::fs::write(&input, p.to_json().unwrap()).unwrap();
        profile["metadata"]["root"] = serde_json::json!(dir.join("absent-metadata"));
        write_profile(&profile);
        let peak = call("peak");
        assert!(peak.status.success(), "{peak:?}");
        let peak: serde_json::Value = serde_json::from_slice(&peak.stdout).unwrap();
        assert!(peak["required_witness_metadata_peak"].as_u64().unwrap() > 0);
        let output = call("status");
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(status["phase"], expected);
        assert_eq!(status["removal_authority"], false);
        assert_eq!(status["new_logical_copy_bytes_removed"], 0);
        assert_eq!(status["selected_roots"], 2);
        assert_eq!(status["unique_objects"], 4);
        assert!(
            !String::from_utf8(output.stdout)
                .unwrap()
                .contains(private_prefix)
        );
        assert!(!dir.join("absent-metadata").exists());
        for operation in ["apply", "resume", "cancel", "reacquire"] {
            let output = call(operation);
            assert!(!output.status.success());
            assert!(output.stderr.is_empty());
            let error: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(error["code"], "usage");
        }
        profile["stores"][0]["root"] = serde_json::json!(dir.join("absent-provider"));
        profile["namespaces"][0]["root"] = serde_json::json!(dir.join("absent-source"));
        write_profile(&profile);
        let output = call("status");
        assert!(output.status.success(), "{output:?}");
        let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(status["phase"], expected);
        assert!(
            status["action_status"]
                .as_array()
                .unwrap()
                .iter()
                .all(serde_json::Value::is_null)
        );
        assert!(!status["holds"].as_array().unwrap().is_empty());
        profile["witness"]["root"] = serde_json::json!(dir.join("absent-witness"));
        write_profile(&profile);
        let output = call("status");
        assert!(!output.status.success());
        let error: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(error["code"], "witness_unavailable");
        for name in [
            "absent-metadata",
            "absent-provider",
            "absent-source",
            "absent-witness",
        ] {
            assert!(!dir.join(name).exists());
        }
        assert_eq!(
            before,
            (
                CapacityLedger::open(&dir.join("ledger"), &before.0)
                    .unwrap()
                    .checkpoint(),
                CapacityLedger::open(&dir.join("witness-ledger"), &before.1)
                    .unwrap()
                    .checkpoint()
            )
        );
        assert_eq!(
            sources,
            sources
                .iter()
                .enumerate()
                .map(
                    |(i, minimum)| Namespace::open(&dir.join(format!("ns-{i}")), minimum)
                        .unwrap()
                        .checkpoint()
                )
                .collect::<Vec<_>>()
        );
        assert_eq!(
            provider_before,
            (
                inspection_files(&dir.join("M")),
                inspection_files(&dir.join("witness"))
            )
        );
        for name in ["shared", "keep"] {
            assert!(f.store.read_object(&f.publications[name].object).is_ok());
        }
    }
}
impl Fixture {
    fn new() -> Self {
        Self::with_clock(d("clock"), 1000)
    }
    fn with_clock(clock: Digest, max_window: u64) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut metadata = MetadataStore::create(
            &temp.path().join("metadata"),
            MetadataLimits {
                nodes: 100_000,
                bytes: 64 * 1024 * 1024,
                database_bytes: 128 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let mut namespaces = (0..2)
            .map(|i| {
                Namespace::create(
                    &temp.path().join(format!("ns-{i}")),
                    key("project"),
                    key("epoch"),
                    policy(),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        let objects = ["a", "b", "c", "shared", "keep"]
            .into_iter()
            .map(|s| (s, Object::from_bytes(s.as_bytes())))
            .collect::<BTreeMap<_, _>>();
        let first = root(
            &mut metadata,
            &[
                ("a", objects["a"].clone()),
                ("b", objects["b"].clone()),
                ("shared", objects["shared"].clone()),
                ("keep", objects["keep"].clone()),
            ],
        );
        let second = root(
            &mut metadata,
            &[("a2", objects["a"].clone()), ("c", objects["c"].clone())],
        );
        let shared = root(&mut metadata, &[("input", objects["shared"].clone())]);
        enroll(&mut namespaces[0], &metadata, "main", "owner", Kind::Branch);
        select(
            &mut namespaces[0],
            &metadata,
            "main",
            "owner",
            first,
            "first",
        );
        namespaces[0]
            .apply(
                key("withdraw-first"),
                Command::Withdraw {
                    source: key("main"),
                    owner: key("owner"),
                    expected_revision: 3,
                },
                &metadata,
                &limits(),
            )
            .unwrap();
        select(
            &mut namespaces[0],
            &metadata,
            "main",
            "owner",
            second,
            "second",
        );
        namespaces[0]
            .apply(
                key("withdraw-second"),
                Command::Withdraw {
                    source: key("main"),
                    owner: key("owner"),
                    expected_revision: 6,
                },
                &metadata,
                &limits(),
            )
            .unwrap();
        enroll(&mut namespaces[1], &metadata, "job", "other", Kind::Job);
        select(&mut namespaces[1], &metadata, "job", "other", shared, "job");
        let roster = claims::Roster::flat(
            d("final-authority"),
            d("epoch"),
            Some(d("complete-scope")),
            namespaces
                .iter()
                .map(|n| claims::NamespacePin::current(n).unwrap())
                .collect(),
            limits(),
        )
        .unwrap();
        let store = LocalStore::create_final(
            &temp.path().join("M"),
            key("M"),
            StoreBinding::new(
                1,
                d("provider-boundary"),
                vec![Member {
                    namespace: namespaces[0].identity().clone(),
                    places: vec![key("M")],
                }],
            )
            .unwrap(),
            provider::Policy::new(
                d("final-authority"),
                d("epoch"),
                roster.scope().unwrap(),
                clock.clone(),
                vec![key("owner")],
                max_window,
            )
            .unwrap(),
        )
        .unwrap();
        let mut ledger = CapacityLedger::create(
            &temp.path().join("ledger"),
            key("capacity"),
            key("epoch"),
            0,
        )
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
        let mut publications = BTreeMap::new();
        for (name, object) in objects {
            let publication = Publication {
                operation: key(&format!("ingest-{name}")),
                owner: key("owner"),
                object,
                required: AckLevel::ProcessRestart,
            };
            let plan = accounted::plan(&store, &publication).unwrap();
            accounted::publish_accounted(
                &mut ledger,
                &store,
                &job,
                &plan,
                &mut Cursor::new(name.as_bytes()),
            )
            .unwrap();
            let retained = retention::plan(
                &store,
                &retention::RetentionRequest {
                    operation: key(&format!("pin-{name}")),
                    owner: key("owner"),
                    publication: publication.clone(),
                    minimum_ack: AckLevel::ProcessRestart,
                },
            )
            .unwrap();
            retention::retain(&mut ledger, &store, &job, &retained).unwrap();
            publications.insert(name.into(), publication);
        }
        let witness = LocalStore::create(&temp.path().join("witness"), key("metadata")).unwrap();
        let mut witness_ledger = CapacityLedger::create(
            &temp.path().join("witness-ledger"),
            key("witness-capacity"),
            key("epoch"),
            0,
        )
        .unwrap();
        LocalCapacityProbe::bind(&witness, &mut witness_ledger)
            .unwrap()
            .observe(&witness, &mut witness_ledger, key("probe"), key("sample"))
            .unwrap();
        let witness_job = witness_ledger
            .reserve(key("job"), key("owner"), 64 * 1024 * 1024)
            .unwrap()
            .reference()
            .clone();
        let source = subset::Enrollment {
            authority: d("source-authority"),
            namespace: namespaces[0].identity().clone(),
            policy: digest(namespaces[0].policy()).unwrap(),
            owner: key("owner"),
            classes: vec![Kind::Branch, Kind::Draft],
            limits: limits(),
            writes: 10_000,
            write_bytes: 8 * 1024 * 1024,
        };
        let enrollment = Enrollment {
            authority: d("batch-authority"),
            epoch: d("epoch"),
            owner: key("owner"),
            context: d("batch-context"),
            scope: roster.scope().unwrap(),
            clock: clock.clone(),
            witness: Place::local(&witness, 1).unwrap(),
            capacity: witness_ledger.identity().clone(),
            max_window,
            max_metadata_bytes: 16 * 1024 * 1024,
            sources: vec![source],
        };
        Self {
            temp,
            namespaces,
            metadata,
            roster,
            store,
            ledger,
            job,
            witness,
            witness_ledger,
            witness_job,
            enrollment,
            clock: Clock(Cell::new(100), clock),
            publications,
        }
    }
    fn request(&self) -> Request {
        let namespace = self.namespaces[0].identity().clone();
        let selections = [
            (3, vec![path("a"), path("b"), path("shared")]),
            (6, vec![path("a2"), path("c")]),
        ]
        .into_iter()
        .map(|(predecessor_revision, paths)| Selection {
            namespace: namespace.clone(),
            request: subset::Request {
                source: key("main"),
                predecessor_revision,
                paths,
            },
        })
        .collect();
        let mut copies = ["a", "b", "c", "shared"]
            .into_iter()
            .map(|name| Copy {
                store: self.store.identity().clone(),
                capacity: self.ledger.identity().clone(),
                publication: self.publications[name].clone(),
            })
            .collect::<Vec<_>>();
        copies.sort_by(|a, b| copy_key(a).cmp(&copy_key(b)));
        Request {
            operation: key("batch"),
            selections,
            copies,
            window: provider::Window {
                not_before: 50,
                expires: 500,
            },
        }
    }
    fn plan(&mut self) -> Plan {
        let request = self.request();
        plan(
            &self.enrollment,
            request,
            &self.roster,
            &mut self.namespaces,
            &self.metadata.finished_reader().unwrap(),
            &[&self.store],
        )
        .unwrap()
    }
    fn run<T>(&mut self, p: &Plan, f: impl FnOnce(&mut Execution<'_, '_>) -> T) -> T {
        let mut places = [Local {
            store: &self.store,
            ledger: &mut self.ledger,
            job: &self.job,
        }];
        let jobs = p
            .actions()
            .map(|(store, _)| {
                assert_eq!(store, self.store.identity());
                self.job.clone()
            })
            .collect::<Vec<_>>();
        f(&mut Execution {
            enrollment: &self.enrollment,
            roster: &self.roster,
            namespaces: &mut self.namespaces,
            metadata: &mut self.metadata,
            places: &mut places,
            action_jobs: &jobs,
            witness: Local {
                store: &self.witness,
                ledger: &mut self.witness_ledger,
                job: &self.witness_job,
            },
            clock: &self.clock,
            admitted_context: &self.enrollment.context,
        })
    }
}

mod operator;
mod paged;

#[test]
fn mixed_block_settles_several_predecessors_and_deletes_only_eligible_exact_bytes() {
    let mut f = Fixture::new();
    let p = f.plan();
    assert_eq!(p.actions.len(), 3);
    assert_eq!(p.settlements.len(), 2);
    assert_eq!(
        p.subjects
            .iter()
            .filter(|s| s.hold == Some(Hold::Protected))
            .count(),
        1
    );
    let p = Plan::from_json(&p.to_json().unwrap()).unwrap();
    let before = (f.ledger.checkpoint(), f.witness_ledger.checkpoint());
    let status = f.run(&p, |e| e.status(&p)).unwrap();
    assert_eq!(status.phase, Phase::NotStarted);
    assert!(!status.removal_authority);
    assert_eq!(
        (f.ledger.checkpoint(), f.witness_ledger.checkpoint()),
        before
    );
    let report = f.run(&p, |e| e.apply(&p, &p.authorize().unwrap())).unwrap();
    assert_eq!(report.phase, Phase::Completed, "{:?}", report.holds);
    assert!(report.holds.is_empty());
    for name in ["a", "b", "c"] {
        assert!(
            !f.store
                .object_present(&f.publications[name].object)
                .unwrap()
        );
    }
    for name in ["shared", "keep"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
    assert!(
        report
            .source_status
            .iter()
            .all(|s| s.as_ref().unwrap().settlement_current)
    );
    f.clock.0.set(700);
    let before = (f.ledger.checkpoint(), f.witness_ledger.checkpoint());
    let report = f.run(&p, |e| e.apply(&p, &p.authorize().unwrap())).unwrap();
    assert_eq!(report.phase, Phase::Completed);
    assert_eq!(
        (f.ledger.checkpoint(), f.witness_ledger.checkpoint()),
        before
    );
}

#[test]
fn independent_vectors_pin_plan_intent_and_terminal_framing() {
    let vector: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/vectors/bulk-final.design-1.json"
    )))
    .unwrap();
    for case in vector["cases"].as_array().unwrap() {
        let raw = case["canonical"].as_str().unwrap().as_bytes();
        assert_eq!(Digest::blob(raw).as_str(), case["sha256"].as_str().unwrap());
        let value: serde_json::Value = serde_json::from_slice(raw).unwrap();
        match case["kind"].as_str().unwrap() {
            "enrollment" => {
                let e: Enrollment = serde_json::from_value(value).unwrap();
                assert_eq!(e.id().unwrap().as_str(), case["id"].as_str().unwrap());
                assert_eq!(encode(&e).unwrap(), raw);
            }
            "plan" | "eligible_plan" => {
                assert_eq!(Plan::from_json(raw).unwrap().to_json().unwrap(), raw)
            }
            "intent" => {
                let p: Plan = serde_json::from_value(value["plan"].clone()).unwrap();
                p.id().unwrap();
                let b: witness::Bindings =
                    serde_json::from_value(value["bindings"].clone()).unwrap();
                assert_eq!(
                    encode(&witness::Intent {
                        schema: "tig.bulk-final-intent.design-1",
                        plan: &p,
                        bindings: &b
                    })
                    .unwrap(),
                    raw
                );
            }
            _ => {
                let outcome: witness::Outcome =
                    serde_json::from_value(value["outcome"].clone()).unwrap();
                assert_eq!(
                    encode(&witness::Terminal {
                        schema: "tig.bulk-final-terminal.design-1",
                        plan: serde_json::from_value(value["plan"].clone()).unwrap(),
                        bindings: serde_json::from_value(value["bindings"].clone()).unwrap(),
                        outcome
                    })
                    .unwrap(),
                    raw
                );
            }
        }
    }
}

fn stops() -> Vec<Stage> {
    vec![
        Stage::Admitted,
        Stage::Intent,
        Stage::Source(0, subset::Stage::MetadataFinished),
        Stage::Source(0, subset::Stage::Enrolled),
        Stage::Source(0, subset::Stage::SurvivorProtected),
        Stage::Source(0, subset::Stage::BeforeRetire),
        Stage::Source(0, subset::Stage::Retired),
        Stage::Settled(1),
        Stage::Removed(0),
        Stage::Completed,
    ]
}
#[test]
fn every_batch_boundary_resumes_without_repeating_loss_or_dropping_held_siblings() {
    for stop in stops() {
        let mut f = Fixture::new();
        let p = f.plan();
        assert!(
            matches!(
                f.run(&p, |e| e.apply_hook(&p, &p.authorize().unwrap(), |stage| {
                    if stage == stop {
                        Err(Error::Injected)
                    } else {
                        Ok(())
                    }
                })),
                Err(Error::Injected)
            ),
            "{stop:?}"
        );
        let report = f.run(&p, |e| e.apply(&p, &p.authorize().unwrap())).unwrap();
        assert_eq!(
            report.phase,
            Phase::Completed,
            "{stop:?}: {:?}",
            report.holds
        );
        assert!(report.holds.is_empty());
        assert_eq!(report.historical_deleted_copy_bytes, 3);
        for name in ["shared", "keep"] {
            assert!(f.store.read_object(&f.publications[name].object).is_ok());
        }
    }
}
#[test]
fn cancellation_never_resumes_source_settlement_and_reports_irreversible_effects() {
    for stop in stops() {
        let mut f = Fixture::new();
        let p = f.plan();
        assert!(
            f.run(&p, |e| e.apply_hook(&p, &p.authorize().unwrap(), |stage| {
                if stage == stop {
                    Err(Error::Injected)
                } else {
                    Ok(())
                }
            }))
            .is_err()
        );
        f.clock.0.set(700);
        let report = f
            .run(&p, |e| e.cancel(&p, &p.authorize().unwrap()))
            .unwrap();
        assert_eq!(
            report.phase,
            if stop == Stage::Completed {
                Phase::Completed
            } else {
                Phase::Cancelled
            }
        );
        assert!(report.holds.is_empty(), "{stop:?}: {:?}", report.holds);
        let missing = ["a", "b", "c"]
            .into_iter()
            .filter(|name| {
                !f.store
                    .object_present(&f.publications[*name].object)
                    .unwrap()
            })
            .count();
        assert_eq!(
            missing,
            if stop == Stage::Completed {
                3
            } else if stop == Stage::Removed(0) {
                1
            } else {
                0
            }
        );
        let before = (
            f.ledger.checkpoint(),
            f.witness_ledger.checkpoint(),
            f.namespaces
                .iter()
                .map(Namespace::checkpoint)
                .collect::<Vec<_>>(),
        );
        let repeated = f.run(&p, |e| e.apply(&p, &p.authorize().unwrap())).unwrap();
        assert_eq!(repeated.phase, report.phase);
        assert_eq!(
            (
                f.ledger.checkpoint(),
                f.witness_ledger.checkpoint(),
                f.namespaces
                    .iter()
                    .map(Namespace::checkpoint)
                    .collect::<Vec<_>>()
            ),
            before
        );
    }
}
fn transient_use(f: &mut Fixture) {
    let selected = root(
        &mut f.metadata,
        &[("input", f.publications["a"].object.clone())],
    );
    enroll(
        &mut f.namespaces[1],
        &f.metadata,
        "transient",
        "other",
        Kind::Job,
    );
    let prepared = f.namespaces[1]
        .apply(
            key("transient-prepare"),
            Command::Prepare {
                source: key("transient"),
                owner: key("other"),
                expected_revision: 1,
                selection: selected,
            },
            &f.metadata,
            &limits(),
        )
        .unwrap();
    f.namespaces[1]
        .apply(
            key("transient-abort"),
            Command::Abort {
                source: key("transient"),
                owner: key("other"),
                expected_revision: 2,
                prepared: prepared.reference,
            },
            &f.metadata,
            &limits(),
        )
        .unwrap();
}
#[test]
fn transient_use_invalidates_approval_before_intent_and_under_final_live_clearance() {
    let mut f = Fixture::new();
    let p = f.plan();
    transient_use(&mut f);
    let before = (f.ledger.checkpoint(), f.witness_ledger.checkpoint());
    assert!(matches!(
        f.run(&p, |e| e.apply(&p, &p.authorize().unwrap())),
        Err(Error::Stale)
    ));
    assert_eq!(
        (f.ledger.checkpoint(), f.witness_ledger.checkpoint()),
        before
    );
    let mut f = Fixture::new();
    let p = f.plan();
    assert!(
        f.run(&p, |e| e.apply_hook(&p, &p.authorize().unwrap(), |stage| {
            if stage == Stage::Settled(1) {
                Err(Error::Injected)
            } else {
                Ok(())
            }
        }))
        .is_err()
    );
    transient_use(&mut f);
    let report = f.run(&p, |e| e.apply(&p, &p.authorize().unwrap())).unwrap();
    assert_eq!(report.phase, Phase::Intent);
    assert_eq!(report.effects.len(), 0);
    assert!(report.holds.iter().any(|h| h.code == "stale_consent"));
    for action in &p.actions {
        assert!(
            f.store
                .read_object(&action.request().publication.object)
                .is_ok()
        );
        for grant in action.grants() {
            retention::verify(&mut f.ledger, &f.store, grant).unwrap();
        }
    }
    assert_eq!(
        f.run(&p, |e| e.cancel(&p, &p.authorize().unwrap()))
            .unwrap()
            .phase,
        Phase::Cancelled
    );
}
#[test]
fn missing_acknowledged_intent_is_not_repaired_and_expired_consent_cannot_start() {
    let mut f = Fixture::new();
    let p = f.plan();
    f.clock.0.set(500);
    assert!(f.run(&p, |e| e.apply(&p, &p.authorize().unwrap())).is_err());
    assert_eq!(
        f.run(&p, |e| e.status(&p)).unwrap().phase,
        Phase::NotStarted
    );
    f.clock.0.set(100);
    assert!(
        f.run(&p, |e| e.apply_hook(&p, &p.authorize().unwrap(), |stage| {
            if stage == Stage::Intent {
                Err(Error::Injected)
            } else {
                Ok(())
            }
        }))
        .is_err()
    );
    let bindings = witness::Bindings {
        metadata: f.metadata.checkpoint().identity().clone(),
        witness_job: f.witness_job.clone(),
        action_jobs: vec![f.job.clone(); p.actions.len()],
    };
    let raw = encode(&witness::Intent {
        schema: "tig.bulk-final-intent.design-1",
        plan: &p,
        bindings: &bindings,
    })
    .unwrap();
    let intent_path = f
        .temp
        .path()
        .join("witness/objects")
        .join(Digest::blob(&raw).as_str());
    std::fs::remove_file(&intent_path).unwrap();
    let before = (f.ledger.checkpoint(), f.witness_ledger.checkpoint());
    assert!(f.run(&p, |e| e.apply(&p, &p.authorize().unwrap())).is_err());
    assert_eq!(
        (f.ledger.checkpoint(), f.witness_ledger.checkpoint()),
        before
    );
    assert!(!intent_path.exists());
    assert_eq!(
        f.namespaces[0]
            .source(&key("main"))
            .unwrap()
            .unwrap()
            .predecessors
            .len(),
        2
    );
}

#[derive(Serialize, Deserialize)]
struct Saved {
    plan: Plan,
    roster: claims::Roster,
    enrollment: Enrollment,
    namespaces: Vec<source_claims::Checkpoint>,
    metadata: crate::metadata::Checkpoint,
    ledger: crate::ledger::Checkpoint,
    witness: crate::ledger::Checkpoint,
    job: JournalRef,
    witness_job: JournalRef,
}
#[test]
fn batch_child() {
    let Some(dir) = std::env::var_os("GIT_ASSET_BULK_CHILD") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let saved: Saved =
        serde_json::from_slice(&std::fs::read(dir.join("saved.json")).unwrap()).unwrap();
    let mut namespaces = saved
        .namespaces
        .iter()
        .enumerate()
        .map(|(i, c)| Namespace::open(&dir.join(format!("ns-{i}")), c).unwrap())
        .collect::<Vec<_>>();
    let mut metadata = MetadataStore::open(&dir.join("metadata"), &saved.metadata).unwrap();
    let store = LocalStore::open(&dir.join("M")).unwrap();
    let mut ledger = CapacityLedger::open(&dir.join("ledger"), &saved.ledger).unwrap();
    let witness = LocalStore::open(&dir.join("witness")).unwrap();
    let mut witness_ledger =
        CapacityLedger::open(&dir.join("witness-ledger"), &saved.witness).unwrap();
    let jobs = vec![saved.job.clone(); saved.plan.actions.len()];
    let mut places = [Local {
        store: &store,
        ledger: &mut ledger,
        job: &saved.job,
    }];
    let clock = Clock(Cell::new(100), d("clock"));
    let stop = std::env::var("GIT_ASSET_BULK_STOP").unwrap();
    let mut execution = Execution {
        enrollment: &saved.enrollment,
        roster: &saved.roster,
        namespaces: &mut namespaces,
        metadata: &mut metadata,
        places: &mut places,
        action_jobs: &jobs,
        witness: Local {
            store: &witness,
            ledger: &mut witness_ledger,
            job: &saved.witness_job,
        },
        clock: &clock,
        admitted_context: &saved.enrollment.context,
    };
    if std::env::var_os("GIT_ASSET_BULK_BUSY").is_some() {
        assert!(matches!(
            execution.apply(&saved.plan, &saved.plan.authorize().unwrap()),
            Err(Error::Store(crate::Error::Busy))
        ));
        std::process::exit(78);
    }
    execution
        .apply_hook(&saved.plan, &saved.plan.authorize().unwrap(), |stage| {
            if format!("{stage:?}") == stop {
                std::process::exit(77);
            }
            Ok(())
        })
        .unwrap();
    panic!("exit boundary not reached");
}
#[test]
fn actual_child_exits_recover_intent_subsets_and_partial_final_loss() {
    for stop in stops() {
        let mut f = Fixture::new();
        let p = f.plan();
        let saved = Saved {
            plan: p.clone(),
            roster: f.roster.clone(),
            enrollment: f.enrollment.clone(),
            namespaces: f.namespaces.iter().map(Namespace::checkpoint).collect(),
            metadata: f.metadata.checkpoint(),
            ledger: f.ledger.checkpoint(),
            witness: f.witness_ledger.checkpoint(),
            job: f.job.clone(),
            witness_job: f.witness_job.clone(),
        };
        std::fs::write(
            f.temp.path().join("saved.json"),
            serde_json::to_vec(&saved).unwrap(),
        )
        .unwrap();
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "final_retirement::bulk::tests::batch_child",
                "--nocapture",
            ])
            .env("GIT_ASSET_BULK_CHILD", f.temp.path())
            .env("GIT_ASSET_BULK_STOP", format!("{stop:?}"))
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(77),
            "{stop:?}: {} {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        f.namespaces = saved
            .namespaces
            .iter()
            .enumerate()
            .map(|(i, c)| Namespace::open(&f.temp.path().join(format!("ns-{i}")), c).unwrap())
            .collect();
        f.metadata = MetadataStore::open(&f.temp.path().join("metadata"), &saved.metadata).unwrap();
        f.ledger = CapacityLedger::open(&f.temp.path().join("ledger"), &saved.ledger).unwrap();
        f.witness_ledger =
            CapacityLedger::open(&f.temp.path().join("witness-ledger"), &saved.witness).unwrap();
        let report = f.run(&p, |e| e.apply(&p, &p.authorize().unwrap())).unwrap();
        assert_eq!(
            report.phase,
            Phase::Completed,
            "{stop:?}: {:?}",
            report.holds
        );
        assert_eq!(report.historical_deleted_copy_bytes, 3);
        for name in ["shared", "keep"] {
            assert!(f.store.read_object(&f.publications[name].object).is_ok());
        }
    }
}
#[test]
fn insufficient_budget_wrong_authority_and_stale_or_unknown_selection_cannot_settle_history() {
    let mut f = Fixture::new();
    let p = f.plan();
    f.enrollment.context = d("changed");
    assert!(matches!(
        f.run(&p, |e| e.apply(&p, &p.authorize().unwrap())),
        Err(Error::Authority)
    ));
    let mut f = Fixture::new();
    let p = f.plan();
    LocalCapacityProbe::bind(&f.witness, &mut f.witness_ledger)
        .unwrap()
        .observe(
            &f.witness,
            &mut f.witness_ledger,
            key("small-probe"),
            key("small-sample"),
        )
        .unwrap();
    f.witness_job = f
        .witness_ledger
        .reserve(key("small-job"), key("owner"), 1)
        .unwrap()
        .reference()
        .clone();
    assert!(f.run(&p, |e| e.apply(&p, &p.authorize().unwrap())).is_err());
    assert_eq!(
        f.namespaces[0]
            .source(&key("main"))
            .unwrap()
            .unwrap()
            .predecessors
            .len(),
        2
    );
    for name in ["a", "b", "c"] {
        assert!(f.store.read_object(&f.publications[name].object).is_ok());
    }
    let mut f = Fixture::new();
    let mut request = f.request();
    request.selections[0].request.paths.push(path("zz-missing"));
    assert!(
        plan(
            &f.enrollment,
            request,
            &f.roster,
            &mut f.namespaces,
            &f.metadata.finished_reader().unwrap(),
            &[&f.store]
        )
        .is_err()
    );
    let mut request = f.request();
    request.selections[0].request.predecessor_revision = 1;
    assert!(
        plan(
            &f.enrollment,
            request,
            &f.roster,
            &mut f.namespaces,
            &f.metadata.finished_reader().unwrap(),
            &[&f.store]
        )
        .is_err()
    );
    let request = f.request();
    let mut roster = f.roster.clone();
    roster.coverage = None;
    assert!(
        plan(
            &f.enrollment,
            request,
            &roster,
            &mut f.namespaces,
            &f.metadata.finished_reader().unwrap(),
            &[&f.store]
        )
        .is_err()
    );
}
#[test]
fn completed_batch_replay_spares_a_new_same_hash_birth_and_new_retention() {
    let mut f = Fixture::new();
    let p = f.plan();
    assert_eq!(
        f.run(&p, |e| e.apply(&p, &p.authorize().unwrap()))
            .unwrap()
            .phase,
        Phase::Completed
    );
    let action = &p.actions[0];
    let publication = Publication {
        operation: key("fresh-same-bytes"),
        ..action.request().publication.clone()
    };
    let rp = provider::reacquire::plan(
        &f.store,
        action,
        provider::reacquire::Request {
            publication: publication.clone(),
            capacity: f.ledger.identity().clone(),
            context: d("fresh-context"),
            window: provider::Window {
                not_before: 50,
                expires: 500,
            },
        },
    )
    .unwrap();
    let name = f
        .publications
        .iter()
        .find(|(_, p)| p.object == publication.object)
        .unwrap()
        .0
        .clone();
    provider::reacquire::Execution {
        store: &f.store,
        ledger: &mut f.ledger,
        job: &f.job,
        clock: &f.clock,
        admitted_context: &d("fresh-context"),
    }
    .apply(
        &rp,
        &rp.authorize().unwrap(),
        &mut Cursor::new(name.as_bytes()),
    )
    .unwrap();
    let retained = retention::plan(
        &f.store,
        &retention::RetentionRequest {
            operation: key("fresh-pin"),
            owner: key("owner"),
            publication: publication.clone(),
            minimum_ack: AckLevel::ProcessRestart,
        },
    )
    .unwrap();
    let grant = retention::retain(&mut f.ledger, &f.store, &f.job, &retained)
        .unwrap()
        .grant;
    let before = (f.ledger.checkpoint(), f.witness_ledger.checkpoint());
    f.clock.0.set(700);
    assert_eq!(
        f.run(&p, |e| e.apply(&p, &p.authorize().unwrap()))
            .unwrap()
            .phase,
        Phase::Completed
    );
    assert_eq!(
        (f.ledger.checkpoint(), f.witness_ledger.checkpoint()),
        before
    );
    assert!(f.store.read_object(&publication.object).is_ok());
    retention::verify(&mut f.ledger, &f.store, &grant).unwrap();
}

#[test]
fn terminal_witnesses_dispose_unused_reservations_and_can_finish_the_metadata_job() {
    for cancel in [false, true] {
        let mut f = Fixture::new();
        let p = f.plan();
        let report = f
            .run(&p, |e| {
                if cancel {
                    e.cancel(&p, &p.authorize().unwrap())
                } else {
                    e.apply(&p, &p.authorize().unwrap())
                }
            })
            .unwrap();
        assert_eq!(
            report.phase,
            if cancel {
                Phase::Cancelled
            } else {
                Phase::Completed
            }
        );
        f.witness_ledger
            .finish(key("finish-metadata"), &f.witness_job)
            .unwrap();
        let before = f.witness_ledger.checkpoint();
        let report = f.run(&p, |e| e.apply(&p, &p.authorize().unwrap())).unwrap();
        assert_eq!(
            report.phase,
            if cancel {
                Phase::Cancelled
            } else {
                Phase::Completed
            }
        );
        assert_eq!(f.witness_ledger.checkpoint(), before);
    }
}

#[test]
fn held_members_do_not_expand_and_missing_copy_scope_cannot_retire_history() {
    let mut f = Fixture::new();
    let mut request = f.request();
    request.copies.clear();
    let p = plan(
        &f.enrollment,
        request,
        &f.roster,
        &mut f.namespaces,
        &f.metadata.finished_reader().unwrap(),
        &[],
    )
    .unwrap();
    assert!(p.actions.is_empty());
    assert!(p.settlements.is_empty());
    assert!(p.subjects.iter().all(|s| s.hold.is_some()));
    let report = f.run(&p, |e| e.apply(&p, &p.authorize().unwrap())).unwrap();
    assert_eq!(report.phase, Phase::Completed);
    assert_eq!(report.new_logical_copy_bytes_removed, 0);
    assert_eq!(
        f.namespaces[0]
            .source(&key("main"))
            .unwrap()
            .unwrap()
            .predecessors
            .len(),
        2
    );

    let mut f = Fixture::new();
    let old = f.plan();
    f.run(&old, |e| e.apply(&old, &old.authorize().unwrap()))
        .unwrap();
    f.namespaces[1]
        .apply(
            key("end-job"),
            Command::Withdraw {
                source: key("job"),
                owner: key("other"),
                expected_revision: 3,
            },
            &f.metadata,
            &limits(),
        )
        .unwrap();
    f.namespaces[1]
        .apply(
            key("retire-job"),
            Command::Retire {
                source: key("job"),
                owner: key("other"),
                expected_revision: 4,
                predecessor_revision: 3,
            },
            &f.metadata,
            &limits(),
        )
        .unwrap();
    f.run(&old, |e| e.apply(&old, &old.authorize().unwrap()))
        .unwrap();
    assert!(
        f.store
            .read_object(&f.publications["shared"].object)
            .is_ok()
    );
    // A fresh decision selects the held member from its protected survivor root.
    let survivor = old.settlements[0].plan.survivor_source().unwrap().unwrap();
    let request = Request {
        operation: key("new-decision"),
        window: old.request.window.clone(),
        selections: vec![Selection {
            namespace: f.namespaces[0].identity().clone(),
            request: subset::Request {
                source: survivor,
                predecessor_revision: 3,
                paths: vec![path("shared")],
            },
        }],
        copies: vec![Copy {
            store: f.store.identity().clone(),
            capacity: f.ledger.identity().clone(),
            publication: f.publications["shared"].clone(),
        }],
    };
    let next = plan(
        &f.enrollment,
        request,
        &f.roster,
        &mut f.namespaces,
        &f.metadata.finished_reader().unwrap(),
        &[&f.store],
    )
    .unwrap();
    assert_eq!(next.actions.len(), 1);
    assert!(matches!(
        f.run(&next, |e| e.apply(&next, &old.authorize().unwrap())),
        Err(Error::Authority)
    ));
    assert_eq!(
        f.run(&next, |e| e.apply(&next, &next.authorize().unwrap()))
            .unwrap()
            .phase,
        Phase::Completed
    );
    assert!(
        !f.store
            .object_present(&f.publications["shared"].object)
            .unwrap()
    );
    assert!(f.store.read_object(&f.publications["keep"].object).is_ok());
}

#[test]
fn exact_batch_fence_blocks_a_process_peer_but_another_batch_uses_the_same_witness() {
    let mut f = Fixture::new();
    let p = f.plan();
    let mut request = f.request();
    request.operation = key("other-batch");
    request.copies.clear();
    let other = plan(
        &f.enrollment,
        request,
        &f.roster,
        &mut f.namespaces,
        &f.metadata.finished_reader().unwrap(),
        &[],
    )
    .unwrap();
    let saved = Saved {
        plan: p.clone(),
        roster: f.roster.clone(),
        enrollment: f.enrollment.clone(),
        namespaces: f.namespaces.iter().map(Namespace::checkpoint).collect(),
        metadata: f.metadata.checkpoint(),
        ledger: f.ledger.checkpoint(),
        witness: f.witness_ledger.checkpoint(),
        job: f.job.clone(),
        witness_job: f.witness_job.clone(),
    };
    let dir = f.temp.path().to_path_buf();
    std::fs::write(dir.join("saved.json"), serde_json::to_vec(&saved).unwrap()).unwrap();
    let report = f
        .run(&p, |e| {
            e.apply_hook(&p, &p.authorize().unwrap(), |stage| {
                if stage != Stage::Intent {
                    return Ok(());
                }
                let out = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "final_retirement::bulk::tests::batch_child",
                        "--nocapture",
                    ])
                    .env("GIT_ASSET_BULK_CHILD", &dir)
                    .env("GIT_ASSET_BULK_STOP", "none")
                    .env("GIT_ASSET_BULK_BUSY", "1")
                    .output()
                    .unwrap();
                assert_eq!(
                    out.status.code(),
                    Some(78),
                    "{} {}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                let mut namespaces = saved
                    .namespaces
                    .iter()
                    .enumerate()
                    .map(|(i, c)| Namespace::open(&dir.join(format!("ns-{i}")), c).unwrap())
                    .collect::<Vec<_>>();
                let mut metadata =
                    MetadataStore::open(&dir.join("metadata"), &saved.metadata).unwrap();
                let witness = LocalStore::open(&dir.join("witness")).unwrap();
                let mut ledger =
                    CapacityLedger::open(&dir.join("witness-ledger"), &saved.witness).unwrap();
                let mut places = [];
                let clock = Clock(Cell::new(100), d("clock"));
                let report = Execution {
                    enrollment: &saved.enrollment,
                    roster: &saved.roster,
                    namespaces: &mut namespaces,
                    metadata: &mut metadata,
                    places: &mut places,
                    action_jobs: &[],
                    witness: Local {
                        store: &witness,
                        ledger: &mut ledger,
                        job: &saved.witness_job,
                    },
                    clock: &clock,
                    admitted_context: &saved.enrollment.context,
                }
                .apply(&other, &other.authorize().unwrap())
                .unwrap();
                assert_eq!(report.phase, Phase::Completed);
                assert_eq!(report.new_logical_copy_bytes_removed, 0);
                Ok(())
            })
        })
        .unwrap();
    assert_eq!(report.phase, Phase::Completed);
    assert_eq!(report.new_logical_copy_bytes_removed, 3);
}

#[test]
fn cancellation_does_not_admit_a_stale_action_that_never_started() {
    let mut f = Fixture::new();
    let p = f.plan();
    for s in &p.settlements {
        subset::apply(
            p.enrollment.source(&s.namespace).unwrap(),
            &mut f.namespaces[0],
            &mut f.metadata,
            &s.plan,
        )
        .unwrap();
    }
    let first = &p.actions[0];
    let other = provider::plan(
        &f.store,
        provider::Request {
            operation: key("other-final"),
            owner: key("owner"),
            publication: first.request().publication.clone(),
            capacity: f.ledger.identity().clone(),
            context: d("other-context"),
            window: p.request.window.clone(),
        },
    )
    .unwrap();
    provider::Execution {
        store: &f.store,
        ledger: &mut f.ledger,
        job: &f.job,
        roster: &f.roster,
        namespaces: &mut f.namespaces,
        nodes: &f.metadata.finished_reader().unwrap(),
        clock: &f.clock,
        admitted_context: &d("other-context"),
    }
    .apply(&other, &other.authorize().unwrap())
    .unwrap();
    let before = f.ledger.checkpoint();
    let report = f
        .run(&p, |e| e.cancel(&p, &p.authorize().unwrap()))
        .unwrap();
    assert_eq!(report.phase, Phase::Cancelled);
    assert_eq!(f.ledger.checkpoint(), before);
    assert_eq!(report.new_logical_copy_bytes_removed, 0);
}

#[test]
fn interrupted_unused_terminal_disposition_resumes_the_same_empty_review() {
    let mut f = Fixture::new();
    let p = f.plan();
    assert!(
        f.run(&p, |e| e.cancel_hook(
            &p,
            &p.authorize().unwrap(),
            |stage| {
                if stage == Stage::Cancelled {
                    Err(Error::Injected)
                } else {
                    Ok(())
                }
            }
        ))
        .is_err()
    );
    let bindings = witness::Bindings {
        metadata: f.metadata.checkpoint().identity().clone(),
        witness_job: f.witness_job.clone(),
        action_jobs: vec![f.job.clone(); p.actions.len()],
    };
    let body = encode(&witness::Terminal {
        schema: "tig.bulk-final-terminal.design-1",
        plan: p.id().unwrap(),
        bindings: digest(&bindings).unwrap(),
        outcome: witness::Outcome::Completed,
    })
    .unwrap();
    let id = digest(&(
        "tig.bulk-final-key.design-1",
        &p.enrollment.authority,
        &p.enrollment.epoch,
        &p.enrollment.owner,
        &p.request.operation,
    ))
    .unwrap();
    let request = Publication {
        operation: key(&format!("bulk-2-{}", id.as_str())),
        owner: key("owner"),
        object: Object::from_bytes(&body),
        required: AckLevel::ProcessRestart,
    };
    let pp = accounted::plan_bound(&f.witness, &request, p.id().unwrap()).unwrap();
    let a = accounted::admit(&mut f.witness_ledger, &f.witness, &f.witness_job, &pp).unwrap();
    let cleanup = accounted::cleanup::CleanupPlan::from_json(
        &encode(&serde_json::json!({
        "schema":"tig.private-cleanup.design-1", "publication":pp,
        "job":f.witness_job, "allocation":a, "files":[] }))
        .unwrap(),
    )
    .unwrap();
    f.witness_ledger
        .begin_cleanup(
            &a,
            digest(&(&pp, &f.witness_job)).unwrap(),
            Digest::blob(&cleanup.to_json().unwrap()),
        )
        .unwrap();
    assert_eq!(
        f.run(&p, |e| e.apply(&p, &p.authorize().unwrap()))
            .unwrap()
            .phase,
        Phase::Cancelled
    );
    f.witness_ledger
        .finish(key("finish-witness"), &f.witness_job)
        .unwrap();
}

#[test]
fn several_stores_bind_private_jobs_and_report_copy_bytes_separately_from_unique_bytes() {
    let mut f = Fixture::new();
    let mut stores = Vec::new();
    let mut ledgers = Vec::new();
    let mut jobs = Vec::new();
    for place in ["F", "D"] {
        let store = LocalStore::create_final(
            &f.temp.path().join(format!("store-{place}")),
            key(place),
            StoreBinding::new(
                1,
                d(&format!("boundary-{place}")),
                vec![Member {
                    namespace: f.namespaces[0].identity().clone(),
                    places: vec![key(place)],
                }],
            )
            .unwrap(),
            provider::Policy::new(
                d("final-authority"),
                d("epoch"),
                f.roster.scope().unwrap(),
                d("clock"),
                vec![key("owner")],
                1000,
            )
            .unwrap(),
        )
        .unwrap();
        let mut ledger = CapacityLedger::create(
            &f.temp.path().join(format!("ledger-{place}")),
            key(&format!("capacity-{place}")),
            key("epoch"),
            0,
        )
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
        for (name, publication) in &f.publications {
            let planned = accounted::plan(&store, publication).unwrap();
            accounted::publish_accounted(
                &mut ledger,
                &store,
                &job,
                &planned,
                &mut Cursor::new(name.as_bytes()),
            )
            .unwrap();
            let retained = retention::plan(
                &store,
                &retention::RetentionRequest {
                    operation: key(&format!("pin-{name}")),
                    owner: key("owner"),
                    publication: publication.clone(),
                    minimum_ack: AckLevel::ProcessRestart,
                },
            )
            .unwrap();
            retention::retain(&mut ledger, &store, &job, &retained).unwrap();
        }
        stores.push(store);
        ledgers.push(ledger);
        jobs.push(job);
    }
    let mut request = f.request();
    for (store, ledger) in stores.iter().zip(&ledgers) {
        for name in ["a", "b", "c", "shared"] {
            request.copies.push(Copy {
                store: store.identity().clone(),
                capacity: ledger.identity().clone(),
                publication: f.publications[name].clone(),
            });
        }
    }
    request.copies.sort_by(|a, b| copy_key(a).cmp(&copy_key(b)));
    let all = [&f.store, &stores[0], &stores[1]];
    let p = plan(
        &f.enrollment,
        request,
        &f.roster,
        &mut f.namespaces,
        &f.metadata.finished_reader().unwrap(),
        &all,
    )
    .unwrap();
    let action_jobs = p
        .actions()
        .map(|(store, _)| {
            if store == f.store.identity() {
                f.job.clone()
            } else {
                jobs[stores.iter().position(|s| s.identity() == store).unwrap()].clone()
            }
        })
        .collect::<Vec<_>>();
    let mut places = vec![Local {
        store: &f.store,
        ledger: &mut f.ledger,
        job: &f.job,
    }];
    for ((store, ledger), job) in stores.iter().zip(ledgers.iter_mut()).zip(&jobs) {
        places.push(Local { store, ledger, job });
    }
    let report = Execution {
        enrollment: &f.enrollment,
        roster: &f.roster,
        namespaces: &mut f.namespaces,
        metadata: &mut f.metadata,
        places: &mut places,
        action_jobs: &action_jobs,
        witness: Local {
            store: &f.witness,
            ledger: &mut f.witness_ledger,
            job: &f.witness_job,
        },
        clock: &f.clock,
        admitted_context: &f.enrollment.context,
    }
    .apply(&p, &p.authorize().unwrap())
    .unwrap();
    assert_eq!(report.phase, Phase::Completed, "{:?}", report.holds);
    assert_eq!(report.eligible_unique_bytes, 3);
    assert_eq!(report.planned_copy_bytes, 9);
    assert_eq!(report.new_logical_copy_bytes_removed, 9);
    assert_eq!(report.historical_deleted_copy_bytes, 9);
    for store in all {
        for name in ["a", "b", "c"] {
            assert!(!store.object_present(&f.publications[name].object).unwrap());
        }
        for name in ["shared", "keep"] {
            assert!(store.read_object(&f.publications[name].object).is_ok());
        }
    }
}
