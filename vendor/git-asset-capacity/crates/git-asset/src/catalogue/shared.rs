//! Exact recorded identity intersection; no media reads or custody inference.
use super::{Catalogue, Config, Result};
use git_asset_core::Object;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Load only the requested pinned projects. Other enrolled checkouts need not exist.
pub fn compare(mut config: Config, left: &str, right: &str, category: &str) -> Result<Value> {
    if left == right || category.len() > 256 || category.chars().any(char::is_control) {
        return Err("Choose two distinct projects and a bounded category".into());
    }
    for id in [left, right] {
        if !config.projects.iter().any(|p| p.id == id) {
            return Err("Unknown shared-object project".into());
        }
    }
    config.projects.retain(|p| p.id == left || p.id == right);
    config.default_project = None;
    config.storage_domains.clear();
    Catalogue::load(config)?.shared_selected(left, right, category)
}

impl Catalogue {
    /// Calculate against captured snapshots, also used by the local UI.
    pub fn shared_selected(&self, left: &str, right: &str, category: &str) -> Result<Value> {
        if left == right
            || category.len() > 256
            || category.chars().any(char::is_control)
            || !self.projects.contains_key(left)
            || !self.projects.contains_key(right)
        {
            return Err("Choose two distinct enrolled projects and a bounded category".into());
        }
        let mut identities = BTreeMap::<String, (Object, Vec<Value>, Vec<Value>)>::new();
        let mut coverage = Vec::new();
        for (side, id) in [left, right].iter().enumerate() {
            let p = &self.projects[*id];
            let mut categories = BTreeMap::<&str, usize>::new();
            let mut matched = 0;
            let mut missing = 0;
            for c in &p.cards {
                *categories.entry(&c.category).or_default() += 1;
                if !category.is_empty() && c.category != category {
                    continue;
                }
                matched += 1;
                let Some(object) = &c.selected else {
                    missing += 1;
                    continue;
                };
                let entry = identities
                    .entry(object.sha256.as_str().into())
                    .or_insert_with(|| (object.clone(), Vec::new(), Vec::new()));
                if entry.0.bytes != object.bytes {
                    return Err("Conflicting lengths for the same SHA-256".into());
                }
                let reference =
                    json!({"asset":c.id,"definition":c.definition,"category":c.category});
                if side == 0 {
                    entry.1.push(reference);
                } else {
                    entry.2.push(reference);
                }
            }
            coverage.push(
                json!({"project":id,"commit":p.commit,"records":p.cards.len(),
            "record_scopes":p.profile.records,"categories":categories,
            "matched_records":matched,"without_selected_object":missing}),
            );
        }
        let mut bytes = 0u64;
        let mut shared = Vec::new();
        for (object, a, b) in identities.into_values() {
            if !a.is_empty() && !b.is_empty() {
                bytes = bytes
                    .checked_add(object.bytes)
                    .ok_or("Shared-byte total overflow")?;
                shared.push(json!({"object":object,"left":a,"right":b}));
            }
        }
        Ok(
            json!({"schema":"tig.shared-selected.experimental-1","left":left,"right":right,
        "category":category,"coverage":coverage,"shared_objects":shared.len(),
        "shared_unique_bytes":bytes,"items":shared,"payloads_opened":0,
        "identity":"recorded SHA-256 and exact length; payload presence not verified",
        "scope":"Selected objects in enrolled native records only; named streams, nested inventories, Git LFS pointers outside these records and unregistered files are not compared.",
        "complete_project_comparison":false,"authorizes_eviction":false}),
        )
    }

    /// Bounded projection, never silently dropping reference counts.
    pub fn shared_page(
        &self,
        left: &str,
        right: &str,
        category: &str,
        offset: usize,
    ) -> Result<Value> {
        if offset > 1_000_000 {
            return Err("Shared-object page budget exceeded".into());
        }
        let mut report = self.shared_selected(left, right, category)?;
        let rows = report["items"]
            .as_array_mut()
            .ok_or("Invalid shared report")?;
        *rows = rows.iter().skip(offset).take(100).cloned().collect();
        for row in rows {
            for side in ["left", "right"] {
                let references = row[side]
                    .as_array_mut()
                    .ok_or("Invalid shared references")?;
                let total = references.len();
                references.truncate(100);
                row[format!("{side}_total")] = json!(total);
            }
        }
        report["offset"] = json!(offset);
        Ok(report)
    }

    /// Direct byte bindings, not an inferred dependency or eviction census.
    pub fn object_references(&self, sha: &str, project: &str, offset: usize) -> Result<Value> {
        let digest = git_asset_core::Digest::parse(sha).map_err(|_| "Invalid SHA-256")?;
        if offset > 1_000_000 || (project != "all" && !self.projects.contains_key(project)) {
            return Err("Unknown project or reference-page budget exceeded".into());
        }
        let mut rows = Vec::new();
        let mut total = 0usize;
        let mut object: Option<Object> = None;
        let mut snapshots = BTreeMap::new();
        for (id, p) in self
            .projects
            .iter()
            .filter(|(id, _)| project == "all" || id.as_str() == project)
        {
            snapshots.insert(id, &p.commit);
            for asset in p.snapshot.selected_records() {
                let selected = asset.selected().ok().map(|o| ("selected", "", "", o));
                for (slot, role, group, o) in
                    selected
                        .into_iter()
                        .chain(asset.stream_bindings().iter().map(|s| {
                            (
                                s.slot.as_str(),
                                s.role.as_str(),
                                s.group.as_str(),
                                &s.object,
                            )
                        }))
                {
                    if o.sha256 != digest {
                        continue;
                    }
                    if object.as_ref().is_some_and(|old| old.bytes != o.bytes) {
                        return Err("Conflicting lengths for the same SHA-256".into());
                    }
                    object = Some(o.clone());
                    if total >= offset && rows.len() < 100 {
                        rows.push(json!({"project":id,"asset":asset.reference().id,"definition":asset.definition().id,
                        "binding":if role.is_empty(){"selected"}else{"stream"},"slot":slot,"role":role,"group":group}));
                    }
                    total += 1;
                }
            }
        }
        Ok(
            json!({"schema":"tig.object-references.experimental-1","sha256":sha,"object":object,
        "snapshots":snapshots,"total":total,"offset":offset,"items":rows,"payloads_opened":0,
        "scope":"Direct selected and named-stream bindings in enrolled native records only; transitive dependencies, nested inventories, other LFS pointers and unregistered files are not indexed.",
        "complete_project_comparison":false,"authorizes_eviction":false}),
        )
    }
}
