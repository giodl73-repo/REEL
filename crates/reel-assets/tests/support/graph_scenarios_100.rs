//! Third batch: 50 synthetic design scenarios, not real storage/durability tests.
use super::*;

fn bind() -> BTreeMap<String, Hash> {
    BTreeMap::from([("a".into(), asset("a")), ("b".into(), asset("b"))])
}
fn raw(cas: &mut Cas, domain: &str, payload: &[u8]) -> Hash {
    let bytes = [domain.as_bytes(), &[0], payload].concat();
    let h = format!("{:x}", Sha256::digest(&bytes));
    cas.0.insert(h.clone(), bytes);
    h
}
fn alter_shard(cas: &mut Cas, descriptor: &mut Descriptor, subject: &str, rows: Vec<Claim>) {
    let mut index: EvidenceIndex = cas.read("evidence-index-v1", &descriptor.source).unwrap();
    index
        .shards
        .insert(subject[..2].into(), cas.put("evidence-shard-v1", &rows));
    descriptor.source = cas.put("evidence-index-v1", &index);
}
fn same_prefix(mut row: Claim, prefix: &str) -> Claim {
    row.subject = format!("{prefix}{}", &row.subject[2..]);
    row
}
fn consistent_bytes(result: &Lookup) -> Result<Option<u64>, &'static str> {
    let sizes: BTreeSet<_> = result.claims.iter().map(|c| c.bytes).collect();
    if sizes.len() > 1 {
        Err("Conflicting evidence")
    } else {
        Ok(sizes.first().copied())
    }
}
// Policy/physical_recheck are synthetic stand-ins, not custody verification.
fn prior_proof_usable(
    row: &Claim,
    now: u64,
    allowed: bool,
    max_age: Option<u64>,
    physical_recheck: bool,
) -> bool {
    allowed
        && physical_recheck
        && row.payload_verified
        && now
            .checked_sub(row.observed)
            .is_some_and(|age| max_age.is_none_or(|limit| age <= limit))
}
fn validate_working_roots(roots: &BTreeMap<String, Root>) -> Result<(), &'static str> {
    if roots.iter().any(|(id, r)| {
        id.trim().is_empty()
            || r.locations.is_empty()
            || r.locations.iter().any(|l| l.trim().is_empty())
    }) {
        Err("Unknown working affinity")
    } else {
        Ok(())
    }
}
fn validate_primary_roles(
    roots: &BTreeMap<String, Root>,
    compiled: &BTreeSet<Hash>,
) -> Result<(), &'static str> {
    if roots.values().any(|r| !r.inputs.is_disjoint(compiled)) {
        Err("Compiled output as primary")
    } else {
        Ok(())
    }
}

