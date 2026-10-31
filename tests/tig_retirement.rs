#![cfg(all(feature = "tig-publication", windows))]
//! Fast synthetic custody packets. No media tool runs or producer claim.
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};
use tig_core::{
    Digest, LogicalPath, Object, identity,
    tree::{NodeStore, Tree, TreeLimits},
};
use tig_store::{
    Key,
    metadata::{Limits, MetadataStore},
    source_claims::{self, Selection, SourceJournal},
};

fn key(value: &str) -> Key {
    Key::parse(value).unwrap()
}
fn phase(label: &str) -> Key {
    key(Digest::blob(
        &identity::canonical_json(&json!({
            "schema":"reel.tig-runner-operation.experimental-1","source":"retirement-source",
            "operation":"retain-original","phase":label,
        }))
        .unwrap(),
    )
    .as_str())
}
fn put(cache: &Path, raw: &[u8]) -> Object {
    let pin = Object::from_bytes(raw);
    let path = reel_assets::object(cache, pin.sha256.as_str());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, raw).unwrap();
    pin
}
fn name(value: &str) -> LogicalPath {
    LogicalPath::parse(value).unwrap()
}

struct Fixture {
    _temp: tempfile::TempDir,
    profile: Value,
    profile_path: PathBuf,
    cache: PathBuf,
    workspace: PathBuf,
    control: PathBuf,
    journal: PathBuf,
    checkpoint: source_claims::Checkpoint,
    primary: Object,
    output: Object,
    installed: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        Self::with_outcome(true)
    }
    fn with_outcome(succeeded: bool) -> Self {
        Self::with_installed_scope(succeeded, false)
    }
    fn with_installed_scope(succeeded: bool, inside: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let cache = root.join("cache");
        let control = root.join("coordination");
        fs::create_dir_all(cache.join("objects/sha256")).unwrap();
        fs::create_dir_all(control.join("admissions")).unwrap();
        let primary = put(&cache, b"primary");
        let tool = put(&cache, b"tool never executed");
        let output = put(&cache, b"rendered");
        let installed = if inside {
            cache.join("staging/caminos/job-retirement-proof/inside-tools/tool.exe")
        } else {
            root.join("old-worktree/bin/tool.exe")
        };
        fs::create_dir_all(installed.parent().unwrap()).unwrap();
        let installed_bytes = b"installed tool never executed";
        fs::write(&installed, installed_bytes).unwrap();
        let installed_pin = Object::from_bytes(installed_bytes);
        let request = identity::canonical_json(&json!({
            "budget_bytes":1048576,"scratch_bytes":65536,
            "inputs":{"source.bin":primary},"tools":{"tools/unused.exe":tool},
            "installed_tools":{"installed":{"path":installed,"sha256":installed_pin.sha256,"bytes":installed_pin.bytes}},
            "steps":[{"tool":"tools/unused.exe","args":[],"timeout_seconds":1}],
            "outputs":{"report":{"path":"out.bin","max_bytes":1024}}
        }))
        .unwrap();
        reel_assets::jobs::check_request(&request).unwrap();
        let request_pin = put(&cache, &request);
        let outputs = json!({"report":output});
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&identity::canonical_json(&json!({
            "schema":"reel.job-retirement-evidence.v1","request_utf8":std::str::from_utf8(&request).unwrap(),
            "result":{"schema":if succeeded {"caminos.production-job-result.v1"} else {"reel.failed-job-result.v1"},"job_id":"job-retirement-proof",
                "manifest_sha256":request_pin.sha256,"outputs":if succeeded {outputs.clone()} else {json!({})}},
            "declared_retained_outputs":outputs,"diagnostics":{}
        })).unwrap()).unwrap();
        let evidence = put(&cache, &encoder.finish().unwrap());
        let metadata = root.join("metadata");
        let mut writer = MetadataStore::create(
            &metadata,
            Limits {
                nodes: 1024,
                bytes: 2 * 1024 * 1024,
                database_bytes: 8 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let input_header = writer
            .put(
                &identity::canonical_json(&json!({
                    "schema":"tig.native-job-context.experimental-1","claim":null
                }))
                .unwrap(),
            )
            .unwrap();
        let context = writer.put(b"synthetic admitted input context").unwrap();
        let input_tree = Tree::build(
            &BTreeMap::from([
                (name("job/identity"), input_header.clone()),
                (name("source/context"), context),
            ]),
            &mut writer,
            TreeLimits::default(),
        )
        .unwrap()
        .0;
        let original = Selection {
            root: input_tree.root().clone(),
            context: input_header.sha256,
        };
        let journal = root.join("journal");
        let mut owners =
            SourceJournal::create(&journal, key("retirement-fixture"), key("epoch")).unwrap();
        let start = owners
            .apply(
                key("input-prepare"),
                source_claims::Command::Prepare {
                    source: key("retirement-source"),
                    owner: key("retirement-owner"),
                    expected_revision: 0,
                    selection: original.clone(),
                },
            )
            .unwrap();
        owners
            .apply(
                key("input-adopt"),
                source_claims::Command::Adopt {
                    source: key("retirement-source"),
                    owner: key("retirement-owner"),
                    expected_revision: 1,
                    prepared: start.reference,
                },
            )
            .unwrap();
        let header = writer.put(&identity::canonical_json(&json!({
            "schema":"reel.tig-runner-retention.experimental-1","job_id":"job-retirement-proof",
            "succeeded":succeeded,"source":original,"journal":owners.identity(),"owner":"retirement-owner",
            "source_id":"retirement-source","revision":2,"outputs":1,"production_claim":null,
            "retention_operation":"retain-original"
        })).unwrap()).unwrap();
        let tree = Tree::build(
            &BTreeMap::from([
                (name("retention/identity"), header.clone()),
                (name("source/context"), original.root.clone()),
                (name("result/request"), request_pin),
                (name("result/evidence"), evidence),
                (
                    name(&format!("outputs/{}", Digest::blob(b"report").as_str())),
                    output.clone(),
                ),
            ]),
            &mut writer,
            TreeLimits::default(),
        )
        .unwrap()
        .0;
        let minimum = writer.finish().unwrap();
        drop(writer);
        let prepared = owners
            .apply(
                phase("prepare"),
                source_claims::Command::Prepare {
                    source: key("retirement-source"),
                    owner: key("retirement-owner"),
                    expected_revision: 2,
                    selection: Selection {
                        root: tree.root().clone(),
                        context: header.sha256,
                    },
                },
            )
            .unwrap();
        owners
            .apply(
                phase("adopt"),
                source_claims::Command::Adopt {
                    source: key("retirement-source"),
                    owner: key("retirement-owner"),
                    expected_revision: 3,
                    prepared: prepared.reference,
                },
            )
            .unwrap();
        let checkpoint = owners.checkpoint();
        drop(owners);
        let workspace = cache.join("staging/caminos/job-retirement-proof");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("job.json"), &request).unwrap();
        fs::hard_link(
            reel_assets::object(&cache, primary.sha256.as_str()),
            workspace.join("source.bin"),
        )
        .unwrap();
        fs::copy(
            reel_assets::object(&cache, output.sha256.as_str()),
            workspace.join("out.bin"),
        )
        .unwrap();
        let profile = json!({
            "schema":"reel.tig-workspace-retirement-profile.experimental-1","job_id":"job-retirement-proof",
            "original_input":original,"cache":{"path_policy":"configured-local-roots","cache_roots":[cache],"coordination_root":control},
            "coordination_root":control,"journal_root":journal,"journal_minimum":checkpoint,
            "metadata_root":metadata,"metadata_minimum":minimum,"source_id":"retirement-source","owner":"retirement-owner",
            "expected_revision":2,"retain_operation":"retain-original",
            "budget":{"max_objects":16,"max_verify_bytes":1048576,"verify_millis":30000},
            "retirement":{"workspace":workspace,"max_workspace_bytes":1048576}
        });
        let profile_path = root.join("retirement.json");
        Self {
            _temp: temp,
            profile,
            profile_path,
            cache,
            workspace,
            control,
            journal,
            checkpoint,
            primary,
            output,
            installed,
        }
    }
    fn run(&self, profile: &Value, apply: bool) -> std::process::Output {
        fs::write(
            &self.profile_path,
            identity::canonical_json(profile).unwrap(),
        )
        .unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_reel-tig-job"));
        command.arg("retire-workspace").arg(&self.profile_path);
        if apply {
            command.arg("--apply");
        }
        command.output().unwrap()
    }
    fn unchanged(&self) {
        let observed =
            SourceJournal::observe(&self.journal, &self.checkpoint, &key("retirement-source"))
                .unwrap();
        assert_eq!(observed.checkpoint, self.checkpoint);
        assert!(self.workspace.exists());
    }
    fn transactions(&self) -> Vec<PathBuf> {
        fs::read_dir(&self.control)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("retirement-")
                    && path
                        .extension()
                        .is_some_and(|extension| extension == "json")
            })
            .collect()
    }
}

