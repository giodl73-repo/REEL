//! Private, offline block review. Browser decisions never execute cache changes.
use crate::{Result, demotion, tiering, versions};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::Path,
};

#[derive(Serialize)]
struct Block {
    id: String,
    class: String,
    owner: String,
    observed_tiers: Vec<String>,
    proposal: String,
    target_tier: Option<String>,
    protected: bool,
    eligible: bool,
    disposition_reason: String,
    member_preview: Vec<String>,
    objects: usize,
    logical_bytes: u64,
    observed_copy_bytes: BTreeMap<String, u64>,
    estimated_reclaim_bytes: Option<u64>,
    #[serde(skip_serializing)]
    members: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    schema: String,
    review_sha256: String,
    plan_sha256: String,
    action: String,
    selected_blocks: BTreeSet<String>,
}

struct Review {
    document: Value,
    blocks: Vec<Block>,
    scope: Value,
    policy: tiering::Policy,
}

fn build(root: &Path, scope_path: &Path, now: u64) -> Result<Review> {
    let bytes = tiering::read_bounded(scope_path, 256 * 1024)?;
    let req: demotion::Request = serde_json::from_slice(&bytes)?;
    demotion::check_source_controls(root, &req, false)?;
    if req.schema != "reel.asset-demotion-request.v1"
        || req.max_seconds == 0
        || req.max_seconds > 3600
    {
        return Err("Invalid review consumer scope/time budget".into());
    }
    let fresh = tiering::check_plan(root, &req.policy, &req.plan, now)?;
    if fresh["plan_sha256"] != req.plan_sha256 {
        return Err("Review scope names a different plan".into());
    }
    let policy: tiering::Policy =
        serde_json::from_slice(&tiering::read_bounded(&req.policy, 256 * 1024)?)?;
    let plan: Value =
        serde_json::from_slice(&tiering::read_bounded(&req.plan, 512 * 1024 * 1024)?)?;
    let protections = demotion::export_protections(root, scope_path)?;
    let protected = protections["protected_hashes"]
        .as_object()
        .ok_or("Missing consumer protections")?;
    let state = versions::read(root)?;
    let mut owners = BTreeMap::new();
    for (key, version) in &state.versions {
        for h in version
            .payloads
            .iter()
            .chain(std::iter::once(&version.manifest_sha256))
        {
            owners.entry(h.clone()).or_insert_with(|| key.clone());
        }
    }
    let tiers: BTreeMap<_, _> = policy
        .tiers
        .iter()
        .filter(|t| t.storage == "active")
        .map(|t| (t.index, t.id.clone()))
        .collect();
    let mut grouped: BTreeMap<String, Vec<Block>> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let block_bytes = policy.max_batch_bytes.min(1024 * 1024 * 1024);
    let block_objects = policy.max_batch_objects.min(256);
    let rows = plan["objects"].as_array().ok_or("Missing plan objects")?;
    for row in rows {
        let h = row["sha256"].as_str().ok_or("Missing object hash")?;
        if !crate::valid_oid(h) || !seen.insert(h.to_owned()) {
            return Err("Invalid/duplicate review object".into());
        }
        let size = row["bytes"].as_u64().ok_or("Missing object size")?;
        let class = row["class"]
            .as_str()
            .ok_or("Missing classification")?
            .to_owned();
        let proposal = row["proposal"]
            .as_str()
            .ok_or("Missing proposal")?
            .to_owned();
        let owner = owners
            .get(h)
            .cloned()
            .unwrap_or_else(|| "No registered version reference".into());
        let is_protected = protected.contains_key(h);
        let copies = row["baseline_copies"]
            .as_array()
            .ok_or("Missing physical observations")?;
        let mut locations = BTreeSet::new();
        let mut copy_bytes = BTreeMap::new();
        for copy in copies {
            let n = copy["tier"].as_u64().ok_or("Missing observed tier")? as usize;
            let tier = tiers.get(&n).ok_or("Unknown observed tier")?.clone();
            locations.insert(tier.clone());
            let n = copy["bytes"].as_u64().ok_or("Missing observed bytes")?;
            let old = copy_bytes.insert(tier, n);
            if old.is_some() {
                return Err("Duplicate observed tier".into());
            }
        }
        let eligible = class == "previously-used"
            && proposal == "verify-and-demote"
            && !is_protected
            && size <= policy.max_batch_bytes
            && locations.contains(&policy.selected_tier)
            && locations.contains(&policy.history_tier);
        let disposition_reason = if is_protected {
            "Required or protected by a managed consumer".to_owned()
        } else if size > policy.max_batch_bytes {
            "Object exceeds the configured operation budget".to_owned()
        } else if class == "previously-used" && !locations.contains(&policy.history_tier) {
            "No history-tier copy was observed; establish retained custody first".to_owned()
        } else {
            row["reason"]
                .as_str()
                .unwrap_or("Separate action required")
                .to_owned()
        };
        let key = serde_json::to_string(&json!([
            class,
            owner,
            locations,
            proposal,
            is_protected,
            eligible,
            disposition_reason
        ]))?;
        let chunks = grouped.entry(key).or_default();
        let new_chunk = chunks.last().is_none_or(|b| {
            b.objects >= block_objects
                || b.logical_bytes
                    .checked_add(size)
                    .is_none_or(|n| n > block_bytes)
        });
        if new_chunk {
            chunks.push(Block {
                id: String::new(),
                class,
                owner,
                observed_tiers: locations.into_iter().collect(),
                proposal,
                target_tier: row["target_tier"].as_str().map(str::to_owned),
                protected: is_protected,
                eligible,
                disposition_reason,
                member_preview: vec![],
                objects: 0,
                logical_bytes: 0,
                observed_copy_bytes: BTreeMap::new(),
                estimated_reclaim_bytes: Some(0),
                members: vec![],
            });
        }
        let b = chunks.last_mut().unwrap();
        b.logical_bytes = b
            .logical_bytes
            .checked_add(size)
            .ok_or("Review byte overflow")?;
        for (tier, n) in copy_bytes {
            let value = b.observed_copy_bytes.entry(tier).or_default();
            *value = value.checked_add(n).ok_or("Review copy-byte overflow")?;
        }
        b.estimated_reclaim_bytes = b
            .estimated_reclaim_bytes
            .zip(row["estimated_hot_reclaim_bytes"].as_u64())
            .and_then(|(a, b)| a.checked_add(b));
        b.objects += 1;
        if b.member_preview.len() < 8 {
            b.member_preview.push(h.into());
        }
        b.members.push(h.into());
    }
    let mut blocks: Vec<Block> = grouped.into_values().flatten().collect();
    if blocks.len() > 10_000 {
        return Err("Review exceeds the 10,000-block rendering budget".into());
    }
    for b in &mut blocks {
        b.id = tiering::digest(&serde_json::to_vec(&json!([
            b.class,
            b.owner,
            b.observed_tiers,
            b.proposal,
            b.protected,
            b.disposition_reason,
            b.members
        ]))?);
    }
    let protection_hash = tiering::digest(&serde_json::to_vec(&protections)?);
    let mut document = json!({"schema":"reel.asset-tier-review.v1","project_id":plan["project_id"],
        "plan_sha256":req.plan_sha256,"scope_sha256":tiering::digest(&bytes),"protections_sha256":protection_hash,
        "baseline":plan["baseline"],"baseline_generation":plan["baseline_generation"],
        "census_started_unix_seconds":plan["census_started_unix_seconds"],"census_completed_unix_seconds":plan["census_completed_unix_seconds"],
        "live_generation":plan["live_generation"],"created_unix_seconds":plan["created_unix_seconds"],
        "expires_unix_seconds":plan["expires_unix_seconds"],"inventory_scope":plan["inventory_scope"],
        "tiers":plan["tiers"],"tier_observations":plan["tier_observations"],"summary":plan["summary"],"blocks":blocks,
        "selected_tier":policy.selected_tier,"history_tier":policy.history_tier,"cold_tier":policy.cold_tier,
        "delta":plan["delta"],
        "consumers":protections["consumers"],"max_batch_bytes":policy.max_batch_bytes,
        "max_batch_objects":policy.max_batch_objects,"archive_probed":false,"removal_authorized":false,
        "copy_proof":"Baseline observations only. Every retained copy must be freshly hash/byte verified before removal.",
        "limits":"Only redundant hot copies of proven history with an observed history-tier copy can enter a removal request. Promotions, archive-first custody and last-copy retirement require separate implementations/decisions. Shared hashes are counted once; owner labels show one reference, not exclusive ownership."});
    document["review_sha256"] = json!(tiering::digest(&serde_json::to_vec(&document)?));
    tiering::check_plan(root, &req.policy, &req.plan, now)?;
    demotion::check_source_controls(root, &req, false)?;
    if fs::read(scope_path)? != bytes {
        return Err("Consumer scope changed during review".into());
    }
    Ok(Review {
        document,
        blocks,
        scope: serde_json::from_slice(&bytes)?,
        policy,
    })
}

