//! Cross-project location accounting. File-copy observations are not custody.
use super::*;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Domain {
    pub id: String,
    pub label: String,
    pub root: PathBuf,
    #[serde(default)]
    pub live_probe: bool,
}
pub fn validate(domains: &[Domain]) -> Result<()> {
    let mut ids = BTreeSet::new();
    let mut roots = BTreeSet::new();
    if domains.len() > 32 {
        return Err("Storage domain budget exceeded".into());
    }
    for d in domains {
        if !token(&d.id)
            || d.label.is_empty()
            || d.label.len() > 120
            || !d.root.is_absolute()
            || !ids.insert(&d.id)
            || !roots.insert(normalize(&d.root))
        {
            return Err("Invalid or duplicate storage domain".into());
        }
    }
    Ok(())
}
fn normalize(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s).to_owned();
    let s = if s.as_bytes().get(1) == Some(&b':') {
        s.to_lowercase()
    } else {
        s
    };
    s.trim_end_matches('/').to_owned()
}
fn contains(root: &Path, path: &Path) -> bool {
    let r = normalize(root);
    let p = normalize(path);
    p == r || p.starts_with(&(r + "/"))
}
#[derive(Default)]
pub(super) struct Cache {
    pub(super) root: String,
    pub(super) resolved_root: Option<PathBuf>,
    pub(super) domain: Option<String>,
    pub(super) projects: BTreeSet<String>,
    pub(super) observations: Vec<Value>,
}
pub(super) struct Sample {
    pub(super) sha: String,
    pub(super) cache: String,
    pub(super) domain: Option<String>,
    pub(super) bytes: u64,
    pub(super) allocated: Option<u64>,
    pub(super) time: Option<u64>,
    pub(super) projects: BTreeSet<String>,
    pub(super) conflict: bool,
}
#[derive(Default)]
struct Totals {
    copies: usize,
    objects: BTreeSet<String>,
    pub(super) bytes: u64,
    allocation: u64,
    unknown: usize,
    conflicts: usize,
}
impl Totals {
    fn add(&mut self, s: &Sample) -> Result<()> {
        self.copies += 1;
        self.objects.insert(s.sha.clone());
        add(&mut self.bytes, s.bytes)?;
        if let Some(n) = s.allocated {
            add(&mut self.allocation, n)?;
        } else {
            self.unknown += 1;
        }
        if s.conflict {
            self.conflicts += 1;
        }
        Ok(())
    }
    fn value(&self) -> Value {
        json!({"file_copies":self.copies,"unique_objects":self.objects.len(),"recorded_file_bytes":self.bytes,"observed_allocated_bytes":self.allocation,"copies_with_unknown_allocation":self.unknown,"conflicting_observations":self.conflicts})
    }
}
pub(super) struct Captured {
    pub declared: BTreeMap<String, BTreeSet<String>>,
    pub caches: BTreeMap<String, Cache>,
    pub samples: BTreeMap<(String, String), Sample>,
    pub snapshots: BTreeMap<String, String>,
    pub locators: BTreeMap<(String, usize), String>,
}
impl Catalogue {
    pub(super) fn capture_storage_index(&self, measure: bool) -> Result<Captured> {
        let mut references = BTreeMap::<String, BTreeSet<String>>::new();
        let mut declared = BTreeMap::<String, BTreeSet<String>>::new();
        for (id, p) in &self.projects {
            let set = declared.entry(id.clone()).or_default();
            for a in p.snapshot.selected_records() {
                for o in a
                    .selected()
                    .ok()
                    .into_iter()
                    .chain(a.stream_bindings().iter().map(|s| &s.object))
                {
                    let hash = o.sha256.as_str().to_owned();
                    set.insert(hash.clone());
                    references.entry(hash).or_default().insert(id.clone());
                    if references.len() > 500_000 {
                        return Err("Global reference budget exceeded".into());
                    }
                }
            }
            for r in p.nested.values().flat_map(|n| n.refs.iter()) {
                let hash = r.object.sha256.as_str().to_owned();
                set.insert(hash.clone());
                references.entry(hash).or_default().insert(id.clone());
                if references.len() > 500_000 {
                    return Err("Global nested reference budget exceeded".into());
                }
            }
        }
        let mut locators = BTreeMap::new();
        let mut caches = BTreeMap::<String, Cache>::new();
        let mut samples = BTreeMap::<(String, String), Sample>::new();
        let mut snapshots = BTreeMap::new();
        for (id, p) in &self.projects {
            snapshots.insert(id.clone(), p.commit.clone());
            let Some(reader) = &p.profile.byte_source else {
                continue;
            };
            for (index, location) in reader.places.iter().enumerate() {
                let may_resolve = measure
                    && p.enrollment.storage.as_ref().is_some_and(|s| {
                        s.places
                            .iter()
                            .any(|place| place.place_index == index && place.live_probe)
                    })
                    && self
                        .storage_domains
                        .iter()
                        .any(|d| d.live_probe && contains(&d.root, &location.root));
                let resolved = if may_resolve {
                    std::fs::canonicalize(&location.root).ok()
                } else {
                    None
                };
                let effective_root = resolved.as_deref().unwrap_or(&location.root);
                // Exact provider locator, not a claim that different roots are independent.
                let key = format!("{}|{:?}", normalize(effective_root), location.layout);
                locators.insert((id.clone(), index), key.clone());
                let domain = self
                    .storage_domains
                    .iter()
                    .filter(|d| contains(&d.root, effective_root))
                    .max_by_key(|d| normalize(&d.root).len())
                    .map(|d| d.id.clone());
                let cache = caches.entry(key.clone()).or_insert_with(|| Cache {
                    root: location.root.to_string_lossy().into_owned(),
                    resolved_root: resolved.clone(),
                    domain: domain.clone(),
                    ..Cache::default()
                });
                cache.projects.insert(id.clone());
                for observation in p.storage_sources.values() {
                    let Some(profile) = &p.enrollment.storage else {
                        continue;
                    };
                    let Some(place) = profile
                        .places
                        .iter()
                        .find(|place| place.place_index == index)
                    else {
                        continue;
                    };
                    let tier = if observation.report.schema
                        == "tig.registered-observation.experimental-1"
                    {
                        Some(index)
                    } else {
                        place.report_tier
                    };
                    let Some(tier) =
                        tier.filter(|t| observation.report.tiers.iter().any(|v| v.tier == *t))
                    else {
                        continue;
                    };
                    cache.observations.push(json!({"project":id,"source":observation.provenance,"observed_at":observation.report.census_completed_unix_seconds,"complete":false}));
                    for e in &observation.entries {
                        for copy in e.row.copies.iter().filter(|c| c.tier == tier) {
                            let sample = Sample {
                                sha: e.row.sha256.clone(),
                                cache: key.clone(),
                                domain: domain.clone(),
                                bytes: copy.bytes,
                                allocated: copy.allocated_bytes,
                                time: observation.report.census_completed_unix_seconds,
                                projects: references
                                    .get(&e.row.sha256)
                                    .cloned()
                                    .unwrap_or_default(),
                                conflict: false,
                            };
                            let k = (key.clone(), e.row.sha256.clone());
                            if let Some(old) = samples.get_mut(&k) {
                                let conflict = old.conflict
                                    || old.bytes != sample.bytes
                                    || old
                                        .allocated
                                        .zip(sample.allocated)
                                        .is_some_and(|(a, b)| a != b);
                                if sample.time > old.time {
                                    *old = sample;
                                }
                                old.conflict = conflict;
                            } else {
                                samples.insert(k, sample);
                            }
                            if samples.len() > 1_000_000 {
                                return Err("Global copy budget exceeded".into());
                            }
                        }
                    }
                }
            }
        }
        Ok(Captured {
            declared,
            caches,
            samples,
            snapshots,
            locators,
        })
    }