#[test]
fn retirement_requires_retained_originals_quiescence_and_explicit_apply() {
    let f = Fixture::new();
    let result = f.run(&f.profile, false);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["workspace_logical_bytes_removed"], 0);
    assert!(f.transactions().is_empty());
    f.unchanged();
    let mut foreign = f.profile.clone();
    foreign["owner"] = json!("foreign-owner");
    assert!(!f.run(&foreign, true).status.success());
    let installed_saved = f.installed.with_extension("saved");
    fs::rename(&f.installed, &installed_saved).unwrap();
    let obsolete_tool = f.run(&f.profile, false);
    fs::rename(&installed_saved, &f.installed).unwrap();
    assert!(
        obsolete_tool.status.success(),
        "{}",
        String::from_utf8_lossy(&obsolete_tool.stderr)
    );
    f.unchanged();
    let primary = reel_assets::object(&f.cache, f.primary.sha256.as_str());
    let saved = primary.with_extension("saved");
    fs::rename(&primary, &saved).unwrap();
    let refused = f.run(&f.profile, true);
    fs::rename(&saved, &primary).unwrap();
    assert!(!refused.status.success()); // The workspace's remaining hard link is not backup.
    f.unchanged();
    for extension in ["json", "lease"] {
        let path = f
            .control
            .join(format!("admissions/job-retirement-proof.{extension}"));
        fs::write(&path, b"reserved").unwrap();
        assert!(!f.run(&f.profile, true).status.success());
        fs::remove_file(path).unwrap();
        f.unchanged();
    }
    let mut small = f.profile.clone();
    small["retirement"]["max_workspace_bytes"] = json!(1);
    assert!(!f.run(&small, true).status.success());
    fs::write(f.workspace.join("out.bin"), b"changed!").unwrap();
    assert!(!f.run(&f.profile, true).status.success());
    fs::write(f.workspace.join("out.bin"), b"rendered").unwrap();
    f.unchanged();
    let outside = f._temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"keep").unwrap();
    let link = f.workspace.join("escape");
    let junction = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(outside.to_string_lossy().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(
        junction.status.success(),
        "{}",
        String::from_utf8_lossy(&junction.stderr)
    );
    assert!(!f.run(&f.profile, true).status.success());
    fs::remove_dir(&link).unwrap(); // Remove the junction itself, never its target.
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"keep");
    let mut escaped = f.profile.clone();
    escaped["retirement"]["workspace"] = json!(outside.join("job-retirement-proof"));
    assert!(!f.run(&escaped, true).status.success());
    f.unchanged();
    // Unrelated writers do not block retirement of this exact completed job.
    fs::write(f.control.join("admissions/job-other.json"), b"unrelated").unwrap();
    let result = f.run(&f.profile, true);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["staging_retired"], true);
    assert_eq!(report["cache_objects_removed"], 0);
    assert!(!f.workspace.exists());
    assert_eq!(fs::read(primary).unwrap(), b"primary");
    assert_eq!(
        fs::read(reel_assets::object(&f.cache, f.output.sha256.as_str())).unwrap(),
        b"rendered"
    );
    let repeated = f.run(&f.profile, true);
    assert!(
        repeated.status.success(),
        "{}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&repeated.stdout).unwrap()["already_absent"],
        true
    );
    let observed =
        SourceJournal::observe(&f.journal, &f.checkpoint, &key("retirement-source")).unwrap();
    assert_eq!(observed.checkpoint, f.checkpoint);
}

