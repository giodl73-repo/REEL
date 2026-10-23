//! Executable architecture model, NOT production graph/custody implementation.
//! Synthetic inputs only; no cache, filesystem, rendering or deletion access.
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

type Hash = String;
fn digest(domain: &str, bytes: &[u8]) -> Hash {
    let mut h = Sha256::new();
    h.update(domain.as_bytes());
    h.update([0]);
    h.update(bytes);
    format!("{:x}", h.finalize())
}
fn asset(label: &str) -> Hash {
    digest("synthetic-payload", label.as_bytes())
}
#[derive(Default)]
struct Cas(BTreeMap<Hash, Vec<u8>>);
impl Cas {
    fn put<T: Serialize>(&mut self, domain: &str, value: &T) -> Hash {
        let bytes = serde_json::to_vec(value).unwrap();
        let h = digest(domain, &bytes);
        let framed = [domain.as_bytes(), &[0], bytes.as_slice()].concat();
        self.0.entry(h.clone()).or_insert(framed);
        h
    }
    fn read<T: DeserializeOwned>(&self, domain: &str, h: &str) -> Result<T, String> {
        let bytes = self.0.get(h).ok_or("Missing node")?;
        if format!("{:x}", Sha256::digest(bytes)) != h {
            return Err("Corrupt node".into());
        }
        let prefix = [domain.as_bytes(), &[0]].concat();
        let payload = bytes
            .strip_prefix(prefix.as_slice())
            .ok_or("Wrong node domain")?;
        serde_json::from_slice(payload).map_err(|e| e.to_string())
    }
}
#[derive(Clone, Serialize, Deserialize)]
enum Node {
    Bindings(BTreeMap<String, Hash>),
    Children(BTreeMap<String, Hash>),
}
fn snapshot(cas: &mut Cas, scenes: &[BTreeMap<String, Hash>]) -> Hash {
    // Initial fixture IDs only. Updates/insertion use snapshot_named to keep IDs.
    let named = scenes
        .iter()
        .enumerate()
        .map(|(i, rows)| (format!("scene-{i:02}"), rows.clone()))
        .collect();
    snapshot_named(cas, &named)
}
fn snapshot_named(cas: &mut Cas, scenes: &BTreeMap<String, BTreeMap<String, Hash>>) -> Hash {
    let children = scenes
        .iter()
        .map(|(id, rows)| {
            (
                id.clone(),
                cas.put("node-v1", &Node::Bindings(rows.clone())),
            )
        })
        .collect();
    cas.put("node-v1", &Node::Children(children))
}
fn inputs(cas: &Cas, root: &str, budget: &mut usize) -> Result<BTreeSet<Hash>, String> {
    if *budget == 0 {
        return Err("Traversal budget".into());
    }
    *budget -= 1;
    match cas.read("node-v1", root)? {
        Node::Bindings(rows) => Ok(rows.into_values().collect()),
        Node::Children(children) => {
            let mut result = BTreeSet::new();
            for h in children.values() {
                result.extend(inputs(cas, h, budget)?);
            }
            Ok(result)
        }
    }
}
fn episode() -> Vec<BTreeMap<String, Hash>> {
    (0..10)
        .map(|scene| {
            (0..100)
                .map(|i| (format!("binding-{i:04}"), asset(&format!("{scene}/{i}"))))
                .collect()
        })
        .collect()
}
#[derive(Clone)]
struct Root {
    inputs: BTreeSet<Hash>,
    outputs: BTreeSet<Hash>,
    references: BTreeSet<Hash>,
    locations: BTreeSet<String>,
}
fn root(inputs: &[&str], locations: &[&str]) -> Root {
    Root {
        inputs: inputs.iter().map(|x| (*x).into()).collect(),
        outputs: BTreeSet::new(),
        references: BTreeSet::new(),
        locations: locations.iter().map(|x| (*x).into()).collect(),
    }
}
fn users<'a>(roots: &'a BTreeMap<String, Root>, h: &str, location: &str) -> BTreeSet<&'a str> {
    roots
        .iter()
        .filter_map(|(id, r)| {
            ((r.inputs.contains(h) || r.outputs.contains(h)) && r.locations.contains(location))
                .then_some(id.as_str())
        })
        .collect()
}
#[derive(Default, Serialize, Deserialize)]
struct Heads(BTreeMap<String, u64>);
impl Heads {
    fn admit(&mut self, changes: &[(&str, u64, u64)]) -> Result<(), &'static str> {
        if changes
            .iter()
            .any(|(_, expected, next)| expected.checked_add(1) != Some(*next))
        {
            return Err("Revision must advance without reuse");
        }
        if changes
            .iter()
            .map(|(key, _, _)| key)
            .collect::<BTreeSet<_>>()
            .len()
            != changes.len()
        {
            return Err("Duplicate transaction key");
        }
        if changes
            .iter()
            .any(|(key, expected, _)| self.0.get(*key).copied().unwrap_or(0) != *expected)
        {
            return Err("Selection conflict");
        }
        for (key, _, next) in changes {
            self.0.insert((*key).into(), *next);
        }
        Ok(())
    }
}

