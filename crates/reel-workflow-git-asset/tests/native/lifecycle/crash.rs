//! Owned synthetic process recipe; not a product enrollment/control schema.
use super::*;
use git_asset_store::{ledger, source_claims};
use reel_workflow::handoff::Ticket;
use serde::{Deserialize, Serialize};
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) enum Phase {
    Waiting,
    ImportPending(Box<reel_workflow::ReturnedResult>),
    Retained,
    Uncertain,
    NativeImport,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Case {
    phase: Phase,
    ticket: Ticket,
    tool: Artifact,
    publication: serde_json::Value,
    metadata: metadata::Checkpoint,
    namespace: source_claims::Checkpoint,
    ledger: ledger::Checkpoint,
}
fn pool_json(p: &annex::Profile) -> serde_json::Value {
    json!({"git":p.git,"annex":p.annex,"repository":p.repository,"bare":p.bare,
        "uuid":p.uuid,"configuration":p.configuration,"tools":p.tools,
        "empty_config":p.empty_config,"empty_hooks":p.empty_hooks,"temporary":p.temporary,
        "command_millis":p.command_millis,"output_bytes":p.output_bytes,"limits":{"object_bytes":p.limits.object_bytes,"total_bytes":p.limits.total_bytes,
        "requests":p.limits.requests,"live_readers":p.limits.live_readers,"verify_millis":p.limits.verify_millis}})
}
fn publication_json(p: &publication::Profile) -> serde_json::Value {
    json!({"pool":{"pool":pool_json(&p.pool.pool),"binding":p.pool.binding,
        "members":p.pool.members.iter().map(|m| json!({"root":m.root,"minimum":m.minimum})).collect::<Vec<_>>()},
        "retained_place":p.retained_place,"staging":p.staging,"staging_marker":p.staging_marker,
        "budget":p.budget,"allow_publish":p.allow_publish})
}
pub(super) fn retained_exit(
    root: &Path,
    cfg: &Configuration,
    profile: &publication::Profile,
    metadata: &metadata::Checkpoint,
    namespace: &source_claims::Checkpoint,
    ledger: &ledger::Checkpoint,
) {
    assert!(matches!(cfg.access, Access::RetainedOnly));
    active_exit(
        root,
        cfg,
        profile,
        metadata,
        namespace,
        ledger,
        Phase::Retained,
    );
}
pub(super) fn active_exit(
    root: &Path,
    cfg: &Configuration,
    profile: &publication::Profile,
    metadata: &metadata::Checkpoint,
    namespace: &source_claims::Checkpoint,
    ledger: &ledger::Checkpoint,
    phase: Phase,
) {
    let suffix = match phase {
        Phase::Waiting => "waiting",
        Phase::ImportPending(_) => "pending",
        Phase::Retained => "retained",
        Phase::Uncertain => "uncertain",
        Phase::NativeImport => "native-import",
    };
    let before_namespace = Namespace::open(&profile.pool.members[0].root, namespace)
        .unwrap()
        .checkpoint();
    let before_ledger = CapacityLedger::open(&root.join("lifecycle-ledger"), ledger)
        .unwrap()
        .checkpoint();
    let native_import = matches!(phase, Phase::NativeImport);
    let before_input = Namespace::open(&profile.pool.members[0].root, namespace)
        .unwrap()
        .source(&key("attempt"))
        .unwrap()
        .unwrap()
        .current;
    let case = Case {
        phase,
        ticket: cfg.expected.clone(),
        tool: cfg.tool.clone(),
        publication: publication_json(profile),
        metadata: metadata.clone(),
        namespace: before_namespace.clone(),
        ledger: before_ledger.clone(),
    };
    let path = root.join(format!("{suffix}-exit-case.json"));
    let bytes = serde_json::to_vec(&case).unwrap();
    assert!(bytes.len() <= 128 * 1024);
    fs::write(&path, &bytes).unwrap();
    let stdout = tempfile::tempfile().unwrap();
    let stderr = tempfile::tempfile().unwrap();
    let mut child = Process::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "lifecycle::crash::retained_exit_worker",
            "--ignored",
            "--nocapture",
        ])
        .env("REEL_PUBLIC_RETAINED_EXIT", &path)
        .stdin(Stdio::null())
        .stdout(stdout.try_clone().unwrap())
        .stderr(stderr.try_clone().unwrap())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("owned retained-exit child timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if status.code() != Some(83) {
        use std::io::{Seek, SeekFrom};
        fn tail(mut f: fs::File) -> String {
            f.seek(SeekFrom::End(0)).unwrap();
            let n = f.stream_position().unwrap().min(16384);
            f.seek(SeekFrom::End(-(n as i64))).unwrap();
            let mut b = Vec::new();
            f.take(n).read_to_end(&mut b).unwrap();
            String::from_utf8_lossy(&b).into_owned()
        }
        panic!("owned child {status}: {} {}", tail(stdout), tail(stderr));
    }
    assert_eq!(
        fs::read(root.join(format!("{suffix}-exit-reached"))).unwrap(),
        b"real retained guards held before workflow acknowledgement\n"
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    if native_import {
        assert_eq!(
            Journal::open(&root.join("task-journal")).unwrap().state(),
            JournalState::ImportPending
        );
        assert_eq!(
            Namespace::open(&profile.pool.members[0].root, namespace)
                .unwrap()
                .source(&key("attempt"))
                .unwrap()
                .unwrap()
                .current,
            before_input
        );
        return; // Native intent/effect legitimately advanced the other journals.
    }
    // Reopen disk state, rather than trusting the parent's cached checkpoint.
    assert_eq!(
        Namespace::open(&profile.pool.members[0].root, namespace)
            .unwrap()
            .checkpoint(),
        before_namespace
    );
    assert_eq!(
        CapacityLedger::open(&root.join("lifecycle-ledger"), ledger)
            .unwrap()
            .checkpoint(),
        before_ledger
    );
}
#[test]
#[ignore = "only performs work when launched by the owned native parent fixture"]
fn retained_exit_worker() {
    let Some(path) = std::env::var_os("REEL_PUBLIC_RETAINED_EXIT") else {
        return;
    };
    let path = Path::new(&path);
    let name = path.file_name().unwrap().to_str().unwrap();
    assert!(matches!(
        name,
        "waiting-exit-case.json"
            | "pending-exit-case.json"
            | "retained-exit-case.json"
            | "uncertain-exit-case.json"
            | "native-import-exit-case.json"
    ));
    let root = path.parent().unwrap();
    assert!(
        root.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("reel-public-native-fixture-")
    );
    let bytes = fs::read(path).unwrap();
    assert!(bytes.len() <= 128 * 1024);
    let case: Case = serde_json::from_slice(&bytes).unwrap();
    let profile: publication::Profile = serde_json::from_value(case.publication).unwrap();
    let mut namespace = Namespace::open(&profile.pool.members[0].root, &case.namespace).unwrap();
    let mut metadata =
        MetadataReader::open(&root.join("lifecycle-metadata"), &case.metadata).unwrap();
    let mut ledger = CapacityLedger::open(&root.join("lifecycle-ledger"), &case.ledger).unwrap();
    let witness = LocalStore::open(&root.join("lifecycle-witness")).unwrap();
    let cfg = Configuration {
        expected: case.ticket.clone(),
        tool: case.tool,
        profile: profile.pool.clone(),
        controls: budget(),
        retained: reel_workflow_git_asset::retention::ProtectionBudget {
            objects: 256,
            object_bytes: 64 * 1024 * 1024 * 1024,
            total_bytes: 1024 * 1024 * 1024 * 1024,
            millis: 60000,
        },
        access: match case.phase {
            Phase::Retained => Access::RetainedOnly,
            _ => {
                let repository = Repository::new(
                    profile.pool.pool.git.clone(),
                    root.join(if profile.pool.pool.bare {
                        "project.git"
                    } else {
                        "project"
                    }),
                    GitLimits::default(),
                )
                .unwrap();
                Access::Active {
                    snapshot: Snapshot::capture(
                        &repository,
                        &case.ticket.packet.source_commit,
                        &["records/item.asset".into()],
                        &["definitions".into()],
                    )
                    .unwrap(),
                    read_budget: 4 * 1024 * 1024,
                }
            }
        },
    };
    let opens = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut runtime = Lifecycle::new(
        cfg,
        Publisher::new(profile.clone()).unwrap(),
        Resources {
            namespace: &mut namespace,
            metadata: &mut metadata,
            limits: limits(),
            witness: &witness,
            ledger: &mut ledger,
        },
        Offline { opens },
    )
    .unwrap();
    let suffix = match case.phase {
        Phase::Waiting => "waiting",
        Phase::ImportPending(_) => "pending",
        Phase::Retained => "retained",
        Phase::Uncertain => "uncertain",
        Phase::NativeImport => "native-import",
    };
    let marker = root.join(format!("{suffix}-exit-reached"));
    if matches!(case.phase, Phase::NativeImport) {
        use reel_workflow::handoff::Lifecycle as _;
        let journal = Journal::open(&root.join("task-journal")).unwrap();
        assert_eq!(journal.state(), JournalState::ImportPending);
        let _held = runtime.reopen(journal.ticket()).unwrap();
        native_import_exit(
            root,
            &case.metadata,
            &case.namespace,
            &case.ledger,
            &profile,
            &journal,
            &marker,
        );
    }
    if matches!(case.phase, Phase::Uncertain) {
        use reel_workflow::handoff::Lifecycle as _;
        let mut journal = Journal::open(&root.join("task-journal")).unwrap();
        assert_eq!(journal.state(), JournalState::ImportPending);
        assert_eq!(journal.resume(&mut runtime).unwrap_err().code, "uncertain");
        assert_eq!(journal.state(), JournalState::ImportPending);
        let _held = runtime.reopen(journal.ticket()).unwrap();
        abrupt(&marker);
    }
    if matches!(case.phase, Phase::Waiting) {
        use reel_workflow::handoff::Lifecycle as _;
        let journal = Journal::open(&root.join("task-journal")).unwrap();
        assert_eq!(journal.state(), JournalState::Waiting);
        let _held = runtime.reopen(journal.ticket()).unwrap();
        abrupt(&marker);
    }
    let mut wrapped = LoseAck {
        inner: runtime,
        once: false,
        exit_marker: Some(marker),
        exit_before_register: matches!(case.phase, Phase::ImportPending(_)),
    };
    let mut journal = Journal::open(&root.join("task-journal")).unwrap();
    if let Phase::ImportPending(result) = case.phase {
        journal.import(*result, &mut wrapped).unwrap();
    } else {
        journal.resume(&mut wrapped).unwrap();
    }
    panic!("expected abrupt exit before workflow acknowledgement");
}

