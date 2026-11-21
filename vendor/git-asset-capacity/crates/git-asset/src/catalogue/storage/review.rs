//! Generic coverage and declared-version review. No storage mutation authority.
use super::*;

impl Catalogue {
    pub fn coverage(
        &self,
        project: &str,
        collection: &str,
        status: &str,
        q: &str,
        offset: usize,
    ) -> Result<Value> {
        if q.len() > 256
            || collection.len() > 80
            || offset > 1_000_000
            || (!project.is_empty() && !self.projects.contains_key(project))
            || ![
                "",
                "expanded",
                "partial",
                "not-loaded",
                "unconfigured",
                "unmatched",
                "configuration-error",
            ]
            .contains(&status)
        {
            return Err("Invalid coverage query".into());
        }
        let mut rows = vec![];
        let mut counts = BTreeMap::<String, usize>::new();
        let query = q.to_lowercase();
        for (id, p) in &self.projects {
            if !project.is_empty() && id != project {
                continue;
            }
            for w in p
                .works
                .iter()
                .filter(|w| w.top_level && (collection.is_empty() || w.collection == collection))
            {
                if !query.is_empty()
                    && !format!("{} {} {}", w.id, w.title, w.category)
                        .to_lowercase()
                        .contains(&query)
                {
                    continue;
                }
                let loaded = p.nested.get(&format!("{}/{}", w.collection, w.id));
                let (roots, error) = match nested::root_count(p, w) {
                    Ok(n) => (n, None),
                    Err(e) => (None, Some(e)),
                };
                let failures = loaded
                    .and_then(|n| n.summary["failures"].as_array())
                    .map_or(0, Vec::len);
                let state = if error.is_some() {
                    "configuration-error"
                } else if loaded.is_some() {
                    if failures > 0 { "partial" } else { "expanded" }
                } else {
                    match roots {
                        None => "unconfigured",
                        Some(0) => "unmatched",
                        _ => "not-loaded",
                    }
                };
                *counts.entry(state.into()).or_default() += 1;
                if !status.is_empty() && state != status {
                    continue;
                }
                rows.push(json!({"work":w,"state":state,"roots":roots,"failures":failures,"error":error,"loaded":loaded.map(|n|json!({"basis":n.basis,"content":n.content,"successful_roots":n.summary["successful_roots"],"documents_read":n.summary["documents_read"],"metadata_bytes":n.summary["metadata_bytes"]}))}));
            }
        }
        rows.sort_by(|a, b| {
            a["work"]["project"]
                .as_str()
                .cmp(&b["work"]["project"].as_str())
                .then(
                    a["work"]["title"]
                        .as_str()
                        .cmp(&b["work"]["title"].as_str()),
                )
        });
        let snapshots = self
            .projects
            .iter()
            .map(|(id, p)| (id, &p.commit))
            .collect::<BTreeMap<_, _>>();
        let basis=Object::from_bytes(&serde_json::to_vec(&json!({"rows":rows,"counts":counts,"snapshots":snapshots,"query":[project,collection,status,q]})).map_err(|_|"Coverage basis refused")?);
        Ok(
            json!({"basis":basis.sha256,"items":rows.iter().skip(offset).take(100).collect::<Vec<_>>(),"total":rows.len(),"states_before_status_filter":counts,"snapshots":snapshots,"metadata_reads":0,"complete_dependency_coverage":false}),
        )
    }
}

