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
            budget: None,
        };
        let task = Task {
            upstream_inputs: None,
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
                task_snapshots: Default::default(),
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
                task_snapshots: Default::default(),
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
            task_snapshots: Default::default(),
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
    use reel_workflow_native::InputReader;
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
    {
        use sha2::{Digest as _, Sha256};
        let mut stream = reader.stream(asset, asset.bytes).unwrap();
        let mut hash = Sha256::new();
        let mut count = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            let read = std::io::Read::read(&mut stream, &mut buffer).unwrap();
            if read == 0 {
                break;
            }
            count += read as u64;
            hash.update(&buffer[..read]);
        }
        assert_eq!(count, asset.bytes);
        assert_eq!(
            hash.finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            asset.sha256
        );
    }
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
#[cfg(windows)]
fn response_fixture(
    packet: &Packet,
    root: &Path,
) -> (ReturnedResult, reel_workflow_native::response::Returns) {
    fs::create_dir(root).unwrap();
    fs::write(root.join("note.txt"), b"synthetic response").unwrap();
    (
        ReturnedResult {
            schema: RESULT_SCHEMA.into(),
            worker_receipt: None,
            task_id: packet.task.id.clone(),
            task_key: packet.task_key.clone(),
            attempt_id: "attempt-1".into(),
            artifacts: BTreeMap::from([(
                "note".into(),
                Artifact {
                    logical_id: "candidate/note".into(),
                    sha256: digest(b"synthetic response"),
                    bytes: 18,
                    media_type: "text/plain".into(),
                },
            )]),
            provenance: Provenance {
                mode: Mode::Handoff,
                provider: None,
                model: None,
                executable: None,
            },
        },
        reel_workflow_native::response::Returns {
            schema: "reel.workflow-returns.v1".into(),
            root: root.into(),
            files: BTreeMap::from([("note".into(), "note.txt".into())]),
        },
    )
}

#[cfg(windows)]
#[test]
fn external_registration_is_logically_idempotent_and_preserves_positive_claims() {
    use reel_workflow_native::{admission::Prepared, response};
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let (result, returns) = response_fixture(&fixture.packet, &case.temp.path().join("returns"));
    let output = Object::from_bytes(b"synthetic response");
    assert!(case.protected(&output).is_empty());
    let first = response::register(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &returns,
    )
    .unwrap();
    let second = response::register(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &returns,
    )
    .unwrap();
    assert_eq!(first.work, second.work);
    assert_eq!(first.registration, second.registration);
    assert_eq!(
        case.protected(&output),
        vec![case.profile.scope.actor.clone()]
    );
    assert_eq!(
        case.protected(&Object::from_bytes(b"synthetic input")),
        vec![case.profile.scope.actor.clone()]
    );
    let mut changed = result.clone();
    changed.provenance.model = Some("different-model".into());
    assert_eq!(
        response::register(
            &fixture.packet,
            &case.source,
            &case.profile,
            &receipt,
            &changed,
            &returns
        )
        .unwrap_err()
        .kind,
        ErrorKind::Conflict
    );
    let mut changed = result.clone();
    changed.artifacts.get_mut("note").unwrap().logical_id = "candidate/another-note".into();
    assert_eq!(
        response::register(
            &fixture.packet,
            &case.source,
            &case.profile,
            &receipt,
            &changed,
            &returns
        )
        .unwrap_err()
        .kind,
        ErrorKind::Conflict
    );
    fs::write(returns.root.join("note.txt"), b"changed response!!").unwrap();
    assert!(
        response::register(
            &fixture.packet,
            &case.source,
            &case.profile,
            &receipt,
            &result,
            &returns
        )
        .is_err()
    );
    assert_eq!(
        case.protected(&output),
        vec![case.profile.scope.actor.clone()]
    );
    assert!(!case.source.source.pool.repository.exists());
}

#[cfg(windows)]
#[test]
fn external_registration_refuses_unsafe_or_incomplete_return_bindings() {
    use reel_workflow_native::{admission::Prepared, response};
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let (result, returns) = response_fixture(&fixture.packet, &case.temp.path().join("returns"));
    let output = Object::from_bytes(b"synthetic response");
    for bad_path in [
        "../note.txt",
        "C:/note.txt",
        "note.txt:stream",
        "NUL",
        "nested/../note.txt",
    ] {
        let mut bad = returns.clone();
        bad.files.insert("note".into(), bad_path.into());
        assert!(
            response::register(
                &fixture.packet,
                &case.source,
                &case.profile,
                &receipt,
                &result,
                &bad
            )
            .is_err()
        );
    }
    let mut bad = returns.clone();
    bad.files.clear();
    assert!(
        response::register(
            &fixture.packet,
            &case.source,
            &case.profile,
            &receipt,
            &result,
            &bad
        )
        .is_err()
    );
    let mut bad = returns.clone();
    bad.root = case.profile.metadata_root.clone();
    assert!(
        response::register(
            &fixture.packet,
            &case.source,
            &case.profile,
            &receipt,
            &result,
            &bad
        )
        .is_err()
    );
    let mut changed = result.clone();
    changed.attempt_id = "other-attempt".into();
    assert!(
        response::register(
            &fixture.packet,
            &case.source,
            &case.profile,
            &receipt,
            &changed,
            &returns
        )
        .is_err()
    );
    assert!(case.protected(&output).is_empty());
}

#[cfg(windows)]
#[test]
fn external_registration_recovers_original_native_prepare_without_new_identity() {
    use reel_workflow_native::{admission::Prepared, response};
    use tig_store::{
        metadata::MetadataStore,
        source_claims::protection::{self, Index, work},
    };
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let (result, returns) = response_fixture(&fixture.packet, &case.temp.path().join("returns"));
    let staged = response::stage(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &returns,
    )
    .unwrap();
    let output = Object::from_bytes(b"synthetic response");
    assert!(case.protected(&output).is_empty());
    let mut nodes =
        MetadataStore::open(&case.profile.metadata_root, &receipt.metadata_checkpoint).unwrap();
    let mut index = Index::open(&case.profile.index_root, &receipt.index_checkpoint).unwrap();
    let original = work::read(&mut index, &staged.work, &nodes).unwrap();
    let mut interrupted = false;
    assert!(
        index
            .update_hook(&original.update, &mut nodes, |stage| {
                if stage == protection::Stage::Prepared {
                    interrupted = true;
                    Err(protection::Error::Unknown)
                } else {
                    Ok(())
                }
            })
            .is_err()
    );
    assert!(interrupted);
    drop(index);
    drop(nodes);
    assert!(
        Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
            .unwrap()
            .reopen(&receipt)
            .is_err()
    );
    let mut changed = result.clone();
    changed.provenance.provider = Some("changed-provider".into());
    assert_eq!(
        response::register(
            &fixture.packet,
            &case.source,
            &case.profile,
            &receipt,
            &changed,
            &returns
        )
        .unwrap_err()
        .kind,
        ErrorKind::Conflict
    );
    assert!(
        Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
            .unwrap()
            .reopen(&receipt)
            .is_err()
    );
    let resumed = response::register(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &returns,
    )
    .unwrap();
    assert_eq!(staged.work, resumed.work);
    assert_eq!(staged.registration, resumed.registration);
    assert_eq!(
        case.protected(&output),
        vec![case.profile.scope.actor.clone()]
    );
    Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .reopen(&receipt)
        .unwrap();
}

