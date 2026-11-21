use super::*;
use git_asset::source::native::placement::{
    handoff::{self, Stage, local},
    journal,
};
use git_asset_store::{
    AckLevel, Request as PublicationRequest,
    ledger::CapacityLedger,
    local::LocalStore,
    retention::{self, RetentionRequest},
};
use std::io::Cursor;

struct Fixture {
    temp: TempDir,
    journal: SourceJournal,
    metadata: MetadataReader,
    minimum: git_asset_store::metadata::Checkpoint,
    request: handoff::Request,
    intent: Selection,
    provider: local::Local,
}

fn reserve_fixture(
    ledger: &mut CapacityLedger,
    object: &Object,
    peak: u64,
    mut before_observe: impl FnMut(&mut CapacityLedger, usize),
) -> Result<git_asset_store::ledger::Receipt, git_asset_store::ledger::LedgerError> {
    use git_asset_store::ledger::LedgerError;
    for attempt in 0..8 {
        let probe =
            ledger.begin_probe(key(&format!("probe-{attempt}-{}", object.sha256.as_str())))?;
        before_observe(ledger, attempt);
        match ledger.observe(
            key(&format!("observe-{attempt}-{}", object.sha256.as_str())),
            probe.reference(),
            1024 * 1024 * 1024,
        ) {
            Err(LedgerError::NeedsProbe) => continue,
            Err(error) => return Err(error),
            Ok(_) => (),
        }
        match ledger.reserve(
            key(&format!("job-{}", object.sha256.as_str())),
            key("owner"),
            peak,
        ) {
            Err(LedgerError::NeedsProbe) => continue,
            result => return result,
        }
    }
    Err(LedgerError::NeedsProbe)
}