// Graph identity, sharing and integrity (1-10).
#[test]
fn twenty_roots_explain_twenty_users_of_one_content_identity() {
    let roots = (0..20)
        .map(|i| (format!("root-{i}"), root(&["shared"], &["hot"])))
        .collect();
    let why = users(&roots, "shared", "hot");
    assert_eq!(why.len(), 20);
    assert!(why.contains("root-19"));
}
#[test]
fn repeated_use_sites_within_one_root_count_as_one_user() {
    let roots = BTreeMap::from([("episode".into(), root(&["cel", "cel", "cel"], &["hot"]))]);
    assert_eq!(users(&roots, "cel", "hot").len(), 1);
}
#[test]
fn nested_shared_leaf_produces_one_asset_membership() {
    let mut cas = Cas::default();
    let leaf = cas.put(
        "node-v1",
        &Node::Bindings(BTreeMap::from([("a".into(), asset("shared"))])),
    );
    let branch = cas.put(
        "node-v1",
        &Node::Children(BTreeMap::from([
            ("left".into(), leaf.clone()),
            ("right".into(), leaf),
        ])),
    );
    let top = cas.put(
        "node-v1",
        &Node::Children(BTreeMap::from([("nested".into(), branch)])),
    );
    assert_eq!(
        inputs(&cas, &top, &mut 4).unwrap(),
        BTreeSet::from([asset("shared")])
    );
}
#[test]
fn deep_graph_exhaustion_is_not_an_empty_closure() {
    let mut cas = Cas::default();
    let mut h = cas.put("node-v1", &Node::Bindings(bind()));
    for _ in 0..40 {
        h = cas.put(
            "node-v1",
            &Node::Children(BTreeMap::from([("child".into(), h)])),
        );
    }
    assert!(inputs(&cas, &h, &mut 10).is_err());
    assert_eq!(inputs(&cas, &h, &mut 41).unwrap().len(), 2);
}
#[test]
fn diff_success_does_not_validate_an_unchanged_corrupt_subtree() {
    let mut cas = Cas::default();
    let mut scenes = episode();
    let old = snapshot(&mut cas, &scenes);
    let children = match cas.read::<Node>("node-v1", &old).unwrap() {
        Node::Children(v) => v,
        _ => panic!(),
    };
    cas.0.get_mut(&children["scene-09"]).unwrap().push(0);
    scenes[0].insert("new".into(), asset("new"));
    let new = snapshot(&mut cas, &scenes[..9]);
    // Use a new root retaining the corrupt scene-09 address without republishing it.
    let mut new_children = match cas.read::<Node>("node-v1", &new).unwrap() {
        Node::Children(v) => v,
        _ => panic!(),
    };
    new_children.insert("scene-09".into(), children["scene-09"].clone());
    let new = cas.put("node-v1", &Node::Children(new_children));
    assert!(changed_page_inputs(&cas, &old, &new, &mut 0).is_ok());
    assert!(inputs(&cas, &new, &mut 11).is_err());
}
#[test]
fn repeated_paths_consume_traversal_budget_even_when_membership_deduplicates() {
    let mut cas = Cas::default();
    let leaf = cas.put("node-v1", &Node::Bindings(bind()));
    let h = cas.put(
        "node-v1",
        &Node::Children((0..100).map(|i| (i.to_string(), leaf.clone())).collect()),
    );
    assert!(inputs(&cas, &h, &mut 2).is_err());
    assert_eq!(inputs(&cas, &h, &mut 101).unwrap().len(), 2);
}
#[test]
fn publication_rejects_a_corrupt_existing_winner_without_overwriting_it() {
    let mut cas = Cas::default();
    let h = cas.put("test", &bind());
    cas.0.get_mut(&h).unwrap().push(0);
    let before = cas.0[&h].clone();
    assert!(cas.try_put("test", &bind()).is_err());
    assert_eq!(cas.0[&h], before);
}
#[test]
fn independent_stores_produce_identical_snapshot_addresses() {
    let (mut a, mut b) = (Cas::default(), Cas::default());
    assert_eq!(snapshot(&mut a, &episode()), snapshot(&mut b, &episode()));
}
#[test]
fn scene_order_changes_authoring_identity_without_changing_payload_membership() {
    let mut cas = Cas::default();
    let h = snapshot(&mut cas, &episode());
    let a = serde_json::json!({"bindings":h,"order":["scene-00","scene-01"]});
    let b = serde_json::json!({"bindings":h,"order":["scene-01","scene-00"]});
    assert_ne!(cas.put("authoring", &a), cas.put("authoring", &b));
    assert_eq!(inputs(&cas, &h, &mut 11).unwrap().len(), 1000);
}
#[test]
fn distinct_unicode_identifiers_are_not_silently_normalized_together() {
    let mut cas = Cas::default();
    let rows = BTreeMap::from([("é".into(), asset("a")), ("e\u{301}".into(), asset("b"))]);
    let h = cas.put("node-v1", &Node::Bindings(rows));
    assert_eq!(inputs(&cas, &h, &mut 1).unwrap().len(), 2);
}

