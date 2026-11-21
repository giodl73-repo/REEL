use super::*;
use crate::metadata::{Limits as MetadataLimits, MetadataReader, MetadataStore};
use git_asset_core::tree::MemoryNodes;
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn hash(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
pub(in crate::ledger) fn fixture(
    root: &Path,
    clock: bool,
) -> (
    CapacityLedger,
    Vec<Receipt>,
    Receipt,
    AllocationTicket,
    ObservationTicket,
) {
    let mut l = if clock {
        CapacityLedger::create_clock(
            root,
            key("volume"),
            key("epoch"),
            10,
            crate::clock::Policy {
                authority: hash("authority"),
                epoch: hash("clock-epoch"),
                boundary: hash("boundary"),
                max_sample_millis: 1000,
                max_drift_millis: 1000,
                max_session_millis: 10000,
            },
        )
        .unwrap()
    } else {
        CapacityLedger::create(root, key("volume"), key("epoch"), 10).unwrap()
    };
    let mut receipts = Vec::new();
    if clock {
        receipts.push(l.observe_clock(key("clock-1"), 1000, true).unwrap());
    }
    let p = l.begin_probe(key("probe-1")).unwrap();
    receipts.push(p.clone());
    receipts.push(l.observe(key("observe-1"), p.reference(), 10000).unwrap());
    let job = l.reserve(key("job"), key("owner"), 2000).unwrap();
    receipts.push(job.clone());
    let a = l
        .begin_allocation(key("generic"), job.reference(), key("event"), 100)
        .unwrap();
    receipts.push(a.clone());
    receipts.push(
        l.complete_allocation(key("complete-generic"), a.reference(), 0)
            .unwrap(),
    );
    let zero = match l.outputs.get("generic").unwrap() {
        Output::Allocation(t) => t.clone(),
        _ => panic!(),
    };
    let binding = hash("binding");
    let publication = l
        .begin_publication(
            key("publication"),
            job.reference(),
            key("owner"),
            binding.clone(),
            100,
        )
        .unwrap();
    receipts.push(publication.clone());
    receipts.push(
        l.settle_footprint(
            key("published"),
            publication.reference(),
            binding.clone(),
            25,
        )
        .unwrap(),
    );
    let retention = l
        .begin_retention(
            key("retention"),
            job.reference(),
            key("owner"),
            binding.clone(),
            100,
        )
        .unwrap();
    receipts.push(retention.clone());
    receipts.push(
        l.settle_retention(key("retained"), retention.reference(), binding.clone(), 30)
            .unwrap(),
    );
    let handoff = l
        .begin_handoff(
            key("handoff"),
            job.reference(),
            key("owner"),
            binding.clone(),
            100,
        )
        .unwrap();
    receipts.push(handoff.clone());
    receipts.push(
        l.settle_handoff(key("handed-off"), handoff.reference(), binding.clone(), 35)
            .unwrap(),
    );
    let publication = l
        .begin_publication(
            key("cleanup-publication"),
            job.reference(),
            key("owner"),
            binding.clone(),
            100,
        )
        .unwrap();
    receipts.push(publication.clone());
    let reviewed = hash("reviewed");
    receipts.push(
        l.begin_cleanup(publication.reference(), binding.clone(), reviewed.clone())
            .unwrap(),
    );
    receipts.push(
        l.complete_cleanup(publication.reference(), binding, reviewed, 0)
            .unwrap(),
    );
    receipts.push(l.finish(key("finished"), job.reference()).unwrap());
    let retired = l.retire_closed(job.reference()).unwrap();
    receipts.push(retired.clone());
    receipts.push(l.invalidate(key("invalidated")).unwrap());
    let p = l.begin_probe(key("probe-2")).unwrap();
    receipts.push(p.clone());
    receipts.push(l.observe(key("observe-2"), p.reference(), 9000).unwrap());
    let live = l.reserve(key("live-job"), key("live-owner"), 500).unwrap();
    receipts.push(live.clone());
    receipts.push(
        l.begin_allocation(
            key("live-allocation"),
            live.reference(),
            key("inflight"),
            200,
        )
        .unwrap(),
    );
    if clock {
        receipts.push(l.observe_clock(key("clock-2"), 2000, true).unwrap());
    }
    let p = l.begin_probe(key("pending-probe")).unwrap();
    receipts.push(p);
    let probe = match l.outputs.get("pending-probe").unwrap() {
        Output::Probe(t) => t.clone(),
        _ => panic!(),
    };
    (l, receipts, retired, zero, probe)
}
pub(in crate::ledger) fn assert_receipt(actual: &Receipt, original: &Receipt) {
    assert!(actual.was_replayed());
    assert_eq!(actual.reference(), original.reference());
    assert_eq!(actual.checkpoint(), original.checkpoint());
    assert_eq!(
        format!("{:?}", actual.response()),
        format!("{:?}", original.response())
    );
}
#[test]
fn every_original_output_survives_physical_metadata_reopen_without_fresh_probe_or_source_commands()
{
    for clock in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("ledger");
        let (mut l, receipts, retired, zero, probe) = fixture(&root, clock);
        let before = l.checkpoint();
        let summary = l.accounting().unwrap();
        let meta = temp.path().join("metadata");
        let mut nodes = MetadataStore::create(
            &meta,
            MetadataLimits {
                nodes: 262144,
                bytes: 64 * 1024 * 1024,
                database_bytes: 96 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let export = l.export_history(&mut nodes, Limits::default()).unwrap();
        assert!(export.metrics().calls > receipts.len());
        assert_eq!(export.snapshot().journal(), &before);
        assert_eq!(
            export.snapshot().clock_high_water(),
            if clock { Some(2000) } else { None }
        );
        assert_eq!(l.checkpoint(), before);
        assert_eq!(l.accounting().unwrap(), summary);
        let encoded = export.snapshot().to_json().unwrap();
        assert!(encoded.len() < 2048);
        let snapshot = Snapshot::from_json(&encoded).unwrap();
        let minimum = nodes.finish().unwrap();
        drop(nodes);
        drop(l);
        let mut reader = MetadataReader::open(&meta, &minimum).unwrap();
        snapshot
            .validate_all(&mut reader, Limits::default())
            .unwrap();
        let mut kinds = std::collections::BTreeSet::new();
        for original in &receipts {
            let inspection = snapshot
                .inspect(
                    &mut reader,
                    &original.reference.operation,
                    Limits::default(),
                )
                .unwrap();
            assert_receipt(inspection.receipt(), original);
            inspection
                .check_request(inspection.original_request())
                .unwrap();
            let data: serde_json::Value =
                serde_json::from_slice(&inspection.dependency_json().unwrap()).unwrap();
            kinds.insert(data["kind"].as_str().unwrap().to_owned());
            assert!(inspection.metrics().calls <= 256 && inspection.metrics().bytes <= 1024 * 1024);
        }
        let mut expected: std::collections::BTreeSet<String> = [
            "retired",
            "handoff",
            "handoff_settled",
            "retention",
            "retention_settled",
            "cleanup",
            "cleaned",
            "publication",
            "footprint",
            "reservation",
            "allocation",
            "sequence",
            "probe",
            "summary",
            "finished",
            "invalidated",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        if clock {
            expected.insert("clock".into());
        }
        assert_eq!(kinds, expected);
        let (outcome, metrics) = snapshot
            .retired_outcome(
                &mut reader,
                &retired.reference.operation,
                &zero,
                Limits::default(),
            )
            .unwrap();
        assert_eq!(
            outcome,
            core_history::Outcome {
                charge: 0,
                sequence: 1
            }
        );
        assert!(metrics.calls <= 512 && metrics.bytes <= 4 * 1024 * 1024);
        let mut model = snapshot
            .working_state()
            .read(&mut reader, state::Limits::default())
            .unwrap()
            .into_domain();
        assert_eq!(
            model.summary(),
            Err(capacity::CapacityError::UnknownObservation)
        );
        assert_eq!(
            model.observe(probe, 10000),
            Err(capacity::CapacityError::Stale)
        );
        let live = model.reserve("live-job", "live-owner", 500).unwrap();
        let pending = model.begin_allocation(&live, "inflight", 200).unwrap();
        assert_eq!(model.allocation_status(&pending).unwrap(), None);
        assert_eq!(model.finish(&live), Err(capacity::CapacityError::InFlight));
        let mut reopened = CapacityLedger::open(&root, &before).unwrap();
        let receipt = reopened
            .complete_allocation(
                key("complete-generic"),
                &JournalRef {
                    genesis: before.genesis.clone(),
                    operation: key("generic"),
                },
                0,
            )
            .unwrap();
        assert_receipt(
            &receipt,
            receipts
                .iter()
                .find(|r| r.reference.operation == key("complete-generic"))
                .unwrap(),
        );
        assert_eq!(reopened.checkpoint(), before);
        assert!(matches!(
            reopened.reserve(key("new-job"), key("owner"), 1),
            Err(LedgerError::NeedsProbe)
        ));
    }
}
struct PeerNodes {
    nodes: MemoryNodes,
    peer: CapacityLedger,
    advanced: bool,
}
impl NodeStore for PeerNodes {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.nodes.get(o)
    }
    fn put(&mut self, b: &[u8]) -> TreeResult<Object> {
        if !self.advanced {
            self.peer
                .invalidate(key("peer-progress"))
                .map_err(|e| TreeError::Store(e.to_string()))?;
            self.advanced = true;
        }
        self.nodes.put(b)
    }
}
#[test]
fn sql_cursor_is_closed_before_node_io_and_peer_progress_refuses_stale_export() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("ledger");
    let (mut l, _, _, _, _) = fixture(&root, false);
    let before = l.checkpoint();
    let peer = CapacityLedger::open(&root, &before).unwrap();
    let mut nodes = PeerNodes {
        nodes: MemoryNodes::default(),
        peer,
        advanced: false,
    };
    assert!(matches!(
        l.export_history(&mut nodes, Limits::default()),
        Err(Error::Journal(LedgerError::Conflict))
    ));
    assert!(nodes.advanced);
    assert_eq!(l.checkpoint().sequence(), before.sequence() + 1);
    let current = l.checkpoint();
    let export = l.export_history(&mut nodes, Limits::default()).unwrap();
    assert_eq!(export.snapshot().journal(), &current);
    assert_eq!(l.checkpoint(), current);
}
#[test]
fn request_conflict_shared_budget_and_bad_descriptor_cannot_change_the_source() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("ledger");
    let (mut l, receipts, _, _, _) = fixture(&root, false);
    let before = l.checkpoint();
    let summary = l.accounting().unwrap();
    let mut nodes = MemoryNodes::default();
    assert!(
        l.export_history(
            &mut nodes,
            Limits {
                max_calls: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert_eq!(l.checkpoint(), before);
    assert_eq!(l.accounting().unwrap(), summary);
    let export = l.export_history(&mut nodes, Limits::default()).unwrap();
    let snapshot = export.snapshot();
    let op = &receipts[0].reference.operation;
    let inspection = snapshot.inspect(&mut nodes, op, Limits::default()).unwrap();
    let mut request: serde_json::Value =
        serde_json::from_slice(inspection.original_request()).unwrap();
    request["operation"] = "changed".into();
    let raw = identity::canonical_json(&request).unwrap();
    assert!(matches!(
        inspection.check_request(&raw),
        Err(Error::Journal(LedgerError::Conflict))
    ));
    assert!(
        snapshot
            .inspect(
                &mut nodes,
                op,
                Limits {
                    max_calls: 1,
                    ..Limits::default()
                }
            )
            .is_err()
    );
    let encoded = snapshot.to_json().unwrap();
    assert!(Snapshot::from_json(&[encoded.clone(), b" ".to_vec()].concat()).is_err());
    let mut value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    value["extra"] = true.into();
    assert!(Snapshot::from_json(&identity::canonical_json(&value).unwrap()).is_err());
    let mut value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    value["operation_count"] = 0.into();
    assert!(Snapshot::from_json(&identity::canonical_json(&value).unwrap()).is_err());
    assert_eq!(l.checkpoint(), before);
    assert_eq!(l.accounting().unwrap(), summary);
}

fn repin(
    snapshot: &Snapshot,
    nodes: &mut MemoryNodes,
    operation: &Key,
    mutate: impl FnOnce(&mut serde_json::Value),
    rebind: bool,
) -> Snapshot {
    let p = operation_path(operation).unwrap();
    let (o, _) = Tree::from_root(snapshot.operations.clone())
        .get(&p, nodes, Limits::default().tree())
        .unwrap();
    let raw = nodes.get(&o.unwrap()).unwrap().unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    mutate(&mut value);
    if rebind {
        let request: Request = serde_json::from_value(value["request"].clone()).unwrap();
        let previous = Digest::parse(value["previous"].as_str().unwrap()).unwrap();
        let sequence = value["sequence"].as_u64().unwrap();
        value["hash"] = event_hash(sequence, &previous, &request)
            .unwrap()
            .as_str()
            .into();
    }
    let o = nodes
        .put(&identity::canonical_json(&value).unwrap())
        .unwrap();
    let mut changes = BTreeMap::new();
    changes.insert(p, Some(o));
    let (tree, _) = Tree::from_root(snapshot.operations.clone())
        .edit(
            &snapshot.operations,
            &changes,
            nodes,
            Limits::default().tree(),
        )
        .unwrap();
    let mut changed = snapshot.clone();
    changed.operations = tree.root().clone();
    changed
}
#[test]
fn repinned_local_and_cross_dependency_mutations_refuse_after_event_hash_rebinding() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("ledger");
    let (mut l, _, _, _, _) = fixture(&root, true);
    let mut nodes = MemoryNodes::default();
    let snapshot = l
        .export_history(&mut nodes, Limits::default())
        .unwrap()
        .snapshot;
    for case in 0..18 {
        let (op, query, rebind) = match case {
            0..=5 => ("publication", "publication", false),
            6 => ("published", "published", true),
            7 => ("published", "published", true),
            8 => ("job", "publication", false),
            9 => ("publication", "publication", true),
            10 => ("generic", "generic", false),
            11 => ("published", "published", false),
            12 => ("clock-1", "clock-1", false),
            13 => ("observe-1", "observe-1", false),
            14 => ("pending-probe", "pending-probe", false),
            15 => ("retire-job", "retire-job", false),
            16 => ("handed-off", "handed-off", true),
            17 => ("retained", "retained", true),
            _ => unreachable!(),
        };
        // Retirement IDs use the existing internal function, not a new public recipe.
        let op = if op == "retire-job" {
            retirement_key(&key("job")).unwrap()
        } else {
            key(op)
        };
        let query = if query == "retire-job" {
            op.clone()
        } else {
            key(query)
        };
        let changed = repin(
            &snapshot,
            &mut nodes,
            &op,
            |v| match case {
                0 => v["output"]["ticket"]["reservation"]["generation"] = 2.into(),
                1 => v["output"]["ticket"]["reservation"]["owner"] = "other".into(),
                2 => v["output"]["binding"] = hash("other-binding").as_str().into(),
                3 => v["output"]["ticket"]["limit"] = 101.into(),
                4 => v["request"]["operation"] = "other-operation".into(),
                5 => v["extra"] = true.into(),
                6 => v["request"]["command"]["binding"] = hash("other-binding").as_str().into(),
                7 => {
                    v["request"]["command"]["footprint"] = 101.into();
                    v["output"]["bytes"] = 101.into();
                }
                8 => v["output"]["token"]["generation"] = 2.into(),
                9 => {
                    v["request"]["command"]["owner"] = "other".into();
                    v["output"]["ticket"]["reservation"]["owner"] = "other".into();
                }
                10 => v["output"]["ticket"]["event"] = "other".into(),
                11 => v["output"]["kind"] = "cleaned".into(),
                12 => v["output"]["millis"] = 999.into(),
                13 => v["output"]["available"] = 1.into(),
                14 => v["output"]["evidence"]["nonce"] = 0.into(),
                15 => v["output"]["archive"]["schema"] = "future".into(),
                16 => v["request"]["command"]["binding"] = hash("other-binding").as_str().into(),
                17 => v["request"]["command"]["binding"] = hash("other-binding").as_str().into(),
                _ => unreachable!(),
            },
            rebind,
        );
        assert!(
            changed
                .inspect(&mut nodes, &query, Limits::default())
                .is_err(),
            "case {case}"
        );
    }
    assert!(
        snapshot
            .inspect(&mut nodes, &key("publication"), Limits::default())
            .is_ok()
    );
}

struct Count<'a> {
    nodes: &'a MemoryNodes,
    reads: Cell<usize>,
}
impl NodeStore for Count<'_> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.reads.set(self.reads.get() + 1);
        self.nodes.get(o)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Err(TreeError::Store("readonly counting port".into()))
    }
}
#[test]
fn maximum_actual_segment_keeps_live_work_and_bounded_point_queries_without_a_cap_waiver() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("ledger");
    let (mut l, receipts, _, _, _) = fixture(&root, true);
    let mut n = 0;
    while l.checkpoint().sequence() < MAX_COMMANDS {
        l.invalidate(key(&format!("{n:0128}"))).unwrap();
        n += 1;
    }
    let before = l.checkpoint();
    assert!(matches!(
        l.invalidate(key("beyond-original-cap")),
        Err(LedgerError::Budget)
    ));
    let mut nodes = MemoryNodes::default();
    let export = l.export_history(&mut nodes, Limits::default()).unwrap();
    assert_eq!(export.snapshot().operation_count, 16384);
    assert_eq!(l.checkpoint(), before);
    assert!(export.metrics().calls <= MAX_CALLS && export.metrics().bytes <= MAX_BYTES);
    export
        .snapshot()
        .validate_all(&mut nodes, Limits::default())
        .unwrap();
    for original in &receipts {
        let mut counter = Count {
            nodes: &nodes,
            reads: Cell::new(0),
        };
        let inspection = export
            .snapshot()
            .inspect(
                &mut counter,
                &original.reference.operation,
                Limits::default(),
            )
            .unwrap();
        assert_receipt(inspection.receipt(), original);
        assert_eq!(counter.reads.get(), inspection.metrics().calls);
        assert!(counter.reads.get() <= 256);
    }
    let mut counter = Count {
        nodes: &nodes,
        reads: Cell::new(0),
    };
    assert!(
        export
            .snapshot()
            .inspect(
                &mut counter,
                &key("job"),
                Limits {
                    max_calls: 1,
                    ..Limits::default()
                }
            )
            .is_err()
    );
    assert_eq!(counter.reads.get(), 1);
    let mut counter = Count {
        nodes: &nodes,
        reads: Cell::new(0),
    };
    assert!(
        export
            .snapshot()
            .inspect(
                &mut counter,
                &key("job"),
                Limits {
                    max_bytes: export.snapshot().operations.bytes as usize - 1,
                    ..Limits::default()
                }
            )
            .is_err()
    );
    assert_eq!(counter.reads.get(), 0);
    let mut model = export
        .snapshot()
        .working_state()
        .read(&mut nodes, state::Limits::default())
        .unwrap()
        .into_domain();
    let token = model.reserve("live-job", "live-owner", 500).unwrap();
    assert_eq!(model.finish(&token), Err(capacity::CapacityError::InFlight));
    for (_, raw) in nodes.objects() {
        assert!(raw.len() <= 65536);
    }
}
struct Corrupt;
impl NodeStore for Corrupt {
    fn get(&self, _: &Object) -> TreeResult<Option<Vec<u8>>> {
        Ok(Some(b"changed".to_vec()))
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Ok(Object::from_bytes(b"wrong acknowledgement"))
    }
}
#[test]
fn missing_corrupt_and_wrong_put_ports_refuse_and_full_validation_checks_descriptor_aggregates() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("ledger");
    let (mut l, _, _, _, _) = fixture(&root, true);
    let before = l.checkpoint();
    assert!(l.export_history(&mut Corrupt, Limits::default()).is_err());
    assert_eq!(l.checkpoint(), before);
    let mut nodes = MemoryNodes::default();
    let snapshot = l
        .export_history(&mut nodes, Limits::default())
        .unwrap()
        .snapshot;
    assert!(matches!(
        snapshot.inspect(&mut Corrupt, &key("job"), Limits::default()),
        Err(Error::Integrity)
    ));
    assert!(
        snapshot
            .inspect(&mut MemoryNodes::default(), &key("job"), Limits::default())
            .is_err()
    );
    let mut changed = snapshot.clone();
    changed.encoded_request_bytes += 1;
    assert!(changed.validate_all(&mut nodes, Limits::default()).is_err());
    let mut changed = snapshot.clone();
    changed.clock_high = Some(1999);
    assert!(changed.validate_all(&mut nodes, Limits::default()).is_err());
    assert!(
        snapshot
            .validate_all(
                &mut nodes,
                Limits {
                    max_calls: 1,
                    ..Limits::default()
                }
            )
            .is_err()
    );
    assert!(snapshot.validate_all(&mut nodes, Limits::default()).is_ok());
    assert_eq!(l.checkpoint(), before);
}

