//! Version comparison, primary-input contracts and guarded publication reconciliation.
use crate::{Result, versions::State};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub old: crate::versions::Dependency,
    pub new: crate::versions::Dependency,
}

pub(crate) fn validate_transition(root: &std::path::Path, t: &Transition) -> Result<Value> {
    let state = crate::versions::read(root)?;
    crate::tiering::validate_graph(
        &state,
        crate::accounting::project(root)?.max_inventory_objects,
    )?;
    for lock in [&t.old, &t.new] {
        let version = state
            .versions
            .get(&format!("{}@{}", lock.logical_id, lock.version_id))
            .ok_or("Transition version missing")?;
        if version.manifest_sha256 != lock.manifest_sha256 {
            return Err("Transition immutable lock mismatch".into());
        }
    }
    let new = &state.versions[&format!("{}@{}", t.new.logical_id, t.new.version_id)];
    if new.dependency_contract.is_none() {
        return Err("Managed replacement requires an explicit primary-input contract".into());
    }
    let report = compare(
        &state,
        &crate::selected(root)?,
        (&t.old.logical_id, &t.old.version_id),
        (&t.new.logical_id, &t.new.version_id),
    )?;
    if report["new_is_current"] != true {
        return Err("Managed replacement is no longer current".into());
    }
    Ok(report)
}

fn hashes(state: &State, root: &str) -> Result<BTreeSet<String>> {
    let mut hashes = BTreeSet::new();
    for key in state.closure(std::iter::once(root.to_owned()))? {
        let version = &state.versions[&key];
        hashes.insert(version.manifest_sha256.clone());
        state.validate_dependency_contract(version)?;
        hashes.extend(version.active_payloads());
        for dep in &version.dependencies {
            if state.versions[&format!("{}@{}", dep.logical_id, dep.version_id)].manifest_sha256
                != dep.manifest_sha256
            {
                return Err("Dependency manifest hash mismatch".into());
            }
        }
    }
    Ok(hashes)
}

fn primary_inputs(state: &State, root: &str) -> Result<Option<BTreeSet<String>>> {
    let mut inputs = BTreeSet::new();
    for key in state.closure(std::iter::once(root.to_owned()))? {
        let Some(c) = &state.versions[&key].dependency_contract else {
            return Ok(None);
        };
        inputs.extend(c.primary_inputs.iter().cloned());
    }
    Ok(Some(inputs))
}

/// Candidate hashes are an advisory scope for the tier planner, never deletion authority.
/// Retention pins preserve history; tier policy, rather than a pin alone, determines its cache.
pub fn compare(
    state: &State,
    sizes: &HashMap<String, u64>,
    (old_id, old): (&str, &str),
    (new_id, new): (&str, &str),
) -> Result<Value> {
    let old_key = format!("{old_id}@{old}");
    let new_key = format!("{new_id}@{new}");
    let before = hashes(state, &old_key)?;
    let after = hashes(state, &new_key)?;
    let primary_diff = match (
        primary_inputs(state, &old_key)?,
        primary_inputs(state, &new_key)?,
    ) {
        (Some(a), Some(b)) => {
            json!({"added":b.difference(&a).collect::<Vec<_>>(),"retained":a.intersection(&b).collect::<Vec<_>>(),"dropped":a.difference(&b).collect::<Vec<_>>()})
        }
        _ => Value::Null,
    };
    let mut protections: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut protect = |root: String, reason: String| -> Result<()> {
        for hash in hashes(state, &root)? {
            protections.entry(hash).or_default().insert(reason.clone());
        }
        Ok(())
    };
    for (id, version) in &state.current {
        let root = format!("{id}@{version}");
        protect(root.clone(), format!("current:{root}"))?;
    }
    for (owner, roots) in &state.leases {
        for root in roots {
            protect(root.clone(), format!("lease:{owner}:{root}"))?;
        }
    }
    for hash in &state.metadata_roots {
        protections
            .entry(hash.clone())
            .or_default()
            .insert("registry-control".into());
    }
    let mut retention: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for root in &state.pins {
        for hash in hashes(state, root)? {
            retention.entry(hash).or_default().insert(root.clone());
        }
    }
    let rows = |set: BTreeSet<String>| -> Result<Vec<Value>> {
        set.into_iter()
            .map(|hash| {
                let bytes = sizes
                    .get(&hash)
                    .ok_or("Version references unregistered asset")?;
                Ok(json!({"sha256":hash,"bytes":bytes}))
            })
            .collect()
    };
    let added = rows(after.difference(&before).cloned().collect())?;
    let retained = rows(before.intersection(&after).cloned().collect())?;
    let mut dropped = rows(before.difference(&after).cloned().collect())?;
    let new_is_current = state.current.get(new_id).is_some_and(|v| v == new);
    let mut candidates = Vec::new();
    let mut candidate_bytes = 0u64;
    for row in &mut dropped {
        let hash = row["sha256"].as_str().unwrap().to_owned();
        let protected_by = protections.get(&hash).cloned().unwrap_or_default();
        // A partial global census is not authority for deletion. Exact candidates still
        // require the executor's complete managed-consumer scope and retained-copy proof.
        let candidate = new_is_current && protected_by.is_empty();
        if candidate {
            candidates.push(hash.clone());
            candidate_bytes = candidate_bytes
                .checked_add(row["bytes"].as_u64().unwrap())
                .ok_or("Candidate byte count overflow")?;
        }
        row["active_reference_count"] = json!(protected_by.len());
        row["protected_by"] = json!(protected_by);
        row["retained_by_pins"] = json!(retention.get(&hash).cloned().unwrap_or_default());
        row["eviction_candidate"] = json!(candidate);
    }
    Ok(json!({
        "schema":"reel.asset-version-diff.v1", "generation":state.generation,
        "old_logical_id":old_id,"new_logical_id":new_id,"old_version":old,"new_version":new,
        "old_manifest_sha256":state.versions[&old_key].manifest_sha256,
        "new_manifest_sha256":state.versions[&new_key].manifest_sha256,
        "new_is_current":new_is_current,"coverage_complete":state.coverage_complete,
        "added":added,"retained":retained,"dropped":dropped,
        "primary_input_diff":primary_diff,
        "eviction_candidate_sha256s":candidates,"candidate_logical_bytes":candidate_bytes,
        "removal_authorized":false,"physical_bytes_reclaimable":null,
        "next_step":"Intersect candidate hashes with a fresh tier plan and all-consumer protections; verify independent retained D/M copies before explicit tier-demote apply. Dropped does not mutate deprecation or selection."
    }))
}