#[test]
fn retirement_refuses_a_source_with_newer_pending_work() {
    let f = Fixture::new();
    let mut journal = SourceJournal::open(&f.journal, &f.checkpoint).unwrap();
    let source = journal.source(&key("retirement-source")).unwrap().unwrap();
    journal
        .apply(
            key("new-work"),
            source_claims::Command::Prepare {
                source: key("retirement-source"),
                owner: key("retirement-owner"),
                expected_revision: 4,
                selection: source.current.unwrap().selection,
            },
        )
        .unwrap();
    let checkpoint = journal.checkpoint();
    drop(journal);
    assert!(!f.run(&f.profile, true).status.success());
    assert!(f.workspace.exists());
    let observed =
        SourceJournal::observe(&f.journal, &checkpoint, &key("retirement-source")).unwrap();
    assert_eq!(observed.checkpoint, checkpoint);
    assert_eq!(observed.source.unwrap().revision, 5);
}

#[test]
fn failed_job_candidates_remain_retained_without_changing_the_outcome() {
    let f = Fixture::with_outcome(false);
    let output = f.run(&f.profile, true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["succeeded"], false);
    assert_eq!(report["staging_retired"], true);
    assert_eq!(report["approval"], false);
    assert!(report["production_claim"].is_null());
    assert!(!f.workspace.exists());
    assert_eq!(
        fs::read(reel_assets::object(&f.cache, f.output.sha256.as_str())).unwrap(),
        b"rendered"
    );
    let observed =
        SourceJournal::observe(&f.journal, &f.checkpoint, &key("retirement-source")).unwrap();
    assert_eq!(observed.checkpoint, f.checkpoint);
}