#[cfg(windows)]
#[test]
#[ignore = "Publishes only a synthetic response into an explicitly declared existing test pool; no setup/hydration/selection"]
fn external_response_native_ingress_reopens_and_replays_across_cli_restart() {
    use reel_workflow_native::{NativeProfile, admission::Prepared, response};
    use std::io::Read;
    let path =
        std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("Set REEL_NATIVE_INPUT_FIXTURE");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let object: Object = serde_json::from_value(value["object"].clone()).unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(value["source"].clone()).unwrap();
    let annex: Tool = serde_json::from_value(value["annex"].clone()).unwrap();
    let mut fixture = Fixture::with_object(object.clone());
    let git_raw = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&git_raw),
        bytes: git_raw.len() as u64,
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
    let (result, returns) = response_fixture(&fixture.packet, &case.temp.path().join("returns"));
    let files = [
        ("packet.json", serde_json::to_vec(&fixture.packet).unwrap()),
        ("native.json", serde_json::to_vec(&case.source).unwrap()),
        ("profile.json", serde_json::to_vec(&case.profile).unwrap()),
        ("admission.json", receipt.canonical_bytes().unwrap()),
        ("result.json", serde_json::to_vec(&result).unwrap()),
        ("returns.json", serde_json::to_vec(&returns).unwrap()),
    ];
    for (name, raw) in &files {
        fs::write(case.temp.path().join(name), raw).unwrap();
    }
    let invoke = |output: &str| {
        Command::new(env!("CARGO_BIN_EXE_reel-workflow-native"))
            .arg("ingest-response")
            .args(files.iter().map(|(name, _)| case.temp.path().join(name)))
            .arg(case.temp.path().join(output))
            .arg("--apply")
            .output()
            .unwrap()
    };
    let first = invoke("candidate-1.json");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stdout)
    );
    // Simulate losing the first response: retry ORIGINAL result, not a new attempt.
    let second = invoke("candidate-2.json");
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stdout)
    );
    let first_candidate: response::Candidate =
        serde_json::from_slice(&fs::read(case.temp.path().join("candidate-1.json")).unwrap())
            .unwrap();
    let second_candidate: response::Candidate =
        serde_json::from_slice(&fs::read(case.temp.path().join("candidate-2.json")).unwrap())
            .unwrap();
    assert_eq!(first_candidate.work, second_candidate.work);
    assert_eq!(first_candidate.registration, second_candidate.registration);
    let report: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(report["native_bytes_reopened"], true);
    assert_eq!(report["retained_owner_acknowledged"], false);
    assert_eq!(report["task_completed"], false);
    assert_eq!(report["selected"], false);
    let output = Object::from_bytes(b"synthetic response");
    let mut reopened =
        tig_git::annex::workflow::reader::open(&case.source.source, &output).unwrap();
    let mut raw = Vec::new();
    reopened.read_to_end(&mut raw).unwrap();
    assert_eq!(raw, b"synthetic response");
    assert_eq!(
        case.protected(&output),
        vec![case.profile.scope.actor.clone()]
    );
    assert_eq!(
        case.protected(&object),
        vec![case.profile.scope.actor.clone()]
    );
    assert_eq!(
        fs::read(returns.root.join("note.txt")).unwrap(),
        b"synthetic response"
    );
    if let Some(directory) = std::env::var_os("REEL_RESPONSE_EVIDENCE_DIRECTORY") {
        use std::io::Write;
        let directory = PathBuf::from(directory);
        assert!(directory.is_absolute() && directory.is_dir());
        for (name, raw) in files.into_iter().chain([
            (
                "candidate.json",
                serde_json::to_vec(&second_candidate).unwrap(),
            ),
            ("ingress-report.json", second.stdout),
        ]) {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap();
            file.write_all(&raw).unwrap();
            file.sync_all().unwrap();
        }
        let _ = case.temp.keep();
        let _ = fixture.temp.keep();
    }
}

#[cfg(windows)]
fn retained_target() -> reel_workflow_native::retention::Target {
    reel_workflow_native::retention::Target {
        schema: "reel.workflow-retained-target.v1".into(),
        actor: tig_store::Key::parse("retained").unwrap(),
        owner: tig_store::Key::parse("owner").unwrap(),
    }
}

#[cfg(windows)]
#[test]
fn retained_registration_stages_exact_identity_and_refuses_wrong_owner_or_store_scope() {
    use reel_workflow_native::{admission::Prepared, response, retention};
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let (result, returns) = response_fixture(&fixture.packet, &case.temp.path().join("returns"));
    response::register(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &returns,
    )
    .unwrap();
    let target = retained_target();
    let first = retention::stage(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &target,
    )
    .unwrap();
    let second = retention::stage(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &target,
    )
    .unwrap();
    assert_eq!(first.work, second.work);
    assert_eq!(first.evidence, second.evidence);
    assert_eq!(
        case.protected(&Object::from_bytes(b"synthetic response")),
        vec![case.profile.scope.actor.clone()]
    );
    for actor in ["slot", "main", "unknown"] {
        let mut bad = target.clone();
        bad.actor = tig_store::Key::parse(actor).unwrap();
        assert_eq!(
            retention::stage(
                &fixture.packet,
                &case.source,
                &case.profile,
                &receipt,
                &result,
                &bad
            )
            .unwrap_err()
            .kind,
            ErrorKind::Denied
        );
    }
    let mut bad = target.clone();
    bad.owner = tig_store::Key::parse("wrong-owner").unwrap();
    assert_eq!(
        retention::stage(
            &fixture.packet,
            &case.source,
            &case.profile,
            &receipt,
            &result,
            &bad
        )
        .unwrap_err()
        .kind,
        ErrorKind::Denied
    );
    // Missing native runtime/provider cannot turn staged metadata into an ack.
    assert!(
        retention::retain(
            &fixture.packet,
            &case.source,
            &case.profile,
            &receipt,
            &result,
            &target
        )
        .is_err()
    );
    let case = AdmissionCase::with_extra_source_store(
        &fixture,
        tig_core::Digest::blob(b"synthetic-store"),
        Some(tig_core::Digest::blob(b"extra-store")),
    );
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let (result, returns) = response_fixture(&fixture.packet, &case.temp.path().join("returns"));
    response::register(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &returns,
    )
    .unwrap();
    assert_eq!(
        retention::stage(
            &fixture.packet,
            &case.source,
            &case.profile,
            &receipt,
            &result,
            &target
        )
        .unwrap_err()
        .kind,
        ErrorKind::Denied
    );
}

