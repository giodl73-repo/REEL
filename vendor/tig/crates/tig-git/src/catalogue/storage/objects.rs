//! Named byte objects, project-owned placement preferences and held previews.
use super::*;
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementRule {
    pub id: String,
    pub label: String,
    pub pattern: String,
    pub usages: Vec<String>,
    pub place_index: usize,
    #[serde(default)]
    pub priority: u16,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub field_pattern: Option<String>,
}
const USAGES: [&str; 4] = [
    "current-revision",
    "previous-revision",
    "selected-subject",
    "attachment",
];
pub fn validate_rules(rules: &[PlacementRule], _places: usize) -> Result<()> {
    if rules.len() > 64 {
        return Err("Placement rule budget exceeded".into());
    }
    let mut ids = BTreeSet::new();
    for r in rules {
        if r.field.is_some() != r.field_pattern.is_some()
            || r.field.as_ref().is_some_and(|p| {
                p.len() > 256 || !(p.starts_with("/authored/") || p.starts_with("/status/"))
            })
        {
            return Err("Invalid admitted placement field".into());
        }
        if let Some(pattern) = &r.field_pattern {
            if !pattern.starts_with('^') || pattern.len() > 512 {
                return Err("Invalid placement field pattern".into());
            }
            regex::RegexBuilder::new(pattern)
                .size_limit(1024 * 1024)
                .build()
                .map_err(|_| "Invalid placement field expression")?;
        }
        if !token(&r.id)
            || !ids.insert(&r.id)
            || r.label.is_empty()
            || r.label.len() > 120
            || r.pattern.len() > 512
            || !r.pattern.starts_with('^')
            || r.place_index > 15
            || r.priority > 1000
            || r.usages.is_empty()
            || r.usages.iter().any(|u| !USAGES.contains(&u.as_str()))
        {
            return Err("Invalid placement preference".into());
        }
        regex::RegexBuilder::new(&r.pattern)
            .size_limit(1024 * 1024)
            .build()
            .map_err(|_| "Invalid placement pattern")?;
    }
    Ok(())
}
#[derive(Clone, Serialize)]
struct Binding {
    project: String,
    id: String,
    title: String,
    category: String,
    slot: String,
    role: String,
    usage: String,
    version: Option<String>,
    selected: Option<Object>,
    status: BTreeMap<String, String>,
    nested: Option<Value>,
}
#[derive(Default)]
struct Item {
    bytes: u64,
    bindings: Vec<Binding>,
    copies: Vec<Value>,
    targets: BTreeMap<String, Value>,
    anomalies: BTreeSet<String>,
    conflict: bool,
}
pub struct Query<'a> {
    pub project: &'a str,
    pub kind: &'a str,
    pub usage: &'a str,
    pub anomaly: &'a str,
    pub primary: &'a str,
    pub observed: &'a str,
    pub q: &'a str,
    pub sort: &'a str,
    pub offset: usize,
    pub sha: &'a str,
}
fn cache_id(key: &str) -> String {
    Object::from_bytes(key.as_bytes())
        .sha256
        .as_str()
        .to_owned()
}
impl Catalogue {
    pub fn object_copy_request(
        &self,
        project: &str,
        sha: &str,
        cache: &str,
        basis: &str,
    ) -> Result<(Object, ByteProfile)> {
        let d = self.object_inventory(&Query {
            project,
            kind: "",
            usage: "",
            anomaly: "",
            primary: "",
            observed: "",
            q: "",
            sort: "bytes",
            offset: 0,
            sha,
        })?;
        let row = d["items"]
            .as_array()
            .and_then(|r| r.first())
            .ok_or("Unknown object")?;
        if row["basis"].as_str() != Some(basis)
            || row["anomalies"]
                .as_array()
                .is_some_and(|a| a.iter().any(|a| a == "inconsistent-size"))
        {
            return Err("Object or source basis changed; refresh before verification".into());
        }
        let index = self.capture_storage_index(true)?;
        for ((id, place), key) in index.locators {
            if cache_id(&key) != cache {
                continue;
            }
            let p = &self.projects[&id];
            if !p.enrollment.storage.as_ref().is_some_and(|s| {
                s.places
                    .iter()
                    .any(|p| p.place_index == place && p.live_probe)
            }) {
                continue;
            }
            if let Some(reader) = &p.profile.byte_source {
                let mut reader = reader.clone();
                let place = reader
                    .places
                    .get(place)
                    .ok_or("Unknown byte location")?
                    .clone();
                reader.places = vec![place];
                let object: Object =
                    serde_json::from_value(json!({"sha256":sha,"bytes":row["bytes"]}))
                        .map_err(|_| "Invalid object")?;
                return Ok((object, reader));
            }
        }
        Err("Cache is not enrolled for explicit live verification".into())
    }
    pub fn object_inventory(&self, q: &Query) -> Result<Value> {
        self.object_inventory_selected(q, None)
    }
    pub fn object_batch_preview(
        &self,
        project: &str,
        selection: &str,
        source_cache: &str,
        action: &str,
    ) -> Result<Value> {
        if !matches!(action, "align" | "evict-review")
            || selection.is_empty()
            || selection.len() > 6500
            || source_cache.len() > 64
        {
            return Err("Invalid batch review or request budget".into());
        }
        let mut selected = BTreeMap::new();
        for entry in selection.split(',') {
            let (sha, basis) = entry.split_once(':').ok_or("Invalid selected object")?;
            let valid = |s: &str| {
                s.len() == 64
                    && s.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            };
            if !valid(sha)
                || !valid(basis)
                || selected.insert(sha.to_owned(), basis.to_owned()).is_some()
            {
                return Err("Invalid or duplicate selected identity".into());
            }
        }
        if selected.len() > 50 {
            return Err("Select at most 50 byte objects per review".into());
        }
        let hashes = selected.keys().cloned().collect();
        let result = self.object_inventory_selected(
            &Query {
                project,
                kind: "",
                usage: "",
                anomaly: "",
                primary: "",
                observed: "",
                q: "",
                sort: "bytes",
                offset: 0,
                sha: "",
            },
            Some(&hashes),
        )?;
        if result["total"].as_u64() != Some(selected.len() as u64) {
            return Err(
                "Selection is no longer in this project view; refresh the entire selection".into(),
            );
        }
        if !source_cache.is_empty()
            && !result["caches"]
                .as_array()
                .is_some_and(|c| c.iter().any(|c| c["id"] == source_cache))
        {
            return Err("Unknown observed-cache scope".into());
        }
        let mut plans = vec![];
        let mut holds = BTreeSet::new();
        let mut content = 0u64;
        let mut copy_bytes = 0u64;
        let mut review_bytes = 0u64;
        for row in result["items"]
            .as_array()
            .ok_or("Invalid inventory result")?
        {
            let sha = row["sha256"].as_str().ok_or("Invalid object identity")?;
            let basis = selected.get(sha).ok_or("Unknown selected object")?;
            // Validate every basis before returning any batch result.
            let plan = preview_row(&result, row, basis, action)?;
            add(
                &mut content,
                row["bytes"].as_u64().ok_or("Invalid content size")?,
            )?;
            let mut proposals = vec![];
            for mut p in plan["proposals"]
                .as_array()
                .ok_or("Invalid proposal")?
                .iter()
                .cloned()
            {
                if !source_cache.is_empty() {
                    if p["kind"] == "older-copy-review" && p["cache"] != source_cache {
                        continue;
                    }
                    if p["kind"] == "copy-for-review" {
                        let candidates = p["from_candidates"]
                            .as_array()
                            .ok_or("Invalid sources")?
                            .iter()
                            .filter(|s| *s == source_cache)
                            .cloned()
                            .collect::<Vec<_>>();
                        p["from_candidates"] = json!(candidates);
                    }
                }
                if p["kind"] == "copy-for-review" {
                    if p["from_candidates"].as_array().is_none_or(Vec::is_empty) {
                        holds
                            .insert("No observed source copy in the chosen cache scope".to_owned());
                        continue;
                    }
                    add(
                        &mut copy_bytes,
                        p["bytes"].as_u64().ok_or("Invalid proposed size")?,
                    )?;
                } else {
                    add(
                        &mut review_bytes,
                        p["file_bytes"].as_u64().ok_or("Invalid review size")?,
                    )?;
                }
                proposals.push(p);
            }
            for h in plan["holds"].as_array().ok_or("Invalid holds")? {
                if let Some(h) = h.as_str() {
                    holds.insert(h.to_owned());
                }
            }
            plans.push(json!({"sha256":sha,"title":row["title"],"bytes":row["bytes"],"basis":row["basis"],"proposals":proposals,"holds":plan["holds"],"executable":false}));
        }
        let inputs = json!({"selection":selected,"project":project,"source_cache":source_cache,"action":action,"snapshots":result["snapshots"],"policies":result["policy_pins"]});
        let decision =
            Object::from_bytes(&serde_json::to_vec(&inputs).map_err(|_| "Batch encoding refused")?);
        Ok(
            json!({"count":plans.len(),"action":action,"source_cache":source_cache,"unique_content_bytes":content,"proposed_copy_file_bytes":copy_bytes,"older_copy_review_file_bytes":review_bytes,"safe_reclaimable_bytes":null,"plans":plans,"holds":holds,"caches":result["caches"],"snapshots":result["snapshots"],"policy_pins":result["policy_pins"],"basis":decision.sha256,"executable":false,"removal_authorized":false}),
        )
    }
    fn object_inventory_selected(
        &self,
        q: &Query,
        selected: Option<&BTreeSet<String>>,
    ) -> Result<Value> {
        self.object_inventory_scoped(q, selected, None, Value::Null)
    }
    pub fn work_object_inventory(&self, q: &Query, collection: &str, work: &str) -> Result<Value> {
        if collection.is_empty() && work.is_empty() {
            return self.object_inventory(q);
        }
        if collection.len() > 80 || work.len() > 512 || q.project.is_empty() {
            return Err("Invalid work object scope".into());
        }
        let p = self.projects.get(q.project).ok_or("Unknown project")?;
        let w = p
            .works
            .iter()
            .find(|w| w.collection == collection && w.id == work)
            .ok_or("Unknown work in this project snapshot")?;
        let mut members: BTreeSet<String> =
            w.members.iter().map(|i| p.cards[*i].id.clone()).collect();
        members.insert(w.root.clone());
        let roots = nested::root_count(p, w)?;
        let scope = json!({"project":q.project,"collection":collection,"id":work,"title":w.title,"registered_records":members.len(),"nested_roots":roots,"nested_available":roots.is_some_and(|n|n>0),"nested":p.nested.get(&format!("{collection}/{work}")).map(|n|&n.summary),"complete_dependency_coverage":false});
        self.object_inventory_scoped(q, None, Some(&members), scope)
    }
    fn object_inventory_scoped(
        &self,
        q: &Query,
        selected: Option<&BTreeSet<String>>,
        members: Option<&BTreeSet<String>>,
        scope: Value,
    ) -> Result<Value> {
        if [
            q.project, q.kind, q.usage, q.anomaly, q.primary, q.observed, q.q, q.sha,
        ]
        .iter()
        .any(|s| s.len() > 256)
            || q.offset > 1_000_000
            || !matches!(q.sort, "bytes" | "stored" | "name")
            || (!q.project.is_empty() && !self.projects.contains_key(q.project))
        {
            return Err("Invalid object inventory query".into());
        }
        let index = self.capture_storage_index(true)?;
        let mut objects = BTreeMap::<String, Item>::new();
        let mut policy_pins = BTreeMap::new();
        let caches:Vec<Value>=index.caches.iter().map(|(key,c)|json!({"id":cache_id(key),"root":c.root,"resolved_root":c.resolved_root,"domain":c.domain,"projects":c.projects,"label":c.domain.clone().unwrap_or_else(||c.root.clone()),"observations":c.observations})).collect();
        for (project, p) in &self.projects {
            let rules = p
                .enrollment
                .storage
                .as_ref()
                .map_or(&[][..], |s| s.placement_rules.as_slice());
            let encoded = serde_json::to_vec(&json!({"placement":rules,"nested_profile":p.enrollment.storage.as_ref().and_then(|s|s.nested_content.as_ref()),"nested_scopes":p.nested.iter().map(|(k,n)|(k,(&n.basis,&n.content))).collect::<BTreeMap<_,_>>()})).map_err(|_| "Policy encoding refused")?;
            policy_pins.insert(project.clone(), Object::from_bytes(&encoded));
            let rules: Vec<_> = rules
                .iter()
                .map(|r| {
                    Ok((
                        r,
                        Regex::new(&r.pattern).map_err(|_| "Invalid placement preference")?,
                        r.field_pattern
                            .as_ref()
                            .map(|p| Regex::new(p).map_err(|_| "Invalid placement field"))
                            .transpose()?,
                    ))
                })
                .collect::<Result<_>>()?;
            let roots: BTreeMap<_, _> = p
                .works
                .iter()
                .filter_map(|w| {
                    let c = p
                        .enrollment
                        .collections
                        .iter()
                        .find(|c| c.id == w.collection)?;
                    Some((w.root.as_str(), (w, c)))
                })
                .collect();
            let mut nested =
                BTreeMap::<(&str, &str, &Object), BTreeSet<&nested::Association>>::new();
            for n in p.nested.values().flat_map(|n| n.refs.iter()) {
                nested
                    .entry((&n.asset, &n.root_slot, &n.root))
                    .or_default()
                    .insert(n);
            }
            for card in &p.cards {
                let a = p
                    .snapshot
                    .resolve_id(&card.id)
                    .map_err(|_| "Registered record unavailable")?;
                let document = value(&a);
                let root = roots.get(card.id.as_str());
                let version_of = |slot: &str, role: &str, o: &Object| -> (String, Option<String>) {
                    if let Some((w, c)) =
                        root.filter(|(_, c)| c.version_role.as_deref() == Some(role))
                    {
                        let version = c
                            .version_slot_prefix
                            .as_ref()
                            .and_then(|prefix| slot.strip_prefix(prefix))
                            .map(str::to_owned);
                        if w.version.is_none() || version.is_none() {
                            return ("attachment".into(), version);
                        }
                        let current = c
                            .version_slot_prefix
                            .as_ref()
                            .zip(w.version.as_ref())
                            .is_some_and(|(prefix, v)| slot == format!("{prefix}{v}"))
                            && a.selected().ok() == Some(o);
                        if version.as_ref() == w.version.as_ref() && !current {
                            return ("attachment".into(), version);
                        }
                        return (
                            if current {
                                "current-revision"
                            } else {
                                "previous-revision"
                            }
                            .into(),
                            version,
                        );
                    }
                    ("attachment".into(), None)
                };
                let mut refs = vec![];
                if let Ok(o) = a.selected() {
                    let mut usage = "selected-subject".to_owned();
                    let mut version = None;
                    for s in a.stream_bindings() {
                        if &s.object == o {
                            let (u, v) = version_of(&s.slot, &s.role, o);
                            if u == "current-revision" {
                                usage = u;
                                version = v;
                                break;
                            }
                        }
                    }
                    refs.push((
                        o,
                        usage,
                        version,
                        "selected".to_owned(),
                        "selected-subject".to_owned(),
                    ));
                }
                for s in a.stream_bindings() {
                    let (u, v) = version_of(&s.slot, &s.role, &s.object);
                    refs.push((&s.object, u, v, s.slot.clone(), s.role.clone()));
                }
                let mut expanded: Vec<_> = refs
                    .iter()
                    .map(|(o, u, v, s, r)| (*o, u.clone(), v.clone(), s.clone(), r.clone(), None))
                    .collect();
                for (o, u, v, s, _) in &refs {
                    for n in nested
                        .get(&(card.id.as_str(), s.as_str(), *o))
                        .into_iter()
                        .flatten()
                    {
                        expanded.push((&n.object,u.clone(),v.clone(),format!("{s} → {}",n.pointer),"nested-byte-reference".into(),Some(json!({"root":n.root,"document":n.document,"adapter":n.adapter,"pointer":n.pointer}))));
                    }
                }
                for (o, usage, version, slot, role, nested) in expanded {
                    let item = objects.entry(o.sha256.as_str().to_owned()).or_default();
                    if !item.bindings.is_empty() && item.bytes != o.bytes {
                        item.conflict = true;
                    }
                    item.bytes = o.bytes;
                    let title = if let Some(n) = &nested {
                        format!(
                            "{} · {}",
                            card.title.chars().take(120).collect::<String>(),
                            n["pointer"]
                                .as_str()
                                .unwrap_or("")
                                .chars()
                                .take(120)
                                .collect::<String>()
                        )
                    } else {
                        card.title.clone()
                    };
                    item.bindings.push(Binding {
                        selected: card.selected.clone(),
                        nested,
                        status: card.status.clone(),
                        project: project.clone(),
                        id: card.id.clone(),
                        title,
                        category: card.category.clone(),
                        slot,
                        role,
                        usage: usage.clone(),
                        version,
                    });
                    let matched: Vec<_> = rules
                        .iter()
                        .filter(|(r, re, field)| {
                            r.usages.contains(&usage)
                                && re.is_match(&card.id)
                                && field.as_ref().is_none_or(|re| {
                                    r.field
                                        .as_ref()
                                        .and_then(|pointer| {
                                            document
                                                .pointer(pointer)
                                                .and_then(Value::as_str)
                                                .map(|s| re.is_match(s))
                                        })
                                        .unwrap_or(false)
                                })
                        })
                        .collect();
                    let priority = matched.iter().map(|(r, _, _)| r.priority).max();
                    let mut destinations = BTreeSet::new();
                    for (r, _, _) in matched
                        .into_iter()
                        .filter(|(r, _, _)| Some(r.priority) == priority)
                    {
                        let key = index
                            .locators
                            .get(&(project.clone(), r.place_index))
                            .ok_or("Placement rule names an unenrolled byte location")?;
                        let id = cache_id(key);
                        destinations.insert(id.clone());
                        let label = p
                            .enrollment
                            .storage
                            .as_ref()
                            .and_then(|s| {
                                s.places
                                    .iter()
                                    .find(|place| place.place_index == r.place_index)
                            })
                            .map(|p| p.label.as_str())
                            .unwrap_or("Unnamed cache");
                        item.targets.insert(format!("{id}/{project}/{}/{usage}",r.id),json!({"cache_id":id,"project":project,"label":label,"rule":r.id,"reason":r.label,"usage":usage,"preference_only":true}));
                    }
                    if destinations.len() > 1 {
                        item.anomalies.insert("policy-conflict".into());
                    }
                    if objects.len() > 500_000 {
                        return Err("Object inventory budget exceeded".into());
                    }
                }
            }
        }
        for s in index.samples.values() {
            let item = objects.entry(s.sha.clone()).or_default();
            let known = !item.bindings.is_empty() || !item.copies.is_empty();
            if known && item.bytes != s.bytes {
                item.conflict = true;
            }
            if !known {
                item.bytes = s.bytes;
            }
            item.conflict |= s.conflict;
            item.copies.push(json!({"cache_id":cache_id(&s.cache),"domain":s.domain,"bytes":s.bytes,"allocated_bytes":s.allocated,"observed_at":s.time,"hash_verified":false}));
        }
        let mut rows = vec![];
        let mut kinds = BTreeSet::new();
        let mut anomalies = BTreeMap::<String, usize>::new();
        let mut total_bytes = 0u64;
        let mut stored_bytes = 0u64;
        for (sha, mut item) in objects {
            let in_scope = |b: &Binding| {
                members.is_none_or(|set| b.project == q.project && set.contains(&b.id))
            };
            if selected.is_some_and(|set| !set.contains(&sha)) {
                continue;
            }
            if members.is_some_and(|set| {
                !item
                    .bindings
                    .iter()
                    .any(|b| b.project == q.project && set.contains(&b.id))
            }) {
                continue;
            }
            let current_projects: BTreeSet<_> = item
                .bindings
                .iter()
                .filter(|b| b.usage == "current-revision")
                .map(|b| b.project.as_str())
                .collect();
            item.targets.retain(|_, p| {
                p["usage"] != "previous-revision"
                    || !current_projects.contains(p["project"].as_str().unwrap_or(""))
            });
            if !q.project.is_empty()
                && !item.bindings.iter().any(|b| b.project == q.project)
                && (!item.bindings.is_empty()
                    || !item.copies.iter().any(|copy| {
                        caches.iter().any(|c| {
                            c["id"] == copy["cache_id"]
                                && c["observations"]
                                    .as_array()
                                    .is_some_and(|p| p.iter().any(|v| v["project"] == q.project))
                        })
                    }))
            {
                continue;
            }
            for b in item.bindings.iter().filter(|b| in_scope(b)) {
                kinds.insert(b.category.clone());
            }
            if item.bindings.is_empty() {
                item.anomalies.insert("unattributed".into());
            }
            if item.copies.is_empty() {
                item.anomalies.insert("not-observed".into());
            }
            if item.targets.is_empty() {
                item.anomalies.insert("no-policy".into());
            }
            if item.conflict {
                item.anomalies.insert("inconsistent-size".into());
            }
            if item.targets.values().any(|target| {
                !item
                    .copies
                    .iter()
                    .any(|c| c["cache_id"] == target["cache_id"])
            }) {
                item.anomalies.insert("primary-not-observed".into());
            }
            if item.bindings.iter().any(|b| b.usage == "previous-revision") {
                item.anomalies.insert("previous-revision".into());
            }
            let query = q.q.to_lowercase();
            let has_current_revision = item.bindings.iter().any(|b| b.usage == "current-revision");
            item.bindings.sort_by_key(|b| {
                (
                    members.is_some_and(|set| b.project != q.project || !set.contains(&b.id)),
                    !query.is_empty()
                        && !format!("{} {} {}", b.title, b.id, b.slot)
                            .to_lowercase()
                            .contains(&query),
                    !q.project.is_empty() && b.project != q.project,
                    match b.usage.as_str() {
                        "current-revision" => 0,
                        "selected-subject" => 1,
                        "previous-revision" => 2,
                        _ => 3,
                    },
                    b.project.clone(),
                    b.id.clone(),
                    b.slot.clone(),
                )
            });
            let title = item
                .bindings
                .first()
                .map(|b| {
                    if b.title == b.id {
                        b.id.rsplit('/').next().unwrap_or(&b.id).to_owned()
                    } else {
                        b.title.clone()
                    }
                })
                .unwrap_or_else(|| "Unattributed observed object".into());
            if !q.observed.is_empty() && !item.copies.iter().any(|c| c["cache_id"] == q.observed) {
                continue;
            }
            let query = q.q.to_lowercase();
            if (!q.sha.is_empty() && sha != q.sha)
                || (!q.kind.is_empty()
                    && !item
                        .bindings
                        .iter()
                        .any(|b| in_scope(b) && b.category == q.kind))
                || (!q.usage.is_empty()
                    && !item
                        .bindings
                        .iter()
                        .any(|b| in_scope(b) && b.usage == q.usage))
                || (!q.primary.is_empty()
                    && !item.targets.values().any(|p| p["cache_id"] == q.primary))
                || (!query.is_empty()
                    && !sha.contains(&query)
                    && !item.bindings.iter().any(|b| {
                        in_scope(b)
                            && format!("{} {} {}", b.title, b.id, b.slot)
                                .to_lowercase()
                                .contains(&query)
                    }))
            {
                continue;
            }
            for a in &item.anomalies {
                *anomalies.entry(a.clone()).or_default() += 1;
            }
            if !q.anomaly.is_empty()
                && (if q.anomaly == "any" {
                    item.anomalies.is_empty()
                } else {
                    !item.anomalies.contains(q.anomaly)
                })
            {
                continue;
            }
            if q.anomaly == "primary-not-observed"
                && !q.primary.is_empty()
                && item.copies.iter().any(|c| c["cache_id"] == q.primary)
            {
                continue;
            }
            let matching = item
                .copies
                .iter()
                .filter(|c| q.observed.is_empty() || c["cache_id"] == q.observed)
                .try_fold(0u64, |sum, c| {
                    sum.checked_add(c["bytes"].as_u64().unwrap_or(0))
                        .ok_or("Copy accounting overflow")
                })?;
            let stored = item.copies.iter().try_fold(0u64, |sum, c| {
                sum.checked_add(c["bytes"].as_u64().unwrap_or(0))
                    .ok_or("Copy accounting overflow")
            })?;
            add(&mut total_bytes, item.bytes)?;
            add(&mut stored_bytes, matching)?;
            let basis = json!({"sha256":sha,"snapshots":index.snapshots,"policies":policy_pins,"copies":item.copies,"targets":item.targets});
            let basis = Object::from_bytes(
                &serde_json::to_vec(&basis).map_err(|_| "Decision encoding refused")?,
            );
            rows.push(json!({"sha256":sha,"title":title,"bytes":item.bytes,"stored_file_bytes":stored,"matching_copy_bytes":matching,"binding_count":item.bindings.len(),"has_current_revision":has_current_revision,"bindings":item.bindings.iter().take(if q.sha.is_empty(){6}else{100}).collect::<Vec<_>>(),"copies":item.copies,"primary":item.targets.values().collect::<Vec<_>>(),"anomalies":item.anomalies,"basis":basis.sha256,"complete_consumer_coverage":false,"removal_authorized":false}));
        }
        rows.sort_by(|a, b| match q.sort {
            "name" => a["title"]
                .as_str()
                .cmp(&b["title"].as_str())
                .then_with(|| a["sha256"].as_str().cmp(&b["sha256"].as_str())),
            field => b[if field == "stored" {
                "matching_copy_bytes"
            } else {
                "bytes"
            }]
            .as_u64()
            .cmp(
                &a[if field == "stored" {
                    "matching_copy_bytes"
                } else {
                    "bytes"
                }]
                .as_u64(),
            )
            .then_with(|| a["sha256"].as_str().cmp(&b["sha256"].as_str())),
        });
        let mut cache_totals = BTreeMap::<String, (u64, u64)>::new();
        for row in &rows {
            for copy in row["copies"].as_array().ok_or("Invalid copies")? {
                let id = copy["cache_id"].as_str().ok_or("Invalid cache identity")?;
                let total = cache_totals.entry(id.to_owned()).or_default();
                add(&mut total.0, 1)?;
                add(
                    &mut total.1,
                    copy["bytes"].as_u64().ok_or("Invalid copy size")?,
                )?;
            }
        }
        let copy_summary: Vec<_> = cache_totals
            .into_iter()
            .map(|(id, (count, bytes))| json!({"cache_id":id,"objects":count,"file_bytes":bytes}))
            .collect();
        Ok(
            json!({"scope":scope,"copy_summary":copy_summary,"snapshots":index.snapshots,"policy_pins":policy_pins,"total":rows.len(),"unique_content_bytes":total_bytes,"stored_file_bytes":stored_bytes,"items":rows.iter().skip(q.offset).take(100).collect::<Vec<_>>(),"kinds":kinds,"anomaly_counts":anomalies,"caches":caches,"complete_consumer_coverage":false,"removal_authorized":false,"limitations":"Named direct registered bindings, not complete dependency closures or current consumer/ref-count authority. Primary caches are project-owned preferences. Observed copies and timestamps are not verified retained custody; absent observations stay unknown."}),
        )
    }
    pub fn object_preview(
        &self,
        project: &str,
        sha: &str,
        basis: &str,
        action: &str,
    ) -> Result<Value> {
        if !matches!(action, "align" | "evict-review") {
            return Err("Unknown preview action".into());
        }
        let result = self.object_inventory(&Query {
            project,
            kind: "",
            usage: "",
            anomaly: "",
            primary: "",
            observed: "",
            q: "",
            sort: "bytes",
            offset: 0,
            sha,
        })?;
        let row = result["items"]
            .as_array()
            .and_then(|r| r.first())
            .ok_or("Unknown object")?;
        preview_row(&result, row, basis, action)
    }
}

