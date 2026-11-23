//! Read-only adapter for retained cache observations. No eviction authority.
use super::*;
use regex::Regex;
pub mod global;
pub mod nested;
pub mod objects;
mod probe;
pub mod review;
#[cfg(test)]
mod tests;
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerRule {
    pub pattern: String,
    pub family: String,
    pub key: String,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    pub label: String,
    pub place_index: usize,
    pub report_tier: Option<usize>,
    #[serde(default)]
    pub live_probe: bool,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub inventory: Option<Object>,
    pub encoding: String,
    pub max_decoded_bytes: u64,
    #[serde(default = "default_probe_millis")]
    pub probe_millis: u64,
    pub places: Vec<Place>,
    #[serde(default)]
    pub owners: Vec<OwnerRule>,
    #[serde(default)]
    pub default_family: String,
    #[serde(default)]
    pub default_cache: String,
    #[serde(default)]
    pub placement_rules: Vec<objects::PlacementRule>,
    #[serde(default)]
    pub nested_content: Option<nested::Profile>,
}
fn default_probe_millis() -> u64 {
    30_000
}
pub fn validate(p: &Profile) -> Result<()> {
    if let Some(n) = &p.nested_content {
        nested::validate(n)?;
    }
    objects::validate_rules(&p.placement_rules, p.places.len())?;
    if p.placement_rules.iter().any(|r| {
        !p.places
            .iter()
            .any(|place| place.place_index == r.place_index)
    }) {
        return Err("Placement preference names an unenrolled cache".into());
    }
    if p.schema != "tig.catalogue-storage.experimental-1"
        || !matches!(p.encoding.as_str(), "json" | "gzip")
        || p.inventory
            .as_ref()
            .is_some_and(|o| o.bytes == 0 || o.bytes > 64 * 1024 * 1024)
        || p.max_decoded_bytes == 0
        || p.max_decoded_bytes > 512 * 1024 * 1024
        || p.places.is_empty()
        || p.places.len() > 16
        || p.owners.len() > 64
        || p.probe_millis == 0
        || p.probe_millis > 120_000
    {
        return Err("Invalid storage enrollment or budgets".into());
    }
    let mut labels = BTreeSet::new();
    let mut tiers = BTreeSet::new();
    for place in &p.places {
        if place.label.is_empty()
            || place.label.len() > 120
            || place.place_index > 15
            || !labels.insert(&place.label)
            || place.report_tier.is_some_and(|n| !tiers.insert(n))
        {
            return Err("Duplicate/invalid storage place".into());
        }
    }
    for r in &p.owners {
        if r.pattern.len() > 512
            || !r.pattern.starts_with('^')
            || r.family.len() > 120
            || r.key.len() > 256
        {
            return Err("Invalid owner projection".into());
        }
        regex::RegexBuilder::new(&r.pattern)
            .size_limit(1024 * 1024)
            .build()
            .map_err(|_| "Invalid owner expression")?;
    }
    Ok(())
}
#[derive(Deserialize)]
struct Report {
    schema: String,
    project_id: String,
    registry_generation: u64,
    #[serde(default)]
    census_started_unix_seconds: Option<u64>,
    #[serde(default)]
    census_completed_unix_seconds: Option<u64>,
    #[serde(default)]
    coverage_complete: bool,
    tiers: Vec<Tier>,
    objects: Vec<Row>,
}
#[derive(Deserialize)]
struct Tier {
    tier: usize,
    #[serde(default)]
    total_bytes: Option<u64>,
    #[serde(default)]
    free_bytes: Option<u64>,
}
#[derive(Deserialize, Serialize)]
struct Copy {
    tier: usize,
    bytes: u64,
    allocated_bytes: Option<u64>,
    physical_id: Option<String>,
    hard_links: Option<u64>,
    #[serde(default)]
    other_hard_links: Option<u64>,
    #[serde(default)]
    descriptor_size_matches: Option<bool>,
}
#[derive(Deserialize)]
struct Row {
    sha256: String,
    bytes: Option<u64>,
    category: String,
    copies: Vec<Copy>,
    #[serde(default)]
    direct_version_ids: Vec<String>,
    #[serde(default)]
    registered: bool,
    #[serde(default)]
    deprecated_reference: bool,
    #[serde(default)]
    noncurrent_reference: bool,
    #[serde(default)]
    required_by_current_version: bool,
    #[serde(default)]
    required_by_selected_version: bool,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    leased: bool,
}
struct Entry {
    row: Row,
    state: &'static str,
    owners: BTreeSet<(String, String)>,
}
pub struct Observation {
    report: Report,
    entries: Vec<Entry>,
    provenance: Value,
}
fn state(r: &Row) -> &'static str {
    if r.required_by_selected_version || r.category == "selected-output" {
        "selected"
    } else if r.pinned || r.leased {
        "protected"
    } else if r.category == "registry-control" {
        "metadata"
    } else if r.required_by_current_version || r.category == "current-working" {
        "current"
    } else if r.deprecated_reference {
        "deprecated"
    } else if r.noncurrent_reference || r.category == "retained-noncurrent" {
        "history"
    } else {
        "unknown"
    }
}
pub fn load(p: &Profile, profile: &ByteProfile) -> Result<Observation> {
    validate(p)?;
    let inventory = p
        .inventory
        .as_ref()
        .ok_or("No retained inventory enrolled; refresh registered objects")?;
    let mut reader = ReadCache::new(profile.clone()).map_err(|_| "Storage reader refused")?;
    let source = reader
        .open_verified(inventory)
        .map_err(|_| "Retained inventory unavailable or corrupt")?;
    let decoded: Box<dyn Read> = if p.encoding == "gzip" {
        Box::new(flate2::read::MultiGzDecoder::new(source))
    } else {
        Box::new(source)
    };
    let mut bounded = decoded.take(p.max_decoded_bytes + 1);
    let mut raw = Vec::new();
    bounded
        .read_to_end(&mut raw)
        .map_err(|_| "Inventory decoding failed")?;
    if raw.len() as u64 > p.max_decoded_bytes {
        return Err("Decoded inventory byte budget exceeded".into());
    }
    let mut report: Report =
        serde_json::from_slice(&raw).map_err(|_| "Invalid cache observation format")?;
    drop(raw);
    if !matches!(
        report.schema.as_str(),
        "reel.asset-cache-report.v1" | "tig.cache-observation.experimental-1"
    ) || report.objects.len() > 500_000
        || report.tiers.len() > 16
    {
        return Err("Unsupported inventory scope or row budget".into());
    }
    let rules: Vec<_> = p
        .owners
        .iter()
        .map(|r| Ok((r, Regex::new(&r.pattern).map_err(|_| "Invalid owner rule")?)))
        .collect::<Result<_>>()?;
    let mut ids = BTreeSet::new();
    let mut entries = Vec::with_capacity(report.objects.len());
    for row in std::mem::take(&mut report.objects) {
        if row.sha256.len() != 64
            || !row
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || !ids.insert(row.sha256.clone())
            || row.copies.len() > 16
            || row.direct_version_ids.len() > 65536
        {
            return Err("Duplicate/invalid object or reference budget".into());
        }
        let mut tiers = BTreeSet::new();
        for copy in &row.copies {
            if !tiers.insert(copy.tier) || !report.tiers.iter().any(|t| t.tier == copy.tier) {
                return Err("Invalid copy tier".into());
            }
        }
        let mut owners = BTreeSet::new();
        for version in &row.direct_version_ids {
            if version.len() > 1024 {
                return Err("Version identity budget exceeded".into());
            }
            for (rule, re) in &rules {
                if let Some(caps) = re.captures(version) {
                    let mut key = String::new();
                    caps.expand(&rule.key, &mut key);
                    if key.len() > 256 {
                        return Err("Owner key budget exceeded".into());
                    }
                    owners.insert((rule.family.clone(), key));
                    break;
                }
            }
        }
        let classification = state(&row);
        entries.push(Entry {
            row,
            state: classification,
            owners,
        });
    }
    Ok(Observation {
        report,
        entries,
        provenance: json!({"inventory":p.inventory,"kind":"retained-inventory"}),
    })
}
#[derive(Default)]
struct Sum {
    objects: usize,
    file_bytes: u64,
    allocated: u64,
    observed_allocated: u64,
    unknown_allocation: usize,
    review_objects: usize,
    review_bytes: u64,
    states: BTreeMap<String, [u64; 3]>,
    physical: BTreeSet<String>,
    review_seen: BTreeSet<String>,
    versions: BTreeSet<String>,
    owners: BTreeSet<String>,
    tiers: BTreeSet<usize>,
}
fn add(a: &mut u64, b: u64) -> Result<()> {
    *a = a.checked_add(b).ok_or("Accounting overflow")?;
    Ok(())
}
impl Sum {
    fn row(&mut self, e: &Entry, copies: &[&Copy], known_ref: bool) -> Result<()> {
        self.objects += 1;
        for v in e.row.direct_version_ids.iter().take(2) {
            if self.versions.len() < 2 {
                self.versions.insert(v.clone());
            }
        }
        for (_, o) in e.owners.iter().take(2) {
            if self.owners.len() < 2 {
                self.owners.insert(o.clone());
            }
        }
        let mut file_bytes = 0;
        let mut allocated = 0;
        let mut observed_allocated = 0;
        for c in copies {
            self.tiers.insert(c.tier);
            let new = c
                .physical_id
                .as_ref()
                .is_none_or(|id| self.physical.insert(id.clone()));
            if new {
                add(&mut file_bytes, c.bytes)?;
                if let Some(n) = c.allocated_bytes {
                    add(&mut observed_allocated, n)?;
                }
                if let (Some(n), Some(_)) = (c.allocated_bytes, c.physical_id.as_ref()) {
                    add(&mut allocated, n)?;
                } else {
                    self.unknown_allocation += 1;
                }
            }
        }
        add(&mut self.file_bytes, file_bytes)?;
        add(&mut self.allocated, allocated)?;
        add(&mut self.observed_allocated, observed_allocated)?;
        let state = if known_ref {
            "known-reference"
        } else {
            e.state
        };
        let values = self.states.entry(state.into()).or_default();
        add(&mut values[0], file_bytes)?;
        add(&mut values[1], allocated)?;
        add(&mut values[2], observed_allocated)?;
        let candidate = copies.len() == 1
            && !matches!(e.state, "selected" | "current" | "protected" | "metadata")
            && !known_ref
            && !e.row.pinned
            && !e.row.leased
            && !e.row.required_by_current_version
            && !e.row.required_by_selected_version
            && (e.row.deprecated_reference || e.row.noncurrent_reference)
            && e.row.category != "registry-control"
            && e.row.copies.iter().any(|other| {
                other.tier != copies[0].tier
                    && other.bytes == copies[0].bytes
                    && other.descriptor_size_matches != Some(false)
                    && other
                        .physical_id
                        .as_ref()
                        .zip(copies[0].physical_id.as_ref())
                        .is_some_and(|(a, b)| a != b)
            });
        if candidate {
            let eligible: Vec<_> = copies
                .iter()
                .filter(|c| {
                    c.hard_links == Some(1)
                        && c.other_hard_links.unwrap_or(0) == 0
                        && c.descriptor_size_matches != Some(false)
                        && c.allocated_bytes.is_some()
                        && c.physical_id.is_some()
                })
                .collect();
            if !eligible.is_empty() {
                self.review_objects += 1;
                for c in eligible {
                    if self
                        .review_seen
                        .insert(c.physical_id.as_ref().unwrap().clone())
                    {
                        add(&mut self.review_bytes, c.allocated_bytes.unwrap())?;
                    }
                }
            }
        }
        Ok(())
    }
    fn json(&self) -> Value {
        let states: BTreeMap<_, _> = self
            .states
            .iter()
            .map(|(k, v)| {
                (
                    k,
                    json!({"recorded_file_bytes":v[0],"measured_allocated_bytes":v[1],"observed_allocated_bytes":v[2]}),
                )
            })
            .collect();
        json!({"objects":self.objects,"recorded_file_bytes":self.file_bytes,"measured_allocated_bytes":self.allocated,"observed_allocated_bytes":self.observed_allocated,"copies_with_unknown_allocation":self.unknown_allocation,"review_objects":self.review_objects,"historical_review_allocation":self.review_bytes,"states":states,"versions":self.versions,"owners":self.owners,"tiers":self.tiers,"safe_reclaimable_bytes":null})
    }
}
pub struct Query<'a> {
    pub cache: &'a str,
    pub family: &'a str,
    pub owner: &'a str,
    pub version: &'a str,
    pub state: &'a str,
    pub pivot: &'a str,
    pub metric: &'a str,
    pub q: &'a str,
    pub offset: usize,
}
pub fn query(
    data: &Observation,
    profile: &Profile,
    q: &Query,
    known: &BTreeSet<String>,
) -> Result<Value> {
    if [q.cache, q.family, q.owner, q.version, q.state, q.pivot, q.q]
        .iter()
        .any(|s| s.len() > 1024)
        || q.offset > 1_000_000
        || !matches!(q.pivot, "objects" | "works" | "versions")
        || !matches!(
            q.metric,
            "recorded_file_bytes" | "measured_allocated_bytes" | "observed_allocated_bytes"
        )
    {
        return Err("Invalid storage filter or budget".into());
    }
    let tier = if q.cache.is_empty() {
        None
    } else {
        Some(
            profile
                .places
                .iter()
                .find(|p| p.label == q.cache)
                .and_then(|p| {
                    if data.report.schema == "tig.registered-observation.experimental-1" {
                        data.report
                            .tiers
                            .iter()
                            .any(|t| t.tier == p.place_index)
                            .then_some(p.place_index)
                    } else {
                        p.report_tier
                    }
                })
                .ok_or("Cache was not observed in this inventory")?,
        )
    };
    let mut total = Sum::default();
    let mut per_cache = BTreeMap::<usize, Sum>::new();
    let mut groups = BTreeMap::<String, Sum>::new();
    let mut families = BTreeSet::new();
    let mut owners = BTreeSet::new();
    let mut rows = Vec::new();
    let mut count = 0;
    for e in &data.entries {
        for (family, owner) in &e.owners {
            families.insert(family.clone());
            if q.family.is_empty() || q.family == family {
                owners.insert(owner.clone());
            }
        }
        let owner_matches = |(family, owner): &(String, String)| {
            (q.family.is_empty() || family == q.family) && (q.owner.is_empty() || owner == q.owner)
        };
        if ((!q.family.is_empty() || !q.owner.is_empty()) && !e.owners.iter().any(owner_matches))
            || (!q.version.is_empty() && !e.row.direct_version_ids.iter().any(|v| v == q.version))
        {
            continue;
        }
        let known_ref = known.contains(&e.row.sha256);
        let matches = match q.state {
            "" => true,
            "deprecated-refs" => e.row.deprecated_reference,
            "history-refs" => e.row.noncurrent_reference,
            "known-reference" => known_ref,
            s => e.state == s,
        };
        if !matches {
            continue;
        }
        if !q.q.is_empty()
            && !e.row.sha256.contains(q.q)
            && !e
                .row
                .direct_version_ids
                .iter()
                .any(|v| v.to_lowercase().contains(&q.q.to_lowercase()))
        {
            continue;
        }
        let copies: Vec<_> = e
            .row
            .copies
            .iter()
            .filter(|c| tier.is_none_or(|t| c.tier == t))
            .collect();
        if copies.is_empty() {
            continue;
        }
        total.row(e, &copies, known_ref)?;
        for copy in &copies {
            per_cache
                .entry(copy.tier)
                .or_default()
                .row(e, &[*copy], known_ref)?;
        }
        let keys: Vec<_> = match q.pivot {
            "objects" => vec![e.row.sha256.clone()],
            "works" => {
                if e.owners.is_empty() {
                    vec!["Unattributed".into()]
                } else {
                    e.owners
                        .iter()
                        .filter(|o| owner_matches(o))
                        .map(|(_, o)| o.clone())
                        .collect()
                }
            }
            _ => {
                if e.row.direct_version_ids.is_empty() {
                    vec!["No recorded version".into()]
                } else {
                    e.row.direct_version_ids.clone()
                }
            }
        };
        for key in keys.into_iter().collect::<BTreeSet<_>>() {
            let limit = if q.pivot == "objects" {
                500_000
            } else {
                100_000
            };
            if groups.len() >= limit && !groups.contains_key(&key) {
                return Err("Storage pivot group budget exceeded".into());
            }
            groups.entry(key).or_default().row(e, &copies, known_ref)?;
        }
        if count >= q.offset && rows.len() < 100 {
            rows.push(json!({"sha256":e.row.sha256,"bytes":e.row.bytes,"state":e.state,"registered":e.row.registered,"known_reference":known_ref,"deprecated_reference":e.row.deprecated_reference,"versions":e.row.direct_version_ids.iter().take(40).collect::<Vec<_>>(),"owners":e.owners,"copies":copies.iter().map(|c|json!({"tier":c.tier,"file_bytes":c.bytes,"allocated_bytes":c.allocated_bytes})).collect::<Vec<_>>(),"removal_authorized":false}));
        }
        count += 1;
    }
    let mut buckets: Vec<_> = groups.into_iter().collect();
    buckets.sort_by(|a, b| {
        let metric = |s: &Sum| {
            if q.metric == "observed_allocated_bytes" {
                s.observed_allocated
            } else if q.metric == "measured_allocated_bytes" {
                s.allocated
            } else {
                s.file_bytes
            }
        };
        metric(&b.1).cmp(&metric(&a.1)).then_with(|| a.0.cmp(&b.0))
    });
    let places:Vec<_>=profile.places.iter().map(|p|{let tier=if data.report.schema=="tig.registered-observation.experimental-1" {Some(p.place_index)} else {p.report_tier};let observation=tier.and_then(|t|data.report.tiers.iter().find(|r|r.tier==t));json!({"label":p.label,"observed":observation.is_some(),"tier":tier,"recorded_volume_total":observation.and_then(|o|o.total_bytes),"recorded_volume_free":observation.and_then(|o|o.free_bytes),"cache":tier.and_then(|t|per_cache.get(&t)).map(Sum::json)})}).collect();
    Ok(
        json!({"source":data.provenance,"source_project":data.report.project_id,"registry_generation":data.report.registry_generation,"started":data.report.census_started_unix_seconds,"observed_at":data.report.census_completed_unix_seconds,"source_coverage_complete":data.report.coverage_complete,"summary":total.json(),"places":places,"families":families,"owners":owners,"pivot":q.pivot,"groups":buckets.iter().take(50).map(|(key,s)|json!({"key":key,"summary":s.json()})).collect::<Vec<_>>(),"groups_total":buckets.len(),"objects":rows,"objects_total":count,"offset":q.offset,"group_totals_overlap":q.pivot!="objects","removal_authorized":false,"fresh_custody_verified":false,"limitations":"Historical metadata observations, not a current cache census or deletion authority. Version/work totals can overlap. Known references are exact selected subjects in the chosen Git snapshot, not complete dependency coverage."}),
    )
}
impl Project {
    fn adopt_storage(&mut self, observation: Observation) {
        let key = format!(
            "{}:{}",
            observation.provenance["kind"].as_str().unwrap_or("unknown"),
            observation.provenance["family"].as_str().unwrap_or("")
        );
        let observation = std::sync::Arc::new(observation);
        self.storage_sources.insert(key, observation.clone());
        self.storage = Some(observation);
    }
}
impl Catalogue {
    pub fn select_storage_scope(
        &mut self,
        project: &str,
        commit: &str,
        family: &str,
    ) -> Result<Value> {
        let p = self.projects.get_mut(project).ok_or("Unknown project")?;
        p.check_snapshot(commit)?;
        let key = format!("registered-object-metadata:{family}");
        if let Some(observation) = p
            .storage_sources
            .get(&key)
            .filter(|o| o.provenance["commit"].as_str() == Some(commit))
        {
            p.storage = Some(observation.clone());
            return Ok(json!({"cached_observation":true,"removal_authorized":false}));
        }
        self.observe_registered(project, commit, family)
    }
    pub fn use_retained_inventory(&mut self, project: &str, commit: &str) -> Result<Value> {
        let p = self.projects.get_mut(project).ok_or("Unknown project")?;
        p.check_snapshot(commit)?;
        let profile = p.enrollment.storage.as_ref().ok_or("No storage profile")?;
        let reader = p
            .profile
            .byte_source
            .as_ref()
            .ok_or("No metadata byte source")?;
        let observation = load(profile, reader)?;
        p.adopt_storage(observation);
        Ok(json!({"kind":"retained-inventory","removal_authorized":false}))
    }
    pub fn storage(&mut self, project: &str, commit: &str, q: &Query) -> Result<Value> {
        let p = self.projects.get_mut(project).ok_or("Unknown project")?;
        p.check_snapshot(commit)?;
        let profile = p
            .enrollment
            .storage
            .as_ref()
            .ok_or("No retained cache observation enrolled")?
            .clone();
        if p.storage.is_none() {
            let reader = p
                .profile
                .byte_source
                .as_ref()
                .ok_or("No metadata byte source")?;
            let observation = load(&profile, reader)?;
            p.adopt_storage(observation);
        }
        let known = p
            .snapshot
            .selected_records()
            .filter_map(|a| a.selected().ok().map(|o| o.sha256.as_str().to_owned()))
            .collect();
        query(p.storage.as_ref().unwrap(), &profile, q, &known)
    }
    pub fn space(&self, project: &str) -> Result<Value> {
        let p = self.projects.get(project).ok_or("Unknown project")?;
        let profile = p.enrollment.storage.as_ref().ok_or("No storage profile")?;
        let reader = p.profile.byte_source.as_ref().ok_or("No byte locations")?;
        Ok(
            json!({"places":profile.places.iter().map(|place|{let root=reader.places.get(place.place_index).map(|p|&p.root);let live=if place.live_probe{root.and_then(|r|fs2::total_space(r).ok().zip(fs2::available_space(r).ok()))}else{None};json!({"label":place.label,"total":live.map(|v|v.0),"free":live.map(|v|v.1),"measured":live.is_some()})}).collect::<Vec<_>>(),"measured_unix_seconds":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|"Clock refused")?.as_secs(),"scope":"Volume capacity only; no cache enumeration or object allocation measurement"}),
        )
    }
}