fn embedded_data(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(value)?
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029"))
}

pub fn render(root: &Path, scope: &Path, output: &Path, now: u64) -> Result<Value> {
    if output.exists() {
        return Err("Review output already exists".into());
    }
    let review = build(root, scope, now)?;
    let data = embedded_data(&review.document)?;
    let html = include_str!("tier_review.html")
        .replacen("__REVIEW_SCRIPT__", include_str!("tier_review.js"), 1)
        .replacen("__REVIEW_DATA__", &data, 1);
    if html.len() > 16 * 1024 * 1024 {
        return Err("Private review exceeds the 16 MiB rendering budget".into());
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let tmp = crate::Temporary(crate::unique(parent, "tier-review"));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp.0)?;
    file.write_all(html.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::hard_link(&tmp.0, output)?;
    Ok(
        json!({"schema":"reel.asset-tier-review-result.v1","review_sha256":review.document["review_sha256"],
        "plan_sha256":review.document["plan_sha256"],"blocks":review.blocks.len(),
        "eligible_blocks":review.blocks.iter().filter(|b| b.eligible).count(),"private_html":output,
        "network_used":false,"daemon_started":false,"removal_authorized":false}),
    )
}

pub fn request(
    root: &Path,
    scope: &Path,
    decision: &Path,
    output: &Path,
    now: u64,
) -> Result<Value> {
    if output.exists() {
        return Err("Request output already exists".into());
    }
    let decision_bytes = tiering::read_bounded(decision, 256 * 1024)?;
    let d: Decision = serde_json::from_slice(&decision_bytes)?;
    if d.schema != "reel.asset-tier-decision.v1"
        || d.action != "remove-redundant-hot-copy"
        || d.selected_blocks.is_empty()
        || d.selected_blocks.len() > 4096
    {
        return Err("Invalid or empty block decision".into());
    }
    let review = build(root, scope, now)?;
    if d.plan_sha256 != review.document["plan_sha256"]
        || d.review_sha256 != review.document["review_sha256"]
    {
        return Err("Review/plan/consumer state changed; review the refreshed blocks".into());
    }
    let mut hashes = BTreeSet::new();
    let mut bytes = 0u64;
    for id in &d.selected_blocks {
        let block = review
            .blocks
            .iter()
            .find(|b| &b.id == id)
            .ok_or("Unknown block decision")?;
        if !block.eligible {
            return Err("Protected or unsupported block cannot enter a removal request".into());
        }
        bytes = bytes
            .checked_add(block.logical_bytes)
            .ok_or("Decision byte overflow")?;
        hashes.extend(block.members.iter().cloned());
    }
    if hashes.len() > review.policy.max_batch_objects || bytes > review.policy.max_batch_bytes {
        return Err(
            "Selected blocks exceed the configured batch budget; choose a smaller block set".into(),
        );
    }
    let mut req = review.scope;
    req["sha256s"] = json!(hashes);
    req["max_verify_bytes"] = json!(bytes.checked_mul(2).ok_or("Verification budget overflow")?);
    if fs::read(decision)? != decision_bytes {
        return Err("Block decision changed during preparation".into());
    }
    tiering::write_plan(output, &req)?;
    Ok(
        json!({"schema":"reel.asset-tier-request-result.v1","objects":hashes.len(),"logical_bytes":bytes,
        "decision_sha256":tiering::digest(&decision_bytes),"review_sha256":d.review_sha256,
        "request_path":output,"default_next_step":"tier-demote verification without --apply",
        "removal_authorized":false,"archive_probed":false}),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::tiering::tests::Fixture;
    #[test]
    fn data_cannot_escape_the_private_json_script_element() {
        let hostile = json!({"label":"</script><script>alert(1)</script>&\u{2028}\u{2029}"});
        let encoded = embedded_data(&hostile).unwrap();
        assert!(!encoded.contains('<') && !encoded.contains('&'));
        assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), hostile);
    }

    pub(crate) fn fixture() -> (Fixture, std::path::PathBuf, String) {
        let mut f = Fixture::new();
        let h = f.put(b"previously used synthetic payload", true);
        let dep = f.version("old", &h);
        f.policy["history_versions"] = json!([dep]);
        f.put(b"unregistered synthetic content", false);
        f.baseline();
        let plan = f.run().unwrap();
        let p = f.root().join("plan.json");
        fs::write(&p, serde_json::to_vec(&plan).unwrap()).unwrap();
        let scope = f.root().join("scope.json");
        fs::write(&scope, serde_json::to_vec(&json!({"schema":"reel.asset-demotion-request.v1",
            "policy":f.root().join("policy.json"),"plan":p,"plan_sha256":plan["plan_sha256"],
            "consumer_roots":[f.root()],"consumer_scope":"all-managed-cache-consumers",
            "sha256s":[h],"max_verify_bytes":1024,"max_seconds":60,"allow_same_volume_copies":true})).unwrap()).unwrap();
        (f, scope, h)
    }
    fn decision(review: &Review, block: &str) -> Value {
        json!({"schema":"reel.asset-tier-decision.v1","review_sha256":review.document["review_sha256"],
            "plan_sha256":review.document["plan_sha256"],"action":"remove-redundant-hot-copy","selected_blocks":[block]})
    }
    #[test]
    fn offline_choice_prepares_exact_native_verification_without_removing_payloads() {
        let (f, scope, h) = fixture();
        let review = build(f.root(), &scope, 1000).unwrap();
        let block = review.blocks.iter().find(|b| b.eligible).unwrap();
        let d = f.root().join("decision.json");
        fs::write(&d, decision(&review, &block.id).to_string()).unwrap();
        let output = f.root().join("request.json");
        let result = request(f.root(), &scope, &d, &output, 1000).unwrap();
        assert_eq!(result["objects"], 1);
        let native: demotion::Request =
            serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(native.sha256s, BTreeSet::from([h.clone()]));
        assert_eq!(
            native.max_verify_bytes,
            2 * b"previously used synthetic payload".len() as u64
        );
        let dry = demotion::run(
            f.root(),
            &output,
            &f.root().canonicalize().unwrap().join(".local/journal"),
            false,
            1000,
        )
        .unwrap();
        assert_eq!(dry["objects"][0]["hot_links_removed"], 0);
        assert!(crate::object(&f.root().join("fast"), &h).exists());
        assert!(crate::object(&f.root().join("history"), &h).exists());
        assert!(!f.root().join("offline-cold").exists());
        assert!(request(f.root(), &scope, &d, &output, 1000).is_err());
    }
    #[test]
    fn held_unknown_and_forged_blocks_cannot_be_requested() {
        let (f, scope, _) = fixture();
        let review = build(f.root(), &scope, 1000).unwrap();
        let held = review.blocks.iter().find(|b| !b.eligible).unwrap();
        let d = f.root().join("decision.json");
        for id in [&held.id, &"f".repeat(64)] {
            fs::write(&d, decision(&review, id).to_string()).unwrap();
            assert!(request(f.root(), &scope, &d, &f.root().join("request.json"), 1000).is_err());
            assert!(!f.root().join("request.json").exists());
        }
    }
    #[test]
    fn review_explains_holds_and_exposes_only_bounded_private_hash_previews() {
        let (f, scope, h) = fixture();
        let review = build(f.root(), &scope, 1000).unwrap();
        assert_eq!(review.document["selected_tier"], "hot");
        assert_eq!(review.document["history_tier"], "warm");
        for block in &review.blocks {
            assert!(!block.disposition_reason.is_empty());
            assert!(block.member_preview.len() <= 8);
            assert!(
                block
                    .member_preview
                    .iter()
                    .all(|hash| block.members.contains(hash))
            );
        }
        let history = review.blocks.iter().find(|b| b.eligible).unwrap();
        assert_eq!(history.member_preview, vec![h]);
        let doc = serde_json::to_value(history).unwrap();
        assert!(doc.get("members").is_none());
    }
    #[test]
    fn choices_bind_scope_protections_plan_identity_and_expiry() {
        let (mut f, scope, h) = fixture();
        let review = build(f.root(), &scope, 1000).unwrap();
        let block = review.blocks.iter().find(|b| b.eligible).unwrap();
        let d = f.root().join("decision.json");
        let mut bad = decision(&review, &block.id);
        bad["review_sha256"] = json!("0".repeat(64));
        fs::write(&d, bad.to_string()).unwrap();
        assert!(request(f.root(), &scope, &d, &f.root().join("request.json"), 1000).is_err());
        fs::write(&d, decision(&review, &block.id).to_string()).unwrap();
        assert!(request(f.root(), &scope, &d, &f.root().join("request.json"), 1120).is_err());
        f.state.leases.insert(
            "new-working-input".into(),
            BTreeSet::from(["old@v1".into()]),
        );
        f.save();
        assert!(request(f.root(), &scope, &d, &f.root().join("request.json"), 1000).is_err());
        assert!(crate::object(&f.root().join("fast"), &h).exists());
    }
    #[test]
    fn another_consumers_new_selection_invalidates_the_choice_even_with_fresh_primary_plan() {
        let (f, scope, h) = fixture();
        let other = f.root().join("other-consumer");
        fs::create_dir_all(other.join(".local")).unwrap();
        fs::create_dir_all(other.join("registry")).unwrap();
        fs::create_dir_all(f.root().join(".local/control")).unwrap();
        let mut cfg: Value = crate::load(&f.root().join(".local/storage-rust.json")).unwrap();
        cfg["coordination_root"] = json!(f.root().join(".local/control"));
        fs::write(f.root().join(".local/storage-rust.json"), cfg.to_string()).unwrap();
        fs::write(other.join(".local/storage-rust.json"), cfg.to_string()).unwrap();
        for name in [
            "asset-project.json",
            "catalog.json",
            "registry/objects.tsv",
            "registry/versions.json",
        ] {
            fs::copy(f.root().join(name), other.join(name)).unwrap();
        }
        let plan = tiering::plan(
            f.root(),
            &f.root().join("policy.json"),
            &f.root().join("baseline.json"),
            1000,
        )
        .unwrap();
        fs::write(f.root().join("plan.json"), plan.to_string()).unwrap();
        let mut req: Value = crate::load(&scope).unwrap();
        req["plan_sha256"] = plan["plan_sha256"].clone();
        req["consumer_roots"] = json!([f.root(), other]);
        fs::write(&scope, req.to_string()).unwrap();
        let review = build(f.root(), &scope, 1000).unwrap();
        let block = review.blocks.iter().find(|b| b.eligible).unwrap();
        let choice = f.root().join("choice.json");
        fs::write(&choice, decision(&review, &block.id).to_string()).unwrap();
        let mut state: Value = crate::load(&other.join("registry/versions.json")).unwrap();
        state["current"] = json!({"old":"v1"});
        fs::write(other.join("registry/versions.json"), state.to_string()).unwrap();
        assert!(
            tiering::check_plan(
                f.root(),
                &f.root().join("policy.json"),
                &f.root().join("plan.json"),
                1000
            )
            .is_ok()
        );
        assert!(
            request(
                f.root(),
                &scope,
                &choice,
                &f.root().join("request.json"),
                1000
            )
            .is_err()
        );
        assert!(
            build(f.root(), &scope, 1000)
                .unwrap()
                .blocks
                .iter()
                .all(|b| !b.eligible)
        );
        assert!(crate::object(&f.root().join("fast"), &h).exists());
    }
    #[test]
    fn private_html_is_compact_paginated_networkless_and_not_an_apply_endpoint() {
        let (f, scope, _) = fixture();
        let p = f.root().join("review.html");
        let output = render(f.root(), &scope, &p, 1000).unwrap();
        assert_eq!(output["removal_authorized"], false);
        let html = fs::read_to_string(&p).unwrap();
        assert!(html.contains("connect-src 'none'"));
        assert!(html.contains("const pageSize=60"));
        assert!(html.contains("Save block choice"));
        assert!(!html.contains("fetch("));
        assert!(!html.contains("__REVIEW_DATA__"));
        assert!(!html.contains("baseline_copies"));
        assert!(render(f.root(), &scope, &p, 1000).is_err());
    }
}
