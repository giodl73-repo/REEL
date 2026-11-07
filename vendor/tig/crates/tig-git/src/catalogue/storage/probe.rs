//! Explicit named-object metadata refresh, never a recursive census or scrub.
use super::*;
use std::{
    fs,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
fn physical(path: &Path) -> std::io::Result<(Option<String>, u64, Option<u64>)> {
    // Safe standard-library metadata does not expose stable Windows file identity.
    // Keep identity/link coverage unknown instead of treating paths as physical IDs.
    use std::os::windows::fs::OpenOptionsExt;
    // FILE_READ_ATTRIBUTES only; never request payload read access for accounting.
    let file = std::fs::OpenOptions::new()
        .access_mode(0x80)
        .share_mode(0x1 | 0x2 | 0x4)
        .open(path)?;
    Ok((None, fs2::FileExt::allocated_size(&file)?, None))
}
#[cfg(unix)]
fn physical(path: &Path) -> std::io::Result<(Option<String>, u64, Option<u64>)> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::metadata(path)?;
    Ok((
        Some(format!("{}:{}", m.dev(), m.ino())),
        m.blocks()
            .checked_mul(512)
            .ok_or(std::io::Error::other("Allocation overflow"))?,
        Some(m.nlink()),
    ))
}
#[cfg(not(any(unix, windows)))]
fn physical(_: &Path) -> std::io::Result<(Option<String>, u64, Option<u64>)> {
    Err(std::io::Error::other("Unsupported metadata"))
}