/// Scope the existing guarded executor to eligible dropped hashes. No payload mutation.
pub fn eviction_request(
    root: &std::path::Path,
    old: (&str, &str),
    new: (&str, &str),
    scope: &std::path::Path,
    output: &std::path::Path,
    now: u64,
) -> Result<Value> {
    let mut request: crate::demotion::Request =
        serde_json::from_slice(&crate::tiering::read_bounded(scope, 1024 * 1024)?)?;
    crate::demotion::check_source_controls(root, &request, false)?;
    if request.allow_live_admissions {
        crate::tiering::check_transition_plan(root, &request.policy, &request.plan, now)?;
    } else {
        crate::tiering::check_plan(root, &request.policy, &request.plan, now)?;
    }
    let policy: crate::tiering::Policy =
        serde_json::from_slice(&crate::tiering::read_bounded(&request.policy, 256 * 1024)?)?;
    if request.max_seconds == 0
        || request.max_seconds > 3600
        || request.max_verify_bytes == 0
        || request
            .retained_tier
            .as_ref()
            .is_some_and(|id| id != &policy.history_tier && id != &policy.cold_tier)
        || (request.verified_cold_index.is_some()
            && request.retained_tier.as_ref() != Some(&policy.cold_tier))
    {
        return Err(
            "Version eviction requires valid budgets and the configured historical retention tier"
                .into(),
        );
    }
    let protections = crate::demotion::export_protections(root, scope)?;
    let protected = protections["protected_hashes"]
        .as_object()
        .ok_or("Missing consumer protections")?;
    let plan: Value = serde_json::from_slice(&crate::tiering::read_bounded(
        &request.plan,
        512 * 1024 * 1024,
    )?)?;
    if plan["plan_sha256"] != request.plan_sha256 {
        return Err("Scope plan hash mismatch".into());
    }
    let report = compare(
        &crate::versions::read(root)?,
        &crate::selected(root)?,
        old,
        new,
    )?;
    let candidates: BTreeSet<String> =
        serde_json::from_value(report["eviction_candidate_sha256s"].clone())?;
    let mut selected = BTreeSet::new();
    let mut bytes = 0u64;
    for row in plan["objects"].as_array().ok_or("Invalid tier plan")? {
        let hash = row["sha256"].as_str().ok_or("Invalid tier plan hash")?;
        if candidates.contains(hash)
            && !protected.contains_key(hash)
            && row["proposal"] == "verify-and-demote"
            && row["class"] == "previously-used"
            && (request.sha256s.is_empty() || request.sha256s.contains(hash))
        {
            selected.insert(hash.to_owned());
            bytes = bytes
                .checked_add(row["bytes"].as_u64().ok_or("Invalid tier plan size")?)
                .ok_or("Request byte count overflow")?;
        }
    }
    if selected.is_empty() {
        return Err("No dropped hashes eligible in this live state and historical tier plan; inspect version-diff protection reasons".into());
    }
    if selected.len() > policy.max_batch_objects
        || bytes > policy.max_batch_bytes
        || bytes.checked_mul(2).ok_or("Verification budget overflow")? > request.max_verify_bytes
    {
        return Err("Version eviction exceeds configured batch/verification budget; narrow the scope hash set".into());
    }
    request.sha256s = selected;
    request.transition = Some(Transition {
        old: crate::versions::Dependency {
            logical_id: old.0.into(),
            version_id: old.1.into(),
            manifest_sha256: report["old_manifest_sha256"].as_str().unwrap().into(),
        },
        new: crate::versions::Dependency {
            logical_id: new.0.into(),
            version_id: new.1.into(),
            manifest_sha256: report["new_manifest_sha256"].as_str().unwrap().into(),
        },
    });
    validate_transition(root, request.transition.as_ref().unwrap())?;
    request.allow_live_admissions = true;
    // Retained-tier and consumer fields are unchanged; the executor revalidates all of them.
    crate::tiering::write_plan(output, &serde_json::to_value(&request)?)?;
    Ok(
        json!({"schema":"reel.asset-version-eviction-request.v1","objects":request.sha256s.len(),
        "logical_bytes":bytes,"request_path":output,"removal_authorized":false,
        "next_step":"tier-demote verification, then explicit --apply; no files removed by request generation"}),
    )
}