fn merge_bindings(
    base: &BTreeMap<String, Hash>,
    left: &BTreeMap<String, Hash>,
    right: &BTreeMap<String, Hash>,
) -> Result<BTreeMap<String, Hash>, &'static str> {
    let mut merged = BTreeMap::new();
    let keys: BTreeSet<_> = base.keys().chain(left.keys()).chain(right.keys()).collect();
    for k in keys {
        let (b, l, r) = (base.get(k), left.get(k), right.get(k));
        let selected = if l == r || r == b {
            l
        } else if l == b {
            r
        } else {
            return Err("Binding conflict");
        };
        if let Some(h) = selected {
            merged.insert(k.clone(), h.clone());
        }
    }
    Ok(merged)
}

// Candidate discovery only: an asset may still occur in an unchanged subtree.
fn changed_page_inputs(
    cas: &Cas,
    old: &str,
    new: &str,
    reads: &mut usize,
) -> Result<(BTreeSet<Hash>, BTreeSet<Hash>), String> {
    if old == new {
        return Ok((BTreeSet::new(), BTreeSet::new()));
    }
    *reads += 2;
    match (cas.read("node-v1", old)?, cas.read("node-v1", new)?) {
        (Node::Bindings(a), Node::Bindings(b)) => {
            Ok((a.into_values().collect(), b.into_values().collect()))
        }
        (Node::Children(a), Node::Children(b)) if a.keys().eq(b.keys()) => {
            let (mut removed, mut added) = (BTreeSet::new(), BTreeSet::new());
            for (id, h) in a {
                let (x, y) = changed_page_inputs(cas, &h, &b[&id], reads)?;
                removed.extend(x);
                added.extend(y);
            }
            Ok((removed, added))
        }
        _ => {
            Err("Model requires matching scene topology; production needs budgeted fallback".into())
        }
    }
}
// This models prerequisites only. Production must pin handles, refresh identities,
// honor all protections and verify physical custody using the existing engine.
fn removal_allowed(
    demand: bool,
    complete: bool,
    planned_revision: u64,
    now_revision: u64,
    source_failure_domain: &str,
    retained_failure_domain: &str,
    retained_verified: bool,
) -> bool {
    !demand
        && complete
        && !source_failure_domain.trim().is_empty()
        && !retained_failure_domain.trim().is_empty()
        && planned_revision == now_revision
        && source_failure_domain != retained_failure_domain
        && retained_verified
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Claim {
    subject: Hash,
    bytes: u64,
    location: String,
    observed: u64,
    payload_verified: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceIndex {
    schema: String,
    project: String,
    authority_namespace: String,
    config_hash: Hash,
    complete: bool,
    shards: BTreeMap<String, Hash>,
}
#[derive(Clone, PartialEq, Eq)]
struct Descriptor {
    project: String,
    authority_namespace: String,
    config_hash: Hash,
    source: Hash,
    adapter_version: u32,
}
struct EvidenceAdapter<'a> {
    cas: &'a Cas,
    descriptor: Descriptor,
}
struct Lookup {
    claims: Vec<Claim>,
    complete: bool,
    reads: usize,
    provenance: Descriptor,
}
#[derive(Debug, PartialEq, Eq)]
enum CapturedMembership {
    Present,
    AbsentAtCapture,
    Unknown,
}
impl Lookup {
    fn membership(&self) -> CapturedMembership {
        if !self.claims.is_empty() {
            CapturedMembership::Present
        } else if self.complete {
            CapturedMembership::AbsentAtCapture
        } else {
            CapturedMembership::Unknown
        }
    }
}
#[derive(Clone, Copy)]
struct LookupLimits {
    node_bytes: usize,
    total_bytes: usize,
    records: usize,
    nodes: usize,
}
struct Meter {
    limits: LookupLimits,
    bytes: usize,
    nodes: usize,
}
impl Meter {
    fn read<T: DeserializeOwned>(
        &mut self,
        cas: &Cas,
        domain: &str,
        hash: &str,
    ) -> Result<T, String> {
        let size = cas.0.get(hash).ok_or("Missing node")?.len();
        let next = self
            .bytes
            .checked_add(size)
            .ok_or("Byte counter overflow")?;
        if size > self.limits.node_bytes
            || next > self.limits.total_bytes
            || self.nodes >= self.limits.nodes
        {
            return Err("Lookup resource budget".into());
        }
        self.bytes = next;
        self.nodes += 1;
        cas.read(domain, hash)
    }
}
impl EvidenceAdapter<'_> {
    fn lookup(&self, subject: &str, budget: usize) -> Result<Lookup, String> {
        self.lookup_limited(
            subject,
            LookupLimits {
                node_bytes: 1024 * 1024,
                total_bytes: 2 * 1024 * 1024,
                records: 1000,
                nodes: budget,
            },
        )
    }
    fn lookup_limited(&self, subject: &str, limits: LookupLimits) -> Result<Lookup, String> {
        if limits.nodes < 1
            || self.descriptor.adapter_version != 1
            || !reel_assets::valid_oid(subject)
        {
            return Err("Unsupported adapter or budget/subject".into());
        }
        let mut meter = Meter {
            limits,
            bytes: 0,
            nodes: 0,
        };
        let index: EvidenceIndex =
            meter.read(self.cas, "evidence-index-v1", &self.descriptor.source)?;
        if index.schema != "synthetic.evidence-index.v1" {
            return Err("Unsupported evidence index schema".into());
        }
        if index.project != self.descriptor.project {
            return Err("Evidence project scope mismatch".into());
        }
        if index.authority_namespace != self.descriptor.authority_namespace
            || index.config_hash != self.descriptor.config_hash
        {
            return Err("Evidence authority/config mismatch; prepare a matching index".into());
        }
        let mut claims = Vec::new();
        let mut reads = 1;
        if let Some(h) = index.shards.get(&subject[..2]) {
            let shard: Vec<Claim> = meter.read(self.cas, "evidence-shard-v1", h)?;
            if shard.len() > limits.records {
                return Err("Lookup record budget".into());
            }
            reads += 1;
            claims.extend(shard.into_iter().filter(|c| c.subject == subject));
        }
        Ok(Lookup {
            claims,
            complete: index.complete,
            reads,
            provenance: self.descriptor.clone(),
        })
    }
}
fn evidence(cas: &mut Cas, rows: Vec<Claim>, complete: bool) -> Descriptor {
    let mut groups: BTreeMap<String, Vec<Claim>> = BTreeMap::new();
    for row in rows {
        groups.entry(row.subject[..2].into()).or_default().push(row);
    }
    let shards = groups
        .into_iter()
        .map(|(k, v)| (k, cas.put("evidence-shard-v1", &v)))
        .collect();
    Descriptor {
        project: "synthetic-project".into(),
        authority_namespace: "synthetic-authority".into(),
        config_hash: asset("default-adapter-config"),
        source: cas.put(
            "evidence-index-v1",
            &EvidenceIndex {
                schema: "synthetic.evidence-index.v1".into(),
                project: "synthetic-project".into(),
                authority_namespace: "synthetic-authority".into(),
                config_hash: asset("default-adapter-config"),
                complete,
                shards,
            },
        ),
        adapter_version: 1,
    }
}
fn claim(i: usize, verified: bool) -> Claim {
    Claim {
        subject: asset(&format!("evidence-{i}")),
        bytes: 100,
        location: "archive".into(),
        observed: 1,
        payload_verified: verified,
    }
}

