//! Real Git snapshots and native annex POINTERS. No native store or payload
//! availability is claimed by these metadata fixtures.
use reel_workflow::*;
use reel_workflow_native::{SnapshotInputs, SnapshotProfile, Tool};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tig_core::{LogicalPath, Object};
use tig_git::{annex::identity::Key, source::native::annex::Binding};

fn git_path() -> PathBuf {
    if let Some(path) = std::env::var_os("REEL_TEST_GIT") {
        return PathBuf::from(path);
    }
    for folder in std::env::split_paths(&std::env::var_os("PATH").unwrap()) {
        let path = folder.join(if cfg!(windows) { "git.exe" } else { "git" });
        if path.is_file() {
            return path;
        }
    }
    panic!("Set REEL_TEST_GIT to an installed Git executable")
}
fn git(root: &Path, args: &[&str]) -> String {
    let result = Command::new(git_path())
        .args(["-c", "core.autocrlf=false"])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().into()
}
fn write(root: &Path, path: &str, value: serde_json::Value) -> Object {
    let raw = serde_json::to_vec(&value).unwrap();
    fs::write(root.join(path), &raw).unwrap();
    Object::from_bytes(&raw)
}
struct Fixture {
    temp: tempfile::TempDir,
    profile: SnapshotProfile,
    packet: Packet,
}
impl Fixture {
    fn new() -> Self {
        Self::with_object(Object::from_bytes(b"synthetic input"))
    }
    fn with_object(selected: Object) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        git(root, &["init", "-b", "main"]);
        git(root, &["config", "user.name", "Fixture"]);
        git(root, &["config", "user.email", "fixture@example.invalid"]);
        for path in ["records", "definitions", "payloads"] {
            fs::create_dir(root.join(path)).unwrap();
        }
        fs::write(root.join(".gitattributes"), "* -text\n").unwrap();
        let schema = write(root, "definitions/schema.json", json!({"type":"object"}));
        let definition = write(
            root,
            "definitions/input.json",
            json!({"schema":"tig.definition.design-1","id":"types/input","authored_schema":schema,
            "authoring":{"properties":["text"],"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
        );
        let preview = Object::from_bytes(b"preview");
        write(
            root,
            "records/input.tig",
            json!({"schema":"tig.instance.design-1","id":"input/source",
            "definition":{"id":"types/input","sha256":definition.sha256,"bytes":definition.bytes},
            "authored":{"text":"PRIVATE SYNTHETIC TEXT"},"selected":selected,
            "streams":[{"slot":"preview","role":"review","group":"draft","object":preview}],"status":{},"metadata":{}}),
        );
        fs::write(
            root.join("payloads/source.bin"),
            Key::from_object(&selected).unlocked_pointer(),
        )
        .unwrap();
        fs::write(
            root.join("payloads/preview.bin"),
            Key::from_object(&preview).unlocked_pointer(),
        )
        .unwrap();
        // Same bytes at another leaf do not authorize a changed declaration.
        fs::write(
            root.join("payloads/alias.bin"),
            Key::from_object(&selected).unlocked_pointer(),
        )
        .unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "synthetic source"]);
        let commit = git(root, &["rev-parse", "HEAD"]);
        let git = git_path();
        let raw = fs::read(&git).unwrap();
        let profile = SnapshotProfile {
            schema: "reel.workflow-native-snapshot.v1".into(),
            git: Tool {
                path: git,
                sha256: digest(&raw),
                bytes: raw.len() as u64,
            },
            repository: root.into(),
            commit: commit.clone(),
            records: vec!["records/input.tig".into()],
            definitions: vec!["definitions".into()],
            bindings: BTreeMap::from([(
                "input/source".into(),
                Binding {
                    selected: LogicalPath::parse("payloads/source.bin").unwrap(),
                    streams: BTreeMap::from([(
                        "preview".into(),
                        LogicalPath::parse("payloads/preview.bin").unwrap(),
                    )]),
                },
            )]),
            input_streams: BTreeMap::from([("input/source".into(), None)]),
        };
        let task = Task {
            id: "review".into(),
            native_inputs: Some(Self::declaration(&profile)),
            operation: Operation::ReviewFinding,
            tool: None,
            language: Some("es".into()),
            prompt: "Review the synthetic input.".into(),
            inputs: vec![Artifact {
                logical_id: "input/source".into(),
                sha256: selected.sha256.as_str().into(),
                bytes: selected.bytes,
                media_type: "application/json".into(),
            }],
            depends_on: vec![],
            outputs: BTreeMap::from([(
                "note".into(),
                OutputContract {
                    media_type: "text/plain".into(),
                    max_bytes: 100,
                    required: true,
                },
            )]),
        };
        let packet = make_packet(
            &Plan {
                schema: PLAN_SCHEMA.into(),
                project: "synthetic-native".into(),
                source_commit: commit,
                tasks: vec![task],
            },
            "review",
            BTreeMap::new(),
        )
        .unwrap();
        Self {
            temp,
            profile,
            packet,
        }
    }
    fn declaration(profile: &SnapshotProfile) -> Artifact {
        let raw = profile.declaration_bytes().unwrap();
        Artifact {
            logical_id: "control/native-input-bindings".into(),
            sha256: digest(&raw),
            bytes: raw.len() as u64,
            media_type: "application/json".into(),
        }
    }
    fn repin(&mut self) {
        let mut task = self.packet.task.clone();
        task.native_inputs = Some(Self::declaration(&self.profile));
        self.packet = make_packet(
            &Plan {
                schema: PLAN_SCHEMA.into(),
                project: self.packet.project.clone(),
                source_commit: self.profile.commit.clone(),
                tasks: vec![task],
            },
            "review",
            BTreeMap::new(),
        )
        .unwrap();
    }
}

