//! Interruption model over an actual runner-published success/failure packet.
use super::*;
use tig_core::{Digest, identity};
use tig_git::source::native::job::JobContext;
use tig_store::source_claims::{Command, Selection};

fn operation(profile: &Profile<'_>, phase: &str) -> Key {
    key(Digest::blob(&identity::canonical_json(&json!({
        "schema":"reel.tig-runner-operation.experimental-1","source":profile.identity.source.as_str(),
        "operation":profile.operation.as_str(),"phase":phase,
    })).unwrap()).as_str())
}
pub(super) fn exercise(
    resources: &JobContext,
    journal: &mut SourceJournal,
    journal_root: &Path,
    original: &Profile<'_>,
    retained: &Selection,
    succeeded: bool,
) {
    let mut reader =
        MetadataReader::open(original.metadata_root, original.metadata_minimum).unwrap();
    let mut entries = Tree::from_root(retained.root.clone())
        .entries(&mut reader, original.tree_limits)
        .unwrap()
        .0;
    let header_name = LogicalPath::parse("retention/identity").unwrap();
    let mut header: Value =
        serde_json::from_slice(&reader.get(&entries[&header_name]).unwrap().unwrap()).unwrap();
    let job_id = header["job_id"].as_str().unwrap().to_owned();
    let identity = Identity {
        source: key(if succeeded {
            "recover-success"
        } else {
            "recover-failure"
        }),
        owner: original.identity.owner.clone(),
    };
    let mut profile = Profile {
        config: original.config,
        coordination: original.coordination,
        identity,
        expected_revision: 2,
        operation: key("recover-retain"),
        metadata_root: original.metadata_root,
        metadata_minimum: original.metadata_minimum,
        tree_limits: original.tree_limits,
    };
    let start = journal
        .apply(
            key(if succeeded {
                "recover-success-start"
            } else {
                "recover-failure-start"
            }),
            Command::Prepare {
                source: profile.identity.source.clone(),
                owner: profile.identity.owner.clone(),
                expected_revision: 0,
                selection: resources.selection().clone(),
            },
        )
        .unwrap();
    journal
        .apply(
            key(if succeeded {
                "recover-success-adopt"
            } else {
                "recover-failure-adopt"
            }),
            Command::Adopt {
                source: profile.identity.source.clone(),
                owner: profile.identity.owner.clone(),
                expected_revision: 1,
                prepared: start.reference,
            },
        )
        .unwrap();
    let budget = tig_job_runner::RecoveryBudget {
        max_objects: 16,
        max_verify_bytes: 32 * 1024 * 1024,
        verify_millis: 30000,
    };
    let call = |journal: &mut SourceJournal,
                profile: &Profile<'_>,
                budget: &tig_job_runner::RecoveryBudget| {
        tig_job_runner::adopt_prepared_result(
            journal_root,
            &job_id,
            resources,
            journal,
            profile,
            budget,
        )
    };
    let before = journal.checkpoint();
    // A root with no prepared result cannot be completed by inventing one.
    assert!(call(journal, &profile, &budget).is_err());
    assert_eq!(journal.checkpoint(), before);
    header["source_id"] = json!(profile.identity.source.as_str());
    let raw = identity::canonical_json(&header).unwrap();
    let mut writer = MetadataStore::open(profile.metadata_root, profile.metadata_minimum).unwrap();
    let object = writer.put(&raw).unwrap();
    entries.insert(header_name, object.clone());
    let tree = Tree::build(&entries, &mut writer, profile.tree_limits)
        .unwrap()
        .0;
    writer.finish().unwrap();
    drop(writer);
    let selection = Selection {
        root: tree.root().clone(),
        context: object.sha256,
    };
    // This is the exact durable boundary reached by the retention sink: cached
    // bytes and metadata are complete, prepare is recorded, adopt is interrupted.
    let prepared = journal
        .apply(
            operation(&profile, "prepare"),
            Command::Prepare {
                source: profile.identity.source.clone(),
                owner: profile.identity.owner.clone(),
                expected_revision: 2,
                selection: selection.clone(),
            },
        )
        .unwrap();
    assert_eq!(prepared.source_revision, 3);
    let pending = journal.source(&profile.identity.source).unwrap().unwrap();
    let checkpoint = journal.checkpoint();
    let owner = profile.identity.owner.clone();
    profile.identity.owner = key("foreign-recovery-owner");
    assert!(call(journal, &profile, &budget).is_err());
    profile.identity.owner = owner;
    profile.operation = key("foreign-recovery-operation");
    assert!(call(journal, &profile, &budget).is_err());
    profile.operation = key("recover-retain");
    profile.expected_revision = 1;
    assert!(call(journal, &profile, &budget).is_err());
    profile.expected_revision = 2;
    assert!(
        tig_job_runner::adopt_prepared_result(
            journal_root,
            "job-other",
            resources,
            journal,
            &profile,
            &budget
        )
        .is_err()
    );
    let small = tig_job_runner::RecoveryBudget {
        max_objects: 16,
        max_verify_bytes: 1,
        verify_millis: 30000,
    };
    assert!(call(journal, &profile, &small).is_err());
    assert_eq!(journal.checkpoint(), checkpoint);
    assert_eq!(
        journal.source(&profile.identity.source).unwrap().unwrap(),
        pending
    );
    // Corrupt retained bytes do not consume or reset the durable prepare.
    let request = &entries[&LogicalPath::parse("result/request").unwrap()];
    let path = reel_assets::object(&profile.config.cache_roots[0], request.sha256.as_str());
    let original_bytes = fs::read(&path).unwrap();
    let mut changed = original_bytes.clone();
    changed[0] ^= 1;
    fs::write(&path, changed).unwrap();
    let refused = call(journal, &profile, &budget);
    fs::write(&path, original_bytes).unwrap();
    assert!(refused.is_err());
    assert_eq!(journal.checkpoint(), checkpoint);
    assert_eq!(
        journal.source(&profile.identity.source).unwrap().unwrap(),
        pending
    );
    let mut admission = json!({
        "schema":"reel.tig-prepared-recovery-profile.experimental-1", "job_id":job_id,
        "original_input":resources.selection(),
        "cache":{"path_policy":"configured-local-roots","cache_roots":profile.config.cache_roots},
        "coordination_root":profile.coordination.root,"journal_root":journal_root,
        "journal_minimum":journal.checkpoint(),"metadata_root":profile.metadata_root,
        "metadata_minimum":profile.metadata_minimum,"source_id":profile.identity.source.as_str(),
        "owner":profile.identity.owner.as_str(),"expected_revision":profile.expected_revision,
        "retain_operation":profile.operation.as_str(),
        "budget":{"max_objects":16,"max_verify_bytes":32*1024*1024,"verify_millis":30000}
    });
    let admission_path = journal_root.join(if succeeded {
        "recover-success.json"
    } else {
        "recover-failure.json"
    });
    let invoke = |value: &Value| {
        fs::write(&admission_path, identity::canonical_json(value).unwrap()).unwrap();
        std::process::Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
            .arg("adopt-prepared")
            .arg(&admission_path)
            .output()
            .unwrap()
    };
    admission["owner"] = json!("foreign-recovery-owner");
    assert!(!invoke(&admission).status.success());
    assert_eq!(journal.checkpoint(), checkpoint);
    assert_eq!(
        journal.source(&profile.identity.source).unwrap().unwrap(),
        pending
    );
    admission["owner"] = json!(profile.identity.owner.as_str());
    // Unknown fields fail closed before journal adoption.
    admission["remove_scratch"] = json!(true);
    assert!(!invoke(&admission).status.success());
    admission.as_object_mut().unwrap().remove("remove_scratch");
    let output = invoke(&admission);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    *journal = SourceJournal::open(journal_root, &checkpoint).unwrap();
    assert_eq!(result["source_revision"], 4);
    assert_eq!(result["succeeded"], succeeded);
    assert_eq!(result["staging_retired"], false);
    assert_eq!(result["authorizes_workspace_removal"], false);
    assert!(result["production_claim"].is_null());
    assert_eq!(result["approval"], false);
    let current = journal.source(&profile.identity.source).unwrap().unwrap();
    assert!(current.pending.is_none());
    assert_eq!(current.current.unwrap().selection, selection);
    assert_eq!(current.predecessors[0].selection, *resources.selection());
    let checkpoint = journal.checkpoint();
    let replay = call(journal, &profile, &budget).unwrap();
    assert_eq!(replay["replayed"], true);
    assert_eq!(journal.checkpoint(), checkpoint);
    let output = invoke(&admission);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["replayed"],
        true
    );
    *journal = SourceJournal::open(journal_root, &checkpoint).unwrap();
    assert_eq!(journal.checkpoint(), checkpoint);
    journal
        .apply(
            key(if succeeded {
                "new-success-work"
            } else {
                "new-failure-work"
            }),
            Command::Prepare {
                source: profile.identity.source.clone(),
                owner: profile.identity.owner.clone(),
                expected_revision: 4,
                selection: resources.selection().clone(),
            },
        )
        .unwrap();
    let checkpoint = journal.checkpoint();
    assert!(call(journal, &profile, &budget).is_err());
    assert_eq!(journal.checkpoint(), checkpoint);
}
