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
#[cfg(windows)]
#[path = "support/automatic_dispatch.rs"]
mod automatic_dispatch;
#[cfg(windows)]
#[path = "support/close_native.rs"]
mod close_native;
#[cfg(windows)]
#[path = "support/direct_review.rs"]
mod direct_review_native;
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../../reel-workflow/tests/support/film_fixture.rs"]
mod film_fixture;
#[cfg(all(windows, feature = "film"))]
#[path = "support/film_inputs.rs"]
mod film_inputs;
#[cfg(windows)]
#[path = "support/film_native.rs"]
mod film_native;
#[cfg(all(windows, feature = "film"))]
#[path = "support/film_process.rs"]
mod film_process;
#[cfg(windows)]
#[path = "support/frozen_expansion.rs"]
mod frozen_expansion;
#[cfg(windows)]
#[path = "support/image_handoff.rs"]
mod image_handoff_native;
#[cfg(all(windows, feature = "film"))]
#[path = "support/movie_close.rs"]
mod movie_close;
#[cfg(all(windows, feature = "film"))]
#[path = "support/movie_native.rs"]
mod movie_native;
#[cfg(all(windows, feature = "film"))]
#[path = "support/movie_verify.rs"]
mod movie_verify;
#[cfg(windows)]
#[path = "support/provider_native.rs"]
mod provider_native;
#[cfg(windows)]
#[path = "support/repair_comic.rs"]
mod repair_comic_native;
#[cfg(windows)]
#[path = "support/repair_plan.rs"]
mod repair_plan_native;
#[cfg(windows)]
#[path = "support/resolution.rs"]
mod resolution_native;
#[cfg(windows)]
#[path = "support/review_brief.rs"]
mod review_brief_native;
#[cfg(windows)]
#[path = "support/review_control.rs"]
mod review_control_native;
#[cfg(all(windows, feature = "film"))]
#[path = "support/review_export.rs"]
mod review_export;
#[cfg(windows)]
#[path = "support/review_native.rs"]
mod review_native;
#[cfg(windows)]
#[path = "support/runner_native.rs"]
mod runner_native;
#[cfg(windows)]
#[path = "support/tracker_draft.rs"]
mod tracker_draft_native;
#[cfg(windows)]
use close_native::exercise_native_close;
#[cfg(windows)]
#[path = "support/issue_native.rs"]
mod issue_native;

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
            upstream: None,
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
            upstream_inputs: None,
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
        let id = task.id.clone();
        task.native_inputs = Some(Self::declaration(&self.profile));
        self.packet = make_packet(
            &Plan {
                schema: PLAN_SCHEMA.into(),
                project: self.packet.project.clone(),
                source_commit: self.profile.commit.clone(),
                tasks: vec![task],
            },
            &id,
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
        movie_verifier: None,
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
    fixture.packet.task.outputs.insert(
        "mirror".into(),
        OutputContract {
            media_type: "text/plain".into(),
            max_bytes: 100,
            required: true,
        },
    );
    fixture.repin();
    let mut case = AdmissionCase::with_store(&fixture, native.store.clone());
    let control = case.source.source.pool.control.clone();
    case.source = NativeProfile {
        movie_verifier: None,
        snapshot: fixture.profile.clone(),
        annex,
        source: native,
    };
    case.source.source.pool.control = control;
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let (mut result, mut returns) =
        response_fixture(&fixture.packet, &case.temp.path().join("returns"));
    let mut mirror = result.artifacts["note"].clone();
    mirror.logical_id = "candidate/mirror".into();
    result.artifacts.insert("mirror".into(), mirror);
    returns.files.insert("mirror".into(), "mirror.txt".into());
    fs::write(returns.root.join("mirror.txt"), b"synthetic response").unwrap();
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
    drop(reopened);
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
    let retained = reel_workflow_native::retention::retain(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &retained_target(),
    )
    .unwrap();
    reel_workflow_native::retention::verify(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &retained,
    )
    .unwrap();
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
        movie_verifier: None,
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
fn exercise_waiting_graph(
    case: &AdmissionCase,
    packet: &Packet,
    profile: &reel_workflow_native::handoff::Profile,
    journal: &Path,
    profile_file: &Path,
    with_child: bool,
) -> (
    reel_workflow::Plan,
    reel_workflow_native::graph::Bindings,
    PathBuf,
) {
    use reel_workflow_native::{
        graph::{Binding, Bindings, PinnedFile},
        staging,
    };
    let mut tasks = vec![packet.task.clone()];
    if let Some(upstream) = &profile.source.snapshot.upstream {
        tasks.splice(
            0..0,
            upstream.parents.values().map(|p| p.packet.task.clone()),
        );
    }
    if with_child {
        let mut child = packet.task.clone();
        child.id = "downstream".into();
        child.depends_on = vec![packet.task.id.clone()];
        child.prompt = "Synthetic ordered downstream task.".into();
        tasks.push(child);
    }
    let plan = reel_workflow::Plan {
        schema: reel_workflow::PLAN_SCHEMA.into(),
        project: packet.project.clone(),
        source_commit: packet.source_commit.clone(),
        tasks,
    };
    let mut bindings = Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::from([(
            packet.task.id.clone(),
            Binding {
                journal: journal.into(),
                profile: PinnedFile {
                    path: profile_file.into(),
                    sha256: digest(&fs::read(profile_file).unwrap()),
                },
                staging_ticket: None,
            },
        )]),
    };
    if profile.source.snapshot.upstream.is_some() {
        let parents: Bindings = serde_json::from_slice(
            &fs::read(case.temp.path().join("graph-parent-bindings.json")).unwrap(),
        )
        .unwrap();
        bindings.tasks.extend(parents.tasks);
    }
    let frontier = reel_workflow_native::graph::inspect(&plan, &bindings).unwrap();
    assert_eq!(
        frontier
            .rows
            .iter()
            .find(|r| r.task_id == packet.task.id)
            .unwrap()
            .status,
        reel_workflow::graph::Status::Waiting
    );
    if with_child {
        assert_eq!(
            frontier.rows[1].status,
            reel_workflow::graph::Status::Blocked
        );
    }
    let output = case.temp.path().join("ready-batch.json");
    assert!(
        reel_workflow_native::graph::export_ready(
            &plan,
            &bindings,
            std::slice::from_ref(&packet.task.id),
            &output
        )
        .is_err()
    );
    assert!(!output.exists());
    let parent = case.temp.path().join("graph-scratch");
    fs::create_dir(&parent).unwrap();
    let stage = staging::prepare(
        packet,
        &profile.source,
        &profile.admission,
        &profile.binding.admission,
        &staging::Profile {
            schema: "reel.workflow-staging.v1".into(),
            parent,
            maximum_bytes: 8 * 1024 * 1024,
            capacity_policy: reel_assets::admission::Policy {
                minimum_free_bytes: 65536,
                minimum_free_basis_points: 0,
                max_object_bytes: 512 * 1024 * 1024,
                ..Default::default()
            },
        },
    )
    .unwrap();
    let raw = stage.canonical_bytes().unwrap();
    let stage_file = case.temp.path().join("graph-stage.json");
    fs::write(&stage_file, &raw).unwrap();
    bindings
        .tasks
        .get_mut(&packet.task.id)
        .unwrap()
        .staging_ticket = Some(PinnedFile {
        path: stage_file,
        sha256: digest(&raw),
    });
    (plan, bindings, output)
}

