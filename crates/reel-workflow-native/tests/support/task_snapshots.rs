//! Opt-in retained native history across actual Git revisions. No admission,
//! selection, render, publication, source close or scratch retirement occurs.
use super::*;
use reel_workflow::graph::Status;
use reel_workflow::publication::InputReader;
use reel_workflow_native::{NativeInputs, NativeProfile, graph, handoff, retention};

fn load<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> T {
    decode(&fs::read(path).unwrap()).unwrap()
}
fn save(path: impl AsRef<Path>, value: &impl serde::Serialize) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    use std::io::Write;
    file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
    file.sync_all().unwrap();
}
fn journal_snapshot(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let entries: Vec<_> = fs::read_dir(path).unwrap().collect();
    assert!(entries.len() <= 128);
    entries
        .into_iter()
        .map(|e| {
            let entry = e.unwrap();
            let metadata = fs::symlink_metadata(entry.path()).unwrap();
            assert!(metadata.is_file() && !metadata.file_type().is_symlink());
            assert!(metadata.len() <= MAX_CONTROL_BYTES as u64);
            (
                PathBuf::from(entry.file_name()),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}
fn owned_git(git: &Path, repository: &Path, index: &Path, args: &[&str]) -> String {
    let output = Command::new(git)
        .args([
            "-c",
            "core.hooksPath=",
            "-c",
            "user.name=Workflow Fixture",
            "-c",
            "user.email=workflow-fixture@example.invalid",
        ])
        .args(args)
        .env("GIT_INDEX_FILE", index)
        .current_dir(repository)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn proposal(id: &str, language: &str, inputs: Vec<Artifact>, parents: Vec<String>) -> Task {
    Task {
        id: id.into(),
        native_inputs: None,
        upstream_inputs: None,
        operation: Operation::TextProposal,
        tool: None,
        language: Some(language.into()),
        prompt: "Synthetic language-local candidate; no translation approval.".into(),
        inputs,
        depends_on: parents,
        outputs: BTreeMap::from([(
            "note".into(),
            OutputContract {
                media_type: "text/plain".into(),
                max_bytes: 1024,
                required: true,
            },
        )]),
    }
}

#[test]
#[ignore = "Read-only existing retained comic/pool; creates only a separate bare fixture Git history and evidence"]
fn retained_task_survives_real_revision_and_rollback_without_rebinding_or_claim_changes() {
    let original = PathBuf::from(
        std::env::var_os("REEL_SCOPED_RETAINED_COMIC").expect("Set retained comic root"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_SCOPED_EVIDENCE_DIRECTORY").expect("Set evidence directory"),
    );
    let packet: Packet = load(original.join("packet.json"));
    let profile_path = original.join("handoff-profile.json");
    let profile: handoff::Profile = load(&profile_path);
    let result: ReturnedResult = load(original.join("result.json"));
    let ack = load(original.join("ack.json"));
    assert!(packet.dependencies.is_empty());
    assert_eq!(packet.source_commit, profile.source.snapshot.commit);
    let claims_path = profile.admission.index_root.join("claims.json");
    let claims = fs::read(&claims_path).unwrap();
    let journal = journal_snapshot(&original.join("journal"));
    let mut temp = tempfile::tempdir().unwrap();
    temp.disable_cleanup(true);
    let root = temp.path();
    let marker = json!({"root":root,"original":original,"original_task_key":packet.task_key,"original_commit":packet.source_commit,"native_effects":false});
    save(root.join("original-operation.json"), &marker);
    save(evidence.join("original-operation-roots.json"), &marker);
    // Verify actual retained bytes before constructing the second revision.
    let before = retention::review_copy(
        &packet,
        &profile.source,
        &profile.admission,
        &profile.binding.admission,
        &result,
        &ack,
    )
    .unwrap();
    let before_copy = serde_json::to_vec(before.copy()).unwrap();
    drop(before);
    let history = root.join("history.git");
    let cloned = Command::new(&profile.source.snapshot.git.path)
        .args(["clone", "--bare", "--no-hardlinks", "--"])
        .arg(&profile.source.snapshot.repository)
        .arg(&history)
        .output()
        .unwrap();
    assert!(
        cloned.status.success(),
        "{}",
        String::from_utf8_lossy(&cloned.stderr)
    );
    let index = root.join("owned-git-index");
    let git = &profile.source.snapshot.git.path;
    owned_git(git, &history, &index, &["read-tree", &packet.source_commit]);
    let authored = root.join("revision.json");
    save(
        &authored,
        &json!({"schema":"reel.synthetic-snapshot-revision.v1","language":"en","change":"language-local candidate prompt","approval":false}),
    );
    let blob = owned_git(
        git,
        &history,
        &index,
        &["hash-object", "-w", authored.to_str().unwrap()],
    );
    owned_git(
        git,
        &history,
        &index,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            "100644",
            &blob,
            "workflow-fixture-revision.json",
        ],
    );
    let tree = owned_git(git, &history, &index, &["write-tree"]);
    let commit = owned_git(
        git,
        &history,
        &index,
        &[
            "commit-tree",
            &tree,
            "-p",
            &packet.source_commit,
            "-m",
            "Synthetic language-local workflow revision",
        ],
    );
    owned_git(
        git,
        &history,
        &index,
        &["update-ref", "refs/heads/workflow-fixture-b", &commit],
    );
    assert_ne!(commit, packet.source_commit);
    assert_eq!(
        owned_git(
            git,
            &history,
            &index,
            &["rev-parse", &format!("{commit}^1")]
        ),
        packet.source_commit
    );
    let mut affected = proposal("en-candidate", "en", packet.task.inputs.clone(), vec![]);
    affected.native_inputs = packet.task.native_inputs.clone();
    let base = Plan {
        schema: PLAN_SCHEMA.into(),
        project: packet.project.clone(),
        source_commit: packet.source_commit.clone(),
        task_snapshots: BTreeMap::new(),
        tasks: vec![
            packet.task.clone(),
            affected,
            proposal(
                "unchanged-consumer",
                "es",
                vec![],
                vec![packet.task.id.clone()],
            ),
            proposal(
                "affected-consumer",
                "en",
                vec![],
                vec!["en-candidate".into()],
            ),
        ],
    };
    let old_affected = make_packet(&base, "en-candidate", BTreeMap::new()).unwrap();
    let mut next = base.clone();
    next.schema = SCOPED_PLAN_SCHEMA.into();
    next.source_commit = commit.clone();
    next.task_snapshots = next
        .tasks
        .iter()
        .map(|t| {
            (
                t.id.clone(),
                if t.id == packet.task.id || t.id == "unchanged-consumer" {
                    packet.source_commit.clone()
                } else {
                    commit.clone()
                },
            )
        })
        .collect();
    let mut new_source: NativeProfile =
        decode(&serde_json::to_vec(&profile.source).unwrap()).unwrap();
    new_source.snapshot.repository = history;
    new_source.snapshot.commit = commit.clone();
    let declaration = new_source.snapshot.declaration_bytes().unwrap();
    next.tasks[1].native_inputs = Some(Artifact {
        logical_id: packet
            .task
            .native_inputs
            .as_ref()
            .unwrap()
            .logical_id
            .clone(),
        sha256: digest(&declaration),
        bytes: declaration.len() as u64,
        media_type: "application/json".into(),
    });
    next.tasks[1]
        .prompt
        .push_str(" Use the explicitly authored revision B context.");
    let new_affected = make_packet(&next, "en-candidate", BTreeMap::new()).unwrap();
    assert_ne!(new_affected.task_key, old_affected.task_key);
    assert_eq!(
        make_packet(&next, &packet.task.id, BTreeMap::new()).unwrap(),
        packet
    );
    // Exact old records/payloads remain readable from the new bare commit;
    // the old native declaration cannot masquerade as a B declaration.
    assert!(SnapshotInputs::open(&new_affected, &profile.source.snapshot).is_err());
    assert!(SnapshotInputs::open(&old_affected, &new_source.snapshot).is_err());
    let mut a = NativeInputs::open(&old_affected, &profile.source).unwrap();
    let historical: BTreeMap<_, _> = packet
        .task
        .inputs
        .iter()
        .map(|pin| (pin.logical_id.clone(), a.read(pin, pin.bytes).unwrap()))
        .collect();
    drop(a);
    // Each NativeInputs retains its native object gates until dropped. Compare
    // these small fixture inputs sequentially rather than opening the same
    // gated objects again while the historical readers are still live.
    let mut b = NativeInputs::open(&new_affected, &new_source).unwrap();
    for pin in &packet.task.inputs {
        assert_eq!(historical[&pin.logical_id], b.read(pin, pin.bytes).unwrap());
    }
    drop(b);
    let bindings = graph::Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::from([(
            packet.task.id.clone(),
            graph::Binding {
                journal: original.join("journal"),
                profile: graph::PinnedFile {
                    path: profile_path,
                    sha256: digest(&fs::read(original.join("handoff-profile.json")).unwrap()),
                },
                staging_ticket: None,
            },
        )]),
    };
    let inspect = |plan: &Plan| {
        let frontier = graph::inspect(plan, &bindings).unwrap();
        let row = |id: &str| {
            frontier
                .rows
                .iter()
                .find(|r| r.task_id == id)
                .unwrap()
                .status
        };
        assert_eq!(row(&packet.task.id), Status::RetainedObserved);
        assert_eq!(row("en-candidate"), Status::NeedsAdmission);
        assert_eq!(row("unchanged-consumer"), Status::NeedsAdmission);
        assert_eq!(row("affected-consumer"), Status::Blocked);
        assert_eq!(
            frontier.needs_admission["unchanged-consumer"].dependencies[&packet.task.id]
                .result_sha256,
            canonical_digest(&result).unwrap()
        );
        frontier
    };
    let previous = inspect(&base);
    let advanced = inspect(&next);
    assert_eq!(
        previous.needs_admission["unchanged-consumer"],
        advanced.needs_admission["unchanged-consumer"]
    );
    assert_ne!(
        previous.needs_admission["en-candidate"].task_key,
        advanced.needs_admission["en-candidate"].task_key
    );
    let rolled_back = inspect(&base);
    assert_eq!(
        previous.needs_admission["en-candidate"],
        rolled_back.needs_admission["en-candidate"]
    );
    // Retargeting the retained task itself must refuse before native effects.
    let mut stale = next.clone();
    stale
        .task_snapshots
        .insert(packet.task.id.clone(), commit.clone());
    let refused = graph::inspect(&stale, &bindings).unwrap();
    assert_eq!(
        refused
            .rows
            .iter()
            .find(|r| r.task_id == packet.task.id)
            .unwrap()
            .status,
        Status::Refused
    );
    assert!(!refused.needs_admission.contains_key(&packet.task.id));
    let after = retention::review_copy(
        &packet,
        &profile.source,
        &profile.admission,
        &profile.binding.admission,
        &result,
        &ack,
    )
    .unwrap();
    assert_eq!(serde_json::to_vec(after.copy()).unwrap(), before_copy);
    drop(after);
    assert_eq!(fs::read(claims_path).unwrap(), claims);
    assert_eq!(journal_snapshot(&original.join("journal")), journal);
    save(root.join("baseline-plan.json"), &base);
    save(root.join("candidate-plan.json"), &next);
    save(root.join("candidate-source.json"), &new_source);
    save(root.join("candidate-packet.json"), &new_affected);
    save(root.join("run-bindings.json"), &bindings);
    save(
        root.join("baseline-frontier.json"),
        &json!({"rows":previous.rows,"needs_admission":previous.needs_admission}),
    );
    save(
        root.join("candidate-frontier.json"),
        &json!({"rows":advanced.rows,"needs_admission":advanced.needs_admission}),
    );
    save(
        root.join("rollback-frontier.json"),
        &json!({"rows":rolled_back.rows,"needs_admission":rolled_back.needs_admission}),
    );
    let outcome = json!({"root":root,"original":original,"commit_a":packet.source_commit,"commit_b":commit,"retained_packet_unchanged":true,"actual_native_bytes_equal":true,"bare_history":true,"native_graph_retained_observed":true,"retargeted_task_refused":true,"baseline_plan_restored":true,"review_copy_and_ids_unchanged":true,"claims_unchanged":true,"new_admission":false,"selected":false,"consumer_adopted":false,"scratch_retired":false});
    save(root.join("qualification-result.json"), &outcome);
    save(evidence.join("qualification-result.json"), &outcome);
}
