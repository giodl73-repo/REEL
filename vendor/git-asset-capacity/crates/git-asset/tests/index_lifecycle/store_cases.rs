use super::*;
use git_asset_store::{
    ledger::CapacityLedger,
    local::LocalStore,
    local_capacity::LocalCapacityProbe,
    orchestration::Place,
    placement::{Authority, Config, Mapping, Provider, Role},
    source_claims::{Selection, admission::Select},
    store_protection::{Binding, Member},
};
use serde_json::{Value, json};
use std::io::Read;
pub(super) struct Storage {
    pub(super) path: PathBuf,
    pub(super) stores: Vec<LocalStore>,
}
impl Storage {
    pub(super) fn new(f: &Fixture) -> Self {
        let mut stores = Vec::new();
        let mut providers = Vec::new();
        let mut roots = Vec::new();
        for (name, role) in [
            ("working", Role::Cache),
            ("history", Role::Cache),
            ("backup", Role::Retained),
        ] {
            let path = f.temp.path().join(name);
            let store = if role == Role::Cache {
                LocalStore::create_managed(
                    &path,
                    key(name),
                    Binding::new(
                        1,
                        Digest::blob(b"fixture managed clients"),
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
            let ledger_root = f.temp.path().join(format!("ledger-{name}"));
            let mut ledger =
                CapacityLedger::create(&ledger_root, key(name), key("epoch"), 0).unwrap();
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
            roots
                .push(json!({"root":path,"ledger_root":ledger_root,"minimum":ledger.checkpoint()}));
            stores.push(store);
        }
        let seed = f.temp.path().join("seed");
        fs::create_dir(&seed).unwrap();
        for bytes in [b"base".as_slice(), b"staged replacement"] {
            let object = Object::from_bytes(bytes);
            let path = git_asset_store::existing::Layout::Flat.object_path(&seed, &object.sha256);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let byte_source =
            json!({"places":[{"root":seed,"layout":"flat"}],"required":"cooperative-immutable"});
        let config = Config {
            schema: "tig.placement-enrollment.design-1".into(),
            owner: key("owner"),
            authority: f.enrollment.authority.clone(),
            seed: hash(&byte_source),
            providers,
            namespaces: vec![Authority {
                namespace: f.namespace.identity().clone(),
                policy: f.enrollment.policy.clone(),
            }],
            minimum_domains: 1,
            budget: git_asset_store::placement::Budget {
                objects: 32,
                max_object_bytes: 1024,
                max_stream_bytes: 1024,
                max_peak_bytes: 256 * 1024 * 1024,
                limits: limits(),
            },
        };
        let path = f.temp.path().join("storage.json");
        fs::write(&path,identity::canonical_json(&json!({"schema":"tig.placement-profile.design-1","config":config,"namespaces":[{"root":f.temp.path().join("namespace"),"minimum":f.namespace.checkpoint()}],"providers":roots,"byte_source":byte_source,"allow_copy":true,"allow_eviction":true,"allow_reacquisition":true,"allow_cancellation":true})).unwrap()).unwrap();
        Self { path, stores }
    }
    pub(super) fn has(&self, store: usize, bytes: &[u8]) -> bool {
        self.stores[store]
            .read_object(&Object::from_bytes(bytes))
            .is_ok()
    }
    fn edit(&self, edit: impl FnOnce(&mut Value)) {
        let mut value = serde_json::from_slice(&fs::read(&self.path).unwrap()).unwrap();
        edit(&mut value);
        fs::write(&self.path, identity::canonical_json(&value).unwrap()).unwrap();
    }
}
#[test]
fn actual_index_reconcile_copies_holds_job_bytes_and_demotes_after_job_end_on_noop() {
    let mut f = Fixture::new();
    let original = f.plan();
    let profile = f.profile();
    let storage = Storage::new(&f);
    let first = success(cli(&[&profile, &storage.path], "reconcile"));
    assert_eq!(first["storage_converged"], true, "{first}");
    assert!(storage.has(0, b"base"));
    assert!(storage.has(2, b"base"));
    assert!(!storage.has(1, b"base"));
    f.metadata =
        MetadataStore::open(&f.temp.path().join("metadata"), &f.metadata.checkpoint()).unwrap();
    f.namespace
        .apply(
            key("job-enroll"),
            Command::Enroll {
                source: key("job"),
                owner: key("owner"),
                class: Kind::Job,
            },
            &MemoryNodes::default(),
            &limits(),
        )
        .unwrap();
    let job_revision = f.namespace.source(&key("job")).unwrap().unwrap().revision;
    f.namespace
        .select(
            &Select {
                prepare: key("job-prepare"),
                adopt: key("job-adopt"),
                source: key("job"),
                owner: key("owner"),
                expected_revision: job_revision,
                selection: Selection {
                    root: original.selection.root.clone(),
                    context: Digest::blob(b"frozen actual job inputs"),
                },
            },
            &f.metadata.finished_reader().unwrap(),
            &limits(),
        )
        .unwrap();
    stage(&f.repo(), b"staged replacement");
    let next = success(cli(&[&profile, &storage.path], "reconcile"));
    assert_eq!(next["storage_converged"], true);
    assert!(storage.has(0, b"base"));
    assert!(!storage.has(1, b"base"));
    assert!(storage.has(0, b"staged replacement"));
    assert!(storage.has(2, b"staged replacement"));
    let job = f.namespace.source(&key("job")).unwrap().unwrap();
    f.namespace
        .apply(
            key("job-end"),
            Command::Withdraw {
                source: key("job"),
                owner: key("owner"),
                expected_revision: job.revision,
            },
            &f.metadata.finished_reader().unwrap(),
            &limits(),
        )
        .unwrap();
    let settled = success(cli(&[&profile, &storage.path], "reconcile"));
    assert_eq!(settled["storage_converged"], true);
    assert!(!storage.has(0, b"base"));
    assert!(storage.has(1, b"base"));
    assert!(storage.has(2, b"base"));
    let mut restored = Vec::new();
    storage.stores[1]
        .read_object(&Object::from_bytes(b"base"))
        .unwrap()
        .read_to_end(&mut restored)
        .unwrap();
    assert_eq!(restored, b"base");
    let retry = success(cli(&[&profile, &storage.path], "reconcile"));
    assert_eq!(retry["storage_converged"], true);
    assert_eq!(retry["placement"]["stream_bytes"], 0);
}
#[test]
fn index_reconcile_default_copy_permission_reports_durable_selection_but_no_copies() {
    let f = Fixture::new();
    let profile = f.profile();
    let storage = Storage::new(&f);
    storage.edit(|s| {
        for key in [
            "allow_copy",
            "allow_eviction",
            "allow_reacquisition",
            "allow_cancellation",
        ] {
            s.as_object_mut().unwrap().remove(key);
        }
    });
    let held = success(cli(&[&profile, &storage.path], "reconcile"));
    assert_eq!(held["selection"]["desired_current"], true);
    assert_eq!(held["storage_converged"], false);
    assert_eq!(held["placement"]["stream_bytes"], 0);
    assert!(!storage.has(0, b"base"));
    assert!(!storage.has(2, b"base"));
}
#[test]
fn bad_or_unavailable_storage_binding_refuses_before_draft_adoption_and_initialization() {
    for unavailable in [false, true] {
        let mut f = Fixture::new();
        let profile = f.profile();
        let storage = Storage::new(&f);
        let absent = f.temp.path().join("must-not-create");
        storage.edit(|s| {
            if unavailable {
                s["providers"][0]["root"] = json!(absent);
            } else {
                s["config"]["owner"] = json!(key("foreign"));
            }
        });
        let before = f.namespace.checkpoint();
        let result = cli(&[&profile, &storage.path], "reconcile");
        assert!(!result.status.success());
        assert!(result.stderr.is_empty());
        let error: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(
            error["code"],
            if unavailable {
                "provider_unavailable"
            } else {
                "authority"
            }
        );
        assert!(
            f.namespace
                .source(&key("index"))
                .unwrap()
                .unwrap()
                .current
                .is_none()
        );
        assert_eq!(f.namespace.checkpoint(), before);
        assert!(!absent.exists());
    }
}