/// Replan the changed block from current physical observations, rather than
/// requiring a whole-drive inventory after each newly created artifact.
pub fn plan_transition(
    root: &std::path::Path,
    old: (&str, &str),
    new: (&str, &str),
    scope: &std::path::Path,
    directory: &std::path::Path,
    now: u64,
) -> Result<Value> {
    let state = crate::versions::read(root)?;
    let old_version = state
        .versions
        .get(&format!("{}@{}", old.0, old.1))
        .ok_or("Old version missing")?;
    let new_version = state
        .versions
        .get(&format!("{}@{}", new.0, new.1))
        .ok_or("New version missing")?;
    if old.0 != new.0 || new_version.supersedes.as_deref() != Some(old.1) {
        return Err(
            "Automatic replacement planning requires an explicit same-artifact supersedes lock"
                .into(),
        );
    }
    let transition = Transition {
        old: crate::versions::Dependency {
            logical_id: old.0.into(),
            version_id: old.1.into(),
            manifest_sha256: old_version.manifest_sha256.clone(),
        },
        new: crate::versions::Dependency {
            logical_id: new.0.into(),
            version_id: new.1.into(),
            manifest_sha256: new_version.manifest_sha256.clone(),
        },
    };
    let report = validate_transition(root, &transition)?;
    let hashes: BTreeSet<String> =
        serde_json::from_value(report["eviction_candidate_sha256s"].clone())?;
    let raw = crate::tiering::read_bounded(scope, 256 * 1024)?;
    let mut request: crate::demotion::Request = serde_json::from_slice(&raw)?;
    let mut policy: crate::tiering::Policy =
        serde_json::from_slice(&crate::tiering::read_bounded(&request.policy, 256 * 1024)?)?;
    policy.validate(&crate::config(root)?)?;
    let census = crate::accounting::observe_named(root, &hashes)?;
    let canonical = root.canonicalize()?;
    let parent = directory
        .parent()
        .ok_or("Missing review parent")?
        .canonicalize()?;
    if !directory.is_absolute()
        || !parent.starts_with(canonical.join(".local"))
        || directory.exists()
    {
        return Err("New version-plan directory must be inside the invoking project's existing .local namespace".into());
    }
    let encoded = serde_json::to_vec(&census)?;
    policy.baseline = crate::tiering::Descriptor {
        sha256: crate::tiering::digest(&encoded),
        bytes: encoded.len() as u64,
    };
    if !policy.history_versions.iter().any(|v| {
        v.logical_id == old.0
            && v.version_id == old.1
            && v.manifest_sha256 == old_version.manifest_sha256
    }) {
        policy.history_versions.push(transition.old);
    }
    std::fs::create_dir(directory)?;
    crate::tiering::write_plan(&directory.join("baseline.json"), &census)?;
    request.policy = directory.join("policy.json");
    request.plan = directory.join("plan.json");
    crate::tiering::write_plan(&request.policy, &serde_json::to_value(policy)?)?;
    let plan = crate::tiering::plan(root, &request.policy, &directory.join("baseline.json"), now)?;
    crate::tiering::write_plan(&request.plan, &plan)?;
    request.plan_sha256 = plan["plan_sha256"].as_str().unwrap().into();
    request.allow_live_admissions = true;
    crate::tiering::write_plan(
        &directory.join("scope.json"),
        &serde_json::to_value(request)?,
    )?;
    crate::tiering::write_plan(&directory.join("comparison.json"), &report)?;
    Ok(
        json!({"schema":"reel.asset-version-plan.v1","scope":directory.join("scope.json"),"named_objects_observed":hashes.len(),"full_cache_scan":false,"archive_probed":false,"removal_authorized":false,"next_step":"version-eviction-request against this scope; verify and explicitly apply; finish with version-management-check"}),
    )
}

/// Job/PR gate: prove the exact transition has a verified retained copy and no
/// remaining unreferenced hot aliases. A dry run cannot satisfy this gate.
pub fn management_check(
    root: &std::path::Path,
    request_path: &std::path::Path,
    receipt_path: &std::path::Path,
    output: &std::path::Path,
) -> Result<Value> {
    let raw = crate::tiering::read_bounded(request_path, 256 * 1024)?;
    let request: crate::demotion::Request = serde_json::from_slice(&raw)?;
    let transition = request
        .transition
        .as_ref()
        .ok_or("No exact version transition in request")?;
    let report = validate_transition(root, transition)?;
    let receipt: Value = serde_json::from_slice(&crate::tiering::read_bounded(
        receipt_path,
        16 * 1024 * 1024,
    )?)?;
    if receipt["schema"] != "reel.asset-demotion-result.v1"
        || receipt["apply"] != true
        || receipt["request_sha256"] != crate::tiering::digest(&raw)
        || receipt["plan_sha256"] != request.plan_sha256
        || receipt["version_transition"] != serde_json::to_value(transition)?
        || receipt["last_copy_retired"] != false
    {
        return Err("Applied management receipt does not match this version transition".into());
    }
    let rows = receipt["objects"]
        .as_array()
        .ok_or("Missing management object results")?;
    let completed: BTreeSet<_> = rows
        .iter()
        .map(|r| r["sha256"].as_str().ok_or("Invalid receipt hash"))
        .collect::<std::result::Result<_, _>>()?;
    if completed.len() != rows.len()
        || completed != request.sha256s.iter().map(String::as_str).collect()
    {
        return Err("Management receipt does not cover the complete requested hash set".into());
    }
    let policy: crate::tiering::Policy =
        serde_json::from_slice(&crate::tiering::read_bounded(&request.policy, 256 * 1024)?)?;
    let cfg = crate::config(root)?;
    policy.validate(&cfg)?;
    let protections = crate::demotion::export_protections(root, request_path)?;
    let protected = protections["protected_hashes"]
        .as_object()
        .ok_or("Missing managed-consumer protections")?;
    let hot = &cfg.cache_roots[policy.tier(&policy.selected_tier).index];
    let history = if request.retained_tier.as_ref() == Some(&policy.cold_tier) {
        &cfg.archive_roots[policy.tier(&policy.cold_tier).index]
    } else {
        &cfg.cache_roots[policy.tier(&policy.history_tier).index]
    };
    let mut verified = 0;
    let mut shared = 0;
    let mut bytes = 0u64;
    let profile = crate::accounting::project(root)?;
    for row in report["dropped"].as_array().unwrap() {
        let hash = row["sha256"].as_str().unwrap();
        if row["eviction_candidate"] != true || protected.contains_key(hash) {
            shared += 1;
            continue;
        }
        let size = row["bytes"].as_u64().unwrap();
        bytes = bytes
            .checked_add(size)
            .ok_or("Verification byte overflow")?;
        if bytes > request.max_verify_bytes || verified >= policy.max_batch_objects {
            return Err("Management gate exceeds configured verification budget".into());
        }
        let mut aliases = vec![crate::object(hot, hash)];
        for ns in &profile.lfs_namespaces {
            aliases.push(
                hot.join(ns)
                    .join("objects")
                    .join(&hash[..2])
                    .join(&hash[2..4])
                    .join(hash),
            );
        }
        for path in aliases {
            match std::fs::symlink_metadata(path) {
                Err(e) if e.kind()==std::io::ErrorKind::NotFound=>(),
                Err(e)=>return Err(e.into()),
                Ok(_)=>return Err("Unreferenced superseded asset still has a hot cache alias; management incomplete".into()),
            }
        }
        crate::verify(&crate::object(history, hash), hash, size)?;
        verified += 1;
    }
    let result = json!({"schema":"reel.asset-version-management-receipt.v1","managed":true,
        "transition":transition,"request_sha256":crate::tiering::digest(&raw),
        "execution_receipt_sha256":crate::tiering::digest(&crate::tiering::read_bounded(receipt_path,16*1024*1024)?),
        "primary_input_diff":report["primary_input_diff"],"verified_retained_and_evicted_objects":verified,
        "still_referenced_objects":shared,"fresh_retained_bytes_verified":bytes,
        "new_version_current":true,"primary_input_contract_validated":true,"last_copy_retired":false,
        "scope":"Current declared managed consumers and exact version transition; no future-state guarantee"});
    crate::tiering::write_plan(output, &result)?;
    Ok(result)
}