#[cfg(windows)]
#[test]
fn graph_cannot_export_from_claims_or_local_controls_when_native_bytes_are_missing() {
    use reel_workflow_native::{admission::Prepared, handoff};
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let profile = handoff_profile(&case, receipt, std::env::current_exe().unwrap());
    let journal = case.temp.path().join("journal");
    handoff::create(&journal, &fixture.packet, &profile).unwrap();
    let profile_file = case.temp.path().join("graph-profile.json");
    fs::write(&profile_file, serde_json::to_vec(&profile).unwrap()).unwrap();
    let (plan, bindings, output) = exercise_waiting_graph(
        &case,
        &fixture.packet,
        &profile,
        &journal,
        &profile_file,
        false,
    );
    assert!(
        reel_workflow_native::graph::export_ready(
            &plan,
            &bindings,
            std::slice::from_ref(&fixture.packet.task.id),
            &output
        )
        .is_err()
    );
    assert!(!output.exists());
    assert_eq!(
        reel_workflow::handoff::Journal::open(&journal)
            .unwrap()
            .state(),
        reel_workflow::handoff::State::Waiting
    );
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
    native_handoff_lifecycle(false);
}
#[cfg(windows)]
#[test]
#[ignore = "Existing declared native pool only: retained close, exact recovery, slot reuse and historical result proof; preserves roots"]
fn retained_job_closes_recovers_and_reuses_slot_without_old_operation_clearing_replacement() {
    native_handoff_lifecycle(true);
}
#[cfg(windows)]
fn native_handoff_lifecycle(close_slot: bool) {
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
    fixture.temp.disable_cleanup(close_slot);
    let git_raw = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&git_raw),
        bytes: git_raw.len() as u64,
    };
    let mut case = AdmissionCase::with_store(&fixture, native.store.clone());
    case.temp.disable_cleanup(close_slot);
    if close_slot {
        eprintln!(
            "Native close fixture: {}; source: {}",
            case.temp.path().display(),
            fixture.temp.path().display()
        );
        if let Some(directory) = std::env::var_os("REEL_CLOSE_EVIDENCE_DIRECTORY") {
            fs::write(
                PathBuf::from(directory).join("original-operation-roots.json"),
                serde_json::to_vec(&json!({"root":case.temp.path(),"source":fixture.temp.path()}))
                    .unwrap(),
            )
            .unwrap();
        }
    }
    let control = case.source.source.pool.control.clone();
    case.source = NativeProfile {
        movie_verifier: None,
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
    if close_slot {
        let target = case.temp.path().join("waiting-close-target.json");
        fs::write(
            &target,
            serde_json::to_vec(&profile.binding.target).unwrap(),
        )
        .unwrap();
        let ticket = case.temp.path().join("waiting-close-ticket.json");
        let before = fs::read(case.profile.index_root.join("claims.json")).unwrap();
        let refused = Command::new(&executable)
            .args([
                "prepare-close".as_ref(),
                journal.as_os_str(),
                profile_file.as_os_str(),
                target.as_os_str(),
                ticket.as_os_str(),
                "--apply".as_ref(),
            ])
            .output()
            .unwrap();
        assert!(!refused.status.success());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&refused.stdout).unwrap()["error"]["kind"],
            "denied"
        );
        assert!(!ticket.exists());
        assert_eq!(
            before,
            fs::read(case.profile.index_root.join("claims.json")).unwrap()
        );
    }
    let (run_plan, run_bindings, batch_file) = exercise_waiting_graph(
        &case,
        &fixture.packet,
        &profile,
        &journal,
        &profile_file,
        true,
    );
    let run_plan_file = case.temp.path().join("run-plan.json");
    let run_bindings_file = case.temp.path().join("run-bindings.json");
    let run_tasks_file = case.temp.path().join("run-tasks.json");
    fs::write(&run_plan_file, serde_json::to_vec(&run_plan).unwrap()).unwrap();
    fs::write(
        &run_bindings_file,
        serde_json::to_vec(&run_bindings).unwrap(),
    )
    .unwrap();
    fs::write(
        &run_tasks_file,
        serde_json::to_vec(&vec![&fixture.packet.task.id]).unwrap(),
    )
    .unwrap();
    let inspected = Command::new(&executable)
        .arg("inspect-run")
        .arg(&run_plan_file)
        .arg(&run_bindings_file)
        .output()
        .unwrap();
    assert!(
        inspected.status.success(),
        "{}",
        String::from_utf8_lossy(&inspected.stdout)
    );
    assert!(!String::from_utf8_lossy(&inspected.stdout).contains(&fixture.packet.task.prompt));
    let exported = Command::new(&executable)
        .arg("export-ready")
        .arg(&run_plan_file)
        .arg(&run_bindings_file)
        .arg(&run_tasks_file)
        .arg(&batch_file)
        .arg("--apply")
        .output()
        .unwrap();
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stdout)
    );
    let batch: serde_json::Value = serde_json::from_slice(&fs::read(&batch_file).unwrap()).unwrap();
    let batch_pin = digest(&fs::read(&batch_file).unwrap());
    assert_eq!(
        batch["tasks"][0]["ticket"]["packet"]["task"]["prompt"],
        fixture.packet.task.prompt
    );
    assert!(Path::new(batch["tasks"][0]["input_bindings"].as_str().unwrap()).is_file());
    assert!(
        reel_workflow_native::graph::export_ready(
            &run_plan,
            &run_bindings,
            &[fixture.packet.task.id.clone()],
            &batch_file
        )
        .is_err()
    );
    let mut changed_plan = run_plan.clone();
    changed_plan.tasks[0].prompt.push_str(" changed");
    assert_eq!(
        reel_workflow_native::graph::import_ready(
            &changed_plan,
            &run_bindings,
            &reel_workflow_native::graph::PinnedFile {
                path: batch_file.clone(),
                sha256: batch_pin.clone()
            },
            &fixture.packet.task.id,
            result.clone(),
            &returns
        )
        .unwrap_err()
        .kind,
        ErrorKind::Stale
    );
    assert_eq!(Journal::open(&journal).unwrap().state(), State::Waiting);
    let failed = Command::new(&executable)
        .arg("import-ready")
        .arg(&run_plan_file)
        .arg(&run_bindings_file)
        .arg(&batch_file)
        .arg(&batch_pin)
        .arg(&fixture.packet.task.id)
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
        reel_workflow_native::graph::inspect(&run_plan, &run_bindings)
            .unwrap()
            .rows[0]
            .status,
        reel_workflow::graph::Status::NeedsRecovery
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
    let completed = Command::new(&executable)
        .arg("import-ready")
        .arg(&run_plan_file)
        .arg(&run_bindings_file)
        .arg(&batch_file)
        .arg(&batch_pin)
        .arg(&fixture.packet.task.id)
        .arg(&result_file)
        .arg(&returns_file)
        .arg("--apply")
        .output()
        .unwrap();
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
    let observed = reel_workflow_native::graph::inspect(&run_plan, &run_bindings).unwrap();
    assert_eq!(
        observed.rows[0].status,
        reel_workflow::graph::Status::RetainedObserved
    );
    assert!(observed.needs_admission.contains_key("downstream"));
    let ack_path = profile.acknowledgements.join(format!(
        "retained-{}-attempt-1.json",
        fixture.packet.task_key
    ));
    let ack_bytes = fs::read(&ack_path).unwrap();
    fs::write(&ack_path, b"synthetic interrupted control").unwrap();
    let corrupt = reel_workflow_native::graph::inspect(&run_plan, &run_bindings).unwrap();
    assert_eq!(
        corrupt.rows[0].status,
        reel_workflow::graph::Status::Refused
    );
    assert_eq!(
        corrupt.rows[1].status,
        reel_workflow::graph::Status::Blocked
    );
    assert!(!resume(&journal, Path::new("-")).status.success());
    assert_eq!(Journal::open(&journal).unwrap().state(), State::Retained);
    fs::write(&ack_path, &ack_bytes).unwrap();
    if close_slot {
        exercise_native_close(
            &case,
            &fixture,
            &profile,
            &journal,
            &profile_file,
            &executable,
            &result,
            &receipt,
            &ack_path,
        );
    }
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
            ("run-plan.json", fs::read(run_plan_file).unwrap()),
            ("run-bindings.json", fs::read(run_bindings_file).unwrap()),
            ("ready-batch.json", fs::read(batch_file).unwrap()),
            ("run-inspection.json", inspected.stdout),
            ("run-export.json", exported.stdout),
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
#[ignore = "Two explicitly enrolled synthetic job slots in an existing native pool; no automatic actor allocation or provider calls"]
fn ready_batch_stages_two_jobs_sharing_one_retained_target_and_ack_directory() {
    use reel_workflow_native::{
        NativeProfile,
        admission::Prepared,
        graph::{Binding, Bindings, PinnedFile},
        staging,
    };
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("native fixture required"))
            .unwrap(),
    )
    .unwrap();
    let object: Object = serde_json::from_value(value["object"].clone()).unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(value["source"].clone()).unwrap();
    let annex: Tool = serde_json::from_value(value["annex"].clone()).unwrap();
    let mut fixture = Fixture::with_object(object.clone());
    let git_bytes = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&git_bytes),
        bytes: git_bytes.len() as u64,
    };
    let mut case = AdmissionCase::with_extra_slot(&fixture, native.store.clone(), None, true);
    let control = case.source.source.pool.control.clone();
    case.source = NativeProfile {
        movie_verifier: None,
        snapshot: fixture.profile.clone(),
        annex,
        source: native,
    };
    case.source.source.pool.control = control;
    let mut second = fixture.packet.task.clone();
    second.id = "parallel".into();
    let plan = reel_workflow::Plan {
        schema: reel_workflow::PLAN_SCHEMA.into(),
        project: fixture.packet.project.clone(),
        source_commit: fixture.packet.source_commit.clone(),
        tasks: vec![fixture.packet.task.clone(), second],
    };
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_reel-workflow-native"));
    let mut bindings = Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::new(),
    };
    let mut shared_ack: Option<PathBuf> = None;
    for (index, task) in plan.tasks.iter().enumerate() {
        let packet = reel_workflow::make_packet(&plan, &task.id, BTreeMap::new()).unwrap();
        let mut admission = case.profile.clone();
        if index == 1 {
            admission.scope.actor = tig_store::Key::parse("slot2").unwrap();
            admission.scope.operation = tig_store::Key::parse("workflow-parallel").unwrap();
        }
        let receipt = Prepared::open(&packet, &case.source, &admission, "attempt-1")
            .unwrap()
            .accept()
            .unwrap();
        let mut profile = if index == 0 {
            handoff_profile(&case, receipt.clone(), executable.clone())
        } else {
            let first = &bindings.tasks[&plan.tasks[0].id];
            serde_json::from_slice(&fs::read(&first.profile.path).unwrap()).unwrap()
        };
        profile.admission = admission.clone();
        profile.binding.admission = receipt.clone();
        if let Some(path) = &shared_ack {
            profile.acknowledgements = path.clone();
        } else {
            shared_ack = Some(profile.acknowledgements.clone());
        }
        let packet_path = case.temp.path().join(format!("packet-{index}.json"));
        let profile_path = case.temp.path().join(format!("profile-{index}.json"));
        fs::write(&packet_path, serde_json::to_vec(&packet).unwrap()).unwrap();
        let raw = serde_json::to_vec(&profile).unwrap();
        fs::write(&profile_path, &raw).unwrap();
        let journal = case.temp.path().join(format!("journal-{index}"));
        let created = Command::new(&executable)
            .arg("create-handoff")
            .arg(&packet_path)
            .arg(&profile_path)
            .arg(&journal)
            .arg("--apply")
            .output()
            .unwrap();
        assert!(
            created.status.success(),
            "{}",
            String::from_utf8_lossy(&created.stdout)
        );
        let parent = case.temp.path().join(format!("scratch-{index}"));
        fs::create_dir(&parent).unwrap();
        let ticket = staging::prepare(
            &packet,
            &case.source,
            &admission,
            &receipt,
            &staging::Profile {
                schema: "reel.workflow-staging.v1".into(),
                parent,
                maximum_bytes: 8 * 1024 * 1024,
                capacity_policy: reel_assets::admission::Policy {
                    minimum_free_bytes: 65536,
                    minimum_free_basis_points: 0,
                    max_object_bytes: 512 * 1024 * 1024,
                    ..Default::default()
                },
            },
        )
        .unwrap();
        let ticket_raw = ticket.canonical_bytes().unwrap();
        let ticket_file = case.temp.path().join(format!("stage-{index}.json"));
        fs::write(&ticket_file, &ticket_raw).unwrap();
        bindings.tasks.insert(
            task.id.clone(),
            Binding {
                journal,
                profile: PinnedFile {
                    path: profile_path,
                    sha256: digest(&raw),
                },
                staging_ticket: Some(PinnedFile {
                    path: ticket_file,
                    sha256: digest(&ticket_raw),
                }),
            },
        );
    }
    let selected: Vec<_> = plan.tasks.iter().map(|t| t.id.clone()).collect();
    let output = case.temp.path().join("batch.json");
    assert_eq!(
        reel_workflow_native::graph::export_ready(&plan, &bindings, &selected, &output).unwrap(),
        2
    );
    let batch: serde_json::Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(batch["tasks"].as_array().unwrap().len(), 2);
    assert_eq!(
        case.protected(&object),
        vec![
            tig_store::Key::parse("slot").unwrap(),
            tig_store::Key::parse("slot2").unwrap()
        ]
    );
    // Two retained results share both an actor fence and identical native bytes.
    // Each logical origin must still bind independently before physical reuse.
    let mut retained = Vec::new();
    for (index, task) in plan.tasks.iter().enumerate() {
        let packet = reel_workflow::make_packet(&plan, &task.id, BTreeMap::new()).unwrap();
        let binding = &bindings.tasks[&task.id];
        let profile: reel_workflow_native::handoff::Profile =
            serde_json::from_slice(&fs::read(&binding.profile.path).unwrap()).unwrap();
        let (mut result, returns) = response_fixture(
            &packet,
            &case.temp.path().join(format!("upstream-return-{index}")),
        );
        result.artifacts.get_mut("note").unwrap().logical_id = format!("candidate/note-{index}");
        let result_path = case
            .temp
            .path()
            .join(format!("upstream-result-{index}.json"));
        let returns_path = case
            .temp
            .path()
            .join(format!("upstream-returns-{index}.json"));
        fs::write(&result_path, serde_json::to_vec(&result).unwrap()).unwrap();
        fs::write(&returns_path, serde_json::to_vec(&returns).unwrap()).unwrap();
        let imported = Command::new(&executable)
            .arg("import-handoff")
            .arg(&binding.journal)
            .arg(&binding.profile.path)
            .arg(&result_path)
            .arg(&returns_path)
            .arg("--apply")
            .output()
            .unwrap();
        assert!(
            imported.status.success(),
            "{}",
            String::from_utf8_lossy(&imported.stdout)
        );
        let path = profile.acknowledgements.join(format!(
            "retained-{}-{}.json",
            packet.task_key, result.attempt_id
        ));
        let ack: reel_workflow_native::retention::Acknowledgement =
            serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        retained.push((packet, profile, result, ack));
    }
    use reel_workflow::{
        publication::InputReader,
        upstream::{Bindings as InputBindings, Reference, Selector},
    };
    let input_bindings = InputBindings {
        schema: "reel.workflow-upstream-inputs.v1".into(),
        inputs: retained
            .iter()
            .map(|(p, _, r, _)| {
                (
                    r.artifacts["note"].logical_id.clone(),
                    Reference {
                        task_id: p.task.id.clone(),
                        artifact: Selector::Output {
                            name: "note".into(),
                        },
                    },
                )
            })
            .collect(),
    };
    let raw = input_bindings.canonical_bytes().unwrap();
    let mut child = fixture.packet.task.clone();
    child.id = "consumer".into();
    child.native_inputs = None;
    child.inputs = retained
        .iter()
        .map(|(_, _, r, _)| r.artifacts["note"].clone())
        .collect();
    child.depends_on = selected.clone();
    child.upstream_inputs = Some(Artifact {
        logical_id: "workflow/upstream".into(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    });
    let mut child_plan = plan.clone();
    child_plan.tasks.push(child);
    let child = reel_workflow::make_packet(
        &child_plan,
        "consumer",
        retained
            .iter()
            .map(|(p, _, r, _)| {
                (
                    p.task.id.clone(),
                    DependencyResult {
                        result_sha256: canonical_digest(r).unwrap(),
                        artifacts: r.artifacts.clone(),
                    },
                )
            })
            .collect(),
    )
    .unwrap();
    assert_eq!(
        reel_workflow_native::SnapshotInputs::open(&child, &case.source.snapshot)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Conflict
    );
    let parents = retained
        .iter()
        .map(|(packet, profile, result, ack)| {
            (
                packet.task.id.clone(),
                reel_workflow_native::upstream::Parent {
                    packet,
                    result,
                    source: &profile.source,
                    admission: &profile.admission,
                    receipt: &profile.binding.admission,
                    acknowledgement: ack,
                },
            )
        })
        .collect();
    let mut reader =
        reel_workflow_native::upstream::BorrowedInputs::open(&child, &input_bindings, parents)
            .unwrap();
    for asset in &child.task.inputs {
        assert_eq!(
            reader.read(asset, asset.bytes).unwrap(),
            b"synthetic response"
        );
    }
    let mut wrong = child.task.inputs[0].clone();
    wrong.logical_id = "candidate/unrelated".into();
    assert_eq!(
        reader.read(&wrong, wrong.bytes).unwrap_err().kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        reader.read(&child.task.inputs[0], 0).unwrap_err().kind,
        ErrorKind::Budget
    );
    drop(reader);
    let mut children = Vec::new();
    let parent_controls = case.temp.path().join("native-parents.json");
    let controls: BTreeMap<_, _> = retained
        .iter()
        .map(|(packet, profile, result, ack)| {
            (
                packet.task.id.clone(),
                json!({"packet":packet,"profile":profile,"result":result,"acknowledgement":ack}),
            )
        })
        .collect();
    fs::write(
        &parent_controls,
        serde_json::to_vec(&json!({"schema":"reel.workflow-native-parents.v1","parents":controls}))
            .unwrap(),
    )
    .unwrap();
    for mixed in [false, true] {
        let parents: BTreeMap<_, _> = retained
            .iter()
            .map(|(packet, profile, result, ack)| {
                (
                    packet.task.id.clone(),
                    reel_workflow_native::upstream::Parent {
                        packet,
                        result,
                        source: &profile.source,
                        admission: &profile.admission,
                        receipt: &profile.binding.admission,
                        acknowledgement: ack,
                    },
                )
            })
            .collect();
        let frozen = reel_workflow_native::upstream::Frozen::capture(&input_bindings, &parents);
        let mut source: NativeProfile =
            serde_json::from_slice(&serde_json::to_vec(&case.source).unwrap()).unwrap();
        source.snapshot.schema = "reel.workflow-native-composite-snapshot.v1".into();
        source.snapshot.upstream = Some(frozen);
        if !mixed {
            source.snapshot.records.clear();
            source.snapshot.definitions.clear();
            source.snapshot.bindings.clear();
            source.snapshot.input_streams.clear();
        }
        let mut task = child.task.clone();
        task.id = if mixed {
            "consumer-mixed"
        } else {
            "consumer-only"
        }
        .into();
        if mixed {
            task.inputs.extend(fixture.packet.task.inputs.clone());
        }
        let declaration = source.snapshot.declaration_bytes().unwrap();
        task.native_inputs = Some(Artifact {
            logical_id: "workflow/composite-source".into(),
            sha256: digest(&declaration),
            bytes: declaration.len() as u64,
            media_type: "application/json".into(),
        });
        let mut configured = plan.clone();
        configured.tasks.push(task.clone());
        let packet = make_packet(&configured, &task.id, child.dependencies.clone()).unwrap();
        let mut admission = case.profile.clone();
        admission.scope.actor =
            tig_store::Key::parse(if mixed { "slot4" } else { "slot3" }).unwrap();
        admission.scope.operation = tig_store::Key::parse(if mixed {
            "workflow-child-mixed"
        } else {
            "workflow-child-only"
        })
        .unwrap();
        assert_eq!(
            Prepared::open(&packet, &source, &admission, "child-1")
                .unwrap()
                .accept()
                .unwrap_err()
                .kind,
            ErrorKind::Denied
        );
        assert_eq!(
            reel_workflow_native::NativeInputs::open(&packet, &source)
                .err()
                .unwrap()
                .kind,
            ErrorKind::Denied
        );
        assert_eq!(
            Prepared::open(&packet, &source, &admission, "child-1")
                .unwrap()
                .recover()
                .unwrap_err()
                .kind,
            ErrorKind::Missing
        );
        let receipt = if mixed {
            let packet_path = case.temp.path().join("accept-child-packet.json");
            let source_path = case.temp.path().join("accept-child-source.json");
            let profile_path = case.temp.path().join("accept-child-profile.json");
            let receipt_path = case.temp.path().join("accept-child-receipt.json");
            fs::write(&packet_path, serde_json::to_vec(&packet).unwrap()).unwrap();
            fs::write(&source_path, serde_json::to_vec(&source).unwrap()).unwrap();
            fs::write(&profile_path, serde_json::to_vec(&admission).unwrap()).unwrap();
            let accepted = Command::new(&executable)
                .arg("accept-upstream")
                .arg(&packet_path)
                .arg(&source_path)
                .arg(&profile_path)
                .arg("child-1")
                .arg(&parent_controls)
                .arg(&receipt_path)
                .arg("--apply")
                .output()
                .unwrap();
            assert!(
                accepted.status.success(),
                "{}",
                String::from_utf8_lossy(&accepted.stdout)
            );
            serde_json::from_slice(&fs::read(receipt_path).unwrap()).unwrap()
        } else {
            let admitted = reel_workflow_native::upstream::Admission::prepare(
                &packet, &source, &admission, "child-1", parents,
            )
            .unwrap();
            assert!(admitted.preview().unwrap().declared_objects < 128);
            admitted.accept().unwrap()
        };
        Prepared::open(&packet, &source, &admission, "child-1")
            .unwrap()
            .reopen(&receipt)
            .unwrap();
        if !mixed {
            let mut profile: reel_workflow_native::handoff::Profile =
                serde_json::from_slice(&serde_json::to_vec(&retained[0].1).unwrap()).unwrap();
            profile.source = serde_json::from_slice(&serde_json::to_vec(&source).unwrap()).unwrap();
            profile.admission = admission.clone();
            profile.binding.admission = receipt.clone();
            let packet_path = case.temp.path().join("child-handoff-packet.json");
            let profile_path = case.temp.path().join("child-handoff-profile.json");
            let journal = case.temp.path().join("child-handoff");
            let result_path = case.temp.path().join("child-handoff-result.json");
            let returns_path = case.temp.path().join("child-handoff-returns.json");
            let (mut result, returns) =
                response_fixture(&packet, &case.temp.path().join("child-handoff-outputs"));
            result.attempt_id = receipt.attempt_id.clone();
            for (path, raw) in [
                (&packet_path, serde_json::to_vec(&packet).unwrap()),
                (&profile_path, serde_json::to_vec(&profile).unwrap()),
                (&result_path, serde_json::to_vec(&result).unwrap()),
                (&returns_path, serde_json::to_vec(&returns).unwrap()),
            ] {
                fs::write(path, raw).unwrap();
            }
            let created = Command::new(&executable)
                .arg("create-handoff")
                .arg(&packet_path)
                .arg(&profile_path)
                .arg(&journal)
                .arg("--apply")
                .output()
                .unwrap();
            assert!(
                created.status.success(),
                "{}",
                String::from_utf8_lossy(&created.stdout)
            );
            let completed = Command::new(&executable)
                .arg("import-handoff")
                .arg(&journal)
                .arg(&profile_path)
                .arg(&result_path)
                .arg(&returns_path)
                .arg("--apply")
                .output()
                .unwrap();
            assert!(
                completed.status.success(),
                "{}",
                String::from_utf8_lossy(&completed.stdout)
            );
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&completed.stdout).unwrap()["task_completed"],
                true
            );
            fs::write(
                case.temp.path().join("child-handoff-completion.json"),
                completed.stdout,
            )
            .unwrap();
        }
        children.push((packet, source, admission, receipt));
    }
    // Test-only loss of parent coverage. This deliberately bypasses production
    // close semantics; no payload is deleted and child assignments stay intact.
    {
        use tig_core::tree::{Tree, TreeLimits};
        use tig_store::{
            metadata::MetadataStore,
            source_claims::{
                Selection,
                protection::{self, Index, work},
            },
        };
        let last = &children.last().unwrap().3;
        let mut index = Index::open(&case.profile.index_root, &last.index_checkpoint).unwrap();
        let mut nodes =
            MetadataStore::open(&case.profile.metadata_root, &last.metadata_checkpoint).unwrap();
        for actor in ["retained", "slot", "slot2"] {
            let actor = tig_store::Key::parse(actor).unwrap();
            let current = index.actor(&mut nodes, &actor).unwrap();
            let entries = Tree::from_root(current.selection.root)
                .entries(&mut nodes, TreeLimits::default())
                .unwrap()
                .0;
            let update = protection::Update {
                operation: tig_store::Key::parse(&format!(
                    "fixture-drop-parent-{}",
                    actor.as_str()
                ))
                .unwrap(),
                actor: actor.clone(),
                owner: case.profile.scope.owner.clone(),
                expected_revision: current.revision,
                selection: Selection {
                    root: Tree::empty(&mut nodes).unwrap().root().clone(),
                    context: current.selection.context,
                },
                changes: entries.keys().map(|path| (path.clone(), None)).collect(),
                ready: true,
            };
            let work = work::stage(&mut index, &update, None, &mut nodes).unwrap();
            work::execute(&mut index, &work, &mut nodes).unwrap();
        }
    }
    assert_eq!(
        case.protected(&Object::from_bytes(b"synthetic response")),
        vec![
            tig_store::Key::parse("slot3").unwrap(),
            tig_store::Key::parse("slot4").unwrap()
        ]
    );
    let bare = case.temp.path().join("child-history.git");
    git(
        fixture.temp.path(),
        &[
            "clone",
            "--bare",
            "--no-hardlinks",
            fixture.temp.path().to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    let relocated = case.temp.path().join("original-source-unavailable");
    assert!(
        relocated.parent().unwrap().canonicalize().unwrap()
            == case.temp.path().canonicalize().unwrap()
    );
    fs::rename(fixture.temp.path(), &relocated).unwrap();
    drop(retained);
    for (index, (packet, mut source, admission, receipt)) in children.into_iter().enumerate() {
        source.snapshot.repository = bare.clone();
        let source: NativeProfile =
            serde_json::from_slice(&serde_json::to_vec(&source).unwrap()).unwrap();
        let mut owned = reel_workflow_native::NativeInputs::open_admitted(
            &packet, &source, &admission, &receipt,
        )
        .unwrap();
        for asset in &packet.task.inputs {
            let bytes = owned.read(asset, asset.bytes).unwrap();
            assert_eq!(digest(&bytes), asset.sha256);
        }
        drop(owned);
        let parent = case.temp.path().join(format!("child-scratch-{index}"));
        fs::create_dir(&parent).unwrap();
        let ticket = staging::prepare(
            &packet,
            &source,
            &admission,
            &receipt,
            &staging::Profile {
                schema: "reel.workflow-staging.v1".into(),
                parent,
                maximum_bytes: 8 * 1024 * 1024,
                capacity_policy: reel_assets::admission::Policy {
                    minimum_free_bytes: 65536,
                    minimum_free_basis_points: 0,
                    max_object_bytes: 512 * 1024 * 1024,
                    ..Default::default()
                },
            },
        )
        .unwrap();
        let raw = ticket.canonical_bytes().unwrap();
        staging::export(&packet, &source, &admission, &raw, &digest(&raw)).unwrap();
        staging::export(&packet, &source, &admission, &raw, &digest(&raw)).unwrap();
        fs::write(
            case.temp.path().join(format!("child-packet-{index}.json")),
            serde_json::to_vec(&packet).unwrap(),
        )
        .unwrap();
        fs::write(
            case.temp.path().join(format!("child-source-{index}.json")),
            serde_json::to_vec(&source).unwrap(),
        )
        .unwrap();
        fs::write(
            case.temp
                .path()
                .join(format!("child-admission-{index}.json")),
            serde_json::to_vec(&admission).unwrap(),
        )
        .unwrap();
        fs::write(
            case.temp.path().join(format!("child-receipt-{index}.json")),
            receipt.canonical_bytes().unwrap(),
        )
        .unwrap();
        fs::write(
            case.temp.path().join(format!("child-ticket-{index}.json")),
            raw,
        )
        .unwrap();
        let recovered_path = case
            .temp
            .path()
            .join(format!("child-recovered-{index}.json"));
        let recovered = Command::new(&executable)
            .arg("recover-admission")
            .arg(case.temp.path().join(format!("child-packet-{index}.json")))
            .arg(case.temp.path().join(format!("child-source-{index}.json")))
            .arg(
                case.temp
                    .path()
                    .join(format!("child-admission-{index}.json")),
            )
            .arg("child-1")
            .arg(&recovered_path)
            .arg("--apply")
            .output()
            .unwrap();
        assert!(
            recovered.status.success(),
            "{}",
            String::from_utf8_lossy(&recovered.stdout)
        );
        let recovered: reel_workflow_native::admission::Receipt =
            serde_json::from_slice(&fs::read(recovered_path).unwrap()).unwrap();
        assert_eq!(recovered.work, receipt.work);
        assert_eq!(recovered.activation, receipt.activation);
        assert_eq!(recovered.source_archive, receipt.source_archive);
    }
    if let Some(directory) = std::env::var_os("REEL_GRAPH_BATCH_EVIDENCE_DIRECTORY") {
        use std::io::Write;
        let directory = PathBuf::from(directory);
        for (name, raw) in [
            ("plan.json", serde_json::to_vec(&plan).unwrap()),
            ("bindings.json", serde_json::to_vec(&bindings).unwrap()),
            ("batch.json", fs::read(output).unwrap()),
            ("consumer-packet.json", serde_json::to_vec(&child).unwrap()),
            ("upstream-inputs.json", raw),
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
fn frozen_composite_metadata_cannot_grant_acceptance_recovery_or_byte_access() {
    use reel_workflow::{
        DependencyResult,
        upstream::{Bindings, Reference, Selector},
    };
    use reel_workflow_native::{
        NativeProfile,
        admission::Prepared,
        upstream::{Evidence, Frozen},
    };
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let (result, _returns) =
        response_fixture(&fixture.packet, &case.temp.path().join("unretained-result"));
    let asset = result.artifacts["note"].clone();
    let bindings = Bindings {
        schema: "reel.workflow-upstream-inputs.v1".into(),
        inputs: BTreeMap::from([(
            asset.logical_id.clone(),
            Reference {
                task_id: fixture.packet.task.id.clone(),
                artifact: Selector::Output {
                    name: "note".into(),
                },
            },
        )]),
    };
    let raw = bindings.canonical_bytes().unwrap();
    let frozen = Frozen {
        schema: "reel.workflow-frozen-origins.v1".into(),
        bindings,
        parents: BTreeMap::from([(
            fixture.packet.task.id.clone(),
            Evidence {
                packet: fixture.packet.clone(),
                result: result.clone(),
            },
        )]),
    };
    let mut source: NativeProfile =
        serde_json::from_slice(&serde_json::to_vec(&case.source).unwrap()).unwrap();
    source.snapshot.schema = "reel.workflow-native-composite-snapshot.v1".into();
    source.snapshot.upstream = Some(frozen);
    source.snapshot.bindings.clear();
    source.snapshot.input_streams.clear();
    source.snapshot.records.clear();
    source.snapshot.definitions.clear();
    let mut task = fixture.packet.task.clone();
    task.id = "unadmitted-child".into();
    task.inputs = vec![asset];
    task.depends_on = vec![fixture.packet.task.id.clone()];
    task.upstream_inputs = Some(Artifact {
        logical_id: "workflow/upstream".into(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    });
    let raw = source.snapshot.declaration_bytes().unwrap();
    task.native_inputs = Some(Artifact {
        logical_id: "workflow/source".into(),
        sha256: digest(&raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    });
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: fixture.packet.project.clone(),
        source_commit: fixture.packet.source_commit.clone(),
        tasks: vec![fixture.packet.task.clone(), task],
    };
    let child = make_packet(
        &plan,
        "unadmitted-child",
        BTreeMap::from([(
            fixture.packet.task.id.clone(),
            DependencyResult {
                result_sha256: canonical_digest(&result).unwrap(),
                artifacts: result.artifacts.clone(),
            },
        )]),
    )
    .unwrap();
    assert_eq!(
        SnapshotInputs::open(&child, &source.snapshot)
            .unwrap()
            .input_count(),
        1
    );
    let prepared = Prepared::open(&child, &source, &case.profile, "attempt-child").unwrap();
    assert!(prepared.preview().unwrap().declared_objects < 128);
    assert_eq!(prepared.accept().unwrap_err().kind, ErrorKind::Denied);
    assert_eq!(prepared.recover().unwrap_err().kind, ErrorKind::Missing);
    assert_eq!(
        reel_workflow_native::NativeInputs::open(&child, &source)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Denied
    );
    assert!(
        case.protected(&Object::from_bytes(b"synthetic response"))
            .is_empty()
    );
    source
        .snapshot
        .upstream
        .as_mut()
        .unwrap()
        .parents
        .get_mut(&fixture.packet.task.id)
        .unwrap()
        .result
        .attempt_id = "changed-attempt".into();
    assert_eq!(
        SnapshotInputs::open(&child, &source.snapshot)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Stale
    );
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
#[ignore = "Synthetic comic, installed private test font and existing explicitly declared native pool; no project adoption"]
fn native_comic_worker_receipt_replay_retention_and_bare_history_complete_together() {
    comic_native_case(false, false, false);
}
#[cfg(windows)]
#[test]
#[ignore = "Synthetic selected issue part and explicit existing native pool; no project adoption"]
fn native_comic_part_preserves_master_identity_and_bounds_its_protected_source_closure() {
    comic_native_case(true, false, false);
}
#[cfg(windows)]
#[test]
#[ignore = "Synthetic in-process comic dispatch and explicitly declared existing native pool; no project adoption"]
fn native_comic_dispatch_materializes_and_retains_original_ready_task() {
    comic_native_case(true, true, false);
}
#[cfg(windows)]
#[test]
#[ignore = "Synthetic composite comic from retained PNG input; existing native pool and explicit actors only"]
fn composite_comic_worker_replays_and_restores_after_source_actor_coverage_is_lost() {
    comic_native_case(true, true, true);
}
#[cfg(windows)]
#[cfg(windows)]
#[test]
#[ignore = "Existing declared native pool: automatic protected comic dispatch, retention and editable historical recovery; preserves controller roots"]
fn automatic_comic_dispatch_stages_runs_retains_and_reopens_editable_history() {
    comic_native_case_mode(false, true, false, true);
}
fn comic_native_case(partitioned: bool, dispatch: bool, composite: bool) {
    comic_native_case_mode(partitioned, dispatch, composite, false);
}
fn comic_native_case_mode(partitioned: bool, dispatch: bool, composite: bool, automatic: bool) {
    use reel_publication::{
        ComicPage, Content, Edition, PageKind, Publication, ReadingDirection,
        comic_source::{
            Layer, LayerRole, Limits, PageSource, Panel, Pin, Receipt as ComicReceipt, Request,
            Utterance,
        },
    };
    use reel_workflow_native::{NativeProfile, admission::Prepared, response};
    use tig_store::source_claims::protection::Index;
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("native fixture required"))
            .unwrap(),
    )
    .unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(value["source"].clone()).unwrap();
    let annex: Tool = serde_json::from_value(value["annex"].clone()).unwrap();
    let mut fixture = Fixture::new();
    fixture.temp.disable_cleanup(automatic);
    let mut raw_source = tempfile::tempdir().unwrap();
    raw_source.disable_cleanup(automatic);
    let executable = std::env::var_os("REEL_NATIVE_HANDOFF_TOOL")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_reel-workflow-native")));
    let worker_bytes = fs::read(&executable).unwrap();
    let worker = Artifact {
        logical_id: "tool/native-comic".into(),
        sha256: digest(&worker_bytes),
        bytes: worker_bytes.len() as u64,
        media_type: "application/octet-stream".into(),
    };
    let mut assets = Vec::<(String, Artifact, Vec<u8>)>::new();
    let mut put = |id: &str, path: &str, media: &str, bytes: Vec<u8>| -> (Artifact, Pin) {
        let target = raw_source.path().join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, &bytes).unwrap();
        let asset = Artifact {
            logical_id: id.into(),
            sha256: digest(&bytes),
            bytes: bytes.len() as u64,
            media_type: media.into(),
        };
        let pin = Pin {
            path: path.into(),
            sha256: asset.sha256.clone(),
            bytes: asset.bytes,
        };
        assets.push((path.into(), asset.clone(), bytes));
        (asset, pin)
    };
    let font_path = std::env::var_os("REEL_COMIC_TEST_FONT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:/Windows/Fonts/arial.ttf"));
    let family = std::env::var("REEL_COMIC_TEST_FONT_FAMILY").unwrap_or_else(|_| "Arial".into());
    let license = std::env::var_os("REEL_COMIC_TEST_FONT_LICENSE")
        .map(|p| fs::read(p).unwrap())
        .unwrap_or_else(|| {
            b"Synthetic private fixture uses an installed font; no redistribution rights granted."
                .to_vec()
        });
    let (_, font) = put(
        "comic/font",
        "fonts/body.ttf",
        "font/ttf",
        fs::read(font_path).unwrap(),
    );
    let (_, license) = put("comic/license", "fonts/license.txt", "text/plain", license);
    let mut pixels = resvg::tiny_skia::Pixmap::new(72, 72).unwrap();
    pixels.fill(resvg::tiny_skia::Color::from_rgba8(230, 240, 244, 255));
    let (_, image) = put(
        "comic/image",
        "art/background.png",
        "image/png",
        pixels.encode_png().unwrap(),
    );
    let page = PageSource {
        schema: "reel.comic-page.v1".into(),
        id: "page01".into(),
        width: 200,
        height: 200,
        panels: vec![Panel {
            id: "panel-a".into(),
            rect: [8, 8, 184, 184],
        }],
        layers: vec![Layer {
            id: "background".into(),
            panel_id: "panel-a".into(),
            role: LayerRole::Background,
            source: image,
            rect: [8, 8, 184, 184],
        }],
        utterances: vec![Utterance {
            id: "line-a".into(),
            panel_id: "panel-a".into(),
            speaker: "Narrador".into(),
            lines: vec!["La luz sigue.".into()],
            rect: [20, 20, 160, 50],
            font_size: 14,
            tail: None,
        }],
    };
    let (page_asset, page_pin) = put(
        "comic/page",
        "pages/page01.json",
        "application/json",
        serde_json::to_vec_pretty(&page).unwrap(),
    );
    let request = Request {
        editable_packet: false,
        part_id: None,
        schema: "reel.comic-build.v1".into(),
        publication_id: "synthetic-comic".into(),
        edition_id: "es".into(),
        language: "es".into(),
        font_family: family,
        font,
        font_license: license,
        pages: vec![page_pin],
        limits: Limits {
            input_bytes: 8 * 1024 * 1024,
            output_bytes: 16 * 1024 * 1024,
        },
    };
    let (request_asset, _) = put(
        "comic/request",
        "request.json",
        "application/json",
        serde_json::to_vec_pretty(&request).unwrap(),
    );
    let mut contracts = BTreeMap::new();
    for (name, media) in [
        ("page01-clean.svg", "image/svg+xml"),
        ("page01-clean.png", "image/png"),
        ("page01-lettering.svg", "image/svg+xml"),
        ("page01-reading.svg", "image/svg+xml"),
        ("page01-reading.png", "image/png"),
        ("reading.pdf", "application/pdf"),
        ("transcript.json", "application/json"),
    ] {
        contracts.insert(
            name.into(),
            OutputContract {
                media_type: media.into(),
                max_bytes: 16 * 1024 * 1024,
                required: true,
            },
        );
    }
    let mut publication = Publication {
        schema: reel_publication::PUBLICATION_SCHEMA.into(),
        id: "synthetic-comic".into(),
        editions: vec![Edition {
            id: "es".into(),
            language: "es".into(),
            content: Content::Comic {
                reading_direction: ReadingDirection::LeftToRight,
                pages: vec![ComicPage {
                    id: "page01".into(),
                    kind: PageKind::Story,
                    source: page_asset,
                    panel_order: vec!["panel-a".into()],
                    utterance_order: vec!["line-a".into()],
                    spread_id: None,
                }],
            },
            outputs: contracts.clone(),
        }],
    };
    let mut unselected = None;
    if partitioned {
        let mut second = page.clone();
        second.id = "page02".into();
        second.utterances[0].lines = vec!["Seguimos.".into()];
        let (asset, _) = put(
            "comic/page-two",
            "pages/page02.json",
            "application/json",
            serde_json::to_vec_pretty(&second).unwrap(),
        );
        unselected = Some(Object {
            sha256: tig_core::Digest::parse(&asset.sha256).unwrap(),
            bytes: asset.bytes,
        });
        let Content::Comic { pages, .. } = &mut publication.editions[0].content else {
            unreachable!()
        };
        let mut selected = pages[0].clone();
        selected.id = second.id;
        selected.source = asset;
        pages.push(selected);
        publication.editions[0]
            .outputs
            .retain(|name, _| name == "reading.pdf" || name == "transcript.json");
    }
    let (publication_asset, _) = put(
        "comic/publication",
        "publication.json",
        "application/json",
        serde_json::to_vec_pretty(&publication).unwrap(),
    );
    let part = if partitioned {
        use reel_publication::comic_issue_source::{Part, Plan as IssuePlan};
        let issue = IssuePlan {
            schema: "reel.comic-issue-plan.v1".into(),
            publication: publication.clone(),
            edition_id: "es".into(),
            parts: vec![
                Part {
                    id: "opening".into(),
                    pages: vec!["page01".into()],
                },
                Part {
                    id: "closing".into(),
                    pages: vec!["page02".into()],
                },
            ],
        };
        let (plan, _) = put(
            "comic/issue-plan",
            "issue-plan.json",
            "application/json",
            serde_json::to_vec_pretty(&issue).unwrap(),
        );
        Some(reel_workflow::ComicPart {
            schema: "reel.workflow-comic-part.v1".into(),
            plan,
            part_id: "opening".into(),
        })
    } else {
        None
    };
    fixture.profile.records.clear();
    fixture.profile.bindings.clear();
    fixture.profile.input_streams.clear();
    let definition =
        Object::from_bytes(&fs::read(fixture.temp.path().join("definitions/input.json")).unwrap());
    for (i, (_, asset, raw)) in assets.iter().enumerate() {
        let record = format!("records/comic-{i}.tig");
        let leaf = format!("payloads/comic-{i}.bin");
        write(
            fixture.temp.path(),
            &record,
            json!({"schema":"tig.instance.design-1","id":asset.logical_id,
            "definition":{"id":"types/input","sha256":definition.sha256,"bytes":definition.bytes},
            "authored":{"text":"synthetic comic source"},"selected":Object::from_bytes(raw),"streams":[],"status":{},"metadata":{}}),
        );
        fs::write(
            fixture.temp.path().join(&leaf),
            Key::from_object(&Object::from_bytes(raw)).unlocked_pointer(),
        )
        .unwrap();
        if asset.logical_id == "comic/page-two" {
            continue;
        }
        fixture.profile.records.push(record);
        fixture
            .profile
            .input_streams
            .insert(asset.logical_id.clone(), None);
        fixture.profile.bindings.insert(
            asset.logical_id.clone(),
            Binding {
                selected: LogicalPath::parse(&leaf).unwrap(),
                streams: BTreeMap::new(),
            },
        );
    }
    git(fixture.temp.path(), &["add", "records", "payloads"]);
    git(
        fixture.temp.path(),
        &["commit", "-m", "synthetic comic declarations"],
    );
    fixture.profile.commit = git(fixture.temp.path(), &["rev-parse", "HEAD"]);
    let git_bytes = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&git_bytes),
        bytes: git_bytes.len() as u64,
    };
    let task = Task {
        id: "comic-render".into(),
        upstream_inputs: None,
        native_inputs: Some(Fixture::declaration(&fixture.profile)),
        operation: Operation::DeterministicTool,
        tool: Some(ToolContract {
            auxiliary_executables: BTreeMap::new(),
            part,
            schema: "reel.workflow-tool.v1".into(),
            capability: ToolCapability::ComicRender,
            executable: worker.clone(),
            publication: publication_asset,
            edition_id: "es".into(),
            request: request_asset,
            source_bindings: assets
                .iter()
                .filter(|(_, a, _)| {
                    a.logical_id != "comic/request"
                        && a.logical_id != "comic/publication"
                        && a.logical_id != "comic/issue-plan"
                        && a.logical_id != "comic/page-two"
                })
                .map(|(p, a, _)| (p.clone(), a.logical_id.clone()))
                .collect(),
        }),
        language: Some("es".into()),
        prompt: String::new(),
        inputs: assets
            .iter()
            .filter(|(_, a, _)| a.logical_id != "comic/page-two")
            .map(|(_, a, _)| a.clone())
            .collect(),
        depends_on: vec![],
        outputs: contracts,
    };
    fixture.packet = make_packet(
        &Plan {
            schema: PLAN_SCHEMA.into(),
            project: "synthetic-comic".into(),
            source_commit: fixture.profile.commit.clone(),
            tasks: vec![task],
        },
        "comic-render",
        BTreeMap::new(),
    )
    .unwrap();
    let mut case = if composite {
        AdmissionCase::with_extra_slot(&fixture, native.store.clone(), None, true)
    } else {
        AdmissionCase::with_store(&fixture, native.store.clone())
    };
    case.temp.disable_cleanup(automatic);
    let control = case.source.source.pool.control.clone();
    case.source = NativeProfile {
        movie_verifier: None,
        snapshot: fixture.profile.clone(),
        annex,
        source: native,
    };
    case.source.source.pool.control = control;
    case.source.source.maximum_bytes = 16 * 1024 * 1024;
    let mut parent = None;
    let initial_packet = if composite {
        let mut task = fixture.packet.task.clone();
        task.id = "comic-art-parent".into();
        task.operation = Operation::ImageCandidate;
        task.tool = None;
        task.prompt = "Return the exact synthetic PNG layer.".into();
        task.outputs = BTreeMap::from([(
            "art".into(),
            OutputContract {
                media_type: "image/png".into(),
                max_bytes: 16 * 1024 * 1024,
                required: true,
            },
        )]);
        let packet = make_packet(
            &Plan {
                schema: PLAN_SCHEMA.into(),
                project: fixture.packet.project.clone(),
                source_commit: fixture.packet.source_commit.clone(),
                tasks: vec![task],
            },
            "comic-art-parent",
            BTreeMap::new(),
        )
        .unwrap();
        let mut profile = case.profile.clone();
        profile.scope.actor = tig_store::Key::parse("slot2").unwrap();
        profile.scope.operation = tig_store::Key::parse("comic-art-parent").unwrap();
        parent = Some((packet.clone(), profile));
        packet
    } else {
        fixture.packet.clone()
    };
    let initial_profile = parent.as_ref().map(|p| &p.1).unwrap_or(&case.profile);
    let admission = Prepared::open(&initial_packet, &case.source, initial_profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    for (path, _, raw) in &assets {
        if path == "pages/page02.json" {
            continue;
        }
        tig_git::annex::workflow::publication::publish(
            &tig_git::annex::workflow::publication::Profile {
                schema: "tig.annex-file-publication.experimental-1".into(),
                pool: case.source.source.pool.clone(),
                store: case.profile.scope.store.clone(),
                source_root: raw_source.path().into(),
                source: LogicalPath::parse(path).unwrap(),
                source_owned: true,
                expected: Some(Object::from_bytes(raw)),
                include_private_location: false,
                maximum_bytes: 16 * 1024 * 1024,
                minimum_free_bytes: 100 * 1024 * 1024,
            },
        )
        .unwrap();
    }
    let admission = if let Some((parent_packet, parent_profile)) = parent {
        use reel_workflow::upstream::{Bindings, Reference, Selector};
        use reel_workflow_native::{retention, upstream};
        let (index, (path, art, _)) = assets
            .iter()
            .enumerate()
            .find(|(_, (_, a, _))| a.media_type == "image/png")
            .unwrap();
        let result = ReturnedResult {
            schema: RESULT_SCHEMA.into(),
            task_id: parent_packet.task.id.clone(),
            task_key: parent_packet.task_key.clone(),
            attempt_id: admission.attempt_id.clone(),
            artifacts: BTreeMap::from([("art".into(), art.clone())]),
            provenance: Provenance {
                mode: Mode::Handoff,
                provider: None,
                model: None,
                executable: None,
            },
            worker_receipt: None,
        };
        let returns = response::Returns {
            schema: "reel.workflow-returns.v1".into(),
            root: raw_source.path().into(),
            files: BTreeMap::from([("art".into(), path.clone())]),
        };
        response::ingest(
            &parent_packet,
            &case.source,
            &parent_profile,
            &admission,
            &result,
            &returns,
        )
        .unwrap();
        let ack = retention::retain(
            &parent_packet,
            &case.source,
            &parent_profile,
            &admission,
            &result,
            &retained_target(),
        )
        .unwrap();
        let acknowledgements = case.temp.path().join("parent-acknowledgements");
        fs::create_dir(&acknowledgements).unwrap();
        let runtime_raw = fs::read(&executable).unwrap();
        let control = reel_workflow_native::handoff::Profile {
            schema: "reel.workflow-native-handoff.v1".into(),
            source: serde_json::from_slice(&serde_json::to_vec(&case.source).unwrap()).unwrap(),
            admission: parent_profile.clone(),
            binding: reel_workflow_native::handoff::Binding {
                schema: "reel.workflow-native-handoff-binding.v1".into(),
                admission: admission.clone(),
                target: retained_target(),
            },
            runtime: Tool {
                path: executable.clone(),
                sha256: digest(&runtime_raw),
                bytes: runtime_raw.len() as u64,
            },
            acknowledgements,
        };
        let packet_path = case.temp.path().join("graph-parent-packet.json");
        let profile_path = case.temp.path().join("graph-parent-profile.json");
        let result_path = case.temp.path().join("graph-parent-result.json");
        let returns_path = case.temp.path().join("graph-parent-returns.json");
        let journal = case.temp.path().join("graph-parent-journal");
        for (path, raw) in [
            (&packet_path, serde_json::to_vec(&parent_packet).unwrap()),
            (&profile_path, serde_json::to_vec(&control).unwrap()),
            (&result_path, serde_json::to_vec(&result).unwrap()),
            (&returns_path, serde_json::to_vec(&returns).unwrap()),
        ] {
            fs::write(path, raw).unwrap();
        }
        let created = Command::new(&executable)
            .arg("create-handoff")
            .arg(&packet_path)
            .arg(&profile_path)
            .arg(&journal)
            .arg("--apply")
            .output()
            .unwrap();
        assert!(
            created.status.success(),
            "{}",
            String::from_utf8_lossy(&created.stdout)
        );
        let completed = Command::new(&executable)
            .arg("import-handoff")
            .arg(&journal)
            .arg(&profile_path)
            .arg(&result_path)
            .arg(&returns_path)
            .arg("--apply")
            .output()
            .unwrap();
        assert!(
            completed.status.success(),
            "{}",
            String::from_utf8_lossy(&completed.stdout)
        );
        let graph = reel_workflow_native::graph::Bindings {
            schema: "reel.workflow-run-bindings.v1".into(),
            tasks: BTreeMap::from([(
                parent_packet.task.id.clone(),
                reel_workflow_native::graph::Binding {
                    journal,
                    profile: reel_workflow_native::graph::PinnedFile {
                        path: profile_path.clone(),
                        sha256: digest(&fs::read(&profile_path).unwrap()),
                    },
                    staging_ticket: None,
                },
            )]),
        };
        fs::write(
            case.temp.path().join("graph-parent-bindings.json"),
            serde_json::to_vec(&graph).unwrap(),
        )
        .unwrap();
        let bindings = Bindings {
            schema: "reel.workflow-upstream-inputs.v1".into(),
            inputs: BTreeMap::from([(
                art.logical_id.clone(),
                Reference {
                    task_id: parent_packet.task.id.clone(),
                    artifact: Selector::Output { name: "art".into() },
                },
            )]),
        };
        let parents = BTreeMap::from([(
            parent_packet.task.id.clone(),
            upstream::Parent {
                packet: &parent_packet,
                result: &result,
                source: &case.source,
                admission: &parent_profile,
                receipt: &admission,
                acknowledgement: &ack,
            },
        )]);
        let frozen = upstream::Frozen::capture(&bindings, &parents);
        let mut child_source: NativeProfile =
            serde_json::from_slice(&serde_json::to_vec(&case.source).unwrap()).unwrap();
        child_source.snapshot.schema = "reel.workflow-native-composite-snapshot.v1".into();
        child_source.snapshot.upstream = Some(frozen);
        child_source.snapshot.bindings.remove(&art.logical_id);
        child_source.snapshot.input_streams.remove(&art.logical_id);
        child_source
            .snapshot
            .records
            .retain(|p| p != &format!("records/comic-{index}.tig"));
        let raw = bindings.canonical_bytes().unwrap();
        let mut task = fixture.packet.task.clone();
        task.depends_on = vec![parent_packet.task.id.clone()];
        task.upstream_inputs = Some(Artifact {
            logical_id: "workflow/comic-upstream".into(),
            sha256: digest(&raw),
            bytes: raw.len() as u64,
            media_type: "application/json".into(),
        });
        task.native_inputs = Some(Fixture::declaration(&child_source.snapshot));
        fixture.packet = make_packet(
            &Plan {
                schema: PLAN_SCHEMA.into(),
                project: fixture.packet.project.clone(),
                source_commit: fixture.packet.source_commit.clone(),
                tasks: vec![parent_packet.task.clone(), task],
            },
            "comic-render",
            BTreeMap::from([(
                parent_packet.task.id.clone(),
                DependencyResult {
                    result_sha256: canonical_digest(&result).unwrap(),
                    artifacts: result.artifacts.clone(),
                },
            )]),
        )
        .unwrap();
        let receipt = upstream::Admission::prepare(
            &fixture.packet,
            &child_source,
            &case.profile,
            "attempt-1",
            parents,
        )
        .unwrap()
        .accept()
        .unwrap();
        case.source = child_source;
        receipt
    } else {
        admission
    };
    case.temp.disable_cleanup(automatic);
    if automatic {
        eprintln!(
            "Automatic comic controller: {}; source: {}",
            case.temp.path().display(),
            fixture.temp.path().display()
        );
        if let Some(directory) = std::env::var_os("REEL_COMIC_AUTO_EVIDENCE_DIRECTORY") {
            fs::write(
                PathBuf::from(directory).join("original-operation-roots.json"),
                serde_json::to_vec(&json!({"root":case.temp.path(),"source":fixture.temp.path()}))
                    .unwrap(),
            )
            .unwrap();
        }
    }
    let mut output_owner = tempfile::tempdir_in(case.temp.path()).unwrap();
    output_owner.disable_cleanup(automatic);
    let output = output_owner.path().join("rendered");
    let rendered = Command::new(&executable)
        .arg("comic-render")
        .arg(raw_source.path().join("request.json"))
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        rendered.status.success(),
        "{}",
        String::from_utf8_lossy(&rendered.stdout)
    );
    let receipt_file = output.join("receipt.json");
    let receipt_raw = fs::read(&receipt_file).unwrap();
    let receipt: ComicReceipt = serde_json::from_slice(&receipt_raw).unwrap();
    let receipt_pin = |raw: &[u8]| Artifact {
        logical_id: "synthetic-comic/es/receipt.json".into(),
        sha256: digest(raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    };
    let result = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: "comic-render".into(),
        task_key: fixture.packet.task_key.clone(),
        attempt_id: "attempt-1".into(),
        artifacts: receipt.outputs.clone(),
        provenance: Provenance {
            mode: Mode::Tool,
            provider: None,
            model: None,
            executable: Some(worker),
        },
        worker_receipt: Some(receipt_pin(&receipt_raw)),
    };
    let returns = response::Returns {
        schema: "reel.workflow-returns.v1".into(),
        root: output.clone(),
        files: result
            .artifacts
            .keys()
            .map(|n| (n.clone(), n.clone()))
            .collect(),
    };
    let before = Index::open(&case.profile.index_root, &case.profile.index_minimum)
        .unwrap()
        .checkpoint();
    let mut false_receipt = receipt.clone();
    false_receipt.human_approval = true;
    let raw = serde_json::to_vec_pretty(&false_receipt).unwrap();
    fs::write(&receipt_file, &raw).unwrap();
    let mut false_result = result.clone();
    false_result.worker_receipt = Some(receipt_pin(&raw));
    assert_eq!(
        response::register(
            &fixture.packet,
            &case.source,
            &case.profile,
            &admission,
            &false_result,
            &returns
        )
        .unwrap_err()
        .kind,
        ErrorKind::Denied
    );
    let picture = output.join("page01-reading.svg");
    let original = fs::read(&picture).unwrap();
    let mut changed = original.clone();
    changed.push(b'\n');
    fs::write(&picture, &changed).unwrap();
    let mut altered = receipt.clone();
    altered
        .outputs
        .get_mut("page01-reading.svg")
        .unwrap()
        .sha256 = digest(&changed);
    altered.outputs.get_mut("page01-reading.svg").unwrap().bytes = changed.len() as u64;
    let raw = serde_json::to_vec_pretty(&altered).unwrap();
    fs::write(&receipt_file, &raw).unwrap();
    let mut false_result = result.clone();
    false_result.artifacts = altered.outputs;
    false_result.worker_receipt = Some(receipt_pin(&raw));
    assert_eq!(
        response::register(
            &fixture.packet,
            &case.source,
            &case.profile,
            &admission,
            &false_result,
            &returns
        )
        .unwrap_err()
        .kind,
        ErrorKind::Integrity
    );
    fs::write(&picture, &original).unwrap();
    fs::write(&receipt_file, &receipt_raw).unwrap();
    assert_eq!(
        Index::open(&case.profile.index_root, &before)
            .unwrap()
            .checkpoint(),
        before
    );
    let mut profile = handoff_profile(&case, admission, executable.clone());
    let packet_path = case.temp.path().join("comic-packet.json");
    let profile_path = case.temp.path().join("comic-profile.json");
    let result_path = case.temp.path().join("comic-result.json");
    let returns_path = case.temp.path().join("comic-returns.json");
    for (p, r) in [
        (&packet_path, serde_json::to_vec(&fixture.packet).unwrap()),
        (&profile_path, serde_json::to_vec(&profile).unwrap()),
        (&result_path, serde_json::to_vec(&result).unwrap()),
        (&returns_path, serde_json::to_vec(&returns).unwrap()),
    ] {
        fs::write(p, r).unwrap();
    }
    let journal = case.temp.path().join("comic-journal");
    let created = Command::new(&executable)
        .arg("create-handoff")
        .arg(&packet_path)
        .arg(&profile_path)
        .arg(&journal)
        .arg("--apply")
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stdout)
    );
    let mut worker_controls = Vec::new();
    let completed = if automatic {
        automatic_dispatch::run_comic(
            &case,
            &fixture.packet,
            &profile,
            &journal,
            &profile_path,
            &executable,
            &result_path,
            &mut worker_controls,
            &BTreeMap::new(),
        )
    } else if dispatch {
        use reel_workflow_native::{graph, staging, worker};
        let (plan, bindings, batch_path) = exercise_waiting_graph(
            &case,
            &fixture.packet,
            &profile,
            &journal,
            &profile_path,
            false,
        );
        graph::export_ready(
            &plan,
            &bindings,
            std::slice::from_ref(&fixture.packet.task.id),
            &batch_path,
        )
        .unwrap();
        let batch = graph::PinnedFile {
            path: batch_path,
            sha256: digest(&fs::read(case.temp.path().join("ready-batch.json")).unwrap()),
        };
        let mut changed_binding = graph::Bindings {
            schema: bindings.schema.clone(),
            tasks: bindings.tasks.clone(),
        };
        changed_binding
            .tasks
            .get_mut(&fixture.packet.task.id)
            .unwrap()
            .staging_ticket = None;
        assert!(
            graph::import_ready(
                &plan,
                &changed_binding,
                &batch,
                &fixture.packet.task.id,
                result.clone(),
                &returns
            )
            .is_err()
        );
        let parent = case.temp.path().join("worker-scratch");
        fs::create_dir(&parent).unwrap();
        let stage = staging::Profile {
            schema: "reel.workflow-staging.v1".into(),
            parent,
            maximum_bytes: 64 * 1024 * 1024,
            capacity_policy: reel_assets::admission::Policy {
                minimum_free_bytes: 65536,
                minimum_free_basis_points: 0,
                max_object_bytes: 512 * 1024 * 1024,
                ..Default::default()
            },
        };
        let plan_path = case.temp.path().join("worker-plan.json");
        let bindings_path = case.temp.path().join("worker-bindings.json");
        let stage_path = case.temp.path().join("worker-profile.json");
        let ticket_path = case.temp.path().join("worker-ticket.json");
        for (path, raw) in [
            (&plan_path, serde_json::to_vec(&plan).unwrap()),
            (&bindings_path, serde_json::to_vec(&bindings).unwrap()),
            (&stage_path, serde_json::to_vec(&stage).unwrap()),
        ] {
            fs::write(path, raw).unwrap();
        }
        let prepared = Command::new(&executable)
            .arg("prepare-worker")
            .arg(&plan_path)
            .arg(&bindings_path)
            .arg(&batch.path)
            .arg(&batch.sha256)
            .arg(&fixture.packet.task.id)
            .arg(&stage_path)
            .arg(&ticket_path)
            .arg("--apply")
            .output()
            .unwrap();
        assert!(
            prepared.status.success(),
            "{}",
            String::from_utf8_lossy(&prepared.stdout)
        );
        let raw = fs::read(&ticket_path).unwrap();
        let pin = digest(&raw);
        let ticket: worker::Ticket = serde_json::from_slice(&raw).unwrap();
        let invoke = |command: &str, hash: &str| {
            Command::new(&executable)
                .arg(command)
                .arg(&plan_path)
                .arg(&bindings_path)
                .arg(&ticket_path)
                .arg(hash)
                .arg("--apply")
                .output()
                .unwrap()
        };
        assert!(!invoke("run-worker", &"0".repeat(64)).status.success());
        let materialized = invoke("materialize-worker", &pin);
        assert!(
            materialized.status.success(),
            "{}",
            String::from_utf8_lossy(&materialized.stdout)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&materialized.stdout).unwrap()["task_completed"],
            false
        );
        assert_eq!(
            graph::inspect(&plan, &bindings)
                .unwrap()
                .rows
                .iter()
                .find(|r| r.task_id == fixture.packet.task.id)
                .unwrap()
                .status,
            reel_workflow::graph::Status::Waiting
        );
        for name in result.artifacts.keys() {
            assert_eq!(
                fs::read(ticket.directory().join("packet").join(name)).unwrap(),
                fs::read(returns.root.join(name)).unwrap()
            );
        }
        let mut unavailable: response::Returns =
            serde_json::from_slice(&fs::read(ticket.directory().join("returns.json")).unwrap())
                .unwrap();
        unavailable.root = case.temp.path().join("unavailable-worker-return");
        let unavailable_path = case.temp.path().join("unavailable-worker-returns.json");
        fs::write(&unavailable_path, serde_json::to_vec(&unavailable).unwrap()).unwrap();
        let pending = Command::new(&executable)
            .arg("import-ready")
            .arg(&plan_path)
            .arg(&bindings_path)
            .arg(&batch.path)
            .arg(&batch.sha256)
            .arg(&fixture.packet.task.id)
            .arg(ticket.directory().join("result.json"))
            .arg(&unavailable_path)
            .arg("--apply")
            .output()
            .unwrap();
        assert!(!pending.status.success());
        assert_eq!(
            graph::inspect(&plan, &bindings)
                .unwrap()
                .rows
                .iter()
                .find(|r| r.task_id == fixture.packet.task.id)
                .unwrap()
                .status,
            reel_workflow::graph::Status::NeedsRecovery
        );
        let completed = invoke("run-worker", &pin);
        if !completed.status.success() {
            eprintln!("Worker recovery fixture: {}", case.temp.path().display());
            let status = completed.status;
            let stdout = String::from_utf8_lossy(&completed.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&completed.stderr).into_owned();
            let _ = case.temp.keep();
            let _ = fixture.temp.keep();
            let _ = raw_source.keep();
            let _ = output_owner.keep();
            panic!("worker status {status}; stdout: {stdout}; stderr: {stderr}");
        }
        assert!(invoke("run-worker", &pin).status.success());
        let name = result
            .artifacts
            .keys()
            .find(|name| name.ends_with(".svg"))
            .unwrap();
        let path = ticket.directory().join("packet").join(name);
        let original = fs::read(&path).unwrap();
        fs::write(&path, b"altered owned fixture").unwrap();
        assert!(!invoke("run-worker", &pin).status.success());
        assert_eq!(fs::read(&path).unwrap(), b"altered owned fixture");
        fs::write(path, original).unwrap();
        fs::copy(ticket.directory().join("result.json"), &result_path).unwrap();
        worker_controls.extend([
            ("worker-ticket.json", raw),
            ("worker-plan.json", fs::read(&plan_path).unwrap()),
            ("worker-bindings.json", fs::read(&bindings_path).unwrap()),
            ("worker-batch.json", fs::read(&batch.path).unwrap()),
            ("worker-materialized.json", materialized.stdout),
            ("worker-interrupted-import.json", pending.stdout),
            (
                "worker-location.json",
                serde_json::to_vec(&json!({"workspace":ticket.directory(),"ticket":ticket_path}))
                    .unwrap(),
            ),
        ]);
        completed
    } else {
        Command::new(&executable)
            .arg("import-handoff")
            .arg(&journal)
            .arg(&profile_path)
            .arg(&result_path)
            .arg(&returns_path)
            .arg("--apply")
            .output()
            .unwrap()
    };
    if !completed.status.success() {
        eprintln!(
            "Synthetic comic diagnostic root: {}",
            case.temp.path().display()
        );
        let _ = case.temp.keep();
        let _ = fixture.temp.keep();
        let _ = raw_source.keep();
        let _ = output_owner.keep();
        panic!("{}", String::from_utf8_lossy(&completed.stdout));
    }
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&completed.stdout).unwrap()["task_completed"],
        true
    );
    let result: ReturnedResult = serde_json::from_slice(&fs::read(&result_path).unwrap()).unwrap();
    if composite {
        // Test-only job coverage loss, not a production close transaction.
        // Retained ownership must independently authorize historical replay.
        use tig_core::tree::{Tree, TreeLimits};
        use tig_store::{
            metadata::MetadataStore,
            source_claims::{
                Selection,
                protection::{self, work},
            },
        };
        let mut index = Index::open(
            &profile.admission.index_root,
            &profile.binding.admission.index_checkpoint,
        )
        .unwrap();
        let mut nodes = MetadataStore::open(
            &profile.admission.metadata_root,
            &profile.binding.admission.metadata_checkpoint,
        )
        .unwrap();
        let current = index
            .actor(&mut nodes, &profile.admission.scope.actor)
            .unwrap();
        let entries = Tree::from_root(current.selection.root)
            .entries(&mut nodes, TreeLimits::default())
            .unwrap()
            .0;
        let update = protection::Update {
            operation: tig_store::Key::parse("fixture-remove-comic-job-coverage").unwrap(),
            actor: profile.admission.scope.actor.clone(),
            owner: profile.admission.scope.owner.clone(),
            expected_revision: current.revision,
            selection: Selection {
                root: Tree::empty(&mut nodes).unwrap().root().clone(),
                context: current.selection.context,
            },
            changes: entries.keys().map(|p| (p.clone(), None)).collect(),
            ready: true,
        };
        let reference = work::stage(&mut index, &update, None, &mut nodes).unwrap();
        work::execute(&mut index, &reference, &mut nodes).unwrap();
    }
    let bare = case.temp.path().join("comic-history.git");
    git(
        fixture.temp.path(),
        &[
            "clone",
            "--bare",
            "--no-hardlinks",
            fixture.temp.path().to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    profile.source.snapshot.repository = bare;
    raw_source.close().unwrap();
    fs::write(&profile_path, serde_json::to_vec(&profile).unwrap()).unwrap();
    let historical = Command::new(&executable)
        .arg("resume-handoff")
        .arg(&journal)
        .arg(&profile_path)
        .arg("-")
        .arg("--apply")
        .output()
        .unwrap();
    assert!(
        historical.status.success(),
        "{}",
        String::from_utf8_lossy(&historical.stdout)
    );
    output_owner.close().unwrap();
    // Restore from native bytes alone, using bare history and the original retained
    // acknowledgement. Both raw worker inputs and the rendered packet are gone.
    use reel_workflow_native::{comic_export, staging};
    let ack_path = profile.acknowledgements.join(format!(
        "retained-{}-{}.json",
        profile.binding.admission.task_key, profile.binding.admission.attempt_id
    ));
    let ack: reel_workflow_native::retention::Acknowledgement =
        serde_json::from_slice(&fs::read(&ack_path).unwrap()).unwrap();
    if partitioned && !dispatch {
        use reel_workflow::{
            publication::InputReader,
            upstream::{Bindings, Reference, Selector},
        };
        let receipt = result.worker_receipt.as_ref().unwrap();
        let (name, pdf) = result
            .artifacts
            .iter()
            .find(|(name, _)| name.ends_with("reading.pdf"))
            .unwrap();
        let upstream = Bindings {
            schema: "reel.workflow-upstream-inputs.v1".into(),
            inputs: BTreeMap::from([
                (
                    receipt.logical_id.clone(),
                    Reference {
                        task_id: fixture.packet.task.id.clone(),
                        artifact: Selector::WorkerReceipt,
                    },
                ),
                (
                    pdf.logical_id.clone(),
                    Reference {
                        task_id: fixture.packet.task.id.clone(),
                        artifact: Selector::Output { name: name.clone() },
                    },
                ),
            ]),
        };
        let raw = upstream.canonical_bytes().unwrap();
        let mut child = fixture.packet.task.clone();
        child.id = "comic-input-consumer".into();
        child.operation = Operation::TextProposal;
        child.tool = None;
        child.prompt = "Synthetic retained comic input check.".into();
        child.native_inputs = None;
        child.inputs = vec![receipt.clone(), pdf.clone()];
        child.depends_on = vec![fixture.packet.task.id.clone()];
        child.upstream_inputs = Some(Artifact {
            logical_id: "workflow/comic-inputs".into(),
            sha256: digest(&raw),
            bytes: raw.len() as u64,
            media_type: "application/json".into(),
        });
        let plan = Plan {
            schema: PLAN_SCHEMA.into(),
            project: fixture.packet.project.clone(),
            source_commit: fixture.packet.source_commit.clone(),
            tasks: vec![fixture.packet.task.clone(), child],
        };
        let child = make_packet(
            &plan,
            "comic-input-consumer",
            BTreeMap::from([(
                fixture.packet.task.id.clone(),
                DependencyResult {
                    result_sha256: canonical_digest(&result).unwrap(),
                    artifacts: result.artifacts.clone(),
                },
            )]),
        )
        .unwrap();
        let parents = BTreeMap::from([(
            fixture.packet.task.id.clone(),
            reel_workflow_native::upstream::Parent {
                packet: &fixture.packet,
                result: &result,
                source: &profile.source,
                admission: &profile.admission,
                receipt: &profile.binding.admission,
                acknowledgement: &ack,
            },
        )]);
        let mut reader =
            reel_workflow_native::upstream::BorrowedInputs::open(&child, &upstream, parents)
                .unwrap();
        assert_eq!(reader.read(receipt, receipt.bytes).unwrap(), receipt_raw);
        let bytes = reader.read(pdf, pdf.bytes).unwrap();
        assert_eq!(digest(&bytes), pdf.sha256);
        worker_controls.extend([
            (
                "upstream-consumer-packet.json",
                serde_json::to_vec(&child).unwrap(),
            ),
            ("upstream-inputs.json", raw),
        ]);
    }
    let parent = case.temp.path().join("export-scratch");
    fs::create_dir(&parent).unwrap();
    let staging = staging::Profile {
        schema: "reel.workflow-staging.v1".into(),
        parent: parent.clone(),
        maximum_bytes: 16 * 1024 * 1024,
        capacity_policy: reel_assets::admission::Policy {
            minimum_free_bytes: 65536,
            minimum_free_basis_points: 0,
            max_object_bytes: 512 * 1024 * 1024,
            ..Default::default()
        },
    };
    let mut wrong_ack = ack.clone();
    wrong_ack.target.owner = tig_store::Key::parse("wrong-owner").unwrap();
    assert!(
        comic_export::prepare(
            &fixture.packet,
            &profile.source,
            &profile.admission,
            &profile.binding.admission,
            &result,
            &wrong_ack,
            &staging
        )
        .is_err()
    );
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
    let native_path = case.temp.path().join("export-native.json");
    let admission_path = case.temp.path().join("export-admission.json");
    let receipt_path = case.temp.path().join("export-receipt.json");
    let staging_path = case.temp.path().join("export-staging.json");
    let ticket_path = case.temp.path().join("export-ticket.json");
    for (p, raw) in [
        (&native_path, serde_json::to_vec(&profile.source).unwrap()),
        (
            &admission_path,
            serde_json::to_vec(&profile.admission).unwrap(),
        ),
        (
            &receipt_path,
            profile.binding.admission.canonical_bytes().unwrap(),
        ),
        (&staging_path, serde_json::to_vec(&staging).unwrap()),
    ] {
        fs::write(p, raw).unwrap();
    }
    let prepared = Command::new(&executable)
        .arg("prepare-comic-export")
        .arg(&packet_path)
        .arg(&native_path)
        .arg(&admission_path)
        .arg(&receipt_path)
        .arg(&result_path)
        .arg(&ack_path)
        .arg(&staging_path)
        .arg(&ticket_path)
        .arg("--apply")
        .output()
        .unwrap();
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stdout)
    );
    let ticket_raw = fs::read(&ticket_path).unwrap();
    let ticket_pin = digest(&ticket_raw);
    let ticket: comic_export::Ticket = serde_json::from_slice(&ticket_raw).unwrap();
    let exported_packet = ticket.packet_directory();
    assert!(!exported_packet.exists());
    assert_eq!(
        comic_export::export(
            &fixture.packet,
            &profile.source,
            &profile.admission,
            &ticket_raw,
            &"0".repeat(64)
        )
        .unwrap_err()
        .kind,
        ErrorKind::Integrity
    );
    let exported = Command::new(&executable)
        .arg("export-comic")
        .arg(&packet_path)
        .arg(&native_path)
        .arg(&admission_path)
        .arg(&ticket_path)
        .arg(&ticket_pin)
        .arg("--apply")
        .output()
        .unwrap();
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stdout)
    );
    assert!(!serde_json::from_slice::<serde_json::Value>(&exported.stdout).unwrap()["export"]["already_complete"].as_bool().unwrap());
    assert_eq!(
        fs::read(exported_packet.join("receipt.json")).unwrap(),
        receipt_raw
    );
    let repeated = comic_export::export_file(
        &fixture.packet,
        &profile.source,
        &profile.admission,
        &ticket_path,
        &ticket_pin,
    )
    .unwrap();
    assert!(repeated.already_complete);
    let mut missing: NativeProfile =
        serde_json::from_slice(&fs::read(&native_path).unwrap()).unwrap();
    missing.source.pool.repository = case.temp.path().join("unavailable-native-store");
    assert!(
        comic_export::export(
            &fixture.packet,
            &missing,
            &profile.admission,
            &ticket_raw,
            &ticket_pin
        )
        .is_err()
    );
    let lettering = exported_packet.join(format!("{}-lettering.svg", receipt.page_order[0]));
    let original_lettering = fs::read(&lettering).unwrap();
    fs::write(&lettering, b"altered local output").unwrap();
    assert_eq!(
        comic_export::export(
            &fixture.packet,
            &profile.source,
            &profile.admission,
            &ticket_raw,
            &ticket_pin
        )
        .unwrap_err()
        .kind,
        ErrorKind::Integrity
    );
    assert_eq!(fs::read(&lettering).unwrap(), b"altered local output");
    fs::write(&lettering, &original_lettering).unwrap();
    let restored = case.temp.path().join("restored-editable");
    let restore = Command::new(&executable)
        .arg("comic-restore")
        .arg(&exported_packet)
        .arg(&restored)
        .arg(&result.worker_receipt.as_ref().unwrap().sha256)
        .output()
        .unwrap();
    assert!(
        restore.status.success(),
        "{}",
        String::from_utf8_lossy(&restore.stdout)
    );
    let rebuilt = case.temp.path().join("rebuilt-restored");
    let rebuild = Command::new(&executable)
        .arg("comic-render")
        .arg(restored.join("request.json"))
        .arg(&rebuilt)
        .output()
        .unwrap();
    assert!(
        rebuild.status.success(),
        "{}",
        String::from_utf8_lossy(&rebuild.stdout)
    );
    for (name, artifact) in &result.artifacts {
        assert_eq!(
            digest(&fs::read(rebuilt.join(name)).unwrap()),
            artifact.sha256
        );
    }
    let mut restored_request: Request =
        serde_json::from_slice(&fs::read(restored.join("request.json")).unwrap()).unwrap();
    let page_path = restored.join(&restored_request.pages[0].path);
    let mut edited_page: PageSource =
        serde_json::from_slice(&fs::read(&page_path).unwrap()).unwrap();
    edited_page.utterances[0].lines = vec!["Mira la luz.".into()];
    let edited_raw = serde_json::to_vec_pretty(&edited_page).unwrap();
    fs::write(page_path, &edited_raw).unwrap();
    restored_request.pages[0].sha256 = digest(&edited_raw);
    restored_request.pages[0].bytes = edited_raw.len() as u64;
    fs::write(
        restored.join("request.json"),
        serde_json::to_vec_pretty(&restored_request).unwrap(),
    )
    .unwrap();
    let edited_output = case.temp.path().join("edited-restored");
    let edited = Command::new(&executable)
        .arg("comic-render")
        .arg(restored.join("request.json"))
        .arg(&edited_output)
        .output()
        .unwrap();
    assert!(
        edited.status.success(),
        "{}",
        String::from_utf8_lossy(&edited.stdout)
    );
    let page_id = &receipt.page_order[0];
    assert_eq!(
        fs::read(rebuilt.join(format!("{page_id}-clean.png"))).unwrap(),
        fs::read(edited_output.join(format!("{page_id}-clean.png"))).unwrap()
    );
    assert_ne!(
        fs::read(rebuilt.join(format!("{page_id}-reading.png"))).unwrap(),
        fs::read(edited_output.join(format!("{page_id}-reading.png"))).unwrap()
    );
    if let Some(unselected) = unselected {
        assert!(case.protected(&unselected).is_empty());
    }
    if let Some(directory) = std::env::var_os(if automatic {
        "REEL_COMIC_AUTO_EVIDENCE_DIRECTORY"
    } else if composite {
        "REEL_COMPOSITE_COMIC_EVIDENCE_DIRECTORY"
    } else if dispatch {
        "REEL_COMIC_WORKER_NATIVE_EVIDENCE_DIRECTORY"
    } else if partitioned {
        "REEL_COMIC_PART_NATIVE_EVIDENCE_DIRECTORY"
    } else {
        "REEL_COMIC_NATIVE_EVIDENCE_DIRECTORY"
    }) {
        use std::io::Write;
        let directory = PathBuf::from(directory);
        assert!(directory.is_absolute() && directory.is_dir());
        for (name, raw) in [
            ("packet.json", fs::read(packet_path).unwrap()),
            ("profile.json", fs::read(profile_path).unwrap()),
            ("result.json", fs::read(result_path).unwrap()),
            ("completion.json", completed.stdout),
            ("historical.json", historical.stdout),
            ("export-preparation.json", prepared.stdout),
            ("export-completion.json", exported.stdout),
            ("export-ticket.json", ticket_raw),
            (
                "locations.json",
                serde_json::to_vec(&json!({"journal":journal,"exported_packet":exported_packet,"restored":restored,"edited_output":edited_output})).unwrap(),
            ),
        ].into_iter().chain(worker_controls) {
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
fn returned_artifact_set_cannot_grow_a_job_beyond_its_qualified_close_bound() {
    use reel_workflow_native::{admission::Prepared, response};
    use tig_store::source_claims::protection::Index;
    let mut fixture = Fixture::new();
    fixture.packet.task.outputs.clear();
    for i in 0..110 {
        fixture.packet.task.outputs.insert(
            format!("part-{i:03}"),
            OutputContract {
                media_type: "text/plain".into(),
                max_bytes: 3,
                required: true,
            },
        );
    }
    fixture.repin();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let root = case.temp.path().join("returns");
    fs::create_dir(&root).unwrap();
    let mut artifacts = BTreeMap::new();
    let mut files = BTreeMap::new();
    for i in 0..110 {
        let name = format!("part-{i:03}");
        let raw = format!("{i:03}").into_bytes();
        fs::write(root.join(&name), &raw).unwrap();
        files.insert(name.clone(), name.clone());
        artifacts.insert(
            name.clone(),
            Artifact {
                logical_id: format!("candidate/{name}"),
                sha256: digest(&raw),
                bytes: 3,
                media_type: "text/plain".into(),
            },
        );
    }
    let result = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: fixture.packet.task.id.clone(),
        task_key: fixture.packet.task_key.clone(),
        attempt_id: "attempt-1".into(),
        artifacts,
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
        worker_receipt: None,
    };
    let before = Index::open(&case.profile.index_root, &receipt.index_checkpoint)
        .unwrap()
        .checkpoint();
    let error = response::register(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &response::Returns {
            schema: "reel.workflow-returns.v1".into(),
            root,
            files,
        },
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Budget);
    assert_eq!(error.code, "response-job-close-path-limit");
    assert_eq!(
        Index::open(&case.profile.index_root, &before)
            .unwrap()
            .checkpoint(),
        before
    );
    assert!(case.protected(&Object::from_bytes(b"000")).is_empty());
    assert_eq!(
        case.protected(&Object::from_bytes(b"synthetic input")),
        vec![case.profile.scope.actor.clone()]
    );
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
        Self::with_extra_slot(fixture, store, extra, false)
    }
    fn with_extra_slot(
        fixture: &Fixture,
        store: tig_core::Digest,
        extra: Option<tig_core::Digest>,
        second_slot: bool,
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
                    .chain(second_slot.then_some("slot2"))
                    .chain(second_slot.then_some("slot3"))
                    .chain(second_slot.then_some("slot4"))
                    .map(|id| Actor {
                        id: key(id),
                        owner: key("owner"),
                        stores: if id.starts_with("slot") {
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
        let mut initial: BTreeMap<_, _> = [
            ("main".into(), empty.clone()),
            ("retained".into(), empty.clone()),
            ("slot".into(), idle),
        ]
        .into();
        if second_slot {
            for slot in ["slot2", "slot3", "slot4"] {
                let raw = tig_core::identity::canonical_json(&json!([
                    "reel.job-actor-idle.experimental-1",
                    index.identity(),
                    slot,
                    "owner",
                    source_stores
                ]))
                .unwrap();
                initial.insert(
                    slot.into(),
                    Selection {
                        root: Tree::empty(&mut nodes).unwrap().root().clone(),
                        context: Digest::blob(&raw),
                    },
                );
            }
        }
        index.bootstrap(initial, &mut nodes).unwrap();
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
            movie_verifier: None,
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
        movie_verifier: None,
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
