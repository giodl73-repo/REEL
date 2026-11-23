use super::*;
use git_asset::source::{
    native::updates,
    proposal::{Identity, Replacement},
};
use git_asset_core::tree::NodeStore;
use git_asset_model::{
    edits::{Change, Prepared},
    revisions::{self, Coverage},
    schema::Dialect,
};
use git_asset_store::metadata::{Limits as StoreLimits, MetadataStore};
#[path = "update_publication_cases.rs"]
mod publication_cases;

#[test]
fn captured_history_sequence_reopens_and_proposes_once_without_intermediate_git_edits() {
    let f = Fixture::new();
    f.record("item", "item/1", true, json!([]), Some(b"selected A"));
    let base = commit(f.root());
    let repository = reader(f.root());
    let snapshot = capture(&repository, base.clone());
    let temp = tempfile::tempdir().unwrap();
    let location = temp.path().join("metadata");
    let store = MetadataStore::create(&location, store_limits()).unwrap();
    let payload = Object::from_bytes(b"selected A");
    let a = manifest("item/1", "run-a", &payload, &[]);
    let revision = Object::from_bytes(&a);
    let subject = serde_json::from_value(serde_json::to_value(&payload).unwrap()).unwrap();
    let status = snapshot.resolve_id("item/1").unwrap().status().clone();
    let expected = snapshot
        .resolve_id("item/1")
        .unwrap()
        .reference()
        .object
        .clone();
    let mut input = Input {
        store: &store,
        reads: 0,
    };
    let prepared = snapshot
        .prepare_history_sequence(
            "item/1",
            &expected,
            Dialect::Draft202012,
            revisions::Limits::default(),
            vec![
                Change::RegisterRevision {
                    manifest: &a,
                    coverage: Coverage::Partial,
                },
                Change::SelectRevision {
                    revision: &revision,
                    representation: "main",
                    subject: &subject,
                    status: &status,
                    streams: &[],
                },
            ],
            &mut input,
        )
        .unwrap();
    assert_eq!(input.reads, 0);
    let (store, proposal) = persist_and_propose(&repository, &f, base, &prepared, &location, store);
    let next = capture(&repository, proposal);
    let inspection = next
        .inspect_history(
            "item/1",
            Dialect::Draft202012,
            revisions::Limits::default(),
            &mut Input {
                store: &store,
                reads: 0,
            },
        )
        .unwrap();
    assert_eq!(inspection.summary().revisions, 1);
    assert_eq!(inspection.summary().selected_revision, Some(revision));
    assert_eq!(
        snapshot.resolve_id("item/1").unwrap().selected().unwrap(),
        &payload
    );
    assert!(matches!(
        next.prepare_history_sequence(
            "item/1",
            &expected,
            Dialect::Draft202012,
            revisions::Limits::default(),
            vec![],
            &mut Input {
                store: &store,
                reads: 0
            }
        ),
        Err(updates::Error::Stale)
    ));
}

fn store_limits() -> StoreLimits {
    StoreLimits {
        nodes: 1024,
        bytes: 1024 * 1024,
        database_bytes: 4 * 1024 * 1024,
        min_free_bytes: 65_536,
    }
}

