use super::super::tests::{assert_receipt, fixture};
use super::*;
use crate::metadata::{Limits as MetadataLimits, MetadataReader, MetadataStore};
use git_asset_core::tree::MemoryNodes;

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}

fn capture_delta(l: &CapacityLedger, base: u64, nodes: &mut impl NodeStore) -> Vec<Item> {
    // All SQL rows copied and cursor dropped before metadata IO.
    let rows: Vec<(u64, Vec<u8>, String, String)> = {
        let mut statement = l.conn.prepare("SELECT sequence,body,previous,hash FROM events WHERE sequence>?1 ORDER BY sequence").unwrap();
        statement
            .query_map([sql_number(base).unwrap()], |r| {
                Ok((number(r, 0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    rows.into_iter()
        .map(|(sequence, raw, previous, hash)| {
            let request: Request = decode(&raw).unwrap();
            let output =
                ArchivedOutput::capture(l.outputs.get(request.operation.as_str()).unwrap(), nodes)
                    .unwrap();
            Item {
                schema: ITEM_SCHEMA.into(),
                sequence,
                previous: Digest::parse(&previous).unwrap(),
                hash: Digest::parse(&hash).unwrap(),
                request,
                output,
            }
        })
        .collect()
}

fn synthetic_invalidations(head: &Checkpoint, count: usize) -> (Vec<Item>, Checkpoint) {
    let mut previous = head.hash.clone();
    let mut delta = Vec::new();
    for i in 0..count {
        let sequence = head.sequence + i as u64 + 1;
        let request = Request {
            operation: key(&format!("continued-{sequence}")),
            command: Command::Invalidate,
        };
        let hash = event_hash(sequence, &previous, &request).unwrap();
        delta.push(Item {
            schema: ITEM_SCHEMA.into(),
            sequence,
            previous,
            hash: hash.clone(),
            request,
            output: ArchivedOutput::Invalidated,
        });
        previous = hash;
    }
    let end = Checkpoint {
        genesis: head.genesis.clone(),
        sequence: head.sequence + count as u64,
        hash: previous,
    };
    (delta, end)
}

#[test]
fn independent_continued_item_vector_preserves_event_hash_and_legacy_range() {
    // Composed separately using ASCII JSON/Python sorted keys and hashlib.
    let raw = br#"{"hash":"d1d3a36dffa2c8c958d7ca41d2f844adebe7fdd2a4d122a65c1054bac7e17667","output":{"kind":"invalidated"},"previous":"0000000000000000000000000000000000000000000000000000000000000000","request":{"command":{"kind":"invalidate"},"operation":"continued-17001"},"schema":"tig.capacity-successor-op.design-1","sequence":17001}"#;
    assert_eq!(
        Digest::blob(raw).as_str(),
        "ae4e4654868c29418b73a8523b359037f6fe6e1658a7b63fb89063876be4addf"
    );
    let mut item: Item = decode(raw).unwrap();
    assert_eq!(encode(&item).unwrap(), raw);
    assert_eq!(
        event_hash(item.sequence, &item.previous, &item.request).unwrap(),
        item.hash
    );
    assert_eq!(
        sequence_path(17001).unwrap(),
        LogicalPath::parse("seq/00000000000000017001").unwrap()
    );
    let (_temp, _l, mut nodes, mut index) = memory_fixture();
    index.head.sequence = item.sequence;
    index.head.hash = item.hash.clone();
    let o = nodes.put(raw).unwrap();
    index.record(&o, item.sequence, &mut nodes).unwrap();
    // Framing only: no admission or global index is manufactured by this test.
    item.schema = "tig.capacity-history-op.design-1".into();
    let o = nodes.put(&encode(&item).unwrap()).unwrap();
    assert!(index.record(&o, item.sequence, &mut nodes).is_err());
}

struct ReadCount<'a, S> {
    nodes: &'a S,
    calls: Cell<usize>,
}
impl<S: NodeStore> NodeStore for ReadCount<'_, S> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.calls.set(self.calls.get() + 1);
        self.nodes.get(o)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        panic!("historical lookup must not write")
    }
}

#[test]
fn physical_indexes_preserve_all_old_receipts_and_real_cross_boundary_dependencies() {
    for clock in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (mut l, mut receipts, retired, zero, _) = fixture(&temp.path().join("ledger"), clock);
        let old_head = l.checkpoint();
        let root = temp.path().join("metadata");
        let mut nodes = MetadataStore::create(
            &root,
            MetadataLimits {
                nodes: 262144,
                bytes: 64 * 1024 * 1024,
                database_bytes: 96 * 1024 * 1024,
                min_free_bytes: 65536,
            },
        )
        .unwrap();
        let exported = l.export_history(&mut nodes, Limits::default()).unwrap();
        let (base, _) =
            Index::from_legacy(exported.snapshot(), &mut nodes, build_limits()).unwrap();
        let allocation = JournalRef {
            genesis: old_head.genesis.clone(),
            operation: key("live-allocation"),
        };
        let completed = l
            .complete_allocation(key("continued-complete"), &allocation, 50)
            .unwrap();
        let completion_sequence = match completed.response() {
            Response::AllocationCompleted(n) => *n,
            _ => panic!(),
        };
        receipts.push(completed);
        let reservation = JournalRef {
            genesis: old_head.genesis.clone(),
            operation: key("live-job"),
        };
        receipts.push(l.finish(key("continued-finish"), &reservation).unwrap());
        let new_retired = l.retire_closed(&reservation).unwrap();
        receipts.push(new_retired.clone());
        let delta = capture_delta(&l, old_head.sequence, &mut nodes);
        let current = l.checkpoint();
        let (next, _) = base
            .extend(&delta, &current, &mut nodes, build_limits())
            .unwrap();
        let checkpoint = nodes.finish().unwrap();
        drop(nodes);
        let reader = MetadataReader::open(&root, &checkpoint).unwrap();
        let mut counted = ReadCount {
            nodes: &reader,
            calls: Cell::new(0),
        };
        for original in &receipts {
            counted.calls.set(0);
            let inspected = next
                .inspect(
                    &original.reference.operation,
                    &mut counted,
                    Limits::default(),
                )
                .unwrap()
                .unwrap();
            assert_receipt(inspected.receipt(), original);
            assert_eq!(counted.calls.get(), inspected.metrics.calls);
            assert!(counted.calls.get() <= 1024);
            assert_eq!(
                next.prefix(
                    original.checkpoint.sequence,
                    &mut counted,
                    Limits::default()
                )
                .unwrap()
                .0,
                *original.checkpoint()
            );
            inspected
                .check_request(inspected.original_request())
                .unwrap();
            let mut different: Request = decode(inspected.original_request()).unwrap();
            different.operation = key("substituted");
            assert!(matches!(
                inspected.check_request(&encode(&different).unwrap()),
                Err(Error::Journal(LedgerError::Conflict))
            ));
        }
        assert_eq!(base.head, old_head);
        assert_eq!(next.head, current);
        assert_eq!(
            next.prefix(old_head.sequence, &mut counted, Limits::default())
                .unwrap()
                .0,
            old_head
        );
        let outcome = next
            .retired_outcome(
                &retired.reference.operation,
                &zero,
                &mut counted,
                Limits::default(),
            )
            .unwrap()
            .0;
        assert_eq!(outcome.charge, 0);
        let ticket = match l.outputs.get("live-allocation").unwrap() {
            Output::Allocation(t) => t,
            _ => panic!(),
        };
        assert_eq!(
            next.retired_outcome(
                &new_retired.reference.operation,
                ticket,
                &mut counted,
                Limits::default()
            )
            .unwrap()
            .0,
            core_history::Outcome {
                charge: 50,
                sequence: completion_sequence
            }
        );
        assert!(
            next.inspect(&key("unknown-operation"), &mut counted, Limits::default())
                .unwrap()
                .is_none()
        );
        assert_eq!(
            next.prefix(0, &mut counted, Limits::default())
                .unwrap()
                .0
                .hash,
            old_head.genesis
        );
        assert!(
            next.prefix(current.sequence + 1, &mut counted, Limits::default())
                .is_err()
        );
    }
}

fn memory_fixture() -> (tempfile::TempDir, CapacityLedger, MemoryNodes, Index) {
    let temp = tempfile::tempdir().unwrap();
    let (mut l, _, _, _, _) = fixture(&temp.path().join("ledger"), false);
    let mut nodes = MemoryNodes::default();
    let exported = l.export_history(&mut nodes, Limits::default()).unwrap();
    let (index, _) = Index::from_legacy(exported.snapshot(), &mut nodes, build_limits()).unwrap();
    (temp, l, nodes, index)
}

#[test]
fn point_budgets_missing_nodes_and_cross_index_rebinding_never_invent_absence() {
    let (_temp, _l, mut nodes, index) = memory_fixture();
    let mut counted = ReadCount {
        nodes: &nodes,
        calls: Cell::new(0),
    };
    assert!(
        index
            .inspect(
                &key("job"),
                &mut counted,
                Limits {
                    max_calls: 1,
                    ..Limits::default()
                }
            )
            .is_err()
    );
    assert_eq!(counted.calls.get(), 1);
    counted.calls.set(0);
    assert!(
        index
            .inspect(
                &key("job"),
                &mut counted,
                Limits {
                    max_bytes: index.operations.bytes as usize - 1,
                    ..Limits::default()
                }
            )
            .is_err()
    );
    assert_eq!(counted.calls.get(), 0);
    let job = index
        .inspect(&key("job"), &mut counted, Limits::default())
        .unwrap()
        .unwrap();
    let wrong = index
        .at(
            &index.operations,
            &operation_path(&key("probe-1")).unwrap(),
            &mut nodes,
            Limits::default(),
        )
        .unwrap()
        .unwrap();
    let (tree, _) = Tree::from_root(index.sequences.clone())
        .edit(
            &index.sequences,
            &BTreeMap::from([(
                sequence_path(job.receipt.checkpoint.sequence).unwrap(),
                Some(wrong),
            )]),
            &mut nodes,
            TreeLimits::default(),
        )
        .unwrap();
    let rebound = Index {
        sequences: tree.root().clone(),
        ..index.clone()
    };
    assert!(
        rebound
            .inspect(&key("job"), &mut nodes, Limits::default())
            .is_err()
    );
    let mut missing = index.clone();
    missing.operations = Object::from_bytes(b"absent root");
    assert!(matches!(
        missing.inspect(&key("unknown-operation"), &mut nodes, Limits::default()),
        Err(Error::Missing(_))
    ));
    assert!(
        index
            .inspect(&key("job"), &mut nodes, Limits::default())
            .unwrap()
            .is_some()
    );
}

#[test]
fn malformed_deltas_conflicting_retries_and_wrong_typed_dependencies_refuse() {
    let (_temp, _l, mut nodes, index) = memory_fixture();
    let (valid, end) = synthetic_invalidations(&index.head, 2);
    for mutation in 0..6 {
        let mut rows = valid.clone();
        match mutation {
            0 => rows[0].sequence += 1,
            1 => rows[0].previous = Digest::blob(b"wrong previous"),
            2 => rows[0].hash = Digest::blob(b"wrong event"),
            3 => rows[0].schema = "tig.capacity-history-op.design-1".into(),
            4 => rows[0].output = ArchivedOutput::Finished,
            5 => {
                rows[0].request.operation = key("job");
                rows[0].hash =
                    event_hash(rows[0].sequence, &rows[0].previous, &rows[0].request).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            index
                .extend(&rows, &end, &mut nodes, build_limits())
                .is_err()
        );
    }
    let request = Request {
        operation: key("bad-kind"),
        command: Command::CompleteAllocation {
            allocation: key("job"),
            actual: 0,
        },
    };
    let sequence = index.head.sequence + 1;
    let hash = event_hash(sequence, &index.head.hash, &request).unwrap();
    let row = Item {
        schema: ITEM_SCHEMA.into(),
        sequence,
        previous: index.head.hash.clone(),
        hash: hash.clone(),
        request,
        output: ArchivedOutput::Sequence { sequence: 1 },
    };
    let end = Checkpoint {
        genesis: index.head.genesis.clone(),
        sequence,
        hash,
    };
    assert!(
        index
            .extend(&[row], &end, &mut nodes, build_limits())
            .is_err()
    );
    assert!(
        index
            .inspect(&key("job"), &mut nodes, Limits::default())
            .unwrap()
            .is_some()
    );
}

#[test]
fn conditional_global_insert_cannot_rebind_an_old_operation_even_with_the_same_command() {
    let (_temp, _l, mut nodes, index) = memory_fixture();
    for name in ["invalidated", "job"] {
        let original = index
            .inspect(&key(name), &mut nodes, Limits::default())
            .unwrap()
            .unwrap();
        let (mut rows, mut end) = synthetic_invalidations(&index.head, 1);
        rows[0].request.operation = key(name);
        rows[0].hash = event_hash(rows[0].sequence, &rows[0].previous, &rows[0].request).unwrap();
        end.hash = rows[0].hash.clone();
        assert!(matches!(
            index.extend(&rows, &end, &mut nodes, build_limits()),
            Err(Error::Journal(LedgerError::Conflict))
        ));
        let unchanged = index
            .inspect(&key(name), &mut nodes, Limits::default())
            .unwrap()
            .unwrap();
        assert_eq!(unchanged.receipt.checkpoint, original.receipt.checkpoint);
        assert_eq!(unchanged.request, original.request);
        assert!(unchanged.receipt.was_replayed());
    }
}

#[test]
fn combined_staging_work_cannot_reset_its_budget_between_trees() {
    let (_temp, _l, mut nodes, index) = memory_fixture();
    let (delta, end) = synthetic_invalidations(&index.head, 48);
    let (next, used) = index
        .extend(&delta, &end, &mut nodes, build_limits())
        .unwrap();
    assert!(used.reads > 0 && used.writes > 96);
    for limits in [
        TreeLimits {
            max_reads: used.reads - 1,
            ..build_limits()
        },
        TreeLimits {
            max_writes: used.writes - 1,
            ..build_limits()
        },
        TreeLimits {
            max_read_bytes: used.read_bytes - 1,
            ..build_limits()
        },
        TreeLimits {
            max_write_bytes: used.write_bytes - 1,
            ..build_limits()
        },
    ] {
        assert!(index.extend(&delta, &end, &mut nodes, limits).is_err());
    }
    let exact = TreeLimits {
        max_reads: used.reads,
        max_writes: used.writes,
        max_read_bytes: used.read_bytes,
        max_write_bytes: used.write_bytes,
        ..build_limits()
    };
    let (retried, again) = index.extend(&delta, &end, &mut nodes, exact).unwrap();
    assert_eq!(retried.operations, next.operations);
    assert_eq!(retried.sequences, next.sequences);
    assert_eq!(used, again);
    assert!(
        index
            .extend(
                &delta,
                &end,
                &mut nodes,
                TreeLimits {
                    max_reads: 2_000_001,
                    ..build_limits()
                }
            )
            .is_err()
    );
}

#[test]
fn original_proof_memory_and_provider_work_share_exact_nonresetting_limits() {
    let (_temp, mut l, mut nodes, _) = memory_fixture();
    let exported = l.export_history(&mut nodes, Limits::default()).unwrap();
    let (index, used) =
        Index::from_legacy(exported.snapshot(), &mut nodes, build_limits()).unwrap();
    let exact = TreeLimits {
        max_reads: used.reads,
        max_writes: used.writes,
        max_read_bytes: used.read_bytes,
        max_write_bytes: used.write_bytes,
        ..build_limits()
    };
    let (again, work) = Index::from_legacy(exported.snapshot(), &mut nodes, exact).unwrap();
    assert_eq!(again.operations, index.operations);
    assert_eq!(again.sequences, index.sequences);
    assert_eq!(work, used);
    for limits in [
        TreeLimits {
            max_reads: used.reads - 1,
            ..exact
        },
        TreeLimits {
            max_writes: used.writes - 1,
            ..exact
        },
        TreeLimits {
            max_read_bytes: used.read_bytes - 1,
            ..exact
        },
        TreeLimits {
            max_write_bytes: used.write_bytes - 1,
            ..exact
        },
    ] {
        assert!(Index::from_legacy(exported.snapshot(), &mut nodes, limits).is_err());
    }
}

struct FaultPort {
    nodes: MemoryNodes,
    corrupt: bool,
}
impl NodeStore for FaultPort {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        if self.corrupt {
            Ok(Some(b"corrupted metadata".to_vec()))
        } else {
            self.nodes.get(o)
        }
    }
    fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
        let expected = self.nodes.put(bytes)?;
        Ok(if self.corrupt {
            expected
        } else {
            Object::from_bytes(b"wrong provider acknowledgement")
        })
    }
}

#[test]
fn corrupt_provider_and_wrong_acknowledgement_refuse_without_an_adopted_root() {
    let (_temp, _l, nodes, index) = memory_fixture();
    let (delta, end) = synthetic_invalidations(&index.head, 2);
    for corrupt in [true, false] {
        let mut copied = MemoryNodes::default();
        for (_, raw) in nodes.objects() {
            copied.put(raw).unwrap();
        }
        let mut fault = FaultPort {
            nodes: copied,
            corrupt,
        };
        assert!(matches!(
            index.extend(&delta, &end, &mut fault, build_limits()),
            Err(Error::Integrity)
        ));
    }
    let mut readable = ReadCount {
        nodes: &nodes,
        calls: Cell::new(0),
    };
    assert!(
        index
            .inspect(&key("job"), &mut readable, Limits::default())
            .unwrap()
            .is_some()
    );
}

#[test]
fn full_original_and_full_index_delta_cross_the_old_lifetime_cap_with_bounded_reads() {
    let temp = tempfile::tempdir().unwrap();
    let (mut l, receipts, _, _, _) = fixture(&temp.path().join("ledger"), true);
    while l.checkpoint.sequence < MAX_COMMANDS {
        l.invalidate(key(&format!("legacy-{}", l.checkpoint.sequence)))
            .unwrap();
    }
    assert!(matches!(
        l.invalidate(key("not-a-cap-waiver")),
        Err(LedgerError::Budget)
    ));
    let mut nodes = MemoryNodes::default();
    let exported = l.export_history(&mut nodes, Limits::default()).unwrap();
    let (base, proof) =
        Index::from_legacy(exported.snapshot(), &mut nodes, build_limits()).unwrap();
    // Typed synthetic immutable second segment; no successor SQL is implemented
    // by this component. Actual joined SQL continuation remains required.
    let (delta, end) = synthetic_invalidations(&base.head, MAX_COMMANDS as usize);
    let (next, work) = base
        .extend(&delta, &end, &mut nodes, build_limits())
        .unwrap();
    assert_eq!(next.head.sequence, 32768);
    for original in receipts {
        let mut counted = ReadCount {
            nodes: &nodes,
            calls: Cell::new(0),
        };
        let inspected = next
            .inspect(
                &original.reference.operation,
                &mut counted,
                Limits::default(),
            )
            .unwrap()
            .unwrap();
        assert_receipt(inspected.receipt(), &original);
        assert!(counted.calls.get() <= 1024);
    }
    for sequence in [1, 8192, 16384, 16385, 24576, 32768] {
        assert_eq!(
            next.prefix(sequence, &mut nodes, Limits::default())
                .unwrap()
                .0
                .sequence,
            sequence
        );
    }
    assert_eq!(
        next.prefix(16384, &mut nodes, Limits::default()).unwrap().0,
        base.head
    );
    assert!(
        next.inspect(&key("never-acknowledged"), &mut nodes, Limits::default())
            .unwrap()
            .is_none()
    );
    eprintln!(
        "global index proof={proof:?}; append={work:?}; head={}",
        next.head.sequence
    );
}
