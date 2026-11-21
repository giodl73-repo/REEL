use super::*;

fn sequence(f: &mut Fixture, changes: Vec<Change<'_>>, limits: Limits) -> Result<Prepared, Error> {
    let expected = Object::from_bytes(&f.record);
    edits::sequence::prepare(
        Inputs {
            expected: &expected,
            instance: &f.record,
            definition: &f.definition,
            authored_schema: &f.schema,
            dialect: Dialect::Draft202012,
            limits,
        },
        changes,
        &mut f.store,
    )
}

#[test]
fn history_sequence_matches_serial_edits_without_intermediate_publication() {
    for kind in ["image", "document"] {
        let mut f = Fixture::new(kind);
        let original = f.value();
        let a = payload("A");
        let b = payload("B");
        let ma = Fixture::manifest("a", &a, &[]);
        let ra = Object::from_bytes(&ma);
        let mb = Fixture::manifest("b", &b, std::slice::from_ref(&ra));
        let rb = Object::from_bytes(&mb);
        let pa = pin(&a);
        let pb = pin(&b);
        let accepted = status("accepted-project-value");
        let draft = status("draft-project-value");
        let changes = || {
            vec![
                Change::RegisterRevision {
                    manifest: &ma,
                    coverage: Coverage::Complete,
                },
                Change::SelectRevision {
                    revision: &ra,
                    representation: "main",
                    subject: &pa,
                    status: &accepted,
                    streams: &["current"],
                },
                Change::RegisterRevision {
                    manifest: &mb,
                    coverage: Coverage::Complete,
                },
                Change::SelectRevision {
                    revision: &rb,
                    representation: "main",
                    subject: &pb,
                    status: &draft,
                    streams: &["current"],
                },
            ]
        };
        let prepared = sequence(&mut f, changes(), Limits::default()).unwrap();
        assert_eq!(prepared.expected(), &Object::from_bytes(&f.record));
        assert_eq!(prepared.metadata().len(), 4);
        assert!(f.store.objects.is_empty() && f.store.calls.is_empty());
        assert!(!prepared.retained() && !prepared.authorizes_eviction());
        let mut serial = Fixture::new(kind);
        serial.register("a", &a, &[]);
        serial.select(&ra, &a, "accepted-project-value");
        serial.register("b", &b, std::slice::from_ref(&ra));
        serial.select(&rb, &b, "draft-project-value");
        assert_eq!(prepared.record().bytes(), serial.record);
        f.retain(prepared);
        assert_eq!(f.value()["authored"], original["authored"]);
        assert_eq!(f.value()["streams"][1], original["streams"][1]);
        f.store.calls.clear();
        let retry = sequence(&mut f, changes(), Limits::default()).unwrap();
        assert!(retry.is_noop() && retry.metadata().is_empty());
        assert_eq!(retry.record().bytes(), f.record);
        assert!(f.store.calls.values().all(|count| *count == 1));
    }
}

#[test]
fn history_sequence_failure_has_no_partial_candidate_or_store_writes() {
    let mut f = Fixture::new("document");
    let original = f.record.clone();
    let manifest = Fixture::manifest("a", &payload("A"), &[]);
    let missing = payload("unregistered revision");
    let subject = pin(&payload("B"));
    let desired = status("draft-project-value");
    let result = sequence(
        &mut f,
        vec![
            Change::RegisterRevision {
                manifest: &manifest,
                coverage: Coverage::Partial,
            },
            Change::SelectRevision {
                revision: &missing,
                representation: "main",
                subject: &subject,
                status: &desired,
                streams: &[],
            },
        ],
        Limits::default(),
    );
    assert!(result.is_err());
    assert_eq!(f.record, original);
    assert!(f.store.objects.is_empty() && f.store.calls.is_empty());
}

#[test]
fn history_sequence_aggregate_generated_budget_and_semantic_noop_are_enforced() {
    let a = Fixture::policy();
    let mut value: Value = serde_json::from_slice(&a).unwrap();
    value["targets"][0]["destination"] = serde_json::to_value(payload("destination-b")).unwrap();
    let b = raw(&value);
    value["targets"][0]["destination"] = serde_json::to_value(payload("destination-c")).unwrap();
    let c = raw(&value);
    let mut f = Fixture::new("document");
    for limits in [
        Limits {
            documents: 2,
            ..Limits::default()
        },
        Limits {
            bytes: a.len() + b.len(),
            ..Limits::default()
        },
    ] {
        assert!(matches!(
            sequence(
                &mut f,
                vec![
                    Change::ConfigureDelivery { policy: &a },
                    Change::ConfigureDelivery { policy: &b },
                    Change::ConfigureDelivery { policy: &c },
                ],
                limits
            ),
            Err(Error::Revision(revisions::Error::Budget))
        ));
    }
    assert!(f.store.objects.is_empty());
    let first = f.prepare(Change::ConfigureDelivery { policy: &a }).unwrap();
    f.retain(first);
    let original = f.record.clone();
    let result = sequence(
        &mut f,
        vec![
            Change::ConfigureDelivery { policy: &b },
            Change::ConfigureDelivery { policy: &a },
        ],
        Limits::default(),
    )
    .unwrap();
    assert!(result.is_noop() && result.metadata().is_empty());
    assert_eq!(result.record().bytes(), original);
}

#[test]
fn history_sequence_limits_and_stale_source_refuse_before_reader_io() {
    let mut f = Fixture::new("document");
    let policy = Fixture::policy();
    assert!(matches!(
        sequence(&mut f, vec![], Limits::default()),
        Err(Error::Budget)
    ));
    let excessive = (0..=edits::sequence::MAX_CHANGES)
        .map(|_| Change::ConfigureDelivery { policy: &policy })
        .collect();
    assert!(matches!(
        sequence(&mut f, excessive, Limits::default()),
        Err(Error::Budget)
    ));
    let expected = payload("different source");
    let result = edits::sequence::prepare(
        Inputs {
            expected: &expected,
            instance: &f.record,
            definition: &f.definition,
            authored_schema: &f.schema,
            dialect: Dialect::Draft202012,
            limits: Limits::default(),
        },
        vec![Change::ConfigureDelivery { policy: &policy }],
        &mut f.store,
    );
    assert!(matches!(result, Err(Error::Stale)));
    assert!(f.store.calls.is_empty() && f.store.objects.is_empty());
    let oversized = vec![b' '; 65_537];
    assert!(matches!(
        edits::sequence::prepare(
            Inputs {
                expected: &expected,
                instance: &oversized,
                definition: &f.definition,
                authored_schema: &f.schema,
                dialect: Dialect::Draft202012,
                limits: Limits::default(),
            },
            vec![Change::ConfigureDelivery { policy: &policy }],
            &mut f.store
        ),
        Err(Error::Budget)
    ));
    let boundary = (0..edits::sequence::MAX_CHANGES)
        .map(|_| Change::ConfigureDelivery { policy: &policy })
        .collect();
    let prepared = sequence(&mut f, boundary, Limits::default()).unwrap();
    assert_eq!(prepared.metadata().len(), 1);
    assert!(f.store.calls.is_empty() && f.store.objects.is_empty());
}