fn preview_row(result: &Value, row: &Value, basis: &str, action: &str) -> Result<Value> {
    if row["basis"].as_str() != Some(basis) {
        return Err(
            "Object, policy, snapshot or observations changed; refresh before preview".into(),
        );
    }
    let mut holds = vec![
        "Complete current-consumer and active-job coverage is not established",
        "Observation metadata is not fresh hash-verified independent custody",
        "This catalog has no move or deletion executor",
    ];
    let copies = row["copies"].as_array().ok_or("Invalid copy view")?;
    let targets = row["primary"].as_array().ok_or("Invalid primary view")?;
    let current = row["has_current_revision"].as_bool().unwrap_or(false);
    let previous = row["anomalies"]
        .as_array()
        .is_some_and(|a| a.iter().any(|a| a == "previous-revision"));
    let conflict = row["anomalies"].as_array().is_some_and(|a| {
        a.iter()
            .any(|a| a == "policy-conflict" || a == "inconsistent-size")
    });
    let mut proposals = vec![];
    let mut destinations = BTreeSet::new();
    if conflict {
        holds.push("Resolve conflicting policy or size observations before proposing a change");
    }
    if !conflict && action == "align" {
        for target in targets {
            if !destinations.insert(target["cache_id"].as_str())
                || copies.iter().any(|c| c["cache_id"] == target["cache_id"])
            {
                continue;
            }
            proposals.push(json!({"kind":"copy-for-review","to":target["cache_id"],"from_candidates":copies.iter().filter(|c|c["bytes"]==row["bytes"]).map(|c|&c["cache_id"]).collect::<Vec<_>>(),"bytes":row["bytes"],"reason":target["reason"],"executable":false}));
        }
    }
    if !conflict && action == "evict-review" && previous && !current && !targets.is_empty() {
        for copy in copies
            .iter()
            .filter(|c| !targets.iter().any(|t| t["cache_id"] == c["cache_id"]))
        {
            proposals.push(json!({"kind":"older-copy-review","cache":copy["cache_id"],"retain_preferences":targets.iter().map(|t|&t["cache_id"]).collect::<Vec<_>>(),"file_bytes":copy["bytes"],"executable":false}));
        }
    }
    if action == "evict-review" && !previous {
        holds.push("No previous-revision classification is established for this object");
    }
    if row["primary"].as_array().is_none_or(Vec::is_empty) {
        holds.push("No primary-cache preference is configured for these bindings");
    }
    if current {
        holds.push("These exact bytes still have a current declared revision reference; older references do not make them obsolete");
    }
    Ok(
        json!({"object":row,"action":action,"proposals":proposals,"caches":result["caches"],"snapshots":result["snapshots"],"policy_pins":result["policy_pins"],"executable":false,"safe_reclaimable_bytes":null,"steps":if action=="align"{vec!["Confirm whether this version should be working, previous or cold","Verify exact source bytes under a protected read","Publish and verify an independently retained destination before considering removal"]}else{vec!["Resolve every current consumer, authorized draft and active job","Verify an independently retained copy of the exact bytes","Revalidate current policy, source generation and scoped protection immediately before removal"]},"holds":holds}),
    )
}