#[test]
fn thirteen_of_one_thousand_bindings_replace_only_two_leaves_and_root() {
    let mut cas = Cas::default();
    let mut scenes = episode();
    let old = snapshot(&mut cas, &scenes);
    let before = cas.0.len();
    // Ten cels and three cues, spread across two scenes; IDs are stable.
    for i in 0..13 {
        scenes[i % 2].insert(
            format!("binding-{:04}", i / 2),
            asset(&format!("replacement-{i}")),
        );
    }
    let new = snapshot(&mut cas, &scenes);
    assert_eq!(cas.0.len() - before, 3);
    let a = inputs(&cas, &old, &mut 11).unwrap();
    let b = inputs(&cas, &new, &mut 11).unwrap();
    assert_eq!((a.len(), b.len()), (1000, 1000));
    assert_eq!(a.difference(&b).count(), 13);
    assert_eq!(b.difference(&a).count(), 13);
    let mut reads = 0;
    let (before, after) = changed_page_inputs(&cas, &old, &new, &mut reads).unwrap();
    assert_eq!(reads, 6); // two roots and two changed leaves in each snapshot
    assert_eq!(before.difference(&after).count(), 13);
    // The new snapshot is readable even with the old root removed: no delta chain.
    cas.0.remove(&old);
    assert_eq!(inputs(&cas, &new, &mut 11).unwrap(), b);
}
#[test]
fn stable_id_insertion_does_not_shift_other_scenes() {
    let mut cas = Cas::default();
    let mut scenes = episode();
    snapshot(&mut cas, &scenes);
    let before = cas.0.len();
    scenes[0].insert("binding-0000-extra".into(), asset("inserted"));
    let new = snapshot(&mut cas, &scenes);
    assert_eq!(cas.0.len() - before, 2);
    assert_eq!(inputs(&cas, &new, &mut 11).unwrap().len(), 1001);
}
#[test]
fn missing_corrupt_or_over_budget_graph_is_not_an_empty_closure() {
    let mut cas = Cas::default();
    let r = snapshot(&mut cas, &episode());
    assert!(inputs(&cas, &r, &mut 1).is_err());
    let bytes = cas.0.remove(&r).unwrap();
    assert!(inputs(&cas, &r, &mut 11).is_err());
    cas.0.insert(r.clone(), [bytes, vec![0]].concat());
    assert!(inputs(&cas, &r, &mut 11).is_err());
}
#[test]
fn five_replaced_cels_leave_one_shared_with_a_draft_and_only_four_candidates() {
    let old: BTreeSet<_> = (0..5).map(|i| asset(&format!("old-{i}"))).collect();
    let shared = old.first().unwrap();
    let mut roots = BTreeMap::from([
        ("episode/golden".into(), root(&[], &["episode-hot"])),
        ("other/draft".into(), root(&[shared], &["episode-hot"])),
    ]);
    roots.get_mut("episode/golden").unwrap().inputs =
        (0..5).map(|i| asset(&format!("new-{i}"))).collect();
    assert_eq!(
        old.iter()
            .filter(|h| users(&roots, h, "episode-hot").is_empty())
            .count(),
        4
    );
    assert_eq!(
        users(&roots, shared, "episode-hot"),
        BTreeSet::from(["other/draft"])
    );
}
#[test]
fn shared_input_requires_both_affinities_but_history_is_not_a_hot_root() {
    let mut episode = root(&["shared"], &["episode-hot"]);
    episode.references.insert("old-movie".into());
    let roots = BTreeMap::from([
        ("episode".into(), episode),
        ("song".into(), root(&["shared"], &["music-hot"])),
    ]);
    assert_eq!(users(&roots, "shared", "episode-hot").len(), 1);
    assert_eq!(users(&roots, "shared", "music-hot").len(), 1);
    assert!(users(&roots, "old-movie", "episode-hot").is_empty());
}
#[test]
fn draft_without_golden_and_overdue_draft_stay_protected_until_explicit_transition() {
    let mut draft = root(&["cel"], &["hot"]);
    draft.outputs.insert("preview".into());
    let mut roots = BTreeMap::from([("draft/owner".into(), draft.clone())]);
    // Passage of time has no mutation API: review due is not release authority.
    assert_eq!(users(&roots, "preview", "hot").len(), 1);
    roots.insert("golden".into(), draft);
    roots.remove("draft/owner");
    assert_eq!(users(&roots, "cel", "hot"), BTreeSet::from(["golden"]));
    assert_eq!(users(&roots, "preview", "hot").len(), 1);
}
#[test]
fn independent_keys_admit_but_same_key_conflict_is_atomic() {
    let mut heads = Heads::default();
    heads.admit(&[("episode", 0, 1)]).unwrap();
    heads.admit(&[("song", 0, 1)]).unwrap();
    let before = heads.0.clone();
    assert!(heads.admit(&[("draft", 0, 1), ("episode", 0, 2)]).is_err());
    assert_eq!(heads.0, before);
    assert!(heads.admit(&[("draft", 0, 1), ("draft", 0, 2)]).is_err());
    assert_eq!(heads.0, before);
}

