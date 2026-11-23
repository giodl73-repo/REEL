use super::*;
use crate::tree::MemoryNodes;
fn observe(d: &mut CapacityDomain, free: u64) {
    let t = d.begin_observation().unwrap();
    d.observe(t, free).unwrap();
}
fn fixture() -> (
    CapacityDomain,
    ReservationToken,
    AllocationTicket,
    AllocationTicket,
    ObservationTicket,
) {
    let mut d = CapacityDomain::new("volume", "epoch", 10).unwrap();
    observe(&mut d, 10_000);
    let old = d.reserve("old", "owner", 500).unwrap();
    let retired = d.begin_allocation(&old, "old-bytes", 200).unwrap();
    d.complete_allocation(&retired, 100).unwrap();
    d.finish(&old).unwrap();
    d.retire_closed(&old).unwrap();
    let token = d.reserve("live", "owner", 1000).unwrap();
    let done = d.begin_allocation(&token, "completed", 300).unwrap();
    d.complete_allocation(&done, 200).unwrap();
    let pending = d.begin_allocation(&token, "in-flight", 500).unwrap();
    let probe = d.begin_observation().unwrap();
    (d, token, done, pending, probe)
}

#[test]
fn checked_context_rejects_coherent_foreign_and_empty_models_without_header_trust() {
    let mut nodes = MemoryNodes::default();
    for (domain, epoch, floor) in [
        ("other-volume", "instance", 10),
        ("volume", "caller-epoch-label", 10),
        ("volume", "instance", 0),
        ("volume", "instance", 11),
    ] {
        let foreign = CapacityDomain::new(domain, epoch, floor).unwrap();
        let (cp, _) = foreign
            .export_checkpoint(&mut nodes, Limits::default())
            .unwrap();
        let decoded = cp.read(&mut nodes, Limits::default()).unwrap();
        let metrics = decoded.metrics();
        let root = decoded.root().clone();
        assert!(!decoded.matches_context("volume", "instance", 10));
        assert_eq!(decoded.metrics(), metrics);
        assert_eq!(decoded.root(), &root);
        assert!(matches!(
            decoded.into_domain_for_context("volume", "instance", 10),
            Err(Error::Invalid)
        ));
    }
    let exact = CapacityDomain::new("volume", "instance", 10).unwrap();
    let (cp, _) = exact
        .export_checkpoint(&mut nodes, Limits::default())
        .unwrap();
    let decoded = cp.read(&mut nodes, Limits::default()).unwrap();
    assert!(decoded.matches_context("volume", "instance", 10));
    let restored = decoded
        .into_domain_for_context("volume", "instance", 10)
        .unwrap();
    assert_eq!(restored.summary(), Err(CapacityError::UnknownObservation));
    assert!(restored.closed_reservations(256).unwrap().is_empty());
}

#[test]
fn checked_context_preserves_held_outcomes_and_strips_pending_probe_admission() {
    let (mut original, token, done, pending, probe) = fixture();
    let summary = original.summary().unwrap();
    let mut nodes = MemoryNodes::default();
    let (cp, _) = original
        .export_checkpoint(&mut nodes, Limits::default())
        .unwrap();
    let decoded = cp.read(&mut nodes, Limits::default()).unwrap();
    assert!(decoded.matches_context("volume", "epoch", 10));
    assert_eq!(decoded.recorded_accounting().unwrap(), summary);
    let mut restored = decoded
        .into_domain_for_context("volume", "epoch", 10)
        .unwrap();
    assert_eq!(restored.summary(), Err(CapacityError::UnknownObservation));
    assert_eq!(restored.observe(probe, 10_000), Err(CapacityError::Stale));
    assert_eq!(
        restored.reserve("new", "owner", 1),
        Err(CapacityError::UnknownObservation)
    );
    assert_eq!(restored.reserve("live", "owner", 1000).unwrap(), token);
    assert_eq!(restored.allocation_status(&done).unwrap(), Some(200));
    assert_eq!(restored.allocation_status(&pending).unwrap(), None);
    assert_eq!(restored.finish(&token), Err(CapacityError::InFlight));
    assert_eq!(
        restored.complete_allocation(&pending, 250).unwrap(),
        original.complete_allocation(&pending, 250).unwrap()
    );
    assert_eq!(restored.observation_nonce, original.observation_nonce);
    assert_eq!(restored.next_generation, original.next_generation);
    assert_eq!(restored.allocation_sequence, original.allocation_sequence);
    assert_eq!(restored.observed_through, original.observed_through);
    assert_eq!(restored.debits, original.debits);
}