struct Input<'a> {
    store: &'a MetadataStore,
    reads: usize,
}
impl revisions::Reader for Input<'_> {
    fn read(
        &mut self,
        object: &Object,
        maximum: usize,
    ) -> std::result::Result<Vec<u8>, revisions::Error> {
        self.reads += 1;
        if object.bytes > maximum as u64 {
            return Err(revisions::Error::Budget);
        }
        let raw = self
            .store
            .get(object)
            .map_err(|_| revisions::Error::Missing)?
            .ok_or(revisions::Error::Missing)?;
        if Object::from_bytes(&raw) != *object {
            return Err(revisions::Error::Pin);
        }
        Ok(raw)
    }
}
fn manifest(id: &str, run: &str, payload: &Object, parents: &[Object]) -> Vec<u8> {
    serde_json::to_vec(&json!({"schema":"tig.asset-revision.experimental-1",
        "asset":id,"revision":run,"parents":parents,
        "representations":[{"key":"main","object":payload}],
        "provenance":{"kind":"imported","source":Object::from_bytes(b"synthetic original import")}}))
    .unwrap()
}
fn prepare(
    snapshot: &AssetSnapshot,
    id: &str,
    change: Change<'_>,
    store: &MetadataStore,
) -> Prepared {
    let expected = snapshot.resolve_id(id).unwrap().reference().object.clone();
    snapshot
        .prepare_history(
            id,
            &expected,
            Dialect::Draft202012,
            revisions::Limits::default(),
            change,
            &mut Input { store, reads: 0 },
        )
        .unwrap()
}
fn identity() -> Identity {
    Identity {
        name: "Synthetic author".into(),
        email: "fixture@example.invalid".into(),
        unix_seconds: 1_700_000_000,
    }
}
/// Caller orchestration over existing ports, not a new provider/custody API.
fn persist_and_propose(
    repository: &Repository,
    source: &Fixture,
    base: GitId,
    prepared: &Prepared,
    location: &Path,
    mut store: MetadataStore,
) -> (MetadataStore, GitId) {
    assert!(!prepared.retained());
    assert!(!prepared.authorizes_eviction());
    let mut written = Vec::new();
    for blob in prepared.metadata().iter().chain([prepared.record()]) {
        assert_eq!(store.put(blob.bytes()).unwrap(), *blob.object());
        written.push((blob.object().clone(), blob.bytes().to_vec()));
    }
    let minimum = store.finish().unwrap();
    drop(store);
    let store = MetadataStore::open(location, &minimum).unwrap();
    for (object, raw) in written {
        assert_eq!(store.get(&object).unwrap(), Some(raw));
    }
    let raw = store.get(prepared.record().object()).unwrap().unwrap();
    assert_eq!(Object::from_bytes(&raw), *prepared.record().object());
    let changes = [Replacement {
        path: LogicalPath::parse("records/item.asset").unwrap(),
        expected: prepared.expected().clone(),
        bytes: raw,
    }];
    let before = head(source.root());
    let working = fs::read(source.root().join("records/item.asset")).unwrap();
    let index = git(source.root(), &["write-tree"]).stdout;
    let identity = identity();
    let checked = repository
        .check_proposal(
            base.clone(),
            &changes,
            &identity,
            "Register explicit history",
        )
        .unwrap();
    let proposal = checked.create().unwrap();
    let retry = repository
        .propose(
            base.clone(),
            &changes,
            &identity,
            "Register explicit history",
        )
        .unwrap();
    assert_eq!(retry.commit(), proposal.commit());
    assert_eq!(retry.reference(), proposal.reference());
    assert_eq!(head(source.root()), before);
    assert_eq!(
        fs::read(source.root().join("records/item.asset")).unwrap(),
        working
    );
    assert_eq!(git(source.root(), &["write-tree"]).stdout, index);
    let changed = git(
        source.root(),
        &[
            "diff",
            "--name-only",
            base.as_str(),
            proposal.commit().as_str(),
        ],
    );
    assert_eq!(
        String::from_utf8(changed.stdout).unwrap().trim(),
        "records/item.asset"
    );
    (store, proposal.commit().clone())
}