// Serial transaction/merge interleavings; not OS concurrency (11-20).
#[test]
fn independent_three_writer_permutations_have_the_same_final_revisions() {
    for order in [
        ["a", "b", "c"],
        ["a", "c", "b"],
        ["b", "a", "c"],
        ["b", "c", "a"],
        ["c", "a", "b"],
        ["c", "b", "a"],
    ] {
        let mut h = Heads::default();
        for key in order {
            h.admit(&[(key, 0, 1)]).unwrap();
        }
        assert_eq!(
            h.0,
            BTreeMap::from([("a".into(), 1), ("b".into(), 1), ("c".into(), 1)])
        );
    }
}
#[test]
fn either_same_key_writer_can_win_but_the_second_stale_write_cannot() {
    for order in [["left", "right"], ["right", "left"]] {
        let mut h = Heads::default();
        let mut winner = None;
        for writer in order {
            if h.admit(&[("selected", 0, 1)]).is_ok() {
                winner = Some(writer);
            }
        }
        assert_eq!(winner, Some(order[0]));
        assert_eq!(h.0["selected"], 1);
    }
}
#[test]
fn disjoint_merge_is_commutative_across_multiple_payload_versions() {
    for i in 0..16 {
        let base = bind();
        let (mut a, mut b) = (base.clone(), base.clone());
        a.insert("a".into(), asset(&format!("left-{i}")));
        b.insert("b".into(), asset(&format!("right-{i}")));
        assert_eq!(
            merge_bindings(&base, &a, &b).unwrap(),
            merge_bindings(&base, &b, &a).unwrap()
        );
    }
}
#[test]
fn unchanged_side_does_not_erase_a_peers_changes() {
    let base = bind();
    let mut changed = base.clone();
    changed.remove("a");
    changed.insert("new".into(), asset("new"));
    assert_eq!(merge_bindings(&base, &base, &changed).unwrap(), changed);
}
#[test]
fn rename_via_delete_add_conflicts_with_edit_at_the_original_id() {
    let base = bind();
    let (mut renamed, mut edited) = (base.clone(), base.clone());
    let moved = renamed.remove("a").unwrap();
    renamed.insert("renamed".into(), moved);
    edited.insert("a".into(), asset("edited"));
    assert!(merge_bindings(&base, &renamed, &edited).is_err());
}
#[test]
fn identical_duplicate_transaction_keys_are_rejected_too() {
    let mut h = Heads::default();
    assert!(h.admit(&[("x", 0, 1), ("x", 0, 1)]).is_err());
    assert!(h.0.is_empty());
}
#[test]
fn an_empty_change_set_does_not_advance_any_revision() {
    let mut h = Heads::default();
    h.admit(&[("x", 0, 1)]).unwrap();
    let before = h.0.clone();
    h.admit(&[]).unwrap();
    assert_eq!(h.0, before);
}
#[test]
fn checkpoint_serialization_is_read_only() {
    let mut h = Heads::default();
    h.admit(&[("x", 0, 1)]).unwrap();
    let before = h.0.clone();
    let first = serde_json::to_vec(&h).unwrap();
    let second = serde_json::to_vec(&h).unwrap();
    assert_eq!(first, second);
    assert_eq!(h.0, before);
}
#[test]
fn a_tampered_checkpoint_is_rejected_under_its_old_content_address() {
    let mut cas = Cas::default();
    let h = cas.put("heads-v1", &Heads::default());
    cas.0.get_mut(&h).unwrap().push(0);
    assert!(cas.read::<Heads>("heads-v1", &h).is_err());
}
#[test]
fn publishing_immutable_nodes_does_not_select_a_working_root() {
    let mut cas = Cas::default();
    snapshot(&mut cas, &episode());
    let heads = Heads::default();
    assert!(!cas.0.is_empty());
    assert!(heads.0.is_empty());
}

