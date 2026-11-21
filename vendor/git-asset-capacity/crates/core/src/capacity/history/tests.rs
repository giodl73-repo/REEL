use super::*;
use crate::tree::MemoryNodes;
fn fixture() -> (
    CapacityDomain,
    RetiredReservation,
    AllocationTicket,
    AllocationTicket,
    ObservationTicket,
) {
    let mut d = CapacityDomain::new("volume", "epoch", 10).unwrap();
    let probe = d.begin_observation().unwrap();
    d.observe(probe, 10000).unwrap();
    let token = d.reserve("job", "owner", 1000).unwrap();
    let zero = d.begin_allocation(&token, "zero", 100).unwrap();
    d.complete_allocation(&zero, 0).unwrap();
    let paid = d.begin_allocation(&token, "paid", 200).unwrap();
    d.complete_allocation(&paid, 125).unwrap();
    d.finish(&token).unwrap();
    let retired = d.retire_closed(&token).unwrap();
    let probe = d.begin_observation().unwrap();
    (d, retired, zero, paid, probe)
}
#[test]
fn matching_records_roundtrip_but_probe_is_only_inspectable_evidence() {
    let (mut d, r, zero, paid, probe) = fixture();
    let token = ReservationRecord::capture(&r.record.token);
    let restored: ReservationRecord = serde_json::from_slice(&canonical(&token).unwrap()).unwrap();
    assert_eq!(restored.token().unwrap(), r.record.token);
    for ticket in [&zero, &paid] {
        let record = AllocationRecord::capture(ticket);
        let decoded: AllocationRecord =
            serde_json::from_slice(&canonical(&record).unwrap()).unwrap();
        assert_eq!(
            r.allocation_status(&decoded.ticket().unwrap()).unwrap(),
            r.allocation_status(ticket).unwrap()
        );
    }
    let evidence = ProbeEvidence::capture(&probe);
    evidence.validate().unwrap();
    assert_eq!(
        canonical(&evidence).unwrap(),
        br#"{"domain":"volume","epoch":"epoch","nonce":2,"watermark":2}"#
    );
    let decoded: ProbeEvidence = serde_json::from_slice(&canonical(&evidence).unwrap()).unwrap();
    assert_eq!(decoded, evidence);
    // Encoding/inspection has not consumed or replaced the actual pending probe.
    d.observe(probe, 8000).unwrap();
    assert_eq!(d.summary().unwrap().allocated_after_watermark, 0);
    let mut bad = serde_json::to_value(&token).unwrap();
    bad["generation"] = 0.into();
    assert!(
        serde_json::from_value::<ReservationRecord>(bad)
            .unwrap()
            .token()
            .is_err()
    );
    let mut bad = serde_json::to_value(AllocationRecord::capture(&paid)).unwrap();
    bad["limit"] = 0.into();
    assert!(
        serde_json::from_value::<AllocationRecord>(bad)
            .unwrap()
            .ticket()
            .is_err()
    );
    let mut bad = serde_json::to_value(&evidence).unwrap();
    bad["nonce"] = 0.into();
    assert!(
        serde_json::from_value::<ProbeEvidence>(bad)
            .unwrap()
            .validate()
            .is_err()
    );
    let mut bad = serde_json::to_value(&evidence).unwrap();
    bad["live"] = true.into();
    assert!(serde_json::from_value::<ProbeEvidence>(bad).is_err());
}
#[test]
fn retired_roundtrip_and_point_outcome_preserve_zero_sequence_and_old_ticket_identity() {
    let (mut d, r, zero, paid, _) = fixture();
    let mut nodes = MemoryNodes::default();
    let (archive, _) = r.export_archive(&mut nodes, Limits::default()).unwrap();
    let encoded = canonical(&archive).unwrap();
    let archive: RetiredArchive = serde_json::from_slice(&encoded).unwrap();
    let (restored, _) = archive.read(&mut nodes, Limits::default()).unwrap();
    assert_eq!(restored.allocation_status(&zero).unwrap(), Some(0));
    assert_eq!(restored.allocation_status(&paid).unwrap(), Some(125));
    assert_eq!(
        archive
            .outcome(&mut nodes, &zero, Limits::default())
            .unwrap()
            .0,
        Outcome {
            charge: 0,
            sequence: 1
        }
    );
    assert_eq!(
        archive
            .outcome(&mut nodes, &paid, Limits::default())
            .unwrap()
            .0,
        Outcome {
            charge: 125,
            sequence: 2
        }
    );
    let probe = d.begin_observation().unwrap();
    d.observe(probe, 8000).unwrap();
    let new = d.reserve("job", "next-owner", 1000).unwrap();
    let next = d.begin_allocation(&new, "paid", 200).unwrap();
    assert!(
        archive
            .outcome(&mut nodes, &next, Limits::default())
            .is_err()
    );
    let mut changed = paid.clone();
    changed.limit = 201;
    assert!(
        archive
            .outcome(&mut nodes, &changed, Limits::default())
            .is_err()
    );
    assert_eq!(d.allocation_status(&paid), Err(CapacityError::Stale));
}
fn repin(
    nodes: &mut MemoryNodes,
    archive: &RetiredArchive,
    mutate: impl FnOnce(&mut BTreeMap<LogicalPath, Object>, &mut MemoryNodes),
) -> RetiredArchive {
    let (mut entries, _) = Tree::from_root(archive.root.clone())
        .entries(nodes, Limits::default().tree())
        .unwrap();
    mutate(&mut entries, nodes);
    let (tree, _) = Tree::build(&entries, nodes, Limits::default().tree()).unwrap();
    RetiredArchive {
        schema: archive.schema.clone(),
        root: tree.root().clone(),
    }
}
fn edit(
    entries: &mut BTreeMap<LogicalPath, Object>,
    nodes: &mut MemoryNodes,
    p: &str,
    f: impl FnOnce(&mut serde_json::Value),
) {
    let p = path(p).unwrap();
    let raw = nodes.get(entries.get(&p).unwrap()).unwrap().unwrap();
    let mut v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    f(&mut v);
    let o = nodes.put(&canonical(&v).unwrap()).unwrap();
    entries.insert(p, o);
}
#[test]
fn repinned_bad_counts_tokens_paths_outcomes_sequences_schema_and_unknown_fields_refuse() {
    let (_, r, _, paid, _) = fixture();
    let mut nodes = MemoryNodes::default();
    let (original, _) = r.export_archive(&mut nodes, Limits::default()).unwrap();
    let event = format!("a/{}", crate::Digest::blob(paid.event.as_bytes()).as_str());
    for case in 0..13 {
        let changed = repin(&mut nodes, &original, |entries, nodes| match case {
            0 => edit(entries, nodes, "header", |v| {
                v["entry"]["event_count"] = 1.into()
            }),
            1 => edit(entries, nodes, "header", |v| {
                v["entry"]["reservation"]["generation"] = 0.into()
            }),
            2 => edit(entries, nodes, "header", |v| {
                v["entry"]["reservation"]["owner"] = "bad owner".into()
            }),
            3 => edit(entries, nodes, "header", |v| {
                v["entry"]["peak"] = 124.into()
            }),
            4 => edit(entries, nodes, &event, |v| v["entry"]["limit"] = 0.into()),
            5 => edit(entries, nodes, &event, |v| {
                v["entry"]["charge"] = 201.into()
            }),
            6 => edit(entries, nodes, &event, |v| {
                v["entry"]["sequence"] = 0.into()
            }),
            7 => edit(entries, nodes, &event, |v| {
                v["entry"]["sequence"] = 1.into()
            }),
            8 => edit(entries, nodes, &event, |v| v["schema"] = "future".into()),
            9 => edit(entries, nodes, &event, |v| v["extra"] = true.into()),
            10 => edit(entries, nodes, &event, |v| {
                v["entry"]["event"] = "changed".into()
            }),
            11 => {
                entries.remove(&path(&event).unwrap());
            }
            12 => {
                let o = entries.remove(&path(&event).unwrap()).unwrap();
                entries.insert(path("a/wrong").unwrap(), o);
            }
            _ => unreachable!(),
        });
        assert!(
            changed.read(&mut nodes, Limits::default()).is_err(),
            "case {case}"
        );
    }
    assert!(original.read(&mut nodes, Limits::default()).is_ok());
}
#[test]
fn shared_budget_failure_missing_node_and_corruption_never_become_empty_history() {
    let (_, r, _, paid, _) = fixture();
    let mut nodes = MemoryNodes::default();
    assert!(
        r.export_archive(
            &mut nodes,
            Limits {
                max_nodes: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
    let (archive, _) = r.export_archive(&mut nodes, Limits::default()).unwrap();
    assert!(
        archive
            .read(
                &mut nodes,
                Limits {
                    max_nodes: 1,
                    ..Limits::default()
                }
            )
            .is_err()
    );
    assert!(
        archive
            .outcome(
                &mut nodes,
                &paid,
                Limits {
                    max_nodes: 1,
                    ..Limits::default()
                }
            )
            .is_err()
    );
    let mut missing = MemoryNodes::default();
    assert!(matches!(
        archive.read(&mut missing, Limits::default()),
        Err(Error::Missing(_))
    ));
    let mut wrong = archive.clone();
    wrong.schema = "future".into();
    assert!(wrong.read(&mut nodes, Limits::default()).is_err());
    assert_eq!(r.allocation_status(&paid).unwrap(), Some(125));
}
#[test]
fn maximum_retired_set_is_paged_and_point_inspection_does_not_enumerate_it() {
    let mut d = CapacityDomain::new("volume", "epoch", 0).unwrap();
    let p = d.begin_observation().unwrap();
    d.observe(p, 10000).unwrap();
    let token = d.reserve(&"j".repeat(128), &"o".repeat(128), 4096).unwrap();
    let mut selected = None;
    for i in 0..MAX_EVENTS {
        let ticket = d.begin_allocation(&token, &format!("{i:0128}"), 1).unwrap();
        d.complete_allocation(&ticket, 1).unwrap();
        if i == 2048 {
            selected = Some(ticket);
        }
    }
    d.finish(&token).unwrap();
    let r = d.retire_closed(&token).unwrap();
    let mut nodes = MemoryNodes::default();
    let (archive, _) = r.export_archive(&mut nodes, Limits::default()).unwrap();
    let (restored, full) = archive.read(&mut nodes, Limits::default()).unwrap();
    let ticket = selected.unwrap();
    assert_eq!(restored.allocation_status(&ticket).unwrap(), Some(1));
    let (outcome, point) = archive
        .outcome(&mut nodes, &ticket, Limits::default())
        .unwrap();
    assert_eq!(
        outcome,
        Outcome {
            charge: 1,
            sequence: 2049
        }
    );
    assert!(
        point.calls < 100 && point.calls * 100 < full.calls,
        "point={point:?},full={full:?}"
    );
    assert!(point.bytes < 1024 * 1024);
    for (_, b) in nodes.objects() {
        assert!(b.len() <= 65536);
    }
}

struct Corrupt;
impl NodeStore for Corrupt {
    fn get(&self, _: &Object) -> crate::tree::TreeResult<Option<Vec<u8>>> {
        Ok(Some(b"altered".to_vec()))
    }
    fn put(&mut self, _: &[u8]) -> crate::tree::TreeResult<Object> {
        Err(crate::tree::TreeError::Integrity)
    }
}
#[test]
fn altered_provider_bytes_are_integrity_failure_not_history() {
    let (_, r, _, paid, _) = fixture();
    let mut nodes = MemoryNodes::default();
    let (archive, _) = r.export_archive(&mut nodes, Limits::default()).unwrap();
    assert!(matches!(
        archive.read(&mut Corrupt, Limits::default()),
        Err(Error::Integrity)
    ));
    assert!(matches!(
        archive.outcome(&mut Corrupt, &paid, Limits::default()),
        Err(Error::Integrity)
    ));
}

#[test]
fn canonical_retired_header_has_an_independently_pinned_literal_and_digest() {
    let (_, r, _, _, _) = fixture();
    let mut nodes = MemoryNodes::default();
    let (archive, _) = r.export_archive(&mut nodes, Limits::default()).unwrap();
    let (object, _) = Tree::from_root(archive.root.clone())
        .get(
            &path("header").unwrap(),
            &mut nodes,
            Limits::default().tree(),
        )
        .unwrap();
    let object = object.unwrap();
    let raw = nodes.get(&object).unwrap().unwrap();
    assert_eq!(raw,br#"{"entry":{"event_count":2,"kind":"header","peak":1000,"reservation":{"domain":"volume","epoch":"epoch","generation":1,"id":"job","owner":"owner"}},"schema":"tig.capacity-retired-node.design-1"}"#);
    assert_eq!(
        object.sha256.as_str(),
        "8f0b2e7ca833e6fd597600427d62f31c2240433131dd8d7cea3286950d1b03de"
    );
}