#[test]
fn captured_revision_persists_reopens_and_proposes_one_file_without_selecting_or_touching_head() {
    for primary in [true, false] {
        let f = Fixture::new();
        let payload = Object::from_bytes(b"selected A");
        f.record("item", "item/1", primary, json!([]), Some(b"selected A"));
        f.record("other", "other/1", true, json!([]), Some(b"unrelated"));
        let base = commit(f.root());
        let repository = reader(f.root());
        let snapshot = capture(&repository, base.clone());
        let temp = tempfile::tempdir().unwrap();
        let location = temp.path().join("metadata");
        let store = MetadataStore::create(&location, store_limits()).unwrap();
        let a = manifest("item/1", "run-a", &payload, &[]);
        let update = prepare(
            &snapshot,
            "item/1",
            Change::RegisterRevision {
                manifest: &a,
                coverage: Coverage::Complete,
            },
            &store,
        );
        assert_eq!(update.metadata().len(), 2);
        let before: Value =
            serde_json::from_slice(&fs::read(f.root().join("records/item.asset")).unwrap())
                .unwrap();
        let candidate: Value = serde_json::from_slice(update.record().bytes()).unwrap();
        assert_eq!(candidate["selected"], before["selected"]);
        assert_eq!(candidate["status"], before["status"]);
        assert_eq!(candidate["authored"], before["authored"]);
        let (store, proposal) =
            persist_and_propose(&repository, &f, base.clone(), &update, &location, store);
        // Independent main change remains: only the explicit caller applies the proposal.
        f.record("other", "other/1", true, json!([]), Some(b"unrelated edit"));
        commit(f.root());
        git(f.root(), &["cherry-pick", proposal.as_str()]);
        let now = capture(&repository, head(f.root()));
        let inspected = now
            .inspect_history(
                "item/1",
                Dialect::Draft202012,
                revisions::Limits::default(),
                &mut Input {
                    store: &store,
                    reads: 0,
                },
            )
            .unwrap();
        assert_eq!(inspected.summary().revisions, 1);
        assert_eq!(inspected.summary().selected_revision, None);
        assert_eq!(
            now.resolve_id("other/1").unwrap().selected().unwrap(),
            &Object::from_bytes(b"unrelated edit")
        );
        assert_eq!(
            snapshot
                .inspect_history(
                    "item/1",
                    Dialect::Draft202012,
                    revisions::Limits::default(),
                    &mut Input {
                        store: &store,
                        reads: 0
                    }
                )
                .unwrap()
                .summary()
                .revisions,
            0
        );

        let mut input = Input {
            store: &store,
            reads: 0,
        };
        let expected = snapshot
            .resolve_id("item/1")
            .unwrap()
            .reference()
            .object
            .clone();
        assert!(matches!(
            now.prepare_history(
                "item/1",
                &expected,
                Dialect::Draft202012,
                revisions::Limits::default(),
                Change::RegisterRevision {
                    manifest: &a,
                    coverage: Coverage::Complete
                },
                &mut input
            ),
            Err(updates::Error::Stale)
        ));
        assert_eq!(input.reads, 0);
    }
}

#[test]
fn captured_update_ignores_dirty_newer_source_and_works_with_git_absent() {
    let f = Fixture::new();
    f.record("item", "item/1", true, json!([]), Some(b"A"));
    let base = commit(f.root());
    let repository = reader(f.root());
    let snapshot = capture(&repository, base);
    let expected = snapshot
        .resolve_id("item/1")
        .unwrap()
        .reference()
        .object
        .clone();
    f.record("item", "item/1", true, json!([]), Some(b"B"));
    commit(f.root());
    let mut newer = capture(&repository, head(f.root()));
    newer
        .retain(capture(&repository, snapshot_commit(f.root())))
        .unwrap();
    fs::write(
        f.root().join("records/item.asset"),
        b"dirty malformed working file",
    )
    .unwrap();
    fs::rename(f.root().join(".git"), f.root().join(".git-offline")).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let store = MetadataStore::create(&temp.path().join("metadata"), store_limits()).unwrap();
    let raw = manifest("item/1", "run-a", &Object::from_bytes(b"A"), &[]);
    let mut input = Input {
        store: &store,
        reads: 0,
    };
    assert!(matches!(
        newer.prepare_history(
            "item/1",
            &expected,
            Dialect::Draft202012,
            revisions::Limits::default(),
            Change::RegisterRevision {
                manifest: &raw,
                coverage: Coverage::Complete
            },
            &mut input
        ),
        Err(updates::Error::Stale)
    ));
    assert!(matches!(
        snapshot.prepare_history(
            "absent",
            &expected,
            Dialect::Draft202012,
            revisions::Limits::default(),
            Change::RegisterRevision {
                manifest: &raw,
                coverage: Coverage::Complete
            },
            &mut input
        ),
        Err(updates::Error::Missing)
    ));
    assert_eq!(input.reads, 0);
    let update = prepare(
        &snapshot,
        "item/1",
        Change::RegisterRevision {
            manifest: &raw,
            coverage: Coverage::Complete,
        },
        &store,
    );
    let candidate: Value = serde_json::from_slice(update.record().bytes()).unwrap();
    assert_eq!(
        candidate["selected"]["sha256"],
        json!(Object::from_bytes(b"A").sha256)
    );
    assert_eq!(
        fs::read(f.root().join("records/item.asset")).unwrap(),
        b"dirty malformed working file"
    );
}
fn snapshot_commit(root: &Path) -> GitId {
    GitId::parse(
        String::from_utf8(git(root, &["rev-parse", "HEAD^"]).stdout)
            .unwrap()
            .trim(),
    )
    .unwrap()
}