#[test]
fn disjoint_edits_inside_one_chunk_merge_by_binding_id_but_competing_edits_conflict() {
    let base = BTreeMap::from([
        ("cel".into(), asset("old-cel")),
        ("cue".into(), asset("old-cue")),
    ]);
    let (mut left, mut right) = (base.clone(), base.clone());
    left.insert("cel".into(), asset("new-cel"));
    right.insert("cue".into(), asset("new-cue"));
    let merged = merge_bindings(&base, &left, &right).unwrap();
    assert_eq!(merged["cel"], left["cel"]);
    assert_eq!(merged["cue"], right["cue"]);
    right.insert("cel".into(), asset("competing-cel"));
    assert!(merge_bindings(&base, &left, &right).is_err());
}

#[test]
fn local_diff_candidate_still_used_by_unchanged_subtree_is_not_zero_reference() {
    let mut cas = Cas::default();
    let mut scenes = episode();
    let shared = scenes[0]["binding-0000"].clone();
    scenes[1].insert("shared".into(), shared.clone());
    let old = snapshot(&mut cas, &scenes);
    scenes[0].insert("binding-0000".into(), asset("replacement"));
    let new = snapshot(&mut cas, &scenes);
    let (before, after) = changed_page_inputs(&cas, &old, &new, &mut 0).unwrap();
    assert!(before.difference(&after).any(|h| h == &shared));
    let live = inputs(&cas, &new, &mut 11).unwrap();
    assert!(live.contains(&shared));
    assert!(!before.difference(&after).any(|h| !live.contains(h)));
}
#[test]
fn stale_plan_unknown_coverage_or_same_volume_alias_cannot_authorize_removal() {
    assert!(removal_allowed(
        false, true, 1, 1, "volume-a", "volume-b", true
    ));
    assert!(!removal_allowed(
        false, true, 1, 2, "volume-a", "volume-b", true
    ));
    assert!(!removal_allowed(
        false, false, 1, 1, "volume-a", "volume-b", true
    ));
    assert!(!removal_allowed(
        false, true, 1, 1, "volume-a", "volume-a", true
    ));
    assert!(!removal_allowed(
        true, true, 1, 1, "volume-a", "volume-b", true
    ));
    assert!(!removal_allowed(
        false, true, 1, 1, "volume-a", "volume-b", false
    ));
}
#[test]
fn adapter_looks_up_one_of_one_thousand_records_with_two_authenticated_node_reads() {
    let mut cas = Cas::default();
    let descriptor = evidence(
        &mut cas,
        (0..1000).map(|i| claim(i, false)).collect(),
        false,
    );
    let adapter = EvidenceAdapter {
        cas: &cas,
        descriptor: descriptor.clone(),
    };
    let result = adapter.lookup(&claim(42, false).subject, 2).unwrap();
    assert_eq!(result.claims, vec![claim(42, false)]);
    assert_eq!(result.reads, 2);
    assert!(!result.complete);
    assert!(result.provenance == descriptor);
    assert!(adapter.lookup(&claim(42, false).subject, 1).is_err());
}
#[test]
fn adapter_rejects_tampered_shard_instead_of_returning_trusted_claims() {
    let mut cas = Cas::default();
    let descriptor = evidence(&mut cas, vec![claim(0, true)], true);
    let index: EvidenceIndex = cas.read("evidence-index-v1", &descriptor.source).unwrap();
    cas.0
        .get_mut(index.shards.values().next().unwrap())
        .unwrap()
        .push(0);
    assert!(
        EvidenceAdapter {
            cas: &cas,
            descriptor
        }
        .lookup(&claim(0, true).subject, 2)
        .is_err()
    );
}
#[test]
fn contradictory_sources_remain_separate_and_partial_absence_is_unknown() {
    let mut cas = Cas::default();
    let a = evidence(&mut cas, vec![claim(0, true)], true);
    let mut newer = claim(0, false);
    newer.bytes = 200;
    newer.observed = 2;
    let b = evidence(&mut cas, vec![newer], false);
    let left = EvidenceAdapter {
        cas: &cas,
        descriptor: a,
    }
    .lookup(&claim(0, true).subject, 2)
    .unwrap();
    let right_adapter = EvidenceAdapter {
        cas: &cas,
        descriptor: b,
    };
    let right = right_adapter.lookup(&claim(0, true).subject, 2).unwrap();
    assert_ne!(left.claims[0].bytes, right.claims[0].bytes);
    assert!(left.claims[0].payload_verified);
    assert!(!right.claims[0].payload_verified);
    let missing = right_adapter.lookup(&asset("not-observed"), 2).unwrap();
    assert!(missing.claims.is_empty());
    assert!(!missing.complete);
}
#[test]
fn evidence_content_or_adapter_revision_invalidates_a_derived_result_key() {
    let mut cas = Cas::default();
    let a = evidence(&mut cas, vec![claim(0, true)], true);
    let b = evidence(&mut cas, vec![claim(1, true)], true);
    assert!(a != b);
    let changed_adapter = Descriptor {
        adapter_version: 2,
        ..a.clone()
    };
    assert!(a != changed_adapter);
    assert!(
        EvidenceAdapter {
            cas: &cas,
            descriptor: changed_adapter
        }
        .lookup(&claim(0, true).subject, 2)
        .is_err()
    );
}