// Protection and explicit semantic roles (21-30).
#[test]
fn one_root_can_require_two_locations_without_becoming_two_logical_users() {
    let roots = BTreeMap::from([("episode".into(), root(&["cel"], &["hot-a", "hot-b"]))]);
    assert_eq!(users(&roots, "cel", "hot-a"), users(&roots, "cel", "hot-b"));
    assert_eq!(users(&roots, "cel", "hot-a").len(), 1);
}
#[test]
fn moving_episode_affinity_does_not_drop_music_demand_on_the_original_location() {
    let mut roots = BTreeMap::from([
        ("episode".into(), root(&["shared"], &["old", "new"])),
        ("music".into(), root(&["shared"], &["old"])),
    ]);
    roots.get_mut("episode").unwrap().locations.remove("old");
    assert_eq!(users(&roots, "shared", "old"), BTreeSet::from(["music"]));
    assert_eq!(users(&roots, "shared", "new"), BTreeSet::from(["episode"]));
}
#[test]
fn explicit_pin_protects_an_old_output_without_a_golden_selection() {
    let mut pin = root(&["source"], &["hot"]);
    pin.outputs.insert("old-output".into());
    let roots = BTreeMap::from([("pin/owner".into(), pin)]);
    assert_eq!(users(&roots, "old-output", "hot").len(), 1);
    assert_eq!(users(&roots, "source", "hot").len(), 1);
}
#[test]
fn known_compiled_output_cannot_be_laundered_into_primary_inputs() {
    let roots = BTreeMap::from([("episode".into(), root(&["compiled"], &["hot"]))]);
    assert!(validate_primary_roles(&roots, &BTreeSet::from(["compiled".into()])).is_err());
}
#[test]
fn generated_primary_cue_is_not_classified_as_compiled_merely_because_a_job_produced_it() {
    let mut job = root(&[], &["hot"]);
    job.outputs.insert("generated-cue".into());
    let roots = BTreeMap::from([
        ("job".into(), job),
        ("episode".into(), root(&["generated-cue"], &["hot"])),
    ]);
    assert!(validate_primary_roles(&roots, &BTreeSet::new()).is_ok());
    assert_eq!(users(&roots, "generated-cue", "hot").len(), 2);
}
#[test]
fn provenance_reference_does_not_cancel_another_roots_primary_requirement() {
    let mut history = root(&[], &["hot"]);
    history.references.insert("cel".into());
    let roots = BTreeMap::from([
        ("history".into(), history),
        ("current".into(), root(&["cel"], &["hot"])),
    ]);
    assert_eq!(users(&roots, "cel", "hot"), BTreeSet::from(["current"]));
}
#[test]
fn retaining_a_receipt_does_not_reauthorize_an_abandoned_draft() {
    let mut cas = Cas::default();
    let d = evidence(&mut cas, vec![claim(0, true)], true);
    let receipt = EvidenceAdapter {
        cas: &cas,
        descriptor: d,
    }
    .lookup(&claim(0, true).subject, 2)
    .unwrap();
    let roots = BTreeMap::new();
    assert_eq!(receipt.claims.len(), 1);
    assert!(users(&roots, &claim(0, true).subject, "hot").is_empty());
}
#[test]
fn missing_working_affinity_is_an_admission_gap_not_zero_demand_authority() {
    let roots = BTreeMap::from([("draft".into(), root(&["cel"], &[]))]);
    assert!(validate_working_roots(&roots).is_err());
}
#[test]
fn releasing_one_location_keeps_the_other_location_protected() {
    let mut roots = BTreeMap::from([("draft".into(), root(&["cel"], &["a", "b"]))]);
    roots.get_mut("draft").unwrap().locations.remove("a");
    assert!(users(&roots, "cel", "a").is_empty());
    assert_eq!(users(&roots, "cel", "b").len(), 1);
}
#[test]
fn zero_users_with_only_unverified_archival_evidence_still_holds_removal() {
    let mut cas = Cas::default();
    let d = evidence(&mut cas, vec![claim(0, false)], true);
    let result = EvidenceAdapter {
        cas: &cas,
        descriptor: d,
    }
    .lookup(&claim(0, false).subject, 2)
    .unwrap();
    assert!(!removal_allowed(
        false,
        true,
        1,
        1,
        "disk-a",
        "disk-b",
        result.claims[0].payload_verified
    ));
}

