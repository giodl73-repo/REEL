//! Actual handoff Journal + native controls/candidates + settled-input readback.
use super::*;
use common::{NOTE, artifact, returned};
use git_asset::annex::publication::{self, Publisher, Resources};
use git_asset_core::tree::NodeStore;
use git_asset_store::{
    existing::{Layout, Place, ReadCache, RequiredProtection},
    ledger::CapacityLedger,
    local::LocalStore,
};
use reel_workflow::handoff::{Journal, State as JournalState};
use reel_workflow_git_asset::{
    binding::{self, Binding, Capture},
    candidates::{Candidates, RETURN_PACKET},
    handoff::{self, Access, Configuration, Lifecycle},
    retention::{Budget, Seed},
};
fn budget() -> Budget {
    Budget {
        objects: 64,
        object_bytes: 1024 * 1024,
        total_bytes: 16 * 1024 * 1024,
        millis: 60000,
    }
}
fn nodes_limits() -> metadata::Limits {
    metadata::Limits {
        nodes: 20000,
        bytes: 8 * 1024 * 1024,
        database_bytes: 32 * 1024 * 1024,
        min_free_bytes: 65536,
    }
}
struct Offline {
    opens: std::rc::Rc<std::cell::Cell<usize>>,
}
impl existing::ByteSource for Offline {
    type Reader = std::io::Cursor<Vec<u8>>;
    fn open_verified(&mut self, _: &Object) -> existing::Result<Self::Reader> {
        self.opens.set(self.opens.get() + 1);
        Err(existing::Error::Missing)
    }
}
struct LoseAck<L> {
    inner: L,
    once: bool,
    exit_marker: Option<std::path::PathBuf>,
    exit_before_register: bool,
}
struct LoseGuard<'a, G> {
    inner: G,
    once: &'a mut bool,
    exit_marker: Option<&'a Path>,
    exit_before_register: bool,
}
impl<L: reel_workflow::handoff::Lifecycle> reel_workflow::handoff::Lifecycle for LoseAck<L> {
    type Guard<'a>
        = LoseGuard<'a, L::Guard<'a>>
    where
        Self: 'a;
    fn reopen<'a>(
        &'a mut self,
        ticket: &reel_workflow::handoff::Ticket,
    ) -> reel_workflow::Result<Self::Guard<'a>> {
        Ok(LoseGuard {
            inner: self.inner.reopen(ticket)?,
            once: &mut self.once,
            exit_marker: self.exit_marker.as_deref(),
            exit_before_register: self.exit_before_register,
        })
    }
}
impl<G: reel_workflow::handoff::ProtectedJob> reel_workflow::handoff::ProtectedJob
    for LoseGuard<'_, G>
{
    fn register(
        &mut self,
        t: &reel_workflow::handoff::Ticket,
        r: &reel_workflow::ReturnedResult,
    ) -> reel_workflow::Result<Artifact> {
        if self.exit_before_register {
            crash::abrupt(self.exit_marker.unwrap());
        }
        self.inner.register(t, r)
    }
    fn reconcile(
        &mut self,
        t: &reel_workflow::handoff::Ticket,
        r: &reel_workflow::ReturnedResult,
    ) -> reel_workflow::Result<Artifact> {
        self.inner.reconcile(t, r)
    }
    fn verify_retained(
        &mut self,
        t: &reel_workflow::handoff::Ticket,
        r: &reel_workflow::ReturnedResult,
        a: &Artifact,
    ) -> reel_workflow::Result<()> {
        self.inner.verify_retained(t, r, a)?;
        if let Some(marker) = self.exit_marker {
            crash::abrupt(marker);
        }
        if std::mem::replace(self.once, false) {
            return Err(reel_workflow::Error {
                kind: ErrorKind::NeedsRecovery,
                code: "synthetic-task-acknowledgement-lost".into(),
            });
        }
        Ok(())
    }
}
fn exercise(bare: bool, lose_ack: bool, uncertain: bool) {
    let f = Fixture::new(bare);
    let root = f.temp.path();
    let ns_path = f.profile.members[0].root.clone();
    let mut ns = Namespace::open(&ns_path, f.handle.minimum()).unwrap();
    for name in [
        "controls",
        "returns",
        "retained",
        "return-note",
        "return-packet",
        "creative",
        "ingress-packet",
        "ingress-preview",
        "ingress-request",
        "ingress-tool",
        "ingress-binding",
        "ingress-return",
        "ingress-plans",
    ] {
        ns.apply(
            key(&format!("enroll-{name}")),
            Command::Enroll {
                source: key(name),
                owner: key("owner"),
                class: if name == "creative" {
                    Kind::Branch
                } else {
                    Kind::Retention
                },
            },
            &f.nodes,
            &limits(),
        )
        .unwrap();
    }
    let control = Seed::prepare(
        &mut ns,
        key("controls"),
        key("owner"),
        key("prepare-controls"),
        key("adopt-controls"),
    )
    .unwrap();
    let returns = Seed::prepare(
        &mut ns,
        key("returns"),
        key("owner"),
        key("prepare-returns"),
        key("adopt-returns"),
    )
    .unwrap();
    let retained = Seed::prepare(
        &mut ns,
        key("retained"),
        key("owner"),
        key("prepare-retained"),
        key("adopt-retained"),
    )
    .unwrap();
    let (template, raw_return) = returned(&f.preview.commit, vec![f.artifact.clone()]);
    let packet = template.packet;
    let raw_packet = serde_json::to_vec_pretty(&packet).unwrap();
    let raw_preview = f.preview.encode().unwrap();
    let raw_request = b"original synthetic manual request\n";
    let raw_tool = b"qualified synthetic lifecycle";
    let controls = BTreeMap::from([
        (
            "packet".into(),
            artifact("control/packet", &raw_packet, "application/json"),
        ),
        (
            "request".into(),
            artifact("control/request", raw_request, "text/plain"),
        ),
        (
            "preview".into(),
            artifact("control/preview", &raw_preview, "application/json"),
        ),
        ("tool".into(), template.lifecycle_tool.clone()),
    ]);
    let staging = root.join("lifecycle-staging");
    fs::create_dir(&staging).unwrap();
    fs::write(staging.join(".git-asset-staging"), b"lifecycle stage").unwrap();
    let profile = publication::Profile {
        pool: f.profile.clone(),
        retained_place: key("pool"),
        staging: staging.clone(),
        staging_marker: Object::from_bytes(b"lifecycle stage"),
        budget: publication::Budget {
            object_bytes: 1024 * 1024,
            transfer_bytes: 16 * 1024 * 1024,
            millis: 60000,
        },
        allow_publish: true,
    };
    let witness = LocalStore::create(&root.join("lifecycle-witness"), key("witness")).unwrap();
    let mut ledger = CapacityLedger::create(
        &root.join("lifecycle-ledger"),
        key("capacity"),
        key("epoch"),
        65536,
    )
    .unwrap();
    let probe = ledger
        .begin_probe(key("probe"))
        .unwrap()
        .reference()
        .clone();
    ledger
        .observe(key("observe"), &probe, fs2::available_space(root).unwrap())
        .unwrap();
    let job = ledger
        .reserve(key("job"), key("owner"), 32 * 1024 * 1024)
        .unwrap()
        .reference()
        .clone();
    let meta_root = root.join("lifecycle-metadata");
    let mut writer = MetadataStore::create(&meta_root, nodes_limits()).unwrap();
    let (tree, _) = Tree::build(
        &binding::resources(&packet, &f.preview, &controls).unwrap(),
        &mut writer,
        TreeLimits::default(),
    )
    .unwrap();
    let claim = Handle::prepare(
        &mut ns,
        Select {
            prepare: key("prepare-active"),
            adopt: key("adopt-active"),
            source: key("attempt"),
            owner: key("owner"),
            expected_revision: 1,
            selection: Selection {
                root: tree.root().clone(),
                context: Object::from_bytes(&raw_preview).sha256,
            },
        },
    )
    .unwrap();
    let binding = Binding::capture(Capture {
        binding_id: "bindings/attempt-1",
        packet: &packet,
        original_packet: &raw_packet,
        attempt: "attempt-1",
        preview: &f.preview,
        claim: &claim,
        controls,
        control_seed: &control,
        result_seed: &returns,
        retained_seed: &retained,
    })
    .unwrap();
    let ticket = binding.ticket(packet).unwrap();
    let control_request = binding.control_selection(&mut writer).unwrap();
    let admission = publication::Admission {
        namespace: ns.identity().clone(),
        witness: witness.identity().clone(),
        capacity: ledger.identity().clone(),
        job: job.clone(),
    };
    let incoming = client::freeze(
        root,
        &binding,
        &profile,
        &admission,
        &mut writer,
        vec![
            (
                "packet",
                binding.controls()["packet"].clone(),
                raw_packet.as_slice(),
            ),
            (
                "preview",
                binding.controls()["preview"].clone(),
                raw_preview.as_slice(),
            ),
            (
                "request",
                binding.controls()["request"].clone(),
                raw_request.as_slice(),
            ),
            (
                "tool",
                binding.controls()["tool"].clone(),
                raw_tool.as_slice(),
            ),
            ("binding", binding.artifact(), binding.control()),
        ],
    );
    let cp = writer.finish().unwrap();
    drop(writer);
    let mut metadata = MetadataReader::open(&meta_root, &cp).unwrap();
    let mut ingress_publisher = Publisher::new(profile.clone()).unwrap();
    let ingress_held = client::publish(
        &binding,
        &profile,
        &incoming,
        &mut ingress_publisher,
        &mut Resources {
            namespace: &mut ns,
            metadata: &mut metadata,
            limits: limits(),
            witness: &witness,
            ledger: &mut ledger,
        },
    );
    let controls_held = control
        .admit(
            &f.profile,
            &control_request,
            &mut metadata,
            &limits(),
            budget(),
        )
        .unwrap();
    let mut input =
        GuardedInputs::new(f.profile.clone(), &f.source, &f.preview, 1024 * 1024).unwrap();
    input.read(&f.artifact, f.artifact.bytes).unwrap();
    input.admit_claim(&claim, &mut metadata, &limits()).unwrap();
    drop(input);
    drop(controls_held);
    drop(ingress_held);
    drop(incoming);
    // Initial manual waiting checks all controls and inputs; no result exists yet.
    let opens = std::rc::Rc::new(std::cell::Cell::new(0));
    let cfg = Configuration {
        expected: ticket.clone(),
        tool: template.lifecycle_tool,
        profile: f.profile.clone(),
        controls: budget(),
        retained: reel_workflow_git_asset::retention::ProtectionBudget {
            objects: 256,
            object_bytes: 64 * 1024 * 1024 * 1024,
            total_bytes: 1024 * 1024 * 1024 * 1024,
            millis: 60000,
        },
        access: Access::Active {
            snapshot: f.source,
            read_budget: 4 * 1024 * 1024,
        },
    };
    let mut runtime = Lifecycle::new(
        cfg,
        Publisher::new(profile.clone()).unwrap(),
        Resources {
            namespace: &mut ns,
            metadata: &mut metadata,
            limits: limits(),
            witness: &witness,
            ledger: &mut ledger,
        },
        Offline {
            opens: opens.clone(),
        },
    )
    .unwrap();
    let path = root.join("task-journal");
    let mut journal = Journal::create(&path, ticket.clone(), &mut runtime).unwrap();
    assert_eq!(journal.state(), JournalState::Waiting);
    assert_eq!(opens.get(), 0);
    let cfg = runtime.into_configuration();
    crash::active_exit(
        root,
        &cfg,
        &profile,
        &cp,
        &ns.checkpoint(),
        &ledger.checkpoint(),
        crash::Phase::Waiting,
    );
    assert_eq!(Journal::open(&path).unwrap().state(), JournalState::Waiting);
    drop(metadata);
    // Freeze original return and all SDK plans, then retain those control blobs.
    let set = Candidates::bind(&ticket, &raw_return, &binding.return_packet_id()).unwrap();
    let mut writer = MetadataStore::open(&meta_root, &cp).unwrap();
    let provenance = writer.put(set.provenance()).unwrap();
    let mut plans = BTreeMap::new();
    for (role, a) in set.expected() {
        let source = if role == RETURN_PACKET {
            "return-packet"
        } else {
            "return-note"
        };
        let request = publication::Request {
            operation: key(source),
            owner: key("owner"),
            source: key(source),
            expected_revision: 1,
            candidate: publication::Candidate {
                logical_id: a.logical_id.clone(),
                object: binding::object(a).unwrap(),
                provenance: provenance.clone(),
            },
        };
        let frozen = publication::freeze(&request, &mut writer, TreeLimits::default()).unwrap();
        plans.insert(
            role.clone(),
            publication::Plan::new(
                &profile,
                request,
                frozen,
                publication::Admission {
                    namespace: ns.identity().clone(),
                    witness: witness.identity().clone(),
                    capacity: ledger.identity().clone(),
                    job: job.clone(),
                },
            )
            .unwrap(),
        );
    }
    let prepared = set.prepare(plans).unwrap();
    let plan_bytes = prepared.control().to_vec();
    let return_request = returns
        .selection(
            &BTreeMap::from([
                (
                    LogicalPath::parse("packet").unwrap(),
                    Object::from_bytes(&raw_return),
                ),
                (
                    LogicalPath::parse("plans").unwrap(),
                    Object::from_bytes(&plan_bytes),
                ),
            ]),
            binding.pin().sha256,
            &mut writer,
        )
        .unwrap();
    handoff::final_selection(&binding, &prepared, &mut writer).unwrap();
    let incoming = client::freeze(
        root,
        &binding,
        &profile,
        &admission,
        &mut writer,
        vec![
            (
                "return",
                artifact("control/raw-return", &raw_return, "application/json"),
                raw_return.as_slice(),
            ),
            (
                "plans",
                artifact("control/frozen-plans", &plan_bytes, "application/json"),
                plan_bytes.as_slice(),
            ),
        ],
    );
    let cp = writer.finish().unwrap();
    drop(writer);
    let mut metadata = MetadataReader::open(&meta_root, &cp).unwrap();
    let ingress_held = client::publish(
        &binding,
        &profile,
        &incoming,
        &mut ingress_publisher,
        &mut Resources {
            namespace: &mut ns,
            metadata: &mut metadata,
            limits: limits(),
            witness: &witness,
            ledger: &mut ledger,
        },
    );
    drop(
        returns
            .admit(
                &f.profile,
                &return_request,
                &mut metadata,
                &limits(),
                budget(),
            )
            .unwrap(),
    );
    drop(ingress_held);
    drop(incoming);
    let produced = root.join("produced");
    fs::create_dir(&produced).unwrap();
    fs::create_dir(produced.join("objects")).unwrap();
    fs::write(
        produced
            .join("objects")
            .join(Object::from_bytes(NOTE).sha256.as_str()),
        NOTE,
    )
    .unwrap();
    let source = ReadCache::new(existing::Profile {
        places: vec![Place {
            root: produced.clone(),
            layout: Layout::Flat,
        }],
        required: RequiredProtection::PreventMutation,
        limits: existing::Limits {
            object_bytes: 1024 * 1024,
            total_bytes: 8 * 1024 * 1024,
            requests: 16,
            live_readers: 8,
            verify_millis: 30000,
        },
    })
    .unwrap();
    if uncertain {
        crash::active_exit(
            root,
            &cfg,
            &profile,
            &cp,
            &ns.checkpoint(),
            &ledger.checkpoint(),
            crash::Phase::ImportPending(Box::new(prepared.candidates().result().clone())),
        );
        crash::active_exit(
            root,
            &cfg,
            &profile,
            &cp,
            &ns.checkpoint(),
            &ledger.checkpoint(),
            crash::Phase::NativeImport,
        );
        journal = Journal::open(&path).unwrap();
        assert_eq!(journal.state(), JournalState::ImportPending);
        let opens = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut runtime = Lifecycle::new(
            cfg,
            Publisher::new(profile.clone()).unwrap(),
            Resources {
                namespace: &mut ns,
                metadata: &mut metadata,
                limits: limits(),
                witness: &witness,
                ledger: &mut ledger,
            },
            Offline {
                opens: opens.clone(),
            },
        )
        .unwrap();
        assert_eq!(journal.resume(&mut runtime).unwrap_err().code, "uncertain");
        assert_eq!(journal.state(), JournalState::ImportPending);
        assert_eq!(journal.resume(&mut runtime).unwrap_err().code, "uncertain");
        let cfg = runtime.into_configuration();
        assert_eq!(opens.get(), 0);
        crash::active_exit(
            root,
            &cfg,
            &profile,
            &cp,
            &ns.checkpoint(),
            &ledger.checkpoint(),
            crash::Phase::Uncertain,
        );
        assert_eq!(
            Journal::open(&path).unwrap().state(),
            JournalState::ImportPending
        );
        assert!(
            ns.source(&key("retained"))
                .unwrap()
                .unwrap()
                .current
                .is_none()
        );
        assert!(
            ns.source(&key("attempt"))
                .unwrap()
                .unwrap()
                .current
                .is_some()
        );
        assert!(
            ns.source(&key("creative"))
                .unwrap()
                .unwrap()
                .current
                .is_none()
        );
        return;
    }
    if !bare {
        crash::active_exit(
            root,
            &cfg,
            &profile,
            &cp,
            &ns.checkpoint(),
            &ledger.checkpoint(),
            crash::Phase::ImportPending(Box::new(prepared.candidates().result().clone())),
        );
        journal = Journal::open(&path).unwrap();
        assert_eq!(journal.state(), JournalState::ImportPending);
    }
    let runtime = Lifecycle::new(
        cfg,
        Publisher::new(profile.clone()).unwrap(),
        Resources {
            namespace: &mut ns,
            metadata: &mut metadata,
            limits: limits(),
            witness: &witness,
            ledger: &mut ledger,
        },
        source,
    )
    .unwrap();
    let result = prepared.candidates().result().clone();
    let mut wrapped = LoseAck {
        inner: runtime,
        once: lose_ack,
        exit_marker: None,
        exit_before_register: false,
    };
    let imported = if journal.state() == JournalState::ImportPending {
        assert_eq!(journal.result(), Some(&result));
        journal.resume(&mut wrapped)
    } else {
        journal.import(result, &mut wrapped)
    };
    if lose_ack {
        assert_eq!(
            imported.unwrap_err().code,
            "synthetic-task-acknowledgement-lost"
        );
        assert_eq!(journal.state(), JournalState::ImportPending);
    } else {
        imported.unwrap();
        assert_eq!(journal.state(), JournalState::Retained);
    }
    let mut cfg = wrapped.inner.into_configuration();
    cfg.access = Access::RetainedOnly;
    assert_eq!(
        claim
            .cancel(&mut ns, key("settle-input-job"), &metadata, &limits())
            .unwrap()
            .state,
        State::Historical
    );
    assert!(
        ns.source(&key("creative"))
            .unwrap()
            .unwrap()
            .current
            .is_none()
    );
    fs::rename(&produced, root.join("produced-offline")).unwrap();
    fs::rename(&staging, root.join("stage-offline")).unwrap();
    fs::rename(
        root.join(if bare { "project.git" } else { "project" }),
        root.join("git-offline"),
    )
    .unwrap();
    let ns_before = ns.checkpoint();
    let ledger_before = ledger.checkpoint();
    // Reopen in a separate process while parent handles remain live. Its exit
    // must release only live guards, without dropping durable task retention.
    crash::retained_exit(
        root,
        &cfg,
        &profile,
        &cp,
        &ns.checkpoint(),
        &ledger.checkpoint(),
    );
    let disk = Journal::open(&path).unwrap();
    assert_eq!(disk.state(), journal.state());
    assert_eq!(disk.acknowledgement(), journal.acknowledgement());
    let opens = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut runtime = Lifecycle::new(
        cfg,
        Publisher::new(profile).unwrap(),
        Resources {
            namespace: &mut ns,
            metadata: &mut metadata,
            limits: limits(),
            witness: &witness,
            ledger: &mut ledger,
        },
        Offline {
            opens: opens.clone(),
        },
    )
    .unwrap();
    journal.resume(&mut runtime).unwrap();
    drop(runtime);
    assert_eq!(opens.get(), 0);
    assert_eq!(ns.checkpoint(), ns_before);
    assert_eq!(ledger.checkpoint(), ledger_before);
    assert_eq!(journal.state(), JournalState::Retained);
    assert!(
        ns.source(&key("attempt"))
            .unwrap()
            .unwrap()
            .current
            .is_none()
    );
}
#[test]
#[ignore = "explicit native tools; new synthetic ordinary pool; no provider/render"]
fn ordinary_handoff_retained_completion_survives_input_settlement() {
    exercise(false, true, false)
}
#[test]
#[ignore = "explicit native tools; new synthetic bare pool; no provider/render"]
fn bare_handoff_retained_completion_survives_input_settlement() {
    exercise(true, false, false)
}

#[test]
#[ignore = "explicit native tools; fresh synthetic pool; unknown effect must hold"]
fn uncertain_native_effect_never_completes_workflow_or_replays_publication() {
    exercise(false, false, true)
}

#[path = "lifecycle/crash.rs"]
mod crash;

#[path = "lifecycle/client.rs"]
mod client;