pub struct Comparison {
    project: String,
    commit: String,
    root: String,
    from: String,
    to: String,
    old: Object,
    new: Object,
    metadata: Option<nested::Request>,
}
pub struct Compared {
    request: Comparison,
    old: BTreeMap<String, Object>,
    new: BTreeMap<String, Object>,
    metadata: Option<Value>,
}
fn partition<'a>(
    old: &'a BTreeMap<String, Object>,
    new: &'a BTreeMap<String, Object>,
) -> Result<[Vec<&'a Object>; 3]> {
    for (h, o) in old {
        if new.get(h).is_some_and(|n| n.bytes != o.bytes) {
            return Err("Conflicting version object sizes".into());
        }
    }
    Ok([
        new.iter()
            .filter(|(h, _)| !old.contains_key(*h))
            .map(|(_, o)| o)
            .collect(),
        new.iter()
            .filter(|(h, _)| old.contains_key(*h))
            .map(|(_, o)| o)
            .collect(),
        old.iter()
            .filter(|(h, _)| !new.contains_key(*h))
            .map(|(_, o)| o)
            .collect(),
    ])
}
pub fn execute(request: Comparison) -> Result<Compared> {
    let mut old = BTreeMap::from([(request.old.sha256.as_str().into(), request.old.clone())]);
    let mut new = BTreeMap::from([(request.new.sha256.as_str().into(), request.new.clone())]);
    let mut metadata = None;
    if let Some(r) = &request.metadata {
        let loaded = nested::execute(r)?;
        for a in loaded.refs {
            let set = if a.root_slot == request.from {
                &mut old
            } else {
                &mut new
            };
            set.insert(a.object.sha256.as_str().into(), a.object.clone());
            if request.from == request.to {
                new.insert(a.object.sha256.as_str().into(), a.object);
            }
        }
        metadata = Some(loaded.summary);
    }
    for (h, a) in &old {
        if new.get(h).is_some_and(|b| a.bytes != b.bytes) {
            return Err("Conflicting version object sizes".into());
        }
    }
    Ok(Compared {
        request,
        old,
        new,
        metadata,
    })
}
impl Catalogue {
    pub fn comparison(
        &self,
        project: &str,
        commit: &str,
        collection: &str,
        work: &str,
        from: &str,
        to: &str,
    ) -> Result<Comparison> {
        let p = self.projects.get(project).ok_or("Unknown project")?;
        p.check_snapshot(commit)?;
        let w = p
            .works
            .iter()
            .find(|w| w.collection == collection && w.id == work)
            .ok_or("Unknown work")?;
        let c = p
            .enrollment
            .collections
            .iter()
            .find(|c| c.id == collection)
            .ok_or("Unknown collection")?;
        let role = c
            .version_role
            .as_ref()
            .ok_or("No declared version interpretation enrolled")?;
        let a = p
            .snapshot
            .resolve_id(&w.root)
            .map_err(|_| "Version root unavailable")?;
        let find = |slot: &str| {
            a.stream_bindings()
                .iter()
                .find(|s| s.slot == slot && &s.role == role)
                .map(|s| s.object.clone())
                .ok_or("Unknown declared version slot")
        };
        let old = find(from)?;
        let new = find(to)?;
        let metadata = self.version_metadata(
            (project, commit, collection, work),
            [(from, &old), (to, &new)],
        )?;
        Ok(Comparison {
            project: project.into(),
            commit: commit.into(),
            root: w.root.clone(),
            from: from.into(),
            to: to.into(),
            old,
            new,
            metadata,
        })
    }
    pub fn finish_comparison(&self, compared: Compared) -> Result<Value> {
        let r = compared.request;
        self.projects
            .get(&r.project)
            .ok_or("Unknown project")?
            .check_snapshot(&r.commit)?;
        let [added, shared, removed_rows] = partition(&compared.old, &compared.new)?;
        let removed: BTreeSet<_> = removed_rows.iter().map(|o| o.sha256.as_str()).collect();
        let mut other = BTreeMap::<String, BTreeSet<(String, String)>>::new();
        let mut scanned = 0usize;
        for (id, p) in &self.projects {
            for a in p.snapshot.selected_records() {
                if id == &r.project && a.reference().id == r.root {
                    continue;
                }
                for o in a
                    .selected()
                    .ok()
                    .into_iter()
                    .chain(a.stream_bindings().iter().map(|s| &s.object))
                {
                    scanned += 1;
                    if scanned > 500_000 {
                        return Err("Other-reference review budget exceeded".into());
                    }
                    if removed.contains(o.sha256.as_str()) {
                        other
                            .entry(o.sha256.as_str().into())
                            .or_default()
                            .insert((id.clone(), a.reference().id.clone()));
                    }
                }
            }
            for a in p.nested.values().flat_map(|n| n.refs.iter()) {
                scanned += 1;
                if scanned > 500_000 {
                    return Err("Other-reference review budget exceeded".into());
                }
                if removed.contains(a.object.sha256.as_str())
                    && !(id == &r.project && a.asset == r.root)
                {
                    other
                        .entry(a.object.sha256.as_str().into())
                        .or_default()
                        .insert((id.clone(), a.asset.clone()));
                }
            }
        }
        let index = self.capture_storage_index(false)?;
        let mut copies = BTreeMap::<&str, Vec<Value>>::new();
        for s in index.samples.values() {
            copies.entry(&s.sha).or_default().push(
                json!({"root":index.caches[&s.cache].root,"bytes":s.bytes,"observed_at":s.time}),
            );
        }
        let group = |objects: Vec<&Object>| -> Result<Value> {
            let mut total = 0;
            let mut rows = vec![];
            for o in objects {
                add(&mut total, o.bytes)?;
                let refs = other.get(o.sha256.as_str());
                rows.push(json!({"object":o,"known_other_records":refs.map_or(0,BTreeSet::len),"other_records":refs.map(|v|v.iter().take(6).collect::<Vec<_>>()),"observed_copies":copies.get(o.sha256.as_str()).cloned().unwrap_or_default()}));
            }
            rows.sort_by(|a, b| {
                b["object"]["bytes"]
                    .as_u64()
                    .cmp(&a["object"]["bytes"].as_u64())
                    .then(
                        a["object"]["sha256"]
                            .as_str()
                            .cmp(&b["object"]["sha256"].as_str()),
                    )
            });
            Ok(
                json!({"count":rows.len(),"bytes":total,"items":rows.into_iter().take(100).collect::<Vec<_>>()}),
            )
        };
        let mut result = json!({"schema":"tig.version-comparison.experimental-1","project":r.project,"commit":r.commit,"snapshots":index.snapshots,"root":r.root,"from":r.from,"to":r.to,"from_object":r.old,"to_object":r.new,"added":group(added)?,"shared":group(shared)?,"removed":group(removed_rows)?,"removed_with_other_records":other.len(),"configured_paths_complete":compared.metadata.as_ref().is_some_and(|n|n["failures"].as_array().is_some_and(Vec::is_empty)),"metadata":compared.metadata,"other_reference_coverage_complete":false,"complete_dependency_coverage":false,"safe_reclaimable_bytes":null,"removal_authorized":false,"executable":false});
        let basis = Object::from_bytes(
            &serde_json::to_vec(&result).map_err(|_| "Comparison encoding refused")?,
        );
        result["basis"] = json!(basis.sha256);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thousand_objects_thirteen_replacements_and_unchanged_view() {
        let old: BTreeMap<_, _> = (0..1000)
            .map(|i| {
                let o = Object::from_bytes(format!("item-{i}").as_bytes());
                (o.sha256.as_str().to_owned(), o)
            })
            .collect();
        let mut new = old.clone();
        for h in old.keys().take(13) {
            new.remove(h);
        }
        for i in 0..13 {
            let o = Object::from_bytes(format!("replacement-{i}").as_bytes());
            new.insert(o.sha256.as_str().into(), o);
        }
        let [added, shared, removed] = partition(&old, &new).unwrap();
        assert_eq!((added.len(), shared.len(), removed.len()), (13, 987, 13));
        let [a, s, r] = partition(&old, &old).unwrap();
        assert!(a.is_empty() && r.is_empty());
        assert_eq!(s.len(), 1000);
        let mut bad = old.clone();
        bad.values_mut().next().unwrap().bytes += 1;
        assert!(partition(&old, &bad).is_err());
    }
}