#[test]
fn independently_pinned_original_operation_literal_preserves_existing_event_hash_and_response() {
    let raw=br#"{"hash":"c7a8fe3485eaadff1c028e84485ddd6fb45bdf79e331b24c52fa3b5301df5c28","output":{"kind":"invalidated"},"previous":"0000000000000000000000000000000000000000000000000000000000000000","request":{"command":{"kind":"invalidate"},"operation":"invalidate"},"schema":"tig.capacity-history-op.design-1","sequence":1}"#;
    let item: Item = decode(raw).unwrap();
    assert_eq!(encode(&item).unwrap(), raw);
    assert_eq!(
        Object::from_bytes(raw).sha256.as_str(),
        "5fb2d6437b32f96f7cd7f21a30038c78cbebabdde12a2c4fc20d6f7bf594b7c1"
    );
    assert_eq!(
        event_hash(item.sequence, &item.previous, &item.request)
            .unwrap()
            .as_str(),
        "c7a8fe3485eaadff1c028e84485ddd6fb45bdf79e331b24c52fa3b5301df5c28"
    );
    assert!(matches!(item.output.response(), Response::Invalidated));
}
#[test]
fn cleanup_review_wrong_dependency_kind_and_cycles_refuse_without_lifetime_replay() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("ledger");
    let (mut l, receipts, _, _, _) = fixture(&root, false);
    let mut nodes = MemoryNodes::default();
    let snapshot = l
        .export_history(&mut nodes, Limits::default())
        .unwrap()
        .snapshot;
    let completed = receipts
        .iter()
        .find(|r| matches!(r.response(), Response::CleanupCompleted(_)))
        .unwrap()
        .reference
        .operation
        .clone();
    let cleanup = cleanup_key(&key("cleanup-publication")).unwrap();
    for case in 0..5 {
        let op = if case < 2 {
            completed.clone()
        } else if case == 2 {
            cleanup.clone()
        } else {
            key("generic")
        };
        let query = if case < 3 {
            completed.clone()
        } else {
            key("generic")
        };
        let changed = repin(
            &snapshot,
            &mut nodes,
            &op,
            |v| match case {
                0 => v["request"]["command"]["reviewed"] = hash("other-review").as_str().into(),
                1 => v["request"]["command"]["binding"] = hash("other-binding").as_str().into(),
                2 => {
                    v["request"]["command"]["reviewed"] = hash("other-review").as_str().into();
                    v["output"]["reviewed"] = hash("other-review").as_str().into();
                }
                3 | 4 => {
                    let id = if case == 3 { "probe-1" } else { "generic" };
                    v["request"]["command"]["reservation"] = id.into();
                    v["output"]["ticket"]["reservation"]["id"] = id.into();
                }
                _ => unreachable!(),
            },
            true,
        );
        assert!(
            changed
                .inspect(&mut nodes, &query, Limits::default())
                .is_err(),
            "case {case}"
        );
    }
    assert!(
        snapshot
            .inspect(&mut nodes, &completed, Limits::default())
            .is_ok()
    );
}