#[test]
fn partial_retirement_reverifies_survivors_before_resuming_without_the_manifest() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    let blocked = f.workspace.join("blocked.tmp");
    fs::write(&blocked, b"temporary scratch").unwrap();
    let guard = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&blocked)
        .unwrap();
    let interrupted = f.run(&f.profile, true);
    assert!(!interrupted.status.success());
    drop(guard);
    assert!(f.workspace.exists());
    let receipts = f.transactions();
    assert_eq!(receipts.len(), 1);
    let receipt = &receipts[0];
    let prepared = fs::read(receipt).unwrap();
    // Model interruption after the recursive removal has consumed job.json.
    let manifest = f.workspace.join("job.json");
    if manifest.exists() {
        fs::remove_file(&manifest).unwrap();
    }
    let checked = f.run(&f.profile, false);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let report: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(report["retirement_transaction"]["resumed"], true);
    assert_eq!(fs::read(receipt).unwrap(), prepared); // Dry-run does not advance intent.
    f.unchanged();
    fs::write(
        f.workspace.join("new-work.tmp"),
        b"not in original retirement",
    )
    .unwrap();
    assert!(!f.run(&f.profile, true).status.success());
    fs::remove_file(f.workspace.join("new-work.tmp")).unwrap();
    fs::write(&blocked, b"changed existing scratch").unwrap();
    assert!(!f.run(&f.profile, true).status.success());
    // Deletion is a valid surviving subset; changed/new files are not.
    fs::remove_file(&blocked).unwrap();
    let mut corrupt: Value = serde_json::from_slice(&prepared).unwrap();
    corrupt["body"]["scope"]["owner"] = json!("different-owner");
    fs::write(receipt, serde_json::to_vec(&corrupt).unwrap()).unwrap();
    assert!(!f.run(&f.profile, true).status.success());
    fs::write(receipt, &prepared).unwrap();
    let resumed = f.run(&f.profile, true);
    assert!(
        resumed.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    assert!(!f.workspace.exists());
    let completed: Value = serde_json::from_slice(&fs::read(receipt).unwrap()).unwrap();
    assert_eq!(completed["body"]["completed"], true);
    assert!(completed["body"]["snapshot"].is_null());
    assert!(fs::metadata(receipt).unwrap().len() < prepared.len() as u64);
    assert_eq!(
        fs::read(reel_assets::object(&f.cache, f.primary.sha256.as_str())).unwrap(),
        b"primary"
    );
    let observed =
        SourceJournal::observe(&f.journal, &f.checkpoint, &key("retirement-source")).unwrap();
    assert_eq!(observed.checkpoint, f.checkpoint);
}

