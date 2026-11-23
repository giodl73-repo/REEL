use super::*;
use crate::{
    local::LocalStore,
    local_capacity::LocalCapacityProbe,
    source_claims::{Command, Kind, Places, Selection, admission::Policy},
    store_protection::{Binding, Member},
};
use git_asset_core::{
    LogicalPath, Object,
    tree::{MemoryNodes, Tree, TreeLimits},
};
use std::{
    fs,
    io::{Cursor, Read},
    path::PathBuf,
    process::Command as Process,
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
fn policy() -> Policy {
    let p = Places {
        current: vec![key("working"), key("backup")],
        pending: vec![key("working"), key("backup")],
        predecessor: vec![key("history"), key("backup")],
    };
    Policy {
        prefer_active: true,
        classes: [Kind::Branch, Kind::Draft, Kind::Job, Kind::Retention]
            .into_iter()
            .map(|k| (k, p.clone()))
            .collect(),
    }
}
fn tree(nodes: &mut MemoryNodes, objects: &[(&str, Object)]) -> Selection {
    let entries = objects
        .iter()
        .map(|(k, v)| (LogicalPath::parse(k).unwrap(), v.clone()))
        .collect();
    let (t, _) = Tree::build(&entries, nodes, TreeLimits::default()).unwrap();
    Selection {
        root: t.root().clone(),
        context: digest("source"),
    }
}
fn select(n: &mut Namespace, source: &str, selection: Selection, nodes: &MemoryNodes) {
    let revision = n.source(&key(source)).unwrap().unwrap().revision;
    let r = n
        .apply(
            key(&format!("prepare-{source}-{revision}")),
            Command::Prepare {
                source: key(source),
                owner: key("owner"),
                expected_revision: revision,
                selection,
            },
            nodes,
            &limits(),
        )
        .unwrap();
    n.apply(
        key(&format!("adopt-{source}-{revision}")),
        Command::Adopt {
            source: key(source),
            owner: key("owner"),
            expected_revision: revision + 1,
            prepared: r.reference,
        },
        nodes,
        &limits(),
    )
    .unwrap();
}
fn reserve_job(ledger: &mut CapacityLedger, id: &str, peak: u64) -> JournalRef {
    let probe = ledger.begin_probe(key(&format!("probe-{id}"))).unwrap();
    ledger
        .observe(
            key(&format!("sample-{id}")),
            probe.reference(),
            1024 * 1024 * 1024,
        )
        .unwrap();
    ledger
        .reserve(key(id), key("owner"), peak)
        .unwrap()
        .reference()
        .clone()
}
struct Custody {
    store: LocalStore,
    ledger: CapacityLedger,
    grant: RetentionGrant,
    enrollment: ReplicaEnrollment,
}
struct Fixture {
    root: tempfile::TempDir,
    store: LocalStore,
    ledger: CapacityLedger,
    job: JournalRef,
    publication: Publication,
    original: PublicationPlan,
    namespaces: Vec<Namespace>,
    nodes: MemoryNodes,
    keepers: Vec<Custody>,
    enrollment: ReplicaEnrollment,
    context: Digest,
}
impl Fixture {
    fn new() -> Self {
        Self::keepers(1)
    }
    fn keepers(count: usize) -> Self {
        Self::bytes(count, b"target")
    }
    fn bytes(count: usize, bytes: &[u8]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut namespaces = Vec::new();
        for index in 0..2 {
            let mut n = Namespace::create(
                &root.path().join(format!("namespace-{index}")),
                key(&format!("project-{index}")),
                key("epoch"),
                policy(),
            )
            .unwrap();
            n.apply(
                key("enroll"),
                Command::Enroll {
                    source: key("main"),
                    owner: key("owner"),
                    class: if index == 0 { Kind::Branch } else { Kind::Job },
                },
                &MemoryNodes::default(),
                &limits(),
            )
            .unwrap();
            namespaces.push(n);
        }
        let binding = Binding::new(
            1,
            digest("trusted-managed-clients"),
            namespaces
                .iter()
                .map(|n| Member {
                    namespace: n.identity().clone(),
                    places: vec![key("working")],
                })
                .collect(),
        )
        .unwrap();
        let store =
            LocalStore::create_managed(&root.path().join("working"), key("working"), binding)
                .unwrap();
        let mut ledger = CapacityLedger::create(
            &root.path().join("capacity"),
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
        let publication = Publication {
            operation: key("initial"),
            owner: key("owner"),
            object: Object::from_bytes(bytes),
            required: AckLevel::ProcessRestart,
        };
        let original = accounted::plan(&store, &publication).unwrap();
        accounted::publish_accounted(
            &mut ledger,
            &store,
            &job,
            &original,
            &mut Cursor::new(bytes),
        )
        .unwrap();
        let mut keepers = Vec::new();
        for index in 0..count {
            let mut backup = LocalStore::create(
                &root.path().join(format!("backup-{index}")),
                key(&format!("backup-{index}")),
            )
            .unwrap();
            crate::publish(&mut backup, &publication, &mut Cursor::new(bytes)).unwrap();
            let mut capacity = CapacityLedger::create(
                &root.path().join(format!("backup-capacity-{index}")),
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
            let allocation = reserve_job(&mut capacity, "backup-job", retained.required_peak());
            let grant = retention::retain(&mut capacity, &backup, &allocation, &retained)
                .unwrap()
                .grant;
            keepers.push(Custody {
                enrollment: ReplicaEnrollment {
                    store: backup.identity().clone(),
                    failure_domain: key(&format!("domain-backup-{index}")),
                },
                store: backup,
                ledger: capacity,
                grant,
            });
        }
        let enrollment = ReplicaEnrollment {
            store: store.identity().clone(),
            failure_domain: key("domain-working"),
        };
        Self {
            root,
            store,
            ledger,
            job,
            publication,
            original,
            namespaces,
            nodes: MemoryNodes::default(),
            keepers,
            enrollment,
            context: digest("admitted-lifecycle"),
        }
    }
    fn request(&self, operation: &str) -> Request {
        Request {
            operation: key(operation),
            owner: key("owner"),
            publication: self.publication.clone(),
            context: self.context.clone(),
            source: self.enrollment.clone(),
            keepers: self
                .keepers
                .iter()
                .map(|k| Keeper {
                    grant: k.grant.clone(),
                    enrollment: k.enrollment.clone(),
                })
                .collect(),
            required_domains: self.keepers.len(),
            capacity: self.ledger.identity().clone(),
        }
    }
    fn plan(&self) -> Plan {
        plan(&self.store, self.request("eviction")).unwrap()
    }
    fn apply(&mut self, plan: &Plan) -> Result<Receipt> {
        self.apply_hook(plan, |_| Ok(()))
    }
    fn apply_hook(
        &mut self,
        plan: &Plan,
        hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<Receipt> {
        let authorization = plan.authorize()?;
        let mut backups = self
            .keepers
            .iter_mut()
            .map(|k| Backup {
                store: &k.store,
                ledger: &mut k.ledger,
                enrollment: &k.enrollment,
            })
            .collect::<Vec<_>>();
        Execution {
            store: &self.store,
            ledger: &mut self.ledger,
            job: &self.job,
            namespaces: &mut self.namespaces,
            nodes: &self.nodes,
            limits: &limits(),
            backups: &mut backups,
            admitted_context: &self.context,
            source_enrollment: &self.enrollment,
        }
        .apply_hook(plan, &authorization, hook)
    }
    fn object_path(&self) -> PathBuf {
        self.root
            .path()
            .join("working/objects")
            .join(self.publication.object.sha256.as_str())
    }
    fn cancel(&mut self, plan: &Plan) -> Result<Receipt> {
        let mut backups = self
            .keepers
            .iter_mut()
            .map(|k| Backup {
                store: &k.store,
                ledger: &mut k.ledger,
                enrollment: &k.enrollment,
            })
            .collect::<Vec<_>>();
        Execution {
            store: &self.store,
            ledger: &mut self.ledger,
            job: &self.job,
            namespaces: &mut self.namespaces,
            nodes: &self.nodes,
            limits: &limits(),
            backups: &mut backups,
            admitted_context: &self.context,
            source_enrollment: &self.enrollment,
        }
        .cancel(plan, &plan.authorize()?)
    }
    fn restored_plan(&self, retired: &Plan, operation: &str) -> ReacquisitionPlan {
        let mut request = self.publication.clone();
        request.operation = key(operation);
        plan_reacquisition(
            &self.store,
            retired,
            request,
            digest("reacquisition-context"),
        )
        .unwrap()
    }
    fn restore(&mut self, plan: &ReacquisitionPlan) -> Result<AccountedReceipt> {
        let mut source = self.keepers[0]
            .store
            .read_object(&self.publication.object)
            .unwrap();
        Reacquisition {
            store: &self.store,
            ledger: &mut self.ledger,
            job: &self.job,
        }
        .apply(plan, &plan.authorize()?, &mut source)
    }
}

#[test]
fn actual_unlink_retains_independent_bytes_and_exact_replay_cannot_resurrect() {
    let mut f = Fixture::new();
    let plan = f.plan();
    let receipt = f.apply(&plan).unwrap();
    assert!(!receipt.historical_replay);
    assert_eq!(receipt.logical_bytes, 6);
    assert!(!f.object_path().exists());
    assert!(matches!(
        f.store.read_object(&f.publication.object),
        Err(crate::Error::AuthorizedAbsence)
    ));
    let mut surviving = Vec::new();
    f.keepers[0]
        .store
        .read_object(&f.publication.object)
        .unwrap()
        .read_to_end(&mut surviving)
        .unwrap();
    assert_eq!(surviving, b"target");
    let replay = f.apply(&plan).unwrap();
    assert!(replay.historical_replay);
    assert_eq!(replay.charged_footprint, receipt.charged_footprint);
    assert!(matches!(
        accounted::publish_accounted(
            &mut f.ledger,
            &f.store,
            &f.job,
            &f.original,
            &mut Cursor::new(b"target")
        ),
        Err(AccountedError::Store(crate::Error::AuthorizedAbsence))
    ));
    assert!(!f.object_path().exists());
}
#[test]
fn explicit_restore_new_incarnation_survives_old_deletion_and_publication_retries() {
    let mut f = Fixture::new();
    let retired = f.plan();
    f.apply(&retired).unwrap();
    let restore = f.restored_plan(&retired, "restore-new");
    f.restore(&restore).unwrap();
    let mut restored = Vec::new();
    f.store
        .read_object(&f.publication.object)
        .unwrap()
        .read_to_end(&mut restored)
        .unwrap();
    assert_eq!(restored, b"target");
    assert!(f.apply(&retired).unwrap().historical_replay);
    assert!(f.object_path().exists());
    assert!(matches!(
        accounted::publish_accounted(
            &mut f.ledger,
            &f.store,
            &f.job,
            &f.original,
            &mut Cursor::new(b"target")
        ),
        Err(AccountedError::Store(crate::Error::AuthorizedAbsence))
    ));
    assert!(f.object_path().exists());
    f.restore(&restore).unwrap();
    assert_eq!(
        f.store
            .life_head(&f.publication.object)
            .unwrap()
            .unwrap()
            .birth
            .index,
        1
    );
}
#[test]
fn managed_marker_requires_accounting_and_legacy_stores_cannot_retire() {
    let mut f = Fixture::new();
    let before = fs::read_dir(f.root.path().join("working/fences"))
        .unwrap()
        .count();
    let request = Publication {
        operation: key("unaccounted"),
        owner: key("owner"),
        object: Object::from_bytes(b"unaccounted"),
        required: AckLevel::ProcessRestart,
    };
    assert_eq!(
        crate::publish(&mut f.store, &request, &mut Cursor::new(b"unaccounted")),
        Err(crate::Error::Unsupported)
    );
    assert_eq!(
        fs::read_dir(f.root.path().join("working/fences"))
            .unwrap()
            .count(),
        before
    );
    assert!(plan(&f.keepers[0].store, f.request("legacy")).is_err());
    let marker: serde_json::Value =
        serde_json::from_slice(&fs::read(f.root.path().join("working/store.json")).unwrap())
            .unwrap();
    assert_eq!(marker["schema"], "tig.local-store.design-7");
    let mut old = marker;
    old["schema"] = "tig.local-store.design-6".into();
    fs::write(
        f.root.path().join("working/store.json"),
        lifecycle::encode(&old).unwrap(),
    )
    .unwrap();
    assert!(LocalStore::open(&f.root.path().join("working")).is_err());
}
#[test]
fn existing_reader_new_claim_and_owned_retention_each_preserve_working_bytes() {
    let mut f = Fixture::new();
    let plan = f.plan();
    let reader = f.store.read_object(&f.publication.object).unwrap();
    assert!(matches!(
        f.apply(&plan),
        Err(Error::Protection(store_protection::Error::Busy))
    ));
    drop(reader);
    let selection = tree(&mut f.nodes, &[("asset", f.publication.object.clone())]);
    select(&mut f.namespaces[1], "main", selection, &f.nodes);
    assert!(matches!(
        f.apply(&plan),
        Err(Error::Protection(store_protection::Error::Required))
    ));
    assert!(f.object_path().exists());
    let mut f = Fixture::new();
    let plan = f.plan();
    let retained = retention::plan(
        &f.store,
        &retention::RetentionRequest {
            operation: key("working-hold"),
            owner: key("owner"),
            publication: f.publication.clone(),
            minimum_ack: AckLevel::ProcessRestart,
        },
    )
    .unwrap();
    retention::retain(&mut f.ledger, &f.store, &f.job, &retained).unwrap();
    assert!(matches!(
        f.apply(&plan),
        Err(Error::Protection(store_protection::Error::Retained))
    ));
    assert!(f.object_path().exists());
}
#[test]
fn invalid_context_authorization_domains_and_missing_custody_never_unlink() {
    let mut f = Fixture::new();
    let mut request = f.request("eviction");
    request.context = digest("stale-context");
    let bad = plan(&f.store, request).unwrap();
    let before = f.ledger.checkpoint();
    assert!(matches!(f.apply(&bad), Err(Error::InvalidPlan)));
    assert_eq!(before, f.ledger.checkpoint());
    let plan = f.plan();
    let mut backups = f
        .keepers
        .iter_mut()
        .map(|k| Backup {
            store: &k.store,
            ledger: &mut k.ledger,
            enrollment: &k.enrollment,
        })
        .collect::<Vec<_>>();
    assert!(matches!(
        Execution {
            store: &f.store,
            ledger: &mut f.ledger,
            job: &f.job,
            namespaces: &mut f.namespaces,
            nodes: &f.nodes,
            limits: &limits(),
            backups: &mut backups,
            admitted_context: &f.context,
            source_enrollment: &f.enrollment
        }
        .apply(
            &plan,
            &Authorization {
                plan: digest("wrong")
            }
        ),
        Err(Error::WrongAuthorization)
    ));
    assert_eq!(before, f.ledger.checkpoint());
    drop(backups);
    f.keepers[0].enrollment.failure_domain = f.enrollment.failure_domain.clone();
    let plan = f.plan();
    assert!(matches!(f.apply(&plan), Err(Error::MissingCustody)));
    assert!(f.object_path().exists());
    let mut f = Fixture::new();
    let plan = f.plan();
    fs::remove_file(
        f.root
            .path()
            .join("backup-0/objects")
            .join(f.publication.object.sha256.as_str()),
    )
    .unwrap();
    assert!(f.apply(&plan).is_err());
    assert!(f.object_path().exists());
}
#[test]
fn unavailable_keeper_authority_after_intent_preserves_source_and_retry_rechecks_custody() {
    let mut f = Fixture::new();
    let plan = f.plan();
    let from = f.root.path().join("backup-0/store.json");
    let to = f.root.path().join("backup-0/offline-marker.json");
    assert!(
        f.apply_hook(&plan, |stage| {
            if stage == Stage::Intent {
                fs::rename(&from, &to).unwrap();
            }
            Ok(())
        })
        .is_err()
    );
    assert!(f.object_path().exists());
    assert!(f.store.read_object(&f.publication.object).is_ok());
    fs::rename(&to, &from).unwrap();
    f.apply(&plan).unwrap();
    assert!(!f.object_path().exists());
}
#[test]
fn mixed_main_and_job_workflow_demotes_only_after_other_use_ends_then_restores() {
    let mut f = Fixture::keepers(2);
    let old = f.publication.object.clone();
    let common1 = Object::from_bytes(b"shared-one");
    let common2 = Object::from_bytes(b"shared-two");
    for (index, bytes) in [b"shared-one".as_slice(), b"shared-two"]
        .into_iter()
        .enumerate()
    {
        let request = Publication {
            operation: key(&format!("unchanged-{index}")),
            owner: key("owner"),
            object: Object::from_bytes(bytes),
            required: AckLevel::ProcessRestart,
        };
        let plan = accounted::plan(&f.store, &request).unwrap();
        accounted::publish_accounted(
            &mut f.ledger,
            &f.store,
            &f.job,
            &plan,
            &mut Cursor::new(bytes),
        )
        .unwrap();
    }
    let previous = tree(
        &mut f.nodes,
        &[
            ("changed", old.clone()),
            ("one", common1.clone()),
            ("two", common2.clone()),
        ],
    );
    select(&mut f.namespaces[0], "main", previous.clone(), &f.nodes);
    select(&mut f.namespaces[1], "main", previous.clone(), &f.nodes);
    let replacement = Publication {
        operation: key("replacement-source"),
        owner: key("owner"),
        object: Object::from_bytes(b"replacement"),
        required: AckLevel::ProcessRestart,
    };
    let publish = accounted::plan(&f.store, &replacement).unwrap();
    accounted::publish_accounted(
        &mut f.ledger,
        &f.store,
        &f.job,
        &publish,
        &mut Cursor::new(b"replacement"),
    )
    .unwrap();
    let archive = &mut f.keepers[1];
    crate::publish(
        &mut archive.store,
        &replacement,
        &mut Cursor::new(b"replacement"),
    )
    .unwrap();
    let keep = retention::plan(
        &archive.store,
        &retention::RetentionRequest {
            operation: key("replacement-backup"),
            owner: key("owner"),
            publication: replacement.clone(),
            minimum_ack: AckLevel::ProcessRestart,
        },
    )
    .unwrap();
    let keep_job = reserve_job(&mut archive.ledger, "replacement-job", keep.required_peak());
    retention::retain(&mut archive.ledger, &archive.store, &keep_job, &keep).unwrap();
    let current = tree(
        &mut f.nodes,
        &[
            ("changed", Object::from_bytes(b"replacement")),
            ("one", common1.clone()),
            ("two", common2.clone()),
        ],
    );
    select(&mut f.namespaces[0], "main", current, &f.nodes);
    let plan = f.plan();
    assert!(matches!(
        f.apply(&plan),
        Err(Error::Protection(store_protection::Error::Required))
    ));
    let revision = f.namespaces[1]
        .source(&key("main"))
        .unwrap()
        .unwrap()
        .revision;
    f.namespaces[1]
        .apply(
            key("withdraw-job"),
            Command::Withdraw {
                source: key("main"),
                owner: key("owner"),
                expected_revision: revision,
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    f.apply(&plan).unwrap();
    assert!(!f.object_path().exists());
    assert!(f.store.read_object(&common1).is_ok());
    assert!(f.store.read_object(&common2).is_ok());
    assert!(f.store.read_object(&replacement.object).is_ok());
    select(&mut f.namespaces[0], "main", previous, &f.nodes);
    let restore = f.restored_plan(&plan, "restore-prior");
    f.restore(&restore).unwrap();
    assert!(f.object_path().exists());
}
#[test]
fn missing_active_bytes_or_incarnation_metadata_is_damage_not_authorized_absence() {
    let mut f = Fixture::new();
    fs::remove_file(f.object_path()).unwrap();
    assert!(matches!(
        accounted::publish_accounted(
            &mut f.ledger,
            &f.store,
            &f.job,
            &f.original,
            &mut Cursor::new(b"target")
        ),
        Err(AccountedError::Store(_))
    ));
    let f = Fixture::new();
    let dir = f
        .root
        .path()
        .join("working/lifecycle")
        .join(f.publication.object.sha256.as_str());
    fs::remove_file(dir.join("00.birth.json")).unwrap();
    assert!(matches!(
        f.store.read_object(&f.publication.object),
        Err(crate::Error::CorruptRecord)
    ));
    assert!(plan(&f.store, f.request("damaged")).is_err());
}

#[test]
fn same_operation_changed_scope_and_spent_publication_id_cannot_be_rebound() {
    let mut f = Fixture::new();
    let original = f.plan();
    let reader = f.store.read_object(&f.publication.object).unwrap();
    assert!(f.apply(&original).is_err());
    drop(reader);
    let mut changed = original.clone();
    changed.request.keepers[0].enrollment.failure_domain = key("changed-domain");
    assert!(matches!(
        f.apply(&changed),
        Err(Error::Ledger(LedgerError::Conflict))
    ));
    assert!(f.object_path().exists());
    f.apply(&original).unwrap();
    assert!(plan_reacquisition(&f.store, &original, f.publication.clone(), digest("new")).is_err());
}
#[test]
fn insufficient_reacquisition_payload_budget_does_not_install_a_new_birth() {
    let mut f = Fixture::new();
    let retired = f.plan();
    f.apply(&retired).unwrap();
    let restore = f.restored_plan(&retired, "restored");
    let too_small = reserve_job(&mut f.ledger, "too-small", restore.limit);
    let before = lifecycle::digest(
        &f.store
            .life_head(&f.publication.object)
            .unwrap()
            .unwrap()
            .birth,
    )
    .unwrap();
    assert!(
        Reacquisition {
            store: &f.store,
            ledger: &mut f.ledger,
            job: &too_small
        }
        .apply(
            &restore,
            &restore.authorize().unwrap(),
            &mut Cursor::new(b"target")
        )
        .is_err()
    );
    assert_eq!(
        lifecycle::digest(
            &f.store
                .life_head(&f.publication.object)
                .unwrap()
                .unwrap()
                .birth
        )
        .unwrap(),
        before
    );
    assert!(!f.object_path().exists());
}
#[test]
fn history_exhaustion_refuses_before_the_last_active_copy_can_be_evicted() {
    let mut f = Fixture::new();
    for index in 0..lifecycle::MAX_BIRTHS - 1 {
        let retired = plan(&f.store, f.request(&format!("eviction-{index}"))).unwrap();
        f.apply(&retired).unwrap();
        let restored = f.restored_plan(&retired, &format!("restore-{index}"));
        f.restore(&restored).unwrap();
        f.publication = restored.publication.clone();
    }
    assert_eq!(
        f.store
            .life_head(&f.publication.object)
            .unwrap()
            .unwrap()
            .birth
            .index,
        lifecycle::MAX_BIRTHS - 1
    );
    assert!(plan(&f.store, f.request("exhausted")).is_err());
    assert!(f.object_path().exists());
}

#[derive(Serialize, Deserialize)]
struct ChildInput {
    plan: Plan,
    source_minimum: crate::ledger::Checkpoint,
    namespace_minimums: Vec<crate::source_claims::Checkpoint>,
    keeper_minimums: Vec<crate::ledger::Checkpoint>,
    job: JournalRef,
    reacquisition: Option<ReacquisitionPlan>,
}
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn spawn(f: &Fixture, plan: &Plan, mode: &str, reacquisition: Option<ReacquisitionPlan>) -> Child {
    fs::write(
        f.root.path().join("child-input.json"),
        lifecycle::encode(&ChildInput {
            plan: plan.clone(),
            source_minimum: f.ledger.checkpoint(),
            namespace_minimums: f.namespaces.iter().map(Namespace::checkpoint).collect(),
            keeper_minimums: f.keepers.iter().map(|k| k.ledger.checkpoint()).collect(),
            job: f.job.clone(),
            reacquisition,
        })
        .unwrap(),
    )
    .unwrap();
    Child(
        Process::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "retirement::tests::retirement_child",
                "--nocapture",
            ])
            .env("GIT_ASSET_RETIREMENT_CHILD_ROOT", f.root.path())
            .env("GIT_ASSET_RETIREMENT_CHILD_STAGE", mode)
            .spawn()
            .unwrap(),
    )
}
#[test]
fn retirement_child() {
    let Some(root) = std::env::var_os("GIT_ASSET_RETIREMENT_CHILD_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let input: ChildInput =
        serde_json::from_slice(&fs::read(root.join("child-input.json")).unwrap()).unwrap();
    let store = LocalStore::open(&root.join("working")).unwrap();
    let mut ledger = CapacityLedger::open(&root.join("capacity"), &input.source_minimum).unwrap();
    let mut namespaces = input
        .namespace_minimums
        .iter()
        .enumerate()
        .map(|(n, min)| Namespace::open(&root.join(format!("namespace-{n}")), min).unwrap())
        .collect::<Vec<_>>();
    let mut keepers = input
        .keeper_minimums
        .iter()
        .enumerate()
        .map(|(n, min)| Custody {
            store: LocalStore::open(&root.join(format!("backup-{n}"))).unwrap(),
            ledger: CapacityLedger::open(&root.join(format!("backup-capacity-{n}")), min).unwrap(),
            grant: input.plan.request.keepers[n].grant.clone(),
            enrollment: input.plan.request.keepers[n].enrollment.clone(),
        })
        .collect::<Vec<_>>();
    let stage = match std::env::var("GIT_ASSET_RETIREMENT_CHILD_STAGE")
        .unwrap()
        .as_str()
    {
        "bound" => Stage::Bound,
        "intent" => Stage::Intent,
        "unlink" => Stage::Unlinked,
        "complete" => Stage::Completed,
        "settled" => Stage::Settled,
        "birth" => Stage::ReacquisitionBorn,
        "abort" => Stage::Aborted,
        "cancel-birth" => Stage::CancellationBorn,
        _ => panic!("unknown stage"),
    };
    let hook = |actual| {
        if actual == stage {
            std::process::exit(75);
        }
        Ok(())
    };
    if let Some(plan) = input.reacquisition {
        let mut source = keepers[0]
            .store
            .read_object(&plan.publication.object)
            .unwrap();
        Reacquisition {
            store: &store,
            ledger: &mut ledger,
            job: &input.job,
        }
        .apply_hook(&plan, &plan.authorize().unwrap(), &mut source, hook)
        .unwrap();
    } else {
        let mut backups = keepers
            .iter_mut()
            .map(|k| Backup {
                store: &k.store,
                ledger: &mut k.ledger,
                enrollment: &k.enrollment,
            })
            .collect::<Vec<_>>();
        let mut execution = Execution {
            store: &store,
            ledger: &mut ledger,
            job: &input.job,
            namespaces: &mut namespaces,
            nodes: &MemoryNodes::default(),
            limits: &limits(),
            backups: &mut backups,
            admitted_context: &input.plan.request.context,
            source_enrollment: &input.plan.request.source,
        };
        if matches!(stage, Stage::Aborted | Stage::CancellationBorn) {
            execution
                .cancel_hook(&input.plan, &input.plan.authorize().unwrap(), hook)
                .unwrap();
        } else {
            execution
                .apply_hook(&input.plan, &input.plan.authorize().unwrap(), hook)
                .unwrap();
        }
    }
    panic!("child missed injected boundary");
}
#[test]
fn real_child_exits_resume_intent_unlink_and_completion_without_blind_deletion() {
    for mode in ["bound", "intent", "unlink", "complete", "settled"] {
        let mut f = Fixture::new();
        let retired = f.plan();
        let mut child = spawn(&f, &retired, mode, None);
        assert_eq!(child.0.wait().unwrap().code(), Some(75));
        f.store = LocalStore::open(&f.root.path().join("working")).unwrap();
        f.ledger =
            CapacityLedger::open(&f.root.path().join("capacity"), &f.ledger.checkpoint()).unwrap();
        assert_eq!(f.object_path().exists(), matches!(mode, "bound" | "intent"));
        let sequence = f.ledger.checkpoint();
        let status = super::status::inspect(&f.store, &mut f.ledger, &f.job, &retired).unwrap();
        assert!(matches!(
            status.phase,
            super::status::Phase::Admitted
                | super::status::Phase::Pending
                | super::status::Phase::Retired
        ));
        assert!(!status.removal_authority);
        assert_eq!(f.ledger.checkpoint(), sequence);
        if mode == "unlink" {
            let selection = tree(
                &mut f.nodes,
                &[("new-demand", f.publication.object.clone())],
            );
            select(&mut f.namespaces[0], "main", selection, &f.nodes);
            // Acknowledging the already absent old incarnation is metadata-only.
        }
        f.apply(&retired).unwrap();
        assert!(!f.object_path().exists());
        let restore = f.restored_plan(&retired, "restore-after-exit");
        f.restore(&restore).unwrap();
        assert!(f.apply(&retired).unwrap().historical_replay);
        assert!(f.object_path().exists());
    }
}
#[test]
fn real_child_exit_after_new_birth_resumes_same_incarnation_and_old_retry_is_harmless() {
    let mut f = Fixture::new();
    let retired = f.plan();
    f.apply(&retired).unwrap();
    let restore = f.restored_plan(&retired, "resume-birth");
    let mut child = spawn(&f, &retired, "birth", Some(restore.clone()));
    assert_eq!(child.0.wait().unwrap().code(), Some(75));
    assert!(!f.object_path().exists());
    assert!(f.apply(&retired).unwrap().historical_replay);
    f.restore(&restore).unwrap();
    assert!(f.object_path().exists());
    assert_eq!(
        f.store
            .life_head(&f.publication.object)
            .unwrap()
            .unwrap()
            .birth
            .index,
        1
    );
    assert!(f.apply(&retired).unwrap().historical_replay);
    assert!(f.object_path().exists());
}

#[test]
fn new_demand_after_pending_intent_can_cancel_without_losing_bytes_or_publication_identity() {
    let mut f = Fixture::new();
    let abandoned = f.plan();
    assert!(matches!(
        f.apply_hook(&abandoned, |stage| if stage == Stage::Intent {
            Err(Error::Injected)
        } else {
            Ok(())
        }),
        Err(Error::Injected)
    ));
    let selected = tree(
        &mut f.nodes,
        &[("needed-again", f.publication.object.clone())],
    );
    select(&mut f.namespaces[0], "main", selected, &f.nodes);
    assert!(matches!(
        f.apply(&abandoned),
        Err(Error::Protection(store_protection::Error::Required))
    ));
    let cancelled = f.cancel(&abandoned).unwrap();
    assert!(cancelled.cancelled);
    let before = f.ledger.checkpoint();
    let status = super::status::inspect(&f.store, &mut f.ledger, &f.job, &abandoned).unwrap();
    assert!(matches!(status.phase, super::status::Phase::Cancelled));
    assert!(matches!(
        status.observed,
        super::status::Observation::VerifiedPresent
    ));
    assert!(status.historical_outcome);
    assert_eq!(f.ledger.checkpoint(), before);
    assert_eq!(cancelled.logical_bytes, 0);
    assert!(f.object_path().exists());
    accounted::publish_accounted(
        &mut f.ledger,
        &f.store,
        &f.job,
        &f.original,
        &mut Cursor::new(b"target"),
    )
    .unwrap();
    assert!(f.apply(&abandoned).unwrap().cancelled);
    let revision = f.namespaces[0]
        .source(&key("main"))
        .unwrap()
        .unwrap()
        .revision;
    f.namespaces[0]
        .apply(
            key("withdraw"),
            Command::Withdraw {
                source: key("main"),
                owner: key("owner"),
                expected_revision: revision,
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    let later = plan(&f.store, f.request("later-eviction")).unwrap();
    f.apply(&later).unwrap();
    let restored = f.restored_plan(&later, "later-restore");
    f.restore(&restored).unwrap();
    assert!(f.apply(&abandoned).unwrap().cancelled);
    assert!(f.object_path().exists());
}
#[test]
fn actual_child_exits_during_cancellation_preserve_original_receipts_and_close_metadata_once() {
    for mode in ["abort", "cancel-birth"] {
        let mut f = Fixture::new();
        let abandoned = f.plan();
        assert!(
            f.apply_hook(&abandoned, |stage| if stage == Stage::Intent {
                Err(Error::Injected)
            } else {
                Ok(())
            })
            .is_err()
        );
        let mut child = spawn(&f, &abandoned, mode, None);
        assert_eq!(child.0.wait().unwrap().code(), Some(75));
        assert!(f.object_path().exists());
        assert!(f.apply(&abandoned).unwrap().cancelled);
        assert_eq!(
            f.store
                .life_head(&f.publication.object)
                .unwrap()
                .unwrap()
                .birth
                .index,
            1
        );
        accounted::publish_accounted(
            &mut f.ledger,
            &f.store,
            &f.job,
            &f.original,
            &mut Cursor::new(b"target"),
        )
        .unwrap();
        assert!(f.cancel(&abandoned).unwrap().cancelled);
    }
}

#[test]
fn unrelated_physical_publication_and_claim_commit_progress_while_retirement_is_fenced() {
    let mut f = Fixture::new();
    let retired = f.plan();
    let target = f.publication.object.clone();
    let bytes = (0..1024)
        .map(|n| format!("unrelated-{n}").into_bytes())
        .find(|b| Object::from_bytes(b).sha256.as_str()[..2] != target.sha256.as_str()[..2])
        .unwrap();
    let object = Object::from_bytes(&bytes);
    let selection = tree(&mut f.nodes, &[("other", object.clone())]);
    let mut node_copy = MemoryNodes::default();
    tree(&mut node_copy, &[("other", object.clone())]);
    let namespace = f.root.path().join("namespace-0");
    let minimum = f.namespaces[0].checkpoint();
    let root = f.root.path().to_owned();
    let ledger_minimum = f.ledger.checkpoint();
    let job = f.job.clone();
    f.apply_hook(&retired, |stage| {
        if stage == Stage::Intent {
            let source = LocalStore::open(&root.join("working")).unwrap();
            assert!(matches!(
                source.read_object(&target),
                Err(crate::Error::Busy)
            ));
            let mut peer = Namespace::open(&namespace, &minimum).unwrap();
            assert!(matches!(
                peer.fence(&target),
                Err(crate::source_claims::admission::Error::Busy)
            ));
            let mut capacity =
                CapacityLedger::open(&root.join("capacity"), &ledger_minimum).unwrap();
            let request = Publication {
                operation: key("unrelated-publish"),
                owner: key("owner"),
                object: object.clone(),
                required: AckLevel::ProcessRestart,
            };
            let publish = accounted::plan(&source, &request).unwrap();
            accounted::publish_accounted(
                &mut capacity,
                &source,
                &job,
                &publish,
                &mut Cursor::new(&bytes),
            )
            .unwrap();
            peer.apply(
                key("unrelated-claim"),
                Command::Prepare {
                    source: key("main"),
                    owner: key("owner"),
                    expected_revision: 1,
                    selection: selection.clone(),
                },
                &node_copy,
                &limits(),
            )
            .unwrap();
        }
        Ok(())
    })
    .unwrap();
    assert!(!f.object_path().exists());
    assert!(f.store.read_object(&object).is_ok());
}
#[test]
fn existing_owned_retention_handoff_composes_with_managed_replica_eviction() {
    let mut f = Fixture::new();
    let retained = retention::plan(
        &f.store,
        &retention::RetentionRequest {
            operation: key("local-retention"),
            owner: key("owner"),
            publication: f.publication.clone(),
            minimum_ack: AckLevel::ProcessRestart,
        },
    )
    .unwrap();
    let old = retention::retain(&mut f.ledger, &f.store, &f.job, &retained)
        .unwrap()
        .grant;
    let successor = &mut f.keepers[0];
    let handoff = retention::handoff::plan(
        &f.store,
        &retention::handoff::HandoffRequest {
            operation: key("handoff"),
            owner: key("owner"),
            previous: old,
            successor: successor.grant.clone(),
            source_domain: f.enrollment.failure_domain.clone(),
            successor_domain: successor.enrollment.failure_domain.clone(),
        },
    )
    .unwrap();
    retention::handoff::apply(
        &mut f.ledger,
        &f.store,
        &mut successor.ledger,
        &successor.store,
        &f.job,
        &handoff,
        &handoff.authorize_handoff().unwrap(),
    )
    .unwrap();
    let retired = f.plan();
    f.apply(&retired).unwrap();
    assert!(!f.object_path().exists());
}
#[test]
fn publication_scope_collision_refuses_reacquisition_before_installing_birth() {
    let mut f = Fixture::new();
    let retired = f.plan();
    f.apply(&retired).unwrap();
    let (collision, _) = keys(&f.store, &retired.request.operation, "evict").unwrap();
    let restored = f.restored_plan(&retired, collision.as_str());
    assert!(f.restore(&restored).is_err());
    assert_eq!(
        f.store
            .life_head(&f.publication.object)
            .unwrap()
            .unwrap()
            .birth
            .index,
        0
    );
    assert!(!f.object_path().exists());
}
#[test]
fn empty_and_multibuffer_payloads_retire_and_restore_with_exact_bytes() {
    for bytes in [Vec::new(), vec![0x5a; 256 * 1024 + 7]] {
        let mut f = Fixture::bytes(1, &bytes);
        let retired = f.plan();
        f.apply(&retired).unwrap();
        assert!(!f.object_path().exists());
        let restored = f.restored_plan(&retired, "restore-bytes");
        f.restore(&restored).unwrap();
        let mut actual = Vec::new();
        f.store
            .read_object(&f.publication.object)
            .unwrap()
            .read_to_end(&mut actual)
            .unwrap();
        assert_eq!(actual, bytes);
    }
}

#[test]
fn wrong_capacity_forged_history_index_and_replaced_provider_cannot_remove_current_bytes() {
    let mut f = Fixture::new();
    let original = f.plan();
    let before = f.ledger.checkpoint();
    let mut forged = original.clone();
    forged.request.capacity = digest("foreign-capacity");
    assert!(matches!(f.apply(&forged), Err(Error::InvalidPlan)));
    assert_eq!(f.ledger.checkpoint(), before);
    let mut forged = original.clone();
    forged.birth.index = usize::MAX;
    assert!(matches!(f.apply(&forged), Err(Error::InvalidPlan)));
    assert_eq!(f.ledger.checkpoint(), before);
    let binding = f.store.protection().unwrap().clone();
    fs::rename(
        f.root.path().join("working"),
        f.root.path().join("old-provider"),
    )
    .unwrap();
    f.store = LocalStore::create_managed(&f.root.path().join("working"), key("working"), binding)
        .unwrap();
    let publish = accounted::plan(&f.store, &f.publication).unwrap();
    accounted::publish_accounted(
        &mut f.ledger,
        &f.store,
        &f.job,
        &publish,
        &mut Cursor::new(b"target"),
    )
    .unwrap();
    assert!(matches!(
        f.apply(&original),
        Err(Error::Protection(store_protection::Error::WrongEnrollment))
    ));
    assert!(f.object_path().exists());
    assert!(
        f.root
            .path()
            .join("old-provider/objects")
            .join(f.publication.object.sha256.as_str())
            .exists()
    );
}

#[test]
fn settled_authority_with_restored_provider_prefix_refuses_repeat_destructive_io() {
    let mut f = Fixture::new();
    let retired = f.plan();
    f.apply(&retired).unwrap();
    let restore = f.restored_plan(&retired, "new-copy");
    f.restore(&restore).unwrap();
    let dir = f
        .root
        .path()
        .join("working/lifecycle")
        .join(f.publication.object.sha256.as_str());
    fs::remove_file(dir.join("01.birth.json")).unwrap();
    fs::remove_file(dir.join("00.done.json")).unwrap();
    let status = super::status::inspect(&f.store, &mut f.ledger, &f.job, &retired).unwrap();
    assert!(matches!(
        status.phase,
        super::status::Phase::RecoveryRequired
    ));
    assert!(!status.historical_outcome);
    assert!(matches!(
        f.apply(&retired),
        Err(Error::Store(crate::Error::NeedsRecovery))
    ));
    assert!(f.object_path().exists());
    fs::remove_file(dir.join("00.intent.json")).unwrap();
    let status = super::status::inspect(&f.store, &mut f.ledger, &f.job, &retired).unwrap();
    assert!(matches!(
        status.phase,
        super::status::Phase::RecoveryRequired
    ));
    assert!(matches!(
        f.apply(&retired),
        Err(Error::Store(crate::Error::NeedsRecovery))
    ));
    assert!(f.object_path().exists());
}

#[test]
fn versioned_keeper_order_is_canonical_and_changed_plan_schema_refuses_before_admission() {
    let mut f = Fixture::keepers(2);
    let mut request = f.request("canonical");
    let first = plan(&f.store, request.clone()).unwrap();
    request.keepers.reverse();
    let second = plan(&f.store, request).unwrap();
    assert_eq!(first.id().unwrap(), second.id().unwrap());
    let encoded = lifecycle::encode(&first).unwrap();
    let decoded: Plan = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded.id().unwrap(), first.id().unwrap());
    let mut invalid = decoded;
    invalid.schema = "unknown".into();
    let before = f.ledger.checkpoint();
    assert!(f.apply(&invalid).is_err());
    assert_eq!(f.ledger.checkpoint(), before);
    assert!(f.object_path().exists());
}

/// Explicit L2 qualification: build git-asset-storage first and supply its absolute
/// path. A library-only Cargo invocation cannot supply Cargo's integration-bin
/// artifact, so this cross-artifact fixture is run separately, never silently skipped.
#[test]
#[ignore = "requires GIT_ASSET_RETIREMENT_CLI pointing to the freshly built git-asset-storage binary"]
fn explicit_cli_pending_cancellation_qualification() {
    let binary =
        PathBuf::from(std::env::var_os("GIT_ASSET_RETIREMENT_CLI").expect("explicit test binary"));
    assert!(binary.is_absolute() && binary.is_file());
    let f = Fixture::new();
    let p = f.plan();
    let mut child = spawn(&f, &p, "intent", None);
    assert_eq!(child.0.wait().unwrap().code(), Some(75));
    let mut metadata = crate::metadata::MetadataStore::create(
        &f.root.path().join("metadata"),
        crate::metadata::Limits {
            nodes: 100,
            bytes: 1024 * 1024,
            database_bytes: 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let minimum = metadata.finish().unwrap();
    let profile = serde_json::json!({"schema":"tig.storage-retirement-profile.design-1",
        "source_root":f.root.path().join("working"),"enrollment":p.enrollment,"ledger_root":f.root.path().join("capacity"),
        "minimum":f.ledger.checkpoint(),"job":f.job,"source":f.enrollment,
        "metadata":{"root":f.root.path().join("offline-metadata"),"minimum":minimum},
        "namespaces":f.namespaces.iter().enumerate().map(|(i,n)|serde_json::json!({"root":f.root.path().join(format!("offline-namespace-{i}")),"minimum":n.checkpoint()})).collect::<Vec<_>>(),
        "backups":f.keepers.iter().map(|k|serde_json::json!({"root":f.root.path().join("offline-keeper"),"ledger_root":f.root.path().join("offline-keeper-ledger"),
            "minimum":k.ledger.checkpoint(),"enrollment":k.enrollment,"grant":k.grant,
            "places":f.namespaces.iter().flat_map(|n|["backup","history"].map(|s|serde_json::json!({"namespace":n.identity(),"place":s}))).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "limits":limits(),"policy":{"owner":"owner","context":f.context,"minimum_domains":1,"max_object_bytes":1024,"max_peak_bytes":128*1024*1024,
            "allow_eviction":false,"allow_reacquisition":false,"allow_cancellation":true}});
    let profile_path = f.root.path().join("profile.json");
    let plan_path = f.root.path().join("plan.json");
    fs::write(&profile_path, serde_json::to_vec(&profile).unwrap()).unwrap();
    fs::write(&plan_path, p.to_json().unwrap()).unwrap();
    let run = |action: &str| {
        let output = Process::new(&binary)
            .args(["retire", action])
            .arg(&profile_path)
            .arg(&plan_path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(output.stderr.is_empty());
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    assert_eq!(run("status")["phase"], "pending");
    assert_eq!(run("cancel")["cancelled"], true);
    let status = run("status");
    assert_eq!(status["phase"], "cancelled");
    assert_eq!(status["observed"], "verified_present");
    assert_eq!(run("cancel")["cancelled"], true);
    assert!(f.object_path().exists());
    // The original publication remains valid after cancellation's action-slot advance.
    let mut ledger =
        CapacityLedger::open(&f.root.path().join("capacity"), &f.ledger.checkpoint()).unwrap();
    accounted::publish_accounted(
        &mut ledger,
        &f.store,
        &f.job,
        &f.original,
        &mut Cursor::new(b"target"),
    )
    .unwrap();
}

#[test]
fn known_missing_managed_anchors_are_not_recreated_by_reads_or_new_publications() {
    let mut f = Fixture::new();
    let anchor = f
        .root
        .path()
        .join("working/fences")
        .join(f.publication.object.sha256.as_str());
    fs::remove_file(&anchor).unwrap();
    assert!(f.store.read_object(&f.publication.object).is_err());
    assert!(!anchor.exists());
    let mut request = f.publication.clone();
    request.operation = key("new-writer");
    let publish = accounted::plan(&f.store, &request).unwrap();
    assert!(
        accounted::publish_accounted(
            &mut f.ledger,
            &f.store,
            &f.job,
            &publish,
            &mut Cursor::new(b"target")
        )
        .is_err()
    );
    assert!(!anchor.exists());
    assert!(f.object_path().exists());
    let f = Fixture::new();
    let anchor = f.root.path().join("working/fences").join(format!(
        "lifecycle-{}",
        f.publication.object.sha256.as_str()
    ));
    fs::remove_file(&anchor).unwrap();
    assert!(f.store.read_object(&f.publication.object).is_err());
    assert!(!anchor.exists());
    let mut f = Fixture::new();
    let dir = f
        .root
        .path()
        .join("working/lifecycle")
        .join(f.publication.object.sha256.as_str());
    fs::remove_file(dir.join("00.birth.json")).unwrap();
    fs::remove_dir(&dir).unwrap();
    let anchor = f.root.path().join("working/fences").join(format!(
        "lifecycle-{}",
        f.publication.object.sha256.as_str()
    ));
    fs::remove_file(&anchor).unwrap();
    let mut request = f.publication.clone();
    request.operation = key("after-prefix-loss");
    let publish = accounted::plan(&f.store, &request).unwrap();
    assert!(
        accounted::publish_accounted(
            &mut f.ledger,
            &f.store,
            &f.job,
            &publish,
            &mut Cursor::new(b"target")
        )
        .is_err()
    );
    assert!(!dir.exists());
    assert!(!anchor.exists());
    assert!(f.object_path().exists());
}
