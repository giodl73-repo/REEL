//! Explicit, read-only tier planning from a retained census plus live registry deltas.
//! Plans are advisory: there is deliberately no deletion or move API in this slice.
use crate::{Result, accounting, config, selected, valid_oid, versions};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::Path,
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tier {
    pub id: String,
    pub storage: String,
    pub index: usize,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub schema: String,
    pub project_id: String,
    pub baseline: Descriptor,
    pub tiers: Vec<Tier>,
    pub selected_tier: String,
    pub history_tier: String,
    pub cold_tier: String,
    /// Explicit version locks with evidence of actual previous use, not age/deprecation.
    pub history_versions: Vec<versions::Dependency>,
    pub tier_pins: BTreeMap<String, String>,
    pub max_objects: usize,
    pub max_plan_seconds: u64,
    pub expires_after_seconds: u64,
    pub max_batch_bytes: u64,
    pub max_batch_objects: usize,
}

#[derive(Deserialize)]
struct Census {
    schema: String,
    project_id: String,
    registry_generation: u64,
    profile_sha256: String,
    tiers: Vec<CensusTier>,
    objects: Vec<Observed>,
}
#[derive(Deserialize)]
struct CensusTier {
    tier: usize,
    root: String,
    #[serde(default)]
    volume_id: Option<String>,
}
#[derive(Deserialize)]
struct Observed {
    sha256: String,
    bytes: Option<u64>,
    registered: bool,
    copies: Vec<Copy>,
}
#[derive(Deserialize, Serialize)]
struct Copy {
    tier: usize,
    bytes: u64,
    #[serde(default)]
    allocated_bytes: Option<u64>,
    #[serde(default)]
    physical_id: Option<String>,
    #[serde(default)]
    hard_links: Option<u64>,
    #[serde(default)]
    known_lfs_hard_links: Option<u64>,
    #[serde(default)]
    other_hard_links: Option<u64>,
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>> {
    let f = fs::File::open(path)?;
    if f.metadata()?.len() > max {
        return Err("Tier input exceeds byte budget".into());
    }
    let mut bytes = Vec::new();
    f.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err("Tier input grew beyond byte budget".into());
    }
    Ok(bytes)
}

pub(crate) struct Live {
    generation: u64,
    pub(crate) fingerprint: String,
    profile_hash: String,
    project_id: String,
    pub(crate) registered: BTreeMap<String, u64>,
    pub(crate) selected: BTreeSet<String>,
    pub(crate) current: BTreeSet<String>,
    pub(crate) leased: BTreeSet<String>,
    pub(crate) controls: BTreeSet<String>,
    pub(crate) history: BTreeSet<String>,
    coverage_complete: bool,
}