#[test]
fn closed_query_is_bounded_readonly_and_never_enumerates_retired_or_active_work() {
    let mut domain = CapacityDomain::new("volume", "epoch", 10).unwrap();
    observe(&mut domain, 100_000);
    let retired = domain.reserve("a-retired", "owner", 100).unwrap();
    domain.finish(&retired).unwrap();
    domain.retire_closed(&retired).unwrap();
    let active = domain.reserve("active", "owner", 100).unwrap();
    let inflight = domain.begin_allocation(&active, "pending", 40).unwrap();
    let mut expected = Vec::new();
    for id in ["b-closed", "c-closed", "d-closed"] {
        let token = domain.reserve(id, "owner", 100).unwrap();
        domain.finish(&token).unwrap();
        expected.push(token);
    }
    let mut nodes = MemoryNodes::default();
    let (before, _) = domain
        .export_checkpoint(&mut nodes, Limits::default())
        .unwrap();
    let summary = domain.summary().unwrap();
    assert_eq!(domain.closed_reservations(2).unwrap(), expected[..2]);
    assert_eq!(domain.closed_reservations(256).unwrap(), expected);
    for limit in [0, 257, usize::MAX] {
        assert_eq!(
            domain.closed_reservations(limit),
            Err(CapacityError::Budget)
        );
    }
    assert_eq!(domain.summary().unwrap(), summary);
    assert_eq!(domain.allocation_status(&inflight).unwrap(), None);
    assert_eq!(
        domain
            .export_checkpoint(&mut nodes, Limits::default())
            .unwrap()
            .0,
        before
    );
    let old = expected.remove(0);
    domain.retire_closed(&old).unwrap();
    let new = domain.reserve(old.id(), "other-owner", 100).unwrap();
    assert!(new.generation > old.generation);
    assert_eq!(domain.closed_reservations(256).unwrap(), expected);
    assert_eq!(domain.is_closed(&old), Err(CapacityError::Stale));
    let restored = domain
        .export_checkpoint(&mut nodes, Limits::default())
        .unwrap()
        .0
        .read(&mut nodes, Limits::default())
        .unwrap()
        .into_domain_for_context("volume", "epoch", 10)
        .unwrap();
    assert_eq!(restored.closed_reservations(256).unwrap(), expected);
}
#[test]
fn active_inflight_retired_debits_and_highs_survive_without_admitting_old_probes() {
    let (mut d, token, done, pending, probe) = fixture();
    let before = d.summary().unwrap();
    assert_eq!(before.allocated_after_watermark, 300);
    assert_eq!(before.reserved_remaining, 800);
    let mut nodes = MemoryNodes::default();
    let (cp, write) = d.export_checkpoint(&mut nodes, Limits::default()).unwrap();
    assert!(write.calls > 4);
    let decoded = cp.read(&mut nodes, Limits::default()).unwrap();
    assert_eq!(decoded.root(), cp.root());
    assert_eq!(decoded.recorded_accounting().unwrap(), before);
    let mut restored = decoded.into_domain();
    assert_eq!(restored.summary(), Err(CapacityError::UnknownObservation));
    assert_eq!(
        restored.reserve("new", "owner", 1),
        Err(CapacityError::UnknownObservation)
    );
    assert_eq!(restored.observe(probe, 10_000), Err(CapacityError::Stale));
    assert_eq!(restored.reserve("live", "owner", 1000).unwrap(), token);
    assert_eq!(restored.allocation_status(&done).unwrap(), Some(200));
    assert_eq!(restored.allocation_status(&pending).unwrap(), None);
    assert_eq!(restored.finish(&token), Err(CapacityError::InFlight));
    let old_seq = d.complete_allocation(&pending, 250).unwrap();
    assert_eq!(
        restored.complete_allocation(&pending, 250).unwrap(),
        old_seq
    );
    assert_eq!(
        restored.complete_allocation(&pending, 250).unwrap(),
        old_seq
    );
    assert_eq!(
        restored.complete_allocation(&pending, 251),
        Err(CapacityError::Conflict)
    );
    restored.finish(&token).unwrap();
    let history = restored.retire_closed(&token).unwrap();
    assert_eq!(history.allocation_status(&done).unwrap(), Some(200));
    assert_eq!(history.allocation_status(&pending).unwrap(), Some(250));
    observe(&mut restored, 9000);
    let next = restored.reserve("live", "another-owner", 1000).unwrap();
    assert!(next.generation > token.generation);
    assert_eq!(
        restored.begin_allocation(&token, "stale", 1).unwrap_err(),
        CapacityError::Stale
    );
}
#[test]
fn a_new_probe_keeps_later_completion_charged_and_zero_outcomes_exact() {
    let (d, token, done, pending, _) = fixture();
    let mut nodes = MemoryNodes::default();
    let (cp, _) = d.export_checkpoint(&mut nodes, Limits::default()).unwrap();
    let mut d = cp
        .read(&mut nodes, Limits::default())
        .unwrap()
        .into_domain();
    let probe = d.begin_observation().unwrap();
    let sequence = d.complete_allocation(&pending, 125).unwrap();
    d.observe(probe, 8000).unwrap();
    assert_eq!(d.summary().unwrap().allocated_after_watermark, 125);
    assert_eq!(d.complete_allocation(&done, 200).unwrap(), 2);
    let zero = d.begin_allocation(&token, "zero", 100).unwrap();
    let n = d.complete_allocation(&zero, 0).unwrap();
    assert!(n > sequence);
    let (cp, _) = d.export_checkpoint(&mut nodes, Limits::default()).unwrap();
    let mut r = cp
        .read(&mut nodes, Limits::default())
        .unwrap()
        .into_domain();
    assert_eq!(r.complete_allocation(&zero, 0).unwrap(), n);
    assert_eq!(r.allocation_status(&zero).unwrap(), Some(0));
}
#[test]
fn canonical_empty_header_matches_independent_literal_and_digest() {
    let d = CapacityDomain::new("volume", "epoch", 9).unwrap();
    let mut nodes = MemoryNodes::default();
    let (cp, _) = d.export_checkpoint(&mut nodes, Limits::default()).unwrap();
    let (header, _) = Tree::from_root(cp.root.clone())
        .get(
            &LogicalPath::parse("header").unwrap(),
            &mut nodes,
            TreeLimits::default(),
        )
        .unwrap();
    let o = header.unwrap();
    let expected=br#"{"entry":{"allocation_sequence":0,"debit_count":0,"domain":"volume","epoch":"epoch","event_count":0,"floor":9,"kind":"header","next_generation":0,"observation_nonce":0,"observed_through":0,"pending_observation":null,"reservation_count":0,"sampled_free":null},"schema":"tig.capacity-state.design-1"}"#;
    assert_eq!(nodes.get(&o).unwrap().unwrap(), expected);
    assert_eq!(o, Object::from_bytes(expected));
    assert_eq!(
        o.sha256.as_str(),
        "9dbc62b8f94b8fc74a05c5389cda06db8cb9652f1435a81458a846a931e24b3c"
    );
    let restored = cp
        .read(&mut nodes, Limits::default())
        .unwrap()
        .into_domain();
    assert_eq!(restored.summary(), Err(CapacityError::UnknownObservation));
}
fn rewrite(
    cp: &Checkpoint,
    nodes: &mut MemoryNodes,
    mut edit: impl FnMut(&mut Value),
    alias: bool,
    missing: bool,
    missing_debit: bool,
) -> Checkpoint {
    let (mut entries, _) = Tree::from_root(cp.root.clone())
        .entries(nodes, TreeLimits::default())
        .unwrap();
    let mut changed = BTreeMap::new();
    for (path, object) in &entries {
        let mut value: Value =
            serde_json::from_slice(&nodes.get(object).unwrap().unwrap()).unwrap();
        edit(&mut value);
        let raw = identity::canonical_json(&value).unwrap();
        changed.insert(path.clone(), nodes.put(&raw).unwrap());
    }
    if alias {
        let key = changed
            .keys()
            .find(|p| p.as_str().starts_with("r/"))
            .unwrap()
            .clone();
        let o = changed.remove(&key).unwrap();
        changed.insert(LogicalPath::parse("r/999").unwrap(), o);
    }
    if missing {
        let key = changed
            .keys()
            .find(|p| p.as_str().starts_with("a/"))
            .unwrap()
            .clone();
        changed.remove(&key);
    }
    if missing_debit {
        assert!(
            changed
                .remove(&LogicalPath::parse("d/2").unwrap())
                .is_some()
        );
    }
    entries = changed;
    let (tree, _) = Tree::build(&entries, nodes, TreeLimits::default()).unwrap();
    Checkpoint::from_root(tree.root().clone())
}
use serde_json::Value;
#[test]
fn repinned_wrong_schema_paths_counts_ownership_generation_arithmetic_and_debits_refuse() {
    for bad in [
        "schema",
        "unknown",
        "domain",
        "generation",
        "remaining",
        "closed",
        "limit",
        "charge",
        "sequence",
        "watermark",
        "nonce",
        "counts",
        "debit",
        "missing_debit",
        "path",
        "missing_event",
    ] {
        let (d, _, _, _, _) = fixture();
        let mut nodes = MemoryNodes::default();
        let (cp, _) = d.export_checkpoint(&mut nodes, Limits::default()).unwrap();
        let changed = rewrite(
            &cp,
            &mut nodes,
            |v| {
                let e = &mut v["entry"];
                let kind = e["kind"].as_str().unwrap().to_owned();
                match bad {
                    "schema" => v["schema"] = "wrong".into(),
                    "unknown" => v["invented"] = true.into(),
                    "domain" if kind == "header" => e["domain"] = "bad token!".into(),
                    "generation" if kind == "header" => e["next_generation"] = 0.into(),
                    "remaining" if kind == "reservation" => e["remaining"] = 999.into(),
                    "closed" if kind == "reservation" => e["closed"] = true.into(),
                    "limit" if kind == "allocation" => e["limit"] = 0.into(),
                    "charge" if kind == "allocation" && !e["completed"].is_null() => {
                        e["completed"]["charge"] = 9999.into()
                    }
                    "sequence" if kind == "allocation" && !e["completed"].is_null() => {
                        e["completed"]["sequence"] = 0.into()
                    }
                    "watermark" if kind == "header" => e["observed_through"] = 999.into(),
                    "nonce" if kind == "header" => e["pending_observation"] = 999.into(),
                    "counts" if kind == "header" => e["event_count"] = 999.into(),
                    "debit" if kind == "debit" => e["bytes"] = 0.into(),
                    "missing_debit" if kind == "header" => e["debit_count"] = 1.into(),
                    _ => {}
                }
            },
            bad == "path",
            bad == "missing_event",
            bad == "missing_debit",
        );
        assert!(
            changed.read(&mut nodes, Limits::default()).is_err(),
            "accepted {bad}"
        );
    }
}
#[test]
fn shared_tree_and_leaf_budgets_cannot_restart_and_failed_export_preserves_model() {
    let (d, _, _, _, _) = fixture();
    let mut nodes = MemoryNodes::default();
    let (cp, _) = d.export_checkpoint(&mut nodes, Limits::default()).unwrap();
    let (_, tree) = Tree::from_root(cp.root.clone())
        .entries(&mut nodes, TreeLimits::default())
        .unwrap();
    assert!(matches!(
        cp.read(
            &mut nodes,
            Limits {
                max_nodes: tree.reads,
                ..Limits::default()
            }
        ),
        Err(Error::Budget)
    ));
    assert!(matches!(
        cp.read(
            &mut nodes,
            Limits {
                max_bytes: tree.read_bytes,
                ..Limits::default()
            }
        ),
        Err(Error::Budget)
    ));
    let failed = d.export_checkpoint(
        &mut nodes,
        Limits {
            max_nodes: 1,
            ..Limits::default()
        },
    );
    assert!(matches!(failed, Err(Error::Budget)));
    let (again, _) = d.export_checkpoint(&mut nodes, Limits::default()).unwrap();
    assert_eq!(again, cp);
    assert!(matches!(
        cp.read(
            &mut nodes,
            Limits {
                max_nodes: MAX_NODES + 1,
                ..Limits::default()
            }
        ),
        Err(Error::Budget)
    ));
}
struct Broken<'a> {
    inner: &'a MemoryNodes,
    missing: bool,
}
impl NodeStore for Broken<'_> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        if self.missing {
            return Ok(None);
        }
        let mut b = self.inner.get(o)?.unwrap();
        b[0] ^= 1;
        Ok(Some(b))
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Err(TreeError::InvalidNode)
    }
}
#[test]
fn missing_and_corrupt_graphs_never_become_empty_working_state() {
    let (d, _, _, _, _) = fixture();
    let mut n = MemoryNodes::default();
    let (cp, _) = d.export_checkpoint(&mut n, Limits::default()).unwrap();
    assert!(matches!(
        cp.read(
            &mut Broken {
                inner: &n,
                missing: true
            },
            Limits::default()
        ),
        Err(Error::Missing(_))
    ));
    assert!(matches!(
        cp.read(
            &mut Broken {
                inner: &n,
                missing: false
            },
            Limits::default()
        ),
        Err(Error::Integrity)
    ));
}
#[test]
fn maximum_bounded_working_set_is_paged_without_larger_records_or_budget_waivers() {
    let mut d = CapacityDomain::new("volume", "epoch", 0).unwrap();
    observe(&mut d, 100_000);
    for i in 0..MAX_RESERVATIONS {
        let r = d.reserve(&format!("job-{i}"), "owner", 4).unwrap();
        for j in 0..4 {
            let a = d.begin_allocation(&r, &format!("event-{j}"), 1).unwrap();
            d.complete_allocation(&a, 1).unwrap();
        }
        d.finish(&r).unwrap();
    }
    let mut nodes = MemoryNodes::default();
    let (cp, m) = d.export_checkpoint(&mut nodes, Limits::default()).unwrap();
    assert!(m.calls <= MAX_NODES);
    assert!(m.bytes <= MAX_BYTES);
    for (_, b) in nodes.objects() {
        assert!(b.len() <= 65536);
    }
    let decoded = cp.read(&mut nodes, Limits::default()).unwrap();
    assert_eq!(decoded.recorded_accounting().unwrap(), d.summary().unwrap());
    assert_eq!(decoded.domain.records.len(), 1024);
    assert_eq!(decoded.domain.event_count, 4096);
    assert_eq!(decoded.domain.debits.len(), 4096);
}