pub(super) fn abrupt(marker: &Path) -> ! {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(marker)
        .unwrap();
    file.write_all(b"real retained guards held before workflow acknowledgement\n")
        .unwrap();
    file.sync_all().unwrap();
    // No Rust destructors, including the actual native/source guards.
    std::process::exit(83);
}

// Actual REEL input/control guards span the original SDK native effect. Separate
// handles allow the producer to advance its own source, not the held input Job.
fn native_import_exit(
    root: &Path,
    cp: &metadata::Checkpoint,
    minimum: &source_claims::Checkpoint,
    capacity: &ledger::Checkpoint,
    profile: &publication::Profile,
    journal: &Journal,
    marker: &Path,
) -> ! {
    let mut namespace = Namespace::open(&profile.pool.members[0].root, minimum).unwrap();
    let mut metadata = MetadataReader::open(&root.join("lifecycle-metadata"), cp).unwrap();
    let mut ledger = CapacityLedger::open(&root.join("lifecycle-ledger"), capacity).unwrap();
    let witness = LocalStore::open(&root.join("lifecycle-witness")).unwrap();
    let mut reader = managed::Reader::new(profile.pool.clone()).unwrap();
    let mut original = reader
        .open(&binding::object(&journal.ticket().native_binding).unwrap())
        .unwrap();
    let mut bytes = vec![];
    original
        .by_ref()
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    let binding = Binding::decode(
        &bytes,
        &binding::object(&journal.ticket().native_binding).unwrap(),
    )
    .unwrap();
    let controls = binding
        .result_seed()
        .original(
            &profile.pool,
            &binding.pin().sha256,
            &mut metadata,
            &limits(),
            budget(),
        )
        .unwrap();
    let packet = controls
        .bytes(&git_asset_core::LogicalPath::parse("packet").unwrap())
        .unwrap();
    let plans_path = git_asset_core::LogicalPath::parse("plans").unwrap();
    let candidates =
        Candidates::bind(journal.ticket(), packet, &binding.return_packet_id()).unwrap();
    let prepared = candidates
        .restore(
            controls.bytes(&plans_path).unwrap(),
            &controls.entries()[&plans_path],
        )
        .unwrap();
    assert_eq!(journal.result(), Some(prepared.candidates().result()));
    let mut producer = ReadCache::new(existing::Profile {
        places: vec![Place {
            root: root.join("produced"),
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
    let mut publisher = Publisher::new(profile.clone()).unwrap();
    let outcome = publisher.apply_hook(
        &prepared.plans()["output/note"],
        &mut Resources {
            namespace: &mut namespace,
            metadata: &mut metadata,
            limits: limits(),
            witness: &witness,
            ledger: &mut ledger,
        },
        &mut producer,
        |stage| {
            if stage == publication::Stage::Imported {
                abrupt(marker);
            }
            Ok(())
        },
    );
    panic!(
        "expected process exit after real native effect, got {}",
        outcome.is_ok()
    );
}