#[cfg(windows)]
#[test]
fn retained_native_prepare_recovers_original_work_and_keeps_job_claims() {
    use reel_workflow_native::{admission::Prepared, response, retention};
    use tig_store::{
        metadata::MetadataStore,
        source_claims::protection::{self, Index, work},
    };
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let (result, returns) = response_fixture(&fixture.packet, &case.temp.path().join("returns"));
    response::register(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &returns,
    )
    .unwrap();
    let target = retained_target();
    let staged = retention::stage(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &target,
    )
    .unwrap();
    let mut nodes =
        MetadataStore::open(&case.profile.metadata_root, &staged.metadata_checkpoint).unwrap();
    let mut index = Index::open(&case.profile.index_root, &staged.index_checkpoint).unwrap();
    let record = work::read(&mut index, &staged.work, &nodes).unwrap();
    let mut interrupted = false;
    assert!(
        index
            .update_hook(&record.update, &mut nodes, |stage| {
                if stage == protection::Stage::Prepared {
                    interrupted = true;
                    Err(protection::Error::Unknown)
                } else {
                    Ok(())
                }
            })
            .is_err()
    );
    assert!(interrupted);
    drop(nodes);
    drop(index);
    let mut changed = result.clone();
    changed.provenance.model = Some("changed-model".into());
    assert!(
        retention::stage(
            &fixture.packet,
            &case.source,
            &case.profile,
            &receipt,
            &changed,
            &target
        )
        .is_err()
    );
    assert!(
        Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
            .unwrap()
            .reopen(&receipt)
            .is_err()
    );
    let resumed = retention::stage(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &target,
    )
    .unwrap();
    assert_eq!(staged.work, resumed.work);
    assert_eq!(staged.evidence, resumed.evidence);
    for object in [
        Object::from_bytes(b"synthetic response"),
        Object::from_bytes(b"synthetic input"),
        receipt.activation.clone(),
        receipt.source_archive.clone(),
    ] {
        assert_eq!(
            case.protected(&object),
            vec![target.actor.clone(), case.profile.scope.actor.clone()]
        );
    }
    Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .reopen(&receipt)
        .unwrap();
}