    pub fn global_storage(
        &self,
        project: &str,
        domain: &str,
        measure: bool,
        offset: usize,
    ) -> Result<Value> {
        if (!project.is_empty() && !self.projects.contains_key(project))
            || (!domain.is_empty() && !self.storage_domains.iter().any(|d| d.id == domain))
            || offset > 1_000_000
        {
            return Err("Invalid global storage filter".into());
        }
        let Captured {
            declared,
            caches,
            samples,
            snapshots,
            ..
        } = self.capture_storage_index(measure)?;
        let mut total = Totals::default();
        let mut domains = BTreeMap::<String, Totals>::new();
        let mut by_cache = BTreeMap::<String, Totals>::new();
        let mut by_project = BTreeMap::<String, Totals>::new();
        let mut rows = vec![];
        let mut count = 0;
        for s in samples.values().filter(|s| {
            (project.is_empty() || s.projects.contains(project))
                && (domain.is_empty() || s.domain.as_deref() == Some(domain))
        }) {
            total.add(s)?;
            if let Some(d) = &s.domain {
                domains.entry(d.clone()).or_default().add(s)?;
            }
            by_cache.entry(s.cache.clone()).or_default().add(s)?;
            for p in &s.projects {
                by_project.entry(p.clone()).or_default().add(s)?;
            }
            if count >= offset && rows.len() < 100 {
                rows.push(json!({"sha256":s.sha,"cache":s.cache,"domain":s.domain,"bytes":s.bytes,"allocated_bytes":s.allocated,"referencing_projects":s.projects,"conflicting_observations":s.conflict}));
            }
            count += 1;
        }
        let cache_rows:Vec<_>=caches.iter().filter(|(_,c)|domain.is_empty()||c.domain.as_deref()==Some(domain)).map(|(id,c)|json!({"id":id,"root":c.root,"resolved_root":c.resolved_root,"domain":c.domain,"enrolled_projects":c.projects,"observations":c.observations,"totals":by_cache.get(id).map(Totals::value)})).collect();
        let drives:Vec<_>=self.storage_domains.iter().map(|d|{
            let capacity=if measure&&d.live_probe{fs2::total_space(&d.root).ok().zip(fs2::available_space(&d.root).ok())}else{None};
            json!({"id":d.id,"label":d.label,"root":d.root,"live_probe_allowed":d.live_probe,"capacity_measured":capacity.is_some(),"total":capacity.map(|v|v.0),"free":capacity.map(|v|v.1),"totals":domains.get(&d.id).map(Totals::value),"cache_count":caches.values().filter(|c|c.domain.as_deref()==Some(&d.id)).count()})
        }).collect();
        let projects:Vec<_>=self.projects.iter().map(|(id,p)|json!({"id":id,"label":p.enrollment.label,"declared_objects":declared.get(id).map_or(0,|s|s.len()),"storage_enabled":p.enrollment.storage.is_some(),"has_observation":p.storage.is_some(),"totals":by_project.get(id).map(Totals::value)})).collect();
        Ok(
            json!({"snapshots":snapshots,"summary":total.value(),"domains":drives,"caches":cache_rows,"projects":projects,"objects":rows,"objects_total":count,"offset":offset,"coverage_complete":false,"removal_authorized":false,"safe_reclaimable_bytes":null,"limitations":"Observed file copies deduplicate exact provider locators across projects. Different cache roots can still alias physical bytes. Project reference totals overlap and are not placement demand. Missing/unloaded observations remain unknown; directory, opaque dependency and current-job coverage is incomplete. Volume usage is separate from observed file-copy allocation; do not subtract or sum them."}),
        )
    }
}