#[test]
fn handoff_fixture_refreshes_lost_probe_freshness_before_reserving() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("ledger");
    let mut ledger = CapacityLedger::create(&root, key("place"), key("epoch"), 0).unwrap();
    let mut attempts = Vec::new();
    let receipt = reserve_fixture(
        &mut ledger,
        &Object::from_bytes(b"synthetic"),
        4096,
        |ledger, attempt| {
            attempts.push(attempt);
            if attempt == 0 {
                *ledger = CapacityLedger::open(&root, &ledger.checkpoint()).unwrap();
            }
        },
    )
    .unwrap();
    assert_eq!(attempts, [0, 1]);
    assert!(!receipt.was_replayed());
}
fn collect(f: &HistoryFixture, raw: &mut BTreeMap<Object, Vec<u8>>) {
    for object in &f.objects {
        raw.insert(
            object.clone(),
            fs::read(f.layout.object_path(f.cache.path(), &object.sha256)).unwrap(),
        );
    }
    for name in ["records", "definitions"] {
        for entry in fs::read_dir(f.source.root().join(name)).unwrap() {
            let bytes = fs::read(entry.unwrap().path()).unwrap();
            raw.insert(Object::from_bytes(&bytes), bytes);
        }
    }
    for leaf in [
        "payload not present in store",
        "new selected output",
        "opaque historical witness",
        "opaque rendition profile",
        "opaque destination",
        "opaque gate",
        "new frozen context",
    ] {
        raw.insert(
            Object::from_bytes(leaf.as_bytes()),
            leaf.as_bytes().to_vec(),
        );
    }
}
impl Fixture {
    fn new() -> Self {
        let mut f = HistoryFixture::new(Layout::ReelCas, true);
        let mut raw = BTreeMap::new();
        collect(&f, &mut raw);
        let i = inspection(&f);
        let a = demand(&i, "temporary", &mut expander(&i));
        next(&mut f, true);
        collect(&f, &mut raw);
        let i = inspection(&f);
        let b = demand(&i, "main", &mut expander(&i));
        let temp = tempfile::tempdir().unwrap();
        let mut store = MetadataStore::create(
            &temp.path().join("metadata"),
            StoreLimits {
                nodes: 65536,
                bytes: 32 * 1024 * 1024,
                database_bytes: 64 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let old = a.archive(&mut store, TreeLimits::default()).unwrap();
        let new = b.archive(&mut store, TreeLimits::default()).unwrap();
        store.finish().unwrap();
        let mut journal =
            SourceJournal::create(&temp.path().join("journal"), key("project"), key("epoch"))
                .unwrap();
        adopt(&mut journal, "temporary", 0, old.clone(), "temporary");
        adopt(&mut journal, "main", 0, new.clone(), "main");
        let request = handoff::Request {
            journal: journal.identity().clone(),
            operation: key("complete-job"),
            required: AckLevel::ProcessRestart,
            previous: handoff::Stamp {
                source: key("temporary"),
                owner: key("owner"),
                revision: 2,
                selection: old,
            },
            successor: handoff::Stamp {
                source: key("main"),
                owner: key("owner"),
                revision: 2,
                selection: new,
            },
        };
        let intent = handoff::prepare(&request, &mut store, TreeLimits::default()).unwrap();
        let minimum = store.finish().unwrap();
        drop(store);
        let metadata = MetadataReader::open(&temp.path().join("metadata"), &minimum).unwrap();
        let mut places = BTreeMap::new();
        for place in ["working", "archive"] {
            let mut store = LocalStore::create(&temp.path().join(place), key(place)).unwrap();
            let mut ledger = CapacityLedger::create(
                &temp.path().join(format!("ledger-{place}")),
                key(place),
                key("epoch"),
                0,
            )
            .unwrap();
            let objects: BTreeSet<_> = a
                .claim()
                .placements
                .iter()
                .chain(&b.claim().placements)
                .filter(|p| p.place == place)
                .map(|p| p.object.clone())
                .collect();
            let mut grants = BTreeMap::new();
            for object in objects {
                let bytes = raw
                    .get(&object)
                    .expect("every synthetic object has real exact bytes");
                let operation = |prefix: &str| key(&format!("{prefix}-{}", object.sha256.as_str()));
                let publication = PublicationRequest {
                    operation: operation("publish"),
                    owner: key("owner"),
                    object: object.clone(),
                    required: AckLevel::ProcessRestart,
                };
                git_asset_store::publish(&mut store, &publication, &mut Cursor::new(bytes))
                    .unwrap();
                let plan = retention::plan(
                    &store,
                    &RetentionRequest {
                        operation: operation("retain"),
                        owner: key("owner"),
                        publication,
                        minimum_ack: AckLevel::ProcessRestart,
                    },
                )
                .unwrap();
                let job =
                    reserve_fixture(&mut ledger, &object, plan.required_peak() * 2, |_, _| {})
                        .unwrap();
                let retained =
                    retention::retain(&mut ledger, &store, job.reference(), &plan).unwrap();
                ledger.finish(operation("finish"), job.reference()).unwrap();
                grants.insert(object, retained.grant);
            }
            places.insert(
                place.into(),
                local::Place {
                    store,
                    ledger,
                    grants,
                },
            );
        }
        let fixture = Self {
            temp,
            journal,
            metadata,
            minimum,
            request,
            intent,
            provider: local::Local { places },
        };
        fixture.save_operation();
        fixture
    }
    fn save_operation(&self) {
        fs::write(
            self.temp.path().join("operation.json"),
            serde_json::to_vec(&(&self.request, &self.intent)).unwrap(),
        )
        .unwrap();
    }
    fn run(&mut self) -> Result<handoff::Completion<local::Guard>, handoff::Error> {
        handoff::execute(
            &self.request,
            &self.intent,
            &mut self.journal,
            &self.metadata,
            handoff::Limits::default(),
            &mut self.provider,
        )
    }
    fn bind(&mut self) {
        let root = self.temp.path().join("metadata");
        let mut store = MetadataStore::open(&root, &self.minimum).unwrap();
        self.intent = handoff::prepare(&self.request, &mut store, TreeLimits::default()).unwrap();
        self.minimum = store.finish().unwrap();
        drop(store);
        self.metadata = MetadataReader::open(&root, &self.minimum).unwrap();
        self.save_operation();
    }
    fn reopen(&mut self) {
        let (request, intent): (handoff::Request, Selection) =
            serde_json::from_slice(&fs::read(self.temp.path().join("operation.json")).unwrap())
                .unwrap();
        self.request = request;
        self.intent = intent;
        self.journal = SourceJournal::open(
            &self.temp.path().join("journal"),
            &self.journal.checkpoint(),
        )
        .unwrap();
        self.metadata =
            MetadataReader::open(&self.temp.path().join("metadata"), &self.minimum).unwrap();
        let mut places = BTreeMap::new();
        for (
            name,
            local::Place {
                store,
                ledger,
                grants,
            },
        ) in std::mem::take(&mut self.provider.places)
        {
            let witness = ledger.checkpoint();
            drop(ledger);
            drop(store);
            places.insert(
                name.clone(),
                local::Place {
                    store: LocalStore::open(&self.temp.path().join(&name)).unwrap(),
                    ledger: CapacityLedger::open(
                        &self.temp.path().join(format!("ledger-{name}")),
                        &witness,
                    )
                    .unwrap(),
                    grants,
                },
            );
        }
        self.provider.places = places;
    }
}

#[test]
fn real_retained_successor_bytes_precede_working_withdrawal_and_replay_is_historical() {
    let mut f = Fixture::new();
    let result = f.run().unwrap();
    assert!(result.fresh_protection_acknowledged() && result.held_guards() > 0);
    assert!(!result.authorizes_eviction());
    // Held successor guards are object scoped; unrelated ingest stays available.
    let bytes = b"independent new item";
    let publication = PublicationRequest {
        operation: key("independent-ingest"),
        owner: key("other-owner"),
        object: Object::from_bytes(bytes),
        required: AckLevel::ProcessRestart,
    };
    git_asset_store::publish(
        &mut f.provider.places.get_mut("working").unwrap().store,
        &publication,
        &mut Cursor::new(bytes),
    )
    .unwrap();
    let source = f.journal.source(&key("temporary")).unwrap().unwrap();
    assert!(source.current.is_none() && source.pending.is_none());
    assert_eq!(source.predecessors.len(), 1);
    let projected = journal::project(
        &f.journal.obligations(10).unwrap(),
        &f.metadata,
        journal::Limits::default(),
    )
    .unwrap();
    let required = projected.view().plan(&BTreeSet::new()).unwrap().ensure;
    let old = Object::from_bytes(b"payload not present in store");
    assert!(!required.contains(&Placement {
        object: old.clone(),
        place: "working".into()
    }));
    assert!(required.contains(&Placement {
        object: old,
        place: "archive".into()
    }));
    let checkpoint = f.journal.checkpoint();
    drop(result);
    f.reopen();
    let replay = f.run().unwrap();
    assert!(replay.receipt().replayed);
    assert!(!replay.fresh_protection_acknowledged());
    assert_eq!(replay.held_guards(), 0);
    assert_eq!(f.journal.checkpoint(), checkpoint);
}

#[test]
fn missing_or_weak_grants_never_withdraw_old_working_scope() {
    for weak in [false, true] {
        let mut f = Fixture::new();
        let object = Object::from_bytes(b"payload not present in store");
        let grant = if weak {
            f.request.required = AckLevel::ProviderDurable;
            f.bind();
            None
        } else {
            Some(
                f.provider
                    .places
                    .get_mut("archive")
                    .unwrap()
                    .grants
                    .remove(&object)
                    .unwrap(),
            )
        };
        assert!(matches!(f.run(), Err(handoff::Error::Protection)));
        let source = f.journal.source(&key("temporary")).unwrap().unwrap();
        assert!(source.current.is_some() && source.pending.is_some());
        if let Some(grant) = grant {
            f.provider
                .places
                .get_mut("archive")
                .unwrap()
                .grants
                .insert(object, grant);
            assert!(f.run().is_ok());
        }
    }
}

#[test]
fn phase_failures_reopen_and_finish_without_a_gap_or_permanent_hot_temporary_root() {
    for phase in 0..4 {
        let mut f = Fixture::new();
        let error = handoff::execute_with(
            &f.request,
            &f.intent,
            &mut f.journal,
            &f.metadata,
            handoff::Limits::default(),
            &mut f.provider,
            |stage| {
                let index = match stage {
                    Stage::Prepared => 0,
                    Stage::Protected => 1,
                    Stage::Aborted => 2,
                    Stage::Withdrawn => 3,
                };
                if index == phase {
                    Err(handoff::Error::Injected)
                } else {
                    Ok(())
                }
            },
        );
        assert!(matches!(error, Err(handoff::Error::Injected)));
        let before = f.journal.source(&key("temporary")).unwrap().unwrap();
        if phase < 3 {
            assert!(before.current.is_some());
        }
        // Lose volatile request values; recovery must load the retained host record.
        f.request.operation = key("lost-volatile-request");
        f.intent.root = Object::from_bytes(b"lost volatile root");
        f.reopen();
        let completion = f.run().unwrap();
        assert_eq!(completion.fresh_protection_acknowledged(), phase < 3);
        let source = f.journal.source(&key("temporary")).unwrap().unwrap();
        assert!(source.current.is_none() && source.pending.is_none());
    }
}

#[test]
fn stale_successor_refuses_and_unrelated_source_progress_does_not_block_handoff() {
    for stale in [false, true] {
        let mut f = Fixture::new();
        let mut peer =
            SourceJournal::open(&f.temp.path().join("journal"), &f.journal.checkpoint()).unwrap();
        let a = f.request.previous.selection.clone();
        let result = handoff::execute_with(
            &f.request,
            &f.intent,
            &mut f.journal,
            &f.metadata,
            handoff::Limits::default(),
            &mut f.provider,
            |stage| {
                if matches!(stage, Stage::Prepared) {
                    if stale {
                        adopt(&mut peer, "main", 2, a.clone(), "move-target");
                    } else {
                        adopt(&mut peer, "unrelated", 0, a.clone(), "unrelated");
                    }
                }
                Ok(())
            },
        );
        if stale {
            assert!(matches!(result, Err(handoff::Error::Stale)));
            assert!(
                f.journal
                    .source(&key("temporary"))
                    .unwrap()
                    .unwrap()
                    .current
                    .is_some()
            );
        } else {
            assert!(result.is_ok());
            assert!(
                f.journal
                    .source(&key("unrelated"))
                    .unwrap()
                    .unwrap()
                    .current
                    .is_some()
            );
        }
    }
}

#[test]
fn corrupt_retained_bytes_refuse_and_repaired_bytes_allow_exact_retry() {
    let mut f = Fixture::new();
    let object = Object::from_bytes(b"payload not present in store");
    let path = f
        .temp
        .path()
        .join("archive/objects")
        .join(object.sha256.as_str());
    fs::write(&path, b"corrupt").unwrap();
    assert!(matches!(f.run(), Err(handoff::Error::Protection)));
    assert!(
        f.journal
            .source(&key("temporary"))
            .unwrap()
            .unwrap()
            .current
            .is_some()
    );
    fs::write(&path, b"payload not present in store").unwrap();
    assert!(f.run().is_ok());
}

#[test]
fn changed_request_cannot_borrow_a_completed_operation_acknowledgment() {
    let mut f = Fixture::new();
    drop(f.run().unwrap());
    let checkpoint = f.journal.checkpoint();
    f.request.required = AckLevel::ProviderDurable;
    f.bind();
    assert!(matches!(
        f.run(),
        Err(handoff::Error::Journal(
            git_asset_store::source_claims::Error::Conflict
        ))
    ));
    assert_eq!(f.journal.checkpoint(), checkpoint);
}

#[test]
fn guard_admission_refuses_before_any_journal_transition() {
    let mut f = Fixture::new();
    let checkpoint = f.journal.checkpoint();
    let result = handoff::execute(
        &f.request,
        &f.intent,
        &mut f.journal,
        &f.metadata,
        handoff::Limits {
            guards: 1,
            ..handoff::Limits::default()
        },
        &mut f.provider,
    );
    assert!(matches!(result, Err(handoff::Error::Budget)));
    assert_eq!(f.journal.checkpoint(), checkpoint);
}

#[test]
fn foreign_journal_with_matching_source_names_and_roots_is_not_the_admitted_authority() {
    let mut f = Fixture::new();
    let mut other = SourceJournal::create(
        &f.temp.path().join("foreign-journal"),
        key("project"),
        key("epoch"),
    )
    .unwrap();
    adopt(
        &mut other,
        "temporary",
        0,
        f.request.previous.selection.clone(),
        "temporary",
    );
    adopt(
        &mut other,
        "main",
        0,
        f.request.successor.selection.clone(),
        "main",
    );
    let checkpoint = other.checkpoint();
    assert!(matches!(
        handoff::execute(
            &f.request,
            &f.intent,
            &mut other,
            &f.metadata,
            handoff::Limits::default(),
            &mut f.provider
        ),
        Err(handoff::Error::Scope)
    ));
    assert_eq!(other.checkpoint(), checkpoint);
}

#[test]
fn native_place_identity_cannot_be_relabelled_as_another_protected_destination() {
    let mut f = Fixture::new();
    let source = &f.provider.places["working"];
    let duplicate = local::Place {
        store: LocalStore::open(&f.temp.path().join("working")).unwrap(),
        ledger: CapacityLedger::open(
            &f.temp.path().join("ledger-working"),
            &source.ledger.checkpoint(),
        )
        .unwrap(),
        grants: source.grants.clone(),
    };
    f.provider.places.insert("archive".into(), duplicate);
    let placement = Placement {
        place: "archive".into(),
        object: Object::from_bytes(b"new selected output"),
    };
    assert!(matches!(
        handoff::Protection::verify(&mut f.provider, &placement, AckLevel::ProcessRestart),
        Err(handoff::Error::Protection)
    ));
    assert!(
        f.journal
            .source(&key("temporary"))
            .unwrap()
            .unwrap()
            .current
            .is_some()
    );
}