#[test]
fn retirement_intent_cannot_be_reused_for_a_replacement_directory() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    let blocked = f.workspace.join("blocked.tmp");
    fs::write(&blocked, b"scratch").unwrap();
    let guard = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&blocked)
        .unwrap();
    assert!(!f.run(&f.profile, true).status.success());
    drop(guard);
    let moved = f._temp.path().join("original-workspace");
    let boundary = f._temp.path().canonicalize().unwrap();
    assert!(f.workspace.canonicalize().unwrap().starts_with(&boundary));
    assert_eq!(moved.parent().unwrap().canonicalize().unwrap(), boundary);
    fs::rename(&f.workspace, &moved).unwrap();
    fs::create_dir(&f.workspace).unwrap();
    fs::write(f.workspace.join("new-item"), b"must survive").unwrap();
    let refused = f.run(&f.profile, true);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("identity"));
    assert_eq!(
        fs::read(f.workspace.join("new-item")).unwrap(),
        b"must survive"
    );
    assert!(moved.exists());
}

#[test]
fn missing_manifest_without_a_recorded_retirement_intent_refuses() {
    let f = Fixture::new();
    fs::remove_file(f.workspace.join("job.json")).unwrap();
    assert!(!f.run(&f.profile, true).status.success());
    assert!(f.transactions().is_empty());
    f.unchanged();
}

#[test]
fn pinned_retirement_preserves_newer_work_and_does_not_require_a_duplicate_input_root() {
    let f = Fixture::new();
    let mut journal = SourceJournal::open(&f.journal, &f.checkpoint).unwrap();
    let old_result = journal
        .source(&key("retirement-source"))
        .unwrap()
        .unwrap()
        .current
        .unwrap()
        .selection;
    let new_input: Selection = serde_json::from_value(f.profile["original_input"].clone()).unwrap();
    let pending = journal
        .apply(
            key("newer-input"),
            source_claims::Command::Prepare {
                source: key("retirement-source"),
                owner: key("retirement-owner"),
                expected_revision: 4,
                selection: new_input,
            },
        )
        .unwrap();
    journal
        .apply(
            key("adopt-newer-input"),
            source_claims::Command::Adopt {
                source: key("retirement-source"),
                owner: key("retirement-owner"),
                expected_revision: 5,
                prepared: pending.reference,
            },
        )
        .unwrap();
    // The retained result itself keeps its source binding. A second direct
    // reference to input version 2 is not needed to retire redundant scratch.
    journal
        .apply(
            key("retire-old-direct-input"),
            source_claims::Command::Retire {
                source: key("retirement-source"),
                owner: key("retirement-owner"),
                expected_revision: 6,
                predecessor_revision: 2,
            },
        )
        .unwrap();
    let checkpoint = journal.checkpoint();
    let expected_source = journal.source(&key("retirement-source")).unwrap().unwrap();
    drop(journal);
    let newer_workspace = f.cache.join("staging/caminos/job-newer-work");
    fs::create_dir(&newer_workspace).unwrap();
    fs::write(newer_workspace.join("new-input"), b"keep newer work").unwrap();
    fs::write(
        f.control.join("admissions/job-newer-work.lease"),
        b"active unrelated job",
    )
    .unwrap();
    assert!(!f.run(&f.profile, true).status.success()); // Default remains current-only.
    let mut profile = f.profile.clone();
    profile["retirement"]["retained_result"] = serde_json::to_value(&old_result).unwrap();
    let checked = f.run(&profile, false);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(f.transactions().is_empty());
    let mut wrong = profile.clone();
    wrong["retirement"]["retained_result"] = f.profile["original_input"].clone();
    assert!(!f.run(&wrong, true).status.success());
    let retired = f.run(&profile, true);
    assert!(
        retired.status.success(),
        "{}",
        String::from_utf8_lossy(&retired.stderr)
    );
    let report: Value = serde_json::from_slice(&retired.stdout).unwrap();
    assert_eq!(report["result_revision"], 4);
    assert_eq!(report["source_revision"], 7);
    assert!(!f.workspace.exists());
    assert_eq!(
        fs::read(newer_workspace.join("new-input")).unwrap(),
        b"keep newer work"
    );
    assert!(f.control.join("admissions/job-newer-work.lease").exists());
    let observed =
        SourceJournal::observe(&f.journal, &checkpoint, &key("retirement-source")).unwrap();
    assert_eq!(observed.checkpoint, checkpoint);
    assert_eq!(observed.source, Some(expected_source));
}

