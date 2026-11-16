use super::*;

fn selection() -> Selection {
    Selection {
        root: Object::from_bytes(b"selected"),
        context: Digest::blob(b"context"),
    }
}
fn closed_header() -> (Vec<String>, Value) {
    let labels = vec!["scene/scene-a".to_owned(), "parent".to_owned()];
    let sources = labels
        .iter()
        .map(|label| {
            phase("episode", "first", label)
                .unwrap()
                .as_str()
                .to_owned()
        })
        .collect::<Vec<_>>();
    let temporary = sources
        .iter()
        .map(|source| json!({"source":source,"selection":selection(),"revision":2}))
        .collect::<Vec<_>>();
    (
        labels,
        json!({"schema":"reel.tig-episode-retained-context.experimental-1","source_id":"episode","owner":"producer","operation":"first","root_revision_after":6,"succeeded":true,"approval":false,"expected_temporary":sources,"temporary":temporary}),
    )
}
fn pool(labels: &[String], header: &Value) -> Result<Pool> {
    pool_from_header(labels, "episode", "producer", 6, header)
}

#[test]
fn derives_only_prior_retired_identities_and_preserves_default_names() {
    let (labels, header) = closed_header();
    let reused = pool(&labels, &header).unwrap();
    for label in &labels {
        assert_eq!(reused[label].revision, 3);
        assert_eq!(
            source("episode", "next", label, Some(&reused)).unwrap(),
            phase("episode", "first", label).unwrap()
        );
        assert_eq!(
            source("episode", "next", label, None).unwrap(),
            phase("episode", "next", label).unwrap()
        );
        assert_ne!(
            source("episode", "next", label, None).unwrap().as_str(),
            reused[label].source
        );
    }
}

#[test]
fn refuses_foreign_stale_incomplete_and_duplicate_handoffs() {
    let (labels, header) = closed_header();
    for (field, value) in [
        ("owner", json!("other")),
        ("source_id", json!("other")),
        ("root_revision_after", json!(8)),
        ("approval", json!(true)),
    ] {
        let mut changed = header.clone();
        changed[field] = value;
        assert!(pool(&labels, &changed).is_err());
    }
    let mut changed = header.clone();
    changed["temporary"].as_array_mut().unwrap().pop();
    assert!(pool(&labels, &changed).is_err());
    changed["succeeded"] = json!(false);
    assert!(pool(&labels, &changed).is_err());
    let mut changed = header.clone();
    changed["temporary"][1] = changed["temporary"][0].clone();
    assert!(pool(&labels, &changed).is_err());
    let mut changed = header.clone();
    changed["temporary"][0]["prepared"] = json!({"unexpected":true});
    assert!(pool(&labels, &changed).is_err());
    let mut changed = header.clone();
    changed["temporary"][0]["revision"] = json!(1);
    changed["temporary"][0]["prepared"] = json!({
        "journal":Digest::blob(b"journal"),"operation":"pending-component"
    });
    assert!(pool(&labels, &changed).is_err());
    let mut changed = header;
    changed["expected_temporary"][1] = changed["expected_temporary"][0].clone();
    assert!(pool(&labels, &changed).is_err());
}

#[test]
fn carries_original_pool_through_success_and_partial_failure() {
    let (labels, mut header) = closed_header();
    let original = pool(&labels, &header).unwrap();
    header["reused_children"] = serde_json::to_value(&original).unwrap();
    header["operation"] = json!("second");
    header["root_revision_after"] = json!(12);
    for temporary in header["temporary"].as_array_mut().unwrap() {
        temporary["revision"] = json!(5);
    }
    let next = pool_from_header(&labels, "episode", "producer", 12, &header).unwrap();
    for label in &labels {
        assert_eq!(next[label].source, original[label].source);
        assert_eq!(next[label].revision, 6);
    }
    header["succeeded"] = json!(false);
    header["temporary"].as_array_mut().unwrap().truncate(1);
    let next = pool_from_header(&labels, "episode", "producer", 12, &header).unwrap();
    assert_eq!(next[&labels[0]].revision, 6);
    assert_eq!(next[&labels[1]].revision, 3);
}

#[test]
fn refuses_ticket_aliases_missing_labels_and_unretired_revisions() {
    let (labels, header) = closed_header();
    let original = pool(&labels, &header).unwrap();
    let mut changed = original.clone();
    changed.get_mut(&labels[0]).unwrap().source = "episode".into();
    assert!(validate_pool(&labels, "episode", Some(&changed)).is_err());
    let mut changed = original.clone();
    changed.get_mut(&labels[0]).unwrap().source = changed[&labels[1]].source.clone();
    assert!(validate_pool(&labels, "episode", Some(&changed)).is_err());
    let mut changed = original.clone();
    changed.remove(&labels[0]);
    assert!(validate_pool(&labels, "episode", Some(&changed)).is_err());
    let mut changed = original;
    changed.get_mut(&labels[0]).unwrap().revision = 0;
    assert!(validate_pool(&labels, "episode", Some(&changed)).is_err());
}