// Provenance, contradictory records and prior-proof policy (31-40).
#[test]
fn one_subjects_independent_location_observations_are_both_retained() {
    let mut cas = Cas::default();
    let a = claim(0, true);
    let mut b = a.clone();
    b.location = "another-archive".into();
    let d = evidence(&mut cas, vec![a.clone(), b.clone()], true);
    let rows = EvidenceAdapter {
        cas: &cas,
        descriptor: d,
    }
    .lookup(&a.subject, 2)
    .unwrap()
    .claims;
    assert_eq!(rows, vec![a, b]);
}
#[test]
fn conflicting_sizes_are_diagnostic_evidence_not_a_consistent_custody_proof() {
    let mut cas = Cas::default();
    let a = claim(0, true);
    let mut b = a.clone();
    b.bytes += 1;
    let d = evidence(&mut cas, vec![a.clone(), b], true);
    let result = EvidenceAdapter {
        cas: &cas,
        descriptor: d,
    }
    .lookup(&a.subject, 2)
    .unwrap();
    assert_eq!(result.claims.len(), 2);
    assert!(consistent_bytes(&result).is_err());
}
#[test]
fn verification_is_per_copy_not_inherited_from_another_record_in_the_source() {
    let mut cas = Cas::default();
    let a = claim(0, true);
    let mut b = a.clone();
    b.payload_verified = false;
    b.location = "unverified".into();
    let d = evidence(&mut cas, vec![a.clone(), b], true);
    let result = EvidenceAdapter {
        cas: &cas,
        descriptor: d,
    }
    .lookup(&a.subject, 2)
    .unwrap();
    assert!(result.claims[0].payload_verified);
    assert!(!result.claims[1].payload_verified);
}
#[test]
fn malformed_record_subject_in_a_hashed_shard_is_rejected() {
    let mut cas = Cas::default();
    let a = claim(0, true);
    let mut d = evidence(&mut cas, vec![a.clone()], true);
    let mut bad = a.clone();
    bad.subject = "अ".into();
    alter_shard(&mut cas, &mut d, &a.subject, vec![bad]);
    assert!(
        EvidenceAdapter {
            cas: &cas,
            descriptor: d
        }
        .lookup(&a.subject, 2)
        .is_err()
    );
}
#[test]
fn a_valid_subject_in_the_wrong_shard_is_not_treated_as_query_absence() {
    let mut cas = Cas::default();
    let a = claim(0, true);
    let mut d = evidence(&mut cas, vec![a.clone()], true);
    let mut misplaced = claim(1, true);
    misplaced.subject = format!(
        "{}{}",
        if a.subject.starts_with("00") {
            "ff"
        } else {
            "00"
        },
        &misplaced.subject[2..]
    );
    alter_shard(&mut cas, &mut d, &a.subject, vec![misplaced]);
    assert!(
        EvidenceAdapter {
            cas: &cas,
            descriptor: d
        }
        .lookup(&a.subject, 2)
        .is_err()
    );
}
#[test]
fn repeated_evidence_records_do_not_inflate_logical_root_refcounts() {
    let mut cas = Cas::default();
    let a = claim(0, true);
    let d = evidence(&mut cas, vec![a.clone(), a.clone()], true);
    let result = EvidenceAdapter {
        cas: &cas,
        descriptor: d,
    }
    .lookup(&a.subject, 2)
    .unwrap();
    let roots = BTreeMap::from([("episode".into(), root(&[&a.subject], &["hot"]))]);
    assert_eq!(result.claims.len(), 2);
    assert_eq!(users(&roots, &a.subject, "hot").len(), 1);
}
#[test]
fn future_dated_copy_proof_is_not_reusable() {
    let mut row = claim(0, true);
    row.observed = 101;
    assert!(!prior_proof_usable(&row, 100, true, None, true));
}
#[test]
fn age_limited_copy_proof_expires_at_the_configured_boundary() {
    let mut row = claim(0, true);
    row.observed = 50;
    assert!(prior_proof_usable(&row, 100, true, Some(50), true));
    assert!(!prior_proof_usable(&row, 101, true, Some(50), true));
}
#[test]
fn explicit_append_only_reuse_can_allow_old_proof_but_not_without_owner_policy() {
    let row = claim(0, true);
    assert!(prior_proof_usable(&row, 100000, true, None, true));
    assert!(!prior_proof_usable(&row, 100000, false, None, true));
}
#[test]
fn changed_physical_identity_prevents_reuse_of_a_recent_verified_receipt() {
    let row = claim(0, true);
    assert!(!prior_proof_usable(&row, 2, true, Some(10), false));
}

