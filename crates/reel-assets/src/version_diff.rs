//! Read-only comparison of immutable versions, including transitive dependencies.
use crate::{Result, versions::State};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn hashes(state: &State, root: &str) -> Result<BTreeSet<String>> {
    let mut hashes = BTreeSet::new();
    for key in state.closure(std::iter::once(root.to_owned()))? {
        let version = &state.versions[&key];
        hashes.insert(version.manifest_sha256.clone());
        hashes.extend(version.payloads.iter().cloned());
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
    crate::tiering::check_plan(root, &request.policy, &request.plan, now)?;
    let policy: crate::tiering::Policy =
        serde_json::from_slice(&crate::tiering::read_bounded(&request.policy, 256 * 1024)?)?;
    if request.max_seconds == 0
        || request.max_seconds > 3600
        || request.max_verify_bytes == 0
        || request
            .retained_tier
            .as_ref()
            .is_some_and(|id| id != &policy.history_tier)
        || request.verified_cold_index.is_some()
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
    // Retained-tier and consumer fields are unchanged; the executor revalidates all of them.
    crate::tiering::write_plan(output, &serde_json::to_value(&request)?)?;
    Ok(
        json!({"schema":"reel.asset-version-eviction-request.v1","objects":request.sha256s.len(),
        "logical_bytes":bytes,"request_path":output,"removal_authorized":false,
        "next_step":"tier-demote verification, then explicit --apply; no files removed by request generation"}),
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
}
