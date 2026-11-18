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
        fs::write(
            directory.join("packet.json"),
            serde_json::to_vec_pretty(&fixture.packet).unwrap(),
        )
        .unwrap();
        fs::write(
            directory.join("snapshot-profile.json"),
            serde_json::to_vec_pretty(&fixture.profile).unwrap(),
        )
        .unwrap();
        // Preserve this synthetic Git source for exact replay; no asset cache created.
        let _ = fixture.temp.keep();
    }
}