#[cfg(windows)]
#[test]
#[ignore = "Uses only an explicitly declared existing synthetic pool; retained work/recovery and historical verification, no pool setup or real project selection"]
fn retained_native_acknowledgement_replays_and_survives_source_advance_but_not_lost_coverage() {
    use reel_workflow_native::{NativeProfile, admission::Prepared, response, retention};
    use tig_core::{
        Digest,
        tree::{NodeStore, Tree, TreeLimits},
    };
    use tig_store::{
        metadata::MetadataStore,
        source_claims::{
            Selection,
            protection::{self, Index, work},
        },
    };
    let path =
        std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("Set REEL_NATIVE_INPUT_FIXTURE");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let object: Object = serde_json::from_value(value["object"].clone()).unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(value["source"].clone()).unwrap();
    let annex: Tool = serde_json::from_value(value["annex"].clone()).unwrap();
    let mut fixture = Fixture::with_object(object.clone());
    let git_raw = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&git_raw),
        bytes: git_raw.len() as u64,
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
    let (result, returns) = response_fixture(&fixture.packet, &case.temp.path().join("returns"));
    response::ingest(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &returns,
    )
    .unwrap();
    let target = retained_target();
    // Preserve unrelated retained history, rather than replacing its tree.
    let mut nodes =
        MetadataStore::open(&case.profile.metadata_root, &case.profile.metadata_minimum).unwrap();
    let mut index = Index::open(&case.profile.index_root, &case.profile.index_minimum).unwrap();
    let old = index.actor(&mut nodes, &target.actor).unwrap();
    let history = nodes.put(b"unrelated retained history").unwrap();
    let history_path = LogicalPath::parse("prior/history").unwrap();
    let entries = BTreeMap::from([(history_path.clone(), history.clone())]);
    let root = Tree::build(&entries, &mut nodes, TreeLimits::default())
        .unwrap()
        .0
        .root()
        .clone();
    let update = protection::Update {
        operation: tig_store::Key::parse("fixture-history").unwrap(),
        actor: target.actor.clone(),
        owner: target.owner.clone(),
        expected_revision: old.revision,
        selection: Selection {
            root,
            context: Digest::blob(b"prior-history"),
        },
        changes: entries.into_iter().map(|(p, o)| (p, Some(o))).collect(),
        ready: true,
    };
    let work_ref = work::stage(&mut index, &update, None, &mut nodes).unwrap();
    work::execute(&mut index, &work_ref, &mut nodes).unwrap();
    drop(index);
    drop(nodes);
    let staged = retention::stage(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &target,
    )
    .unwrap();
    let mut nodes =
        MetadataStore::open(&case.profile.metadata_root, &staged.metadata_checkpoint).unwrap();
    let mut index = Index::open(&case.profile.index_root, &staged.index_checkpoint).unwrap();
    let record = work::read(&mut index, &staged.work, &nodes).unwrap();
    assert!(
        index
            .update_hook(&record.update, &mut nodes, |stage| {
                if stage == protection::Stage::Prepared {
                    Err(protection::Error::Unknown)
                } else {
                    Ok(())
                }
            })
            .is_err()
    );
    drop(index);
    drop(nodes);
    let controls = [
        ("packet.json", serde_json::to_vec(&fixture.packet).unwrap()),
        ("native.json", serde_json::to_vec(&case.source).unwrap()),
        ("profile.json", serde_json::to_vec(&case.profile).unwrap()),
        ("admission.json", receipt.canonical_bytes().unwrap()),
        ("result.json", serde_json::to_vec(&result).unwrap()),
        ("target.json", serde_json::to_vec(&target).unwrap()),
    ];
    for (name, raw) in &controls {
        fs::write(case.temp.path().join(name), raw).unwrap();
    }
    let invoke = |command: &str, suffix: &str| {
        let mut call = Command::new(env!("CARGO_BIN_EXE_reel-workflow-native"));
        call.arg(command);
        call.args(
            controls
                .iter()
                .take(5)
                .map(|(name, _)| case.temp.path().join(name)),
        );
        if command == "retain-response" {
            call.arg(case.temp.path().join("target.json"))
                .arg(case.temp.path().join(suffix))
                .arg("--apply");
        } else {
            call.arg(case.temp.path().join(suffix));
        }
        call.output().unwrap()
    };
    let first = invoke("retain-response", "ack-1.json");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stdout)
    );
    let repeated = invoke("retain-response", "ack-2.json");
    assert!(
        repeated.status.success(),
        "{}",
        String::from_utf8_lossy(&repeated.stdout)
    );
    let ack: retention::Acknowledgement =
        serde_json::from_slice(&fs::read(case.temp.path().join("ack-2.json")).unwrap()).unwrap();
    assert_eq!(ack.work, staged.work);
    assert_eq!(ack.evidence, staged.evidence);
    let verification = invoke("verify-retained", "ack-2.json");
    assert!(
        verification.status.success(),
        "{}",
        String::from_utf8_lossy(&verification.stdout)
    );
    for object in [&object, &Object::from_bytes(b"synthetic response")] {
        assert_eq!(
            case.protected(object),
            vec![target.actor.clone(), case.profile.scope.actor.clone()]
        );
    }
    assert_eq!(case.protected(&history), vec![target.actor.clone()]);
    let mut corrupted = ack.clone();
    corrupted.work = receipt.work.clone();
    assert!(
        retention::verify(
            &fixture.packet,
            &case.source,
            &case.profile,
            &receipt,
            &result,
            &corrupted
        )
        .is_err()
    );
    // Synthetic SDK mutation models another source generation, NOT qualified
    // scratch retirement or guarded slot close. Retained proof is historical.
    let mut nodes =
        MetadataStore::open(&case.profile.metadata_root, &ack.metadata_checkpoint).unwrap();
    let mut index = Index::open(&case.profile.index_root, &ack.index_checkpoint).unwrap();
    let old = index.actor(&mut nodes, &case.profile.scope.actor).unwrap();
    let old_entries = Tree::from_root(old.selection.root)
        .entries(&mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    let update = protection::Update {
        operation: tig_store::Key::parse("fixture-source-advance").unwrap(),
        actor: case.profile.scope.actor.clone(),
        owner: case.profile.scope.owner.clone(),
        expected_revision: old.revision,
        selection: Selection {
            root: Tree::empty(&mut nodes).unwrap().root().clone(),
            context: Digest::blob(b"new-generation"),
        },
        changes: old_entries.into_keys().map(|p| (p, None)).collect(),
        ready: true,
    };
    let work_ref = work::stage(&mut index, &update, None, &mut nodes).unwrap();
    work::execute(&mut index, &work_ref, &mut nodes).unwrap();
    drop(index);
    drop(nodes);
    // Historical readback must not take the former source job's actor fence.
    let busy_source = reel_assets::admission::lock(
        &case
            .profile
            .scope
            .coordination_root
            .join("tig-native-publication")
            .join(case.profile.index_identity.as_str())
            .join(Digest::blob(case.profile.scope.actor.as_str().as_bytes()).as_str()),
    )
    .unwrap();
    let historical = invoke("verify-retained", "ack-2.json");
    assert!(
        historical.status.success(),
        "{}",
        String::from_utf8_lossy(&historical.stdout)
    );
    let recovered_historical = invoke("retain-response", "ack-after-advance.json");
    assert!(
        recovered_historical.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered_historical.stdout)
    );
    drop(busy_source);
    assert!(
        Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
            .unwrap()
            .reopen(&receipt)
            .is_err()
    );
    if let Some(directory) = std::env::var_os("REEL_RETENTION_EVIDENCE_DIRECTORY") {
        use std::io::Write;
        let directory = PathBuf::from(directory);
        assert!(directory.is_absolute() && directory.is_dir());
        for (name, raw) in controls.iter().cloned().chain([
            ("acknowledgement.json", ack.canonical_bytes().unwrap()),
            ("retention-report.json", repeated.stdout),
            ("historical-verification.json", historical.stdout),
        ]) {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap();
            file.write_all(&raw).unwrap();
            file.sync_all().unwrap();
        }
    }
    // Model loss of retained coverage without deleting any payload. Restore it
    // through another explicit immutable SDK work record after the refusal.
    let mut nodes =
        MetadataStore::open(&case.profile.metadata_root, &ack.metadata_checkpoint).unwrap();
    let mut index = Index::open(&case.profile.index_root, &ack.index_checkpoint).unwrap();
    let current = index.actor(&mut nodes, &target.actor).unwrap();
    let original_selection = current.selection.clone();
    let mut next = Tree::from_root(current.selection.root)
        .entries(&mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    assert!(next.contains_key(&history_path));
    let output = Object::from_bytes(b"synthetic response");
    let changes: BTreeMap<_, _> = next
        .iter()
        .filter(|(_, o)| **o == output)
        .map(|(p, _)| (p.clone(), None))
        .collect();
    for path in changes.keys() {
        next.remove(path);
    }
    let update = protection::Update {
        operation: tig_store::Key::parse("fixture-lost-retained-coverage").unwrap(),
        actor: target.actor.clone(),
        owner: target.owner.clone(),
        expected_revision: current.revision,
        selection: Selection {
            root: Tree::build(&next, &mut nodes, TreeLimits::default())
                .unwrap()
                .0
                .root()
                .clone(),
            context: Digest::blob(b"lost-coverage"),
        },
        changes: changes.clone(),
        ready: true,
    };
    let work_ref = work::stage(&mut index, &update, None, &mut nodes).unwrap();
    work::execute(&mut index, &work_ref, &mut nodes).unwrap();
    drop(index);
    drop(nodes);
    assert!(
        retention::verify(
            &fixture.packet,
            &case.source,
            &case.profile,
            &receipt,
            &result,
            &ack
        )
        .is_err()
    );
    let mut nodes =
        MetadataStore::open(&case.profile.metadata_root, &ack.metadata_checkpoint).unwrap();
    let mut index = Index::open(&case.profile.index_root, &ack.index_checkpoint).unwrap();
    let current = index.actor(&mut nodes, &target.actor).unwrap();
    let update = protection::Update {
        operation: tig_store::Key::parse("fixture-restore-retained-coverage").unwrap(),
        actor: target.actor.clone(),
        owner: target.owner.clone(),
        expected_revision: current.revision,
        selection: original_selection,
        changes: changes
            .into_keys()
            .map(|p| (p, Some(output.clone())))
            .collect(),
        ready: true,
    };
    let work_ref = work::stage(&mut index, &update, None, &mut nodes).unwrap();
    work::execute(&mut index, &work_ref, &mut nodes).unwrap();
    drop(index);
    drop(nodes);
    retention::verify(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &ack,
    )
    .unwrap();
    if std::env::var_os("REEL_RETENTION_EVIDENCE_DIRECTORY").is_some() {
        let _ = case.temp.keep();
        let _ = fixture.temp.keep();
    }
}

#[cfg(windows)]
fn handoff_profile(
    case: &AdmissionCase,
    receipt: reel_workflow_native::admission::Receipt,
    executable: PathBuf,
) -> reel_workflow_native::handoff::Profile {
    let raw = fs::read(&executable).unwrap();
    let controls = case.temp.path().join("acknowledgements");
    fs::create_dir(&controls).unwrap();
    reel_workflow_native::handoff::Profile {
        schema: "reel.workflow-native-handoff.v1".into(),
        source: serde_json::from_slice(&serde_json::to_vec(&case.source).unwrap()).unwrap(),
        admission: case.profile.clone(),
        binding: reel_workflow_native::handoff::Binding {
            schema: "reel.workflow-native-handoff-binding.v1".into(),
            admission: receipt,
            target: retained_target(),
        },
        runtime: Tool {
            path: executable,
            sha256: digest(&raw),
            bytes: raw.len() as u64,
        },
        acknowledgements: controls,
    }
}