#[test]
fn pinned_retirement_refuses_a_released_result_even_when_bytes_still_exist() {
    let f = Fixture::new();
    let mut journal = SourceJournal::open(&f.journal, &f.checkpoint).unwrap();
    let old = journal
        .source(&key("retirement-source"))
        .unwrap()
        .unwrap()
        .current
        .unwrap()
        .selection;
    let prepared = journal
        .apply(
            key("replace-result"),
            source_claims::Command::Prepare {
                source: key("retirement-source"),
                owner: key("retirement-owner"),
                expected_revision: 4,
                selection: serde_json::from_value(f.profile["original_input"].clone()).unwrap(),
            },
        )
        .unwrap();
    journal
        .apply(
            key("adopt-replacement"),
            source_claims::Command::Adopt {
                source: key("retirement-source"),
                owner: key("retirement-owner"),
                expected_revision: 5,
                prepared: prepared.reference,
            },
        )
        .unwrap();
    journal
        .apply(
            key("release-result"),
            source_claims::Command::Retire {
                source: key("retirement-source"),
                owner: key("retirement-owner"),
                expected_revision: 6,
                predecessor_revision: 4,
            },
        )
        .unwrap();
    let checkpoint = journal.checkpoint();
    drop(journal);
    let mut profile = f.profile.clone();
    profile["retirement"]["retained_result"] = serde_json::to_value(old).unwrap();
    assert!(!f.run(&profile, true).status.success());
    assert!(f.workspace.exists());
    assert!(f.transactions().is_empty());
    let observed =
        SourceJournal::observe(&f.journal, &checkpoint, &key("retirement-source")).unwrap();
    assert_eq!(observed.checkpoint, checkpoint);
}

#[test]
fn pinned_old_result_retirement_preserves_newer_pending_selection() {
    let f = Fixture::new();
    let mut journal = SourceJournal::open(&f.journal, &f.checkpoint).unwrap();
    let old = journal
        .source(&key("retirement-source"))
        .unwrap()
        .unwrap()
        .current
        .unwrap()
        .selection;
    journal
        .apply(
            key("pending-newer-work"),
            source_claims::Command::Prepare {
                source: key("retirement-source"),
                owner: key("retirement-owner"),
                expected_revision: 4,
                selection: serde_json::from_value(f.profile["original_input"].clone()).unwrap(),
            },
        )
        .unwrap();
    let checkpoint = journal.checkpoint();
    let before = journal.source(&key("retirement-source")).unwrap().unwrap();
    drop(journal);
    let mut profile = f.profile.clone();
    profile["retirement"]["retained_result"] = serde_json::to_value(old).unwrap();
    let result = f.run(&profile, true);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let after = SourceJournal::observe(&f.journal, &checkpoint, &key("retirement-source")).unwrap();
    assert_eq!(after.checkpoint, checkpoint);
    assert_eq!(after.source, Some(before));
    assert!(!f.workspace.exists());
}