// Query resource limits, format rejection and immutable evidence refresh (41-50).
#[test]
fn zero_total_byte_budget_rejects_even_a_small_root() {
    let mut cas = Cas::default();
    let d = evidence(&mut cas, vec![claim(0, true)], true);
    let limits = LookupLimits {
        node_bytes: usize::MAX,
        total_bytes: 0,
        records: 1,
        nodes: 2,
    };
    assert!(
        EvidenceAdapter {
            cas: &cas,
            descriptor: d
        }
        .lookup_limited(&claim(0, true).subject, limits)
        .is_err()
    );
}
#[test]
fn an_empty_captured_scope_needs_only_one_node_read() {
    let mut cas = Cas::default();
    let d = evidence(&mut cas, vec![], true);
    let result = EvidenceAdapter {
        cas: &cas,
        descriptor: d,
    }
    .lookup(&asset("absent"), 1)
    .unwrap();
    assert_eq!(result.reads, 1);
    assert_eq!(result.membership(), CapturedMembership::AbsentAtCapture);
}
#[test]
fn the_index_header_itself_is_subject_to_the_node_byte_limit() {
    let mut cas = Cas::default();
    let d = evidence(
        &mut cas,
        (0..1000).map(|i| claim(i, false)).collect(),
        false,
    );
    let root_size = cas.0[&d.source].len();
    let limits = LookupLimits {
        node_bytes: root_size - 1,
        total_bytes: usize::MAX,
        records: 1000,
        nodes: 2,
    };
    assert!(
        EvidenceAdapter {
            cas: &cas,
            descriptor: d
        }
        .lookup_limited(&claim(0, false).subject, limits)
        .is_err()
    );
}
#[test]
fn an_invalid_later_record_prevents_earlier_valid_claims_from_being_emitted() {
    let mut cas = Cas::default();
    let a = claim(0, true);
    let mut d = evidence(&mut cas, vec![a.clone()], true);
    let mut bad = same_prefix(claim(1, false), &a.subject[..2]);
    bad.location = "".into();
    alter_shard(&mut cas, &mut d, &a.subject, vec![a.clone(), bad]);
    assert!(
        EvidenceAdapter {
            cas: &cas,
            descriptor: d
        }
        .lookup(&a.subject, 2)
        .is_err()
    );
}
#[test]
fn repeated_reads_are_charged_again_to_the_same_operation_meter() {
    let mut cas = Cas::default();
    let h = cas.put("test", &1);
    let size = cas.0[&h].len();
    let mut meter = Meter {
        limits: LookupLimits {
            node_bytes: size,
            total_bytes: 2 * size,
            records: 1,
            nodes: 3,
        },
        bytes: 0,
        nodes: 0,
    };
    meter.read::<u64>(&cas, "test", &h).unwrap();
    meter.read::<u64>(&cas, "test", &h).unwrap();
    assert!(meter.read::<u64>(&cas, "test", &h).is_err());
    assert_eq!(meter.nodes, 2);
}
#[test]
fn rejected_budget_charge_does_not_partially_advance_meter_counters() {
    let mut cas = Cas::default();
    let h = cas.put("test", &1);
    let mut meter = Meter {
        limits: LookupLimits {
            node_bytes: 0,
            total_bytes: usize::MAX,
            records: 1,
            nodes: 2,
        },
        bytes: 7,
        nodes: 1,
    };
    assert!(meter.read::<u64>(&cas, "test", &h).is_err());
    assert_eq!((meter.bytes, meter.nodes), (7, 1));
}
#[test]
fn duplicate_index_fields_are_rejected_even_with_a_valid_content_hash() {
    let mut cas = Cas::default();
    let mut d = evidence(&mut cas, vec![], true);
    let index: EvidenceIndex = cas.read("evidence-index-v1", &d.source).unwrap();
    let valid = serde_json::to_string(&index).unwrap();
    let duplicated = format!(
        r#"{},"project":"duplicate"}}"#,
        valid.strip_suffix('}').unwrap()
    );
    d.source = raw(&mut cas, "evidence-index-v1", duplicated.as_bytes());
    assert!(
        matches!(cas.read::<EvidenceIndex>("evidence-index-v1", &d.source), Err(e) if e.contains("duplicate field"))
    );
    assert!(
        EvidenceAdapter {
            cas: &cas,
            descriptor: d
        }
        .lookup(&asset("absent"), 2)
        .is_err()
    );
}
#[test]
fn unknown_scope_affecting_fields_are_not_silently_ignored() {
    let mut cas = Cas::default();
    let mut d = evidence(&mut cas, vec![], true);
    let index: EvidenceIndex = cas.read("evidence-index-v1", &d.source).unwrap();
    let mut value = serde_json::to_value(index).unwrap();
    value["additional_authority"] = serde_json::json!("unexpected");
    d.source = cas.put("evidence-index-v1", &value);
    assert!(
        EvidenceAdapter {
            cas: &cas,
            descriptor: d
        }
        .lookup(&asset("absent"), 2)
        .is_err()
    );
}
#[test]
fn valid_hash_of_truncated_json_is_not_a_valid_index() {
    let mut cas = Cas::default();
    let mut d = evidence(&mut cas, vec![], true);
    d.source = raw(
        &mut cas,
        "evidence-index-v1",
        br#"{"schema":"synthetic.evidence-index.v1""#,
    );
    assert!(
        EvidenceAdapter {
            cas: &cas,
            descriptor: d
        }
        .lookup(&asset("absent"), 2)
        .is_err()
    );
}
#[test]
fn refreshing_evidence_keeps_the_old_source_independently_readable() {
    let mut cas = Cas::default();
    let a = claim(0, true);
    let mut b = a.clone();
    b.observed = 2;
    let old = evidence(&mut cas, vec![a.clone()], true);
    let new = evidence(&mut cas, vec![b.clone()], true);
    assert!(old != new);
    assert_eq!(
        EvidenceAdapter {
            cas: &cas,
            descriptor: old
        }
        .lookup(&a.subject, 2)
        .unwrap()
        .claims,
        vec![a]
    );
    assert_eq!(
        EvidenceAdapter {
            cas: &cas,
            descriptor: new
        }
        .lookup(&b.subject, 2)
        .unwrap()
        .claims,
        vec![b]
    );
}