#[test]
fn exact_historical_record_and_leaf_survive_dirty_worktree_and_newer_commit() {
    let fixture = Fixture::new();
    let original = SnapshotInputs::open(&fixture.packet, &fixture.profile).unwrap();
    assert_eq!(original.input_count(), 1);
    fs::write(
        fixture.temp.path().join("payloads/source.bin"),
        Key::from_object(&Object::from_bytes(b"changed")).unlocked_pointer(),
    )
    .unwrap();
    SnapshotInputs::open(&fixture.packet, &fixture.profile).unwrap();
    git(fixture.temp.path(), &["add", "."]);
    git(
        fixture.temp.path(),
        &["commit", "-m", "mismatched later pointer"],
    );
    assert_eq!(
        SnapshotInputs::open(&fixture.packet, &fixture.profile)
            .unwrap()
            .checkpoint(),
        original.checkpoint()
    );
    let mut later = fixture.profile.clone();
    later.commit = git(fixture.temp.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        SnapshotInputs::open(&fixture.packet, &later)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Stale
    );
}

#[test]
fn bare_git_history_restores_binding_without_original_checkout_files() {
    let mut fixture = Fixture::new();
    let archive = tempfile::tempdir().unwrap();
    let bare = archive.path().join("history.git");
    git(
        fixture.temp.path(),
        &[
            "clone",
            "--bare",
            fixture.temp.path().to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    fs::write(
        fixture.temp.path().join("records/input.tig"),
        b"unavailable authored checkout",
    )
    .unwrap();
    fs::write(
        fixture.temp.path().join("payloads/source.bin"),
        b"unavailable pointer checkout",
    )
    .unwrap();
    fixture.profile.repository = bare;
    SnapshotInputs::open(&fixture.packet, &fixture.profile).unwrap();
}

#[test]
fn byte_identical_annex_alias_cannot_replace_the_task_bound_leaf() {
    let mut fixture = Fixture::new();
    fixture
        .profile
        .bindings
        .get_mut("input/source")
        .unwrap()
        .selected = LogicalPath::parse("payloads/alias.bin").unwrap();
    assert_eq!(
        SnapshotInputs::open(&fixture.packet, &fixture.profile)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Conflict
    );
    let key = fixture.packet.task_key.clone();
    fixture.repin();
    assert_ne!(fixture.packet.task_key, key);
    SnapshotInputs::open(&fixture.packet, &fixture.profile).unwrap(); // New explicit authored declaration.
}

#[test]
fn missing_leaf_and_incomplete_stream_binding_refuse_after_explicit_repin() {
    let mut fixture = Fixture::new();
    fixture
        .profile
        .bindings
        .get_mut("input/source")
        .unwrap()
        .selected = LogicalPath::parse("payloads/missing.bin").unwrap();
    fixture.repin();
    assert!(SnapshotInputs::open(&fixture.packet, &fixture.profile).is_err());
    fixture
        .profile
        .bindings
        .get_mut("input/source")
        .unwrap()
        .selected = LogicalPath::parse("payloads/source.bin").unwrap();
    fixture
        .profile
        .bindings
        .get_mut("input/source")
        .unwrap()
        .streams
        .clear();
    fixture.repin();
    assert!(SnapshotInputs::open(&fixture.packet, &fixture.profile).is_err());
}

#[test]
fn explicit_stream_selection_binds_the_stream_object_and_invalidates_old_results() {
    let mut fixture = Fixture::new();
    fixture
        .profile
        .input_streams
        .insert("input/source".into(), Some("preview".into()));
    fixture.repin();
    assert_eq!(
        SnapshotInputs::open(&fixture.packet, &fixture.profile)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Conflict
    );
    let object = Object::from_bytes(b"preview");
    let mut task = fixture.packet.task.clone();
    task.inputs[0].sha256 = object.sha256.as_str().into();
    task.inputs[0].bytes = object.bytes;
    fixture.packet = make_packet(
        &Plan {
            schema: PLAN_SCHEMA.into(),
            project: fixture.packet.project.clone(),
            source_commit: fixture.profile.commit.clone(),
            tasks: vec![task],
        },
        "review",
        BTreeMap::new(),
    )
    .unwrap();
    SnapshotInputs::open(&fixture.packet, &fixture.profile).unwrap();
}

#[test]
fn tool_corruption_and_zero_or_missing_commit_refuse_without_latest_fallback() {
    let mut fixture = Fixture::new();
    fixture.profile.git.sha256 = "a".repeat(64);
    assert_eq!(
        SnapshotInputs::open(&fixture.packet, &fixture.profile)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Integrity
    );
    fixture.profile.commit = "0".repeat(40);
    assert!(SnapshotInputs::open(&fixture.packet, &fixture.profile).is_err());
}

#[test]
fn read_only_cli_reports_metadata_scope_without_private_source_text() {
    let fixture = Fixture::new();
    let packet = fixture.temp.path().join("packet.json");
    let profile = fixture.temp.path().join("profile.json");
    fs::write(&packet, serde_json::to_vec(&fixture.packet).unwrap()).unwrap();
    fs::write(&profile, serde_json::to_vec(&fixture.profile).unwrap()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_reel-workflow-native"))
        .arg("check-snapshot")
        .arg(packet)
        .arg(profile)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let raw = String::from_utf8(result.stdout).unwrap();
    assert!(!raw.contains("PRIVATE"));
    assert!(!raw.contains("input/source"));
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(value["snapshot_binding_verified"], true);
    assert_eq!(value["payloads_verified"], false);
    assert_eq!(value["durable_job_protection_verified"], false);
}

#[cfg(windows)]
#[test]
#[ignore = "Requires an explicitly declared EXISTING synthetic native store and retained object"]
fn existing_native_store_reads_exact_committed_input_with_guards_and_no_publication() {
    use reel_workflow::publication::InputReader;
    use reel_workflow_native::{NativeInputs, NativeProfile};
    let path =
        std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("Set REEL_NATIVE_INPUT_FIXTURE");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let object: Object = serde_json::from_value(value["object"].clone()).unwrap();
    let source = serde_json::from_value(value["source"].clone()).unwrap();
    let annex: Tool = serde_json::from_value(value["annex"].clone()).unwrap();
    let fixture = Fixture::with_object(object);
    let profile = NativeProfile {
        snapshot: fixture.profile.clone(),
        source,
        annex,
    };
    let mut reader = NativeInputs::open(&fixture.packet, &profile).unwrap();
    let asset = &fixture.packet.task.inputs[0];
    assert_eq!(
        reader.read(asset, asset.bytes - 1).unwrap_err().kind,
        ErrorKind::Budget
    );
    let bytes = reader.read(asset, asset.bytes).unwrap();
    assert_eq!(bytes.len() as u64, asset.bytes);
    assert_eq!(digest(&bytes), asset.sha256);
    let mut wrong = asset.clone();
    wrong.logical_id = "undeclared/alias".into();
    assert_eq!(
        reader.read(&wrong, asset.bytes).unwrap_err().kind,
        ErrorKind::Conflict
    );
    // The reader, native stream and executable guards remain live through these checks.
    assert_eq!(reader.snapshot().input_count(), 1);
    drop(reader);
    if let Some(directory) = std::env::var_os("REEL_NATIVE_EVIDENCE_DIRECTORY") {
        use std::io::Write;
        let directory = PathBuf::from(directory);
        assert!(directory.is_absolute() && directory.is_dir());
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("native-read.json"))
            .unwrap();
        let value = json!({"schema":"reel.workflow-native-read-fixture.v1","bytes_verified":bytes.len(),"snapshot_binding_verified":true,
            "guarded_read_verified":true,"durable_job_protection_verified":false,"publication_performed":false,"selection_changed":false});
        output
            .write_all(&serde_json::to_vec(&value).unwrap())
            .unwrap();
        output.sync_all().unwrap();
        for (name, raw) in [
            (
                "packet.json",
                serde_json::to_vec_pretty(&fixture.packet).unwrap(),
            ),
            (
                "snapshot-profile.json",
                serde_json::to_vec_pretty(&fixture.profile).unwrap(),
            ),
        ] {
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap();
            output.write_all(&raw).unwrap();
            output.sync_all().unwrap();
        }
        // Preserve this synthetic Git source for exact replay; no asset cache created.
        let _ = fixture.temp.keep();
    }
}

// These admission fixtures create only isolated native metadata/index journals.
// Their synthetic store declaration does not claim a real payload pool exists.
struct AdmissionCase {
    temp: tempfile::TempDir,
    profile: reel_workflow_native::admission::Profile,
    source: reel_workflow_native::NativeProfile,
}
impl AdmissionCase {
    fn new(fixture: &Fixture) -> Self {
        Self::with_store(fixture, tig_core::Digest::blob(b"synthetic-store"))
    }
    fn with_store(fixture: &Fixture, store: tig_core::Digest) -> Self {
        use tig_core::{Digest, tree::Tree};
        use tig_store::{
            Key,
            metadata::{Limits, MetadataStore},
            source_claims::{
                Selection, SourceJournal, coordinated,
                protection::{Actor, Enrollment, Index, Routing, work},
            },
        };
        let key = |s: &str| Key::parse(s).unwrap();
        let digest = |s: &str| Digest::blob(s.as_bytes());
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir(root.join("coordination")).unwrap();
        let mut nodes = MetadataStore::create(
            &root.join("metadata"),
            Limits {
                nodes: 100000,
                bytes: 32 * 1024 * 1024,
                database_bytes: 64 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let empty = Selection {
            root: Tree::empty(&mut nodes).unwrap().root().clone(),
            context: digest("empty"),
        };
        let mut index = Index::create_effects(
            &root.join("index"),
            key("epoch"),
            Enrollment {
                owner: key("index-owner"),
                subject: digest("group"),
                policies: vec![digest("policy")],
                stores: vec![store.clone()],
                actors: ["main", "retained", "slot"]
                    .into_iter()
                    .map(|id| Actor {
                        id: key(id),
                        owner: key("owner"),
                        stores: vec![store.clone()],
                    })
                    .collect(),
                max_bindings: 8192,
            },
            Some(Routing {
                source: key("main"),
                actor: key("main"),
            }),
        )
        .unwrap();
        // Exact typed idle binding from the qualified slots protocol.
        let raw = tig_core::identity::canonical_json(&json!([
            "reel.job-actor-idle.experimental-1",
            index.identity(),
            "slot",
            "owner",
            [store]
        ]))
        .unwrap();
        let idle = Selection {
            root: Tree::empty(&mut nodes).unwrap().root().clone(),
            context: Digest::blob(&raw),
        };
        index
            .bootstrap(
                [
                    ("main".into(), empty.clone()),
                    ("retained".into(), empty.clone()),
                    ("slot".into(), idle),
                ]
                .into(),
                &mut nodes,
            )
            .unwrap();
        let mut journal = SourceJournal::create_coordinated(
            &root.join("source"),
            key("project"),
            key("epoch"),
            coordinated::Binding {
                index: index.identity().clone(),
                source: key("main"),
                actor: key("main"),
                owner: key("owner"),
                initial: empty,
            },
        )
        .unwrap();
        work::adoption::enrollment::initialize(&mut index, &mut journal, &mut nodes).unwrap();
        let minimum = index.checkpoint();
        let identity = index.identity().clone();
        let profile = reel_workflow_native::admission::Profile {
            schema: "reel.workflow-native-admission.v1".into(),
            index_root: root.join("index"),
            index_identity: identity.clone(),
            index_minimum: minimum.clone(),
            metadata_root: root.join("metadata"),
            metadata_minimum: nodes.finish().unwrap(),
            scope: reel_workflow_native::admission::Scope {
                coordination_root: root.join("coordination"),
                actor: key("slot"),
                owner: key("owner"),
                operation: key("workflow-job"),
                policy: digest("policy"),
                store: store.clone(),
            },
        };
        let annex = Tool {
            path: root.join("unused-annex.exe"),
            sha256: digest("synthetic-annex-runtime").as_str().into(),
            bytes: 23,
        };
        let source = reel_workflow_native::NativeProfile {
            snapshot: fixture.profile.clone(),
            annex: annex.clone(),
            source: tig_git::annex::workflow::reader::Profile {
                store,
                maximum_bytes: 1024,
                pool: tig_git::annex::process::Enrollment {
                    git: fixture.profile.git.path.clone(),
                    executable: annex.path,
                    repository: root.join("no-payload-pool"),
                    tools: vec![root.into()],
                    empty_config: root.join("unused.config"),
                    empty_hooks: root.join("unused-hooks"),
                    temporary: root.into(),
                    legacy_profile: None,
                    command_millis: 1000,
                    output_bytes: 65536,
                    control: Some(tig_git::annex::process::ControlBinding {
                        root: profile.index_root.clone(),
                        index: identity,
                        minimum,
                        store: profile.scope.store.clone(),
                    }),
                },
            },
        };
        Self {
            temp,
            profile,
            source,
        }
    }
    fn protected(&self, object: &Object) -> Vec<tig_store::Key> {
        use tig_store::{metadata::MetadataStore, source_claims::protection::Index};
        let mut nodes =
            MetadataStore::open(&self.profile.metadata_root, &self.profile.metadata_minimum)
                .unwrap();
        let mut index = Index::open(&self.profile.index_root, &self.profile.index_minimum).unwrap();
        index
            .inspect(&mut nodes, object, &self.profile.scope.store)
            .unwrap()
            .protected_by
    }
}

#[test]
fn workflow_claims_survive_restart_and_lost_response_with_original_work_and_activation() {
    use reel_workflow_native::admission::Prepared;
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let before = fs::read(case.profile.metadata_root.join("metadata.json")).unwrap();
    let prepared =
        Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1").unwrap();
    let preview = prepared.preview().unwrap();
    assert_eq!(preview.total_actor_limit, 16);
    assert_eq!(preview.changed_path_limit, 128);
    assert_eq!(
        before,
        fs::read(case.profile.metadata_root.join("metadata.json")).unwrap()
    );
    let payload = Object::from_bytes(b"synthetic input");
    assert!(case.protected(&payload).is_empty());
    let accepted = prepared.accept().unwrap();
    assert_eq!(
        case.protected(&payload),
        vec![case.profile.scope.actor.clone()]
    );
    // Unused record stream bytes are also covered, not just the chosen input.
    assert_eq!(
        case.protected(&Object::from_bytes(b"preview")),
        vec![case.profile.scope.actor.clone()]
    );
    drop(prepared);
    let retry = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    assert_eq!(retry.work, accepted.work);
    assert_eq!(retry.activation, accepted.activation);
    let guard = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .reopen(&accepted)
        .unwrap();
    // Reopened generation is fenced; another cooperating acceptance cannot run.
    assert!(
        Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
            .unwrap()
            .accept()
            .is_err()
    );
    drop(guard);
    assert_eq!(
        case.protected(&payload),
        vec![case.profile.scope.actor.clone()]
    );
    assert!(!case.temp.path().join("no-payload-pool").exists());
}

#[test]
fn admission_rejects_changed_attempt_store_scope_receipt_and_unmanaged_provider() {
    use reel_workflow_native::admission::Prepared;
    let fixture = Fixture::new();
    let mut case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    assert!(
        Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-2")
            .unwrap()
            .accept()
            .is_err()
    );
    let mut changed = receipt.clone();
    changed.activation = Object::from_bytes(b"wrong-generation");
    assert!(
        Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
            .unwrap()
            .reopen(&changed)
            .is_err()
    );
    let mut changed = receipt.clone();
    changed.scope.operation = tig_store::Key::parse("other-operation").unwrap();
    assert!(
        Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
            .unwrap()
            .reopen(&changed)
            .is_err()
    );
    let original = case.source.annex.sha256.clone();
    case.source.annex.sha256 = tig_core::Digest::blob(b"different-runtime").as_str().into();
    assert!(
        Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
            .unwrap()
            .preview()
            .is_err()
    );
    assert!(
        Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
            .unwrap()
            .reopen(&receipt)
            .is_err()
    );
    case.source.annex.sha256 = original;
    case.profile.scope.store = tig_core::Digest::blob(b"wrong-store");
    assert!(Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1").is_err());
    case.source.source.pool.control = None;
    assert!(Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1").is_err());
}

#[test]
fn claimed_source_archive_reopens_without_authored_working_files() {
    use reel_workflow_native::admission::Prepared;
    use tig_assets::resolve::AssetResolver;
    use tig_core::tree::TreeLimits;
    use tig_store::metadata::MetadataStore;
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    fs::write(
        fixture.temp.path().join("records/input.tig"),
        b"not available",
    )
    .unwrap();
    let mut nodes =
        MetadataStore::open(&case.profile.metadata_root, &receipt.metadata_checkpoint).unwrap();
    let restored = tig_git::source::native::AssetSnapshot::from_metadata_archive(
        &mut nodes,
        &receipt.source_archive,
        tig_git::source::native::Limits::default(),
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(
        restored
            .resolve_id("input/source")
            .unwrap()
            .selected()
            .unwrap(),
        &Object::from_bytes(b"synthetic input")
    );
    Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .reopen(&receipt)
        .unwrap();
}

#[test]
fn admission_cli_requires_explicit_apply_private_new_receipts_and_preserves_failures() {
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let root = case.temp.path();
    let packet = root.join("packet.json");
    let source = root.join("native-profile.json");
    let profile = root.join("admission-profile.json");
    fs::write(&packet, serde_json::to_vec(&fixture.packet).unwrap()).unwrap();
    fs::write(&source, serde_json::to_vec(&case.source).unwrap()).unwrap();
    fs::write(&profile, serde_json::to_vec(&case.profile).unwrap()).unwrap();
    let call = |command: &str, tail: &[&std::ffi::OsStr]| {
        Command::new(env!("CARGO_BIN_EXE_reel-workflow-native"))
            .arg(command)
            .arg(&packet)
            .arg(&source)
            .arg(&profile)
            .args(tail)
            .output()
            .unwrap()
    };
    let attempt = std::ffi::OsStr::new("attempt-1");
    let apply = std::ffi::OsStr::new("--apply");
    let preview = call("preview-admission", &[attempt]);
    assert!(preview.status.success());
    let receipt = root.join("receipt.json");
    assert!(
        !call("accept-admission", &[attempt, receipt.as_os_str()])
            .status
            .success()
    );
    assert!(!receipt.exists());
    let forbidden = case.profile.index_root.join("not-a-private-receipt.json");
    assert!(
        !call("accept-admission", &[attempt, forbidden.as_os_str(), apply])
            .status
            .success()
    );
    assert!(!forbidden.exists());
    assert!(
        case.protected(&Object::from_bytes(b"synthetic input"))
            .is_empty()
    );
    let result = call("accept-admission", &[attempt, receipt.as_os_str(), apply]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let raw = String::from_utf8(result.stdout).unwrap();
    assert!(!raw.contains("PRIVATE"));
    assert!(!raw.contains("input/source"));
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(value["durable_input_claims_verified"], true);
    assert_eq!(value["payloads_verified"], false);
    assert_eq!(value["retained_owner_acknowledged"], false);
    let original = fs::read(&receipt).unwrap();
    assert!(
        !call("accept-admission", &[attempt, receipt.as_os_str(), apply])
            .status
            .success()
    );
    assert_eq!(fs::read(&receipt).unwrap(), original);
    assert!(
        call("reopen-admission", &[receipt.as_os_str()])
            .status
            .success()
    );
    let partial = root.join("changed-attempt-partial.json");
    assert!(
        !call(
            "accept-admission",
            &[
                std::ffi::OsStr::new("attempt-2"),
                partial.as_os_str(),
                apply
            ]
        )
        .status
        .success()
    );
    assert_eq!(fs::metadata(&partial).unwrap().len(), 0); // Original operation remains admitted; failure is preserved.
    let recovered = root.join("recovered-receipt.json");
    assert!(
        call("accept-admission", &[attempt, recovered.as_os_str(), apply])
            .status
            .success()
    );
    let old: reel_workflow_native::admission::Receipt = serde_json::from_slice(&original).unwrap();
    let new: reel_workflow_native::admission::Receipt =
        serde_json::from_slice(&fs::read(recovered).unwrap()).unwrap();
    assert_eq!(old.work, new.work);
    assert_eq!(old.activation, new.activation);
    if let Some(directory) = std::env::var_os("REEL_ADMISSION_EVIDENCE_DIRECTORY") {
        let directory = PathBuf::from(directory);
        assert!(directory.is_absolute() && directory.is_dir());
        // Preserve exact synthetic metadata/source roots for frozen-tool readback.
        // No payload pool exists in this fixture.
        for (name, path) in [
            ("packet.json", &packet),
            ("native-profile.json", &source),
            ("admission-profile.json", &profile),
            ("receipt.json", &receipt),
        ] {
            let raw = fs::read(path).unwrap();
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap();
            use std::io::Write;
            output.write_all(&raw).unwrap();
            output.sync_all().unwrap();
        }
        let _ = case.temp.keep();
        let _ = fixture.temp.keep();
    }
}

#[test]
fn admission_node_budget_refuses_before_claim_or_metadata_changes() {
    let mut fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let mut task = fixture.packet.task.clone();
    task.prompt = "p".repeat(64 * 1024);
    fixture.packet = make_packet(
        &Plan {
            schema: PLAN_SCHEMA.into(),
            project: fixture.packet.project.clone(),
            source_commit: fixture.profile.commit.clone(),
            tasks: vec![task],
        },
        "review",
        BTreeMap::new(),
    )
    .unwrap();
    let before = fs::read(case.profile.metadata_root.join("metadata.json")).unwrap();
    let error = reel_workflow_native::admission::Prepared::open(
        &fixture.packet,
        &case.source,
        &case.profile,
        "attempt-1",
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, ErrorKind::Budget);
    assert_eq!(
        before,
        fs::read(case.profile.metadata_root.join("metadata.json")).unwrap()
    );
    assert!(
        case.protected(&Object::from_bytes(b"synthetic input"))
            .is_empty()
    );
}

#[test]
fn native_pending_update_blocks_claim_guard_until_its_original_operation_is_reconciled() {
    use reel_workflow_native::admission::Prepared;
    use tig_store::{
        Key,
        metadata::MetadataStore,
        source_claims::protection::{Error as NativeError, Index, Stage, Update},
    };
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let mut nodes =
        MetadataStore::open(&case.profile.metadata_root, &receipt.metadata_checkpoint).unwrap();
    let mut index = Index::open(&case.profile.index_root, &receipt.index_checkpoint).unwrap();
    let actor = Key::parse("retained").unwrap();
    let state = index.actor(&mut nodes, &actor).unwrap();
    let update = Update {
        operation: Key::parse("original-interrupted-update").unwrap(),
        actor,
        owner: case.profile.scope.owner.clone(),
        expected_revision: state.revision,
        selection: state.selection,
        changes: BTreeMap::new(),
        ready: true,
    };
    assert!(
        index
            .update_hook(&update, &mut nodes, |stage| if stage == Stage::Prepared {
                Err(NativeError::Injected)
            } else {
                Ok(())
            })
            .is_err()
    );
    nodes.finish().unwrap();
    drop(index);
    drop(nodes);
    assert!(
        Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
            .unwrap()
            .reopen(&receipt)
            .is_err()
    );
    let mut nodes =
        MetadataStore::open(&case.profile.metadata_root, &receipt.metadata_checkpoint).unwrap();
    let mut index = Index::open(&case.profile.index_root, &receipt.index_checkpoint).unwrap();
    index.update(&update, &mut nodes).unwrap();
    nodes.finish().unwrap();
    drop(index);
    drop(nodes);
    Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .reopen(&receipt)
        .unwrap();
    assert_eq!(
        case.protected(&Object::from_bytes(b"synthetic input")),
        vec![case.profile.scope.actor.clone()]
    );
}

#[cfg(windows)]
#[test]
fn staging_cli_prepares_a_pinned_ticket_but_never_substitutes_local_bytes_for_missing_native_inputs()
 {
    use reel_workflow_native::{admission::Prepared, staging};
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let root = case.temp.path();
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let scratch = root.join("scratch");
    fs::create_dir(&scratch).unwrap();
    let stage = staging::Profile {
        schema: "reel.workflow-staging.v1".into(),
        parent: scratch.clone(),
        maximum_bytes: 8 * 1024 * 1024,
        capacity_policy: reel_assets::admission::Policy {
            minimum_free_bytes: 65536,
            minimum_free_basis_points: 0,
            max_object_bytes: 512 * 1024 * 1024,
            ..Default::default()
        },
    };
    let packet = root.join("packet.json");
    let source = root.join("source.json");
    let admission = root.join("admission.json");
    let accepted = root.join("accepted.json");
    let stage_profile = root.join("stage.json");
    let ticket = root.join("ticket.json");
    for (path, raw) in [
        (&packet, serde_json::to_vec(&fixture.packet).unwrap()),
        (&source, serde_json::to_vec(&case.source).unwrap()),
        (&admission, serde_json::to_vec(&case.profile).unwrap()),
        (&accepted, receipt.canonical_bytes().unwrap()),
        (&stage_profile, serde_json::to_vec(&stage).unwrap()),
    ] {
        fs::write(path, raw).unwrap();
    }
    let prepare = || {
        Command::new(env!("CARGO_BIN_EXE_reel-workflow-native"))
            .arg("prepare-stage")
            .arg(&packet)
            .arg(&source)
            .arg(&admission)
            .arg(&accepted)
            .arg(&stage_profile)
            .arg(&ticket)
            .output()
            .unwrap()
    };
    let result = prepare();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    let original = fs::read(&ticket).unwrap();
    let pin = digest(&original);
    assert!(!prepare().status.success());
    assert_eq!(fs::read(&ticket).unwrap(), original);
    let exported = Command::new(env!("CARGO_BIN_EXE_reel-workflow-native"))
        .arg("export-stage")
        .arg(&packet)
        .arg(&source)
        .arg(&admission)
        .arg(&ticket)
        .arg(&pin)
        .output()
        .unwrap();
    assert!(!exported.status.success());
    let workspace = scratch.join(format!("handoff-{}", receipt.operation_identity().unwrap()));
    assert_eq!(fs::read_dir(workspace).unwrap().count(), 1); // Only durable intent; no input publication.
    assert_eq!(
        case.protected(&Object::from_bytes(b"synthetic input")),
        vec![case.profile.scope.actor.clone()]
    );
    assert!(!case.source.source.pool.repository.exists());
}

#[cfg(windows)]
#[test]
#[ignore = "Requires an explicitly declared existing synthetic native payload; no pool setup or adoption"]
fn accepted_claims_native_bytes_and_pinned_workspace_stage_together_across_cli_restart() {
    use reel_workflow_native::{NativeProfile, admission::Prepared, staging};
    let path =
        std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("Set REEL_NATIVE_INPUT_FIXTURE");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let object: Object = serde_json::from_value(value["object"].clone()).unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(value["source"].clone()).unwrap();
    let annex: Tool = serde_json::from_value(value["annex"].clone()).unwrap();
    let mut fixture = Fixture::with_object(object.clone());
    let raw = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
    };
    let mut case = AdmissionCase::with_store(&fixture, native.store.clone());
    let control = case.source.source.pool.control.clone();
    case.source = NativeProfile {
        snapshot: fixture.profile.clone(),
        annex,
        source: native,
    };
    case.source.source.pool.control = control;
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let parent = case.temp.path().join("scratch");
    fs::create_dir(&parent).unwrap();
    let profile = staging::Profile {
        schema: "reel.workflow-staging.v1".into(),
        parent: parent.clone(),
        maximum_bytes: 8 * 1024 * 1024,
        capacity_policy: reel_assets::admission::Policy {
            minimum_free_bytes: 65536,
            minimum_free_basis_points: 0,
            max_object_bytes: 512 * 1024 * 1024,
            ..Default::default()
        },
    };
    let ticket = staging::prepare(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &profile,
    )
    .unwrap();
    let bytes = ticket.canonical_bytes().unwrap();
    let pin = digest(&bytes);
    assert!(
        staging::prepare(
            &fixture.packet,
            &case.source,
            &case.profile,
            &receipt,
            &profile
        )
        .is_err()
    );
    assert!(
        staging::export(
            &fixture.packet,
            &case.source,
            &case.profile,
            &bytes,
            &"0".repeat(64)
        )
        .is_err()
    );
    let packet = case.temp.path().join("stage-packet.json");
    let source = case.temp.path().join("stage-native.json");
    let admission = case.temp.path().join("stage-admission.json");
    let ticket_path = case.temp.path().join("stage-ticket.json");
    for (path, raw) in [
        (&packet, serde_json::to_vec(&fixture.packet).unwrap()),
        (&source, serde_json::to_vec(&case.source).unwrap()),
        (&admission, serde_json::to_vec(&case.profile).unwrap()),
        (&ticket_path, bytes.clone()),
    ] {
        fs::write(path, raw).unwrap();
    }
    let call = || {
        Command::new(env!("CARGO_BIN_EXE_reel-workflow-native"))
            .arg("export-stage")
            .arg(&packet)
            .arg(&source)
            .arg(&admission)
            .arg(&ticket_path)
            .arg(&pin)
            .output()
            .unwrap()
    };
    let first = call();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stdout)
    );
    let repeated = call();
    assert!(
        repeated.status.success(),
        "{}",
        String::from_utf8_lossy(&repeated.stdout)
    );
    let report: serde_json::Value = serde_json::from_slice(&repeated.stdout).unwrap();
    assert_eq!(report["workspace"]["already_complete"], true);
    assert_eq!(report["retained_owner_acknowledged"], false);
    assert_eq!(
        case.protected(&object),
        vec![case.profile.scope.actor.clone()]
    );
    assert_eq!(
        fs::read_dir(case.profile.scope.coordination_root.join("admissions"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|s| s == "json"))
            .count(),
        1
    );
    let original = case.source.source.pool.repository.clone();
    case.source.source.pool.repository = case.temp.path().join("unavailable-native-pool");
    assert!(staging::export(&fixture.packet, &case.source, &case.profile, &bytes, &pin).is_err());
    case.source.source.pool.repository = original;
    assert_eq!(
        case.protected(&object),
        vec![case.profile.scope.actor.clone()]
    );
    if let Some(directory) = std::env::var_os("REEL_STAGING_EVIDENCE_DIRECTORY") {
        use std::io::Write;
        let directory = PathBuf::from(directory);
        assert!(directory.is_absolute() && directory.is_dir());
        for (name, raw) in [
            ("packet.json", fs::read(&packet).unwrap()),
            ("native-profile.json", fs::read(&source).unwrap()),
            ("admission-profile.json", fs::read(&admission).unwrap()),
            ("admission-receipt.json", receipt.canonical_bytes().unwrap()),
            (
                "staging-profile.json",
                serde_json::to_vec(&profile).unwrap(),
            ),
            ("ticket.json", bytes),
            ("result.json", repeated.stdout),
        ] {
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap();
            output.write_all(&raw).unwrap();
            output.sync_all().unwrap();
        }
        let _ = case.temp.keep();
        let _ = fixture.temp.keep();
    }
}