#[test]
fn timing_only_authoring_change_creates_a_new_snapshot_without_recopying_inputs() {
    let mut cas = Cas::default();
    let bindings = snapshot(&mut cas, &episode());
    let before = cas.0.len();
    // Synthetic authoring envelope, not an alternative native scene grammar.
    let a = serde_json::json!({"bindings":bindings,"cue_start_ticks":100,"ticks_per_second":48000});
    let b = serde_json::json!({"bindings":bindings,"cue_start_ticks":101,"ticks_per_second":48000});
    assert_ne!(
        cas.put("synthetic-authoring-v1", &a),
        cas.put("synthetic-authoring-v1", &b)
    );
    assert_eq!(cas.0.len() - before, 2);
    assert_eq!(inputs(&cas, &bindings, &mut 11).unwrap().len(), 1000);
}

#[test]
fn valid_hashed_evidence_from_a_foreign_project_is_rejected() {
    let mut cas = Cas::default();
    let mut descriptor = evidence(&mut cas, vec![claim(0, true)], true);
    descriptor.project = "other-project".into();
    let result = EvidenceAdapter {
        cas: &cas,
        descriptor,
    }
    .lookup(&claim(0, true).subject, 2);
    assert!(matches!(result, Err(message) if message == "Evidence project scope mismatch"));
}

#[test]
fn shareable_summary_does_not_serialize_private_claim_graph() {
    let mut cas = Cas::default();
    let mut row = claim(0, true);
    row.location = "SYNTHETIC_PRIVATE_LOCATION".into();
    let descriptor = evidence(&mut cas, vec![row.clone()], true);
    let result = EvidenceAdapter {
        cas: &cas,
        descriptor: descriptor.clone(),
    }
    .lookup(&row.subject, 2)
    .unwrap();
    // Allowlisted projection: the private graph is not Serialize input here.
    let shared =
        serde_json::json!({"schema":"synthetic-summary-v1","claim_count":result.claims.len()})
            .to_string();
    for private in [&row.location, &row.subject, &descriptor.source] {
        assert!(!shared.contains(private));
    }
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&shared).unwrap()["claim_count"],
        1
    );
}

#[test]
fn inserting_a_named_scene_reuses_all_existing_scene_nodes() {
    let mut cas = Cas::default();
    let mut scenes: BTreeMap<_, _> = episode()
        .into_iter()
        .enumerate()
        .map(|(i, rows)| (format!("scene-{i:02}"), rows))
        .collect();
    snapshot_named(&mut cas, &scenes);
    let before = cas.0.len();
    scenes.insert(
        "scene-00-inserted".into(),
        BTreeMap::from([("cel".into(), asset("inserted-scene"))]),
    );
    let new = snapshot_named(&mut cas, &scenes);
    assert_eq!(cas.0.len() - before, 2); // new scene and root, no shifted IDs
    assert_eq!(inputs(&cas, &new, &mut 12).unwrap().len(), 1001);
}

#[test]
fn selection_revisions_cannot_roll_back_and_accept_an_aba_writer() {
    let mut heads = Heads::default();
    heads.admit(&[("episode", 0, 1)]).unwrap();
    heads.admit(&[("episode", 1, 2)]).unwrap();
    assert!(heads.admit(&[("episode", 2, 0)]).is_err());
    assert!(heads.admit(&[("episode", 0, 1)]).is_err());
    assert_eq!(heads.0["episode"], 2);
}

#[test]
fn different_partition_labels_do_not_prove_independent_custody() {
    let labels = ("partition-a", "partition-b");
    assert_ne!(labels.0, labels.1);
    // Both map to the same physical failure domain under this synthetic policy.
    assert!(!removal_allowed(
        false,
        true,
        1,
        1,
        "same-disk",
        "same-disk",
        true
    ));
}

#[test]
fn every_model_node_address_is_the_standard_hash_of_its_stored_frame() {
    let mut cas = Cas::default();
    snapshot(&mut cas, &episode());
    evidence(&mut cas, vec![claim(0, true)], false);
    for (address, bytes) in &cas.0 {
        assert_eq!(*address, format!("{:x}", Sha256::digest(bytes)));
    }
}