fn copy(path: &Path, tier: usize, expected: u64) -> Option<Copy> {
    let m = fs::symlink_metadata(path).ok()?;
    if !m.is_file() || m.file_type().is_symlink() {
        return None;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return None;
        }
    }
    let p = physical(path).ok();
    Some(Copy {
        tier,
        bytes: m.len(),
        allocated_bytes: p.as_ref().map(|v| v.1),
        physical_id: p.as_ref().and_then(|v| v.0.clone()),
        hard_links: p.as_ref().and_then(|v| v.2),
        other_hard_links: None,
        descriptor_size_matches: Some(m.len() == expected),
    })
}
impl Catalogue {
    pub fn observe_registered(
        &mut self,
        project: &str,
        commit: &str,
        family: &str,
    ) -> Result<Value> {
        let p = self.projects.get_mut(project).ok_or("Unknown project")?;
        p.check_snapshot(commit)?;
        if family.len() > 120 {
            return Err("Family budget exceeded".into());
        }
        let profile = p.enrollment.storage.as_ref().ok_or("No storage profile")?;
        let reader = p.profile.byte_source.as_ref().ok_or("No byte locations")?;
        let probe_budget = Duration::from_millis(profile.probe_millis);
        let started = Instant::now();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "Clock refused")?
            .as_secs();
        let mut objects = BTreeMap::<String, Entry>::new();
        let mut admitted = BTreeMap::<String, BTreeSet<(String, String)>>::new();
        let fallback:works::Collection=serde_json::from_value(json!({"id":"all-records","label":"Assets","icon":"asset","roots":[{"pattern":"^(.+)$","key":"${1}"}]})).map_err(|_|"Fallback projection invalid")?;
        let covered: BTreeSet<_> = p
            .works
            .iter()
            .filter(|w| w.top_level)
            .flat_map(|w| w.members.iter().copied())
            .collect();
        let mut works = p.works.clone();
        works.extend(
            works::build(std::slice::from_ref(&fallback), &p.snapshot, &p.cards)
                .into_iter()
                .filter(|w| w.members.iter().all(|i| !covered.contains(i))),
        );
        for w in works
            .iter()
            .filter(|w| w.top_level && (family.is_empty() || w.category == family))
        {
            let c = if w.collection == "all-records" {
                &fallback
            } else {
                p.enrollment
                    .collections
                    .iter()
                    .find(|c| c.id == w.collection)
                    .ok_or("Collection unavailable")?
            };
            for i in &w.members {
                admitted
                    .entry(p.cards[*i].id.clone())
                    .or_default()
                    .insert((w.category.clone(), w.id.clone()));
                let a = p
                    .snapshot
                    .resolve_id(&p.cards[*i].id)
                    .map_err(|_| "Asset unavailable")?;
                let root = p.cards[*i].id == w.root;
                let selected = a.selected().ok();
                let subjects = selected
                    .into_iter()
                    .map(|o| (o, None))
                    .chain(a.stream_bindings().iter().map(|s| (&s.object, Some(s))));
                for (o, stream) in subjects {
                    let e = objects
                        .entry(o.sha256.as_str().to_owned())
                        .or_insert_with(|| Entry {
                            row: Row {
                                sha256: o.sha256.as_str().to_owned(),
                                bytes: Some(o.bytes),
                                category: "registered".into(),
                                copies: vec![],
                                direct_version_ids: vec![],
                                registered: true,
                                deprecated_reference: false,
                                noncurrent_reference: false,
                                required_by_current_version: false,
                                required_by_selected_version: false,
                                pinned: false,
                                leased: false,
                            },
                            state: "unknown",
                            owners: BTreeSet::new(),
                        });
                    if e.row.bytes != Some(o.bytes) {
                        return Err("Conflicting declared lengths".into());
                    }
                    e.owners.insert((w.category.clone(), w.id.clone()));
                    if selected == Some(o) {
                        e.row.required_by_selected_version = true;
                    }
                    if let Some(s) =
                        stream.filter(|s| root && c.version_role.as_ref() == Some(&s.role))
                    {
                        let version = format!("{}@{}", w.id, s.slot);
                        if !e.row.direct_version_ids.contains(&version) {
                            e.row.direct_version_ids.push(version);
                        }
                        if c.version_slot_prefix
                            .as_ref()
                            .zip(w.version.as_ref())
                            .is_some_and(|(prefix, v)| s.slot == format!("{prefix}{v}"))
                        {
                            e.row.required_by_current_version = true;
                        } else {
                            e.row.noncurrent_reference = true;
                        }
                    }
                }
                if objects.len() > 100000 || started.elapsed() > probe_budget {
                    return Err("Registered observation budget exceeded; choose one family".into());
                }
            }
        }
        for r in p.nested.values().flat_map(|n| n.refs.iter()) {
            let Some(owners) = admitted.get(&r.asset) else {
                continue;
            };
            let e = objects
                .entry(r.object.sha256.as_str().into())
                .or_insert_with(|| Entry {
                    row: Row {
                        sha256: r.object.sha256.as_str().into(),
                        bytes: Some(r.object.bytes),
                        category: "nested".into(),
                        copies: vec![],
                        direct_version_ids: vec![],
                        registered: true,
                        deprecated_reference: false,
                        noncurrent_reference: false,
                        required_by_current_version: false,
                        required_by_selected_version: false,
                        pinned: false,
                        leased: false,
                    },
                    state: "unknown",
                    owners: BTreeSet::new(),
                });
            if e.row.bytes != Some(r.object.bytes) {
                return Err("Conflicting nested object sizes".into());
            }
            e.owners.extend(owners.iter().cloned());
            if objects.len() > 100_000 {
                return Err("Nested observation budget exceeded".into());
            }
        }
        if objects.is_empty() {
            return Err("No registered work objects in this family".into());
        }
        let mut tiers = vec![];
        for place in profile.places.iter().filter(|place| place.live_probe) {
            let location = reader
                .places
                .get(place.place_index)
                .ok_or("Unknown byte location")?;
            let Ok(root) = fs::canonicalize(&location.root) else {
                continue;
            };
            tiers.push(Tier {
                tier: place.place_index,
                total_bytes: None,
                free_bytes: None,
            });
            let mut parents = BTreeMap::<PathBuf, bool>::new();
            for entry in objects.values_mut() {
                if started.elapsed() > probe_budget {
                    return Err(
                        "Named-object refresh time budget exceeded; old view preserved".into(),
                    );
                }
                let oid =
                    tig_core::Digest::parse(&entry.row.sha256).map_err(|_| "Invalid digest")?;
                let path = location.layout.object_path(&location.root, &oid);
                let parent = path.parent().ok_or("Invalid object locator")?.to_owned();
                let admitted = *parents.entry(parent.clone()).or_insert_with(|| {
                    fs::canonicalize(&parent).is_ok_and(|p| p.starts_with(&root))
                });
                if !admitted {
                    continue;
                }
                if let Some(copy) = copy(&path, place.place_index, entry.row.bytes.unwrap()) {
                    entry.row.copies.push(copy);
                }
            }
        }
        let declared = objects.len();
        let absent = objects.values().filter(|e| e.row.copies.is_empty()).count();
        let report = Report {
            schema: "tig.registered-observation.experimental-1".into(),
            project_id: project.into(),
            registry_generation: 0,
            census_started_unix_seconds: Some(now),
            census_completed_unix_seconds: Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| "Clock refused")?
                    .as_secs(),
            ),
            coverage_complete: false,
            tiers,
            objects: vec![],
        };
        let entries = objects
            .into_values()
            .map(|mut e| {
                e.state = state(&e.row);
                e
            })
            .collect();
        p.adopt_storage(Observation {
            report,
            entries,
            provenance: json!({"kind":"registered-object-metadata","commit":commit,"family":family,"declared_objects":declared,"objects_not_observed":absent,"payloads_read":0,"hashes_verified":false,"complete_dependency_coverage":false,"directory_scan":false}),
        });
        Ok(
            json!({"observed":true,"declared_objects":declared,"objects_not_observed":absent,"payloads_read":0,"removal_authorized":false}),
        )
    }
}