pub(crate) fn validate_graph(state: &versions::State, limit: usize) -> Result<()> {
    if state.versions.len() > limit {
        return Err("Version graph exceeds object budget".into());
    }
    let mut done = BTreeSet::new();
    for start in state.versions.keys() {
        let mut active = BTreeSet::new();
        let mut stack = vec![(start.clone(), false)];
        while let Some((key, exit)) = stack.pop() {
            if exit {
                active.remove(&key);
                done.insert(key);
                continue;
            }
            if done.contains(&key) {
                continue;
            }
            if !active.insert(key.clone()) {
                return Err("Cycle in version graph".into());
            }
            let version = state.versions.get(&key).ok_or("Missing graph dependency")?;
            if !valid_oid(&version.manifest_sha256)
                || version.payloads.iter().any(|h| !valid_oid(h))
            {
                return Err("Invalid graph payload hash".into());
            }
            stack.push((key, true));
            for dep in &version.dependencies {
                let child = format!("{}@{}", dep.logical_id, dep.version_id);
                if state
                    .versions
                    .get(&child)
                    .is_none_or(|v| v.manifest_sha256 != dep.manifest_sha256)
                {
                    return Err("Missing/mismatched graph dependency".into());
                }
                stack.push((child, false));
            }
        }
    }
    Ok(())
}
pub(crate) fn live(root: &Path, policy: &Policy) -> Result<Live> {
    let p = accounting::project(root)?;
    let cfg = config(root)?;
    let profile = fs::read(root.join("asset-project.json"))?;
    let raw = fs::read(root.join("registry/versions.json"))?;
    let state = versions::read(root)?;
    validate_graph(&state, policy.max_objects)?;
    let registered: BTreeMap<_, _> = selected(root)?.into_iter().collect();
    if state.versions.values().any(|v| {
        !registered.contains_key(&v.manifest_sha256)
            || v.payloads.iter().any(|h| !registered.contains_key(h))
    }) {
        return Err("Version graph contains unregistered payloads".into());
    }
    let (outputs, roots) = accounting::selected_roots(root, &p, &state, &selected(root)?, &cfg)?;
    let selected = accounting::expand(&state, roots)?;
    let current = accounting::expand(
        &state,
        state.current.iter().map(|(id, v)| format!("{id}@{v}")),
    )?;
    let leased = accounting::expand(&state, state.leases.values().flatten().cloned())?;
    let mut history = BTreeSet::new();
    for lock in &policy.history_versions {
        history.extend(versions::payload_closure(&state, lock)?);
    }
    let doc: Value = serde_json::from_slice(&raw)?;
    let mut controls: BTreeSet<String> = doc["version_records"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    controls.extend(state.metadata_roots.iter().cloned());
    let index_path = root.join("registry/object-index.json");
    let index_bytes = if index_path.exists() {
        read_bounded(&index_path, 256 * 1024)?
    } else {
        vec![]
    };
    if !index_bytes.is_empty() {
        let index: Value = serde_json::from_slice(&index_bytes)?;
        controls.insert(
            index["asset"]["sha256"]
                .as_str()
                .ok_or("Missing control index hash")?
                .into(),
        );
    }
    // Capture actual resolved authoring selections as well as registry generations.
    let token = json!({"state":state,"registered":registered,"outputs":outputs,
        "selected":selected,"profile":digest(&profile),"object_index":digest(&index_bytes),"registry_control":digest(&raw),
        "storage":fs::read(accounting::relative(root,&p.storage_config)?)?});
    Ok(Live {
        generation: state.generation,
        fingerprint: digest(&serde_json::to_vec(&token)?),
        profile_hash: digest(&profile),
        project_id: p.project_id,
        registered,
        selected,
        current,
        leased,
        controls,
        history,
        coverage_complete: state.coverage_complete,
    })
}

impl Policy {
    pub(crate) fn validate(&self, cfg: &crate::Config) -> Result<()> {
        if self.schema != "reel.asset-tier-policy.v1"
            || self.project_id.is_empty()
            || !valid_oid(&self.baseline.sha256)
            || self.baseline.bytes > 512 * 1024 * 1024
            || self.max_objects == 0
            || self.max_objects > 1_000_000
            || self.max_plan_seconds == 0
            || self.max_plan_seconds > 3600
            || self.expires_after_seconds == 0
            || self.expires_after_seconds > 86400
            || self.max_batch_bytes == 0
            || self.max_batch_objects == 0
            || self.max_batch_objects > self.max_objects
            || self.history_versions.len() > 4096
            || self.tiers.len() < 2
            || self.tiers.len() > 32
        {
            return Err("Invalid tier policy or budget".into());
        }
        let mut ids = BTreeSet::new();
        let mut slots = BTreeSet::new();
        for t in &self.tiers {
            if t.id.is_empty()
                || t.id.len() > 64
                || !ids.insert(t.id.as_str())
                || !slots.insert((t.storage.as_str(), t.index))
            {
                return Err("Duplicate/invalid tier".into());
            }
            match t.storage.as_str() {
                "active" if t.index < cfg.cache_roots.len() => (),
                "archive" if t.index < cfg.archive_roots.len() => (),
                _ => return Err("Tier storage binding is invalid".into()),
            }
        }
        if self.selected_tier == self.history_tier
            || self.selected_tier == self.cold_tier
            || self.history_tier == self.cold_tier
        {
            return Err("Placement roles must name distinct tiers".into());
        }
        for id in [&self.selected_tier, &self.history_tier, &self.cold_tier] {
            if !ids.contains(id.as_str()) {
                return Err("Placement references unknown tier".into());
            }
        }
        if self.tier(&self.selected_tier).storage != "active"
            || self.tier(&self.history_tier).storage != "active"
            || self.tier(&self.cold_tier).storage != "archive"
        {
            return Err("Invalid active/cold tier role".into());
        }
        for (h, t) in &self.tier_pins {
            if !valid_oid(h) || !ids.contains(t.as_str()) {
                return Err("Invalid tier pin".into());
            }
        }
        // Lexical root overlaps are invalid even when cold storage is offline.
        let paths: Vec<_> = self
            .tiers
            .iter()
            .map(|t| {
                if t.storage == "active" {
                    &cfg.cache_roots[t.index]
                } else {
                    &cfg.archive_roots[t.index]
                }
            })
            .collect();
        for (i, a) in paths.iter().enumerate() {
            if !a.is_absolute() {
                return Err("Tier root must be absolute".into());
            }
            for b in paths.iter().skip(i + 1) {
                if a.starts_with(b) || b.starts_with(a) {
                    return Err("Tier roots overlap".into());
                }
            }
        }
        Ok(())
    }
    pub(crate) fn tier(&self, id: &str) -> &Tier {
        self.tiers.iter().find(|t| t.id == id).unwrap()
    }
}

/// Reads only named metadata/census inputs, never enumerates storage or probes archive roots.
/// Baseline physical observations remain stale until a future explicit apply-time refresh.
pub fn plan(root: &Path, policy_path: &Path, census_path: &Path, now: u64) -> Result<Value> {
    let started = std::time::Instant::now();
    let policy_bytes = read_bounded(policy_path, 256 * 1024)?;
    let policy: Policy = serde_json::from_slice(&policy_bytes)?;
    let cfg = config(root)?;
    policy.validate(&cfg)?;
    let baseline = read_bounded(census_path, 512 * 1024 * 1024)?;
    if baseline.len() as u64 != policy.baseline.bytes || digest(&baseline) != policy.baseline.sha256
    {
        return Err("Baseline inventory hash/byte mismatch".into());
    }
    let census: Census = serde_json::from_slice(&baseline)?;
    drop(baseline);
    if census.schema != "reel.asset-cache-report.v1"
        || census.project_id != policy.project_id
        || !valid_oid(&census.profile_sha256)
        || census.objects.len() > policy.max_objects
    {
        return Err("Baseline scope/budget mismatch".into());
    }
    let before = live(root, &policy)?;
    if before.project_id != policy.project_id || before.generation < census.registry_generation {
        return Err("Live registry is outside baseline lineage".into());
    }
    let mut tier_slots = BTreeSet::new();
    let mut volumes = BTreeSet::new();
    for t in &census.tiers {
        if t.tier >= cfg.cache_roots.len()
            || Path::new(&t.root) != cfg.cache_roots[t.tier]
            || !tier_slots.insert(t.tier)
        {
            return Err("Baseline cache bindings differ; explicit new census required".into());
        }
        if let Some(v) = &t.volume_id
            && !volumes.insert(v)
        {
            return Err("Active tiers alias one volume; not independent tier copies".into());
        }
    }
    if tier_slots.len() != cfg.cache_roots.len() {
        return Err("Baseline active tier coverage missing".into());
    }
    let mut observed = BTreeMap::new();
    for row in census.objects {
        if !valid_oid(&row.sha256) || row.bytes.is_none() || observed.contains_key(&row.sha256) {
            return Err("Duplicate/invalid baseline object".into());
        }
        let mut seen = BTreeSet::new();
        for c in &row.copies {
            if c.tier >= cfg.cache_roots.len() || !seen.insert(c.tier) {
                return Err("Invalid/duplicate baseline copy tier".into());
            }
        }
        observed.insert(row.sha256.clone(), row);
    }
    let mut hashes: BTreeSet<_> = observed.keys().cloned().collect();
    hashes.extend(before.registered.keys().cloned());
    if hashes.len() > policy.max_objects {
        return Err("Reconciled inventory exceeds budget".into());
    }
    let mut rows = Vec::new();
    let mut summary: BTreeMap<String, Value> = BTreeMap::new();
    let mut new_registered = 0u64;
    let mut newly_registered = 0u64;
    let mut candidate_bytes = 0u64;
    let mut candidate_objects = 0usize;
    let mut claimed_physical = BTreeSet::new();
    let hot = policy.tier(&policy.selected_tier);
    let history_tier = policy.tier(&policy.history_tier);
    for h in hashes {
        if started.elapsed().as_secs() >= policy.max_plan_seconds {
            return Err("Tier plan time budget exceeded".into());
        }
        let old = observed.get(&h);
        let registered = before.registered.get(&h).copied();
        if registered.is_some() && old.is_none() {
            new_registered += 1;
        }
        if registered.is_some() && old.is_some_and(|v| !v.registered) {
            newly_registered += 1;
        }
        let size = registered
            .or_else(|| old.and_then(|r| r.bytes))
            .ok_or("Unknown object size")?;
        let (class, target) = if before.controls.contains(&h) {
            ("registry-control", None)
        } else if before.leased.contains(&h) {
            ("active-lease", None)
        } else if let Some(pin) = policy.tier_pins.get(&h) {
            ("tier-pin", Some(pin.as_str()))
        } else if before.selected.contains(&h) {
            ("selected", Some(policy.selected_tier.as_str()))
        } else if before.current.contains(&h) {
            ("current-working-hold", None)
        } else if before.history.contains(&h) {
            ("previously-used", Some(policy.history_tier.as_str()))
        } else if registered.is_some() {
            ("registered-unused-or-unproven", None)
        } else if old.is_some_and(|r| r.registered) {
            ("registration-removed-hold", None)
        } else {
            (
                "unregistered-cold-candidate",
                Some(policy.cold_tier.as_str()),
            )
        };
        let copies = old.map(|r| r.copies.as_slice()).unwrap_or_default();
        let from = copies.iter().find(|c| c.tier == hot.index);
        let to = copies.iter().find(|c| c.tier == history_tier.index);
        let mut reason = "classification-only; physical refresh and protection checks required";
        let mut proposed = "hold";
        let mut estimate = None;
        if target == Some(policy.history_tier.as_str()) && from.is_some() {
            proposed = "verify-and-demote";
            reason = "explicit previous-use closure; retain verified history copy before removing hot aliases";
            if let (Some(src), Some(dst)) = (from, to)
                && src.bytes == size
                && dst.bytes == size
                && src.other_hard_links == Some(0)
                && src.hard_links == src.known_lfs_hard_links.and_then(|n| n.checked_add(1))
                && src.physical_id.is_some()
                && src.physical_id != dst.physical_id
                && claimed_physical.insert(src.physical_id.clone().unwrap())
            {
                estimate = src.allocated_bytes;
            }
        } else if class == "selected" {
            proposed = if from.is_some() {
                "keep-selected"
            } else {
                "promote-selected"
            };
        } else if class == "unregistered-cold-candidate" {
            proposed = "register-and-archive-first";
        }
        if copies.iter().any(|c| c.bytes != size)
            || old.is_some_and(|r| registered.is_some() && r.bytes != registered)
        {
            proposed = "hold";
            reason = "baseline size conflict requires investigation";
            estimate = None;
        }
        if old.is_none() {
            proposed = "hold";
            reason = "new registration has no baseline physical observation";
        }
        if proposed == "verify-and-demote" {
            if candidate_objects >= policy.max_batch_objects
                || size > policy.max_batch_bytes.saturating_sub(candidate_bytes)
            {
                proposed = "hold";
                reason = "outside configured first-batch budget";
                estimate = None;
            } else {
                candidate_objects += 1;
                candidate_bytes += size;
            }
        }
        let e=summary.entry(class.into()).or_insert(json!({"objects":0,"logical_bytes":0,"baseline_hot_copy_bytes":0,"estimated_hot_reclaim_bytes":0}));
        for (key, n) in [
            ("objects", 1),
            ("logical_bytes", size),
            (
                "baseline_hot_copy_bytes",
                from.map(|c| c.bytes).unwrap_or(0),
            ),
            ("estimated_hot_reclaim_bytes", estimate.unwrap_or(0)),
        ] {
            e[key] = json!(
                e[key]
                    .as_u64()
                    .unwrap()
                    .checked_add(n)
                    .ok_or("Tier accounting overflow")?
            );
        }
        rows.push(
            json!({"sha256":h,"bytes":size,"class":class,"target_tier":target,
            "proposal":proposed,"reason":reason,"baseline_copies":copies,
            "estimated_hot_reclaim_bytes":estimate,"executable":false}),
        );
    }
    let after = live(root, &policy)?;
    if before.fingerprint != after.fingerprint || fs::read(policy_path)? != policy_bytes {
        return Err(
            "Registry/selection/config/policy changed while planning; retry explicitly".into(),
        );
    }
    let expires = now
        .checked_add(policy.expires_after_seconds)
        .ok_or("Expiry overflow")?;
    let mut plan = json!({"schema":"reel.asset-tier-plan.v1","project_id":policy.project_id,
        "created_unix_seconds":now,"expires_unix_seconds":expires,"policy_sha256":digest(&policy_bytes),
        "baseline":policy.baseline,"baseline_generation":census.registry_generation,
        "live_generation":before.generation,"live_fingerprint_sha256":before.fingerprint,
        "baseline_profile_sha256":census.profile_sha256,"live_profile_sha256":before.profile_hash,
        "delta":{"new_registered_objects":new_registered,"baseline_unknown_now_registered":newly_registered},
        "coverage_complete":before.coverage_complete,"inventory_scope":"retained baseline plus current registered delta; new unregistered files require explicit census",
        "tiers":policy.tiers,"summary":summary,"objects":rows,
        "candidate_batch_objects":candidate_objects,"candidate_batch_logical_bytes":candidate_bytes,
        "dry_run":true,"executable":false,"removal_authorized":false,"archive_probed":false,
        "payload_hashes_verified":false,"payload_bytes_copied":0,"media_deleted":false,
        "limits":"Physical observations/capacity are baseline-only. Estimates are not guaranteed free space. Current-working and unproven history stay held. No permanent-retirement actions."});
    plan["plan_sha256"] = json!(digest(&serde_json::to_vec(&plan)?));
    Ok(plan)
}

/// New output only, atomic publication; never overwrite an earlier plan or payload.
pub fn write_plan(path: &Path, plan: &Value) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temp = crate::Temporary(crate::unique(parent, "tier-plan"));
    let f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp.0)?;
    let mut out = std::io::BufWriter::new(f);
    serde_json::to_writer(&mut out, plan)?;
    out.flush()?;
    out.get_ref().sync_all()?;
    drop(out);
    fs::hard_link(&temp.0, path)?;
    Ok(())
}