/// Additive custody preparation; source deletion remains a separate verified command.
pub fn backup(
    root: &std::path::Path,
    request_path: &std::path::Path,
    output: &std::path::Path,
    apply: bool,
) -> Result<Value> {
    if apply && !cfg!(windows) {
        return Err("Guarded backup publication is Windows-only in this slice".into());
    }
    let request: crate::demotion::Request =
        serde_json::from_slice(&crate::tiering::read_bounded(request_path, 256 * 1024)?)?;
    validate_transition(
        root,
        request
            .transition
            .as_ref()
            .ok_or("Backup requires an exact managed transition")?,
    )?;
    let policy: crate::tiering::Policy =
        serde_json::from_slice(&crate::tiering::read_bounded(&request.policy, 256 * 1024)?)?;
    let cfg = crate::config(root)?;
    policy.validate(&cfg)?;
    let hot = &cfg.cache_roots[policy.tier(&policy.selected_tier).index];
    let archive = request.retained_tier.as_ref() == Some(&policy.cold_tier);
    if request
        .retained_tier
        .as_ref()
        .is_some_and(|id| id != &policy.history_tier && id != &policy.cold_tier)
    {
        return Err("Unsupported retention tier".into());
    }
    let target = if archive {
        &cfg.archive_roots[policy.tier(&policy.cold_tier).index]
    } else {
        &cfg.cache_roots[policy.tier(&policy.history_tier).index]
    };
    if !request.allow_same_volume_copies
        && crate::admission::volume_id(hot)? == crate::admission::volume_id(target)?
    {
        return Err("Retained cache must be on an independent volume".into());
    }
    let report = validate_transition(root, request.transition.as_ref().unwrap())?;
    let candidates: BTreeSet<String> =
        serde_json::from_value(report["eviction_candidate_sha256s"].clone())?;
    if !request.sha256s.is_subset(&candidates) || request.sha256s.len() > policy.max_batch_objects {
        return Err("Backup set exceeds the bounded current replacement".into());
    }
    let index = crate::selected(root)?;
    let mut bytes = 0u64;
    let mut copied = 0u64;
    let mut rows = Vec::new();
    let start = std::time::Instant::now();
    for hash in &request.sha256s {
        let size = *index.get(hash).ok_or("Backup hash is unregistered")?;
        bytes = bytes.checked_add(size).ok_or("Backup byte overflow")?;
        if bytes > request.max_verify_bytes
            || bytes > policy.max_batch_bytes
            || size > cfg.capacity.max_object_bytes
            || start.elapsed().as_secs() >= request.max_seconds
        {
            return Err("Backup exceeds configured budget".into());
        }
        let destination = crate::object(target, hash);
        let exists = match std::fs::symlink_metadata(&destination) {
            Ok(m) if m.file_type().is_file() => true,
            Ok(_) => return Err("Retained object must be a regular immutable file".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        if exists {
            let _dirs = crate::demotion::platform::guard_dirs(&destination)?;
            let _file = crate::demotion::platform::open(&destination, false)?;
            crate::verify(&destination, hash, size)?;
        } else if apply {
            let source = crate::object(hot, hash);
            if !std::fs::symlink_metadata(&source)?.file_type().is_file() {
                return Err("Backup source must be a regular immutable object".into());
            }
            let _source_dirs = crate::demotion::platform::guard_dirs(&source)?;
            let _source = crate::demotion::platform::open(&source, false)?;
            crate::verify(&source, hash, size)?;
            let _reservation = if archive {
                crate::admission::reserve_archive_on(root, &cfg, target, size)?
            } else {
                crate::admission::reserve_on(root, &cfg, target, size)?
            };
            // Hold each existing ancestor before creating its next child.
            let mut missing = Vec::new();
            let mut parent = destination.parent().ok_or("Missing cache parent")?;
            while !parent.exists() {
                missing.push(parent.to_path_buf());
                parent = parent.parent().ok_or("Missing cache ancestor")?;
            }
            let mut guards =
                crate::demotion::platform::guard_dirs(&parent.join(".custody-parent"))?;
            for path in missing.into_iter().rev() {
                match std::fs::create_dir(&path) {
                    Ok(()) => (),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                    Err(e) => return Err(e.into()),
                };
                guards.extend(crate::demotion::platform::guard_dirs(
                    &path.join(".custody-parent"),
                )?);
            }
            crate::recover(root, &source, target, hash, size)?;
            copied = copied.checked_add(size).ok_or("Copied byte overflow")?;
        }
        rows.push(json!({"sha256":hash,"bytes":size,"retained_path":destination,"already_present":exists,"verified":exists || apply,"copy_required":!exists}));
    }
    let result = json!({"schema":"reel.asset-version-backup.v1","apply":apply,"objects":rows,"payload_bytes_copied":copied,"source_objects_removed":0,"archive_target_explicit":archive,"next_step":"tier-demote verification and explicit apply, then version-management-check"});
    crate::tiering::write_plan(output, &result)?;
    Ok(result)
}

/// One bounded post-publication hook. It never selects a version or renders.
pub fn manage(
    root: &std::path::Path,
    id: &str,
    old: &str,
    new: &str,
    scope: &std::path::Path,
    directory: &std::path::Path,
    apply: bool,
) -> Result<Value> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let state = crate::versions::read(root)?;
    let old_record = state
        .versions
        .get(&format!("{id}@{old}"))
        .ok_or("Old version missing")?;
    let new_record = state
        .versions
        .get(&format!("{id}@{new}"))
        .ok_or("New version missing")?;
    if old != new && new_record.supersedes.as_deref() != Some(old) {
        return Err("Managed publication requires an explicit supersedes version".into());
    }
    let transition = Transition {
        old: crate::versions::Dependency {
            logical_id: id.into(),
            version_id: old.into(),
            manifest_sha256: old_record.manifest_sha256.clone(),
        },
        new: crate::versions::Dependency {
            logical_id: id.into(),
            version_id: new.into(),
            manifest_sha256: new_record.manifest_sha256.clone(),
        },
    };
    let compared = validate_transition(root, &transition)?;
    let scope_request: crate::demotion::Request =
        serde_json::from_slice(&crate::tiering::read_bounded(scope, 256 * 1024)?)?;
    let policy: crate::tiering::Policy = serde_json::from_slice(&crate::tiering::read_bounded(
        &scope_request.policy,
        256 * 1024,
    )?)?;
    let cfg = crate::config(root)?;
    policy.validate(&cfg)?;
    let protection = crate::demotion::export_protections(root, scope)?;
    if scope_request.max_seconds == 0
        || scope_request.max_seconds > 3600
        || scope_request.max_verify_bytes == 0
        || scope_request
            .retained_tier
            .as_ref()
            .is_some_and(|id| id != &policy.history_tier && id != &policy.cold_tier)
    {
        return Err("Invalid management scope budgets/retention tier".into());
    }
    let protected = protection["protected_hashes"]
        .as_object()
        .ok_or("Missing consumer protections")?;
    let hot = &cfg.cache_roots[policy.tier(&policy.selected_tier).index];
    let mut pending = false;
    let mut shared = 0;
    let mut already = Vec::new();
    for row in compared["dropped"].as_array().unwrap() {
        let h = row["sha256"].as_str().unwrap();
        if row["eviction_candidate"] != true || protected.contains_key(h) {
            shared += 1;
            continue;
        }
        let mut aliases = vec![crate::object(hot, h)];
        for ns in &crate::accounting::project(root)?.lfs_namespaces {
            aliases.push(
                hot.join(ns)
                    .join("objects")
                    .join(&h[..2])
                    .join(&h[2..4])
                    .join(h),
            );
        }
        for alias in aliases {
            match std::fs::symlink_metadata(alias) {
                Ok(_) => pending = true,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.into()),
            };
        }
        already.push(row);
    }
    if !pending {
        let parent = directory
            .parent()
            .ok_or("Missing receipt parent")?
            .canonicalize()?;
        if !directory.is_absolute()
            || !parent.starts_with(root.canonicalize()?.join(".local"))
            || directory.exists()
        {
            return Err(
                "New management directory must be inside the invoking .local namespace".into(),
            );
        }
        let mut retained_bytes = 0u64;
        for row in &already {
            let h = row["sha256"].as_str().unwrap();
            let n = row["bytes"].as_u64().unwrap();
            retained_bytes = retained_bytes
                .checked_add(n)
                .ok_or("Retained byte overflow")?;
            if retained_bytes > scope_request.max_verify_bytes || already.len() > policy.max_objects
            {
                return Err("Retained verification budget exceeded".into());
            }
            let archive = scope_request.retained_tier.as_ref() == Some(&policy.cold_tier);
            let target = if archive {
                &cfg.archive_roots[policy.tier(&policy.cold_tier).index]
            } else {
                &cfg.cache_roots[policy.tier(&policy.history_tier).index]
            };
            let path = crate::object(target, h);
            match std::fs::symlink_metadata(&path) {
                Ok(m) if m.file_type().is_file() => crate::verify(&path, h, n)?,
                Ok(_) => return Err("Retained backup is not a regular file".into()),
                Err(e)
                    if e.kind() == std::io::ErrorKind::NotFound
                        && !archive
                        && scope_request.history_fallback_to_archive =>
                {
                    crate::verify(
                        &crate::object(&cfg.archive_roots[policy.tier(&policy.cold_tier).index], h),
                        h,
                        n,
                    )?
                }
                Err(e) => return Err(e.into()),
            }
        }
        let rows = if old == new {
            compared["retained"].as_array().unwrap()
        } else {
            compared["added"].as_array().unwrap()
        };
        let mut verified = 0u64;
        for row in rows {
            let n = row["bytes"].as_u64().unwrap();
            verified = verified.checked_add(n).ok_or("New payload byte overflow")?;
            if verified > scope_request.max_verify_bytes {
                return Err("New version verification budget exceeded".into());
            }
            crate::active_existing(&cfg, row["sha256"].as_str().unwrap(), n)?
                .ok_or("New version payload missing")?;
        }
        std::fs::create_dir(directory)?;
        let receipt = json!({"schema":"reel.asset-version-management-receipt.v1","managed":apply,"dry_run":!apply,"transition":transition,"primary_input_contract_validated":true,"new_version_current":true,"new_payload_bytes_verified":verified,"already_evicted_and_retained_objects":already.len(),"still_referenced_objects":shared,"new_hot_links_removed":0,"reason":"No unreferenced superseded hot copies remain","last_copy_retired":false});
        crate::tiering::write_plan(&directory.join("comparison.json"), &compared)?;
        crate::tiering::write_plan(&directory.join("management.json"), &receipt)?;
        return Ok(
            json!({"schema":"reel.asset-version-manage.v1","managed":apply,"dry_run":!apply,"receipt":directory.join("management.json"),"management":receipt,"daemon_started":false}),
        );
    }
    plan_transition(root, (id, old), (id, new), scope, directory, now)?;
    let saved_scope = directory.join("scope.json");
    let mut request_path = directory.join("request.json");
    eviction_request(root, (id, old), (id, new), &saved_scope, &request_path, now)?;
    let mut request: crate::demotion::Request =
        serde_json::from_slice(&crate::tiering::read_bounded(&request_path, 256 * 1024)?)?;
    let report = validate_transition(root, request.transition.as_ref().unwrap())?;
    let cfg = crate::config(root)?;
    // Only changed/new payloads are read again; unchanged primary inputs are
    // protected by the exact immutable locks and remain outside this I/O budget.
    let mut verified_bytes = 0u64;
    for row in report["added"].as_array().unwrap() {
        let size = row["bytes"].as_u64().unwrap();
        verified_bytes = verified_bytes
            .checked_add(size)
            .ok_or("New payload byte overflow")?;
        if verified_bytes > request.max_verify_bytes {
            return Err("New payload verification exceeds the configured transition budget".into());
        }
        crate::active_existing(&cfg, row["sha256"].as_str().unwrap(), size)?
            .ok_or("New version payload is missing from active caches")?;
    }
    let attempted = backup(
        root,
        &request_path,
        &directory.join(if apply {
            "backup.json"
        } else {
            "backup-dry.json"
        }),
        apply,
    );
    let custody = match attempted {
        Ok(v) => v,
        Err(error)
            if apply
                && request.history_fallback_to_archive
                && error
                    .downcast_ref::<crate::admission::AdmissionDenied>()
                    .is_some() =>
        {
            let policy: crate::tiering::Policy = serde_json::from_slice(
                &crate::tiering::read_bounded(&request.policy, 256 * 1024)?,
            )?;
            if request.retained_tier.as_ref() == Some(&policy.cold_tier) {
                return Err(error);
            }
            request.retained_tier = Some(policy.cold_tier);
            request.verified_cold_index = None;
            request_path = directory.join("archive-request.json");
            crate::tiering::write_plan(&request_path, &serde_json::to_value(&request)?)?;
            backup(
                root,
                &request_path,
                &directory.join("archive-backup.json"),
                true,
            )?
        }
        Err(e) => return Err(e),
    };
    if !apply {
        return Ok(
            json!({"schema":"reel.asset-version-manage.v1","managed":false,"dry_run":true,"request":request_path,"backup":custody,"new_payload_bytes_verified":verified_bytes,"removal_authorized":false}),
        );
    }
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    crate::demotion::run(
        root,
        &request_path,
        &directory.join("verify"),
        false,
        current_time,
    )?;
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let executed = crate::demotion::run(
        root,
        &request_path,
        &directory.join("apply"),
        true,
        current_time,
    )?;
    let execution_path = directory.join("execution.json");
    crate::tiering::write_plan(&execution_path, &executed)?;
    let receipt_path = directory.join("management.json");
    let receipt = management_check(root, &request_path, &execution_path, &receipt_path)?;
    Ok(
        json!({"schema":"reel.asset-version-manage.v1","managed":true,"dry_run":false,"receipt":receipt_path,"management":receipt,"new_payload_bytes_verified":verified_bytes,"daemon_started":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versions::{Dependency, Version};

    fn version(id: &str, version: &str, manifest: &str, payloads: &[&str]) -> Version {
        Version {
            logical_id: id.into(),
            version_id: version.into(),
            manifest_sha256: manifest.into(),
            payloads: payloads.iter().map(|h| (*h).into()).collect(),
            dependencies: vec![],
            supersedes: None,
            deprecated: false,
            dependency_contract: None,
        }
    }

    #[test]
    fn replacement_closure_keeps_shared_current_leased_pinned_and_control_assets() {
        let h: Vec<_> = (0..12).map(|n| format!("{n:064x}")).collect();
        let sizes = h.iter().map(|h| (h.clone(), 10)).collect();
        let mut s = State {
            coverage_complete: true,
            ..State::default()
        };
        s.register(version("cel", "v1", &h[0], &[&h[1]])).unwrap();
        s.register(version("cel", "v2", &h[2], &[&h[3]])).unwrap();
        for (v, m, dep) in [("v1", 4, "v1"), ("v2", 5, "v2")] {
            let mut movie = version("movie", v, &h[m], &[&h[6], &h[7], &h[8], &h[9]]);
            // Only the old movie referenced these three extra assets.
            if v == "v2" {
                movie.payloads = BTreeSet::from([h[6].clone()]);
            }
            movie.dependencies.push(Dependency {
                logical_id: "cel".into(),
                version_id: dep.into(),
                manifest_sha256: h[if v == "v1" { 0 } else { 2 }].clone(),
            });
            s.register(movie).unwrap();
        }
        s.register(version("other", "v1", &h[10], &[&h[7]]))
            .unwrap();
        s.register(version("leased", "v1", &h[11], &[&h[8]]))
            .unwrap();
        s.select("movie", "v1").unwrap();
        assert!(
            compare(&s, &sizes, ("movie", "v1"), ("movie", "v2")).unwrap()["eviction_candidate_sha256s"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        s.select("movie", "v2").unwrap();
        s.select("other", "v1").unwrap();
        s.lease("render", BTreeSet::from(["leased@v1".into()]))
            .unwrap();
        s.metadata_roots.insert(h[9].clone());
        let report = compare(&s, &sizes, ("movie", "v1"), ("movie", "v2")).unwrap();
        assert_eq!(report["added"].as_array().unwrap().len(), 3);
        assert_eq!(report["retained"].as_array().unwrap().len(), 1);
        assert_eq!(report["dropped"].as_array().unwrap().len(), 6);
        assert_eq!(
            report["eviction_candidate_sha256s"],
            json!([h[0], h[1], h[4]])
        );
        assert_eq!(report["candidate_logical_bytes"], 30);
        assert_eq!(report["removal_authorized"], false);
        // A history pin preserves recovery, rather than requiring a hot-tier copy.
        s.pins.insert("cel@v1".into());
        assert_eq!(
            compare(&s, &sizes, ("movie", "v1"), ("movie", "v2")).unwrap()["eviction_candidate_sha256s"],
            json!([h[0], h[1], h[4]])
        );
        s.coverage_complete = false;
        assert_eq!(
            compare(&s, &sizes, ("movie", "v1"), ("movie", "v2")).unwrap()["coverage_complete"],
            false
        );
        assert!(compare(&s, &sizes, ("movie", "missing"), ("movie", "v2")).is_err());
        assert!(compare(&s, &HashMap::new(), ("movie", "v1"), ("movie", "v2")).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn five_cel_replacement_evicts_zero_ref_inputs_and_old_output_during_unrelated_job() {
        for (shared, archive, controller, fallback) in [
            (false, false, false, false),
            (true, false, false, false),
            (false, true, false, false),
            (false, false, true, false),
            (false, false, true, true),
        ] {
            let mut f = crate::tiering::tests::Fixture::new();
            let old: BTreeSet<_> = (0..5)
                .map(|n| f.put(format!("old cel {n}").as_bytes(), true))
                .collect();
            let new: BTreeSet<_> = (0..5)
                .map(|n| f.put(format!("new cel {n}").as_bytes(), true))
                .collect();
            let old_movie = f.put(b"compiled old episode", true);
            let new_movie = f.put(b"compiled new episode", true);
            let old_manifest = f.put(b"old episode source snapshot", true);
            let new_manifest = f.put(b"new episode source snapshot", true);
            for (id, inputs, movie, manifest) in [
                ("v1", &old, &old_movie, &old_manifest),
                ("v2", &new, &new_movie, &new_manifest),
            ] {
                let references = if id == "v2" {
                    BTreeSet::from([old_movie.clone()])
                } else {
                    BTreeSet::new()
                };
                let mut payloads = inputs.clone();
                payloads.insert(movie.clone());
                payloads.extend(references.clone());
                f.state
                    .register(Version {
                        logical_id: "episode".into(),
                        version_id: id.into(),
                        manifest_sha256: manifest.clone(),
                        payloads,
                        dependencies: vec![],
                        supersedes: if id == "v2" { Some("v1".into()) } else { None },
                        deprecated: false,
                        dependency_contract: Some(crate::versions::DependencyContract {
                            primary_inputs: inputs.clone(),
                            outputs: BTreeSet::from([movie.clone()]),
                            references,
                        }),
                    })
                    .unwrap();
            }
            f.state.select("episode", "v1").unwrap();
            f.state.select("episode", "v2").unwrap();
            f.state.pins.insert("episode@v1".into());
            let shared_hash = old.first().unwrap().clone();
            if shared {
                f.state
                    .register(Version {
                        logical_id: "other-episode".into(),
                        version_id: "v1".into(),
                        manifest_sha256: new_manifest.clone(),
                        payloads: BTreeSet::from([shared_hash.clone()]),
                        dependencies: vec![],
                        supersedes: None,
                        deprecated: false,
                        dependency_contract: Some(crate::versions::DependencyContract {
                            primary_inputs: BTreeSet::from([shared_hash.clone()]),
                            outputs: BTreeSet::new(),
                            references: BTreeSet::new(),
                        }),
                    })
                    .unwrap();
                f.state.select("other-episode", "v1").unwrap();
            }
            f.save();
            f.policy["allow_same_volume_copies"] = json!(true);
            if fallback {
                let path = f.root().join(".local/storage-rust.json");
                let mut storage: Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                storage["capacity"]["tier_minimum_free_bytes"] = json!({});
                storage["capacity"]["tier_minimum_free_bytes"][f
                    .root()
                    .canonicalize()
                    .unwrap()
                    .join("history")
                    .to_str()
                    .unwrap()] = json!(u64::MAX);
                std::fs::write(path, storage.to_string()).unwrap();
                std::fs::create_dir(f.root().join("offline-cold")).unwrap();
            }
            f.policy["history_versions"] =
                json!([{"logical_id":"episode","version_id":"v1","manifest_sha256":old_manifest}]);
            f.baseline();
            let plan = f.run().unwrap();
            crate::tiering::write_plan(&f.root().join("plan.json"), &plan).unwrap();
            let scope = f.root().join("scope.json");
            std::fs::write(&scope,json!({"schema":"reel.asset-demotion-request.v1","policy":f.root().join("policy.json"),"plan":f.root().join("plan.json"),"plan_sha256":plan["plan_sha256"],"consumer_roots":[f.root().canonicalize().unwrap()],"consumer_scope":"all-managed-cache-consumers","sha256s":[],"max_verify_bytes":2048,"max_seconds":60,"allow_same_volume_copies":true,"retained_tier":if archive {"cold"} else {"warm"},"history_fallback_to_archive":fallback}).to_string()).unwrap();
            let cfg = crate::config(f.root()).unwrap();
            let _unrelated_render = crate::admission::reserve(f.root(), &cfg, 16).unwrap();
            let request = f.root().join("request.json");
            eviction_request(
                f.root(),
                ("episode", "v1"),
                ("episode", "v2"),
                &scope,
                &request,
                1000,
            )
            .unwrap();
            // A different producer can publish unrelated metadata after the
            // plan/request. Managed execution refreshes exact protections.
            crate::versions::transact(f.root(), |s| {
                s.register(Version {
                    logical_id: "unrelated-producer".into(),
                    version_id: "v1".into(),
                    manifest_sha256: new_manifest.clone(),
                    payloads: BTreeSet::from([new_manifest.clone()]),
                    dependencies: vec![],
                    supersedes: None,
                    deprecated: false,
                    dependency_contract: None,
                })?;
                Ok(())
            })
            .unwrap();
            assert!(
                crate::tiering::check_plan(
                    f.root(),
                    &f.root().join("policy.json"),
                    &f.root().join("plan.json"),
                    1000
                )
                .is_err()
            );
            let retained = f
                .root()
                .join(if archive { "offline-cold" } else { "history" });
            if archive {
                std::fs::create_dir(&retained).unwrap();
            } else {
                let missing = old.iter().find(|h| !shared || *h != &shared_hash).unwrap();
                std::fs::remove_file(crate::object(&retained, missing)).unwrap();
            }
            if controller {
                let dry = manage(
                    f.root(),
                    "episode",
                    "v1",
                    "v2",
                    &scope,
                    &f.root()
                        .canonicalize()
                        .unwrap()
                        .join(".local/controller-dry"),
                    false,
                )
                .unwrap();
                assert_eq!(dry["managed"], false);
                assert!(
                    old.iter()
                        .all(|h| crate::object(&f.root().join("fast"), h).exists())
                );
                let done = manage(
                    f.root(),
                    "episode",
                    "v1",
                    "v2",
                    &scope,
                    &f.root()
                        .canonicalize()
                        .unwrap()
                        .join(".local/controller-apply"),
                    true,
                )
                .unwrap();
                assert_eq!(done["managed"], true);
                assert!(
                    old.iter()
                        .all(|h| !crate::object(&f.root().join("fast"), h).exists())
                );
                assert!(
                    new.iter()
                        .all(|h| crate::object(&f.root().join("fast"), h).exists())
                );
                let final_tier = f
                    .root()
                    .join(if fallback { "offline-cold" } else { "history" });
                assert!(old.iter().all(|h| crate::object(&final_tier, h).exists()));
                assert!(!crate::object(&f.root().join("fast"), &old_movie).exists());
                assert!(crate::object(&final_tier, &old_movie).exists());
                let again = manage(
                    f.root(),
                    "episode",
                    "v1",
                    "v2",
                    &scope,
                    &f.root()
                        .canonicalize()
                        .unwrap()
                        .join(".local/controller-repeat"),
                    true,
                )
                .unwrap();
                assert_eq!(again["managed"], true);
                assert_eq!(again["management"]["new_hot_links_removed"], 0);
                continue;
            }
            let backup_dry =
                backup(f.root(), &request, &f.root().join("backup-dry.json"), false).unwrap();
            assert_eq!(backup_dry["payload_bytes_copied"], 0);
            let copied = backup(
                f.root(),
                &request,
                &f.root().join("backup-applied.json"),
                true,
            )
            .unwrap();
            assert!(copied["payload_bytes_copied"].as_u64().unwrap() > 0);
            assert!(
                old.iter()
                    .all(|h| crate::object(&f.root().join("fast"), h).exists())
            );
            let journal = f.root().canonicalize().unwrap().join(".local/replacement");
            if !shared && !archive {
                crate::versions::transact(f.root(), |s| {
                    s.register(Version {
                        logical_id: "late-reader".into(),
                        version_id: "v1".into(),
                        manifest_sha256: new_manifest.clone(),
                        payloads: BTreeSet::from([shared_hash.clone()]),
                        dependencies: vec![],
                        supersedes: None,
                        deprecated: false,
                        dependency_contract: Some(crate::versions::DependencyContract {
                            primary_inputs: BTreeSet::from([shared_hash.clone()]),
                            outputs: BTreeSet::new(),
                            references: BTreeSet::new(),
                        }),
                    })?;
                    s.select("late-reader", "v1")?;
                    Ok(())
                })
                .unwrap();
                assert!(
                    crate::demotion::run(
                        f.root(),
                        &request,
                        &journal.join("new-reference-hold"),
                        true,
                        1000
                    )
                    .is_err()
                );
                assert!(
                    old.iter()
                        .all(|h| crate::object(&f.root().join("fast"), h).exists())
                );
                crate::versions::transact(f.root(), |s| s.unselect("late-reader", "v1")).unwrap();
            }
            let dry =
                crate::demotion::run(f.root(), &request, &journal.join("verify"), false, 1000)
                    .unwrap();
            assert_eq!(dry["apply"], false);
            let dry_path = f.root().join("dry-result.json");
            std::fs::write(&dry_path, dry.to_string()).unwrap();
            assert!(
                management_check(
                    f.root(),
                    &request,
                    &dry_path,
                    &f.root().join("must-not-pass.json")
                )
                .is_err()
            );
            assert!(
                old.iter()
                    .all(|h| crate::object(&f.root().join("fast"), h).exists())
            );
            let result =
                crate::demotion::run(f.root(), &request, &journal.join("apply"), true, 1000)
                    .unwrap();
            assert_eq!(result["admissions_present_at_completion"], true);
            let diff = &result["version_comparison"]["primary_input_diff"];
            assert_eq!(diff["added"].as_array().unwrap().len(), 5);
            assert_eq!(diff["dropped"].as_array().unwrap().len(), 5);
            for h in &old {
                assert_eq!(
                    crate::object(&f.root().join("fast"), h).exists(),
                    shared && h == &shared_hash
                );
                assert!(crate::object(&retained, h).exists());
            }
            assert!(
                new.iter()
                    .all(|h| crate::object(&f.root().join("fast"), h).exists())
            );
            assert!(!crate::object(&f.root().join("fast"), &old_movie).exists());
            assert!(crate::object(&retained, &old_movie).exists());
            assert!(crate::object(&f.root().join("fast"), &new_movie).exists());
            assert!(f.state.pins.contains("episode@v1"));
            assert_eq!(result["last_copy_retired"], false);
            let applied = f.root().join("apply-result.json");
            std::fs::write(&applied, result.to_string()).unwrap();
            let gate = management_check(
                f.root(),
                &request,
                &applied,
                &f.root().join("management.json"),
            )
            .unwrap();
            assert_eq!(gate["managed"], true);
            assert_eq!(
                gate["verified_retained_and_evicted_objects"],
                if shared { 6 } else { 7 }
            );
            assert_eq!(gate["still_referenced_objects"], if shared { 1 } else { 0 });
            let lost = old.iter().find(|h| !shared || *h != &shared_hash).unwrap();
            std::fs::write(crate::object(&retained, lost), b"corrupt").unwrap();
            assert!(
                management_check(
                    f.root(),
                    &request,
                    &applied,
                    &f.root().join("corrupt-must-not-pass.json")
                )
                .is_err()
            );
        }
    }
}