#[cfg(windows)]
#[test]
fn native_handoff_records_intent_and_preserves_claims_when_provider_is_unavailable() {
    use reel_workflow::handoff::{Journal, State};
    use reel_workflow_native::{admission::Prepared, handoff};
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let profile = handoff_profile(&case, receipt, std::env::current_exe().unwrap());
    let journal = case.temp.path().join("journal");
    assert_eq!(
        handoff::create(&journal, &fixture.packet, &profile).unwrap(),
        State::Waiting
    );
    let (result, returns) = response_fixture(&fixture.packet, &case.temp.path().join("returns"));
    assert!(handoff::import(&journal, &profile, result.clone(), &returns).is_err());
    let observed = Journal::open(&journal).unwrap();
    assert_eq!(observed.state(), State::ImportPending);
    assert_eq!(observed.result(), Some(&result));
    assert!(observed.acknowledgement().is_none());
    assert!(handoff::resume(&journal, &profile, None).is_err());
    assert_eq!(
        Journal::open(&journal).unwrap().state(),
        State::ImportPending
    );
    assert_eq!(
        case.protected(&Object::from_bytes(b"synthetic input")),
        vec![case.profile.scope.actor.clone()]
    );
    assert!(!case.source.source.pool.repository.exists());
}

#[cfg(windows)]
#[test]
fn native_handoff_pins_the_actual_runtime_target_and_control_namespaces() {
    use reel_workflow::handoff::{Journal, State};
    use reel_workflow_native::{admission::Prepared, handoff};
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let mut profile = handoff_profile(&case, receipt, fixture.profile.git.path.clone());
    let journal = case.temp.path().join("journal");
    assert_eq!(
        handoff::create(&journal, &fixture.packet, &profile)
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
    assert!(!journal.exists());
    let executable = std::env::current_exe().unwrap();
    let raw = fs::read(&executable).unwrap();
    profile.runtime = Tool {
        path: executable,
        sha256: digest(&raw),
        bytes: raw.len() as u64,
    };
    assert_eq!(
        handoff::create(&journal, &fixture.packet, &profile).unwrap(),
        State::Waiting
    );
    let original = profile.binding.target.owner.clone();
    profile.binding.target.owner = tig_store::Key::parse("different-owner").unwrap();
    assert_eq!(
        handoff::resume(&journal, &profile, None).unwrap_err().kind,
        ErrorKind::Conflict
    );
    profile.binding.target.owner = original;
    profile.runtime.sha256 = "0".repeat(64);
    assert_eq!(
        handoff::resume(&journal, &profile, None).unwrap_err().kind,
        ErrorKind::Conflict
    );
    assert_eq!(Journal::open(&journal).unwrap().state(), State::Waiting);
    let controls = profile.acknowledgements.clone();
    profile.acknowledgements = case.profile.metadata_root.clone();
    assert!(handoff::resume(&journal, &profile, None).is_err());
    profile.acknowledgements = journal;
    assert!(handoff::resume(&profile.acknowledgements, &profile, None).is_err());
    profile.acknowledgements = controls;
}

#[cfg(windows)]
#[test]
#[ignore = "Existing synthetic native pool only: journal/native Prepare/lost completion recovery, no project assets or cleanup"]
fn native_handoff_completes_only_after_retained_proof_and_recovers_lost_completion() {
    use reel_workflow::handoff::{Journal, State};
    use reel_workflow_native::{NativeProfile, admission::Prepared, response};
    use tig_store::{
        metadata::MetadataStore,
        source_claims::protection::{self, Index, work},
    };
    let path =
        std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("Set REEL_NATIVE_INPUT_FIXTURE");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let object: Object = serde_json::from_value(value["object"].clone()).unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(value["source"].clone()).unwrap();
    let annex: Tool = serde_json::from_value(value["annex"].clone()).unwrap();
    let mut fixture = Fixture::with_object(object.clone());
    let git_raw = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&git_raw),
        bytes: git_raw.len() as u64,
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
    let executable = std::env::var_os("REEL_NATIVE_HANDOFF_TOOL")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_reel-workflow-native")));
    let profile = handoff_profile(&case, receipt.clone(), executable.clone());
    let (result, returns) = response_fixture(&fixture.packet, &case.temp.path().join("returns"));
    let packet_file = case.temp.path().join("packet.json");
    let profile_file = case.temp.path().join("handoff.json");
    let result_file = case.temp.path().join("result.json");
    let returns_file = case.temp.path().join("returns.json");
    let missing_file = case.temp.path().join("missing-returns.json");
    let mut missing = returns.clone();
    missing.files.insert("note".into(), "missing.txt".into());
    for (path, raw) in [
        (&packet_file, serde_json::to_vec(&fixture.packet).unwrap()),
        (&profile_file, serde_json::to_vec(&profile).unwrap()),
        (&result_file, serde_json::to_vec(&result).unwrap()),
        (&returns_file, serde_json::to_vec(&returns).unwrap()),
        (&missing_file, serde_json::to_vec(&missing).unwrap()),
    ] {
        fs::write(path, raw).unwrap();
    }
    let journal = case.temp.path().join("journal");
    let created = Command::new(&executable)
        .arg("create-handoff")
        .arg(&packet_file)
        .arg(&profile_file)
        .arg(&journal)
        .arg("--apply")
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stdout)
    );
    let failed = Command::new(&executable)
        .arg("import-handoff")
        .arg(&journal)
        .arg(&profile_file)
        .arg(&result_file)
        .arg(&missing_file)
        .arg("--apply")
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert_eq!(
        Journal::open(&journal).unwrap().state(),
        State::ImportPending
    );
    assert_eq!(
        case.protected(&object),
        vec![case.profile.scope.actor.clone()]
    );
    // Stage original response work, then interrupt its native Prepare. Reopening
    // the journal must be passive; only original SDK reconciliation may finish.
    let staged = response::stage(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &returns,
    )
    .unwrap();
    let mut nodes =
        MetadataStore::open(&case.profile.metadata_root, &case.profile.metadata_minimum).unwrap();
    let mut index = Index::open(&case.profile.index_root, &case.profile.index_minimum).unwrap();
    let record = work::read(&mut index, &staged.work, &nodes).unwrap();
    let mut interrupted = false;
    assert!(
        index
            .update_hook(&record.update, &mut nodes, |stage| {
                if stage == protection::Stage::Prepared {
                    interrupted = true;
                    Err(protection::Error::Unknown)
                } else {
                    Ok(())
                }
            })
            .is_err()
    );
    assert!(interrupted);
    drop(index);
    drop(nodes);
    let resume = |directory: &Path, returned: &Path| {
        Command::new(&executable)
            .arg("resume-handoff")
            .arg(directory)
            .arg(&profile_file)
            .arg(returned)
            .arg("--apply")
            .output()
            .unwrap()
    };
    let completed = resume(&journal, &returns_file);
    if !completed.status.success() {
        eprintln!(
            "Synthetic diagnostic control root: {}",
            case.temp.path().display()
        );
        let _ = case.temp.keep();
        let _ = fixture.temp.keep();
        panic!("{}", String::from_utf8_lossy(&completed.stdout));
    }
    assert!(
        completed.status.success(),
        "{}",
        String::from_utf8_lossy(&completed.stdout)
    );
    let report: serde_json::Value = serde_json::from_slice(&completed.stdout).unwrap();
    assert_eq!(report["task_completed"], true);
    assert_eq!(report["selected"], false);
    assert_eq!(report["input_claims_released"], false);
    assert_eq!(report["scratch_retirement_authorized"], false);
    let complete = Journal::open(&journal).unwrap();
    assert_eq!(complete.state(), State::Retained);
    let proof = complete.acknowledgement().unwrap().clone();
    assert_eq!(
        case.protected(&object),
        vec![
            profile.binding.target.actor.clone(),
            case.profile.scope.actor.clone()
        ]
    );
    let original_index = Index::open(&case.profile.index_root, &case.profile.index_minimum)
        .unwrap()
        .checkpoint();
    // A new private control copy of only the durable prefix models a lost final
    // journal append. Original records and native publications remain untouched.
    let lost_reply = case.temp.path().join("lost-reply");
    fs::create_dir(&lost_reply).unwrap();
    for name in ["00000.json", "00001.json"] {
        fs::copy(journal.join(name), lost_reply.join(name)).unwrap();
    }
    let recovered = resume(&lost_reply, Path::new("-"));
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stdout)
    );
    assert_eq!(
        Journal::open(&lost_reply).unwrap().acknowledgement(),
        Some(&proof)
    );
    assert_eq!(
        Index::open(&case.profile.index_root, &original_index)
            .unwrap()
            .checkpoint(),
        original_index
    );
    let busy_source = reel_assets::admission::lock(
        &case
            .profile
            .scope
            .coordination_root
            .join("tig-native-publication")
            .join(case.profile.index_identity.as_str())
            .join(tig_core::Digest::blob(case.profile.scope.actor.as_str().as_bytes()).as_str()),
    )
    .unwrap();
    let verified = resume(&journal, Path::new("-"));
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stdout)
    );
    drop(busy_source);
    let ack_path = profile.acknowledgements.join(format!(
        "retained-{}-attempt-1.json",
        fixture.packet.task_key
    ));
    let ack_bytes = fs::read(&ack_path).unwrap();
    fs::write(&ack_path, b"synthetic interrupted control").unwrap();
    assert!(!resume(&journal, Path::new("-")).status.success());
    assert_eq!(Journal::open(&journal).unwrap().state(), State::Retained);
    fs::write(&ack_path, &ack_bytes).unwrap();
    if let Some(directory) = std::env::var_os("REEL_HANDOFF_EVIDENCE_DIRECTORY") {
        use std::io::Write;
        let directory = PathBuf::from(directory);
        assert!(directory.is_absolute() && directory.is_dir());
        for (name, raw) in [
            ("packet.json", fs::read(&packet_file).unwrap()),
            ("profile.json", fs::read(&profile_file).unwrap()),
            ("result.json", fs::read(&result_file).unwrap()),
            ("returns.json", fs::read(&returns_file).unwrap()),
            ("completion-report.json", completed.stdout),
            ("recovery-report.json", recovered.stdout),
            ("verification-report.json", verified.stdout),
            (
                "locations.json",
                serde_json::to_vec(&json!({"journal":journal,"recovered_journal":lost_reply}))
                    .unwrap(),
            ),
        ] {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap();
            file.write_all(&raw).unwrap();
            file.sync_all().unwrap();
        }
        let _ = case.temp.keep();
        let _ = fixture.temp.keep();
    }
}