#[test]
fn identical_json_under_another_node_domain_is_not_interchangeable() {
    let mut cas = Cas::default();
    let data = BTreeMap::from([("same", 1)]);
    let a = cas.put("domain-a", &data);
    let b = cas.put("domain-b", &data);
    assert_ne!(a, b);
    assert!(cas.read::<BTreeMap<String, u64>>("domain-b", &a).is_err());
}

#[test]
fn byte_budgets_charge_root_and_shard_before_decoding_or_emitting_claims() {
    let mut cas = Cas::default();
    let descriptor = evidence(&mut cas, vec![claim(0, true)], true);
    let index: EvidenceIndex = cas.read("evidence-index-v1", &descriptor.source).unwrap();
    let root_size = cas.0[&descriptor.source].len();
    let shard_size = cas.0[index.shards.values().next().unwrap()].len();
    let adapter = EvidenceAdapter {
        cas: &cas,
        descriptor,
    };
    let exact = LookupLimits {
        node_bytes: root_size.max(shard_size),
        total_bytes: root_size + shard_size,
        records: 1,
        nodes: 2,
    };
    assert!(
        adapter
            .lookup_limited(&claim(0, true).subject, exact)
            .is_ok()
    );
    assert!(
        adapter
            .lookup_limited(
                &claim(0, true).subject,
                LookupLimits {
                    total_bytes: exact.total_bytes - 1,
                    ..exact
                }
            )
            .is_err()
    );
    assert!(
        adapter
            .lookup_limited(
                &claim(0, true).subject,
                LookupLimits {
                    node_bytes: 1,
                    ..exact
                }
            )
            .is_err()
    );
}

#[test]
fn record_budget_charges_all_decoded_rows_not_only_matching_subjects() {
    let mut cas = Cas::default();
    let a = claim(0, true);
    let mut b = claim(1, true);
    b.subject = format!("{}{}", &a.subject[..2], &b.subject[2..]);
    let descriptor = evidence(&mut cas, vec![a.clone(), b], true);
    let adapter = EvidenceAdapter {
        cas: &cas,
        descriptor,
    };
    let limits = LookupLimits {
        node_bytes: 65536,
        total_bytes: 131072,
        records: 1,
        nodes: 2,
    };
    assert!(adapter.lookup_limited(&a.subject, limits).is_err());
    assert_eq!(
        adapter
            .lookup_limited(
                &a.subject,
                LookupLimits {
                    records: 2,
                    ..limits
                }
            )
            .unwrap()
            .claims
            .len(),
        1
    );
}

#[test]
fn invalid_unicode_short_or_uppercase_subjects_are_errors_without_slicing_panics() {
    let mut cas = Cas::default();
    let descriptor = evidence(&mut cas, vec![claim(0, true)], false);
    let adapter = EvidenceAdapter {
        cas: &cas,
        descriptor,
    };
    for invalid in ["अ", "x", "", &"A".repeat(64)] {
        assert!(adapter.lookup(invalid, 2).is_err());
    }
}

/// Second scenario batch: design exploration only, not production I/O tests.
mod additional_scenarios {
    use super::*;

    fn bindings() -> BTreeMap<String, Hash> {
        BTreeMap::from([("cel".into(), asset("cel")), ("cue".into(), asset("cue"))])
    }
    fn named_scenes() -> BTreeMap<String, BTreeMap<String, Hash>> {
        episode()
            .into_iter()
            .enumerate()
            .map(|(i, rows)| (format!("scene-{i:02}"), rows))
            .collect()
    }