#[test]
fn retired_only_state_preserves_generation_high_and_stales_old_tickets() {
    let mut d = CapacityDomain::new("volume", "epoch", 0).unwrap();
    observe(&mut d, 1000);
    let first = d.reserve("name", "owner", 10).unwrap();
    d.finish(&first).unwrap();
    d.retire_closed(&first).unwrap();
    let second = d.reserve("gone", "owner", 10).unwrap();
    d.finish(&second).unwrap();
    d.retire_closed(&second).unwrap();
    let mut nodes = MemoryNodes::default();
    let (cp, _) = d.export_checkpoint(&mut nodes, Limits::default()).unwrap();
    let mut restored = cp
        .read(&mut nodes, Limits::default())
        .unwrap()
        .into_domain();
    observe(&mut restored, 1000);
    let current = restored.reserve("name", "new-owner", 10).unwrap();
    assert_eq!(current.generation, 3);
    assert_eq!(
        restored.begin_allocation(&first, "old", 1).unwrap_err(),
        CapacityError::Stale
    );
}

#[test]
fn long_tokens_and_safe_integer_high_watermarks_fit_the_same_frozen_limits() {
    const HIGH: u64 = 9_007_199_254_740_991;
    let mut d = CapacityDomain::new("volume", "epoch", 0).unwrap();
    d.next_generation = HIGH - MAX_RESERVATIONS as u64;
    d.allocation_sequence = HIGH - MAX_EVENTS as u64;
    d.observation_nonce = HIGH - 1;
    observe(&mut d, 100_000);
    let owner = "o".repeat(128);
    for i in 0..MAX_RESERVATIONS {
        let id = format!("{i:0128}");
        let token = d.reserve(&id, &owner, 4).unwrap();
        for j in 0..4 {
            let event = format!("{j:0128}");
            let ticket = d.begin_allocation(&token, &event, 1).unwrap();
            d.complete_allocation(&ticket, 1).unwrap();
        }
    }
    let mut nodes = MemoryNodes::default();
    let (checkpoint, metrics) = d.export_checkpoint(&mut nodes, Limits::default()).unwrap();
    assert!(metrics.calls <= MAX_NODES && metrics.bytes <= MAX_BYTES);
    let decoded = checkpoint.read(&mut nodes, Limits::default()).unwrap();
    assert_eq!(decoded.recorded_accounting().unwrap(), d.summary().unwrap());
    let restored = decoded.into_domain();
    assert_eq!(restored.next_generation, HIGH);
    assert_eq!(restored.allocation_sequence, HIGH);
    assert_eq!(restored.observation_nonce, HIGH);
}