struct JournalFixture {
    journal: SourceJournal,
    pool: Pool,
    root: Selection,
    coordination: Coordination,
    _directory: tempfile::TempDir,
}
fn adopt(journal: &mut SourceJournal, id: &str, owner: &str, revision: u64, selected: Selection) {
    let prepared = journal
        .apply(
            key(&format!("{id}-prepare-{revision}")).unwrap(),
            Command::Prepare {
                source: key(id).unwrap(),
                owner: key(owner).unwrap(),
                expected_revision: revision,
                selection: selected,
            },
        )
        .unwrap();
    journal
        .apply(
            key(&format!("{id}-adopt-{revision}")).unwrap(),
            Command::Adopt {
                source: key(id).unwrap(),
                owner: key(owner).unwrap(),
                expected_revision: revision + 1,
                prepared: prepared.reference,
            },
        )
        .unwrap();
}
impl JournalFixture {
    fn new(child_owner: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let base = directory.path().canonicalize().unwrap();
        let mut journal = SourceJournal::create(
            &base.join("journal"),
            key("project").unwrap(),
            key("epoch").unwrap(),
        )
        .unwrap();
        let root = selection();
        adopt(&mut journal, "episode", "producer", 0, root.clone());
        adopt(&mut journal, "child", child_owner, 0, root.clone());
        journal
            .apply(
                key("child-withdraw").unwrap(),
                Command::Withdraw {
                    source: key("child").unwrap(),
                    owner: key(child_owner).unwrap(),
                    expected_revision: 2,
                },
            )
            .unwrap();
        let coordination = Coordination {
            root: base.join("coordination"),
        };
        fs::create_dir(&coordination.root).unwrap();
        Self {
            journal,
            pool: BTreeMap::from([(
                "parent".into(),
                Child {
                    source: "child".into(),
                    revision: 3,
                },
            )]),
            root,
            coordination,
            _directory: directory,
        }
    }
    fn check(&mut self) -> Result<()> {
        check_states(
            "episode",
            "producer",
            2,
            &self.root,
            &self.pool,
            &mut self.journal,
        )
    }
}

#[test]
fn refuses_active_pending_foreign_and_stale_sources_without_journal_changes() {
    let mut fixture = JournalFixture::new("producer");
    fixture.check().unwrap();
    let before = fixture.journal.checkpoint();
    fixture.pool.get_mut("parent").unwrap().revision = 2;
    assert!(fixture.check().is_err());
    assert_eq!(fixture.journal.checkpoint(), before);
    let mut fixture = JournalFixture::new("other");
    let before = fixture.journal.checkpoint();
    assert!(fixture.check().is_err());
    assert_eq!(fixture.journal.checkpoint(), before);
    let mut fixture = JournalFixture::new("producer");
    fixture
        .journal
        .apply(
            key("pending-child").unwrap(),
            Command::Prepare {
                source: key("child").unwrap(),
                owner: key("producer").unwrap(),
                expected_revision: 3,
                selection: fixture.root.clone(),
            },
        )
        .unwrap();
    fixture.pool.get_mut("parent").unwrap().revision = 4;
    let before = fixture.journal.checkpoint();
    assert!(fixture.check().is_err());
    assert_eq!(fixture.journal.checkpoint(), before);
    let mut fixture = JournalFixture::new("producer");
    adopt(
        &mut fixture.journal,
        "child",
        "producer",
        3,
        fixture.root.clone(),
    );
    fixture.pool.get_mut("parent").unwrap().revision = 5;
    let before = fixture.journal.checkpoint();
    assert!(fixture.check().is_err());
    assert_eq!(fixture.journal.checkpoint(), before);
}

#[test]
fn existing_identity_versions_work_at_the_unchanged_full_journal_guard() {
    let mut fixture = JournalFixture::new("producer");
    for i in 2..source_claims::MAX_SOURCES {
        fixture
            .journal
            .apply(
                key(&format!("fill-{i}")).unwrap(),
                Command::Prepare {
                    source: key(&format!("reserved-{i}")).unwrap(),
                    owner: key("reserved").unwrap(),
                    expected_revision: 0,
                    selection: fixture.root.clone(),
                },
            )
            .unwrap();
    }
    let before = fixture.journal.checkpoint();
    assert!(matches!(
        fixture.journal.apply(
            key("overflow").unwrap(),
            Command::Prepare {
                source: key("overflow").unwrap(),
                owner: key("producer").unwrap(),
                expected_revision: 0,
                selection: fixture.root.clone()
            }
        ),
        Err(source_claims::Error::Budget)
    ));
    assert_eq!(fixture.journal.checkpoint(), before);
    fixture.check().unwrap();
    let next = Selection {
        root: Object::from_bytes(b"next"),
        context: Digest::blob(b"next-context"),
    };
    let guard = jobs::ScopedSource::protect(
        &mut fixture.journal,
        &fixture.coordination,
        Identity {
            source: key("child").unwrap(),
            owner: key("producer").unwrap(),
        },
        3,
        &key("reuse-existing").unwrap(),
        &next,
    )
    .unwrap();
    drop(guard);
    fixture
        .journal
        .apply(
            key("retire-existing").unwrap(),
            Command::Withdraw {
                source: key("child").unwrap(),
                owner: key("producer").unwrap(),
                expected_revision: 5,
            },
        )
        .unwrap();
    let child = fixture
        .journal
        .source(&key("child").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(child.revision, 6);
    assert!(child.current.is_none());
    assert!(child.pending.is_none());
    assert_eq!(child.predecessors.len(), 2);
    assert_eq!(child.predecessors[0].selection, fixture.root);
    assert_eq!(child.predecessors[1].selection, next);
    assert!(matches!(
        fixture.journal.apply(
            key("still-full").unwrap(),
            Command::Prepare {
                source: key("still-full").unwrap(),
                owner: key("producer").unwrap(),
                expected_revision: 0,
                selection: fixture.root.clone()
            }
        ),
        Err(source_claims::Error::Budget)
    ));
}