    #[test]
    fn identical_republication_reuses_all_nodes() {
        let mut cas = Cas::default();
        let scenes = episode();
        let a = snapshot(&mut cas, &scenes);
        let count = cas.0.len();
        assert_eq!(snapshot(&mut cas, &scenes), a);
        assert_eq!(cas.0.len(), count);
    }
    #[test]
    fn binding_insertion_order_does_not_change_sorted_map_identity() {
        let mut cas = Cas::default();
        let a = bindings();
        let b: BTreeMap<_, _> = a
            .iter()
            .rev()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        assert_eq!(
            cas.put("node-v1", &Node::Bindings(a)),
            cas.put("node-v1", &Node::Bindings(b))
        );
    }
    #[test]
    fn moving_a_binding_between_scenes_preserves_whole_episode_membership() {
        let mut cas = Cas::default();
        let mut scenes = episode();
        let a = snapshot(&mut cas, &scenes);
        let moved = scenes[0].remove("binding-0000").unwrap();
        scenes[1].insert("moved-use-site".into(), moved);
        let b = snapshot(&mut cas, &scenes);
        assert_ne!(a, b);
        assert_eq!(
            inputs(&cas, &a, &mut 11).unwrap(),
            inputs(&cas, &b, &mut 11).unwrap()
        );
    }
    #[test]
    fn removing_a_scene_writes_only_a_root_and_preserves_the_old_snapshot() {
        let mut cas = Cas::default();
        let mut scenes = named_scenes();
        let old = snapshot_named(&mut cas, &scenes);
        let before = cas.0.len();
        scenes.remove("scene-00");
        let new = snapshot_named(&mut cas, &scenes);
        assert_eq!(cas.0.len() - before, 1);
        assert_eq!(inputs(&cas, &new, &mut 10).unwrap().len(), 900);
        assert_eq!(inputs(&cas, &old, &mut 11).unwrap().len(), 1000);
    }
    #[test]
    fn topology_change_is_an_explicit_diff_gap_not_an_empty_removal_set() {
        let mut cas = Cas::default();
        let mut scenes = named_scenes();
        let a = snapshot_named(&mut cas, &scenes);
        scenes.remove("scene-00");
        let b = snapshot_named(&mut cas, &scenes);
        assert!(changed_page_inputs(&cas, &a, &b, &mut 0).is_err());
    }
    #[test]
    fn deletion_merges_with_an_unchanged_peer() {
        let base = bindings();
        let mut left = base.clone();
        left.remove("cel");
        assert_eq!(merge_bindings(&base, &left, &base).unwrap(), left);
    }
    #[test]
    fn deletion_conflicts_with_an_edit_to_the_same_binding() {
        let base = bindings();
        let (mut left, mut right) = (base.clone(), base.clone());
        left.remove("cel");
        right.insert("cel".into(), asset("changed"));
        assert!(merge_bindings(&base, &left, &right).is_err());
    }
    #[test]
    fn identical_concurrent_edits_merge_without_a_false_conflict() {
        let base = bindings();
        let mut edited = base.clone();
        edited.insert("cel".into(), asset("same-change"));
        assert_eq!(merge_bindings(&base, &edited, &edited).unwrap(), edited);
    }
    #[test]
    fn disjoint_added_binding_ids_merge() {
        let base = bindings();
        let (mut left, mut right) = (base.clone(), base.clone());
        left.insert("new-cel".into(), asset("new-cel"));
        right.insert("new-cue".into(), asset("new-cue"));
        let merged = merge_bindings(&base, &left, &right).unwrap();
        assert_eq!(merged.len(), 4);
        assert_eq!(merged["new-cel"], left["new-cel"]);
        assert_eq!(merged["new-cue"], right["new-cue"]);
    }
    #[test]
    fn competing_additions_at_the_same_id_require_resolution() {
        let base = bindings();
        let (mut left, mut right) = (base.clone(), base.clone());
        left.insert("new".into(), asset("a"));
        right.insert("new".into(), asset("b"));
        assert!(merge_bindings(&base, &left, &right).is_err());
    }
    #[test]
    fn releasing_one_draft_preserves_another_and_only_last_release_removes_demand() {
        let mut roots = BTreeMap::from([
            ("draft/a".into(), root(&["cel"], &["hot"])),
            ("draft/b".into(), root(&["cel"], &["hot"])),
        ]);
        roots.remove("draft/a");
        assert_eq!(users(&roots, "cel", "hot"), BTreeSet::from(["draft/b"]));
        roots.remove("draft/b");
        assert!(users(&roots, "cel", "hot").is_empty()); // not removal permission
    }
    #[test]
    fn abandoning_a_draft_does_not_release_a_golden_shared_input() {
        let mut roots = BTreeMap::from([
            ("draft".into(), root(&["cel", "candidate"], &["hot"])),
            ("golden".into(), root(&["cel"], &["hot"])),
        ]);
        roots.remove("draft");
        assert_eq!(users(&roots, "cel", "hot"), BTreeSet::from(["golden"]));
        assert!(users(&roots, "candidate", "hot").is_empty());
    }
    #[test]
    fn delivered_variants_have_separate_output_demand_and_shared_inputs() {
        let mut clean = root(&["cel"], &["hot"]);
        clean.outputs.insert("clean-movie".into());
        let mut captioned = root(&["cel"], &["hot"]);
        captioned.outputs.insert("captioned-movie".into());
        let mut roots = BTreeMap::from([("clean".into(), clean), ("captioned".into(), captioned)]);
        roots.remove("captioned");
        assert_eq!(users(&roots, "clean-movie", "hot").len(), 1);
        assert_eq!(users(&roots, "cel", "hot").len(), 1);
        assert!(users(&roots, "captioned-movie", "hot").is_empty());
    }
    #[test]
    fn job_to_draft_handoff_overlaps_protection() {
        let mut protection = root(&["input"], &["hot"]);
        protection.outputs.insert("preview".into());
        let mut roots = BTreeMap::from([("job".into(), protection.clone())]);
        assert_eq!(users(&roots, "preview", "hot").len(), 1);
        roots.insert("draft".into(), protection);
        assert_eq!(users(&roots, "preview", "hot").len(), 2);
        roots.remove("job");
        assert_eq!(users(&roots, "preview", "hot"), BTreeSet::from(["draft"]));
    }
    #[test]
    fn checkpoint_roundtrip_preserves_revisions_and_rejects_stale_creation() {
        let mut heads = Heads::default();
        heads.admit(&[("draft", 0, 1)]).unwrap();
        let checkpoint = serde_json::to_vec(&heads).unwrap();
        let mut restored: Heads = serde_json::from_slice(&checkpoint).unwrap();
        assert!(restored.admit(&[("draft", 0, 1)]).is_err());
        restored.admit(&[("draft", 1, 2)]).unwrap();
        assert_eq!(restored.0["draft"], 2);
    }
    #[test]
    fn revision_overflow_cannot_partially_apply_an_independent_key() {
        let mut heads = Heads::default();
        heads.0.insert("exhausted".into(), u64::MAX);
        let before = heads.0.clone();
        assert!(
            heads
                .admit(&[("new", 0, 1), ("exhausted", u64::MAX, 0)])
                .is_err()
        );
        assert_eq!(heads.0, before);
    }
    #[test]
    fn unknown_failure_domain_never_counts_as_an_independent_backup() {
        for unknown in ["", " "] {
            assert!(!removal_allowed(false, true, 1, 1, unknown, "known", true));
            assert!(!removal_allowed(false, true, 1, 1, "known", unknown, true));
        }
    }
    #[test]
    fn complete_inventory_absence_is_only_absence_from_the_captured_membership() {
        let mut cas = Cas::default();
        let descriptor = evidence(&mut cas, vec![], true);
        let result = EvidenceAdapter {
            cas: &cas,
            descriptor,
        }
        .lookup(&asset("absent"), 2)
        .unwrap();
        assert_eq!(result.membership(), CapturedMembership::AbsentAtCapture);
        let partial = evidence(&mut cas, vec![], false);
        assert_eq!(
            EvidenceAdapter {
                cas: &cas,
                descriptor: partial
            }
            .lookup(&asset("absent"), 2)
            .unwrap()
            .membership(),
            CapturedMembership::Unknown
        );
    }
    #[test]
    fn same_project_different_authority_namespace_cannot_reuse_an_index() {
        let mut cas = Cas::default();
        let mut descriptor = evidence(&mut cas, vec![claim(0, true)], true);
        descriptor.authority_namespace = "different-authority".into();
        assert!(
            EvidenceAdapter {
                cas: &cas,
                descriptor
            }
            .lookup(&claim(0, true).subject, 2)
            .is_err()
        );
    }
    #[test]
    fn adapter_configuration_change_requires_a_new_prepared_index() {
        let mut cas = Cas::default();
        let mut descriptor = evidence(&mut cas, vec![claim(0, true)], true);
        descriptor.config_hash = asset("new-interpretation");
        assert!(
            EvidenceAdapter {
                cas: &cas,
                descriptor
            }
            .lookup(&claim(0, true).subject, 2)
            .is_err()
        );
    }
    #[test]
    fn an_unknown_index_schema_is_not_silently_guessed() {
        let mut cas = Cas::default();
        let mut descriptor = evidence(&mut cas, vec![claim(0, true)], true);
        let mut index: EvidenceIndex = cas.read("evidence-index-v1", &descriptor.source).unwrap();
        index.schema = "synthetic.evidence-index.v999".into();
        descriptor.source = cas.put("evidence-index-v1", &index);
        assert!(
            matches!(EvidenceAdapter { cas: &cas, descriptor }.lookup(&claim(0, true).subject, 2), Err(e) if e == "Unsupported evidence index schema")
        );
    }
    #[test]
    fn unrelated_corrupt_shard_is_not_scanned_but_its_own_lookup_fails() {
        let mut cas = Cas::default();
        let a = claim(0, true);
        let mut b = claim(1, true);
        b.subject = format!(
            "{}{}",
            if a.subject.starts_with("00") {
                "ff"
            } else {
                "00"
            },
            &b.subject[2..]
        );
        let descriptor = evidence(&mut cas, vec![a.clone(), b.clone()], false);
        let index: EvidenceIndex = cas.read("evidence-index-v1", &descriptor.source).unwrap();
        cas.0
            .get_mut(&index.shards[&b.subject[..2]])
            .unwrap()
            .push(0);
        let adapter = EvidenceAdapter {
            cas: &cas,
            descriptor,
        };
        let found = adapter.lookup(&a.subject, 2).unwrap();
        assert_eq!(found.reads, 2);
        assert_eq!(found.claims, vec![a]);
        assert!(adapter.lookup(&b.subject, 2).is_err());
    }
    #[test]
    fn a_missing_referenced_shard_is_an_error_not_an_empty_success() {
        let mut cas = Cas::default();
        let descriptor = evidence(&mut cas, vec![claim(0, true)], true);
        let index: EvidenceIndex = cas.read("evidence-index-v1", &descriptor.source).unwrap();
        cas.0.remove(index.shards.values().next().unwrap());
        assert!(
            EvidenceAdapter {
                cas: &cas,
                descriptor
            }
            .lookup(&claim(0, true).subject, 2)
            .is_err()
        );
    }
    #[test]
    fn zero_record_budget_cannot_emit_a_present_claim() {
        let mut cas = Cas::default();
        let descriptor = evidence(&mut cas, vec![claim(0, true)], true);
        let adapter = EvidenceAdapter {
            cas: &cas,
            descriptor,
        };
        let limits = LookupLimits {
            node_bytes: 65536,
            total_bytes: 131072,
            records: 0,
            nodes: 2,
        };
        assert!(
            adapter
                .lookup_limited(&claim(0, true).subject, limits)
                .is_err()
        );
        assert_eq!(
            adapter
                .lookup(&claim(0, true).subject, 2)
                .unwrap()
                .membership(),
            CapturedMembership::Present
        );
    }
    #[test]
    fn byte_counter_overflow_is_an_error_without_wrapping() {
        let mut cas = Cas::default();
        let h = cas.put("test", &1);
        let mut meter = Meter {
            limits: LookupLimits {
                node_bytes: usize::MAX,
                total_bytes: usize::MAX,
                records: usize::MAX,
                nodes: usize::MAX,
            },
            bytes: usize::MAX,
            nodes: 0,
        };
        assert!(meter.read::<u64>(&cas, "test", &h).is_err());
        assert_eq!(meter.bytes, usize::MAX);
        assert_eq!(meter.nodes, 0);
    }
}