fn apply_change(
    f: &Fixture,
    repository: &Repository,
    location: &Path,
    store: MetadataStore,
    change: Change<'_>,
) -> MetadataStore {
    let base = head(f.root());
    let snapshot = capture(repository, base.clone());
    let update = prepare(&snapshot, "item/1", change, &store);
    let (store, proposal) = persist_and_propose(repository, f, base, &update, location, store);
    git(f.root(), &["cherry-pick", proposal.as_str()]);
    store
}

#[test]
fn concurrent_history_proposals_conflict_during_git_application_and_keep_both_candidates() {
    let f = Fixture::new();
    f.record("item", "item/1", true, json!([]), None);
    let base = commit(f.root());
    let repository = reader(f.root());
    let snapshot = capture(&repository, base.clone());
    let temp = tempfile::tempdir().unwrap();
    let location = temp.path().join("metadata");
    let store = MetadataStore::create(&location, store_limits()).unwrap();
    let a = manifest("item/1", "run-a", &Object::from_bytes(b"A"), &[]);
    let b = manifest("item/1", "run-b", &Object::from_bytes(b"B"), &[]);
    let update_a = prepare(
        &snapshot,
        "item/1",
        Change::RegisterRevision {
            manifest: &a,
            coverage: Coverage::Complete,
        },
        &store,
    );
    let update_b = prepare(
        &snapshot,
        "item/1",
        Change::RegisterRevision {
            manifest: &b,
            coverage: Coverage::Complete,
        },
        &store,
    );
    let (store, proposal_a) =
        persist_and_propose(&repository, &f, base.clone(), &update_a, &location, store);
    let (store, proposal_b) =
        persist_and_propose(&repository, &f, base, &update_b, &location, store);
    assert_ne!(proposal_a, proposal_b);
    git(f.root(), &["cherry-pick", proposal_a.as_str()]);
    let first_head = head(f.root());
    let conflict = Command::new(git_executable())
        .arg("-C")
        .arg(f.root())
        .args(["cherry-pick", proposal_b.as_str()])
        .output()
        .unwrap();
    assert!(!conflict.status.success());
    assert_eq!(head(f.root()), first_head);
    let unmerged =
        String::from_utf8(git(f.root(), &["diff", "--name-only", "--diff-filter=U"]).stdout)
            .unwrap();
    assert_eq!(unmerged.trim(), "records/item.asset");
    // Neither immutable candidate nor its new metadata is lost by the conflict.
    for update in [&update_a, &update_b] {
        for blob in update.metadata().iter().chain([update.record()]) {
            assert_eq!(
                store.get(blob.object()).unwrap(),
                Some(blob.bytes().to_vec())
            );
        }
    }
    assert!(
        !String::from_utf8(git(f.root(), &["for-each-ref", "refs/tig/proposals"]).stdout)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn draft_b_and_explicit_selection_preserve_retained_a_across_actual_git_changes() {
    let f = Fixture::new();
    f.record("item", "item/1", false, json!([]), None);
    commit(f.root());
    let repository = reader(f.root());
    let temp = tempfile::tempdir().unwrap();
    let location = temp.path().join("metadata");
    let store = MetadataStore::create(&location, store_limits()).unwrap();
    let a = manifest("item/1", "run-a", &Object::from_bytes(b"A"), &[]);
    let a_pin = Object::from_bytes(&a);
    let b = manifest(
        "item/1",
        "run-b",
        &Object::from_bytes(b"B"),
        std::slice::from_ref(&a_pin),
    );
    let b_pin = Object::from_bytes(&b);
    let a_subject = serde_json::from_value(json!(Object::from_bytes(b"A"))).unwrap();
    let b_subject = serde_json::from_value(json!(Object::from_bytes(b"B"))).unwrap();
    let approved = json!({"editorial":"accepted"}).as_object().unwrap().clone();
    let review = json!({"editorial":"needs-review"})
        .as_object()
        .unwrap()
        .clone();
    let store = apply_change(
        &f,
        &repository,
        &location,
        store,
        Change::RegisterRevision {
            manifest: &a,
            coverage: Coverage::Complete,
        },
    );
    let store = apply_change(
        &f,
        &repository,
        &location,
        store,
        Change::SelectRevision {
            revision: &a_pin,
            representation: "main",
            subject: &a_subject,
            status: &approved,
            streams: &[],
        },
    );
    let a_commit = head(f.root());
    let a_record = fs::read(f.root().join("records/item.asset")).unwrap();
    let objects_before = store.checkpoint().nodes();
    let store = apply_change(
        &f,
        &repository,
        &location,
        store,
        Change::RegisterRevision {
            manifest: &b,
            coverage: Coverage::Complete,
        },
    );
    let draft: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/item.asset")).unwrap()).unwrap();
    assert_eq!(draft["selected"], json!(Object::from_bytes(b"A")));
    assert_eq!(draft["status"], json!(approved));
    assert_eq!(store.checkpoint().nodes() - objects_before, 3); // manifest, page, new record only
    let store = apply_change(
        &f,
        &repository,
        &location,
        store,
        Change::SelectRevision {
            revision: &b_pin,
            representation: "main",
            subject: &b_subject,
            status: &review,
            streams: &[],
        },
    );
    let final_record = fs::read(f.root().join("records/item.asset")).unwrap();
    let final_value: Value = serde_json::from_slice(&final_record).unwrap();
    assert_eq!(final_value["selected"], json!(Object::from_bytes(b"B")));
    assert_eq!(final_value["status"], json!(review));
    assert_eq!(store.get(&a_pin).unwrap(), Some(a));
    assert_eq!(
        store.get(&Object::from_bytes(&a_record)).unwrap(),
        Some(a_record.clone())
    );
    let frozen = capture(&repository, a_commit);
    let old = frozen
        .inspect_history(
            "item/1",
            Dialect::Draft202012,
            revisions::Limits::default(),
            &mut Input {
                store: &store,
                reads: 0,
            },
        )
        .unwrap();
    assert_eq!(old.summary().selected_revision, Some(a_pin));
    assert_eq!(old.summary().revisions, 1);
    let now = capture(&repository, head(f.root()));
    let current = now
        .inspect_history(
            "item/1",
            Dialect::Draft202012,
            revisions::Limits::default(),
            &mut Input {
                store: &store,
                reads: 0,
            },
        )
        .unwrap();
    assert_eq!(current.summary().selected_revision, Some(b_pin));
    assert_eq!(current.summary().revisions, 2);
    // A stale expected record cannot write a proposal against the new base.
    let before_refs = git(f.root(), &["for-each-ref", "refs/tig/proposals"]).stdout;
    let replacement = [Replacement {
        path: LogicalPath::parse("records/item.asset").unwrap(),
        expected: Object::from_bytes(&a_record),
        bytes: final_record.clone(),
    }];
    assert!(
        repository
            .check_proposal(head(f.root()), &replacement, &identity(), "stale")
            .is_err()
    );
    assert_eq!(
        git(f.root(), &["for-each-ref", "refs/tig/proposals"]).stdout,
        before_refs
    );
    assert_eq!(
        fs::read(f.root().join("records/item.asset")).unwrap(),
        final_record
    );
}