#[test]
fn interrupted_retirement_can_resume_after_its_result_becomes_a_predecessor() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    let blocked = f.workspace.join("blocked.tmp");
    fs::write(&blocked, b"scratch").unwrap();
    let guard = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&blocked)
        .unwrap();
    assert!(!f.run(&f.profile, true).status.success());
    drop(guard);
    let manifest = f.workspace.join("job.json");
    if manifest.exists() {
        fs::remove_file(manifest).unwrap();
    }
    let mut journal = SourceJournal::open(&f.journal, &f.checkpoint).unwrap();
    let old = journal
        .source(&key("retirement-source"))
        .unwrap()
        .unwrap()
        .current
        .unwrap()
        .selection;
    let prepared = journal
        .apply(
            key("next-input"),
            source_claims::Command::Prepare {
                source: key("retirement-source"),
                owner: key("retirement-owner"),
                expected_revision: 4,
                selection: serde_json::from_value(f.profile["original_input"].clone()).unwrap(),
            },
        )
        .unwrap();
    journal
        .apply(
            key("adopt-next-input"),
            source_claims::Command::Adopt {
                source: key("retirement-source"),
                owner: key("retirement-owner"),
                expected_revision: 5,
                prepared: prepared.reference,
            },
        )
        .unwrap();
    let checkpoint = journal.checkpoint();
    drop(journal);
    let mut profile = f.profile.clone();
    profile["retirement"]["retained_result"] = serde_json::to_value(old).unwrap();
    let result = f.run(&profile, true);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["retirement_transaction"]["resumed"], true);
    assert_eq!(report["retirement_transaction"]["state"], "completed");
    assert!(!f.workspace.exists());
    let after = SourceJournal::observe(&f.journal, &checkpoint, &key("retirement-source")).unwrap();
    assert_eq!(after.checkpoint, checkpoint);
}

#[test]
fn obsolete_external_installation_does_not_block_owned_scratch_retirement() {
    let f = Fixture::new();
    fs::remove_file(&f.installed).unwrap();
    let bin = f.installed.parent().unwrap();
    fs::remove_dir(bin).unwrap();
    fs::remove_dir(bin.parent().unwrap()).unwrap();
    let result = f.run(&f.profile, true);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["installed_tool_paths_outside_scope"], 1);
    assert!(!f.workspace.exists());
    assert_eq!(
        fs::read(reel_assets::object(&f.cache, f.primary.sha256.as_str())).unwrap(),
        b"primary"
    );
}

#[test]
fn upgraded_external_tool_is_neither_revalidated_as_old_production_nor_removed() {
    let f = Fixture::new();
    fs::write(&f.installed, b"newer external tool version").unwrap();
    let result = f.run(&f.profile, true);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read(&f.installed).unwrap(),
        b"newer external tool version"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap()["production_claim"],
        Value::Null
    );
}

#[test]
fn installed_tool_inside_the_removal_scope_still_refuses() {
    let f = Fixture::with_installed_scope(true, true);
    let result = f.run(&f.profile, true);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("installed tool would be removed"));
    assert!(f.installed.exists());
    f.unchanged();
    assert!(f.transactions().is_empty());
}

#[test]
fn external_tool_path_junction_cannot_hide_a_tool_inside_scratch() {
    let f = Fixture::new();
    let saved = f._temp.path().join("external-runtime.saved");
    fs::rename(&f.installed, &saved).unwrap();
    let bin = f.installed.parent().unwrap();
    fs::remove_dir(bin).unwrap();
    let inside = f.workspace.join("inside-tools");
    fs::create_dir(&inside).unwrap();
    fs::write(inside.join("tool.exe"), b"installed tool never executed").unwrap();
    let junction = Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(bin.to_string_lossy().replace('/', "\\"))
        .arg(inside.to_string_lossy().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(
        junction.status.success(),
        "{}",
        String::from_utf8_lossy(&junction.stderr)
    );
    let result = f.run(&f.profile, true);
    assert!(!result.status.success());
    fs::remove_dir(bin).unwrap(); // Junction itself, never its target.
    assert!(inside.join("tool.exe").exists());
    assert!(saved.exists());
    f.unchanged();
}