#[cfg(windows)]
#[test]
fn staged_retention_refuses_lost_original_response_coverage_before_new_admission() {
    use reel_workflow_native::{admission::Prepared, response, retention};
    use tig_core::tree::{Tree, TreeLimits};
    use tig_store::{
        metadata::MetadataStore,
        source_claims::protection::{self, Index, work},
    };
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let (result, returns) = response_fixture(&fixture.packet, &case.temp.path().join("returns"));
    response::register(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &returns,
    )
    .unwrap();
    let staged = retention::stage(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &retained_target(),
    )
    .unwrap();
    let mut nodes =
        MetadataStore::open(&case.profile.metadata_root, &staged.metadata_checkpoint).unwrap();
    let mut index = Index::open(&case.profile.index_root, &staged.index_checkpoint).unwrap();
    let current = index.actor(&mut nodes, &case.profile.scope.actor).unwrap();
    let old = Tree::from_root(current.selection.root)
        .entries(&mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    let original_input = work::read(&mut index, &receipt.work, &nodes)
        .unwrap()
        .update
        .selection;
    let next = Tree::from_root(original_input.root.clone())
        .entries(&mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    let mut changes = BTreeMap::new();
    for (path, object) in &next {
        if old.get(path) != Some(object) {
            changes.insert(path.clone(), Some(object.clone()));
        }
    }
    for path in old.keys() {
        if !next.contains_key(path) {
            changes.insert(path.clone(), None);
        }
    }
    let update = protection::Update {
        operation: tig_store::Key::parse("fixture-response-coverage-loss").unwrap(),
        actor: case.profile.scope.actor.clone(),
        owner: case.profile.scope.owner.clone(),
        expected_revision: current.revision,
        selection: original_input,
        changes,
        ready: true,
    };
    let reference = work::stage(&mut index, &update, None, &mut nodes).unwrap();
    work::execute(&mut index, &reference, &mut nodes).unwrap();
    drop(index);
    drop(nodes);
    Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .reopen(&receipt)
        .unwrap();
    assert_eq!(
        retention::stage(
            &fixture.packet,
            &case.source,
            &case.profile,
            &receipt,
            &result,
            &retained_target()
        )
        .unwrap_err()
        .kind,
        ErrorKind::Stale
    );
    assert!(
        case.protected(&Object::from_bytes(b"synthetic response"))
            .is_empty()
    );
    assert_eq!(
        case.protected(&Object::from_bytes(b"synthetic input")),
        vec![case.profile.scope.actor.clone()]
    );
}

#[cfg(windows)]
#[test]
#[ignore = "Existing synthetic native pool and explicit evidence directory only; preserves original roots and runtime, no initialization or production profile adoption"]
fn native_retained_close_recovers_original_staged_handoff_and_preserves_history() {
    use reel_workflow::handoff::{Journal, State};
    use reel_workflow_native::{NativeProfile, admission::Prepared, closing};
    let evidence = PathBuf::from(
        std::env::var_os("REEL_NATIVE_CLOSE_EVIDENCE")
            .expect("Explicit new evidence directory required"),
    );
    assert!(evidence.is_absolute() && evidence.is_dir());
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").unwrap()).unwrap(),
    )
    .unwrap();
    let object: Object = serde_json::from_value(value["object"].clone()).unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(value["source"].clone()).unwrap();
    let mut fixture = Fixture::with_object(object.clone());
    fixture.temp.disable_cleanup(true);
    let git_raw = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&git_raw),
        bytes: git_raw.len() as u64,
    };
    let mut case = AdmissionCase::with_store(&fixture, native.store.clone());
    case.temp.disable_cleanup(true);
    let control = case.source.source.pool.control.clone();
    case.source = NativeProfile {
        snapshot: fixture.profile.clone(),
        source: native,
        annex: serde_json::from_value(value["annex"].clone()).unwrap(),
    };
    case.source.source.pool.control = control;
    let journal = case.temp.path().join("journal");
    let mut saved = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(evidence.join("locations.json"))
        .unwrap();
    use std::io::Write;
    saved
        .write_all(
            &serde_json::to_vec(
                &json!({"case":case.temp.path(),"source":fixture.temp.path(),"journal":journal}),
            )
            .unwrap(),
        )
        .unwrap();
    saved.sync_all().unwrap();
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let executable = std::env::var_os("REEL_NATIVE_HANDOFF_TOOL")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_reel-workflow-native")));
    let profile = handoff_profile(&case, receipt.clone(), executable.clone());
    let (result, returns) = response_fixture(&fixture.packet, &case.temp.path().join("returns"));
    for (name, raw) in [
        ("packet.json", serde_json::to_vec(&fixture.packet).unwrap()),
        ("profile.json", serde_json::to_vec(&profile).unwrap()),
        ("result.json", serde_json::to_vec(&result).unwrap()),
        ("returns.json", serde_json::to_vec(&returns).unwrap()),
    ] {
        fs::write(case.temp.path().join(name), raw).unwrap();
    }
    let call = |args: Vec<std::ffi::OsString>| {
        let output = Command::new(&executable).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    call(vec![
        "create-handoff".into(),
        case.temp.path().join("packet.json").into(),
        case.temp.path().join("profile.json").into(),
        journal.clone().into(),
        "--apply".into(),
    ]);
    call(vec![
        "import-handoff".into(),
        journal.clone().into(),
        case.temp.path().join("profile.json").into(),
        case.temp.path().join("result.json").into(),
        case.temp.path().join("returns.json").into(),
        "--apply".into(),
    ]);
    assert_eq!(Journal::open(&journal).unwrap().state(), State::Retained);
    let ack_path = fs::read_dir(&profile.acknowledgements)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .unwrap();
    let acknowledgement = serde_json::from_slice(&fs::read(ack_path).unwrap()).unwrap();
    let original_lifecycle_tool = Journal::open(&journal)
        .unwrap()
        .ticket()
        .lifecycle_tool
        .clone();
    let request = closing::Request {
        schema: "reel.workflow-native-close.v1".into(),
        journal,
        packet: fixture.packet.clone(),
        source: serde_json::from_slice(&serde_json::to_vec(&case.source).unwrap()).unwrap(),
        admission: case.profile.clone(),
        receipt: receipt.clone(),
        result: result.clone(),
        acknowledgement,
        operation: tig_store::Key::parse("close-original").unwrap(),
        original_lifecycle_tool,
        runtime: profile.runtime.clone(),
    };
    let bytes = serde_json::to_vec(&request).unwrap();
    let pin = digest(&bytes);
    let request_path = case.temp.path().join("close.json");
    fs::write(&request_path, &bytes).unwrap();
    let preview = call(vec![
        "preview-close".into(),
        request_path.clone().into(),
        pin.clone().into(),
    ]);
    assert_eq!(preview["applied"], false);
    let staged = call(vec![
        "stage-close".into(),
        request_path.clone().into(),
        pin.clone().into(),
        case.temp.path().join("staged.json").into(),
        "--apply".into(),
    ]);
    assert_eq!(staged["original_clear_complete"], false);
    // Model a real crash in the original handoff's native Prepare, not only a
    // stopped CLI reply. Recovery must finish THAT WorkRef before source clear.
    {
        use tig_store::{
            metadata::MetadataStore,
            source_claims::protection::{self, Index, work},
        };
        let outcome: closing::Outcome =
            serde_json::from_slice(&fs::read(case.temp.path().join("staged.json")).unwrap())
                .unwrap();
        let mut nodes =
            MetadataStore::open(&case.profile.metadata_root, &outcome.metadata_checkpoint).unwrap();
        let mut index = Index::open(&case.profile.index_root, &outcome.index_checkpoint).unwrap();
        let record = work::read(&mut index, outcome.handoff.as_ref().unwrap(), &nodes).unwrap();
        let mut interrupted = false;
        assert!(
            index
                .update_hook(&record.update, &mut nodes, |stage| {
                    if stage == protection::Stage::Prepared {
                        interrupted = true;
                        Err(protection::Error::Unknown)
                    } else {
                        Ok(())
                    }
                })
                .is_err()
        );
        assert!(interrupted);
    }
    let recovered = call(vec![
        "recover-close".into(),
        request_path.clone().into(),
        pin.clone().into(),
        case.temp.path().join("closed.json").into(),
        "--apply".into(),
    ]);
    assert_eq!(recovered["slot_idle"], true);
    assert_eq!(recovered["original_clear_complete"], true);
    let replay = call(vec![
        "recover-close".into(),
        request_path.clone().into(),
        pin.into(),
        case.temp.path().join("replay.json").into(),
        "--apply".into(),
    ]);
    assert_eq!(replay["slot_idle"], true);
    assert_eq!(
        case.protected(&object),
        vec![profile.binding.target.actor.clone()]
    );
    assert_eq!(fs::read(request_path).unwrap(), bytes);
    assert_eq!(
        request.receipt.canonical_bytes().unwrap(),
        receipt.canonical_bytes().unwrap()
    );
    let _history = reel_workflow_native::retention::verify(
        &request.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &request.acknowledgement,
    )
    .unwrap();
    fs::write(evidence.join("complete.json"),serde_json::to_vec(&json!({"preview":preview,"staged":staged,"recovered":recovered,"replay":replay,"historical_native_bytes_verified":true})).unwrap()).unwrap();
}

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
        Self::with_extra_source_store(fixture, store, None)
    }
    fn with_extra_source_store(
        fixture: &Fixture,
        store: tig_core::Digest,
        extra: Option<tig_core::Digest>,
    ) -> Self {
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
        let source_stores: Vec<_> = std::iter::once(store.clone()).chain(extra).collect();
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
                stores: source_stores.clone(),
                actors: ["main", "retained", "slot"]
                    .into_iter()
                    .map(|id| Actor {
                        id: key(id),
                        owner: key("owner"),
                        stores: if id == "slot" {
                            source_stores.clone()
                        } else {
                            vec![store.clone()]
                        },
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
            source_stores
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
fn explicit_large_input_metadata_uses_a_new_task_bound_budget_and_refuses_unqualified_admission() {
    use reel_workflow_native::{SnapshotBudget, admission::Prepared};
    let object = Object {
        sha256: tig_core::Digest::blob(b"synthetic large pointer only"),
        bytes: 3_600_000_001,
    };
    let mut fixture = Fixture::with_object(object.clone());
    assert_eq!(
        SnapshotInputs::open(&fixture.packet, &fixture.profile)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Budget
    );
    let old_key = fixture.packet.task_key.clone();
    fixture.profile.schema = "reel.workflow-native-snapshot.v2".into();
    fixture.profile.budget = Some(SnapshotBudget {
        schema: "reel.workflow-native-input-budget.v1".into(),
        input_bytes: object.bytes,
        input_object_bytes: object.bytes,
    });
    fixture.repin();
    assert_ne!(fixture.packet.task_key, old_key);
    assert_eq!(
        SnapshotInputs::open(&fixture.packet, &fixture.profile)
            .unwrap()
            .input_count(),
        1
    );
    let mut case = AdmissionCase::new(&fixture);
    case.source.source.maximum_bytes = object.bytes;
    let before = fs::read(case.profile.metadata_root.join("metadata.json")).unwrap();
    assert_eq!(
        Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
            .unwrap()
            .accept()
            .err()
            .unwrap()
            .kind,
        ErrorKind::Budget
    );
    assert!(case.protected(&object).is_empty());
    assert_eq!(
        before,
        fs::read(case.profile.metadata_root.join("metadata.json")).unwrap()
    );
    fixture.profile.budget.as_mut().unwrap().input_object_bytes -= 1;
    fixture.repin();
    assert_eq!(
        SnapshotInputs::open(&fixture.packet, &fixture.profile)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Budget
    );
}

#[test]
fn response_close_preflight_refuses_before_admission_even_when_inputs_fit() {
    use reel_workflow_native::admission::Prepared;
    let mut fixture = Fixture::new();
    fixture.packet.task.outputs = (0..128)
        .map(|i| {
            (
                format!("output-{i}"),
                OutputContract {
                    media_type: "text/plain".into(),
                    max_bytes: 1024,
                    required: true,
                },
            )
        })
        .collect();
    fixture.repin();
    let case = AdmissionCase::new(&fixture);
    let before = fs::read(case.profile.metadata_root.join("metadata.json")).unwrap();
    let prepared =
        Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1").unwrap();
    let preview = prepared.preview().unwrap();
    assert!(!preview.response_close_bound_ok);
    assert!(preview.response_close_paths_upper_bound > preview.changed_path_limit);
    assert_eq!(
        prepared.accept().unwrap_err().code,
        "admission-response-close-path-bound"
    );
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
fn response_and_reader_budgets_refuse_before_claims_or_metadata_writes() {
    use reel_workflow_native::{SnapshotBudget, admission::Prepared};
    for mode in [
        "objects",
        "response-bytes",
        "combined-bytes",
        "reader-input",
        "reader-output",
    ] {
        let input_bytes = if mode == "combined-bytes" {
            900 * 1024 * 1024
        } else {
            17
        };
        let object = if mode == "combined-bytes" {
            Object {
                sha256: tig_core::Digest::blob(b"synthetic large input pointer"),
                bytes: input_bytes,
            }
        } else {
            Object::from_bytes(b"synthetic input")
        };
        let mut fixture = Fixture::with_object(object.clone());
        if mode == "combined-bytes" {
            fixture.profile.schema = "reel.workflow-native-snapshot.v2".into();
            fixture.profile.budget = Some(SnapshotBudget {
                schema: "reel.workflow-native-input-budget.v1".into(),
                input_bytes,
                input_object_bytes: input_bytes,
            });
        }
        let count = if mode == "objects" { 33 } else { 1 };
        let maximum = match mode {
            "response-bytes" => 128 * 1024 * 1024 + 1,
            "combined-bytes" => 128 * 1024 * 1024,
            "reader-output" => 2048,
            _ => 1024,
        };
        fixture.packet.task.outputs = (0..count)
            .map(|i| {
                (
                    format!("output-{i}"),
                    OutputContract {
                        media_type: "text/plain".into(),
                        max_bytes: maximum,
                        required: true,
                    },
                )
            })
            .collect();
        fixture.repin();
        let mut case = AdmissionCase::new(&fixture);
        case.source.source.maximum_bytes = match mode {
            "reader-input" => 1,
            "reader-output" => 1024,
            _ => 64 * 1024 * 1024 * 1024,
        };
        let before = fs::read(case.profile.metadata_root.join("metadata.json")).unwrap();
        if mode == "reader-input" {
            assert_eq!(
                Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
                    .err()
                    .unwrap()
                    .code,
                "admission-native-reader-bound"
            );
        } else {
            let prepared =
                Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1").unwrap();
            assert!(!prepared.preview().unwrap().response_budget_ok, "{mode}");
            assert_eq!(
                prepared.accept().unwrap_err().kind,
                ErrorKind::Budget,
                "{mode}"
            );
        }
        assert!(case.protected(&object).is_empty(), "{mode}");
        assert_eq!(
            before,
            fs::read(case.profile.metadata_root.join("metadata.json")).unwrap(),
            "{mode}"
        );
    }
}

#[cfg(windows)]
#[test]
fn native_film_inspection_refuses_wrong_project_snapshot_or_byte_identical_record_alias_before_reads()
 {
    let fixture = Fixture::new();
    let mut case = AdmissionCase::new(&fixture);
    // Held existing executable for pre-read refusals only; never invoked as
    // annex. This fixture intentionally contains no native payload.
    case.source.annex = fixture.profile.git.clone();
    case.source.source.pool.executable = case.source.annex.path.clone();
    let control = json!({"path":"control.json","artifact":fixture.packet.task.inputs[0]});
    let request = json!({"schema":"reel.workflow-film-inspection.v1","project":fixture.packet.project,
        "source_commit":fixture.packet.source_commit,"episode":control,"scene":control,
        "catalog":control,"policy":control,"scopes":[control,control,control],"identities":{},"cues":{},"approvals":[],
        "direction":{"desired_seconds_per_cel":[1,3],"visual_school":"synthetic","performance_style":"held",
        "animation_technique":"held-cels","renderer":fixture.packet.task.inputs[0],"settings":control}});
    for case_name in ["project", "commit", "alias"] {
        let mut changed = request.clone();
        match case_name {
            "project" => changed["project"] = json!("another-project"),
            "commit" => changed["source_commit"] = json!("b".repeat(40)),
            "alias" => {
                changed["scene"]["artifact"]["logical_id"] =
                    json!("byte-identical/undeclared-record")
            }
            _ => unreachable!(),
        }
        let changed = serde_json::from_value(changed).unwrap();
        let mut inputs =
            reel_workflow_native::NativeInputs::open(&fixture.packet, &case.source).unwrap();
        let refused =
            reel_workflow_native::film_metadata::inspect(&changed, &mut inputs).unwrap_err();
        assert_eq!(
            refused.kind,
            if case_name == "alias" {
                ErrorKind::Conflict
            } else {
                ErrorKind::Stale
            }
        );
    }
    assert!(
        case.protected(&Object::from_bytes(b"synthetic input"))
            .is_empty()
    );
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
            task_snapshots: Default::default(),
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