/// Freshness check is metadata-only. It never converts an advisory plan into authorization.
pub fn check_plan(root: &Path, policy_path: &Path, plan_path: &Path, now: u64) -> Result<Value> {
    let mut saved: Value = serde_json::from_slice(&read_bounded(plan_path, 512 * 1024 * 1024)?)?;
    let expected = saved["plan_sha256"]
        .as_str()
        .ok_or("Missing plan hash")?
        .to_owned();
    saved
        .as_object_mut()
        .ok_or("Plan must be an object")?
        .remove("plan_sha256");
    if digest(&serde_json::to_vec(&saved)?) != expected
        || saved["schema"] != "reel.asset-tier-plan.v1"
        || saved["executable"] != false
        || saved["removal_authorized"] != false
        || saved["dry_run"] != true
    {
        return Err("Plan integrity/schema mismatch".into());
    }
    let bytes = read_bounded(policy_path, 256 * 1024)?;
    let policy: Policy = serde_json::from_slice(&bytes)?;
    policy.validate(&config(root)?)?;
    let created = saved["created_unix_seconds"]
        .as_u64()
        .ok_or("Missing creation time")?;
    let expires = saved["expires_unix_seconds"]
        .as_u64()
        .ok_or("Missing expiry")?;
    if now < created
        || now >= expires
        || saved["policy_sha256"] != digest(&bytes)
        || saved["baseline"] != serde_json::to_value(&policy.baseline)?
    {
        return Err("Plan is expired or policy changed".into());
    }
    let fresh = live(root, &policy)?;
    if saved["live_fingerprint_sha256"] != fresh.fingerprint
        || saved["project_id"] != fresh.project_id
    {
        return Err("Registry/selection/config changed since plan".into());
    }
    Ok(
        json!({"schema":"reel.asset-tier-plan-check.v1","plan_sha256":expected,
        "fresh":true,"live_generation":fresh.generation,"executable":false,"removal_authorized":false,
        "physical_state_revalidated":false,"archive_probed":false}),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) struct Fixture {
        dir: tempfile::TempDir,
        pub(crate) state: versions::State,
        pub(crate) policy: Value,
    }
    impl Fixture {
        pub(crate) fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().canonicalize().unwrap();
            for name in [".local", "registry", "fast", "history"] {
                fs::create_dir(root.join(name)).unwrap();
            }
            // The cold root deliberately does not exist. Planning must not create/probe it.
            fs::write(root.join(".local/storage-rust.json"),json!({"cache_roots":[root.join("fast"),root.join("history")],"archive_roots":[root.join("offline-cold")],"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}}).to_string()).unwrap();
            fs::write(root.join("catalog.json"), r#"{"videos":[]}"#).unwrap();
            fs::write(root.join("asset-project.json"),json!({"schema":"reel.asset-project.v1","project_id":"fixture","storage_config":".local/storage-rust.json","selections":[],"scan_cas":true,"max_inventory_objects":100,"lfs_namespaces":["lfs/test"]}).to_string()).unwrap();
            let mut f = Self {
                dir,
                state: versions::State::default(),
                policy: json!({"schema":"reel.asset-tier-policy.v1","project_id":"fixture","baseline":{"sha256":"","bytes":0},"tiers":[{"id":"hot","storage":"active","index":0},{"id":"warm","storage":"active","index":1},{"id":"cold","storage":"archive","index":0}],"selected_tier":"hot","history_tier":"warm","cold_tier":"cold","history_versions":[],"tier_pins":{},"max_objects":100,"max_plan_seconds":60,"expires_after_seconds":120,"max_batch_bytes":1024,"max_batch_objects":10}),
            };
            f.save();
            f
        }
        pub(crate) fn root(&self) -> &Path {
            self.dir.path()
        }
        pub(crate) fn save(&mut self) {
            fs::write(
                self.root().join("registry/versions.json"),
                serde_json::to_vec(&self.state).unwrap(),
            )
            .unwrap();
        }
        pub(crate) fn put(&self, data: &[u8], registered: bool) -> String {
            let h = digest(data);
            for tier in ["fast", "history"] {
                let p = crate::object(&self.root().join(tier), &h);
                fs::create_dir_all(p.parent().unwrap()).unwrap();
                fs::write(p, data).unwrap();
            }
            if registered {
                let mut f = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(self.root().join("registry/objects.tsv"))
                    .unwrap();
                writeln!(f, "{h}\t{}", data.len()).unwrap();
            }
            h
        }
        pub(crate) fn version(&mut self, id: &str, h: &str) -> versions::Dependency {
            self.state
                .register(versions::Version {
                    logical_id: id.into(),
                    version_id: "v1".into(),
                    manifest_sha256: h.into(),
                    payloads: BTreeSet::from([h.into()]),
                    dependencies: vec![],
                    supersedes: None,
                    deprecated: false,
                })
                .unwrap();
            self.save();
            versions::Dependency {
                logical_id: id.into(),
                version_id: "v1".into(),
                manifest_sha256: h.into(),
            }
        }
        pub(crate) fn baseline(&mut self) {
            let mut census = accounting::report(self.root()).unwrap();
            // Model distinct configured volumes in this read-only fixture. Real
            // apply-time volume/identity verification is a later implementation gate.
            for (n, tier) in census["tiers"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .enumerate()
            {
                tier["volume_id"] = json!(format!("synthetic-volume-{n}"));
            }
            let bytes = serde_json::to_vec(&census).unwrap();
            fs::write(self.root().join("baseline.json"), &bytes).unwrap();
            self.policy["baseline"] = json!({"sha256":digest(&bytes),"bytes":bytes.len()});
        }
        pub(crate) fn run(&self) -> Result<Value> {
            let p = self.root().join("policy.json");
            fs::write(&p, self.policy.to_string()).unwrap();
            plan(self.root(), &p, &self.root().join("baseline.json"), 1000)
        }
        fn row<'a>(p: &'a Value, h: &str) -> &'a Value {
            p["objects"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["sha256"] == h)
                .unwrap()
        }
    }
    #[test]
    fn historical_retention_pin_allows_advisory_demotion_not_deletion() {
        let mut f = Fixture::new();
        let h = f.put(b"old actually used", true);
        let dep = f.version("old", &h);
        f.state.pins.insert("old@v1".into());
        f.save();
        f.policy["history_versions"] = json!([dep]);
        f.baseline();
        let before = fs::read(f.root().join("registry/versions.json")).unwrap();
        let p = f.run().unwrap();
        assert_eq!(Fixture::row(&p, &h)["proposal"], "verify-and-demote");
        assert_eq!(p["candidate_batch_objects"], 1);
        assert_eq!(p["removal_authorized"], false);
        assert_eq!(p["archive_probed"], false);
        assert_eq!(p["executable"], false);
        assert_eq!(
            before,
            fs::read(f.root().join("registry/versions.json")).unwrap()
        );
        assert!(crate::object(&f.root().join("fast"), &h).exists());
        assert!(!f.root().join("offline-cold").exists());
        assert_eq!(p, f.run().unwrap());
    }
    #[test]
    fn unknown_needs_registration_and_unused_candidate_is_held() {
        let mut f = Fixture::new();
        let unknown = f.put(b"unknown", false);
        let unused = f.put(b"unused", true);
        f.baseline();
        let p = f.run().unwrap();
        assert_eq!(
            Fixture::row(&p, &unknown)["proposal"],
            "register-and-archive-first"
        );
        assert_eq!(
            Fixture::row(&p, &unused)["class"],
            "registered-unused-or-unproven"
        );
        assert_eq!(p["candidate_batch_objects"], 0);
    }
    #[test]
    fn baseline_is_patched_by_live_registration_and_current_lease_changes() {
        let mut f = Fixture::new();
        let h = f.put(b"baseline unknown", false);
        f.baseline();
        f.put(b"baseline unknown", true);
        let added = f.put(b"new registration", true);
        let dep = f.version("new", &added);
        f.state
            .leases
            .insert("worker".into(), BTreeSet::from(["new@v1".into()]));
        f.save();
        f.policy["history_versions"] = json!([dep]);
        let p = f.run().unwrap();
        assert_eq!(p["delta"]["new_registered_objects"], 1);
        assert_eq!(p["delta"]["baseline_unknown_now_registered"], 1);
        assert_eq!(
            Fixture::row(&p, &h)["class"],
            "registered-unused-or-unproven"
        );
        assert_eq!(Fixture::row(&p, &added)["class"], "active-lease");
        assert_eq!(Fixture::row(&p, &added)["proposal"], "hold");
    }
    #[test]
    fn selected_scene_input_overrides_previous_use_and_current_work_is_held() {
        let mut f = Fixture::new();
        let h = f.put(b"selected", true);
        let input = f.put(b"input", true);
        let scene = json!({"schema":"reel.scene-authoring.v2","scene_id":"test","authoring_state":"imported-evidence"});
        let bindings = json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"test","assets":{"delivery/movie":{"sha256":h,"bytes":8,"cache_uri":format!("cache://sha256/{h}"),"logical_id":"movie","selection_state":"selected-private"},"input/cel":{"sha256":input,"bytes":5,"cache_uri":format!("cache://sha256/{input}"),"logical_id":"cel","selection_state":"retained"}}});
        let manifest = f.put(&crate::scenes::snapshot(&scene, &bindings).unwrap(), true);
        let lock = f.version("scene/test", &manifest);
        let v = f.state.versions.get_mut("scene/test@v1").unwrap();
        v.payloads.extend([h.clone(), input.clone()]);
        f.state.select("scene/test", "v1").unwrap();
        f.save();
        fs::write(f.root().join("scene.json"), scene.to_string()).unwrap();
        fs::write(f.root().join("bindings.json"), bindings.to_string()).unwrap();
        fs::write(f.root().join("catalog.json"),json!({"videos":[{"scene":"scene.json","bindings":"bindings.json","version_lock":lock}]}).to_string()).unwrap();
        let path = f.root().join("asset-project.json");
        let mut profile = crate::load(&path).unwrap();
        profile["selections"] = json!([{"index":"catalog.json","entries":"/videos","record_field":"bindings","format":"bindings","role_prefixes":["delivery/"]}]);
        fs::write(path, profile.to_string()).unwrap();
        f.policy["history_versions"] = json!([lock]);
        f.baseline();
        let p = f.run().unwrap();
        assert_eq!(Fixture::row(&p, &input)["class"], "selected");
        assert_eq!(Fixture::row(&p, &input)["proposal"], "keep-selected");
        // Removing output selection does not turn current work into old history.
        fs::write(f.root().join("catalog.json"), r#"{"videos":[]}"#).unwrap();
        assert_eq!(
            Fixture::row(&f.run().unwrap(), &input)["class"],
            "current-working-hold"
        );
    }
    #[test]
    fn tier_pin_and_new_lease_prevent_historical_demotion() {
        let mut f = Fixture::new();
        let h = f.put(b"leased", true);
        let dep = f.version("old", &h);
        f.policy["history_versions"] = json!([dep]);
        f.baseline();
        f.policy["tier_pins"] = json!({h.clone():"hot"});
        assert_eq!(Fixture::row(&f.run().unwrap(), &h)["class"], "tier-pin");
        f.state
            .leases
            .insert("worker".into(), BTreeSet::from(["old@v1".into()]));
        f.save();
        assert_eq!(Fixture::row(&f.run().unwrap(), &h)["class"], "active-lease");
    }
    #[test]
    fn baseline_tamper_and_policy_scope_are_rejected() {
        let mut f = Fixture::new();
        f.put(b"data", true);
        f.baseline();
        f.policy["project_id"] = json!("other");
        assert!(f.run().is_err());
        f.policy["project_id"] = json!("fixture");
        fs::write(f.root().join("baseline.json"), b"{}").unwrap();
        assert!(f.run().is_err());
    }
    #[test]
    fn invalid_tier_binding_overlap_and_budget_fail_closed() {
        let mut f = Fixture::new();
        f.baseline();
        let original = f.policy.clone();
        f.policy["tiers"][1]["index"] = json!(0);
        assert!(f.run().is_err());
        f.policy = original.clone();
        f.policy["history_tier"] = json!("missing");
        assert!(f.run().is_err());
        f.policy = original;
        f.policy["expires_after_seconds"] = json!(0);
        assert!(f.run().is_err());
    }
    #[test]
    fn cycles_missing_dependencies_and_wrong_locks_are_rejected() {
        let mut f = Fixture::new();
        let h = f.put(b"history", true);
        let dep = f.version("a", &h);
        f.baseline();
        f.state
            .versions
            .get_mut("a@v1")
            .unwrap()
            .dependencies
            .push(dep);
        f.save();
        assert!(f.run().is_err());
        f.state.versions.get_mut("a@v1").unwrap().dependencies[0].logical_id = "missing".into();
        f.save();
        assert!(f.run().is_err());
    }
    #[test]
    fn first_batch_budget_holds_excess_and_output_never_overwrites() {
        let mut f = Fixture::new();
        let h = f.put(b"history", true);
        let dep = f.version("a", &h);
        f.policy["history_versions"] = json!([dep]);
        f.policy["max_batch_bytes"] = json!(1);
        f.baseline();
        let p = f.run().unwrap();
        assert_eq!(p["candidate_batch_objects"], 0);
        let out = f.root().join("plan.json");
        write_plan(&out, &p).unwrap();
        assert!(write_plan(&out, &json!({})).is_err());
        assert_eq!(crate::load(&out).unwrap(), p);
    }
    #[test]
    fn unrecognized_hard_links_never_claim_reclaimable_bytes() {
        let mut f = Fixture::new();
        let h = f.put(b"linked history", true);
        let dep = f.version("a", &h);
        f.policy["history_versions"] = json!([dep]);
        fs::hard_link(
            crate::object(&f.root().join("fast"), &h),
            f.root().join("unowned-alias"),
        )
        .unwrap();
        f.baseline();
        assert!(Fixture::row(&f.run().unwrap(), &h)["estimated_hot_reclaim_bytes"].is_null());
    }

    #[test]
    fn check_rejects_expiry_tamper_and_new_lease_without_mutating_bytes() {
        let mut f = Fixture::new();
        let h = f.put(b"history", true);
        f.version("a", &h);
        f.baseline();
        let p = f.run().unwrap();
        let out = f.root().join("plan.json");
        write_plan(&out, &p).unwrap();
        let policy = f.root().join("policy.json");
        assert_eq!(
            check_plan(f.root(), &policy, &out, 1001).unwrap()["fresh"],
            true
        );
        assert!(check_plan(f.root(), &policy, &out, 999).is_err());
        assert!(check_plan(f.root(), &policy, &out, 1120).is_err());
        f.state
            .leases
            .insert("new-worker".into(), BTreeSet::from(["a@v1".into()]));
        f.save();
        assert!(check_plan(f.root(), &policy, &out, 1001).is_err());
        let mut tampered = p;
        tampered["objects"][0]["target_tier"] = json!("elsewhere");
        fs::write(&out, tampered.to_string()).unwrap();
        assert!(check_plan(f.root(), &policy, &out, 1001).is_err());
    }

    #[test]
    fn alias_volumes_and_duplicate_objects_are_rejected_even_with_valid_input_hash() {
        let mut f = Fixture::new();
        f.put(b"data", true);
        f.baseline();
        let path = f.root().join("baseline.json");
        let original = crate::load(&path).unwrap();
        for duplicate_objects in [false, true] {
            let mut census = original.clone();
            if duplicate_objects {
                let row = census["objects"][0].clone();
                census["objects"].as_array_mut().unwrap().push(row);
            } else {
                census["tiers"][1]["volume_id"] = census["tiers"][0]["volume_id"].clone();
            }
            let bytes = serde_json::to_vec(&census).unwrap();
            fs::write(&path, &bytes).unwrap();
            f.policy["baseline"] = json!({"sha256":digest(&bytes),"bytes":bytes.len()});
            assert!(f.run().is_err());
        }
    }
}
